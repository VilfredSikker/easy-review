//! Posting a GitHub comment at creation: the state half of
//! [`crate::github_post`].
//!
//! A create runs in three phases so no `gh` call holds the `App` lock:
//! `begin_github_comment_post` captures what the record needs, the caller runs
//! `execute_comment_post` outside the lock, and `finish_github_comment_post`
//! writes the record. A retry reuses the client id so the pending attempt is
//! reconciled instead of posting twice.

use std::sync::Arc;

use crate::github_post::{
    new_client_id, CommentPostContext, GitHubCommentPoster, GitHubPostError, GitHubPostExecution,
    GitHubPostRequest, PostedGitHubComment, PrPostTarget,
};
use crate::github_post_session::{PostBegin, PostSession};

use tui_textarea::TextArea;

use super::{App, CommentType, DiffMode, InputMode, LineAnchor, TabState};

pub use crate::github_post_session::{
    GitHubPostTicket, NewGitHubComment, PostedGitHubCommentLocation,
};

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
        let mut session = PostSession::from_pending(std::mem::take(&mut self.pending_github_posts));
        let active = self.comment_post_context();
        let tab = self.tab();
        let diff_file = match &intent {
            NewGitHubComment::Line { file, .. } => Self::annotation_diff_file(tab, file),
            NewGitHubComment::Reply { .. } | NewGitHubComment::General { .. } => None,
        };
        let comments_dir = tab.github_comments_dir();
        let ticket = session.begin(
            intent,
            PostBegin {
                active,
                ctx,
                pr,
                comments: tab.ai.github_comments.as_ref(),
                diff_file,
                comments_dir: &comments_dir,
                branch_diff_hash: tab.branch_diff_hash.clone(),
                diff_hash: tab.diff_hash.clone(),
                client_id,
            },
        );
        self.pending_github_posts = session.into_pending();
        ticket
    }

    /// Record a create's outcome. A general comment is not recorded: the pull
    /// never lists issue comments, so it would vanish on the next pull.
    pub fn finish_github_comment_post(
        &mut self,
        ticket: GitHubPostTicket,
        execution: GitHubPostExecution,
    ) -> Result<PostedGitHubComment, GitHubPostError> {
        let ticket_tab = ticket.tab.clone();
        let client_id = ticket.request.client_id.clone();
        let mut session = PostSession::from_pending(std::mem::take(&mut self.pending_github_posts));
        let record = session.finish(ticket, execution);
        self.pending_github_posts = session.into_pending();
        let record = record?;
        if let Some(comments) = record.comments {
            if let Some(tab) = self.tabs.iter_mut().find(|tab| {
                tab.repo_root == ticket_tab.repo_root
                    && tab.pr_number == ticket_tab.pr_number
                    && tab.is_remote() == ticket_tab.is_remote
            }) {
                tab.ai.github_comments = Some(comments);
                tab.ai.rebuild_comment_index();
                if let Some(path) = record.comments_path.as_ref() {
                    tab.mark_sidecar_written(path.to_string_lossy().as_ref());
                }
                if tab.comment_client_id.as_deref() == Some(client_id.as_str()) {
                    tab.comment_client_id = None;
                }
            }
        }
        // The pull's comment list is stale whether or not a row was recorded.
        self.ports.github.invalidate_comments();
        Ok(record.posted)
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
        let mut session = PostSession::from_pending(std::mem::take(&mut self.pending_github_posts));
        session.remember_posted_for_retry(request);
        self.pending_github_posts = session.into_pending();
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
        crate::annotations::line_anchor_at(
            Self::annotation_diff_file(tab, file),
            hunk_idx,
            line,
            side,
        )
    }

    /// The file an annotation targets: the selected file when the name is
    /// empty, a history commit's file in History, else the tab's files.
    fn annotation_diff_file<'a>(tab: &'a TabState, file: &str) -> Option<&'a crate::git::DiffFile> {
        if file.is_empty() {
            tab.selected_diff_file()
        } else if tab.mode == DiffMode::History {
            tab.history
                .as_ref()
                .and_then(|history| history.commit_files.iter().find(|f| f.path == file))
        } else {
            tab.files.iter().find(|f| f.path == file)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai;
    use crate::github_post::{
        execute_comment_post, GitHubCommentPoster, GitHubPostOutcome, GitHubPostRequest,
        GitHubPostTarget, ListedGitHubComment, PrPostTarget, RETRY_TOO_EARLY,
    };
    use crate::sync::chrono_now;
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
        let dir = app.tab().github_comments_dir();
        crate::test_support::read_sidecar(std::path::Path::new(&dir))
    }

    fn write_store(app: &App, gc: &ai::ErGithubComments) {
        let dir = app.tab().github_comments_dir();
        crate::test_support::write_sidecar(std::path::Path::new(&dir), gc);
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
