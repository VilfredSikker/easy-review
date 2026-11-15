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

/// What a saved tab resolves to before its first diff: the owned fields of a
/// [`TabIdentity`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTab {
    pub repo_root: String,
    pub current_branch: String,
    pub base_branch: String,
    pub mode: DiffMode,
    pub tour_is_pr: bool,
    pub remote_repo: Option<String>,
    pub pr_number: Option<u64>,
    pub local_branch_view: Option<String>,
    pub pr_head_ref: Option<String>,
    pub checkout_root: Option<String>,
}

impl ResolvedTab {
    pub fn identity(&self) -> TabIdentity<'_> {
        TabIdentity {
            repo_root: &self.repo_root,
            current_branch: &self.current_branch,
            base_branch: &self.base_branch,
            mode: self.mode,
            tour_is_pr: self.tour_is_pr,
            remote_repo: self.remote_repo.as_deref(),
            pr_number: self.pr_number,
            local_branch_view: self.local_branch_view.as_deref(),
            pr_head_ref: self.pr_head_ref.as_deref(),
            checkout_root: self.checkout_root.as_deref(),
        }
    }
}

/// Resolve a saved tab the way a restore does: the git reads a tab makes
/// before its first refresh, and no network call. A local PR tab opens on PR
/// Diff and a plain or branch tab on Branch; the checkout root is left unset.
///
/// A remote PR tab has no clone, so it roots at the process's directory, as
/// its constructor does.
pub fn resolve_descriptor(
    git: &dyn er_git::Git,
    d: &crate::tabs_record::TabDescriptor,
) -> anyhow::Result<ResolvedTab> {
    use crate::tabs_record::TabKind;
    use anyhow::Context;

    let non_empty = |value: &Option<String>| value.clone().filter(|s| !s.is_empty());
    if d.kind == TabKind::RemotePr {
        let owner = d.pr_owner.clone().context("remote_pr missing owner")?;
        let repo = d.pr_repo.clone().context("remote_pr missing repo")?;
        let number = d.pr_number.context("remote_pr missing number")?;
        let repo_root = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| ".".to_string());
        return Ok(ResolvedTab {
            repo_root,
            current_branch: non_empty(&d.branch).unwrap_or_default(),
            base_branch: non_empty(&d.base_ref).unwrap_or_default(),
            mode: DiffMode::PrDiff,
            tour_is_pr: false,
            remote_repo: Some(format!("{owner}/{repo}")),
            pr_number: Some(number),
            local_branch_view: None,
            pr_head_ref: None,
            checkout_root: None,
        });
    }

    let (local_branch_view, pr_number, pr_head_ref, mode) = match d.kind {
        TabKind::LocalBranch => {
            let branch = d
                .branch
                .clone()
                .context("local_branch descriptor missing branch")?;
            (Some(branch), None, None, DiffMode::Branch)
        }
        TabKind::LocalPr => {
            let number = d
                .pr_number
                .context("local_pr descriptor missing pr_number")?;
            let view = d.branch.clone().unwrap_or_else(|| format!("pr/{number}"));
            (
                Some(view),
                Some(number),
                d.pr_head_ref.clone(),
                DiffMode::PrDiff,
            )
        }
        TabKind::Working | TabKind::RemotePr => (None, None, None, DiffMode::Branch),
    };
    // Prefer the persisted (user-chosen) base; fall back to auto-detection.
    let base_branch = match non_empty(&d.base_ref) {
        Some(base) => base,
        None => git.detect_base_branch(&d.repo_root)?,
    };
    let current_branch = git.current_branch(&d.repo_root)?;
    Ok(ResolvedTab {
        repo_root: d.repo_root.clone(),
        current_branch,
        base_branch,
        mode,
        tour_is_pr: false,
        remote_repo: None,
        pr_number,
        local_branch_view,
        pr_head_ref,
        checkout_root: None,
    })
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

    /// Whether this tab's reviewed marks are saved. A remote tab outside the PR
    /// bucket has no managed dir of its own to write to.
    pub const fn persists_reviewed(&self) -> bool {
        !self.is_remote() || matches!(self.review_bucket(), ReviewBucket::Pr)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tabs_record::{TabDescriptor, TabKind};
    use er_git::FakeGit;

    fn descriptor(kind: TabKind) -> TabDescriptor {
        TabDescriptor {
            kind,
            repo_root: "/repo".to_string(),
            branch: None,
            pr_owner: None,
            pr_repo: None,
            pr_number: None,
            pr_head_ref: None,
            base_ref: None,
            extra: serde_json::Map::new(),
        }
    }

    fn git() -> FakeGit {
        FakeGit::default()
            .with_current_branch(Ok("checkout".to_string()))
            .with_detect_base_branch(Ok("detected".to_string()))
    }

    #[test]
    fn a_plain_tab_reads_its_checkout_and_detects_its_base() -> anyhow::Result<()> {
        let git = git();
        let resolved = resolve_descriptor(&git, &descriptor(TabKind::Working))?;
        assert_eq!(resolved.current_branch, "checkout");
        assert_eq!(resolved.base_branch, "detected");
        assert_eq!(resolved.mode, DiffMode::Branch);
        assert_eq!(resolved.local_branch_view, None);
        assert_eq!(resolved.identity().review_bucket(), ReviewBucket::Branch);
        assert_eq!(
            git.calls(),
            vec![
                "detect_base_branch(repo_root=\"/repo\")",
                "current_branch(repo_root=\"/repo\")"
            ]
        );
        Ok(())
    }

    #[test]
    fn a_saved_base_skips_detection() -> anyhow::Result<()> {
        let git = git();
        let mut d = descriptor(TabKind::LocalBranch);
        d.branch = Some("feature".to_string());
        d.base_ref = Some("origin/main".to_string());
        let resolved = resolve_descriptor(&git, &d)?;
        assert_eq!(resolved.base_branch, "origin/main");
        assert_eq!(resolved.local_branch_view.as_deref(), Some("feature"));
        assert_eq!(resolved.mode, DiffMode::Branch);
        assert_eq!(git.calls(), vec!["current_branch(repo_root=\"/repo\")"]);
        Ok(())
    }

    #[test]
    fn a_local_pr_tab_opens_on_its_pr_diff() -> anyhow::Result<()> {
        let mut d = descriptor(TabKind::LocalPr);
        d.pr_number = Some(12);
        d.pr_head_ref = Some("refs/er/pr/12/head".to_string());
        let resolved = resolve_descriptor(&git(), &d)?;
        assert_eq!(resolved.mode, DiffMode::PrDiff);
        assert_eq!(resolved.pr_number, Some(12));
        assert_eq!(resolved.local_branch_view.as_deref(), Some("pr/12"));
        assert_eq!(resolved.pr_head_ref.as_deref(), Some("refs/er/pr/12/head"));
        assert_eq!(resolved.remote_repo, None);
        assert_eq!(resolved.identity().review_bucket(), ReviewBucket::Pr);
        Ok(())
    }

    #[test]
    fn a_remote_pr_tab_touches_no_git() -> anyhow::Result<()> {
        let git = git();
        let mut d = descriptor(TabKind::RemotePr);
        d.pr_owner = Some("acme".to_string());
        d.pr_repo = Some("widgets".to_string());
        d.pr_number = Some(7);
        d.branch = Some("head".to_string());
        d.base_ref = Some(String::new());
        let resolved = resolve_descriptor(&git, &d)?;
        assert!(resolved.identity().is_remote());
        assert_eq!(resolved.remote_repo.as_deref(), Some("acme/widgets"));
        assert_eq!(resolved.current_branch, "head");
        assert_eq!(resolved.base_branch, "");
        assert_eq!(resolved.mode, DiffMode::PrDiff);
        assert!(git.calls().is_empty());
        Ok(())
    }

    #[test]
    fn a_descriptor_missing_its_kind_fields_is_refused() {
        let git = git();
        assert!(resolve_descriptor(&git, &descriptor(TabKind::LocalBranch)).is_err());
        assert!(resolve_descriptor(&git, &descriptor(TabKind::LocalPr)).is_err());
        assert!(resolve_descriptor(&git, &descriptor(TabKind::RemotePr)).is_err());
        assert!(git.calls().is_empty(), "a refused descriptor ran git");
    }

    #[test]
    fn only_a_remote_tab_outside_the_pr_bucket_skips_saving_reviewed() {
        let resolved = |remote: Option<&str>, view: Option<&str>, mode| ResolvedTab {
            repo_root: "/repo".into(),
            current_branch: "main".into(),
            base_branch: "main".into(),
            mode,
            tour_is_pr: false,
            remote_repo: remote.map(str::to_string),
            pr_number: Some(1),
            local_branch_view: view.map(str::to_string),
            pr_head_ref: None,
            checkout_root: None,
        };
        assert!(resolved(None, None, DiffMode::Unstaged)
            .identity()
            .persists_reviewed());
        assert!(resolved(Some("a/b"), None, DiffMode::PrDiff)
            .identity()
            .persists_reviewed());
        assert!(resolved(Some("a/b"), Some("feature"), DiffMode::Staged)
            .identity()
            .persists_reviewed());
    }
}
