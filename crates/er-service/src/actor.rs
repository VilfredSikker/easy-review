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

use er_engine::tab_identity::DiffMode;
use er_engine::TabKey;

use crate::content::{self, ContentPatch, TabContent};
use crate::effect::{resolve_target, LoadInput};
use crate::flows::{FlowRunner, FlowsUpdate};
use crate::github_sync::{Applied, Fetched};
use crate::guard::{self, ActorThread};
use crate::handle::{ActorMsg, Effected, ReplySender, WorkspaceError, WorkspaceHandle};
use crate::jobs::JobHost;
use crate::protocol::{Command, Event, Reply, TabContext};
use crate::sidecar_reload::SidecarReloads;
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
    let mut reloads = SidecarReloads::default();
    while let Ok(message) = rx.recv() {
        match message {
            ActorMsg::Command {
                command: Command::ReloadSidecars { key },
                reply,
            } => reloads.start(&deps, &state, &tx, key, reply),
            ActorMsg::Reloaded(done) => reloads.finish(&mut state, done, &subscribers, &view),
            ActorMsg::Command { command, reply } => {
                handle_command(&deps, &state, &mut jobs, &flows, &tx, command, reply);
            }
            ActorMsg::LoadFailed { key, message } => state.record_load_error(key, message),
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
                        patch,
                    }) => {
                        // A plain read changes nothing, so it publishes nothing.
                        if let Some(event) = event {
                            // A sync's sidecar reaches the tabs that read it
                            // in the same revision as its event.
                            let revision = match patch.map(|patch| state.apply_patch(patch)) {
                                Some(Ok((revision, _))) => revision,
                                Some(Err(_)) | None => state.bump_revision(),
                            };
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
    if !context_matches(state, &context) {
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
fn spawn_effect<R: Into<Effected>>(
    tx: &Sender<ActorMsg>,
    reply: ReplySender,
    work: impl FnOnce() -> Result<R, WorkspaceError> + Send + 'static,
) {
    let tx = tx.clone();
    // Unnamed on purpose: the storage guard lets only unnamed worker threads
    // use a process-wide test root, and production spawns unnamed threads too.
    let _ = thread::Builder::new().spawn(move || {
        let result = work().map(Into::into);
        let _ = tx.send(ActorMsg::EffectDone { result, reply });
    });
}

/// Answers a tab command whose key this actor has not loaded.
fn refuse_unknown_tab(reply: ReplySender) {
    let _ = reply.send(Err(WorkspaceError::Failed(
        "no loaded tab has that key".into(),
    )));
}

/// Loads `key`'s content off the actor. A refresh or a mode switch starts
/// from the content the key holds now, and is applied only over it.
fn spawn_load(
    deps: &WorkspaceDeps,
    state: &WorkspaceState,
    tx: &Sender<ActorMsg>,
    reply: ReplySender,
    key: TabKey,
    load: impl FnOnce(&WorkspaceDeps, Option<&TabContent>) -> Result<LoadInput, WorkspaceError>
        + Send
        + 'static,
) {
    let current = state.view().tabs.get(&key).cloned();
    let since = current.as_ref().map(|content| content.generation());
    let deps = deps.clone();
    let failed = tx.clone();
    spawn_effect(tx, reply, move || {
        // A first load that fails is recorded for its key, so a reader of
        // the key sees why instead of an empty tab.
        let loaded = load(&deps, current.as_deref())
            .and_then(|input| crate::effect::load_tab(&deps, input))
            .inspect_err(|error| {
                if since.is_none() {
                    let message = error.to_string();
                    let _ = failed.send(ActorMsg::LoadFailed { key, message });
                }
            })?;
        let reviewed_cleared = loaded.cleared;
        Ok(Effected::patched(
            Reply::ContentLoaded {
                key,
                reviewed_cleared,
            },
            ContentPatch::Loaded {
                key,
                since,
                loaded: Box::new(loaded),
            },
        ))
    });
}

/// The input that reloads loaded content as it is, in `mode`.
fn reload_input(content: &TabContent, mode: DiffMode) -> LoadInput {
    let mut tab = content.tab.clone();
    tab.mode = mode;
    LoadInput {
        tab,
        auto_unmark: true,
        recheck_branch: true,
        fetch_pr_refs: false,
        pr_refs_fetched: content.pr_refs_fetched,
        prev_ai: Some(Arc::clone(&content.ai)),
        prev_branch_diff_hash: content.diff.meta.branch_diff_hash.clone(),
        expanded: content.diff.expanded.clone(),
        prev_pr: Some(Arc::clone(&content.pr)),
        refresh_pr: false,
        adopt_head_oid: None,
        tips: content.freshness.value.tips.clone(),
    }
}

/// Whether an annotation command's tab belongs to the workspace the actor is
/// showing: both name the same workspace key.
///
/// A command for another workspace is a stale view: the user switched tabs
/// between the paint and the write (ADR 0017). An actor with no opened target
/// accepts any tab, since annotations do not require the workspace view.
fn context_matches(state: &WorkspaceState, context: &TabContext) -> bool {
    let view = state.view();
    let Some(target) = view.active.and_then(|index| view.targets.get(index)) else {
        return true;
    };
    target.workspace_key() == context.workspace_key()
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
            if !context_matches(state, &context) {
                return refuse_stale(reply);
            }
            let deps = deps.clone();
            spawn_effect(tx, reply, move || {
                crate::annotations::toggle_reviewed(&deps, &context, &path).map(
                    |(changed, patch)| Effected::patched(Reply::ReviewedChanged { changed }, patch),
                )
            });
        }
        Command::MarkReviewed {
            context,
            path,
            reviewed,
        } => {
            if !context_matches(state, &context) {
                return refuse_stale(reply);
            }
            let deps = deps.clone();
            spawn_effect(tx, reply, move || {
                crate::annotations::mark_reviewed(&deps, &context, &path, reviewed).map(
                    |(changed, patch)| Effected::patched(Reply::ReviewedChanged { changed }, patch),
                )
            });
        }
        Command::BulkReviewPillar {
            context,
            pillar_id,
            reviewed,
        } => {
            if !context_matches(state, &context) {
                return refuse_stale(reply);
            }
            let deps = deps.clone();
            spawn_effect(tx, reply, move || {
                crate::annotations::bulk_review_pillar(&deps, &context, &pillar_id, reviewed).map(
                    |(changed, patch)| Effected::patched(Reply::ReviewedChanged { changed }, patch),
                )
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
            if !context_matches(state, &context) {
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
                .map(|(id, patch)| Effected::patched(Reply::AnnotationWritten { id }, patch))
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
            if !context_matches(state, &context) {
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
                .map(|(id, patch)| Effected::patched(Reply::AnnotationWritten { id }, patch))
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
            if !context_matches(state, &context) {
                return refuse_stale(reply);
            }
            let deps = deps.clone();
            spawn_effect(tx, reply, move || {
                crate::annotations::add_document_thread(
                    &deps, &context, &file, line, line_end, kind, &body, &source,
                )
                .map(|(id, patch)| Effected::patched(Reply::AnnotationWritten { id }, patch))
            });
        }
        Command::ReplyToThread {
            context,
            parent_id,
            body,
            client_id,
        } => {
            if !context_matches(state, &context) {
                return refuse_stale(reply);
            }
            let deps = deps.clone();
            spawn_effect(tx, reply, move || {
                crate::annotations::reply_to_thread(&deps, &context, &parent_id, &body, client_id)
                    .map(|(id, patch)| Effected::patched(Reply::AnnotationWritten { id }, patch))
            });
        }
        Command::UpdateThreadMessage { context, id, body } => {
            if !context_matches(state, &context) {
                return refuse_stale(reply);
            }
            spawn_effect(tx, reply, move || {
                crate::annotations::update_thread_message(&context, &id, &body)
                    .map(|(path, patch)| Effected::patched(Reply::ThreadChanged { path }, patch))
            });
        }
        Command::DeleteThread { context, id } => {
            if !context_matches(state, &context) {
                return refuse_stale(reply);
            }
            spawn_effect(tx, reply, move || {
                crate::annotations::delete_thread(&context, &id)
                    .map(|(path, patch)| Effected::patched(Reply::ThreadChanged { path }, patch))
            });
        }
        Command::ResolveThread { context, id } => {
            if !context_matches(state, &context) {
                return refuse_stale(reply);
            }
            spawn_effect(tx, reply, move || {
                crate::annotations::resolve_thread(&context, &id)
                    .map(|(path, patch)| Effected::patched(Reply::ThreadChanged { path }, patch))
            });
        }
        Command::PromoteToNote { context, id, body } => {
            if !context_matches(state, &context) {
                return refuse_stale(reply);
            }
            let deps = deps.clone();
            spawn_effect(tx, reply, move || {
                crate::annotations::promote_to_note(&deps, &context, &id, body)
                    .map(|(id, patch)| Effected::patched(Reply::NotePromoted { id }, patch))
            });
        }
        Command::ReplyToFinding {
            context,
            finding_id,
            body,
            ai_assist,
            client_id,
        } => {
            if !context_matches(state, &context) {
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
                .map(|((root_id, reply_id), patch)| {
                    Effected::patched(Reply::FindingReplied { root_id, reply_id }, patch)
                })
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
            if !context_matches(state, &context) {
                return refuse_stale(reply);
            }
            let deps = deps.clone();
            spawn_effect(tx, reply, move || {
                crate::github_comments::add_comment(
                    &deps, &context, &target, &body, side, client_id,
                )
                .map(|(id, patch)| Effected::patched(Reply::AnnotationWritten { id }, patch))
            });
        }
        Command::ReplyToGithubThread {
            context,
            parent_id,
            body,
            client_id,
        } => {
            if !context_matches(state, &context) {
                return refuse_stale(reply);
            }
            let deps = deps.clone();
            spawn_effect(tx, reply, move || {
                crate::github_comments::reply_to_github_thread(
                    &deps, &context, &parent_id, &body, client_id,
                )
                .map(|(id, patch)| Effected::patched(Reply::AnnotationWritten { id }, patch))
            });
        }
        Command::PromoteToComment { context, id, body } => {
            if !context_matches(state, &context) {
                return refuse_stale(reply);
            }
            let deps = deps.clone();
            spawn_effect(tx, reply, move || {
                crate::github_comments::promote_to_comment(&deps, &context, &id, body)
                    .map(|(id, patch)| Effected::patched(Reply::CommentPromoted { id }, patch))
            });
        }
        Command::PromoteFindingToComment {
            context,
            finding_id,
            body,
        } => {
            if !context_matches(state, &context) {
                return refuse_stale(reply);
            }
            let deps = deps.clone();
            let readers = crate::sidecar_reload::readers(state, &context.er_dir);
            spawn_effect(tx, reply, move || {
                crate::github_comments::promote_finding_to_comment(
                    &deps,
                    &context,
                    &finding_id,
                    body,
                )
                .map(|(id, patch)| {
                    let patch = crate::sidecar_reload::with_reloaded_ai(&deps, &readers, patch);
                    Effected::patched(Reply::CommentPromoted { id }, patch)
                })
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
        Command::LoadTab { key, tab, mode } => {
            // A key the app registry no longer lists belongs to a closed tab.
            // Its drop reached no actor when this one was not up yet, so the
            // closed keys cannot tell; the registry's list can.
            if deps
                .app_registry()
                .is_some_and(|registry| !registry.tabs_view().tabs.iter().any(|t| t.key == key))
            {
                let _ = reply.send(Err(WorkspaceError::Failed(
                    "the tab closed before its load".into(),
                )));
                return;
            }
            spawn_load(deps, state, tx, reply, key, move |deps, _| {
                crate::effect::first_load(deps, &tab, mode)
            });
        }
        Command::RefreshDiff {
            key,
            force,
            adopt_head_oid,
        } => {
            let Some(mode) = state.view().tabs.get(&key).map(|c| c.tab.mode) else {
                return refuse_unknown_tab(reply);
            };
            // A view the workspace only records has nothing to refresh.
            if !content::loads(mode) {
                let reviewed_cleared = 0;
                let _ = reply.send(Ok(Reply::ContentLoaded {
                    key,
                    reviewed_cleared,
                }));
                return;
            }
            spawn_load(deps, state, tx, reply, key, move |_, content| {
                let content = content
                    .ok_or_else(|| WorkspaceError::Failed("no loaded tab has that key".into()))?;
                let mut input = reload_input(content, content.tab.mode);
                input.fetch_pr_refs = force;
                input.refresh_pr = force;
                input.adopt_head_oid = adopt_head_oid;
                Ok(input)
            });
        }
        Command::SetRemoteTips { key, tips } => {
            let Some(content) = state.view().tabs.get(&key).cloned() else {
                return refuse_unknown_tab(reply);
            };
            // Measured again even with the same tips: a local commit or fetch
            // moves the counts. A measure that changed nothing publishes
            // nothing.
            let deps = deps.clone();
            spawn_effect(tx, reply, move || {
                let freshness = crate::freshness::remeasure(&deps, &content, tips);
                if freshness == content.freshness.value {
                    return Ok(Effected::from(Reply::Acknowledged));
                }
                let patch = ContentPatch::Freshness {
                    key,
                    freshness: Box::new(freshness),
                };
                Ok(Effected::patched(Reply::Acknowledged, patch))
            });
        }
        Command::SetDiffMode {
            key,
            mode,
            pr_number,
        } => {
            let Some(since) = state.view().tabs.get(&key).map(|c| c.generation()) else {
                return refuse_unknown_tab(reply);
            };
            if !content::loads(mode) {
                let patch = ContentPatch::Mode { key, since, mode };
                return spawn_effect(tx, reply, move || {
                    Ok(Effected::patched(Reply::ModeRecorded { key }, patch))
                });
            }
            spawn_load(deps, state, tx, reply, key, move |_, content| {
                let content = content
                    .ok_or_else(|| WorkspaceError::Failed("no loaded tab has that key".into()))?;
                let mut input = reload_input(content, mode);
                input.tab.pr_number = input.tab.pr_number.or(pr_number);
                // A tour entered from the PR diff stays on the PR.
                if mode == DiffMode::Tour {
                    let from_pr = content.tab.mode == DiffMode::PrDiff;
                    input.tab.tour_is_pr = from_pr || content.tab.identity().is_remote();
                }
                // The views show different diffs, so a mark whose hash differs
                // in the new view still holds.
                input.auto_unmark = false;
                input.recheck_branch = false;
                // The first entry into a local PR's diff fetches its refs.
                input.fetch_pr_refs = mode == DiffMode::PrDiff
                    && content.tab.mode != DiffMode::PrDiff
                    && !content.pr_refs_fetched;
                Ok(input)
            });
        }
        Command::ChangeBase { key, base } => {
            let Some(current) = state
                .view()
                .tabs
                .get(&key)
                .map(|c| c.tab.base_branch.clone())
            else {
                return refuse_unknown_tab(reply);
            };
            if base.trim() == current {
                let reviewed_cleared = 0;
                let _ = reply.send(Ok(Reply::ContentLoaded {
                    key,
                    reviewed_cleared,
                }));
                return;
            }
            spawn_load(deps, state, tx, reply, key, move |deps, content| {
                let content = content
                    .ok_or_else(|| WorkspaceError::Failed("no loaded tab has that key".into()))?;
                let mut input = reload_input(content, content.tab.mode);
                input.tab.base_branch = crate::effect::resolve_base(deps, &content.tab, &base)?;
                // The diff's scope moved, so a mark whose hash differs still
                // holds; a local PR fetches its refs against the new base later.
                input.auto_unmark = false;
                input.pr_refs_fetched = false;
                Ok(input)
            });
        }
        Command::ParseFiles { key, paths, expand } => {
            let Some(content) = state.view().tabs.get(&key).cloned() else {
                return refuse_unknown_tab(reply);
            };
            let deps = deps.clone();
            spawn_effect(tx, reply, move || {
                let (files, expanded) =
                    crate::effect::parse_files(&deps, &content.tab, &content.diff, &paths, expand);
                let parsed = files.len();
                Ok(Effected::patched(
                    Reply::FilesParsed { key, parsed },
                    ContentPatch::Parsed {
                        key,
                        since: content.diff.revision,
                        files,
                        expanded,
                    },
                ))
            });
        }
        Command::CompactFiles { key, paths } => {
            let Some(content) = state.view().tabs.get(&key).cloned() else {
                return refuse_unknown_tab(reply);
            };
            spawn_effect(tx, reply, move || {
                let compacted = content
                    .diff
                    .files
                    .iter()
                    .filter(|file| paths.contains(&file.path))
                    .count();
                Ok(Effected::patched(
                    Reply::FilesCompacted { key, compacted },
                    ContentPatch::Compacted {
                        key,
                        since: content.diff.revision,
                        paths,
                    },
                ))
            });
        }
        // The loop hands reloads to their gate before dispatch.
        Command::ReloadSidecars { key } => {
            let reloaded = false;
            let _ = reply.send(Ok(Reply::SidecarsReloaded { key, reloaded }));
        }
        Command::DropTab { key } => {
            // A key not loaded yet may have a first load in flight; the patch
            // records the close, so that load is refused when it lands.
            spawn_effect(tx, reply, move || {
                Ok(Effected::patched(
                    Reply::ContentDropped { key },
                    ContentPatch::Dropped { key },
                ))
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
        #[cfg(any(test, feature = "test-support"))]
        Command::Stall => {
            // Hold the reply sender so the caller's own timeout fires. Leaked
            // on purpose; this command exists only in tests.
            std::mem::forget(reply);
        }
        #[cfg(any(test, feature = "test-support"))]
        Command::Hold => {
            // Hold the actor thread, so every later command queues behind
            // this one. Exists only in tests.
            std::mem::forget(reply);
            loop {
                thread::park();
            }
        }
    }
}

/// Applies an effect result on the actor thread, publishes the new view, and
/// tells subscribers and the caller.
fn apply(
    state: &mut WorkspaceState,
    result: Result<Effected, WorkspaceError>,
    reply: ReplySender,
    subscribers: &Mutex<Vec<Sender<Event>>>,
    view: &RwLock<Arc<WorkspaceView>>,
) {
    // A drop of a key that holds no content changes no view, so it moves no
    // revision and publishes nothing.
    let result = match result {
        Ok(Effected {
            reply: value,
            patch: Some(ContentPatch::Dropped { key }),
        }) if state.drop_unloaded(key) => {
            let _ = reply.send(Ok(value));
            return;
        }
        other => other,
    };
    // A content patch lands before the reply, so a caller that reads the view
    // after its reply sees the change. A refused patch answers with why.
    let result = result.and_then(|Effected { reply, patch }| match patch {
        None => Ok((reply, None)),
        Some(patch) => state
            .apply_patch(patch)
            .map(|(revision, _)| (reply, Some(revision)))
            .map_err(|refused| WorkspaceError::Failed(refused.to_string())),
    });
    match result {
        Ok((reply_value, patched)) => {
            let revision = match (&reply_value, patched) {
                (Reply::TargetOpened(target), _) => state.apply_opened(target.clone()),
                (_, Some(revision)) => revision,
                // An annotation write does not change the workspace view, but
                // readers still need a revision to refresh on.
                (_, None) => state.bump_revision(),
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

pub(crate) fn publish(view: &RwLock<Arc<WorkspaceView>>, state: &WorkspaceState) {
    let next = Arc::new(state.view().clone());
    match view.write() {
        Ok(mut current) => *current = next,
        // A poisoned lock still holds a complete view; refuse to blank the
        // workspace over a writer panic.
        Err(poisoned) => *poisoned.into_inner() = next,
    }
}

/// Sends one event to every subscriber, dropping those whose receiver is gone.
pub(crate) fn broadcast(subscribers: &Mutex<Vec<Sender<Event>>>, event: Event) {
    if let Ok(mut subscribers) = subscribers.lock() {
        subscribers.retain(|sender| sender.send(event.clone()).is_ok());
    }
}
