//! GitHub access, re-exported from `er-github`.
//!
//! Everything that talks to `gh` lives in `er-github`. Three functions stay
//! here because they name engine-only items: the storage slug and the engine's
//! diff stats.

pub use er_github::*;

/// Derive the canonical owner/repo slug for a local clone, matching the slug
/// produced by `er --remote` for the same repository.
///
/// Parses `(owner, repo)` from `origin`, then asks [`crate::storage::repo_storage_slug`]
/// for the folder name, so a local clone and a remote session resolve the same
/// directory even when the host differs.
///
/// Returns `None` when the origin remote is absent or unparseable.
pub fn canonical_owner_repo_slug(git: &dyn er_git::Git, repo_root: &str) -> Option<String> {
    let (owner, repo) = parse_origin_remote(git, repo_root)?;
    Some(crate::storage::repo_storage_slug(&owner, &repo))
}

/// Production-vs-noise line stats for a remote PR (fetches `gh pr diff`).
pub fn gh_pr_prod_diff_stats(
    git: &dyn er_git::Git,
    owner: &str,
    repo: &str,
    number: u64,
) -> anyhow::Result<crate::git::ProdDiffStats> {
    let raw = gh_pr_diff_remote(git, owner, repo, number)?;
    Ok(crate::git::ProdDiffStats::from_raw_diff(&raw))
}

/// Owner/repo slug used for managed PR buckets (`owner-repo-<8 hex>`).
pub fn owner_repo_storage_slug(owner: &str, repo: &str) -> String {
    crate::storage::repo_storage_slug(owner, repo)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── canonical_owner_repo_slug (pure slug logic, no git subprocess) ──

    #[test]
    fn canonical_slug_matches_for_ssh_and_https_urls() {
        // Test the slug produced by the same logic canonical_owner_repo_slug uses,
        // without needing a real git repo: parse URL → repo_storage_slug.
        let (ssh_owner, ssh_repo) = parse_remote_url("git@github.com:Acme/My-Repo.git").unwrap();
        let (https_owner, https_repo) =
            parse_remote_url("https://github.com/Acme/My-Repo.git").unwrap();

        let slug_ssh = crate::storage::repo_storage_slug(&ssh_owner, &ssh_repo);
        let slug_https = crate::storage::repo_storage_slug(&https_owner, &https_repo);

        // Both forms must produce the same slug, and it is the hashed name.
        assert_eq!(slug_ssh, slug_https);
        assert_eq!(slug_ssh, "acme-my-repo-80b59845");
    }

    #[test]
    fn canonical_slug_is_case_insensitive() {
        // "Acme/My-Repo" and "acme/my-repo" must resolve to the same directory.
        let slug_upper = crate::storage::repo_storage_slug("Acme", "My-Repo");
        let slug_lower = crate::storage::repo_storage_slug("acme", "my-repo");
        assert_eq!(slug_upper, slug_lower);
        assert_eq!(slug_upper, "acme-my-repo-80b59845");
    }

    #[test]
    fn canonical_slug_lowercased() {
        let (owner, repo) = parse_remote_url("https://github.com/UPPER/lower.git").unwrap();
        let slug = crate::storage::repo_storage_slug(&owner, &repo);
        assert_eq!(slug, "upper-lower-eacd0cf6");
    }

    #[test]
    fn canonical_owner_repo_slug_reads_origin_remote() {
        // Exercises the REAL canonical_owner_repo_slug (origin remote -> slug),
        // not a re-implementation of its composition.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::process::Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(root)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["remote", "add", "origin", "git@github.com:Acme/My-Repo.git"])
            .current_dir(root)
            .output()
            .unwrap();
        let slug = canonical_owner_repo_slug(&er_git::CliGit, root.to_str().unwrap())
            .expect("slug from origin");
        assert_eq!(slug, crate::storage::repo_storage_slug("Acme", "My-Repo"));
    }
}
