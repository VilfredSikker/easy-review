//! The CLI adapter: one seam to the runner and one set of caches.

use std::sync::Arc;
use std::time::Duration;

use crate::actions::{PrAction, RepoMergeSettings};
use crate::cache::TtlCache;
use crate::classify::{classify, GhOutput};
use crate::error::GhError;
use crate::pending_review::PendingReview;
use crate::review::ReviewBatchEntry;
use crate::runner::{GhCall, GhRunner};
use crate::types::ThreadAddressingSummary;
use crate::GitHub;
use crate::{
    BranchPrProbe, CheckRun, CreatedComment, GhNotification, GitHubComment, IssueComment,
    NewLineComment, PrCommentBundle, PrLookup, PrOverviewData, PrStatusBundle, ReviewThreadState,
};
use std::collections::HashMap;

/// The caches `CliGitHub` owns. Each has its own generation counter.
pub struct Caches {
    /// Origin `owner/repo` per repo root. `git remote get-url` runs on every
    /// agent spawn of a remote PR via `local_checkout_for_repo`; the remote is
    /// effectively static, so a short TTL keeps the subprocess off the hot
    /// path.
    pub repo_info: TtlCache<String, Arc<(String, String)>>,
    /// PR comment sync bundle, warmed by the sidebar hover prefetch.
    pub pr_comments: TtlCache<(String, String, u64), Arc<crate::comments::PrCommentBundle>>,
    pub merge_queue: TtlCache<String, bool>,
    pub bypass: TtlCache<String, bool>,
    pub branch_exists: TtlCache<String, bool>,
    pub repo_settings: TtlCache<String, crate::actions::RepoMergeSettings>,
}

impl Caches {
    pub fn new() -> Self {
        // Repo settings rarely change, and the status loop runs every 30s; one
        // query per repo per TTL keeps it off the rate limit (ADR 0026).
        Self {
            repo_info: TtlCache::new(Duration::from_secs(60), Some(64)),
            pr_comments: TtlCache::new(Duration::from_secs(60), Some(32)),
            merge_queue: TtlCache::new(Duration::from_mins(15), None),
            bypass: TtlCache::new(Duration::from_mins(15), None),
            // A merged PR's branch rarely changes, but the status loop asks
            // every 30s. Delete and restore update the entry, so the box
            // reflects its own actions at once (ADR 0026).
            branch_exists: TtlCache::new(Duration::from_mins(5), None),
            repo_settings: TtlCache::new(Duration::from_mins(15), None),
        }
    }
}

impl Default for Caches {
    fn default() -> Self {
        Self::new()
    }
}

pub struct CliGitHub {
    runner: Arc<dyn GhRunner>,
    caches: Caches,
}

impl CliGitHub {
    pub fn new(runner: Arc<dyn GhRunner>) -> Self {
        Self {
            runner,
            caches: Caches::new(),
        }
    }

    pub(crate) fn cache(&self) -> &Caches {
        &self.caches
    }

    /// The only path to the runner. `gh api` calls get `--include`, so the
    /// status line and headers are available to the classifier.
    pub(crate) fn run_gh(
        &self,
        args: &[String],
        dir: Option<&str>,
        max_stdout: Option<usize>,
    ) -> Result<GhOutput, GhError> {
        let include = args.first().is_some_and(|arg| arg == "api");
        let paginated = args.iter().any(|arg| arg == "--paginate");
        let mut final_args = args.to_vec();
        if include {
            final_args.insert(1, "--include".to_string());
        }
        let call = GhCall {
            args: &final_args,
            dir,
            timeout: er_git::proc::GH_TIMEOUT,
            max_stdout,
        };
        let output = classify(self.runner.run(&call), include, paginated)?;
        if let Some(cap) = max_stdout {
            if output.body.len() > cap {
                return Err(GhError::TooLarge {
                    message: "command output exceeds byte limit".into(),
                });
            }
        }
        Ok(output)
    }

    pub(crate) fn gh_ok(&self, args: &[String], dir: Option<&str>) -> Result<String, GhError> {
        let output = self.run_gh(args, dir, None)?;
        Ok(String::from_utf8_lossy(&output.body).into_owned())
    }

    /// The moved bodies spell their arguments as literals.
    pub(crate) fn run_gh_args(
        &self,
        args: &[&str],
        dir: Option<&str>,
    ) -> Result<GhOutput, GhError> {
        let args: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
        self.run_gh(&args, dir, None)
    }

    pub(crate) fn graphql(
        &self,
        query: &str,
        strings: &[(&str, &str)],
        typed: &[(&str, &str)],
    ) -> Result<String, GhError> {
        self.gh_ok(&graphql_args(query, strings, typed), None)
    }
}

impl GitHub for CliGitHub {
    fn run_pr_action(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
        action: &PrAction,
    ) -> Result<(), GhError> {
        CliGitHub::run_pr_action(self, owner, repo, number, action)
    }

    fn delete_remote_branch(
        &self,
        owner: &str,
        repo: &str,
        branch: &str,
        expected_tip: &str,
    ) -> Result<(), GhError> {
        CliGitHub::delete_remote_branch(self, owner, repo, branch, expected_tip)
    }

    fn pr_state(&self, owner: &str, repo: &str, number: u64) -> Result<String, GhError> {
        CliGitHub::pr_state(self, owner, repo, number)
    }

    fn base_has_merge_queue(&self, owner: &str, repo: &str, base: &str) -> Result<bool, GhError> {
        CliGitHub::base_has_merge_queue(self, owner, repo, base)
    }

    fn pr_in_merge_queue(&self, owner: &str, repo: &str, number: u64) -> Result<bool, GhError> {
        CliGitHub::pr_in_merge_queue(self, owner, repo, number)
    }

    fn viewer_can_bypass(
        &self,
        owner: &str,
        repo: &str,
        base: &str,
        number: u64,
    ) -> Result<bool, GhError> {
        CliGitHub::viewer_can_bypass(self, owner, repo, base, number)
    }

    fn restore_remote_branch(
        &self,
        owner: &str,
        repo: &str,
        branch: &str,
        sha: &str,
    ) -> Result<(), GhError> {
        CliGitHub::restore_remote_branch(self, owner, repo, branch, sha)
    }

    fn remote_branch_exists(&self, owner: &str, repo: &str, branch: &str) -> Result<bool, GhError> {
        CliGitHub::remote_branch_exists(self, owner, repo, branch)
    }

    fn repo_merge_settings(&self, owner: &str, repo: &str) -> Result<RepoMergeSettings, GhError> {
        CliGitHub::repo_merge_settings(self, owner, repo)
    }

    fn cached_base_has_merge_queue(&self, owner: &str, repo: &str, base: &str) -> Option<bool> {
        CliGitHub::cached_base_has_merge_queue(self, owner, repo, base)
    }

    fn cached_viewer_can_bypass(
        &self,
        owner: &str,
        repo: &str,
        base: &str,
        number: u64,
    ) -> Option<bool> {
        CliGitHub::cached_viewer_can_bypass(self, owner, repo, base, number)
    }

    fn cached_remote_branch_exists(
        &self,
        owner: &str,
        repo: &str,
        branch: &str,
        deleting_itself: bool,
    ) -> Option<bool> {
        CliGitHub::cached_remote_branch_exists(self, owner, repo, branch, deleting_itself)
    }

    fn cached_repo_merge_settings(&self, owner: &str, repo: &str) -> Option<RepoMergeSettings> {
        CliGitHub::cached_repo_merge_settings(self, owner, repo)
    }

    fn pending_review(&self, owner: &str, repo: &str, pr: u64) -> Result<PendingReview, GhError> {
        CliGitHub::pending_review(self, owner, repo, pr)
    }

    fn pending_review_add_comment(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        entry: &ReviewBatchEntry,
    ) -> Result<(), GhError> {
        CliGitHub::pending_review_add_comment(self, owner, repo, pr, entry)
    }

    fn pending_review_remove_comment(&self, comment_node_id: &str) -> Result<(), GhError> {
        CliGitHub::pending_review_remove_comment(self, comment_node_id)
    }

    fn pending_review_submit(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        event: &str,
        body: &str,
    ) -> Result<usize, GhError> {
        CliGitHub::pending_review_submit(self, owner, repo, pr, event, body)
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
        CliGitHub::submit_review(self, owner, repo, pr, comments, repo_root, event, body)
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
        CliGitHub::submit_review_remote(self, owner, repo, pr, comments, event, body)
    }

    fn pr_head_oid_remote(&self, owner: &str, repo: &str, pr: u64) -> Result<String, GhError> {
        CliGitHub::pr_head_oid_remote(self, owner, repo, pr)
    }

    fn get_repo_info(
        &self,
        git: &dyn er_git::Git,
        repo_root: &str,
    ) -> Result<(String, String), GhError> {
        CliGitHub::get_repo_info(self, git, repo_root)
    }

    fn pr_comments(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        repo_root: &str,
    ) -> Result<Vec<GitHubComment>, GhError> {
        CliGitHub::pr_comments(self, owner, repo, pr, repo_root)
    }

    fn pr_comments_remote(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
    ) -> Result<Vec<GitHubComment>, GhError> {
        CliGitHub::pr_comments_remote(self, owner, repo, pr)
    }

    fn review_threads(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        repo_root: &str,
    ) -> Result<HashMap<u64, ReviewThreadState>, GhError> {
        CliGitHub::review_threads(self, owner, repo, pr, repo_root)
    }

    fn review_threads_remote(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
    ) -> Result<HashMap<u64, ReviewThreadState>, GhError> {
        CliGitHub::review_threads_remote(self, owner, repo, pr)
    }

    fn pr_head_oid(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        repo_root: Option<&str>,
    ) -> Result<String, GhError> {
        CliGitHub::pr_head_oid(self, owner, repo, pr, repo_root)
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
        CliGitHub::post_line_comment(self, owner, repo, pr, comment, commit_id, repo_root)
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
        CliGitHub::post_reply(self, owner, repo, pr, in_reply_to, body, repo_root)
    }

    fn post_general(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        body: &str,
        repo_root: Option<&str>,
    ) -> Result<CreatedComment, GhError> {
        CliGitHub::post_general(self, owner, repo, pr, body, repo_root)
    }

    fn issue_comments(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        repo_root: Option<&str>,
    ) -> Result<Vec<IssueComment>, GhError> {
        CliGitHub::issue_comments(self, owner, repo, pr, repo_root)
    }

    fn viewer_login(&self, repo_root: Option<&str>) -> Result<String, GhError> {
        CliGitHub::viewer_login(self, repo_root)
    }

    fn push_comment(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        comment: NewLineComment<'_>,
        repo_root: &str,
    ) -> Result<u64, GhError> {
        CliGitHub::push_comment(self, owner, repo, pr, comment, repo_root)
    }

    fn push_comment_remote(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        comment: NewLineComment<'_>,
    ) -> Result<u64, GhError> {
        CliGitHub::push_comment_remote(self, owner, repo, pr, comment)
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
        CliGitHub::reply_comment(self, owner, repo, pr, in_reply_to, body, repo_root)
    }

    fn reply_comment_remote(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        in_reply_to: u64,
        body: &str,
    ) -> Result<u64, GhError> {
        CliGitHub::reply_comment_remote(self, owner, repo, pr, in_reply_to, body)
    }

    fn update_review_comment(
        &self,
        owner: &str,
        repo: &str,
        comment_id: u64,
        body: &str,
        repo_root: &str,
    ) -> Result<(), GhError> {
        CliGitHub::update_review_comment(self, owner, repo, comment_id, body, repo_root)
    }

    fn delete_comment(
        &self,
        owner: &str,
        repo: &str,
        comment_id: u64,
        repo_root: &str,
    ) -> Result<(), GhError> {
        CliGitHub::delete_comment(self, owner, repo, comment_id, repo_root)
    }

    fn edit_body(&self, repo_root: &str, body: &str) -> Result<(), GhError> {
        CliGitHub::edit_body(self, repo_root, body)
    }

    fn approve(
        &self,
        repo_root: &str,
        remote_repo: Option<&str>,
        pr_number: Option<u64>,
    ) -> Result<(), GhError> {
        CliGitHub::approve(self, repo_root, remote_repo, pr_number)
    }

    fn fetch_pr_comment_bundle(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        repo_root: Option<&str>,
    ) -> Result<PrCommentBundle, GhError> {
        CliGitHub::fetch_pr_comment_bundle(self, owner, repo, pr, repo_root)
    }

    fn pr_comment_bundle_cached(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        repo_root: Option<&str>,
    ) -> Result<Arc<PrCommentBundle>, GhError> {
        CliGitHub::pr_comment_bundle_cached(self, owner, repo, pr, repo_root)
    }

    fn invalidate_comments(&self) {
        CliGitHub::invalidate_comments(self);
    }

    fn overview(
        &self,
        repo_root: &str,
        pr_number: Option<u64>,
    ) -> Result<Option<PrOverviewData>, GhError> {
        CliGitHub::overview(self, repo_root, pr_number)
    }

    fn overview_no_checks(
        &self,
        repo_root: &str,
        pr_number: Option<u64>,
    ) -> Result<Option<PrOverviewData>, GhError> {
        CliGitHub::overview_no_checks(self, repo_root, pr_number)
    }

    fn overview_remote(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<Option<PrOverviewData>, GhError> {
        CliGitHub::overview_remote(self, owner, repo, number)
    }

    fn api_get_bounded(&self, path: &str, max_bytes: usize) -> Result<Vec<u8>, GhError> {
        CliGitHub::api_get_bounded(self, path, max_bytes)
    }

    fn fetch_pr_base_ref(
        &self,
        git: &dyn er_git::Git,
        repo_root: &str,
        pr_number: u64,
        base_branch: &str,
    ) -> Result<String, GhError> {
        CliGitHub::fetch_pr_base_ref(self, git, repo_root, pr_number, base_branch)
    }

    fn pr_head_branch_name(&self, number: u64, root: &str) -> Result<String, GhError> {
        CliGitHub::pr_head_branch_name(self, number, root)
    }

    fn pr_size_check_remote(&self, owner: &str, repo: &str, number: u64) -> Result<(), GhError> {
        CliGitHub::pr_size_check_remote(self, owner, repo, number)
    }

    fn pr_diff_remote(
        &self,
        git: &dyn er_git::Git,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<String, GhError> {
        CliGitHub::pr_diff_remote(self, git, owner, repo, number)
    }

    fn pr_diff(
        &self,
        git: &dyn er_git::Git,
        pr_number: u64,
        repo_root: &str,
    ) -> Result<String, GhError> {
        CliGitHub::pr_diff(self, git, pr_number, repo_root)
    }

    fn commit_shas_remote(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<(String, String), GhError> {
        CliGitHub::commit_shas_remote(self, owner, repo, number)
    }

    fn file_at_ref(
        &self,
        repo_slug: &str,
        path: &str,
        git_ref: &str,
        max_bytes: usize,
    ) -> Result<Vec<u8>, GhError> {
        CliGitHub::file_at_ref(self, repo_slug, path, git_ref, max_bytes)
    }

    fn pr_commits(
        &self,
        repo_root: &str,
        number: u64,
        limit: usize,
    ) -> Result<Vec<er_git::CommitInfo>, GhError> {
        CliGitHub::pr_commits(self, repo_root, number, limit)
    }

    fn pr_base_branch_and_commits(
        &self,
        repo_root: &str,
        number: u64,
        limit: usize,
    ) -> Result<(String, Vec<er_git::CommitInfo>), GhError> {
        CliGitHub::pr_base_branch_and_commits(self, repo_root, number, limit)
    }

    fn commits_remote(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
        limit: usize,
    ) -> Result<Vec<er_git::CommitInfo>, GhError> {
        CliGitHub::commits_remote(self, owner, repo, number, limit)
    }

    fn ensure_installed(&self) -> Result<(), GhError> {
        CliGitHub::ensure_installed(self)
    }

    fn list_participating_notifications(&self) -> Result<Vec<GhNotification>, GhError> {
        CliGitHub::list_participating_notifications(self)
    }

    fn pr_base_branch(&self, pr_number: u64, repo_root: &str) -> Result<String, GhError> {
        CliGitHub::pr_base_branch(self, pr_number, repo_root)
    }

    fn pr_branch_names(
        &self,
        pr_number: u64,
        repo_root: &str,
    ) -> Result<(String, String), GhError> {
        CliGitHub::pr_branch_names(self, pr_number, repo_root)
    }

    fn probe_pr_for_current_branch(&self, repo_root: &str) -> BranchPrProbe {
        CliGitHub::probe_pr_for_current_branch(self, repo_root)
    }

    fn pr_for_current_branch(&self, repo_root: &str) -> Option<(u64, String)> {
        CliGitHub::pr_for_current_branch(self, repo_root)
    }

    fn lookup_pr_for_current_branch(&self, git: &dyn er_git::Git, repo_root: &str) -> PrLookup {
        CliGitHub::lookup_pr_for_current_branch(self, git, repo_root)
    }

    fn open_pr_number_for_head(
        &self,
        owner: &str,
        repo: &str,
        head: &str,
    ) -> Result<Option<u64>, GhError> {
        CliGitHub::open_pr_number_for_head(self, owner, repo, head)
    }

    fn pr_title(&self, owner: &str, repo: &str, number: u64) -> Option<String> {
        CliGitHub::pr_title(self, owner, repo, number)
    }

    fn get_pr_info(
        &self,
        git: &dyn er_git::Git,
        repo_root: &str,
    ) -> Result<(String, String, u64), GhError> {
        CliGitHub::get_pr_info(self, git, repo_root)
    }

    fn current_login(&self) -> Option<String> {
        CliGitHub::current_login(self)
    }

    fn stack_view_json(&self, repo_root: &str) -> Result<String, GhError> {
        CliGitHub::stack_view_json(self, repo_root)
    }

    fn open_pr_refs_json(&self, repo_slug: &str) -> Result<String, GhError> {
        CliGitHub::open_pr_refs_json(self, repo_slug)
    }

    fn pr_metadata_remote(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<(String, String), GhError> {
        CliGitHub::pr_metadata_remote(self, owner, repo, number)
    }

    fn pr_status_remote(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<PrStatusBundle, GhError> {
        CliGitHub::pr_status_remote(self, owner, repo, number)
    }

    fn pr_checks_remote(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<Vec<CheckRun>, GhError> {
        CliGitHub::pr_checks_remote(self, owner, repo, number)
    }

    fn pr_checks_state_remote(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<&'static str, GhError> {
        CliGitHub::pr_checks_state_remote(self, owner, repo, number)
    }

    fn pr_thread_addressing_remote(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
    ) -> Result<ThreadAddressingSummary, GhError> {
        CliGitHub::pr_thread_addressing_remote(self, owner, repo, pr)
    }
}

/// `gh api graphql` with string variables (`-f`, so a numeric repo name stays
/// a string) and typed ones (`-F`, for `Int!`).
pub(crate) fn graphql_args(
    query: &str,
    strings: &[(&str, &str)],
    typed: &[(&str, &str)],
) -> Vec<String> {
    let mut args = vec![
        "api".to_string(),
        "graphql".to_string(),
        "-f".to_string(),
        format!("query={query}"),
    ];
    for (key, value) in strings {
        args.extend(["-f".to_string(), format!("{key}={value}")]);
    }
    for (key, value) in typed {
        args.extend(["-F".to_string(), format!("{key}={value}")]);
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::{MergeMethod, PrAction};
    use crate::testing::ScriptedRunner;
    use std::sync::Arc;

    fn github(runner: &Arc<ScriptedRunner>) -> CliGitHub {
        CliGitHub::new(runner.clone())
    }

    #[test]
    fn merge_sends_the_pinned_head_commit() {
        let runner = Arc::new(ScriptedRunner::new());
        runner.script(
            &["gh", "pr", "merge"],
            Ok(ScriptedRunner::output(0, "", "")),
        );
        let github = github(&runner);
        github
            .run_pr_action(
                "o",
                "r",
                7,
                &PrAction::Merge {
                    method: MergeMethod::Squash,
                    auto: false,
                    admin: false,
                    head_oid: "abc".into(),
                },
            )
            .unwrap();
        let calls = runner.calls();
        assert_eq!(calls.len(), 1);
        assert!(calls[0].contains(&"--match-head-commit".to_string()));
        assert!(calls[0].contains(&"abc".to_string()));
    }

    #[test]
    fn delete_branch_refuses_when_the_tip_moved_and_never_sends_the_delete() {
        let runner = Arc::new(ScriptedRunner::new());
        runner.script(
            &["gh", "api"],
            Ok(ScriptedRunner::output(0, "newsha\n", "")),
        );
        let github = github(&runner);
        let error = github
            .delete_remote_branch("o", "r", "feat", "prhead")
            .unwrap_err();
        assert!(
            error.to_string().contains("has moved past this PR"),
            "{error}"
        );
        let calls = runner.calls();
        assert_eq!(calls.len(), 1);
        assert!(calls
            .iter()
            .all(|argv| !argv.iter().any(|arg| arg == "DELETE")));
    }

    #[test]
    fn delete_branch_treats_a_422_missing_ref_as_done_and_a_404_as_failure() {
        let not_found = || {
            ScriptedRunner::output(
                1,
                "HTTP/2.0 404 Not Found\r\n\r\n{}",
                "gh: Not Found (HTTP 404)",
            )
        };
        let missing_ref = ScriptedRunner::output(
            1,
            "HTTP/2.0 422 Unprocessable Entity\r\n\r\n\
             {\"message\":\"Reference does not exist\"}",
            "gh: Unprocessable Entity (HTTP 422)",
        );

        let runner = Arc::new(ScriptedRunner::new());
        runner.script(&["gh", "api"], Ok(not_found()));
        runner.script(&["gh", "api", "--include", "-X", "DELETE"], Ok(missing_ref));
        github(&runner)
            .delete_remote_branch("o", "r", "feat", "abc")
            .unwrap();

        let runner = Arc::new(ScriptedRunner::new());
        runner.script(&["gh", "api"], Ok(not_found()));
        runner.script(&["gh", "api", "--include", "-X", "DELETE"], Ok(not_found()));
        let error = github(&runner)
            .delete_remote_branch("o", "r", "feat", "abc")
            .unwrap_err();
        assert!(matches!(error, GhError::NotFound { .. }), "{error:?}");
    }

    #[test]
    fn pending_review_submit_reads_the_review_before_submitting() {
        let pending = r#"{"data":{"viewer":{"login":"vsd"},"repository":{"pullRequest":{
          "reviews":{"nodes":[{"id":"PRR_1","databaseId":77,"viewerDidAuthor":true,"comments":{"nodes":[
            {"id":"a","path":"x","line":1,"body":"b"},
            {"id":"b","path":"y","line":2,"body":"c"}]}}]},
          "reviewThreads":{"nodes":[]},"timelineItems":{"nodes":[]}}}}}"#;
        let runner = Arc::new(ScriptedRunner::new());
        runner.script(
            &["gh", "api", "--include", "graphql"],
            Ok(ScriptedRunner::output(0, pending, "")),
        );
        runner.script(
            &["gh", "api", "--include", "-X", "POST"],
            Ok(ScriptedRunner::output(0, "", "")),
        );
        let count = github(&runner)
            .pending_review_submit("o", "r", 7, "COMMENT", "body")
            .unwrap();
        assert_eq!(count, 2);
        let calls = runner.calls();
        assert_eq!(calls.len(), 2);
        assert!(calls[0].iter().any(|arg| arg == "graphql"), "{calls:?}");
        assert!(
            calls[1].iter().any(|arg| arg.contains("reviews/77/events")),
            "{calls:?}"
        );
    }

    #[test]
    fn a_rate_limited_answer_is_not_cached_as_a_value() {
        let rate_limited = "HTTP/2.0 403 Forbidden\r\nX-Ratelimit-Remaining: 0\r\n\
             X-Ratelimit-Reset: 1793976383\r\n\r\n\
             {\"message\":\"API rate limit exceeded\"}";
        let settings = r#"{"data":{"repository":{"mergeCommitAllowed":true}}}"#;
        let runner = Arc::new(ScriptedRunner::new());
        runner.script(
            &["gh", "api", "--include", "graphql"],
            Ok(ScriptedRunner::output(
                1,
                rate_limited,
                "gh: API rate limit exceeded (HTTP 403)",
            )),
        );
        runner.script(
            &["gh", "api", "--include", "graphql"],
            Ok(ScriptedRunner::output(0, settings, "")),
        );
        let github = github(&runner);
        assert!(github.cached_repo_merge_settings("o", "r").is_none());
        assert!(github.cached_repo_merge_settings("o", "r").is_some());
        assert_eq!(runner.calls().len(), 2);
    }
}
