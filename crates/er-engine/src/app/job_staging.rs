//! Per-job staging: an agent writes here, and the host validates and commits.
//!
//! A job never writes the review bucket directly. Its prompt is re-pointed at
//! this directory, the inputs it reads are copied in, and after the run the
//! host parses the staged files and commits them through `ArtifactStore`. The
//! directory is removed when the job ends, however it ends.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

/// Inputs a prepared-diff prompt reads. Copied into staging before the run so
/// the agent never needs a path into the bucket. `.diff-tmp.sha256` is the
/// printed hash some providers read instead of hashing the file themselves.
const DIFF_INPUTS: &[&str] = &[
    "diff-tmp",
    "diff-annotated",
    "review-files.txt",
    ".diff-tmp.sha256",
];

/// How long an abandoned staging directory is kept before the startup sweep
/// removes it. A running job's directory is newer than this.
const STALE_AFTER: Duration = Duration::from_hours(24);

#[derive(Debug)]
pub struct JobStaging {
    root: PathBuf,
}

impl JobStaging {
    /// Create a staging directory under managed storage.
    ///
    /// The first call also starts the stale-directory sweep on its own thread,
    /// so a large leftover set never runs on the caller's (UI) thread.
    pub fn create() -> std::io::Result<Self> {
        static SWEPT: std::sync::OnceLock<()> = std::sync::OnceLock::new();
        let staging_root = crate::storage::storage_root().join("staging");
        // Capture the directory now: the sweep outlives the caller, and a
        // test storage root is only current while the test holds it.
        SWEPT.get_or_init(|| {
            let staging_root = staging_root.clone();
            std::thread::spawn(move || Self::sweep_stale_at(&staging_root));
        });
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let now = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_millis());
        let root = staging_root.join(format!("{}-{now}-{sequence}", std::process::id()));
        std::fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    /// The directory the prompt and `--add-dir` point at.
    pub fn path(&self) -> String {
        self.root.to_string_lossy().into_owned()
    }

    /// Re-point a prompt built against `er_dir` at this staging directory.
    ///
    /// Prompts interpolate both a shell-quoted and a raw form of the directory,
    /// so both are replaced. The quoted form keeps its quoting with the staging
    /// path, which may contain spaces under `~/Library/Application Support`.
    pub fn repoint_prompt(&self, prompt: &str, er_dir: &str) -> String {
        let from = crate::ai::prompts::sanitize_for_shell(er_dir);
        let to = crate::ai::prompts::sanitize_for_shell(&self.path());
        prompt.replace(&from, &to).replace(er_dir, &self.path())
    }

    /// Copy the inputs a prompt reads into staging, preserving relative paths.
    pub fn stage_inputs(&self, er_dir: &str) -> std::io::Result<()> {
        let er_dir = Path::new(er_dir);
        for name in DIFF_INPUTS {
            let source = er_dir.join(name);
            if source.is_file() {
                std::fs::copy(&source, self.root.join(name))?;
            }
        }
        let tour_inputs = er_dir.join("tour-input");
        if tour_inputs.is_dir() {
            copy_dir(&tour_inputs, &self.root.join("tour-input"))?;
        }
        Ok(())
    }

    /// Copy the files an update-kind job reads and rewrites into staging twice:
    /// the visible path the prompt reads, and `.base/` for the merge's
    /// launch-time copy. A missing source is not an error: the job may be
    /// creating the file for the first time.
    pub fn stage_update_inputs(&self, er_dir: &str, files: &[&str]) -> std::io::Result<()> {
        for name in files {
            let source = Path::new(er_dir).join(name);
            if !source.is_file() {
                continue;
            }
            std::fs::copy(&source, self.root.join(name))?;
            let base = self.root.join(".base").join(name);
            if let Some(parent) = base.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&source, base)?;
        }
        Ok(())
    }

    /// Remove stale staging directories left by a crash. Runs off the UI
    /// thread; a failure to read one entry must not stop the sweep.
    pub fn sweep_stale() {
        Self::sweep_stale_at(&crate::storage::storage_root().join("staging"));
    }

    fn sweep_stale_at(dir: &Path) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let now = SystemTime::now();
        for entry in entries.flatten() {
            let path = entry.path();
            let stale = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(|modified| now.duration_since(modified).ok())
                .is_some_and(|age| age > STALE_AFTER);
            if stale {
                let _ = std::fs::remove_dir_all(path);
            }
        }
    }
}

impl Drop for JobStaging {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sweep_removes_only_stale_staging_dirs() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let staging = crate::storage::storage_root().join("staging");
            let old = staging.join("old");
            let fresh = staging.join("fresh");
            std::fs::create_dir_all(&old).unwrap();
            std::fs::create_dir_all(&fresh).unwrap();
            let aged = SystemTime::now() - STALE_AFTER - Duration::from_secs(60);
            std::fs::File::open(&old)
                .unwrap()
                .set_times(std::fs::FileTimes::new().set_modified(aged))
                .unwrap();

            JobStaging::sweep_stale();

            assert!(!old.exists(), "the stale directory survived the sweep");
            assert!(fresh.exists(), "the sweep removed a fresh directory");
        });
    }
}
