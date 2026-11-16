//! Every view of one tab names the same workspace actor: the App's tab, the
//! descriptor it saves as, the descriptor an open sends, and the content the
//! actor loads for it.

use std::sync::Arc;

use er_engine::app::TabState;
use er_engine::tab_identity::workspace_key;
use er_engine::tabs_record::{TabDescriptor, TabKind};
use er_service::{Command, OpenTarget};

use super::{review_flow_test_state, while_the_app_is_locked, ReviewFlowTestDir};
use crate::commands::{content_routes, service_open_remote_pr, AppState};
use crate::service::ServiceState;
use crate::tabs::descriptor_from_tab;
use crate::view_state::DesktopViewState;

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
            .with_diff_raw(Ok(diff()))
            .with_diff_against_branch(Ok(diff()))
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

fn state(dir: &ReviewFlowTestDir) -> AppState {
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

/// An App tab shaped the way its constructor leaves it.
fn app_tab(root: &str, shape: impl FnOnce(&mut TabState)) -> TabState {
    let mut tab = TabState::new_for_test(vec![]);
    tab.repo_root = root.to_string();
    shape(&mut tab);
    tab
}

fn remote_tab(tab: &mut TabState) {
    tab.remote_repo = Some(REMOTE.to_string());
    tab.pr_number = Some(7);
    tab.local_branch_view = None;
}

/// Opens `sent` as the open routes do, and answers the key the content's
/// tab and context name, after checking it sits on the actor of that key.
fn loaded_key(state: &AppState, sent: TabDescriptor) -> String {
    let opened = while_the_app_is_locked(state, "open", move |state| {
        content_routes::open(state, sent).map(drop)
    });
    assert!(opened.is_ok(), "{opened:?}");
    active_content_key(state)
}

/// The key the active tab's content names in its tab and its context, after
/// checking the content sits on the actor of that key.
fn active_content_key(state: &AppState) -> String {
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

/// Each kind of tab: the App's tab, its saved descriptor, the open's
/// descriptor and the loaded content agree on one key, and a remote PR keys
/// on its remote whether or not a project tracks it.
#[test]
fn every_view_of_a_tab_names_one_workspace() {
    with_storage(|dir| {
        let state = state(dir);
        let root = dir.path().join("repo").to_string_lossy().into_owned();
        let worktree = dir.path().join("worktree");
        std::fs::create_dir_all(&worktree).expect("worktree dir");
        let worktree = worktree.to_string_lossy().into_owned();
        let cwd = std::env::current_dir()
            .expect("cwd")
            .to_string_lossy()
            .into_owned();
        let remote_key = workspace_key(None, REMOTE);

        let mut branch = content_routes::working_tab(root.clone());
        branch.kind = TabKind::LocalBranch;
        branch.branch = Some("feature".to_string());
        let local_pr = content_routes::pr_tab(&OpenTarget::PrReview {
            project_id: "widgets".to_string(),
            remote: REMOTE.to_string(),
            pr_number: 7,
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

        let rows: Vec<(&str, TabState, TabDescriptor, &str)> = vec![
            (
                "working",
                app_tab(&root, |_| {}),
                content_routes::working_tab(root.clone()),
                &root,
            ),
            (
                "local branch",
                app_tab(&root, |tab| {
                    tab.local_branch_view = Some("feature".to_string());
                }),
                branch,
                &root,
            ),
            (
                "worktree",
                app_tab(&worktree, |_| {}),
                content_routes::working_tab(worktree.clone()),
                &worktree,
            ),
            (
                "local PR",
                app_tab(&root, |tab| {
                    tab.remote_repo = Some(REMOTE.to_string());
                    tab.pr_number = Some(7);
                    tab.local_branch_view = Some("pr/7".to_string());
                }),
                local_pr,
                &root,
            ),
            (
                "remote PR",
                app_tab(&cwd, remote_tab),
                remote_pr,
                &remote_key,
            ),
        ];
        let view = DesktopViewState::default();
        for (kind, app, sent, expected) in rows {
            let saved = descriptor_from_tab(&app, &view);
            assert_eq!(app.identity().workspace_key(), expected, "{kind}: App tab");
            assert_eq!(saved.workspace_key(), expected, "{kind}: saved descriptor");
            assert_eq!(sent.workspace_key(), expected, "{kind}: open descriptor");
            assert_eq!(loaded_key(&state, sent), expected, "{kind}: content");
            assert_eq!(
                loaded_key(&state, saved),
                expected,
                "{kind}: restored content"
            );
        }

        // A tracked remote still opens as a remote tab, on the remote's actor:
        // the open's descriptor, its loaded content and an App tab on either
        // the cwd or the project's root all name the remote's key.
        track_remote(&root);
        let opened = while_the_app_is_locked(&state, "open_remote_pr", |state| {
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
        for app_root in [&cwd, &root] {
            let app = app_tab(app_root, remote_tab);
            let saved = descriptor_from_tab(&app, &view);
            assert_eq!(
                app.identity().workspace_key(),
                remote_key,
                "tracked, App on {app_root}: App tab"
            );
            assert_eq!(
                saved.workspace_key(),
                remote_key,
                "tracked, App on {app_root}: saved descriptor"
            );
            assert_eq!(
                loaded_key(&state, saved),
                remote_key,
                "tracked, App on {app_root}: restored content"
            );
        }
    });
}

/// A remote-only PR tab, opened through its open route, takes a reviewed
/// write: the write carries the content's context to the remote's actor,
/// whose target was opened with no local root.
#[test]
fn a_remote_pr_tab_toggles_reviewed() {
    with_storage(|dir| {
        let state = state(dir);
        let opened = while_the_app_is_locked(&state, "open_remote_pr", |state| {
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

        let toggled = while_the_app_is_locked(&state, "toggle_reviewed", |state| {
            content_routes::write_reviewed(state, |context| Command::ToggleReviewed {
                context,
                path: "src/a.rs".to_string(),
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
