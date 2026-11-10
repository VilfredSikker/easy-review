//! Raw sidecar reads for the files the store does not own yet.
//!
//! The store covers every registered kind; the agent-input files that remain
//! direct reads (`diff-tmp`, arena seeds and similar) share this size guard
//! until phase 3.

use std::path::Path;

pub(crate) const MAX_SIDECAR_BYTES: u64 = 10_000_000;

/// Read a sidecar file with a size limit to prevent memory spikes from large/adversarial files.
pub(crate) fn read_sidecar(path: &Path) -> std::io::Result<String> {
    let metadata = std::fs::metadata(path)?;
    if metadata.len() > MAX_SIDECAR_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "Sidecar file too large: {} bytes (limit {})",
                metadata.len(),
                MAX_SIDECAR_BYTES
            ),
        ));
    }
    std::fs::read_to_string(path)
}
