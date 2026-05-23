//! Background refresh for remote-PR tab diffs without holding the App mutex.
//!
//! Mirrors the three-phase pattern in [`super::github_sync`]:
//!   1. Caller acquires App briefly → calls [`App::snapshot_for_remote_diff_refresh`].
//!   2. Caller drops the lock → calls [`fetch_remote_diff_data`] (shells out to
//!      `gh pr diff`, parses, hashes — all outside the mutex).
//!   3. Caller re-acquires App briefly → calls [`App::apply_remote_diff_result`].
//!
//! Dedup: callers pass the latest known head_oid (from the PR cache) via
//! `expected_head_oid`; if it matches the tab's `last_diff_head_oid` the
//! fetch is skipped and `Ok(None)` is returned.

use anyhow::Result;

use crate::ai::{compute_diff_hash, compute_diff_hash_fast};
use crate::git;
use crate::github;

use super::App;

/// Inputs for one refresh cycle. Built while holding the App lock.
#[derive(Debug, Clone)]
pub struct RemoteDiffContext {
    pub owner: String,
    pub repo: String,
    pub pr_number: u64,
    /// Used by `apply_remote_diff_result` to find the right tab if the user
    /// switches or closes tabs during the network fetch.
    pub repo_root: String,
    /// What `last_diff_head_oid` was on the tab when the snapshot was taken.
    pub last_head_oid: Option<String>,
    /// Latest head_oid available out-of-band (typically the PR cache). When
    /// this equals `last_head_oid` the loop short-circuits.
    pub expected_head_oid: Option<String>,
}

/// Output of one refresh cycle. Applied via `apply_remote_diff_result`.
#[derive(Debug, Clone)]
pub struct RemoteDiffResult {
    pub raw_diff: String,
    pub files: Vec<git::DiffFile>,
    pub branch_diff_hash: String,
    pub diff_hash: String,
    pub head_oid: Option<String>,
    /// (repo_root, pr_number, is_remote) — used to find the right tab on apply.
    pub tab_key: (String, Option<u64>, bool),
}

impl App {
    /// Snapshot the active tab's identity for a remote-diff refresh. Returns
    /// `None` when the active tab is not a remote PR (nothing to refresh).
    ///
    /// `expected_head_oid` is left `None`; the caller fills it from pr_cache
    /// after releasing the App lock.
    pub fn snapshot_for_remote_diff_refresh(&self) -> Option<RemoteDiffContext> {
        let tab = self.tab();
        if !tab.is_remote() {
            return None;
        }
        let slug = tab.remote_repo.as_ref()?;
        let (owner, repo) = slug.split_once('/')?;
        let pr_number = tab.pr_number?;
        Some(RemoteDiffContext {
            owner: owner.to_string(),
            repo: repo.to_string(),
            pr_number,
            repo_root: tab.repo_root.clone(),
            last_head_oid: tab.last_diff_head_oid.clone(),
            expected_head_oid: None,
        })
    }

    /// Apply a pre-fetched remote diff. No-op if the tab was closed or the
    /// identity no longer matches.
    pub fn apply_remote_diff_result(&mut self, result: RemoteDiffResult) {
        let (target_root, target_pr, target_is_remote) = &result.tab_key;
        let idx = self.tabs.iter().position(|t| {
            &t.repo_root == target_root
                && t.pr_number == *target_pr
                && t.is_remote() == *target_is_remote
        });
        let Some(idx) = idx else { return };
        let tab = &mut self.tabs[idx];
        tab.files = result.files;
        tab.raw_diff = Some(result.raw_diff);
        tab.branch_diff_hash = result.branch_diff_hash;
        tab.diff_hash = result.diff_hash;
        tab.last_diff_head_oid = result.head_oid;
        // Rebuild precomputed scroll offsets for the new file list.
        tab.rebuild_hunk_offsets();
    }
}

/// Run the network fetch + parse + hash for a remote-PR diff refresh.
/// Returns `Ok(None)` when the expected head_oid matches the last fetched
/// one (no work needed).
pub fn fetch_remote_diff_data(ctx: &RemoteDiffContext) -> Result<Option<RemoteDiffResult>> {
    if let (Some(expected), Some(last)) = (ctx.expected_head_oid.as_deref(), ctx.last_head_oid.as_deref()) {
        if expected == last {
            return Ok(None);
        }
    }

    let raw = github::gh_pr_diff_remote(&ctx.owner, &ctx.repo, ctx.pr_number)?;
    let files = git::parse_diff(&raw);
    let branch_diff_hash = compute_diff_hash(&raw);
    let diff_hash = format!("{:016x}", compute_diff_hash_fast(&raw));

    Ok(Some(RemoteDiffResult {
        raw_diff: raw,
        files,
        branch_diff_hash,
        diff_hash,
        head_oid: ctx.expected_head_oid.clone(),
        tab_key: (ctx.repo_root.clone(), Some(ctx.pr_number), true),
    }))
}
