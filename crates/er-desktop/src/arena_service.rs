//! The arena commands on the flag path. Each reads the tab and the App's
//! settings here, where they live, and sends the command to the workspace
//! actor, which runs the arena and its jobs. Reads never wait on the actor
//! (ADR 0067): progress and live status come from its published view, and run
//! records from the store.
//!
//! Only compiled with `v1-service`.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use er_engine::app::App;
use er_engine::arena::{
    ArenaBatchStartParams, ArenaDiffPreview, ArenaProgressState, ArenaStartParams,
};
use er_service::{ArenaCommand, ArenaContext, Command, Reply, WorkspaceView};

use crate::arena_commands::{
    parse_scope, parse_verdict, wire_groups, wire_reviewer, wire_reviewers, ArenaAcceptRequest,
    ArenaBatchEstimateRequest, ArenaBatchStartRequest, ArenaEstimateRequest, ArenaOverrideRequest,
    ArenaStartRequest,
};
use crate::commands::{tab_context, AppState};
use crate::snapshot::{ArenaRunSnapshotWire, ArenaRunSummaryWire};

fn arena_context(app: &App) -> ArenaContext {
    ArenaContext {
        tab: tab_context(app),
        branch_ref: app.arena_branch_ref(),
        config: Arc::new(app.config.clone()),
        buckets: app.tabs.iter().map(|tab| tab.er_dir()).collect(),
        review_hash: app
            .tab()
            .ai
            .review
            .as_ref()
            .map(|review| review.diff_hash.clone())
            .unwrap_or_default(),
    }
}

/// The run's effort, resolved against the App's current pick the way the
/// flag-off start does.
fn resolve_effort(app: &App, requested: Option<&str>) -> Option<String> {
    er_engine::config::resolve_effort(
        &app.config.ai_hub,
        &app.config.agent,
        app.current_ai_effort.as_deref(),
        requested,
    )
}

/// The actor key and the arena context, read under the App lock. `prepare`
/// sees the App too, for the inputs only it holds.
fn resolve<T>(
    state: &AppState,
    prepare: impl FnOnce(&App, ArenaContext) -> T,
) -> Result<(String, T), String> {
    let app = state.app.lock().map_err(|error| error.to_string())?;
    let arena = arena_context(&app);
    let key = arena.tab.workspace_key();
    Ok((key, prepare(&app, arena)))
}

/// Sends one arena command and waits for the actor's bounded reply.
fn ask(
    state: &AppState,
    build: impl FnOnce(&App, ArenaContext) -> ArenaCommand,
) -> Result<Reply, String> {
    let (key, command) = resolve(state, build)?;
    state
        .service
        .send(&key, Command::Arena(command))
        .map_err(|error| error.to_string())
}

/// Sends one arena command that changes a run and bumps the desktop revision,
/// as the flag-off commands do, so the front end re-reads.
fn send(
    state: &AppState,
    build: impl FnOnce(&App, ArenaContext) -> ArenaCommand,
) -> Result<Reply, String> {
    let reply = ask(state, build)?;
    state.desktop_revision.fetch_add(1, Ordering::Relaxed);
    Ok(reply)
}

/// The actor's published view and the arena context, for a read. The App
/// lock is released before the read touches the store.
fn read_context(state: &AppState) -> Result<(Arc<WorkspaceView>, ArenaContext), String> {
    let (key, arena) = resolve(state, |_, arena| arena)?;
    Ok((state.service.view(&key), arena))
}

fn unexpected(answer: impl std::fmt::Debug) -> String {
    format!("unexpected answer from the workspace: {answer:?}")
}

fn estimated(reply: Reply) -> Result<ArenaDiffPreview, String> {
    match reply {
        Reply::ArenaEstimate(preview) => Ok(preview),
        other => Err(unexpected(other)),
    }
}

pub fn estimate(state: &AppState, req: ArenaEstimateRequest) -> Result<ArenaDiffPreview, String> {
    estimated(ask(state, |_, arena| ArenaCommand::Estimate {
        arena,
        scope: parse_scope(&req.scope),
        files: req.files,
        reviewers: wire_reviewers(&req.reviewers),
        rounds: req.rounds,
        arbiter: req.arbiter.as_ref().map(wire_reviewer),
    })?)
}

pub fn estimate_batch(
    state: &AppState,
    req: ArenaBatchEstimateRequest,
) -> Result<ArenaDiffPreview, String> {
    let params = ArenaBatchStartParams {
        scope: parse_scope(&req.scope),
        files: req.files,
        rounds: req.rounds,
        arbiter: req.arbiter.as_ref().map(wire_reviewer),
        confirm: false,
        groups: wire_groups(&req.groups),
        effort: None,
    };
    estimated(ask(state, |_, arena| ArenaCommand::EstimateBatch {
        arena,
        params,
    })?)
}

pub fn start(state: &AppState, req: ArenaStartRequest) -> Result<String, String> {
    let reply = send(state, |app, arena| ArenaCommand::StartRun {
        arena,
        params: ArenaStartParams {
            title: req.title,
            reviewers: wire_reviewers(&req.reviewers),
            scope: parse_scope(&req.scope),
            files: req.files,
            rounds: req.rounds,
            arbiter: req.arbiter.as_ref().map(wire_reviewer),
            confirm: req.confirm.unwrap_or(false),
            agent_kind: req.agent_kind,
            effort: resolve_effort(app, req.effort.as_deref()),
        },
    })?;
    match reply {
        Reply::ArenaStarted { mut run_ids } if run_ids.len() == 1 => Ok(run_ids.remove(0)),
        other => Err(unexpected(other)),
    }
}

pub fn start_batch(state: &AppState, req: ArenaBatchStartRequest) -> Result<Vec<String>, String> {
    let reply = send(state, |app, arena| ArenaCommand::StartBatch {
        arena,
        params: ArenaBatchStartParams {
            scope: parse_scope(&req.scope),
            files: req.files,
            rounds: req.rounds,
            arbiter: req.arbiter.as_ref().map(wire_reviewer),
            confirm: req.confirm.unwrap_or(false),
            groups: wire_groups(&req.groups),
            effort: resolve_effort(app, req.effort.as_deref()),
        },
    })?;
    match reply {
        Reply::ArenaStarted { run_ids } => Ok(run_ids),
        other => Err(unexpected(other)),
    }
}

pub fn validate_findings(state: &AppState) -> Result<Option<String>, String> {
    match send(state, |_, arena| ArenaCommand::ValidateFindings { arena })? {
        Reply::ArenaValidating { run_id } => Ok(run_id),
        other => Err(unexpected(other)),
    }
}

pub fn accept_findings(state: &AppState, req: ArenaAcceptRequest) -> Result<usize, String> {
    let reply = send(state, |_, arena| ArenaCommand::AcceptFindings {
        arena,
        run_id: req.run_id,
        finding_ids: req.finding_ids,
    })?;
    match reply {
        Reply::ArenaAccepted { accepted } => Ok(accepted),
        other => Err(unexpected(other)),
    }
}

pub fn progress(state: &AppState, run_id: String) -> Result<ArenaProgressState, String> {
    let (view, _) = read_context(state)?;
    Ok(er_service::arena::read_progress(&view, &run_id))
}

pub fn get(state: &AppState, run_id: String) -> Result<ArenaRunSnapshotWire, String> {
    let (view, arena) = read_context(state)?;
    er_service::arena::read_run(&view, &arena, &run_id).map_err(|error| error.to_string())
}

pub fn list(state: &AppState) -> Result<Vec<ArenaRunSummaryWire>, String> {
    let (_, arena) = read_context(state)?;
    er_service::arena::read_runs(&arena).map_err(|error| error.to_string())
}

pub fn delete(state: &AppState, run_id: String) -> Result<(), String> {
    send(state, |_, arena| ArenaCommand::Delete { arena, run_id }).map(|_| ())
}

pub fn cancel(state: &AppState, run_id: String) -> Result<(), String> {
    send(state, |_, _| ArenaCommand::Cancel { run_id }).map(|_| ())
}

/// Records the verdict, then reads the run back for the overlay, as the
/// flag-off command answers with the updated snapshot.
pub fn override_finding(
    state: &AppState,
    req: ArenaOverrideRequest,
) -> Result<ArenaRunSnapshotWire, String> {
    let run_id = req.run_id.clone();
    send(state, |_, arena| ArenaCommand::Override {
        arena,
        run_id: req.run_id,
        finding_id: req.finding_id,
        verdict: parse_verdict(&req.verdict),
        note: req.note,
    })?;
    get(state, run_id)
}

/// Tells the App's arena recovery which runs the actors own. Without it a tab
/// switch marks an actor's running run stale, since the App's registry never
/// holds it.
pub fn attach(app: &App, service: Arc<crate::service::ServiceState>) {
    app.set_arena_live_elsewhere(Arc::new(move |run_id| service.arena_run_live(run_id)));
}

/// The flag-on wiring between the desktop and an actor's live runs: progress
/// reaches the front end through the desktop revision, and the App's
/// recovery leaves a run an actor owns alone.
#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::atomic::AtomicU64;
    use std::sync::mpsc::{self, Sender};
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use er_engine::app::TabState;
    use er_engine::arena::{ArenaPaths, ArenaScope, ReviewerRef, RunStatus};
    use er_engine::config::{AiModelConfig, AiProviderConfig, ErConfig};
    use er_engine::jobs::{JobId, JobResult, JobSink, JobSubmission, JobTicket};
    use er_engine::paths::ErRoot;
    use er_engine::ports::Ports;
    use er_git::FakeGit;
    use er_service::{TabContext, WorkspaceDeps};

    use super::*;
    use crate::service::ServiceState;

    const DEADLINE: Duration = Duration::from_secs(10);
    const KEY: &str = "/repo";

    const RAW: &str = "\
diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,2 +1,3 @@
 fn a() {}
+fn b() {}
 fn c() {}
";

    /// Holds every job unanswered, so a run stays mid-round until the test
    /// lets go. A dropped answer fails its job; once released, the sink fails
    /// each new job at once, so the run ends whatever stage it had reached.
    struct HoldingSink {
        held: Mutex<Option<Vec<HeldJob>>>,
    }

    /// A job and the answer the sink has not sent.
    type HeldJob = (JobSubmission, Sender<JobResult>);

    impl Default for HoldingSink {
        fn default() -> Self {
            Self {
                held: Mutex::new(Some(Vec::new())),
            }
        }
    }

    impl JobSink for HoldingSink {
        fn submit(&self, job: JobSubmission) -> JobTicket {
            let (answer, ticket) = mpsc::channel();
            match self.held.lock().unwrap().as_mut() {
                Some(held) => held.push((job, answer)),
                None => job.handle.mark_finished(),
            }
            JobTicket::new(JobId::next(), ticket)
        }
    }

    impl HoldingSink {
        fn release(&self) {
            for (job, _answer) in self.held.lock().unwrap().take().unwrap_or_default() {
                job.handle.mark_finished();
            }
        }
    }

    fn service(sink: Arc<HoldingSink>, revision: Arc<AtomicU64>) -> Arc<ServiceState> {
        let git: Arc<dyn er_git::Git> =
            Arc::new(FakeGit::default().with_diff_against_branch(Ok(RAW.to_string())));
        let deps = WorkspaceDeps::new(Ports::with_git(git)).with_jobs(sink);
        // The relay `main.rs` gives the service: each published view bumps
        // the desktop revision.
        Arc::new(ServiceState::with_deps(deps, move |_| {
            crate::profile_log::bump_desktop_revision(&revision, "service_view");
        }))
    }

    fn fake_ref() -> ReviewerRef {
        ReviewerRef {
            provider_id: "fake".to_string(),
            model_id: "fake-1".to_string(),
            agent_kind: None,
        }
    }

    fn context(bucket: &str) -> ArenaContext {
        let mut config = ErConfig::default();
        config.ai_hub.providers.insert(
            "fake".to_string(),
            AiProviderConfig {
                command: "fake-provider".to_string(),
                args: vec!["{prompt}".to_string()],
                models: vec![AiModelConfig {
                    id: "fake-1".to_string(),
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        ArenaContext {
            tab: TabContext {
                er_dir: bucket.to_string(),
                comments_dir: bucket.to_string(),
                is_remote: false,
                repo_root: bucket.to_string(),
                base_branch: "main".to_string(),
                scope: "branch".to_string(),
                remote_repo: None,
                pr_number: None,
                pr_head_ref: None,
                local_branch_view: Some("feature".to_string()),
                checkout_root: None,
                branch_diff_hash: String::new(),
                diff_hash: String::new(),
            },
            branch_ref: "feature".to_string(),
            config: Arc::new(config),
            buckets: vec![bucket.to_string()],
            review_hash: String::new(),
        }
    }

    /// Starts a run through the actor directly. `ServiceState::send` never
    /// bumps the desktop revision itself, unlike the routed commands.
    fn start_run(service: &ServiceState, bucket: &str) -> String {
        let reply = service
            .send(
                KEY,
                Command::Arena(ArenaCommand::StartRun {
                    arena: context(bucket),
                    params: ArenaStartParams {
                        title: None,
                        reviewers: vec![fake_ref(), fake_ref()],
                        scope: ArenaScope::Branch,
                        files: None,
                        rounds: Some(1),
                        arbiter: Some(fake_ref()),
                        confirm: true,
                        agent_kind: None,
                        effort: None,
                    },
                }),
            )
            .unwrap();
        let Reply::ArenaStarted { mut run_ids } = reply else {
            panic!("unexpected reply {reply:?}");
        };
        run_ids.remove(0)
    }

    fn status(bucket: &str, run_id: &str) -> RunStatus {
        er_engine::arena::load_run(&ArenaPaths::for_run(Path::new(bucket), run_id))
            .unwrap()
            .status
    }

    fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + DEADLINE;
        while !done() {
            assert!(Instant::now() < deadline, "{what} within {DEADLINE:?}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn is_terminal(status: &RunStatus) -> bool {
        matches!(
            status,
            RunStatus::Complete | RunStatus::Failed | RunStatus::Cancelled
        )
    }

    fn bucket_in(dir: &Path) -> String {
        let bucket = dir.join("bucket");
        std::fs::create_dir_all(&bucket).unwrap();
        bucket.to_string_lossy().into_owned()
    }

    /// The live arena page re-reads only on `er://revision`. A run's progress
    /// and its end publish from the actor after the start command has
    /// answered, so they reach the page only if a published view bumps the
    /// desktop revision.
    #[test]
    fn a_published_arena_view_bumps_the_desktop_revision() {
        let dir = tempfile::tempdir().unwrap();
        let bucket = bucket_in(dir.path());
        er_engine::storage::with_process_test_storage_root(dir.path(), || {
            let sink = Arc::new(HoldingSink::default());
            let revision = Arc::new(AtomicU64::new(0));
            let service = service(Arc::clone(&sink), Arc::clone(&revision));
            let run_id = start_run(&service, &bucket);

            sink.release();
            wait_until("the run's end reaches the view", || {
                service
                    .view(KEY)
                    .arena_run(&run_id)
                    .is_some_and(|live| is_terminal(&live.status))
            });
            let published = service.view(KEY).revision;
            wait_until("the desktop revision follows the view", || {
                revision.load(Ordering::Relaxed) >= published
            });
            wait_until("the run's record is terminal", || {
                is_terminal(&status(&bucket, &run_id))
            });
        });
    }

    /// A tab switch runs the App's arena recovery over the bucket. With the
    /// predicate `main.rs` attaches, the actor's mid-round run stays
    /// `Running` while an unowned one is still marked `Failed`.
    #[test]
    fn a_tab_switch_leaves_an_actor_run_running() {
        let dir = tempfile::tempdir().unwrap();
        let bucket = bucket_in(dir.path());
        er_engine::storage::with_process_test_storage_root(dir.path(), || {
            let sink = Arc::new(HoldingSink::default());
            let service = service(Arc::clone(&sink), Arc::new(AtomicU64::new(0)));
            let run_id = start_run(&service, &bucket);
            wait_until("the run is live in the view", || {
                service.view(KEY).arena_run(&run_id).is_some()
            });
            wait_until("the run's record is running", || {
                matches!(status(&bucket, &run_id), RunStatus::Running { .. })
            });
            let orphan = ArenaPaths::for_run(Path::new(&bucket), "orphan-run");
            let mut record =
                er_engine::arena::load_run(&ArenaPaths::for_run(Path::new(&bucket), &run_id))
                    .unwrap();
            record.id = "orphan-run".to_string();
            er_engine::arena::save_run(&orphan, &record).unwrap();

            let mut app = App::new_for_test(Vec::new());
            let mut tab = TabState::new_for_test(Vec::new());
            tab.er_root = ErRoot::managed(bucket.clone());
            app.tabs.push(tab);
            attach(&app, Arc::clone(&service));
            app.select_tab(1);

            let left = status(&bucket, &run_id);
            let orphaned = status(&bucket, "orphan-run");
            sink.release();
            wait_until("the run's record is terminal", || {
                is_terminal(&status(&bucket, &run_id))
            });
            assert!(
                matches!(left, RunStatus::Running { .. }),
                "the actor's run is left alone: {left:?}"
            );
            assert_eq!(orphaned, RunStatus::Failed, "an unowned run is recovered");
        });
    }
}
