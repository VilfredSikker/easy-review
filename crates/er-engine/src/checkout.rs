//! Where a branch is checked out: the repo root or one of its linked
//! worktrees. The desktop's watcher and the workspace's load resolve a tab's
//! checkout root by the same rule.

use crate::git::Git;

/// The working tree where `branch` is checked out: `repo_root` when its
/// HEAD is on `branch`, else a linked worktree on it. None when it is
/// checked out nowhere.
pub fn resolve_head_checkout(git: &dyn Git, repo_root: &str, branch: &str) -> Option<String> {
    if branch.is_empty() {
        return None;
    }
    // An unreadable HEAD (an unborn branch, say) still falls through to the
    // linked-worktree check.
    let head = git.current_branch(repo_root).unwrap_or_default();
    if head == branch {
        return Some(repo_root.to_string());
    }
    let worktrees = git.list_worktrees(repo_root).ok()?;
    let wt = worktrees.into_iter().find(|w| w.branch == branch)?;
    Some(wt.path)
}

/// The branch a tab's checkout follows: its branch view, else the branch its
/// checkout is on. None for a remote tab, which has no checkout.
pub fn watched_branch(
    is_remote: bool,
    branch_view: Option<&str>,
    current_branch: &str,
) -> Option<String> {
    if is_remote {
        return None;
    }
    let branch = branch_view.unwrap_or(current_branch);
    (!branch.is_empty()).then(|| branch.to_string())
}
