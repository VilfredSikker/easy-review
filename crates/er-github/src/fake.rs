//! A scripted in-memory GitHub for tests.

use crate::actions::{PrAction, RepoMergeSettings};
use crate::error::GhError;
use crate::pending_review::PendingReview;
use crate::review::ReviewBatchEntry;
use crate::GitHub;
use std::sync::{Mutex, MutexGuard, PoisonError};

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
