//! `pr(tab)` and `pr_status` on the flag path: a stack lookup lands in the
//! PR section and moves `pr(tab)`, a stored status moves `pr_status` at the
//! owner's next publish, and the status loop picks its PR with the App
//! locked.

use std::sync::Arc;

use er_engine::app::StackState;
use er_engine::gh_stack::StackInfo;
use er_protocol::{
    Command, PrStatus, Resource, ResourceChanged, ResourceKey, ResourceVersion, TabPr,
};

use super::diff_command_tests::send;
use super::diff_resource_tests::{fake_git, state_over, with_storage};
use super::while_the_app_is_locked;
use crate::app_owner::SidebarOwner;
use crate::commands::{content_routes, AppState};
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
fn with_stacked_tab(test: impl FnOnce(&AppState, er_engine::TabKey)) {
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

fn pr(state: &AppState, key: er_engine::TabKey) -> (u64, TabPr) {
    let tab = er_service::wire_tab_key(key);
    let fetched = crate::resources::fetch(&state.desktop, ResourceKey::Pr { tab })
        .unwrap_or_else(|e| panic!("{e:?}"));
    match fetched.value {
        Resource::Pr(pr) => (fetched.version.0, *pr),
        other => panic!("not a PR: {other:?}"),
    }
}

/// A stack refresh sent with the App locked lands in the PR section: the
/// `Ack` names `pr(tab)` at a version above the one before, and `pr(tab)`
/// at that version carries the stack as the snapshot's builder shows it.
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

        let info = er_engine::gh_stack::load_from_with(
            &er_engine::github::FakeGitHub::default().with_stack_view_json(Ok(stack_json())),
            &er_engine::gh_stack::StackSource::Checkout(String::new()),
        );
        assert!(matches!(info, StackInfo::Stack(_)), "{info:?}");
        let state = StackState {
            info: Some(info),
            ..StackState::default()
        };
        let mut built = serde_json::to_value(crate::snapshot::snapshot_stack_state(true, &state))
            .unwrap_or_else(|e| panic!("{e}"));
        if let Some(fields) = built.as_object_mut() {
            fields.remove("likely_stacked");
        }
        let wire = serde_json::to_value(&stack).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(wire, built, "pr(tab)'s stack differs from the snapshot's");
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
        let fetched = crate::resources::fetch(&state.desktop, wire.clone())
            .unwrap_or_else(|e| panic!("{e:?}"));
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
/// and the PR cache while the App is locked. Read from the App, a held lock
/// would skip the tick.
#[test]
fn the_status_loop_picks_the_active_pr_with_the_app_locked() {
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
        let picked = while_the_app_is_locked(state, "the status pick", |state| {
            crate::status_loop::pick(state)
        });
        assert_eq!(picked, Some(("acme".into(), "widgets".into(), 7)));
    });
}
