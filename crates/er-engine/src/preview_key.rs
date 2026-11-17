//! The keys a document preview is checked against, and the stamps they read.
//!
//! A front end paints a preview with the keys it was read under and drops it
//! when the tab's keys move. The keys are pure over their inputs, so the App's
//! tab and the service's published content give the same key for the same
//! diff. The stamps are the inputs that come from outside the diff: a file's
//! checkout mtime and a renamed file's blob.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::time::SystemTime;

use crate::git::{DiffFile, DiffFileHeader, FileStatus};
use crate::tab_identity::DiffMode;

pub fn supports_preview(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| {
            matches!(
                e.to_ascii_lowercase().as_str(),
                "md" | "markdown" | "mdown" | "mkd" | "mkdn" | "txt" | "text"
            )
        })
}

/// The MIME type an image file is rendered as, or `None` when it is not an image.
pub fn image_mime(path: &str) -> Option<&'static str> {
    let extension = Path::new(path).extension()?.to_str()?.to_ascii_lowercase();
    Some(match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "svg" => "image/svg+xml",
        _ => return None,
    })
}

pub fn has_preview(path: &str) -> bool {
    supports_preview(path) || image_mime(path).is_some()
}

fn fingerprint(value: impl Hash) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut h);
    format!("{:016x}", h.finish())
}

/// What a diff's keys read beside the diff itself.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PreviewStamps {
    /// Each file's mtime in the checkout, the epoch when it cannot be read.
    pub mtimes: HashMap<String, SystemTime>,
    /// The blob a renamed previewable file has at the reviewed revision.
    pub blob_ids: HashMap<String, String>,
}

/// Where the stamps of a diff are read from.
pub struct StampSource<'a> {
    pub repo_root: &'a str,
    /// The working tree a checked-out branch view reads instead of the root.
    pub checkout_root: Option<&'a str>,
    /// A remote tab has no checkout, so no blob is read.
    pub is_remote: bool,
    pub mode: DiffMode,
    /// The staged view of a commit made in it, which reads its blobs at HEAD.
    pub committed_unpushed: bool,
    /// The branch or PR head a renamed file is read at; HEAD when `None`.
    pub revision: Option<&'a str>,
}

/// Reads the stamps of `files` from the checkout and from git.
pub fn capture_stamps<'f>(
    git: &dyn er_git::Git,
    files: impl IntoIterator<Item = &'f DiffFile>,
    source: &StampSource<'_>,
) -> PreviewStamps {
    let root = source.checkout_root.unwrap_or(source.repo_root);
    let mut stamps = PreviewStamps::default();
    for file in files {
        let mtime = std::fs::metadata(format!("{}/{}", root, file.path))
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        stamps.mtimes.insert(file.path.clone(), mtime);
        if source.is_remote || !has_preview(&file.path) {
            continue;
        }
        let FileStatus::Renamed(old_path) = &file.status else {
            continue;
        };
        let spec = if source.mode == DiffMode::Staged && !source.committed_unpushed {
            format!("HEAD:{old_path}")
        } else {
            format!("{}:{}", source.revision.unwrap_or("HEAD"), file.path)
        };
        if let Ok(oid) = git.blob_oid(root, &spec) {
            stamps.blob_ids.insert(file.path.clone(), oid);
        }
    }
    stamps
}

/// What both keys read: the tab's view and its diff.
pub struct PreviewKeyInputs<'a> {
    pub repo_root: &'a str,
    pub remote_repo: Option<&'a str>,
    pub mode: DiffMode,
    pub checkout_root: Option<&'a str>,
    pub preview_head_oid: Option<&'a str>,
    /// The commit a History view shows; `None` in every other view.
    pub history_commit: Option<&'a str>,
    pub raw: Option<&'a str>,
    pub diff_hash: &'a str,
    pub headers: &'a [DiffFileHeader],
    pub mtimes: &'a HashMap<String, SystemTime>,
    pub blob_ids: &'a HashMap<String, String>,
}

impl<'a> PreviewKeyInputs<'a> {
    fn history(&self) -> bool {
        self.mode == DiffMode::History
    }

    /// The file's section of the raw diff.
    pub fn section(&self, path: &str) -> Option<&'a str> {
        let raw = self.raw?;
        let h = self.headers.iter().find(|h| h.path == path)?;
        raw.get(h.byte_offset..h.byte_offset + h.byte_length)
    }
}

/// The key of the whole view: any change to the diff, its revision or a
/// renamed file's blob moves it.
pub fn preview_context_key(inputs: &PreviewKeyInputs<'_>) -> String {
    let history = inputs.history();
    let mut blobs: Vec<_> = if history {
        Vec::new()
    } else {
        inputs.blob_ids.iter().collect()
    };
    blobs.sort_by_key(|(path, _)| *path);
    fingerprint((
        inputs.repo_root,
        inputs.remote_repo,
        inputs.mode.git_mode(),
        inputs.checkout_root,
        inputs.preview_head_oid,
        inputs.history_commit,
        if history { None } else { inputs.raw },
        if history { "" } else { inputs.diff_hash },
        blobs,
    ))
}

/// The key of one file's preview, empty for a file with no preview.
pub fn file_preview_key(inputs: &PreviewKeyInputs<'_>, path: &str) -> String {
    if !has_preview(path) {
        return String::new();
    }
    let history = inputs.history();
    fingerprint((
        inputs.repo_root,
        inputs.remote_repo,
        inputs.mode.git_mode(),
        inputs.preview_head_oid,
        inputs.checkout_root,
        inputs.history_commit,
        path,
        if history {
            None
        } else {
            inputs.blob_ids.get(path)
        },
        if history { None } else { inputs.section(path) },
        if history {
            None
        } else {
            inputs.mtimes.get(path)
        },
    ))
}
