//! The arena with no App: a command names its tab and runs there, whichever
//! tab is active, with the App locked, and its context comes from that tab's
//! content and the settings actor's view.

use std::path::Path;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use er_engine::arena::{ArenaPaths, ArenaScope, ArenaStartParams, ReviewerRef};
use er_engine::jobs::{JobId, JobResult, JobSink, JobSubmission, JobTicket};
use er_engine::tab_identity::DiffMode;
use er_engine::tabs_record::TabDescriptor;
use er_engine::TabKey;
use er_protocol::Command;
use er_service::WorkspaceDeps;

use super::diff_command_tests::send;
use super::diff_resource_tests::{raw_diff, with_storage};
use super::{review_flow_test_state, ReviewFlowTestDir};
use crate::commands::{content_routes, DesktopState};
use crate::service::{actor_key, ServiceState};

const WAIT: Duration = Duration::from_secs(10);

/// A provider the settings actor loads from the test's own config, so a run
/// can name its reviewers.
const CONFIG: &str = r#"
[ai_hub.providers.fake]
command = "fake-provider"
args = ["{prompt}"]

[[ai_hub.providers.fake.models]]
id = "fake-1"
effort_levels = ["low", "high"]
"#;

/// A job and the answer the sink has not sent.
type HeldJob = (JobSubmission, Sender<JobResult>);

/// Holds every job unanswered, so a run stays live until the test lets go;
/// once released, each new job ends at once.
#[derive(Default)]
pub(super) struct HoldingSink {
    held: Mutex<Option<Vec<HeldJob>>>,
    released: std::sync::atomic::AtomicBool,
}

impl JobSink for HoldingSink {
    fn submit(&self, job: JobSubmission) -> JobTicket {
        let (answer, ticket) = mpsc::channel();
        let mut held = self.held.lock().unwrap_or_else(|e| e.into_inner());
        if self.released.load(std::sync::atomic::Ordering::Acquire) {
            job.handle.mark_finished();
        } else {
            held.get_or_insert_with(Vec::new).push((job, answer));
        }
        JobTicket::new(JobId::next(), ticket)
    }
}

impl HoldingSink {
    /// How many jobs the sink holds unanswered.
    pub(super) fn held(&self) -> usize {
        self.held
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map_or(0, Vec::len)
    }

    /// Ends the oldest held job with no answer, which its reviewer reads as a
    /// failure; the other held jobs stay live.
    pub(super) fn end_one(&self) {
        let mut held = self.held.lock().unwrap_or_else(|e| e.into_inner());
        let jobs = held.get_or_insert_with(Vec::new);
        assert!(!jobs.is_empty(), "no job is held");
        let (job, _answer) = jobs.remove(0);
        job.handle.mark_finished();
    }

    pub(super) fn release(&self) {
        self.released
            .store(true, std::sync::atomic::Ordering::Release);
        let held = self.held.lock().unwrap_or_else(|e| e.into_inner()).take();
        for (job, _answer) in held.unwrap_or_default() {
            job.handle.mark_finished();
        }
    }
}

/// A state whose actors hand their jobs to `sink` and whose settings actor
/// reads the test's config.
pub(super) fn arena_state(
    dir: &ReviewFlowTestDir,
    ports: er_engine::ports::Ports,
    sink: Arc<HoldingSink>,
) -> DesktopState {
    let config = dir.path().join("settings").join("config.toml");
    std::fs::create_dir_all(config.parent().unwrap_or_else(|| Path::new("/")))
        .unwrap_or_else(|e| panic!("settings dir: {e}"));
    std::fs::write(&config, CONFIG).unwrap_or_else(|e| panic!("config: {e}"));
    let mut state = review_flow_test_state(dir);
    state.ports = ports.clone();
    let deps = WorkspaceDeps::new(ports).with_jobs(sink);
    state.service = Arc::new(ServiceState::with_deps(deps, |_| {}).with_config_path(config));
    state
}

/// A git with no remote, so each repo's buckets are its own.
pub(super) fn local_ports() -> er_engine::ports::Ports {
    let git: Arc<dyn er_git::Git> = Arc::new(
        er_git::FakeGit::default()
            .with_current_branch(Ok("feature".to_string()))
            .with_detect_base_branch(Ok("main".to_string()))
            .with_diff_raw(Ok(raw_diff()))
            .with_diff_shortstat((3, 3, 3))
            .with_rev_parse_verify(Ok(Some("b".repeat(40))))
            .with_is_merge_in_progress(false),
    );
    er_engine::ports::Ports::with_git(git)
}

/// Opens a Working tab on a directory of its own; it becomes the active tab.
pub(super) fn open(state: &DesktopState, dir: &Path, name: &str) -> (TabKey, TabDescriptor) {
    let root = dir.join(name);
    std::fs::create_dir_all(&root).unwrap_or_else(|e| panic!("{name}: {e}"));
    let tab = content_routes::working_tab(root.to_string_lossy().into_owned());
    let opened = content_routes::open(state, tab.clone(), false);
    assert!(opened.is_ok(), "{opened:?}");
    let active = state.service.app_registry().tabs_view().active;
    (active.unwrap_or_else(|| panic!("no active tab")), tab)
}

/// The tab's bucket once its content is published.
pub(super) fn bucket(state: &DesktopState, (key, tab): &(TabKey, TabDescriptor)) -> String {
    wait_until("the tab's content", || {
        state.service.content(*key, tab).is_some()
    });
    state
        .service
        .content(*key, tab)
        .map(|content| content.context.er_dir.clone())
        .unwrap_or_else(|| panic!("no content under {key:?}"))
}

pub(super) fn fake() -> ReviewerRef {
    ReviewerRef {
        provider_id: "fake".into(),
        model_id: "fake-1".into(),
        agent_kind: None,
    }
}

fn params() -> ArenaStartParams {
    ArenaStartParams {
        title: None,
        reviewers: vec![fake(), fake()],
        scope: ArenaScope::Branch,
        files: None,
        rounds: Some(1),
        arbiter: Some(fake()),
        confirm: true,
        agent_kind: None,
        effort: None,
    }
}

pub(super) fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !done() {
        assert!(Instant::now() < deadline, "{what} within {WAIT:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

pub(super) fn record(bucket: &str, run_id: &str) -> Option<er_engine::arena::ArenaRun> {
    er_engine::arena::load_run(&ArenaPaths::for_run(Path::new(bucket), run_id)).ok()
}

/// Starts two fake reviewers on `tab` with the App locked; answers the run.
pub(super) fn started(state: &DesktopState, tab: TabKey) -> String {
    super::on_a_thread(state, "arena start", move |state| {
        let wire = er_service::wire_tab_key(tab);
        let tab = crate::arena_service::tab(&state, Some(wire))?;
        crate::arena_service::start(&state, &tab, params())
    })
    .unwrap_or_else(|e| panic!("{e:?}"))
}

/// The registry moved to B while the page shows A: a start keyed to A, run
/// with the App locked, files its run in A's bucket and none in B's.
#[test]
fn an_arena_start_runs_on_the_tab_it_names_with_the_app_locked() {
    with_storage(|dir| {
        let sink = Arc::new(HoldingSink::default());
        let state = arena_state(dir, local_ports(), Arc::clone(&sink));
        let painted = open(&state, dir.path(), "a");
        let active = open(&state, dir.path(), "b");
        let (painted_bucket, active_bucket) = (bucket(&state, &painted), bucket(&state, &active));
        assert_ne!(
            painted_bucket, active_bucket,
            "the fixture's two buckets are one"
        );

        let run_id = started(&state, painted.0);

        assert!(
            record(&painted_bucket, &run_id).is_some(),
            "no run in the named tab's bucket"
        );
        assert!(
            record(&active_bucket, &run_id).is_none(),
            "the run went to the active tab"
        );
        sink.release();
    });
}

/// With the App locked, the saved selection's effort is what a start with no
/// effort of its own runs at: the settings actor's view, which `SetAiEffort`
/// just wrote, never a copy the App holds.
#[test]
fn an_arena_start_runs_at_the_saved_effort_with_the_app_locked() {
    with_storage(|dir| {
        let sink = Arc::new(HoldingSink::default());
        let state = arena_state(dir, local_ports(), Arc::clone(&sink));
        let tab = open(&state, dir.path(), "a");
        let pick = Command::SetAiEffort {
            effort: Some("high".into()),
        };
        let saved = send(&state, "set_ai_effort", pick);
        assert!(saved.is_ok(), "{saved:?}");

        let run_id = started(&state, tab.0);

        let run = record(&bucket(&state, &tab), &run_id).unwrap_or_else(|| panic!("no run"));
        assert_eq!(run.config.effort.as_deref(), Some("high"));
        sink.release();
    });
}

/// A branch tab viewing a branch other than the checkout's, with a review in
/// its AI section: the context built with the App locked files under the
/// viewed branch and carries the review's hash.
#[test]
fn an_arena_context_takes_the_viewed_branch_and_the_review_hash_with_the_app_locked() {
    with_storage(|dir| {
        let sink = Arc::new(HoldingSink::default());
        let state = arena_state(dir, super::tab_key_tests::ports(), Arc::clone(&sink));
        let root = dir.path().join("repo");
        std::fs::create_dir_all(&root).unwrap_or_else(|e| panic!("repo: {e}"));
        let mut tab = content_routes::working_tab(root.to_string_lossy().into_owned());
        tab.kind = er_engine::tabs_record::TabKind::LocalBranch;
        tab.branch = Some("topic".to_string());
        let opened = content_routes::open(&state, tab.clone(), false);
        assert!(opened.is_ok(), "{opened:?}");
        let key = state.service.app_registry().tabs_view().active;
        let entry = (key.unwrap_or_else(|| panic!("no active tab")), tab);
        let er_dir = bucket(&state, &entry);
        let content = state.service.content(entry.0, &entry.1);
        let content = content.unwrap_or_else(|| panic!("no content"));
        assert_eq!(
            content.tab.local_branch_view.as_deref(),
            Some("topic"),
            "the fixture views no branch"
        );
        assert_ne!(
            content.tab.current_branch, "topic",
            "the fixture's two branches are one"
        );
        er_engine::test_support::write_sidecar_json::<er_engine::ai::ErReview>(
            Path::new(&er_dir),
            serde_json::json!({ "version": 1, "diff_hash": "reviewed-hash", "files": {} }),
        );
        let reload = er_service::Command::ReloadSidecars { key: entry.0 };
        let reloaded = state.service.send(&actor_key(&entry.1), reload);
        assert!(reloaded.is_ok(), "{reloaded:?}");
        wait_until("the review in the AI section", || {
            state
                .service
                .content(entry.0, &entry.1)
                .is_some_and(|content| content.ai.state.review.is_some())
        });

        let wire = er_service::wire_tab_key(entry.0);
        let arena = super::on_a_thread(&state, "arena context", move |state| {
            let tab = crate::arena_service::tab(&state, Some(wire))?;
            let config = crate::arena_service::config(&state);
            Ok::<_, er_protocol::ErError>(crate::arena_service::context(&state, &tab, config))
        })
        .unwrap_or_else(|e| panic!("{e:?}"));

        assert_eq!(arena.branch_ref, "topic");
        assert_eq!(arena.review_hash, "reviewed-hash");
        sink.release();
    });
}

/// A remote PR tab's content lives on its `remote:` actor, never on the
/// actor of the process directory its content roots at: a start keyed to it
/// runs there.
#[test]
fn an_arena_run_on_a_remote_pr_tab_keys_through_tabs() {
    with_storage(|dir| {
        let sink = Arc::new(HoldingSink::default());
        let state = arena_state(dir, super::tab_key_tests::ports(), Arc::clone(&sink));
        let (_, remote) = super::tab_key_tests::kinds(dir)
            .into_iter()
            .find(|(kind, _)| *kind == "remote PR")
            .unwrap_or_else(|| panic!("no remote PR kind"));
        let opened = content_routes::open(&state, remote.clone(), false);
        assert!(opened.is_ok(), "{opened:?}");
        let key = state.service.app_registry().tabs_view().active;
        let key = key.unwrap_or_else(|| panic!("no active tab"));
        let content = state
            .service
            .content(key, &remote)
            .unwrap_or_else(|| panic!("no content"));
        assert_ne!(
            actor_key(&remote),
            content.context.repo_root,
            "the fixture keys one actor"
        );

        let run_id = started(&state, key);

        let actor = state.service.published_view(&actor_key(&remote));
        assert!(
            actor.is_some_and(|view| view.arena_run(&run_id).is_some()),
            "the run is not on the remote PR's actor"
        );
        sink.release();
    });
}

/// A loaded tab that left the mode it was restored in files under its
/// content's bucket, which differs from the bucket its descriptor resolves
/// to; a context built for another tab lists that content bucket.
#[test]
fn a_tab_moved_off_its_restore_mode_lists_its_content_bucket() {
    with_storage(|dir| {
        let sink = Arc::new(HoldingSink::default());
        let state = arena_state(dir, local_ports(), Arc::clone(&sink));
        let moved = open(&state, dir.path(), "a");
        let staged =
            content_routes::set_tab_mode(&state, moved.0, "staged", DiffMode::Staged, None);
        assert!(staged.is_ok(), "{staged:?}");
        wait_until("the staged view to load", || {
            state
                .service
                .content(moved.0, &moved.1)
                .is_some_and(|c| c.tab.mode == DiffMode::Staged)
        });
        let content_bucket = bucket(&state, &moved);
        let restore_bucket = er_service::descriptor_bucket(&*state.ports.git, &moved.1)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_ne!(
            content_bucket, restore_bucket,
            "the fixture's two buckets are one"
        );
        let other = open(&state, dir.path(), "b");

        let tab = crate::arena_service::tab(&state, Some(er_service::wire_tab_key(other.0)))
            .unwrap_or_else(|e| panic!("{e:?}"));
        let config = crate::arena_service::config(&state);
        let arena = crate::arena_service::context(&state, &tab, config);

        assert!(
            arena.buckets.contains(&content_bucket),
            "{:?}",
            arena.buckets
        );
        assert!(
            !arena.buckets.contains(&restore_bucket),
            "{:?}",
            arena.buckets
        );
        sink.release();
    });
}

/// For each kind of tab, the bucket its descriptor resolves to with no load
/// is the bucket its first load shows. The local PR tab opens on PR Diff,
/// whose bucket differs from its Branch view's.
#[test]
fn a_descriptor_resolves_to_the_bucket_its_first_load_shows() {
    // Each kind in storage of its own: a saved tab of the same PR would take
    // the open over.
    for at in 0..5 {
        with_storage(|dir| {
            let (kind, tab) = super::tab_key_tests::kinds(dir).swap_remove(at);
            let state = super::tab_key_tests::state(dir);
            let git = super::tab_key_tests::ports().git;
            let opened = content_routes::open(&state, tab.clone(), false);
            assert!(opened.is_ok(), "{kind}: {opened:?}");
            let key = state.service.app_registry().tabs_view().active;
            let entry = (
                key.unwrap_or_else(|| panic!("{kind}: no active tab")),
                tab.clone(),
            );
            wait_until(kind, || {
                let view = state.service.published_view(&actor_key(&tab));
                if let Some(error) = view.as_ref().and_then(|v| v.load_errors.get(&entry.0)) {
                    panic!("{kind}: {}", error.message);
                }
                view.is_some_and(|v| v.tab(entry.0).is_some())
            });
            let loaded = bucket(&state, &entry);
            if kind == "local PR" {
                let mut branch = er_engine::tab_identity::resolve_descriptor(&*git, &tab)
                    .unwrap_or_else(|e| panic!("{e}"));
                assert_eq!(branch.mode, DiffMode::PrDiff);
                branch.mode = DiffMode::Branch;
                assert_ne!(
                    er_service::tab_bucket(&*git, &branch),
                    loaded,
                    "{kind}: the PR Diff and Branch buckets are one"
                );
            }

            let resolved = er_service::descriptor_bucket(&*git, &tab);

            assert_eq!(resolved.ok().as_deref(), Some(loaded.as_str()), "{kind}");
        });
    }
}

/// `arena_service.rs` names the App nowhere outside its tests: every command
/// and read builds its context from the tab's content and the settings view.
#[test]
fn the_arena_module_names_no_app() {
    for (name, source) in [("arena_service.rs", include_str!("../../arena_service.rs"))] {
        let body = source.split("#[cfg(test)]").next().unwrap_or(source);
        for app in ["er_engine::app", "AppState", ".app.lock(", "state.app"] {
            assert!(!body.contains(app), "{name} names {app}");
        }
    }
}
