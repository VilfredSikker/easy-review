//! Review and issue comments: reading them, posting, replying, editing,
//! deleting, review-thread state, and the cached comment bundle.

use crate::with_prefix;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;

use crate::{default_handle, CliGitHub, GhError};

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

/// Parse `gh api --paginate` output: one array, or several concatenated arrays.
///
/// Any parse error propagates: a chunk that fails must not be skipped, or a
/// partial list would look complete to a caller reconciling a retry.
pub(crate) fn parse_paginated<T: serde::de::DeserializeOwned>(stdout: &str) -> Result<Vec<T>> {
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
pub(crate) fn parse_review_threads_response(json: &str) -> Result<HashMap<u64, ReviewThreadState>> {
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
impl CliGitHub {
    pub(crate) fn pr_thread_addressing_remote(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
    ) -> Result<crate::types::ThreadAddressingSummary, GhError> {
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
            let out = match self
                .run_gh_args(&["api", "graphql", "-f", &format!("query={query}")], None)
            {
                Ok(out) => out,
                Err(_) => {
                    if page == 0 {
                        return Ok(crate::types::ThreadAddressingSummary::default());
                    }
                    truncated = true;
                    break;
                }
            };
            let json = String::from_utf8_lossy(&out.body);
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
}

/// Compatibility wrapper for front ends and engine callers.
pub fn gh_pr_thread_addressing_remote(
    owner: &str,
    repo: &str,
    pr: u64,
) -> Result<crate::types::ThreadAddressingSummary> {
    default_handle()
        .pr_thread_addressing_remote(owner, repo, pr)
        .map_err(anyhow::Error::from)
}
