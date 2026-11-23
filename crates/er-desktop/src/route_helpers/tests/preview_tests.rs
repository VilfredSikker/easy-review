//! The previews and the path list: a document or image preview is read from
//! the tab's content, and refused once the view moved, before the read or
//! during it; the path list is every file the diff holds, History's the files
//! of its commit.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use er_engine::tab_identity::DiffMode;
use er_engine::tabs_record::TabDescriptor;
use er_protocol::{Resource, ResourceKey};
use er_service::{AppCommand, Command};

use super::diff_resource_tests::{fake_git, listed, state_over, with_storage};
use super::on_a_thread;
use super::watcher_tests::{canonical, git};
use crate::resources;
use crate::resources::content_routes;
use crate::route_helpers::DesktopState;
use crate::service::actor_key;

fn context(state: &DesktopState, key: er_engine::TabKey) -> String {
    let tab = er_service::wire_tab_key(key);
    match resources::fetch(state, ResourceKey::Diff { tab }).map(|f| f.value) {
        Ok(Resource::Diff(diff)) => diff.preview_context_key,
        other => panic!("not a diff: {other:?}"),
    }
}

/// The active tab's entry in the registry, which a preview reads.
fn active(state: &DesktopState) -> er_service::TabEntry {
    let view = state.service.app_registry().tabs_view();
    view.active_entry()
        .cloned()
        .unwrap_or_else(|| panic!("no active tab"))
}

fn preview(state: &DesktopState, context: String) -> Result<String, String> {
    on_a_thread(state, "the document preview", move |state| {
        resources::preview::document(state, &active(state), "README.md", &context)
            .map(|p| p.text)
            .map_err(|e| e.message)
    })
}

/// A repo whose README has an unstaged edit.
fn repo(root: &Path) {
    std::fs::create_dir_all(root).unwrap_or_else(|e| panic!("repo dir: {e}"));
    git(root, &["init", "-q", "-b", "main"]);
    std::fs::write(root.join("README.md"), "# one\n").unwrap_or_else(|e| panic!("{e}"));
    git(root, &["add", "README.md"]);
    git(root, &["commit", "-q", "-m", "a"]);
    std::fs::write(root.join("README.md"), "# two\n").unwrap_or_else(|e| panic!("{e}"));
}

/// The preview answers from the active tab's content;
/// after the file changes and the tab refreshes, the old context is refused.
#[test]
fn a_document_preview_reads_the_content_and_refuses_a_moved_view() {
    with_storage(|dir| {
        let root = dir.path().join("repo");
        repo(&root);
        let state = state_over(dir, er_engine::ports::Ports::cli());
        let tab = content_routes::working_tab(canonical(&root).to_string_lossy().into_owned());
        let added = state
            .service
            .send_app(AppCommand::NewTab { tab: tab.clone() });
        assert!(added.is_ok(), "{added:?}");
        let key = state.service.app_registry().tabs_view().active;
        let key = key.unwrap_or_else(|| panic!("no tab"));
        let load = Command::LoadTab {
            key,
            tab: tab.clone(),
            mode: Some(DiffMode::Unstaged),
        };
        assert!(state.service.send(&actor_key(&tab), load).is_ok());

        let before = context(&state, key);
        assert_eq!(preview(&state, before.clone()), Ok("# two\n".to_string()));

        std::fs::write(root.join("README.md"), "# three\n").unwrap_or_else(|e| panic!("{e}"));
        assert!(content_routes::refresh_tab(&state, key, false).is_ok());
        let moved = preview(&state, before);
        assert_eq!(moved, Err("Document preview context changed".to_string()));
        assert_eq!(
            preview(&state, context(&state, key)),
            Ok("# three\n".to_string())
        );
    });
}

/// History is loaded on the service, so its preview reads the selected
/// commit's file from the content, as the other views do.
#[test]
fn a_history_preview_reads_the_commit_from_the_content() {
    with_storage(|dir| {
        let root = dir.path().join("repo");
        repo(&root);
        git(&root, &["checkout", "-q", "-b", "feature"]);
        git(&root, &["commit", "-q", "-am", "b"]);
        std::fs::write(root.join("README.md"), "# three\n").unwrap_or_else(|e| panic!("{e}"));
        let state = state_over(dir, er_engine::ports::Ports::cli());
        let tab = content_routes::working_tab(canonical(&root).to_string_lossy().into_owned());
        let added = state
            .service
            .send_app(AppCommand::NewTab { tab: tab.clone() });
        assert!(added.is_ok(), "{added:?}");
        let key = state.service.app_registry().tabs_view().active;
        let key = key.unwrap_or_else(|| panic!("no tab"));
        let load = Command::LoadTab {
            key,
            tab: tab.clone(),
            mode: Some(DiffMode::History),
        };
        assert!(state.service.send(&actor_key(&tab), load).is_ok());

        // The commit's text, not the working tree's.
        assert_eq!(
            preview(&state, context(&state, key)),
            Ok("# two\n".to_string())
        );
    });
}

const COMMIT_DIFF: &str =
    "diff --git a/src/c.rs b/src/c.rs\n--- a/src/c.rs\n+++ b/src/c.rs\n@@ -1 +1 @@\n-a\n+b\n";

/// A git whose History lists one commit, which touches `src/c.rs`.
fn history_ports() -> er_engine::ports::Ports {
    let commit = er_engine::git::CommitInfo {
        hash: "c1".into(),
        short_hash: "c1".into(),
        subject: String::new(),
        author: String::new(),
        date: String::new(),
        relative_date: String::new(),
        file_count: 0,
        adds: 0,
        dels: 0,
        is_merge: false,
        diffstat_known: false,
    };
    let git = fake_git()
        .with_log_range(Ok(vec![commit]))
        .with_commit_shortstats(HashMap::new())
        .with_unpushed_shas(HashSet::new())
        .with_diff_commit(Ok(COMMIT_DIFF.into()));
    let git: Arc<dyn er_git::Git> = Arc::new(git);
    let github: Arc<dyn er_engine::github::GitHub> =
        Arc::new(er_engine::github::FakeGitHub::default());
    er_engine::ports::Ports { git, github }
}

/// The list holds every file whatever the filter keeps, and History lists
/// the files of its commit, read from the content as every other view is.
#[test]
fn the_path_list_is_every_file_and_history_lists_its_commit() {
    with_storage(|dir| {
        let state = state_over(dir, history_ports());
        let (key, tab) = listed(&state, dir, "repo");
        let load = Command::LoadTab {
            key,
            tab: tab.clone(),
            mode: None,
        };
        assert!(state.service.send(&actor_key(&tab), load).is_ok());
        if let Ok(mut view) = state.view.lock() {
            view.set_filter(key, "src/f1.rs");
        }
        let tab_key = er_service::wire_tab_key(key);
        let paths = |state: &DesktopState| {
            let fetched = resources::fetch(state, ResourceKey::DiffPaths { tab: tab_key });
            fetched.map(|f| f.value)
        };
        let Ok(Resource::DiffPaths(listed)) = paths(&state) else {
            panic!("no path list: {:?}", paths(&state));
        };
        assert_eq!(listed.paths, ["src/f0.rs", "src/f1.rs", "src/f2.rs"]);

        let history = Command::SetDiffMode {
            key,
            mode: DiffMode::History,
            pr_number: None,
        };
        assert!(state.service.send(&actor_key(&tab), history).is_ok());
        let Ok(Resource::DiffPaths(listed)) = paths(&state) else {
            panic!("no History path list: {:?}", paths(&state));
        };
        assert_eq!(listed.paths, ["src/c.rs"]);
    });
}

const PNG_ONE: &[u8] = b"\x89PNG\r\n\x1a\n\0one";
const PNG_TWO: &[u8] = b"\x89PNG\r\n\x1a\n\0two";
const PNG_THREE: &[u8] = b"\x89PNG\r\n\x1a\n\0three";

type Sides = (Option<String>, Option<String>);

fn png(bytes: &[u8]) -> Option<String> {
    Some(crate::route_helpers::image_data_url("image/png", bytes))
}

fn image(state: &DesktopState, context: String) -> Result<Sides, String> {
    on_a_thread(state, "the image preview", move |state| {
        resources::preview::image(state, &active(state), "logo.png", &context)
            .map(|p| (p.before, p.after))
            .map_err(|e| e.message)
    })
}

/// A repo whose image has an unstaged edit, loaded in Unstaged as the
/// registry's active tab.
fn image_tab(state: &DesktopState, root: &Path) -> (er_engine::TabKey, TabDescriptor) {
    std::fs::create_dir_all(root).unwrap_or_else(|e| panic!("repo dir: {e}"));
    git(root, &["init", "-q", "-b", "main"]);
    std::fs::write(root.join("logo.png"), PNG_ONE).unwrap_or_else(|e| panic!("{e}"));
    git(root, &["add", "logo.png"]);
    git(root, &["commit", "-q", "-m", "a"]);
    std::fs::write(root.join("logo.png"), PNG_TWO).unwrap_or_else(|e| panic!("{e}"));
    let tab = content_routes::working_tab(canonical(root).to_string_lossy().into_owned());
    let added = state
        .service
        .send_app(AppCommand::NewTab { tab: tab.clone() });
    assert!(added.is_ok(), "{added:?}");
    let key = state.service.app_registry().tabs_view().active;
    let key = key.unwrap_or_else(|| panic!("no tab"));
    let load = Command::LoadTab {
        key,
        tab: tab.clone(),
        mode: Some(DiffMode::Unstaged),
    };
    assert!(state.service.send(&actor_key(&tab), load).is_ok());
    (key, tab)
}

/// Both sides answer from the active tab's content, by the read and by the
/// resource; after the image changes and the tab refreshes, the old context
/// is refused.
#[test]
fn an_image_preview_reads_the_content_and_refuses_a_moved_view() {
    with_storage(|dir| {
        let root = dir.path().join("repo");
        let state = state_over(dir, er_engine::ports::Ports::cli());
        let (key, _) = image_tab(&state, &root);

        let before = context(&state, key);
        assert_eq!(
            image(&state, before.clone()),
            Ok((png(PNG_ONE), png(PNG_TWO)))
        );
        let fetch = |context: String| {
            let tab = er_service::wire_tab_key(key);
            let path = "logo.png".to_string();
            let fetched =
                resources::fetch(&state, ResourceKey::ImagePreview { tab, path, context });
            match fetched.map(|f| f.value) {
                Ok(Resource::ImagePreview(p)) => Ok((p.before, p.after)),
                Ok(other) => panic!("not an image preview: {other:?}"),
                Err(e) => Err(e.message),
            }
        };
        assert_eq!(fetch(before.clone()), Ok((png(PNG_ONE), png(PNG_TWO))));

        std::fs::write(root.join("logo.png"), PNG_THREE).unwrap_or_else(|e| panic!("{e}"));
        assert!(content_routes::refresh_tab(&state, key, false).is_ok());
        let moved = Err("Image preview context changed".to_string());
        assert_eq!(image(&state, before.clone()), moved);
        assert_eq!(fetch(before), moved);
        assert_eq!(
            image(&state, context(&state, key)),
            Ok((png(PNG_ONE), png(PNG_THREE)))
        );
    });
}

/// The view moves after the current side is read and while the earlier side
/// is: the context matched before the read, so only the check after it can
/// refuse the preview.
#[test]
fn an_image_preview_refuses_a_view_that_moved_while_it_read() {
    with_storage(|dir| {
        let root = dir.path().join("repo");
        let mut state = state_over(dir, er_engine::ports::Ports::cli());
        let (key, tab) = image_tab(&state, &root);
        let service = Arc::clone(&state.service);
        let file = root.join("logo.png");
        let moving = er_git::FakeGit::default()
            .with_read_blob(Ok(PNG_ONE.to_vec()))
            .with_read_blob_hook(move || {
                std::fs::write(&file, PNG_THREE).unwrap_or_else(|e| panic!("{e}"));
                let refresh = Command::RefreshDiff {
                    key,
                    force: false,
                    adopt_head_oid: None,
                };
                let refreshed = service.send(&actor_key(&tab), refresh);
                assert!(refreshed.is_ok(), "{refreshed:?}");
            });
        state.ports.git = Arc::new(moving);

        let context = context(&state, key);
        let read = resources::preview::image(&state, &active(&state), "logo.png", &context);
        assert_eq!(
            read.map(|p| (p.before, p.after)).map_err(|e| e.message),
            Err("Image preview context changed".to_string())
        );
    });
}
