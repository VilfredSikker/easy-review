//! The arena on the flag path. Each command names a tab, or takes the
//! registry's active one, and builds its context from that tab's published
//! content and the settings actor's view, then sends the command to the tab's
//! workspace actor, which runs the arena and its jobs. Reads never wait on the
//! actor (ADR 0067): progress and live status come from its published view,
//! and run records from the store. The App is out of reach here: every
//! function takes `DesktopState`.
//!
//! Only compiled with `v1-service`.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use er_engine::arena::{
    ArenaBatchStartParams, ArenaDiffPreview, ArenaProgressState, ArenaRunSnapshot, ArenaRunSummary,
    ArenaScope, ArenaStartParams, ReviewerRef, Verdict,
};
use er_engine::config::ErConfig;
use er_protocol::{ErError, ErErrorKind};
use er_service::{ArenaCommand, ArenaContext, Command, Reply, TabContent, TabEntry, WorkspaceView};

use crate::service::actor_key;
use crate::state::DesktopState;

/// The tab an arena command acts on, with the content it shows.
pub struct ArenaTab {
    pub entry: TabEntry,
    pub content: Arc<TabContent>,
}

impl ArenaTab {
    /// The actor that holds this tab's content.
    pub fn workspace(&self) -> String {
        actor_key(&self.entry.descriptor)
    }
}

/// The tab `wire` names in the registry, else the registry's active one, with
/// its published content. A key the registry does not list is `NotFound`; a
/// listed tab nothing has loaded is sent a load and refused for now.
pub fn tab(desktop: &DesktopState, wire: Option<er_protocol::TabKey>) -> Result<ArenaTab, ErError> {
    let tabs = desktop.service.app_registry().tabs_view();
    let key = match wire {
        Some(wire) => er_service::resolve_tab_key(&tabs, wire)?,
        None => tabs
            .active
            .ok_or_else(|| error(ErErrorKind::NotFound, "no tab is open"))?,
    };
    let entry = tabs
        .tabs
        .iter()
        .find(|entry| entry.key == key)
        .cloned()
        .ok_or_else(|| error(ErErrorKind::NotFound, "no open tab has that key"))?;
    let Some(content) = desktop.service.content(entry.key, &entry.descriptor) else {
        desktop.service.request_load(entry.key, &entry.descriptor);
        return Err(error(ErErrorKind::Refused, "The tab has not loaded yet"));
    };
    Ok(ArenaTab { entry, content })
}

/// A listed tab's bucket: its content's when it is loaded, since a loaded tab
/// may have left the mode it was restored in, and otherwise the bucket its
/// descriptor resolves to, read with no load.
pub fn listed_bucket(desktop: &DesktopState, entry: &TabEntry) -> Option<String> {
    match desktop.service.content(entry.key, &entry.descriptor) {
        Some(content) => Some(content.context.er_dir.clone()),
        None => er_service::descriptor_bucket(&*desktop.ports.git, &entry.descriptor).ok(),
    }
}

/// `tab`'s bucket, then every other listed tab's, each once.
fn buckets(desktop: &DesktopState, tab: &ArenaTab) -> Vec<String> {
    let mut buckets = vec![tab.content.context.er_dir.clone()];
    let tabs = desktop.service.app_registry().tabs_view();
    for entry in tabs.tabs.iter().filter(|entry| entry.key != tab.entry.key) {
        if let Some(bucket) = listed_bucket(desktop, entry) {
            if !buckets.contains(&bucket) {
                buckets.push(bucket);
            }
        }
    }
    buckets
}

/// The settings actor's config, which owns `config.toml` (ADR 0069).
pub fn config(desktop: &DesktopState) -> Arc<ErConfig> {
    Arc::clone(&desktop.service.settings().view().config)
}

/// The run's effort: the one asked for, else the saved selection's (A18),
/// resolved the way the flag-off start resolves the App's pick.
pub fn resolve_effort(config: &ErConfig, requested: Option<&str>) -> Option<String> {
    let saved = config
        .ai_hub
        .resolve_default_selection(&config.agent)
        .effort;
    er_engine::config::resolve_effort(&config.ai_hub, &config.agent, saved.as_deref(), requested)
}

/// What an arena effect reads from `tab`, from its content and the
/// settings view.
pub fn context(desktop: &DesktopState, tab: &ArenaTab, config: Arc<ErConfig>) -> ArenaContext {
    let content = &tab.content;
    ArenaContext {
        tab: content.context.clone(),
        branch_ref: content.tab.identity().branch_ref().to_string(),
        config,
        buckets: buckets(desktop, tab),
        review_hash: content
            .ai
            .state
            .review
            .as_ref()
            .map(|review| review.diff_hash.clone())
            .unwrap_or_default(),
    }
}

fn error(kind: ErErrorKind, message: impl Into<String>) -> ErError {
    ErError {
        kind,
        message: message.into(),
    }
}

fn failed(error: impl std::fmt::Display) -> ErError {
    self::error(ErErrorKind::Refused, error.to_string())
}

fn unexpected(answer: impl std::fmt::Debug) -> ErError {
    error(
        ErErrorKind::Internal,
        format!("unexpected answer from the workspace: {answer:?}"),
    )
}

/// Sends one arena command to the tab's actor and waits for its bounded
/// reply. A command that changes a run bumps the desktop revision, as the
/// flag-off commands do, for the readers still on the snapshot.
fn send(desktop: &DesktopState, key: &str, command: ArenaCommand) -> Result<Reply, ErError> {
    let reply = desktop
        .service
        .send(key, Command::Arena(command))
        .map_err(failed)?;
    desktop.desktop_revision.fetch_add(1, Ordering::Relaxed);
    Ok(reply)
}

/// The published view that holds `run_id` live, else the tab's actor's. A
/// run keeps running on the actor that started it after the page moved to a
/// tab of another project.
fn live_view(desktop: &DesktopState, tab: &ArenaTab, run_id: &str) -> Arc<WorkspaceView> {
    desktop
        .service
        .published_views()
        .into_iter()
        .find(|view| view.arena_run(run_id).is_some())
        .unwrap_or_else(|| desktop.service.view(&tab.workspace()))
}

/// The actor running `run_id`, else the tab's.
fn live_key(desktop: &DesktopState, tab: &ArenaTab, run_id: &str) -> String {
    desktop
        .service
        .live_arena_key(run_id)
        .unwrap_or_else(|| tab.workspace())
}

/// One run's reviewers and scope, as the launcher prices them.
pub struct Plan {
    pub scope: ArenaScope,
    pub files: Option<Vec<String>>,
    pub reviewers: Vec<ReviewerRef>,
    pub rounds: Option<u8>,
    pub arbiter: Option<ReviewerRef>,
}

/// What `plan` would cost on the tab's diff. The diff is read here, through
/// the ports, so the estimate never waits on the actor's queue.
pub fn estimate(
    desktop: &DesktopState,
    tab: &ArenaTab,
    plan: &Plan,
) -> Result<ArenaDiffPreview, ErError> {
    let arena = context(desktop, tab, config(desktop));
    let plan = er_service::arena::Estimate {
        scope: plan.scope,
        files: plan.files.as_deref(),
        reviewers: &plan.reviewers,
        rounds: plan.rounds,
        arbiter: plan.arbiter.as_ref(),
    };
    er_service::arena::estimate(&desktop.ports, &arena, &plan).map_err(failed)
}

pub fn estimate_batch(
    desktop: &DesktopState,
    tab: &ArenaTab,
    params: &ArenaBatchStartParams,
) -> Result<ArenaDiffPreview, ErError> {
    let arena = context(desktop, tab, config(desktop));
    er_service::arena::estimate_batch(&desktop.ports, &arena, params).map_err(failed)
}

/// Starts one run. A run with no effort of its own takes the saved one.
pub fn start(
    desktop: &DesktopState,
    tab: &ArenaTab,
    mut params: ArenaStartParams,
) -> Result<String, ErError> {
    let config = config(desktop);
    params.effort = resolve_effort(&config, params.effort.as_deref());
    let arena = context(desktop, tab, config);
    match send(
        desktop,
        &tab.workspace(),
        ArenaCommand::StartRun { arena, params },
    )? {
        Reply::ArenaStarted { mut run_ids } if run_ids.len() == 1 => Ok(run_ids.remove(0)),
        other => Err(unexpected(other)),
    }
}

pub fn start_batch(
    desktop: &DesktopState,
    tab: &ArenaTab,
    mut params: ArenaBatchStartParams,
) -> Result<Vec<String>, ErError> {
    let config = config(desktop);
    params.effort = resolve_effort(&config, params.effort.as_deref());
    let arena = context(desktop, tab, config);
    match send(
        desktop,
        &tab.workspace(),
        ArenaCommand::StartBatch { arena, params },
    )? {
        Reply::ArenaStarted { run_ids } => Ok(run_ids),
        other => Err(unexpected(other)),
    }
}

pub fn validate_findings(
    desktop: &DesktopState,
    tab: &ArenaTab,
) -> Result<Option<String>, ErError> {
    let arena = context(desktop, tab, config(desktop));
    match send(
        desktop,
        &tab.workspace(),
        ArenaCommand::ValidateFindings { arena },
    )? {
        Reply::ArenaValidating { run_id } => Ok(run_id),
        other => Err(unexpected(other)),
    }
}

pub fn accept_findings(
    desktop: &DesktopState,
    tab: &ArenaTab,
    run_id: String,
    finding_ids: Option<Vec<String>>,
) -> Result<usize, ErError> {
    let arena = context(desktop, tab, config(desktop));
    let command = ArenaCommand::AcceptFindings {
        arena,
        run_id,
        finding_ids,
    };
    match send(desktop, &tab.workspace(), command)? {
        Reply::ArenaAccepted { accepted } => Ok(accepted),
        other => Err(unexpected(other)),
    }
}

pub fn progress(desktop: &DesktopState, tab: &ArenaTab, run_id: &str) -> ArenaProgressState {
    er_service::arena::read_progress(&live_view(desktop, tab, run_id), run_id)
}

pub fn get(
    desktop: &DesktopState,
    tab: &ArenaTab,
    run_id: &str,
) -> Result<ArenaRunSnapshot, ErError> {
    let arena = context(desktop, tab, config(desktop));
    er_service::arena::read_run(&live_view(desktop, tab, run_id), &arena, run_id).map_err(failed)
}

pub fn list(desktop: &DesktopState, tab: &ArenaTab) -> Result<Vec<ArenaRunSummary>, ErError> {
    let arena = context(desktop, tab, config(desktop));
    er_service::arena::read_runs(&arena).map_err(failed)
}

pub fn delete(desktop: &DesktopState, tab: &ArenaTab, run_id: String) -> Result<(), ErError> {
    let arena = context(desktop, tab, config(desktop));
    let key = live_key(desktop, tab, &run_id);
    send(desktop, &key, ArenaCommand::Delete { arena, run_id }).map(|_| ())
}

pub fn cancel(desktop: &DesktopState, tab: &ArenaTab, run_id: String) -> Result<(), ErError> {
    let key = live_key(desktop, tab, &run_id);
    send(desktop, &key, ArenaCommand::Cancel { run_id }).map(|_| ())
}

/// Records the verdict beside the arbiter's.
pub fn override_finding(
    desktop: &DesktopState,
    tab: &ArenaTab,
    run_id: String,
    finding_id: String,
    verdict: Verdict,
    note: String,
) -> Result<(), ErError> {
    let arena = context(desktop, tab, config(desktop));
    let key = live_key(desktop, tab, &run_id);
    let command = ArenaCommand::Override {
        arena,
        run_id,
        finding_id,
        verdict,
        note,
    };
    send(desktop, &key, command).map(|_| ())
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
    use er_engine::app::App;

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
            crate::arena_commands::attach_live_runs(&app, Arc::clone(&service));
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
