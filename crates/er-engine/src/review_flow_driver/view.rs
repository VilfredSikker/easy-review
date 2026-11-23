//! What a review flow reads from the tab it reviews. The App reads it from
//! its tab and the workspace service from a tab's published content, both by
//! the rules here, so the two hosts review the same diff of the same view.

use anyhow::{bail, Result};

use super::ReviewFlowInputs;
use crate::ai::change_facts::OwnedRepoRules;
use crate::ai::review_md::ReviewMdSource;
use crate::arena::ArenaScope;
use crate::ports::Ports;
use crate::tab_identity::{DiffMode, TabIdentity};
use crate::task_target::BackgroundTaskTarget;

/// A tab a flow can review, as its host holds it.
pub trait ReviewedTab {
    fn identity(&self) -> TabIdentity<'_>;
    /// The bucket the tab's private sidecars live in, in the mode it shows.
    fn er_dir(&self) -> String;
    /// The unfiltered diff a review of `scope` reads.
    fn raw_diff(&self, scope: &str) -> Result<String>;
    fn owned_rules(&self) -> OwnedRepoRules;
    fn review_md_source(&self) -> ReviewMdSource;
}

/// Whether the diff a tab shows is the one a review of `scope` reads, so a
/// review need not fetch it again. A remote tab and a branch view with no
/// checkout show their one diff in every view.
pub fn cached_diff_matches_scope(id: &TabIdentity<'_>, scope: &str) -> bool {
    if id.remote_repo.is_some() {
        return true;
    }
    if id.local_branch_view.is_some() && id.checkout_root.is_none() {
        return true;
    }
    match scope {
        "unstaged" => id.mode == DiffMode::Unstaged,
        "staged" => id.mode == DiffMode::Staged,
        // Tour is the branch diff regrouped, so its cached raw_diff is the
        // branch diff and matches a "branch"-scoped review.
        _ => matches!(
            id.mode,
            DiffMode::Branch | DiffMode::PrDiff | DiffMode::Tour
        ),
    }
}

/// The checkout that has the viewed branch, where a review's providers run.
fn checkout_root<'a>(id: &TabIdentity<'a>) -> Option<&'a str> {
    if id.is_remote() {
        return None;
    }
    if id.local_branch_view.is_some() {
        return id.checkout_root;
    }
    Some(id.repo_root)
}

/// `raw` without the files the project's review-ignore globs name.
pub fn filtered_review_diff(raw: &str, repo_root: &str, remote_repo: Option<&str>) -> String {
    let globs = crate::projects_pins::review_ignore_globs_for_repo(repo_root, remote_repo);
    crate::git::filter_raw_diff_exclude_globs(raw, &globs)
}

/// The view to review for `scope`, or why it cannot be reviewed.
pub fn review_flow_inputs(tab: &dyn ReviewedTab, scope: &str) -> Result<ReviewFlowInputs> {
    let id = tab.identity();
    let arena_scope = match (scope, id.mode) {
        ("branch", DiffMode::Branch | DiffMode::PrDiff | DiffMode::Tour) => ArenaScope::Branch,
        ("unstaged", DiffMode::Unstaged) => ArenaScope::Unstaged,
        ("staged", DiffMode::Staged) => ArenaScope::Staged,
        _ => bail!("Review requires the active branch, staged, or unstaged diff"),
    };
    if id.repo_root.trim().is_empty() || id.is_remote() {
        bail!("Review requires a local repository view");
    }
    let branch_ref = id.branch_ref().to_owned();
    // A PR or branch view whose head has no worktree runs in the main
    // checkout, like the other review actions, and the prompts name the
    // ref that holds the reviewed code. Staged and unstaged diffs only
    // exist in a checkout of the viewed branch.
    let (execution_root, reviewed_ref_note) = match checkout_root(&id) {
        Some(root) => (root.to_owned(), None),
        None if arena_scope == ArenaScope::Branch => (
            id.repo_root.to_owned(),
            Some(crate::ai::prompts::reviewed_ref_section(
                id.pr_head_ref.unwrap_or(&branch_ref),
                id.pr_number,
            )),
        ),
        None => {
            bail!("Review of staged or unstaged changes requires a checkout of the viewed branch")
        }
    };
    let raw = tab.raw_diff(scope)?;
    let filtered = filtered_review_diff(&raw, id.repo_root, id.remote_repo);
    if filtered.trim().is_empty() {
        bail!("Nothing to review after ignored files are removed");
    }
    let target = BackgroundTaskTarget {
        repo_root: id.repo_root.to_owned(),
        er_dir: tab.er_dir(),
        branch_label: branch_ref.clone(),
        base_branch: id.base_branch.to_owned(),
        scope: scope.to_owned(),
        pr_number: id.pr_number,
        remote_repo: id.remote_repo.map(str::to_owned),
        managed_local: id.remote_repo.is_none(),
    };
    Ok(ReviewFlowInputs {
        target,
        arena_scope,
        repo_root: execution_root,
        reviewed_ref_note,
        branch_ref,
        base_branch: id.base_branch.to_owned(),
        raw_diff: filtered,
        owned_rules: tab.owned_rules(),
        review_md_source: tab.review_md_source(),
    })
}

/// Whether a tab showing `id` on `er_dir` shows `target`, the view a flow
/// captured, so acceptance can read that view's diff again.
pub fn shows_target(id: &TabIdentity<'_>, er_dir: &str, target: &BackgroundTaskTarget) -> bool {
    let scope_matches = match target.scope.as_str() {
        "branch" => matches!(
            id.mode,
            DiffMode::Branch | DiffMode::PrDiff | DiffMode::Tour
        ),
        "staged" => id.mode == DiffMode::Staged,
        "unstaged" => id.mode == DiffMode::Unstaged,
        _ => false,
    };
    id.repo_root == target.repo_root
        && er_dir == target.er_dir
        && id.branch_ref() == target.branch_label
        && id.base_branch == target.base_branch
        && id.pr_number == target.pr_number
        && id.remote_repo == target.remote_repo.as_deref()
        && id.is_remote() == (target.remote_repo.is_some() && id.local_branch_view.is_none())
        && scope_matches
}

/// Where a tab's base-branch `REVIEW.md` files are read from.
pub fn review_md_source(ports: &Ports, id: &TabIdentity<'_>) -> ReviewMdSource {
    if id.base_branch.is_empty() {
        return ReviewMdSource::None;
    }
    if !id.is_remote() {
        return ReviewMdSource::Git {
            repo_root: id.repo_root.to_owned(),
            base_ref: id.base_branch.to_owned(),
        };
    }
    match id.remote_repo {
        Some(slug) => match ports
            .github
            .local_checkout_for_repo(&*ports.git, id.repo_root, slug)
        {
            Some(path) => ReviewMdSource::Git {
                repo_root: path,
                base_ref: format!("origin/{}", id.base_branch),
            },
            None => ReviewMdSource::Gh {
                repo_slug: slug.to_owned(),
                base_ref: id.base_branch.to_owned(),
            },
        },
        None => ReviewMdSource::None,
    }
}
