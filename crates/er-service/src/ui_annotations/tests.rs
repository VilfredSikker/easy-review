//! The browser annotation writes and the screenshot read, on their own and
//! through a workspace actor.

use std::collections::BTreeMap;
use std::sync::{Arc, Barrier};
use std::time::Duration;

use er_engine::ports::Ports;
use er_github::FakeGitHub;

use super::*;
use crate::content::{self, TabContent};
use crate::protocol::{Command, Reply, TabContext};

fn ann(id: &str) -> UiAnnotation {
    UiAnnotation {
        id: id.into(),
        url: "/x".into(),
        selector: Some(format!("#{id}")),
        box_x: 1.0,
        box_y: 2.0,
        box_w: 10.0,
        box_h: 20.0,
        viewport_w: 800,
        viewport_h: 600,
        text: "hi".into(),
        timestamp: "t".into(),
        author: "You".into(),
        screenshot_path: None,
        stale: false,
        element_context: None,
        dom_context: None,
    }
}

#[test]
fn save_annotation_screenshot_writes_bytes_and_returns_path() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_string_lossy().to_string();

    // Minimal 1x1 transparent PNG (8-byte signature + IHDR + IDAT + IEND).
    // Bytes don't need to be a valid image for the I/O test — just round-trip.
    let png_bytes: Vec<u8> = vec![
        0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0xDE, 0xAD, 0xBE, 0xEF,
    ];

    let path = save_screenshot_bytes(&dir, "ui-test-42", &png_bytes).unwrap();
    assert!(
        path.ends_with("ui-test-42.png"),
        "path should be the id.png: {path}"
    );
    let read_back = std::fs::read(&path).unwrap();
    assert_eq!(read_back, png_bytes, "saved bytes must match input");

    // No temporary file may survive the rename.
    let leftovers: Vec<String> = std::fs::read_dir(std::path::Path::new(&path).parent().unwrap())
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".tmp."))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn decode_data_url_png_roundtrips_through_base64() {
    let payload = b"\x89PNG\r\n\x1a\n hello";
    let encoded = base64_encode(payload);
    let data_url = format!("data:image/png;base64,{encoded}");
    let decoded = decode_data_url_png(&data_url).expect("should decode");
    assert_eq!(decoded, payload);
}

#[test]
fn decode_data_url_png_rejects_non_base64_data_url() {
    assert!(decode_data_url_png("not-a-data-url").is_none());
    assert!(decode_data_url_png("data:text/plain,hello").is_none());
    assert!(decode_data_url_png("data:image/png,plainstuff").is_none());
}

#[test]
fn update_ui_annotation_anchors_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_string_lossy().to_string();
    save_ui_annotations(&dir, &[ann("a1"), ann("a2")]).unwrap();

    let updates = vec![
        AnchorUpdate {
            id: "a1".into(),
            fresh: true,
            new_box: Some([5.0, 6.0, 7.0, 8.0]),
        },
        AnchorUpdate {
            id: "a2".into(),
            fresh: false,
            new_box: None,
        },
    ];
    apply_anchor_updates(&dir, &updates).unwrap();

    let back = load_ui_annotations(&dir);
    let a1 = back.iter().find(|a| a.id == "a1").unwrap();
    assert!(!a1.stale, "a1 should be fresh");
    assert_eq!(
        (a1.box_x, a1.box_y, a1.box_w, a1.box_h),
        (5.0, 6.0, 7.0, 8.0),
        "a1 box should be updated"
    );

    let a2 = back.iter().find(|a| a.id == "a2").unwrap();
    assert!(a2.stale, "a2 should be marked stale");
    assert_eq!(
        (a2.box_x, a2.box_y, a2.box_w, a2.box_h),
        (1.0, 2.0, 10.0, 20.0),
        "a2 box should be unchanged when new_box is None"
    );
}

fn deps() -> WorkspaceDeps {
    WorkspaceDeps::new(Ports::with_github(Arc::new(FakeGitHub::default())))
}

fn context(bucket: &str) -> TabContext {
    TabContext {
        er_dir: bucket.to_string(),
        comments_dir: bucket.to_string(),
        is_remote: false,
        repo_root: "/repo".to_string(),
        base_branch: "main".to_string(),
        scope: "branch".to_string(),
        remote_repo: None,
        pr_number: None,
        pr_head_ref: None,
        local_branch_view: None,
        checkout_root: None,
        branch_diff_hash: String::new(),
        diff_hash: String::new(),
    }
}

fn added(id: &str) -> UiAnnotationWrite {
    UiAnnotationWrite::Add(Box::new(NewUiAnnotation {
        url: "/a".into(),
        selector: None,
        bbox: [1.0, 2.0, 30.0, 10.0],
        viewport: [1280, 800],
        text: "moved".into(),
        screenshot_data_url: None,
        element_context: None,
        dom_context: None,
        id: Some(id.into()),
    }))
}

/// Writes sent to the workspace together each land: 24 adds started at once
/// leave 24 rows, since each effect holds the bucket's lock around its
/// load-change-save.
#[test]
fn ui_annotation_writes_sent_together_each_land() {
    let tmp = tempfile::tempdir().unwrap();
    let bucket = tmp.path().to_string_lossy().into_owned();
    let handle = crate::spawn(deps());
    let mut ids: Vec<String> = (0..24).map(|n| format!("ui-{n}")).collect();
    let start = Barrier::new(ids.len());
    std::thread::scope(|scope| {
        for id in &ids {
            let (start, handle, bucket) = (&start, handle.clone(), &bucket);
            scope.spawn(move || {
                let command = Command::WriteUiAnnotations {
                    context: context(bucket),
                    write: added(id),
                };
                start.wait();
                let reply = handle.send_and_wait_with(command, Duration::from_secs(10));
                assert_eq!(
                    reply.map_err(|e| e.to_string()),
                    Ok(Reply::UiAnnotationsWritten {
                        created: Some(id.clone())
                    })
                );
            });
        }
    });
    let mut landed: Vec<String> = load_ui_annotations(&bucket)
        .into_iter()
        .map(|a| a.id)
        .collect();
    landed.sort();
    ids.sort();
    assert_eq!(
        landed,
        ids,
        "lost updates: {} of 24 rows survived",
        landed.len()
    );
}

/// The tabs on `bucket`, one loaded at revision 1 whose rows were read at
/// `seq`.
fn tabs_on(bucket: &str, seq: u64) -> BTreeMap<er_engine::TabKey, Arc<TabContent>> {
    let mut loaded = crate::content_tests::loaded_tab("/repo");
    loaded.context.er_dir = bucket.to_string();
    loaded.ui = UiRows {
        rows: vec![ann("ui-loaded")],
        seq,
    };
    let key = er_engine::TabKey::next();
    BTreeMap::from([(key, Arc::new(TabContent::from_loaded(key, *loaded, 1)))])
}

fn ids(tabs: &BTreeMap<er_engine::TabKey, Arc<TabContent>>) -> Vec<String> {
    let content = tabs.values().next().unwrap();
    content.ui.rows.iter().map(|a| a.id.clone()).collect()
}

/// A write's rows reach the tabs on its bucket and move their published
/// revision, so `ui_annotations(tab)` is pushed again. A write that took an
/// older sequence, or a load read before the write, leaves the newer rows.
#[test]
fn a_ui_annotation_write_moves_the_published_revision() {
    let mut tabs = tabs_on("/bucket", 1);
    let before = ui_annotations_version(tabs.values().next().unwrap());

    let write = |rows: &[&str], seq| ContentPatch::Ui {
        er_dir: "/bucket".into(),
        rows: rows.iter().map(|id| ann(id)).collect(),
        seq,
    };
    content::apply(&mut tabs, write(&["ui-1"], 5), 2);
    let after = ui_annotations_version(tabs.values().next().unwrap());
    assert!(after > before, "the write left the version at {after}");
    assert_eq!(ids(&tabs), ["ui-1"]);

    content::apply(&mut tabs, write(&["ui-old"], 3), 3);
    assert_eq!(ids(&tabs), ["ui-1"], "an older write replaced a newer one");

    let mut stale = crate::content_tests::loaded_tab("/repo");
    stale.context.er_dir = "/bucket".into();
    stale.ui = UiRows {
        rows: Vec::new(),
        seq: 4,
    };
    let key = *tabs.keys().next().unwrap();
    content::apply(&mut tabs, ContentPatch::loaded(key, None, stale), 4);
    assert_eq!(
        ids(&tabs),
        ["ui-1"],
        "a load read before the write dropped it"
    );

    let mut other = tabs_on("/other", 1);
    content::apply(&mut other, write(&["ui-1"], 6), 5);
    assert_eq!(ids(&other), ["ui-loaded"], "a write reached another bucket");
}

/// A screenshot saved through a write reads back with the bytes it was sent.
#[test]
fn a_saved_screenshot_reads_back_byte_for_byte() {
    let tmp = tempfile::tempdir().unwrap();
    let bucket = tmp.path().to_string_lossy().into_owned();
    let deps = deps();
    let bytes: Vec<u8> = (0..=255u8).chain([0x89, b'P', b'N', b'G']).collect();
    let data_url = format!("data:image/png;base64,{}", base64_encode(&bytes));

    write(&deps, &bucket, added("ui-shot")).unwrap();
    let (created, _) = write(
        &deps,
        &bucket,
        UiAnnotationWrite::Screenshot {
            id: "ui-shot".into(),
            data_url,
        },
    )
    .unwrap();
    let path = created.expect("the write names the screenshot");
    let row = load_ui_annotations(&bucket);
    assert_eq!(row[0].screenshot_path.as_deref(), Some(path.as_str()));

    let read = read_annotation_screenshot(path).unwrap();
    assert_eq!(decode_data_url_png(&read), Some(bytes));
}
