//! The one-time pre-1.0 project-list read (owner decision D15).
//!
//! This is the only place 1.0.0 reads a file from the old install. It lives
//! outside `projects_pins` so the artifact-store rule there stays absolute:
//! the old file is foreign, never written or migrated, and this module only
//! reads it and drops a notice marker in the new root.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::projects_pins::ProjectsFile;

/// Empty file under the storage root: the first-launch notice says the project
/// list was imported when it exists. The store record is the once-gate.
pub(crate) const IMPORTED_MARKER: &str = ".legacy-projects-imported";

/// The last old path whose read or parse failed, so one corrupt file warns
/// once per process instead of on every load.
static FAILED_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

/// The config directory the pre-1.0 build kept projects under, when overridden
/// for this thread (tests) or by the environment.
fn config_dir_override() -> Option<String> {
    #[cfg(any(test, feature = "test-support"))]
    {
        crate::test_support::var("ER_LEGACY_CONFIG_DIR")
    }
    #[cfg(not(any(test, feature = "test-support")))]
    {
        std::env::var("ER_LEGACY_CONFIG_DIR").ok()
    }
}

/// The pre-1.0 project list: `<config_dir>/er/projects.json`.
fn legacy_projects_path() -> Option<PathBuf> {
    if let Some(dir) = config_dir_override().filter(|dir| !dir.is_empty()) {
        return Some(PathBuf::from(dir).join("er").join("projects.json"));
    }
    // A test build never touches the real config dir: an escaped test must not
    // import a developer's project list or spend the one-time import. A test
    // that wants an old file sets the override.
    #[cfg(any(test, feature = "test-support"))]
    {
        None
    }
    #[cfg(not(any(test, feature = "test-support")))]
    {
        dirs::config_dir().map(|dir| dir.join("er").join("projects.json"))
    }
}

/// Remember a failed read or parse so the warning appears once.
fn remember_failure(path: &Path) {
    if let Ok(mut failed) = FAILED_PATH.lock() {
        *failed = Some(path.to_path_buf());
    }
}

fn failed_already(path: &Path) -> bool {
    FAILED_PATH
        .lock()
        .is_ok_and(|failed| failed.as_deref() == Some(path))
}

/// The old project list, or `None` when it is missing or unparseable.
///
/// A read or parse failure logs one warning naming the path and is remembered
/// for the process; the caller starts with an empty list. The file is never
/// modified, moved or deleted.
pub(crate) fn read_legacy_projects() -> Option<ProjectsFile> {
    let path = legacy_projects_path()?;
    if failed_already(&path) {
        return None;
    }
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            remember_failure(&path);
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
            remember_failure(&path);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_test_build_never_resolves_the_real_config_dir() {
        assert!(config_dir_override().is_none());
        assert!(legacy_projects_path().is_none());
        assert!(read_legacy_projects().is_none());
    }

    #[test]
    fn a_failed_read_is_remembered_for_the_process() {
        let config = tempfile::tempdir().unwrap();
        let dir = config.path().join("er");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("projects.json");
        std::fs::write(&path, "{ not json").unwrap();

        crate::test_support::with_var(
            "ER_LEGACY_CONFIG_DIR",
            config.path().to_string_lossy().into_owned(),
            || {
                assert!(read_legacy_projects().is_none());
                // Fixing the file does not make this process read it again.
                std::fs::write(&path, r#"{"projects":[],"active_id":null}"#).unwrap();
                assert!(read_legacy_projects().is_none());
            },
        );
    }
}
