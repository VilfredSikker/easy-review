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
    pub url: String,
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

/// Deduplicate reviewers by login from the reviews array.
/// APPROVED and CHANGES_REQUESTED are "decisive" — a later COMMENTED review
/// does not downgrade them (matches GitHub's displayed review state).
fn deduplicate_reviewers(reviews_arr: &[serde_json::Value]) -> Vec<ReviewerStatus> {
    let mut reviewer_map: HashMap<String, String> = HashMap::new();
    for r in reviews_arr {
        if let Some(login) = r["author"]["login"].as_str() {
            let state = r["state"].as_str().unwrap_or("PENDING").to_string();
            if let Some(existing) = reviewer_map.get(login) {
                // Don't let COMMENTED overwrite a decisive review state
                if (existing == "APPROVED" || existing == "CHANGES_REQUESTED")
                    && state == "COMMENTED"
                {
                    continue;
                }
            }
            reviewer_map.insert(login.to_string(), state);
        }
    }
    reviewer_map
        .into_iter()
        .map(|(login, state)| ReviewerStatus { login, state })
        .collect()
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
    /// GitHub marks a comment outdated when the lines it was left on have since changed.
    #[serde(default)]
    pub outdated: bool,
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

    Some(PrRef {
        owner,
        repo,
        number,
    })
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

/// Get the base and head branch names for a PR with a single `gh pr view` call.
pub fn gh_pr_branch_names(pr_number: u64, repo_root: &str) -> Result<(String, String)> {
    let output = Command::new("gh")
        .args([
            "pr",
            "view",
            &pr_number.to_string(),
            "--json",
            "baseRefName,headRefName",
            "--jq",
            r#"[.baseRefName, .headRefName] | @tsv"#,
        ])
        .current_dir(repo_root)
        .output()
        .context("Failed to get PR branch names")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to get PR #{}: {}", pr_number, stderr.trim());
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let mut parts = text.trim().split('\t');
    let base = parts.next().unwrap_or("").to_string();
    let head = parts.next().unwrap_or("").to_string();
    if base.is_empty() {
        anyhow::bail!("PR #{} has no base branch", pr_number);
    }

    Ok((base, head))
}

/// Checkout a PR by number using `gh pr checkout`
#[allow(dead_code)]
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

/// Fetch PR head to a local ref without checking out. Returns the local ref name.
pub fn fetch_pr_head(number: u64, root: &str) -> Result<String> {
    let ref_name = format!("refs/er/pr/{}/head", number);
    let output = std::process::Command::new("git")
        .args([
            "fetch",
            "origin",
            &format!("pull/{}/head:{}", number, ref_name),
        ])
        .current_dir(root)
        .output()
        .context("failed to run git fetch for PR head")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git fetch PR head failed: {}", stderr.trim());
    }
    Ok(ref_name)
}

/// Easy Review owned ref path for a refreshed local branch view.
/// Stable for the same branch name so subsequent refreshes overwrite the same ref.
pub fn er_branch_ref_name(branch: &str) -> String {
    format!("refs/er/branches/{}/head", branch)
}

/// Resolve a branch's upstream short-name via `for-each-ref`.
/// Returns `Some("remote/branch")` when an upstream is configured, `None` otherwise.
pub(crate) fn branch_upstream_short(repo_root: &str, branch: &str) -> Result<Option<String>> {
    let output = Command::new("git")
        .args([
            "for-each-ref",
            "--format=%(upstream:short)",
            &format!("refs/heads/{}", branch),
        ])
        .current_dir(repo_root)
        .output()
        .context("failed to run git for-each-ref")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git for-each-ref failed: {}", stderr.trim());
    }
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if s.is_empty() {
        Ok(None)
    } else {
        Ok(Some(s))
    }
}

/// Returns `(ahead, behind)` counts for `branch` relative to its upstream.
/// `ahead` = commits local has that upstream doesn't.
/// `behind` = commits upstream has that local doesn't.
/// Returns `None` if the branch has no upstream configured.
pub(crate) fn ahead_behind_local_vs_upstream(
    repo_root: &str,
    branch: &str,
) -> Result<Option<(u32, u32)>> {
    let upstream = match branch_upstream_short(repo_root, branch)? {
        Some(u) => u,
        None => return Ok(None),
    };
    let range = format!("{}...{}", branch, upstream);
    let output = Command::new("git")
        .args(["rev-list", "--left-right", "--count", &range])
        .current_dir(repo_root)
        .output()
        .context("failed to run git rev-list for ahead/behind")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git rev-list ahead/behind failed: {}", stderr.trim());
    }
    let s = String::from_utf8_lossy(&output.stdout);
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.len() != 2 {
        anyhow::bail!("unexpected git rev-list output: {:?}", s.trim());
    }
    let ahead: u32 = parts[0].parse().context("parsing ahead count")?;
    let behind: u32 = parts[1].parse().context("parsing behind count")?;
    Ok(Some((ahead, behind)))
}

/// Force-fetch a branch's upstream into an Easy Review owned ref
/// (`refs/er/branches/<branch>/head`) without moving the user's local branch.
/// Never runs `git pull`, never checks out, never updates `refs/heads/<branch>`.
/// Returns the local Easy Review ref name on success.
pub fn fetch_branch_upstream_into_er_ref(repo_root: &str, branch: &str) -> Result<String> {
    let upstream = branch_upstream_short(repo_root, branch)?.ok_or_else(|| {
        anyhow::anyhow!(
            "Branch '{branch}' has no upstream to refresh. Run \
             `git branch --set-upstream-to=origin/{branch} {branch}` \
             or use the Local branch source instead."
        )
    })?;
    // Split remote/branch from upstream short name. Branches with slashes are valid;
    // splitn(2) gives "remote" and the rest as the remote-side branch.
    let (remote, remote_branch) = match upstream.split_once('/') {
        Some((r, b)) => (r.to_string(), b.to_string()),
        None => anyhow::bail!(
            "Branch '{}' has an unexpected upstream format: '{}'",
            branch,
            upstream
        ),
    };
    let er_ref = er_branch_ref_name(branch);
    let refspec = format!("+refs/heads/{}:{}", remote_branch, er_ref);

    let fetch = Command::new("git")
        .args(["fetch", &remote, &refspec])
        .current_dir(repo_root)
        .output()
        .context("Failed to fetch branch upstream")?;
    if !fetch.status.success() {
        let stderr = String::from_utf8_lossy(&fetch.stderr);
        anyhow::bail!(
            "Failed to fetch upstream for '{}' from '{}': {}",
            branch,
            remote,
            stderr.trim()
        );
    }
    Ok(er_ref)
}

/// Whether an error came from a local branch that genuinely has no upstream.
/// Callers may fall back to a local-only diff for this case, but not for fetch
/// failures where falling back would show stale remote-backed state.
pub fn is_no_upstream_to_refresh(err: &anyhow::Error) -> bool {
    err.chain()
        .any(|cause| cause.to_string().contains("has no upstream to refresh"))
}

/// Returns the Easy Review branch ref name if it already exists locally.
/// Used to reuse a previously refreshed view when the branch tab is recreated.
pub fn refreshed_branch_ref_if_exists(repo_root: &str, branch: &str) -> Option<String> {
    let er_ref = er_branch_ref_name(branch);
    let out = Command::new("git")
        .args(["rev-parse", "--verify", &er_ref])
        .current_dir(repo_root)
        .output()
        .ok()?;
    if out.status.success() {
        Some(er_ref)
    } else {
        None
    }
}

pub fn ref_exists_locally(repo_root: &str, ref_name: &str) -> bool {
    let out = Command::new("git")
        .args(["rev-parse", "--verify", ref_name])
        .current_dir(repo_root)
        .output();
    out.map(|o| o.status.success()).unwrap_or(false)
}

/// Resolve a fast local branch diff target without running any network fetches.
/// Priority order:
/// 1) existing `refs/er/branches/<branch>/head`
/// 2) configured upstream short ref (if locally resolvable)
/// 3) `origin/<branch>` (if locally resolvable)
/// 4) local branch name (if locally resolvable)
pub fn resolve_fast_local_branch_diff_ref(repo_root: &str, branch: &str) -> Option<String> {
    if let Some(er_ref) = refreshed_branch_ref_if_exists(repo_root, branch) {
        return Some(er_ref);
    }

    if let Ok(Some(upstream)) = branch_upstream_short(repo_root, branch) {
        if ref_exists_locally(repo_root, &upstream) {
            return Some(upstream);
        }
    }

    let origin_branch = format!("origin/{branch}");
    if ref_exists_locally(repo_root, &origin_branch) {
        return Some(origin_branch);
    }

    if ref_exists_locally(repo_root, branch) {
        return Some(branch.to_string());
    }

    None
}

/// Force-fetch a base branch from origin, updating `origin/<base>` even if it already exists.
/// Returns `origin/<base_branch>` on success.
pub fn fetch_base_branch_ref(repo_root: &str, base_branch: &str) -> Result<String> {
    let remote_ref = format!("origin/{}", base_branch);
    let refspec = format!(
        "+refs/heads/{}:refs/remotes/origin/{}",
        base_branch, base_branch
    );

    let fetch = std::process::Command::new("git")
        .args(["fetch", "origin", &refspec])
        .current_dir(repo_root)
        .output()
        .context("Failed to fetch base branch from origin")?;

    if !fetch.status.success() {
        let stderr = String::from_utf8_lossy(&fetch.stderr);
        anyhow::bail!(
            "Failed to fetch base branch '{}' from origin: {}",
            base_branch,
            stderr.trim()
        );
    }

    let verify = std::process::Command::new("git")
        .args(["rev-parse", "--verify", &remote_ref])
        .current_dir(repo_root)
        .output()
        .context("Failed to verify fetched base branch")?;

    if !verify.status.success() {
        anyhow::bail!(
            "Base branch '{}' fetched but '{}' is not resolvable",
            base_branch,
            remote_ref
        );
    }

    Ok(remote_ref)
}

/// Force-fetch the remote-tracking base ref for a diff base.
///
/// Branch tabs should not diff against a potentially stale local `main`; this
/// normalizes either `main` or `origin/main` to a freshly fetched `origin/main`.
pub fn fetch_remote_base_ref_for_diff(repo_root: &str, base_branch: &str) -> Result<String> {
    let base = base_branch.strip_prefix("origin/").unwrap_or(base_branch);
    fetch_base_branch_ref(repo_root, base)
}

/// Get the head branch name of a PR via gh CLI
pub fn gh_pr_head_branch_name(number: u64, root: &str) -> Result<String> {
    let output = std::process::Command::new("gh")
        .args([
            "pr",
            "view",
            &number.to_string(),
            "--json",
            "headRefName",
            "--jq",
            ".headRefName",
        ])
        .current_dir(root)
        .output()
        .context("failed to run gh pr view")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh pr view failed: {}", stderr.trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
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
            "pr",
            "view",
            "--json",
            "number,baseRefName",
            "--jq",
            r#"[.number, .baseRefName] | @tsv"#,
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
    let v: serde_json::Value =
        serde_json::from_str(&json_str).context("Failed to parse PR info JSON")?;

    let number = v["number"].as_u64().context("Missing PR number")?;

    // TODO(risk:minor): unwrap_or("") silently substitutes an empty string when headRepository
    // fields are absent (e.g., forked PRs where the fork has been deleted). The code below
    // detects the empty case and falls back to the git remote, which is correct. But if the
    // remote parse also fails, get_pr_info() returns an error that loses the PR number that
    // was already successfully parsed. Callers won't know which PR was involved.
    let owner = v["headRepository"]["owner"]["login"].as_str().unwrap_or("");
    let repo_name = v["headRepository"]["name"].as_str().unwrap_or("");

    // If headRepository is missing, try to get owner/repo from remote
    if owner.is_empty() || repo_name.is_empty() {
        let remote_output = Command::new("git")
            .args(["remote", "get-url", "origin"])
            .current_dir(repo_root)
            .output()?;
        let remote = String::from_utf8_lossy(&remote_output.stdout)
            .trim()
            .to_string();
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
pub fn gh_pr_comments(
    owner: &str,
    repo: &str,
    pr: u64,
    repo_root: &str,
) -> Result<Vec<GitHubComment>> {
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
        // TODO(risk:medium): this manual bracket-depth parser assumes the paginated output is
        // a flat concatenation of valid JSON arrays. If a comment body contains the string "]["
        // (which is valid JSON string content), the outer `if stdout.contains("][")` branch is
        // taken unnecessarily. Inside, the bracket counter correctly handles nesting, but
        // `start` is advanced to `i + 1` after each top-level `]`. If the next character is
        // whitespace before `[`, `start` will point at whitespace and serde_json::from_str
        // will skip it fine. However, if pagination produces non-array top-level values
        // (e.g., an error object `{}`), depth will never reach 0 from `[` tracking and the
        // batch will be silently dropped. This would silently lose comments on error responses.
        for (i, ch) in stdout.char_indices() {
            match ch {
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        if let Ok(mut batch) =
                            serde_json::from_str::<Vec<GitHubComment>>(&stdout[start..=i])
                        {
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
    owner: &str,
    repo: &str,
    pr: u64,
    path: &str,
    line: usize,
    body: &str,
    repo_root: &str,
) -> Result<u64> {
    // TODO(risk:medium): `body` is user-typed comment text passed directly to `gh api -f body=<body>`.
    // The `-f` flag in gh CLI treats the value as a literal string (not shell-expanded), so shell
    // injection is not possible. However, if `body` contains a newline character, the gh CLI may
    // split the argument at the newline in some versions, truncating the comment silently.
    // Validate or strip newlines from `body` before passing it here, or switch to writing a
    // JSON payload to a temp file and passing `--input` to gh api.

    // TODO(risk:medium): `path` is a file path from DiffFile.path (git diff output). It is
    // passed as `-f path=<path>` to the GitHub API. A file path containing "=" could be
    // misinterpreted by gh's -f flag as a key=value pair. The "--" separator is not used here.
    // Paths with "=" are valid on all major filesystems, so this is a real (if rare) edge case.

    // Get the latest commit SHA for the PR (required for review comments)
    let sha_output = Command::new("gh")
        .args([
            "pr",
            "view",
            &pr.to_string(),
            "--json",
            "headRefOid",
            "--jq",
            ".headRefOid",
        ])
        .current_dir(repo_root)
        .output()
        .context("Failed to get PR head SHA")?;

    if !sha_output.status.success() {
        let stderr = String::from_utf8_lossy(&sha_output.stderr);
        anyhow::bail!("Failed to get HEAD SHA from gh pr view: {}", stderr.trim());
    }

    let commit_id = String::from_utf8_lossy(&sha_output.stdout)
        .trim()
        .to_string();
    if commit_id.is_empty() {
        anyhow::bail!("Failed to get HEAD SHA from gh pr view: empty output");
    }

    // TODO(risk:medium): `commit_id` is a SHA obtained from `gh pr view --jq .headRefOid`.
    // Between fetching the SHA and posting the comment, a force-push could change the PR head.
    // The comment would then be posted against a commit SHA that is no longer part of the PR,
    // causing the GitHub API to return a 422 Unprocessable Entity. The current error path
    // surfaces the stderr but doesn't give the user guidance on what went wrong.
    let output = Command::new("gh")
        .args([
            "api",
            "-X",
            "POST",
            &format!("repos/{}/{}/pulls/{}/comments", owner, repo, pr),
            "-f",
            &format!("body={}", body),
            "-f",
            &format!("path={}", path),
            "-F",
            &format!("line={}", line),
            "-f",
            "side=RIGHT",
            "-f",
            &format!("commit_id={}", commit_id),
        ])
        .current_dir(repo_root)
        .output()
        .context("Failed to push comment to GitHub")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to push comment: {}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let resp: CreateCommentResponse =
        serde_json::from_str(&stdout).context("Failed to parse create comment response")?;

    Ok(resp.id)
}

/// Push a reply to an existing review comment
pub fn gh_pr_reply_comment(
    owner: &str,
    repo: &str,
    pr: u64,
    in_reply_to: u64,
    body: &str,
    repo_root: &str,
) -> Result<u64> {
    // TODO(risk:medium): same `body` newline concern as gh_pr_push_comment — newlines in
    // the body string may truncate the comment silently via gh's -f flag handling.
    let output = Command::new("gh")
        .args([
            "api",
            "-X",
            "POST",
            &format!("repos/{}/{}/pulls/{}/comments", owner, repo, pr),
            "-f",
            &format!("body={}", body),
            "-F",
            &format!("in_reply_to={}", in_reply_to),
        ])
        .current_dir(repo_root)
        .output()
        .context("Failed to push reply to GitHub")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to push reply: {}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let resp: CreateCommentResponse =
        serde_json::from_str(&stdout).context("Failed to parse reply response")?;

    Ok(resp.id)
}

/// Delete a review comment from a PR
pub fn gh_pr_delete_comment(
    owner: &str,
    repo: &str,
    comment_id: u64,
    repo_root: &str,
) -> Result<()> {
    let output = Command::new("gh")
        .args([
            "api",
            "-X",
            "DELETE",
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

/// Update the PR body with the given content via `gh pr edit --body`
pub fn gh_pr_edit_body(repo_root: &str, body: &str) -> Result<()> {
    let output = Command::new("gh")
        .args(["pr", "edit", "--body", body])
        .current_dir(repo_root)
        .output()
        .context("Failed to update PR body")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to update PR body: {}", stderr.trim());
    }

    Ok(())
}

/// Approve a PR via `gh pr review --approve`.
/// Uses `--repo` and PR number when reviewing a remote PR.
pub fn gh_pr_approve(
    repo_root: &str,
    remote_repo: Option<&str>,
    pr_number: Option<u64>,
) -> Result<()> {
    let mut cmd = Command::new("gh");
    cmd.args(["pr", "review", "--approve"]);
    if let (Some(slug), Some(n)) = (remote_repo, pr_number) {
        cmd.args(["--repo", slug, &n.to_string()]);
    }
    cmd.current_dir(repo_root);
    let output = cmd
        .output()
        .context("Failed to run gh pr review --approve")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh pr approve failed: {}", stderr.trim());
    }
    Ok(())
}

/// Fetch PR overview data: title, body, state, author, branches, reviewers.
/// If `pr_number` is Some, fetches that specific PR; otherwise auto-detects from current branch.
pub fn gh_pr_overview(repo_root: &str, pr_number: Option<u64>) -> Option<PrOverviewData> {
    // Fetch core PR fields
    let mut args = vec!["pr", "view"];
    let pr_num_str;
    if let Some(n) = pr_number {
        pr_num_str = n.to_string();
        args.push(&pr_num_str);
    }
    args.extend_from_slice(&[
        "--json",
        "number,title,body,state,author,url,baseRefName,headRefName,reviews",
    ]);
    let view_output = Command::new("gh")
        .args(&args)
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
    let url = v["url"].as_str().unwrap_or("").to_string();
    let base_branch = v["baseRefName"].as_str().unwrap_or("").to_string();
    let head_branch = v["headRefName"].as_str().unwrap_or("").to_string();

    let reviewers: Vec<ReviewerStatus> = if let Some(reviews_arr) = v["reviews"].as_array() {
        deduplicate_reviewers(reviews_arr)
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
        url,
        base_branch,
        head_branch,
        checks,
        reviewers,
    })
}

/// Fetch CI check runs for the current PR
fn gh_pr_checks_data(repo_root: &str) -> Result<Vec<CiCheck>> {
    let output = Command::new("gh")
        .args(["pr", "checks", "--json", "name,state,bucket"])
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
            // gh pr checks uses "state" (e.g. SUCCESS, FAILURE, PENDING)
            // and "bucket" (pass, fail, pending) for grouping
            let state = c["state"].as_str().unwrap_or("unknown");
            let bucket = c["bucket"].as_str().map(|s| s.to_string());
            Some(CiCheck {
                name: name.to_string(),
                status: state.to_string(),
                conclusion: bucket,
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

/// Get raw unified diff for a PR via `gh pr diff N --repo owner/repo`.
/// Works without a local clone — uses GitHub API via gh CLI.
/// Falls back to shallow clone + local git diff when the PR exceeds GitHub's
/// API line limit (HTTP 406 / diff_too_large).
pub fn gh_pr_diff_remote(owner: &str, repo: &str, number: u64) -> Result<String> {
    let repo_slug = format!("{}/{}", owner, repo);
    let output = Command::new("gh")
        .args(["pr", "diff", &number.to_string(), "--repo", &repo_slug])
        .output()
        .context("Failed to run gh pr diff")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("406") || stderr.contains("too_large") {
            return gh_pr_diff_via_clone(owner, repo, number);
        }
        anyhow::bail!("Failed to get PR diff: {}", stderr.trim());
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Get raw unified diff for a PR using the local repo context.
///
/// This intentionally delegates PR-source rendering to GitHub instead of
/// reconstructing the range with local refs. GitHub PR diffs are not just
/// "whatever local `origin/<base>...refs/pull/N/head` happens to produce" in
/// every edge case, and using `gh pr diff` keeps Easy Review aligned with the
/// Files changed view.
pub fn gh_pr_diff(pr_number: u64, repo_root: &str) -> Result<String> {
    let output = Command::new("gh")
        .args(["pr", "diff", &pr_number.to_string()])
        .current_dir(repo_root)
        .output()
        .context("Failed to run gh pr diff")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to get PR diff: {}", stderr.trim());
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Fallback for large PRs: shallow-clone the repo into a temp dir, fetch both
/// refs, and produce the diff locally. The temp dir is cleaned up automatically.
fn gh_pr_diff_via_clone(owner: &str, repo: &str, number: u64) -> Result<String> {
    let (base_ref, head_ref) = gh_pr_metadata_remote(owner, repo, number)?;
    let repo_url = format!("https://github.com/{}/{}.git", owner, repo);
    let tmp_dir = std::env::temp_dir().join(format!("er-remote-{}-{}-{}", owner, repo, number));
    let tmp_path = tmp_dir
        .to_str()
        .context("Temp dir path is not valid UTF-8")?
        .to_string();

    // Clean up any previous failed attempt
    let _ = std::fs::remove_dir_all(&tmp_dir);

    // Shallow clone with only the two refs we need
    let clone = Command::new("git")
        .args([
            "clone",
            "--depth=1",
            "--no-checkout",
            "--filter=blob:none",
            &repo_url,
            &tmp_path,
        ])
        .output()
        .context("Failed to shallow clone for large PR diff")?;

    if !clone.status.success() {
        let _ = std::fs::remove_dir_all(&tmp_dir);
        let stderr = String::from_utf8_lossy(&clone.stderr);
        anyhow::bail!("Shallow clone failed: {}", stderr.trim());
    }

    // Fetch the base and head refs
    let fetch = Command::new("git")
        .args([
            "-C",
            &tmp_path,
            "fetch",
            "--depth=1",
            "origin",
            &base_ref,
            &head_ref,
        ])
        .output()
        .context("Failed to fetch PR refs")?;

    if !fetch.status.success() {
        let _ = std::fs::remove_dir_all(&tmp_dir);
        let stderr = String::from_utf8_lossy(&fetch.stderr);
        anyhow::bail!("Failed to fetch PR refs: {}", stderr.trim());
    }

    // Generate the diff
    let unified_arg = format!("--unified={}", crate::git::DEFAULT_CONTEXT_LINES);
    let diff = Command::new("git")
        .args([
            "-C",
            &tmp_path,
            "diff",
            &format!("origin/{}", base_ref),
            &format!("origin/{}", head_ref),
            &unified_arg,
            "--no-color",
            "--no-ext-diff",
        ])
        .output()
        .context("Failed to generate diff from cloned repo")?;

    // Clean up
    let _ = std::fs::remove_dir_all(&tmp_dir);

    if !diff.status.success() {
        let stderr = String::from_utf8_lossy(&diff.stderr);
        anyhow::bail!("git diff failed in cloned repo: {}", stderr.trim());
    }

    Ok(String::from_utf8_lossy(&diff.stdout).to_string())
}

/// Get PR metadata (base branch, head branch) via `gh pr view --repo`.
/// Returns (base_ref_name, head_ref_name).
pub fn gh_pr_metadata_remote(owner: &str, repo: &str, number: u64) -> Result<(String, String)> {
    let repo_slug = format!("{}/{}", owner, repo);
    let output = Command::new("gh")
        .args([
            "pr",
            "view",
            &number.to_string(),
            "--repo",
            &repo_slug,
            "--json",
            "baseRefName,headRefName",
            "--jq",
            r#"[.baseRefName, .headRefName] | @tsv"#,
        ])
        .output()
        .context("Failed to get PR metadata")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to get PR #{} metadata: {}", number, stderr.trim());
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let text = text.trim();
    let (base, head) = text
        .split_once('\t')
        .ok_or_else(|| anyhow::anyhow!("Unexpected gh pr view output: {}", text))?;

    Ok((base.to_string(), head.to_string()))
}

/// Fetch PR overview data for a remote repo (no local clone needed).
pub fn gh_pr_overview_remote(owner: &str, repo: &str, number: u64) -> Option<PrOverviewData> {
    let repo_slug = format!("{}/{}", owner, repo);
    let view_output = Command::new("gh")
        .args([
            "pr",
            "view",
            &number.to_string(),
            "--repo",
            &repo_slug,
            "--json",
            "number,title,body,state,author,url,baseRefName,headRefName,reviews",
        ])
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
    let url = v["url"].as_str().unwrap_or("").to_string();
    let base_branch = v["baseRefName"].as_str().unwrap_or("").to_string();
    let head_branch = v["headRefName"].as_str().unwrap_or("").to_string();

    let reviewers: Vec<ReviewerStatus> = if let Some(reviews_arr) = v["reviews"].as_array() {
        deduplicate_reviewers(reviews_arr)
    } else {
        Vec::new()
    };

    // Skip CI checks for remote (would need --repo flag on gh pr checks too)
    let checks = Vec::new();

    Some(PrOverviewData {
        number,
        title,
        body,
        state,
        author,
        url,
        base_branch,
        head_branch,
        checks,
        reviewers,
    })
}

/// Get PR info (owner, repo, number) for a remote repo. Used for comment sync in remote mode.
#[allow(dead_code)]
pub fn get_pr_info_remote(owner: &str, repo: &str, number: u64) -> (String, String, u64) {
    (owner.to_string(), repo.to_string(), number)
}

/// Fetch PR comments for a remote repo (no local clone needed).
pub fn gh_pr_comments_remote(owner: &str, repo: &str, pr: u64) -> Result<Vec<GitHubComment>> {
    let output = Command::new("gh")
        .args([
            "api",
            &format!("repos/{}/{}/pulls/{}/comments", owner, repo, pr),
            "--paginate",
        ])
        .output()
        .context("Failed to fetch PR comments")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to fetch PR comments: {}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let all_comments: Vec<GitHubComment> = if stdout.contains("][") {
        let mut results = Vec::new();
        let mut depth = 0i32;
        let mut start = 0;
        for (i, ch) in stdout.char_indices() {
            match ch {
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        if let Ok(mut batch) =
                            serde_json::from_str::<Vec<GitHubComment>>(&stdout[start..=i])
                        {
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

// ── GraphQL review thread resolution ──

/// Response from GraphQL review threads query
#[derive(Debug, Deserialize)]
struct ReviewThreadsResponse {
    data: ReviewThreadsData,
}

#[derive(Debug, Deserialize)]
struct ReviewThreadsData {
    repository: ReviewThreadsRepo,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewThreadsRepo {
    pull_request: ReviewThreadsPr,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewThreadsPr {
    review_threads: ReviewThreadsConnection,
}

#[derive(Debug, Deserialize)]
struct ReviewThreadsConnection {
    nodes: Vec<ReviewThread>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewThread {
    is_resolved: bool,
    is_outdated: bool,
    comments: ReviewThreadComments,
}

#[derive(Debug, Deserialize)]
struct ReviewThreadComments {
    nodes: Vec<ReviewThreadComment>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewThreadComment {
    database_id: Option<u64>,
}

/// GitHub review thread state keyed by REST review comment ID.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReviewThreadState {
    pub resolved: bool,
    pub outdated: bool,
}

/// Fetch review thread state for a PR (local repo).
/// Returns a map of comment_id -> thread state for all review-thread comments.
pub fn gh_pr_review_threads(
    owner: &str,
    repo: &str,
    pr: u64,
    repo_root: &str,
) -> Result<HashMap<u64, ReviewThreadState>> {
    let query = format!(
        r#"query {{ repository(owner: "{}", name: "{}") {{ pullRequest(number: {}) {{ reviewThreads(first: 100) {{ nodes {{ isResolved isOutdated comments(first: 100) {{ nodes {{ databaseId }} }} }} }} }} }} }}"#,
        owner, repo, pr
    );

    let output = Command::new("gh")
        .args(["api", "graphql", "-f", &format!("query={}", query)])
        .current_dir(repo_root)
        .output()
        .context("Failed to fetch review threads")?;

    if !output.status.success() {
        // Non-fatal: fall back to all-unresolved if GraphQL fails
        return Ok(HashMap::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_review_threads_response(&stdout)
}

/// Fetch review thread state for a remote PR (no local clone needed).
pub fn gh_pr_review_threads_remote(
    owner: &str,
    repo: &str,
    pr: u64,
) -> Result<HashMap<u64, ReviewThreadState>> {
    let query = format!(
        r#"query {{ repository(owner: "{}", name: "{}") {{ pullRequest(number: {}) {{ reviewThreads(first: 100) {{ nodes {{ isResolved isOutdated comments(first: 100) {{ nodes {{ databaseId }} }} }} }} }} }} }}"#,
        owner, repo, pr
    );

    let output = Command::new("gh")
        .args(["api", "graphql", "-f", &format!("query={}", query)])
        .output()
        .context("Failed to fetch review threads")?;

    if !output.status.success() {
        return Ok(HashMap::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_review_threads_response(&stdout)
}

/// Parse the GraphQL response into a comment_id -> review thread state map.
fn parse_review_threads_response(json: &str) -> Result<HashMap<u64, ReviewThreadState>> {
    let mut state_map = HashMap::new();
    let response: ReviewThreadsResponse = match serde_json::from_str(json) {
        Ok(r) => r,
        Err(_) => return Ok(state_map),
    };

    for thread in &response.data.repository.pull_request.review_threads.nodes {
        for comment in &thread.comments.nodes {
            if let Some(id) = comment.database_id {
                state_map.insert(
                    id,
                    ReviewThreadState {
                        resolved: thread.is_resolved,
                        outdated: thread.is_outdated,
                    },
                );
            }
        }
    }

    Ok(state_map)
}

/// Push a new review comment to a remote PR (no local clone needed).
pub fn gh_pr_push_comment_remote(
    owner: &str,
    repo: &str,
    pr: u64,
    path: &str,
    line: usize,
    body: &str,
) -> Result<u64> {
    // Get the latest commit SHA for the PR
    let repo_slug = format!("{}/{}", owner, repo);
    let sha_output = Command::new("gh")
        .args([
            "pr",
            "view",
            &pr.to_string(),
            "--repo",
            &repo_slug,
            "--json",
            "headRefOid",
            "--jq",
            ".headRefOid",
        ])
        .output()
        .context("Failed to get PR head SHA")?;

    if !sha_output.status.success() {
        let stderr = String::from_utf8_lossy(&sha_output.stderr);
        anyhow::bail!("Failed to get HEAD SHA: {}", stderr.trim());
    }

    let commit_id = String::from_utf8_lossy(&sha_output.stdout)
        .trim()
        .to_string();
    if commit_id.is_empty() {
        anyhow::bail!("Failed to get HEAD SHA: empty output");
    }

    let output = Command::new("gh")
        .args([
            "api",
            "-X",
            "POST",
            &format!("repos/{}/{}/pulls/{}/comments", owner, repo, pr),
            "-f",
            &format!("body={}", body),
            "-f",
            &format!("path={}", path),
            "-F",
            &format!("line={}", line),
            "-f",
            "side=RIGHT",
            "-f",
            &format!("commit_id={}", commit_id),
        ])
        .output()
        .context("Failed to push comment to GitHub")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to push comment: {}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let resp: CreateCommentResponse =
        serde_json::from_str(&stdout).context("Failed to parse create comment response")?;

    Ok(resp.id)
}

/// A single comment entry for a batch PR review submission.
pub struct ReviewBatchEntry {
    pub file: String,
    pub line: usize,
    pub body: String,
    pub side: String,
}

/// Submit a batch PR review with multiple comments in one API call.
/// `comments` is a slice of `ReviewBatchEntry`. Marks all included comments as synced
/// (no individual comment IDs are returned by the review API).
pub fn gh_pr_submit_review(
    owner: &str,
    repo: &str,
    pr: u64,
    comments: &[ReviewBatchEntry],
    repo_root: &str,
    event: &str,
    body: &str,
) -> Result<()> {
    // Fetch the head commit SHA
    let sha_output = Command::new("gh")
        .args([
            "pr",
            "view",
            &pr.to_string(),
            "--json",
            "headRefOid",
            "--jq",
            ".headRefOid",
        ])
        .current_dir(repo_root)
        .output()
        .context("Failed to get PR head SHA")?;

    if !sha_output.status.success() {
        let stderr = String::from_utf8_lossy(&sha_output.stderr);
        anyhow::bail!("Failed to get HEAD SHA: {}", stderr.trim());
    }

    let commit_id = String::from_utf8_lossy(&sha_output.stdout)
        .trim()
        .to_string();
    if commit_id.is_empty() {
        anyhow::bail!("Failed to get HEAD SHA: empty output");
    }

    // Build the review JSON payload
    let comment_values: Vec<serde_json::Value> = comments
        .iter()
        .map(|entry| {
            serde_json::json!({
                "path": entry.file,
                "line": entry.line,
                "side": entry.side,
                "body": entry.body
            })
        })
        .collect();

    let payload = serde_json::json!({
        "commit_id": commit_id,
        "event": event,
        "body": body,
        "comments": comment_values
    });

    // Write to temp file and pass via --input
    let tmp_path = format!("/tmp/er_review_payload_{}.json", std::process::id());
    std::fs::write(&tmp_path, serde_json::to_string(&payload)?)
        .context("Failed to write review payload")?;

    let output = Command::new("gh")
        .args([
            "api",
            "-X",
            "POST",
            &format!("repos/{}/{}/pulls/{}/reviews", owner, repo, pr),
            "--input",
            &tmp_path,
        ])
        .current_dir(repo_root)
        .output()
        .context("Failed to submit PR review")?;

    let _ = std::fs::remove_file(&tmp_path);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to submit review: {}", stderr.trim());
    }

    Ok(())
}

/// Submit a batch PR review with multiple comments on a remote PR (no local repo required).
pub fn gh_pr_submit_review_remote(
    owner: &str,
    repo: &str,
    pr: u64,
    comments: &[ReviewBatchEntry],
    event: &str,
    body: &str,
) -> Result<()> {
    let repo_slug = format!("{}/{}", owner, repo);
    let sha_output = Command::new("gh")
        .args([
            "pr",
            "view",
            &pr.to_string(),
            "--repo",
            &repo_slug,
            "--json",
            "headRefOid",
            "--jq",
            ".headRefOid",
        ])
        .output()
        .context("Failed to get PR head SHA")?;

    if !sha_output.status.success() {
        let stderr = String::from_utf8_lossy(&sha_output.stderr);
        anyhow::bail!("Failed to get HEAD SHA: {}", stderr.trim());
    }

    let commit_id = String::from_utf8_lossy(&sha_output.stdout)
        .trim()
        .to_string();
    if commit_id.is_empty() {
        anyhow::bail!("Failed to get HEAD SHA: empty output");
    }

    let comment_values: Vec<serde_json::Value> = comments
        .iter()
        .map(|entry| {
            serde_json::json!({
                "path": entry.file,
                "line": entry.line,
                "side": entry.side,
                "body": entry.body
            })
        })
        .collect();

    let payload = serde_json::json!({
        "commit_id": commit_id,
        "event": event,
        "body": body,
        "comments": comment_values
    });

    let tmp_path = format!("/tmp/er_review_payload_{}.json", std::process::id());
    std::fs::write(&tmp_path, serde_json::to_string(&payload)?)
        .context("Failed to write review payload")?;

    let output = Command::new("gh")
        .args([
            "api",
            "-X",
            "POST",
            &format!("repos/{}/{}/pulls/{}/reviews", owner, repo, pr),
            "--input",
            &tmp_path,
        ])
        .output()
        .context("Failed to submit PR review")?;

    let _ = std::fs::remove_file(&tmp_path);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to submit review: {}", stderr.trim());
    }

    Ok(())
}

/// Post a general comment on a PR (not attached to any file/line).
/// Uses the Issues API — PRs are issues in GitHub.
pub fn gh_pr_general_comment(
    owner: &str,
    repo: &str,
    pr: u64,
    body: &str,
    repo_root: &str,
) -> Result<u64> {
    let output = Command::new("gh")
        .args([
            "api",
            "-X",
            "POST",
            &format!("repos/{}/{}/issues/{}/comments", owner, repo, pr),
            "-f",
            &format!("body={}", body),
        ])
        .current_dir(repo_root)
        .output()
        .context("Failed to post general PR comment")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to post general comment: {}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let resp: CreateCommentResponse =
        serde_json::from_str(&stdout).context("Failed to parse general comment response")?;

    Ok(resp.id)
}

/// Post a general comment on a remote PR (no local repo required).
pub fn gh_pr_general_comment_remote(owner: &str, repo: &str, pr: u64, body: &str) -> Result<u64> {
    let output = Command::new("gh")
        .args([
            "api",
            "-X",
            "POST",
            &format!("repos/{}/{}/issues/{}/comments", owner, repo, pr),
            "-f",
            &format!("body={}", body),
        ])
        .output()
        .context("Failed to post general PR comment")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to post general comment: {}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let resp: CreateCommentResponse =
        serde_json::from_str(&stdout).context("Failed to parse general comment response")?;

    Ok(resp.id)
}

// ─────────────────────────────────────────────────────────────────────────────
// Rich GitHub status for the SourcesCard. These mirror existing `gh_pr_*`
// helpers but return richer data (review decision, mergeable, labels, checks,
// recent comments/reviews). Parsing is extracted into pure functions so it can
// be unit-tested against fixed JSON blobs.
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct PrOverviewFull {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub state: String,
    pub is_draft: bool,
    pub author: String,
    pub url: String,
    pub review_decision: Option<String>,
    pub mergeable: Option<String>,
    pub head_ref_name: String,
    pub base_ref_name: String,
    pub labels: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PrComment {
    pub author: String,
    pub body: String,
    pub created_at: String,
    pub url: String,
}

#[derive(Debug, Clone)]
pub struct PrReview {
    pub author: String,
    pub state: String,
    pub body: String,
    pub submitted_at: String,
}

#[derive(Debug, Clone)]
pub struct CheckRun {
    pub name: String,
    pub status: String,
    pub conclusion: String,
    pub url: Option<String>,
}

/// Pure parser for `gh pr view --json number,title,state,isDraft,author,reviewDecision,mergeable,headRefName,baseRefName,labels,url` output.
pub fn parse_pr_overview(json: &str) -> Result<PrOverviewFull> {
    let v: serde_json::Value = serde_json::from_str(json).context("invalid PR overview JSON")?;
    Ok(PrOverviewFull {
        number: v["number"].as_u64().unwrap_or(0),
        title: v["title"].as_str().unwrap_or("").to_string(),
        body: v["body"].as_str().unwrap_or("").to_string(),
        state: v["state"].as_str().unwrap_or("").to_string(),
        is_draft: v["isDraft"].as_bool().unwrap_or(false),
        author: v["author"]["login"].as_str().unwrap_or("").to_string(),
        url: v["url"].as_str().unwrap_or("").to_string(),
        review_decision: v["reviewDecision"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string()),
        mergeable: v["mergeable"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string()),
        head_ref_name: v["headRefName"].as_str().unwrap_or("").to_string(),
        base_ref_name: v["baseRefName"].as_str().unwrap_or("").to_string(),
        labels: v["labels"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|l| l["name"].as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default(),
    })
}

/// Pure parser for `gh pr view --json comments` output (the wrapping object).
pub fn parse_pr_comments(json: &str) -> Result<Vec<PrComment>> {
    let v: serde_json::Value = serde_json::from_str(json).context("invalid comments JSON")?;
    let arr = v["comments"].as_array().cloned().unwrap_or_default();
    Ok(arr
        .into_iter()
        .map(|c| PrComment {
            author: c["author"]["login"].as_str().unwrap_or("").to_string(),
            body: c["body"].as_str().unwrap_or("").to_string(),
            created_at: c["createdAt"].as_str().unwrap_or("").to_string(),
            url: c["url"].as_str().unwrap_or("").to_string(),
        })
        .collect())
}

/// Pure parser for `gh pr view --json reviews` output (the wrapping object).
pub fn parse_pr_reviews(json: &str) -> Result<Vec<PrReview>> {
    let v: serde_json::Value = serde_json::from_str(json).context("invalid reviews JSON")?;
    let arr = v["reviews"].as_array().cloned().unwrap_or_default();
    Ok(arr
        .into_iter()
        .map(|r| PrReview {
            author: r["author"]["login"].as_str().unwrap_or("").to_string(),
            state: r["state"].as_str().unwrap_or("").to_string(),
            body: r["body"].as_str().unwrap_or("").to_string(),
            submitted_at: r["submittedAt"].as_str().unwrap_or("").to_string(),
        })
        .collect())
}

/// Pure parser for `gh pr checks --json name,state,bucket,link` output.
pub fn parse_pr_checks(json: &str) -> Result<Vec<CheckRun>> {
    let v: serde_json::Value = serde_json::from_str(json).context("invalid checks JSON")?;
    let arr = v.as_array().cloned().unwrap_or_default();
    Ok(arr
        .into_iter()
        .map(|c| {
            let state = c["state"].as_str().unwrap_or("").to_string();
            let bucket = c["bucket"].as_str().unwrap_or("").to_string();
            // Map `gh pr checks` semantics into status/conclusion:
            //   state ~ PENDING|SUCCESS|FAILURE|...
            //   bucket ~ pass|fail|pending|cancel|skipping
            let (status, conclusion) = if state == "PENDING" || bucket == "pending" {
                ("PENDING".to_string(), "".to_string())
            } else {
                ("COMPLETED".to_string(), state.clone())
            };
            CheckRun {
                name: c["name"].as_str().unwrap_or("").to_string(),
                status,
                conclusion: if conclusion.is_empty() {
                    bucket
                } else {
                    conclusion
                },
                url: c["link"].as_str().map(|s| s.to_string()),
            }
        })
        .collect())
}

/// Fetch a rich PR overview (state, draft, review decision, mergeable, labels)
/// for a remote PR. No local clone required.
pub fn gh_pr_overview_remote_full(owner: &str, repo: &str, number: u64) -> Result<PrOverviewFull> {
    let repo_slug = format!("{}/{}", owner, repo);
    let output = Command::new("gh")
        .args([
            "pr",
            "view",
            &number.to_string(),
            "--repo",
            &repo_slug,
            "--json",
            "number,title,body,state,isDraft,author,reviewDecision,mergeable,headRefName,baseRefName,labels,url",
        ])
        .output()
        .context("Failed to run gh pr view")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh pr view failed: {}", stderr.trim());
    }
    parse_pr_overview(&String::from_utf8_lossy(&output.stdout))
}

/// Fetch general PR conversation comments (the issue-comments stream — NOT the
/// per-line review comments fetched by `gh_pr_comments_remote`).
pub fn gh_pr_comments_overview(owner: &str, repo: &str, number: u64) -> Result<Vec<PrComment>> {
    let repo_slug = format!("{}/{}", owner, repo);
    let output = Command::new("gh")
        .args([
            "pr",
            "view",
            &number.to_string(),
            "--repo",
            &repo_slug,
            "--json",
            "comments",
        ])
        .output()
        .context("Failed to run gh pr view (comments)")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh pr view comments failed: {}", stderr.trim());
    }
    parse_pr_comments(&String::from_utf8_lossy(&output.stdout))
}

/// Fetch reviews (APPROVED / CHANGES_REQUESTED / COMMENTED) for a remote PR.
pub fn gh_pr_reviews(owner: &str, repo: &str, number: u64) -> Result<Vec<PrReview>> {
    let repo_slug = format!("{}/{}", owner, repo);
    let output = Command::new("gh")
        .args([
            "pr",
            "view",
            &number.to_string(),
            "--repo",
            &repo_slug,
            "--json",
            "reviews",
        ])
        .output()
        .context("Failed to run gh pr view (reviews)")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh pr view reviews failed: {}", stderr.trim());
    }
    parse_pr_reviews(&String::from_utf8_lossy(&output.stdout))
}

/// Fetch CI check runs for a remote PR.
pub fn gh_pr_checks_remote(owner: &str, repo: &str, number: u64) -> Result<Vec<CheckRun>> {
    let repo_slug = format!("{}/{}", owner, repo);
    let output = Command::new("gh")
        .args([
            "pr",
            "checks",
            &number.to_string(),
            "--repo",
            &repo_slug,
            "--json",
            "name,state,bucket,link",
        ])
        .output()
        .context("Failed to run gh pr checks")?;
    if !output.status.success() {
        // No checks configured is not an error — return empty.
        return Ok(Vec::new());
    }
    parse_pr_checks(&String::from_utf8_lossy(&output.stdout))
}

/// Push a reply to an existing review comment on a remote PR.
pub fn gh_pr_reply_comment_remote(
    owner: &str,
    repo: &str,
    pr: u64,
    in_reply_to: u64,
    body: &str,
) -> Result<u64> {
    let output = Command::new("gh")
        .args([
            "api",
            "-X",
            "POST",
            &format!("repos/{}/{}/pulls/{}/comments", owner, repo, pr),
            "-f",
            &format!("body={}", body),
            "-F",
            &format!("in_reply_to={}", in_reply_to),
        ])
        .output()
        .context("Failed to push reply to GitHub")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to push reply: {}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let resp: CreateCommentResponse =
        serde_json::from_str(&stdout).context("Failed to parse reply response")?;

    Ok(resp.id)
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

    #[test]
    fn is_no_upstream_to_refresh_only_matches_no_upstream_errors() {
        let no_upstream = anyhow::anyhow!("Branch 'feature/a' has no upstream to refresh");
        let fetch_failed =
            anyhow::anyhow!("Failed to fetch upstream for 'feature/a' from 'origin': failed");

        assert!(is_no_upstream_to_refresh(&no_upstream));
        assert!(!is_no_upstream_to_refresh(&fetch_failed));
    }

    #[test]
    fn parse_review_threads_response_extracts_resolved_and_outdated_state() {
        let json = r#"{
            "data": {
                "repository": {
                    "pullRequest": {
                        "reviewThreads": {
                            "nodes": [
                                {
                                    "isResolved": false,
                                    "isOutdated": true,
                                    "comments": {
                                        "nodes": [
                                            { "databaseId": 3188301632 }
                                        ]
                                    }
                                },
                                {
                                    "isResolved": true,
                                    "isOutdated": false,
                                    "comments": {
                                        "nodes": [
                                            { "databaseId": 3188301633 },
                                            { "databaseId": null }
                                        ]
                                    }
                                }
                            ]
                        }
                    }
                }
            }
        }"#;

        let state = parse_review_threads_response(json).unwrap();

        assert_eq!(
            state.get(&3188301632),
            Some(&ReviewThreadState {
                resolved: false,
                outdated: true,
            })
        );
        assert_eq!(
            state.get(&3188301633),
            Some(&ReviewThreadState {
                resolved: true,
                outdated: false,
            })
        );
        assert!(!state.contains_key(&0));
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
            url: "https://github.com/owner/repo/pull/42".to_string(),
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
    fn get_pr_info_remote_returns_tuple() {
        let (o, r, n) = super::get_pr_info_remote("owner", "repo", 42);
        assert_eq!(o, "owner");
        assert_eq!(r, "repo");
        assert_eq!(n, 42);
    }

    // ── parse_pr_overview ──

    #[test]
    fn parse_pr_overview_full_json() {
        let json = r#"{
            "number": 7,
            "title": "Add feature",
            "state": "OPEN",
            "isDraft": false,
            "author": {"login": "alice"},
            "url": "https://github.com/o/r/pull/7",
            "reviewDecision": "APPROVED",
            "mergeable": "MERGEABLE",
            "headRefName": "feat/x",
            "baseRefName": "main",
            "labels": [{"name": "bug"}, {"name": "ui"}]
        }"#;
        let pr = parse_pr_overview(json).unwrap();
        assert_eq!(pr.number, 7);
        assert_eq!(pr.title, "Add feature");
        assert_eq!(pr.state, "OPEN");
        assert!(!pr.is_draft);
        assert_eq!(pr.author, "alice");
        assert_eq!(pr.review_decision.as_deref(), Some("APPROVED"));
        assert_eq!(pr.mergeable.as_deref(), Some("MERGEABLE"));
        assert_eq!(pr.head_ref_name, "feat/x");
        assert_eq!(pr.base_ref_name, "main");
        assert_eq!(pr.labels, vec!["bug".to_string(), "ui".to_string()]);
    }

    #[test]
    fn parse_pr_overview_missing_optional_fields() {
        let json = r#"{"number": 1, "title": "T", "state": "OPEN", "isDraft": true, "author": {}, "labels": []}"#;
        let pr = parse_pr_overview(json).unwrap();
        assert_eq!(pr.number, 1);
        assert!(pr.is_draft);
        assert_eq!(pr.author, "");
        assert!(pr.labels.is_empty());
        assert!(pr.review_decision.is_none());
        assert!(pr.mergeable.is_none());
    }

    #[test]
    fn parse_pr_overview_invalid_json_errors() {
        assert!(parse_pr_overview("not json").is_err());
    }

    // ── parse_pr_comments ──

    #[test]
    fn parse_pr_comments_extracts_list() {
        let json = r#"{"comments": [
            {"author": {"login": "bob"}, "body": "Nice!", "createdAt": "2025-01-01T00:00:00Z", "url": "https://x/1"},
            {"author": {"login": "carol"}, "body": "+1", "createdAt": "2025-01-02T00:00:00Z", "url": "https://x/2"}
        ]}"#;
        let comments = parse_pr_comments(json).unwrap();
        assert_eq!(comments.len(), 2);
        assert_eq!(comments[0].author, "bob");
        assert_eq!(comments[0].body, "Nice!");
        assert_eq!(comments[1].author, "carol");
    }

    #[test]
    fn parse_pr_comments_empty_array() {
        let json = r#"{"comments": []}"#;
        let comments = parse_pr_comments(json).unwrap();
        assert!(comments.is_empty());
    }

    // ── parse_pr_reviews ──

    #[test]
    fn parse_pr_reviews_extracts_states() {
        let json = r#"{"reviews": [
            {"author": {"login": "r1"}, "state": "APPROVED", "body": "", "submittedAt": "2025-01-01T00:00:00Z"},
            {"author": {"login": "r2"}, "state": "CHANGES_REQUESTED", "body": "fix", "submittedAt": "2025-01-02T00:00:00Z"}
        ]}"#;
        let reviews = parse_pr_reviews(json).unwrap();
        assert_eq!(reviews.len(), 2);
        assert_eq!(reviews[0].state, "APPROVED");
        assert_eq!(reviews[1].state, "CHANGES_REQUESTED");
        assert_eq!(reviews[1].body, "fix");
    }

    // ── parse_pr_checks ──

    #[test]
    fn parse_pr_checks_maps_state_and_bucket() {
        let json = r#"[
            {"name": "test", "state": "SUCCESS", "bucket": "pass", "link": "https://ci/1"},
            {"name": "lint", "state": "FAILURE", "bucket": "fail", "link": "https://ci/2"},
            {"name": "build", "state": "PENDING", "bucket": "pending"}
        ]"#;
        let checks = parse_pr_checks(json).unwrap();
        assert_eq!(checks.len(), 3);
        assert_eq!(checks[0].status, "COMPLETED");
        assert_eq!(checks[0].conclusion, "SUCCESS");
        assert_eq!(checks[0].url.as_deref(), Some("https://ci/1"));
        assert_eq!(checks[1].conclusion, "FAILURE");
        assert_eq!(checks[2].status, "PENDING");
        assert!(checks[2].url.is_none());
    }

    #[test]
    fn parse_pr_checks_empty_array() {
        assert!(parse_pr_checks("[]").unwrap().is_empty());
    }

    #[test]
    fn ci_checks_parsed_from_gh_json() {
        // Simulate `gh pr checks --json name,state,bucket` output
        let json = r#"[
            {"name": "test", "state": "SUCCESS", "bucket": "pass"},
            {"name": "lint", "state": "FAILURE", "bucket": "fail"},
            {"name": "build", "state": "PENDING"}
        ]"#;
        let arr: Vec<serde_json::Value> = serde_json::from_str(json).unwrap_or_default();
        let checks: Vec<CiCheck> = arr
            .iter()
            .filter_map(|c| {
                let name = c["name"].as_str()?;
                let state = c["state"].as_str().unwrap_or("unknown");
                let bucket = c["bucket"].as_str().map(|s| s.to_string());
                Some(CiCheck {
                    name: name.to_string(),
                    status: state.to_string(),
                    conclusion: bucket,
                })
            })
            .collect();

        assert_eq!(checks.len(), 3);
        assert_eq!(checks[0].name, "test");
        assert_eq!(checks[0].conclusion, Some("pass".to_string()));
        assert_eq!(checks[1].conclusion, Some("fail".to_string()));
        assert!(checks[2].conclusion.is_none()); // pending — no bucket yet
    }
}
