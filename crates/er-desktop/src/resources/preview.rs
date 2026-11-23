//! The document and image previews and the diff's path list on the flag
//! path, read from the content a tab's workspace published. A preview checks
//! the page's context key against that content before and after the read, so
//! a view that moved under it is refused and the App is never read.

use std::sync::Arc;

use er_engine::git::FileStatus;
use er_engine::preview_key::{file_preview_key, preview_context_key};
use er_engine::preview_read::{
    capture_file_preview, capture_image_preview, FilePreviewRequest, PreviewSource,
    MAX_IMAGE_PREVIEW_BYTES,
};
use er_protocol::{DiffPaths, ErError, ErErrorKind, FilePreview, ImagePreview, Resource};
use er_service::{TabContent, TabEntry};

use crate::state::DesktopState;

const DOCUMENT_MOVED: &str = "Document preview context changed";
const IMAGE_MOVED: &str = "Image preview context changed";

/// `path`'s text in `wire`'s view, read when the view is still the one at
/// `context`. Version 0: read on each fetch, never pushed.
pub fn file_preview(
    desktop: &DesktopState,
    wire: er_protocol::TabKey,
    path: &str,
    context: &str,
) -> Result<(u64, Resource), ErError> {
    let entry = super::diff::listed(desktop, wire)?;
    document(desktop, &entry, path, context).map(|preview| (0, Resource::FilePreview(preview)))
}

/// Both sides of the image at `path` in `wire`'s view, as `file_preview`.
pub fn image_preview(
    desktop: &DesktopState,
    wire: er_protocol::TabKey,
    path: &str,
    context: &str,
) -> Result<(u64, Resource), ErError> {
    let entry = super::diff::listed(desktop, wire)?;
    image(desktop, &entry, path, context).map(|preview| (0, Resource::ImagePreview(preview)))
}

/// Every path `wire`'s diff holds, in whichever view it shows.
pub fn diff_paths(
    desktop: &DesktopState,
    wire: er_protocol::TabKey,
) -> Result<(u64, Resource), ErError> {
    let entry = super::diff::listed(desktop, wire)?;
    let content = loaded(desktop, &entry)?;
    let paths = content.diff.files.iter().map(|f| f.path.clone()).collect();
    Ok((0, Resource::DiffPaths(DiffPaths { paths })))
}

pub fn document(
    desktop: &DesktopState,
    entry: &TabEntry,
    path: &str,
    context: &str,
) -> Result<FilePreview, ErError> {
    let request = captured(
        desktop,
        entry,
        path,
        context,
        DOCUMENT_MOVED,
        |source, status| capture_file_preview(source, path, status),
    )?;
    let text = request.read(&*desktop.ports.git).map_err(failed)?;
    unmoved(desktop, entry, &request, DOCUMENT_MOVED)?;
    Ok(FilePreview {
        path: path.to_string(),
        text,
        preview_context_key: request.preview_context_key,
        preview_key: request.preview_key,
    })
}

pub fn image(
    desktop: &DesktopState,
    entry: &TabEntry,
    path: &str,
    context: &str,
) -> Result<ImagePreview, ErError> {
    let mime = er_engine::preview_key::image_mime(path)
        .ok_or_else(|| failed("This file is not an image"))?;
    let request = captured(
        desktop,
        entry,
        path,
        context,
        IMAGE_MOVED,
        |source, status| capture_image_preview(source, path, status),
    )?;
    let read = |side: &Option<FilePreviewRequest>| {
        side.as_ref()
            .map(|r| r.read_bytes(&*desktop.ports.git, MAX_IMAGE_PREVIEW_BYTES))
            .transpose()
    };
    let after = read(&request.after).map_err(failed)?;
    // The earlier side is context. A failed read leaves the current side alone.
    let before = match read(&request.before) {
        Ok(bytes) => bytes,
        Err(_) if after.is_some() => None,
        Err(e) => return Err(failed(e)),
    };
    let keys = request.after.as_ref().or(request.before.as_ref());
    let keys = keys.ok_or_else(|| failed("Image has no side to show"))?;
    unmoved(desktop, entry, keys, IMAGE_MOVED)?;
    Ok(ImagePreview {
        path: path.to_string(),
        before: before.map(|b| crate::route_helpers::image_data_url(mime, &b)),
        after: after.map(|b| crate::route_helpers::image_data_url(mime, &b)),
        preview_context_key: keys.preview_context_key.clone(),
        preview_key: keys.preview_key.clone(),
    })
}

/// Captures a read in the tab's view, when it is still the one at `context`.
fn captured<T>(
    desktop: &DesktopState,
    entry: &TabEntry,
    path: &str,
    context: &str,
    moved: &str,
    capture: impl FnOnce(&PreviewSource<'_>, Option<FileStatus>) -> anyhow::Result<T>,
) -> Result<T, ErError> {
    let content = loaded(desktop, entry)?;
    let keys = content.preview_key_inputs();
    if preview_context_key(&keys) != context {
        return Err(stale(moved));
    }
    let status = content.diff.files.iter().find(|f| f.path == path);
    let source = PreviewSource {
        keys,
        is_remote: content.tab.identity().is_remote(),
        pr_number: content.tab.pr_number,
        branch_view: content.tab.local_branch_view.is_some(),
        tour_is_pr: content.tab.tour_is_pr,
        github: Arc::clone(&desktop.ports.github),
    };
    capture(&source, status.map(|f| f.status.clone())).map_err(failed)
}

/// Refuses a read whose view moved while the file was read.
fn unmoved(
    desktop: &DesktopState,
    entry: &TabEntry,
    request: &FilePreviewRequest,
    moved: &str,
) -> Result<(), ErError> {
    let content = loaded(desktop, entry)?;
    let keys = content.preview_key_inputs();
    if preview_context_key(&keys) != request.preview_context_key
        || file_preview_key(&keys, &request.path) != request.preview_key
    {
        return Err(stale(moved));
    }
    Ok(())
}

fn loaded(desktop: &DesktopState, entry: &TabEntry) -> Result<Arc<TabContent>, ErError> {
    desktop
        .service
        .content(entry.key, &entry.descriptor)
        .ok_or_else(|| stale("the tab's diff is not loaded yet"))
}

fn stale(message: &str) -> ErError {
    ErError {
        kind: ErErrorKind::Stale,
        message: message.to_string(),
    }
}

fn refused(message: String) -> ErError {
    ErError {
        kind: ErErrorKind::Refused,
        message,
    }
}

fn failed(error: impl std::fmt::Display) -> ErError {
    refused(error.to_string())
}
