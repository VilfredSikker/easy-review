//! The content routes: each returns within the harness's bound, and moves
//! only the active tab's content, on that tab's actor.

use std::sync::Arc;

use er_engine::tab_identity::DiffMode;
use er_engine::tabs_record::TabDescriptor;
use er_engine::TabKey;
use er_service::{Command, TabContent};

use super::{active_key, on_a_thread, review_flow_test_state, ReviewFlowTestDir};
use crate::resources::content_routes;
use crate::route_helpers::DesktopState;
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
            .with_diff_shortstat((0, 0, 0))
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
    state: DesktopState,
    first: (TabKey, TabDescriptor),
    active: (TabKey, TabDescriptor),
}

/// A desktop state whose service runs on `ports`.
fn state_over(dir: &ReviewFlowTestDir, ports: er_engine::ports::Ports) -> DesktopState {
    let mut state = review_flow_test_state(dir);
    let settings = dir.path().join("settings").join("config.toml");
    state.service = Arc::new(ServiceState::with_settings_path(ports, settings));
    state
}

fn fixture(dir: &ReviewFlowTestDir) -> Fixture {
    let state = state_over(dir, ports());
    for repo in ["one", "two"] {
        let root = dir.path().join(repo);
        std::fs::create_dir_all(&root).unwrap_or_else(|e| panic!("repo dir: {e}"));
        let tab = content_routes::working_tab(root.to_string_lossy().into_owned());
        let opened = on_a_thread(&state, "open", move |state| {
            content_routes::open(state, tab, false)
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

fn content(state: &DesktopState, (key, tab): &(TabKey, TabDescriptor)) -> Arc<TabContent> {
    state
        .service
        .content(*key, tab)
        .unwrap_or_else(|| panic!("no content under {key:?}"))
}

fn revision(state: &DesktopState, tab: &(TabKey, TabDescriptor)) -> u64 {
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

type Route = fn(&DesktopState) -> Result<(), String>;

/// Each routed command on the active tab, as its `er_send` mapping calls it.
const ROUTES: [(&str, Route); 5] = [
    ("refresh_diff", |state| {
        content_routes::refresh_tab(state, active_key(state), false).map(drop)
    }),
    ("set_mode", |state| {
        let key = active_key(state);
        content_routes::set_tab_mode(state, key, "staged", DiffMode::Staged, None)
    }),
    ("mark_reviewed", |state| {
        content_routes::write_keyed(state, active_key(state), None, |context| {
            Command::MarkReviewed {
                context,
                path: "src/f0.rs".to_string(),
                reviewed: true,
            }
        })
    }),
    ("toggle_reviewed", |state| {
        content_routes::write_keyed(state, active_key(state), None, |context| {
            Command::ToggleReviewed {
                context,
                path: "src/f1.rs".to_string(),
            }
        })
    }),
    ("unmark_reviewed", |state| {
        content_routes::write_keyed(state, active_key(state), None, |context| {
            Command::MarkReviewed {
                context,
                path: "src/f1.rs".to_string(),
                reviewed: false,
            }
        })
    }),
];

/// The first tab's actor and content must not move while the routes act on
/// the active tab; the active tab's must.
#[test]
fn each_content_route_moves_only_the_active_tab() {
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
            let result = on_a_thread(&state, name, route);
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
        assert!(active_after.reviewed.marks.contains_key("src/f0.rs"));
        let unmarked = !active_after.reviewed.marks.contains_key("src/f1.rs");
        assert!(unmarked, "the toggled mark stayed");
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

        let key = active.0;
        let closed = on_a_thread(&state, "close_tab", move |state| {
            content_routes::close_key(state, key).map(drop)
        });

        assert!(closed.is_ok(), "{closed:?}");
        assert!(
            state.service.content(active.0, &active.1).is_none(),
            "the closed tab's content stayed"
        );
        assert!(state.service.content(first.0, &first.1).is_some());
    });
}

/// A review an outside writer leaves in the active tab's bucket reaches its
/// content through the backend tick's reload, which takes no App lock.
#[test]
fn the_backend_tick_brings_an_outside_write_into_the_active_content() {
    with_storage(|dir| {
        let Fixture { state, active, .. } = fixture(dir);
        let loaded = content(&state, &active);
        er_engine::test_support::write_sidecar_json::<er_engine::ai::ErReview>(
            std::path::Path::new(&loaded.context.er_dir),
            serde_json::json!({
                "version": 1,
                "diff_hash": loaded.context.branch_diff_hash,
                "files": { "src/f0.rs": { "risk": "low", "findings": [{
                    "id": "f-1", "severity": "low", "title": "A defect",
                    "description": "desc", "hunk_index": 0, "line_start": 1
                }] } }
            }),
        );

        let sent = on_a_thread(&state, "backend tick", |state| {
            crate::resources::tick::reload_loaded_tabs(state)
        });

        assert!(sent > 0, "the tick sent nothing");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while content(&state, &active).ai.state.total_findings() == 0 {
            assert!(
                std::time::Instant::now() < deadline,
                "the write never arrived"
            );
            std::thread::yield_now();
        }
    });
}

/// Every view loads on the active content, History, Conflicts and Hidden
/// included.
#[test]
fn every_mode_reaches_the_active_content() {
    with_storage(|dir| {
        let Fixture { state, active, .. } = fixture(dir);
        let views = [
            ("branch", DiffMode::Branch),
            ("unstaged", DiffMode::Unstaged),
            ("staged", DiffMode::Staged),
            ("history", DiffMode::History),
            ("conflicts", DiffMode::Conflicts),
            ("hidden", DiffMode::Hidden),
            ("pr", DiffMode::PrDiff),
            ("tour", DiffMode::Tour),
        ];
        for (name, mode) in views {
            assert_eq!(content_routes::routed_mode(name), Some(mode), "{name}");
        }
        for (name, mode) in [
            ("hidden", DiffMode::Hidden),
            ("conflicts", DiffMode::Conflicts),
            ("history", DiffMode::History),
            ("tour", DiffMode::Tour),
        ] {
            let switched = on_a_thread(&state, name, move |state| {
                content_routes::set_tab_mode(state, active_key(state), name, mode, None)
            });
            assert!(switched.is_ok(), "{name}: {switched:?}");
            assert_eq!(content(&state, &active).tab.mode, mode);
        }
        let toured = content(&state, &active);
        assert!(!toured.diff.files.is_empty(), "the tour loaded no diff");
    });
}

/// A plain tab's git, which answers every ref and accepts every fetch.
fn ports_with_refs() -> (Arc<er_git::FakeGit>, er_engine::ports::Ports) {
    let fake = Arc::new(
        er_git::FakeGit::default()
            .with_current_branch(Ok("feature".to_string()))
            .with_detect_base_branch(Ok("main".to_string()))
            .with_diff_shortstat((0, 0, 0))
            .with_diff_raw(Ok(lazy_raw()))
            .with_is_merge_in_progress(false)
            .with_remote_url(Ok("https://github.com/acme/widgets.git".to_string()))
            .with_rev_parse_verify(Ok(Some("base-oid".to_string())))
            .with_fetch(Ok(())),
    );
    let git: Arc<dyn er_git::Git> = fake.clone();
    let github: Arc<dyn er_engine::github::GitHub> =
        Arc::new(er_engine::github::FakeGitHub::default());
    (fake, er_engine::ports::Ports { git, github })
}

fn open_one(state: &DesktopState, tab: TabDescriptor) -> (TabKey, TabDescriptor) {
    let opened = on_a_thread(state, "open", move |state| {
        content_routes::open(state, tab, false)
    });
    assert!(opened.is_ok(), "{opened:?}");
    let view = state.service.app_registry().tabs_view();
    let entry = view
        .active_entry()
        .unwrap_or_else(|| panic!("no active tab"));
    (entry.key, entry.descriptor.clone())
}

/// A sync of a branch view fetches `origin/<base>` again, then records that
/// base as the probed one, so the branch pill clears.
#[test]
fn a_branch_sync_fetches_the_base_and_lines_the_probe_up() {
    with_storage(|dir| {
        let (git, ports) = ports_with_refs();
        let mut state = state_over(dir, ports.clone());
        state.ports = ports;
        let root = dir.path().join("one");
        std::fs::create_dir_all(&root).unwrap_or_else(|e| panic!("repo dir: {e}"));
        let root = root.to_string_lossy().into_owned();
        let tab = open_one(&state, content_routes::working_tab(root.clone()));

        let synced = on_a_thread(&state, "sync", |state| {
            content_routes::sync_tab(state, active_key(state))
        });

        let synced = synced.unwrap_or_else(|e| panic!("sync: {e}"));
        let refspec = "+refs/heads/main:refs/remotes/origin/main";
        let fetched = git.calls().iter().any(|call| call.contains(refspec));
        assert!(fetched, "the sync did not fetch the base");
        assert_eq!(content(&state, &tab).tab.base_branch, "origin/main");
        let probed = state.branch_base_remote_oid.lock().ok().and_then(|cache| {
            let key = format!("{root}\u{0}main");
            cache.get(&key).cloned()
        });
        assert_eq!(probed.as_deref(), Some("base-oid"));
        assert_eq!(synced.repo_root, root);
        assert_eq!(synced.status_key, None, "no PR is listed for the branch");
    });
}

/// A sync of a PR tab adopts the PR list's head, which the stale pill
/// compares, and names that PR for the status refresh.
#[test]
fn a_pr_sync_adopts_the_listed_head_and_names_its_status() {
    with_storage(|dir| {
        let github = er_engine::github::FakeGitHub::default()
            .with_pr_diff_remote(Ok(lazy_raw()))
            .with_pr_head_oid_remote(Ok("h-fetched".to_string()));
        let ports = er_engine::ports::Ports {
            git: ports().git,
            github: Arc::new(github),
        };
        let state = state_over(dir, ports);
        let target = er_service::OpenTarget::RemotePr {
            owner: "acme".to_string(),
            repo: "widgets".to_string(),
            number: 7,
            repo_root: String::new(),
        };
        let tab = content_routes::pr_tab(&target).unwrap_or_else(|e| panic!("pr tab: {e}"));
        let tab = open_one(&state, tab);
        let mut listed = crate::page_view::minimal_pr_info(7, "t");
        listed.head_oid = "h-listed".to_string();
        if let Ok(mut cache) = state.pr_cache.lock() {
            cache.insert("acme/widgets".to_string(), vec![listed]);
        }

        let synced = on_a_thread(&state, "sync", |state| {
            content_routes::sync_tab(state, active_key(state))
        });

        let synced = synced.unwrap_or_else(|e| panic!("sync: {e}"));
        let head = content(&state, &tab).pr.head_oid.clone();
        assert_eq!(
            head.as_deref(),
            Some("h-listed"),
            "the listed head was not adopted"
        );
        let expected = ("acme".to_string(), "widgets".to_string(), 7);
        assert_eq!(synced.status_key, Some(expected));
    });
}
