//! Import skill-written `.er/` sidecars into managed storage.
//!
//! `/er-*` skills run inside the user's Claude Code sandbox, which can write the
//! repo `.er/` directory but **not** the managed storage root (it lives outside
//! the repo, e.g. `~/Library/Application Support/easy-review` on macOS). The
//! desktop "Generate …" buttons sidestep this by spawning an *unsandboxed* agent
//! that writes managed storage directly; a hand-run `/er-tour` cannot, so it
//! falls back to `.er/` and never surfaces in the app.
//!
//! `er` itself runs unsandboxed, so it can copy those `.er/` sidecars into
//! managed storage on demand. This module is that bridge: detect what is pending,
//! and copy it into the same bucket the app reads it from (`tour.json` is
//! branch-scoped; everything else lives in the active view bucket). Imports never
//! overwrite a differing managed copy without the caller passing `overwrite`.

use super::TabState;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// AI-generated sidecars a skill may write to `.er/`. The app-owned files
/// (`questions.json`, `notes.json`, `github-comments.json`, `reviewed`,
/// `session.json`) are intentionally excluded — `er` writes those to managed
/// storage itself, so importing them from `.er/` would clobber live state.
pub const IMPORTABLE_SIDECARS: &[&str] = &[
    "review.json",
    "order.json",
    "tour.json",
    "summary.md",
    "checklist.json",
    "triage.json",
    "professor.json",
    "quiz.json",
    "quiz-feedback.json",
];

/// One `.er/` sidecar whose import would change managed storage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErImportItem {
    pub name: String,
    /// A managed copy already exists, so importing would overwrite it.
    pub managed_exists: bool,
    /// The `.er/` and managed copies differ in content (only set when `managed_exists`).
    pub differs: bool,
    /// The `.er/` copy is at least as new as the managed copy (by mtime).
    pub er_newer: bool,
    /// `.er/` copy mtime, Unix-epoch seconds — lets the UI say which is newest.
    pub er_mtime: Option<u64>,
    /// Managed copy mtime, Unix-epoch seconds.
    pub managed_mtime: Option<u64>,
}

/// Result of attempting to import a single sidecar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErImportOutcome {
    /// Copied into managed storage (new file or confirmed overwrite).
    Imported,
    /// A differing managed copy exists and `overwrite` was not set — needs confirmation.
    Conflict,
    /// Nothing to do (source missing, same directory, or already identical).
    Skipped,
}

/// Outcome for one sidecar plus the timing info the UI needs for a confirm prompt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErImportItemResult {
    pub name: String,
    pub outcome: ErImportOutcome,
    pub er_newer: bool,
    pub er_mtime: Option<u64>,
    pub managed_mtime: Option<u64>,
}

fn mtime_secs(p: &Path) -> Option<u64> {
    std::fs::metadata(p)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

/// Byte comparison. Treats unreadable files as differing so we never claim two
/// files match when we couldn't actually check.
fn files_differ(a: &Path, b: &Path) -> bool {
    match (std::fs::read(a), std::fs::read(b)) {
        (Ok(x), Ok(y)) => x != y,
        _ => true,
    }
}

/// Inspect a single sidecar. Returns `Some` only when importing it would change
/// managed storage (managed copy missing, or present but differing). Returns
/// `None` when there is nothing worth importing (source missing or already in sync).
pub fn inspect_sidecar(name: &str, src: &Path, dst: &Path) -> Option<ErImportItem> {
    if !src.is_file() {
        return None;
    }
    let managed_exists = dst.is_file();
    let differs = !managed_exists || files_differ(src, dst);
    if managed_exists && !differs {
        return None; // already in sync — nothing to import
    }
    let er_mtime = mtime_secs(src);
    let managed_mtime = if managed_exists { mtime_secs(dst) } else { None };
    let er_newer = match (er_mtime, managed_mtime) {
        (Some(a), Some(b)) => a >= b,
        _ => true,
    };
    Some(ErImportItem {
        name: name.to_string(),
        managed_exists,
        differs,
        er_newer,
        er_mtime,
        managed_mtime,
    })
}

/// Copy `src` → `dst` atomically (tmp + rename). Refuses to overwrite a differing
/// managed copy unless `overwrite` is set, returning [`ErImportOutcome::Conflict`].
pub fn import_file(src: &Path, dst: &Path, overwrite: bool) -> Result<ErImportOutcome> {
    if !src.is_file() {
        return Ok(ErImportOutcome::Skipped);
    }
    if dst.is_file() {
        if !files_differ(src, dst) {
            return Ok(ErImportOutcome::Skipped); // identical — no-op
        }
        if !overwrite {
            return Ok(ErImportOutcome::Conflict);
        }
    }
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating managed dir {}", parent.display()))?;
    }
    let tmp = dst.with_extension("import-tmp");
    std::fs::copy(src, &tmp).with_context(|| format!("copying {} → {}", src.display(), tmp.display()))?;
    std::fs::rename(&tmp, dst)
        .with_context(|| format!("finalizing import to {}", dst.display()))?;
    Ok(ErImportOutcome::Imported)
}

impl TabState {
    /// Resolve `(src, dst)` paths for a sidecar: source is always repo `.er/<name>`;
    /// destination is the managed bucket the app reads that file from (`tour.json`
    /// is branch-scoped, everything else lives in the active view bucket). Returns
    /// `None` when source and destination resolve to the same directory
    /// (`ER_REPO_LOCAL=1`) — there is nothing to import in that mode.
    fn er_import_pair(&self, name: &str) -> Option<(PathBuf, PathBuf)> {
        let target_dir = if name == "tour.json" {
            self.branch_bucket_er_dir()?
        } else {
            self.er_dir()
        };
        let src = PathBuf::from(&self.repo_root).join(".er").join(name);
        let dst = PathBuf::from(&target_dir).join(name);
        if src == dst {
            return None;
        }
        Some((src, dst))
    }

    /// Cheap per-poll check (mtime + existence only, no content reads): is there a
    /// `.er/` sidecar newer than — or absent from — managed storage? Drives whether
    /// the "Import local review files" affordance is shown.
    pub fn has_pending_er_import(&self) -> bool {
        IMPORTABLE_SIDECARS.iter().any(|&name| {
            let Some((src, dst)) = self.er_import_pair(name) else {
                return false;
            };
            let Some(src_m) = mtime_secs(&src) else {
                return false; // source missing
            };
            match mtime_secs(&dst) {
                None => true,                 // managed missing → pending
                Some(dst_m) => src_m > dst_m, // `.er/` strictly newer → pending
            }
        })
    }

    /// Full scan with content comparison — used to populate the import/confirm UI.
    /// Only includes sidecars whose import would actually change managed storage.
    pub fn scan_er_imports(&self) -> Vec<ErImportItem> {
        IMPORTABLE_SIDECARS
            .iter()
            .filter_map(|&name| {
                let (src, dst) = self.er_import_pair(name)?;
                inspect_sidecar(name, &src, &dst)
            })
            .collect()
    }

    /// Import every pending `.er/` sidecar. With `overwrite = false`, sidecars whose
    /// managed copy differs are reported as [`ErImportOutcome::Conflict`] instead of
    /// being overwritten, so the caller can confirm. Only returns an entry per
    /// sidecar that exists in `.er/` and isn't already in sync.
    pub fn import_er_sidecars(&self, overwrite: bool) -> Vec<ErImportItemResult> {
        IMPORTABLE_SIDECARS
            .iter()
            .filter_map(|&name| {
                let (src, dst) = self.er_import_pair(name)?;
                let item = inspect_sidecar(name, &src, &dst)?; // None → nothing to import
                let outcome = import_file(&src, &dst, overwrite).unwrap_or(ErImportOutcome::Skipped);
                Some(ErImportItemResult {
                    name: name.to_string(),
                    outcome,
                    er_newer: item.er_newer,
                    er_mtime: item.er_mtime,
                    managed_mtime: item.managed_mtime,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write(p: &Path, content: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }

    #[test]
    fn inspect_reports_new_file_when_managed_missing() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("er/tour.json");
        let dst = tmp.path().join("managed/tour.json");
        write(&src, r#"{"version":1}"#);

        let item = inspect_sidecar("tour.json", &src, &dst).expect("pending import");
        assert!(!item.managed_exists);
        assert!(item.differs);
        assert!(item.er_newer);
    }

    #[test]
    fn inspect_skips_identical_files() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("er/review.json");
        let dst = tmp.path().join("managed/review.json");
        write(&src, r#"{"v":1}"#);
        write(&dst, r#"{"v":1}"#);

        assert!(inspect_sidecar("review.json", &src, &dst).is_none());
    }

    #[test]
    fn inspect_flags_conflict_when_managed_differs() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("er/review.json");
        let dst = tmp.path().join("managed/review.json");
        write(&src, r#"{"v":2}"#);
        write(&dst, r#"{"v":1}"#);

        let item = inspect_sidecar("review.json", &src, &dst).expect("pending import");
        assert!(item.managed_exists);
        assert!(item.differs);
    }

    #[test]
    fn import_copies_when_managed_missing() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("er/tour.json");
        let dst = tmp.path().join("managed/tour.json");
        write(&src, r#"{"version":1}"#);

        assert_eq!(import_file(&src, &dst, false).unwrap(), ErImportOutcome::Imported);
        assert_eq!(std::fs::read_to_string(&dst).unwrap(), r#"{"version":1}"#);
        // tmp file cleaned up by rename
        assert!(!dst.with_extension("import-tmp").exists());
    }

    #[test]
    fn import_refuses_differing_overwrite_without_flag() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("er/review.json");
        let dst = tmp.path().join("managed/review.json");
        write(&src, r#"{"v":2}"#);
        write(&dst, r#"{"v":1}"#);

        assert_eq!(import_file(&src, &dst, false).unwrap(), ErImportOutcome::Conflict);
        // managed copy untouched
        assert_eq!(std::fs::read_to_string(&dst).unwrap(), r#"{"v":1}"#);
    }

    #[test]
    fn import_overwrites_with_flag() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("er/review.json");
        let dst = tmp.path().join("managed/review.json");
        write(&src, r#"{"v":2}"#);
        write(&dst, r#"{"v":1}"#);

        assert_eq!(import_file(&src, &dst, true).unwrap(), ErImportOutcome::Imported);
        assert_eq!(std::fs::read_to_string(&dst).unwrap(), r#"{"v":2}"#);
    }

    #[test]
    fn import_skips_identical() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("er/order.json");
        let dst = tmp.path().join("managed/order.json");
        write(&src, "same");
        write(&dst, "same");

        assert_eq!(import_file(&src, &dst, false).unwrap(), ErImportOutcome::Skipped);
    }

    #[test]
    fn er_newer_reflects_mtime_order() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("er/summary.md");
        let dst = tmp.path().join("managed/summary.md");
        write(&dst, "old-managed");
        // Ensure src is written after dst so its mtime is >= dst's.
        write(&src, "new-er");

        let item = inspect_sidecar("summary.md", &src, &dst).expect("pending import");
        assert!(item.er_newer, "freshly written .er/ copy should be >= managed");
    }
}
