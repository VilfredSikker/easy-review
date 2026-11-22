//! The browser annotation writes on `er_send`: each names the tab and view
//! the page painted, writes `ui-annotations.json` and the screenshots in that
//! tab's view bucket with the App locked, and moves `ui_annotations(tab)`.

use std::path::Path;

use er_engine::TabKey;
use er_protocol::{
    AnchorUpdate, Command, ErErrorKind, NewUiAnnotation, PaintedView, Resource, ResourceKey,
};

use super::annotation_write_tests::view;
use super::annotations_resource_tests::branch_view_of_a_pr;
use super::diff_command_tests::{send, with_tab};
use super::diff_resource_tests::with_storage;
use crate::commands::AppState;

/// A one-pixel PNG's bytes do not matter here; the data URL's shape does.
const SHOT: &str = "data:image/png;base64,iVBORw0KGgo=";

fn annotation(id: &str, url: &str) -> NewUiAnnotation {
    NewUiAnnotation {
        url: url.into(),
        selector: Some("#save".into()),
        bbox: [1.0, 2.0, 30.0, 10.0],
        viewport: [1280, 800],
        text: "This button moved".into(),
        screenshot_data_url: Some(SHOT.into()),
        element_context: None,
        dom_context_json: Some(r#"{"tag":"button"}"#.into()),
        client_id: Some(id.into()),
    }
}

fn add(key: TabKey, view: PaintedView, id: &str, url: &str) -> Command {
    Command::AddUiAnnotation {
        tab: er_service::wire_tab_key(key),
        view,
        annotation: annotation(id, url),
    }
}

fn rows(state: &AppState, key: TabKey) -> (u64, Vec<String>) {
    let tab = er_service::wire_tab_key(key);
    let fetched = crate::resources::fetch(&state.desktop, ResourceKey::UiAnnotations { tab })
        .unwrap_or_else(|e| panic!("{e:?}"));
    match fetched.value {
        Resource::UiAnnotations(ui) => (
            fetched.version.0,
            ui.rows.into_iter().map(|r| r.id).collect(),
        ),
        other => panic!("not the browser annotations: {other:?}"),
    }
}

fn ids_in(dir: &str) -> Vec<String> {
    er_engine::ai::load_ui_annotations(dir)
        .into_iter()
        .map(|a| a.id)
        .collect()
}

/// On a local PR tab in its branch view, whose PR bucket is not its view
/// bucket, an annotation and its screenshots land in the view bucket.
#[test]
fn the_browser_annotation_writes_land_in_the_view_bucket_never_the_pr_bucket() {
    with_storage(|dir| {
        let (state, (key, tab)) = branch_view_of_a_pr(dir);
        let content = state.service.content(key, &tab).expect("the branch view");
        let (er_dir, comments_dir) = (&content.context.er_dir, &content.context.comments_dir);
        assert_ne!(er_dir, comments_dir, "the fixture's two buckets are one");
        let painted = view(&state, key);

        let added = send(
            &state,
            "add_ui_annotation",
            add(key, painted.clone(), "ui-1", "/a"),
        );
        let shot = Command::SaveAnnotationScreenshot {
            tab: er_service::wire_tab_key(key),
            view: painted,
            id: "ui-1".into(),
            data_url: SHOT.into(),
        };
        let saved = send(&state, "save_annotation_screenshot", shot);

        assert!(added.is_ok() && saved.is_ok(), "{added:?} {saved:?}");
        assert_eq!(
            ids_in(er_dir),
            ["ui-1"],
            "the row is not in the view bucket"
        );
        assert!(
            ids_in(comments_dir).is_empty(),
            "the row went to the PR bucket"
        );
        let screenshot = |dir: &str| Path::new(dir).join("screenshots").join("ui-1.png");
        assert!(
            screenshot(er_dir).exists(),
            "no screenshot in the view bucket"
        );
        assert!(
            !screenshot(comments_dir).exists(),
            "a screenshot went to the PR bucket"
        );
    });
}

/// With the App locked, each write answers, moves `ui_annotations(tab)` to
/// the version its `Ack` names, and leaves the rows it should.
#[test]
fn each_browser_annotation_write_moves_ui_annotations_with_the_app_locked() {
    with_tab(|state, (key, _)| {
        let painted = view(state, key);
        let tab = er_service::wire_tab_key(key);
        let anchors = vec![AnchorUpdate {
            id: "ui-2".into(),
            fresh: false,
            new_box: None,
        }];
        let writes: [(&str, Command, &[&str]); 7] = [
            ("add", add(key, painted.clone(), "ui-1", "/a"), &["ui-1"]),
            (
                "add another",
                add(key, painted.clone(), "ui-2", "/b"),
                &["ui-1", "ui-2"],
            ),
            (
                "anchors",
                Command::UpdateUiAnnotationAnchors {
                    tab,
                    view: painted.clone(),
                    updates: anchors,
                },
                &["ui-1", "ui-2"],
            ),
            (
                "screenshot",
                Command::SaveAnnotationScreenshot {
                    tab,
                    view: painted.clone(),
                    id: "ui-1".into(),
                    data_url: SHOT.into(),
                },
                &["ui-1", "ui-2"],
            ),
            (
                "delete",
                Command::DeleteUiAnnotation {
                    tab,
                    view: painted.clone(),
                    id: "ui-1".into(),
                },
                &["ui-2"],
            ),
            (
                "clear page",
                Command::ClearUiAnnotationsForPage {
                    tab,
                    view: painted.clone(),
                    page_url: "/a".into(),
                },
                &["ui-2"],
            ),
            (
                "clear all",
                Command::ClearUiAnnotations {
                    tab,
                    view: painted.clone(),
                },
                &[],
            ),
        ];
        for (name, command, left) in writes {
            let (before, _) = rows(state, key);

            let ack = send(state, name, command).unwrap_or_else(|e| panic!("{name}: {e:?}"));

            let (after, ids) = rows(state, key);
            assert!(after > before, "{name} moved no version");
            assert_eq!(ids, left, "{name}");
            let named: Vec<_> = ack
                .changed
                .iter()
                .map(|c| (c.key.clone(), c.version.0))
                .collect();
            assert_eq!(
                named,
                [(ResourceKey::UiAnnotations { tab }, after)],
                "{name}"
            );
        }
    });
}

/// An annotation painted while the tab showed Branch is refused once it
/// shows Staged, and the bucket it shows gets no row.
#[test]
fn a_browser_annotation_painted_in_another_mode_is_refused() {
    with_tab(|state, (key, tab)| {
        let painted = view(state, key);
        let staged = Command::SetDiffMode {
            tab: er_service::wire_tab_key(key),
            mode: "staged".into(),
            pr_number: None,
        };
        let switched = send(state, "set_diff_mode", staged);
        assert!(switched.is_ok(), "{switched:?}");

        let refused = send(state, "stale annotation", add(key, painted, "ui-1", "/a"));

        let kind = refused.as_ref().err().map(|e| e.kind);
        assert_eq!(kind, Some(ErErrorKind::Refused), "{refused:?}");
        let content = state.service.content(key, &tab).expect("the staged tab");
        assert!(
            ids_in(&content.context.er_dir).is_empty(),
            "the stale annotation landed"
        );
    });
}
