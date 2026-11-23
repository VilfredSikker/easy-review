//! The project syncs on `er_send`: `SyncPr` syncs every open tab of the PR
//! and warms its open cache, `SyncBranch` every tab that shows the branch,
//! and neither moves another tab's diff.

use std::sync::Arc;
use std::time::{Duration, Instant};

use er_engine::pr_open_entry::PrOpenCacheKey;
use er_engine::tabs_record::{TabDescriptor, TabKind};
use er_engine::TabKey;
use er_protocol::{Ack, Command, ResourceKey};

use super::diff_resource_tests::{fake_git, raw_diff, state_over, with_storage};
use super::ReviewFlowTestDir;
use crate::projects::{self, ProjectRecord, ProjectsFile};
use crate::resources::content_routes;
use crate::resources::send::{apply, Effects};
use crate::route_helpers::DesktopState;

const PROJECT: &str = "widgets";
const REMOTE: &str = "acme/widgets";
const WAIT: Duration = Duration::from_secs(10);

/// The diff fixture's git, which also diffs a branch view, and a GitHub that
/// answers the open cache's PR diff.
fn ports() -> er_engine::ports::Ports {
    let git: Arc<dyn er_git::Git> = Arc::new(fake_git().with_diff_against_branch(Ok(raw_diff())));
    let github: Arc<dyn er_engine::github::GitHub> = Arc::new(
        er_engine::github::FakeGitHub::default()
            .with_pr_diff(Ok(raw_diff()))
            .with_pr_commits(Ok(Vec::new())),
    );
    er_engine::ports::Ports { git, github }
}

/// A state with the project saved and PR 7 in its PR list.
fn project(dir: &ReviewFlowTestDir) -> (DesktopState, String) {
    let state = state_over(dir, ports());
    let root = dir.path().join("repo");
    std::fs::create_dir_all(&root).unwrap_or_else(|e| panic!("repo dir: {e}"));
    let root = root.to_string_lossy().into_owned();
    let record = ProjectRecord {
        id: PROJECT.into(),
        name: PROJECT.into(),
        root_path: root.clone(),
        remote: Some(REMOTE.into()),
        ..Default::default()
    };
    let file = ProjectsFile {
        projects: vec![record],
        active_id: None,
    };
    let key = er_engine::store_registry::app_key::<ProjectsFile>()
        .unwrap_or_else(|e| panic!("projects key: {e}"));
    let saved = er_engine::store_registry::store().write_durable(&key, &file);
    assert!(saved.is_ok(), "{saved:?}");
    projects::invalidate_load_cache();
    let pr = serde_json::json!({
        "number": 7, "title": "PR 7", "head_ref": "topic", "base_ref": "main",
        "head_oid": "7".repeat(40), "state": "OPEN", "is_draft": false, "author": "me",
    });
    let pr = serde_json::from_value(pr).unwrap_or_else(|e| panic!("pr: {e}"));
    if let Ok(mut cache) = state.pr_cache.lock() {
        cache.insert(REMOTE.into(), vec![pr]);
    }
    (state, root)
}

fn open(state: &DesktopState, tab: TabDescriptor) -> TabKey {
    let opened = content_routes::open(state, tab, false);
    assert!(opened.is_ok(), "{opened:?}");
    let key = state.service.app_registry().tabs_view().active;
    key.unwrap_or_else(|| panic!("no active tab"))
}

fn send(state: &DesktopState, command: Command) -> Ack {
    let effects = Effects {
        notify: &|_, _| {},
        check_update: || panic!("no update check in this test"),
        webviews: &crate::browser_webview::NoWebviews,
        pick_folder: &|| None,
    };
    let applied = apply(state, command, &effects);
    applied
        .map(|applied| applied.ack)
        .unwrap_or_else(|e| panic!("{e:?}"))
}

/// The tabs whose diff the `Ack` names.
fn named(ack: &Ack) -> Vec<er_protocol::TabKey> {
    ack.changed
        .iter()
        .filter_map(|changed| match &changed.key {
            ResourceKey::Diff { tab } => Some(*tab),
            _ => None,
        })
        .collect()
}

#[test]
fn a_pr_sync_moves_only_the_prs_tabs_and_warms_its_open_cache() {
    with_storage(|dir| {
        let (state, root) = project(dir);
        let other = open(&state, content_routes::working_tab(root.clone()));
        let pr_tab = TabDescriptor {
            kind: TabKind::LocalPr,
            pr_owner: Some("acme".into()),
            pr_repo: Some("widgets".into()),
            pr_number: Some(7),
            pr_head_ref: Some("topic".into()),
            ..content_routes::working_tab(root.clone())
        };
        let pr = open(&state, pr_tab);

        let sync = Command::SyncPr {
            project_id: PROJECT.into(),
            pr_number: 7,
        };
        let ack = send(&state, sync);
        assert_eq!(named(&ack), [er_service::wire_tab_key(pr)]);
        assert!(!named(&ack).contains(&er_service::wire_tab_key(other)));

        let key = PrOpenCacheKey {
            project_id: PROJECT.into(),
            repo_root: root,
            pr_number: 7,
        };
        let registry = state.service.app_registry();
        let deadline = Instant::now() + WAIT;
        while registry.pr_open_entry(&key).is_none() {
            assert!(
                Instant::now() < deadline,
                "the sync left the open cache cold"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    });
}

#[test]
fn a_branch_sync_moves_only_the_tabs_that_show_the_branch() {
    with_storage(|dir| {
        let (state, root) = project(dir);
        let working = open(&state, content_routes::working_tab(root.clone()));
        let view = TabDescriptor {
            kind: TabKind::LocalBranch,
            branch: Some("topic".into()),
            ..content_routes::working_tab(root)
        };
        let view = open(&state, view);

        let sync = |branch: &str| Command::SyncBranch {
            project_id: PROJECT.into(),
            branch: branch.into(),
        };
        let topic = send(&state, sync("topic"));
        assert_eq!(named(&topic), [er_service::wire_tab_key(view)]);
        let feature = send(&state, sync("feature"));
        assert_eq!(named(&feature), [er_service::wire_tab_key(working)]);
    });
}
