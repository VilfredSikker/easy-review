//! GitHub access behind a port.
//!
//! GitHub access shells out to `gh` (docs/adr/0026-gh-cli-not-http-api.md).
//! This crate owns the subprocess behind the [`GitHub`] trait: [`CliGitHub`]
//! runs the real binary, [`FakeGitHub`] answers from a script, and
//! `classify` turns exit codes and `gh api --include` status lines into
//! [`GhError`] in one place (docs/adr/0055-gh-errors-are-classified-by-http-status-then-stderr.md).

pub mod actions;
mod cache;
mod checkout;
pub mod classify;
mod cli;
mod comments;
mod commits;
mod diff;
mod error;
mod fake;
mod lookup;
mod overview;
pub mod pending_review;
mod repo;
pub mod review;
mod runner;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
pub mod types;

pub use actions::{
    cached_base_has_merge_queue, cached_remote_branch_exists, cached_repo_merge_settings,
    cached_viewer_can_bypass, gh_delete_remote_branch, gh_pr_in_merge_queue, gh_pr_state,
    gh_restore_remote_branch, run_pr_action, MergeMethod, PrAction, RepoMergeSettings,
};
pub use checkout::*;
pub use classify::{classify, is_unknown_command, split_pages, GhHeaders, GhOutput};
pub use cli::{Caches, CliGitHub};
pub use comments::*;
pub use commits::*;
pub use diff::*;
pub use error::GhError;
pub use fake::FakeGitHub;
pub use lookup::*;
pub use overview::*;
pub use pending_review::{
    gh_pending_review, gh_pending_review_add_comment, gh_pending_review_remove_comment,
    gh_pending_review_submit, review_body_required, PendingReview, PendingReviewComment,
    ReviewRequest,
};
pub use repo::*;
pub use review::{
    gh_pr_head_oid_remote, gh_pr_submit_review, gh_pr_submit_review_remote, ReviewBatchEntry,
};
pub use runner::{GhCall, GhRunner, ProcessRunner};
pub use types::{parse_github_updated_at, QueuePr, ThreadAddressingSummary};

pub mod testing {
    pub use crate::runner::testing::ScriptedRunner;
}

use std::collections::HashMap;
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

    fn get_repo_info(
        &self,
        git: &dyn er_git::Git,
        repo_root: &str,
    ) -> Result<(String, String), GhError>;

    /// The local checkout to run a remote PR review in, when `repo_root`'s
    /// `origin` is the same GitHub repository the PR belongs to (`remote_repo`
    /// is an `"owner/repo"` slug).
    ///
    /// Remote reviews otherwise run from the managed artifact directory with no
    /// repository around them, which pushes the agent into hunting the
    /// filesystem for a copy of the code. Returns `None` whenever the match
    /// cannot be established — no origin, unparseable remote, or a different
    /// repository — so the caller keeps the artifact-dir behaviour.
    fn local_checkout_for_repo(
        &self,
        git: &dyn er_git::Git,
        repo_root: &str,
        remote_repo: &str,
    ) -> Option<String> {
        if repo_root.is_empty() || remote_repo.is_empty() {
            return None;
        }
        let (owner, repo) = self.get_repo_info(git, repo_root).ok()?;
        crate::repo::owner_repo_matches_slug(&owner, &repo, remote_repo)
            .then(|| repo_root.to_string())
    }

    fn pr_comments(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        repo_root: &str,
    ) -> Result<Vec<GitHubComment>, GhError>;
    fn pr_comments_remote(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
    ) -> Result<Vec<GitHubComment>, GhError>;
    fn review_threads(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        repo_root: &str,
    ) -> Result<HashMap<u64, ReviewThreadState>, GhError>;
    fn review_threads_remote(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
    ) -> Result<HashMap<u64, ReviewThreadState>, GhError>;
    fn pr_head_oid(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        repo_root: Option<&str>,
    ) -> Result<String, GhError>;
    fn post_line_comment(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        comment: NewLineComment<'_>,
        commit_id: &str,
        repo_root: Option<&str>,
    ) -> Result<CreatedComment, GhError>;
    fn post_reply(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        in_reply_to: u64,
        body: &str,
        repo_root: Option<&str>,
    ) -> Result<CreatedComment, GhError>;
    fn post_general(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        body: &str,
        repo_root: Option<&str>,
    ) -> Result<CreatedComment, GhError>;
    fn issue_comments(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        repo_root: Option<&str>,
    ) -> Result<Vec<IssueComment>, GhError>;
    fn viewer_login(&self, repo_root: Option<&str>) -> Result<String, GhError>;
    fn push_comment(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        comment: NewLineComment<'_>,
        repo_root: &str,
    ) -> Result<u64, GhError>;
    fn push_comment_remote(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        comment: NewLineComment<'_>,
    ) -> Result<u64, GhError>;
    fn reply_comment(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        in_reply_to: u64,
        body: &str,
        repo_root: &str,
    ) -> Result<u64, GhError>;
    fn reply_comment_remote(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        in_reply_to: u64,
        body: &str,
    ) -> Result<u64, GhError>;
    fn update_review_comment(
        &self,
        owner: &str,
        repo: &str,
        comment_id: u64,
        body: &str,
        repo_root: &str,
    ) -> Result<(), GhError>;
    fn delete_comment(
        &self,
        owner: &str,
        repo: &str,
        comment_id: u64,
        repo_root: &str,
    ) -> Result<(), GhError>;
    fn edit_body(&self, repo_root: &str, body: &str) -> Result<(), GhError>;
    fn approve(
        &self,
        repo_root: &str,
        remote_repo: Option<&str>,
        pr_number: Option<u64>,
    ) -> Result<(), GhError>;
    fn fetch_pr_comment_bundle(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        repo_root: Option<&str>,
    ) -> Result<PrCommentBundle, GhError>;
    fn pr_comment_bundle_cached(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        repo_root: Option<&str>,
    ) -> Result<Arc<PrCommentBundle>, GhError>;
    fn invalidate_comments(&self);

    fn overview(&self, repo_root: &str, pr_number: Option<u64>) -> Option<PrOverviewData>;
    fn overview_no_checks(&self, repo_root: &str, pr_number: Option<u64>)
        -> Option<PrOverviewData>;
    fn overview_remote(&self, owner: &str, repo: &str, number: u64) -> Option<PrOverviewData>;

    /// One bounded `gh api` read whose body is returned without headers.
    fn api_get_bounded(&self, path: &str, max_bytes: usize) -> Result<Vec<u8>, GhError>;

    fn fetch_pr_base_ref(
        &self,
        git: &dyn er_git::Git,
        repo_root: &str,
        pr_number: u64,
        base_branch: &str,
    ) -> Result<String, GhError>;
    fn pr_head_branch_name(&self, number: u64, root: &str) -> Result<String, GhError>;

    fn pr_size_check_remote(&self, owner: &str, repo: &str, number: u64) -> Result<(), GhError>;
    fn pr_diff_remote(
        &self,
        git: &dyn er_git::Git,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<String, GhError>;
    fn pr_diff(
        &self,
        git: &dyn er_git::Git,
        pr_number: u64,
        repo_root: &str,
    ) -> Result<String, GhError>;
    fn commit_shas_remote(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<(String, String), GhError>;
    fn file_at_ref(
        &self,
        repo_slug: &str,
        path: &str,
        git_ref: &str,
        max_bytes: usize,
    ) -> Result<Vec<u8>, GhError>;
    fn pr_commits(
        &self,
        repo_root: &str,
        number: u64,
        limit: usize,
    ) -> Result<Vec<er_git::CommitInfo>, GhError>;
    fn pr_base_branch_and_commits(
        &self,
        repo_root: &str,
        number: u64,
        limit: usize,
    ) -> Result<(String, Vec<er_git::CommitInfo>), GhError>;
    fn commits_remote(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
        limit: usize,
    ) -> Result<Vec<er_git::CommitInfo>, GhError>;

    fn ensure_installed(&self) -> Result<(), GhError>;
    fn list_participating_notifications(&self) -> Result<Vec<GhNotification>, GhError>;
    fn pr_base_branch(&self, pr_number: u64, repo_root: &str) -> Result<String, GhError>;
    fn pr_branch_names(&self, pr_number: u64, repo_root: &str)
        -> Result<(String, String), GhError>;
    fn probe_pr_for_current_branch(&self, repo_root: &str) -> BranchPrProbe;
    fn pr_for_current_branch(&self, repo_root: &str) -> Option<(u64, String)>;
    fn lookup_pr_for_current_branch(&self, git: &dyn er_git::Git, repo_root: &str) -> PrLookup;
    fn open_pr_number_for_head(
        &self,
        owner: &str,
        repo: &str,
        head: &str,
    ) -> Result<Option<u64>, GhError>;
    fn pr_title(&self, owner: &str, repo: &str, number: u64) -> Option<String>;
    fn get_pr_info(
        &self,
        git: &dyn er_git::Git,
        repo_root: &str,
    ) -> Result<(String, String, u64), GhError>;
    fn current_login(&self) -> Option<String>;
    fn stack_view_json(&self, repo_root: &str) -> Result<String, GhError>;
    fn open_pr_refs_json(&self, repo_slug: &str) -> Result<String, GhError>;
    fn pr_metadata_remote(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<(String, String), GhError>;
    fn pr_status_remote(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<PrStatusBundle, GhError>;
    fn pr_checks_remote(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<Vec<CheckRun>, GhError>;
    fn pr_checks_state_remote(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<&'static str, GhError>;
    fn pr_thread_addressing_remote(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
    ) -> Result<ThreadAddressingSummary, GhError>;
}

static DEFAULT_HANDLE: OnceLock<Arc<CliGitHub>> = OnceLock::new();

/// The process-wide handle the compatibility wrappers use. Tests build their
/// own `CliGitHub` on a `ScriptedRunner` and never reach this one.
pub fn default_handle() -> Arc<CliGitHub> {
    DEFAULT_HANDLE
        .get_or_init(|| Arc::new(CliGitHub::new(Arc::new(ProcessRunner))))
        .clone()
}
