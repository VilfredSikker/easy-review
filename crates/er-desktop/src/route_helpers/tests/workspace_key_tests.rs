//! Every view of one tab names the same workspace actor: the descriptor an
//! open sends, and the content the actor loads for it.

use std::sync::Arc;

use er_engine::tab_identity::workspace_key;
use er_engine::tabs_record::{TabDescriptor, TabKind};
use er_service::{Command, OpenTarget};

use super::{on_a_thread, review_flow_test_state, ReviewFlowTestDir};
use crate::resources::content_routes;
use crate::route_helpers::{service_open_remote_pr, DesktopState};
use crate::service::ServiceState;

const REMOTE: &str = "acme/widgets";

fn diff() -> String {
    "diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1 +1 @@\n-a\n+b\n"
        .to_string()
}

fn ports() -> er_engine::ports::Ports {
    let git: Arc<dyn er_git::Git> = Arc::new(
        er_git::FakeGit::default()
            .with_current_branch(Ok("feature".to_string()))
            .with_detect_base_branch(Ok("main".to_string()))
            .with_diff_shortstat((0, 0, 0))
            .with_diff_raw(Ok(diff()))
            .with_diff_against_branch(Ok(diff()))
            .with_diff_checkout_against_base(Ok(diff()))
            .with_is_merge_in_progress(false)
            .with_remote_url(Ok(format!("https://github.com/{REMOTE}.git"))),
    );
    let github: Arc<dyn er_engine::github::GitHub> = Arc::new(
        er_engine::github::FakeGitHub::default()
            .with_pr_state(Ok("OPEN".to_string()))
            .with_pr_diff_remote(Ok(diff()))
            .with_pr_diff(Ok(diff())),
    );
    er_engine::ports::Ports { git, github }
}

fn state(dir: &ReviewFlowTestDir) -> DesktopState {
    let mut state = review_flow_test_state(dir);
    let settings = dir.path().join("settings").join("config.toml");
    state.service = Arc::new(ServiceState::with_settings_path(ports(), settings));
    state
}

fn with_storage(test: impl FnOnce(&ReviewFlowTestDir)) {
    let dir = ReviewFlowTestDir::new();
    let storage = dir.path().join("storage");
    er_engine::storage::with_process_test_storage_root(storage, || test(&dir));
}

/// A project on `root` that tracks the remote, so a remote open has a local
/// project it must not key on.
fn track_remote(root: &str) {
    let record = crate::projects::ProjectRecord {
        id: "widgets".to_string(),
        name: "widgets".to_string(),
        root_path: root.to_string(),
        remote: Some(REMOTE.to_string()),
        ..Default::default()
    };
    let saved = er_engine::projects_pins::update(|file| {
        file.projects.push(record);
        Ok(true)
    });
    assert!(saved.is_ok(), "{saved:?}");
    crate::projects::invalidate_load_cache();
}

/// Opens `sent` as the open routes do, and answers the key the content's
/// tab and context name, after checking it sits on the actor of that key.
fn loaded_key(state: &DesktopState, sent: TabDescriptor) -> String {
    let opened = on_a_thread(state, "open", move |state| {
        content_routes::open(state, sent, false).map(drop)
    });
    assert!(opened.is_ok(), "{opened:?}");
    active_content_key(state)
}

/// The key the active tab's content names in its tab and its context, after
/// checking the content sits on the actor of that key.
fn active_content_key(state: &DesktopState) -> String {
    let view = state.service.app_registry().tabs_view();
    let entry = view.active_entry().expect("the opened tab is active");
    let content = state
        .service
        .content(entry.key, &entry.descriptor)
        .expect("the tab's content loaded");
    let key = content.tab.identity().workspace_key();
    assert_eq!(
        content.context.workspace_key(),
        key,
        "the content's context"
    );
    let on_actor = state.service.published_view(&key);
    assert!(
        on_actor.is_some_and(|view| view.tab(entry.key).is_some()),
        "the content is not on the {key} actor"
    );
    key
}

/// Each kind of tab: the open's descriptor and the loaded content agree on
/// one key, and a remote PR keys
/// on its remote whether or not a project tracks it.
#[test]
fn every_view_of_a_tab_names_one_workspace() {
    with_storage(|dir| {
        let state = state(dir);
        let root = dir.path().join("repo").to_string_lossy().into_owned();
        let worktree = dir.path().join("worktree");
        std::fs::create_dir_all(&worktree).expect("worktree dir");
        let worktree = worktree.to_string_lossy().into_owned();
        let remote_key = workspace_key(None, REMOTE);

        let mut branch = content_routes::working_tab(root.clone());
        branch.kind = TabKind::LocalBranch;
        branch.branch = Some("feature".to_string());
        // Another PR than the remote row's: an open of a PR already open as a
        // local PR selects that tab (A15), and this test is about the keys.
        let local_pr = content_routes::pr_tab(&OpenTarget::PrReview {
            project_id: "widgets".to_string(),
            remote: REMOTE.to_string(),
            pr_number: 8,
            repo_root: root.clone(),
        })
        .expect("local PR tab");
        let remote_pr = content_routes::pr_tab(&OpenTarget::RemotePr {
            owner: "acme".to_string(),
            repo: "widgets".to_string(),
            number: 7,
            repo_root: String::new(),
        })
        .expect("remote PR tab");

        let rows: Vec<(&str, TabDescriptor, &str)> = vec![
            ("working", content_routes::working_tab(root.clone()), &root),
            ("local branch", branch, &root),
            (
                "worktree",
                content_routes::working_tab(worktree.clone()),
                &worktree,
            ),
            ("local PR", local_pr, &root),
            ("remote PR", remote_pr, &remote_key),
        ];
        for (kind, sent, expected) in rows {
            assert_eq!(sent.workspace_key(), expected, "{kind}: open descriptor");
            assert_eq!(loaded_key(&state, sent), expected, "{kind}: content");
        }

        // A tracked remote still opens as a remote tab, on the remote's actor:
        // the open's descriptor and its loaded content name the remote's key.
        track_remote(&root);
        let opened = on_a_thread(&state, "open_remote_pr", |state| {
            service_open_remote_pr(state, "acme".into(), "widgets".into(), 7, None).map(drop)
        });
        assert!(opened.is_ok(), "{opened:?}");
        let tabs = state.service.app_registry().tabs_view();
        let entry = tabs.active_entry().expect("the opened tab is active");
        assert_eq!(
            entry.descriptor.kind,
            TabKind::RemotePr,
            "tracked: open kind"
        );
        assert_eq!(
            entry.descriptor.workspace_key(),
            remote_key,
            "tracked: open descriptor"
        );
        assert_eq!(active_content_key(&state), remote_key, "tracked: content");
    });
}

/// A remote-only PR tab, opened through its open route, takes a reviewed
/// write: the write carries the content's context to the remote's actor,
/// whose target was opened with no local root.
#[test]
fn a_remote_pr_tab_toggles_reviewed() {
    with_storage(|dir| {
        let state = state(dir);
        let opened = on_a_thread(&state, "open_remote_pr", |state| {
            service_open_remote_pr(state, "acme".into(), "widgets".into(), 7, None).map(drop)
        });
        assert!(opened.is_ok(), "{opened:?}");
        let key = workspace_key(None, REMOTE);
        let target = state
            .service
            .published_view(&key)
            .and_then(|view| view.active.and_then(|idx| view.targets.get(idx).cloned()));
        assert_eq!(
            target.map(|target| target.workspace_key()),
            Some(key),
            "the open reached the remote's actor"
        );

        let toggled = on_a_thread(&state, "toggle_reviewed", |state| {
            content_routes::write_keyed(state, super::active_key(state), None, |context| {
                Command::ToggleReviewed {
                    context,
                    path: "src/a.rs".to_string(),
                }
            })
            .map(drop)
        });
        assert!(toggled.is_ok(), "{toggled:?}");
        let view = state.service.app_registry().tabs_view();
        let entry = view.active_entry().expect("the remote tab is active");
        let content = state.service.content(entry.key, &entry.descriptor);
        assert!(
            content.is_some_and(|content| content.reviewed.marks.contains_key("src/a.rs")),
            "the mark did not land"
        );
    });
}
