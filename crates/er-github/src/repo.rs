//! Repository identity: origin remotes, owner/repo parsing and slug matching.

use anyhow::Result;
use er_git::{Git, GitError};
use std::sync::Arc;

use crate::slug::RepoSlug;
use crate::{default_handle, CliGitHub, GhError};

impl CliGitHub {
    /// Get GitHub owner/repo from the repository's `origin` remote, cached for
    /// 60 seconds per repo root.
    pub(crate) fn get_repo_info(
        &self,
        git: &dyn Git,
        repo_root: &str,
    ) -> Result<(String, String), GhError> {
        let cached = self.cache().repo_info.get_or_fetch(
            repo_root.to_string(),
            || -> Result<Arc<(String, String)>, GhError> {
                let remote = git.remote_url(repo_root, "origin").map_err(|error| {
                    let detail = match error {
                        GitError::Failed { stderr, .. } => stderr.trim().to_string(),
                        error => error.to_string(),
                    };
                    GhError::Local {
                        message: format!("Failed to get origin remote: {detail}"),
                    }
                })?;
                let parsed = RepoSlug::parse(&remote)
                    .map(|slug| (slug.owner, slug.repo))
                    .ok_or_else(|| GhError::Local {
                        message: format!("Cannot parse owner/repo from remote: {remote}"),
                    })?;
                Ok(Arc::new(parsed))
            },
        )?;
        Ok((cached.0.clone(), cached.1.clone()))
    }
}

/// Compatibility wrapper for front ends; removed when they take the handle.
pub fn get_repo_info(git: &dyn Git, repo_root: &str) -> Result<(String, String)> {
    default_handle()
        .get_repo_info(git, repo_root)
        .map_err(anyhow::Error::from)
}

/// True when `owner`/`repo` name the same GitHub repository as the `"owner/repo"` slug.
/// GitHub treats owner and repo names case-insensitively.
pub(crate) fn owner_repo_matches_slug(owner: &str, repo: &str, slug: &str) -> bool {
    let Some(slug) = RepoSlug::parse(slug) else {
        return false;
    };
    let expected = RepoSlug {
        owner: owner.to_string(),
        repo: repo.strip_suffix(".git").unwrap_or(repo).to_string(),
    };
    expected.eq_ignore_case(&slug)
}

/// Test whether a remote URL belongs to the given owner/repo.
///
/// The comparison is exact and case-insensitive, so a remote for
/// `owner/repo-fork` does not match `owner/repo`.
pub(crate) fn remote_matches_repo(remote: &str, owner: &str, repo: &str) -> bool {
    let Some(slug) = RepoSlug::parse(remote) else {
        return false;
    };
    let expected = RepoSlug {
        owner: owner.to_string(),
        repo: repo.strip_suffix(".git").unwrap_or(repo).to_string(),
    };
    expected.eq_ignore_case(&slug)
}

/// Parse a git remote URL into `(owner, repo)` without making any network call.
///
/// Handles HTTPS, `ssh://`, `git://`, the scp-like `git@host:owner/repo`, and a
/// bare `owner/repo`. Returns `None` for anything that does not reduce to an
/// owner and a repo.
pub fn parse_remote_url(url: &str) -> Option<(String, String)> {
    RepoSlug::parse(url).map(|slug| (slug.owner, slug.repo))
}

/// Parse `(owner, repo)` from the `origin` remote of the given repo root.
///
/// Runs `git -C <repo_root> remote get-url origin`. Returns `None` if the
/// remote is absent, unreachable, or the URL cannot be parsed.
pub fn parse_origin_remote(git: &dyn Git, repo_root: &str) -> Option<(String, String)> {
    let url = git.remote_url(repo_root, "origin").ok()?;
    parse_remote_url(&url)
}
