//! On the flag path the app registry issues every tab key, and the App's tabs
//! follow its list by key: one key names a tab in the registry, the App, the
//! loaded content and the desktop view state.

use std::sync::Arc;

use er_engine::tabs_record::{TabDescriptor, TabKind};
use er_engine::TabKey;
use er_service::{Command, OpenTarget};

use super::{review_flow_test_state, ReviewFlowTestDir};
use crate::commands::{content_routes, route_annotation, service_new_tab, AppState};
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

    // A follow prunes the view state to the App's keys, so the nav and
    // browser entries survive it only when they share the App's key.
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

/// An annotation takes its context from the registry's active tab and its
/// published content, loading it first when it never loaded, whatever the
/// App's own tab holds.
#[test]
fn an_annotation_writes_through_the_registry_tab_content() {
    with_storage(|dir| {
        let state = state(dir);
        let root = dir_in(dir, "repo-under-review");
        let tab = content_routes::working_tab(root.clone());
        let entry = state
            .service
            .send_app(er_service::AppCommand::NewTab { tab: tab.clone() });
        assert!(entry.is_ok(), "{entry:?}");
        let key = state
            .service
            .app_registry()
            .tabs_view()
            .active
            .expect("the new tab");
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
/// line. A module declared inline in a parent is found in the file above it.
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
                return idx > 0 && lines[idx - 1].contains("feature = \"v1-service\"");
            }
        }
        if parent.ends_with("src") {
            return false;
        }
        dir = parent.parent();
    }
    false
}
