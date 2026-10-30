use std::collections::HashSet;

use super::{types::*, BrowserAnnotationsError, BROWSER_ANNOTATIONS_VERSION};

pub const MAX_NOTE_BYTES: usize = 8 * 1024;
pub const MAX_DOM_BYTES: usize = 24 * 1024;
pub const MAX_METADATA_BYTES: u64 = 64 * 1024 * 1024;
const MAX_COORDINATE: f64 = 10_000_000.0;
const MAX_DIMENSION: f64 = 100_000.0;
const MAX_CHAIN: usize = 32;

pub(super) fn host_id(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(|hex| {
        (16..=32).contains(&hex.len())
            && hex
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    })
}

fn bounded(value: &str, max: usize) -> bool {
    value.len() <= max && !value.contains('\0')
}
fn optional(value: Option<&str>, max: usize) -> bool {
    value.is_none_or(|v| bounded(v, max))
}
fn coordinate(value: f64) -> bool {
    value.is_finite() && value.abs() <= MAX_COORDINATE
}
fn dimension(value: f64) -> bool {
    value.is_finite() && value > 0.0 && value <= MAX_DIMENSION
}
fn scale(value: f64) -> bool {
    value.is_finite() && value > 0.0 && value <= 100.0
}
fn rect(value: &Rect) -> bool {
    coordinate(value.x) && coordinate(value.y) && dimension(value.width) && dimension(value.height)
}
fn fingerprint(value: &Fingerprint) -> bool {
    optional(value.role.as_deref(), 256)
        && optional(value.name.as_deref(), 1024)
        && optional(value.text_excerpt.as_deref(), 2048)
}
fn selectors(value: &[String]) -> bool {
    value.len() <= MAX_CHAIN && value.iter().all(|v| !v.is_empty() && bounded(v, 2048))
}
pub(super) fn resolution(value: &Resolution) -> bool {
    value.current_rect.as_ref().is_none_or(rect)
        && (value.status == ResolutionStatus::Resolved || value.current_rect.is_none())
}
fn origin(value: &str) -> bool {
    let authority = value
        .strip_prefix("http://")
        .or_else(|| value.strip_prefix("https://"));
    bounded(value, 2048)
        && authority.is_some_and(|v| {
            !v.is_empty() && !v.chars().any(|c| c.is_whitespace() || "/?#@\\".contains(c))
        })
}

fn valid_record(a: &BrowserAnnotation) -> bool {
    let geometry = &a.captured;
    let target = &a.target;
    let page = &a.page;
    host_id(&a.id, "ba-")
        && a.number > 0
        && !a.idempotency_key.is_empty()
        && bounded(&a.idempotency_key, 256)
        && !a.created_at.is_empty()
        && bounded(&a.created_at, 128)
        && bounded(&a.author, 256)
        && bounded(&a.text, MAX_NOTE_BYTES)
        && origin(&page.origin)
        && page.path.starts_with('/')
        && bounded(&page.path, 4096)
        && optional(page.query.as_deref(), 8192)
        && optional(page.hash_route.as_deref(), 4096)
        && optional(target.role.as_deref(), 256)
        && optional(target.name.as_deref(), 1024)
        && optional(target.text_excerpt.as_deref(), 2048)
        && optional(target.limitation.as_deref(), 2048)
        && (matches!(target.kind, TargetKind::Element) == a.locator.is_some())
        && a.locator.as_ref().is_none_or(|l| {
            !l.selector.is_empty()
                && bounded(&l.selector, 2048)
                && selectors(&l.shadow_hosts)
                && fingerprint(&l.fingerprint)
                && l.frames.len() <= MAX_CHAIN
                && l.frames.iter().all(|f| {
                    !f.selector.is_empty()
                        && bounded(&f.selector, 2048)
                        && selectors(&f.shadow_hosts)
                        && fingerprint(&f.fingerprint)
                })
        })
        && rect(&geometry.rect)
        && dimension(geometry.viewport.width)
        && dimension(geometry.viewport.height)
        && coordinate(geometry.scroll.x)
        && coordinate(geometry.scroll.y)
        && scale(geometry.device_scale)
        && scale(geometry.zoom)
        && a.dom_context
            .as_ref()
            .is_none_or(|v| serde_json::to_vec(v).is_ok_and(|bytes| bytes.len() <= MAX_DOM_BYTES))
        && resolution(&a.resolution)
        && match &a.image {
            ImageEvidence::None { .. } => true,
            ImageEvidence::Unavailable { reason, .. } => {
                !reason.is_empty() && bounded(reason, 2048)
            }
            ImageEvidence::Captured {
                asset_id, capture, ..
            } => {
                host_id(asset_id, "ba-asset-")
                    && rect(&capture.crop)
                    && capture.crop.x >= 0.0
                    && capture.crop.y >= 0.0
                    && rect(&capture.marker)
                    && scale(capture.scale)
                    && !capture.captured_at.is_empty()
                    && bounded(&capture.captured_at, 128)
            }
        }
}

pub(super) fn document(file: &BrowserAnnotationsFile) -> Result<(), BrowserAnnotationsError> {
    if file.version != BROWSER_ANNOTATIONS_VERSION {
        return Err(BrowserAnnotationsError::UnsupportedVersion(
            serde_json::json!(file.version),
        ));
    }
    let mut ids = HashSet::new();
    let mut numbers = HashSet::new();
    let mut keys = HashSet::new();
    let mut assets = HashSet::new();
    for annotation in &file.annotations {
        let unique_asset = match &annotation.image {
            ImageEvidence::Captured { asset_id, .. } => assets.insert(asset_id),
            _ => true,
        };
        if !valid_record(annotation)
            || !ids.insert(&annotation.id)
            || !numbers.insert(annotation.number)
            || !keys.insert(&annotation.idempotency_key)
            || !unique_asset
            || annotation.number >= file.next_number
        {
            return Err(BrowserAnnotationsError::InvalidRecord {
                id: annotation.id.clone(),
                reason: "invalid bounds, identity, duplicate, or numbering".into(),
            });
        }
    }
    if file.next_number == 0 {
        return Err(BrowserAnnotationsError::Malformed(
            "next_number must be positive".into(),
        ));
    }
    Ok(())
}
