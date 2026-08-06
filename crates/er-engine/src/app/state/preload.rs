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
///
/// `parity` marks content that is the PR-parity diff (`gh pr diff`) — it
/// depends only on the PR number, so `pr_head_ref`/`base_branch` drift after
/// open must not drop it (the background ref fetch lands ~1.8 s after open,
/// before the user's first Branch click).
#[derive(Debug, Clone)]
pub struct PreloadedBranchRaw {
    pub raw: String,
    pub base_branch: String,
    pub pr_number: Option<u64>,
    pub local_branch_view: Option<String>,
    pub checkout_root: Option<String>,
    pub remote_repo: Option<String>,
    pub pr_head_ref: Option<String>,
    /// True when `raw` is the PR-parity diff (ref-independent). All current
    /// preload sources are parity; kept as a field so a future non-parity
    /// preload (e.g. a checked-out worktree diff) keeps the strict ref check.
    pub parity: bool,
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
        } else if let Some(pr_number) = inputs.pr_number {
            // Local PR tab whose refs haven't landed yet: fetch the PR-parity
            // diff directly instead of the local clone's branch diff — this is
            // what the consume-time fetch produces once the refs land, so the
            // preload stays valid across the ref-fetch drift (and doesn't
            // silently fail when the branch isn't checked out locally).
            crate::github::gh_pr_diff(pr_number, &inputs.repo_root)
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
        // `base_branch` may drift from its bare local name (e.g. "main") to
        // "origin/main" when the background ref fetch lands after open — for
        // local PR tabs the branch scope is `gh pr diff`, which does not
        // depend on the base name, so compare normalized (both sides).
        let base_matches = normalize_base(&pre.base_branch) == normalize_base(&self.base_branch);
        // `pr_head_ref` drifts from None to Some when the background ref
        // fetch lands after open. Parity content (`gh pr diff`) depends only
        // on the PR number, so ref drift must not drop it (collapsed Branch
        // view used to wait on a synchronous fetch).
        let ref_matches = pre.pr_head_ref == self.pr_head_ref || pre.parity;
        let valid = base_matches
            && ref_matches
            && pre.pr_number == self.pr_number
            && pre.local_branch_view == self.local_branch_view
            && pre.checkout_root == self.local_branch_checkout_root
            && pre.remote_repo == self.remote_repo;
        if valid {
            Some(pre.raw)
        } else {
            None
        }
    }

    /// One-shot consume of the background-loaded branch AI state. Returns the
    /// state only when both the bucket dir and the diff hash still match the
    /// tab's current branch view; the slot is KEPT on mismatch (an unrelated
    /// view's reload — e.g. the PR bucket at open — must not destroy the
    /// branch preload before the first Branch click). Note: under
    /// `ER_REPO_LOCAL=1` every bucket collapses to `{repo}/.er`, so only the
    /// diff hash separates the PR reload from the slot — harmless in practice
    /// (the open-time hash is empty), but worth knowing.
    pub fn take_preloaded_branch_ai(
        &mut self,
        bucket_dir: &str,
        diff_hash: &str,
    ) -> Option<super::AiState> {
        if let Some(pre) = self.preloaded_branch_ai.as_ref() {
            if pre.bucket_dir == bucket_dir && pre.diff_hash == diff_hash {
                return self.preloaded_branch_ai.take().map(|p| p.ai);
            }
        }
        None
    }
}

/// Strip an `origin/` prefix for base-branch drift comparison.
fn normalize_base(base: &str) -> &str {
    base.strip_prefix("origin/").unwrap_or(base)
}

/// Background-loaded AI sidecar state for the branch view bucket, so the
/// first switch to the Branch view doesn't re-read + re-parse every sidecar
/// under the App lock (the diff itself is preloaded via
/// [`PreloadedBranchRaw`]). Adopted by `reload_ai_state` only when the bucket
/// dir and diff hash still match.
pub struct BranchAiPreload {
    pub bucket_dir: String,
    pub diff_hash: String,
    pub ai: super::AiState,
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
            parity: true,
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
    fn take_survives_origin_prefix_drift_on_base_branch() {
        // `kick_pr_ref_fetch` rewrites `tab.base_branch` from "main" to
        // "origin/main" ~1.8 s after open. The branch scope for local PR tabs
        // is `gh pr diff`, which does not depend on the base name — the
        // one-shot preload must survive the drift (review-fix-loop Spec-1).
        let mut tab = tab_with(|t| t.preloaded_branch_raw = Some(make_preloaded("raw")));
        assert_eq!(tab.base_branch, "main");
        tab.base_branch = "origin/main".to_string();
        assert_eq!(tab.take_preloaded_branch_raw().as_deref(), Some("raw"));
        // And the reverse drift (origin-prefixed capture, bare tab) too.
        let mut tab = tab_with(|t| {
            let mut pre = make_preloaded("raw2");
            pre.base_branch = "origin/main".to_string();
            t.preloaded_branch_raw = Some(pre);
        });
        tab.base_branch = "main".to_string();
        assert_eq!(tab.take_preloaded_branch_raw().as_deref(), Some("raw2"));
    }

    #[test]
    fn take_survives_pr_head_ref_drift_when_parity() {
        // The background ref fetch lands after open (pr_head_ref None → Some).
        // Parity content (`gh pr diff`) is ref-independent — the preload must
        // survive so the first Branch click doesn't run a synchronous fetch
        // (the collapsed-Branch-view report).
        let mut tab = tab_with(|t| {
            let mut pre = make_preloaded("raw");
            pre.pr_head_ref = None; // captured before the ref fetch landed
            t.preloaded_branch_raw = Some(pre);
        });
        assert_eq!(
            tab.pr_head_ref.as_deref(),
            Some("refs/er/pr/42/head"),
            "fixture tab has the ref (drift: preload None → tab Some)"
        );
        tab.pr_head_ref = Some("refs/er/pr/42/head".to_string());
        assert_eq!(
            tab.take_preloaded_branch_raw().as_deref(),
            Some("raw"),
            "parity preload survives ref drift"
        );
    }

    #[test]
    fn take_drops_when_ref_drift_and_non_parity() {
        let mut tab = tab_with(|t| {
            let mut pre = make_preloaded("raw");
            pre.parity = false; // e.g. a checked-out worktree diff
            t.preloaded_branch_raw = Some(pre);
        });
        tab.pr_head_ref = Some("refs/er/pr/99/head".to_string());
        assert_eq!(
            tab.take_preloaded_branch_raw(),
            None,
            "non-parity preload is ref-sensitive"
        );
    }

    #[test]
    fn branch_ai_preload_adopted_only_on_bucket_and_hash_match() {
        let mut tab = tab_with(|t| {
            t.repo_root = "/home/user/my-project".to_string();
            t.local_branch_view = Some("feature-x".to_string());
            t.current_branch = "feature-x".to_string();
            t.branch_diff_hash = "hash-1".to_string();
        });
        let bucket = tab.branch_bucket_er_dir().expect("bucket dir");
        tab.preloaded_branch_ai = Some(BranchAiPreload {
            bucket_dir: bucket.clone(),
            diff_hash: "hash-1".to_string(),
            ai: super::super::AiState::default(),
        });
        // Matching bucket + hash → adopted (slot consumed).
        let adopted = tab
            .take_preloaded_branch_ai(&bucket, "hash-1")
            .expect("adopted");
        assert!(adopted.questions.is_none(), "preload state returned as-is");
        assert!(tab.preloaded_branch_ai.is_none(), "consumed on match");

        // Mismatched hash → None returned, slot KEPT (an unrelated view's
        // reload — e.g. the PR bucket at open — must not destroy the branch
        // preload before the first Branch click).
        tab.preloaded_branch_ai = Some(BranchAiPreload {
            bucket_dir: bucket.clone(),
            diff_hash: "hash-1".to_string(),
            ai: super::super::AiState::default(),
        });
        // A different bucket (the PR bucket at open) must not consume it.
        assert!(
            tab.take_preloaded_branch_ai("pr-bucket-dir", "hash-1")
                .is_none(),
            "other-bucket reload leaves the slot alone"
        );
        assert!(tab.preloaded_branch_ai.is_some(), "cross-bucket keep");
        // And a later matching consume still adopts it.
        assert!(
            tab.take_preloaded_branch_ai(&bucket, "hash-1").is_some(),
            "later matching consume adopts"
        );
        assert!(tab.preloaded_branch_ai.is_none(), "one-shot");
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
        let mut tab = tab_with(|t| {
            let mut pre = make_preloaded("raw");
            pre.parity = false; // non-parity content is ref-sensitive
            t.preloaded_branch_raw = Some(pre);
        });
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
            parity: true,
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
            parity: true,
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
            parity: true,
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
