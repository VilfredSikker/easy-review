//! The previews and the path list on the flag path: a document or image
//! preview is read from the tab's content with the App locked, and refused
//! once the view moved, before the read or during it; the path list is every
//! file the diff holds, and the views the App loads keep their own.

use std::path::Path;
use std::sync::Arc;

use er_engine::tab_identity::DiffMode;
use er_engine::tabs_record::TabDescriptor;
use er_protocol::{ErErrorKind, Resource, ResourceKey};
use er_service::{AppCommand, Command};

use super::diff_resource_tests::{fake_ports, listed, state_over, with_storage};
use super::watcher_tests::{canonical, git};
use super::while_the_app_is_locked;
use crate::commands::{content_routes, AppState};
use crate::resources;
use crate::service::actor_key;

fn context(state: &AppState, key: er_engine::TabKey) -> String {
    let tab = er_service::wire_tab_key(key);
    match resources::fetch(&state.desktop, ResourceKey::Diff { tab }).map(|f| f.value) {
        Ok(Resource::Diff(diff)) => diff.preview_context_key,
        other => panic!("not a diff: {other:?}"),
    }
}

fn preview(state: &AppState, context: String) -> Result<String, String> {
    while_the_app_is_locked(state, "the document preview", move |state| {
        crate::commands::file_preview(state, "README.md".into(), context).map(|p| p.text)
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

/// With the App locked, the preview answers from the active tab's content;
/// after the file changes and the tab refreshes, the old context is refused.
#[test]
fn a_document_preview_reads_the_content_with_the_app_locked_and_refuses_a_moved_view() {
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

/// The list holds every file whatever the filter keeps, and a view the App
/// loads is refused, as the old command refuses it.
#[test]
fn the_path_list_is_every_file_and_refuses_the_views_the_app_loads() {
    with_storage(|dir| {
        let state = state_over(dir, fake_ports());
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
        let paths = |state: &AppState| {
            let fetched = resources::fetch(&state.desktop, ResourceKey::DiffPaths { tab: tab_key });
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
        assert_eq!(paths(&state).map_err(|e| e.kind), Err(ErErrorKind::Refused));
    });
}

const PNG_ONE: &[u8] = b"\x89PNG\r\n\x1a\n\0one";
const PNG_TWO: &[u8] = b"\x89PNG\r\n\x1a\n\0two";
const PNG_THREE: &[u8] = b"\x89PNG\r\n\x1a\n\0three";

type Sides = (Option<String>, Option<String>);

fn png(bytes: &[u8]) -> Option<String> {
    Some(crate::commands::image_data_url("image/png", bytes))
}

fn image(state: &AppState, context: String) -> Result<Sides, String> {
    while_the_app_is_locked(state, "the image preview", move |state| {
        crate::commands::image_preview(state, "logo.png".into(), context)
            .map(|p| (p.before, p.after))
    })
}

/// A repo whose image has an unstaged edit, loaded in Unstaged as the
/// registry's active tab.
fn image_tab(state: &AppState, root: &Path) -> (er_engine::TabKey, TabDescriptor) {
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

/// With the App locked, both sides answer from the active tab's content, by
/// the old command and by the resource; after the image changes and the tab
/// refreshes, the old context is refused.
#[test]
fn an_image_preview_reads_the_content_with_the_app_locked_and_refuses_a_moved_view() {
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
            let fetched = resources::fetch(
                &state.desktop,
                ResourceKey::ImagePreview { tab, path, context },
            );
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
        state.desktop.ports.git = Arc::new(moving);

        let read = crate::commands::image_preview(&state, "logo.png".into(), context(&state, key));
        assert_eq!(
            read.map(|p| (p.before, p.after)),
            Err("Image preview context changed".to_string())
        );
    });
}
