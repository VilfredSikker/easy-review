//! A scripted in-memory GitHub for tests.

use crate::actions::{PrAction, RepoMergeSettings};
use crate::error::GhError;
use crate::pending_review::PendingReview;
use crate::review::ReviewBatchEntry;
use crate::types::ThreadAddressingSummary;
use crate::GitHub;
use crate::{
    BranchPrProbe, CheckRun, CreatedComment, GhNotification, GitHubComment, IssueComment,
    NewLineComment, PrCommentBundle, PrLookup, PrOverviewData, PrStatusBundle, ReviewThreadState,
};
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
    fetch_pr_base_ref: Option<Result<String, GhError>>,
    pr_head_branch_name: Option<Result<String, GhError>>,
    pr_size_check_remote: Option<Result<(), GhError>>,
    pr_diff_remote: Option<Result<String, GhError>>,
    pr_diff: Option<Result<String, GhError>>,
    commit_shas_remote: Option<Result<(String, String), GhError>>,
    file_at_ref: Option<Result<Vec<u8>, GhError>>,
    pr_commits: Option<Result<Vec<er_git::CommitInfo>, GhError>>,
    pr_base_branch_and_commits: Option<Result<(String, Vec<er_git::CommitInfo>), GhError>>,
    commits_remote: Option<Result<Vec<er_git::CommitInfo>, GhError>>,
    ensure_installed: Option<Result<(), GhError>>,
    list_participating_notifications: Option<Result<Vec<GhNotification>, GhError>>,
    pr_base_branch: Option<Result<String, GhError>>,
    pr_branch_names: Option<Result<(String, String), GhError>>,
    probe_pr_for_current_branch: Option<BranchPrProbe>,
    pr_for_current_branch: Option<Option<(u64, String)>>,
    lookup_pr_for_current_branch: Option<PrLookup>,
    open_pr_number_for_head: Option<Result<Option<u64>, GhError>>,
    pr_title: Option<Option<String>>,
    get_pr_info: Option<Result<(String, String, u64), GhError>>,
    current_login: Option<Option<String>>,
    stack_view_json: Option<Result<String, GhError>>,
    open_pr_refs_json: Option<Result<String, GhError>>,
    pr_metadata_remote: Option<Result<(String, String), GhError>>,
    pr_status_remote: Option<Result<PrStatusBundle, GhError>>,
    pr_checks_remote: Option<Result<Vec<CheckRun>, GhError>>,
    pr_checks_state_remote: Option<Result<&'static str, GhError>>,
    pr_thread_addressing_remote: Option<Result<ThreadAddressingSummary, GhError>>,
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

    #[must_use]
    pub fn with_fetch_pr_base_ref(self, value: Result<String, GhError>) -> Self {
        self.lock().fetch_pr_base_ref = Some(value);
        self
    }

    #[must_use]
    pub fn with_pr_head_branch_name(self, value: Result<String, GhError>) -> Self {
        self.lock().pr_head_branch_name = Some(value);
        self
    }

    #[must_use]
    pub fn with_pr_size_check_remote(self, value: Result<(), GhError>) -> Self {
        self.lock().pr_size_check_remote = Some(value);
        self
    }

    #[must_use]
    pub fn with_pr_diff_remote(self, value: Result<String, GhError>) -> Self {
        self.lock().pr_diff_remote = Some(value);
        self
    }

    #[must_use]
    pub fn with_pr_diff(self, value: Result<String, GhError>) -> Self {
        self.lock().pr_diff = Some(value);
        self
    }

    #[must_use]
    pub fn with_commit_shas_remote(self, value: Result<(String, String), GhError>) -> Self {
        self.lock().commit_shas_remote = Some(value);
        self
    }

    #[must_use]
    pub fn with_file_at_ref(self, value: Result<Vec<u8>, GhError>) -> Self {
        self.lock().file_at_ref = Some(value);
        self
    }

    #[must_use]
    pub fn with_pr_commits(self, value: Result<Vec<er_git::CommitInfo>, GhError>) -> Self {
        self.lock().pr_commits = Some(value);
        self
    }

    #[must_use]
    pub fn with_pr_base_branch_and_commits(
        self,
        value: Result<(String, Vec<er_git::CommitInfo>), GhError>,
    ) -> Self {
        self.lock().pr_base_branch_and_commits = Some(value);
        self
    }

    #[must_use]
    pub fn with_commits_remote(self, value: Result<Vec<er_git::CommitInfo>, GhError>) -> Self {
        self.lock().commits_remote = Some(value);
        self
    }

    #[must_use]
    pub fn with_ensure_installed(self, value: Result<(), GhError>) -> Self {
        self.lock().ensure_installed = Some(value);
        self
    }

    #[must_use]
    pub fn with_list_participating_notifications(
        self,
        value: Result<Vec<GhNotification>, GhError>,
    ) -> Self {
        self.lock().list_participating_notifications = Some(value);
        self
    }

    #[must_use]
    pub fn with_pr_base_branch(self, value: Result<String, GhError>) -> Self {
        self.lock().pr_base_branch = Some(value);
        self
    }

    #[must_use]
    pub fn with_pr_branch_names(self, value: Result<(String, String), GhError>) -> Self {
        self.lock().pr_branch_names = Some(value);
        self
    }

    #[must_use]
    pub fn with_probe_pr_for_current_branch(self, value: BranchPrProbe) -> Self {
        self.lock().probe_pr_for_current_branch = Some(value);
        self
    }

    #[must_use]
    pub fn with_pr_for_current_branch(self, value: Option<(u64, String)>) -> Self {
        self.lock().pr_for_current_branch = Some(value);
        self
    }

    #[must_use]
    pub fn with_lookup_pr_for_current_branch(self, value: PrLookup) -> Self {
        self.lock().lookup_pr_for_current_branch = Some(value);
        self
    }

    #[must_use]
    pub fn with_open_pr_number_for_head(self, value: Result<Option<u64>, GhError>) -> Self {
        self.lock().open_pr_number_for_head = Some(value);
        self
    }

    #[must_use]
    pub fn with_pr_title(self, value: Option<String>) -> Self {
        self.lock().pr_title = Some(value);
        self
    }

    #[must_use]
    pub fn with_get_pr_info(self, value: Result<(String, String, u64), GhError>) -> Self {
        self.lock().get_pr_info = Some(value);
        self
    }

    #[must_use]
    pub fn with_current_login(self, value: Option<String>) -> Self {
        self.lock().current_login = Some(value);
        self
    }

    #[must_use]
    pub fn with_stack_view_json(self, value: Result<String, GhError>) -> Self {
        self.lock().stack_view_json = Some(value);
        self
    }

    #[must_use]
    pub fn with_open_pr_refs_json(self, value: Result<String, GhError>) -> Self {
        self.lock().open_pr_refs_json = Some(value);
        self
    }

    #[must_use]
    pub fn with_pr_metadata_remote(self, value: Result<(String, String), GhError>) -> Self {
        self.lock().pr_metadata_remote = Some(value);
        self
    }

    #[must_use]
    pub fn with_pr_status_remote(self, value: Result<PrStatusBundle, GhError>) -> Self {
        self.lock().pr_status_remote = Some(value);
        self
    }

    #[must_use]
    pub fn with_pr_checks_remote(self, value: Result<Vec<CheckRun>, GhError>) -> Self {
        self.lock().pr_checks_remote = Some(value);
        self
    }

    #[must_use]
    pub fn with_pr_checks_state_remote(self, value: Result<&'static str, GhError>) -> Self {
        self.lock().pr_checks_state_remote = Some(value);
        self
    }

    #[must_use]
    pub fn with_pr_thread_addressing_remote(
        self,
        value: Result<ThreadAddressingSummary, GhError>,
    ) -> Self {
        self.lock().pr_thread_addressing_remote = Some(value);
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

    fn fetch_pr_base_ref(
        &self,
        _git: &dyn er_git::Git,
        repo_root: &str,
        pr_number: u64,
        base_branch: &str,
    ) -> Result<String, GhError> {
        self.record(format!(
            "fetch_pr_base_ref(repo_root={repo_root:?}, pr_number={pr_number:?}, base_branch={base_branch:?})"
        ))
        .fetch_pr_base_ref
        .clone()
        .unwrap_or_else(|| Err(unscripted("fetch_pr_base_ref")))
    }

    fn pr_head_branch_name(&self, number: u64, root: &str) -> Result<String, GhError> {
        self.record(format!(
            "pr_head_branch_name(number={number:?}, root={root:?})"
        ))
        .pr_head_branch_name
        .clone()
        .unwrap_or_else(|| Err(unscripted("pr_head_branch_name")))
    }

    fn pr_size_check_remote(&self, owner: &str, repo: &str, number: u64) -> Result<(), GhError> {
        self.record(format!(
            "pr_size_check_remote(owner={owner:?}, repo={repo:?}, number={number:?})"
        ))
        .pr_size_check_remote
        .clone()
        .unwrap_or_else(|| Err(unscripted("pr_size_check_remote")))
    }

    fn pr_diff_remote(
        &self,
        _git: &dyn er_git::Git,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<String, GhError> {
        self.record(format!(
            "pr_diff_remote(owner={owner:?}, repo={repo:?}, number={number:?})"
        ))
        .pr_diff_remote
        .clone()
        .unwrap_or_else(|| Err(unscripted("pr_diff_remote")))
    }

    fn pr_diff(
        &self,
        _git: &dyn er_git::Git,
        pr_number: u64,
        repo_root: &str,
    ) -> Result<String, GhError> {
        self.record(format!(
            "pr_diff(pr_number={pr_number:?}, repo_root={repo_root:?})"
        ))
        .pr_diff
        .clone()
        .unwrap_or_else(|| Err(unscripted("pr_diff")))
    }

    fn commit_shas_remote(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<(String, String), GhError> {
        self.record(format!(
            "commit_shas_remote(owner={owner:?}, repo={repo:?}, number={number:?})"
        ))
        .commit_shas_remote
        .clone()
        .unwrap_or_else(|| Err(unscripted("commit_shas_remote")))
    }

    fn file_at_ref(
        &self,
        repo_slug: &str,
        path: &str,
        git_ref: &str,
        max_bytes: usize,
    ) -> Result<Vec<u8>, GhError> {
        self.record(format!(
            "file_at_ref(repo_slug={repo_slug:?}, path={path:?}, git_ref={git_ref:?}, max_bytes={max_bytes:?})"
        ))
        .file_at_ref
        .clone()
        .unwrap_or_else(|| Err(unscripted("file_at_ref")))
    }

    fn pr_commits(
        &self,
        repo_root: &str,
        number: u64,
        limit: usize,
    ) -> Result<Vec<er_git::CommitInfo>, GhError> {
        self.record(format!(
            "pr_commits(repo_root={repo_root:?}, number={number:?}, limit={limit:?})"
        ))
        .pr_commits
        .clone()
        .unwrap_or_else(|| Err(unscripted("pr_commits")))
    }

    fn pr_base_branch_and_commits(
        &self,
        repo_root: &str,
        number: u64,
        limit: usize,
    ) -> Result<(String, Vec<er_git::CommitInfo>), GhError> {
        self.record(format!(
            "pr_base_branch_and_commits(repo_root={repo_root:?}, number={number:?}, limit={limit:?})"
        ))
        .pr_base_branch_and_commits
        .clone()
        .unwrap_or_else(|| Err(unscripted("pr_base_branch_and_commits")))
    }

    fn commits_remote(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
        limit: usize,
    ) -> Result<Vec<er_git::CommitInfo>, GhError> {
        self.record(format!(
            "commits_remote(owner={owner:?}, repo={repo:?}, number={number:?}, limit={limit:?})"
        ))
        .commits_remote
        .clone()
        .unwrap_or_else(|| Err(unscripted("commits_remote")))
    }

    fn ensure_installed(&self) -> Result<(), GhError> {
        self.record("ensure_installed()".to_string())
            .ensure_installed
            .clone()
            .unwrap_or_else(|| Err(unscripted("ensure_installed")))
    }

    fn list_participating_notifications(&self) -> Result<Vec<GhNotification>, GhError> {
        self.record("list_participating_notifications()".to_string())
            .list_participating_notifications
            .clone()
            .unwrap_or_else(|| Err(unscripted("list_participating_notifications")))
    }

    fn pr_base_branch(&self, pr_number: u64, repo_root: &str) -> Result<String, GhError> {
        self.record(format!(
            "pr_base_branch(pr_number={pr_number:?}, repo_root={repo_root:?})"
        ))
        .pr_base_branch
        .clone()
        .unwrap_or_else(|| Err(unscripted("pr_base_branch")))
    }

    fn pr_branch_names(
        &self,
        pr_number: u64,
        repo_root: &str,
    ) -> Result<(String, String), GhError> {
        self.record(format!(
            "pr_branch_names(pr_number={pr_number:?}, repo_root={repo_root:?})"
        ))
        .pr_branch_names
        .clone()
        .unwrap_or_else(|| Err(unscripted("pr_branch_names")))
    }

    fn probe_pr_for_current_branch(&self, repo_root: &str) -> BranchPrProbe {
        self.record(format!(
            "probe_pr_for_current_branch(repo_root={repo_root:?})"
        ))
        .probe_pr_for_current_branch
        .clone()
        .unwrap_or(BranchPrProbe::Unknown)
    }

    fn pr_for_current_branch(&self, repo_root: &str) -> Option<(u64, String)> {
        self.record(format!("pr_for_current_branch(repo_root={repo_root:?})"))
            .pr_for_current_branch
            .clone()
            .unwrap_or(None)
    }

    fn lookup_pr_for_current_branch(&self, _git: &dyn er_git::Git, repo_root: &str) -> PrLookup {
        self.record(format!(
            "lookup_pr_for_current_branch(repo_root={repo_root:?})"
        ))
        .lookup_pr_for_current_branch
        .clone()
        .unwrap_or_else(|| PrLookup::Unknown("unscripted lookup".to_string()))
    }

    fn open_pr_number_for_head(
        &self,
        owner: &str,
        repo: &str,
        head: &str,
    ) -> Result<Option<u64>, GhError> {
        self.record(format!(
            "open_pr_number_for_head(owner={owner:?}, repo={repo:?}, head={head:?})"
        ))
        .open_pr_number_for_head
        .clone()
        .unwrap_or_else(|| Err(unscripted("open_pr_number_for_head")))
    }

    fn pr_title(&self, owner: &str, repo: &str, number: u64) -> Option<String> {
        self.record(format!(
            "pr_title(owner={owner:?}, repo={repo:?}, number={number:?})"
        ))
        .pr_title
        .clone()
        .unwrap_or(None)
    }

    fn get_pr_info(
        &self,
        _git: &dyn er_git::Git,
        repo_root: &str,
    ) -> Result<(String, String, u64), GhError> {
        self.record(format!("get_pr_info(repo_root={repo_root:?})"))
            .get_pr_info
            .clone()
            .unwrap_or_else(|| Err(unscripted("get_pr_info")))
    }

    fn current_login(&self) -> Option<String> {
        self.record("current_login()".to_string())
            .current_login
            .clone()
            .unwrap_or(None)
    }

    fn stack_view_json(&self, repo_root: &str) -> Result<String, GhError> {
        self.record(format!("stack_view_json(repo_root={repo_root:?})"))
            .stack_view_json
            .clone()
            .unwrap_or_else(|| Err(unscripted("stack_view_json")))
    }

    fn open_pr_refs_json(&self, repo_slug: &str) -> Result<String, GhError> {
        self.record(format!("open_pr_refs_json(repo_slug={repo_slug:?})"))
            .open_pr_refs_json
            .clone()
            .unwrap_or_else(|| Err(unscripted("open_pr_refs_json")))
    }

    fn pr_metadata_remote(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<(String, String), GhError> {
        self.record(format!(
            "pr_metadata_remote(owner={owner:?}, repo={repo:?}, number={number:?})"
        ))
        .pr_metadata_remote
        .clone()
        .unwrap_or_else(|| Err(unscripted("pr_metadata_remote")))
    }

    fn pr_status_remote(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<PrStatusBundle, GhError> {
        self.record(format!(
            "pr_status_remote(owner={owner:?}, repo={repo:?}, number={number:?})"
        ))
        .pr_status_remote
        .clone()
        .unwrap_or_else(|| Err(unscripted("pr_status_remote")))
    }

    fn pr_checks_remote(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<Vec<CheckRun>, GhError> {
        self.record(format!(
            "pr_checks_remote(owner={owner:?}, repo={repo:?}, number={number:?})"
        ))
        .pr_checks_remote
        .clone()
        .unwrap_or_else(|| Err(unscripted("pr_checks_remote")))
    }

    fn pr_checks_state_remote(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<&'static str, GhError> {
        self.record(format!(
            "pr_checks_state_remote(owner={owner:?}, repo={repo:?}, number={number:?})"
        ))
        .pr_checks_state_remote
        .clone()
        .unwrap_or_else(|| Err(unscripted("pr_checks_state_remote")))
    }

    fn pr_thread_addressing_remote(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
    ) -> Result<ThreadAddressingSummary, GhError> {
        self.record(format!(
            "pr_thread_addressing_remote(owner={owner:?}, repo={repo:?}, pr={pr:?})"
        ))
        .pr_thread_addressing_remote
        .clone()
        .unwrap_or_else(|| Err(unscripted("pr_thread_addressing_remote")))
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
