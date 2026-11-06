//! The git port and its CLI adapter.

use crate::blame::{BlameRequest, LineAttribution};
use crate::error::GitError;
use crate::status::{CommitInfo, Worktree};
use std::collections::{HashMap, HashSet};

/// One method per function that spawns git, so a caller holds the seam rather
/// than reaching a free function.
pub trait Git: Send + Sync {
    fn repo_root_cwd(&self) -> Result<String, GitError>;
    fn repo_root_in(&self, dir: &str) -> Result<String, GitError>;
    fn repo_root_at(&self, path: &str) -> Result<String, GitError>;
    fn current_branch(&self, repo_root: &str) -> Result<String, GitError>;
    fn detect_base_branch(&self, repo_root: &str) -> Result<String, GitError>;

    fn diff_raw(
        &self,
        mode: &str,
        base: &str,
        repo_root: &str,
        head_ref: Option<&str>,
    ) -> Result<String, GitError>;
    fn diff_raw_file(
        &self,
        mode: &str,
        base: &str,
        repo_root: &str,
        path: &str,
        context_lines: Option<usize>,
        head_ref: Option<&str>,
    ) -> Result<String, GitError>;
    fn diff_raw_range(&self, from: &str, to: &str, repo_root: &str) -> Result<String, GitError>;
    fn diff_checkout_against_base(&self, root: &str, base: &str) -> Result<String, GitError>;
    fn diff_against_branch(&self, root: &str, base: &str, branch: &str)
        -> Result<String, GitError>;
    fn diff_commit(&self, hash: &str, repo_root: &str) -> Result<String, GitError>;
    fn diff_conflicts(&self, repo_root: &str) -> Result<String, GitError>;
    fn unmerged_files(&self, repo_root: &str) -> Result<Vec<String>, GitError>;
    fn is_merge_in_progress(&self, repo_root: &str) -> bool;
    fn diff_shortstat(&self, repo_root: &str, staged: bool) -> (usize, usize, usize);

    fn stage_file(&self, repo_root: &str, file_path: &str) -> Result<(), GitError>;
    fn unstage_file(&self, repo_root: &str, file_path: &str) -> Result<(), GitError>;
    fn stage_all(&self, repo_root: &str) -> Result<(), GitError>;
    fn commit(&self, repo_root: &str, message: &str) -> Result<(), GitError>;
    fn push(&self, repo_root: &str) -> Result<String, GitError>;

    fn log_range(
        &self,
        from: &str,
        to: &str,
        repo_root: &str,
        limit: usize,
        skip: usize,
    ) -> Result<Vec<CommitInfo>, GitError>;
    fn log_head(&self, repo_root: &str, limit: usize) -> Result<Vec<CommitInfo>, GitError>;
    fn commit_shortstats(
        &self,
        repo_root: &str,
        shas: &[&str],
    ) -> HashMap<String, (usize, usize, usize)>;
    fn unpushed_shas(
        &self,
        repo_root: &str,
        head: &str,
        pushed_refs: &[&str],
        limit: usize,
    ) -> HashSet<String>;

    fn list_worktrees(&self, repo_root: &str) -> Result<Vec<Worktree>, GitError>;

    fn blob_oid(&self, root: &str, spec: &str) -> Result<String, GitError>;
    fn read_blob(&self, root: &str, spec: &str, max_bytes: usize) -> Result<Vec<u8>, GitError>;
    fn paths_present_at(
        &self,
        root: &str,
        rev: &str,
        paths: &[String],
    ) -> Result<Vec<String>, GitError>;

    fn gitignored_paths(&self, repo_root: &str, paths: &[String]) -> HashSet<String>;
    fn diff_watched_file_snapshot(
        &self,
        repo_root: &str,
        rel_path: &str,
        snapshots_dir: &str,
    ) -> Result<Option<String>, GitError>;

    fn blame_range(&self, request: &BlameRequest<'_>) -> Result<LineAttribution, GitError>;
    fn merge_base(&self, repo_root: &str, a: &str, b: &str) -> Result<String, GitError>;
    fn rev_parse_commit(&self, repo_root: &str, rev: &str) -> Result<String, GitError>;
}

/// The adapter that runs the real git binary.
#[derive(Debug, Clone, Copy)]
pub struct CliGit;

impl Git for CliGit {
    fn repo_root_cwd(&self) -> Result<String, GitError> {
        crate::status::get_repo_root()
    }

    fn repo_root_in(&self, dir: &str) -> Result<String, GitError> {
        crate::status::get_repo_root_in(dir)
    }

    fn repo_root_at(&self, path: &str) -> Result<String, GitError> {
        crate::status::get_repo_root_at(path)
    }

    fn current_branch(&self, repo_root: &str) -> Result<String, GitError> {
        crate::status::get_current_branch_in(repo_root)
    }

    fn detect_base_branch(&self, repo_root: &str) -> Result<String, GitError> {
        crate::status::detect_base_branch_in(repo_root)
    }

    fn diff_raw(
        &self,
        mode: &str,
        base: &str,
        repo_root: &str,
        head_ref: Option<&str>,
    ) -> Result<String, GitError> {
        crate::status::git_diff_raw(mode, base, repo_root, head_ref)
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
        crate::status::git_diff_raw_file(mode, base, repo_root, path, context_lines, head_ref)
    }

    fn diff_raw_range(&self, from: &str, to: &str, repo_root: &str) -> Result<String, GitError> {
        crate::status::git_diff_raw_range(from, to, repo_root)
    }

    fn diff_checkout_against_base(&self, root: &str, base: &str) -> Result<String, GitError> {
        crate::status::git_diff_checkout_against_base(root, base)
    }

    fn diff_against_branch(
        &self,
        root: &str,
        base: &str,
        branch: &str,
    ) -> Result<String, GitError> {
        crate::status::git_diff_against_branch(root, base, branch)
    }

    fn diff_commit(&self, hash: &str, repo_root: &str) -> Result<String, GitError> {
        crate::status::git_diff_commit(hash, repo_root)
    }

    fn diff_conflicts(&self, repo_root: &str) -> Result<String, GitError> {
        crate::status::git_diff_conflicts(repo_root)
    }

    fn unmerged_files(&self, repo_root: &str) -> Result<Vec<String>, GitError> {
        crate::status::unmerged_files(repo_root)
    }

    fn is_merge_in_progress(&self, repo_root: &str) -> bool {
        crate::status::is_merge_in_progress(repo_root)
    }

    fn diff_shortstat(&self, repo_root: &str, staged: bool) -> (usize, usize, usize) {
        crate::status::diff_shortstat(repo_root, staged)
    }

    fn stage_file(&self, repo_root: &str, file_path: &str) -> Result<(), GitError> {
        crate::status::git_stage_file(repo_root, file_path)
    }

    fn unstage_file(&self, repo_root: &str, file_path: &str) -> Result<(), GitError> {
        crate::status::git_unstage_file(repo_root, file_path)
    }

    fn stage_all(&self, repo_root: &str) -> Result<(), GitError> {
        crate::status::git_stage_all(repo_root)
    }

    fn commit(&self, repo_root: &str, message: &str) -> Result<(), GitError> {
        crate::status::git_commit(repo_root, message)
    }

    fn push(&self, repo_root: &str) -> Result<String, GitError> {
        crate::status::git_push(repo_root)
    }

    fn log_range(
        &self,
        from: &str,
        to: &str,
        repo_root: &str,
        limit: usize,
        skip: usize,
    ) -> Result<Vec<CommitInfo>, GitError> {
        crate::status::git_log_range(from, to, repo_root, limit, skip)
    }

    fn log_head(&self, repo_root: &str, limit: usize) -> Result<Vec<CommitInfo>, GitError> {
        crate::status::git_log_head(repo_root, limit)
    }

    fn commit_shortstats(
        &self,
        repo_root: &str,
        shas: &[&str],
    ) -> HashMap<String, (usize, usize, usize)> {
        crate::status::git_commit_shortstats(repo_root, shas)
    }

    fn unpushed_shas(
        &self,
        repo_root: &str,
        head: &str,
        pushed_refs: &[&str],
        limit: usize,
    ) -> HashSet<String> {
        crate::status::git_unpushed_shas(repo_root, head, pushed_refs, limit)
    }

    fn list_worktrees(&self, repo_root: &str) -> Result<Vec<Worktree>, GitError> {
        crate::status::list_worktrees(repo_root)
    }

    fn blob_oid(&self, root: &str, spec: &str) -> Result<String, GitError> {
        crate::status::git_blob_oid(root, spec)
    }

    fn read_blob(&self, root: &str, spec: &str, max_bytes: usize) -> Result<Vec<u8>, GitError> {
        crate::status::git_read_blob(root, spec, max_bytes)
    }

    fn paths_present_at(
        &self,
        root: &str,
        rev: &str,
        paths: &[String],
    ) -> Result<Vec<String>, GitError> {
        crate::status::git_paths_present_at(root, rev, paths)
    }

    fn gitignored_paths(&self, repo_root: &str, paths: &[String]) -> HashSet<String> {
        crate::status::gitignored_paths(repo_root, paths)
    }

    fn diff_watched_file_snapshot(
        &self,
        repo_root: &str,
        rel_path: &str,
        snapshots_dir: &str,
    ) -> Result<Option<String>, GitError> {
        crate::status::diff_watched_file_snapshot(repo_root, rel_path, snapshots_dir)
    }

    fn blame_range(&self, request: &BlameRequest<'_>) -> Result<LineAttribution, GitError> {
        crate::blame::blame_range(request)
    }

    fn merge_base(&self, repo_root: &str, a: &str, b: &str) -> Result<String, GitError> {
        crate::blame::git_merge_base(repo_root, a, b)
    }

    fn rev_parse_commit(&self, repo_root: &str, rev: &str) -> Result<String, GitError> {
        crate::blame::git_rev_parse_commit(repo_root, rev)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(root: &std::path::Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(root)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {out:?}");
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    #[test]
    fn cli_git_diff_raw_equals_the_free_function() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q", "-b", "main"]);
        git(root, &["config", "user.email", "t@example.invalid"]);
        git(root, &["config", "user.name", "Tests"]);
        std::fs::write(root.join("f"), "one\n").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-q", "-m", "base"]);
        git(root, &["checkout", "-q", "-b", "feature"]);
        std::fs::write(root.join("f"), "one\ntwo\n").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-q", "-m", "change"]);

        let root = root.to_str().unwrap();
        let via_trait = CliGit.diff_raw("branch", "main", root, None).unwrap();
        let via_free = crate::status::git_diff_raw("branch", "main", root, None).unwrap();
        assert_eq!(via_trait, via_free);
        assert!(via_trait.contains("+two"), "{via_trait}");
    }
}
