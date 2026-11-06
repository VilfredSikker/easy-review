//! Post GitHub review comments at creation and reconcile a retry.
//!
//! A create has three possible outcomes: GitHub accepted it, GitHub refused it
//! before doing anything, or the POST ran and its result is unknown (a timeout
//! or a 5xx). A retry must not post a second copy for the third case, so the
//! pending attempt is remembered and the PR's comments are listed before the
//! retry posts.
//!
//! This module has no `App` dependency: the state layer in
//! `app::state::github_post` owns the pending records and drives the phases.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;

use crate::github;

/// Where a comment is posted: one PR in one repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrPostTarget {
    pub owner: String,
    pub repo: String,
    pub pr: u64,
    /// `Some(repo_root)` for a local checkout; `None` for a remote PR tab.
    pub repo_root: Option<String>,
}

/// What kind of GitHub comment a create is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitHubPostTarget {
    Line {
        path: String,
        line_start: usize,
        line_end: usize,
        side: String,
    },
    Reply {
        in_reply_to: u64,
    },
    General,
}

/// One create intent. A retry reuses the client id and target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubPostRequest {
    /// Stable id of one create intent; a retry reuses it.
    pub client_id: String,
    pub pr: PrPostTarget,
    pub target: GitHubPostTarget,
    pub body: String,
}

/// A comment as the reconcile list returns it, across both listers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedGitHubComment {
    pub id: u64,
    pub body: String,
    pub author: String,
    pub created_at: String,
    pub path: Option<String>,
    pub line: Option<usize>,
    pub start_line: Option<usize>,
    pub original_line: Option<usize>,
    pub original_start_line: Option<usize>,
    pub side: Option<String>,
    pub in_reply_to_id: Option<u64>,
}

/// What one `post` attempt did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitHubPostOutcome {
    Posted {
        github_id: u64,
        created_at: String,
        author: String,
    },
    /// The POST ran; GitHub may or may not hold the comment.
    UnknownOutcome { error: String },
    /// Nothing was sent.
    NotPosted { error: String },
}

/// How a comment reaches GitHub. Tests implement this to drive the phases.
pub trait GitHubCommentPoster: Send + Sync {
    fn post(&self, request: &GitHubPostRequest) -> GitHubPostOutcome;
    /// Uncached; the reconcile list, never the 60 s bundle cache.
    fn list(&self, request: &GitHubPostRequest) -> Result<Vec<ListedGitHubComment>>;
    /// `gh api user --jq .login`; called only when a pending attempt exists.
    fn viewer_login(&self, request: &GitHubPostRequest) -> Result<String>;
}

/// Zero-sized real implementation over `crate::github`.
pub struct GhCommentPoster;

/// One create attempt remembered across retries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingGitHubPost {
    /// `chrono_now()` at the first begin; never moved by a retry.
    pub started_at: String,
    /// Epoch seconds before which a retry must not list or post.
    pub not_before_epoch: u64,
    /// Ids already in the store at the first begin; never adopted.
    pub known_ids: HashSet<u64>,
    /// Every body attempted under this client id, trimmed for comparison.
    pub bodies: Vec<String>,
    /// The latest attempt, for diagnostics and the target match.
    pub request: GitHubPostRequest,
}

/// The result of reconciling, or of posting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitHubPostExecution {
    Post(GitHubPostOutcome),
    /// An earlier attempt already landed; do not post again.
    Adopted {
        comment: ListedGitHubComment,
    },
}

/// What a successful create produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostedGitHubComment {
    pub github_id: u64,
    /// `Some("gh-<id>")` for a recorded line comment or reply; `None` for a
    /// general comment, which is not written to the sidecar.
    pub store_id: Option<String>,
}

/// A create's failure, already worded for a front end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitHubPostError {
    NotPosted(String),
    UnknownOutcome(String),
    InFlight,
    RecordFailed(String),
}

impl std::fmt::Display for GitHubPostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotPosted(m) => write!(f, "Comment not posted: {m}"),
            Self::UnknownOutcome(m) => {
                write!(
                    f,
                    "Comment may have reached GitHub; send again to check: {m}"
                )
            }
            Self::InFlight => write!(f, "A comment post for this draft is already running."),
            Self::RecordFailed(m) => {
                write!(
                    f,
                    "Comment posted to GitHub, but er could not record it: {m}"
                )
            }
        }
    }
}

/// The single message for a refused standalone line comment.
pub const PENDING_REVIEW_BLOCKS_COMMENT: &str =
    "You have a pending review — add this to it or submit the review first";

/// Why a tab known to have no pull request refuses a GitHub-kind comment.
/// GitHub comments post when created, so they need a PR; a question or a note
/// stays private and needs none.
pub const NO_PR_FOR_COMMENT: &str =
    "No pull request for this branch: comments post to GitHub when created. Start a question or a note to keep it private.";

/// A retry must not list before GitHub has finished the first attempt.
pub const UNKNOWN_RETRY_NOT_BEFORE_SECS: u64 = 5;

/// The result of a retry that arrived before its not-before time. `finish`
/// recognises this and leaves the pending record untouched.
pub const RETRY_TOO_EARLY: &str =
    "GitHub is still finishing the previous attempt; send again in a few seconds";

/// Per-process claim on a client id. Drop removes the id, including on an
/// early return before `finish`.
pub struct InFlightGuard {
    client_id: String,
}

impl std::fmt::Debug for InFlightGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InFlightGuard")
            .field("client_id", &self.client_id)
            .finish()
    }
}

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        if let Ok(mut guard) = in_flight().lock() {
            guard.remove(&self.client_id);
        }
    }
}

fn in_flight() -> &'static Mutex<HashSet<String>> {
    static IN_FLIGHT: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    IN_FLIGHT.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Claim `client_id` for one attempt.
pub fn claim_in_flight(client_id: &str) -> Result<InFlightGuard, GitHubPostError> {
    let mut guard = in_flight().lock().map_err(|_| GitHubPostError::InFlight)?;
    if !guard.insert(client_id.to_string()) {
        return Err(GitHubPostError::InFlight);
    }
    Ok(InFlightGuard {
        client_id: client_id.to_string(),
    })
}

/// A stable id for one create intent: `c-<unix millis>-<seq>`.
pub fn new_client_id() -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("c-{millis}-{seq}")
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Find the earlier attempt's comment among the listed ones.
///
/// A match needs the same author (case-insensitive), an attempted body, a
/// timestamp close to the first begin, an id that was not already in the store,
/// and a target that fits the request. `Err` means the pending record cannot
/// be trusted, so nothing may post.
pub fn match_pending_post(
    pending: &PendingGitHubPost,
    request: &GitHubPostRequest,
    viewer_login: &str,
    listed: &[ListedGitHubComment],
) -> Result<Option<ListedGitHubComment>, String> {
    let started = crate::sync::parse_iso8601_secs(&pending.started_at)
        .ok_or_else(|| "The pending post has no valid start time".to_string())?;

    let mut best: Option<(u64, ListedGitHubComment)> = None;
    for comment in listed {
        if pending.known_ids.contains(&comment.id) {
            continue;
        }
        if !comment.author.eq_ignore_ascii_case(viewer_login) {
            continue;
        }
        let body = comment.body.trim_end();
        if !pending.bodies.iter().any(|b| b.trim_end() == body) {
            continue;
        }
        let Some(created) = crate::sync::parse_iso8601_secs(&comment.created_at) else {
            continue;
        };
        if created.saturating_add(60) < started {
            continue;
        }
        let target_matches = match &request.target {
            GitHubPostTarget::Line {
                path,
                line_start,
                line_end,
                side,
            } => {
                comment.path.as_deref() == Some(path.as_str())
                    && comment.in_reply_to_id.is_none()
                    && comment
                        .side
                        .as_deref()
                        .is_none_or(|s| s.eq_ignore_ascii_case(side))
                    && {
                        let current = (comment.start_line.or(comment.line), comment.line);
                        let original = (
                            comment.original_start_line.or(comment.original_line),
                            comment.original_line,
                        );
                        let want = (Some(*line_start), Some(*line_end));
                        current == want || original == want
                    }
            }
            GitHubPostTarget::Reply { in_reply_to } => comment.in_reply_to_id == Some(*in_reply_to),
            GitHubPostTarget::General => true,
        };
        if !target_matches {
            continue;
        }
        if best.as_ref().is_none_or(|(best_created, best_comment)| {
            (created, comment.id) > (*best_created, best_comment.id)
        }) {
            best = Some((created, comment.clone()));
        }
    }
    Ok(best.map(|(_, comment)| comment))
}

/// Reconcile an earlier unknown outcome, then post unless it already landed.
pub fn execute_comment_post(
    poster: &dyn GitHubCommentPoster,
    request: &GitHubPostRequest,
    pending: Option<&PendingGitHubPost>,
) -> GitHubPostExecution {
    if let Some(pending) = pending {
        if now_epoch_secs() < pending.not_before_epoch {
            return GitHubPostExecution::Post(GitHubPostOutcome::UnknownOutcome {
                error: RETRY_TOO_EARLY.to_string(),
            });
        }
        let viewer = match poster.viewer_login(request) {
            Ok(viewer) => viewer,
            Err(e) => {
                return GitHubPostExecution::Post(GitHubPostOutcome::UnknownOutcome {
                    error: format!("{e:#}"),
                })
            }
        };
        let listed = match poster.list(request) {
            Ok(listed) => listed,
            Err(e) => {
                return GitHubPostExecution::Post(GitHubPostOutcome::UnknownOutcome {
                    error: format!("{e:#}"),
                })
            }
        };
        match match_pending_post(pending, request, &viewer, &listed) {
            Ok(Some(comment)) => return GitHubPostExecution::Adopted { comment },
            Ok(None) => {}
            Err(error) => {
                return GitHubPostExecution::Post(GitHubPostOutcome::UnknownOutcome { error })
            }
        }
    }
    GitHubPostExecution::Post(poster.post(request))
}

/// Identity of the tab a create came from, captured before resolving the PR
/// target so a tab switch during the `gh` call cannot post to the wrong tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentPostContext {
    pub repo_root: String,
    pub is_remote: bool,
    pub remote_repo: Option<String>,
    pub pr_number: Option<u64>,
}

/// Resolve the PR to post to. Never called under the App lock.
pub fn resolve_post_target(ctx: &CommentPostContext) -> Result<PrPostTarget, GitHubPostError> {
    if ctx.is_remote {
        let slug = ctx.remote_repo.as_deref().ok_or_else(|| {
            GitHubPostError::NotPosted("No remote repository for this tab".to_string())
        })?;
        let pr = ctx.pr_number.ok_or_else(|| {
            GitHubPostError::NotPosted("No pull request for this tab".to_string())
        })?;
        let (owner, repo) = slug.split_once('/').ok_or_else(|| {
            GitHubPostError::NotPosted(format!("Invalid remote repository slug: {slug}"))
        })?;
        return Ok(PrPostTarget {
            owner: owner.to_string(),
            repo: repo.to_string(),
            pr,
            repo_root: None,
        });
    }
    if let Some(pr) = ctx.pr_number {
        let (owner, repo) = crate::github::get_repo_info(&ctx.repo_root).map_err(|e| {
            GitHubPostError::NotPosted(format!(
                "Could not read the repository for this pull request: {e:#}"
            ))
        })?;
        return Ok(PrPostTarget {
            owner,
            repo,
            pr,
            repo_root: Some(ctx.repo_root.clone()),
        });
    }
    match crate::github::lookup_pr_for_current_branch(&ctx.repo_root) {
        crate::github::PrLookup::Present {
            owner,
            repo,
            number,
        } => Ok(PrPostTarget {
            owner,
            repo,
            pr: number,
            repo_root: Some(ctx.repo_root.clone()),
        }),
        crate::github::PrLookup::Absent => {
            Err(GitHubPostError::NotPosted(NO_PR_FOR_COMMENT.to_string()))
        }
        crate::github::PrLookup::Unknown(e) => Err(GitHubPostError::NotPosted(format!(
            "Could not read this branch's pull request: {e}"
        ))),
    }
}

fn is_http_4xx(error: &anyhow::Error) -> bool {
    format!("{error:#}").contains("(HTTP 4")
}

fn classify_post_error(error: &anyhow::Error) -> GitHubPostOutcome {
    let message = format!("{error:#}");
    if message.to_lowercase().contains("one pending review") {
        GitHubPostOutcome::NotPosted {
            error: PENDING_REVIEW_BLOCKS_COMMENT.to_string(),
        }
    } else if is_http_4xx(error) {
        GitHubPostOutcome::NotPosted { error: message }
    } else {
        GitHubPostOutcome::UnknownOutcome { error: message }
    }
}

fn listed_from_github(comment: github::GitHubComment) -> ListedGitHubComment {
    ListedGitHubComment {
        id: comment.id,
        body: comment.body,
        author: comment.user.login,
        created_at: comment.created_at,
        path: comment.path,
        line: comment.line,
        start_line: comment.start_line,
        original_line: comment.original_line,
        original_start_line: comment.original_start_line,
        side: comment.side,
        in_reply_to_id: comment.in_reply_to_id,
    }
}

fn listed_from_issue(comment: github::IssueComment) -> ListedGitHubComment {
    ListedGitHubComment {
        id: comment.id,
        body: comment.body,
        author: comment.user.login,
        created_at: comment.created_at,
        path: None,
        line: None,
        start_line: None,
        original_line: None,
        original_start_line: None,
        side: None,
        in_reply_to_id: None,
    }
}

impl GitHubCommentPoster for GhCommentPoster {
    fn post(&self, request: &GitHubPostRequest) -> GitHubPostOutcome {
        let pr = &request.pr;
        let repo_root = pr.repo_root.as_deref();
        match &request.target {
            GitHubPostTarget::Line {
                path,
                line_start,
                line_end,
                side,
            } => {
                // A pending review makes GitHub attach (or refuse) a standalone
                // line comment; refuse before sending anything. A failed check
                // proceeds, because the POST itself still reports a refusal.
                if let Ok(pending) = github::gh_pending_review(&pr.owner, &pr.repo, pr.pr) {
                    if pending.review_id.is_some() {
                        return GitHubPostOutcome::NotPosted {
                            error: PENDING_REVIEW_BLOCKS_COMMENT.to_string(),
                        };
                    }
                }
                let commit_id = match github::gh_pr_head_oid(&pr.owner, &pr.repo, pr.pr, repo_root)
                {
                    Ok(commit_id) => commit_id,
                    Err(e) => {
                        return GitHubPostOutcome::NotPosted {
                            error: format!("{e:#}"),
                        }
                    }
                };
                let comment = github::NewLineComment {
                    path,
                    line_start: *line_start,
                    line_end: Some(*line_end),
                    body: &request.body,
                    side,
                };
                match github::gh_pr_post_line_comment(
                    &pr.owner, &pr.repo, pr.pr, comment, &commit_id, repo_root,
                ) {
                    Ok(created) => GitHubPostOutcome::Posted {
                        github_id: created.id,
                        created_at: created.created_at,
                        author: created.author,
                    },
                    Err(e) => classify_post_error(&e),
                }
            }
            GitHubPostTarget::Reply { in_reply_to } => {
                match github::gh_pr_post_reply(
                    &pr.owner,
                    &pr.repo,
                    pr.pr,
                    *in_reply_to,
                    &request.body,
                    repo_root,
                ) {
                    Ok(created) => GitHubPostOutcome::Posted {
                        github_id: created.id,
                        created_at: created.created_at,
                        author: created.author,
                    },
                    Err(e) => classify_post_error(&e),
                }
            }
            GitHubPostTarget::General => {
                match github::gh_pr_post_general(
                    &pr.owner,
                    &pr.repo,
                    pr.pr,
                    &request.body,
                    repo_root,
                ) {
                    Ok(created) => GitHubPostOutcome::Posted {
                        github_id: created.id,
                        created_at: created.created_at,
                        author: created.author,
                    },
                    Err(e) => classify_post_error(&e),
                }
            }
        }
    }

    fn list(&self, request: &GitHubPostRequest) -> Result<Vec<ListedGitHubComment>> {
        let pr = &request.pr;
        let repo_root = pr.repo_root.as_deref();
        let comments = match &request.target {
            GitHubPostTarget::General => {
                github::gh_pr_issue_comments(&pr.owner, &pr.repo, pr.pr, repo_root)?
                    .into_iter()
                    .map(listed_from_issue)
                    .collect()
            }
            _ => match repo_root {
                Some(root) => github::gh_pr_comments(&pr.owner, &pr.repo, pr.pr, root)?
                    .into_iter()
                    .map(listed_from_github)
                    .collect(),
                None => github::gh_pr_comments_remote(&pr.owner, &pr.repo, pr.pr)?
                    .into_iter()
                    .map(listed_from_github)
                    .collect(),
            },
        };
        Ok(comments)
    }

    fn viewer_login(&self, request: &GitHubPostRequest) -> Result<String> {
        github::gh_viewer_login(request.pr.repo_root.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_post_error_marks_a_4xx_as_not_posted() {
        let error = anyhow::anyhow!("gh: Unprocessable Entity (HTTP 422)");
        match classify_post_error(&error) {
            GitHubPostOutcome::NotPosted { error } => {
                assert!(error.contains("HTTP 422"), "{error}");
            }
            other => panic!("expected NotPosted, got {other:?}"),
        }
    }

    #[test]
    fn classify_post_error_marks_a_pending_review_refusal_as_not_posted() {
        let error = anyhow::anyhow!("user_id can only have one pending review per pull request");
        assert_eq!(
            classify_post_error(&error),
            GitHubPostOutcome::NotPosted {
                error: PENDING_REVIEW_BLOCKS_COMMENT.to_string()
            }
        );
    }

    #[test]
    fn classify_post_error_marks_a_timeout_as_unknown() {
        let error = anyhow::anyhow!("request timed out");
        assert!(matches!(
            classify_post_error(&error),
            GitHubPostOutcome::UnknownOutcome { .. }
        ));
    }
}
