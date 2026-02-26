use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::process::Command;

/// Parsed reference to a GitHub PR
#[derive(Debug, Clone)]
pub struct PrRef {
    pub owner: String,
    pub repo: String,
    pub number: u64,
}

/// Fetched PR overview data for the PrOverview panel
#[derive(Debug, Clone)]
pub struct PrOverviewData {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub state: String,
    pub author: String,
    pub base_branch: String,
    pub head_branch: String,
    pub checks: Vec<CiCheck>,
    pub reviewers: Vec<ReviewerStatus>,
}

#[derive(Debug, Clone)]
pub struct CiCheck {
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ReviewerStatus {
    pub login: String,
    pub state: String,
}

/// GitHub review comment from the API
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct GitHubComment {
    pub id: u64,
    pub body: String,
    pub path: Option<String>,
    pub line: Option<usize>,
    pub original_line: Option<usize>,
    pub side: Option<String>,
    pub in_reply_to_id: Option<u64>,
    pub user: GitHubUser,
    pub created_at: String,
    pub updated_at: String,
    pub diff_hunk: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitHubUser {
    pub login: String,
}

/// Response from creating a comment (we only need the id)
#[derive(Debug, Deserialize)]
struct CreateCommentResponse {
    id: u64,
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
pub fn ensure_base_ref_available(repo_root: &str, base_branch: &str) -> Result<String> {
    let rev_parse_ok = |refname: &str| -> Result<bool> {
        let out = Command::new("git")
            .args(["rev-parse", "--verify", refname])
            .current_dir(repo_root)
            .output()
            .context("Failed to check ref")?;
        Ok(out.status.success())
    };

    // Local branch exists — use it directly
    if rev_parse_ok(base_branch)? {
        return Ok(base_branch.to_string());
    }

    // Remote-tracking ref exists — use it (no fetch needed)
    let remote_ref = format!("origin/{}", base_branch);
    if rev_parse_ok(&remote_ref)? {
        return Ok(remote_ref);
    }

    // Fetch from origin
    let fetch = Command::new("git")
        .args(["fetch", "origin", base_branch])
        .current_dir(repo_root)
        .output()
        .context("Failed to fetch base branch from origin")?;

    if !fetch.status.success() {
        let stderr = String::from_utf8_lossy(&fetch.stderr);
        anyhow::bail!(
            "Base branch '{}' not found locally or on origin: {}",
            base_branch,
            stderr.trim()
        );
    }

    // After fetch, origin/<base> should exist
    if rev_parse_ok(&remote_ref)? {
        return Ok(remote_ref);
    }

    // Fallback: the fetch may have created a local ref via FETCH_HEAD
    if rev_parse_ok(base_branch)? {
        return Ok(base_branch.to_string());
    }

    anyhow::bail!(
        "Base branch '{}' fetched but still not resolvable",
        base_branch
    )
}

/// Check if the current branch has an open PR. Returns (number, base_branch) or None.
/// Silently returns None if gh is unavailable, not authenticated, or no PR exists.
pub fn gh_pr_for_current_branch(repo_root: &str) -> Option<(u64, String)> {
    // Use --jq to extract "number<tab>baseRefName" — robust against JSON formatting
    let output = Command::new("gh")
        .args([
            "pr", "view",
            "--json", "number,baseRefName",
            "--jq", r#"[.number, .baseRefName] | @tsv"#,
        ])
        .current_dir(repo_root)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let text = text.trim();
    let (num_str, base) = text.split_once('\t')?;
    let number = num_str.parse::<u64>().ok()?;

    if base.is_empty() {
        return None;
    }

    Some((number, base.to_string()))
}

/// Check if a string looks like a GitHub PR URL
pub fn is_github_pr_url(s: &str) -> bool {
    parse_github_pr_url(s).is_some()
}

/// Get PR info (owner, repo, number) for the current branch
pub fn get_pr_info(repo_root: &str) -> Result<(String, String, u64)> {
    // Try `gh pr view --json number,headRepository,baseRefName`
    let output = Command::new("gh")
        .args(["pr", "view", "--json", "number,headRepository"])
        .current_dir(repo_root)
        .output()
        .context("Failed to get PR info")?;

    if !output.status.success() {
        anyhow::bail!("No PR associated with current branch");
    }

    let json_str = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(&json_str)
        .context("Failed to parse PR info JSON")?;

    let number = v["number"].as_u64()
        .context("Missing PR number")?;

    let owner = v["headRepository"]["owner"]["login"].as_str()
        .unwrap_or("");
    let repo_name = v["headRepository"]["name"].as_str()
        .unwrap_or("");

    // If headRepository is missing, try to get owner/repo from remote
    if owner.is_empty() || repo_name.is_empty() {
        let remote_output = Command::new("git")
            .args(["remote", "get-url", "origin"])
            .current_dir(repo_root)
            .output()?;
        let remote = String::from_utf8_lossy(&remote_output.stdout).trim().to_string();
        // Parse owner/repo from remote URL
        let (o, r) = parse_owner_repo_from_remote(&remote)?;
        return Ok((o, r, number));
    }

    Ok((owner.to_string(), repo_name.to_string(), number))
}

/// Parse owner/repo from a git remote URL
fn parse_owner_repo_from_remote(remote: &str) -> Result<(String, String)> {
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

/// Fetch all review comments for a PR
pub fn gh_pr_comments(owner: &str, repo: &str, pr: u64, repo_root: &str) -> Result<Vec<GitHubComment>> {
    let output = Command::new("gh")
        .args([
            "api",
            &format!("repos/{}/{}/pulls/{}/comments", owner, repo, pr),
            "--paginate",
        ])
        .current_dir(repo_root)
        .output()
        .context("Failed to fetch PR comments")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to fetch PR comments: {}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    // gh api --paginate concatenates JSON arrays: [...][...]
    // Parse each chunk separately and merge
    let all_comments: Vec<GitHubComment> = if stdout.contains("][") {
        // Split paginated response into individual arrays
        let mut results = Vec::new();
        let mut depth = 0i32;
        let mut start = 0;
        for (i, ch) in stdout.char_indices() {
            match ch {
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        if let Ok(mut batch) = serde_json::from_str::<Vec<GitHubComment>>(&stdout[start..=i]) {
                            results.append(&mut batch);
                        }
                        start = i + 1;
                    }
                }
                _ => {}
            }
        }
        results
    } else {
        serde_json::from_str(&stdout)?
    };

    Ok(all_comments)
}

/// Push a new review comment to a PR
pub fn gh_pr_push_comment(
    owner: &str, repo: &str, pr: u64,
    path: &str, line: usize, body: &str,
    repo_root: &str,
) -> Result<u64> {
    // Get the latest commit SHA for the PR (required for review comments)
    let sha_output = Command::new("gh")
        .args(["pr", "view", &pr.to_string(), "--json", "headRefOid", "--jq", ".headRefOid"])
        .current_dir(repo_root)
        .output()
        .context("Failed to get PR head SHA")?;

    if !sha_output.status.success() {
        let stderr = String::from_utf8_lossy(&sha_output.stderr);
        anyhow::bail!("Failed to get HEAD SHA from gh pr view: {}", stderr.trim());
    }

    let commit_id = String::from_utf8_lossy(&sha_output.stdout).trim().to_string();
    if commit_id.is_empty() {
        anyhow::bail!("Failed to get HEAD SHA from gh pr view: empty output");
    }

    let output = Command::new("gh")
        .args([
            "api",
            "-X", "POST",
            &format!("repos/{}/{}/pulls/{}/comments", owner, repo, pr),
            "-f", &format!("body={}", body),
            "-f", &format!("path={}", path),
            "-F", &format!("line={}", line),
            "-f", "side=RIGHT",
            "-f", &format!("commit_id={}", commit_id),
        ])
        .current_dir(repo_root)
        .output()
        .context("Failed to push comment to GitHub")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to push comment: {}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let resp: CreateCommentResponse = serde_json::from_str(&stdout)
        .context("Failed to parse create comment response")?;

    Ok(resp.id)
}

/// Push a reply to an existing review comment
pub fn gh_pr_reply_comment(
    owner: &str, repo: &str, pr: u64,
    in_reply_to: u64, body: &str,
    repo_root: &str,
) -> Result<u64> {
    let output = Command::new("gh")
        .args([
            "api",
            "-X", "POST",
            &format!("repos/{}/{}/pulls/{}/comments", owner, repo, pr),
            "-f", &format!("body={}", body),
            "-F", &format!("in_reply_to={}", in_reply_to),
        ])
        .current_dir(repo_root)
        .output()
        .context("Failed to push reply to GitHub")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to push reply: {}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let resp: CreateCommentResponse = serde_json::from_str(&stdout)
        .context("Failed to parse reply response")?;

    Ok(resp.id)
}

/// Delete a review comment from a PR
pub fn gh_pr_delete_comment(owner: &str, repo: &str, comment_id: u64, repo_root: &str) -> Result<()> {
    let output = Command::new("gh")
        .args([
            "api",
            "-X", "DELETE",
            &format!("repos/{}/{}/pulls/comments/{}", owner, repo, comment_id),
        ])
        .current_dir(repo_root)
        .output()
        .context("Failed to delete comment from GitHub")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // 404 is OK — comment may already be deleted
        if !stderr.contains("404") {
            anyhow::bail!("Failed to delete comment: {}", stderr.trim());
        }
    }

    Ok(())
}

/// Fetch PR overview data: title, body, state, author, branches, reviewers
pub fn gh_pr_overview(repo_root: &str) -> Option<PrOverviewData> {
    // Fetch core PR fields
    let view_output = Command::new("gh")
        .args([
            "pr", "view",
            "--json", "number,title,body,state,author,baseRefName,headRefName,reviews",
        ])
        .current_dir(repo_root)
        .output()
        .ok()?;

    if !view_output.status.success() {
        return None;
    }

    let json_str = String::from_utf8_lossy(&view_output.stdout);
    let v: serde_json::Value = serde_json::from_str(&json_str).ok()?;

    let number = v["number"].as_u64().unwrap_or(0);
    let title = v["title"].as_str().unwrap_or("").to_string();
    let body = v["body"].as_str().unwrap_or("").to_string();
    let state = v["state"].as_str().unwrap_or("").to_string();
    let author = v["author"]["login"].as_str().unwrap_or("").to_string();
    let base_branch = v["baseRefName"].as_str().unwrap_or("").to_string();
    let head_branch = v["headRefName"].as_str().unwrap_or("").to_string();

    // Deduplicate reviewers by login, keeping the latest state
    let reviewers: Vec<ReviewerStatus> = if let Some(reviews_arr) = v["reviews"].as_array() {
        let mut reviewer_map: HashMap<String, String> = HashMap::new();
        for r in reviews_arr {
            if let Some(login) = r["author"]["login"].as_str() {
                let state = r["state"].as_str().unwrap_or("PENDING").to_string();
                reviewer_map.insert(login.to_string(), state);
            }
        }
        reviewer_map.into_iter()
            .map(|(login, state)| ReviewerStatus { login, state })
            .collect()
    } else {
        Vec::new()
    };

    // Fetch CI checks (separate call — may fail if no checks configured)
    let checks = gh_pr_checks_data(repo_root).unwrap_or_default();

    Some(PrOverviewData {
        number,
        title,
        body,
        state,
        author,
        base_branch,
        head_branch,
        checks,
        reviewers,
    })
}

/// Fetch CI check runs for the current PR
fn gh_pr_checks_data(repo_root: &str) -> Result<Vec<CiCheck>> {
    let output = Command::new("gh")
        .args(["pr", "checks", "--json", "name,status,conclusion"])
        .current_dir(repo_root)
        .output()
        .context("Failed to run gh pr checks")?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let json_str = String::from_utf8_lossy(&output.stdout);
    let arr: Vec<serde_json::Value> = serde_json::from_str(&json_str).unwrap_or_default();

    let checks = arr
        .iter()
        .filter_map(|c| {
            let name = c["name"].as_str()?;
            let status = c["status"].as_str().unwrap_or("unknown");
            let conclusion = c["conclusion"].as_str().map(|s| s.to_string());
            Some(CiCheck {
                name: name.to_string(),
                status: status.to_string(),
                conclusion,
            })
        })
        .collect();

    Ok(checks)
}

/// Test whether a remote URL belongs to the given owner/repo.
/// HTTPS URLs contain "/owner/repo" and SSH URLs contain ":owner/repo".
fn remote_matches_repo(remote: &str, owner: &str, repo: &str) -> bool {
    let expected = format!("{}/{}", owner, repo);
    remote.contains(&format!("/{}", expected)) || remote.contains(&format!(":{}", expected))
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

    // ── remote_matches_repo ──

    #[test]
    fn remote_matches_https_url() {
        assert!(remote_matches_repo(
            "https://github.com/owner/repo.git",
            "owner",
            "repo"
        ));
    }

    #[test]
    fn remote_matches_ssh_url() {
        assert!(remote_matches_repo(
            "git@github.com:owner/repo.git",
            "owner",
            "repo"
        ));
    }

    #[test]
    fn remote_no_false_positive_substring_owner() {
        // "other-owner/repo" must not match "owner/repo"
        assert!(!remote_matches_repo(
            "https://github.com/other-owner/repo.git",
            "owner",
            "repo"
        ));
    }

    #[test]
    fn remote_no_false_positive_substring_ssh() {
        assert!(!remote_matches_repo(
            "git@github.com:other-owner/repo.git",
            "owner",
            "repo"
        ));
    }

    #[test]
    fn remote_matches_https_without_git_suffix() {
        assert!(remote_matches_repo(
            "https://github.com/owner/repo",
            "owner",
            "repo"
        ));
    }

    // ── PrOverviewData struct ──

    #[test]
    fn pr_overview_data_fields_accessible() {
        let data = PrOverviewData {
            number: 42,
            title: "Fix the bug".to_string(),
            body: "Detailed description".to_string(),
            state: "OPEN".to_string(),
            author: "contributor".to_string(),
            base_branch: "main".to_string(),
            head_branch: "fix/the-bug".to_string(),
            checks: vec![],
            reviewers: vec![],
        };
        assert_eq!(data.number, 42);
        assert_eq!(data.title, "Fix the bug");
        assert_eq!(data.state, "OPEN");
        assert_eq!(data.author, "contributor");
        assert_eq!(data.base_branch, "main");
        assert_eq!(data.head_branch, "fix/the-bug");
        assert!(data.checks.is_empty());
        assert!(data.reviewers.is_empty());
    }

    #[test]
    fn ci_check_fields_accessible() {
        let check = CiCheck {
            name: "CI / test".to_string(),
            status: "completed".to_string(),
            conclusion: Some("success".to_string()),
        };
        assert_eq!(check.name, "CI / test");
        assert_eq!(check.status, "completed");
        assert_eq!(check.conclusion, Some("success".to_string()));
    }

    #[test]
    fn ci_check_conclusion_can_be_none() {
        let check = CiCheck {
            name: "CI / test".to_string(),
            status: "in_progress".to_string(),
            conclusion: None,
        };
        assert!(check.conclusion.is_none());
    }

    #[test]
    fn reviewer_status_fields_accessible() {
        let reviewer = ReviewerStatus {
            login: "octocat".to_string(),
            state: "APPROVED".to_string(),
        };
        assert_eq!(reviewer.login, "octocat");
        assert_eq!(reviewer.state, "APPROVED");
    }

    // ── gh pr view JSON parsing (unit tests using serde_json directly) ──

    #[test]
    fn pr_overview_parsed_from_gh_json() {
        // Simulate the JSON output of `gh pr view --json number,title,body,state,author,...`
        let json = r#"{
            "number": 123,
            "title": "Add feature X",
            "body": "This PR adds feature X",
            "state": "OPEN",
            "author": {"login": "developer"},
            "baseRefName": "main",
            "headRefName": "feature/x",
            "reviews": [
                {"author": {"login": "reviewer1"}, "state": "APPROVED"},
                {"author": {"login": "reviewer2"}, "state": "CHANGES_REQUESTED"}
            ]
        }"#;

        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        let number = v["number"].as_u64().unwrap_or(0);
        let title = v["title"].as_str().unwrap_or("").to_string();
        let state = v["state"].as_str().unwrap_or("").to_string();
        let author = v["author"]["login"].as_str().unwrap_or("").to_string();
        let base_branch = v["baseRefName"].as_str().unwrap_or("").to_string();
        let head_branch = v["headRefName"].as_str().unwrap_or("").to_string();
        let reviewers: Vec<ReviewerStatus> = v["reviews"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|r| {
                        let login = r["author"]["login"].as_str()?;
                        let state = r["state"].as_str().unwrap_or("PENDING");
                        Some(ReviewerStatus {
                            login: login.to_string(),
                            state: state.to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        assert_eq!(number, 123);
        assert_eq!(title, "Add feature X");
        assert_eq!(state, "OPEN");
        assert_eq!(author, "developer");
        assert_eq!(base_branch, "main");
        assert_eq!(head_branch, "feature/x");
        assert_eq!(reviewers.len(), 2);
        assert_eq!(reviewers[0].login, "reviewer1");
        assert_eq!(reviewers[0].state, "APPROVED");
        assert_eq!(reviewers[1].login, "reviewer2");
        assert_eq!(reviewers[1].state, "CHANGES_REQUESTED");
    }

    #[test]
    fn pr_overview_handles_missing_optional_fields() {
        // Minimal JSON with only required fields
        let json = r#"{"number": 1, "title": "T", "state": "OPEN"}"#;
        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        let number = v["number"].as_u64().unwrap_or(0);
        let author = v["author"]["login"].as_str().unwrap_or("").to_string();
        let reviews: Vec<ReviewerStatus> = v["reviews"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|r| {
                        let login = r["author"]["login"].as_str()?;
                        let state = r["state"].as_str().unwrap_or("PENDING");
                        Some(ReviewerStatus {
                            login: login.to_string(),
                            state: state.to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        assert_eq!(number, 1);
        assert_eq!(author, ""); // missing → empty string default
        assert!(reviews.is_empty()); // missing → empty vec
    }

    #[test]
    fn ci_checks_parsed_from_gh_json() {
        // Simulate `gh pr checks --json name,status,conclusion` output
        let json = r#"[
            {"name": "test", "status": "completed", "conclusion": "success"},
            {"name": "lint", "status": "completed", "conclusion": "failure"},
            {"name": "build", "status": "in_progress"}
        ]"#;
        let arr: Vec<serde_json::Value> = serde_json::from_str(json).unwrap_or_default();
        let checks: Vec<CiCheck> = arr
            .iter()
            .filter_map(|c| {
                let name = c["name"].as_str()?;
                let status = c["status"].as_str().unwrap_or("unknown");
                let conclusion = c["conclusion"].as_str().map(|s| s.to_string());
                Some(CiCheck {
                    name: name.to_string(),
                    status: status.to_string(),
                    conclusion,
                })
            })
            .collect();

        assert_eq!(checks.len(), 3);
        assert_eq!(checks[0].name, "test");
        assert_eq!(checks[0].conclusion, Some("success".to_string()));
        assert_eq!(checks[1].conclusion, Some("failure".to_string()));
        assert!(checks[2].conclusion.is_none()); // in_progress — no conclusion yet
    }
}
