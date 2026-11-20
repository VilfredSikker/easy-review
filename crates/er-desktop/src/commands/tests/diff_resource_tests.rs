//! `diff(tab)` on the flag path: a fetch answers from the published content
//! and never waits on the actor (ADR 0067), a listed key nothing loaded
//! answers empty at version 0 and loads, and a failed first load is answered
//! as an error that a refresh clears.

use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use er_engine::tabs_record::TabDescriptor;
use er_engine::TabKey;
use er_protocol::{ErErrorKind, Resource, ResourceKey, TabDiff, Versioned};
use er_service::{AppCommand, Command};

use super::{review_flow_test_state, ReviewFlowTestDir};
use crate::commands::{content_routes, AppState};
use crate::resources::{self, Watched};
use crate::service::{actor_key, ServiceState};

/// How long a read may take while the tab's actor is held. A read that sent
/// the actor a command and waited would take the full reply timeout.
const BOUND: Duration = Duration::from_secs(2);
const WAIT: Duration = Duration::from_secs(10);

fn raw_diff() -> String {
    use std::fmt::Write as _;

    let mut raw = String::new();
    for n in 0..3 {
        let _ = write!(
            raw,
            "diff --git a/src/f{n}.rs b/src/f{n}.rs\n--- a/src/f{n}.rs\n+++ b/src/f{n}.rs\n\
             @@ -1,2 +1,2 @@\n-old\n+new\n ctx\n"
        );
    }
    raw
}

fn fake_ports() -> er_engine::ports::Ports {
    let git: Arc<dyn er_git::Git> = Arc::new(
        er_git::FakeGit::default()
            .with_current_branch(Ok("feature".to_string()))
            .with_detect_base_branch(Ok("main".to_string()))
            .with_diff_raw(Ok(raw_diff()))
            .with_diff_shortstat((3, 3, 3))
            .with_is_merge_in_progress(false)
            .with_remote_url(Ok("https://github.com/acme/widgets.git".to_string())),
    );
    let github: Arc<dyn er_engine::github::GitHub> =
        Arc::new(er_engine::github::FakeGitHub::default());
    er_engine::ports::Ports { git, github }
}

fn state_over(dir: &ReviewFlowTestDir, ports: er_engine::ports::Ports) -> AppState {
    let mut state = review_flow_test_state(dir);
    let settings = dir.path().join("settings").join("config.toml");
    state.service = Arc::new(ServiceState::with_settings_path(ports, settings));
    state
}

fn with_storage(test: impl FnOnce(&ReviewFlowTestDir)) {
    let dir = ReviewFlowTestDir::new();
    let storage = dir.path().join("storage");
    er_engine::storage::with_process_test_storage_root(storage, || test(&dir));
}

/// A working tab on `repo` in the registry, which nothing has loaded.
fn listed(state: &AppState, dir: &ReviewFlowTestDir, repo: &str) -> (TabKey, TabDescriptor) {
    let root = dir.path().join(repo);
    std::fs::create_dir_all(&root).unwrap_or_else(|e| panic!("repo dir: {e}"));
    let tab = content_routes::working_tab(root.to_string_lossy().into_owned());
    let added = state.service.send_app(AppCommand::NewTab { tab: tab.clone() });
    assert!(added.is_ok(), "{added:?}");
    let view = state.service.app_registry().tabs_view();
    let key = view.active.unwrap_or_else(|| panic!("no active tab"));
    (key, tab)
}

fn opened(state: &AppState, dir: &ReviewFlowTestDir, repo: &str) -> (TabKey, TabDescriptor) {
    let root = dir.path().join(repo);
    std::fs::create_dir_all(&root).unwrap_or_else(|e| panic!("repo dir: {e}"));
    let tab = content_routes::working_tab(root.to_string_lossy().into_owned());
    let opened = content_routes::open(state, tab.clone(), false);
    assert!(opened.is_ok(), "{opened:?}");
    let key = state.service.app_registry().tabs_view().active;
    (key.unwrap_or_else(|| panic!("no active tab")), tab)
}

fn diff_key(key: TabKey) -> ResourceKey {
    ResourceKey::Diff {
        tab: er_service::wire_tab_key(key),
    }
}

/// Fetches `key`'s diff on a thread of its own, so a fetch that waits on a
/// held actor fails the bound instead of hanging the test.
fn fetch_within_bound(state: &AppState, key: TabKey) -> (Versioned<Resource>, Duration) {
    let desktop = state.desktop.clone();
    let (tx, rx) = mpsc::channel();
    let started = Instant::now();
    std::thread::spawn(move || {
        let _ = tx.send(resources::fetch(&desktop, diff_key(key)));
    });
    let fetched = rx
        .recv_timeout(BOUND)
        .unwrap_or_else(|_| panic!("the diff fetch waited past {BOUND:?}"));
    let fetched = fetched.unwrap_or_else(|e| panic!("fetch: {e:?}"));
    (fetched, started.elapsed())
}

fn diff_of(fetched: &Versioned<Resource>) -> &TabDiff {
    match &fetched.value {
        Resource::Diff(diff) => diff,
        other => panic!("not a diff: {other:?}"),
    }
}

/// Holds `tab`'s actor: every later command to it queues behind this one.
fn hold(state: &AppState, tab: &TabDescriptor) {
    let held = state.service.post_if_started(&actor_key(tab), Command::Stall);
    assert!(held, "the actor was not up");
}

fn wait_until(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !ready() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_loaded_tab_answers_its_diff_while_its_actor_is_held() {
    with_storage(|dir| {
        let state = state_over(dir, fake_ports());
        let (key, tab) = opened(&state, dir, "one");
        hold(&state, &tab);

        let (fetched, took) = fetch_within_bound(&state, key);
        let diff = diff_of(&fetched);
        let paths: Vec<&str> = diff.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["src/f0.rs", "src/f1.rs", "src/f2.rs"]);
        assert!(fetched.version.0 > 0, "a loaded diff answered version 0");
        assert!(took < BOUND, "the fetch took {took:?}");
        assert_eq!((diff.unstaged_stat.files, diff.staged_stat.files), (3, 3));
    });
}

#[test]
fn a_listed_tab_nothing_loaded_answers_empty_at_version_zero_while_its_actor_is_held() {
    with_storage(|dir| {
        let state = state_over(dir, fake_ports());
        let (_, loaded) = opened(&state, dir, "one");
        hold(&state, &loaded);
        // A second tab on the same repo, so on the held actor, never loaded.
        let tab = content_routes::working_tab(loaded.repo_root.clone());
        let added = state.service.send_app(AppCommand::NewTab { tab });
        assert!(added.is_ok(), "{added:?}");
        let key = state
            .service
            .app_registry()
            .tabs_view()
            .active
            .unwrap_or_else(|| panic!("no active tab"));

        let (fetched, _) = fetch_within_bound(&state, key);
        assert_eq!(fetched.version.0, 0);
        assert_eq!(diff_of(&fetched), &TabDiff::default());
    });
}

#[test]
fn the_push_after_the_load_brings_the_rows_of_a_tab_nothing_loaded() {
    with_storage(|dir| {
        let state = state_over(dir, fake_ports());
        let (key, tab) = listed(&state, dir, "two");

        let (first, _) = fetch_within_bound(&state, key);
        assert_eq!(first.version.0, 0);
        assert!(diff_of(&first).files.is_empty());

        wait_until("the load the fetch sent", || {
            state.service.content(key, &tab).is_some()
        });
        let watched = resources::watched(&state.desktop);
        let pushed = watched.get(&Watched::TabDiff(key)).copied().unwrap_or(0);
        assert!(pushed > 0, "the relay announces no diff version");
        let (second, _) = fetch_within_bound(&state, key);
        assert_eq!(second.version.0, pushed, "the push and the fetch disagree");
        assert_eq!(diff_of(&second).files.len(), 3);
    });
}

#[test]
fn an_unlisted_key_is_not_found() {
    with_storage(|dir| {
        let state = state_over(dir, fake_ports());
        let fetched = resources::fetch(&state.desktop, diff_key(TabKey::next()));
        let kind = fetched.err().map(|e| e.kind);
        assert_eq!(kind, Some(ErErrorKind::NotFound));
    });
}

fn git(dir: &std::path::Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .args(["-c", "user.name=er", "-c", "user.email=er@example.com"])
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap_or_else(|e| panic!("git {args:?}: {e}"));
    assert!(status.success(), "git {args:?} failed");
}

#[test]
fn a_failed_first_load_answers_its_error_and_a_refresh_loads_once_the_cause_is_gone() {
    with_storage(|dir| {
        let ports = er_engine::ports::Ports::with_git(Arc::new(er_git::CliGit));
        let state = state_over(dir, ports);
        // Not a repository yet, so the load cannot read its branch.
        let (key, tab) = listed(&state, dir, "later");
        let (first, _) = fetch_within_bound(&state, key);
        assert_eq!(first.version.0, 0);

        let watched = || {
            resources::watched(&state.desktop)
                .get(&Watched::TabDiff(key))
                .copied()
                .unwrap_or(0)
        };
        wait_until("the failed load to publish", || watched() > 0);
        let (failed, _) = fetch_within_bound(&state, key);
        assert!(failed.version.0 > 0, "the failure moved no version");
        assert!(diff_of(&failed).load_error.is_some(), "no load error");
        assert!(diff_of(&failed).files.is_empty());

        let root = std::path::PathBuf::from(&tab.repo_root);
        git(&root, &["init", "-q", "-b", "main"]);
        std::fs::write(root.join("a.txt"), "one\n").unwrap_or_else(|e| panic!("{e}"));
        git(&root, &["add", "a.txt"]);
        git(&root, &["commit", "-q", "-m", "first"]);

        let refreshed = content_routes::refresh(&state, false);
        assert!(refreshed.is_ok(), "{refreshed:?}");
        let (loaded, _) = fetch_within_bound(&state, key);
        assert!(loaded.version.0 > failed.version.0);
        assert_eq!(diff_of(&loaded).load_error, None);
    });
}
