//! Background branch-scope diff preloading (desktop).
//!
//! The desktop prefetches the branch-scope raw diff (`git diff base...head`,
//! `gh pr diff`, or the working-tree diff) in the background right after a PR
//! tab opens, so the first switch to the Branch view doesn't run a network
//! `gh` call or a large `git diff` synchronously under the App lock.
//!
//! Two pieces live here:
//! - [`fetch_branch_scope_raw`] — the lock-free fetch, factored out of
//!   `TabState::fetch_tab_raw_diff` so a worker thread can run it with just
//!   captured inputs (never holding the App mutex during subprocess work).
//! - `TabState::take_preloaded_branch_raw` — the one-shot consume with
//!   validation: if any input the fetch depended on (base branch, PR number,
//!   branch name, checkout root, remote slug) moved, the slot is dropped and
//!   the normal synchronous fetch runs instead.

use super::TabState;

/// Branch-scope raw diff prefetched by the desktop in the background.
/// Consumed (one-shot) by the first Branch-view refresh; dropped when the
/// captured inputs no longer match the tab (base/PR/checkout/branch moved).
#[derive(Debug, Clone)]
pub struct PreloadedBranchRaw {
    pub raw: String,
    pub base_branch: String,
    pub pr_number: Option<u64>,
    pub local_branch_view: Option<String>,
    pub checkout_root: Option<String>,
    pub remote_repo: Option<String>,
    pub pr_head_ref: Option<String>,
}

/// Inputs needed to fetch a branch-scope raw diff without holding `&TabState`.
/// Captured under the App lock by the desktop preloader, then handed to
/// [`fetch_branch_scope_raw`] on a worker thread.
#[derive(Debug, Clone)]
pub struct BranchScopeFetchInputs {
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
/// background thread can run it without holding the App lock. Handles both
/// local PR tabs (working-tree diff when checked out, `gh pr diff` /
/// `git diff base...ref` otherwise) and remote PR tabs (`gh pr diff --repo`).
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
                "unstaged" | "staged" => {
                    crate::git::git_diff_raw(scope, &inputs.base_branch, checkout_root, None)
                }
                _ => crate::git::git_diff_checkout_against_base(checkout_root, &inputs.base_branch),
            }
        } else if let Some(head_ref) = inputs.pr_head_ref.as_deref() {
            if let Some(pr_number) = inputs.pr_number {
                crate::github::gh_pr_diff(pr_number, &inputs.repo_root)
            } else {
                crate::git::git_diff_against_branch(
                    &inputs.repo_root,
                    &inputs.base_branch,
                    head_ref,
                )
            }
        } else {
            crate::git::git_diff_against_branch(&inputs.repo_root, &inputs.base_branch, branch)
        };
    }

    if let Some(ref repo_slug) = inputs.remote_repo {
        let parts: Vec<&str> = repo_slug.split('/').collect();
        if parts.len() == 2 {
            let owner = parts[0];
            let repo = parts[1];
            if let Some(pr_number) = inputs.pr_number {
                return crate::github::gh_pr_diff_remote(owner, repo, pr_number);
            }
        }
        anyhow::bail!("Remote tab missing owner/repo or pr_number");
    }

    anyhow::bail!("Cannot fetch branch diff: tab has no branch view, checkout root, or remote")
}

impl TabState {
    /// One-shot consume of the background-prefetched branch raw diff.
    ///
    /// Returns `None` (and still drops the slot) when any input the fetch
    /// depended on has moved — base branch, PR number, branch name, checkout
    /// root, or remote slug. Callers then run the normal synchronous fetch.
    /// Only meaningful for `scope == "branch"` refreshes; callers guard that.
    pub fn take_preloaded_branch_raw(&mut self) -> Option<String> {
        let pre = self.preloaded_branch_raw.take()?;
        let valid = pre.base_branch == self.base_branch
            && pre.pr_number == self.pr_number
            && pre.local_branch_view == self.local_branch_view
            && pre.checkout_root == self.local_branch_checkout_root
            && pre.remote_repo == self.remote_repo
            && pre.pr_head_ref == self.pr_head_ref;
        if valid {
            Some(pre.raw)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_preloaded(raw: &str) -> PreloadedBranchRaw {
        PreloadedBranchRaw {
            raw: raw.to_string(),
            base_branch: "main".to_string(),
            pr_number: Some(42),
            local_branch_view: Some("feature-x".to_string()),
            checkout_root: None,
            remote_repo: None,
            pr_head_ref: Some("refs/er/pr/42/head".to_string()),
        }
    }

    fn tab_with(f: impl FnOnce(&mut TabState)) -> TabState {
        let mut tab = TabState::new_for_test(vec![]);
        tab.base_branch = "main".to_string();
        tab.pr_number = Some(42);
        tab.local_branch_view = Some("feature-x".to_string());
        tab.pr_head_ref = Some("refs/er/pr/42/head".to_string());
        f(&mut tab);
        tab
    }

    #[test]
    fn take_consumes_once_and_returns_raw_when_inputs_match() {
        let mut tab = tab_with(|t| t.preloaded_branch_raw = Some(make_preloaded("raw-branch")));
        assert_eq!(
            tab.take_preloaded_branch_raw().as_deref(),
            Some("raw-branch")
        );
        assert!(tab.take_preloaded_branch_raw().is_none(), "one-shot");
    }

    #[test]
    fn take_drops_slot_when_pr_number_changed() {
        let mut tab = tab_with(|t| t.preloaded_branch_raw = Some(make_preloaded("raw")));
        tab.pr_number = Some(99);
        assert!(tab.take_preloaded_branch_raw().is_none());
        assert!(tab.preloaded_branch_raw.is_none(), "slot dropped");
    }

    #[test]
    fn take_drops_slot_when_base_branch_changed() {
        let mut tab = tab_with(|t| t.preloaded_branch_raw = Some(make_preloaded("raw")));
        tab.base_branch = "develop".to_string();
        assert!(tab.take_preloaded_branch_raw().is_none());
    }

    #[test]
    fn take_drops_slot_when_checkout_root_appears() {
        let mut tab = tab_with(|t| t.preloaded_branch_raw = Some(make_preloaded("raw")));
        tab.local_branch_checkout_root = Some("/worktree".to_string());
        assert!(tab.take_preloaded_branch_raw().is_none());
    }

    #[test]
    fn take_drops_slot_when_branch_name_changed() {
        let mut tab = tab_with(|t| t.preloaded_branch_raw = Some(make_preloaded("raw")));
        tab.local_branch_view = Some("other-branch".to_string());
        assert!(tab.take_preloaded_branch_raw().is_none());
    }

    #[test]
    fn take_drops_slot_when_remote_flips() {
        let mut tab = tab_with(|t| t.preloaded_branch_raw = Some(make_preloaded("raw")));
        tab.remote_repo = Some("owner/repo".to_string());
        assert!(tab.take_preloaded_branch_raw().is_none());
    }

    #[test]
    fn take_drops_slot_when_head_ref_moves() {
        let mut tab = tab_with(|t| t.preloaded_branch_raw = Some(make_preloaded("raw")));
        tab.pr_head_ref = Some("refs/er/pr/99/head".to_string());
        assert!(tab.take_preloaded_branch_raw().is_none());
    }

    #[test]
    fn dedupe_key_prefers_remote_slug_then_checkout_root() {
        let local = BranchScopeFetchInputs {
            repo_root: "/r".to_string(),
            base_branch: "main".to_string(),
            local_branch_view: Some("f".to_string()),
            pr_head_ref: Some("refs/er/pr/7/head".to_string()),
            pr_number: Some(7),
            checkout_root: Some("/wt".to_string()),
            remote_repo: None,
        };
        assert_eq!(local.dedupe_key(), ("/wt".to_string(), 7));
        let remote = BranchScopeFetchInputs {
            remote_repo: Some("owner/repo".to_string()),
            checkout_root: None,
            ..local.clone()
        };
        assert_eq!(remote.dedupe_key(), ("owner/repo".to_string(), 7));
    }

    #[test]
    fn branch_mode_refresh_consumes_preloaded_raw() {
        const CONSTRUCTOR_DIFF: &str = "diff --git a/a.rs b/a.rs\nindex 0000000..1111111 100644\n--- a/a.rs\n+++ b/a.rs\n@@ -1 +1,2 @@\n fn a() {}\n+fn a2() {}\n";
        const PRELOADED_DIFF: &str = "diff --git a/b.rs b/b.rs\nindex 0000000..2222222 100644\n--- a/b.rs\n+++ b/b.rs\n@@ -1 +1,2 @@\n fn b() {}\n+fn b2() {}\n";
        // A local PR tab (mode=Branch, no checkout root). The branch-scope
        // fetch would be `gh pr diff` (network) — with the preload present it
        // must be skipped entirely and the files parsed from the preloaded raw.
        let mut tab = TabState::new_for_test(crate::git::parse_diff(CONSTRUCTOR_DIFF));
        tab.base_branch = "main".to_string();
        tab.pr_number = Some(42);
        tab.local_branch_view = Some("feature-x".to_string());
        tab.pr_head_ref = Some("refs/er/pr/42/head".to_string());
        tab.preloaded_branch_raw = Some(PreloadedBranchRaw {
            raw: PRELOADED_DIFF.to_string(),
            base_branch: "main".to_string(),
            pr_number: Some(42),
            local_branch_view: Some("feature-x".to_string()),
            checkout_root: None,
            remote_repo: None,
            pr_head_ref: Some("refs/er/pr/42/head".to_string()),
        });
        tab.refresh_diff_mode_switch().unwrap();
        assert!(tab.preloaded_branch_raw.is_none(), "preload consumed");
        assert_eq!(tab.files.len(), 1);
        assert_eq!(tab.files[0].path, "b.rs", "files parsed from preloaded raw");
    }

    #[test]
    fn pr_diff_mode_refresh_does_not_consume_branch_preload() {
        const PR_DIFF: &str = "diff --git a/a.rs b/a.rs\nindex 0000000..1111111 100644\n--- a/a.rs\n+++ b/a.rs\n@@ -1 +1,2 @@\n fn a() {}\n+fn a2() {}\n";
        const PRELOADED_DIFF: &str = "diff --git a/b.rs b/b.rs\nindex 0000000..2222222 100644\n--- a/b.rs\n+++ b/b.rs\n@@ -1 +1,2 @@\n fn b() {}\n+fn b2() {}\n";
        // A local PR tab sitting in PrDiff mode with a pending branch preload.
        // A refresh in PrDiff mode must NOT consume it — the branch raw is
        // wrong-scope for the PrDiff view (which shows the parity diff).
        let mut tab = TabState::new_for_test(crate::git::parse_diff(PR_DIFF));
        tab.base_branch = "main".to_string();
        tab.pr_number = Some(42);
        tab.local_branch_view = Some("feature-x".to_string());
        tab.pr_head_ref = Some("refs/er/pr/42/head".to_string());
        tab.mode = super::super::DiffMode::PrDiff;
        tab.preloaded_branch_raw = Some(PreloadedBranchRaw {
            raw: PRELOADED_DIFF.to_string(),
            base_branch: "main".to_string(),
            pr_number: Some(42),
            local_branch_view: Some("feature-x".to_string()),
            checkout_root: None,
            remote_repo: None,
            pr_head_ref: Some("refs/er/pr/42/head".to_string()),
        });
        // The refresh will try `gh pr diff` (unavailable in tests) and fail —
        // that is fine; the point is the preload must survive untouched.
        // (`refresh_diff_mode_switch` — not `refresh_diff`, which clears the
        // preload by design — exercises the mode guard.)
        let _ = tab.refresh_diff_mode_switch();
        assert!(
            tab.preloaded_branch_raw.is_some(),
            "PrDiff-mode refresh must not burn the branch preload"
        );
    }

    #[test]
    fn explicit_refresh_drops_remote_preload() {
        // A remote PR tab with a seeded branch preload: an explicit
        // `refresh_diff` (the desktop Refresh command) must NOT consume the
        // open-time preload — it clears it so the fetch hits the network
        // (which fails in tests; the drop is the asserted behavior).
        let mut tab = TabState::new_for_test(vec![]);
        tab.remote_repo = Some("owner/repo".to_string());
        tab.pr_number = Some(7);
        tab.preloaded_branch_raw = Some(PreloadedBranchRaw {
            raw: "diff".to_string(),
            base_branch: "main".to_string(),
            pr_number: Some(7),
            local_branch_view: None,
            checkout_root: None,
            remote_repo: Some("owner/repo".to_string()),
            pr_head_ref: None,
        });
        let _ = tab.refresh_diff();
        assert!(
            tab.preloaded_branch_raw.is_none(),
            "explicit refresh must drop the preload (fresh fetch required)"
        );
    }

    #[test]
    fn new_remote_with_data_builds_tab_without_network() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("ER_STORAGE_ROOT", tmp.path());
        let pr_ref = crate::github::PrRef {
            owner: "o".to_string(),
            repo: "r".to_string(),
            number: 7,
        };
        const DIFF: &str = "diff --git a/f.rs b/f.rs\nindex 0000000..1111111 100644\n--- a/f.rs\n+++ b/f.rs\n@@ -1 +1,2 @@\n fn f() {}\n+fn f2() {}\n";
        let tab = TabState::new_remote_with_data(
            &pr_ref,
            "main".to_string(),
            "feature".to_string(),
            DIFF.to_string(),
            Vec::new(),
        )
        .unwrap();
        assert_eq!(tab.remote_repo.as_deref(), Some("o/r"));
        assert_eq!(tab.pr_number, Some(7));
        assert_eq!(tab.base_branch, "main");
        assert_eq!(tab.current_branch, "feature");
        assert_eq!(tab.files.len(), 1);
        assert_eq!(tab.files[0].path, "f.rs");
        assert!(!tab.needs_initial_refresh, "loaded tab is not a stub");
    }
}
