//! The browser annotation writes on `er_send`: each names the tab and view
//! the page painted, writes `ui-annotations.json` and the screenshots in that
//! tab's view bucket, and moves `ui_annotations(tab)`.

use std::path::Path;
use std::sync::Barrier;

use er_engine::TabKey;
use er_protocol::{
    Ack, AnchorUpdate, Command, ErErrorKind, NewUiAnnotation, PaintedView, Resource, ResourceKey,
    UiAnnotation,
};

use super::annotation_write_tests::view;
use super::annotations_resource_tests::branch_view_of_a_pr;
use super::diff_command_tests::{send, with_tab};
use super::diff_resource_tests::with_storage;
use crate::resources::tabs::Routed;
use crate::resources::ui_annotation_writes;
use crate::state::DesktopState;

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
        annotation: Box::new(annotation(id, url)),
    }
}

/// A write's own effect, read from the rows the fetch answers and its `Ack`.
type Check<'a> = Box<dyn Fn(&[UiAnnotation], &Ack) + 'a>;

fn fetched(state: &DesktopState, key: TabKey) -> (u64, Vec<UiAnnotation>) {
    let tab = er_service::wire_tab_key(key);
    let fetched = crate::resources::fetch(state, ResourceKey::UiAnnotations { tab })
        .unwrap_or_else(|e| panic!("{e:?}"));
    match fetched.value {
        Resource::UiAnnotations(ui) => (fetched.version.0, ui.rows),
        other => panic!("not the browser annotations: {other:?}"),
    }
}

fn row<'a>(rows: &'a [UiAnnotation], id: &str) -> &'a UiAnnotation {
    rows.iter()
        .find(|r| r.id == id)
        .unwrap_or_else(|| panic!("no row {id}"))
}

#[expect(
    clippy::float_cmp,
    reason = "the box is stored and read back as sent, so the same box is the same bits"
)]
fn assert_box(row: &UiAnnotation, want: [f64; 4], what: &str) {
    assert_eq!([row.box_x, row.box_y, row.box_w, row.box_h], want, "{what}");
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

/// Each write answers, moves `ui_annotations(tab)` to
/// the version its `Ack` names, and leaves the rows and files it should, as
/// the `ui_annotations(tab)` fetch and the view bucket show them.
#[test]
fn each_browser_annotation_write_moves_ui_annotations() {
    with_tab(|state, (key, tab)| {
        let painted = view(state, key);
        let wire = er_service::wire_tab_key(key);
        let content = state.service.content(key, &tab).expect("the tab's content");
        let er_dir = content.context.er_dir.clone();
        let shot_of = |id: &str| {
            Path::new(&er_dir)
                .join("screenshots")
                .join(format!("{id}.png"))
        };
        let moved = [5.0, 6.0, 70.0, 20.0];
        let anchors = vec![AnchorUpdate {
            id: "ui-2".into(),
            fresh: false,
            new_box: Some(moved),
        }];
        let unshot = Command::AddUiAnnotation {
            tab: wire,
            view: painted.clone(),
            annotation: Box::new(NewUiAnnotation {
                screenshot_data_url: None,
                ..annotation("ui-3", "/b")
            }),
        };
        let writes: Vec<(&str, Command, &[&str], Check<'_>)> = vec![
            (
                "add",
                add(key, painted.clone(), "ui-1", "/a"),
                &["ui-1"],
                Box::new(|_, _| assert!(shot_of("ui-1").exists(), "add saved no screenshot")),
            ),
            (
                "add another",
                add(key, painted.clone(), "ui-2", "/b"),
                &["ui-1", "ui-2"],
                Box::new(|rows, _| {
                    let added = row(rows, "ui-2");
                    assert!(!added.stale, "a new row is stale");
                    assert_box(added, [1.0, 2.0, 30.0, 10.0], "a new row's box");
                }),
            ),
            (
                "add one with no screenshot",
                unshot,
                &["ui-1", "ui-2", "ui-3"],
                Box::new(|rows, _| {
                    assert_eq!(row(rows, "ui-3").screenshot_path, None);
                    assert!(!shot_of("ui-3").exists(), "a screenshot nobody sent");
                }),
            ),
            (
                "anchors",
                Command::UpdateUiAnnotationAnchors {
                    tab: wire,
                    view: painted.clone(),
                    updates: anchors,
                },
                &["ui-1", "ui-2", "ui-3"],
                Box::new(|rows, _| {
                    let anchored = row(rows, "ui-2");
                    assert!(anchored.stale, "the anchor update left ui-2 fresh");
                    assert_box(anchored, moved, "the anchor update moved no box");
                }),
            ),
            (
                "screenshot",
                Command::SaveAnnotationScreenshot {
                    tab: wire,
                    view: painted.clone(),
                    id: "ui-3".into(),
                    data_url: SHOT.into(),
                },
                &["ui-1", "ui-2", "ui-3"],
                Box::new(|rows, ack| {
                    let saved = std::fs::canonicalize(shot_of("ui-3"))
                        .unwrap_or_else(|e| panic!("no screenshot in the view bucket: {e}"));
                    let created = ack.created.as_deref().map(Path::new);
                    assert_eq!(created, Some(saved.as_path()), "the Ack names another file");
                    assert_eq!(
                        row(rows, "ui-3").screenshot_path,
                        ack.created,
                        "the row does not point at its screenshot"
                    );
                }),
            ),
            (
                "clear page",
                Command::ClearUiAnnotationsForPage {
                    tab: wire,
                    view: painted.clone(),
                    page_url: "/a".into(),
                },
                &["ui-2", "ui-3"],
                Box::new(|_, _| {
                    assert!(
                        !shot_of("ui-1").exists(),
                        "the cleared row's screenshot stayed"
                    );
                }),
            ),
            (
                "delete",
                Command::DeleteUiAnnotation {
                    tab: wire,
                    view: painted.clone(),
                    id: "ui-3".into(),
                },
                &["ui-2"],
                Box::new(|_, _| {}),
            ),
            (
                "clear all",
                Command::ClearUiAnnotations {
                    tab: wire,
                    view: painted.clone(),
                },
                &[],
                Box::new(|_, _| {}),
            ),
        ];
        for (name, command, left, check) in writes {
            let (before, _) = fetched(state, key);

            let ack = send(state, name, command).unwrap_or_else(|e| panic!("{name}: {e:?}"));

            let (after, rows) = fetched(state, key);
            assert!(after > before, "{name} moved no version");
            let ids: Vec<_> = rows.iter().map(|r| r.id.as_str()).collect();
            assert_eq!(ids, left, "{name}");
            check(&rows, &ack);
            let named: Vec<_> = ack
                .changed
                .iter()
                .map(|c| (c.key.clone(), c.version.0))
                .collect();
            assert_eq!(
                named,
                [(ResourceKey::UiAnnotations { tab: wire }, after)],
                "{name}"
            );
        }
    });
}

/// Writes sent together each land: 24 adds started at once leave 24 rows.
/// The threads call the route itself, since `send` waits on each one and
/// would line them up on its own.
#[test]
fn browser_annotation_writes_sent_together_each_land() {
    with_tab(|state, (key, _)| {
        let painted = view(state, key);
        let wire = er_service::wire_tab_key(key);
        let mut ids: Vec<String> = (0..24).map(|n| format!("ui-{n}")).collect();
        let start = Barrier::new(ids.len());

        std::thread::scope(|scope| {
            for id in &ids {
                let (start, painted) = (&start, painted.clone());
                scope.spawn(move || {
                    let command = Command::AddUiAnnotation {
                        tab: wire,
                        view: painted,
                        annotation: Box::new(NewUiAnnotation {
                            screenshot_data_url: None,
                            ..annotation(id, "/a")
                        }),
                    };
                    start.wait();
                    match ui_annotation_writes::apply(state, command) {
                        Routed::Applied(Ok(_)) => {}
                        Routed::Applied(Err(e)) => panic!("{id}: {e:?}"),
                        Routed::Other(_) => panic!("{id} was not a browser annotation write"),
                    }
                });
            }
        });

        let (_, rows) = fetched(state, key);
        let mut landed: Vec<String> = rows.into_iter().map(|r| r.id).collect();
        landed.sort();
        ids.sort();
        assert_eq!(
            landed,
            ids,
            "lost updates: {} of {} rows survived",
            landed.len(),
            ids.len()
        );
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
