//! Raw access to the files beside the scoped overlays that the store does not
//! own.
//!
//! `*.prev.json` and `review-files.txt` are not registered kinds, so the stamp
//! and the full-run clear delete them directly, and the unchanged check
//! compares the agent's bytes. Keeping that raw access in one helper lets the
//! overlay module itself stay on the sidecar-fs deny list.

use std::path::Path;

/// The file's bytes; empty when it cannot be read, matching the unchanged
/// check's "nothing to compare" answer.
pub(crate) fn read_bytes(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_default()
}

/// Best-effort delete: absent is the expected case for a lens that never ran.
pub(crate) fn remove(path: &Path) {
    let _ = std::fs::remove_file(path);
}
