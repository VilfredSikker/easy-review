use anyhow::{Context, Result};
use std::process::Command;

/// Parsed reference to a GitHub PR
#[derive(Debug, Clone)]
pub struct PrRef {
    pub owner: String,
    pub repo: String,
    pub number: u64,
}

/// Parse a GitHub PR URL into its components.
/// Supports: https://github.com/owner/repo/pull/42
/// Also handles: trailing /files, /commits, /checks, etc.
/// Also handles: github.com/owner/repo/pull/42 (no scheme)
pub fn parse_github_pr_url(url: &str) -> Option<PrRef> {
    // Strip scheme
    let stripped = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);

    // Must start with github.com
    let rest = stripped.strip_prefix("github.com/")?;

    // Split: owner/repo/pull/number[/...]
    let parts: Vec<&str> = rest.split('/').collect();
    if parts.len() < 4 {
        return None;
    }
    if parts[2] != "pull" {
        return None;
    }

    let owner = parts[0].to_string();
    let repo = parts[1].to_string();
    let number = parts[3].parse::<u64>().ok()?;

    if owner.is_empty() || repo.is_empty() || number == 0 {
        return None;
    }

    Some(PrRef { owner, repo, number })
}

/// Check if `gh` CLI is installed and authenticated
pub fn ensure_gh_installed() -> Result<()> {
    let output = Command::new("gh")
        .args(["--version"])
        .output()
        .context("GitHub CLI (gh) is not installed. Install it: https://cli.github.com")?;

    if !output.status.success() {
        anyhow::bail!("GitHub CLI (gh) is not working properly");
    }

    // Check auth
    let auth = Command::new("gh")
        .args(["auth", "status"])
        .output()
        .context("Failed to check gh auth status")?;

    if !auth.status.success() {
        anyhow::bail!("GitHub CLI is not authenticated. Run: gh auth login");
    }

    Ok(())
}

/// Get the base branch for a PR using `gh pr view`
pub fn gh_pr_base_branch(pr_number: u64, repo_root: &str) -> Result<String> {
    let output = Command::new("gh")
        .args([
            "pr",
            "view",
            &pr_number.to_string(),
            "--json",
            "baseRefName",
            "--jq",
            ".baseRefName",
        ])
        .current_dir(repo_root)
        .output()
        .context("Failed to get PR base branch")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to get PR #{}: {}", pr_number, stderr.trim());
    }

    let base = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if base.is_empty() {
        anyhow::bail!("PR #{} has no base branch", pr_number);
    }

    Ok(base)
}

/// Checkout a PR by number using `gh pr checkout`
pub fn gh_pr_checkout(pr_number: u64, repo_root: &str) -> Result<()> {
    let output = Command::new("gh")
        .args(["pr", "checkout", &pr_number.to_string()])
        .current_dir(repo_root)
        .output()
        .context("Failed to checkout PR")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to checkout PR #{}: {}", pr_number, stderr.trim());
    }

    Ok(())
}

/// Verify the local repo's remote matches the PR's owner/repo
pub fn verify_remote_matches(repo_root: &str, pr_ref: &PrRef) -> Result<()> {
    let output = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(repo_root)
        .output()
        .context("Failed to get git remote URL")?;

    if !output.status.success() {
        // No remote — can't verify, let gh handle it
        return Ok(());
    }

    let remote = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let expected = format!("{}/{}", pr_ref.owner, pr_ref.repo);
    if !remote.contains(&expected) {
        anyhow::bail!(
            "PR is for {} but current repo remote is '{}'. Navigate to the correct repo first.",
            expected,
            remote
        );
    }

    Ok(())
}

/// Check if a string looks like a GitHub PR URL
pub fn is_github_pr_url(s: &str) -> bool {
    parse_github_pr_url(s).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_standard_url() {
        let pr = parse_github_pr_url("https://github.com/owner/repo/pull/42").unwrap();
        assert_eq!(pr.owner, "owner");
        assert_eq!(pr.repo, "repo");
        assert_eq!(pr.number, 42);
    }

    #[test]
    fn parse_url_with_trailing_path() {
        let pr = parse_github_pr_url("https://github.com/owner/repo/pull/42/files").unwrap();
        assert_eq!(pr.number, 42);
    }

    #[test]
    fn parse_url_with_commits() {
        let pr = parse_github_pr_url("https://github.com/owner/repo/pull/42/commits").unwrap();
        assert_eq!(pr.number, 42);
    }

    #[test]
    fn parse_url_no_scheme() {
        let pr = parse_github_pr_url("github.com/owner/repo/pull/42").unwrap();
        assert_eq!(pr.owner, "owner");
        assert_eq!(pr.repo, "repo");
        assert_eq!(pr.number, 42);
    }

    #[test]
    fn parse_http_url() {
        let pr = parse_github_pr_url("http://github.com/owner/repo/pull/99").unwrap();
        assert_eq!(pr.number, 99);
    }

    #[test]
    fn parse_invalid_not_github() {
        assert!(parse_github_pr_url("https://gitlab.com/owner/repo/pull/42").is_none());
    }

    #[test]
    fn parse_invalid_not_pull() {
        assert!(parse_github_pr_url("https://github.com/owner/repo/issues/42").is_none());
    }

    #[test]
    fn parse_invalid_no_number() {
        assert!(parse_github_pr_url("https://github.com/owner/repo/pull/").is_none());
    }

    #[test]
    fn parse_invalid_non_numeric() {
        assert!(parse_github_pr_url("https://github.com/owner/repo/pull/abc").is_none());
    }

    #[test]
    fn parse_invalid_too_short() {
        assert!(parse_github_pr_url("https://github.com/owner").is_none());
    }

    #[test]
    fn parse_invalid_empty() {
        assert!(parse_github_pr_url("").is_none());
    }

    #[test]
    fn parse_invalid_zero_pr_number() {
        assert!(parse_github_pr_url("https://github.com/owner/repo/pull/0").is_none());
    }

    #[test]
    fn is_github_pr_url_true() {
        assert!(is_github_pr_url("https://github.com/owner/repo/pull/1"));
    }

    #[test]
    fn is_github_pr_url_false() {
        assert!(!is_github_pr_url("/some/local/path"));
        assert!(!is_github_pr_url("not-a-url"));
    }
}
