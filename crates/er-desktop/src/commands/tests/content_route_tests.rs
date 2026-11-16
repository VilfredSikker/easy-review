//! The flag-on content routes, run while this thread holds the App lock: each
//! returns within the harness's bound, and moves only the active tab's
//! content, on that tab's actor.

use std::sync::Arc;

use er_engine::tab_identity::DiffMode;
use er_engine::tabs_record::TabDescriptor;
use er_engine::TabKey;
use er_service::{Command, TabContent};

use super::{review_flow_test_state, while_the_app_is_locked, ReviewFlowTestDir};
use crate::commands::{content_routes, AppState};
use crate::service::{actor_key, ServiceState};

/// Over the lazy-parse size, with more files than a lazy diff parses up
/// front, so every file starts as a stub.
fn lazy_raw() -> String {
    use std::fmt::Write as _;

    let filler = "x".repeat(120);
    let mut raw = String::new();
    for n in 0..600 {
        let _ = write!(
            raw,
            "diff --git a/src/f{n}.rs b/src/f{n}.rs\n--- a/src/f{n}.rs\n+++ b/src/f{n}.rs\n\
             @@ -1,2 +1,2 @@\n-old {filler}\n+new {filler}\n {filler}\n"
        );
    }
    raw
}

fn ports() -> er_engine::ports::Ports {
    let git: Arc<dyn er_git::Git> = Arc::new(
        er_git::FakeGit::default()
            .with_current_branch(Ok("feature".to_string()))
            .with_detect_base_branch(Ok("main".to_string()))
            .with_diff_raw(Ok(lazy_raw()))
            .with_is_merge_in_progress(false)
            .with_remote_url(Ok("https://github.com/acme/widgets.git".to_string())),
    );
    let github: Arc<dyn er_engine::github::GitHub> =
        Arc::new(er_engine::github::FakeGitHub::default());
    er_engine::ports::Ports { git, github }
}

/// Two working tabs on two repos, so on two actors, the second one active.
struct Fixture {
    state: AppState,
    first: (TabKey, TabDescriptor),
    active: (TabKey, TabDescriptor),
}

fn fixture(dir: &ReviewFlowTestDir) -> Fixture {
    let mut state = review_flow_test_state(dir);
    let settings = dir.path().join("settings").join("config.toml");
    state.service = Arc::new(ServiceState::with_settings_path(ports(), settings));
    for repo in ["one", "two"] {
        let root = dir.path().join(repo);
        std::fs::create_dir_all(&root).unwrap_or_else(|e| panic!("repo dir: {e}"));
        let tab = content_routes::working_tab(root.to_string_lossy().into_owned());
        let opened = while_the_app_is_locked(&state, "open", move |state| {
            content_routes::open(state, tab)
        });
        assert!(opened.is_ok(), "{opened:?}");
    }
    let view = state.service.app_registry().tabs_view();
    let entry = |idx: usize| (view.tabs[idx].key, view.tabs[idx].descriptor.clone());
    let (first, active) = (entry(0), entry(1));
    assert_eq!(view.active, Some(active.0));
    Fixture {
        state,
        first,
        active,
    }
}

fn content(state: &AppState, (key, tab): &(TabKey, TabDescriptor)) -> Arc<TabContent> {
    state
        .service
        .content(*key, tab)
        .unwrap_or_else(|| panic!("no content under {key:?}"))
}

fn revision(state: &AppState, tab: &(TabKey, TabDescriptor)) -> u64 {
    state
        .service
        .published_view(&actor_key(&tab.1))
        .map_or(0, |view| view.revision)
}

fn with_storage(test: impl FnOnce(&ReviewFlowTestDir)) {
    let dir = ReviewFlowTestDir::new();
    let storage = dir.path().join("storage");
    er_engine::storage::with_process_test_storage_root(storage, || test(&dir));
}

type Route = fn(&AppState) -> Result<(), String>;

/// Each routed command, as its Tauri command calls it.
const ROUTES: [(&str, Route); 6] = [
    ("refresh_diff", |state| {
        content_routes::refresh(state, false).map(drop)
    }),
    ("set_mode", |state| {
        content_routes::set_mode(state, "staged", DiffMode::Staged, None, None).map(drop)
    }),
    ("select_file", |state| {
        content_routes::select_file(state, 3).map(drop)
    }),
    ("request_file_content", |state| {
        content_routes::request_file_content(state, &[4, 5])
    }),
    ("mark_reviewed", |state| {
        content_routes::write_reviewed(state, |context| Command::MarkReviewed {
            context,
            path: "src/f0.rs".to_string(),
            reviewed: true,
        })
        .map(drop)
    }),
    ("toggle_reviewed", |state| {
        content_routes::write_reviewed(state, |context| Command::ToggleReviewed {
            context,
            path: "src/f1.rs".to_string(),
        })
        .map(drop)
    }),
];

/// The first tab's actor and content must not move while the routes act on
/// the active tab; the active tab's must.
#[test]
fn each_content_route_moves_only_the_active_tab_without_the_app_lock() {
    with_storage(|dir| {
        let Fixture {
            state,
            first,
            active,
        } = fixture(dir);
        let first_before = content(&state, &first);
        let first_revision = revision(&state, &first);

        for (name, route) in ROUTES {
            let before = content(&state, &active);
            let active_revision = revision(&state, &active);
            let result = while_the_app_is_locked(&state, name, route);
            assert!(result.is_ok(), "{name}: {result:?}");
            let after = content(&state, &active);
            assert!(
                revision(&state, &active) > active_revision,
                "{name} did not reach the active tab's actor"
            );
            let moved = after.generation() > before.generation();
            assert!(moved, "{name} moved no section of the active tab");
        }

        let first_after = content(&state, &first);
        assert_eq!(
            revision(&state, &first),
            first_revision,
            "the first actor moved"
        );
        assert!(
            Arc::ptr_eq(&first_before, &first_after),
            "the first tab moved"
        );
        let active_after = content(&state, &active);
        assert_eq!(active_after.tab.mode, DiffMode::Staged);
        assert!(
            !active_after.diff.files[3].hunks.is_empty(),
            "select parsed nothing"
        );
        assert!(!active_after.diff.files[5].hunks.is_empty());
        assert!(active_after.reviewed.marks.contains_key("src/f0.rs"));
        let nav = state.view.lock().map(|view| view.nav(active.0));
        assert_eq!(nav.map(|nav| nav.selected_file).ok(), Some(3));
    });
}

/// Closing a tab drops its content on its actor and leaves the other tab's.
#[test]
fn closing_a_tab_drops_its_content() {
    with_storage(|dir| {
        let Fixture {
            state,
            first,
            active,
        } = fixture(dir);

        let closed = while_the_app_is_locked(&state, "close_tab", |state| {
            content_routes::close(state, 1).map(drop)
        });

        assert!(closed.is_ok(), "{closed:?}");
        assert!(
            state.service.content(active.0, &active.1).is_none(),
            "the closed tab's content stayed"
        );
        assert!(state.service.content(first.0, &first.1).is_some());
    });
}
