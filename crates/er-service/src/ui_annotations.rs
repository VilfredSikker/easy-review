//! The browser annotations: `ui-annotations.json` in a tab's view bucket and
//! the screenshots beside it. The workspace is their one writer (ADR 0067).
//! Each write is a command whose effect takes the bucket's write lock, then
//! loads, changes and saves the file, and publishes the rows to every tab that
//! reads the bucket. The file is no store kind, so the lock is the workspace's
//! own and not the store's scope lock.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use er_engine::ai::{load_ui_annotations, save_ui_annotations, UiAnnotation};

use crate::actor::WorkspaceDeps;
use crate::annotations::Written;
use crate::content::ContentPatch;
use crate::handle::WorkspaceError;

/// A browser annotation as the page submits it.
#[derive(Clone, Debug, PartialEq)]
pub struct NewUiAnnotation {
    pub url: String,
    pub selector: Option<String>,
    pub bbox: [f64; 4],
    pub viewport: [u32; 2],
    pub text: String,
    pub screenshot_data_url: Option<String>,
    pub element_context: Option<String>,
    pub dom_context: Option<serde_json::Value>,
    pub id: Option<String>,
}

/// Whether an annotation's anchor still matched, and its new box.
#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
pub struct AnchorUpdate {
    pub id: String,
    pub fresh: bool,
    #[serde(default)]
    pub new_box: Option<[f64; 4]>,
}

/// One browser annotation write.
#[derive(Clone, Debug, PartialEq)]
pub enum UiAnnotationWrite {
    Add(Box<NewUiAnnotation>),
    /// A PNG data URL saved as `id`'s screenshot.
    Screenshot {
        id: String,
        data_url: String,
    },
    Delete {
        id: String,
    },
    /// The annotations on `page_url`, or every one with none, and their
    /// screenshots.
    Clear {
        page_url: Option<String>,
    },
    Anchors(Vec<AnchorUpdate>),
}

/// A bucket's rows as a tab publishes them. `seq` is the last write the rows
/// hold, so a load read before a write never replaces the rows it saved.
#[derive(Clone, Debug, Default)]
pub struct UiSection {
    pub revision: u64,
    pub seq: u64,
    pub rows: Arc<Vec<UiAnnotation>>,
}

/// `ui_annotations(tab)`'s version: the rows' revision, or the load that
/// chose the bucket. Both only rise, so a write or a move to another bucket
/// moves it.
pub fn ui_annotations_version(content: &crate::content::TabContent) -> u64 {
    content.ui.revision.max(content.mode_revision)
}

/// The rows a load or a write read, with the sequence they hold.
#[derive(Clone, Debug, Default)]
pub(crate) struct UiRows {
    pub rows: Vec<UiAnnotation>,
    pub seq: u64,
}

/// The last sequence a browser annotation write took, process-wide.
static UI_SEQ: AtomicU64 = AtomicU64::new(0);

/// One write lock per bucket, shared by the actors built from one
/// `WorkspaceDeps`.
#[derive(Clone, Default)]
pub(crate) struct BucketLocks(Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>);

impl BucketLocks {
    /// Runs `f` while holding `er_dir`'s lock.
    pub(crate) fn holding<T>(&self, er_dir: &str, f: impl FnOnce() -> T) -> T {
        let lock = {
            let mut locks = self.0.lock().unwrap_or_else(PoisonError::into_inner);
            Arc::clone(locks.entry(er_dir.to_string()).or_default())
        };
        let _held = lock.lock().unwrap_or_else(PoisonError::into_inner);
        f()
    }
}

/// The rows in `er_dir`, read under its lock so the sequence they carry
/// covers every write saved before them.
pub(crate) fn read(deps: &WorkspaceDeps, er_dir: &str) -> UiRows {
    deps.ui_locks().holding(er_dir, || UiRows {
        rows: load_ui_annotations(er_dir),
        seq: UI_SEQ.load(Ordering::SeqCst),
    })
}

/// Runs `write` in `er_dir` under its lock. Answers the id or path it
/// created, and the rows to publish.
pub(crate) fn write(
    deps: &WorkspaceDeps,
    er_dir: &str,
    write: UiAnnotationWrite,
) -> Written<Option<String>> {
    deps.ui_locks().holding(er_dir, || {
        let created = written(er_dir, write).map_err(WorkspaceError::Failed)?;
        let seq = UI_SEQ.fetch_add(1, Ordering::SeqCst) + 1;
        let patch = ContentPatch::Ui {
            er_dir: er_dir.to_string(),
            rows: load_ui_annotations(er_dir),
            seq,
        };
        Ok((created, patch))
    })
}

fn written(dir: &str, write: UiAnnotationWrite) -> Result<Option<String>, String> {
    match write {
        UiAnnotationWrite::Add(new) => write_ui_annotation(dir, *new).map(Some),
        UiAnnotationWrite::Screenshot { id, data_url } => {
            let bytes = decode_data_url_png(&data_url)
                .ok_or_else(|| "The screenshot is not a PNG data URL".to_string())?;
            save_annotation_screenshot_in(dir, &id, &bytes).map(|(path, _)| Some(path))
        }
        UiAnnotationWrite::Delete { id } => delete_ui_annotation_in(dir, &id).map(|()| None),
        UiAnnotationWrite::Clear { page_url } => {
            clear_ui_annotations_in(dir, page_url.as_deref()).map(|()| None)
        }
        UiAnnotationWrite::Anchors(updates) => apply_anchor_updates(dir, &updates)
            .map(|()| None)
            .map_err(|e| e.to_string()),
    }
}

/// Adds `new` to the annotations file in `dir`, its screenshot beside it.
/// Answers the row's id.
fn write_ui_annotation(dir: &str, new: NewUiAnnotation) -> Result<String, String> {
    let NewUiAnnotation {
        url,
        selector,
        bbox,
        viewport,
        text,
        screenshot_data_url,
        element_context,
        dom_context,
        id,
    } = new;
    let mut anns = load_ui_annotations(dir);
    let id = id.filter(|s| !s.is_empty()).unwrap_or_else(|| {
        format!(
            "ui-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        )
    });
    if !id.starts_with("ui-")
        || id.contains(['/', '\\'])
        || id.contains("..")
        || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err("invalid annotation id".to_string());
    }

    // If a screenshot data URL was provided, decode and persist it under
    // `<dir>/screenshots/<id>.png`. Failure to decode is non-fatal:
    // the annotation is still saved without a screenshot path.
    let screenshot_path = match screenshot_data_url.as_deref() {
        Some(data_url) => decode_data_url_png(data_url)
            .and_then(|bytes| save_screenshot_bytes(dir, &id, &bytes).ok()),
        None => None,
    };

    let ts = chrono_like_timestamp();
    anns.push(UiAnnotation {
        id: id.clone(),
        url,
        selector,
        box_x: bbox[0],
        box_y: bbox[1],
        box_w: bbox[2],
        box_h: bbox[3],
        viewport_w: viewport[0],
        viewport_h: viewport[1],
        text,
        timestamp: ts,
        author: "You".to_string(),
        screenshot_path,
        stale: false,
        element_context,
        dom_context,
    });
    save_ui_annotations(dir, &anns).map_err(|e| e.to_string())?;
    Ok(id)
}

fn delete_ui_annotation_in(dir: &str, id: &str) -> Result<(), String> {
    let mut anns = load_ui_annotations(dir);
    anns.retain(|a| a.id != id);
    save_ui_annotations(dir, &anns).map_err(|e| e.to_string())
}

/// Removes the annotations on `page_url`, or every one with none, and their
/// screenshots.
fn clear_ui_annotations_in(dir: &str, page_url: Option<&str>) -> Result<(), String> {
    let anns = load_ui_annotations(dir);
    let (removed, kept): (Vec<_>, Vec<_>) = anns
        .into_iter()
        .partition(|a| page_url.is_none_or(|url| a.url == url));
    for path in removed.iter().filter_map(|a| a.screenshot_path.as_deref()) {
        let _ = std::fs::remove_file(path);
    }
    save_ui_annotations(dir, &kept).map_err(|e| e.to_string())
}

/// Saves `bytes` as `id`'s screenshot in `dir` and points its row at it.
/// Answers the path, and whether a row took it.
fn save_annotation_screenshot_in(
    dir: &str,
    id: &str,
    bytes: &[u8],
) -> Result<(String, bool), String> {
    let path = save_screenshot_bytes(dir, id, bytes).map_err(|e| e.to_string())?;
    let mut anns = load_ui_annotations(dir);
    let Some(a) = anns.iter_mut().find(|a| a.id == id) else {
        return Ok((path, false));
    };
    a.screenshot_path = Some(path.clone());
    save_ui_annotations(dir, &anns).map_err(|e| e.to_string())?;
    Ok((path, true))
}

/// Decode a `data:image/png;base64,<payload>` URL into raw PNG bytes. Returns
/// `None` if the prefix is missing or base64 is malformed. We accept any
/// `data:image/*;base64,` MIME — the caller is trusted to produce PNG.
fn decode_data_url_png(data_url: &str) -> Option<Vec<u8>> {
    let (header, payload) = data_url.split_once(',')?;
    if !header.starts_with("data:image/") || !header.ends_with(";base64") {
        return None;
    }
    base64_decode(payload).ok()
}

/// Minimal standard-base64 decoder (RFC 4648). Skips whitespace, requires the
/// canonical alphabet. Avoids adding a base64 crate dep just for screenshots.
fn base64_decode(input: &str) -> Result<Vec<u8>, &'static str> {
    const fn val(b: u8) -> Result<u8, &'static str> {
        match b {
            b'A'..=b'Z' => Ok(b - b'A'),
            b'a'..=b'z' => Ok(b - b'a' + 26),
            b'0'..=b'9' => Ok(b - b'0' + 52),
            b'+' => Ok(62),
            b'/' => Ok(63),
            _ => Err("invalid base64 char"),
        }
    }
    let bytes: Vec<u8> = input.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    let mut stripped: &[u8] = &bytes;
    while stripped.last() == Some(&b'=') {
        stripped = &stripped[..stripped.len() - 1];
    }
    let mut out = Vec::with_capacity(stripped.len() * 3 / 4);
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;
    for &b in stripped {
        let v = u32::from(val(b)?);
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1u32 << bits) - 1;
        }
    }
    Ok(out)
}

/// Atomically save PNG bytes to `<comments_dir>/screenshots/<id>.png`. Returns
/// the absolute path written. Atomic: tmp file → rename.
fn save_screenshot_bytes(comments_dir: &str, id: &str, bytes: &[u8]) -> std::io::Result<String> {
    let screenshots_dir = format!("{comments_dir}/screenshots");
    let final_path = format!("{screenshots_dir}/{id}.png");
    er_store::atomic::write_atomic(std::path::Path::new(&final_path), bytes)?;
    // Best-effort canonicalize for absolute path; fall back to the raw join.
    let abs = std::fs::canonicalize(&final_path)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or(final_path);
    Ok(abs)
}

/// Reads a saved screenshot back as a `data:image/png;base64,<...>` URL so the
/// frontend can render it without configuring the Tauri asset protocol.
pub fn read_annotation_screenshot(path: String) -> Result<String, String> {
    let bytes = std::fs::read(&path).map_err(|e| format!("read {path}: {e}"))?;
    Ok(format!("data:image/png;base64,{}", base64_encode(&bytes)))
}

/// Minimal standard-base64 encoder. Mirrors `base64_decode`.
fn base64_encode(input: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    let chunks = input.chunks(3);
    for c in chunks {
        let b0 = c[0];
        let b1 = c.get(1).copied().unwrap_or(0);
        let b2 = c.get(2).copied().unwrap_or(0);
        out.push(A[(b0 >> 2) as usize] as char);
        out.push(A[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        if c.len() > 1 {
            out.push(A[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if c.len() > 2 {
            out.push(A[(b2 & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// Applies a list of anchor updates to the annotations file in `dir`.
fn apply_anchor_updates(dir: &str, updates: &[AnchorUpdate]) -> std::io::Result<()> {
    let mut anns = load_ui_annotations(dir);
    for upd in updates {
        if let Some(a) = anns.iter_mut().find(|a| a.id == upd.id) {
            a.stale = !upd.fresh;
            if let Some(b) = upd.new_box {
                a.box_x = b[0];
                a.box_y = b[1];
                a.box_w = b[2];
                a.box_h = b[3];
            }
        }
    }
    save_ui_annotations(dir, &anns)
}

/// Minimal RFC3339-ish timestamp. Avoids pulling chrono just for this.
fn chrono_like_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Best-effort UTC: seconds-since-epoch wrapped in a sortable string.
    format!("{secs}")
}

#[cfg(test)]
mod tests;
