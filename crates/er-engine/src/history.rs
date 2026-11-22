//! History's steps: which commits a tab lists, one commit's diff, and the
//! short list the commit scroller shows in every view.
//!
//! They read the tab's fields, not the tab, so they live in the half of the
//! engine that always compiles: `TabState` runs them for the TUI and the
//! flag-off desktop, and the headless service runs the same steps for the
//! tabs it loads.

use std::collections::{HashMap, HashSet};

use crate::git::{self, CommitInfo, DiffFile, Git};
use crate::tab_identity::DiffMode;

/// How many commits History lists at first, and how many each page adds.
pub const HISTORY_PAGE: usize = 50;

/// How many commits the commit scroller shows.
pub const SCROLLER_LIMIT: usize = 10;

/// What History reads from a tab to list and diff its commits.
#[derive(Debug, Clone, Copy)]
pub struct HistorySource<'a> {
    pub repo_root: &'a str,
    pub current_branch: &'a str,
    pub base_branch: &'a str,
    pub pr_number: Option<u64>,
    pub local_branch_view: Option<&'a str>,
    /// The fetched PR head ref a local PR tab diffs against.
    pub pr_head_ref: Option<&'a str>,
    /// The working tree that has the viewed branch checked out, if any.
    pub checkout_root: Option<&'a str>,
    pub is_remote: bool,
    /// The view History was entered from, which it returns to.
    pub return_mode: DiffMode,
}

impl<'a> HistorySource<'a> {
    /// A local PR tab: a branch view of a PR's head.
    pub const fn is_pr_review_tab(&self) -> bool {
        self.pr_number.is_some() && self.local_branch_view.is_some()
    }

    /// Root to run commit-log and diff git commands in: the branch's own
    /// worktree when the tab views a checked-out local branch, else the repo.
    pub fn commit_log_root(&self) -> &'a str {
        self.checkout_root.unwrap_or(self.repo_root)
    }

    /// Ref to log the branch's commits against base (`base..head`): the
    /// viewed branch, so the list matches the diff even when that branch is
    /// not checked out at the log root.
    pub fn commit_head_ref(&self) -> &'a str {
        self.local_branch_view.unwrap_or(self.current_branch)
    }

    /// The `(head ref, root)` History logs `base..head` in. The first page and
    /// every later page must share it, or paging past the PR's last commit
    /// appends the checked-out branch's commits to the PR's list.
    pub fn log_source(&self) -> (String, String) {
        match self.pr_head_ref {
            Some(pr_head) if self.return_mode == DiffMode::PrDiff => {
                (pr_head.to_string(), self.repo_root.to_string())
            }
            _ => (
                self.commit_head_ref().to_string(),
                self.commit_log_root().to_string(),
            ),
        }
    }
}

/// The view History returns to when entered from `from`: the PR diff when
/// it was entered from the PR diff, a Tour of it, or a remote PR.
pub const fn return_mode(from: DiffMode, tour_is_pr: bool, is_remote: bool) -> DiffMode {
    let from_pr = matches!(from, DiffMode::PrDiff)
        || (matches!(from, DiffMode::Tour) && tour_is_pr)
        || is_remote;
    if from_pr {
        DiffMode::PrDiff
    } else {
        DiffMode::Branch
    }
}

/// The commits History lists, and whether that list is already complete.
///
/// Entered from the PR diff, the list is the PR's own commits: the local
/// branch log would list whatever HEAD is checked out, which on a local PR
/// tab is usually not the PR. Without a fetched commit list it logs the
/// fetched PR head ref, never a checkout (ADR 0020).
pub fn history_commits(
    git: &dyn Git,
    source: &HistorySource<'_>,
    pr_commits: &[CommitInfo],
) -> (Vec<CommitInfo>, bool) {
    let from_pr = source.return_mode == DiffMode::PrDiff;
    if source.is_pr_review_tab() || (from_pr && (source.is_remote || !pr_commits.is_empty())) {
        return (pr_commits.to_vec(), true);
    }
    (more_commits(git, source, 0), false)
}

/// The next page of History's commits after the first `skip`.
pub fn more_commits(git: &dyn Git, source: &HistorySource<'_>, skip: usize) -> Vec<CommitInfo> {
    let (head_ref, log_root) = source.log_source();
    git.log_range(source.base_branch, &head_ref, &log_root, HISTORY_PAGE, skip)
        .unwrap_or_default()
}

/// One commit's diff, parsed.
#[derive(Debug, Default)]
pub struct CommitDiff {
    /// Empty when git could not diff the commit.
    pub raw: String,
    pub files: Vec<DiffFile>,
    /// The PR head ref fetched to bring a missing commit in, if one was.
    pub fetched_head: Option<String>,
    /// Why git could not diff the commit.
    pub error: Option<String>,
}

/// Diffs `hash`. On a local PR tab a commit missing from the object DB
/// means the PR head was never fetched here: fetching the head ref brings it
/// in without touching the working tree (ADR 0020), and the diff runs again.
pub fn commit_diff(git: &dyn Git, source: &HistorySource<'_>, hash: &str) -> CommitDiff {
    let mut fetched_head = None;
    let raw = match git.diff_commit(hash, source.repo_root) {
        Ok(raw) => Ok(raw),
        Err(first_err)
            if source.pr_number.is_some()
                && (source.local_branch_view.is_some()
                    || source.return_mode == DiffMode::PrDiff)
                && !source.is_remote =>
        {
            if let Some(pr_number) = source.pr_number {
                fetched_head = crate::github::fetch_pr_head(git, pr_number, source.repo_root).ok();
            }
            git.diff_commit(hash, source.repo_root)
                .map_err(|_| first_err)
        }
        Err(err) => Err(err),
    };
    match raw {
        Ok(raw) => CommitDiff {
            files: git::parse_diff(&raw),
            raw,
            fetched_head,
            error: None,
        },
        Err(err) => CommitDiff {
            fetched_head,
            error: Some(err.to_string()),
            ..CommitDiff::default()
        },
    }
}

/// The commits the scroller lists: History's list once History has loaded
/// one, a PR tab's commits from GitHub, else the viewed branch's own log. On
/// the base branch itself `base..head` is empty, so it falls back to the
/// recent HEAD history.
pub fn scroller_commits(
    git: &dyn Git,
    source: &HistorySource<'_>,
    history: Option<&[CommitInfo]>,
    pr_commits: &[CommitInfo],
) -> Vec<CommitInfo> {
    if let Some(history) = history {
        return history.iter().take(SCROLLER_LIMIT).cloned().collect();
    }
    if source.pr_number.is_some() {
        return pr_commits.iter().take(SCROLLER_LIMIT).cloned().collect();
    }
    let log_root = source.commit_log_root();
    let ranged = git
        .log_range(
            source.base_branch,
            source.commit_head_ref(),
            log_root,
            SCROLLER_LIMIT,
            0,
        )
        .unwrap_or_default();
    if ranged.is_empty() {
        git.log_head(log_root, SCROLLER_LIMIT).unwrap_or_default()
    } else {
        ranged
    }
}

/// Whether the scroller's commits are GitHub's list, by the rule History
/// seeds its list by: on a PR review tab the list is GitHub's in either view,
/// and a commit GitHub lists is pushed.
pub const fn listed_by_github(source: &HistorySource<'_>, history_loaded: bool) -> bool {
    source.pr_number.is_some() && (!history_loaded || source.local_branch_view.is_some())
}

/// Object-DB diffstats for commits whose listing carried none, and the
/// commits no remote holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommitMeta {
    pub stats: HashMap<String, (usize, usize, usize)>,
    pub unpushed: HashSet<String>,
}

/// What the scroller's rows read beside the listing. A local PR tab's GitHub
/// listing has no diffstat, so it is read from the object DB, and stays
/// unknown for a commit the DB lacks.
pub fn commit_meta(
    git: &dyn Git,
    source: &HistorySource<'_>,
    listed_by_github: bool,
    commits: &[CommitInfo],
) -> CommitMeta {
    let log_root = source.commit_log_root();
    let missing: Vec<&str> = commits
        .iter()
        .filter(|c| !c.diffstat_known)
        .map(|c| c.hash.as_str())
        .collect();
    let stats = git.commit_shortstats(log_root, &missing);
    let unpushed = if listed_by_github {
        HashSet::new()
    } else {
        // A local PR tab's fetched head counts as pushed even when it lives
        // outside refs/remotes (a fork's PR).
        let pushed_refs: Vec<&str> = source.pr_head_ref.into_iter().collect();
        git.unpushed_shas(
            log_root,
            source.commit_head_ref(),
            &pushed_refs,
            SCROLLER_LIMIT,
        )
    };
    CommitMeta { stats, unpushed }
}

/// One row of the commit scroller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrollerCommit {
    pub sha: String,
    pub title: String,
    pub author: String,
    pub committed_at: String,
    /// `None` when the diffstat is unknown, which the page shows apart from
    /// zero.
    pub additions: Option<usize>,
    pub deletions: Option<usize>,
    pub unpushed: bool,
}

/// The scroller's rows: each commit with its diffstat and whether a remote
/// holds it.
pub fn scroller_rows(commits: Vec<CommitInfo>, meta: &CommitMeta) -> Vec<ScrollerCommit> {
    commits
        .into_iter()
        .map(|c| {
            let (additions, deletions) = if c.diffstat_known {
                (Some(c.adds), Some(c.dels))
            } else {
                meta.stats
                    .get(&c.hash)
                    .map_or((None, None), |s| (Some(s.1), Some(s.2)))
            };
            ScrollerCommit {
                unpushed: meta.unpushed.contains(&c.hash),
                sha: c.hash,
                title: c.subject,
                author: c.author,
                committed_at: c.date,
                additions,
                deletions,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use er_git::{FakeGit, GitError};

    fn commit(hash: &str, known: bool) -> CommitInfo {
        CommitInfo {
            hash: hash.to_string(),
            short_hash: hash.to_string(),
            subject: format!("subject {hash}"),
            author: "a".to_string(),
            date: "2026-11-01T00:00:00Z".to_string(),
            relative_date: String::new(),
            file_count: 1,
            adds: 3,
            dels: 1,
            is_merge: false,
            diffstat_known: known,
        }
    }

    fn source(return_mode: DiffMode) -> HistorySource<'static> {
        HistorySource {
            repo_root: "/repo",
            current_branch: "feature",
            base_branch: "main",
            pr_number: None,
            local_branch_view: None,
            pr_head_ref: None,
            checkout_root: None,
            is_remote: false,
            return_mode,
        }
    }

    fn hashes(commits: &[CommitInfo]) -> Vec<&str> {
        commits.iter().map(|c| c.hash.as_str()).collect()
    }

    fn failed() -> GitError {
        GitError::Failed {
            context: "git show".to_string(),
            stderr: "bad object".to_string(),
        }
    }

    #[test]
    fn history_returns_to_the_pr_diff_only_from_it_a_tour_of_it_or_a_remote_pr() {
        assert_eq!(
            return_mode(DiffMode::PrDiff, false, false),
            DiffMode::PrDiff
        );
        assert_eq!(return_mode(DiffMode::Tour, true, false), DiffMode::PrDiff);
        assert_eq!(return_mode(DiffMode::Tour, false, false), DiffMode::Branch);
        assert_eq!(
            return_mode(DiffMode::Unstaged, false, true),
            DiffMode::PrDiff
        );
        assert_eq!(
            return_mode(DiffMode::Staged, false, false),
            DiffMode::Branch
        );
    }

    /// From the branch view History logs `base..branch` in the branch's own
    /// checkout; from the PR diff it lists the PR's commits, complete.
    #[test]
    fn history_lists_the_branch_log_or_the_prs_commits() {
        let git = FakeGit::default().with_log_range(Ok(vec![commit("b1", true)]));
        let mut branch = source(DiffMode::Branch);
        branch.checkout_root = Some("/wt");
        let (commits, all_loaded) = history_commits(&git, &branch, &[commit("p1", false)]);
        assert_eq!(hashes(&commits), ["b1"]);
        assert!(!all_loaded);
        assert_eq!(
            git.calls(),
            [r#"log_range(from="main", to="feature", repo_root="/wt", limit=50, skip=0)"#]
        );

        let mut pr = source(DiffMode::PrDiff);
        pr.pr_number = Some(7);
        let (commits, all_loaded) = history_commits(&git, &pr, &[commit("p1", false)]);
        assert_eq!(hashes(&commits), ["p1"]);
        assert!(all_loaded);
        assert_eq!(git.calls().len(), 1, "the PR's list needs no git call");
    }

    /// Paged from the PR diff, History logs the fetched PR head in the repo,
    /// never the checkout.
    #[test]
    fn a_later_page_from_the_pr_diff_logs_the_pr_head() {
        let git = FakeGit::default().with_log_range(Ok(Vec::new()));
        let mut pr = source(DiffMode::PrDiff);
        pr.pr_head_ref = Some("refs/er/pr/7/head");
        pr.checkout_root = Some("/wt");
        assert!(more_commits(&git, &pr, 50).is_empty());
        assert_eq!(
            git.calls(),
            [
                r#"log_range(from="main", to="refs/er/pr/7/head", repo_root="/repo", limit=50, skip=50)"#
            ]
        );
    }

    #[test]
    fn a_commit_diff_parses_the_commit() {
        let raw = "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-a\n+b\n";
        let git = FakeGit::default().with_diff_commit(Ok(raw.to_string()));
        let diff = commit_diff(&git, &source(DiffMode::Branch), "c1");
        assert_eq!(diff.raw, raw);
        let paths: Vec<&str> = diff.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["a.rs"]);
        assert_eq!(diff.error, None);
        assert_eq!(diff.fetched_head, None);
    }

    /// A commit missing on a local PR tab fetches the PR head and diffs again;
    /// a plain tab fetches nothing. Either way a diff that still fails is an
    /// empty list with its error.
    #[test]
    fn a_missing_commit_on_a_local_pr_fetches_the_pr_head_first() {
        let git = FakeGit::default()
            .with_diff_commit(Err(failed()))
            .with_fetch(Ok(()));
        let mut pr = source(DiffMode::PrDiff);
        pr.pr_number = Some(7);
        let diff = commit_diff(&git, &pr, "c1");
        assert_eq!(diff.fetched_head.as_deref(), Some("refs/er/pr/7/head"));
        assert!(diff.files.is_empty());
        assert_eq!(diff.error.as_deref(), Some("git show: bad object"));
        assert_eq!(git.calls().len(), 3, "diff, fetch, diff: {:?}", git.calls());

        let plain = FakeGit::default().with_diff_commit(Err(failed()));
        let diff = commit_diff(&plain, &source(DiffMode::Branch), "c1");
        assert_eq!(diff.fetched_head, None);
        assert_eq!(plain.calls().len(), 1);
    }

    /// The scroller lists History's commits once loaded, a PR tab's from
    /// GitHub, else the branch's log, and the recent HEAD log on the base.
    #[test]
    fn the_scroller_lists_ten_commits_from_the_first_source_that_has_them() {
        let many: Vec<CommitInfo> = (0..12).map(|n| commit(&format!("h{n}"), true)).collect();
        let git = FakeGit::default()
            .with_log_range(Ok(Vec::new()))
            .with_log_head(Ok(vec![commit("head", true)]));
        let plain = source(DiffMode::Branch);
        let listed = scroller_commits(&git, &plain, Some(&many), &[]);
        assert_eq!(listed.len(), SCROLLER_LIMIT);
        assert_eq!(listed[0].hash, "h0");

        let mut pr = source(DiffMode::Branch);
        pr.pr_number = Some(7);
        assert_eq!(
            hashes(&scroller_commits(&git, &pr, None, &many[..2])),
            ["h0", "h1"]
        );
        assert!(git.calls().is_empty(), "{:?}", git.calls());

        assert_eq!(hashes(&scroller_commits(&git, &plain, None, &[])), ["head"]);
        assert_eq!(
            git.calls(),
            [
                r#"log_range(from="main", to="feature", repo_root="/repo", limit=10, skip=0)"#,
                r#"log_head(repo_root="/repo", limit=10)"#,
            ]
        );
    }

    /// A row reads its listed diffstat, else the object DB's, else none; a
    /// commit GitHub lists is pushed, and the PR head counts as pushed.
    #[test]
    fn the_scroller_rows_read_the_diffstat_and_the_unpushed_set() {
        let git = FakeGit::default()
            .with_commit_shortstats([("u1".to_string(), (1, 9, 2))].into_iter().collect())
            .with_unpushed_shas(["k1".to_string()].into_iter().collect());
        let commits = vec![commit("k1", true), commit("u1", false), commit("u2", false)];
        let mut pr = source(DiffMode::Branch);
        pr.pr_head_ref = Some("refs/er/pr/7/head");
        let meta = commit_meta(&git, &pr, false, &commits);
        assert!(git
            .calls()
            .iter()
            .any(|c| c.contains(r#"pushed_refs=["refs/er/pr/7/head"]"#)));
        let rows = scroller_rows(commits.clone(), &meta);
        let stats: Vec<(Option<usize>, Option<usize>, bool)> = rows
            .iter()
            .map(|r| (r.additions, r.deletions, r.unpushed))
            .collect();
        assert_eq!(
            stats,
            [
                (Some(3), Some(1), true),
                (Some(9), Some(2), false),
                (None, None, false)
            ]
        );
        assert_eq!(rows[0].title, "subject k1");

        let listed = commit_meta(&git, &pr, true, &commits);
        assert!(listed.unpushed.is_empty(), "a listed commit is pushed");
    }

    #[test]
    fn a_pr_review_tab_lists_githubs_commits_in_every_view() {
        let mut pr = source(DiffMode::Branch);
        pr.pr_number = Some(7);
        assert!(listed_by_github(&pr, false));
        assert!(!listed_by_github(&pr, true), "History's own log");
        pr.local_branch_view = Some("pr/7");
        assert!(listed_by_github(&pr, true));
        assert!(!listed_by_github(&source(DiffMode::Branch), false));
    }
}
