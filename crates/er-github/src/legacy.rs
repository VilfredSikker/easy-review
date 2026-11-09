use anyhow::{Context, Result};
use er_git::proc::CommandTimeoutExt;
use er_git::{Git, GitError};
use serde::Deserialize;

use crate::{default_handle, CliGitHub, GhError};
use std::collections::HashMap;
use std::process::Command;
use std::sync::Arc;

/// Parsed reference to a GitHub PR
#[derive(Debug, Clone)]
pub struct PrRef {
    pub owner: String,
    pub repo: String,
    pub number: u64,
}

/// Fetched PR overview data for the PrOverview panel
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CiCheck {
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReviewerStatus {
    pub login: String,
    pub state: String,
}

/// Parse `gh pr view --json commits` output into commit metadata.
///
/// GitHub returns PR commits in chronological order. The desktop scroller
/// matches GitHub's PR Commits tab by showing newest first.
pub fn parse_pr_commits_view_json(bytes: &[u8], limit: usize) -> Result<Vec<er_git::CommitInfo>> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).context("Failed to parse PR commits JSON")?;
    Ok(parse_pr_commits_view_value(&value, limit))
}

pub fn parse_pr_commits_view_value(
    value: &serde_json::Value,
    limit: usize,
) -> Vec<er_git::CommitInfo> {
    let Some(commits) = value.get("commits").and_then(|v| v.as_array()) else {
        return Vec::new();
    };

    commits
        .iter()
        .rev()
        .take(limit)
        .filter_map(|commit| {
            let oid = commit.get("oid").and_then(|v| v.as_str())?;
            let author = commit
                .get("authors")
                .and_then(|v| v.as_array())
                .map(|authors| {
                    authors
                        .iter()
                        .filter_map(|author| {
                            author
                                .get("login")
                                .and_then(|v| v.as_str())
                                .or_else(|| author.get("name").and_then(|v| v.as_str()))
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            let date = commit
                .get("committedDate")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            // `gh pr view --json commits` has no diffstat; only the GraphQL
            // listing (`gh_pr_commits_remote`) carries these fields.
            let count = |key: &str| {
                commit
                    .get(key)
                    .and_then(|v| v.as_u64())
                    .and_then(|n| usize::try_from(n).ok())
            };
            let (adds, dels) = (count("additions"), count("deletions"));
            Some(er_git::CommitInfo {
                hash: oid.to_string(),
                short_hash: oid.chars().take(7).collect(),
                subject: commit
                    .get("messageHeadline")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                author,
                date: date.clone(),
                relative_date: date,
                file_count: count("changedFiles").unwrap_or(0),
                adds: adds.unwrap_or(0),
                dels: dels.unwrap_or(0),
                is_merge: false,
                diffstat_known: adds.is_some() && dels.is_some(),
            })
        })
        .collect()
}

/// GraphQL query behind [`gh_pr_commits_remote`]. `gh pr view --json commits`
/// cannot return a commit's diffstat, and a remote tab has no object DB to
/// read one from, so the remote listing asks GraphQL directly — still one `gh`
/// process per fetch, paginated by `gh api --paginate`.
const PR_COMMITS_GRAPHQL: &str =
    "query($owner: String!, $repo: String!, $number: Int!, $endCursor: String) {
  repository(owner: $owner, name: $repo) {
    pullRequest(number: $number) {
      commits(first: 100, after: $endCursor) {
        pageInfo { hasNextPage endCursor }
        nodes {
          commit {
            oid
            messageHeadline
            committedDate
            additions
            deletions
            changedFilesIfAvailable
            authors(first: 10) { nodes { name user { login } } }
          }
        }
      }
    }
  }
}";

/// Parse `gh api graphql --paginate` output for [`PR_COMMITS_GRAPHQL`] — one
/// JSON document per page, concatenated — into the newest-first list
/// [`parse_pr_commits_view_value`] produces, each commit with its diffstat.
pub fn parse_pr_commits_graphql(bytes: &[u8], limit: usize) -> Result<Vec<er_git::CommitInfo>> {
    let mut commits = Vec::new();
    for page in serde_json::Deserializer::from_slice(bytes).into_iter::<serde_json::Value>() {
        let page = page.context("Failed to parse PR commits GraphQL JSON")?;
        let nodes = page
            .pointer("/data/repository/pullRequest/commits/nodes")
            .and_then(|v| v.as_array())
            .map(Vec::as_slice)
            .unwrap_or_default();
        for c in nodes.iter().filter_map(|node| node.get("commit")) {
            let authors: Vec<serde_json::Value> = c
                .pointer("/authors/nodes")
                .and_then(|v| v.as_array())
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .map(|a| serde_json::json!({ "login": a.pointer("/user/login"), "name": a.get("name") }))
                .collect();
            commits.push(serde_json::json!({
                "oid": c.get("oid"),
                "messageHeadline": c.get("messageHeadline"),
                "committedDate": c.get("committedDate"),
                "authors": authors,
                "additions": c.get("additions"),
                "deletions": c.get("deletions"),
                "changedFiles": c.get("changedFilesIfAvailable"),
            }));
        }
    }
    Ok(parse_pr_commits_view_value(
        &serde_json::json!({ "commits": commits }),
        limit,
    ))
}

/// Fetch the commits on a local-clone PR using the GitHub API via `gh`.
pub fn gh_pr_commits(
    repo_root: &str,
    number: u64,
    limit: usize,
) -> Result<Vec<er_git::CommitInfo>> {
    let mut cmd = Command::new("gh");
    cmd.args(["pr", "view", &number.to_string(), "--json", "commits"])
        .current_dir(repo_root);
    let output = er_git::proc::run_with_timeout(&mut cmd, er_git::proc::GH_TIMEOUT)
        .context("Failed to run gh pr view for commits")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to get PR #{number} commits: {}", stderr.trim());
    }
    parse_pr_commits_view_json(&output.stdout, limit)
}

/// A PR's base branch and commit list, from one `gh pr view`.
///
/// [`gh_pr_base_branch`] and [`gh_pr_commits`] are two `gh pr view --json`
/// invocations against the same PR, so asking for both costs two network round
/// trips (~0.6s each) to read two fields of one object. Callers that need both
/// — the desktop Sync path — should use this instead.
pub fn gh_pr_base_branch_and_commits(
    repo_root: &str,
    number: u64,
    limit: usize,
) -> Result<(String, Vec<er_git::CommitInfo>)> {
    let mut cmd = Command::new("gh");
    cmd.args([
        "pr",
        "view",
        &number.to_string(),
        "--json",
        "baseRefName,commits",
    ])
    .current_dir(repo_root);
    let output = er_git::proc::run_with_timeout(&mut cmd, er_git::proc::GH_TIMEOUT)
        .context("Failed to run gh pr view for base branch and commits")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to get PR #{}: {}", number, stderr.trim());
    }

    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("Failed to parse gh pr view JSON")?;
    let base = value
        .get("baseRefName")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .trim()
        .to_string();
    if base.is_empty() {
        anyhow::bail!("PR #{} has no base branch", number);
    }
    let commits = parse_pr_commits_view_value(&value, limit);
    Ok((base, commits))
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
pub struct GitHubComment {
    pub id: u64,
    pub body: String,
    pub path: Option<String>,
    pub line: Option<usize>,
    #[serde(default)]
    pub start_line: Option<usize>,
    pub original_line: Option<usize>,
    #[serde(default)]
    pub original_start_line: Option<usize>,
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

/// Response from creating a comment.
#[derive(Debug, Deserialize)]
struct CreateCommentResponse {
    id: u64,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    user: Option<GitHubUser>,
}

impl CreateCommentResponse {
    fn into_created(self) -> CreatedComment {
        CreatedComment {
            id: self.id,
            created_at: self.created_at,
            author: self.user.map(|user| user.login).unwrap_or_default(),
        }
    }
}

/// The parts of a created comment the caller records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedComment {
    pub id: u64,
    pub created_at: String,
    pub author: String,
}

/// An issue comment (a PR-wide comment from the issues API).
#[derive(Debug, Clone, Deserialize)]
pub struct IssueComment {
    pub id: u64,
    pub body: String,
    pub user: GitHubUser,
    pub created_at: String,
}

/// Parse a GitHub PR URL into its components.
///
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

pub(crate) fn gh_spawn_context(err: std::io::Error) -> anyhow::Error {
    if err.kind() == std::io::ErrorKind::NotFound {
        anyhow::anyhow!(
            "GitHub CLI (`gh`) not found on PATH. Install it (https://cli.github.com) \
             and ensure it is on PATH for GUI apps (e.g. /opt/homebrew/bin)."
        )
    } else {
        anyhow::Error::new(err).context("Failed to run gh")
    }
}

/// Check if `gh` CLI is installed and authenticated
pub fn ensure_gh_installed() -> Result<()> {
    let output = Command::new("gh")
        .args(["--version"])
        .output_timed(er_git::proc::GH_TIMEOUT)
        .context("GitHub CLI (gh) is not installed. Install it: https://cli.github.com")?;

    if !output.status.success() {
        anyhow::bail!("GitHub CLI (gh) is not working properly");
    }

    // Check auth
    let auth = Command::new("gh")
        .args(["auth", "status"])
        .output_timed(er_git::proc::GH_TIMEOUT)
        .context("Failed to check gh auth status")?;

    if !auth.status.success() {
        anyhow::bail!("GitHub CLI is not authenticated. Run: gh auth login");
    }

    Ok(())
}

/// A GitHub notification thread from `GET /notifications`.
#[derive(Debug, Clone, Deserialize)]
pub struct GhNotification {
    pub id: String,
    pub reason: String,
    #[serde(default)]
    pub unread: bool,
    #[serde(default)]
    pub updated_at: String,
    pub subject: GhNotificationSubject,
    pub repository: GhNotificationRepo,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GhNotificationSubject {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub url: String,
    #[serde(default, rename = "type")]
    pub subject_type: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GhNotificationRepo {
    #[serde(default)]
    pub full_name: String,
}

/// Participating unread notifications (not the full watched-repo firehose).
pub fn gh_list_participating_notifications() -> Result<Vec<GhNotification>> {
    let output = Command::new("gh")
        .args(["api", "notifications?participating=true&per_page=50"])
        .output_timed(er_git::proc::GH_TIMEOUT)
        .context("Failed to fetch GitHub notifications")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to fetch GitHub notifications: {}", stderr.trim());
    }

    Ok(serde_json::from_slice(&output.stdout)?)
}

/// Pull request number from a notification subject URL.
///
/// Handles both `/pulls/N` and `/issues/N` (PR comments use the issues path).
pub fn pr_number_from_notification_url(url: &str) -> Option<u64> {
    let path = url.split('?').next().unwrap_or(url);
    let mut parts = path.rsplit('/');
    let last = parts.next()?;
    let kind = parts.next()?;
    if kind != "pulls" && kind != "issues" && kind != "pull" {
        return None;
    }
    last.parse().ok()
}

/// Get the base branch for a PR using `gh pr view`
pub fn gh_pr_base_branch(pr_number: u64, repo_root: &str) -> Result<String> {
    let mut cmd = Command::new("gh");
    cmd.args([
        "pr",
        "view",
        &pr_number.to_string(),
        "--json",
        "baseRefName",
        "--jq",
        ".baseRefName",
    ])
    .current_dir(repo_root);
    let output = er_git::proc::run_with_timeout(&mut cmd, er_git::proc::GH_TIMEOUT)
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
        .output_timed(er_git::proc::GH_TIMEOUT)
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

/// [`fetch_base_branch_ref`] for a PR whose base branch may be gone from
/// origin. Falls back to the PR's `baseRefOid`, which GitHub keeps after the
/// branch is deleted. The extra `gh` call happens only on that failure.
pub fn fetch_pr_base_ref(
    git: &dyn Git,
    repo_root: &str,
    pr_number: u64,
    base_branch: &str,
) -> Result<String> {
    fetch_pr_base_ref_with(git, repo_root, pr_number, base_branch, || {
        gh_pr_base_oid(pr_number, repo_root)
    })
}

fn fetch_pr_base_ref_with(
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

fn gh_pr_base_oid(pr_number: u64, repo_root: &str) -> Result<String> {
    let mut cmd = Command::new("gh");
    cmd.args([
        "pr",
        "view",
        &pr_number.to_string(),
        "--json",
        "baseRefOid",
        "--jq",
        ".baseRefOid",
    ])
    .current_dir(repo_root);
    let output = er_git::proc::run_with_timeout(&mut cmd, er_git::proc::GH_TIMEOUT)
        .context("Failed to get PR base commit")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "Failed to get PR #{pr_number} base commit: {}",
            stderr.trim()
        );
    }
    let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
    // Hex-only, so the value can never parse as a git option below.
    if sha.is_empty() || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        anyhow::bail!("PR #{pr_number} has no usable base commit: '{sha}'");
    }
    Ok(sha)
}

/// Point `refs/er/pr/<n>/base` at `sha`, fetching it from origin only when the
/// commit is not already local (it usually is: the base was fetched before it
/// was deleted). Returns the ref name.
fn pin_pr_base_commit(git: &dyn Git, repo_root: &str, pr_number: u64, sha: &str) -> Result<String> {
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
        .output_timed(er_git::proc::GH_TIMEOUT)
        .context("failed to run gh pr view")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh pr view failed: {}", stderr.trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
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

/// Check if the current branch has an open PR. Returns (number, base_branch) or None.
/// Silently returns None if gh is unavailable, not authenticated, or no PR exists.
/// Three-way result of probing the checked-out branch's PR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchPrProbe {
    Present {
        number: u64,
        base: String,
    },
    /// gh answered that this branch has no pull request.
    Absent,
    /// The probe itself failed (offline, unauthenticated, timeout, …).
    Unknown,
}

/// Whether gh's failure stderr means "this branch has no PR". Only that
/// explicit answer counts: an offline or unauthenticated gh is unknown.
fn gh_failure_is_absent(stderr: &str) -> bool {
    stderr.to_lowercase().contains("no pull requests found")
}

pub fn probe_pr_for_current_branch(repo_root: &str) -> BranchPrProbe {
    // Use --jq to extract "number<tab>baseRefName" — robust against JSON formatting
    let output = match Command::new("gh")
        .args([
            "pr",
            "view",
            "--json",
            "number,baseRefName",
            "--jq",
            r#"[.number, .baseRefName] | @tsv"#,
        ])
        .current_dir(repo_root)
        .output_timed(er_git::proc::GH_TIMEOUT)
    {
        Ok(output) => output,
        Err(_) => return BranchPrProbe::Unknown,
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return if gh_failure_is_absent(&stderr) {
            BranchPrProbe::Absent
        } else {
            BranchPrProbe::Unknown
        };
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let text = text.trim();
    let Some((num_str, base)) = text.split_once('\t') else {
        return BranchPrProbe::Unknown;
    };
    let Ok(number) = num_str.parse::<u64>() else {
        return BranchPrProbe::Unknown;
    };
    if base.is_empty() {
        return BranchPrProbe::Unknown;
    }
    BranchPrProbe::Present {
        number,
        base: base.to_string(),
    }
}

/// The PR for the checked-out branch, if one can be read.
///
/// Prefer [`probe_pr_for_current_branch`] when "no PR" and "could not ask"
/// must be told apart.
pub fn gh_pr_for_current_branch(repo_root: &str) -> Option<(u64, String)> {
    match probe_pr_for_current_branch(repo_root) {
        BranchPrProbe::Present { number, base } => Some((number, base)),
        BranchPrProbe::Absent | BranchPrProbe::Unknown => None,
    }
}

/// Three-way result of a PR lookup for the checked-out branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrLookup {
    Present {
        owner: String,
        repo: String,
        number: u64,
    },
    /// gh answered that this branch has no pull request.
    Absent,
    /// The lookup itself failed (offline, unauthenticated, timeout, …).
    Unknown(String),
}

/// Classify a failed `gh pr view` by its stderr. Only gh's explicit answer
/// counts as absent; everything else is unknown.
pub fn classify_pr_lookup_failure(stderr: &str) -> PrLookup {
    if gh_failure_is_absent(stderr) {
        return PrLookup::Absent;
    }
    let detail = stderr.trim();
    PrLookup::Unknown(if detail.is_empty() {
        "gh pr view failed".to_string()
    } else {
        detail.to_string()
    })
}

/// Resolve the checked-out branch's PR without collapsing "no PR" and
/// "could not ask" into one answer.
pub fn lookup_pr_for_current_branch(git: &dyn Git, repo_root: &str) -> PrLookup {
    let output = match Command::new("gh")
        .args(["pr", "view", "--json", "number,headRepository"])
        .current_dir(repo_root)
        .output_timed(er_git::proc::GH_TIMEOUT)
    {
        Ok(output) => output,
        Err(e) => return PrLookup::Unknown(format!("{e:#}")),
    };

    if !output.status.success() {
        return classify_pr_lookup_failure(&String::from_utf8_lossy(&output.stderr));
    }

    let v: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(v) => v,
        Err(e) => return PrLookup::Unknown(format!("Failed to parse PR info JSON: {e}")),
    };
    let Some(number) = v["number"].as_u64() else {
        return PrLookup::Unknown("Missing PR number".to_string());
    };
    let owner = v["headRepository"]["owner"]["login"].as_str().unwrap_or("");
    let repo_name = v["headRepository"]["name"].as_str().unwrap_or("");
    if !owner.is_empty() && !repo_name.is_empty() {
        return PrLookup::Present {
            owner: owner.to_string(),
            repo: repo_name.to_string(),
            number,
        };
    }
    match parse_origin_remote(git, repo_root) {
        Some((owner, repo)) => PrLookup::Present {
            owner,
            repo,
            number,
        },
        None => PrLookup::Unknown("Could not read the repository's origin remote".to_string()),
    }
}

/// Open PR number whose head branch matches `head` on `owner/repo` (first non-draft).
pub fn gh_open_pr_number_for_head(owner: &str, repo: &str, head: &str) -> Result<Option<u64>> {
    let output = Command::new("gh")
        .args([
            "pr",
            "list",
            "--repo",
            &format!("{owner}/{repo}"),
            "--head",
            head,
            "--state",
            "open",
            "--limit",
            "10",
            "--json",
            "number,isDraft",
        ])
        .output_timed(er_git::proc::GH_TIMEOUT)
        .map_err(gh_spawn_context)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh pr list --head failed: {}", stderr.trim());
    }

    let rows: Vec<serde_json::Value> =
        serde_json::from_slice(&output.stdout).context("invalid gh pr list JSON")?;
    let number = rows
        .iter()
        .find(|row| row.get("isDraft").and_then(|v| v.as_bool()) != Some(true))
        .and_then(|row| row.get("number").and_then(|v| v.as_u64()));
    Ok(number)
}

/// PR title via `gh pr view --repo` (returns `None` when `gh` fails).
pub fn gh_pr_title(owner: &str, repo: &str, number: u64) -> Option<String> {
    let output = Command::new("gh")
        .args([
            "pr",
            "view",
            &number.to_string(),
            "--repo",
            &format!("{owner}/{repo}"),
            "--json",
            "title",
        ])
        .output_timed(er_git::proc::GH_TIMEOUT)
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    value
        .get("title")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Check if a string looks like a GitHub PR URL
pub fn is_github_pr_url(s: &str) -> bool {
    parse_github_pr_url(s).is_some()
}

/// Get PR info (owner, repo, number) for the current branch
pub fn get_pr_info(git: &dyn Git, repo_root: &str) -> Result<(String, String, u64)> {
    match lookup_pr_for_current_branch(git, repo_root) {
        PrLookup::Present {
            owner,
            repo,
            number,
        } => Ok((owner, repo, number)),
        PrLookup::Absent => anyhow::bail!("No PR associated with current branch"),
        PrLookup::Unknown(e) => anyhow::bail!("Failed to get PR info: {e}"),
    }
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

/// Parse `gh api --paginate` output: one array, or several concatenated arrays.
///
/// Any parse error propagates: a chunk that fails must not be skipped, or a
/// partial list would look complete to a caller reconciling a retry.
fn parse_paginated<T: serde::de::DeserializeOwned>(stdout: &str) -> Result<Vec<T>> {
    let mut out = Vec::new();
    for batch in serde_json::Deserializer::from_str(stdout).into_iter::<Vec<T>>() {
        out.extend(batch.context("Failed to parse paginated gh api output")?);
    }
    Ok(out)
}

impl CliGitHub {
    /// Fetch all review comments for a PR.
    pub(crate) fn pr_comments(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        repo_root: &str,
    ) -> Result<Vec<GitHubComment>, GhError> {
        let path = format!("repos/{owner}/{repo}/pulls/{pr}/comments");
        let output = self.run_gh_args(&["api", &path, "--paginate"], Some(repo_root))?;
        let stdout = String::from_utf8_lossy(&output.body);
        parse_paginated(&stdout).map_err(GhError::from)
    }
}

/// Build `gh api -f/-F` args for a line or multi-line review comment.
fn review_comment_field_args(
    path: &str,
    line_start: usize,
    line_end: usize,
    side: &str,
    body: &str,
    commit_id: &str,
) -> Vec<String> {
    let end = line_end.max(line_start);
    let mut args = vec![
        format!("body={}", body),
        format!("path={}", path),
        format!("commit_id={}", commit_id),
        format!("side={}", side),
        format!("line={}", end),
    ];
    if end > line_start {
        args.push(format!("start_line={}", line_start));
        if side == "LEFT" {
            args.push("start_side=LEFT".to_string());
        }
    }
    args
}

/// A new line-anchored review comment to push. `line_end` is inclusive; omit it
/// or set it equal to `line_start` for a single-line comment.
#[derive(Debug, Clone, Copy)]
pub struct NewLineComment<'a> {
    pub path: &'a str,
    pub line_start: usize,
    pub line_end: Option<usize>,
    pub body: &'a str,
    pub side: &'a str,
}

impl CliGitHub {
    /// Head oid of a PR: from a local checkout when one is given, remote
    /// otherwise. Review comments need the latest commit SHA.
    pub(crate) fn pr_head_oid(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        repo_root: Option<&str>,
    ) -> Result<String, GhError> {
        let Some(repo_root) = repo_root else {
            return self.pr_head_oid_remote(owner, repo, pr);
        };
        let out = self.run_gh_args(
            &[
                "pr",
                "view",
                &pr.to_string(),
                "--json",
                "headRefOid",
                "--jq",
                ".headRefOid",
            ],
            Some(repo_root),
        )?;
        let commit_id = String::from_utf8_lossy(&out.body).trim().to_string();
        if commit_id.is_empty() {
            return Err(GhError::Local {
                message: "Failed to get HEAD SHA from gh pr view: empty output".to_string(),
            });
        }
        Ok(commit_id)
    }

    /// Post a line-anchored review comment at the given head commit.
    pub(crate) fn post_line_comment(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        comment: NewLineComment<'_>,
        commit_id: &str,
        repo_root: Option<&str>,
    ) -> Result<CreatedComment, GhError> {
        let NewLineComment {
            path,
            line_start,
            line_end,
            body,
            side,
        } = comment;
        let end = line_end.unwrap_or(line_start);
        let field_args = review_comment_field_args(path, line_start, end, side, body, commit_id);
        let mut args = vec![
            "api".to_string(),
            "-X".to_string(),
            "POST".to_string(),
            format!("repos/{owner}/{repo}/pulls/{pr}/comments"),
        ];
        for arg in &field_args {
            let flag = if arg.starts_with("line=") || arg.starts_with("start_line=") {
                "-F"
            } else {
                "-f"
            };
            args.push(flag.to_string());
            args.push(arg.clone());
        }
        let output = self.run_gh(&args, repo_root, None)?;
        let stdout = String::from_utf8_lossy(&output.body);
        let resp: CreateCommentResponse =
            serde_json::from_str(&stdout).map_err(|error| GhError::Local {
                message: format!("Failed to parse create comment response: {error}"),
            })?;
        Ok(resp.into_created())
    }
}

impl CliGitHub {
    /// Post a reply to an existing review comment.
    pub(crate) fn post_reply(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        in_reply_to: u64,
        body: &str,
        repo_root: Option<&str>,
    ) -> Result<CreatedComment, GhError> {
        let path = format!("repos/{owner}/{repo}/pulls/{pr}/comments");
        let in_reply_to = in_reply_to.to_string();
        let body = format!("body={body}");
        let reply = format!("in_reply_to={in_reply_to}");
        let output = self.run_gh_args(
            &["api", "-X", "POST", &path, "-f", &body, "-F", &reply],
            repo_root,
        )?;
        let stdout = String::from_utf8_lossy(&output.body);
        let resp: CreateCommentResponse =
            serde_json::from_str(&stdout).map_err(|error| GhError::Local {
                message: format!("Failed to parse reply response: {error}"),
            })?;
        Ok(resp.into_created())
    }

    /// Post a PR-wide comment (the issues API).
    pub(crate) fn post_general(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        body: &str,
        repo_root: Option<&str>,
    ) -> Result<CreatedComment, GhError> {
        let path = format!("repos/{owner}/{repo}/issues/{pr}/comments");
        let body = format!("body={body}");
        let output = self.run_gh_args(&["api", "-X", "POST", &path, "-f", &body], repo_root)?;
        let stdout = String::from_utf8_lossy(&output.body);
        let resp: CreateCommentResponse =
            serde_json::from_str(&stdout).map_err(|error| GhError::Local {
                message: format!("Failed to parse general comment response: {error}"),
            })?;
        Ok(resp.into_created())
    }

    /// Fetch a PR's issue comments (PR-wide comments, not review comments).
    pub(crate) fn issue_comments(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        repo_root: Option<&str>,
    ) -> Result<Vec<IssueComment>, GhError> {
        let path = format!("repos/{owner}/{repo}/issues/{pr}/comments");
        let output = self.run_gh_args(&["api", &path, "--paginate"], repo_root)?;
        parse_paginated(&String::from_utf8_lossy(&output.body)).map_err(GhError::from)
    }

    /// Login of the authenticated `gh` user.
    pub(crate) fn viewer_login(&self, repo_root: Option<&str>) -> Result<String, GhError> {
        let output = self.run_gh_args(&["api", "user", "--jq", ".login"], repo_root)?;
        let login = String::from_utf8_lossy(&output.body).trim().to_string();
        if login.is_empty() {
            return Err(GhError::Local {
                message: "Failed to read the gh user: empty login".to_string(),
            });
        }
        Ok(login)
    }
}

impl CliGitHub {
    /// Push a new review comment to a PR.
    pub(crate) fn push_comment(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        comment: NewLineComment<'_>,
        repo_root: &str,
    ) -> Result<u64, GhError> {
        let commit_id = self.pr_head_oid(owner, repo, pr, Some(repo_root))?;
        Ok(self
            .post_line_comment(owner, repo, pr, comment, &commit_id, Some(repo_root))?
            .id)
    }

    /// Push a reply to an existing review comment.
    pub(crate) fn reply_comment(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        in_reply_to: u64,
        body: &str,
        repo_root: &str,
    ) -> Result<u64, GhError> {
        Ok(self
            .post_reply(owner, repo, pr, in_reply_to, body, Some(repo_root))?
            .id)
    }

    /// Update a review comment body on a PR.
    pub(crate) fn update_review_comment(
        &self,
        owner: &str,
        repo: &str,
        comment_id: u64,
        body: &str,
        repo_root: &str,
    ) -> Result<(), GhError> {
        let path = format!("repos/{owner}/{repo}/pulls/comments/{comment_id}");
        let body = format!("body={body}");
        self.run_gh_args(&["api", "-X", "PATCH", &path, "-f", &body], Some(repo_root))
            .map(|_| ())
    }

    /// Delete a review comment from a PR. A 404 means it is already gone.
    pub(crate) fn delete_comment(
        &self,
        owner: &str,
        repo: &str,
        comment_id: u64,
        repo_root: &str,
    ) -> Result<(), GhError> {
        let path = format!("repos/{owner}/{repo}/pulls/comments/{comment_id}");
        match self.run_gh_args(&["api", "-X", "DELETE", &path], Some(repo_root)) {
            Ok(_) => Ok(()),
            Err(GhError::NotFound { .. }) => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Update the PR body with the given content via `gh pr edit --body`.
    pub(crate) fn edit_body(&self, repo_root: &str, body: &str) -> Result<(), GhError> {
        self.run_gh_args(&["pr", "edit", "--body", body], Some(repo_root))
            .map(|_| ())
    }

    /// Approve a PR via `gh pr review --approve`. Uses `--repo` and PR number
    /// when reviewing a remote PR.
    pub(crate) fn approve(
        &self,
        repo_root: &str,
        remote_repo: Option<&str>,
        pr_number: Option<u64>,
    ) -> Result<(), GhError> {
        let mut args = vec![
            "pr".to_string(),
            "review".to_string(),
            "--approve".to_string(),
        ];
        if let (Some(slug), Some(n)) = (remote_repo, pr_number) {
            args.push("--repo".to_string());
            args.push(slug.to_string());
            args.push(n.to_string());
        }
        self.run_gh(&args, Some(repo_root), None).map(|_| ())
    }
}

impl CliGitHub {
    /// Fetch PR overview data: title, body, state, author, branches, reviewers.
    /// If `pr_number` is Some, fetches that specific PR; otherwise auto-detects
    /// from the current branch.
    pub(crate) fn overview(
        &self,
        repo_root: &str,
        pr_number: Option<u64>,
    ) -> Result<Option<PrOverviewData>, GhError> {
        Ok(self.overview_impl(repo_root, pr_number, true))
    }

    /// Like [`Self::overview`], but skips the separate `gh pr checks`
    /// subprocess (`checks` is always empty in the result). Use this when the
    /// caller already has a fresher CI-checks source for the same PR — e.g. the
    /// desktop's 30s gh-status loop (`fetch_github_status` / `gh_status_cache`)
    /// — and only needs title/state/author/branch/reviewer metadata, to avoid a
    /// duplicate `gh pr checks` call on every tick.
    pub(crate) fn overview_no_checks(
        &self,
        repo_root: &str,
        pr_number: Option<u64>,
    ) -> Result<Option<PrOverviewData>, GhError> {
        Ok(self.overview_impl(repo_root, pr_number, false))
    }

    fn overview_impl(
        &self,
        repo_root: &str,
        pr_number: Option<u64>,
        include_checks: bool,
    ) -> Option<PrOverviewData> {
        let mut args = vec!["pr".to_string(), "view".to_string()];
        if let Some(n) = pr_number {
            args.push(n.to_string());
        }
        args.extend([
            "--json".to_string(),
            "number,title,body,state,author,url,baseRefName,headRefName,reviews".to_string(),
        ]);
        let view_output = self.run_gh(&args, Some(repo_root), None).ok()?;
        let json_str = String::from_utf8_lossy(&view_output.body);
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

        // Fetch CI checks (separate call — may fail if no checks configured).
        let checks = if include_checks {
            self.checks_data(repo_root).unwrap_or_default()
        } else {
            Vec::new()
        };

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

    /// Fetch CI check runs for the current PR; a failure reads as no checks.
    fn checks_data(&self, repo_root: &str) -> Result<Vec<CiCheck>, GhError> {
        let Ok(output) = self.run_gh_args(
            &["pr", "checks", "--json", "name,state,bucket"],
            Some(repo_root),
        ) else {
            return Ok(Vec::new());
        };
        let json_str = String::from_utf8_lossy(&output.body);
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
}

/// Test whether a remote URL belongs to the given owner/repo.
/// HTTPS URLs contain "/owner/repo" and SSH URLs contain ":owner/repo".
fn remote_matches_repo(remote: &str, owner: &str, repo: &str) -> bool {
    let expected = format!("{}/{}", owner, repo);
    remote.contains(&format!("/{}", expected)) || remote.contains(&format!(":{}", expected))
}

/// Remote-only PRs above these limits are rejected up front with a clear message.
/// The IPC line budget (SNAPSHOT_DIFF_LINE_BUDGET) enforces what reaches the UI;
/// these gates only guard against pathological network / memory usage.
const REMOTE_PR_MAX_CHANGED_FILES: u64 = 10_000;
const REMOTE_PR_MAX_LINE_CHANGES: u64 = 5_000_000;

/// Quick size check via `gh pr view` so we do not clone multi‑GB repos for hopeless PRs.
pub fn gh_pr_size_check_remote(owner: &str, repo: &str, number: u64) -> Result<()> {
    let repo_slug = format!("{}/{}", owner, repo);
    let output = Command::new("gh")
        .args([
            "pr",
            "view",
            &number.to_string(),
            "--repo",
            &repo_slug,
            "--json",
            "additions,deletions,changedFiles",
            "--jq",
            r#"{additions: .additions, deletions: .deletions, files: .changedFiles}"#,
        ])
        .output_timed(er_git::proc::GH_TIMEOUT)
        .context("Failed to check PR size")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to check PR #{} size: {}", number, stderr.trim());
    }

    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&output.stdout)).context("PR size JSON")?;
    let additions = v["additions"].as_u64().unwrap_or(0);
    let deletions = v["deletions"].as_u64().unwrap_or(0);
    let files = v["files"].as_u64().unwrap_or(0);
    let lines = additions.saturating_add(deletions);

    if files > REMOTE_PR_MAX_CHANGED_FILES || lines > REMOTE_PR_MAX_LINE_CHANGES {
        anyhow::bail!(
            "PR #{} is too large to review here ({} files, +{}/-{} lines). \
             Open a smaller PR, or clone {}/{} locally.",
            number,
            files,
            additions,
            deletions,
            owner,
            repo
        );
    }
    Ok(())
}

/// Get raw unified diff for a PR via `gh pr diff N --repo owner/repo`.
///
/// Works without a local clone — uses GitHub API via gh CLI.
/// Falls back to shallow clone + local git diff when the PR exceeds GitHub's
/// API line limit (HTTP 406 / diff_too_large).
pub fn gh_pr_diff_remote(git: &dyn Git, owner: &str, repo: &str, number: u64) -> Result<String> {
    gh_pr_size_check_remote(owner, repo, number)?;
    let repo_slug = format!("{}/{}", owner, repo);
    let output = Command::new("gh")
        .args(["pr", "diff", &number.to_string(), "--repo", &repo_slug])
        .output_timed(er_git::proc::GH_TIMEOUT)
        .map_err(gh_spawn_context)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if gh_diff_too_large(&stderr) {
            return gh_pr_diff_via_clone(git, owner, repo, number);
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
pub fn gh_pr_diff(git: &dyn Git, pr_number: u64, repo_root: &str) -> Result<String> {
    let output = Command::new("gh")
        .args(["pr", "diff", &pr_number.to_string()])
        .current_dir(repo_root)
        .output_timed(er_git::proc::GH_TIMEOUT)
        .map_err(gh_spawn_context)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if gh_diff_too_large(&stderr) {
            return gh_pr_diff_too_large_fallback(git, pr_number, repo_root);
        }
        anyhow::bail!("Failed to get PR diff: {}", stderr.trim());
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Whether `gh pr diff` stderr says GitHub refused to render the diff.
///
/// GitHub caps the diff endpoint by file count (300) and by size, and the cap
/// has nothing to do with how many lines changed: a few thousand lines spread
/// over more than 300 files is refused, as is a small change to one huge file.
fn gh_diff_too_large(stderr: &str) -> bool {
    stderr.contains("HTTP 406")
        || stderr.contains("too_large")
        || stderr.contains("exceeded the maximum number")
}

/// A PR the diff endpoint refused: build the diff from the local checkout, and
/// only if that cannot work (no usable `origin`, a shallow clone with no merge
/// base) clone the repo into a temp dir.
fn gh_pr_diff_too_large_fallback(git: &dyn Git, pr_number: u64, repo_root: &str) -> Result<String> {
    let local = gh_pr_diff_via_local_git(git, pr_number, repo_root);
    let Err(local_err) = local else {
        return local;
    };
    let Some((owner, repo)) = parse_origin_remote(git, repo_root) else {
        return Err(local_err.context("GitHub refused to render this large PR diff"));
    };
    gh_pr_diff_via_clone(git, &owner, &repo, pr_number).map_err(|clone_err| {
        clone_err.context(format!(
            "GitHub refused to render this large PR diff; local git diff also failed: {local_err:#}"
        ))
    })
}

fn gh_pr_diff_via_local_git(git: &dyn Git, pr_number: u64, repo_root: &str) -> Result<String> {
    let output = Command::new("gh")
        .args([
            "pr",
            "view",
            &pr_number.to_string(),
            "--json",
            "baseRefOid,headRefOid",
            "--jq",
            r#"[.baseRefOid, .headRefOid] | @tsv"#,
        ])
        .current_dir(repo_root)
        .output_timed(er_git::proc::GH_TIMEOUT)
        .map_err(gh_spawn_context)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "Failed to get PR #{pr_number} commit SHAs: {}",
            stderr.trim()
        );
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let (base_sha, head_sha) = text
        .split_once('\t')
        .filter(|(base, head)| !base.is_empty() && !head.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Unexpected gh pr view output: {text}"))?;

    // By SHA rather than by branch name: it works for forks and for a head
    // branch that has since been deleted. No refs are created.
    match git.fetch(repo_root, &["--no-tags", "origin", base_sha, head_sha]) {
        Ok(()) => {}
        Err(GitError::Failed { stderr, .. }) => {
            anyhow::bail!("Failed to fetch PR commits: {}", stderr.trim());
        }
        Err(e) => return Err(anyhow::Error::from(e).context("Failed to fetch PR commits")),
    }
    git_merge_base_diff(git, repo_root, base_sha, head_sha)
}

/// `git diff base...head`: against the merge base, which is what GitHub's
/// Files changed view shows. A two-dot diff against the base tip would also
/// show the base branch's own newer commits, reversed.
fn git_merge_base_diff(git: &dyn Git, repo_root: &str, base: &str, head: &str) -> Result<String> {
    let range = format!("{base}...{head}");
    match git.diff_revs(repo_root, &[&range]) {
        Ok(diff) => Ok(diff),
        Err(GitError::Failed { stderr, .. }) => {
            anyhow::bail!("git diff failed for large PR: {}", stderr.trim());
        }
        Err(e) => Err(anyhow::Error::from(e).context("Failed to run git diff for large PR")),
    }
}

/// Base and head commit SHAs for a PR (from `gh pr view`). Works for merged PRs and
/// deleted head branches — unlike branch names, which may no longer exist on origin.
pub fn gh_pr_commit_shas_remote(owner: &str, repo: &str, number: u64) -> Result<(String, String)> {
    let repo_slug = format!("{}/{}", owner, repo);
    let output = Command::new("gh")
        .args([
            "pr",
            "view",
            &number.to_string(),
            "--repo",
            &repo_slug,
            "--json",
            "baseRefOid,headRefOid",
            "--jq",
            r#"[.baseRefOid, .headRefOid] | @tsv"#,
        ])
        .output_timed(er_git::proc::GH_TIMEOUT)
        .context("Failed to get PR commit SHAs")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "Failed to get PR #{} commit SHAs: {}",
            number,
            stderr.trim()
        );
    }

    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let (base_sha, head_sha) = text
        .split_once('\t')
        .ok_or_else(|| anyhow::anyhow!("Unexpected gh pr view output: {}", text))?;
    if base_sha.is_empty() || head_sha.is_empty() {
        anyhow::bail!("Missing base or head SHA for PR #{}", number);
    }
    Ok((base_sha.to_string(), head_sha.to_string()))
}

/// Fallback for large PRs: shallow-clone the repo into a temp dir, fetch both
/// commits by SHA, and produce the diff locally. The temp dir is cleaned up automatically.
fn gh_pr_diff_via_clone(git: &dyn Git, owner: &str, repo: &str, number: u64) -> Result<String> {
    let (base_sha, head_sha) = gh_pr_commit_shas_remote(owner, repo, number)?;
    let repo_url = format!("https://github.com/{}/{}.git", owner, repo);
    // tempfile::Builder::tempdir: random name + 0700, so an attacker cannot
    // pre-create the clone dir (the old fixed `er-remote-{owner}-{repo}-{n}`
    // name with remove_dir_all + clone let an attacker win the dir, then swap
    // .git/config — core.sshCommand + ssh origin → code execution on the
    // fetch below; or poison the diff). 0700 also blocks reading the clone.
    let tmp_dir = tempfile::Builder::new()
        .prefix("er-remote-")
        .tempdir()
        .context("Failed to create temp dir for large PR diff")?;
    let tmp_path = tmp_dir
        .path()
        .to_str()
        .context("Temp dir path is not valid UTF-8")?
        .to_string();

    // Shallow clone with only the two refs we need
    match git.shallow_clone(&repo_url, &tmp_path) {
        Ok(()) => {}
        Err(GitError::Failed { stderr, .. }) => {
            anyhow::bail!("Shallow clone failed: {}", stderr.trim());
        }
        Err(e) => {
            return Err(anyhow::Error::from(e).context("Failed to shallow clone for large PR diff"));
        }
    }

    // Fetch exact base/head commits (branch names fail for merged/deleted heads and some forks).
    match git.fetch(&tmp_path, &["--depth=1", "origin", &base_sha, &head_sha]) {
        Ok(()) => {}
        Err(GitError::Failed { stderr, .. }) => {
            anyhow::bail!("Failed to fetch PR commits: {}", stderr.trim());
        }
        Err(e) => return Err(anyhow::Error::from(e).context("Failed to fetch PR commits")),
    }

    // Generate the diff
    let diff = match git.diff_revs(&tmp_path, &[&base_sha, &head_sha]) {
        Ok(diff) => diff,
        Err(e) => {
            // Clean up (TempDir removes the clone on drop — including on error).
            drop(tmp_dir);
            return Err(match e {
                GitError::Failed { stderr, .. } => {
                    anyhow::anyhow!("git diff failed in cloned repo: {}", stderr.trim())
                }
                other => {
                    anyhow::Error::from(other).context("Failed to generate diff from cloned repo")
                }
            });
        }
    };

    // Clean up (TempDir removes the clone on drop — including on error).
    drop(tmp_dir);

    Ok(diff)
}

/// Raw contents of `path` at `git_ref` on GitHub, capped at `max_bytes`.
pub fn gh_file_at_ref(
    repo_slug: &str,
    path: &str,
    git_ref: &str,
    max_bytes: usize,
) -> Result<Vec<u8>> {
    fn encode(s: &str) -> String {
        s.bytes()
            .map(|b| {
                if b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'_' | b'.') {
                    (b as char).to_string()
                } else {
                    format!("%{b:02X}")
                }
            })
            .collect()
    }
    let output = er_git::proc::run_with_bounded_stdout(
        Command::new("gh").args([
            "api",
            "-H",
            "Accept: application/vnd.github.raw+json",
            &format!(
                "repos/{repo_slug}/contents/{}?ref={}",
                encode(path),
                encode(git_ref)
            ),
        ]),
        er_git::proc::GH_TIMEOUT,
        max_bytes,
    )?;
    if !output.status.success() {
        anyhow::bail!("File could not be read from GitHub");
    }
    Ok(output.stdout)
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
        .output_timed(er_git::proc::GH_TIMEOUT)
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

/// Run `gh stack view --json` (github/gh-stack extension) for the repo at
/// `repo_root` and return its stdout.
///
/// A non-zero exit is an expected steady state — the branch may not be in a
/// stack, or the extension may not be installed — so `gh`'s stderr diagnostic is
/// returned as the error message and surfaced to the user as the reason.
pub fn gh_stack_view_json(repo_root: &str) -> Result<String> {
    let output = Command::new("gh")
        .args(["stack", "view", "--json"])
        .current_dir(repo_root)
        // `gh stack view` opens an interactive TUI under a TTY; the JSON flag
        // avoids it, and these env vars keep any prompt from blocking the
        // worker thread that runs this.
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_NO_UPDATE_NOTIFIER", "1")
        .output_timed(er_git::proc::GH_TIMEOUT)
        .context("Failed to run `gh stack view`")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let reason = stderr.trim();
        return Err(anyhow::anyhow!(
            "{}",
            if reason.is_empty() {
                "gh stack view failed"
            } else {
                reason
            }
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Open PRs of `owner/repo` with just the refs needed to walk a stack chain
/// (`gh pr list --json number,url,headRefName,baseRefName`). Blocking; callers
/// run it on a worker thread.
pub fn gh_open_pr_refs_json(repo_slug: &str) -> Result<String> {
    let output = Command::new("gh")
        .args([
            "pr",
            "list",
            "--repo",
            repo_slug,
            "--state",
            "open",
            "--limit",
            "200",
            "--json",
            "number,url,headRefName,baseRefName",
        ])
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_NO_UPDATE_NOTIFIER", "1")
        .output()
        .map_err(gh_spawn_context)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh pr list failed: {}", stderr.trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

impl CliGitHub {
    /// One bounded `gh api` read whose body is returned without headers.
    pub(crate) fn api_get_bounded(&self, path: &str, max_bytes: usize) -> Result<Vec<u8>, GhError> {
        let args = vec![
            "api".to_string(),
            path.to_string(),
            "-H".to_string(),
            "Accept: application/vnd.github.raw+json".to_string(),
        ];
        let output = self.run_gh(&args, None, Some(max_bytes))?;
        Ok(output.body)
    }

    /// Fetch PR overview data for a remote repo (no local clone needed).
    pub(crate) fn overview_remote(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<Option<PrOverviewData>, GhError> {
        let repo_slug = format!("{owner}/{repo}");
        let number_arg = number.to_string();
        let Ok(view_output) = self.run_gh_args(
            &[
                "pr",
                "view",
                &number_arg,
                "--repo",
                &repo_slug,
                "--json",
                "number,title,body,state,author,url,baseRefName,headRefName,reviews",
            ],
            None,
        ) else {
            return Ok(None);
        };
        let json_str = String::from_utf8_lossy(&view_output.body);
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&json_str) else {
            return Ok(None);
        };

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
        Ok(Some(PrOverviewData {
            number,
            title,
            body,
            state,
            author,
            url,
            base_branch,
            head_branch,
            checks: Vec::new(),
            reviewers,
        }))
    }
}

/// Compatibility wrappers for front ends and engine callers.
pub fn gh_pr_overview(repo_root: &str, pr_number: Option<u64>) -> Option<PrOverviewData> {
    default_handle()
        .overview(repo_root, pr_number)
        .ok()
        .flatten()
}

pub fn gh_pr_overview_no_checks(repo_root: &str, pr_number: Option<u64>) -> Option<PrOverviewData> {
    default_handle()
        .overview_no_checks(repo_root, pr_number)
        .ok()
        .flatten()
}

pub fn gh_pr_overview_remote(owner: &str, repo: &str, number: u64) -> Option<PrOverviewData> {
    default_handle()
        .overview_remote(owner, repo, number)
        .ok()
        .flatten()
}

impl CliGitHub {
    /// Fetch PR comments for a remote repo (no local clone needed).
    pub(crate) fn pr_comments_remote(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
    ) -> Result<Vec<GitHubComment>, GhError> {
        let path = format!("repos/{owner}/{repo}/pulls/{pr}/comments");
        let output = self.run_gh_args(&["api", &path, "--paginate"], None)?;
        parse_paginated(&String::from_utf8_lossy(&output.body)).map_err(GhError::from)
    }
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
    #[serde(default)]
    #[serde(rename = "pageInfo")]
    page_info: Option<GraphQlPageInfo>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct GraphQlPageInfo {
    #[serde(default)]
    has_next_page: bool,
    #[serde(default)]
    end_cursor: Option<String>,
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

impl CliGitHub {
    /// Fetch review thread state for a PR (local repo). A GraphQL failure is
    /// non-fatal: the caller falls back to all-unresolved.
    pub(crate) fn review_threads(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        repo_root: &str,
    ) -> Result<HashMap<u64, ReviewThreadState>, GhError> {
        let query = review_threads_query(owner, repo, pr);
        match self.run_gh_args(
            &["api", "graphql", "-f", &format!("query={query}")],
            Some(repo_root),
        ) {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.body);
                parse_review_threads_response(&stdout).map_err(GhError::from)
            }
            // A spawn or timeout failure is not "no threads"; only a gh answer
            // that is a non-zero exit falls back to all-unresolved.
            Err(error) => match error {
                GhError::NotInstalled | GhError::Timeout | GhError::Tool { status: -1, .. } => {
                    Err(error)
                }
                _ => Ok(HashMap::new()),
            },
        }
    }

    /// Fetch review thread state for a remote PR (no local clone needed).
    pub(crate) fn review_threads_remote(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
    ) -> Result<HashMap<u64, ReviewThreadState>, GhError> {
        let query = review_threads_query(owner, repo, pr);
        match self.run_gh_args(&["api", "graphql", "-f", &format!("query={query}")], None) {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.body);
                parse_review_threads_response(&stdout).map_err(GhError::from)
            }
            // A spawn or timeout failure is not "no threads"; only a gh answer
            // that is a non-zero exit falls back to all-unresolved.
            Err(error) => match error {
                GhError::NotInstalled | GhError::Timeout | GhError::Tool { status: -1, .. } => {
                    Err(error)
                }
                _ => Ok(HashMap::new()),
            },
        }
    }
}

fn review_threads_query(owner: &str, repo: &str, pr: u64) -> String {
    format!(
        r#"query {{ repository(owner: "{owner}", name: "{repo}") {{ pullRequest(number: {pr}) {{ reviewThreads(first: 100) {{ nodes {{ isResolved isOutdated comments(first: 100) {{ nodes {{ databaseId }} }} }} }} }} }} }}"#
    )
}

/// Run two independent closures concurrently and return both results.
///
/// Used so REST comments and GraphQL review-threads (and similar gh pairs)
/// overlap instead of stacking ~2.5–3 s of sequential CLI time.
pub(crate) fn run_parallel<A, B>(
    left: impl FnOnce() -> A + Send,
    right: impl FnOnce() -> B + Send,
) -> (A, B)
where
    A: Send,
    B: Send,
{
    std::thread::scope(|s| {
        let left_h = s.spawn(left);
        let right_h = s.spawn(right);
        (
            left_h
                .join()
                .unwrap_or_else(|_| panic!("left parallel task panicked")),
            right_h
                .join()
                .unwrap_or_else(|_| panic!("right parallel task panicked")),
        )
    })
}

/// Cached PR comment sync bundle (hover-prefetch warming).
///
/// Keyed by (owner, repo, pr); 60s TTL; failures never cached. The two gh calls
/// (REST comments + GraphQL review threads) cost ~2.5–3 s sequentially — they
/// run in parallel here, and the sidebar hover prefetch warms this cache so the
/// post-open `pull_github_comments` is served from memory instead of the
/// network.
#[derive(Debug, Clone)]
pub struct PrCommentBundle {
    pub comments: Vec<GitHubComment>,
    pub threads: HashMap<u64, ReviewThreadState>,
}

impl CliGitHub {
    /// Fetch REST comments + GraphQL review-thread state in parallel; bypasses
    /// the cache. `repo_root = None` uses the remote variants.
    pub(crate) fn fetch_pr_comment_bundle(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        repo_root: Option<&str>,
    ) -> Result<PrCommentBundle, GhError> {
        let (comments, threads) = run_parallel(
            || match repo_root {
                Some(root) => self.pr_comments(owner, repo, pr, root),
                None => self.pr_comments_remote(owner, repo, pr),
            },
            || match repo_root {
                Some(root) => self.review_threads(owner, repo, pr, root),
                None => self.review_threads_remote(owner, repo, pr),
            },
        );
        Ok(PrCommentBundle {
            comments: comments?,
            threads: threads?,
        })
    }

    /// Fetch (and cache) the PR comment sync bundle. A warm cache makes
    /// `sync_github_comments` network-free.
    pub(crate) fn pr_comment_bundle_cached(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        repo_root: Option<&str>,
    ) -> Result<Arc<PrCommentBundle>, GhError> {
        let key = (owner.to_ascii_lowercase(), repo.to_ascii_lowercase(), pr);
        self.cache().pr_comments.get_or_fetch(key, || {
            self.fetch_pr_comment_bundle(owner, repo, pr, repo_root)
                .map(Arc::new)
        })
    }

    /// Drop all cached PR comment bundles. Called on push: a comment pushed
    /// while the 60 s TTL is warm would otherwise be absent from the stale
    /// bundle and dropped from `github-comments.json` on the next pull.
    pub(crate) fn invalidate_comments(&self) {
        self.cache().pr_comments.clear();
    }
}

/// Compatibility wrappers for front ends; removed when they take the handle.
/// Compatibility wrapper for front ends; removed when they take the handle.
pub fn fetch_pr_comment_bundle(
    owner: &str,
    repo: &str,
    pr: u64,
    repo_root: Option<&str>,
) -> Result<PrCommentBundle> {
    default_handle()
        .fetch_pr_comment_bundle(owner, repo, pr, repo_root)
        .map_err(anyhow::Error::from)
}

/// Compatibility wrapper for front ends; removed when they take the handle.
pub fn gh_pr_comment_bundle_cached(
    owner: &str,
    repo: &str,
    pr: u64,
    repo_root: Option<&str>,
) -> Result<Arc<PrCommentBundle>> {
    default_handle()
        .pr_comment_bundle_cached(owner, repo, pr, repo_root)
        .map_err(anyhow::Error::from)
}

pub fn invalidate_pr_comments_cache() {
    default_handle().invalidate_comments();
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

impl CliGitHub {
    /// Push a new review comment to a remote PR (no local clone needed).
    pub(crate) fn push_comment_remote(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        comment: NewLineComment<'_>,
    ) -> Result<u64, GhError> {
        let commit_id = self.pr_head_oid(owner, repo, pr, None)?;
        Ok(self
            .post_line_comment(owner, repo, pr, comment, &commit_id, None)?
            .id)
    }

    /// Post a general comment on a PR (not attached to any file/line). Uses the
    /// Issues API — PRs are issues in GitHub.
    pub(crate) fn general_comment(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        body: &str,
        repo_root: &str,
    ) -> Result<u64, GhError> {
        Ok(self
            .post_general(owner, repo, pr, body, Some(repo_root))?
            .id)
    }

    /// Post a general comment on a remote PR (no local repo required).
    pub(crate) fn general_comment_remote(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        body: &str,
    ) -> Result<u64, GhError> {
        Ok(self.post_general(owner, repo, pr, body, None)?.id)
    }
}

/// The prefix the old free functions put in front of gh's reason, kept so
/// callers that match the rendered text keep working.
fn with_prefix(prefix: &str, error: GhError) -> anyhow::Error {
    anyhow::anyhow!("{prefix}: {error}")
}

/// Compatibility wrappers for front ends and engine callers.
/// Compatibility wrapper for front ends; removed when they take the handle.
pub fn gh_pr_push_comment_remote(
    owner: &str,
    repo: &str,
    pr: u64,
    comment: NewLineComment<'_>,
) -> Result<u64> {
    default_handle()
        .push_comment_remote(owner, repo, pr, comment)
        .map_err(|error| with_prefix("Failed to push comment", error))
}

/// Compatibility wrapper for front ends; removed when they take the handle.
pub fn gh_pr_general_comment(
    owner: &str,
    repo: &str,
    pr: u64,
    body: &str,
    repo_root: &str,
) -> Result<u64> {
    default_handle()
        .general_comment(owner, repo, pr, body, repo_root)
        .map_err(|error| with_prefix("Failed to post general comment", error))
}

/// Compatibility wrapper for front ends; removed when they take the handle.
pub fn gh_pr_general_comment_remote(owner: &str, repo: &str, pr: u64, body: &str) -> Result<u64> {
    default_handle()
        .general_comment_remote(owner, repo, pr, body)
        .map_err(|error| with_prefix("Failed to post general comment", error))
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
    /// GitHub's merge-box verdict: `CLEAN`, `BEHIND`, `BLOCKED`, `DIRTY`,
    /// `UNSTABLE`, `HAS_HOOKS`, `DRAFT`, `UNKNOWN`.
    pub merge_state_status: Option<String>,
    /// Head commit the PR points at now. The desktop merge pins to the reviewed
    /// diff's head when one is on screen, and to this only otherwise (ADR 0040).
    pub head_ref_oid: String,
    /// Head branch lives in a fork. Branch delete/restore is offered only for
    /// same-repo PRs (ADR 0040).
    pub is_cross_repository: bool,
    /// Merge method of a pending auto-merge (`MERGE`, `SQUASH`, `REBASE`).
    pub auto_merge_method: Option<String>,
    /// ISO-8601 time the PR was opened. `gh pr view --json` has no timeline,
    /// so "opened N ago" and the opening activity event are built from this
    /// instead of a timeline query (ADR 0026).
    pub created_at: Option<String>,
    /// Reviews requested and not yet submitted. GitHub drops a request once
    /// the reviewer submits, and adds it back on a re-request.
    pub review_requests: Vec<PrReviewRequest>,
    /// Commits on the PR, counted from the `commits` list `gh` returns.
    pub commit_count: usize,
}

/// One pending review request: a user, or a team named by its `org/slug`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrReviewRequest {
    pub reviewer: String,
    pub is_team: bool,
}

/// A person on the PR's reviewer list, folded from pending requests and
/// submitted reviews.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrReviewer {
    pub reviewer: String,
    pub is_team: bool,
    /// Their review currently has a pending request (first ask or re-ask).
    pub requested: bool,
    /// State of the review that counts for them (`APPROVED`,
    /// `CHANGES_REQUESTED`, `DISMISSED`, `COMMENTED`); `None` until they review.
    pub state: Option<String>,
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

#[derive(Debug, Clone, Default)]
pub struct CheckRun {
    pub name: String,
    pub status: String,
    pub conclusion: String,
    pub url: Option<String>,
    /// ISO-8601 start/end. `gh` reports a not-yet-reached time as Go's zero
    /// time; that parses to `None`, not to year 1.
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    /// Status-context text, e.g. a deploy preview's "Ready". It is what a
    /// check without timings shows in place of a duration.
    pub description: Option<String>,
}

impl CheckRun {
    /// Wall time of a finished check. `None` while it runs, when either time
    /// is missing, or when the clock order is inverted.
    pub fn duration_secs(&self) -> Option<u64> {
        let start = crate::types::parse_github_updated_at(self.started_at.as_deref()?)?;
        let end = crate::types::parse_github_updated_at(self.completed_at.as_deref()?)?;
        u64::try_from(end - start).ok()
    }
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
        merge_state_status: non_empty_str(&v["mergeStateStatus"]),
        head_ref_oid: v["headRefOid"].as_str().unwrap_or("").to_string(),
        is_cross_repository: v["isCrossRepository"].as_bool().unwrap_or(false),
        // `autoMergeRequest` is null when auto-merge is off.
        auto_merge_method: non_empty_str(&v["autoMergeRequest"]["mergeMethod"]),
        created_at: non_empty_str(&v["createdAt"]),
        review_requests: v["reviewRequests"]
            .as_array()
            .map(|arr| arr.iter().filter_map(parse_review_request).collect())
            .unwrap_or_default(),
        commit_count: v["commits"].as_array().map_or(0, Vec::len),
    })
}

/// `gh` flattens a requested reviewer to `{__typename, login}` for a user or
/// bot and `{__typename, name, slug}` for a team, where `slug` is `org/slug`.
fn parse_review_request(r: &serde_json::Value) -> Option<PrReviewRequest> {
    let is_team = r["__typename"].as_str() == Some("Team");
    let reviewer = non_empty_str(&r["login"])
        .or_else(|| non_empty_str(&r["slug"]))
        .or_else(|| non_empty_str(&r["name"]))?;
    Some(PrReviewRequest { reviewer, is_team })
}

/// Fold pending requests and submitted reviews into one reviewer list:
/// requested reviewers first in request order, then everyone else who
/// reviewed, in order of their first review. The PR author's own reviews are
/// replies on their PR, not a review of it, so they are left out.
///
/// The state that counts is the reviewer's latest approve/request-changes/
/// dismiss; a later plain comment does not undo an approval on GitHub either.
/// Someone who only commented shows `COMMENTED`.
pub fn pr_reviewers(
    author: &str,
    requests: &[PrReviewRequest],
    reviews: &[PrReview],
) -> Vec<PrReviewer> {
    let mut out: Vec<PrReviewer> = requests
        .iter()
        .map(|r| PrReviewer {
            reviewer: r.reviewer.clone(),
            is_team: r.is_team,
            requested: true,
            state: None,
        })
        .collect();
    let mut ordered: Vec<&PrReview> = reviews
        .iter()
        .filter(|r| !r.author.is_empty() && !r.author.eq_ignore_ascii_case(author))
        .filter(|r| r.state != "PENDING")
        .collect();
    // `gh` returns reviews oldest first; sort anyway so the latest-wins rule
    // does not lean on that. ISO-8601 in UTC sorts as text.
    ordered.sort_by(|a, b| a.submitted_at.cmp(&b.submitted_at));
    for review in ordered {
        let idx = match out
            .iter()
            .position(|p| !p.is_team && p.reviewer.eq_ignore_ascii_case(&review.author))
        {
            Some(i) => i,
            None => {
                out.push(PrReviewer {
                    reviewer: review.author.clone(),
                    is_team: false,
                    requested: false,
                    state: None,
                });
                out.len() - 1
            }
        };
        let slot = &mut out[idx].state;
        let decisive = review.state != "COMMENTED";
        if decisive || slot.is_none() {
            *slot = Some(review.state.clone());
        }
    }
    out
}

fn non_empty_str(v: &serde_json::Value) -> Option<String> {
    v.as_str().filter(|s| !s.is_empty()).map(|s| s.to_string())
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

/// Pure parser for `gh pr checks --json name,state,bucket,link,startedAt,completedAt,description` output.
pub fn parse_pr_checks(json: &str) -> Result<Vec<CheckRun>> {
    let v: serde_json::Value = serde_json::from_str(json).context("invalid checks JSON")?;
    let arr = v.as_array().cloned().unwrap_or_default();
    Ok(arr
        .into_iter()
        .map(|c| {
            let state = c["state"].as_str().unwrap_or("").to_string();
            let bucket = c["bucket"].as_str().unwrap_or("").to_string();
            // Map `gh pr checks` semantics into status/conclusion:
            //   state ~ PENDING|IN_PROGRESS|QUEUED|SUCCESS|FAILURE|...
            //   bucket ~ pass|fail|pending|cancel|skipping
            let state_l = state.to_ascii_lowercase();
            let (status, conclusion) = if state_l == "pending"
                || state_l == "in_progress"
                || state_l == "queued"
                || state_l == "waiting"
                || state_l == "requested"
                || state_l == "expected"
                || bucket.eq_ignore_ascii_case("pending")
            {
                ("PENDING".to_string(), "".to_string())
            } else {
                ("COMPLETED".to_string(), state)
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
                started_at: check_time(&c["startedAt"]),
                completed_at: check_time(&c["completedAt"]),
                description: non_empty_str(&c["description"]),
            }
        })
        .collect())
}

/// `gh` serializes an unset check time as Go's zero time rather than null.
fn check_time(v: &serde_json::Value) -> Option<String> {
    non_empty_str(v).filter(|s| !s.starts_with("0001-01-01"))
}

/// Fetch the list of commits on a remote PR (no local clone required).
///
/// Returns commits in reverse-chronological order (newest first), capped at
/// `limit`, each with its diffstat — see [`PR_COMMITS_GRAPHQL`] for why this one
/// listing is GraphQL rather than `gh pr view --json commits`.
///
/// On any failure (gh unavailable, no auth, etc.) returns an empty list so the
/// caller degrades gracefully.
pub fn gh_pr_commits_remote(
    owner: &str,
    repo: &str,
    number: u64,
    limit: usize,
) -> Vec<er_git::CommitInfo> {
    let output = match Command::new("gh")
        .args(["api", "graphql", "--paginate", "-f"])
        .arg(format!("query={PR_COMMITS_GRAPHQL}"))
        .arg("-f")
        .arg(format!("owner={owner}"))
        .arg("-f")
        .arg(format!("repo={repo}"))
        .arg("-F")
        .arg(format!("number={number}"))
        .output_timed(er_git::proc::GH_TIMEOUT)
    {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };

    if !output.status.success() {
        return Vec::new();
    }

    parse_pr_commits_graphql(&output.stdout, limit).unwrap_or_default()
}

/// Combined overview + conversation-comments + reviews for a remote PR, in
/// ONE `gh pr view --json` subprocess. Collapses three separate `gh pr view`
/// calls (`gh_pr_overview_remote_full` + `gh_pr_comments_overview` +
/// `gh_pr_reviews`) into one GraphQL-backed call — verified empirically that
/// requesting `comments`/`reviews` alongside other fields returns byte-identical
/// data to requesting them alone.
///
/// `gh pr checks` (CI status) is intentionally NOT folded in here — it's a
/// different `gh` subcommand and must stay a separate call (`gh_pr_checks_remote`).
pub struct PrStatusBundle {
    pub overview: PrOverviewFull,
    pub comments: Vec<PrComment>,
    pub reviews: Vec<PrReview>,
}

/// Fetch overview + comments + reviews for a remote PR in one `gh pr view` call.
/// No local clone required.
pub fn gh_pr_status_remote(owner: &str, repo: &str, number: u64) -> Result<PrStatusBundle> {
    let repo_slug = format!("{}/{}", owner, repo);
    let output = Command::new("gh")
        .args([
            "pr",
            "view",
            &number.to_string(),
            "--repo",
            &repo_slug,
            "--json",
            "number,title,body,state,isDraft,author,reviewDecision,mergeable,mergeStateStatus,headRefName,headRefOid,isCrossRepository,autoMergeRequest,baseRefName,labels,url,comments,reviews,createdAt,reviewRequests,commits",
        ])
        .output_timed(er_git::proc::GH_TIMEOUT)
        .context("Failed to run gh pr view (status bundle)")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh pr view (status bundle) failed: {}", stderr.trim());
    }
    let json = String::from_utf8_lossy(&output.stdout);
    // overview parse failure is fatal (matches old gh_pr_overview_remote_full
    // behavior — caller treats a failed overview as "no status available").
    let overview = parse_pr_overview(&json)?;
    // comments/reviews failures are non-fatal (matches old call sites, which
    // wrapped these in `.unwrap_or_default()` at the spawn closure).
    let comments = parse_pr_comments(&json).unwrap_or_default();
    let reviews = parse_pr_reviews(&json).unwrap_or_default();
    Ok(PrStatusBundle {
        overview,
        comments,
        reviews,
    })
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
            "name,state,bucket,link,startedAt,completedAt,description",
        ])
        .output_timed(er_git::proc::GH_TIMEOUT)
        .context("Failed to run gh pr checks")?;
    if !output.status.success() {
        // No checks configured is not an error — return empty.
        return Ok(Vec::new());
    }
    parse_pr_checks(&String::from_utf8_lossy(&output.stdout))
}

// ── Canonical repo identity ──

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

/// Push a reply to an existing review comment on a remote PR.
/// Compatibility wrapper for front ends; removed when they take the handle.
pub fn gh_pr_reply_comment_remote(
    owner: &str,
    repo: &str,
    pr: u64,
    in_reply_to: u64,
    body: &str,
) -> Result<u64> {
    default_handle()
        .reply_comment_remote(owner, repo, pr, in_reply_to, body)
        .map_err(|error| with_prefix("Failed to push reply", error))
}

impl CliGitHub {
    pub(crate) fn reply_comment_remote(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        in_reply_to: u64,
        body: &str,
    ) -> Result<u64, GhError> {
        Ok(self
            .post_reply(owner, repo, pr, in_reply_to, body, None)?
            .id)
    }
}

// Compatibility wrappers, kept until the callers take the handle.
/// Compatibility wrapper for front ends; removed when they take the handle.
pub fn gh_pr_comments(
    owner: &str,
    repo: &str,
    pr: u64,
    repo_root: &str,
) -> Result<Vec<GitHubComment>> {
    default_handle()
        .pr_comments(owner, repo, pr, repo_root)
        .map_err(|error| with_prefix("Failed to fetch PR comments", error))
}

/// Compatibility wrapper for front ends; removed when they take the handle.
pub fn gh_pr_comments_remote(owner: &str, repo: &str, pr: u64) -> Result<Vec<GitHubComment>> {
    default_handle()
        .pr_comments_remote(owner, repo, pr)
        .map_err(|error| with_prefix("Failed to fetch PR comments", error))
}

/// Compatibility wrapper for front ends; removed when they take the handle.
pub fn gh_pr_review_threads(
    owner: &str,
    repo: &str,
    pr: u64,
    repo_root: &str,
) -> Result<HashMap<u64, ReviewThreadState>> {
    default_handle()
        .review_threads(owner, repo, pr, repo_root)
        .map_err(anyhow::Error::from)
}

/// Compatibility wrapper for front ends; removed when they take the handle.
pub fn gh_pr_review_threads_remote(
    owner: &str,
    repo: &str,
    pr: u64,
) -> Result<HashMap<u64, ReviewThreadState>> {
    default_handle()
        .review_threads_remote(owner, repo, pr)
        .map_err(anyhow::Error::from)
}

/// Compatibility wrapper for front ends; removed when they take the handle.
pub fn gh_pr_head_oid(owner: &str, repo: &str, pr: u64, repo_root: Option<&str>) -> Result<String> {
    default_handle()
        .pr_head_oid(owner, repo, pr, repo_root)
        .map_err(|error| with_prefix("Failed to get HEAD SHA from gh pr view", error))
}

/// Compatibility wrapper for front ends; removed when they take the handle.
pub fn gh_pr_post_line_comment(
    owner: &str,
    repo: &str,
    pr: u64,
    comment: NewLineComment<'_>,
    commit_id: &str,
    repo_root: Option<&str>,
) -> Result<CreatedComment> {
    default_handle()
        .post_line_comment(owner, repo, pr, comment, commit_id, repo_root)
        .map_err(|error| with_prefix("Failed to push comment", error))
}

/// Compatibility wrapper for front ends; removed when they take the handle.
pub fn gh_pr_post_reply(
    owner: &str,
    repo: &str,
    pr: u64,
    in_reply_to: u64,
    body: &str,
    repo_root: Option<&str>,
) -> Result<CreatedComment> {
    default_handle()
        .post_reply(owner, repo, pr, in_reply_to, body, repo_root)
        .map_err(|error| with_prefix("Failed to push reply", error))
}

/// Compatibility wrapper for front ends; removed when they take the handle.
pub fn gh_pr_post_general(
    owner: &str,
    repo: &str,
    pr: u64,
    body: &str,
    repo_root: Option<&str>,
) -> Result<CreatedComment> {
    default_handle()
        .post_general(owner, repo, pr, body, repo_root)
        .map_err(|error| with_prefix("Failed to post general comment", error))
}

/// Compatibility wrapper for front ends; removed when they take the handle.
pub fn gh_pr_issue_comments(
    owner: &str,
    repo: &str,
    pr: u64,
    repo_root: Option<&str>,
) -> Result<Vec<IssueComment>> {
    default_handle()
        .issue_comments(owner, repo, pr, repo_root)
        .map_err(|error| with_prefix("Failed to fetch PR issue comments", error))
}

/// Compatibility wrapper for front ends; removed when they take the handle.
pub fn gh_viewer_login(repo_root: Option<&str>) -> Result<String> {
    default_handle()
        .viewer_login(repo_root)
        .map_err(|error| with_prefix("Failed to read the gh user", error))
}

/// Compatibility wrapper for front ends; removed when they take the handle.
pub fn gh_pr_push_comment(
    owner: &str,
    repo: &str,
    pr: u64,
    comment: NewLineComment<'_>,
    repo_root: &str,
) -> Result<u64> {
    default_handle()
        .push_comment(owner, repo, pr, comment, repo_root)
        .map_err(|error| with_prefix("Failed to push comment", error))
}

/// Compatibility wrapper for front ends; removed when they take the handle.
pub fn gh_pr_reply_comment(
    owner: &str,
    repo: &str,
    pr: u64,
    in_reply_to: u64,
    body: &str,
    repo_root: &str,
) -> Result<u64> {
    default_handle()
        .reply_comment(owner, repo, pr, in_reply_to, body, repo_root)
        .map_err(|error| with_prefix("Failed to push reply", error))
}

/// Compatibility wrapper for front ends; removed when they take the handle.
pub fn gh_pr_update_review_comment(
    owner: &str,
    repo: &str,
    comment_id: u64,
    body: &str,
    repo_root: &str,
) -> Result<()> {
    default_handle()
        .update_review_comment(owner, repo, comment_id, body, repo_root)
        .map_err(|error| with_prefix("Failed to update comment", error))
}

/// Compatibility wrapper for front ends; removed when they take the handle.
pub fn gh_pr_delete_comment(
    owner: &str,
    repo: &str,
    comment_id: u64,
    repo_root: &str,
) -> Result<()> {
    default_handle()
        .delete_comment(owner, repo, comment_id, repo_root)
        .map_err(|error| with_prefix("Failed to delete comment", error))
}

/// Compatibility wrapper for front ends; removed when they take the handle.
pub fn gh_pr_edit_body(repo_root: &str, body: &str) -> Result<()> {
    default_handle()
        .edit_body(repo_root, body)
        .map_err(|error| with_prefix("Failed to update PR body", error))
}

/// Compatibility wrapper for front ends; removed when they take the handle.
pub fn gh_pr_approve(
    repo_root: &str,
    remote_repo: Option<&str>,
    pr_number: Option<u64>,
) -> Result<()> {
    default_handle()
        .approve(repo_root, remote_repo, pr_number)
        .map_err(|error| with_prefix("gh pr approve failed", error))
}

// ─────────────────────────────────────────────────────────────────────────────
// Review-queue listing (MCP / headless). Includes size + merge-state fields that
// the desktop PR list intentionally omits for latency.
// ─────────────────────────────────────────────────────────────────────────────

/// Login of the authenticated `gh` user, if available.
pub fn gh_current_login() -> Option<String> {
    let output = Command::new("gh")
        .args(["api", "user", "--jq", ".login"])
        .output_timed(er_git::proc::GH_TIMEOUT)
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let login = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if login.is_empty() {
        None
    } else {
        Some(login)
    }
}

/// List PRs for a repo with the fields the engine's review queue needs.
///
/// `state` is passed to `gh pr list --state` (`open`, `closed`, `merged`, `all`).
pub fn gh_pr_list_queue(
    owner: &str,
    repo: &str,
    state: &str,
    limit: usize,
) -> Result<Vec<crate::types::QueuePr>> {
    let repo_slug = format!("{}/{}", owner, repo);
    let limit = limit.clamp(1, 100);
    let output = Command::new("gh")
        .args([
            "pr",
            "list",
            "--repo",
            &repo_slug,
            "--state",
            state,
            "--limit",
            &limit.to_string(),
            "--json",
            "number,title,headRefName,baseRefName,state,isDraft,author,reviewRequests,reviewDecision,mergeable,mergeStateStatus,additions,deletions,changedFiles,updatedAt,labels,url,latestReviews",
        ])
        .output_timed(er_git::proc::GH_TIMEOUT)
        .context("Failed to run gh pr list")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh pr list failed: {}", stderr.trim());
    }
    let me = gh_current_login();
    parse_pr_list_queue(&String::from_utf8_lossy(&output.stdout), me.as_deref())
}

/// Pure parser for `gh pr list --json …` review-queue payloads.
pub fn parse_pr_list_queue(
    json: &str,
    current_login: Option<&str>,
) -> Result<Vec<crate::types::QueuePr>> {
    #[derive(Deserialize)]
    struct Raw {
        number: u64,
        title: String,
        #[serde(default, rename = "headRefName")]
        head_ref_name: String,
        #[serde(default, rename = "baseRefName")]
        base_ref_name: String,
        state: String,
        #[serde(rename = "isDraft")]
        is_draft: bool,
        author: RawAuthor,
        #[serde(default, rename = "reviewRequests")]
        review_requests: Vec<RawReviewRequest>,
        #[serde(default, rename = "reviewDecision")]
        review_decision: Option<String>,
        #[serde(default)]
        mergeable: Option<String>,
        #[serde(default, rename = "mergeStateStatus")]
        merge_state_status: Option<String>,
        #[serde(default)]
        additions: u64,
        #[serde(default)]
        deletions: u64,
        #[serde(default, rename = "changedFiles")]
        changed_files: u64,
        #[serde(default, rename = "updatedAt")]
        updated_at: String,
        #[serde(default)]
        labels: Vec<RawLabel>,
        #[serde(default)]
        url: String,
        #[serde(default, rename = "latestReviews")]
        latest_reviews: Vec<RawReview>,
    }
    #[derive(Deserialize)]
    struct RawAuthor {
        login: Option<String>,
    }
    #[derive(Deserialize)]
    struct RawReviewRequest {
        #[serde(default)]
        login: Option<String>,
    }
    #[derive(Deserialize)]
    struct RawLabel {
        name: String,
    }
    #[derive(Deserialize)]
    struct RawReview {
        author: RawAuthor,
        state: String,
    }

    let raw: Vec<Raw> = serde_json::from_str(json).context("invalid gh pr list JSON")?;
    let me = current_login.map(|s| s.to_ascii_lowercase());
    Ok(raw
        .into_iter()
        .map(|r| {
            let reviewers: Vec<String> = r
                .review_requests
                .into_iter()
                .filter_map(|rr| rr.login)
                .collect();
            let review_requested_of_me = me
                .as_ref()
                .map(|m| reviewers.iter().any(|r| r.eq_ignore_ascii_case(m)))
                .unwrap_or(false);
            let my_latest_review_state = me.as_ref().and_then(|m| {
                r.latest_reviews
                    .iter()
                    .find(|rv| {
                        rv.author
                            .login
                            .as_deref()
                            .is_some_and(|l| l.eq_ignore_ascii_case(m))
                    })
                    .map(|rv| rv.state.clone())
            });
            crate::types::QueuePr {
                number: r.number,
                title: r.title,
                author: r.author.login.unwrap_or_default(),
                is_draft: r.is_draft,
                state: r.state,
                review_decision: r.review_decision.filter(|s| !s.is_empty()),
                mergeable: r.mergeable.filter(|s| !s.is_empty()),
                merge_state_status: r.merge_state_status.filter(|s| !s.is_empty()),
                additions: r.additions,
                deletions: r.deletions,
                changed_files: r.changed_files,
                updated_at: r.updated_at,
                labels: r.labels.into_iter().map(|l| l.name).collect(),
                reviewers,
                url: r.url,
                head_ref: r.head_ref_name,
                base_ref: r.base_ref_name,
                production_lines: None,
                review_requested_of_me,
                my_latest_review_state,
                checks_state: None,
            }
        })
        .collect())
}

/// Whether a CI check run counts as failing.
pub fn check_run_is_failing(check: &CheckRun) -> bool {
    let conclusion = check.conclusion.to_ascii_lowercase();
    matches!(
        conclusion.as_str(),
        "failure"
            | "fail"
            | "cancelled"
            | "canceled"
            | "timed_out"
            | "timeout"
            | "error"
            | "action_required"
    )
}

/// Aggregate CI state for a PR from `gh pr checks` results.
pub fn summarize_checks_state(checks: &[CheckRun]) -> &'static str {
    if checks.is_empty() {
        return "unknown";
    }
    if checks.iter().any(check_run_is_failing) {
        return "failing";
    }
    if checks.iter().any(check_run_is_pending) {
        return "pending";
    }
    "passing"
}

fn check_run_is_pending(c: &CheckRun) -> bool {
    let status = c.status.to_ascii_lowercase();
    let conclusion = c.conclusion.to_ascii_lowercase();
    matches!(
        status.as_str(),
        "pending" | "in_progress" | "queued" | "waiting" | "requested" | "expected"
    ) || matches!(conclusion.as_str(), "pending" | "in_progress" | "queued")
}

/// Fetch checks and return rollup state for a remote PR.
pub fn gh_pr_checks_state_remote(owner: &str, repo: &str, number: u64) -> Result<&'static str> {
    let checks = gh_pr_checks_remote(owner, repo, number)?;
    Ok(summarize_checks_state(&checks))
}

/// Thread-level addressing summary (resolved / outdated / open).
pub fn summarize_thread_addressing_from_graphql(
    json: &str,
) -> crate::types::ThreadAddressingSummary {
    let Ok(response) = serde_json::from_str::<ReviewThreadsResponse>(json) else {
        return crate::types::ThreadAddressingSummary::default();
    };
    let flags: Vec<(bool, bool)> = response
        .data
        .repository
        .pull_request
        .review_threads
        .nodes
        .iter()
        .map(|t| (t.is_resolved, t.is_outdated))
        .collect();
    crate::types::ThreadAddressingSummary::from_thread_flags(&flags)
}

/// Fetch review threads and summarize whether feedback is already addressed.
///
/// Paginates GraphQL `reviewThreads` (100/page, up to 10 pages). If more threads
/// remain after the cap, `truncated` is set and `all_addressed` is forced false.
pub fn gh_pr_thread_addressing_remote(
    owner: &str,
    repo: &str,
    pr: u64,
) -> Result<crate::types::ThreadAddressingSummary> {
    const PAGE_SIZE: u32 = 100;
    const MAX_PAGES: u32 = 10;

    let mut flags: Vec<(bool, bool)> = Vec::new();
    let mut cursor: Option<String> = None;
    let mut truncated = false;

    for page in 0..MAX_PAGES {
        let after_arg = match &cursor {
            Some(c) => format!(r#", after: "{}""#, c.replace('"', "")),
            None => String::new(),
        };
        let query = format!(
            r#"query {{ repository(owner: "{}", name: "{}") {{ pullRequest(number: {}) {{ reviewThreads(first: {}{}) {{ pageInfo {{ hasNextPage endCursor }} nodes {{ isResolved isOutdated comments(first: 1) {{ nodes {{ databaseId }} }} }} }} }} }} }}"#,
            owner, repo, pr, PAGE_SIZE, after_arg
        );
        let output = Command::new("gh")
            .args(["api", "graphql", "-f", &format!("query={}", query)])
            .output_timed(er_git::proc::GH_TIMEOUT)
            .context("Failed to fetch review threads")?;
        if !output.status.success() {
            if page == 0 {
                return Ok(crate::types::ThreadAddressingSummary::default());
            }
            truncated = true;
            break;
        }
        let json = String::from_utf8_lossy(&output.stdout);
        let Ok(response) = serde_json::from_str::<ReviewThreadsResponse>(&json) else {
            if page == 0 {
                return Ok(crate::types::ThreadAddressingSummary::default());
            }
            truncated = true;
            break;
        };
        let conn = response.data.repository.pull_request.review_threads;
        for t in &conn.nodes {
            flags.push((t.is_resolved, t.is_outdated));
        }
        let page_info = conn.page_info.unwrap_or_default();
        if !page_info.has_next_page {
            break;
        }
        if page + 1 >= MAX_PAGES {
            truncated = true;
            break;
        }
        cursor = page_info.end_cursor;
        if cursor.as_ref().map(|s| s.is_empty()).unwrap_or(true) {
            truncated = true;
            break;
        }
    }

    Ok(crate::types::ThreadAddressingSummary::from_thread_flags_truncated(&flags, truncated))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_parallel_overlaps_independent_work() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::time::{Duration, Instant};

        let concurrent = AtomicUsize::new(0);
        let max_concurrent = AtomicUsize::new(0);
        let start = Instant::now();
        let (left, right) = run_parallel(
            || {
                let n = concurrent.fetch_add(1, Ordering::SeqCst) + 1;
                max_concurrent.fetch_max(n, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(40));
                concurrent.fetch_sub(1, Ordering::SeqCst);
                1u8
            },
            || {
                let n = concurrent.fetch_add(1, Ordering::SeqCst) + 1;
                max_concurrent.fetch_max(n, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(40));
                concurrent.fetch_sub(1, Ordering::SeqCst);
                2u8
            },
        );
        assert_eq!((left, right), (1, 2));
        assert_eq!(
            max_concurrent.load(Ordering::SeqCst),
            2,
            "both closures should run at the same time"
        );
        assert!(
            start.elapsed() < Duration::from_millis(70),
            "sequential 40ms+40ms should overlap; took {:?}",
            start.elapsed()
        );
    }

    // ── local checkout matching (remote PR reviews) ──

    #[test]
    fn ssh_and_https_origins_resolve_to_the_same_slug() {
        for remote in [
            "git@github.com:Acme/discovery.git",
            "https://github.com/Acme/discovery.git",
            "https://github.com/Acme/discovery",
        ] {
            let (owner, repo) = parse_owner_repo_from_remote(remote).unwrap();
            assert_eq!((owner.as_str(), repo.as_str()), ("Acme", "discovery"));
        }
    }

    #[test]
    fn slug_match_is_case_insensitive() {
        assert!(owner_repo_matches_slug(
            "Acme",
            "Discovery",
            "acme/discovery"
        ));
        assert!(owner_repo_matches_slug(
            "acme",
            "discovery",
            "Acme/Discovery"
        ));
        assert!(owner_repo_matches_slug(
            "acme",
            "discovery",
            "acme/discovery.git"
        ));
    }

    #[test]
    fn slug_match_rejects_other_repos_and_malformed_slugs() {
        assert!(!owner_repo_matches_slug(
            "acme",
            "discovery",
            "acme/discovery-api"
        ));
        assert!(!owner_repo_matches_slug(
            "acme",
            "discovery",
            "other/discovery"
        ));
        assert!(!owner_repo_matches_slug("acme", "discovery", "discovery"));
        assert!(!owner_repo_matches_slug("acme", "discovery", ""));
    }

    #[test]
    fn local_checkout_for_repo_needs_both_inputs() {
        let github = crate::FakeGitHub::default()
            .with_get_repo_info(Ok(("acme".into(), "discovery".into())));
        assert_eq!(
            crate::GitHub::local_checkout_for_repo(&github, &er_git::CliGit, "", "acme/discovery"),
            None
        );
        assert_eq!(
            crate::GitHub::local_checkout_for_repo(&github, &er_git::CliGit, "/tmp/some-repo", ""),
            None
        );
        assert_eq!(
            crate::GitHub::local_checkout_for_repo(
                &github,
                &er_git::CliGit,
                "/tmp/some-repo",
                "acme/discovery"
            ),
            Some("/tmp/some-repo".to_string())
        );
    }

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
    fn parse_pr_commits_reverses_chronological_github_order() {
        let raw = br#"{
          "commits": [
            {
              "oid": "1111111111111111111111111111111111111111",
              "messageHeadline": "old commit",
              "authors": [{"login": "ada"}],
              "committedDate": "2026-06-01T10:00:00Z"
            },
            {
              "oid": "2222222222222222222222222222222222222222",
              "messageHeadline": "new commit",
              "authors": [{"name": "Grace Hopper"}],
              "committedDate": "2026-06-02T10:00:00Z"
            }
          ]
        }"#;

        let commits = parse_pr_commits_view_json(raw, 10).expect("parse commits");

        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].hash, "2222222222222222222222222222222222222222");
        assert_eq!(commits[0].short_hash, "2222222");
        assert_eq!(commits[0].subject, "new commit");
        assert_eq!(commits[0].author, "Grace Hopper");
        assert_eq!(commits[1].hash, "1111111111111111111111111111111111111111");
        assert_eq!(commits[1].author, "ada");
    }

    #[test]
    fn parse_pr_commits_view_marks_diffstat_unknown() {
        let raw = br#"{"commits": [{"oid": "1111111111111111111111111111111111111111"}]}"#;
        let commits = parse_pr_commits_view_json(raw, 10).expect("parse commits");
        assert!(!commits[0].diffstat_known);
    }

    #[test]
    fn parse_pr_commits_graphql_reads_every_page_with_diffstat() {
        let page = |oid: &str, subject: &str, adds: u64, more: bool| {
            format!(
                r#"{{"data":{{"repository":{{"pullRequest":{{"commits":{{
                  "pageInfo":{{"hasNextPage":{more},"endCursor":"c"}},
                  "nodes":[{{"commit":{{"oid":"{oid}","messageHeadline":"{subject}",
                    "committedDate":"2026-06-01T10:00:00Z","additions":{adds},"deletions":2,
                    "changedFilesIfAvailable":3,
                    "authors":{{"nodes":[{{"name":"Ada","user":{{"login":"ada"}}}},
                                         {{"name":"Ghost","user":null}}]}}}}}}]}}}}}}}}}}"#
            )
        };
        // `gh api --paginate` concatenates one document per page.
        let raw = format!(
            "{}\n{}",
            page("1111111111111111111111111111111111111111", "old", 5, true),
            page("2222222222222222222222222222222222222222", "new", 0, false),
        );

        let commits = parse_pr_commits_graphql(raw.as_bytes(), 10).expect("parse commits");

        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].subject, "new");
        assert_eq!((commits[0].adds, commits[0].dels), (0, 2));
        assert!(commits[0].diffstat_known);
        assert_eq!(commits[1].subject, "old");
        assert_eq!(
            (commits[1].file_count, commits[1].adds, commits[1].dels),
            (3, 5, 2)
        );
        assert_eq!(commits[1].author, "ada, Ghost");
    }

    #[test]
    fn combined_status_json_parses_overview_comments_reviews_independently() {
        let json = r#"{
            "number": 42,
            "title": "Fix the bug",
            "body": "Detailed description",
            "state": "OPEN",
            "isDraft": false,
            "author": { "login": "contributor" },
            "reviewDecision": "APPROVED",
            "mergeable": "MERGEABLE",
            "headRefName": "fix/the-bug",
            "baseRefName": "main",
            "labels": [{ "name": "bug" }, { "name": "p1" }],
            "url": "https://github.com/owner/repo/pull/42",
            "comments": [
                {
                    "author": { "login": "alice" },
                    "body": "looks good",
                    "createdAt": "2026-06-01T10:00:00Z",
                    "url": "https://github.com/owner/repo/pull/42#issuecomment-1"
                },
                {
                    "author": { "login": "bob" },
                    "body": "one nit",
                    "createdAt": "2026-06-02T11:00:00Z",
                    "url": "https://github.com/owner/repo/pull/42#issuecomment-2"
                }
            ],
            "reviews": [
                {
                    "author": { "login": "carol" },
                    "state": "CHANGES_REQUESTED",
                    "body": "needs work",
                    "submittedAt": "2026-06-01T12:00:00Z"
                },
                {
                    "author": { "login": "carol" },
                    "state": "APPROVED",
                    "body": "lgtm now",
                    "submittedAt": "2026-06-03T09:00:00Z"
                }
            ]
        }"#;

        let overview = parse_pr_overview(json).unwrap();
        assert_eq!(overview.number, 42);
        assert_eq!(overview.author, "contributor");
        assert_eq!(overview.review_decision.as_deref(), Some("APPROVED"));
        assert_eq!(overview.labels, vec!["bug".to_string(), "p1".to_string()]);

        let comments = parse_pr_comments(json).unwrap();
        assert_eq!(comments.len(), 2);
        assert_eq!(comments[0].author, "alice");
        assert_eq!(comments[1].author, "bob");
        assert_eq!(comments[1].body, "one nit");

        let reviews = parse_pr_reviews(json).unwrap();
        assert_eq!(reviews.len(), 2);
        assert_eq!(reviews[0].state, "CHANGES_REQUESTED");
        assert_eq!(reviews[1].state, "APPROVED");
        assert_eq!(reviews[1].submitted_at, "2026-06-03T09:00:00Z");
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

        // Exercise the REAL parser — a parser regression must fail this test.
        let pr = parse_pr_overview(json).unwrap();
        assert_eq!(pr.number, 123);
        assert_eq!(pr.title, "Add feature X");
        assert_eq!(pr.state, "OPEN");
        assert_eq!(pr.author, "developer");
        assert_eq!(pr.base_ref_name, "main");
        assert_eq!(pr.head_ref_name, "feature/x");
    }

    #[test]
    fn pr_overview_handles_missing_optional_fields() {
        // Minimal JSON with only required fields — defaults must kick in.
        let json = r#"{"number": 1, "title": "T", "state": "OPEN"}"#;
        let pr = parse_pr_overview(json).unwrap();
        assert_eq!(pr.number, 1);
        assert_eq!(pr.author, ""); // missing → empty string default
        assert!(pr.labels.is_empty()); // missing → empty vec
        assert!(!pr.is_draft); // missing → false default
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
            "labels": [{"name": "bug"}, {"name": "ui"}],
            "createdAt": "2026-10-30T09:12:00Z",
            "reviewRequests": [
                {"__typename": "User", "login": "bob"},
                {"__typename": "Team", "name": "Core", "slug": "o/core"}
            ],
            "commits": [
                {"oid": "aaa", "messageHeadline": "first"},
                {"oid": "bbb", "messageHeadline": "second"}
            ]
        }"#;
        let pr = parse_pr_overview(json).unwrap();
        assert_eq!(pr.created_at.as_deref(), Some("2026-10-30T09:12:00Z"));
        assert_eq!(
            pr.review_requests,
            vec![
                PrReviewRequest {
                    reviewer: "bob".into(),
                    is_team: false
                },
                PrReviewRequest {
                    reviewer: "o/core".into(),
                    is_team: true
                },
            ]
        );
        assert_eq!(pr.commit_count, 2);
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
        assert!(pr.merge_state_status.is_none());
        assert!(pr.auto_merge_method.is_none());
        assert_eq!(pr.head_ref_oid, "");
        assert!(!pr.is_cross_repository);
        assert!(pr.created_at.is_none());
        assert!(pr.review_requests.is_empty());
        assert_eq!(pr.commit_count, 0);
    }

    #[test]
    fn parse_pr_overview_skips_review_requests_without_a_name() {
        let json = r#"{"number": 3, "reviewRequests": [
            {"__typename": "Bot", "login": "copilot-pull-request-reviewer"},
            {"__typename": "User"},
            {"__typename": "Team", "name": "Docs"}
        ]}"#;
        let pr = parse_pr_overview(json).unwrap();
        let names: Vec<_> = pr
            .review_requests
            .iter()
            .map(|r| r.reviewer.as_str())
            .collect();
        assert_eq!(names, vec!["copilot-pull-request-reviewer", "Docs"]);
        assert!(pr.review_requests[1].is_team);
    }

    // ── pr_reviewers ──

    fn review(author: &str, state: &str, at: &str) -> PrReview {
        PrReview {
            author: author.into(),
            state: state.into(),
            body: String::new(),
            submitted_at: at.into(),
        }
    }

    #[test]
    fn pr_reviewers_lists_requested_first_then_reviewers_by_first_review() {
        let requests = vec![PrReviewRequest {
            reviewer: "dan".into(),
            is_team: false,
        }];
        let reviews = vec![
            review("carol", "COMMENTED", "2026-10-30T10:00:00Z"),
            review("bob", "APPROVED", "2026-10-30T10:05:00Z"),
        ];
        let got = pr_reviewers("alice", &requests, &reviews);
        let order: Vec<_> = got.iter().map(|r| r.reviewer.as_str()).collect();
        assert_eq!(order, vec!["dan", "carol", "bob"]);
        assert!(got[0].requested && got[0].state.is_none());
        assert_eq!(got[1].state.as_deref(), Some("COMMENTED"));
        assert_eq!(got[2].state.as_deref(), Some("APPROVED"));
        assert!(!got[2].requested);
    }

    #[test]
    fn pr_reviewers_keeps_an_approval_over_a_later_comment() {
        let reviews = vec![
            review("bob", "COMMENTED", "2026-10-30T11:00:00Z"),
            review("bob", "APPROVED", "2026-10-30T10:00:00Z"),
        ];
        let got = pr_reviewers("alice", &[], &reviews);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].state.as_deref(), Some("APPROVED"));
    }

    #[test]
    fn pr_reviewers_latest_decisive_review_wins() {
        let reviews = vec![
            review("bob", "CHANGES_REQUESTED", "2026-10-30T10:00:00Z"),
            review("bob", "APPROVED", "2026-10-30T11:00:00Z"),
        ];
        let got = pr_reviewers("alice", &[], &reviews);
        assert_eq!(got[0].state.as_deref(), Some("APPROVED"));
    }

    #[test]
    fn pr_reviewers_re_request_keeps_prior_state() {
        let requests = vec![PrReviewRequest {
            reviewer: "Bob".into(),
            is_team: false,
        }];
        let reviews = vec![review("bob", "CHANGES_REQUESTED", "2026-10-30T10:00:00Z")];
        let got = pr_reviewers("alice", &requests, &reviews);
        assert_eq!(got.len(), 1);
        assert!(got[0].requested);
        assert_eq!(got[0].state.as_deref(), Some("CHANGES_REQUESTED"));
    }

    #[test]
    fn pr_reviewers_leaves_out_the_author_and_pending_drafts() {
        let reviews = vec![
            review("alice", "COMMENTED", "2026-10-30T10:00:00Z"),
            review("bob", "PENDING", ""),
        ];
        assert!(pr_reviewers("Alice", &[], &reviews).is_empty());
    }

    #[test]
    fn pr_reviewers_does_not_merge_a_user_into_a_team_of_the_same_name() {
        let requests = vec![PrReviewRequest {
            reviewer: "core".into(),
            is_team: true,
        }];
        let reviews = vec![review("core", "APPROVED", "2026-10-30T10:00:00Z")];
        let got = pr_reviewers("alice", &requests, &reviews);
        assert_eq!(got.len(), 2);
        assert!(got[0].state.is_none());
        assert_eq!(got[1].state.as_deref(), Some("APPROVED"));
    }

    #[test]
    fn parse_pr_overview_reads_merge_box_fields() {
        let json = r#"{
            "number": 9,
            "state": "OPEN",
            "mergeStateStatus": "BEHIND",
            "headRefOid": "abc123",
            "isCrossRepository": true,
            "autoMergeRequest": {"mergeMethod": "SQUASH"}
        }"#;
        let pr = parse_pr_overview(json).unwrap();
        assert_eq!(pr.merge_state_status.as_deref(), Some("BEHIND"));
        assert_eq!(pr.head_ref_oid, "abc123");
        assert!(pr.is_cross_repository);
        assert_eq!(pr.auto_merge_method.as_deref(), Some("SQUASH"));
    }

    #[test]
    fn parse_pr_overview_null_auto_merge_is_none() {
        let json = r#"{"number": 9, "autoMergeRequest": null}"#;
        let pr = parse_pr_overview(json).unwrap();
        assert!(pr.auto_merge_method.is_none());
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
    fn parse_pr_checks_reads_timings_and_description() {
        let json = r#"[
            {"name": "test / python", "state": "SUCCESS", "bucket": "pass",
             "startedAt": "2026-10-30T09:00:00Z", "completedAt": "2026-10-30T09:01:52Z",
             "description": ""},
            {"name": "Vercel", "state": "SUCCESS", "bucket": "pass",
             "startedAt": "0001-01-01T00:00:00Z", "completedAt": "0001-01-01T00:00:00Z",
             "description": "Ready"},
            {"name": "cleanup", "state": "IN_PROGRESS", "bucket": "pending",
             "startedAt": "2026-10-30T09:00:00Z", "completedAt": "0001-01-01T00:00:00Z"}
        ]"#;
        let checks = parse_pr_checks(json).unwrap();
        assert_eq!(
            checks[0].started_at.as_deref(),
            Some("2026-10-30T09:00:00Z")
        );
        assert_eq!(checks[0].duration_secs(), Some(112));
        assert!(checks[0].description.is_none());

        assert!(checks[1].started_at.is_none());
        assert!(checks[1].completed_at.is_none());
        assert_eq!(checks[1].duration_secs(), None);
        assert_eq!(checks[1].description.as_deref(), Some("Ready"));

        assert!(checks[2].started_at.is_some());
        assert!(checks[2].completed_at.is_none());
        assert_eq!(checks[2].duration_secs(), None);
    }

    #[test]
    fn check_duration_is_none_when_end_precedes_start() {
        let check = CheckRun {
            started_at: Some("2026-10-30T09:01:00Z".into()),
            completed_at: Some("2026-10-30T09:00:00Z".into()),
            ..Default::default()
        };
        assert_eq!(check.duration_secs(), None);
    }

    #[test]
    fn parse_pr_checks_empty_array() {
        assert!(parse_pr_checks("[]").unwrap().is_empty());
    }

    // ── parse_remote_url ──

    #[test]
    fn parse_remote_url_ssh_scp_style() {
        let (owner, repo) = parse_remote_url("git@github.com:Acme/My-Repo.git").unwrap();
        assert_eq!(owner, "Acme");
        assert_eq!(repo, "My-Repo");
    }

    #[test]
    fn parse_remote_url_https() {
        let (owner, repo) = parse_remote_url("https://github.com/Acme/My-Repo.git").unwrap();
        assert_eq!(owner, "Acme");
        assert_eq!(repo, "My-Repo");
    }

    #[test]
    fn parse_remote_url_https_no_git_suffix() {
        let (owner, repo) = parse_remote_url("https://github.com/Acme/My-Repo").unwrap();
        assert_eq!(owner, "Acme");
        assert_eq!(repo, "My-Repo");
    }

    #[test]
    fn parse_remote_url_ssh_scheme() {
        let (owner, repo) = parse_remote_url("ssh://git@github.com/Acme/My-Repo.git").unwrap();
        assert_eq!(owner, "Acme");
        assert_eq!(repo, "My-Repo");
    }

    #[test]
    fn parse_remote_url_same_owner_repo_for_ssh_and_https() {
        // Both forms of the same origin must resolve identically.
        let ssh = parse_remote_url("git@github.com:Acme/My-Repo.git").unwrap();
        let https = parse_remote_url("https://github.com/Acme/My-Repo.git").unwrap();
        assert_eq!(ssh, https);
        assert_eq!(ssh, ("Acme".to_string(), "My-Repo".to_string()));
    }

    #[test]
    fn parse_remote_url_preserves_case() {
        let (owner, repo) = parse_remote_url("https://github.com/UPPER/lower.git").unwrap();
        assert_eq!(owner, "UPPER");
        assert_eq!(repo, "lower");
    }

    #[test]
    fn parse_remote_url_invalid_no_slash() {
        assert!(parse_remote_url("https://github.com/only-one-segment").is_none());
    }

    #[test]
    fn parse_remote_url_empty() {
        assert!(parse_remote_url("").is_none());
    }

    fn git(root: &std::path::Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// A clone whose origin had a `stack-base` branch that was then deleted —
    /// a stacked PR's base after it merged. Returns (tempdir, clone, base sha).
    fn clone_with_deleted_base() -> (tempfile::TempDir, std::path::PathBuf, String) {
        let dir = tempfile::tempdir().unwrap();
        let origin = dir.path().join("origin.git");
        let clone = dir.path().join("clone");
        git(
            dir.path(),
            &["init", "--bare", "-b", "main", origin.to_str().unwrap()],
        );
        git(
            dir.path(),
            &["clone", origin.to_str().unwrap(), clone.to_str().unwrap()],
        );
        git(&clone, &["commit", "--allow-empty", "-m", "root"]);
        git(&clone, &["push", "origin", "HEAD:main"]);
        git(&clone, &["checkout", "-b", "stack-base"]);
        git(&clone, &["commit", "--allow-empty", "-m", "base"]);
        let sha = git(&clone, &["rev-parse", "HEAD"]);
        git(&clone, &["push", "origin", "stack-base"]);
        git(&clone, &["checkout", "main"]);
        git(&clone, &["push", "origin", "--delete", "stack-base"]);
        (dir, clone, sha)
    }

    #[test]
    fn deleted_base_branch_is_a_missing_remote_ref() {
        let (_dir, clone, _) = clone_with_deleted_base();
        let err = fetch_base_branch_ref(&er_git::CliGit, clone.to_str().unwrap(), "stack-base")
            .unwrap_err();
        assert!(is_missing_remote_ref(&err), "{err:#}");
        assert!(!is_missing_remote_ref(&anyhow::anyhow!(
            "Could not resolve host: github.com"
        )));
    }

    #[test]
    fn pr_base_falls_back_to_the_pr_base_commit_when_the_branch_is_deleted() {
        let (_dir, clone, sha) = clone_with_deleted_base();
        let root = clone.to_str().unwrap();
        // Before: the plain base fetch is the error the user saw in set_mode.
        assert!(fetch_base_branch_ref(&er_git::CliGit, root, "stack-base").is_err());

        let base =
            fetch_pr_base_ref_with(&er_git::CliGit, root, 7, "stack-base", || Ok(sha.clone()))
                .unwrap();
        assert!(is_pinned_pr_base(&base));
        assert_eq!(
            rev_parse_oid(&er_git::CliGit, root, &base).as_deref(),
            Some(sha.as_str())
        );
    }

    #[test]
    fn pr_base_fetch_of_a_live_branch_never_looks_up_the_commit() {
        let (_dir, clone, _) = clone_with_deleted_base();
        let base =
            fetch_pr_base_ref_with(&er_git::CliGit, clone.to_str().unwrap(), 7, "main", || {
                panic!("the fallback must not run for a branch origin still has")
            })
            .unwrap();
        assert_eq!(base, "origin/main");
    }

    #[test]
    fn fetch_failure_text_survives_the_git_error() {
        let missing = er_git::FakeGit::default().with_fetch(Err(er_git::GitError::Failed {
            context: "git fetch failed".into(),
            stderr: "fatal: couldn't find remote ref refs/heads/x".into(),
        }));
        let err = fetch_base_branch_ref(&missing, "/repo", "x").unwrap_err();
        assert!(is_missing_remote_ref(&err), "{err:#}");

        let no_upstream = er_git::FakeGit::default().with_fetch(Err(er_git::GitError::Failed {
            context: "git fetch failed".into(),
            stderr: "fatal: has no upstream to refresh".into(),
        }));
        let err = fetch_base_branch_ref(&no_upstream, "/repo", "x").unwrap_err();
        assert!(is_no_upstream_to_refresh(&err), "{err:#}");
    }

    #[test]
    fn fetch_pr_head_calls_fetch_with_the_pull_refspec() {
        let fake = er_git::FakeGit::default().with_fetch(Ok(()));

        let ref_name = fetch_pr_head(&fake, 7, "/repo").unwrap();

        assert_eq!(ref_name, "refs/er/pr/7/head");
        assert!(
            fake.calls().iter().any(|c| c
                == "fetch(root=\"/repo\", args=[\"origin\", \"+pull/7/head:refs/er/pr/7/head\"])"),
            "{:?}",
            fake.calls()
        );
    }

    #[test]
    fn deleted_base_pins_to_its_local_commit() {
        let (_dir, clone, sha) = clone_with_deleted_base();
        let root = clone.to_str().unwrap();
        let pinned = pin_pr_base_commit(&er_git::CliGit, root, 7, &sha).unwrap();
        assert_eq!(pinned, "refs/er/pr/7/base");
        assert_eq!(
            rev_parse_oid(&er_git::CliGit, root, &pinned).as_deref(),
            Some(sha.as_str())
        );
    }

    #[test]
    fn deleted_base_commit_missing_locally_is_fetched_by_sha() {
        let (dir, _clone, sha) = clone_with_deleted_base();
        let origin = dir.path().join("origin.git");
        // GitHub serves any commit by SHA; a bare repo only does with this set.
        git(
            &origin,
            &["config", "uploadpack.allowAnySHA1InWant", "true"],
        );
        let fresh = dir.path().join("fresh");
        git(
            dir.path(),
            &[
                "clone",
                "--no-local",
                origin.to_str().unwrap(),
                fresh.to_str().unwrap(),
            ],
        );
        let root = fresh.to_str().unwrap();
        assert!(!ref_exists_locally(
            &er_git::CliGit,
            root,
            &format!("{sha}^{{commit}}")
        ));

        let pinned = pin_pr_base_commit(&er_git::CliGit, root, 7, &sha).unwrap();
        assert_eq!(
            rev_parse_oid(&er_git::CliGit, root, &pinned).as_deref(),
            Some(sha.as_str())
        );
    }

    #[test]
    fn ci_checks_parsed_from_gh_json() {
        // Simulate `gh pr checks --json name,state,bucket` output
        let json = r#"[
            {"name": "test", "state": "SUCCESS", "bucket": "pass"},
            {"name": "lint", "state": "FAILURE", "bucket": "fail"},
            {"name": "build", "state": "PENDING"}
        ]"#;
        // Exercise the REAL parser — a regression in state/bucket mapping must fail this.
        let checks = parse_pr_checks(json).unwrap();
        assert_eq!(checks.len(), 3);
        assert_eq!(checks[0].name, "test");
        assert_eq!(checks[0].status, "COMPLETED");
        assert_eq!(checks[0].conclusion, "SUCCESS");
        assert_eq!(checks[1].name, "lint");
        assert_eq!(checks[1].status, "COMPLETED");
        assert_eq!(checks[1].conclusion, "FAILURE");
        assert_eq!(checks[2].name, "build");
        assert_eq!(checks[2].status, "PENDING");
        assert_eq!(checks[2].conclusion, "");
    }

    #[test]
    fn parse_pr_list_queue_marks_review_requested_of_me() {
        let json = r#"[
          {
            "number": 7,
            "title": "Tiny fix",
            "headRefName": "fix",
            "baseRefName": "main",
            "state": "OPEN",
            "isDraft": false,
            "author": {"login": "alice"},
            "reviewRequests": [{"login": "bob"}],
            "reviewDecision": "REVIEW_REQUIRED",
            "mergeable": "MERGEABLE",
            "mergeStateStatus": "CLEAN",
            "additions": 4,
            "deletions": 1,
            "changedFiles": 1,
            "updatedAt": "2026-07-01T00:00:00Z",
            "labels": [{"name": "bug"}],
            "url": "https://github.com/o/r/pull/7",
            "latestReviews": [{"author": {"login": "carol"}, "state": "COMMENTED"}]
          }
        ]"#;
        let prs = parse_pr_list_queue(json, Some("bob")).unwrap();
        assert_eq!(prs.len(), 1);
        assert!(prs[0].review_requested_of_me);
        assert_eq!(prs[0].additions, 4);
        assert_eq!(prs[0].labels, vec!["bug".to_string()]);
        assert!(prs[0].my_latest_review_state.is_none());

        let prs2 = parse_pr_list_queue(json, Some("carol")).unwrap();
        assert!(!prs2[0].review_requested_of_me);
        assert_eq!(prs2[0].my_latest_review_state.as_deref(), Some("COMMENTED"));
    }

    #[test]
    fn summarize_checks_state_detects_failures() {
        let checks = vec![
            CheckRun {
                name: "lint".into(),
                status: "COMPLETED".into(),
                conclusion: "SUCCESS".into(),
                ..Default::default()
            },
            CheckRun {
                name: "test".into(),
                status: "COMPLETED".into(),
                conclusion: "FAILURE".into(),
                ..Default::default()
            },
        ];
        assert_eq!(summarize_checks_state(&checks), "failing");
    }

    #[test]
    fn summarize_checks_state_treats_in_progress_as_pending() {
        let checks = vec![CheckRun {
            name: "build".into(),
            status: "IN_PROGRESS".into(),
            conclusion: "".into(),
            ..Default::default()
        }];
        assert_eq!(summarize_checks_state(&checks), "pending");
        let queued = vec![CheckRun {
            name: "build".into(),
            status: "QUEUED".into(),
            conclusion: "".into(),
            ..Default::default()
        }];
        assert_eq!(summarize_checks_state(&queued), "pending");
    }

    #[test]
    fn thread_addressing_summary_from_graphql() {
        let json = r#"{
          "data": {
            "repository": {
              "pullRequest": {
                "reviewThreads": {
                  "nodes": [
                    {"isResolved": true, "isOutdated": false, "comments": {"nodes": [{"databaseId": 1}]}},
                    {"isResolved": false, "isOutdated": true, "comments": {"nodes": [{"databaseId": 2}]}}
                  ]
                }
              }
            }
          }
        }"#;
        let s = summarize_thread_addressing_from_graphql(json);
        assert!(s.all_addressed);
        assert_eq!(s.thread_count, 2);
    }

    // ── caches ──

    fn cli_github() -> CliGitHub {
        CliGitHub::new(Arc::new(crate::testing::ScriptedRunner::new()))
    }

    #[test]
    fn repo_info_is_served_from_cache_within_60_seconds() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_str().unwrap();
        git_init_with_remote(root, "https://github.com/Acme/discovery.git");
        let github = cli_github();
        let (owner, repo) = github
            .get_repo_info(&er_git::CliGit, root)
            .expect("parses origin remote");
        assert_eq!((owner.as_str(), repo.as_str()), ("Acme", "discovery"));
        // A fresh lookup would now fail; a hit proves the cache served it.
        std::process::Command::new("git")
            .args(["remote", "remove", "origin"])
            .current_dir(root)
            .output()
            .unwrap();
        let (owner2, repo2) = github
            .get_repo_info(&er_git::CliGit, root)
            .expect("cache serves");
        assert_eq!((owner2.as_str(), repo2.as_str()), ("Acme", "discovery"));
    }

    #[test]
    fn repo_info_cache_is_keyed_by_the_root_as_given() {
        let caches = crate::Caches::new();
        caches
            .repo_info
            .put("/Repo/Root".to_string(), Arc::new(("o".into(), "r".into())));
        assert!(caches.repo_info.get(&"/Repo/Root".to_string()).is_some());
        assert!(
            caches.repo_info.get(&"/repo/root".to_string()).is_none(),
            "the repo root key keeps case"
        );
    }

    #[test]
    fn comment_bundle_cache_serves_within_ttl_and_expires() {
        let cache: crate::cache::TtlCache<(String, String, u64), Arc<PrCommentBundle>> =
            crate::cache::TtlCache::new(std::time::Duration::from_millis(5), Some(4));
        let bundle = Arc::new(PrCommentBundle {
            comments: Vec::new(),
            threads: HashMap::new(),
        });
        cache.put(("o".into(), "r".into(), 1u64), Arc::clone(&bundle));
        assert!(
            cache.get(&("o".into(), "r".into(), 1u64)).is_some(),
            "fresh entry serves"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert!(
            cache.get(&("o".into(), "r".into(), 1u64)).is_none(),
            "expired entry must miss"
        );
    }

    #[test]
    fn comment_bundle_cache_is_keyed_by_lowercased_owner_repo_pr() {
        let runner = Arc::new(crate::testing::ScriptedRunner::new());
        runner.script(
            &[
                "gh",
                "api",
                "--include",
                "repos/Acme/Repo/pulls/1/comments",
                "--paginate",
            ],
            Ok(crate::testing::ScriptedRunner::output(0, "[]", "")),
        );
        runner.script(
            &["gh", "api", "--include", "graphql"],
            Ok(crate::testing::ScriptedRunner::output(0, "{}", "")),
        );
        let github = CliGitHub::new(runner);
        github
            .pr_comment_bundle_cached("Acme", "Repo", 1, None)
            .expect("bundle");
        assert!(
            github
                .cache()
                .pr_comments
                .get(&("acme".into(), "repo".into(), 1))
                .is_some(),
            "the cache key is lowercased"
        );
    }

    #[test]
    fn push_comment_sends_path_line_side_and_body() {
        let runner = Arc::new(crate::testing::ScriptedRunner::new());
        runner.script(
            &["gh", "pr", "view"],
            Ok(crate::test_support::output(0, "abc\n", "")),
        );
        runner.script(
            &["gh", "api", "--include", "-X", "POST"],
            Ok(crate::test_support::output(0, r#"{"id":9}"#, "")),
        );
        let github = CliGitHub::new(runner.clone());
        let id = github
            .push_comment(
                "o",
                "r",
                7,
                NewLineComment {
                    path: "a.rs",
                    line_start: 3,
                    line_end: Some(5),
                    body: "hey",
                    side: "RIGHT",
                },
                "/root",
            )
            .unwrap();
        assert_eq!(id, 9);
        let calls = runner.calls();
        let post = &calls[1];
        for expected in [
            "path=a.rs",
            "line=5",
            "start_line=3",
            "side=RIGHT",
            "body=hey",
            "commit_id=abc",
        ] {
            assert!(
                post.iter().any(|arg| arg == expected),
                "{expected} missing from {post:?}"
            );
        }
    }

    #[test]
    fn reply_comment_targets_the_parent_comment_id() {
        let runner = Arc::new(crate::testing::ScriptedRunner::new());
        runner.script(
            &["gh", "api", "--include", "-X", "POST"],
            Ok(crate::test_support::output(0, r#"{"id":10}"#, "")),
        );
        let github = CliGitHub::new(runner.clone());
        let id = github
            .reply_comment("o", "r", 7, 42, "hi", "/root")
            .unwrap();
        assert_eq!(id, 10);
        let calls = runner.calls();
        assert!(
            calls[0].iter().any(|arg| arg == "in_reply_to=42"),
            "{:?}",
            calls[0]
        );
    }

    #[test]
    fn delete_comment_treats_404_as_already_gone() {
        let runner = Arc::new(crate::testing::ScriptedRunner::new());
        runner.script(
            &["gh", "api", "--include", "-X", "DELETE"],
            Ok(crate::test_support::output(
                1,
                "HTTP/2.0 404 Not Found\r\n\r\n{}",
                "gh: Not Found (HTTP 404)",
            )),
        );
        let github = CliGitHub::new(runner);
        github.delete_comment("o", "r", 5, "/root").unwrap();
    }

    #[test]
    fn delete_comment_fails_on_403() {
        let runner = Arc::new(crate::testing::ScriptedRunner::new());
        runner.script(
            &["gh", "api", "--include", "-X", "DELETE"],
            Ok(crate::test_support::output(
                1,
                "HTTP/2.0 403 Forbidden\r\n\r\n\
                 {\"message\":\"Resource not accessible by integration\"}",
                "gh: Resource not accessible by integration (HTTP 403)",
            )),
        );
        let github = CliGitHub::new(runner);
        let error = github.delete_comment("o", "r", 5, "/root").unwrap_err();
        assert!(matches!(error, GhError::Forbidden { .. }), "{error:?}");
    }

    #[test]
    fn invalidate_comments_leaves_merge_settings_cached() {
        let github = cli_github();
        let bundle = Arc::new(PrCommentBundle {
            comments: Vec::new(),
            threads: HashMap::new(),
        });
        github
            .cache()
            .pr_comments
            .put(("o".into(), "r".into(), 1), bundle);
        github.cache().merge_queue.put("o/r:main".to_string(), true);
        github.invalidate_comments();
        assert!(github
            .cache()
            .pr_comments
            .get(&("o".into(), "r".into(), 1))
            .is_none());
        assert_eq!(
            github.cache().merge_queue.get(&"o/r:main".to_string()),
            Some(true)
        );
    }

    fn git_init_with_remote(root: &str, remote: &str) {
        let out = Command::new("git")
            .args(["init", "-q", root])
            .output()
            .expect("git init");
        assert!(out.status.success());
        let out = Command::new("git")
            .args(["remote", "add", "origin", remote])
            .current_dir(root)
            .output()
            .expect("git remote add");
        assert!(out.status.success());
    }

    #[test]
    fn pr_number_from_notification_url_parses_pulls_and_issues() {
        assert_eq!(
            pr_number_from_notification_url("https://api.github.com/repos/org/repo/pulls/42"),
            Some(42)
        );
        assert_eq!(
            pr_number_from_notification_url("https://api.github.com/repos/org/repo/issues/99"),
            Some(99)
        );
        assert_eq!(
            pr_number_from_notification_url("https://github.com/org/repo/pull/7"),
            Some(7)
        );
        assert_eq!(
            pr_number_from_notification_url("https://api.github.com/repos/org/repo/commits/abc"),
            None
        );
    }

    #[test]
    fn diff_too_large_matches_githubs_refusals_and_nothing_else() {
        assert!(gh_diff_too_large(
            "could not find pull request diff: HTTP 406: Sorry, the diff exceeded the maximum \
             number of files (300). Consider using 'List pull requests files' API"
        ));
        assert!(gh_diff_too_large("PullRequest.diff too_large"));
        assert!(!gh_diff_too_large("HTTP 404: Not Found"));
        assert!(!gh_diff_too_large(
            "no pull requests found for branch 14060"
        ));
    }

    #[test]
    fn merge_base_diff_excludes_commits_the_base_gained_since_the_branch_point() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q", "-b", "main"]);
        std::fs::write(root.join("shared.txt"), "one\n").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-q", "-m", "base"]);

        git(root, &["checkout", "-q", "-b", "feature"]);
        std::fs::write(root.join("feature.txt"), "from the PR\n").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-q", "-m", "pr"]);
        let head = git(root, &["rev-parse", "HEAD"]);

        git(root, &["checkout", "-q", "main"]);
        std::fs::write(root.join("landed-later.txt"), "not part of the PR\n").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-q", "-m", "base moved on"]);
        let base_tip = git(root, &["rev-parse", "HEAD"]);

        let diff =
            git_merge_base_diff(&er_git::CliGit, root.to_str().unwrap(), &base_tip, &head).unwrap();
        assert!(diff.contains("feature.txt"), "{diff}");
        assert!(!diff.contains("landed-later.txt"), "{diff}");
    }

    #[test]
    fn parse_paginated_handles_brackets_inside_strings() {
        let one = r#"[{"id":1,"body":"a ] bracket and ][ in text"}]"#;
        let parsed: Vec<serde_json::Value> = parse_paginated(one).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0]["body"], "a ] bracket and ][ in text");

        let two = r#"[{"id":1}][{"id":2}]"#;
        let parsed: Vec<serde_json::Value> = parse_paginated(two).unwrap();
        assert_eq!(parsed.len(), 2);
    }

    #[test]
    fn parse_paginated_rejects_a_bad_chunk() {
        let bad = r#"[{"id":1}][not json]"#;
        assert!(parse_paginated::<serde_json::Value>(bad).is_err());
    }

    #[test]
    fn pr_lookup_absent_only_when_gh_says_no_pull_request() {
        assert_eq!(
            classify_pr_lookup_failure("no pull requests found for branch \"feat\""),
            PrLookup::Absent
        );
        assert!(matches!(
            classify_pr_lookup_failure("gh: authentication required"),
            PrLookup::Unknown(_)
        ));
        assert!(matches!(
            classify_pr_lookup_failure("request timed out"),
            PrLookup::Unknown(_)
        ));
    }

    fn prefixed(prefix: &str) -> String {
        with_prefix(
            prefix,
            GhError::Forbidden {
                message: "Resource not accessible by integration".into(),
            },
        )
        .to_string()
    }

    #[test]
    fn wrapper_prefix_fetch_pr_comments() {
        assert_eq!(
            prefixed("Failed to fetch PR comments"),
            "Failed to fetch PR comments: Resource not accessible by integration"
        );
    }

    #[test]
    fn wrapper_prefix_get_head_sha() {
        assert_eq!(
            prefixed("Failed to get HEAD SHA from gh pr view"),
            "Failed to get HEAD SHA from gh pr view: Resource not accessible by integration"
        );
    }

    #[test]
    fn wrapper_prefix_push_comment() {
        assert_eq!(
            prefixed("Failed to push comment"),
            "Failed to push comment: Resource not accessible by integration"
        );
    }

    #[test]
    fn wrapper_prefix_push_reply() {
        assert_eq!(
            prefixed("Failed to push reply"),
            "Failed to push reply: Resource not accessible by integration"
        );
    }

    #[test]
    fn wrapper_prefix_post_general_comment() {
        assert_eq!(
            prefixed("Failed to post general comment"),
            "Failed to post general comment: Resource not accessible by integration"
        );
    }

    #[test]
    fn wrapper_prefix_fetch_pr_issue_comments() {
        assert_eq!(
            prefixed("Failed to fetch PR issue comments"),
            "Failed to fetch PR issue comments: Resource not accessible by integration"
        );
    }

    #[test]
    fn wrapper_prefix_read_gh_user() {
        assert_eq!(
            prefixed("Failed to read the gh user"),
            "Failed to read the gh user: Resource not accessible by integration"
        );
    }

    #[test]
    fn wrapper_prefix_update_comment() {
        assert_eq!(
            prefixed("Failed to update comment"),
            "Failed to update comment: Resource not accessible by integration"
        );
    }

    #[test]
    fn wrapper_prefix_delete_comment() {
        assert_eq!(
            prefixed("Failed to delete comment"),
            "Failed to delete comment: Resource not accessible by integration"
        );
    }

    #[test]
    fn wrapper_prefix_update_pr_body() {
        assert_eq!(
            prefixed("Failed to update PR body"),
            "Failed to update PR body: Resource not accessible by integration"
        );
    }

    #[test]
    fn wrapper_prefix_approve() {
        assert_eq!(
            prefixed("gh pr approve failed"),
            "gh pr approve failed: Resource not accessible by integration"
        );
    }
}
