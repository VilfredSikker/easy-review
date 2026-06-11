//! Persistent PR diff cache (issue #70).
//!
//! Persists the raw unified diff of remote PRs to disk so opening a PR is a
//! disk read + parse instead of a `gh pr diff` network download. Every write
//! is a write-through at a place the diff was already downloaded — this module
//! never fetches anything itself.
//!
//! Layout (one diff per PR, replaced atomically on head move):
//! `<storage_root>/repos/<slug>/prs/pr-<N>/diff-<head12>.patch` + `diff-meta.json`
//! where `<slug>` comes from [`storage::remote_repo_dir_slug`] (shared with
//! `pr_cache` so the two can't drift).
//!
//! Validity key is `(head_oid, base_branch)` — `base_branch` catches PR
//! retargeting; `updated_at` is deliberately *not* part of validity (it bumps
//! on comments, which don't change the diff).
//!
//! NOTE: the `pr-<N>` bucket dir also holds review sidecars (review.json,
//! questions.json, …). Eviction deletes only `diff-*` files, never the dir
//! wholesale.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::storage;

const DIFF_STORE_SCHEMA_VERSION: u32 = 1;

/// Diffs larger than this are not persisted (the in-memory caches still hold
/// them for the session; the IPC line budget limits what reaches the UI anyway).
pub const MAX_PERSISTED_DIFF_BYTES: usize = 5 * 1024 * 1024;

/// Cap on the number of PR diff entries kept per remote. With the size cap
/// above this bounds worst-case disk usage at ~120 MB per remote (typically
/// a few MB).
pub const MAX_CACHED_DIFFS_PER_REMOTE: usize = 24;

const META_FILE: &str = "diff-meta.json";

/// Monotonic counter for unique tmp-file names (concurrent-writer safety:
/// the open write-through and the hash backfill can race on the same PR).
static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Process-wide revision counter, bumped on every store mutation (save /
/// evict / self-heal delete). Lets callers cache [`has_diff`] probe results
/// and invalidate them only when the store actually changed.
static STORE_REVISION: AtomicU64 = AtomicU64::new(0);

/// Current diff-store revision. Changes whenever a diff is saved or evicted
/// in this process — cheap cache key for [`has_diff`] memoization.
pub fn store_revision() -> u64 {
    STORE_REVISION.load(Ordering::Relaxed)
}

fn bump_store_revision() {
    STORE_REVISION.fetch_add(1, Ordering::Relaxed);
}

/// Sidecar metadata for a persisted PR diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffMeta {
    pub version: u32,
    pub pr_number: u64,
    /// Head commit SHA the diff was downloaded at — primary validity key.
    pub head_oid: String,
    /// Base branch the PR targets — catches PR retargeting.
    pub base_branch: String,
    pub head_branch: String,
    /// GitHub `updatedAt` at save time (informational, not validity).
    #[serde(default)]
    pub updated_at: String,
    /// Base commit SHA when known (informational — base force-pushes with an
    /// unchanged head aren't detected, same trust level as the 60s loop).
    #[serde(default)]
    pub base_oid: Option<String>,
    /// Byte length of the patch file — integrity check on load.
    pub size_bytes: u64,
    /// SHA-256 of the patch contents — integrity check on load.
    pub sha256: String,
    /// Epoch ms of the save — LRU key for [`prune_remote`].
    pub saved_at_epoch_ms: u64,
}

impl DiffMeta {
    /// Descriptor for a diff about to be saved. The derived fields
    /// (`size_bytes`, `sha256`) are filled by [`save_diff`] from the raw diff;
    /// `saved_at_epoch_ms` is stamped at write time when left as `0`.
    pub fn new(
        pr_number: u64,
        head_oid: impl Into<String>,
        base_branch: impl Into<String>,
        head_branch: impl Into<String>,
        updated_at: impl Into<String>,
        base_oid: Option<String>,
    ) -> Self {
        DiffMeta {
            version: DIFF_STORE_SCHEMA_VERSION,
            pr_number,
            head_oid: head_oid.into(),
            base_branch: base_branch.into(),
            head_branch: head_branch.into(),
            updated_at: updated_at.into(),
            base_oid,
            size_bytes: 0,
            sha256: String::new(),
            saved_at_epoch_ms: 0,
        }
    }
}

/// Bucket directory for one PR's persisted diff (also holds review sidecars).
fn pr_diff_dir(remote: &str, pr_number: u64) -> PathBuf {
    storage::pr_bucket_dir(&storage::remote_repo_dir_slug(remote), pr_number)
}

/// `prs/` directory for a remote.
fn prs_dir(remote: &str) -> PathBuf {
    storage::storage_root()
        .join("repos")
        .join(storage::remote_repo_dir_slug(remote))
        .join("prs")
}

fn patch_file_name(head_oid: &str) -> String {
    let head12: String = head_oid.chars().take(12).collect();
    format!("diff-{head12}.patch")
}

/// Whether a directory entry belongs to the diff store (vs a review sidecar).
fn is_diff_store_file(name: &str) -> bool {
    name.starts_with("diff-")
}

/// Atomic write via a unique tmp file + rename. Unique names keep concurrent
/// writers (open write-through vs backfill) from clobbering each other's
/// in-flight tmp files; rename makes the final swap atomic. The tmp name keeps
/// the `diff-` prefix so orphans from a crash are swept by eviction.
fn write_atomic_unique(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path
        .parent()
        .with_context(|| format!("no parent dir for {}", path.display()))?;
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .with_context(|| format!("invalid file name for {}", path.display()))?;
    let tmp = dir.join(format!(
        "{file_name}.tmp-{}-{}",
        std::process::id(),
        TMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&tmp, bytes).with_context(|| format!("failed to write {}", tmp.display()))?;
    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("failed to rename into {}", path.display()));
    }
    Ok(())
}

/// Persist a PR diff for later instant opens. No-op for empty head SHAs and
/// diffs above [`MAX_PERSISTED_DIFF_BYTES`]. The patch is written before the
/// meta (a meta only ever points at a fully-written patch); older
/// `diff-*.patch` files are deleted after success (single-file invariant).
pub fn save_diff(remote: &str, meta: &DiffMeta, raw: &str) -> Result<()> {
    if meta.head_oid.trim().is_empty() {
        return Ok(());
    }
    if raw.len() > MAX_PERSISTED_DIFF_BYTES {
        return Ok(());
    }
    let dir = pr_diff_dir(remote, meta.pr_number);
    fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;

    let patch_name = patch_file_name(&meta.head_oid);
    write_atomic_unique(&dir.join(&patch_name), raw.as_bytes())?;

    let mut full = meta.clone();
    full.version = DIFF_STORE_SCHEMA_VERSION;
    full.size_bytes = raw.len() as u64;
    full.sha256 = crate::ai::compute_diff_hash(raw);
    if full.saved_at_epoch_ms == 0 {
        full.saved_at_epoch_ms = crate::pr_cache::now_epoch_ms();
    }
    let json = serde_json::to_string_pretty(&full).context("failed to serialize diff meta")?;
    write_atomic_unique(&dir.join(META_FILE), json.as_bytes())?;

    // Single-file invariant: drop patches for older head SHAs (and any
    // orphaned tmp files) now that the new patch + meta pair is in place.
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if is_diff_store_file(name) && name != patch_name && name != META_FILE {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    bump_store_revision();
    Ok(())
}

/// Cheap probe: is a diff persisted for this PR at this head SHA?
///
/// Reads only the small meta sidecar and `stat`s the patch file (byte-length
/// check) — it never reads the patch body or recomputes the SHA-256, so it is
/// safe to call from snapshot building. The full corrupt-check still runs on
/// [`load_diff`] at open time.
pub fn has_diff(remote: &str, pr_number: u64, expected_head_oid: &str) -> bool {
    if expected_head_oid.trim().is_empty() {
        return false;
    }
    let dir = pr_diff_dir(remote, pr_number);
    let Ok(content) = fs::read_to_string(dir.join(META_FILE)) else {
        return false;
    };
    let Ok(meta) = serde_json::from_str::<DiffMeta>(&content) else {
        return false;
    };
    if meta.version != DIFF_STORE_SCHEMA_VERSION
        || meta.pr_number != pr_number
        || meta.head_oid != expected_head_oid
    {
        return false;
    }
    fs::metadata(dir.join(patch_file_name(&meta.head_oid)))
        .map(|m| m.len() == meta.size_bytes)
        .unwrap_or(false)
}

/// Delete all diff-store files for one PR (review sidecars are untouched).
fn delete_diff_files(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if is_diff_store_file(name) {
            let _ = fs::remove_file(entry.path());
        }
    }
    // Tidy up the bucket dir when nothing else (sidecars) lives there.
    let _ = fs::remove_dir(dir);
    bump_store_revision();
}

/// Load the persisted diff for a PR when it matches the expected freshness.
///
/// - `head_oid` / `base_branch` mismatch → `Ok(None)` without deleting (a
///   concurrent writer may be mid-replace; the entry self-corrects on the
///   next write-through).
/// - Corrupt meta, missing/truncated patch, or size/sha mismatch → delete the
///   diff files (self-heal) and return `Ok(None)` so the caller refetches.
pub fn load_diff(
    remote: &str,
    pr_number: u64,
    expected_head_oid: &str,
    expected_base_branch: &str,
) -> Result<Option<String>> {
    if expected_head_oid.trim().is_empty() {
        return Ok(None);
    }
    let dir = pr_diff_dir(remote, pr_number);
    let meta_path = dir.join(META_FILE);
    let content = match fs::read_to_string(&meta_path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("failed to read {}", meta_path.display())),
    };
    let meta: DiffMeta = match serde_json::from_str(&content) {
        Ok(m) => m,
        Err(_) => {
            // Corrupt or hand-edited meta — self-heal and refetch.
            delete_diff_files(&dir);
            return Ok(None);
        }
    };
    if meta.version != DIFF_STORE_SCHEMA_VERSION || meta.pr_number != pr_number {
        delete_diff_files(&dir);
        return Ok(None);
    }
    if meta.head_oid != expected_head_oid || meta.base_branch != expected_base_branch {
        return Ok(None);
    }
    let patch_path = dir.join(patch_file_name(&meta.head_oid));
    let raw = match fs::read_to_string(&patch_path) {
        Ok(r) => r,
        Err(_) => {
            delete_diff_files(&dir);
            return Ok(None);
        }
    };
    if raw.len() as u64 != meta.size_bytes || crate::ai::compute_diff_hash(&raw) != meta.sha256 {
        delete_diff_files(&dir);
        return Ok(None);
    }
    Ok(Some(raw))
}

/// Stale-tolerant read: load the persisted diff for a PR regardless of which
/// head SHA / base branch it was downloaded at (stale-while-revalidate open
/// path). Integrity checks still apply — corrupt meta, missing/truncated
/// patch, or size/sha mismatch self-heal exactly like [`load_diff`]. Returns
/// the raw diff together with its meta so the caller can see how stale it is
/// (and which base/head branches the diff is self-consistent with).
///
/// Callers MUST treat a hit whose `head_oid` doesn't match the live PR as
/// stale: render it, then kick a background refetch — never serve it as final.
pub fn load_diff_any(remote: &str, pr_number: u64) -> Result<Option<(String, DiffMeta)>> {
    let dir = pr_diff_dir(remote, pr_number);
    let meta_path = dir.join(META_FILE);
    let content = match fs::read_to_string(&meta_path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("failed to read {}", meta_path.display())),
    };
    let meta: DiffMeta = match serde_json::from_str(&content) {
        Ok(m) => m,
        Err(_) => {
            delete_diff_files(&dir);
            return Ok(None);
        }
    };
    if meta.version != DIFF_STORE_SCHEMA_VERSION || meta.pr_number != pr_number {
        delete_diff_files(&dir);
        return Ok(None);
    }
    let patch_path = dir.join(patch_file_name(&meta.head_oid));
    let raw = match fs::read_to_string(&patch_path) {
        Ok(r) => r,
        Err(_) => {
            delete_diff_files(&dir);
            return Ok(None);
        }
    };
    if raw.len() as u64 != meta.size_bytes || crate::ai::compute_diff_hash(&raw) != meta.sha256 {
        delete_diff_files(&dir);
        return Ok(None);
    }
    Ok(Some((raw, meta)))
}

/// Evict the persisted diff for one PR (merged/closed/out-of-top-N). Review
/// sidecars in the same bucket dir are preserved.
pub fn evict_pr_diff(remote: &str, pr_number: u64) -> Result<()> {
    let dir = pr_diff_dir(remote, pr_number);
    if dir.is_dir() {
        delete_diff_files(&dir);
    }
    Ok(())
}

/// Enforce the per-remote cap on persisted diffs. PRs in `keep_numbers` are
/// never evicted; beyond that, the least recently saved diffs go first until
/// at most [`MAX_CACHED_DIFFS_PER_REMOTE`] entries remain.
pub fn prune_remote(remote: &str, keep_numbers: &HashSet<u64>) -> Result<()> {
    let prs = prs_dir(remote);
    let entries = match fs::read_dir(&prs) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e).with_context(|| format!("failed to read {}", prs.display())),
    };
    // (pr_number, saved_at) for every bucket that currently holds a diff.
    let mut cached: Vec<(u64, u64)> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(number) = name
            .to_str()
            .and_then(|n| n.strip_prefix("pr-"))
            .and_then(|n| n.parse::<u64>().ok())
        else {
            continue;
        };
        let meta_path = entry.path().join(META_FILE);
        if !meta_path.is_file() {
            continue;
        }
        // Unreadable/corrupt meta sorts oldest (saved_at 0) — pruned first.
        let saved_at = fs::read_to_string(&meta_path)
            .ok()
            .and_then(|c| serde_json::from_str::<DiffMeta>(&c).ok())
            .map(|m| m.saved_at_epoch_ms)
            .unwrap_or(0);
        cached.push((number, saved_at));
    }
    if cached.len() <= MAX_CACHED_DIFFS_PER_REMOTE {
        return Ok(());
    }
    // Newest first; protected entries don't count against the evictable tail.
    cached.sort_by_key(|c| std::cmp::Reverse(c.1));
    let mut kept = cached
        .iter()
        .filter(|(n, _)| keep_numbers.contains(n))
        .count();
    for (number, _) in &cached {
        if keep_numbers.contains(number) {
            continue;
        }
        if kept < MAX_CACHED_DIFFS_PER_REMOTE {
            kept += 1;
            continue;
        }
        evict_pr_diff(remote, *number)?;
    }
    Ok(())
}

// ── Local branch diffs ────────────────────────────────────────────────────────
//
// Committed-state branch diffs (`git diff <base>...<branch>`) are deterministic
// given `(branch head oid, resolved base oid)` — git derives the merge base
// from the two commits. The same SWR open flow as PRs applies: exact key match
// serves instantly, a stale entry renders while a background recompute swaps in
// the fresh diff.
//
// Layout (one diff per branch, replaced atomically on head/base move):
// `<storage_root>/repos/<repo_slug>/branch-diffs/<branch_slug>/diff-<head12>-<base12>.patch`
// + `diff-meta.json`, where `<repo_slug>` comes from [`storage::slug_repo`]
// (same slug as the managed review storage for local repos).

/// Cap on persisted branch-diff entries per repo (one per branch).
pub const MAX_CACHED_BRANCH_DIFFS_PER_REPO: usize = 24;

/// Sidecar metadata for a persisted local branch diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchDiffMeta {
    pub version: u32,
    /// Branch name the diff was computed for (informational; the slug keys the dir).
    pub branch: String,
    /// Branch head commit SHA — primary validity key.
    pub head_oid: String,
    /// Resolved base commit SHA — second validity key (catches base moves).
    pub base_oid: String,
    /// Base ref name the diff was computed against (e.g. `origin/main`).
    pub base_ref: String,
    /// Byte length of the patch file — integrity check on load.
    pub size_bytes: u64,
    /// SHA-256 of the patch contents — integrity check on load.
    pub sha256: String,
    /// Epoch ms of the save — LRU key for [`prune_branch_diffs`].
    pub saved_at_epoch_ms: u64,
}

impl BranchDiffMeta {
    /// Descriptor for a branch diff about to be saved; derived fields are
    /// filled by [`save_branch_diff`].
    pub fn new(
        branch: impl Into<String>,
        head_oid: impl Into<String>,
        base_oid: impl Into<String>,
        base_ref: impl Into<String>,
    ) -> Self {
        BranchDiffMeta {
            version: DIFF_STORE_SCHEMA_VERSION,
            branch: branch.into(),
            head_oid: head_oid.into(),
            base_oid: base_oid.into(),
            base_ref: base_ref.into(),
            size_bytes: 0,
            sha256: String::new(),
            saved_at_epoch_ms: 0,
        }
    }
}

/// Patch file name keyed by both commit OIDs. Pure — unit-tested.
pub fn branch_patch_file_name(head_oid: &str, base_oid: &str) -> String {
    let head12: String = head_oid.chars().take(12).collect();
    let base12: String = base_oid.chars().take(12).collect();
    format!("diff-{head12}-{base12}.patch")
}

/// `branch-diffs/` directory for a repo slug.
fn branch_diffs_dir(repo_slug: &str) -> PathBuf {
    storage::storage_root()
        .join("repos")
        .join(repo_slug)
        .join("branch-diffs")
}

/// Bucket directory for one branch's persisted diff.
fn branch_diff_dir(repo_slug: &str, branch: &str) -> PathBuf {
    branch_diffs_dir(repo_slug).join(storage::slug_branch(branch))
}

/// Persist a committed-state branch diff for later instant opens. No-op for
/// empty OIDs and diffs above [`MAX_PERSISTED_DIFF_BYTES`]. Mirrors
/// [`save_diff`]: patch first, then meta, then older `diff-*` files dropped.
pub fn save_branch_diff(repo_slug: &str, meta: &BranchDiffMeta, raw: &str) -> Result<()> {
    if meta.head_oid.trim().is_empty() || meta.base_oid.trim().is_empty() {
        return Ok(());
    }
    if raw.len() > MAX_PERSISTED_DIFF_BYTES {
        return Ok(());
    }
    let dir = branch_diff_dir(repo_slug, &meta.branch);
    fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;

    let patch_name = branch_patch_file_name(&meta.head_oid, &meta.base_oid);
    write_atomic_unique(&dir.join(&patch_name), raw.as_bytes())?;

    let mut full = meta.clone();
    full.version = DIFF_STORE_SCHEMA_VERSION;
    full.size_bytes = raw.len() as u64;
    full.sha256 = crate::ai::compute_diff_hash(raw);
    if full.saved_at_epoch_ms == 0 {
        full.saved_at_epoch_ms = crate::pr_cache::now_epoch_ms();
    }
    let json =
        serde_json::to_string_pretty(&full).context("failed to serialize branch diff meta")?;
    write_atomic_unique(&dir.join(META_FILE), json.as_bytes())?;

    // Single-file invariant: drop patches for older keys (and orphaned tmps).
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if is_diff_store_file(name) && name != patch_name && name != META_FILE {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    bump_store_revision();
    prune_branch_diffs(repo_slug)?;
    Ok(())
}

/// Read + integrity-check the meta/patch pair for one branch. Self-heals
/// (deletes diff files) on corruption, exactly like the PR-diff loads.
fn load_branch_diff_entry(dir: &Path) -> Result<Option<(String, BranchDiffMeta)>> {
    let meta_path = dir.join(META_FILE);
    let content = match fs::read_to_string(&meta_path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("failed to read {}", meta_path.display())),
    };
    let meta: BranchDiffMeta = match serde_json::from_str(&content) {
        Ok(m) => m,
        Err(_) => {
            delete_diff_files(dir);
            return Ok(None);
        }
    };
    if meta.version != DIFF_STORE_SCHEMA_VERSION {
        delete_diff_files(dir);
        return Ok(None);
    }
    let patch_path = dir.join(branch_patch_file_name(&meta.head_oid, &meta.base_oid));
    let raw = match fs::read_to_string(&patch_path) {
        Ok(r) => r,
        Err(_) => {
            delete_diff_files(dir);
            return Ok(None);
        }
    };
    if raw.len() as u64 != meta.size_bytes || crate::ai::compute_diff_hash(&raw) != meta.sha256 {
        delete_diff_files(dir);
        return Ok(None);
    }
    Ok(Some((raw, meta)))
}

/// Load the persisted branch diff when it matches `(head_oid, base_oid)`.
/// Key mismatch → `Ok(None)` without deleting (next write-through replaces it).
pub fn load_branch_diff(
    repo_slug: &str,
    branch: &str,
    expected_head_oid: &str,
    expected_base_oid: &str,
) -> Result<Option<String>> {
    if expected_head_oid.trim().is_empty() || expected_base_oid.trim().is_empty() {
        return Ok(None);
    }
    let dir = branch_diff_dir(repo_slug, branch);
    let Some((raw, meta)) = load_branch_diff_entry(&dir)? else {
        return Ok(None);
    };
    if meta.head_oid != expected_head_oid || meta.base_oid != expected_base_oid {
        return Ok(None);
    }
    Ok(Some(raw))
}

/// Stale-tolerant read for the SWR open path: the persisted diff regardless of
/// which head/base it was computed at, together with its meta. Callers MUST
/// treat a key mismatch as stale — render, then recompute in the background.
pub fn load_branch_diff_any(
    repo_slug: &str,
    branch: &str,
) -> Result<Option<(String, BranchDiffMeta)>> {
    load_branch_diff_entry(&branch_diff_dir(repo_slug, branch))
}

/// Enforce the per-repo cap on persisted branch diffs (least recently saved
/// evicted first). Review sidecars don't live in `branch-diffs/`, so empty
/// bucket dirs are removed wholesale.
pub fn prune_branch_diffs(repo_slug: &str) -> Result<()> {
    let root = branch_diffs_dir(repo_slug);
    let entries = match fs::read_dir(&root) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e).with_context(|| format!("failed to read {}", root.display())),
    };
    let mut cached: Vec<(PathBuf, u64)> = Vec::new();
    for entry in entries.flatten() {
        let meta_path = entry.path().join(META_FILE);
        if !meta_path.is_file() {
            continue;
        }
        let saved_at = fs::read_to_string(&meta_path)
            .ok()
            .and_then(|c| serde_json::from_str::<BranchDiffMeta>(&c).ok())
            .map(|m| m.saved_at_epoch_ms)
            .unwrap_or(0);
        cached.push((entry.path(), saved_at));
    }
    if cached.len() <= MAX_CACHED_BRANCH_DIFFS_PER_REPO {
        return Ok(());
    }
    cached.sort_by_key(|c| std::cmp::Reverse(c.1));
    for (dir, _) in cached.iter().skip(MAX_CACHED_BRANCH_DIFFS_PER_REPO) {
        delete_diff_files(dir);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::STORAGE_TEST_ENV_LOCK;
    use tempfile::TempDir;

    const REMOTE: &str = "org/repo";

    fn meta(pr_number: u64, head_oid: &str) -> DiffMeta {
        DiffMeta::new(
            pr_number,
            head_oid,
            "main",
            "feature/x",
            "2026-06-09T00:00:00Z",
            None,
        )
    }

    /// Run `f` with `ER_STORAGE_ROOT` pointed at a temp dir, serialized against
    /// other env-mutating tests (same pattern as the pr_cache tests).
    fn with_temp_storage(f: impl FnOnce() + std::panic::UnwindSafe) {
        let _guard = STORAGE_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = TempDir::new().unwrap();
        std::env::set_var("ER_STORAGE_ROOT", tmp.path());
        let result = std::panic::catch_unwind(f);
        std::env::remove_var("ER_STORAGE_ROOT");
        if let Err(e) = result {
            std::panic::resume_unwind(e);
        }
    }

    #[test]
    fn save_load_roundtrip() {
        with_temp_storage(|| {
            let raw = "diff --git a/foo b/foo\n+hello\n";
            save_diff(REMOTE, &meta(7, "abcdef1234567890"), raw).unwrap();
            let loaded = load_diff(REMOTE, 7, "abcdef1234567890", "main").unwrap();
            assert_eq!(loaded.as_deref(), Some(raw));
            // Meta carries the derived fields.
            let meta_path = pr_diff_dir(REMOTE, 7).join(META_FILE);
            let parsed: DiffMeta =
                serde_json::from_str(&std::fs::read_to_string(meta_path).unwrap()).unwrap();
            assert_eq!(parsed.size_bytes, raw.len() as u64);
            assert_eq!(parsed.sha256, crate::ai::compute_diff_hash(raw));
            assert!(parsed.saved_at_epoch_ms > 0);
        });
    }

    #[test]
    fn wrong_oid_or_base_returns_none_without_delete() {
        with_temp_storage(|| {
            let raw = "diff --git a/foo b/foo\n+hello\n";
            save_diff(REMOTE, &meta(7, "abcdef1234567890"), raw).unwrap();
            // Head moved → miss, files untouched.
            assert!(load_diff(REMOTE, 7, "other-sha", "main").unwrap().is_none());
            // PR retargeted → miss, files untouched.
            assert!(load_diff(REMOTE, 7, "abcdef1234567890", "develop")
                .unwrap()
                .is_none());
            // Original key still hits — nothing was deleted.
            assert!(load_diff(REMOTE, 7, "abcdef1234567890", "main")
                .unwrap()
                .is_some());
        });
    }

    #[test]
    fn empty_expected_oid_returns_none() {
        with_temp_storage(|| {
            save_diff(REMOTE, &meta(7, "abcdef1234567890"), "diff\n").unwrap();
            assert!(load_diff(REMOTE, 7, "", "main").unwrap().is_none());
        });
    }

    #[test]
    fn corrupt_meta_self_heals() {
        with_temp_storage(|| {
            let raw = "diff --git a/foo b/foo\n+hello\n";
            save_diff(REMOTE, &meta(7, "abcdef1234567890"), raw).unwrap();
            let dir = pr_diff_dir(REMOTE, 7);
            std::fs::write(dir.join(META_FILE), "not json {").unwrap();
            assert!(load_diff(REMOTE, 7, "abcdef1234567890", "main")
                .unwrap()
                .is_none());
            // Both files were deleted — next save starts clean.
            assert!(!dir.join(META_FILE).exists());
            assert!(!dir.join(patch_file_name("abcdef1234567890")).exists());
        });
    }

    #[test]
    fn truncated_patch_self_heals() {
        with_temp_storage(|| {
            let raw = "diff --git a/foo b/foo\n+hello\n";
            save_diff(REMOTE, &meta(7, "abcdef1234567890"), raw).unwrap();
            let dir = pr_diff_dir(REMOTE, 7);
            std::fs::write(dir.join(patch_file_name("abcdef1234567890")), "diff --g").unwrap();
            assert!(load_diff(REMOTE, 7, "abcdef1234567890", "main")
                .unwrap()
                .is_none());
            assert!(!dir.join(META_FILE).exists());
        });
    }

    #[test]
    fn oversize_diff_is_not_persisted() {
        with_temp_storage(|| {
            let raw = "x".repeat(MAX_PERSISTED_DIFF_BYTES + 1);
            save_diff(REMOTE, &meta(7, "abcdef1234567890"), &raw).unwrap();
            assert!(load_diff(REMOTE, 7, "abcdef1234567890", "main")
                .unwrap()
                .is_none());
            assert!(!pr_diff_dir(REMOTE, 7).join(META_FILE).exists());
        });
    }

    #[test]
    fn empty_head_oid_is_a_noop() {
        with_temp_storage(|| {
            save_diff(REMOTE, &meta(7, ""), "diff\n").unwrap();
            assert!(!pr_diff_dir(REMOTE, 7).join(META_FILE).exists());
        });
    }

    #[test]
    fn head_move_keeps_a_single_patch_file() {
        with_temp_storage(|| {
            save_diff(REMOTE, &meta(7, "aaaaaaaaaaaaaaaa"), "old diff\n").unwrap();
            save_diff(REMOTE, &meta(7, "bbbbbbbbbbbbbbbb"), "new diff\n").unwrap();
            let dir = pr_diff_dir(REMOTE, 7);
            let patches: Vec<String> = std::fs::read_dir(&dir)
                .unwrap()
                .flatten()
                .filter_map(|e| e.file_name().to_str().map(String::from))
                .filter(|n| n.ends_with(".patch"))
                .collect();
            assert_eq!(patches, vec![patch_file_name("bbbbbbbbbbbbbbbb")]);
            // Old key misses, new key hits.
            assert!(load_diff(REMOTE, 7, "aaaaaaaaaaaaaaaa", "main")
                .unwrap()
                .is_none());
            assert_eq!(
                load_diff(REMOTE, 7, "bbbbbbbbbbbbbbbb", "main")
                    .unwrap()
                    .as_deref(),
                Some("new diff\n")
            );
        });
    }

    #[test]
    fn has_diff_probe_matches_load_semantics() {
        with_temp_storage(|| {
            let raw = "diff --git a/foo b/foo\n+hello\n";
            // Nothing persisted yet — probe misses.
            assert!(!has_diff(REMOTE, 7, "abcdef1234567890"));
            save_diff(REMOTE, &meta(7, "abcdef1234567890"), raw).unwrap();
            // Hit on the saved head; miss on a moved head or empty oid.
            assert!(has_diff(REMOTE, 7, "abcdef1234567890"));
            assert!(!has_diff(REMOTE, 7, "other-sha"));
            assert!(!has_diff(REMOTE, 7, ""));
            assert!(!has_diff(REMOTE, 404, "abcdef1234567890"));
        });
    }

    #[test]
    fn has_diff_misses_on_truncated_patch_without_deleting() {
        with_temp_storage(|| {
            let raw = "diff --git a/foo b/foo\n+hello\n";
            save_diff(REMOTE, &meta(7, "abcdef1234567890"), raw).unwrap();
            let dir = pr_diff_dir(REMOTE, 7);
            // Truncate the patch — byte-length check must fail the probe.
            std::fs::write(dir.join(patch_file_name("abcdef1234567890")), "diff").unwrap();
            assert!(!has_diff(REMOTE, 7, "abcdef1234567890"));
            // The cheap probe never deletes — self-heal is load_diff's job.
            assert!(dir.join(META_FILE).exists());
        });
    }

    #[test]
    fn has_diff_misses_after_evict() {
        with_temp_storage(|| {
            save_diff(REMOTE, &meta(7, "abcdef1234567890"), "diff\n").unwrap();
            assert!(has_diff(REMOTE, 7, "abcdef1234567890"));
            evict_pr_diff(REMOTE, 7).unwrap();
            assert!(!has_diff(REMOTE, 7, "abcdef1234567890"));
        });
    }

    #[test]
    fn store_revision_bumps_on_save_and_evict() {
        with_temp_storage(|| {
            let r0 = store_revision();
            save_diff(REMOTE, &meta(7, "abcdef1234567890"), "diff\n").unwrap();
            let r1 = store_revision();
            assert!(r1 > r0, "save must bump the revision");
            evict_pr_diff(REMOTE, 7).unwrap();
            assert!(store_revision() > r1, "evict must bump the revision");
        });
    }

    #[test]
    fn load_diff_any_returns_stale_entry_with_meta() {
        with_temp_storage(|| {
            let raw = "diff --git a/foo b/foo\n+hello\n";
            // Missing entry → None.
            assert!(load_diff_any(REMOTE, 7).unwrap().is_none());
            save_diff(REMOTE, &meta(7, "abcdef1234567890"), raw).unwrap();
            // The strict read misses once the head moves…
            assert!(load_diff(REMOTE, 7, "new-head-sha", "main")
                .unwrap()
                .is_none());
            // …but the stale-tolerant read still serves the stored diff + meta.
            let (loaded, m) = load_diff_any(REMOTE, 7).unwrap().expect("stale hit");
            assert_eq!(loaded, raw);
            assert_eq!(m.head_oid, "abcdef1234567890");
            assert_eq!(m.base_branch, "main");
            assert_eq!(m.head_branch, "feature/x");
            // Nothing was deleted by either read.
            assert!(load_diff(REMOTE, 7, "abcdef1234567890", "main")
                .unwrap()
                .is_some());
        });
    }

    #[test]
    fn load_diff_any_self_heals_on_truncated_patch() {
        with_temp_storage(|| {
            let raw = "diff --git a/foo b/foo\n+hello\n";
            save_diff(REMOTE, &meta(7, "abcdef1234567890"), raw).unwrap();
            let dir = pr_diff_dir(REMOTE, 7);
            std::fs::write(dir.join(patch_file_name("abcdef1234567890")), "diff --g").unwrap();
            assert!(load_diff_any(REMOTE, 7).unwrap().is_none());
            // Integrity checks behave like load_diff: corrupt entry is deleted.
            assert!(!dir.join(META_FILE).exists());
        });
    }

    #[test]
    fn load_diff_any_self_heals_on_corrupt_meta() {
        with_temp_storage(|| {
            save_diff(REMOTE, &meta(7, "abcdef1234567890"), "diff\n").unwrap();
            let dir = pr_diff_dir(REMOTE, 7);
            std::fs::write(dir.join(META_FILE), "not json {").unwrap();
            assert!(load_diff_any(REMOTE, 7).unwrap().is_none());
            assert!(!dir.join(patch_file_name("abcdef1234567890")).exists());
        });
    }

    #[test]
    fn evict_preserves_review_sidecars() {
        with_temp_storage(|| {
            save_diff(REMOTE, &meta(7, "abcdef1234567890"), "diff\n").unwrap();
            let dir = pr_diff_dir(REMOTE, 7);
            std::fs::write(dir.join("review.json"), "{}").unwrap();
            evict_pr_diff(REMOTE, 7).unwrap();
            assert!(dir.join("review.json").exists(), "sidecar must survive");
            assert!(!dir.join(META_FILE).exists());
            assert!(!dir.join(patch_file_name("abcdef1234567890")).exists());
        });
    }

    #[test]
    fn evict_missing_pr_is_a_noop() {
        with_temp_storage(|| {
            evict_pr_diff(REMOTE, 404).unwrap();
        });
    }

    #[test]
    fn prune_respects_keep_set_and_cap() {
        with_temp_storage(|| {
            // 30 cached diffs with strictly increasing saved_at (PR 1 oldest).
            for n in 1..=30u64 {
                let mut m = meta(n, "abcdef1234567890");
                m.saved_at_epoch_ms = 1_000 + n;
                save_diff(REMOTE, &m, "diff\n").unwrap();
            }
            // Keep set includes the two oldest — they must survive the prune.
            let keep: HashSet<u64> = [1, 2].into_iter().collect();
            prune_remote(REMOTE, &keep).unwrap();

            let has_diff = |n: u64| {
                load_diff(REMOTE, n, "abcdef1234567890", "main")
                    .unwrap()
                    .is_some()
            };
            assert!(has_diff(1) && has_diff(2), "keep-set entries survive");
            // 2 protected + the 22 newest non-keep entries (30 down to 9) fill
            // the cap of 24.
            assert!(has_diff(30) && has_diff(9));
            // Oldest non-keep entries (3..=8) were evicted to satisfy the cap.
            for n in 3..=8 {
                assert!(!has_diff(n), "pr-{n} should be pruned");
            }
        });
    }

    #[test]
    fn prune_under_cap_is_a_noop() {
        with_temp_storage(|| {
            save_diff(REMOTE, &meta(1, "abcdef1234567890"), "diff\n").unwrap();
            prune_remote(REMOTE, &HashSet::new()).unwrap();
            assert!(load_diff(REMOTE, 1, "abcdef1234567890", "main")
                .unwrap()
                .is_some());
        });
    }

    fn branch_meta(branch: &str, head: &str, base: &str) -> BranchDiffMeta {
        BranchDiffMeta::new(branch, head, base, "origin/main")
    }

    #[test]
    fn branch_patch_file_name_keys_on_both_oids() {
        assert_eq!(
            branch_patch_file_name("aaaabbbbccccdddd", "1111222233334444"),
            "diff-aaaabbbbcccc-111122223333.patch"
        );
        // Short OIDs are used as-is (no panic, deterministic).
        assert_eq!(branch_patch_file_name("ab", "cd"), "diff-ab-cd.patch");
    }

    #[test]
    fn branch_diff_save_load_roundtrip() {
        with_temp_storage(|| {
            let raw = "diff --git a/foo b/foo\n+hello\n";
            save_branch_diff(
                "my-repo",
                &branch_meta("feature/x", "headoid12345", "baseoid12345"),
                raw,
            )
            .unwrap();
            let loaded =
                load_branch_diff("my-repo", "feature/x", "headoid12345", "baseoid12345").unwrap();
            assert_eq!(loaded.as_deref(), Some(raw));
        });
    }

    #[test]
    fn branch_diff_misses_on_moved_head_or_base_without_delete() {
        with_temp_storage(|| {
            let raw = "diff --git a/foo b/foo\n+hello\n";
            save_branch_diff(
                "my-repo",
                &branch_meta("feature/x", "headoid12345", "baseoid12345"),
                raw,
            )
            .unwrap();
            // Head moved → miss; base moved → miss; files untouched.
            assert!(
                load_branch_diff("my-repo", "feature/x", "other-head", "baseoid12345")
                    .unwrap()
                    .is_none()
            );
            assert!(
                load_branch_diff("my-repo", "feature/x", "headoid12345", "other-base")
                    .unwrap()
                    .is_none()
            );
            assert!(
                load_branch_diff("my-repo", "feature/x", "headoid12345", "baseoid12345")
                    .unwrap()
                    .is_some()
            );
            // Empty expected OIDs never match.
            assert!(load_branch_diff("my-repo", "feature/x", "", "baseoid12345")
                .unwrap()
                .is_none());
        });
    }

    #[test]
    fn branch_diff_any_returns_stale_entry_with_meta() {
        with_temp_storage(|| {
            let raw = "diff --git a/foo b/foo\n+hello\n";
            assert!(load_branch_diff_any("my-repo", "feature/x")
                .unwrap()
                .is_none());
            save_branch_diff(
                "my-repo",
                &branch_meta("feature/x", "headoid12345", "baseoid12345"),
                raw,
            )
            .unwrap();
            let (loaded, meta) = load_branch_diff_any("my-repo", "feature/x")
                .unwrap()
                .expect("stale hit");
            assert_eq!(loaded, raw);
            assert_eq!(meta.head_oid, "headoid12345");
            assert_eq!(meta.base_oid, "baseoid12345");
            assert_eq!(meta.base_ref, "origin/main");
        });
    }

    #[test]
    fn branch_diff_truncated_patch_self_heals() {
        with_temp_storage(|| {
            let raw = "diff --git a/foo b/foo\n+hello\n";
            save_branch_diff(
                "my-repo",
                &branch_meta("feature/x", "headoid12345", "baseoid12345"),
                raw,
            )
            .unwrap();
            let dir = branch_diff_dir("my-repo", "feature/x");
            std::fs::write(
                dir.join(branch_patch_file_name("headoid12345", "baseoid12345")),
                "diff --g",
            )
            .unwrap();
            assert!(load_branch_diff_any("my-repo", "feature/x")
                .unwrap()
                .is_none());
            assert!(!dir.join(META_FILE).exists());
        });
    }

    #[test]
    fn branch_diff_key_move_keeps_single_patch_file() {
        with_temp_storage(|| {
            save_branch_diff(
                "my-repo",
                &branch_meta("feature/x", "aaaaaaaaaaaa", "bbbbbbbbbbbb"),
                "old\n",
            )
            .unwrap();
            save_branch_diff(
                "my-repo",
                &branch_meta("feature/x", "cccccccccccc", "bbbbbbbbbbbb"),
                "new\n",
            )
            .unwrap();
            let dir = branch_diff_dir("my-repo", "feature/x");
            let patches: Vec<String> = std::fs::read_dir(&dir)
                .unwrap()
                .flatten()
                .filter_map(|e| e.file_name().to_str().map(String::from))
                .filter(|n| n.ends_with(".patch"))
                .collect();
            assert_eq!(
                patches,
                vec![branch_patch_file_name("cccccccccccc", "bbbbbbbbbbbb")]
            );
            assert_eq!(
                load_branch_diff("my-repo", "feature/x", "cccccccccccc", "bbbbbbbbbbbb")
                    .unwrap()
                    .as_deref(),
                Some("new\n")
            );
        });
    }

    #[test]
    fn branch_diff_prune_evicts_least_recent() {
        with_temp_storage(|| {
            for n in 1..=(MAX_CACHED_BRANCH_DIFFS_PER_REPO as u64 + 3) {
                let mut m = branch_meta(&format!("branch-{n}"), "headoid12345", "baseoid12345");
                m.saved_at_epoch_ms = 1_000 + n;
                save_branch_diff("my-repo", &m, "diff\n").unwrap();
            }
            let has = |n: u64| {
                load_branch_diff(
                    "my-repo",
                    &format!("branch-{n}"),
                    "headoid12345",
                    "baseoid12345",
                )
                .unwrap()
                .is_some()
            };
            // Oldest three evicted; newest survive.
            assert!(!has(1) && !has(2) && !has(3), "oldest entries pruned");
            assert!(has(4) && has(MAX_CACHED_BRANCH_DIFFS_PER_REPO as u64 + 3));
        });
    }

    #[test]
    fn slug_layout_agrees_with_pr_cache() {
        with_temp_storage(|| {
            let remote = "My-Org/some.repo";
            // pr-cache.json and the diff store must share the same
            // `repos/<slug>/` directory.
            let cache_repo_dir = crate::pr_cache::cache_path(remote)
                .parent()
                .unwrap()
                .to_path_buf();
            let diff_dir = pr_diff_dir(remote, 42);
            assert!(
                diff_dir.starts_with(&cache_repo_dir),
                "diff dir {} not under pr-cache repo dir {}",
                diff_dir.display(),
                cache_repo_dir.display()
            );
            assert!(diff_dir.ends_with("prs/pr-42"));
        });
    }
}
