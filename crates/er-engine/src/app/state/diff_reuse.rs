//! Reusing the loaded diff across a Tour ↔ source-view switch.
//!
//! The Tour regroups the diff of the view it was entered from, so switching
//! between the two re-ran the same `git diff` / `gh pr diff`, re-parsed it and
//! re-hashed it only to show the bytes already in memory. ADR 0050 records why
//! the switch now trusts the loaded diff, and what keeps that safe.

use super::{DiffMode, TabState};

/// Which fetch a refresh in a given mode would run. Two modes share a loaded
/// diff only when they would run the same one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffRoute {
    /// `gh pr diff` (local clone or `--repo`): the PR's parity diff.
    PrParity,
    /// The checked-out working tree against the base.
    CheckoutAgainstBase,
    /// `git diff` of the branch against the base.
    Branch,
}

/// Everything a Branch / PR Diff / Tour refresh reads to decide what to load.
/// Equal keys mean the refresh would fetch the same diff from the same refs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiffSourceKey {
    route: DiffRoute,
    /// Whether the refresh also resolves the PR head oid it pairs with the diff.
    pairs_preview_head: bool,
    base_branch: String,
    pr_head_ref: Option<String>,
    pr_number: Option<u64>,
    local_branch_view: Option<String>,
    checkout_root: Option<String>,
    remote_repo: Option<String>,
    current_branch: String,
    last_diff_head_oid: Option<String>,
    /// The hash the loaded diff carried; anything that replaced `files`
    /// without going through a refresh also moved it.
    diff_hash: String,
}

impl TabState {
    /// The key a refresh in `mode` would load under, or `None` for modes whose
    /// diff is never shared with the Tour.
    fn diff_source_key(&self, mode: DiffMode) -> Option<DiffSourceKey> {
        if !matches!(mode, DiffMode::Branch | DiffMode::PrDiff | DiffMode::Tour) {
            return None;
        }
        let checkout = self.local_branch_checkout_root.is_some();
        // Mirrors `fetch_pr_diff_for_review`: a checked-out PR's Branch view is
        // the working tree, every other PR-tab view is the parity diff.
        let route = if self.pr_number.is_some()
            && !(checkout && mode == DiffMode::Branch)
            && (self.is_remote() || !self.repo_root.is_empty())
        {
            DiffRoute::PrParity
        } else if checkout && self.local_branch_view.is_some() {
            DiffRoute::CheckoutAgainstBase
        } else {
            DiffRoute::Branch
        };
        // Mirrors `fetch_preview_head`.
        let pairs_preview_head = self.pr_number.is_some()
            && (self.is_remote()
                || mode == DiffMode::PrDiff
                || (mode == DiffMode::Tour && self.tour_is_pr)
                || (self.local_branch_view.is_some() && !checkout));
        Some(DiffSourceKey {
            route,
            pairs_preview_head,
            base_branch: self.base_branch.clone(),
            pr_head_ref: self.pr_head_ref.clone(),
            pr_number: self.pr_number,
            local_branch_view: self.local_branch_view.clone(),
            checkout_root: self.local_branch_checkout_root.clone(),
            remote_repo: self.remote_repo.clone(),
            current_branch: self.current_branch.clone(),
            last_diff_head_oid: self.last_diff_head_oid.clone(),
            diff_hash: self.diff_hash.clone(),
        })
    }

    /// Note that `files` now holds the diff a refresh in the current mode loads.
    pub(super) fn record_loaded_diff_source(&mut self) {
        self.loaded_diff_source = self.diff_source_key(self.mode);
    }

    /// Whether switching from `from` to `to` can keep the loaded diff instead of
    /// fetching it again.
    ///
    /// Only the Tour and the view it regroups qualify, and only while nothing
    /// that decides the fetch has moved since the diff was loaded. Content that
    /// moved under unchanged refs (a working-tree edit) is the watcher's to
    /// pick up: it refreshes whatever mode is current, so the diff it leaves
    /// is the one both views would load.
    pub(crate) fn can_reuse_loaded_diff(&self, from: DiffMode, to: DiffMode) -> bool {
        let tour_and_source = match (from, to) {
            (DiffMode::Tour, other) | (other, DiffMode::Tour) => {
                let source = if self.tour_is_pr {
                    DiffMode::PrDiff
                } else {
                    DiffMode::Branch
                };
                other == source || (self.is_remote() && other == DiffMode::Branch)
            }
            _ => false,
        };
        if !tour_and_source || self.preloaded_branch_raw.is_some() || self.raw_diff.is_none() {
            return false;
        }
        let Some(loaded) = self.loaded_diff_source.as_ref() else {
            return false;
        };
        self.diff_source_key(to).as_ref() == Some(loaded)
    }
}
