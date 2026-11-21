//! The flag-on watcher: its target comes from the registry and the content
//! with the App locked, a file change refreshes the watched tab's content and
//! clears the marks whose diff changed, and a load resolves the checkout root
//! by the watcher's rule, a linked worktree included.

use std::path::Path;
use std::process::Command as Git;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use er_engine::app::TabState;
use er_engine::tab_identity::{resolve_descriptor, DiffMode};
use er_engine::tabs_record::{TabDescriptor, TabKind};
use er_engine::TabKey;
use er_service::{AppCommand, Command, TabContent};

use super::diff_resource_tests::{state_over, with_storage};
use super::ReviewFlowTestDir;
use crate::commands::{content_routes, AppState};
use crate::service::actor_key;
use crate::watcher;

const WAIT: Duration = Duration::from_secs(20);

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Git::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap_or_else(|e| panic!("git {args:?}: {e}"));
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// A repo on `main` with `a.txt`, and a `feature` branch that adds `b.txt`.
fn repo(dir: &ReviewFlowTestDir) -> std::path::PathBuf {
    let root = dir.path().join("repo");
    std::fs::create_dir_all(&root).unwrap_or_else(|e| panic!("repo dir: {e}"));
    git(&root, &["init", "-q", "-b", "main"]);
    std::fs::write(root.join("a.txt"), "one\n").unwrap_or_else(|e| panic!("{e}"));
    git(&root, &["add", "a.txt"]);
    git(&root, &["commit", "-q", "-m", "a"]);
    git(&root, &["branch", "feature"]);
    git(&root, &["checkout", "-q", "feature"]);
    std::fs::write(root.join("b.txt"), "two\n").unwrap_or_else(|e| panic!("{e}"));
    git(&root, &["add", "b.txt"]);
    git(&root, &["commit", "-q", "-m", "b"]);
    git(&root, &["checkout", "-q", "main"]);
    canonical(&root)
}

fn canonical(path: &Path) -> std::path::PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn real_state(dir: &ReviewFlowTestDir) -> AppState {
    state_over(dir, er_engine::ports::Ports::cli())
}

fn branch_view(root: &Path) -> TabDescriptor {
    TabDescriptor {
        kind: TabKind::LocalBranch,
        branch: Some("feature".into()),
        base_ref: Some("main".into()),
        ..content_routes::working_tab(root.to_string_lossy().into_owned())
    }
}

/// Lists `tab` in the registry and loads it in `mode`.
fn load(state: &AppState, tab: &TabDescriptor, mode: DiffMode) -> TabKey {
    let added = state.service.send_app(AppCommand::NewTab { tab: tab.clone() });
    assert!(added.is_ok(), "{added:?}");
    let key = state.service.app_registry().tabs_view().active;
    let key = key.unwrap_or_else(|| panic!("no active tab"));
    let load = Command::LoadTab {
        key,
        tab: tab.clone(),
        mode: Some(mode),
    };
    let loaded = state.service.send(&actor_key(tab), load);
    assert!(loaded.is_ok(), "{loaded:?}");
    key
}

fn content(state: &AppState, key: TabKey, tab: &TabDescriptor) -> std::sync::Arc<TabContent> {
    state
        .service
        .content(key, tab)
        .unwrap_or_else(|| panic!("no content"))
}

fn paths(content: &TabContent) -> Vec<String> {
    content.diff.files.iter().map(|f| f.path.clone()).collect()
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// On a branch view whose branch is checked out in a linked worktree with an
/// unstaged edit, the load finds the worktree, so its Unstaged view is the
/// worktree's, as the App's is once its watcher sets the root.
#[test]
fn a_branch_checked_out_in_a_linked_worktree_loads_that_worktree() {
    with_storage(|dir| {
        let root = repo(dir);
        let wt = dir.path().join("wt");
        git(&root, &["worktree", "add", "-q", &wt.to_string_lossy(), "feature"]);
        let wt = canonical(&wt);
        std::fs::write(wt.join("a.txt"), "edited\n").unwrap_or_else(|e| panic!("{e}"));
        let unstaged = git(&wt, &["diff", "--name-only"]);
        let branch = git(&root, &["diff", "--name-only", "main...feature"]);
        assert_ne!(unstaged, branch, "the fixture's two diffs agree");

        let state = real_state(dir);
        let tab = branch_view(&root);
        let key = load(&state, &tab, DiffMode::Unstaged);
        let loaded = content(&state, key, &tab);
        let checkout = loaded.tab.checkout_root.as_deref().map(|r| canonical(Path::new(r)));
        assert_eq!(checkout, Some(wt.clone()), "the load left the checkout root unset");

        let ports = er_engine::ports::Ports::cli();
        let mut resolved = resolve_descriptor(&*ports.git, &tab).unwrap_or_else(|e| panic!("{e}"));
        resolved.mode = DiffMode::Unstaged;
        let mut app_tab = TabState::from_resolved(resolved, ports).unwrap_or_else(|e| panic!("{e}"));
        app_tab.local_branch_checkout_root = loaded.tab.checkout_root.clone();
        app_tab.refresh_diff().unwrap_or_else(|e| panic!("{e}"));
        let app: Vec<String> = app_tab.files.iter().map(|f| f.path.clone()).collect();
        assert_eq!(paths(&loaded), app);
        assert_eq!(app, ["a.txt"]);
    });
}

/// A branch checked out after the load is followed: the watcher's next turn
/// finds the worktree and reloads the tab onto it, once.
#[test]
fn a_branch_checked_out_after_the_load_is_followed() {
    with_storage(|dir| {
        let root = repo(dir);
        let state = real_state(dir);
        let tab = branch_view(&root);
        let key = load(&state, &tab, DiffMode::Branch);
        assert_eq!(content(&state, key, &tab).tab.checkout_root, None);

        let wt = dir.path().join("wt");
        git(&root, &["worktree", "add", "-q", &wt.to_string_lossy(), "feature"]);
        let ports = er_engine::ports::Ports::cli();
        let mut follower = watcher::Follower::default();
        let watched = follower.next(&state.service, &*ports.git);
        let target = watched.and_then(|w| w.target());
        let wt = canonical(&wt);
        assert_eq!(
            target.map(|(branch, root)| (branch, canonical(Path::new(&root)))),
            Some(("feature".to_string(), wt.clone()))
        );
        wait_until("the reload onto the worktree", || {
            let root = content(&state, key, &tab).tab.checkout_root.clone();
            root.is_some_and(|r| canonical(Path::new(&r)) == wt)
        });
    });
}

/// A file write in the watched checkout, then the watcher's refresh, moves
/// the tab's `diff(tab)` version and its reviewed section, and clears the
/// mark on the file whose diff changed.
#[test]
fn a_file_change_refreshes_the_watched_content_and_clears_its_marks() {
    with_storage(|dir| {
        let root = repo(dir);
        std::fs::write(root.join("a.txt"), "first\n").unwrap_or_else(|e| panic!("{e}"));
        let state = real_state(dir);
        let tab = content_routes::working_tab(root.to_string_lossy().into_owned());
        let key = load(&state, &tab, DiffMode::Unstaged);
        let marked = content_routes::write_reviewed(&state, |context| Command::MarkReviewed {
            context,
            path: "a.txt".into(),
            reviewed: true,
        });
        assert!(marked.is_ok(), "{marked:?}");
        let before = content(&state, key, &tab);
        assert!(before.reviewed.marks.contains_key("a.txt"));
        let diff_before = crate::resources::diff::published_version(&state.desktop, key, &tab);

        let ports = er_engine::ports::Ports::cli();
        let mut follower = watcher::Follower::default();
        let watched = follower.next(&state.service, &*ports.git);
        assert_eq!(watched.map(|w| w.key), Some(key));
        std::fs::write(root.join("a.txt"), "second\n").unwrap_or_else(|e| panic!("{e}"));
        assert!(follower.on_change(&state.service), "the change posted no refresh");

        wait_until("the refresh", || {
            content(&state, key, &tab).reviewed.revision > before.reviewed.revision
        });
        let after = content(&state, key, &tab);
        assert!(!after.reviewed.marks.contains_key("a.txt"), "the mark held");
        let diff_after = crate::resources::diff::published_version(&state.desktop, key, &tab);
        assert!(diff_after > diff_before, "diff(tab) did not move");
    });
}

/// The watcher chooses its target from the registry and the content, so it
/// answers while the App is locked.
#[test]
fn the_watcher_target_is_chosen_with_the_app_locked() {
    with_storage(|dir| {
        let root = repo(dir);
        let state = real_state(dir);
        let tab = content_routes::working_tab(root.to_string_lossy().into_owned());
        let key = load(&state, &tab, DiffMode::Branch);
        let _held = state.app.lock().unwrap_or_else(|e| panic!("{e}"));
        let (tx, rx) = mpsc::channel();
        let service = std::sync::Arc::clone(&state.service);
        std::thread::spawn(move || {
            let ports = er_engine::ports::Ports::cli();
            let _ = tx.send(watcher::Follower::default().next(&service, &*ports.git));
        });
        let watched = rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap_or_else(|_| panic!("the watcher waited on the App lock"));
        assert_eq!(watched.map(|w| w.key), Some(key));
    });
}

/// The flag-on watcher loop takes its target from the follower, never the
/// App, and sends a file change to the follower, which refreshes the content.
#[test]
fn the_flag_on_watcher_never_reads_the_app_to_choose() {
    let source = include_str!("../../main.rs");
    let start = source
        .find("fn run_active_branch_watcher(")
        .unwrap_or_else(|| panic!("no watcher"));
    let body = &source[start..];
    let body = &body[..body.find("\n}\n").unwrap_or(body.len())];
    let flag_on = body
        .find("let desired = follower.next(")
        .unwrap_or_else(|| panic!("the flag-on watcher does not take the follower's target"));
    let choice = &body[flag_on..];
    let choice = &choice[..choice.find(';').unwrap_or(choice.len())];
    for app in ["lock(", "desired_local_branch_watch", "watcher_app"] {
        assert!(!choice.contains(app), "the flag-on target reads {app}");
    }
    assert!(
        body.contains("follower.on_change(&watcher_service)"),
        "a file change does not refresh the content"
    );
}
