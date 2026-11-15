//! The actor loop.
//!
//! One thread owns the state and processes one message at a time. Anything
//! that can block (the ports) runs on a fresh effect thread; its typed result
//! comes back as another message, so the actor never waits on a subprocess.
//! The guard in [`crate::guard`] turns a port call on this thread into a
//! panic at the call site.

use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;

use er_engine::jobs::JobSink;
use er_engine::ports::Ports;

use crate::effect::resolve_target;
use crate::flows::{FlowRunner, FlowsUpdate};
use crate::github_sync::{Applied, Fetched};
use crate::guard::{self, ActorThread};
use crate::handle::{ActorMsg, ReplySender, WorkspaceError, WorkspaceHandle};
use crate::jobs::JobHost;
use crate::protocol::{Command, Event, Reply, TabContext};
use crate::workspace::{WorkspaceState, WorkspaceView};

/// The actor's dependencies. This is the seam P45 extends: the actor takes
/// this value rather than the ports directly, so a new dependency (the shared
/// app registry) is added here without touching callers of [`spawn`].
#[derive(Clone)]
pub struct WorkspaceDeps {
    ports: Ports,
    jobs: Option<Arc<dyn JobSink>>,
    post_session: Arc<Mutex<er_engine::github_post_session::PostSession>>,
    /// The process's app registry, shared by every workspace actor. Actors
    /// read or request through it; it alone writes the app records.
    app_registry: Option<Arc<crate::app_registry::AppRegistry>>,
}

impl WorkspaceDeps {
    pub fn new(ports: Ports) -> Self {
        Self {
            ports,
            jobs: None,
            post_session: Arc::new(Mutex::new(
                er_engine::github_post_session::PostSession::default(),
            )),
            app_registry: None,
        }
    }

    /// Shares the service's one app registry with the actors built from this.
    #[must_use]
    pub fn with_app_registry(mut self, registry: Arc<crate::app_registry::AppRegistry>) -> Self {
        self.app_registry = Some(registry);
        self
    }

    /// The app registry, when the service gave one.
    pub fn app_registry(&self) -> Option<&Arc<crate::app_registry::AppRegistry>> {
        self.app_registry.as_ref()
    }

    /// Sends the workspace's agent jobs to `jobs` instead of the actor's own
    /// sink. Tests record submissions with this.
    #[must_use]
    pub fn with_jobs(mut self, jobs: Arc<dyn JobSink>) -> Self {
        self.jobs = Some(jobs);
        self
    }

    pub(crate) fn jobs(&self) -> Option<Arc<dyn JobSink>> {
        self.jobs.clone()
    }

    /// The retry state a GitHub comment create keeps between attempts. Effects
    /// lock it only around begin and finish; the post itself runs unlocked.
    pub(crate) fn post_session(&self) -> &Arc<Mutex<er_engine::github_post_session::PostSession>> {
        &self.post_session
    }

    /// The ports, for use inside an off-actor effect. Every effect runs on its
    /// own unmarked thread, so this is the path a port call must take.
    ///
    /// # Panics
    ///
    /// Panics on a workspace actor thread: a handler that reaches a port here
    /// would block the actor on a subprocess and serialize the workspace.
    pub fn ports(&self) -> &Ports {
        guard::assert_off_actor();
        &self.ports
    }
}

/// Starts the actor for one workspace and returns its handle.
///
/// The actor thread runs until every sender is dropped. A command whose
/// effect thread cannot be started answers with a dropped reply channel.
pub fn spawn(deps: WorkspaceDeps) -> WorkspaceHandle {
    let (tx, rx) = std::sync::mpsc::channel();
    let subscribers = Arc::new(Mutex::new(Vec::new()));
    let view = Arc::new(RwLock::new(Arc::new(WorkspaceView::default())));
    let handle = WorkspaceHandle {
        tx: tx.clone(),
        subscribers: Arc::clone(&subscribers),
        view: Arc::clone(&view),
    };
    #[expect(
        clippy::expect_used,
        reason = "the actor thread is process setup: if it cannot start, no command can ever run"
    )]
    let _ = thread::Builder::new()
        .name("er-workspace-actor".into())
        .spawn(move || run(deps, tx, rx, subscribers, view))
        .expect("spawn the workspace actor thread");
    handle
}

fn run(
    deps: WorkspaceDeps,
    tx: Sender<ActorMsg>,
    rx: Receiver<ActorMsg>,
    subscribers: Arc<Mutex<Vec<Sender<Event>>>>,
    view: Arc<RwLock<Arc<WorkspaceView>>>,
) {
    let _actor = ActorThread::enter();
    let mut state = WorkspaceState::new();
    let mut jobs = JobHost::new(&deps, &tx);
    let flows = FlowRunner::new(&deps, jobs.sink(), jobs.arena().clone(), &tx);
    while let Ok(message) = rx.recv() {
        match message {
            ActorMsg::Command { command, reply } => {
                handle_command(&deps, &state, &mut jobs, &flows, &tx, command, reply);
            }
            ActorMsg::EffectDone { result, reply } => {
                // An arena effect may have started, cancelled or removed a run;
                // the revision it publishes carries the runs as they are now.
                state.set_arena_runs(jobs.live_arena_runs());
                apply(&mut state, result, reply, &subscribers, &view);
            }
            ActorMsg::JobProgress { job, line } => {
                broadcast(&subscribers, Event::JobProgress { job, line });
            }
            ActorMsg::JobFinished { job, outcome } => {
                jobs.finished(job);
                broadcast(&subscribers, Event::JobFinished { job, outcome });
            }
            ActorMsg::ArenaChanged => {
                state.set_arena_runs(jobs.live_arena_runs());
                let revision = state.bump_revision();
                publish(&view, &state);
                broadcast(&subscribers, Event::ViewPublished { revision });
                // A flow waits on its arena run; the run's progress and end
                // are what move it on.
                if !state.view().review_flows.is_empty() {
                    flows.request_advance();
                }
            }
            ActorMsg::FlowTick => flows.request_advance(),
            ActorMsg::FlowsChanged(update) => {
                apply_flows(&mut state, &jobs, update, &subscribers, &view);
            }
            ActorMsg::Fetched { result, reply } => {
                match result.and_then(crate::github_sync::apply) {
                    Ok(Applied {
                        reply: value,
                        event,
                    }) => {
                        // A plain read changes nothing, so it publishes nothing.
                        if let Some(event) = event {
                            let revision = state.bump_revision();
                            publish(&view, &state);
                            broadcast(&subscribers, Event::ViewPublished { revision });
                            broadcast(&subscribers, event);
                        }
                        let _ = reply.send(Ok(value));
                    }
                    Err(error) => apply(&mut state, Err(error), reply, &subscribers, &view),
                }
            }
        }
    }
}

/// Publishes what a flow call changed: the flow section with a new revision
/// and `WorkflowChanged` per changed bucket, then `PanelRunChanged` per new
/// panel record.
fn apply_flows(
    state: &mut WorkspaceState,
    jobs: &JobHost,
    update: FlowsUpdate,
    subscribers: &Mutex<Vec<Sender<Event>>>,
    view: &RwLock<Arc<WorkspaceView>>,
) {
    let changed = state.set_review_flows(update.flows);
    if !changed.is_empty() {
        // A flow's arena run may have started or ended in the same call.
        state.set_arena_runs(jobs.live_arena_runs());
        let revision = state.bump_revision();
        publish(view, state);
        broadcast(subscribers, Event::ViewPublished { revision });
        for er_dir in changed {
            broadcast(subscribers, Event::WorkflowChanged { er_dir, revision });
        }
    }
    for (er_dir, run_id) in update.panel_runs {
        broadcast(subscribers, Event::PanelRunChanged { er_dir, run_id });
    }
}

/// Runs one GitHub read for the tab off the actor thread and sends what it
/// read back for the actor to apply. A tab the workspace has left is refused.
fn spawn_fetch(
    deps: &WorkspaceDeps,
    state: &WorkspaceState,
    tx: &Sender<ActorMsg>,
    reply: ReplySender,
    context: TabContext,
    work: impl FnOnce(&WorkspaceDeps, TabContext) -> Result<Fetched, WorkspaceError> + Send + 'static,
) {
    if !context_matches(state, &context.repo_root) {
        return refuse_stale(reply);
    }
    let (deps, tx) = (deps.clone(), tx.clone());
    let _ = thread::Builder::new().spawn(move || {
        let result = work(&deps, context);
        let _ = tx.send(ActorMsg::Fetched { result, reply });
    });
}

/// Runs one command's blocking work off the actor thread and sends the typed
/// result back.
fn spawn_effect(
    tx: &Sender<ActorMsg>,
    reply: ReplySender,
    work: impl FnOnce() -> Result<Reply, WorkspaceError> + Send + 'static,
) {
    let tx = tx.clone();
    // Unnamed on purpose: the storage guard lets only unnamed worker threads
    // use a process-wide test root, and production spawns unnamed threads too.
    let _ = thread::Builder::new().spawn(move || {
        let result = work();
        let _ = tx.send(ActorMsg::EffectDone { result, reply });
    });
}

/// Whether an annotation command names the repo the actor is showing.
///
/// A command for another repo is a stale view: the user switched tabs between
/// the paint and the write (ADR 0017). An actor with no opened target accepts
/// any repo, since annotations do not require the workspace view.
fn context_matches(state: &WorkspaceState, repo_root: &str) -> bool {
    let view = state.view();
    let Some(target) = view.active.and_then(|index| view.targets.get(index)) else {
        return true;
    };
    repo_root.is_empty() || target.repo_root == repo_root
}

/// Answers a command whose view has moved on, without running its effect.
fn refuse_stale(reply: ReplySender) {
    let _ = reply.send(Err(WorkspaceError::Failed(
        "the workspace moved to another target before the write".into(),
    )));
}

/// Dispatches one command on the actor thread. This function must not touch a
/// port: only the effects it spawns may, and they run on new threads.
fn handle_command(
    deps: &WorkspaceDeps,
    state: &WorkspaceState,
    jobs: &mut JobHost,
    flows: &FlowRunner,
    tx: &Sender<ActorMsg>,
    command: Command,
    reply: ReplySender,
) {
    match command {
        Command::OpenTarget { target, replace: _ } => {
            let deps = deps.clone();
            spawn_effect(tx, reply, move || {
                resolve_target(&deps, &target).map(Reply::TargetOpened)
            });
        }
        Command::ToggleReviewed { context, path } => {
            if !context_matches(state, &context.repo_root) {
                return refuse_stale(reply);
            }
            let deps = deps.clone();
            spawn_effect(tx, reply, move || {
                crate::annotations::toggle_reviewed(&deps, &context, &path)
                    .map(|changed| Reply::ReviewedChanged { changed })
            });
        }
        Command::MarkReviewed {
            context,
            path,
            reviewed,
        } => {
            if !context_matches(state, &context.repo_root) {
                return refuse_stale(reply);
            }
            let deps = deps.clone();
            spawn_effect(tx, reply, move || {
                crate::annotations::mark_reviewed(&deps, &context, &path, reviewed)
                    .map(|changed| Reply::ReviewedChanged { changed })
            });
        }
        Command::BulkReviewPillar {
            context,
            pillar_id,
            reviewed,
        } => {
            if !context_matches(state, &context.repo_root) {
                return refuse_stale(reply);
            }
            let deps = deps.clone();
            spawn_effect(tx, reply, move || {
                crate::annotations::bulk_review_pillar(&deps, &context, &pillar_id, reviewed)
                    .map(|changed| Reply::ReviewedChanged { changed })
            });
        }
        Command::AddQuestion {
            context,
            target,
            body,
            side,
            client_id,
            reply_to,
            finding_ref,
        } => {
            if !context_matches(state, &context.repo_root) {
                return refuse_stale(reply);
            }
            let deps = deps.clone();
            spawn_effect(tx, reply, move || {
                crate::annotations::add_question(
                    &deps,
                    &context,
                    &target,
                    &body,
                    side.as_deref(),
                    client_id,
                    reply_to,
                    finding_ref,
                )
                .map(|id| Reply::AnnotationWritten { id })
            });
        }
        Command::AddNote {
            context,
            target,
            body,
            side,
            client_id,
            reply_to,
            finding_ref,
        } => {
            if !context_matches(state, &context.repo_root) {
                return refuse_stale(reply);
            }
            let deps = deps.clone();
            spawn_effect(tx, reply, move || {
                crate::annotations::add_note(
                    &deps,
                    &context,
                    &target,
                    &body,
                    side.as_deref(),
                    client_id,
                    reply_to,
                    finding_ref,
                )
                .map(|id| Reply::AnnotationWritten { id })
            });
        }
        Command::AddDocumentThread {
            context,
            file,
            line,
            line_end,
            kind,
            body,
            source,
        } => {
            if !context_matches(state, &context.repo_root) {
                return refuse_stale(reply);
            }
            let deps = deps.clone();
            spawn_effect(tx, reply, move || {
                crate::annotations::add_document_thread(
                    &deps, &context, &file, line, line_end, kind, &body, &source,
                )
                .map(|id| Reply::AnnotationWritten { id })
            });
        }
        Command::ReplyToThread {
            context,
            parent_id,
            body,
            client_id,
        } => {
            if !context_matches(state, &context.repo_root) {
                return refuse_stale(reply);
            }
            let deps = deps.clone();
            spawn_effect(tx, reply, move || {
                crate::annotations::reply_to_thread(&deps, &context, &parent_id, &body, client_id)
                    .map(|id| Reply::AnnotationWritten { id })
            });
        }
        Command::UpdateThreadMessage { context, id, body } => {
            if !context_matches(state, &context.repo_root) {
                return refuse_stale(reply);
            }
            spawn_effect(tx, reply, move || {
                crate::annotations::update_thread_message(&context, &id, &body)
                    .map(|path| Reply::ThreadChanged { path })
            });
        }
        Command::DeleteThread { context, id } => {
            if !context_matches(state, &context.repo_root) {
                return refuse_stale(reply);
            }
            spawn_effect(tx, reply, move || {
                crate::annotations::delete_thread(&context, &id)
                    .map(|path| Reply::ThreadChanged { path })
            });
        }
        Command::ResolveThread { context, id } => {
            if !context_matches(state, &context.repo_root) {
                return refuse_stale(reply);
            }
            spawn_effect(tx, reply, move || {
                crate::annotations::resolve_thread(&context, &id)
                    .map(|path| Reply::ThreadChanged { path })
            });
        }
        Command::PromoteToNote { context, id, body } => {
            if !context_matches(state, &context.repo_root) {
                return refuse_stale(reply);
            }
            let deps = deps.clone();
            spawn_effect(tx, reply, move || {
                crate::annotations::promote_to_note(&deps, &context, &id, body)
                    .map(|id| Reply::NotePromoted { id })
            });
        }
        Command::ReplyToFinding {
            context,
            finding_id,
            body,
            ai_assist,
            client_id,
        } => {
            if !context_matches(state, &context.repo_root) {
                return refuse_stale(reply);
            }
            let deps = deps.clone();
            spawn_effect(tx, reply, move || {
                crate::annotations::reply_to_finding(
                    &deps,
                    &context,
                    &finding_id,
                    &body,
                    ai_assist,
                    client_id,
                )
                .map(|(root_id, reply_id)| Reply::FindingReplied { root_id, reply_id })
            });
        }
        Command::StartJob {
            job,
            submission,
            result,
        } => jobs.start(tx, job, submission, result, reply),
        Command::CancelJob { job } => match jobs.cancel(job) {
            Some(kill) => spawn_effect(tx, reply, move || {
                kill();
                Ok(Reply::JobCancelled { job })
            }),
            None => crate::jobs::refuse_unknown(job, reply),
        },
        Command::Reconcile { buckets } => {
            let flows = flows.clone();
            spawn_effect(tx, reply, move || Ok(flows.reconcile(&buckets)));
        }
        Command::Flow(command) => {
            let flows = flows.clone();
            spawn_effect(tx, reply, move || flows.run(command));
        }
        Command::Arena(command) => {
            let deps = deps.clone();
            let arena = jobs.arena().clone();
            spawn_effect(tx, reply, move || crate::arena::run(&deps, &arena, command));
        }
        Command::AddComment {
            context,
            target,
            body,
            side,
            client_id,
        } => {
            if !context_matches(state, &context.repo_root) {
                return refuse_stale(reply);
            }
            let deps = deps.clone();
            spawn_effect(tx, reply, move || {
                crate::github_comments::add_comment(
                    &deps, &context, &target, &body, side, client_id,
                )
                .map(|id| Reply::AnnotationWritten { id })
            });
        }
        Command::ReplyToGithubThread {
            context,
            parent_id,
            body,
            client_id,
        } => {
            if !context_matches(state, &context.repo_root) {
                return refuse_stale(reply);
            }
            let deps = deps.clone();
            spawn_effect(tx, reply, move || {
                crate::github_comments::reply_to_github_thread(
                    &deps, &context, &parent_id, &body, client_id,
                )
                .map(|id| Reply::AnnotationWritten { id })
            });
        }
        Command::PromoteToComment { context, id, body } => {
            if !context_matches(state, &context.repo_root) {
                return refuse_stale(reply);
            }
            let deps = deps.clone();
            spawn_effect(tx, reply, move || {
                crate::github_comments::promote_to_comment(&deps, &context, &id, body)
                    .map(|id| Reply::CommentPromoted { id })
            });
        }
        Command::PromoteFindingToComment {
            context,
            finding_id,
            body,
        } => {
            if !context_matches(state, &context.repo_root) {
                return refuse_stale(reply);
            }
            let deps = deps.clone();
            spawn_effect(tx, reply, move || {
                crate::github_comments::promote_finding_to_comment(
                    &deps,
                    &context,
                    &finding_id,
                    body,
                )
                .map(|id| Reply::CommentPromoted { id })
            });
        }
        Command::SyncGithubComments { context, source } => {
            spawn_fetch(deps, state, tx, reply, context, move |deps, context| {
                crate::github_sync::fetch_sync(deps, context, source)
            });
        }
        Command::GetPendingReview { context } => {
            spawn_fetch(deps, state, tx, reply, context, crate::pending_review::get);
        }
        Command::AddPendingReviewComment {
            context,
            target,
            body,
        } => {
            spawn_fetch(deps, state, tx, reply, context, move |deps, context| {
                crate::pending_review::add_comment(deps, context, &target, &body)
            });
        }
        Command::RemovePendingReviewComment {
            context,
            comment_node_id,
        } => {
            spawn_fetch(deps, state, tx, reply, context, move |deps, context| {
                crate::pending_review::remove_comment(deps, context, &comment_node_id)
            });
        }
        Command::SubmitPendingReview {
            context,
            event,
            summary,
        } => {
            spawn_fetch(deps, state, tx, reply, context, move |deps, context| {
                crate::pending_review::submit(deps, context, event, &summary)
            });
        }
        Command::PostGithubPrComment { context, body } => {
            spawn_fetch(deps, state, tx, reply, context, move |deps, context| {
                crate::pr_posts::post_pr_comment(deps, context, &body)
            });
        }
        Command::PostGithubLineComment {
            context,
            target,
            body,
        } => {
            spawn_fetch(deps, state, tx, reply, context, move |deps, context| {
                crate::pr_posts::post_line_comment(deps, context, &target, &body)
            });
        }
        Command::SubmitGithubPrDecision {
            context,
            event,
            summary,
        } => {
            spawn_fetch(deps, state, tx, reply, context, move |deps, context| {
                crate::pr_posts::submit_decision(deps, context, event, &summary)
            });
        }
        #[cfg(test)]
        Command::Ping => {
            let _ = reply.send(Ok(Reply::Pong));
        }
        #[cfg(test)]
        Command::PortProbe => {
            // Deliberate: the guard is the thing under test.
            let _ = deps.ports();
        }
        #[cfg(test)]
        Command::Stall => {
            // Hold the reply sender so the caller's own timeout fires. Leaked
            // on purpose; this command exists only in tests.
            std::mem::forget(reply);
        }
    }
}

/// Applies an effect result on the actor thread, publishes the new view, and
/// tells subscribers and the caller.
fn apply(
    state: &mut WorkspaceState,
    result: Result<Reply, WorkspaceError>,
    reply: ReplySender,
    subscribers: &Mutex<Vec<Sender<Event>>>,
    view: &RwLock<Arc<WorkspaceView>>,
) {
    match result {
        Ok(reply_value) => {
            let revision = match &reply_value {
                Reply::TargetOpened(target) => state.apply_opened(target.clone()),
                // An annotation write does not change the workspace view, but
                // readers still need a revision to refresh on.
                _ => state.bump_revision(),
            };
            publish(view, state);
            broadcast(subscribers, Event::ViewPublished { revision });
            if let Reply::TargetOpened(target) = &reply_value {
                broadcast(
                    subscribers,
                    Event::TargetOpened {
                        target: target.clone(),
                    },
                );
            }
            let _ = reply.send(Ok(reply_value));
        }
        Err(error) => {
            let message = error.to_string();
            let revision = state.apply_failure(message.clone());
            publish(view, state);
            broadcast(subscribers, Event::ViewPublished { revision });
            broadcast(subscribers, Event::CommandFailed { message });
            let _ = reply.send(Err(error));
        }
    }
}

fn publish(view: &RwLock<Arc<WorkspaceView>>, state: &WorkspaceState) {
    let next = Arc::new(state.view().clone());
    match view.write() {
        Ok(mut current) => *current = next,
        // A poisoned lock still holds a complete view; refuse to blank the
        // workspace over a writer panic.
        Err(poisoned) => *poisoned.into_inner() = next,
    }
}

/// Sends one event to every subscriber, dropping those whose receiver is gone.
fn broadcast(subscribers: &Mutex<Vec<Sender<Event>>>, event: Event) {
    if let Ok(mut subscribers) = subscribers.lock() {
        subscribers.retain(|sender| sender.send(event.clone()).is_ok());
    }
}
