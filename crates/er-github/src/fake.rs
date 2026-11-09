//! A scripted in-memory GitHub for tests.

use crate::actions::{PrAction, RepoMergeSettings};
use crate::error::GhError;
use crate::legacy::{
    CreatedComment, GitHubComment, IssueComment, NewLineComment, PrCommentBundle, PrOverviewData,
    ReviewThreadState,
};
use crate::pending_review::PendingReview;
use crate::review::ReviewBatchEntry;
use crate::GitHub;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

#[derive(Debug, Default)]
pub struct FakeGitHub {
    state: Mutex<FakeState>,
}

#[expect(
    clippy::option_option,
    reason = "an unscripted value method panics; a scripted None is a real answer"
)]
#[derive(Debug, Default)]
struct FakeState {
    calls: Vec<String>,
    run_pr_action: Option<Result<(), GhError>>,
    delete_remote_branch: Option<Result<(), GhError>>,
    pr_state: Option<Result<String, GhError>>,
    base_has_merge_queue: Option<Result<bool, GhError>>,
    pr_in_merge_queue: Option<Result<bool, GhError>>,
    viewer_can_bypass: Option<Result<bool, GhError>>,
    restore_remote_branch: Option<Result<(), GhError>>,
    remote_branch_exists: Option<Result<bool, GhError>>,
    repo_merge_settings: Option<Result<RepoMergeSettings, GhError>>,
    cached_base_has_merge_queue: Option<Option<bool>>,
    cached_viewer_can_bypass: Option<Option<bool>>,
    cached_remote_branch_exists: Option<Option<bool>>,
    cached_repo_merge_settings: Option<Option<RepoMergeSettings>>,
    pending_review: Option<Result<PendingReview, GhError>>,
    pending_review_add_comment: Option<Result<(), GhError>>,
    pending_review_remove_comment: Option<Result<(), GhError>>,
    pending_review_submit: Option<Result<usize, GhError>>,
    submit_review: Option<Result<(), GhError>>,
    submit_review_remote: Option<Result<(), GhError>>,
    pr_head_oid_remote: Option<Result<String, GhError>>,
    get_repo_info: Option<Result<(String, String), GhError>>,
    pr_comments: Option<Result<Vec<GitHubComment>, GhError>>,
    pr_comments_remote: Option<Result<Vec<GitHubComment>, GhError>>,
    review_threads: Option<Result<HashMap<u64, ReviewThreadState>, GhError>>,
    review_threads_remote: Option<Result<HashMap<u64, ReviewThreadState>, GhError>>,
    pr_head_oid: Option<Result<String, GhError>>,
    post_line_comment: Option<Result<CreatedComment, GhError>>,
    post_reply: Option<Result<CreatedComment, GhError>>,
    post_general: Option<Result<CreatedComment, GhError>>,
    issue_comments: Option<Result<Vec<IssueComment>, GhError>>,
    viewer_login: Option<Result<String, GhError>>,
    push_comment: Option<Result<u64, GhError>>,
    push_comment_remote: Option<Result<u64, GhError>>,
    reply_comment: Option<Result<u64, GhError>>,
    reply_comment_remote: Option<Result<u64, GhError>>,
    update_review_comment: Option<Result<(), GhError>>,
    delete_comment: Option<Result<(), GhError>>,
    edit_body: Option<Result<(), GhError>>,
    approve: Option<Result<(), GhError>>,
    fetch_pr_comment_bundle: Option<Result<PrCommentBundle, GhError>>,
    pr_comment_bundle_cached: Option<Result<Arc<PrCommentBundle>, GhError>>,
    overview: Option<Result<Option<PrOverviewData>, GhError>>,
    overview_no_checks: Option<Result<Option<PrOverviewData>, GhError>>,
    overview_remote: Option<Result<Option<PrOverviewData>, GhError>>,
    api_get_bounded: Option<Result<Vec<u8>, GhError>>,
}

fn unscripted(method: &str) -> GhError {
    GhError::Local {
        message: format!("unscripted {method}"),
    }
}

impl FakeGitHub {
    fn lock(&self) -> MutexGuard<'_, FakeState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn record(&self, call: String) -> MutexGuard<'_, FakeState> {
        let mut state = self.lock();
        state.calls.push(call);
        state
    }

    /// Every call made so far, in order.
    pub fn calls(&self) -> Vec<String> {
        self.lock().calls.clone()
    }

    #[must_use]
    pub fn with_run_pr_action(self, value: Result<(), GhError>) -> Self {
        self.lock().run_pr_action = Some(value);
        self
    }

    #[must_use]
    pub fn with_delete_remote_branch(self, value: Result<(), GhError>) -> Self {
        self.lock().delete_remote_branch = Some(value);
        self
    }

    #[must_use]
    pub fn with_pr_state(self, value: Result<String, GhError>) -> Self {
        self.lock().pr_state = Some(value);
        self
    }

    #[must_use]
    pub fn with_base_has_merge_queue(self, value: Result<bool, GhError>) -> Self {
        self.lock().base_has_merge_queue = Some(value);
        self
    }

    #[must_use]
    pub fn with_pr_in_merge_queue(self, value: Result<bool, GhError>) -> Self {
        self.lock().pr_in_merge_queue = Some(value);
        self
    }

    #[must_use]
    pub fn with_viewer_can_bypass(self, value: Result<bool, GhError>) -> Self {
        self.lock().viewer_can_bypass = Some(value);
        self
    }

    #[must_use]
    pub fn with_restore_remote_branch(self, value: Result<(), GhError>) -> Self {
        self.lock().restore_remote_branch = Some(value);
        self
    }

    #[must_use]
    pub fn with_remote_branch_exists(self, value: Result<bool, GhError>) -> Self {
        self.lock().remote_branch_exists = Some(value);
        self
    }

    #[must_use]
    pub fn with_repo_merge_settings(self, value: Result<RepoMergeSettings, GhError>) -> Self {
        self.lock().repo_merge_settings = Some(value);
        self
    }

    #[must_use]
    pub fn with_cached_base_has_merge_queue(self, value: Option<bool>) -> Self {
        self.lock().cached_base_has_merge_queue = Some(value);
        self
    }

    #[must_use]
    pub fn with_cached_viewer_can_bypass(self, value: Option<bool>) -> Self {
        self.lock().cached_viewer_can_bypass = Some(value);
        self
    }

    #[must_use]
    pub fn with_cached_remote_branch_exists(self, value: Option<bool>) -> Self {
        self.lock().cached_remote_branch_exists = Some(value);
        self
    }

    #[must_use]
    pub fn with_cached_repo_merge_settings(self, value: Option<RepoMergeSettings>) -> Self {
        self.lock().cached_repo_merge_settings = Some(value);
        self
    }

    #[must_use]
    pub fn with_pending_review(self, value: Result<PendingReview, GhError>) -> Self {
        self.lock().pending_review = Some(value);
        self
    }

    #[must_use]
    pub fn with_pending_review_add_comment(self, value: Result<(), GhError>) -> Self {
        self.lock().pending_review_add_comment = Some(value);
        self
    }

    #[must_use]
    pub fn with_pending_review_remove_comment(self, value: Result<(), GhError>) -> Self {
        self.lock().pending_review_remove_comment = Some(value);
        self
    }

    #[must_use]
    pub fn with_pending_review_submit(self, value: Result<usize, GhError>) -> Self {
        self.lock().pending_review_submit = Some(value);
        self
    }

    #[must_use]
    pub fn with_submit_review(self, value: Result<(), GhError>) -> Self {
        self.lock().submit_review = Some(value);
        self
    }

    #[must_use]
    pub fn with_submit_review_remote(self, value: Result<(), GhError>) -> Self {
        self.lock().submit_review_remote = Some(value);
        self
    }

    #[must_use]
    pub fn with_pr_head_oid_remote(self, value: Result<String, GhError>) -> Self {
        self.lock().pr_head_oid_remote = Some(value);
        self
    }

    #[must_use]
    pub fn with_get_repo_info(self, value: Result<(String, String), GhError>) -> Self {
        self.lock().get_repo_info = Some(value);
        self
    }

    #[must_use]
    pub fn with_pr_comments(self, value: Result<Vec<GitHubComment>, GhError>) -> Self {
        self.lock().pr_comments = Some(value);
        self
    }

    #[must_use]
    pub fn with_pr_comments_remote(self, value: Result<Vec<GitHubComment>, GhError>) -> Self {
        self.lock().pr_comments_remote = Some(value);
        self
    }

    #[must_use]
    pub fn with_review_threads(
        self,
        value: Result<HashMap<u64, ReviewThreadState>, GhError>,
    ) -> Self {
        self.lock().review_threads = Some(value);
        self
    }

    #[must_use]
    pub fn with_review_threads_remote(
        self,
        value: Result<HashMap<u64, ReviewThreadState>, GhError>,
    ) -> Self {
        self.lock().review_threads_remote = Some(value);
        self
    }

    #[must_use]
    pub fn with_pr_head_oid(self, value: Result<String, GhError>) -> Self {
        self.lock().pr_head_oid = Some(value);
        self
    }

    #[must_use]
    pub fn with_post_line_comment(self, value: Result<CreatedComment, GhError>) -> Self {
        self.lock().post_line_comment = Some(value);
        self
    }

    #[must_use]
    pub fn with_post_reply(self, value: Result<CreatedComment, GhError>) -> Self {
        self.lock().post_reply = Some(value);
        self
    }

    #[must_use]
    pub fn with_post_general(self, value: Result<CreatedComment, GhError>) -> Self {
        self.lock().post_general = Some(value);
        self
    }

    #[must_use]
    pub fn with_issue_comments(self, value: Result<Vec<IssueComment>, GhError>) -> Self {
        self.lock().issue_comments = Some(value);
        self
    }

    #[must_use]
    pub fn with_viewer_login(self, value: Result<String, GhError>) -> Self {
        self.lock().viewer_login = Some(value);
        self
    }

    #[must_use]
    pub fn with_push_comment(self, value: Result<u64, GhError>) -> Self {
        self.lock().push_comment = Some(value);
        self
    }

    #[must_use]
    pub fn with_push_comment_remote(self, value: Result<u64, GhError>) -> Self {
        self.lock().push_comment_remote = Some(value);
        self
    }

    #[must_use]
    pub fn with_reply_comment(self, value: Result<u64, GhError>) -> Self {
        self.lock().reply_comment = Some(value);
        self
    }

    #[must_use]
    pub fn with_reply_comment_remote(self, value: Result<u64, GhError>) -> Self {
        self.lock().reply_comment_remote = Some(value);
        self
    }

    #[must_use]
    pub fn with_update_review_comment(self, value: Result<(), GhError>) -> Self {
        self.lock().update_review_comment = Some(value);
        self
    }

    #[must_use]
    pub fn with_delete_comment(self, value: Result<(), GhError>) -> Self {
        self.lock().delete_comment = Some(value);
        self
    }

    #[must_use]
    pub fn with_edit_body(self, value: Result<(), GhError>) -> Self {
        self.lock().edit_body = Some(value);
        self
    }

    #[must_use]
    pub fn with_approve(self, value: Result<(), GhError>) -> Self {
        self.lock().approve = Some(value);
        self
    }

    #[must_use]
    pub fn with_fetch_pr_comment_bundle(self, value: Result<PrCommentBundle, GhError>) -> Self {
        self.lock().fetch_pr_comment_bundle = Some(value);
        self
    }

    #[must_use]
    pub fn with_pr_comment_bundle_cached(
        self,
        value: Result<Arc<PrCommentBundle>, GhError>,
    ) -> Self {
        self.lock().pr_comment_bundle_cached = Some(value);
        self
    }

    #[must_use]
    pub fn with_overview(self, value: Result<Option<PrOverviewData>, GhError>) -> Self {
        self.lock().overview = Some(value);
        self
    }

    #[must_use]
    pub fn with_overview_no_checks(self, value: Result<Option<PrOverviewData>, GhError>) -> Self {
        self.lock().overview_no_checks = Some(value);
        self
    }

    #[must_use]
    pub fn with_overview_remote(self, value: Result<Option<PrOverviewData>, GhError>) -> Self {
        self.lock().overview_remote = Some(value);
        self
    }

    #[must_use]
    pub fn with_api_get_bounded(self, value: Result<Vec<u8>, GhError>) -> Self {
        self.lock().api_get_bounded = Some(value);
        self
    }
}

impl GitHub for FakeGitHub {
    fn run_pr_action(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
        action: &PrAction,
    ) -> Result<(), GhError> {
        self.record(format!(
            "run_pr_action(owner={owner:?}, repo={repo:?}, number={number:?}, action={action:?})"
        ))
        .run_pr_action
        .clone()
        .unwrap_or_else(|| Err(unscripted("run_pr_action")))
    }

    fn delete_remote_branch(
        &self,
        owner: &str,
        repo: &str,
        branch: &str,
        expected_tip: &str,
    ) -> Result<(), GhError> {
        self.record(format!(
            "delete_remote_branch(owner={owner:?}, repo={repo:?}, branch={branch:?}, expected_tip={expected_tip:?})"
        ))
        .delete_remote_branch
        .clone()
        .unwrap_or_else(|| Err(unscripted("delete_remote_branch")))
    }

    fn pr_state(&self, owner: &str, repo: &str, number: u64) -> Result<String, GhError> {
        self.record(format!(
            "pr_state(owner={owner:?}, repo={repo:?}, number={number:?})"
        ))
        .pr_state
        .clone()
        .unwrap_or_else(|| Err(unscripted("pr_state")))
    }

    fn base_has_merge_queue(&self, owner: &str, repo: &str, base: &str) -> Result<bool, GhError> {
        self.record(format!(
            "base_has_merge_queue(owner={owner:?}, repo={repo:?}, base={base:?})"
        ))
        .base_has_merge_queue
        .clone()
        .unwrap_or_else(|| Err(unscripted("base_has_merge_queue")))
    }

    fn pr_in_merge_queue(&self, owner: &str, repo: &str, number: u64) -> Result<bool, GhError> {
        self.record(format!(
            "pr_in_merge_queue(owner={owner:?}, repo={repo:?}, number={number:?})"
        ))
        .pr_in_merge_queue
        .clone()
        .unwrap_or_else(|| Err(unscripted("pr_in_merge_queue")))
    }

    fn viewer_can_bypass(
        &self,
        owner: &str,
        repo: &str,
        base: &str,
        number: u64,
    ) -> Result<bool, GhError> {
        self.record(format!(
            "viewer_can_bypass(owner={owner:?}, repo={repo:?}, base={base:?}, number={number:?})"
        ))
        .viewer_can_bypass
        .clone()
        .unwrap_or_else(|| Err(unscripted("viewer_can_bypass")))
    }

    fn restore_remote_branch(
        &self,
        owner: &str,
        repo: &str,
        branch: &str,
        sha: &str,
    ) -> Result<(), GhError> {
        self.record(format!(
            "restore_remote_branch(owner={owner:?}, repo={repo:?}, branch={branch:?}, sha={sha:?})"
        ))
        .restore_remote_branch
        .clone()
        .unwrap_or_else(|| Err(unscripted("restore_remote_branch")))
    }

    fn remote_branch_exists(&self, owner: &str, repo: &str, branch: &str) -> Result<bool, GhError> {
        self.record(format!(
            "remote_branch_exists(owner={owner:?}, repo={repo:?}, branch={branch:?})"
        ))
        .remote_branch_exists
        .clone()
        .unwrap_or_else(|| Err(unscripted("remote_branch_exists")))
    }

    fn repo_merge_settings(&self, owner: &str, repo: &str) -> Result<RepoMergeSettings, GhError> {
        self.record(format!(
            "repo_merge_settings(owner={owner:?}, repo={repo:?})"
        ))
        .repo_merge_settings
        .clone()
        .unwrap_or_else(|| Err(unscripted("repo_merge_settings")))
    }

    fn cached_base_has_merge_queue(&self, owner: &str, repo: &str, base: &str) -> Option<bool> {
        self.record(format!(
            "cached_base_has_merge_queue(owner={owner:?}, repo={repo:?}, base={base:?})"
        ))
        .cached_base_has_merge_queue
        .unwrap_or_else(|| panic!("FakeGitHub: unscripted cached_base_has_merge_queue"))
    }

    fn cached_viewer_can_bypass(
        &self,
        owner: &str,
        repo: &str,
        base: &str,
        number: u64,
    ) -> Option<bool> {
        self.record(format!(
            "cached_viewer_can_bypass(owner={owner:?}, repo={repo:?}, base={base:?}, number={number:?})"
        ))
        .cached_viewer_can_bypass
        .unwrap_or_else(|| panic!("FakeGitHub: unscripted cached_viewer_can_bypass"))
    }

    fn cached_remote_branch_exists(
        &self,
        owner: &str,
        repo: &str,
        branch: &str,
        deleting_itself: bool,
    ) -> Option<bool> {
        self.record(format!(
            "cached_remote_branch_exists(owner={owner:?}, repo={repo:?}, branch={branch:?}, deleting_itself={deleting_itself:?})"
        ))
        .cached_remote_branch_exists
        .unwrap_or_else(|| panic!("FakeGitHub: unscripted cached_remote_branch_exists"))
    }

    fn cached_repo_merge_settings(&self, owner: &str, repo: &str) -> Option<RepoMergeSettings> {
        self.record(format!(
            "cached_repo_merge_settings(owner={owner:?}, repo={repo:?})"
        ))
        .cached_repo_merge_settings
        .clone()
        .unwrap_or_else(|| panic!("FakeGitHub: unscripted cached_repo_merge_settings"))
    }

    fn pending_review(&self, owner: &str, repo: &str, pr: u64) -> Result<PendingReview, GhError> {
        self.record(format!(
            "pending_review(owner={owner:?}, repo={repo:?}, pr={pr:?})"
        ))
        .pending_review
        .clone()
        .unwrap_or_else(|| Err(unscripted("pending_review")))
    }

    fn pending_review_add_comment(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        entry: &ReviewBatchEntry,
    ) -> Result<(), GhError> {
        self.record(format!(
            "pending_review_add_comment(owner={owner:?}, repo={repo:?}, pr={pr:?}, entry={entry:?})"
        ))
        .pending_review_add_comment
        .clone()
        .unwrap_or_else(|| Err(unscripted("pending_review_add_comment")))
    }

    fn pending_review_remove_comment(&self, comment_node_id: &str) -> Result<(), GhError> {
        self.record(format!(
            "pending_review_remove_comment(comment_node_id={comment_node_id:?})"
        ))
        .pending_review_remove_comment
        .clone()
        .unwrap_or_else(|| Err(unscripted("pending_review_remove_comment")))
    }

    fn pending_review_submit(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        event: &str,
        body: &str,
    ) -> Result<usize, GhError> {
        self.record(format!(
            "pending_review_submit(owner={owner:?}, repo={repo:?}, pr={pr:?}, event={event:?}, body={body:?})"
        ))
        .pending_review_submit
        .clone()
        .unwrap_or_else(|| Err(unscripted("pending_review_submit")))
    }

    fn submit_review(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        comments: &[ReviewBatchEntry],
        repo_root: &str,
        event: &str,
        body: &str,
    ) -> Result<(), GhError> {
        self.record(format!(
            "submit_review(owner={owner:?}, repo={repo:?}, pr={pr:?}, comments={comments:?}, repo_root={repo_root:?}, event={event:?}, body={body:?})"
        ))
        .submit_review
        .clone()
        .unwrap_or_else(|| Err(unscripted("submit_review")))
    }

    fn submit_review_remote(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        comments: &[ReviewBatchEntry],
        event: &str,
        body: &str,
    ) -> Result<(), GhError> {
        self.record(format!(
            "submit_review_remote(owner={owner:?}, repo={repo:?}, pr={pr:?}, comments={comments:?}, event={event:?}, body={body:?})"
        ))
        .submit_review_remote
        .clone()
        .unwrap_or_else(|| Err(unscripted("submit_review_remote")))
    }

    fn pr_head_oid_remote(&self, owner: &str, repo: &str, pr: u64) -> Result<String, GhError> {
        self.record(format!(
            "pr_head_oid_remote(owner={owner:?}, repo={repo:?}, pr={pr:?})"
        ))
        .pr_head_oid_remote
        .clone()
        .unwrap_or_else(|| Err(unscripted("pr_head_oid_remote")))
    }

    fn get_repo_info(
        &self,
        _git: &dyn er_git::Git,
        repo_root: &str,
    ) -> Result<(String, String), GhError> {
        self.record(format!("get_repo_info(repo_root={repo_root:?})"))
            .get_repo_info
            .clone()
            .unwrap_or_else(|| Err(unscripted("get_repo_info")))
    }

    fn pr_comments(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        repo_root: &str,
    ) -> Result<Vec<GitHubComment>, GhError> {
        self.record(format!(
            "pr_comments(owner={owner:?}, repo={repo:?}, pr={pr:?}, repo_root={repo_root:?})"
        ))
        .pr_comments
        .clone()
        .unwrap_or_else(|| Err(unscripted("pr_comments")))
    }

    fn pr_comments_remote(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
    ) -> Result<Vec<GitHubComment>, GhError> {
        self.record(format!(
            "pr_comments_remote(owner={owner:?}, repo={repo:?}, pr={pr:?})"
        ))
        .pr_comments_remote
        .clone()
        .unwrap_or_else(|| Err(unscripted("pr_comments_remote")))
    }

    fn review_threads(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        repo_root: &str,
    ) -> Result<HashMap<u64, ReviewThreadState>, GhError> {
        self.record(format!(
            "review_threads(owner={owner:?}, repo={repo:?}, pr={pr:?}, repo_root={repo_root:?})"
        ))
        .review_threads
        .clone()
        .unwrap_or_else(|| Err(unscripted("review_threads")))
    }

    fn review_threads_remote(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
    ) -> Result<HashMap<u64, ReviewThreadState>, GhError> {
        self.record(format!(
            "review_threads_remote(owner={owner:?}, repo={repo:?}, pr={pr:?})"
        ))
        .review_threads_remote
        .clone()
        .unwrap_or_else(|| Err(unscripted("review_threads_remote")))
    }

    fn pr_head_oid(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        repo_root: Option<&str>,
    ) -> Result<String, GhError> {
        self.record(format!(
            "pr_head_oid(owner={owner:?}, repo={repo:?}, pr={pr:?}, repo_root={repo_root:?})"
        ))
        .pr_head_oid
        .clone()
        .unwrap_or_else(|| Err(unscripted("pr_head_oid")))
    }

    fn post_line_comment(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        comment: NewLineComment<'_>,
        commit_id: &str,
        repo_root: Option<&str>,
    ) -> Result<CreatedComment, GhError> {
        self.record(format!(
            "post_line_comment(owner={owner:?}, repo={repo:?}, pr={pr:?}, comment={comment:?}, commit_id={commit_id:?}, repo_root={repo_root:?})"
        ))
        .post_line_comment
        .clone()
        .unwrap_or_else(|| Err(unscripted("post_line_comment")))
    }

    fn post_reply(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        in_reply_to: u64,
        body: &str,
        repo_root: Option<&str>,
    ) -> Result<CreatedComment, GhError> {
        self.record(format!(
            "post_reply(owner={owner:?}, repo={repo:?}, pr={pr:?}, in_reply_to={in_reply_to:?}, body={body:?}, repo_root={repo_root:?})"
        ))
        .post_reply
        .clone()
        .unwrap_or_else(|| Err(unscripted("post_reply")))
    }

    fn post_general(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        body: &str,
        repo_root: Option<&str>,
    ) -> Result<CreatedComment, GhError> {
        self.record(format!(
            "post_general(owner={owner:?}, repo={repo:?}, pr={pr:?}, body={body:?}, repo_root={repo_root:?})"
        ))
        .post_general
        .clone()
        .unwrap_or_else(|| Err(unscripted("post_general")))
    }

    fn issue_comments(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        repo_root: Option<&str>,
    ) -> Result<Vec<IssueComment>, GhError> {
        self.record(format!(
            "issue_comments(owner={owner:?}, repo={repo:?}, pr={pr:?}, repo_root={repo_root:?})"
        ))
        .issue_comments
        .clone()
        .unwrap_or_else(|| Err(unscripted("issue_comments")))
    }

    fn viewer_login(&self, repo_root: Option<&str>) -> Result<String, GhError> {
        self.record(format!("viewer_login(repo_root={repo_root:?})"))
            .viewer_login
            .clone()
            .unwrap_or_else(|| Err(unscripted("viewer_login")))
    }

    fn push_comment(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        comment: NewLineComment<'_>,
        repo_root: &str,
    ) -> Result<u64, GhError> {
        self.record(format!(
            "push_comment(owner={owner:?}, repo={repo:?}, pr={pr:?}, comment={comment:?}, repo_root={repo_root:?})"
        ))
        .push_comment
        .clone()
        .unwrap_or_else(|| Err(unscripted("push_comment")))
    }

    fn push_comment_remote(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        comment: NewLineComment<'_>,
    ) -> Result<u64, GhError> {
        self.record(format!(
            "push_comment_remote(owner={owner:?}, repo={repo:?}, pr={pr:?}, comment={comment:?})"
        ))
        .push_comment_remote
        .clone()
        .unwrap_or_else(|| Err(unscripted("push_comment_remote")))
    }

    fn reply_comment(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        in_reply_to: u64,
        body: &str,
        repo_root: &str,
    ) -> Result<u64, GhError> {
        self.record(format!(
            "reply_comment(owner={owner:?}, repo={repo:?}, pr={pr:?}, in_reply_to={in_reply_to:?}, body={body:?}, repo_root={repo_root:?})"
        ))
        .reply_comment
        .clone()
        .unwrap_or_else(|| Err(unscripted("reply_comment")))
    }

    fn reply_comment_remote(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        in_reply_to: u64,
        body: &str,
    ) -> Result<u64, GhError> {
        self.record(format!(
            "reply_comment_remote(owner={owner:?}, repo={repo:?}, pr={pr:?}, in_reply_to={in_reply_to:?}, body={body:?})"
        ))
        .reply_comment_remote
        .clone()
        .unwrap_or_else(|| Err(unscripted("reply_comment_remote")))
    }

    fn update_review_comment(
        &self,
        owner: &str,
        repo: &str,
        comment_id: u64,
        body: &str,
        repo_root: &str,
    ) -> Result<(), GhError> {
        self.record(format!(
            "update_review_comment(owner={owner:?}, repo={repo:?}, comment_id={comment_id:?}, body={body:?}, repo_root={repo_root:?})"
        ))
        .update_review_comment
        .clone()
        .unwrap_or_else(|| Err(unscripted("update_review_comment")))
    }

    fn delete_comment(
        &self,
        owner: &str,
        repo: &str,
        comment_id: u64,
        repo_root: &str,
    ) -> Result<(), GhError> {
        self.record(format!(
            "delete_comment(owner={owner:?}, repo={repo:?}, comment_id={comment_id:?}, repo_root={repo_root:?})"
        ))
        .delete_comment
        .clone()
        .unwrap_or_else(|| Err(unscripted("delete_comment")))
    }

    fn edit_body(&self, repo_root: &str, body: &str) -> Result<(), GhError> {
        self.record(format!("edit_body(repo_root={repo_root:?}, body={body:?})"))
            .edit_body
            .clone()
            .unwrap_or_else(|| Err(unscripted("edit_body")))
    }

    fn approve(
        &self,
        repo_root: &str,
        remote_repo: Option<&str>,
        pr_number: Option<u64>,
    ) -> Result<(), GhError> {
        self.record(format!(
            "approve(repo_root={repo_root:?}, remote_repo={remote_repo:?}, pr_number={pr_number:?})"
        ))
        .approve
        .clone()
        .unwrap_or_else(|| Err(unscripted("approve")))
    }

    fn fetch_pr_comment_bundle(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        repo_root: Option<&str>,
    ) -> Result<PrCommentBundle, GhError> {
        self.record(format!(
            "fetch_pr_comment_bundle(owner={owner:?}, repo={repo:?}, pr={pr:?}, repo_root={repo_root:?})"
        ))
        .fetch_pr_comment_bundle
        .clone()
        .unwrap_or_else(|| Err(unscripted("fetch_pr_comment_bundle")))
    }

    fn pr_comment_bundle_cached(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        repo_root: Option<&str>,
    ) -> Result<Arc<PrCommentBundle>, GhError> {
        self.record(format!(
            "pr_comment_bundle_cached(owner={owner:?}, repo={repo:?}, pr={pr:?}, repo_root={repo_root:?})"
        ))
        .pr_comment_bundle_cached
        .clone()
        .unwrap_or_else(|| Err(unscripted("pr_comment_bundle_cached")))
    }

    fn invalidate_comments(&self) {
        drop(self.record("invalidate_comments()".to_string()));
    }

    fn overview(
        &self,
        repo_root: &str,
        pr_number: Option<u64>,
    ) -> Result<Option<PrOverviewData>, GhError> {
        self.record(format!(
            "overview(repo_root={repo_root:?}, pr_number={pr_number:?})"
        ))
        .overview
        .clone()
        .unwrap_or_else(|| Err(unscripted("overview")))
    }

    fn overview_no_checks(
        &self,
        repo_root: &str,
        pr_number: Option<u64>,
    ) -> Result<Option<PrOverviewData>, GhError> {
        self.record(format!(
            "overview_no_checks(repo_root={repo_root:?}, pr_number={pr_number:?})"
        ))
        .overview_no_checks
        .clone()
        .unwrap_or_else(|| Err(unscripted("overview_no_checks")))
    }

    fn overview_remote(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<Option<PrOverviewData>, GhError> {
        self.record(format!(
            "overview_remote(owner={owner:?}, repo={repo:?}, number={number:?})"
        ))
        .overview_remote
        .clone()
        .unwrap_or_else(|| Err(unscripted("overview_remote")))
    }

    fn api_get_bounded(&self, path: &str, max_bytes: usize) -> Result<Vec<u8>, GhError> {
        self.record(format!(
            "api_get_bounded(path={path:?}, max_bytes={max_bytes:?})"
        ))
        .api_get_bounded
        .clone()
        .unwrap_or_else(|| Err(unscripted("api_get_bounded")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_github_records_calls_in_order() {
        let fake = FakeGitHub::default()
            .with_pr_state(Ok("OPEN".into()))
            .with_pr_head_oid_remote(Ok("abc".into()));
        assert_eq!(fake.pr_state("o", "r", 7).unwrap(), "OPEN");
        assert_eq!(fake.pr_head_oid_remote("o", "r", 7).unwrap(), "abc");
        assert_eq!(
            fake.calls(),
            vec![
                r#"pr_state(owner="o", repo="r", number=7)"#.to_string(),
                r#"pr_head_oid_remote(owner="o", repo="r", pr=7)"#.to_string(),
            ]
        );
    }

    #[test]
    fn fake_github_unscripted_method_returns_local_error() {
        let fake = FakeGitHub::default();
        let error = fake.pr_state("o", "r", 7).unwrap_err();
        assert_eq!(
            error,
            GhError::Local {
                message: "unscripted pr_state".into()
            }
        );
    }
}
