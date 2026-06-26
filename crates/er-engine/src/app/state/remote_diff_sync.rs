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

// The pure fetch core (no App dependency) lives in `crate::sync`; re-exported
// here so existing `crate::app::...` paths keep working.
pub use crate::sync::{fetch_remote_diff_data, RemoteDiffContext, RemoteDiffResult};

use super::App;

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
        // Refresh the COMMITS panel. Best-effort: `None` means the fetch failed
        // or returned empty, so keep the tab's existing list.
        if let Some(commits) = result.commits {
            tab.pr_commits = commits;
        }
        // Rebuild precomputed scroll offsets for the new file list.
        tab.rebuild_hunk_offsets();
        tab.reload_ai_state();
        tab.relocate_all_comments();
        if tab.ai.is_stale {
            if let Some(raw) = tab.raw_diff.clone() {
                tab.compute_stale_files(&raw);
            }
        }
    }
}
