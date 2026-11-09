//! PR head and base refs: fetching them, pinning them, and checking whether a
//! local checkout matches the PR's repository.

use anyhow::{Context, Result};
use er_git::{Git, GitError};

use crate::lookup::PrRef;
use crate::repo::remote_matches_repo;
use crate::{default_handle, CliGitHub, GhError};

/// Fetch PR head to a local ref without checking out. Returns the local ref name.
///
/// Uses a force refspec (`+`): PR heads are force-pushable, so when a PR branch
/// is rebased or force-pushed the new head is no longer a descendant of the
/// previously fetched commit. A plain fetch rejects that as non-fast-forward,
/// leaving the stale `refs/er/pr/<n>/head` and breaking re-entry into PR diff.
/// The `+` only affects this `refs/er/` destination ref — never local branches
/// (`refs/heads/`), remote-tracking refs, HEAD, or the working tree.
pub fn fetch_pr_head(git: &dyn Git, number: u64, root: &str) -> Result<String> {
    let ref_name = format!("refs/er/pr/{}/head", number);
    let refspec = format!("+pull/{}/head:{}", number, ref_name);
    match git.fetch(root, &["origin", &refspec]) {
        Ok(()) => Ok(ref_name),
        Err(GitError::Failed { stderr, .. }) => {
            anyhow::bail!("git fetch PR head failed: {}", stderr.trim())
        }
        Err(e) => Err(anyhow::Error::from(e).context("failed to run git fetch for PR head")),
    }
}

/// Whether an error came from a local branch that genuinely has no upstream.
///
/// Callers may fall back to a local-only diff for this case, but not for fetch
/// failures where falling back would show stale remote-backed state.
pub fn is_no_upstream_to_refresh(err: &anyhow::Error) -> bool {
    err.chain()
        .any(|cause| cause.to_string().contains("has no upstream to refresh"))
}

pub fn ref_exists_locally(git: &dyn Git, repo_root: &str, ref_name: &str) -> bool {
    git.rev_parse_verify(repo_root, ref_name)
        .is_ok_and(|oid| oid.is_some())
}

/// Resolve a git ref to its commit oid (full sha). Returns None on any failure.
pub fn rev_parse_oid(git: &dyn Git, repo_root: &str, ref_name: &str) -> Option<String> {
    git.rev_parse_verify(repo_root, ref_name).ok().flatten()
}

/// Force-fetch a base branch from origin, updating `origin/<base>` even if it already exists.
/// Returns `origin/<base_branch>` on success.
pub fn fetch_base_branch_ref(git: &dyn Git, repo_root: &str, base_branch: &str) -> Result<String> {
    let remote_ref = format!("origin/{}", base_branch);
    let refspec = format!(
        "+refs/heads/{}:refs/remotes/origin/{}",
        base_branch, base_branch
    );

    match git.fetch(repo_root, &["origin", &refspec]) {
        Ok(()) => {}
        Err(GitError::Failed { stderr, .. }) => {
            anyhow::bail!(
                "Failed to fetch base branch '{}' from origin: {}",
                base_branch,
                stderr.trim()
            );
        }
        Err(e) => {
            return Err(anyhow::Error::from(e).context("Failed to fetch base branch from origin"));
        }
    }

    match git.rev_parse_verify(repo_root, &remote_ref) {
        Ok(Some(_)) => Ok(remote_ref),
        Ok(None) => {
            anyhow::bail!(
                "Base branch '{}' fetched but '{}' is not resolvable",
                base_branch,
                remote_ref
            )
        }
        Err(e) => Err(anyhow::Error::from(e).context("Failed to verify fetched base branch")),
    }
}

/// Whether a fetch failed because origin no longer has the branch. A stacked
/// PR's base is usually deleted once it merges, and only this case may fall
/// back to the PR's recorded base commit — a network failure must still fail.
pub fn is_missing_remote_ref(err: &anyhow::Error) -> bool {
    err.chain()
        .any(|cause| cause.to_string().contains("couldn't find remote ref"))
}

/// Whether a base ref is a commit pinned by [`fetch_pr_base_ref`] rather than a
/// branch: there is nothing on origin to re-fetch, and it is not a name to show.
pub fn is_pinned_pr_base(base: &str) -> bool {
    base.starts_with("refs/er/")
}

impl CliGitHub {
    /// [`fetch_base_branch_ref`] for a PR whose base branch may be gone from
    /// origin. Falls back to the PR's `baseRefOid`, which GitHub keeps after
    /// the branch is deleted. The extra `gh` call happens only on that failure.
    pub(crate) fn fetch_pr_base_ref(
        &self,
        git: &dyn Git,
        repo_root: &str,
        pr_number: u64,
        base_branch: &str,
    ) -> Result<String, GhError> {
        fetch_pr_base_ref_with(git, repo_root, pr_number, base_branch, || {
            self.pr_base_oid(pr_number, repo_root)
                .map_err(anyhow::Error::from)
        })
        .map_err(GhError::from)
    }

    fn pr_base_oid(&self, pr_number: u64, repo_root: &str) -> Result<String, GhError> {
        let out = self.run_gh_args(
            &[
                "pr",
                "view",
                &pr_number.to_string(),
                "--json",
                "baseRefOid",
                "--jq",
                ".baseRefOid",
            ],
            Some(repo_root),
        )?;
        let sha = String::from_utf8_lossy(&out.body).trim().to_string();
        // Hex-only, so the value can never parse as a git option below.
        if sha.is_empty() || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(GhError::Local {
                message: format!("PR #{pr_number} has no usable base commit: '{sha}'"),
            });
        }
        Ok(sha)
    }
}

pub(crate) fn fetch_pr_base_ref_with(
    git: &dyn Git,
    repo_root: &str,
    pr_number: u64,
    base_branch: &str,
    base_oid: impl FnOnce() -> Result<String>,
) -> Result<String> {
    match fetch_base_branch_ref(git, repo_root, base_branch) {
        Err(err) if is_missing_remote_ref(&err) => {
            let sha = base_oid()?;
            pin_pr_base_commit(git, repo_root, pr_number, &sha).with_context(|| {
                format!("Base branch '{base_branch}' is gone from origin and its commit could not be fetched")
            })
        }
        other => other,
    }
}

/// Compatibility wrapper for front ends and engine callers.
pub fn fetch_pr_base_ref(
    git: &dyn Git,
    repo_root: &str,
    pr_number: u64,
    base_branch: &str,
) -> Result<String> {
    default_handle()
        .fetch_pr_base_ref(git, repo_root, pr_number, base_branch)
        .map_err(anyhow::Error::from)
}

/// Point `refs/er/pr/<n>/base` at `sha`, fetching it from origin only when the
/// commit is not already local (it usually is: the base was fetched before it
/// was deleted). Returns the ref name.
pub(crate) fn pin_pr_base_commit(
    git: &dyn Git,
    repo_root: &str,
    pr_number: u64,
    sha: &str,
) -> Result<String> {
    let ref_name = format!("refs/er/pr/{pr_number}/base");
    if !ref_exists_locally(git, repo_root, &format!("{sha}^{{commit}}")) {
        let refspec = format!("+{sha}:{ref_name}");
        match git.fetch(repo_root, &["origin", &refspec]) {
            Ok(()) => return Ok(ref_name),
            Err(GitError::Failed { stderr, .. }) => {
                anyhow::bail!("git fetch PR base commit failed: {}", stderr.trim());
            }
            Err(e) => {
                return Err(
                    anyhow::Error::from(e).context("failed to run git fetch for PR base commit")
                );
            }
        }
    }
    match git.update_ref(repo_root, &ref_name, sha) {
        Ok(()) => Ok(ref_name),
        Err(GitError::Failed { stderr, .. }) => {
            anyhow::bail!("git update-ref PR base commit failed: {}", stderr.trim());
        }
        Err(e) => {
            Err(anyhow::Error::from(e).context("failed to run git update-ref for PR base commit"))
        }
    }
}

impl CliGitHub {
    /// Get the head branch name of a PR via `gh pr view`.
    pub(crate) fn pr_head_branch_name(&self, number: u64, root: &str) -> Result<String, GhError> {
        let out = self.run_gh_args(
            &[
                "pr",
                "view",
                &number.to_string(),
                "--json",
                "headRefName",
                "--jq",
                ".headRefName",
            ],
            Some(root),
        )?;
        Ok(String::from_utf8_lossy(&out.body).trim().to_string())
    }
}

/// Compatibility wrapper for front ends and engine callers.
pub fn gh_pr_head_branch_name(number: u64, root: &str) -> Result<String> {
    default_handle()
        .pr_head_branch_name(number, root)
        .map_err(anyhow::Error::from)
}

/// Verify the local repo's remote matches the PR's owner/repo
pub fn verify_remote_matches(git: &dyn Git, repo_root: &str, pr_ref: &PrRef) -> Result<()> {
    let remote = match git.remote_url(repo_root, "origin") {
        Ok(remote) => remote,
        // No remote — can't verify, let gh handle it
        Err(GitError::Failed { .. }) => return Ok(()),
        Err(e) => return Err(anyhow::Error::from(e).context("Failed to get git remote URL")),
    };
    let expected = format!("{}/{}", pr_ref.owner, pr_ref.repo);
    if !remote_matches_repo(&remote, &pr_ref.owner, &pr_ref.repo) {
        anyhow::bail!(
            "PR is for {} but current repo remote is '{}'. Navigate to the correct repo first.",
            expected,
            remote
        );
    }

    Ok(())
}

/// Ensure a remote ref is available locally by fetching if needed.
/// Returns the ref name that actually resolves — may be `origin/<base>` if
/// no local branch exists.
pub fn ensure_base_ref_available(
    git: &dyn Git,
    repo_root: &str,
    base_branch: &str,
) -> Result<String> {
    let base_branch = base_branch.trim();
    let base_branch = if base_branch.is_empty() {
        git.detect_base_branch(repo_root)
            .context("PR has no base branch; could not detect a default base")?
    } else {
        base_branch.to_string()
    };

    let rev_parse_ok = |refname: &str| -> Result<bool> {
        match git.rev_parse_verify(repo_root, refname) {
            Ok(oid) => Ok(oid.is_some()),
            Err(e) => Err(anyhow::Error::from(e).context("Failed to check ref")),
        }
    };

    // Local branch exists — use it directly
    if rev_parse_ok(&base_branch)? {
        return Ok(base_branch);
    }

    // Remote-tracking ref exists — use it (no fetch needed)
    let remote_ref = format!("origin/{}", base_branch);
    if rev_parse_ok(&remote_ref)? {
        return Ok(remote_ref);
    }

    // Fetch from origin. Guard against option injection: a dash-prefixed
    // ref (e.g. `--upload-pack=…`) parses as a git option, not a refspec —
    // qualify bare names as `refs/heads/<name>` so the value never starts
    // with `-` (the fetch then simply fails to resolve, no injection).
    let qualified = format!("refs/heads/{base_branch}");
    let fetch_ref = if base_branch.starts_with("refs/") {
        base_branch.as_str()
    } else {
        qualified.as_str()
    };
    match git.fetch(repo_root, &["origin", fetch_ref]) {
        Ok(()) => {}
        Err(GitError::Failed { stderr, .. }) => {
            anyhow::bail!(
                "Base branch '{}' not found locally or on origin: {}",
                base_branch,
                stderr.trim()
            );
        }
        Err(e) => {
            return Err(anyhow::Error::from(e).context("Failed to fetch base branch from origin"));
        }
    }

    // After fetch, origin/<base> should exist
    if rev_parse_ok(&remote_ref)? {
        return Ok(remote_ref);
    }

    // Fallback: the fetch may have created a local ref via FETCH_HEAD
    if rev_parse_ok(&base_branch)? {
        return Ok(base_branch);
    }

    anyhow::bail!(
        "Base branch '{}' fetched but still not resolvable",
        base_branch
    )
}
