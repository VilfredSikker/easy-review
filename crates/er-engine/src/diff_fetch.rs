//! The branch-scope diff fetch, without an `App` or `TabState` dependency.
//!
//! Captured inputs go in, the raw diff comes out. The desktop preloader runs
//! it on a worker thread; the service runs it in an effect; `TabState` runs it
//! for a synchronous refresh. All three share this one implementation.

use crate::ports::Ports;

/// Inputs needed to fetch a branch-scope raw diff without holding `&TabState`.
#[derive(Debug, Clone)]
pub struct BranchScopeFetchInputs {
    pub ports: Ports,
    pub repo_root: String,
    pub base_branch: String,
    pub local_branch_view: Option<String>,
    pub pr_head_ref: Option<String>,
    pub pr_number: Option<u64>,
    pub checkout_root: Option<String>,
    pub remote_repo: Option<String>,
}

impl BranchScopeFetchInputs {
    /// Stable identity for in-flight dedupe: (repo, PR). Remote tabs use the
    /// owner/repo slug, local tabs use the checkout root (or repo root).
    pub fn dedupe_key(&self) -> (String, u64) {
        let repo = self
            .remote_repo
            .clone()
            .or_else(|| self.checkout_root.clone())
            .or_else(|| Some(self.repo_root.clone()))
            .unwrap_or_default();
        (repo, self.pr_number.unwrap_or(0))
    }
}

/// Fetch the branch-scope raw diff — the same computation
/// `TabState::fetch_tab_raw_diff("branch")` performs, as a free function so a
/// caller can run it without holding the App lock.
///
/// Handles both local PR tabs (working-tree diff when checked out, `gh pr diff`
/// / `git diff base...ref` otherwise) and remote PR tabs (`gh pr diff --repo`).
pub fn fetch_branch_scope_raw(
    scope: &str,
    inputs: &BranchScopeFetchInputs,
) -> anyhow::Result<String> {
    if let Some(ref branch) = inputs.local_branch_view {
        // A checked-out head branch (worktree or main tree) takes precedence
        // over the fetched PR head ref: Branch/Unstaged/Staged review the live
        // working tree, not `gh pr diff`.
        return if let Some(checkout_root) = inputs.checkout_root.as_deref() {
            match scope {
                "unstaged" | "staged" => inputs
                    .ports
                    .git
                    .diff_raw(scope, &inputs.base_branch, checkout_root, None)
                    .map_err(anyhow::Error::from),
                _ => inputs
                    .ports
                    .git
                    .diff_checkout_against_base(checkout_root, &inputs.base_branch)
                    .map_err(anyhow::Error::from),
            }
        } else if let Some(head_ref) = inputs.pr_head_ref.as_deref() {
            if let Some(pr_number) = inputs.pr_number {
                inputs
                    .ports
                    .github
                    .pr_diff(&*inputs.ports.git, pr_number, &inputs.repo_root)
                    .map_err(anyhow::Error::from)
            } else {
                inputs
                    .ports
                    .git
                    .diff_against_branch(&inputs.repo_root, &inputs.base_branch, head_ref)
                    .map_err(anyhow::Error::from)
            }
        } else if let Some(pr_number) = inputs.pr_number {
            // Local PR tab whose refs haven't landed yet: fetch the PR-parity
            // diff directly instead of the local clone's branch diff.
            inputs
                .ports
                .github
                .pr_diff(&*inputs.ports.git, pr_number, &inputs.repo_root)
                .map_err(anyhow::Error::from)
        } else {
            inputs
                .ports
                .git
                .diff_against_branch(&inputs.repo_root, &inputs.base_branch, branch)
                .map_err(anyhow::Error::from)
        };
    }

    if let Some(ref repo_slug) = inputs.remote_repo {
        let parts: Vec<&str> = repo_slug.split('/').collect();
        if parts.len() == 2 {
            let owner = parts[0];
            let repo = parts[1];
            if let Some(pr_number) = inputs.pr_number {
                return inputs
                    .ports
                    .github
                    .pr_diff_remote(&*inputs.ports.git, owner, repo, pr_number)
                    .map_err(anyhow::Error::from);
            }
        }
        anyhow::bail!("Remote tab missing owner/repo or pr_number");
    }

    anyhow::bail!("Cannot fetch branch diff: tab has no branch view, checkout root, or remote")
}
