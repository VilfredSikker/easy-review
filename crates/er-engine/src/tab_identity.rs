//! What a tab reviews, and the storage buckets that follows from it.
//!
//! A tab's identity is its repo, branch, PR and view mode. Everything that
//! decides where its sidecars live is a function of those fields, so it lives
//! here, in the half of the engine that always compiles: `TabState` resolves
//! its buckets through these functions, and the headless service resolves the
//! same buckets for the tabs it loads.

use crate::paths::ErRoot;

/// Which set of changes we're viewing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffMode {
    Branch,
    Unstaged,
    Staged,
    History,
    Conflicts,
    Hidden,
    PrDiff,
    /// AI guided walkthrough — branch diff reordered/grouped into pillars.
    Tour,
}

impl DiffMode {
    #[cfg(test)]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Branch => "BRANCH DIFF",
            Self::Unstaged => "UNSTAGED",
            Self::Staged => "STAGED",
            Self::History => "HISTORY",
            Self::Conflicts => "CONFLICTS",
            Self::Hidden => "HIDDEN",
            Self::PrDiff => "PR DIFF",
            Self::Tour => "TOUR",
        }
    }

    pub const fn git_mode(&self) -> &'static str {
        match self {
            Self::Branch => "branch",
            Self::Unstaged => "unstaged",
            Self::Staged => "staged",
            Self::History => "history",
            Self::Conflicts => "conflicts",
            Self::Hidden => "hidden",
            // PrDiff uses branch-style git diff under the hood (PR head vs base).
            // "pr" is used as the session-persistence key and bucket identifier only.
            Self::PrDiff => "pr",
            // Tour reorders the branch diff; it shares the branch git scope/bucket.
            Self::Tour => "tour",
        }
    }

    /// The scope string passed to `git_diff_raw` / `fetch_tab_raw_diff`.
    /// PrDiff diffs against a PR head ref — same git mechanics as `branch`.
    /// Tour walks the branch diff, so it fetches with branch mechanics too.
    pub const fn fetch_scope(&self) -> &'static str {
        match self {
            Self::PrDiff | Self::Tour => "branch",
            other => other.git_mode(),
        }
    }
}

/// Which isolated review bucket a tab's diff belongs to.
/// Each bucket gets its own storage directory so review notes never bleed across views.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewBucket {
    Branch,
    Unstaged,
    Staged,
    History,
    Pr,
}

/// Which review bucket a tab belongs to.
/// Remote tabs (`remote_repo.is_some()`) always map to `Pr` regardless of `mode`.
/// Tour mode follows the originating view via `tour_is_pr` so Guide stays on the
/// same `review.json` as PR Diff / Local Branch (does not swap onto a stale
/// branch-bucket review when opened from PR Diff).
pub const fn review_bucket(
    has_remote_repo: bool,
    mode: DiffMode,
    tour_is_pr: bool,
) -> ReviewBucket {
    if has_remote_repo {
        return ReviewBucket::Pr;
    }
    match mode {
        DiffMode::PrDiff => ReviewBucket::Pr,
        DiffMode::Tour => {
            if tour_is_pr {
                ReviewBucket::Pr
            } else {
                ReviewBucket::Branch
            }
        }
        DiffMode::Unstaged => ReviewBucket::Unstaged,
        DiffMode::Staged => ReviewBucket::Staged,
        DiffMode::History => ReviewBucket::History,
        _ => ReviewBucket::Branch,
    }
}

/// Whether the current view's guided tour belongs to the PR diff (vs the
/// local branch diff). Remote-only tabs and PR Diff mode are always
/// PR-scoped; Tour mode follows whichever view it was entered from
/// (`tour_is_pr`). Local PR tabs store a GitHub slug for status keys; that
/// does not make Branch/Unstaged/Staged PR-scoped. All other modes are
/// branch-scoped.
pub const fn tour_context_is_pr(is_remote: bool, mode: DiffMode, tour_is_pr: bool) -> bool {
    if is_remote {
        return true;
    }
    match mode {
        DiffMode::PrDiff => true,
        DiffMode::Tour => tour_is_pr,
        _ => false,
    }
}

/// The fields of one tab that decide its buckets and its diff fetch, borrowed
/// from whoever owns the tab.
#[derive(Debug, Clone, Copy)]
pub struct TabIdentity<'a> {
    pub repo_root: &'a str,
    pub current_branch: &'a str,
    pub base_branch: &'a str,
    pub mode: DiffMode,
    pub tour_is_pr: bool,
    pub remote_repo: Option<&'a str>,
    pub pr_number: Option<u64>,
    pub local_branch_view: Option<&'a str>,
    /// The fetched PR head ref a local PR tab diffs against.
    pub pr_head_ref: Option<&'a str>,
    /// The working tree that has the viewed branch checked out, if any.
    pub checkout_root: Option<&'a str>,
}

impl<'a> TabIdentity<'a> {
    /// Whether this tab is reviewing a remote-only PR (no local clone).
    pub const fn is_remote(&self) -> bool {
        self.remote_repo.is_some() && self.local_branch_view.is_none()
    }

    pub const fn review_bucket(&self) -> ReviewBucket {
        review_bucket(self.remote_repo.is_some(), self.mode, self.tour_is_pr)
    }

    pub const fn tour_context_is_pr(&self) -> bool {
        tour_context_is_pr(self.is_remote(), self.mode, self.tour_is_pr)
    }

    /// Sub-directory name for local view buckets (branch/unstaged/staged/history).
    /// Not used for the `Pr` bucket (which lives under `prs/pr-<N>/`).
    pub const fn review_bucket_name(&self) -> &'static str {
        match self.review_bucket() {
            ReviewBucket::Unstaged => "unstaged",
            ReviewBucket::Staged => "staged",
            ReviewBucket::History => "history",
            ReviewBucket::Branch | ReviewBucket::Pr => "branch",
        }
    }

    /// The branch whose view buckets this tab uses: the viewed branch, else the
    /// checkout's.
    fn bucket_branch(&self) -> &'a str {
        self.local_branch_view.unwrap_or(self.current_branch)
    }

    /// PR bucket dir (`prs/pr-<N>/`) for a PR tab (local clone or remote), or
    /// `None` when the tab has no PR. Mirrors the `Pr`-bucket arm of
    /// [`Self::managed_root`]; needs a resolvable owner/repo for local clones.
    pub fn pr_bucket_er_dir(&self, git: &dyn er_git::Git) -> Option<String> {
        let pr_num = self.pr_number?;
        let owner_repo_slug = if let Some(remote_repo) = self.remote_repo {
            crate::storage::repo_storage_slug_for(git, self.repo_root, Some(remote_repo))
        } else {
            crate::github::canonical_owner_repo_slug(git, self.repo_root)?
        };
        Some(
            crate::storage::pr_bucket_dir(&owner_repo_slug, pr_num)
                .to_string_lossy()
                .into_owned(),
        )
    }

    /// Directory of the "branch" review bucket for this tab — where `tour.json`
    /// lives — regardless of the currently active view bucket. The guided tour
    /// walks the branch diff, so it lives in the per-branch "branch" view bucket
    /// and stays discoverable from the working-tree views (Branch/Unstaged/Staged/
    /// History/Tour) — but not the PR Diff view (see `reload_ai_state`). Remote
    /// tabs have no working tree, so the PR bucket is their only bucket. Returns
    /// `None` when storage can't be resolved (empty repo/branch). Uses pure path
    /// helpers (no dir creation).
    pub fn branch_bucket_er_dir(&self, git: &dyn er_git::Git) -> Option<String> {
        // Remote tabs have no local working tree — the PR bucket is their only bucket.
        if let Some(remote_repo) = self.remote_repo {
            let pr_num = self.pr_number?;
            let owner_repo_slug =
                crate::storage::repo_storage_slug_for(git, self.repo_root, Some(remote_repo));
            return Some(
                crate::storage::pr_bucket_dir(&owner_repo_slug, pr_num)
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        // Local tabs (including local PR tabs) keep the tour with the branch.
        let branch = self.bucket_branch();
        if branch.is_empty() || self.repo_root.is_empty() {
            return None;
        }
        let repo_slug = crate::storage::repo_storage_slug_from_root(git, self.repo_root);
        let branch_slug = crate::storage::slug_branch(branch);
        Some(
            crate::storage::view_bucket_dir(&repo_slug, &branch_slug, "branch")
                .to_string_lossy()
                .into_owned(),
        )
    }

    /// Storage bucket for the tour attached to the current view context: the PR bucket
    /// when the active context is the PR diff (PrDiff, remote, or a Guide tab opened from
    /// the PR view — see `tour_context_is_pr`), otherwise the branch bucket. Both
    /// `generate_tour` (write) and `resolve_view_tour` (read) route through this, so a
    /// tour is always written where it will be read — including when "Re-run guide" is
    /// clicked from inside the Guide (Tour mode), where the active mode alone is ambiguous.
    pub fn tour_bucket_er_dir(&self, git: &dyn er_git::Git) -> Option<String> {
        if self.tour_context_is_pr() {
            self.pr_bucket_er_dir(git)
        } else {
            self.branch_bucket_er_dir(git)
        }
    }

    /// The bucket the other tour context reads from: the branch bucket for a PR
    /// context, else the PR bucket.
    pub fn cross_tour_bucket_er_dir(&self, git: &dyn er_git::Git) -> Option<String> {
        if self.tour_context_is_pr() {
            self.branch_bucket_er_dir(git)
        } else {
            self.pr_bucket_er_dir(git)
        }
    }

    /// The managed root this tab's active bucket resolves to, or `None` when
    /// it cannot be resolved (empty repo or branch) and the caller should keep
    /// the root it has.
    ///
    /// Only the PR Diff view (head-vs-base) uses the shared PR bucket
    /// (prs/pr-<N>/). Its review artifacts — triage, review.json, questions,
    /// reviewed — stay separate from the local working-tree views. Remote tabs
    /// are always PrDiff (review_bucket() == Pr). Branch/Unstaged/Staged/History
    /// fall through to per-branch view buckets even on a local PR tab: the local
    /// branch diff is a different diff than the PR head-vs-base diff, so it is
    /// reviewed independently.
    pub fn managed_root(&self, git: &dyn er_git::Git) -> Option<ErRoot> {
        if let Some(pr_num) = self.pr_number {
            if self.review_bucket() == ReviewBucket::Pr {
                let owner_repo_slug = if let Some(remote_repo) = self.remote_repo {
                    Some(crate::storage::repo_storage_slug_for(
                        git,
                        self.repo_root,
                        Some(remote_repo),
                    ))
                } else {
                    crate::github::canonical_owner_repo_slug(git, self.repo_root)
                };
                // Only the PR bucket needs the owner/repo slug. If it can't be resolved
                // (no usable git remote), fall through to the local view bucket rather
                // than leaving er_root pointing at a previous mode's bucket.
                if let Some(slug) = owner_repo_slug {
                    return Some(crate::storage::resolve_managed_root_for_pr_bucket(
                        &slug, pr_num,
                    ));
                }
            }
        }

        // Local view buckets (branch/unstaged/staged/history)
        let branch = self.bucket_branch();
        if branch.is_empty() || self.repo_root.is_empty() {
            return None;
        }
        let repo_slug = crate::storage::repo_storage_slug_from_root(git, self.repo_root);
        let branch_slug = crate::storage::slug_branch(branch);
        let bucket = self.review_bucket_name();

        // Per-bucket dirs start clean — no migration from repo `.er/` or legacy cache.
        // Migrating here would copy the same data into every bucket (branch/unstaged/
        // staged/history) and break isolation between them.
        Some(crate::storage::resolve_managed_root_for_view_bucket(
            &repo_slug,
            &branch_slug,
            bucket,
        ))
    }

    /// Branch name used to resolve managed storage for this tab (for sidecar validation).
    ///
    /// A PR tab opened before GitHub answered with its head branch carries the
    /// `pr/<N>` placeholder as its view label. That label is not a branch, so
    /// it yields no scope — returning it made `artifacts_branch_mismatch`
    /// discard every sidecar whose `head_branch` named the real PR head, and
    /// the review card showed "No findings" / "fresh" over a complete review.
    pub fn storage_branch_scope(&self) -> Option<&'a str> {
        if let Some(view) = self.local_branch_view {
            if self.pr_number.is_some() && crate::storage::is_pr_placeholder_branch(view) {
                return None;
            }
            return Some(view);
        }
        if self.remote_repo.is_some() || self.current_branch.is_empty() {
            // PR storage is keyed by pr-{n}; path isolation is enough.
            None
        } else {
            Some(self.current_branch)
        }
    }
}
