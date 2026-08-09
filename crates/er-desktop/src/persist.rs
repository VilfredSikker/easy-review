//! Shared atomic JSON persistence for desktop-owned state files.
//!
//! Every desktop cache (tabs, projects, PR cache, GitHub-status cache,
//! open-diff cache, inbox) persists as pretty JSON via a tmp file + rename so
//! a crash mid-save can never truncate the real file. This helper is the
//! single implementation of that pattern — previously each writer re-inlined
//! it, and most silently swallowed failures. Returning the error lets callers
//! log with context (see the project rule: user-visible failures produce
//! durable `log::error!` entries).

use std::io;
use std::path::Path;

/// Atomically write `payload` as pretty JSON to `path` (tmp file + rename).
///
/// - Creates the parent directory if needed.
/// - On write or rename failure the tmp file is removed and the error is
///   returned, so callers can log with context. The previous contents at
///   `path` survive until the rename — a failed save never corrupts the cache.
pub(crate) fn save_json_atomic(path: &Path, payload: &impl serde::Serialize) -> io::Result<()> {
    let json = serde_json::to_string_pretty(payload).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("serialize payload: {e}"),
        )
    })?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    if let Err(e) = std::fs::write(&tmp, json) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(tmp);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    #[derive(Serialize)]
    struct Payload {
        version: u32,
        name: String,
    }

    #[test]
    fn round_trip_writes_readable_json_without_tmp_litter() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.json");
        save_json_atomic(
            &path,
            &Payload {
                version: 1,
                name: "er".into(),
            },
        )
        .unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        let back: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(back["version"], 1);
        assert_eq!(back["name"], "er");
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn creates_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a").join("b").join("cache.json");
        save_json_atomic(
            &path,
            &Payload {
                version: 2,
                name: "x".into(),
            },
        )
        .unwrap();
        assert!(path.exists());
    }

    #[test]
    fn failed_rename_returns_error_and_cleans_tmp() {
        let dir = tempfile::tempdir().unwrap();
        // A directory at `path` makes the final rename fail (file over dir).
        let path = dir.path().join("cache.json");
        std::fs::create_dir(&path).unwrap();
        let err = save_json_atomic(
            &path,
            &Payload {
                version: 3,
                name: "y".into(),
            },
        );
        assert!(err.is_err());
        // The tmp file is cleaned up and the directory is untouched.
        assert!(!path.with_extension("json.tmp").exists());
        assert!(path.is_dir());
    }

    #[test]
    fn failed_parent_creation_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        // `blocker` is a file, so `blocker/cache.json`'s parent cannot exist.
        let blocker = dir.path().join("blocker");
        std::fs::write(&blocker, "file").unwrap();
        let path = blocker.join("cache.json");
        let err = save_json_atomic(
            &path,
            &Payload {
                version: 4,
                name: "z".into(),
            },
        );
        assert!(err.is_err());
        assert!(!path.with_extension("json.tmp").exists());
    }
}
