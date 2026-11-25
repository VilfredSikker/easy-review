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

/// Get the mtime of the most recently modified .er-* file
pub fn latest_er_mtime(er_dir: &str) -> Option<std::time::SystemTime> {
    latest_er_mtime_skipping(er_dir, None)
}

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

pub fn latest_er_mtime_skipping(
    er_dir: &str,
    skip: Option<&Path>,
) -> Option<std::time::SystemTime> {
    let latest = er_mtimes(er_dir)
        .into_iter()
        .filter(|(path, _)| skip.is_none_or(|s| s != path))
        .map(|(_, mtime)| mtime)
        .max();

    // `None` sorts below every `Some`, so the newer of the two wins.
    latest.max(crate::arena::latest_arena_mtime(Path::new(er_dir)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_er_mtime_sees_notes_json() {
        // An agent that edits notes.json outside er (resolving a note it acted
        // on) must trigger the reload poll, or the desktop keeps showing it.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("notes.json"), r#"{"version":1,"notes":[]}"#).unwrap();
        assert!(latest_er_mtime(dir.path().to_str().unwrap()).is_some());
    }

    #[test]
    fn an_arena_file_counts_toward_the_reload_poll() {
        // The poll sees an arena run's progress in every build, the desktop's
        // included; the gate that limited it to the App's build is gone.
        let dir = tempfile::tempdir().unwrap();
        let run = dir.path().join("arena").join("run-1");
        std::fs::create_dir_all(&run).unwrap();
        std::fs::write(run.join("state.json"), "{}").unwrap();
        assert!(latest_er_mtime(dir.path().to_str().unwrap()).is_some());
    }

    #[test]
    fn latest_er_mtime_ignores_feedback_json() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("feedback.json"),
            r#"{"version":1,"diff_hash":"h","comments":[]}"#,
        )
        .unwrap();
        assert!(latest_er_mtime(dir.path().to_str().unwrap()).is_none());
    }
}
