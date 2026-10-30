use std::{
    cell::RefCell,
    sync::{Arc, Barrier},
};

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use super::*;

thread_local! {
    static FAIL_METADATA: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}
pub(super) fn metadata_write_checkpoint(dir: &Path) -> super::Result<()> {
    FAIL_METADATA.with(|fail| {
        if fail.borrow().as_deref() == Some(dir) {
            fail.replace(None);
            Err(std::io::Error::other("injected metadata commit failure").into())
        } else {
            Ok(())
        }
    })
}
fn fail_next_commit(dir: &Path) -> Result<()> {
    let dir = dir.canonicalize()?;
    FAIL_METADATA.with(|fail| fail.replace(Some(dir)));
    Ok(())
}
fn fixture() -> Value {
    json!({
        "id":"ba-0123456789abcdef", "number":1, "idempotency_key":"draft-1",
        "created_at":"2026-10-30T20:00:00Z", "author":"reviewer", "text":"Fix alignment",
        "page":{"origin":"http://localhost:5173", "path":"/settings", "query":"b=2&a=1", "hash_route":"/preferences"},
        "locator":{"frames":[{"selector":"iframe", "shadow_hosts":["app-root"], "fingerprint":{"tag":"iframe", "role":"document"}}],
            "shadow_hosts":["settings-panel"], "selector":"button.save", "fingerprint":{"tag":"button", "role":"button", "name":"Save", "text_excerpt":"Save changes"}},
        "target":{"kind":"element", "role":"button", "name":"Save", "text_excerpt":"Save changes"},
        "captured":{"rect":{"x":10.0,"y":20.0,"width":80.0,"height":24.0},
            "viewport":{"width":1024.0,"height":768.0}, "scroll":{"x":0.0,"y":100.0}, "device_scale":2.0, "zoom":1.25},
        "dom_context":{"tag":"button", "text":"Save changes"},
        "resolution":{"status":"resolved", "current_rect":{"x":11.0,"y":21.0,"width":80.0,"height":24.0}},
        "image":{"status":"none"}
    })
}
fn draft(key: &str) -> Result<NewBrowserAnnotation> {
    let a: BrowserAnnotation = serde_json::from_value(fixture())?;
    Ok(NewBrowserAnnotation {
        idempotency_key: key.into(),
        created_at: a.created_at,
        author: a.author,
        text: a.text,
        page: a.page,
        locator: a.locator,
        target: a.target,
        captured: a.captured,
        dom_context: a.dom_context,
        resolution: a.resolution,
        image: NewImageEvidence::None,
    })
}
fn capture() -> Result<ImageCapture> {
    Ok(serde_json::from_value(json!({
        "crop":{"x":0.0,"y":0.0,"width":128.0,"height":96.0},
        "marker":{"x":10.0,"y":10.0,"width":16.0,"height":16.0},
        "scale":2.0, "captured_at":"2026-10-30T20:00:00Z"
    }))?)
}
fn png(width: u32, height: u32) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&vec![0; (width * height) as usize])?;
        writer.finish()?;
    }
    Ok(bytes)
}
fn image_id(annotation: &BrowserAnnotation) -> Result<&str> {
    match &annotation.image {
        ImageEvidence::Captured { asset_id, .. } => Ok(asset_id),
        _ => Err(anyhow!("expected captured image")),
    }
}
fn save_image(dir: &Path, key: &str) -> Result<BrowserAnnotation> {
    let mut new = draft(key)?;
    new.image = NewImageEvidence::Captured(capture()?);
    Ok(save_browser_annotation(dir, new, Some(&png(2, 2)?))?)
}
fn read_json(dir: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&fs::read(
        dir.join(BROWSER_ANNOTATIONS_FILE),
    )?)?)
}
fn write_json(dir: &Path, value: &Value) -> Result<()> {
    fs::write(
        dir.join(BROWSER_ANNOTATIONS_FILE),
        serde_json::to_vec(value)?,
    )?;
    Ok(())
}
fn missing() -> Result<Resolution> {
    Ok(serde_json::from_value(json!({"status":"missing"}))?)
}

#[test]
fn missing_file_is_empty_and_does_not_create_bucket() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let dir = temp.path().join("not-created");
    assert!(load_browser_annotations(&dir)?.is_empty());
    assert!(!delete_browser_annotation(&dir, "absent")?);
    assert!(update_resolution(&dir, "absent", missing()?)?.is_none());
    assert!(!dir.exists());
    let saved = save_browser_annotation(&dir, draft("first")?, None)?;
    assert_eq!(saved.number, 1);
    Ok(())
}

#[test]
fn every_mutation_refuses_malformed_future_and_invalid_files() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let good = save_image(temp.path(), "original")?;
    let valid = read_json(temp.path())?;
    let mut invalid = valid.clone();
    invalid["annotations"][0]["captured"]["zoom"] = json!(-1);
    let mut future = valid.clone();
    future["version"] = json!(3);
    let mut bad_next = valid.clone();
    bad_next["next_number"] = json!(1);
    let mut duplicate = valid.clone();
    duplicate["annotations"]
        .as_array_mut()
        .ok_or_else(|| anyhow!("array"))?
        .push(fixture());
    let mut missing_version = valid.clone();
    missing_version
        .as_object_mut()
        .ok_or_else(|| anyhow!("object"))?
        .remove("version");
    for bytes in [
        b"{bad".to_vec(),
        serde_json::to_vec(&future)?,
        serde_json::to_vec(&invalid)?,
        serde_json::to_vec(&bad_next)?,
        serde_json::to_vec(&duplicate)?,
        serde_json::to_vec(&missing_version)?,
    ] {
        fs::write(temp.path().join(BROWSER_ANNOTATIONS_FILE), &bytes)?;
        assert!(load_browser_annotations(temp.path()).is_err());
        assert!(save_browser_annotation(temp.path(), draft("new")?, None).is_err());
        assert!(save_browser_annotation(temp.path(), draft("original")?, None).is_err());
        assert!(save_image(temp.path(), "new-image").is_err());
        assert!(delete_browser_annotation(temp.path(), &good.id).is_err());
        assert!(delete_browser_annotation(temp.path(), "absent").is_err());
        assert!(update_resolution(temp.path(), &good.id, missing()?).is_err());
        assert_eq!(fs::read(temp.path().join(BROWSER_ANNOTATIONS_FILE))?, bytes);
        assert_eq!(
            read_browser_asset(temp.path(), image_id(&good)?)?,
            png(2, 2)?
        );
        assert_eq!(
            fs::read_dir(temp.path().join(assets::ASSETS_DIR))?.count(),
            1
        );
    }
    Ok(())
}

#[test]
fn full_round_trip_unknown_optional_fields_survive_rewrites() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let saved = save_image(temp.path(), "round-trip")?;
    let mut json = read_json(temp.path())?;
    json["future_file"] = json!({"enabled":true});
    for pointer in [
        "/annotations/0",
        "/annotations/0/page",
        "/annotations/0/locator",
        "/annotations/0/locator/frames/0",
        "/annotations/0/locator/fingerprint",
        "/annotations/0/target",
        "/annotations/0/captured",
        "/annotations/0/captured/rect",
        "/annotations/0/captured/viewport",
        "/annotations/0/captured/scroll",
        "/annotations/0/image",
        "/annotations/0/image/capture",
        "/annotations/0/image/capture/crop",
        "/annotations/0/image/capture/marker",
        "/annotations/0/resolution",
    ] {
        json.pointer_mut(pointer)
            .ok_or_else(|| anyhow!("missing fixture path"))?["future"] = json!([1, "optional"]);
    }
    write_json(temp.path(), &json)?;
    let loaded = load_browser_annotations(temp.path())?;
    assert_eq!(
        serde_json::from_slice::<BrowserAnnotation>(&serde_json::to_vec(&loaded[0])?)?,
        loaded[0]
    );
    let current = loaded[0].resolution.clone();
    update_resolution(temp.path(), &saved.id, current)?;
    assert_eq!(read_json(temp.path())?, json);
    let extra = save_browser_annotation(temp.path(), draft("extra")?, None)?;
    delete_browser_annotation(temp.path(), &extra.id)?;
    json["next_number"] = json!(3);
    assert_eq!(read_json(temp.path())?, json);
    assert_eq!(
        read_browser_asset(temp.path(), image_id(&saved)?)?,
        png(2, 2)?
    );
    let updated = update_resolution(temp.path(), &saved.id, missing()?)?
        .ok_or_else(|| anyhow!("missing record"))?;
    assert_eq!(
        updated.resolution.extra.get("future"),
        Some(&json!([1, "optional"]))
    );
    Ok(())
}

#[test]
fn idempotency_deletion_numbers_and_resolution_preserve_evidence() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let first = save_image(temp.path(), "one")?;
    let mut retry = draft("one")?;
    retry.text = "different retry".repeat(10000);
    assert_eq!(
        save_browser_annotation(temp.path(), retry, Some(b"bad png"))?,
        first
    );
    let second = save_browser_annotation(temp.path(), draft("two")?, None)?;
    let third = save_browser_annotation(temp.path(), draft("three")?, None)?;
    assert_eq!([first.number, second.number, third.number], [1, 2, 3]);
    let updated = update_resolution(temp.path(), &first.id, missing()?)?
        .ok_or_else(|| anyhow!("missing record"))?;
    assert_eq!(updated.captured, first.captured);
    assert_eq!(updated.image, first.image);
    assert_eq!(updated.page, first.page);
    assert!(delete_browser_annotation(temp.path(), &second.id)?);
    assert_eq!(
        save_browser_annotation(temp.path(), draft("four")?, None)?.number,
        4
    );
    assert!(delete_browser_annotation(temp.path(), &first.id)?);
    assert!(read_browser_asset(temp.path(), image_id(&first)?).is_err());
    assert!(!delete_browser_annotation(temp.path(), &first.id)?);
    Ok(())
}

#[test]
fn concurrent_saves_are_serialized_in_one_bucket() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let start = Arc::new(Barrier::new(8));
    let mut handles = Vec::new();
    for n in 0..8 {
        let dir = temp.path().to_owned();
        let barrier = start.clone();
        let new = draft(&format!("draft-{n}"))?;
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            save_browser_annotation(&dir, new, None)
        }));
    }
    let mut numbers = Vec::new();
    for handle in handles {
        numbers.push(
            handle
                .join()
                .map_err(|_| anyhow!("save thread panicked"))??
                .number,
        );
    }
    numbers.sort_unstable();
    assert_eq!(numbers, (1..=8).collect::<Vec<_>>());
    assert_eq!(load_browser_annotations(temp.path())?.len(), 8);
    Ok(())
}

#[test]
fn metadata_failure_cleans_new_asset_and_keeps_previous_file() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let first = save_image(temp.path(), "old")?;
    let previous = fs::read(temp.path().join(BROWSER_ANNOTATIONS_FILE))?;
    fail_next_commit(temp.path())?;
    assert!(save_image(temp.path(), "new").is_err());
    assert_eq!(
        fs::read_dir(temp.path().join(assets::ASSETS_DIR))?.count(),
        1
    );
    assert_eq!(
        fs::read(temp.path().join(BROWSER_ANNOTATIONS_FILE))?,
        previous
    );
    assert_eq!(
        read_browser_asset(temp.path(), image_id(&first)?)?,
        png(2, 2)?
    );
    fail_next_commit(temp.path())?;
    assert!(delete_browser_annotation(temp.path(), &first.id).is_err());
    assert_eq!(
        read_browser_asset(temp.path(), image_id(&first)?)?,
        png(2, 2)?
    );
    assert_eq!(save_image(temp.path(), "new")?.number, 2);
    let fresh = tempfile::tempdir()?;
    fail_next_commit(fresh.path())?;
    assert!(save_image(fresh.path(), "first").is_err());
    assert!(!fresh.path().join(BROWSER_ANNOTATIONS_FILE).exists());
    assert_eq!(
        fs::read_dir(fresh.path().join(assets::ASSETS_DIR))?.count(),
        0
    );
    assert_eq!(save_image(fresh.path(), "first")?.number, 1);
    Ok(())
}

#[test]
fn png_validation_rejects_corruption_oversize_and_invalid_dimensions() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let valid = png(2, 2)?;
    let mut corrupt = valid.clone();
    corrupt.truncate(corrupt.len() - 8);
    for bytes in [
        b"not png".to_vec(),
        b"\x89PNG\r\n\x1a\n".to_vec(),
        corrupt,
        vec![0; MAX_PNG_BYTES + 1],
        png(MAX_PNG_SIDE + 1, 1)?,
        png(1, MAX_PNG_SIDE + 1)?,
    ] {
        let mut new = draft("bad-image")?;
        new.image = NewImageEvidence::Captured(capture()?);
        assert!(save_browser_annotation(temp.path(), new, Some(&bytes)).is_err());
        assert!(!temp.path().join(BROWSER_ANNOTATIONS_FILE).exists());
        assert!(!temp.path().join(assets::ASSETS_DIR).exists());
    }
    let saved = save_image(temp.path(), "corrupt-on-disk")?;
    let path = temp
        .path()
        .join(assets::ASSETS_DIR)
        .join(format!("{}.png", image_id(&saved)?));
    fs::write(&path, &valid[..24])?;
    assert!(read_browser_asset(temp.path(), image_id(&saved)?).is_err());
    fs::write(&path, vec![0; MAX_PNG_BYTES + 1])?;
    assert!(read_browser_asset(temp.path(), image_id(&saved)?).is_err());
    Ok(())
}

#[test]
fn invalid_records_and_nonfinite_geometry_cannot_commit() -> Result<()> {
    let temp = tempfile::tempdir()?;
    for coordinate in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 10_000_001.0] {
        let mut new = draft("bad")?;
        new.captured.rect.x = coordinate;
        assert!(save_browser_annotation(temp.path(), new, None).is_err());
    }
    let mut variants = Vec::new();
    let mut new = draft("bad")?;
    new.text = "x".repeat(MAX_NOTE_BYTES + 1);
    variants.push(new);
    let mut new = draft("bad")?;
    new.dom_context = Some(json!("x".repeat(MAX_DOM_BYTES)));
    variants.push(new);
    let mut new = draft("bad")?;
    new.locator = None;
    variants.push(new);
    let mut new = draft("bad")?;
    new.captured.viewport.width = 0.0;
    variants.push(new);
    let mut new = draft("bad")?;
    new.captured.zoom = -1.0;
    variants.push(new);
    let mut new = draft("bad")?;
    new.page.origin = "http://localhost:5173/path".into();
    variants.push(new);
    let mut new = draft("bad-fingerprint")?;
    if let Some(locator) = &mut new.locator {
        locator.fingerprint.name = Some("界".repeat(81));
    }
    variants.push(new);
    for new in variants {
        assert!(save_browser_annotation(temp.path(), new, None).is_err());
    }
    assert!(!temp.path().join(BROWSER_ANNOTATIONS_FILE).exists());
    let saved = save_browser_annotation(temp.path(), draft("good")?, None)?;
    let mut resolution = missing()?;
    resolution.current_rect = Some(saved.captured.rect);
    assert!(update_resolution(temp.path(), &saved.id, resolution).is_err());
    let mut file = read_json(temp.path())?;
    file["next_number"] = json!(u32::MAX);
    write_json(temp.path(), &file)?;
    assert!(matches!(
        save_browser_annotation(temp.path(), draft("exhausted")?, None),
        Err(BrowserAnnotationsError::NumberExhausted)
    ));
    Ok(())
}

#[test]
fn asset_ids_only_and_image_evidence_must_agree() -> Result<()> {
    let temp = tempfile::tempdir()?;
    for id in [
        "../secret",
        "/tmp/test",
        "ba-asset-0123456789ABCDEf",
        "ba-asset-0123",
        "ba-asset-0123456789abcdef.png",
        "ba-asset-0123456789abcdef/../other",
    ] {
        assert!(matches!(
            read_browser_asset(temp.path(), id),
            Err(BrowserAnnotationsError::InvalidAsset(_))
        ));
    }
    assert!(save_browser_annotation(temp.path(), draft("bad")?, Some(&png(1, 1)?)).is_err());
    let mut new = draft("bad")?;
    new.image = NewImageEvidence::Captured(capture()?);
    assert!(save_browser_annotation(temp.path(), new, None).is_err());
    let mut new = draft("unavailable")?;
    new.image = NewImageEvidence::Unavailable {
        reason: "native capture unavailable".into(),
    };
    assert!(save_browser_annotation(temp.path(), new, None).is_ok());
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlinks_cannot_escape_and_bucket_aliases_share_lock() -> Result<()> {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir()?;
    let foreign = tempfile::tempdir()?;
    let saved = save_image(temp.path(), "one")?;
    let asset = temp
        .path()
        .join(assets::ASSETS_DIR)
        .join(format!("{}.png", image_id(&saved)?));
    let original = fs::read(&asset)?;
    fs::remove_file(&asset)?;
    let outside = foreign.path().join("outside.png");
    fs::write(&outside, &original)?;
    symlink(&outside, &asset)?;
    assert!(read_browser_asset(temp.path(), image_id(&saved)?).is_err());
    let before = read_json(temp.path())?;
    assert!(delete_browser_annotation(temp.path(), &saved.id).is_err());
    assert_eq!(read_json(temp.path())?, before);
    fs::remove_file(&asset)?;
    fs::remove_dir(temp.path().join(assets::ASSETS_DIR))?;
    symlink(foreign.path(), temp.path().join(assets::ASSETS_DIR))?;
    assert!(save_image(temp.path(), "two").is_err());
    assert!(read_browser_asset(temp.path(), image_id(&saved)?).is_err());
    assert_eq!(fs::read(&outside)?, original);
    let alias = foreign.path().join("bucket-alias");
    symlink(temp.path(), &alias)?;
    let barrier = Arc::new(Barrier::new(8));
    let mut handles = Vec::new();
    for n in 0..8 {
        let dir = if n % 2 == 0 {
            alias.clone()
        } else {
            temp.path().to_owned()
        };
        let start = barrier.clone();
        let new = draft(&format!("alias-{n}"))?;
        handles.push(std::thread::spawn(move || {
            start.wait();
            save_browser_annotation(&dir, new, None)
        }));
    }
    for handle in handles {
        handle.join().map_err(|_| anyhow!("thread panicked"))??;
    }
    assert_eq!(load_browser_annotations(temp.path())?.len(), 9);
    let metadata = temp.path().join(BROWSER_ANNOTATIONS_FILE);
    fs::remove_file(&metadata)?;
    symlink(&outside, &metadata)?;
    assert!(load_browser_annotations(temp.path()).is_err());
    assert!(save_browser_annotation(temp.path(), draft("bad-link")?, None).is_err());
    assert_eq!(fs::read(&outside)?, original);
    Ok(())
}

#[test]
fn region_records_ignore_legacy_files_and_keep_number_after_deleting_all() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let legacy = temp.path().join("ui-annotations.json");
    fs::write(&legacy, b"legacy data remains untouched")?;
    assert!(load_browser_annotations(temp.path())?.is_empty());
    let mut new = draft("region")?;
    new.target.kind = TargetKind::Region;
    new.target.limitation = Some("Cross-origin frame".into());
    new.locator = None;
    let first = save_browser_annotation(temp.path(), new, None)?;
    assert_eq!(load_browser_annotations(temp.path())?, vec![first.clone()]);
    delete_browser_annotation(temp.path(), &first.id)?;
    assert_eq!(
        save_browser_annotation(temp.path(), draft("next")?, None)?.number,
        2
    );
    assert_eq!(fs::read(legacy)?, b"legacy data remains untouched");
    Ok(())
}

#[test]
fn version_errors_and_extension_collisions_are_explicit() -> Result<()> {
    let temp = tempfile::tempdir()?;
    write_json(temp.path(), &json!({"version":3}))?;
    assert!(
        matches!(load_browser_annotations(temp.path()), Err(BrowserAnnotationsError::UnsupportedVersion(v)) if v == json!(3))
    );
    fs::remove_file(temp.path().join(BROWSER_ANNOTATIONS_FILE))?;
    let mut new = draft("collision")?;
    new.page
        .extra
        .insert("origin".into(), json!("http://other:3000"));
    new.image = NewImageEvidence::Captured(capture()?);
    assert!(save_browser_annotation(temp.path(), new, Some(&png(2, 2)?)).is_err());
    assert!(!temp.path().join(BROWSER_ANNOTATIONS_FILE).exists());
    assert_eq!(
        fs::read_dir(temp.path().join(assets::ASSETS_DIR))?.count(),
        0
    );
    Ok(())
}
