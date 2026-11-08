//! A scripted in-memory git for tests.
//!
//! It compiles in normal builds because the engine's test constructors are
//! production code.

use crate::blame::{BlameRequest, LineAttribution};
use crate::cli::Git;
use crate::error::GitError;
use crate::status::{CommitInfo, Worktree};
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, MutexGuard, PoisonError};

#[derive(Debug, Default)]
pub struct FakeGit {
    state: Mutex<FakeState>,
}

#[derive(Debug, Default)]
struct FakeState {
    calls: Vec<String>,
    repo_root_cwd: Option<Result<String, GitError>>,
    repo_root_in: Option<Result<String, GitError>>,
    repo_root_at: Option<Result<String, GitError>>,
    current_branch: Option<Result<String, GitError>>,
    detect_base_branch: Option<Result<String, GitError>>,
    diff_raw: Option<Result<String, GitError>>,
    diff_raw_file: Option<Result<String, GitError>>,
    diff_raw_range: Option<Result<String, GitError>>,
    diff_checkout_against_base: Option<Result<String, GitError>>,
    diff_against_branch: Option<Result<String, GitError>>,
    diff_commit: Option<Result<String, GitError>>,
    diff_conflicts: Option<Result<String, GitError>>,
    unmerged_files: Option<Result<Vec<String>, GitError>>,
    is_merge_in_progress: Option<bool>,
    diff_shortstat: Option<(usize, usize, usize)>,
    stage_file: Option<Result<(), GitError>>,
    unstage_file: Option<Result<(), GitError>>,
    stage_all: Option<Result<(), GitError>>,
    commit: Option<Result<(), GitError>>,
    push: Option<Result<String, GitError>>,
    log_range: Option<Result<Vec<CommitInfo>, GitError>>,
    log_head: Option<Result<Vec<CommitInfo>, GitError>>,
    commit_shortstats: Option<HashMap<String, (usize, usize, usize)>>,
    unpushed_shas: Option<HashSet<String>>,
    list_worktrees: Option<Result<Vec<Worktree>, GitError>>,
    blob_oid: Option<Result<String, GitError>>,
    read_blob: Option<Result<Vec<u8>, GitError>>,
    paths_present_at: Option<Result<Vec<String>, GitError>>,
    gitignored_paths: Option<HashSet<String>>,
    diff_watched_file_snapshot: Option<Result<Option<String>, GitError>>,
    blame_range: Option<Result<LineAttribution, GitError>>,
    merge_base: Option<Result<String, GitError>>,
    rev_parse_commit: Option<Result<String, GitError>>,
    remote_url: Option<Result<String, GitError>>,
    rev_parse_verify: Option<Result<Option<String>, GitError>>,
    fetch: Option<Result<(), GitError>>,
    update_ref: Option<Result<(), GitError>>,
    diff_revs: Option<Result<String, GitError>>,
    shallow_clone: Option<Result<(), GitError>>,
    show_ref_verify: Option<Result<bool, GitError>>,
    current_branch_show: Option<Result<String, GitError>>,
    for_each_ref: Option<Result<String, GitError>>,
    ls_remote: Option<Result<String, GitError>>,
    config_get_regexp: Option<Result<String, GitError>>,
    rev_list_count: Option<Result<String, GitError>>,
    branch_merged: Option<Result<String, GitError>>,
}

fn unscripted(method: &str) -> GitError {
    GitError::Failed {
        context: format!("unscripted {method}"),
        stderr: String::new(),
    }
}

impl FakeGit {
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
    pub fn with_repo_root_cwd(self, value: Result<String, GitError>) -> Self {
        self.lock().repo_root_cwd = Some(value);
        self
    }

    #[must_use]
    pub fn with_repo_root_in(self, value: Result<String, GitError>) -> Self {
        self.lock().repo_root_in = Some(value);
        self
    }

    #[must_use]
    pub fn with_repo_root_at(self, value: Result<String, GitError>) -> Self {
        self.lock().repo_root_at = Some(value);
        self
    }

    #[must_use]
    pub fn with_current_branch(self, value: Result<String, GitError>) -> Self {
        self.lock().current_branch = Some(value);
        self
    }

    #[must_use]
    pub fn with_detect_base_branch(self, value: Result<String, GitError>) -> Self {
        self.lock().detect_base_branch = Some(value);
        self
    }

    #[must_use]
    pub fn with_diff_raw(self, value: Result<String, GitError>) -> Self {
        self.lock().diff_raw = Some(value);
        self
    }

    #[must_use]
    pub fn with_diff_raw_file(self, value: Result<String, GitError>) -> Self {
        self.lock().diff_raw_file = Some(value);
        self
    }

    #[must_use]
    pub fn with_diff_raw_range(self, value: Result<String, GitError>) -> Self {
        self.lock().diff_raw_range = Some(value);
        self
    }

    #[must_use]
    pub fn with_diff_checkout_against_base(self, value: Result<String, GitError>) -> Self {
        self.lock().diff_checkout_against_base = Some(value);
        self
    }

    #[must_use]
    pub fn with_diff_against_branch(self, value: Result<String, GitError>) -> Self {
        self.lock().diff_against_branch = Some(value);
        self
    }

    #[must_use]
    pub fn with_diff_commit(self, value: Result<String, GitError>) -> Self {
        self.lock().diff_commit = Some(value);
        self
    }

    #[must_use]
    pub fn with_diff_conflicts(self, value: Result<String, GitError>) -> Self {
        self.lock().diff_conflicts = Some(value);
        self
    }

    #[must_use]
    pub fn with_unmerged_files(self, value: Result<Vec<String>, GitError>) -> Self {
        self.lock().unmerged_files = Some(value);
        self
    }

    #[must_use]
    pub fn with_is_merge_in_progress(self, value: bool) -> Self {
        self.lock().is_merge_in_progress = Some(value);
        self
    }

    #[must_use]
    pub fn with_diff_shortstat(self, value: (usize, usize, usize)) -> Self {
        self.lock().diff_shortstat = Some(value);
        self
    }

    #[must_use]
    pub fn with_stage_file(self, value: Result<(), GitError>) -> Self {
        self.lock().stage_file = Some(value);
        self
    }

    #[must_use]
    pub fn with_unstage_file(self, value: Result<(), GitError>) -> Self {
        self.lock().unstage_file = Some(value);
        self
    }

    #[must_use]
    pub fn with_stage_all(self, value: Result<(), GitError>) -> Self {
        self.lock().stage_all = Some(value);
        self
    }

    #[must_use]
    pub fn with_commit(self, value: Result<(), GitError>) -> Self {
        self.lock().commit = Some(value);
        self
    }

    #[must_use]
    pub fn with_push(self, value: Result<String, GitError>) -> Self {
        self.lock().push = Some(value);
        self
    }

    #[must_use]
    pub fn with_log_range(self, value: Result<Vec<CommitInfo>, GitError>) -> Self {
        self.lock().log_range = Some(value);
        self
    }

    #[must_use]
    pub fn with_log_head(self, value: Result<Vec<CommitInfo>, GitError>) -> Self {
        self.lock().log_head = Some(value);
        self
    }

    #[must_use]
    pub fn with_commit_shortstats(self, value: HashMap<String, (usize, usize, usize)>) -> Self {
        self.lock().commit_shortstats = Some(value);
        self
    }

    #[must_use]
    pub fn with_unpushed_shas(self, value: HashSet<String>) -> Self {
        self.lock().unpushed_shas = Some(value);
        self
    }

    #[must_use]
    pub fn with_list_worktrees(self, value: Result<Vec<Worktree>, GitError>) -> Self {
        self.lock().list_worktrees = Some(value);
        self
    }

    #[must_use]
    pub fn with_blob_oid(self, value: Result<String, GitError>) -> Self {
        self.lock().blob_oid = Some(value);
        self
    }

    #[must_use]
    pub fn with_read_blob(self, value: Result<Vec<u8>, GitError>) -> Self {
        self.lock().read_blob = Some(value);
        self
    }

    #[must_use]
    pub fn with_paths_present_at(self, value: Result<Vec<String>, GitError>) -> Self {
        self.lock().paths_present_at = Some(value);
        self
    }

    #[must_use]
    pub fn with_gitignored_paths(self, value: HashSet<String>) -> Self {
        self.lock().gitignored_paths = Some(value);
        self
    }

    #[must_use]
    pub fn with_diff_watched_file_snapshot(self, value: Result<Option<String>, GitError>) -> Self {
        self.lock().diff_watched_file_snapshot = Some(value);
        self
    }

    #[must_use]
    pub fn with_blame_range(self, value: Result<LineAttribution, GitError>) -> Self {
        self.lock().blame_range = Some(value);
        self
    }

    #[must_use]
    pub fn with_merge_base(self, value: Result<String, GitError>) -> Self {
        self.lock().merge_base = Some(value);
        self
    }

    #[must_use]
    pub fn with_rev_parse_commit(self, value: Result<String, GitError>) -> Self {
        self.lock().rev_parse_commit = Some(value);
        self
    }

    #[must_use]
    pub fn with_remote_url(self, value: Result<String, GitError>) -> Self {
        self.lock().remote_url = Some(value);
        self
    }

    #[must_use]
    pub fn with_rev_parse_verify(self, value: Result<Option<String>, GitError>) -> Self {
        self.lock().rev_parse_verify = Some(value);
        self
    }

    #[must_use]
    pub fn with_fetch(self, value: Result<(), GitError>) -> Self {
        self.lock().fetch = Some(value);
        self
    }

    #[must_use]
    pub fn with_update_ref(self, value: Result<(), GitError>) -> Self {
        self.lock().update_ref = Some(value);
        self
    }

    #[must_use]
    pub fn with_diff_revs(self, value: Result<String, GitError>) -> Self {
        self.lock().diff_revs = Some(value);
        self
    }

    #[must_use]
    pub fn with_shallow_clone(self, value: Result<(), GitError>) -> Self {
        self.lock().shallow_clone = Some(value);
        self
    }

    #[must_use]
    pub fn with_show_ref_verify(self, value: Result<bool, GitError>) -> Self {
        self.lock().show_ref_verify = Some(value);
        self
    }

    #[must_use]
    pub fn with_current_branch_show(self, value: Result<String, GitError>) -> Self {
        self.lock().current_branch_show = Some(value);
        self
    }

    #[must_use]
    pub fn with_for_each_ref(self, value: Result<String, GitError>) -> Self {
        self.lock().for_each_ref = Some(value);
        self
    }

    #[must_use]
    pub fn with_ls_remote(self, value: Result<String, GitError>) -> Self {
        self.lock().ls_remote = Some(value);
        self
    }

    #[must_use]
    pub fn with_config_get_regexp(self, value: Result<String, GitError>) -> Self {
        self.lock().config_get_regexp = Some(value);
        self
    }

    #[must_use]
    pub fn with_rev_list_count(self, value: Result<String, GitError>) -> Self {
        self.lock().rev_list_count = Some(value);
        self
    }

    #[must_use]
    pub fn with_branch_merged(self, value: Result<String, GitError>) -> Self {
        self.lock().branch_merged = Some(value);
        self
    }
}

impl Git for FakeGit {
    fn repo_root_cwd(&self) -> Result<String, GitError> {
        self.record("repo_root_cwd()".into())
            .repo_root_cwd
            .clone()
            .unwrap_or_else(|| Err(unscripted("repo_root_cwd")))
    }

    fn repo_root_in(&self, dir: &str) -> Result<String, GitError> {
        self.record(format!("repo_root_in(dir={dir:?})"))
            .repo_root_in
            .clone()
            .unwrap_or_else(|| Err(unscripted("repo_root_in")))
    }

    fn repo_root_at(&self, path: &str) -> Result<String, GitError> {
        self.record(format!("repo_root_at(path={path:?})"))
            .repo_root_at
            .clone()
            .unwrap_or_else(|| Err(unscripted("repo_root_at")))
    }

    fn current_branch(&self, repo_root: &str) -> Result<String, GitError> {
        self.record(format!("current_branch(repo_root={repo_root:?})"))
            .current_branch
            .clone()
            .unwrap_or_else(|| Err(unscripted("current_branch")))
    }

    fn detect_base_branch(&self, repo_root: &str) -> Result<String, GitError> {
        self.record(format!("detect_base_branch(repo_root={repo_root:?})"))
            .detect_base_branch
            .clone()
            .unwrap_or_else(|| Err(unscripted("detect_base_branch")))
    }

    fn diff_raw(
        &self,
        mode: &str,
        base: &str,
        repo_root: &str,
        head_ref: Option<&str>,
    ) -> Result<String, GitError> {
        self.record(format!(
            "diff_raw(mode={mode:?}, base={base:?}, repo_root={repo_root:?}, head_ref={head_ref:?})"
        ))
        .diff_raw
        .clone()
        .unwrap_or_else(|| Err(unscripted("diff_raw")))
    }

    fn diff_raw_file(
        &self,
        mode: &str,
        base: &str,
        repo_root: &str,
        path: &str,
        context_lines: Option<usize>,
        head_ref: Option<&str>,
    ) -> Result<String, GitError> {
        self.record(format!(
            "diff_raw_file(mode={mode:?}, base={base:?}, repo_root={repo_root:?}, path={path:?}, context_lines={context_lines:?}, head_ref={head_ref:?})"
        ))
        .diff_raw_file
        .clone()
        .unwrap_or_else(|| Err(unscripted("diff_raw_file")))
    }

    fn diff_raw_range(&self, from: &str, to: &str, repo_root: &str) -> Result<String, GitError> {
        self.record(format!(
            "diff_raw_range(from={from:?}, to={to:?}, repo_root={repo_root:?})"
        ))
        .diff_raw_range
        .clone()
        .unwrap_or_else(|| Err(unscripted("diff_raw_range")))
    }

    fn diff_checkout_against_base(&self, root: &str, base: &str) -> Result<String, GitError> {
        self.record(format!(
            "diff_checkout_against_base(root={root:?}, base={base:?})"
        ))
        .diff_checkout_against_base
        .clone()
        .unwrap_or_else(|| Err(unscripted("diff_checkout_against_base")))
    }

    fn diff_against_branch(
        &self,
        root: &str,
        base: &str,
        branch: &str,
    ) -> Result<String, GitError> {
        self.record(format!(
            "diff_against_branch(root={root:?}, base={base:?}, branch={branch:?})"
        ))
        .diff_against_branch
        .clone()
        .unwrap_or_else(|| Err(unscripted("diff_against_branch")))
    }

    fn diff_commit(&self, hash: &str, repo_root: &str) -> Result<String, GitError> {
        self.record(format!(
            "diff_commit(hash={hash:?}, repo_root={repo_root:?})"
        ))
        .diff_commit
        .clone()
        .unwrap_or_else(|| Err(unscripted("diff_commit")))
    }

    fn diff_conflicts(&self, repo_root: &str) -> Result<String, GitError> {
        self.record(format!("diff_conflicts(repo_root={repo_root:?})"))
            .diff_conflicts
            .clone()
            .unwrap_or_else(|| Err(unscripted("diff_conflicts")))
    }

    fn unmerged_files(&self, repo_root: &str) -> Result<Vec<String>, GitError> {
        self.record(format!("unmerged_files(repo_root={repo_root:?})"))
            .unmerged_files
            .clone()
            .unwrap_or_else(|| Err(unscripted("unmerged_files")))
    }

    fn is_merge_in_progress(&self, repo_root: &str) -> bool {
        self.record(format!("is_merge_in_progress(repo_root={repo_root:?})"))
            .is_merge_in_progress
            .unwrap_or_else(|| panic!("FakeGit: unscripted is_merge_in_progress"))
    }

    fn diff_shortstat(&self, repo_root: &str, staged: bool) -> (usize, usize, usize) {
        self.record(format!(
            "diff_shortstat(repo_root={repo_root:?}, staged={staged:?})"
        ))
        .diff_shortstat
        .unwrap_or_else(|| panic!("FakeGit: unscripted diff_shortstat"))
    }

    fn stage_file(&self, repo_root: &str, file_path: &str) -> Result<(), GitError> {
        self.record(format!(
            "stage_file(repo_root={repo_root:?}, file_path={file_path:?})"
        ))
        .stage_file
        .clone()
        .unwrap_or_else(|| Err(unscripted("stage_file")))
    }

    fn unstage_file(&self, repo_root: &str, file_path: &str) -> Result<(), GitError> {
        self.record(format!(
            "unstage_file(repo_root={repo_root:?}, file_path={file_path:?})"
        ))
        .unstage_file
        .clone()
        .unwrap_or_else(|| Err(unscripted("unstage_file")))
    }

    fn stage_all(&self, repo_root: &str) -> Result<(), GitError> {
        self.record(format!("stage_all(repo_root={repo_root:?})"))
            .stage_all
            .clone()
            .unwrap_or_else(|| Err(unscripted("stage_all")))
    }

    fn commit(&self, repo_root: &str, message: &str) -> Result<(), GitError> {
        self.record(format!(
            "commit(repo_root={repo_root:?}, message={message:?})"
        ))
        .commit
        .clone()
        .unwrap_or_else(|| Err(unscripted("commit")))
    }

    fn push(&self, repo_root: &str) -> Result<String, GitError> {
        self.record(format!("push(repo_root={repo_root:?})"))
            .push
            .clone()
            .unwrap_or_else(|| Err(unscripted("push")))
    }

    fn log_range(
        &self,
        from: &str,
        to: &str,
        repo_root: &str,
        limit: usize,
        skip: usize,
    ) -> Result<Vec<CommitInfo>, GitError> {
        self.record(format!(
            "log_range(from={from:?}, to={to:?}, repo_root={repo_root:?}, limit={limit:?}, skip={skip:?})"
        ))
        .log_range
        .clone()
        .unwrap_or_else(|| Err(unscripted("log_range")))
    }

    fn log_head(&self, repo_root: &str, limit: usize) -> Result<Vec<CommitInfo>, GitError> {
        self.record(format!(
            "log_head(repo_root={repo_root:?}, limit={limit:?})"
        ))
        .log_head
        .clone()
        .unwrap_or_else(|| Err(unscripted("log_head")))
    }

    fn commit_shortstats(
        &self,
        repo_root: &str,
        shas: &[&str],
    ) -> HashMap<String, (usize, usize, usize)> {
        self.record(format!(
            "commit_shortstats(repo_root={repo_root:?}, shas={shas:?})"
        ))
        .commit_shortstats
        .clone()
        .unwrap_or_else(|| panic!("FakeGit: unscripted commit_shortstats"))
    }

    fn unpushed_shas(
        &self,
        repo_root: &str,
        head: &str,
        pushed_refs: &[&str],
        limit: usize,
    ) -> HashSet<String> {
        self.record(format!(
            "unpushed_shas(repo_root={repo_root:?}, head={head:?}, pushed_refs={pushed_refs:?}, limit={limit:?})"
        ))
        .unpushed_shas
        .clone()
        .unwrap_or_else(|| panic!("FakeGit: unscripted unpushed_shas"))
    }

    fn list_worktrees(&self, repo_root: &str) -> Result<Vec<Worktree>, GitError> {
        self.record(format!("list_worktrees(repo_root={repo_root:?})"))
            .list_worktrees
            .clone()
            .unwrap_or_else(|| Err(unscripted("list_worktrees")))
    }

    fn blob_oid(&self, root: &str, spec: &str) -> Result<String, GitError> {
        self.record(format!("blob_oid(root={root:?}, spec={spec:?})"))
            .blob_oid
            .clone()
            .unwrap_or_else(|| Err(unscripted("blob_oid")))
    }

    fn read_blob(&self, root: &str, spec: &str, max_bytes: usize) -> Result<Vec<u8>, GitError> {
        self.record(format!(
            "read_blob(root={root:?}, spec={spec:?}, max_bytes={max_bytes:?})"
        ))
        .read_blob
        .clone()
        .unwrap_or_else(|| Err(unscripted("read_blob")))
    }

    fn paths_present_at(
        &self,
        root: &str,
        rev: &str,
        paths: &[String],
    ) -> Result<Vec<String>, GitError> {
        self.record(format!(
            "paths_present_at(root={root:?}, rev={rev:?}, paths={paths:?})"
        ))
        .paths_present_at
        .clone()
        .unwrap_or_else(|| Err(unscripted("paths_present_at")))
    }

    fn gitignored_paths(&self, repo_root: &str, paths: &[String]) -> HashSet<String> {
        self.record(format!(
            "gitignored_paths(repo_root={repo_root:?}, paths={paths:?})"
        ))
        .gitignored_paths
        .clone()
        .unwrap_or_else(|| panic!("FakeGit: unscripted gitignored_paths"))
    }

    fn diff_watched_file_snapshot(
        &self,
        repo_root: &str,
        rel_path: &str,
        snapshots_dir: &str,
    ) -> Result<Option<String>, GitError> {
        self.record(format!(
            "diff_watched_file_snapshot(repo_root={repo_root:?}, rel_path={rel_path:?}, snapshots_dir={snapshots_dir:?})"
        ))
        .diff_watched_file_snapshot
        .clone()
        .unwrap_or_else(|| Err(unscripted("diff_watched_file_snapshot")))
    }

    fn blame_range(&self, request: &BlameRequest<'_>) -> Result<LineAttribution, GitError> {
        self.record(format!("blame_range(request={request:?})"))
            .blame_range
            .clone()
            .unwrap_or_else(|| Err(unscripted("blame_range")))
    }

    fn merge_base(&self, repo_root: &str, a: &str, b: &str) -> Result<String, GitError> {
        self.record(format!(
            "merge_base(repo_root={repo_root:?}, a={a:?}, b={b:?})"
        ))
        .merge_base
        .clone()
        .unwrap_or_else(|| Err(unscripted("merge_base")))
    }

    fn rev_parse_commit(&self, repo_root: &str, rev: &str) -> Result<String, GitError> {
        self.record(format!(
            "rev_parse_commit(repo_root={repo_root:?}, rev={rev:?})"
        ))
        .rev_parse_commit
        .clone()
        .unwrap_or_else(|| Err(unscripted("rev_parse_commit")))
    }

    fn remote_url(&self, root: &str, name: &str) -> Result<String, GitError> {
        self.record(format!("remote_url(root={root:?}, name={name:?})"))
            .remote_url
            .clone()
            .unwrap_or_else(|| Err(unscripted("remote_url")))
    }

    fn rev_parse_verify(&self, root: &str, rev: &str) -> Result<Option<String>, GitError> {
        self.record(format!("rev_parse_verify(root={root:?}, rev={rev:?})"))
            .rev_parse_verify
            .clone()
            .unwrap_or_else(|| Err(unscripted("rev_parse_verify")))
    }

    fn fetch(&self, root: &str, args: &[&str]) -> Result<(), GitError> {
        self.record(format!("fetch(root={root:?}, args={args:?})"))
            .fetch
            .clone()
            .unwrap_or_else(|| Err(unscripted("fetch")))
    }

    fn update_ref(&self, root: &str, name: &str, target: &str) -> Result<(), GitError> {
        self.record(format!(
            "update_ref(root={root:?}, name={name:?}, target={target:?})"
        ))
        .update_ref
        .clone()
        .unwrap_or_else(|| Err(unscripted("update_ref")))
    }

    fn diff_revs(&self, root: &str, revs: &[&str]) -> Result<String, GitError> {
        self.record(format!("diff_revs(root={root:?}, revs={revs:?})"))
            .diff_revs
            .clone()
            .unwrap_or_else(|| Err(unscripted("diff_revs")))
    }

    fn shallow_clone(&self, url: &str, dest: &str) -> Result<(), GitError> {
        self.record(format!("shallow_clone(url={url:?}, dest={dest:?})"))
            .shallow_clone
            .clone()
            .unwrap_or_else(|| Err(unscripted("shallow_clone")))
    }

    fn show_ref_verify(&self, root: &str, refname: &str) -> Result<bool, GitError> {
        self.record(format!(
            "show_ref_verify(root={root:?}, refname={refname:?})"
        ))
        .show_ref_verify
        .clone()
        .unwrap_or_else(|| Err(unscripted("show_ref_verify")))
    }

    fn current_branch_show(&self, root: &str) -> Result<String, GitError> {
        self.record(format!("current_branch_show(root={root:?})"))
            .current_branch_show
            .clone()
            .unwrap_or_else(|| Err(unscripted("current_branch_show")))
    }

    fn for_each_ref(
        &self,
        root: &str,
        format: &str,
        sort: Option<&str>,
        pattern: &str,
    ) -> Result<String, GitError> {
        self.record(format!(
            "for_each_ref(root={root:?}, format={format:?}, sort={sort:?}, pattern={pattern:?})"
        ))
        .for_each_ref
        .clone()
        .unwrap_or_else(|| Err(unscripted("for_each_ref")))
    }

    fn ls_remote(&self, root: &str, remote: &str, refname: &str) -> Result<String, GitError> {
        self.record(format!(
            "ls_remote(root={root:?}, remote={remote:?}, refname={refname:?})"
        ))
        .ls_remote
        .clone()
        .unwrap_or_else(|| Err(unscripted("ls_remote")))
    }

    fn config_get_regexp(&self, root: &str, pattern: &str) -> Result<String, GitError> {
        self.record(format!(
            "config_get_regexp(root={root:?}, pattern={pattern:?})"
        ))
        .config_get_regexp
        .clone()
        .unwrap_or_else(|| Err(unscripted("config_get_regexp")))
    }

    fn rev_list_count(&self, root: &str, range: &str) -> Result<String, GitError> {
        self.record(format!("rev_list_count(root={root:?}, range={range:?})"))
            .rev_list_count
            .clone()
            .unwrap_or_else(|| Err(unscripted("rev_list_count")))
    }

    fn branch_merged(&self, root: &str, base: &str) -> Result<String, GitError> {
        self.record(format!("branch_merged(root={root:?}, base={base:?})"))
            .branch_merged
            .clone()
            .unwrap_or_else(|| Err(unscripted("branch_merged")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_git_returns_the_scripted_value_and_records_the_call() {
        let fake = FakeGit::default().with_diff_raw(Ok("raw".into()));
        assert_eq!(
            fake.diff_raw("branch", "main", "/tmp/x", None).unwrap(),
            "raw"
        );
        assert_eq!(
            fake.calls(),
            vec![
                r#"diff_raw(mode="branch", base="main", repo_root="/tmp/x", head_ref=None)"#
                    .to_string()
            ]
        );
    }

    #[test]
    fn fake_git_unscripted_result_method_returns_an_error() {
        let fake = FakeGit::default();
        let err = fake.diff_raw("branch", "main", "/tmp/x", None).unwrap_err();
        assert!(
            matches!(
                err,
                GitError::Failed { ref context, ref stderr }
                    if context == "unscripted diff_raw" && stderr.is_empty()
            ),
            "{err:?}"
        );
    }

    #[test]
    fn fake_git_unscripted_value_method_panics() {
        let fake = FakeGit::default();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            fake.is_merge_in_progress("/tmp/x")
        }));
        assert!(result.is_err());
    }
}
