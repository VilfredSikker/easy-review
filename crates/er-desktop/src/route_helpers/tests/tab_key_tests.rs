//! The app registry issues every tab key: one key names a tab in the
//! registry, the loaded content and the desktop view state.

use std::sync::Arc;

use er_engine::tabs_record::{TabDescriptor, TabKind};
use er_engine::TabKey;
use er_service::{AppCommand, Command, OpenTarget};

use super::{review_flow_test_state, ReviewFlowTestDir};
use crate::resources::content_routes;
use crate::route_helpers::{service_new_tab, DesktopState};
use crate::service::ServiceState;

const REMOTE: &str = "acme/widgets";

fn diff() -> String {
    let mut raw = String::new();
    for name in ["a", "b"] {
        raw.push_str(&format!(
            "diff --git a/src/{name}.rs b/src/{name}.rs\n--- a/src/{name}.rs\n+++ b/src/{name}.rs\n@@ -1 +1 @@\n-old\n+new\n"
        ));
    }
    raw
}

pub(super) fn ports() -> er_engine::ports::Ports {
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

pub(super) fn state(dir: &ReviewFlowTestDir) -> DesktopState {
    let mut state = review_flow_test_state(dir);
    let settings = dir.path().join("settings").join("config.toml");
    state.service = Arc::new(ServiceState::with_settings_path(ports(), settings));
    state
}

pub(super) fn with_storage(test: impl FnOnce(&ReviewFlowTestDir)) {
    let dir = ReviewFlowTestDir::new();
    let storage = dir.path().join("storage");
    er_engine::storage::with_process_test_storage_root(storage, || test(&dir));
}

fn dir_in(dir: &ReviewFlowTestDir, name: &str) -> String {
    let path = dir.path().join(name);
    std::fs::create_dir_all(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
    path.to_string_lossy().into_owned()
}

/// One key names the active tab everywhere: the registry, the content on
/// the tab's actor and its browser state, which a prune keeps.
fn assert_one_key(state: &DesktopState, step: &str) -> TabKey {
    let view = state.service.app_registry().tabs_view();
    let active = view.active.expect("an active tab");
    let entry = view.active_entry().expect("the active entry");
    let loaded = content_routes::keyed_content(state, active, None);
    assert!(loaded.is_ok(), "{step}: {:?}", loaded.err());
    let on_actor = state
        .service
        .published_view(&entry.descriptor.workspace_key())
        .and_then(|view| view.tab(active).cloned());
    assert!(
        on_actor.is_some(),
        "{step}: no content under the active key"
    );
    if let Ok(mut desktop) = state.view.lock() {
        desktop.browser_mut(active).url = format!("https://example.invalid/{step}");
    }

    crate::tabs::prune(&state.view, state.service.app_registry());
    let desktop = state.view.lock().unwrap_or_else(|e| e.into_inner());
    assert!(
        desktop.browser(active).url.ends_with(step),
        "{step}: browser key"
    );
    active
}

pub(super) fn kinds(dir: &ReviewFlowTestDir) -> Vec<(&'static str, TabDescriptor)> {
    let root = dir_in(dir, "repo");
    let mut branch = content_routes::working_tab(root.clone());
    branch.kind = TabKind::LocalBranch;
    branch.branch = Some("feature".to_string());
    let local_pr = content_routes::pr_tab(&OpenTarget::PrReview {
        project_id: "widgets".to_string(),
        remote: REMOTE.to_string(),
        pr_number: 7,
        repo_root: root.clone(),
    });
    let remote_pr = content_routes::pr_tab(&OpenTarget::RemotePr {
        owner: "acme".to_string(),
        repo: "widgets".to_string(),
        number: 7,
        repo_root: String::new(),
    });
    vec![
        ("working", content_routes::working_tab(root)),
        ("local branch", branch),
        (
            "worktree",
            content_routes::working_tab(dir_in(dir, "worktree")),
        ),
        ("local PR", local_pr.expect("local PR tab")),
        ("remote PR", remote_pr.expect("remote PR tab")),
    ]
}

/// Each kind of tab keeps one key through an open, a Cmd-T duplicate and a
/// replace-open; the replaced tab's key leaves the registry.
#[test]
fn every_kind_keeps_one_key_through_open_duplicate_and_replace() {
    with_storage(|dir| {
        // A remote PR's Cmd-T opens on the root of the project tracking it.
        register_project(&dir_in(dir, "repo"));
        let replacement = content_routes::working_tab(dir_in(dir, "replacement"));
        for (kind, tab) in kinds(dir) {
            let state = state(dir);
            let opened = content_routes::open(&state, tab.clone(), false);
            assert!(opened.is_ok(), "{kind}: {opened:?}");
            assert_one_key(&state, kind);

            let duplicated = service_new_tab(&state);
            assert!(duplicated.is_ok(), "{kind}: {duplicated:?}");
            let duplicate = assert_one_key(&state, &format!("{kind} duplicate"));

            let replaced = content_routes::open(&state, replacement.clone(), true);
            assert!(replaced.is_ok(), "{kind}: {replaced:?}");
            let new = assert_one_key(&state, &format!("{kind} replace"));
            assert_ne!(new, duplicate);
            let view = state.service.app_registry().tabs_view();
            assert!(
                view.key_at(1).is_none_or(|key| key != duplicate),
                "{kind}: the replaced tab stayed"
            );
            let old_actor = state.service.published_view(&tab.workspace_key());
            assert!(
                old_actor.is_none_or(|view| view.tab(duplicate).is_none()),
                "{kind}: the replaced tab's content stayed"
            );
        }
    });
}

/// A project on `root` that tracks the remote.
fn register_project(root: &str) {
    let record = er_service::app_records::projects::ProjectRecord {
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
    er_service::app_records::projects::invalidate_load_cache();
}

/// A descriptor that no longer builds: a local PR with no PR number.
fn broken_tab(dir: &ReviewFlowTestDir, name: &str) -> TabDescriptor {
    TabDescriptor {
        kind: TabKind::LocalPr,
        pr_number: None,
        ..content_routes::working_tab(dir_in(dir, name))
    }
}

/// With every saved tab broken, the restore opens a Working tab on the
/// launch root and closes the broken ones; the registry keeps its last tab,
/// so the Working tab opens first.
#[test]
fn a_restore_with_every_tab_broken_leaves_a_working_tab_on_the_launch_root() {
    with_storage(|dir| {
        let saved = crate::tabs::save_tabs(&[broken_tab(dir, "gone"), broken_tab(dir, "lost")], 1);
        assert!(saved.is_ok(), "{saved:?}");
        let state = state(dir);
        let launch = dir_in(dir, "launch");

        crate::tabs::restore(
            &state.view,
            state.service.app_registry(),
            &ports(),
            Some(&launch),
        );

        let view = state.service.app_registry().tabs_view();
        let tabs: Vec<_> = view
            .tabs
            .iter()
            .map(|tab| {
                (
                    tab.descriptor.kind.clone(),
                    tab.descriptor.repo_root.clone(),
                )
            })
            .collect();
        assert_eq!(tabs, vec![(TabKind::Working, launch.clone())]);
        assert_eq!(Some(view.tabs[0].key), view.active);
        let root = crate::app_owner::startup::launch_root(state.service.app_registry(), None);
        assert_eq!(root, launch, "the launch root");
    });
}

/// The launch root is the registry's active tab after the restore, and a
/// remote PR takes the fallback.
#[test]
fn the_launch_root_comes_from_the_registry() {
    with_storage(|dir| {
        let (one, two) = (dir_in(dir, "one"), dir_in(dir, "two"));
        let saved = crate::tabs::save_tabs(
            &[
                content_routes::working_tab(one),
                content_routes::working_tab(two.clone()),
            ],
            1,
        );
        assert!(saved.is_ok(), "{saved:?}");
        let state = state(dir);
        crate::tabs::restore(&state.view, state.service.app_registry(), &ports(), None);

        let registry = state.service.app_registry();
        let fallback = dir_in(dir, "fallback");
        assert_eq!(
            crate::app_owner::startup::launch_root(registry, Some(&fallback)),
            two
        );
        let remote = content_routes::pr_tab(&OpenTarget::RemotePr {
            owner: "acme".to_string(),
            repo: "widgets".to_string(),
            number: 7,
            repo_root: String::new(),
        })
        .expect("remote PR tab");
        assert!(content_routes::open(&state, remote, false).is_ok());
        assert_eq!(
            crate::app_owner::startup::launch_root(registry, Some(&fallback)),
            fallback
        );
    });
}

/// `main.rs` takes the launch root from the registry after the restore; no
/// test runs `main`, so the source is what is checked.
#[test]
fn main_takes_the_launch_root_from_the_registry() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/main.rs");
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{e}"));
    let source = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let restore = source.find("tabs::restore(").expect("the restore");
    let read = "let launch_root = er_desktop::app_owner::startup::launch_root( \
                service.app_registry(), restore_root.as_deref(), );";
    let at = source
        .find(read)
        .expect("the launch root reads the registry");
    assert!(at > restore, "the launch root is read before the restore");
    assert_eq!(
        source.matches("let launch_root").count(),
        1,
        "another launch root"
    );
}

/// Cmd-T on a remote PR tab opens a Working tab on the root of the project
/// that tracks the PR's repo.
#[test]
fn cmd_t_on_a_remote_pr_tab_opens_a_working_tab_with_a_root() {
    with_storage(|dir| {
        let root = dir_in(dir, "widgets");
        register_project(&root);
        let state = state(dir);
        let remote = content_routes::pr_tab(&OpenTarget::RemotePr {
            owner: "acme".to_string(),
            repo: "widgets".to_string(),
            number: 7,
            repo_root: String::new(),
        })
        .expect("remote PR tab");
        assert!(content_routes::open(&state, remote, false).is_ok());

        let opened = service_new_tab(&state);

        assert!(opened.is_ok(), "{opened:?}");
        let view = state.service.app_registry().tabs_view();
        let active = view.active_entry().expect("an active tab");
        assert_eq!(active.descriptor.kind, TabKind::Working);
        assert_eq!(active.descriptor.repo_root, root);
    });
}

/// An open of a PR that is open as a local PR selects that tab when the
/// same PR opens as a remote PR, and the other way round (A15).
#[test]
fn an_open_pr_is_selected_whatever_its_tab_kind() {
    for remote_first in [false, true] {
        with_storage(|dir| open_one_pr_as_both_kinds(dir, remote_first));
    }
}

fn open_one_pr_as_both_kinds(dir: &ReviewFlowTestDir, remote_first: bool) {
    {
        let root = dir_in(dir, "widgets");
        let local = content_routes::pr_tab(&OpenTarget::PrReview {
            project_id: "widgets".to_string(),
            remote: REMOTE.to_string(),
            pr_number: 7,
            repo_root: root.clone(),
        })
        .expect("local PR tab");
        let remote = content_routes::pr_tab(&OpenTarget::RemotePr {
            owner: "acme".to_string(),
            repo: "widgets".to_string(),
            number: 7,
            repo_root: String::new(),
        })
        .expect("remote PR tab");
        let (open, again) = if remote_first {
            (remote, local)
        } else {
            (local, remote)
        };
        let state = state(dir);
        assert!(content_routes::open(&state, open, false).is_ok());
        let key = state.service.app_registry().tabs_view().active;
        let other = content_routes::working_tab(dir_in(dir, "other"));
        assert!(content_routes::open(&state, other, false).is_ok());

        let opened = content_routes::open(&state, again.clone(), false);

        assert!(opened.is_ok(), "{opened:?}");
        let view = state.service.app_registry().tabs_view();
        assert_eq!(view.tabs.len(), 2, "{:?} opened a second tab", again.kind);
        assert_eq!(view.active, key, "the open PR's tab is not active");
    }
}

/// Two Working tabs on one repo are two tabs: two keys, two contents.
#[test]
fn two_working_tabs_on_one_repo_keep_two_keys_and_two_contents() {
    with_storage(|dir| {
        let state = state(dir);
        let root = dir_in(dir, "repo");
        for _ in 0..2 {
            let opened =
                content_routes::open(&state, content_routes::working_tab(root.clone()), false);
            assert!(opened.is_ok(), "{opened:?}");
        }
        let view = state.service.app_registry().tabs_view();
        let keys: Vec<TabKey> = view.tabs.iter().map(|entry| entry.key).collect();
        assert_eq!(keys.len(), 2);
        assert_ne!(keys[0], keys[1]);
        let actor = state
            .service
            .published_view(&root)
            .expect("the repo's actor");
        assert!(keys.iter().all(|key| actor.tab(*key).is_some()));
    });
}

/// The restore: three saved tabs, one of which no longer resolves. The
/// broken tab is closed in the registry.
#[test]
fn a_restore_follows_the_registry_and_closes_a_broken_tab() {
    with_storage(|dir| {
        let first = content_routes::working_tab(dir_in(dir, "one"));
        let broken = TabDescriptor {
            kind: TabKind::LocalPr,
            pr_number: None,
            ..content_routes::working_tab(dir_in(dir, "gone"))
        };
        let last = content_routes::working_tab(dir_in(dir, "two"));
        let saved = crate::tabs::save_tabs(&[first, broken, last.clone()], 2);
        assert!(saved.is_ok(), "{saved:?}");
        let state = state(dir);

        crate::tabs::restore(&state.view, state.service.app_registry(), &ports(), None);

        let view = state.service.app_registry().tabs_view();
        assert_eq!(view.tabs.len(), 2, "the broken tab stayed open");
        let active = view.active_entry().expect("an active tab");
        assert_eq!(active.descriptor.repo_root, last.repo_root);
    });
}

/// A launch with no saved tabs opens a Working tab on the launch root, and
/// one with no launch root opens none.
#[test]
fn a_restore_with_no_saved_tabs_opens_the_launch_root() {
    with_storage(|dir| {
        let state = state(dir);
        crate::tabs::restore(&state.view, state.service.app_registry(), &ports(), None);
        assert!(state.service.app_registry().tabs_view().tabs.is_empty());
        let launch = dir_in(dir, "launch");
        crate::tabs::restore(
            &state.view,
            state.service.app_registry(),
            &ports(),
            Some(&launch),
        );
        let view = state.service.app_registry().tabs_view();
        let roots: Vec<&str> = view
            .tabs
            .iter()
            .map(|t| t.descriptor.repo_root.as_str())
            .collect();
        assert_eq!(roots, [launch.as_str()]);
        assert_eq!(view.active, Some(view.tabs[0].key));
    });
}

/// Opening a PR that is already open selects its tab, as the flag-off open
/// does, instead of adding a second tab for it.
#[test]
fn opening_an_open_pr_selects_its_tab() {
    with_storage(|dir| {
        let state = state(dir);
        let pr = content_routes::pr_tab(&OpenTarget::RemotePr {
            owner: "acme".to_string(),
            repo: "widgets".to_string(),
            number: 7,
            repo_root: String::new(),
        })
        .expect("remote PR tab");
        let first = content_routes::open(&state, pr.clone(), false);
        assert!(first.is_ok(), "{first:?}");
        let key = state.service.app_registry().tabs_view().active;
        let other = content_routes::open(
            &state,
            content_routes::working_tab(dir_in(dir, "repo")),
            false,
        );
        assert!(other.is_ok(), "{other:?}");

        let again = content_routes::open(&state, pr, false);

        assert!(again.is_ok(), "{again:?}");
        let view = state.service.app_registry().tabs_view();
        assert_eq!(view.tabs.len(), 2, "the PR opened a second tab");
        assert_eq!(view.active, key, "the open PR's tab is not active");
    });
}

/// An annotation takes its context from the registry's active tab and its
/// published content, loading it first when it never loaded.
#[test]
fn an_annotation_writes_through_the_registry_tab_content() {
    with_storage(|dir| {
        let state = state(dir);
        let root = dir_in(dir, "repo-under-review");
        let tab = content_routes::working_tab(root.clone());
        let key = open_unloaded(&state, &tab);
        assert!(state.service.content(key, &tab).is_none());

        let written =
            content_routes::write_keyed(&state, key, None, |context| Command::MarkReviewed {
                context,
                path: "src/a.rs".to_string(),
                reviewed: true,
            });

        assert!(written.is_ok(), "{written:?}");
        let content = state
            .service
            .content(key, &tab)
            .expect("the route loaded it");
        assert!(content.reviewed.marks.contains_key("src/a.rs"));
        assert_eq!(content.context.repo_root, root);
    });
}

/// Opens `tab` in the registry alone, with no load, and answers its key.
fn open_unloaded(state: &DesktopState, tab: &TabDescriptor) -> TabKey {
    let opened = state
        .service
        .send_app(AppCommand::NewTab { tab: tab.clone() });
    assert!(opened.is_ok(), "{opened:?}");
    state
        .service
        .app_registry()
        .tabs_view()
        .active
        .expect("the new tab")
}
