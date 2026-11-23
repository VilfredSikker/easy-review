//! `pr(tab)` and `pr_status` on the flag path: a stack lookup lands in the
//! PR section and moves `pr(tab)`, a stored status moves `pr_status` at the
//! owner's next publish, and the status loop picks its PR from the registry.

use std::sync::Arc;

use er_protocol::{
    Command, PrStatus, Resource, ResourceChanged, ResourceKey, ResourceVersion, TabPr,
};

use super::diff_command_tests::send;
use super::diff_resource_tests::{fake_git, state_over, with_storage};
use super::on_a_thread;
use crate::app_owner::SidebarOwner;
use crate::commands::{content_routes, DesktopState};
use crate::snapshot::{GithubStatusSnapshot, PrInfo};

fn stack_json() -> String {
    serde_json::json!({
        "trunk": "main",
        "currentBranch": "feature",
        "branches": [
            { "name": "feature-2", "pr": { "number": 8, "url": "https://github.com/acme/widgets/pull/8", "state": "OPEN" } },
            { "name": "feature", "isCurrent": true, "needsRebase": true, "pr": { "number": 7, "url": "https://github.com/acme/widgets/pull/7", "state": "OPEN" } }
        ]
    })
    .to_string()
}

/// A working tab on a repo whose GitHub answers `gh stack view` with a
/// two-layer stack, opened as the active tab.
fn with_stacked_tab(test: impl FnOnce(&DesktopState, er_engine::TabKey)) {
    with_storage(|dir| {
        let git: Arc<dyn er_git::Git> = Arc::new(fake_git());
        let github =
            er_engine::github::FakeGitHub::default().with_stack_view_json(Ok(stack_json()));
        let github: Arc<dyn er_engine::github::GitHub> = Arc::new(github);
        let state = state_over(dir, er_engine::ports::Ports { git, github });
        let root = dir.path().join("repo");
        std::fs::create_dir_all(&root).unwrap_or_else(|e| panic!("repo dir: {e}"));
        let tab = content_routes::working_tab(root.to_string_lossy().into_owned());
        let opened = content_routes::open(&state, tab, false);
        assert!(opened.is_ok(), "{opened:?}");
        let key = state.service.app_registry().tabs_view().active;
        test(&state, key.unwrap_or_else(|| panic!("no active tab")));
    });
}

fn pr(state: &DesktopState, key: er_engine::TabKey) -> (u64, TabPr) {
    let tab = er_service::wire_tab_key(key);
    let fetched =
        crate::resources::fetch(state, ResourceKey::Pr { tab }).unwrap_or_else(|e| panic!("{e:?}"));
    match fetched.value {
        Resource::Pr(pr) => (fetched.version.0, *pr),
        other => panic!("not a PR: {other:?}"),
    }
}

/// A stack refresh lands in the PR section: the `Ack` names `pr(tab)` at a
/// version above the one before, and `pr(tab)` at that version carries the
/// stack.
#[test]
fn a_stack_refresh_moves_pr_tab_and_carries_the_stack() {
    with_stacked_tab(|state, key| {
        let (before, held) = pr(state, key);
        assert_eq!(
            held.stack,
            Some(er_protocol::Stack::default()),
            "a checkout tab offers a lookup"
        );
        let tab = er_service::wire_tab_key(key);
        let ack = send(state, "refresh stack", Command::RefreshStack { tab })
            .unwrap_or_else(|e| panic!("{e:?}"));
        let (after, held) = pr(state, key);
        assert!(after > before, "the PR section stayed at {before}");
        assert_eq!(
            ack.changed,
            vec![ResourceChanged {
                key: ResourceKey::Pr { tab },
                version: ResourceVersion(after),
            }]
        );
        let stack = held.stack.unwrap_or_else(|| panic!("no stack"));
        let layers: Vec<&str> = stack.layers.iter().map(|l| l.branch.as_str()).collect();
        assert_eq!(layers, ["feature", "feature-2"]);
        assert_eq!((stack.position, stack.size), (Some(1), 2));
    });
}

fn status(title: &str) -> GithubStatusSnapshot {
    serde_json::from_value(serde_json::json!({
        "owner": "acme", "repo": "widgets", "number": 7, "url": "https://github.com/acme/widgets/pull/7",
        "state": "OPEN", "is_draft": false, "title": title, "body": "", "author": "Octo",
        "head_ref": "feature", "base_ref": "main", "review_decision": "APPROVED", "mergeable": "MERGEABLE",
        "labels": ["l"],
        "checks": [{ "name": "ci", "status": "COMPLETED", "conclusion": "SUCCESS", "url": null, "duration_secs": 4 }],
        "comments_count": 2, "reviews_count": 1,
        "recent_comments": [{ "author": "a", "body": "b", "created_at": "c", "url": "d" }],
        "recent_reviews": [{ "author": "a", "state": "APPROVED", "body": "", "submitted_at": "s" }],
        "last_updated": "1", "head_oid": "abc",
        "repo_merge": { "merge_commit_allowed": true, "squash_merge_allowed": true, "rebase_merge_allowed": false, "delete_branch_on_merge": false, "viewer_permission": "WRITE" },
        "reviewers": [{ "reviewer": "r", "is_team": false, "requested": true, "state": null }]
    }))
    .unwrap_or_else(|e| panic!("{e}"))
}

/// A status a fetch stores reaches `pr_status` at the owner's next publish,
/// which the store's desktop revision wakes the tick for, at a new version
/// and in the snapshot's shape; the same status stored again moves nothing.
#[test]
fn a_stored_status_moves_pr_status_at_the_next_publish() {
    with_storage(|dir| {
        let state = state_over(dir, super::diff_resource_tests::fake_ports());
        state
            .app_owner
            .read_status_from(Arc::clone(&state.gh_status_cache));
        *state.gh_user.lock().unwrap_or_else(|e| e.into_inner()) = Some("octo".into());
        state.app_owner.publish();
        assert_eq!(
            state.app_owner.pr_status("acme/widgets", 7),
            (0, PrStatus::default())
        );

        let key = ("acme".to_string(), "widgets".to_string(), 7);
        let stored = status("Widgets");
        let ticket = crate::gh_status_cache::begin_fetch();
        assert!(crate::gh_status_cache::store_fetched(
            &state.gh_status_cache,
            key.clone(),
            ticket,
            stored.clone()
        ));
        state.app_owner.publish();
        let wire = ResourceKey::PrStatus {
            remote: "acme/widgets".into(),
            number: 7,
        };
        let fetched =
            crate::resources::fetch(&state, wire.clone()).unwrap_or_else(|e| panic!("{e:?}"));
        assert!(fetched.version.0 > 0, "the stored status moved no version");
        let Resource::PrStatus(held) = fetched.value else {
            panic!("not a status: {:?}", fetched.value);
        };
        let held = held.status.unwrap_or_else(|| panic!("no status"));
        let mut snapshot = serde_json::to_value(&stored).unwrap_or_else(|e| panic!("{e}"));
        snapshot["is_authored_by_me"] = serde_json::Value::Bool(true);
        assert_eq!(
            serde_json::to_value(&held).unwrap_or_else(|e| panic!("{e}")),
            snapshot
        );

        let again = crate::gh_status_cache::begin_fetch();
        assert!(crate::gh_status_cache::store_fetched(
            &state.gh_status_cache,
            key,
            again,
            stored
        ));
        state.app_owner.publish();
        assert_eq!(
            state.app_owner.pr_status("acme/widgets", 7).0,
            fetched.version.0
        );
    });
}

fn pr_info(number: u64, head_ref: &str, state: &str) -> PrInfo {
    serde_json::from_value(serde_json::json!({
        "number": number, "title": "", "head_ref": head_ref, "state": state,
        "is_draft": false, "author": ""
    }))
    .unwrap_or_else(|e| panic!("{e}"))
}

/// The status loop picks the active tab's PR from the registry, its content
/// and the PR cache.
#[test]
fn the_status_loop_picks_the_active_pr() {
    with_stacked_tab(|state, _| {
        state
            .pr_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                "acme/widgets".into(),
                vec![
                    pr_info(6, "feature", "CLOSED"),
                    pr_info(7, "feature", "OPEN"),
                ],
            );
        let picked = on_a_thread(state, "the status pick", |state| {
            crate::status_loop::pick(state)
        });
        assert_eq!(picked, Some(("acme".into(), "widgets".into(), 7)));
    });
}

fn status_of(state: &DesktopState, remote: &str, number: u64) -> (u64, PrStatus) {
    let key = ResourceKey::PrStatus {
        remote: remote.into(),
        number,
    };
    let fetched = crate::resources::fetch(state, key).unwrap_or_else(|e| panic!("{e:?}"));
    match fetched.value {
        Resource::PrStatus(status) => (fetched.version.0, *status),
        other => panic!("not a status: {other:?}"),
    }
}

/// A status fetch shows on its `pr_status` while it runs, moves the version
/// when it starts and again when it ends, and the relay watches the key
/// before any status is stored. The fetch's begin and end are the ones the
/// status loop and the status kick use, so no `gh` runs here: a read while
/// the fetch runs starts none.
#[test]
fn a_status_fetch_shows_on_its_pr_status_while_it_runs() {
    with_storage(|dir| {
        let state = state_over(dir, er_engine::ports::Ports::for_test());
        let key = ("acme".to_string(), "widgets".to_string(), 9);
        let wire = ResourceKey::PrStatus {
            remote: "acme/widgets".into(),
            number: 9,
        };
        let version = || {
            crate::resources::version(&state, wire.clone())
                .unwrap_or_else(|e| panic!("{e:?}"))
                .0
        };
        let before = version();
        let fetch = state.begin_status_fetch(&key);
        assert!(fetch.is_some());
        assert!(
            state.begin_status_fetch(&key).is_none(),
            "a second fetch began"
        );
        let (started, status) = status_of(&state, "acme/widgets", 9);
        assert!(status.fetching, "{status:?}");
        assert!(started > before, "{started} after {before}");
        let watched = crate::resources::watched(&state);
        let watched = watched.get(&crate::resources::Watched::Sidebar(wire.clone()));
        assert_eq!(watched, Some(&started));
        drop(fetch);
        // A read of a status none stored starts a fetch, so the end is read
        // from its version and the in-flight set.
        let ended = version();
        assert!(ended > started, "{ended} after {started}");
        assert!(state.gh_status_in_flight.lock().unwrap().is_empty());
    });
}

/// A first status fetch that stores nothing (no network, a 404) still
/// pushes its end, so the branch card's spinner stops.
#[test]
fn the_end_of_a_first_fetch_that_stored_nothing_is_pushed() {
    with_storage(|dir| {
        let state = state_over(dir, er_engine::ports::Ports::for_test());
        let key = ("acme".to_string(), "widgets".to_string(), 9);
        let wire = crate::resources::Watched::Sidebar(ResourceKey::PrStatus {
            remote: "acme/widgets".into(),
            number: 9,
        });
        let mut last = crate::resources::watched(&state);
        let fetch = state.begin_status_fetch(&key);
        assert!(fetch.is_some());
        let began = crate::resources::relay::moved(&mut last, crate::resources::watched(&state));
        assert!(
            began.iter().any(|(k, _)| *k == wire),
            "begin not pushed: {began:?}"
        );
        drop(fetch);
        let ended = crate::resources::relay::moved(&mut last, crate::resources::watched(&state));
        let pushed = ended.iter().find(|(k, _)| *k == wire);
        assert!(pushed.is_some(), "end not pushed: {ended:?}");
        // What the page fetches for the pushed version says the fetch ended.
        let fetching = state.gh_status_in_flight.lock().unwrap().contains(&key);
        assert!(!fetching);
    });
}

/// A refresh or fetch that panics still ends: the guard's drop clears the
/// flag, so the next refresh is not refused forever.
#[test]
fn a_guard_dropped_in_a_panic_ends_its_refresh_and_fetch() {
    with_storage(|dir| {
        let state = state_over(dir, er_engine::ports::Ports::for_test());
        let key = ("acme".to_string(), "widgets".to_string(), 9);
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _refresh = state.begin_pr_list_refresh();
            let _fetch = state.begin_status_fetch(&key);
            panic!("the fetch failed");
        }));
        assert!(panicked.is_err());
        assert!(!jobs_of(&state).1.pr_list_refresh);
        assert!(!state.loading.lock().unwrap().pr_list);
        assert!(state.begin_pr_list_refresh().is_some());
        assert!(state.begin_status_fetch(&key).is_some());
    });
}

/// Every source file under `src`, outside the tests, with its path.
fn sources() -> Vec<(String, String)> {
    fn walk(dir: &std::path::Path, out: &mut Vec<(String, String)>) {
        let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{dir:?}: {e}"));
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().is_some_and(|name| name != "tests") {
                    walk(&path, out);
                }
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{e}"));
                out.push((path.to_string_lossy().into_owned(), text));
            }
        }
    }
    let mut out = Vec::new();
    walk(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut out,
    );
    out
}

/// Every PR-list refresh and status fetch starts through its begin helper,
/// whose guard ends it: nothing outside `state.rs` writes the flags or the
/// in-flight set, and each kick, loop and fetch site names the helper.
/// Reads the source, so no `gh` runs.
#[test]
fn every_refresh_and_status_fetch_goes_through_its_guard() {
    let sources = sources();
    for (path, text) in sources
        .iter()
        .filter(|(path, _)| !path.ends_with("/state.rs"))
    {
        for (i, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or_default();
            for flag in [".pr_list", ".gh_status"] {
                for (at, _) in code.match_indices(flag) {
                    let rest = code.get(at + flag.len()..).unwrap_or_default().trim_start();
                    let assigns = rest.starts_with('=') && !rest.starts_with("==");
                    assert!(!assigns, "{path}:{} sets {flag} itself", i + 1);
                }
                let swaps = code.contains(&format!("&mut flags{flag}"));
                assert!(!swaps, "{path}:{} sets {flag} itself", i + 1);
            }
            // The set may be named only where a state is built.
            let named = code.contains("gh_status_in_flight");
            let builds =
                code.contains("gh_status_in_flight:") || code.contains("let gh_status_in_flight");
            let reads = path.ends_with("/resources/pr.rs");
            assert!(
                !named || builds || reads,
                "{path}:{} reaches the in-flight set",
                i + 1
            );
        }
        if path.ends_with("/resources/pr.rs") {
            assert!(
                !text.contains(".insert(") && !text.contains(".remove("),
                "{path} writes a set"
            );
        }
    }
    // The body of `fn name` in `text`, up to the next item at column 0.
    let body = |name: &str| -> String {
        let pattern = format!("fn {name}(");
        let (_, text) = sources
            .iter()
            .find(|(_, text)| text.contains(&pattern))
            .unwrap_or_else(|| panic!("no fn {name}"));
        let rest = text.split_once(&pattern).map_or("", |(_, rest)| rest);
        rest.split("\n}\n").next().unwrap_or_default().to_string()
    };
    for (name, begin) in [
        ("kick_pr_list_refresh", "begin_pr_list_refresh()"),
        ("kick_github_status_refresh", "begin_status_fetch("),
        ("refresh_status_now", "begin_status_fetch("),
    ] {
        assert!(body(name).contains(begin), "{name} skips {begin}");
    }
    // Each fetch call outside its definition sits under a begin whose guard
    // is kept.
    for (path, text) in &sources {
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            let begin = if line.contains(".refresh_prs(") || line.contains(".refresh_remote_prs(") {
                "begin_pr_list_refresh()"
            } else if line.contains("fetch_and_store_github_status(") && !line.contains("fn ") {
                "begin_status_fetch("
            } else {
                continue;
            };
            let above = &lines[i.saturating_sub(30)..i];
            let kept = above
                .iter()
                .any(|l| l.contains(begin) && !l.contains("let _ =") && !l.contains("if "));
            assert!(kept, "{path}:{} fetches without {begin}", i + 1);
        }
    }
}

fn jobs_of(state: &DesktopState) -> (u64, er_protocol::Jobs) {
    let fetched =
        crate::resources::fetch(state, ResourceKey::Jobs).unwrap_or_else(|e| panic!("{e:?}"));
    match fetched.value {
        Resource::Jobs(jobs) => (fetched.version.0, jobs),
        other => panic!("not jobs: {other:?}"),
    }
}

/// A PR-list refresh shows in `jobs` while it runs, and moves `jobs` when it
/// starts and again when it ends. The begin and end are the ones the refresh
/// kick and the startup sweep use, so no `gh` runs here.
#[test]
fn a_pr_list_refresh_shows_in_jobs_while_it_runs() {
    with_storage(|dir| {
        let state = state_over(dir, er_engine::ports::Ports::for_test());
        let (before, idle) = jobs_of(&state);
        assert!(!idle.pr_list_refresh);
        let refresh = state.begin_pr_list_refresh();
        assert!(refresh.is_some());
        assert!(
            state.begin_pr_list_refresh().is_none(),
            "a second refresh began"
        );
        let (started, running) = jobs_of(&state);
        assert!(running.pr_list_refresh);
        assert!(started > before, "{started} after {before}");
        drop(refresh);
        let (ended, done) = jobs_of(&state);
        assert!(!done.pr_list_refresh);
        assert!(ended > started, "{ended} after {started}");
    });
}
