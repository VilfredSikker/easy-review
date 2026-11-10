//! Sidecar mtime polling for the reload watchers.
//!
//! This is a watch over the bucket's files, not a read of their contents, so it
//! stays a filesystem stat until the store owns change events everywhere.

use std::path::Path;

/// Get the mtime of the most recently modified .er-* file
pub fn latest_er_mtime(er_dir: &str) -> Option<std::time::SystemTime> {
    latest_er_mtime_skipping(er_dir, None)
}

pub fn latest_er_mtime_skipping(
    er_dir: &str,
    skip: Option<&Path>,
) -> Option<std::time::SystemTime> {
    let er_dir = Path::new(er_dir);
    let files = [
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

    let skip_eq = |path: &Path| skip.is_some_and(|s| s == path);

    let mut latest = files
        .iter()
        .filter_map(|name| {
            let path = er_dir.join(name);
            if skip_eq(&path) {
                return None;
            }
            std::fs::metadata(&path).ok()?.modified().ok()
        })
        .max();

    let experts_dir = er_dir.join("experts");
    if let Ok(entries) = std::fs::read_dir(&experts_dir) {
        for entry in entries.flatten() {
            if let Ok(m) = entry.metadata().and_then(|m| m.modified()) {
                latest = Some(match latest {
                    Some(prev) => prev.max(m),
                    None => m,
                });
            }
        }
    }

    let diagrams_dir = er_dir.join("diagrams");
    if let Ok(entries) = std::fs::read_dir(&diagrams_dir) {
        for entry in entries.flatten() {
            if let Ok(m) = entry.metadata().and_then(|m| m.modified()) {
                latest = Some(match latest {
                    Some(prev) => prev.max(m),
                    None => m,
                });
            }
        }
    }

    #[cfg(feature = "ui")]
    if let Some(arena_mtime) = crate::arena::latest_arena_mtime(er_dir) {
        latest = Some(match latest {
            Some(prev) => prev.max(arena_mtime),
            None => arena_mtime,
        });
    }

    latest
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
