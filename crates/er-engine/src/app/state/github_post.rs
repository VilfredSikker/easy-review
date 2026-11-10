//! Posting a GitHub comment at creation: the state half of
//! [`crate::github_post`].
//!
//! A create runs in three phases so no `gh` call holds the `App` lock:
//! `begin_github_comment_post` captures what the record needs, the caller runs
//! `execute_comment_post` outside the lock, and `finish_github_comment_post`
//! writes the record. A retry reuses the client id so the pending attempt is
//! reconciled instead of posting twice.

use std::collections::HashSet;
use std::sync::Arc;

use crate::ai;
use crate::github_post::{
    claim_in_flight, new_client_id, CommentPostContext, GitHubCommentPoster, GitHubPostError,
    GitHubPostExecution, GitHubPostOutcome, GitHubPostRequest, GitHubPostTarget, InFlightGuard,
    PendingGitHubPost, PostedGitHubComment, PrPostTarget, RETRY_TOO_EARLY,
    UNKNOWN_RETRY_NOT_BEFORE_SECS,
};

use tui_textarea::TextArea;

use super::{chrono_now, App, CommentType, InputMode, LineAnchor};

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

impl App {
    /// Identity of the active tab, captured before resolving the PR target.
    pub fn comment_post_context(&self) -> CommentPostContext {
        let tab = self.tab();
        CommentPostContext {
            repo_root: tab.repo_root.clone(),
            is_remote: tab.is_remote(),
            remote_repo: tab.remote_repo.clone(),
            pr_number: tab.pr_number,
        }
    }

    pub fn github_comment_poster(&self) -> Arc<dyn GitHubCommentPoster> {
        Arc::clone(&self.comment_poster)
    }

    pub fn set_github_comment_poster(&mut self, poster: Arc<dyn GitHubCommentPoster>) {
        self.comment_poster = poster;
    }

    /// Capture everything a create needs before the `gh` call. No subprocess.
    pub fn begin_github_comment_post(
        &mut self,
        intent: NewGitHubComment,
        pr: PrPostTarget,
        ctx: CommentPostContext,
        client_id: Option<String>,
    ) -> Result<GitHubPostTicket, GitHubPostError> {
        if self.comment_post_context() != ctx {
            return Err(GitHubPostError::NotPosted(
                "The active tab changed while the comment was being prepared".to_string(),
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
                let anchor = self.line_anchor_at(file, *hunk_idx, *line_start, Some(&side));
                let location = PostedGitHubCommentLocation {
                    file: file.clone(),
                    hunk_index: Some(*hunk_idx),
                    line_start: anchor.line_start,
                    line_end: Self::normalize_line_end(anchor.line_start, *line_end),
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
                let parent = self
                    .tab()
                    .ai
                    .github_comments
                    .as_ref()
                    .and_then(|gc| gc.comments.iter().find(|c| c.id == *parent_id))
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

        let client_id = match client_id.filter(|id| !id.is_empty()) {
            Some(id) => id,
            None => match self.tab().comment_client_id.clone() {
                Some(id) => id,
                None => {
                    let id = new_client_id();
                    self.tab_mut().comment_client_id = Some(id.clone());
                    id
                }
            },
        };

        let pending = self.pending_github_posts.get(&client_id).cloned();
        let known_ids = pending
            .as_ref()
            .map(|pending| pending.known_ids.clone())
            .unwrap_or_else(|| self.known_github_ids());

        let (comments_path, branch_diff_hash, diff_hash) = {
            let tab = self.tab();
            let store = crate::store_registry::store();
            let comments_dir = tab.github_comments_dir();
            let scope = er_store::Scope::dir(comments_dir.as_str())
                .map_err(|e| GitHubPostError::NotPosted(format!("Invalid comments path: {e}")))?;
            (
                crate::ai::annotation_store::path_of::<ai::ErGithubComments>(&store, &scope)
                    .map_err(|e| GitHubPostError::NotPosted(format!("Invalid comments path: {e}")))?
                    .to_string_lossy()
                    .into_owned(),
                tab.branch_diff_hash.clone(),
                tab.diff_hash.clone(),
            )
        };

        let guard = claim_in_flight(&client_id)?;
        Ok(GitHubPostTicket {
            request: GitHubPostRequest {
                client_id,
                pr,
                target,
                body,
            },
            pending,
            tab: ctx,
            comments_path,
            branch_diff_hash,
            diff_hash,
            started_at: chrono_now(),
            known_ids,
            location,
            _in_flight: guard,
        })
    }

    /// Record a create's outcome. A general comment is not recorded: the pull
    /// never lists issue comments, so it would vanish on the next pull.
    pub fn finish_github_comment_post(
        &mut self,
        ticket: GitHubPostTicket,
        execution: GitHubPostExecution,
    ) -> Result<PostedGitHubComment, GitHubPostError> {
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
            }) => self.record_posted_comment(
                &ticket,
                github_id,
                created_at,
                author,
                ticket.request.body.clone(),
                false,
            ),
            GitHubPostExecution::Adopted { comment } => {
                let kept_earlier_body = comment.body.trim_end() != ticket.request.body.trim_end();
                self.record_posted_comment(
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

    /// The comment the open composer would post, if it is a GitHub-kind create
    /// that must not stay local: edits, finding replies, questions, notes,
    /// empty text and replies to local-only parents return `None`.
    pub fn github_comment_intent(&self) -> Option<NewGitHubComment> {
        let tab = self.tab();
        if tab.comment_edit_id.is_some() || tab.comment_finding_ref.is_some() {
            return None;
        }
        if tab.comment_type != CommentType::GitHubComment {
            return None;
        }
        let body = tab.comment_text();
        if body.trim().is_empty() {
            return None;
        }
        if let Some(parent_id) = tab.comment_reply_to.clone() {
            let parent = tab
                .ai
                .github_comments
                .as_ref()?
                .comments
                .iter()
                .find(|c| c.id == parent_id)?;
            parent.github_id?;
            return Some(NewGitHubComment::Reply { parent_id, body });
        }
        if tab.comment_file.is_empty() {
            return Some(NewGitHubComment::General { body });
        }
        Some(NewGitHubComment::Line {
            file: tab.comment_file.clone(),
            hunk_idx: tab.comment_hunk,
            line_start: tab.comment_line_num,
            line_end: tab.comment_line_end,
            side: tab.comment_side.clone(),
            body,
        })
    }

    /// TUI submit: post the open composer's comment now.
    pub fn post_github_comment_now(&mut self) -> Result<PostedGitHubComment, GitHubPostError> {
        let intent = self
            .github_comment_intent()
            .ok_or_else(|| GitHubPostError::NotPosted("Nothing to post to GitHub".to_string()))?;
        let ctx = self.comment_post_context();
        let pr = crate::github_post::resolve_post_target_with(
            &*self.ports.github,
            &*self.ports.git,
            &ctx,
        )?;
        let ticket = self.begin_github_comment_post(intent, pr, ctx, None)?;
        let poster = self.github_comment_poster();
        let execution = crate::github_post::execute_comment_post(
            poster.as_ref(),
            &ticket.request,
            ticket.pending.as_ref(),
        );
        let posted = self.finish_github_comment_post(ticket, execution);
        match &posted {
            Ok(posted) if posted.kept_earlier_body => {
                // Keep the edited text: sending it again is a deliberate new
                // comment, under a new client id.
                self.tab_mut().comment_client_id = None;
                self.notify("Your earlier version was posted; this edit was not");
            }
            Ok(_) => {
                let tab = self.tab_mut();
                tab.comment_textarea = TextArea::default();
                tab.comment_client_id = None;
                self.input_mode = InputMode::Normal;
                self.notify("Comment posted to GitHub");
            }
            Err(_) => {}
        }
        posted
    }

    /// Keep a posted create reconcilable after a later step failed (for
    /// example a promote whose source deletion failed), so the next send
    /// adopts the comment instead of posting a second copy.
    pub fn remember_posted_for_retry(&mut self, request: GitHubPostRequest) {
        self.pending_github_posts.insert(
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

    /// `get_line_anchor` with the location passed in; the composer fields are
    /// not read.
    pub(crate) fn line_anchor_at(
        &self,
        file: &str,
        hunk_idx: usize,
        line: Option<usize>,
        side: Option<&str>,
    ) -> LineAnchor {
        let tab = self.tab();
        let diff_file = if file.is_empty() {
            tab.selected_diff_file()
        } else if tab.mode == super::DiffMode::History {
            tab.history
                .as_ref()
                .and_then(|history| history.commit_files.iter().find(|f| f.path == file))
        } else {
            tab.files.iter().find(|f| f.path == file)
        };

        let Some(df) = diff_file else {
            return LineAnchor::default();
        };
        let Some(hunk) = df.hunks.get(hunk_idx) else {
            return LineAnchor::default();
        };
        let Some(ln) = line else {
            return LineAnchor {
                line_start: None,
                line_content: hunk.header.clone(),
                context_before: Vec::new(),
                context_after: Vec::new(),
                old_line_start: None,
                hunk_header: hunk.header.clone(),
            };
        };

        let target_idx = match side {
            Some("LEFT") => hunk
                .lines
                .iter()
                .position(|l| l.old_num == Some(ln))
                .or_else(|| hunk.lines.iter().position(|l| l.new_num == Some(ln))),
            Some("RIGHT") => hunk
                .lines
                .iter()
                .position(|l| l.new_num == Some(ln))
                .or_else(|| hunk.lines.iter().position(|l| l.old_num == Some(ln))),
            _ => hunk
                .lines
                .iter()
                .position(|l| l.new_num == Some(ln))
                .or_else(|| hunk.lines.iter().position(|l| l.old_num == Some(ln))),
        };
        let (line_content, old_line_start) = match target_idx {
            Some(idx) => {
                let dl = &hunk.lines[idx];
                (dl.content.clone(), dl.old_num)
            }
            None => (String::new(), None),
        };
        let context_before = match target_idx {
            Some(idx) => {
                let start = idx.saturating_sub(3);
                hunk.lines[start..idx]
                    .iter()
                    .map(|l| l.content.clone())
                    .collect()
            }
            None => Vec::new(),
        };
        let context_after = match target_idx {
            Some(idx) => {
                let end = (idx + 4).min(hunk.lines.len());
                hunk.lines[(idx + 1)..end]
                    .iter()
                    .map(|l| l.content.clone())
                    .collect()
            }
            None => Vec::new(),
        };

        LineAnchor {
            line_start: Some(ln),
            line_content,
            context_before,
            context_after,
            old_line_start,
            hunk_header: hunk.header.clone(),
        }
    }

    fn known_github_ids(&self) -> HashSet<u64> {
        let tab = self.tab();
        let comments_dir = tab.github_comments_dir();
        let store = crate::store_registry::store();
        let from_file = er_store::Scope::dir(comments_dir.as_str())
            .ok()
            .and_then(|scope| {
                match crate::ai::annotation_store::load_github_comments(&store, &scope) {
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
        tab.ai
            .github_comments
            .as_ref()
            .map(|gc| gc.comments.iter().filter_map(|c| c.github_id).collect())
            .unwrap_or_default()
    }

    fn remember_unknown_outcome(&mut self, ticket: &GitHubPostTicket) {
        let not_before_epoch = now_epoch_secs() + UNKNOWN_RETRY_NOT_BEFORE_SECS;
        let client_id = ticket.request.client_id.clone();
        match self.pending_github_posts.get_mut(&client_id) {
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
                self.pending_github_posts.insert(
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

    fn record_posted_comment(
        &mut self,
        ticket: &GitHubPostTicket,
        github_id: u64,
        created_at: String,
        author: String,
        body: String,
        kept_earlier_body: bool,
    ) -> Result<PostedGitHubComment, GitHubPostError> {
        if matches!(ticket.request.target, GitHubPostTarget::General) {
            self.pending_github_posts.remove(&ticket.request.client_id);
            self.ports.github.invalidate_comments();
            return Ok(PostedGitHubComment {
                github_id,
                store_id: None,
                kept_earlier_body,
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
        let comments_path =
            crate::ai::annotation_store::path_of::<ai::ErGithubComments>(&store, &scope)
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
            crate::ai::annotation_store::update_github_comments(&store, &scope, move |current| {
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
                match self.pending_github_posts.get_mut(&ticket.request.client_id) {
                    Some(pending) => pending.not_before_epoch = 0,
                    None => {
                        self.pending_github_posts.insert(
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

        self.pending_github_posts.remove(&ticket.request.client_id);
        if let Some(tab) = self.tabs.iter_mut().find(|t| {
            t.repo_root == ticket.tab.repo_root
                && t.pr_number == ticket.tab.pr_number
                && t.is_remote() == ticket.tab.is_remote
        }) {
            tab.ai.github_comments = Some(gc);
            tab.ai.rebuild_comment_index();
            tab.mark_sidecar_written(comments_path.to_string_lossy().as_ref());
            if tab.comment_client_id.as_deref() == Some(ticket.request.client_id.as_str()) {
                tab.comment_client_id = None;
            }
        }
        self.ports.github.invalidate_comments();
        Ok(PostedGitHubComment {
            github_id,
            store_id: Some(store_id),
            kept_earlier_body,
        })
    }
}

fn now_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github_post::{
        execute_comment_post, GitHubCommentPoster, GitHubPostOutcome, GitHubPostRequest,
        GitHubPostTarget, ListedGitHubComment, PrPostTarget, RETRY_TOO_EARLY,
    };
    use std::collections::{HashMap, VecDeque};
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
    use std::sync::Mutex;

    struct FakeCommentPoster {
        outcomes: Mutex<VecDeque<GitHubPostOutcome>>,
        listed: Mutex<Vec<ListedGitHubComment>>,
        posted: Mutex<Vec<GitHubPostRequest>>,
        list_calls: AtomicUsize,
        viewer_calls: AtomicUsize,
        viewer: String,
        record_unknown_as_listed: AtomicBool,
        next_id: AtomicU64,
    }

    impl FakeCommentPoster {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                outcomes: Mutex::new(VecDeque::new()),
                listed: Mutex::new(Vec::new()),
                posted: Mutex::new(Vec::new()),
                list_calls: AtomicUsize::new(0),
                viewer_calls: AtomicUsize::new(0),
                viewer: "viewer".to_string(),
                record_unknown_as_listed: AtomicBool::new(false),
                next_id: AtomicU64::new(100),
            })
        }

        fn push_outcome(&self, outcome: GitHubPostOutcome) {
            self.outcomes.lock().unwrap().push_back(outcome);
        }

        fn add_listed(&self, comment: ListedGitHubComment) {
            self.listed.lock().unwrap().push(comment);
        }

        fn posts(&self) -> usize {
            self.posted.lock().unwrap().len()
        }
    }

    fn listed_for(request: &GitHubPostRequest, id: u64, author: &str) -> ListedGitHubComment {
        let (path, line, start_line, side, in_reply_to) = match &request.target {
            GitHubPostTarget::Line {
                path,
                line_start,
                line_end,
                side,
            } => (
                Some(path.clone()),
                Some(*line_end),
                (line_end > line_start).then_some(*line_start),
                Some(side.clone()),
                None,
            ),
            GitHubPostTarget::Reply { in_reply_to } => (None, None, None, None, Some(*in_reply_to)),
            GitHubPostTarget::General => (None, None, None, None, None),
        };
        ListedGitHubComment {
            id,
            body: request.body.clone(),
            author: author.to_string(),
            created_at: chrono_now(),
            path,
            line,
            start_line,
            original_line: None,
            original_start_line: None,
            side,
            in_reply_to_id: in_reply_to,
        }
    }

    impl GitHubCommentPoster for FakeCommentPoster {
        fn post(&self, request: &GitHubPostRequest) -> GitHubPostOutcome {
            self.posted.lock().unwrap().push(request.clone());
            let outcome = self
                .outcomes
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| GitHubPostOutcome::Posted {
                    github_id: self.next_id.fetch_add(1, Ordering::Relaxed),
                    created_at: chrono_now(),
                    author: self.viewer.clone(),
                });
            if matches!(outcome, GitHubPostOutcome::UnknownOutcome { .. })
                && self.record_unknown_as_listed.load(Ordering::Relaxed)
            {
                let id = self.next_id.fetch_add(1, Ordering::Relaxed);
                self.listed
                    .lock()
                    .unwrap()
                    .push(listed_for(request, id, &self.viewer));
            }
            outcome
        }

        fn list(&self, _request: &GitHubPostRequest) -> anyhow::Result<Vec<ListedGitHubComment>> {
            self.list_calls.fetch_add(1, Ordering::Relaxed);
            Ok(self.listed.lock().unwrap().clone())
        }

        fn viewer_login(&self, _request: &GitHubPostRequest) -> anyhow::Result<String> {
            self.viewer_calls.fetch_add(1, Ordering::Relaxed);
            Ok(self.viewer.clone())
        }
    }

    struct Fixture {
        app: App,
        poster: Arc<FakeCommentPoster>,
        dir: tempfile::TempDir,
    }

    fn sample_file() -> crate::git::DiffFile {
        crate::git::DiffFile {
            path: "src/a.rs".to_string(),
            status: crate::git::FileStatus::Modified,
            hunks: vec![crate::git::DiffHunk {
                header: "@@ -1,1 +1,2 @@".to_string(),
                old_start: 1,
                old_count: 1,
                new_start: 1,
                new_count: 2,
                lines: vec![
                    crate::git::DiffLine {
                        line_type: crate::git::LineType::Context,
                        content: "one".to_string(),
                        old_num: Some(1),
                        new_num: Some(1),
                    },
                    crate::git::DiffLine {
                        line_type: crate::git::LineType::Add,
                        content: "two".to_string(),
                        old_num: None,
                        new_num: Some(2),
                    },
                ],
            }],
            adds: 1,
            dels: 0,
            compacted: false,
            raw_hunk_count: 1,
            similarity: None,
        }
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new_for_test(vec![sample_file()]);
        app.tab_mut().er_root =
            crate::paths::ErRoot::managed(dir.path().to_string_lossy().into_owned());
        app.tab_mut().branch_diff_hash = "branch-hash".to_string();
        app.tab_mut().diff_hash = "diff-hash".to_string();
        let poster = FakeCommentPoster::new();
        app.set_github_comment_poster(poster.clone());
        Fixture { app, poster, dir }
    }

    fn target() -> PrPostTarget {
        PrPostTarget {
            owner: "owner".to_string(),
            repo: "repo".to_string(),
            pr: 7,
            repo_root: None,
        }
    }

    fn line_intent(body: &str) -> NewGitHubComment {
        NewGitHubComment::Line {
            file: "src/a.rs".to_string(),
            hunk_idx: 0,
            line_start: Some(2),
            line_end: None,
            side: Some("RIGHT".to_string()),
            body: body.to_string(),
        }
    }

    fn drive(
        app: &mut App,
        poster: &dyn GitHubCommentPoster,
        intent: NewGitHubComment,
        client_id: &str,
    ) -> Result<PostedGitHubComment, GitHubPostError> {
        let ctx = app.comment_post_context();
        let ticket =
            app.begin_github_comment_post(intent, target(), ctx, Some(client_id.to_string()))?;
        let execution = execute_comment_post(poster, &ticket.request, ticket.pending.as_ref());
        app.finish_github_comment_post(ticket, execution)
    }

    fn clear_retry_delay(app: &mut App, client_id: &str) {
        if let Some(pending) = app.pending_github_posts.get_mut(client_id) {
            pending.not_before_epoch = 0;
        }
    }

    /// The REST shape `rebuild_comment_store` reads, from a fake's listed one.
    fn github_comment_for(
        request: &GitHubPostRequest,
        id: u64,
        body: &str,
        author: &str,
    ) -> crate::github::GitHubComment {
        let (path, line, start_line, side, in_reply_to_id) = match &request.target {
            GitHubPostTarget::Line {
                path,
                line_start,
                line_end,
                side,
            } => (
                Some(path.clone()),
                Some(*line_end),
                (line_end > line_start).then_some(*line_start),
                Some(side.clone()),
                None,
            ),
            GitHubPostTarget::Reply { in_reply_to } => (None, None, None, None, Some(*in_reply_to)),
            GitHubPostTarget::General => (None, None, None, None, None),
        };
        serde_json::from_value(serde_json::json!({
            "id": id,
            "body": body,
            "path": path,
            "line": line,
            "start_line": start_line,
            "original_line": null,
            "side": side,
            "in_reply_to_id": in_reply_to_id,
            "user": { "login": author },
            "created_at": chrono_now(),
            "updated_at": chrono_now(),
            "diff_hunk": null,
            "outdated": false,
        }))
        .unwrap()
    }

    fn store(app: &App) -> ai::ErGithubComments {
        let path = app.tab().github_comments_path();
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()
    }

    fn write_store(app: &App, gc: &ai::ErGithubComments) {
        let path = app.tab().github_comments_path();
        if let Some(dir) = std::path::Path::new(&path).parent() {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(&path, serde_json::to_string_pretty(gc).unwrap()).unwrap();
    }

    fn empty_store() -> ai::ErGithubComments {
        ai::ErGithubComments {
            version: 1,
            diff_hash: "branch-hash".to_string(),
            github: None,
            comments: Vec::new(),
        }
    }

    fn recorded_comment(id: &str, github_id: u64, body: &str) -> ai::GithubComment {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "file": "src/a.rs",
            "hunk_index": 0,
            "line_start": 2,
            "comment": body,
            "source": "github",
            "github_id": github_id,
            "author": "viewer",
        }))
        .unwrap()
    }

    #[test]
    fn retried_create_after_a_timeout_posts_once() {
        let mut f = fixture();
        let storage = f.dir.path().to_path_buf();
        crate::storage::with_test_storage_root(&storage, || {
            let client = "c-retry-timeout";
            f.poster
                .record_unknown_as_listed
                .store(true, Ordering::Relaxed);
            f.poster.push_outcome(GitHubPostOutcome::UnknownOutcome {
                error: "timeout".to_string(),
            });

            let first = drive(&mut f.app, f.poster.as_ref(), line_intent("hello"), client);
            assert!(matches!(first, Err(GitHubPostError::UnknownOutcome(_))));
            assert_eq!(f.poster.posts(), 1);
            clear_retry_delay(&mut f.app, client);

            let second = drive(&mut f.app, f.poster.as_ref(), line_intent("hello"), client)
                .expect("the retry adopts the first post");
            assert_eq!(f.poster.posts(), 1, "a retry must not post a second copy");
            let gc = store(&f.app);
            assert_eq!(gc.comments.len(), 1);
            assert_eq!(gc.comments[0].id, second.store_id.unwrap());
        });
    }

    #[test]
    fn a_definite_failure_leaves_no_record_and_allows_a_clean_retry() {
        let mut f = fixture();
        let storage = f.dir.path().to_path_buf();
        crate::storage::with_test_storage_root(&storage, || {
            let client = "c-definite";
            f.poster.push_outcome(GitHubPostOutcome::NotPosted {
                error: "refused".to_string(),
            });

            let first = drive(&mut f.app, f.poster.as_ref(), line_intent("hello"), client);
            assert!(matches!(first, Err(GitHubPostError::NotPosted(_))));
            assert!(f.app.pending_github_posts.is_empty());
            assert!(!std::path::Path::new(&f.app.tab().github_comments_path()).exists());

            let second =
                drive(&mut f.app, f.poster.as_ref(), line_intent("hello"), client).unwrap();
            assert_eq!(f.poster.posts(), 2);
            assert_eq!(store(&f.app).comments.len(), 1);
            assert_eq!(store(&f.app).comments[0].id, second.store_id.unwrap());
        });
    }

    #[test]
    fn a_second_submit_while_the_first_is_in_flight_posts_once() {
        let mut f = fixture();
        let storage = f.dir.path().to_path_buf();
        crate::storage::with_test_storage_root(&storage, || {
            let client = "c-in-flight";
            let ctx = f.app.comment_post_context();
            let ticket = f
                .app
                .begin_github_comment_post(line_intent("hello"), target(), ctx, Some(client.into()))
                .unwrap();

            let ctx = f.app.comment_post_context();
            let second = f.app.begin_github_comment_post(
                line_intent("hello"),
                target(),
                ctx,
                Some(client.into()),
            );
            assert_eq!(second.unwrap_err(), GitHubPostError::InFlight);
            assert_eq!(f.poster.posts(), 0);

            let execution =
                execute_comment_post(f.poster.as_ref(), &ticket.request, ticket.pending.as_ref());
            f.app.finish_github_comment_post(ticket, execution).unwrap();
            assert_eq!(f.poster.posts(), 1);
        });
    }

    #[test]
    fn a_posted_comment_is_recorded_as_a_pull_would_record_it() {
        let mut f = fixture();
        let storage = f.dir.path().to_path_buf();
        crate::storage::with_test_storage_root(&storage, || {
            let posted = drive(
                &mut f.app,
                f.poster.as_ref(),
                line_intent("hello"),
                "c-record",
            )
            .unwrap();
            let gc = store(&f.app);
            assert_eq!(gc.comments.len(), 1);
            let entry = &gc.comments[0];
            assert_eq!(entry.id, format!("gh-{}", posted.github_id));
            assert_eq!(entry.github_id, Some(posted.github_id));
            assert_eq!(entry.source, "github");
            assert_eq!(entry.line_start, Some(2));

            let request = f.poster.posted.lock().unwrap()[0].clone();
            let listed = github_comment_for(&request, posted.github_id, "hello", "viewer");
            let ctx = crate::sync::CommentSyncContext {
                owner: "owner".to_string(),
                repo_name: "repo".to_string(),
                pr_number: 7,
                is_remote: true,
                repo_root: String::new(),
                comments_path: f.app.tab().github_comments_path(),
                diff_hash: "branch-hash".to_string(),
                anchor_hash: "diff-hash".to_string(),
                files: Vec::new(),
                pr_number_for_overview: Some(7),
                github: f.app.tab().ports.github.clone(),
            };
            let threads: HashMap<u64, crate::github::ReviewThreadState> = HashMap::new();
            let rebuilt = crate::sync::rebuild_comment_store(&ctx, gc, &[listed], &threads);
            let matching: Vec<_> = rebuilt
                .comments
                .iter()
                .filter(|c| c.github_id == Some(posted.github_id))
                .collect();
            assert_eq!(matching.len(), 1, "the pull must not duplicate the entry");
        });
    }

    #[test]
    fn a_reply_to_a_local_only_parent_does_not_post() {
        let mut f = fixture();
        f.app.tab_mut().ai.github_comments = Some(
            serde_json::from_value(serde_json::json!({
                "version": 1,
                "diff_hash": "h",
                "comments": [{
                    "id": "c-local",
                    "file": "src/a.rs",
                    "hunk_index": 0,
                    "line_start": 2,
                    "comment": "private",
                    "source": "local",
                    "author": "You"
                }]
            }))
            .unwrap(),
        );

        let result = drive(
            &mut f.app,
            f.poster.as_ref(),
            NewGitHubComment::Reply {
                parent_id: "c-local".to_string(),
                body: "reply".to_string(),
            },
            "c-reply-local",
        );
        assert!(matches!(result, Err(GitHubPostError::NotPosted(_))));
        assert_eq!(f.poster.posts(), 0);
    }

    #[test]
    fn a_finding_followup_thread_never_posts() {
        let mut f = fixture();
        let storage = f.dir.path().to_path_buf();
        crate::storage::with_test_storage_root(&storage, || {
            {
                let tab = f.app.tab_mut();
                tab.comment_type = CommentType::GitHubComment;
                tab.comment_finding_ref = Some("f-1".to_string());
                tab.comment_file = "src/a.rs".to_string();
                tab.comment_textarea = TextArea::new(vec!["finding reply".to_string()]);
            }
            assert!(f.app.github_comment_intent().is_none());

            // The desktop finding-reply path still writes a local comment.
            f.app
                .submit_comment_text(
                    crate::app::CommentTarget {
                        file: "src/a.rs".to_string(),
                        hunk_idx: 0,
                        line_num: Some(2),
                        line_num_end: None,
                    },
                    "finding reply".to_string(),
                    CommentType::GitHubComment,
                    None,
                    Some("f-1".to_string()),
                )
                .unwrap();
            assert_eq!(f.poster.posts(), 0);
            let gc = store(&f.app);
            assert_eq!(gc.comments.len(), 1);
            assert!(gc.comments[0].github_id.is_none());
            assert_eq!(gc.comments[0].source, "local");
        });
    }

    #[test]
    fn a_general_comment_is_posted_but_not_recorded() {
        let mut f = fixture();
        let storage = f.dir.path().to_path_buf();
        crate::storage::with_test_storage_root(&storage, || {
            let posted = drive(
                &mut f.app,
                f.poster.as_ref(),
                NewGitHubComment::General {
                    body: "hello PR".to_string(),
                },
                "c-general",
            )
            .unwrap();
            assert_eq!(posted.store_id, None);
            assert_eq!(f.poster.posts(), 1);
            assert!(!std::path::Path::new(&f.app.tab().github_comments_path()).exists());
        });
    }

    #[test]
    fn a_pull_between_attempts_settles_the_retry_without_posting() {
        let mut f = fixture();
        let storage = f.dir.path().to_path_buf();
        crate::storage::with_test_storage_root(&storage, || {
            let client = "c-pull-between";
            f.poster.push_outcome(GitHubPostOutcome::UnknownOutcome {
                error: "timeout".to_string(),
            });
            let first = drive(&mut f.app, f.poster.as_ref(), line_intent("hello"), client);
            assert!(matches!(first, Err(GitHubPostError::UnknownOutcome(_))));

            // The pull lands the comment before the retry.
            let mut gc = empty_store();
            gc.comments.push(recorded_comment("gh-42", 42, "hello"));
            write_store(&f.app, &gc);
            let request = f.poster.posted.lock().unwrap()[0].clone();
            f.poster.add_listed(listed_for(&request, 42, "viewer"));
            clear_retry_delay(&mut f.app, client);

            let second =
                drive(&mut f.app, f.poster.as_ref(), line_intent("hello"), client).unwrap();
            assert_eq!(f.poster.posts(), 1, "the retry must not post again");
            assert_eq!(second.github_id, 42);
            assert_eq!(store(&f.app).comments.len(), 1);
        });
    }

    #[test]
    fn an_earlier_identical_comment_is_not_adopted() {
        let mut f = fixture();
        let storage = f.dir.path().to_path_buf();
        crate::storage::with_test_storage_root(&storage, || {
            let client = "c-known-id";
            let mut gc = empty_store();
            gc.comments.push(recorded_comment("gh-42", 42, "hello"));
            write_store(&f.app, &gc);

            f.poster.push_outcome(GitHubPostOutcome::UnknownOutcome {
                error: "timeout".to_string(),
            });
            let first = drive(&mut f.app, f.poster.as_ref(), line_intent("hello"), client);
            assert!(matches!(first, Err(GitHubPostError::UnknownOutcome(_))));

            let request = f.poster.posted.lock().unwrap()[0].clone();
            f.poster.add_listed(listed_for(&request, 42, "viewer"));
            clear_retry_delay(&mut f.app, client);

            let second =
                drive(&mut f.app, f.poster.as_ref(), line_intent("hello"), client).unwrap();
            assert_eq!(
                f.poster.posts(),
                2,
                "an id that predates the attempt must not be adopted"
            );
            assert_ne!(second.github_id, 42);
            assert_eq!(store(&f.app).comments.len(), 2);
        });
    }

    #[test]
    fn a_restored_finding_reply_draft_does_not_post() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let mut app = App::new_for_test(vec![sample_file()]);
            app.tab_mut().er_root =
                crate::paths::ErRoot::managed(dir.path().to_string_lossy().into_owned());
            app.tab_mut().branch_diff_hash = "branch-hash".to_string();
            {
                let tab = app.tab_mut();
                tab.comment_textarea = TextArea::new(vec!["a finding reply".to_string()]);
                tab.comment_file = "src/a.rs".to_string();
                tab.comment_hunk = 0;
                tab.comment_line_num = Some(2);
                tab.comment_type = CommentType::GitHubComment;
                tab.comment_finding_ref = Some("f-1".to_string());
                tab.comment_client_id = Some("c-restored".to_string());
            }
            app.tab().save_session();

            let mut restored = App::new_for_test(vec![sample_file()]);
            restored.tab_mut().er_root =
                crate::paths::ErRoot::managed(dir.path().to_string_lossy().into_owned());
            restored.tab_mut().branch_diff_hash = "branch-hash".to_string();
            assert!(restored.tab_mut().restore_session());
            assert_eq!(restored.tab().comment_finding_ref.as_deref(), Some("f-1"));
            assert_eq!(
                restored.tab().comment_client_id.as_deref(),
                Some("c-restored")
            );
            assert!(restored.github_comment_intent().is_none());
        });
    }

    #[test]
    fn a_4xx_refusal_is_not_posted_and_does_not_need_reconcile() {
        let mut f = fixture();
        let storage = f.dir.path().to_path_buf();
        crate::storage::with_test_storage_root(&storage, || {
            let client = "c-refusal";
            f.poster.push_outcome(GitHubPostOutcome::NotPosted {
                error: "Unprocessable Entity (HTTP 422)".to_string(),
            });
            let first = drive(&mut f.app, f.poster.as_ref(), line_intent("hello"), client);
            assert!(matches!(first, Err(GitHubPostError::NotPosted(_))));

            let second =
                drive(&mut f.app, f.poster.as_ref(), line_intent("hello"), client).unwrap();
            assert!(second.github_id > 0);
            assert_eq!(
                f.poster.viewer_calls.load(Ordering::Relaxed),
                0,
                "a refusal is decisive; no reconcile"
            );
            assert_eq!(f.poster.list_calls.load(Ordering::Relaxed), 0);
            assert_eq!(f.poster.posts(), 2);
        });
    }

    #[test]
    fn an_adopted_retry_records_the_posted_body_not_the_edit() {
        let mut f = fixture();
        let storage = f.dir.path().to_path_buf();
        crate::storage::with_test_storage_root(&storage, || {
            let client = "c-adopt-edited";
            f.poster
                .record_unknown_as_listed
                .store(true, Ordering::Relaxed);
            f.poster.push_outcome(GitHubPostOutcome::UnknownOutcome {
                error: "timeout".to_string(),
            });
            let first = drive(&mut f.app, f.poster.as_ref(), line_intent("first"), client);
            assert!(matches!(first, Err(GitHubPostError::UnknownOutcome(_))));
            clear_retry_delay(&mut f.app, client);

            let second =
                drive(&mut f.app, f.poster.as_ref(), line_intent("edited"), client).unwrap();
            assert_eq!(f.poster.posts(), 1);
            assert_eq!(second.github_id, 100);
            let gc = store(&f.app);
            assert_eq!(gc.comments.len(), 1);
            assert_eq!(
                gc.comments[0].comment, "first",
                "the sidecar records what GitHub holds"
            );
            assert!(
                second.kept_earlier_body,
                "the caller keeps the edited text and says so"
            );
        });
    }

    #[test]
    fn an_adopted_edit_keeps_the_composer_open() {
        let mut f = fixture();
        let storage = f.dir.path().to_path_buf();
        crate::storage::with_test_storage_root(&storage, || {
            {
                let tab = f.app.tab_mut();
                // A remote tab resolves its post target without a gh call.
                tab.remote_repo = Some("owner/repo".to_string());
                tab.pr_number = Some(7);
                tab.comment_type = CommentType::GitHubComment;
                tab.comment_file = "src/a.rs".to_string();
                tab.comment_hunk = 0;
                tab.comment_line_num = Some(2);
                tab.comment_textarea = TextArea::new(vec!["first".to_string()]);
            }
            f.poster
                .record_unknown_as_listed
                .store(true, Ordering::Relaxed);
            f.poster.push_outcome(GitHubPostOutcome::UnknownOutcome {
                error: "timeout".to_string(),
            });
            assert!(f.app.post_github_comment_now().is_err());
            assert_eq!(f.poster.posts(), 1);
            let client = f
                .app
                .tab()
                .comment_client_id
                .clone()
                .expect("the composer holds the client id");
            clear_retry_delay(&mut f.app, &client);

            f.app.tab_mut().comment_textarea = TextArea::new(vec!["edited".to_string()]);
            f.app.input_mode = InputMode::Comment;
            let posted = f.app.post_github_comment_now().unwrap();

            assert!(posted.kept_earlier_body);
            assert_eq!(f.app.input_mode, InputMode::Comment);
            assert_eq!(f.app.tab().comment_textarea.lines().join("\n"), "edited");
            assert!(
                f.app.tab().comment_client_id.is_none(),
                "a resend is a deliberate new comment"
            );
            assert!(f.app.notification.as_ref().is_some_and(|n| {
                n.message == "Your earlier version was posted; this edit was not"
            }));
        });
    }

    #[test]
    fn a_record_failure_keeps_an_existing_pending_record() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let mut app = App::new_for_test(vec![sample_file()]);
            let blocked = dir.path().join("er");
            std::fs::write(&blocked, b"not a directory").unwrap();
            app.tab_mut().er_root =
                crate::paths::ErRoot::managed(blocked.to_string_lossy().into_owned());
            app.tab_mut().branch_diff_hash = "branch-hash".to_string();
            app.tab_mut().diff_hash = "diff-hash".to_string();
            let poster = FakeCommentPoster::new();
            app.set_github_comment_poster(poster.clone());
            poster
                .record_unknown_as_listed
                .store(true, Ordering::Relaxed);

            let client = "c-three-sends";
            // Send 1: unknown, remembered with the first body and start time.
            poster.push_outcome(GitHubPostOutcome::UnknownOutcome {
                error: "timeout".to_string(),
            });
            let first = drive(&mut app, poster.as_ref(), line_intent("first"), client);
            assert!(matches!(first, Err(GitHubPostError::UnknownOutcome(_))));
            let started_at = app
                .pending_github_posts
                .get(client)
                .unwrap()
                .started_at
                .clone();
            clear_retry_delay(&mut app, client);

            // Send 2: adopts the first comment, but the write fails.
            let second = drive(&mut app, poster.as_ref(), line_intent("edited"), client);
            assert!(matches!(second, Err(GitHubPostError::RecordFailed(_))));
            let pending = app.pending_github_posts.get(client).cloned().unwrap();
            assert_eq!(
                pending.started_at, started_at,
                "the first start time is kept"
            );
            assert_eq!(pending.bodies, vec!["first".to_string()]);
            assert_eq!(pending.not_before_epoch, 0);

            // Send 3: still adopts the earlier comment, so it must not post again.
            std::fs::remove_file(&blocked).unwrap();
            std::fs::create_dir_all(&blocked).unwrap();
            let third = drive(&mut app, poster.as_ref(), line_intent("third"), client).unwrap();
            assert_eq!(poster.posts(), 1);
            assert_eq!(third.github_id, 100);
            assert!(third.kept_earlier_body);
        });
    }

    #[test]
    fn an_absent_lookup_maps_to_the_no_pr_sentence() {
        let lookup = crate::github::PrLookup::Absent;
        let ctx = CommentPostContext {
            repo_root: "/repo".to_string(),
            is_remote: false,
            remote_repo: None,
            pr_number: None,
        };
        assert_eq!(
            crate::github_post::post_target_from_lookup(lookup, &ctx).unwrap_err(),
            GitHubPostError::NotPosted(crate::github_post::NO_PR_FOR_COMMENT.to_string())
        );
    }

    #[test]
    fn a_record_failure_reconciles_on_retry() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let mut app = App::new_for_test(vec![sample_file()]);
            // A regular file where the store dir must be makes the write fail.
            let blocked = dir.path().join("er");
            std::fs::write(&blocked, b"not a directory").unwrap();
            app.tab_mut().er_root =
                crate::paths::ErRoot::managed(blocked.to_string_lossy().into_owned());
            app.tab_mut().branch_diff_hash = "branch-hash".to_string();
            app.tab_mut().diff_hash = "diff-hash".to_string();
            let poster = FakeCommentPoster::new();
            app.set_github_comment_poster(poster.clone());

            let client = "c-record-fail";
            let first = drive(&mut app, poster.as_ref(), line_intent("hello"), client);
            assert!(matches!(first, Err(GitHubPostError::RecordFailed(_))));
            assert_eq!(poster.posts(), 1);
            let pending = app
                .pending_github_posts
                .get(client)
                .cloned()
                .expect("a record failure keeps the pending post");
            assert_eq!(pending.not_before_epoch, 0);

            // Repair the path; the comment GitHub holds is the one to adopt.
            std::fs::remove_file(&blocked).unwrap();
            std::fs::create_dir_all(&blocked).unwrap();
            let request = poster.posted.lock().unwrap()[0].clone();
            let listed = listed_for(&request, 100, "viewer");
            poster.add_listed(listed);

            let second = drive(&mut app, poster.as_ref(), line_intent("hello"), client).unwrap();
            assert_eq!(poster.posts(), 1, "the retry must adopt, not post again");
            let gc = store(&app);
            assert_eq!(gc.comments.len(), 1);
            assert_eq!(gc.comments[0].id, second.store_id.unwrap());
        });
    }

    #[test]
    fn a_promote_delete_failure_reconciles_instead_of_reposting() {
        let mut f = fixture();
        let storage = f.dir.path().to_path_buf();
        crate::storage::with_test_storage_root(&storage, || {
            let client = "c-promote-retry";
            let first = drive(&mut f.app, f.poster.as_ref(), line_intent("hello"), client).unwrap();
            let request = f.poster.posted.lock().unwrap()[0].clone();
            // What the promote commands do when deleting the source fails.
            f.app.remember_posted_for_retry(request.clone());
            f.poster
                .add_listed(listed_for(&request, first.github_id, "viewer"));

            let second =
                drive(&mut f.app, f.poster.as_ref(), line_intent("hello"), client).unwrap();
            assert_eq!(f.poster.posts(), 1, "the retry must adopt, not post again");
            assert_eq!(second.github_id, first.github_id);
            assert_eq!(store(&f.app).comments.len(), 1);
        });
    }

    #[test]
    fn a_retry_before_the_not_before_time_does_not_post() {
        let mut f = fixture();
        let storage = f.dir.path().to_path_buf();
        crate::storage::with_test_storage_root(&storage, || {
            let client = "c-too-early";
            f.poster.push_outcome(GitHubPostOutcome::UnknownOutcome {
                error: "timeout".to_string(),
            });
            let first = drive(&mut f.app, f.poster.as_ref(), line_intent("hello"), client);
            assert!(matches!(first, Err(GitHubPostError::UnknownOutcome(_))));
            let before = f.app.pending_github_posts.get(client).cloned().unwrap();

            let second = drive(&mut f.app, f.poster.as_ref(), line_intent("hello"), client);
            assert!(matches!(
                second,
                Err(GitHubPostError::UnknownOutcome(ref error)) if error == RETRY_TOO_EARLY
            ));
            assert_eq!(f.poster.posts(), 1);
            assert_eq!(f.poster.list_calls.load(Ordering::Relaxed), 0);
            assert_eq!(f.poster.viewer_calls.load(Ordering::Relaxed), 0);
            let after = f.app.pending_github_posts.get(client).cloned().unwrap();
            assert_eq!(after.not_before_epoch, before.not_before_epoch);
            assert_eq!(after.bodies, before.bodies);
        });
    }

    #[test]
    fn a_promoted_comment_converts_to_the_post_intent() {
        let source: crate::ai::PrivateAnnotation = serde_json::from_value(serde_json::json!({
            "id": "q-1",
            "file": "a.rs",
            "hunk_index": 2,
            "line_start": 4,
            "text": "text"
        }))
        .unwrap();
        let promoted =
            crate::ai::promote::promote_to_github(&source, std::slice::from_ref(&source), None)
                .unwrap();
        match NewGitHubComment::from(promoted) {
            NewGitHubComment::Line {
                file,
                hunk_idx,
                line_start,
                line_end,
                side,
                body,
            } => {
                assert_eq!(file, "a.rs");
                assert_eq!(hunk_idx, 2);
                assert_eq!(line_start, Some(4));
                assert_eq!(line_end, None);
                assert_eq!(side.as_deref(), Some("RIGHT"));
                assert_eq!(body, "text");
            }
            other => panic!("expected a line intent, got {other:?}"),
        }
    }
}
