//! On the flag path the app registry issues every tab key, and the App's tabs
//! follow its list by key: one key names a tab in the registry, the App, the
//! loaded content and the desktop view state.

use std::sync::Arc;

use er_engine::tabs_record::{TabDescriptor, TabKind};
use er_engine::TabKey;
use er_service::{AppCommand, Command, OpenTarget};

use super::{review_flow_test_state, ReviewFlowTestDir};
use crate::commands::{
    content_routes, route_annotation, service_add_document_thread, service_new_tab, AppState,
    DocumentThread,
};
use crate::service::ServiceState;
use crate::snapshot::OptimisticView;

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

fn dir_in(dir: &ReviewFlowTestDir, name: &str) -> String {
    let path = dir.path().join(name);
    std::fs::create_dir_all(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
    path.to_string_lossy().into_owned()
}

/// What the follower thread does after a tab change.
fn follow(state: &AppState) {
    crate::tabs::follow(
        &state.app,
        &state.view,
        state.service.app_registry(),
        &ports(),
    );
}

fn app_keys(state: &AppState) -> (Vec<TabKey>, TabKey) {
    let app = state.app.lock().unwrap_or_else(|e| e.into_inner());
    (
        app.tabs.iter().map(|tab| tab.key()).collect(),
        app.tab().key(),
    )
}

/// The App follows, then one key names the active tab everywhere: the
/// registry, the App, the content on the tab's actor, its nav state and its
/// browser state.
fn assert_one_key(state: &AppState, step: &str) -> TabKey {
    follow(state);
    let view = state.service.app_registry().tabs_view();
    let registry: Vec<TabKey> = view.tabs.iter().map(|entry| entry.key).collect();
    let (app, app_active) = app_keys(state);
    assert_eq!(app, registry, "{step}: the App's keys");
    let active = view.active.expect("an active tab");
    assert_eq!(app_active, active, "{step}: the App's active key");

    let selected = content_routes::select_file(state, 1);
    assert!(selected.is_ok(), "{step}: {selected:?}");
    let entry = view.active_entry().expect("the active entry");
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

    // The prune keeps the registry's keys, so the nav and browser entries
    // survive it and a follow only when they share the App's key.
    crate::tabs::prune(&state.view, state.service.app_registry());
    follow(state);
    let desktop = state.view.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(desktop.nav(app_active).selected_file, 1, "{step}: nav key");
    assert!(
        desktop.browser(app_active).url.ends_with(step),
        "{step}: browser key"
    );
    active
}

fn kinds(dir: &ReviewFlowTestDir) -> Vec<(&'static str, TabDescriptor)> {
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
/// replace-open; the replaced tab's key leaves the registry and the App.
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
            let (app, _) = app_keys(&state);
            assert!(!app.contains(&duplicate), "{kind}: the replaced tab stayed");
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
            &state.app,
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
        let (app, app_active) = app_keys(&state);
        assert_eq!(app, vec![view.tabs[0].key], "the App's keys");
        assert_eq!(Some(app_active), view.active);
        let root = crate::app_owner::startup::launch_root(state.service.app_registry(), None);
        assert_eq!(root, launch, "the launch root");
    });
}

/// The flag-on launch root is the registry's active tab after the restore,
/// not the App's startup tab, and a remote PR takes the fallback.
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
        let startup = app(&state).tab().repo_root.clone();
        assert_ne!(startup, two, "the App starts on another root");
        crate::tabs::restore(
            &state.app,
            &state.view,
            state.service.app_registry(),
            &ports(),
            None,
        );

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

/// `main.rs` takes the flag-on launch root from the registry after the
/// restore; no flag-on test runs `main`, so the source is what is checked.
#[test]
fn main_takes_the_flag_on_launch_root_from_the_registry() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/main.rs");
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{e}"));
    let source = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let restore = source.find("tabs::restore(").expect("the flag-on restore");
    let flag_on = "#[cfg(feature = \"v1-service\")] let launch_root = \
                   er_desktop::app_owner::startup::launch_root( service.app_registry(), \
                   restore_root.as_deref(), );";
    let at = source
        .find(flag_on)
        .expect("the flag-on launch root reads the registry");
    assert!(at > restore, "the launch root is read before the restore");
    let reads_app = "#[cfg(not(feature = \"v1-service\"))] let launch_root: String = \
                     app.tab().repo_root.clone();";
    assert!(
        source.contains(reads_app),
        "the App's root is the flag-off launch root only"
    );
    assert_eq!(
        source.matches("let launch_root").count(),
        2,
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
        follow(&state);
        let view = state.service.app_registry().tabs_view();
        let keys: Vec<TabKey> = view.tabs.iter().map(|entry| entry.key).collect();
        assert_eq!(keys.len(), 2);
        assert_ne!(keys[0], keys[1]);
        assert_eq!(app_keys(&state).0, keys);
        let actor = state
            .service
            .published_view(&root)
            .expect("the repo's actor");
        assert!(keys.iter().all(|key| actor.tab(*key).is_some()));
    });
}

/// The flag-on restore: three saved tabs, one of which no longer builds. The
/// App holds the registry's tabs under the registry's keys, and the broken
/// tab is closed in the registry.
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

        crate::tabs::restore(
            &state.app,
            &state.view,
            state.service.app_registry(),
            &ports(),
            None,
        );

        let view = state.service.app_registry().tabs_view();
        let registry: Vec<TabKey> = view.tabs.iter().map(|entry| entry.key).collect();
        let (app, app_active) = app_keys(&state);
        assert_eq!(app, registry, "the App's keys");
        assert_eq!(registry.len(), 2, "the broken tab stayed open");
        assert_eq!(Some(app_active), view.active);
        let active = view.active_entry().expect("an active tab");
        assert_eq!(active.descriptor.repo_root, last.repo_root);
    });
}

/// A launch with no saved tabs: the App's startup tab joins the registry
/// under the key the registry gives it.
#[test]
fn a_restore_with_no_saved_tabs_keys_the_startup_tab_in_the_registry() {
    with_storage(|dir| {
        let state = state(dir);
        if let Ok(mut app) = state.app.lock() {
            app.tab_mut().placeholder = false;
        }
        crate::tabs::restore(
            &state.app,
            &state.view,
            state.service.app_registry(),
            &ports(),
            None,
        );
        let view = state.service.app_registry().tabs_view();
        let registry: Vec<TabKey> = view.tabs.iter().map(|entry| entry.key).collect();
        let (app, app_active) = app_keys(&state);
        assert_eq!(registry.len(), 1, "the startup tab did not join");
        assert_eq!(app, registry, "the App's keys");
        assert_eq!(Some(app_active), view.active);
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
/// published content, loading it first when it never loaded: a followed App
/// tab holds no content of its own.
#[test]
fn an_annotation_writes_through_the_registry_tab_content() {
    with_storage(|dir| {
        let state = state(dir);
        let root = dir_in(dir, "repo-under-review");
        let tab = content_routes::working_tab(root.clone());
        let key = open_unfollowed(&state, &tab);
        follow(&state);
        assert!(state.service.content(key, &tab).is_none());

        let written = route_annotation(&state, None, |context| Command::MarkReviewed {
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

fn app(state: &AppState) -> std::sync::MutexGuard<'_, er_engine::app::App> {
    state.app.lock().unwrap_or_else(|e| e.into_inner())
}

/// Opens `tab` in the registry alone, as a tab command does before the
/// follower thread runs, and answers its key.
fn open_unfollowed(state: &AppState, tab: &TabDescriptor) -> TabKey {
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

/// The view the page paints for the App's active tab.
fn painted(state: &AppState) -> OptimisticView {
    let app = app(state);
    let tab = app.tab();
    OptimisticView {
        active_tab: app.active_tab,
        repo_root: tab.repo_root.clone(),
        pr_number: tab.pr_number,
        branch: tab.local_branch_view.clone().unwrap_or_default(),
        mode: crate::snapshot::mode_str(tab.mode).to_string(),
    }
}

/// A follow keeps the App tabs it holds as they are: a tab's loaded diff and
/// selected file survive another tab opening.
#[test]
fn a_follow_keeps_the_state_of_a_tab_the_app_holds() {
    with_storage(|dir| {
        let state = state(dir);
        let first = content_routes::working_tab(dir_in(dir, "one"));
        let opened = content_routes::open(&state, first, false);
        assert!(opened.is_ok(), "{opened:?}");
        follow(&state);
        let held = {
            let mut app = app(&state);
            let tab = app.tab_mut();
            tab.files = er_engine::git::parse_diff(&diff());
            tab.selected_file = 1;
            tab.key()
        };

        let second = content_routes::working_tab(dir_in(dir, "two"));
        let opened = content_routes::open(&state, second, false);
        assert!(opened.is_ok(), "{opened:?}");
        follow(&state);

        let app = app(&state);
        assert_eq!(app.tabs.len(), 2);
        let tab = app
            .tabs
            .iter()
            .find(|tab| tab.key() == held)
            .expect("the held tab");
        assert_eq!(tab.files.len(), 2, "the held tab's diff was dropped");
        assert_eq!(tab.selected_file, 1, "the held tab's selection was reset");
    });
}

/// While the App still shows the tab the registry left, a paint on the App's
/// tab is refused and nothing lands on the registry's new tab. Once the App
/// follows, the same paint is written there.
#[test]
fn a_paint_is_refused_while_the_app_lags_the_registry() {
    with_storage(|dir| {
        let state = state(dir);
        let first = content_routes::working_tab(dir_in(dir, "one"));
        let opened = content_routes::open(&state, first, false);
        assert!(opened.is_ok(), "{opened:?}");
        follow(&state);
        let second = content_routes::working_tab(dir_in(dir, "two"));
        let key = open_unfollowed(&state, &second);
        let mark = |view: OptimisticView| {
            route_annotation(&state, Some(&view), |context| Command::MarkReviewed {
                context,
                path: "src/a.rs".to_string(),
                reviewed: true,
            })
        };
        let marked = || {
            state
                .service
                .content(key, &second)
                .is_some_and(|content| content.reviewed.marks.contains_key("src/a.rs"))
        };

        let refused = mark(painted(&state));

        assert!(refused.is_err(), "a lagging paint was written: {refused:?}");
        assert!(!marked(), "the mark landed on the registry's other tab");
        follow(&state);
        let written = mark(painted(&state));
        assert!(written.is_ok(), "{written:?}");
        assert!(marked(), "the followed paint was not written");
    });
}

/// `route_annotation` checks the App's tab against the registry's, so while
/// the App still shows the tab the registry left, a write sent through it is
/// refused, and the registry's new tab keeps its mark. `reply_to_github_thread`
/// sends through it. The reviewed writes do not: they send without the
/// painted tab's key, so the unmark here is only a write whose effect the
/// marks show.
#[test]
fn route_annotation_refuses_a_write_while_the_app_lags_the_registry() {
    with_storage(|dir| {
        let state = state(dir);
        let first = content_routes::working_tab(dir_in(dir, "one"));
        let opened = content_routes::open(&state, first, false);
        assert!(opened.is_ok(), "{opened:?}");
        follow(&state);
        let left = app(&state).tab().key();
        let second = content_routes::working_tab(dir_in(dir, "two"));
        let key = open_unfollowed(&state, &second);
        follow(&state);
        let review = |reviewed: bool| {
            route_annotation(&state, None, |context| Command::MarkReviewed {
                context,
                path: "src/a.rs".to_string(),
                reviewed,
            })
        };
        let marked = || {
            state
                .service
                .content(key, &second)
                .is_some_and(|content| content.reviewed.marks.contains_key("src/a.rs"))
        };
        let written = review(true);
        assert!(written.is_ok(), "{written:?}");
        assert!(marked(), "the followed mark was not written");
        let select = |key: TabKey| {
            let selected = state.service.send_app(AppCommand::SelectTab { key });
            assert!(selected.is_ok(), "{selected:?}");
        };
        select(left);
        follow(&state);
        select(key);

        let refused = review(false);

        assert!(
            refused.is_err(),
            "route_annotation wrote while the App lagged: {refused:?}"
        );
        assert!(
            marked(),
            "the lagging write landed on the registry's other tab"
        );
    });
}

/// An added README the preview rebuilds from the diff.
const README: &str = "diff --git a/README.md b/README.md\nnew file mode 100644\n--- /dev/null\n+++ b/README.md\n@@ -0,0 +1,3 @@\n+# Title\n+\n+alpha\n";

/// Gives the App's active tab the README and answers its preview key.
fn seed_readme(state: &AppState) -> String {
    let mut app = app(state);
    let tab = app.tab_mut();
    tab.set_raw_diff_for_test(README);
    tab.file_headers = er_engine::git::parse_diff_headers(README);
    tab.files = er_engine::git::parse_diff(README);
    tab.file_preview_key("README.md")
}

fn document_note(preview_key: String) -> DocumentThread {
    DocumentThread {
        file: "README.md".to_string(),
        kind: "note".to_string(),
        line_num: 3,
        line_num_end: None,
        text: "Why alpha?".to_string(),
        preview_key,
    }
}

fn notes(state: &AppState, key: TabKey, tab: &TabDescriptor) -> usize {
    state
        .service
        .content(key, tab)
        .and_then(|content| content.ai.state.notes.as_ref().map(|n| n.notes.len()))
        .unwrap_or(0)
}

/// A document thread carries the App tab's text, so while the App still
/// shows the tab the registry left it is refused, and the registry's new tab
/// gets no thread. Once the App follows, the thread is written there.
#[test]
fn a_document_thread_is_refused_while_the_app_lags_the_registry() {
    with_storage(|dir| {
        let state = state(dir);
        let first = content_routes::working_tab(dir_in(dir, "one"));
        let opened = content_routes::open(&state, first, false);
        assert!(opened.is_ok(), "{opened:?}");
        follow(&state);
        let preview_key = seed_readme(&state);
        let second = content_routes::working_tab(dir_in(dir, "two"));
        let key = open_unfollowed(&state, &second);

        let refused = service_add_document_thread(&state, document_note(preview_key));

        assert!(
            refused.is_err(),
            "a lagging thread was written: {refused:?}"
        );
        assert_eq!(notes(&state, key, &second), 0, "the thread moved tabs");
        follow(&state);
        let preview_key = seed_readme(&state);
        let written = service_add_document_thread(&state, document_note(preview_key));
        assert!(written.is_ok(), "{written:?}");
        assert_eq!(notes(&state, key, &second), 1, "the thread was not written");
    });
}

/// `adopt_key` and the App's follow exist for the flag path only: every
/// file that calls them is a module compiled with `v1-service` alone.
#[test]
fn the_tab_key_callers_are_gated_on_the_service() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let crates = src.join("..").join("..");
    let needles = [
        [".adopt", "_key("].concat(),
        ["follow", "_registry("].concat(),
    ];
    let mut ungated = Vec::new();
    let mut files = Vec::new();
    collect_rust(&crates, &mut files);
    for path in files {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let calls = text.lines().any(|line| {
            needles.iter().any(|needle| line.contains(needle.as_str()))
                && !line.trim_start().starts_with("pub fn ")
                && !line.trim_start().starts_with("//")
        });
        if calls && !gated(&path) {
            ungated.push(path.display().to_string());
        }
    }
    assert!(ungated.is_empty(), "flag-off callers in {ungated:?}");
}

fn collect_rust(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path
                .file_name()
                .is_some_and(|name| name != "target" && name != "node_modules")
            {
                collect_rust(&path, out);
            }
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// Whether `path` is a module whose declaration is gated on `v1-service`: the
/// `mod` line in its parent file follows a `cfg(feature = "v1-service")`
/// line with no `not(` in it. A module declared inline in a parent is found
/// in the file above it.
fn gated(path: &std::path::Path) -> bool {
    let Some(name) = path.file_stem().and_then(|stem| stem.to_str()) else {
        return false;
    };
    let declaration = format!("mod {name};");
    let mut dir = path.parent();
    while let Some(parent) = dir {
        let candidates = if parent.ends_with("src") {
            vec![parent.join("lib.rs"), parent.join("main.rs")]
        } else {
            vec![parent.with_extension("rs"), parent.join("mod.rs")]
        };
        for candidate in candidates {
            let Ok(text) = std::fs::read_to_string(&candidate) else {
                continue;
            };
            let lines: Vec<&str> = text.lines().collect();
            let found = lines
                .iter()
                .position(|line| line.trim_start().trim_start_matches("pub ") == declaration);
            if let Some(idx) = found {
                // `not(feature = "v1-service")` names the flag too, and
                // gates the module on the flag-off path.
                return idx > 0
                    && lines[idx - 1].contains("feature = \"v1-service\"")
                    && !lines[idx - 1].contains("not(");
            }
        }
        if parent.ends_with("src") {
            return false;
        }
        dir = parent.parent();
    }
    false
}
