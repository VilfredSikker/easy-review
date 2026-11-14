//! The state machine behind creating a GitHub comment.
//!
//! A create runs in three phases so no `gh` call holds a caller's lock:
//! [`PostSession::begin`] captures what the record needs and claims the client
//! id in flight, the caller runs [`crate::github_post::execute_comment_post`]
//! outside the lock, and [`PostSession::finish`] records the outcome. A retry
//! reuses the client id so the pending attempt is reconciled instead of posting
//! twice.
//!
//! The App and the workspace actor each keep a session: the state here is the
//! retry map and the ticket, not either front end's tab.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::ai;
use crate::git::DiffFile;
use crate::github_post::{
    claim_in_flight, CommentPostContext, GitHubPostError, GitHubPostExecution, GitHubPostOutcome,
    GitHubPostRequest, GitHubPostTarget, InFlightGuard, PendingGitHubPost, PostedGitHubComment,
    PrPostTarget, RETRY_TOO_EARLY, UNKNOWN_RETRY_NOT_BEFORE_SECS,
};
use crate::sync::chrono_now;

/// A comment the user just created, ready to become a GitHub request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NewGitHubComment {
    Line {
        file: String,
        hunk_idx: usize,
        line_start: Option<usize>,
        line_end: Option<usize>,
        side: Option<String>,
        body: String,
    },
    Reply {
        parent_id: String,
        body: String,
    },
    General {
        body: String,
    },
}

impl NewGitHubComment {
    /// A line comment intent. The service's routed create builds through this
    /// boundary, so `er::comment_boundary` keeps one construction site.
    pub fn line(
        file: String,
        hunk_idx: usize,
        line_start: Option<usize>,
        line_end: Option<usize>,
        side: Option<String>,
        body: String,
    ) -> Self {
        Self::Line {
            file,
            hunk_idx,
            line_start,
            line_end,
            side,
            body,
        }
    }

    /// A reply intent.
    pub fn reply(parent_id: String, body: String) -> Self {
        Self::Reply { parent_id, body }
    }

    /// A PR-wide comment intent.
    pub fn general(body: String) -> Self {
        Self::General { body }
    }
}

impl From<crate::ai::promote::PromotedComment> for NewGitHubComment {
    fn from(promoted: crate::ai::promote::PromotedComment) -> Self {
        Self::Line {
            file: promoted.path().to_string(),
            hunk_idx: promoted.hunk_index().unwrap_or(0),
            line_start: Some(promoted.line_start()),
            line_end: promoted.line_end(),
            side: Some(promoted.side().to_string()),
            body: promoted.body().to_string(),
        }
    }
}

/// Where a posted comment belongs in `github-comments.json`.
#[derive(Debug, Clone, Default)]
pub struct PostedGitHubCommentLocation {
    pub file: String,
    pub hunk_index: Option<usize>,
    pub line_start: Option<usize>,
    pub line_end: Option<usize>,
    pub line_content: String,
    pub context_before: Vec<String>,
    pub context_after: Vec<String>,
    pub old_line_start: Option<usize>,
    pub hunk_header: String,
    pub side: String,
    pub in_reply_to: Option<String>,
}

/// Everything one create needs between `begin` and `finish`. The in-flight
/// guard is not `Clone`: cloning it would release the client id twice.
#[derive(Debug)]
pub struct GitHubPostTicket {
    pub request: GitHubPostRequest,
    pub pending: Option<PendingGitHubPost>,
    pub tab: CommentPostContext,
    pub comments_path: String,
    pub branch_diff_hash: String,
    pub diff_hash: String,
    pub started_at: String,
    pub known_ids: HashSet<u64>,
    pub location: PostedGitHubCommentLocation,
    _in_flight: InFlightGuard,
}

/// What recording a create changed outside the retry map.
#[derive(Debug)]
pub struct PostRecord {
    pub posted: PostedGitHubComment,
    /// The sidecar contents after the write; `None` for a general comment,
    /// which is not recorded.
    pub comments: Option<ai::ErGithubComments>,
    /// The sidecar path to mark as written; `None` for a general comment.
    pub comments_path: Option<PathBuf>,
}

/// The inputs one begin needs, captured by the caller before the `gh` call.
#[derive(Debug)]
pub struct PostBegin<'a> {
    /// The caller's active tab identity, read at begin time.
    pub active: CommentPostContext,
    /// The identity captured before the PR target was resolved.
    pub ctx: CommentPostContext,
    pub pr: PrPostTarget,
    /// The loaded GitHub comments, for the reply parent and the known ids.
    pub comments: Option<&'a ai::ErGithubComments>,
    /// The diff file the line anchor is read from; `None` for a reply or a
    /// general comment, or when the file is no longer in the view.
    pub diff_file: Option<&'a DiffFile>,
    /// Bucket the comment sidecar is written into.
    pub comments_dir: &'a str,
    pub branch_diff_hash: String,
    pub diff_hash: String,
    /// The id resolved by the caller: the painted id, else the composer's id,
    /// else a fresh one. Must not be empty.
    pub client_id: String,
}

/// The retry state a caller keeps across creates.
#[derive(Debug, Default)]
pub struct PostSession {
    pending: HashMap<String, PendingGitHubPost>,
}

impl PostSession {
    pub fn from_pending(pending: HashMap<String, PendingGitHubPost>) -> Self {
        Self { pending }
    }

    pub fn into_pending(self) -> HashMap<String, PendingGitHubPost> {
        self.pending
    }

    /// Keep a posted create reconcilable after a later step failed (for
    /// example a promote whose source deletion failed), so the next send
    /// adopts the comment instead of posting a second copy.
    pub fn remember_posted_for_retry(&mut self, request: GitHubPostRequest) {
        self.pending.insert(
            request.client_id.clone(),
            PendingGitHubPost {
                started_at: chrono_now(),
                not_before_epoch: 0,
                known_ids: HashSet::new(),
                bodies: vec![request.body.clone()],
                request,
            },
        );
    }

    /// Capture everything a create needs before the `gh` call. No subprocess.
    pub fn begin(
        &mut self,
        intent: NewGitHubComment,
        input: PostBegin<'_>,
    ) -> Result<GitHubPostTicket, GitHubPostError> {
        if input.active != input.ctx {
            return Err(GitHubPostError::NotPosted(
                "The active tab changed while the comment was being prepared".to_string(),
            ));
        }
        if input.client_id.is_empty() {
            return Err(GitHubPostError::NotPosted(
                "The comment has no client id".to_string(),
            ));
        }

        let body = match &intent {
            NewGitHubComment::Line { body, .. }
            | NewGitHubComment::Reply { body, .. }
            | NewGitHubComment::General { body } => body.trim().to_string(),
        };
        if body.is_empty() {
            return Err(GitHubPostError::NotPosted(
                "Comment body cannot be empty".to_string(),
            ));
        }

        let (target, location) = match &intent {
            NewGitHubComment::Line {
                file,
                hunk_idx,
                line_start,
                line_end,
                side,
                ..
            } => {
                let line = line_start.unwrap_or(1);
                let end = line_end.filter(|end| *end > line).unwrap_or(line);
                let side = side.clone().unwrap_or_else(|| "RIGHT".to_string());
                if side != "LEFT" && side != "RIGHT" {
                    return Err(GitHubPostError::NotPosted(format!(
                        "Unsupported comment side: {side}"
                    )));
                }
                let anchor = crate::annotations::line_anchor_at(
                    input.diff_file,
                    *hunk_idx,
                    *line_start,
                    Some(&side),
                );
                let location = PostedGitHubCommentLocation {
                    file: file.clone(),
                    hunk_index: Some(*hunk_idx),
                    line_start: anchor.line_start,
                    line_end: match (anchor.line_start, *line_end) {
                        (Some(start), Some(end)) if end > start => Some(end),
                        _ => None,
                    },
                    line_content: anchor.line_content,
                    context_before: anchor.context_before,
                    context_after: anchor.context_after,
                    old_line_start: anchor.old_line_start,
                    hunk_header: anchor.hunk_header,
                    side: side.clone(),
                    in_reply_to: None,
                };
                (
                    GitHubPostTarget::Line {
                        path: file.clone(),
                        line_start: line,
                        line_end: end,
                        side,
                    },
                    location,
                )
            }
            NewGitHubComment::Reply { parent_id, .. } => {
                let parent = input
                    .comments
                    .and_then(|comments| comments.comments.iter().find(|c| c.id == *parent_id))
                    .cloned()
                    .ok_or_else(|| {
                        GitHubPostError::NotPosted("The reply target no longer exists".to_string())
                    })?;
                let parent_github = parent.github_id.ok_or_else(|| {
                    GitHubPostError::NotPosted("The reply target is not on GitHub".to_string())
                })?;
                let location = PostedGitHubCommentLocation {
                    file: parent.file.clone(),
                    hunk_index: parent.hunk_index,
                    line_start: parent.line_start,
                    line_end: parent.line_end,
                    line_content: parent.line_content.clone(),
                    context_before: parent.context_before.clone(),
                    context_after: parent.context_after.clone(),
                    old_line_start: parent.old_line_start,
                    hunk_header: parent.hunk_header.clone(),
                    side: parent.side.clone(),
                    in_reply_to: Some(format!("gh-{parent_github}")),
                };
                (
                    GitHubPostTarget::Reply {
                        in_reply_to: parent_github,
                    },
                    location,
                )
            }
            NewGitHubComment::General { .. } => (
                GitHubPostTarget::General,
                PostedGitHubCommentLocation {
                    side: "RIGHT".to_string(),
                    ..Default::default()
                },
            ),
        };

        let client_id = input.client_id;
        let pending = self.pending.get(&client_id).cloned();
        let known_ids = pending
            .as_ref()
            .map(|pending| pending.known_ids.clone())
            .unwrap_or_else(|| known_github_ids(input.comments, input.comments_dir));

        let store = crate::store_registry::store();
        let scope = er_store::Scope::dir(input.comments_dir)
            .map_err(|e| GitHubPostError::NotPosted(format!("Invalid comments path: {e}")))?;
        let comments_path = ai::annotation_store::path_of::<ai::ErGithubComments>(&store, &scope)
            .map_err(|e| GitHubPostError::NotPosted(format!("Invalid comments path: {e}")))?
            .to_string_lossy()
            .into_owned();

        let guard = claim_in_flight(&client_id)?;
        Ok(GitHubPostTicket {
            request: GitHubPostRequest {
                client_id,
                pr: input.pr,
                target,
                body,
            },
            pending,
            tab: input.ctx,
            comments_path,
            branch_diff_hash: input.branch_diff_hash,
            diff_hash: input.diff_hash,
            started_at: chrono_now(),
            known_ids,
            location,
            _in_flight: guard,
        })
    }

    /// Record a create's outcome. A general comment is not recorded: the pull
    /// never lists issue comments, so it would vanish on the next pull.
    pub fn finish(
        &mut self,
        ticket: GitHubPostTicket,
        execution: GitHubPostExecution,
    ) -> Result<PostRecord, GitHubPostError> {
        match execution {
            GitHubPostExecution::Post(GitHubPostOutcome::UnknownOutcome { error }) => {
                if error != RETRY_TOO_EARLY {
                    self.remember_unknown_outcome(&ticket);
                }
                Err(GitHubPostError::UnknownOutcome(error))
            }
            GitHubPostExecution::Post(GitHubPostOutcome::NotPosted { error }) => {
                Err(GitHubPostError::NotPosted(error))
            }
            GitHubPostExecution::Post(GitHubPostOutcome::Posted {
                github_id,
                created_at,
                author,
            }) => self.record_posted(
                &ticket,
                github_id,
                created_at,
                author,
                ticket.request.body.clone(),
                false,
            ),
            GitHubPostExecution::Adopted { comment } => {
                let kept_earlier_body = comment.body.trim_end() != ticket.request.body.trim_end();
                self.record_posted(
                    &ticket,
                    comment.id,
                    comment.created_at,
                    comment.author,
                    comment.body,
                    kept_earlier_body,
                )
            }
        }
    }

    fn remember_unknown_outcome(&mut self, ticket: &GitHubPostTicket) {
        let not_before_epoch = now_epoch_secs() + UNKNOWN_RETRY_NOT_BEFORE_SECS;
        let client_id = ticket.request.client_id.clone();
        match self.pending.get_mut(&client_id) {
            Some(pending) => {
                if !pending
                    .bodies
                    .iter()
                    .any(|body| body.trim_end() == ticket.request.body.trim_end())
                {
                    pending.bodies.push(ticket.request.body.clone());
                }
                pending.request = ticket.request.clone();
                pending.not_before_epoch = not_before_epoch;
            }
            None => {
                self.pending.insert(
                    client_id,
                    PendingGitHubPost {
                        started_at: ticket.started_at.clone(),
                        not_before_epoch,
                        known_ids: ticket.known_ids.clone(),
                        bodies: vec![ticket.request.body.clone()],
                        request: ticket.request.clone(),
                    },
                );
            }
        }
    }

    fn record_posted(
        &mut self,
        ticket: &GitHubPostTicket,
        github_id: u64,
        created_at: String,
        author: String,
        body: String,
        kept_earlier_body: bool,
    ) -> Result<PostRecord, GitHubPostError> {
        if matches!(ticket.request.target, GitHubPostTarget::General) {
            self.pending.remove(&ticket.request.client_id);
            return Ok(PostRecord {
                posted: PostedGitHubComment {
                    github_id,
                    store_id: None,
                    kept_earlier_body,
                },
                comments: None,
                comments_path: None,
            });
        }

        let store = crate::store_registry::store();
        let Some(comments_dir) = std::path::Path::new(&ticket.comments_path).parent() else {
            return Err(GitHubPostError::RecordFailed(format!(
                "invalid comments path: {}",
                ticket.comments_path
            )));
        };
        let scope = er_store::Scope::dir(comments_dir)
            .map_err(|e| GitHubPostError::RecordFailed(format!("{e}")))?;
        let comments_path = ai::annotation_store::path_of::<ai::ErGithubComments>(&store, &scope)
            .map_err(|e| GitHubPostError::RecordFailed(format!("{e}")))?;
        let store_id = format!("gh-{github_id}");
        let location = &ticket.location;
        let new_comment = ai::GithubComment {
            id: store_id.clone(),
            timestamp: created_at,
            file: location.file.clone(),
            hunk_index: location.hunk_index,
            line_start: location.line_start,
            line_end: location.line_end,
            line_content: location.line_content.clone(),
            comment: body,
            in_reply_to: location.in_reply_to.clone(),
            resolved: false,
            source: "github".to_string(),
            github_id: Some(github_id),
            author,
            outdated: false,
            stale: false,
            context_before: location.context_before.clone(),
            context_after: location.context_after.clone(),
            old_line_start: location.old_line_start,
            hunk_header: location.hunk_header.clone(),
            anchor_status: "original".to_string(),
            relocated_at_hash: ticket.diff_hash.clone(),
            finding_ref: None,
            side: location.side.clone(),
        };
        let diff_hash_for_write = ticket.branch_diff_hash.clone();
        let written =
            ai::annotation_store::update_github_comments(&store, &scope, move |current| {
                let mut gc = current.unwrap_or_else(|| ai::ErGithubComments {
                    version: 1,
                    diff_hash: diff_hash_for_write.clone(),
                    github: None,
                    comments: Vec::new(),
                });
                gc.diff_hash = diff_hash_for_write;
                if !gc.comments.iter().any(|c| c.github_id == Some(github_id)) {
                    gc.comments.push(new_comment);
                }
                Ok::<_, std::convert::Infallible>(gc)
            });
        let gc = match written {
            Ok(updated) => updated.value,
            Err(e) => {
                // GitHub holds the comment, so the retry must reconcile rather
                // than post it again. Keep an existing record: replacing it would
                // discard the first attempt's start time and bodies, and a later
                // retry could miss the earlier comment. Only the wait is cleared,
                // because the comment already exists.
                match self.pending.get_mut(&ticket.request.client_id) {
                    Some(pending) => pending.not_before_epoch = 0,
                    None => {
                        self.pending.insert(
                            ticket.request.client_id.clone(),
                            PendingGitHubPost {
                                started_at: ticket.started_at.clone(),
                                not_before_epoch: 0,
                                known_ids: ticket.known_ids.clone(),
                                bodies: vec![ticket.request.body.clone()],
                                request: ticket.request.clone(),
                            },
                        );
                    }
                }
                return Err(GitHubPostError::RecordFailed(format!("{e:#}")));
            }
        };

        self.pending.remove(&ticket.request.client_id);
        Ok(PostRecord {
            posted: PostedGitHubComment {
                github_id,
                store_id: Some(store_id),
                kept_earlier_body,
            },
            comments: Some(gc),
            comments_path: Some(comments_path),
        })
    }
}

/// The ids already in the bucket's comment sidecar, falling back to the
/// loaded payload when the file cannot be read.
pub fn known_github_ids(
    comments: Option<&ai::ErGithubComments>,
    comments_dir: &str,
) -> HashSet<u64> {
    let store = crate::store_registry::store();
    let from_file = er_store::Scope::dir(comments_dir).ok().and_then(|scope| {
        match ai::annotation_store::load_github_comments(&store, &scope) {
            Ok(er_store::Loaded::Found(stored)) => Some(
                stored
                    .value
                    .comments
                    .iter()
                    .filter_map(|c| c.github_id)
                    .collect(),
            ),
            _ => None,
        }
    });
    if let Some(ids) = from_file {
        return ids;
    }
    comments
        .map(|gc| gc.comments.iter().filter_map(|c| c.github_id).collect())
        .unwrap_or_default()
}

fn now_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
