//! The one-time pre-1.0 project-list read (owner decision D15).
//!
//! This is the only place 1.0.0 reads a file from the old install. It lives
//! outside `projects_pins` so the artifact-store rule there stays absolute:
//! the old file is foreign, never written or migrated, and this module only
//! reads it and drops a notice marker in the new root.

use std::path::PathBuf;

use crate::projects_pins::ProjectsFile;

/// Empty file under the storage root: the first-launch notice says the project
/// list was imported when it exists. The store record is the once-gate.
pub(crate) const IMPORTED_MARKER: &str = ".legacy-projects-imported";

/// The config directory the pre-1.0 build kept projects under, when overridden
/// for this thread (tests) or by the environment.
fn config_dir_override() -> Option<String> {
    #[cfg(feature = "test-support")]
    {
        crate::test_support::var("ER_LEGACY_CONFIG_DIR")
    }
    #[cfg(not(feature = "test-support"))]
    {
        std::env::var("ER_LEGACY_CONFIG_DIR").ok()
    }
}

/// The pre-1.0 project list: `<config_dir>/er/projects.json`.
fn legacy_projects_path() -> Option<PathBuf> {
    match config_dir_override().filter(|dir| !dir.is_empty()) {
        Some(dir) => Some(PathBuf::from(dir).join("er").join("projects.json")),
        None => dirs::config_dir().map(|dir| dir.join("er").join("projects.json")),
    }
}

/// The old project list, or `None` when it is missing or unparseable.
///
/// A read or parse failure logs one warning naming the path; the caller starts
/// with an empty list. The file is never modified, moved or deleted.
pub(crate) fn read_legacy_projects() -> Option<ProjectsFile> {
    let path = legacy_projects_path()?;
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            log::warn!(
                "cannot read the old project list at {}: {error}",
                path.display()
            );
            return None;
        }
    };
    match serde_json::from_str(&text) {
        Ok(file) => Some(file),
        Err(error) => {
            log::warn!(
                "cannot parse the old project list at {}: {error}",
                path.display()
            );
            None
        }
    }
}

/// Leave the marker the first-launch notice reads. A failed write only means
/// the notice omits the sentence.
pub(crate) fn mark_imported() {
    let root = crate::storage::storage_root();
    let _ = std::fs::create_dir_all(&root);
    let _ = std::fs::write(root.join(IMPORTED_MARKER), b"");
}
