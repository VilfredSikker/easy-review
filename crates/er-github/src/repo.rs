//! Repository identity: origin remotes, owner/repo parsing and slug matching.

use anyhow::Result;
use er_git::{Git, GitError};
use std::sync::Arc;

use crate::{default_handle, CliGitHub, GhError};

/// Parse owner/repo from a git remote URL
pub(crate) fn parse_owner_repo_from_remote(remote: &str) -> Result<(String, String)> {
    // SSH: git@github.com:owner/repo.git
    // HTTPS: https://github.com/owner/repo.git
    let stripped = remote
        .strip_prefix("https://github.com/")
        .or_else(|| remote.strip_prefix("http://github.com/"))
        .or_else(|| remote.strip_prefix("git@github.com:"))
        .unwrap_or(remote);

    let stripped = stripped.strip_suffix(".git").unwrap_or(stripped);
    let parts: Vec<&str> = stripped.split('/').collect();
    if parts.len() >= 2 {
        Ok((parts[0].to_string(), parts[1].to_string()))
    } else {
        anyhow::bail!("Cannot parse owner/repo from remote: {}", remote);
    }
}

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
                let parsed =
                    parse_owner_repo_from_remote(&remote).map_err(|error| GhError::Local {
                        message: error.to_string(),
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
    let (slug_owner, slug_repo) = match slug.split_once('/') {
        Some(parts) => parts,
        None => return false,
    };
    let repo = repo.strip_suffix(".git").unwrap_or(repo);
    let slug_repo = slug_repo.strip_suffix(".git").unwrap_or(slug_repo);
    owner.eq_ignore_ascii_case(slug_owner) && repo.eq_ignore_ascii_case(slug_repo)
}

/// Test whether a remote URL belongs to the given owner/repo.
/// HTTPS URLs contain "/owner/repo" and SSH URLs contain ":owner/repo".
pub(crate) fn remote_matches_repo(remote: &str, owner: &str, repo: &str) -> bool {
    let expected = format!("{}/{}", owner, repo);
    remote.contains(&format!("/{}", expected)) || remote.contains(&format!(":{}", expected))
}

/// Parse a git remote URL into `(owner, repo)` without making any network call.
///
/// Handles:
/// - HTTPS:  `https://github.com/owner/repo[.git]`
/// - SSH:    `git@github.com:owner/repo[.git]`
/// - ssh://: `ssh://git@github.com/owner/repo[.git]`
///
/// Returns `None` if the URL cannot be parsed into at least two path components.
pub fn parse_remote_url(url: &str) -> Option<(String, String)> {
    let url = url.trim();

    // SCP-style SSH: git@host:owner/repo[.git]
    // Detected by: a colon present, no "://" scheme marker.
    if !url.contains("://") {
        if let Some((_, after_colon)) = url.split_once(':') {
            return parse_path_components(after_colon);
        }
    }

    // Strip scheme: https://, http://, ssh://, git://
    let rest = if let Some(r) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .or_else(|| url.strip_prefix("ssh://"))
        .or_else(|| url.strip_prefix("git://"))
    {
        r
    } else {
        url
    };

    // Drop the host portion (everything up to and including the first `/`).
    let path = rest.split_once('/')?.1;
    parse_path_components(path)
}

/// Split `owner/repo[.git]` path into `(owner, repo)`.
fn parse_path_components(path: &str) -> Option<(String, String)> {
    let path = path.strip_suffix(".git").unwrap_or(path);
    // Accept paths with leading slashes (e.g. ssh:// URLs may have /owner/repo)
    let path = path.trim_start_matches('/');
    let mut parts = path.splitn(3, '/');
    let owner = parts.next().filter(|s| !s.is_empty())?.to_string();
    let repo = parts.next().filter(|s| !s.is_empty())?.to_string();
    Some((owner, repo))
}

/// Parse `(owner, repo)` from the `origin` remote of the given repo root.
///
/// Runs `git -C <repo_root> remote get-url origin`. Returns `None` if the
/// remote is absent, unreachable, or the URL cannot be parsed.
pub fn parse_origin_remote(git: &dyn Git, repo_root: &str) -> Option<(String, String)> {
    let url = git.remote_url(repo_root, "origin").ok()?;
    parse_remote_url(&url)
}
