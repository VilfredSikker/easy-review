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

    fn remote_url(&self, root: &str, name: &str) -> Result<String, GitError>;
    fn rev_parse_verify(&self, root: &str, rev: &str) -> Result<Option<String>, GitError>;
    fn fetch(&self, root: &str, args: &[&str]) -> Result<(), GitError>;
    fn update_ref(&self, root: &str, name: &str, target: &str) -> Result<(), GitError>;
    fn diff_revs(&self, root: &str, revs: &[&str]) -> Result<String, GitError>;
    fn shallow_clone(&self, url: &str, dest: &str) -> Result<(), GitError>;

    fn show_ref_verify(&self, root: &str, refname: &str) -> Result<bool, GitError>;
    fn current_branch_show(&self, root: &str) -> Result<String, GitError>;
    fn for_each_ref(
        &self,
        root: &str,
        format: &str,
        sort: Option<&str>,
        pattern: &str,
    ) -> Result<String, GitError>;
    fn ls_remote(&self, root: &str, remote: &str, refname: &str) -> Result<String, GitError>;
    fn config_get_regexp(&self, root: &str, pattern: &str) -> Result<String, GitError>;
    fn rev_list_count(&self, root: &str, range: &str) -> Result<String, GitError>;
    fn branch_merged(&self, root: &str, base: &str) -> Result<String, GitError>;
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

    fn remote_url(&self, root: &str, name: &str) -> Result<String, GitError> {
        crate::status::git_remote_url(root, name)
    }

    fn rev_parse_verify(&self, root: &str, rev: &str) -> Result<Option<String>, GitError> {
        crate::status::git_rev_parse_verify(root, rev)
    }

    fn fetch(&self, root: &str, args: &[&str]) -> Result<(), GitError> {
        crate::status::git_fetch(root, args)
    }

    fn update_ref(&self, root: &str, name: &str, target: &str) -> Result<(), GitError> {
        crate::status::git_update_ref(root, name, target)
    }

    fn diff_revs(&self, root: &str, revs: &[&str]) -> Result<String, GitError> {
        crate::status::git_diff_revs(root, revs)
    }

    fn shallow_clone(&self, url: &str, dest: &str) -> Result<(), GitError> {
        crate::status::git_shallow_clone(url, dest)
    }

    fn show_ref_verify(&self, root: &str, refname: &str) -> Result<bool, GitError> {
        crate::status::git_show_ref_verify(root, refname)
    }

    fn current_branch_show(&self, root: &str) -> Result<String, GitError> {
        crate::status::git_current_branch_show(root)
    }

    fn for_each_ref(
        &self,
        root: &str,
        format: &str,
        sort: Option<&str>,
        pattern: &str,
    ) -> Result<String, GitError> {
        crate::status::git_for_each_ref(root, format, sort, pattern)
    }

    fn ls_remote(&self, root: &str, remote: &str, refname: &str) -> Result<String, GitError> {
        crate::status::git_ls_remote(root, remote, refname)
    }

    fn config_get_regexp(&self, root: &str, pattern: &str) -> Result<String, GitError> {
        crate::status::git_config_get_regexp(root, pattern)
    }

    fn rev_list_count(&self, root: &str, range: &str) -> Result<String, GitError> {
        crate::status::git_rev_list_count(root, range)
    }

    fn branch_merged(&self, root: &str, base: &str) -> Result<String, GitError> {
        crate::status::git_branch_merged(root, base)
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

    fn repo_with_a_feature_commit() -> tempfile::TempDir {
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
        dir
    }

    #[test]
    fn cli_git_diff_raw_equals_the_free_function() {
        let dir = repo_with_a_feature_commit();
        let root = dir.path().to_str().unwrap();
        let via_trait = CliGit.diff_raw("branch", "main", root, None).unwrap();
        let via_free = crate::status::git_diff_raw("branch", "main", root, None).unwrap();
        assert_eq!(via_trait, via_free);
        assert!(via_trait.contains("+two"), "{via_trait}");
    }

    #[test]
    fn remote_url_reads_origin() {
        let dir = repo_with_a_feature_commit();
        let root = dir.path();
        git(
            root,
            &["remote", "add", "origin", "git@example.invalid:o/r.git"],
        );
        assert_eq!(
            CliGit.remote_url(root.to_str().unwrap(), "origin").unwrap(),
            "git@example.invalid:o/r.git"
        );
    }

    #[test]
    fn rev_parse_verify_is_none_for_a_missing_ref() {
        let dir = repo_with_a_feature_commit();
        let root = dir.path().to_str().unwrap();
        assert!(CliGit.rev_parse_verify(root, "HEAD").unwrap().is_some());
        assert_eq!(
            CliGit
                .rev_parse_verify(root, "refs/heads/not-there")
                .unwrap(),
            None
        );
    }

    #[test]
    fn update_ref_points_a_ref_at_a_commit() {
        let dir = repo_with_a_feature_commit();
        let root = dir.path().to_str().unwrap();
        let head = CliGit.rev_parse_verify(root, "HEAD").unwrap().unwrap();

        CliGit.update_ref(root, "refs/er/test/head", &head).unwrap();

        assert_eq!(
            CliGit.rev_parse_verify(root, "refs/er/test/head").unwrap(),
            Some(head)
        );
    }

    #[test]
    fn diff_revs_three_dot_diffs_against_the_merge_base() {
        let dir = repo_with_a_feature_commit();
        let root = dir.path().to_str().unwrap();

        let raw = CliGit.diff_revs(root, &["main...HEAD"]).unwrap();

        assert!(raw.contains("+two"), "{raw}");
    }

    #[test]
    fn show_ref_verify_is_false_for_a_missing_branch() {
        let dir = repo_with_a_feature_commit();
        let root = dir.path().to_str().unwrap();

        assert!(CliGit.show_ref_verify(root, "refs/heads/feature").unwrap());
        assert!(!CliGit
            .show_ref_verify(root, "refs/heads/not-there")
            .unwrap());
    }

    #[test]
    fn current_branch_show_names_the_checked_out_branch() {
        let dir = repo_with_a_feature_commit();
        assert_eq!(
            CliGit
                .current_branch_show(dir.path().to_str().unwrap())
                .unwrap(),
            "feature"
        );
    }

    #[test]
    fn for_each_ref_returns_the_requested_format() {
        let dir = repo_with_a_feature_commit();
        let root = dir.path().to_str().unwrap();

        let names = CliGit
            .for_each_ref(root, "%(refname:short)", None, "refs/heads/")
            .unwrap();
        assert!(names.lines().any(|l| l == "feature"), "{names}");
        assert!(names.lines().any(|l| l == "main"), "{names}");
    }

    /// Commit with a fixed author and committer date, so a date sort can be
    /// told apart from the default refname order.
    fn commit_at(root: &std::path::Path, date: &str, message: &str) {
        std::fs::write(root.join("f"), format!("{message}\n")).unwrap();
        git(root, &["add", "."]);
        let out = std::process::Command::new("git")
            .args(["-c", "commit.gpgsign=false", "commit", "-q", "-m", message])
            .current_dir(root)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_DATE", date)
            .env("GIT_COMMITTER_DATE", date)
            .output()
            .unwrap();
        assert!(out.status.success(), "git commit: {out:?}");
    }

    #[test]
    fn for_each_ref_sorts_by_the_requested_key() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q", "-b", "main"]);
        git(root, &["config", "user.email", "t@example.invalid"]);
        git(root, &["config", "user.name", "Tests"]);
        commit_at(root, "2019-01-01 00:00:00 +0000", "base");
        git(root, &["checkout", "-q", "-b", "aaa"]);
        commit_at(root, "2020-01-01 00:00:00 +0000", "older");
        git(root, &["checkout", "-q", "main"]);
        git(root, &["checkout", "-q", "-b", "zzz"]);
        commit_at(root, "2020-06-01 00:00:00 +0000", "newer");

        let sorted = CliGit
            .for_each_ref(
                root.to_str().unwrap(),
                "%(refname:short)",
                Some("-committerdate"),
                "refs/heads/",
            )
            .unwrap();
        assert_eq!(
            sorted.lines().collect::<Vec<_>>(),
            ["zzz", "aaa", "main"],
            "{sorted}"
        );

        // The default order is refname, so the assertion above only holds when
        // the sort argument reaches git.
        let unsorted = CliGit
            .for_each_ref(
                root.to_str().unwrap(),
                "%(refname:short)",
                None,
                "refs/heads/",
            )
            .unwrap();
        assert_eq!(
            unsorted.lines().collect::<Vec<_>>(),
            ["aaa", "main", "zzz"],
            "{unsorted}"
        );
    }

    #[test]
    fn config_get_regexp_lists_remote_urls() {
        let dir = repo_with_a_feature_commit();
        let root = dir.path();
        git(
            root,
            &["remote", "add", "origin", "git@example.invalid:o/r.git"],
        );

        let out = CliGit
            .config_get_regexp(root.to_str().unwrap(), r"^remote\..*\.url$")
            .unwrap();

        assert!(out.contains("remote.origin.url"), "{out}");
    }

    #[test]
    fn rev_list_count_counts_commits_in_a_range() {
        let dir = repo_with_a_feature_commit();
        let root = dir.path().to_str().unwrap();

        assert_eq!(CliGit.rev_list_count(root, "main..feature").unwrap(), "1");
    }

    #[test]
    fn branch_merged_lists_branches_merged_into_base() {
        let dir = repo_with_a_feature_commit();
        let root = dir.path();
        git(root, &["checkout", "-q", "main"]);
        git(root, &["merge", "--no-ff", "-m", "merge", "feature"]);

        let merged = CliGit
            .branch_merged(root.to_str().unwrap(), "main")
            .unwrap();

        assert!(merged.lines().any(|l| l.trim() == "feature"), "{merged}");
    }

    #[test]
    fn fetch_pulls_a_ref_from_a_local_origin() {
        let origin = repo_with_a_feature_commit();
        let dest = tempfile::tempdir().unwrap();
        git(dest.path(), &["init", "-q", "-b", "main"]);
        git(
            dest.path(),
            &["remote", "add", "origin", origin.path().to_str().unwrap()],
        );

        CliGit
            .fetch(
                dest.path().to_str().unwrap(),
                &["origin", "feature:refs/remotes/origin/feature"],
            )
            .unwrap();

        assert!(CliGit
            .rev_parse_verify(dest.path().to_str().unwrap(), "refs/remotes/origin/feature")
            .unwrap()
            .is_some());
    }

    #[test]
    fn ls_remote_asks_the_remote_for_one_ref() {
        let origin = repo_with_a_feature_commit();
        let dest = tempfile::tempdir().unwrap();
        git(dest.path(), &["init", "-q", "-b", "main"]);
        git(
            dest.path(),
            &["remote", "add", "origin", origin.path().to_str().unwrap()],
        );

        let out = CliGit
            .ls_remote(
                dest.path().to_str().unwrap(),
                "origin",
                "refs/heads/feature",
            )
            .unwrap();

        let mut parts = out.split_whitespace();
        assert_eq!(parts.next().map(str::len), Some(40), "{out}");
        assert_eq!(parts.next(), Some("refs/heads/feature"), "{out}");
        assert_eq!(parts.next(), None, "{out}");
    }
}
