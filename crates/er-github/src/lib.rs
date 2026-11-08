//! GitHub access behind a port.
//!
//! GitHub access shells out to `gh` (docs/adr/0026-gh-cli-not-http-api.md).
//! This crate owns the subprocess behind the [`GitHub`] trait: [`CliGitHub`]
//! runs the real binary, [`FakeGitHub`] answers from a script, and
//! `classify` turns exit codes and `gh api --include` status lines into
//! [`GhError`] in one place (docs/adr/0055-gh-errors-are-classified-by-http-status-then-stderr.md).

pub mod actions;
mod cache;
pub mod classify;
mod cli;
mod error;
mod fake;
mod legacy;
pub mod pending_review;
pub mod review;
mod runner;
#[cfg(test)]
mod test_support;
pub mod types;

pub use actions::{
    cached_base_has_merge_queue, cached_remote_branch_exists, cached_repo_merge_settings,
    cached_viewer_can_bypass, gh_delete_remote_branch, gh_pr_in_merge_queue, gh_pr_state,
    gh_restore_remote_branch, run_pr_action, MergeMethod, PrAction, RepoMergeSettings,
};
pub use classify::{classify, is_unknown_command, split_pages, GhHeaders, GhOutput};
pub use cli::{Caches, CliGitHub};
pub use error::GhError;
pub use fake::FakeGitHub;
pub use legacy::*;
pub use pending_review::{
    gh_pending_review, gh_pending_review_add_comment, gh_pending_review_remove_comment,
    gh_pending_review_submit, review_body_required, PendingReview, PendingReviewComment,
    ReviewRequest,
};
pub use review::{
    gh_pr_head_oid_remote, gh_pr_submit_review, gh_pr_submit_review_remote, ReviewBatchEntry,
};
pub use runner::{GhCall, GhRunner, ProcessRunner};
pub use types::{parse_github_updated_at, QueuePr, ThreadAddressingSummary};

pub mod testing {
    pub use crate::runner::testing::ScriptedRunner;
}

use std::sync::{Arc, OnceLock};

/// One method per public function that spawns `gh`.
pub trait GitHub: Send + Sync {
    fn run_pr_action(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
        action: &PrAction,
    ) -> Result<(), GhError>;
    fn delete_remote_branch(
        &self,
        owner: &str,
        repo: &str,
        branch: &str,
        expected_tip: &str,
    ) -> Result<(), GhError>;
    fn pr_state(&self, owner: &str, repo: &str, number: u64) -> Result<String, GhError>;
    fn base_has_merge_queue(&self, owner: &str, repo: &str, base: &str) -> Result<bool, GhError>;
    fn pr_in_merge_queue(&self, owner: &str, repo: &str, number: u64) -> Result<bool, GhError>;
    fn viewer_can_bypass(
        &self,
        owner: &str,
        repo: &str,
        base: &str,
        number: u64,
    ) -> Result<bool, GhError>;
    fn restore_remote_branch(
        &self,
        owner: &str,
        repo: &str,
        branch: &str,
        sha: &str,
    ) -> Result<(), GhError>;
    fn remote_branch_exists(&self, owner: &str, repo: &str, branch: &str) -> Result<bool, GhError>;
    fn repo_merge_settings(&self, owner: &str, repo: &str) -> Result<RepoMergeSettings, GhError>;
    fn cached_base_has_merge_queue(&self, owner: &str, repo: &str, base: &str) -> Option<bool>;
    fn cached_viewer_can_bypass(
        &self,
        owner: &str,
        repo: &str,
        base: &str,
        number: u64,
    ) -> Option<bool>;
    fn cached_remote_branch_exists(
        &self,
        owner: &str,
        repo: &str,
        branch: &str,
        deleting_itself: bool,
    ) -> Option<bool>;
    fn cached_repo_merge_settings(&self, owner: &str, repo: &str) -> Option<RepoMergeSettings>;

    fn pending_review(&self, owner: &str, repo: &str, pr: u64) -> Result<PendingReview, GhError>;
    fn pending_review_add_comment(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        entry: &ReviewBatchEntry,
    ) -> Result<(), GhError>;
    fn pending_review_remove_comment(&self, comment_node_id: &str) -> Result<(), GhError>;
    fn pending_review_submit(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        event: &str,
        body: &str,
    ) -> Result<usize, GhError>;
    #[expect(
        clippy::too_many_arguments,
        reason = "the review inputs travel together and match the wrapper's signature"
    )]
    fn submit_review(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        comments: &[ReviewBatchEntry],
        repo_root: &str,
        event: &str,
        body: &str,
    ) -> Result<(), GhError>;
    fn submit_review_remote(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        comments: &[ReviewBatchEntry],
        event: &str,
        body: &str,
    ) -> Result<(), GhError>;
    fn pr_head_oid_remote(&self, owner: &str, repo: &str, pr: u64) -> Result<String, GhError>;
}

static DEFAULT_HANDLE: OnceLock<Arc<CliGitHub>> = OnceLock::new();

/// The process-wide handle the compatibility wrappers use. Tests build their
/// own `CliGitHub` on a `ScriptedRunner` and never reach this one.
pub fn default_handle() -> Arc<CliGitHub> {
    DEFAULT_HANDLE
        .get_or_init(|| Arc::new(CliGitHub::new(Arc::new(ProcessRunner))))
        .clone()
}
