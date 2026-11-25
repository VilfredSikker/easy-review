//! Sidecar mtime polling for the reload watchers.
//!
//! This is a watch over the bucket's files, not a read of their contents, so it
//! stays a filesystem stat until the store owns change events everywhere.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The sidecars a bucket's AI state is read from.
const SIDECARS: [&str; 10] = [
    "review.json",
    "order.json",
    "tour.json",
    "summary.md",
    "checklist.json",
    "questions.json",
    "notes.json",
    "github-comments.json",
    "triage.json",
    "professor.json",
];

/// Each sidecar of `er_dir` that exists, with its mtime: the files above and
/// every entry of the per-expert and diagram directories.
pub fn er_mtimes(er_dir: &str) -> Vec<(PathBuf, SystemTime)> {
    let er_dir = Path::new(er_dir);
    let mtime = |path: PathBuf| {
        Some((
            path.clone(),
            std::fs::metadata(&path).ok()?.modified().ok()?,
        ))
    };
    let mut found: Vec<_> = SIDECARS
        .iter()
        .filter_map(|name| mtime(er_dir.join(name)))
        .collect();
    for sub in ["experts", "diagrams"] {
        if let Ok(entries) = std::fs::read_dir(er_dir.join(sub)) {
            found.extend(entries.flatten().filter_map(|entry| mtime(entry.path())));
        }
    }
    found
}
