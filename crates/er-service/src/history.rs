//! The three views whose diff is no git diff of the tab's scope: History
//! (one commit's diff), Conflicts (the merge changeset) and Hidden (no diff).
//! Each loads by the engine steps the App runs for it, off the actor.

use std::sync::Arc;

use er_engine::diff_load::{self, ParsedDiff};
use er_engine::git::{self, CommitInfo};
use er_engine::history::{self, HistorySource};
use er_engine::tab_identity::{DiffMode, ResolvedTab};

use crate::content::HistorySection;

/// Whether `mode` shows a diff the tab's scope does not fetch.
pub(crate) const fn own_diff(mode: DiffMode) -> bool {
    matches!(
        mode,
        DiffMode::History | DiffMode::Conflicts | DiffMode::Hidden
    )
}

/// What a load knows about History.
pub(crate) struct HistoryInput {
    /// The section the tab held, which a load in another view carries.
    pub prev: Option<Arc<HistorySection>>,
    /// The view History returns to.
    pub return_mode: DiffMode,
    /// The commit to show, when History lists it.
    pub select: Option<String>,
}

impl HistoryInput {
    /// The section `prev` names, returning to where it did.
    pub fn carried(prev: Option<Arc<HistorySection>>) -> Self {
        let return_mode = prev.as_ref().map_or(DiffMode::Branch, |h| h.return_mode);
        Self {
            prev,
            return_mode,
            select: None,
        }
    }
}

/// What History reads from `tab`.
pub(crate) fn source(tab: &ResolvedTab, return_mode: DiffMode) -> HistorySource<'_> {
    HistorySource {
        repo_root: &tab.repo_root,
        current_branch: &tab.current_branch,
        base_branch: &tab.base_branch,
        pr_number: tab.pr_number,
        local_branch_view: tab.local_branch_view.as_deref(),
        pr_head_ref: tab.pr_head_ref.as_deref(),
        checkout_root: tab.checkout_root.as_deref(),
        is_remote: tab.identity().is_remote(),
        return_mode,
    }
}

/// The view's files, parsed whole.
pub(crate) struct OwnDiff {
    pub raw: String,
    pub parsed: ParsedDiff,
    pub history: Option<Arc<HistorySection>>,
    /// The PR head ref a missing commit made History fetch.
    pub fetched_head: Option<String>,
    /// Conflicts' merge flag and unresolved count.
    pub conflicts: Option<(bool, usize)>,
}

/// Loads the view `tab.mode` shows, which [`own_diff`] names. History lists
/// its commits as the App does, keeping the list it held unless it was
/// entered from the other source, and diffs the selected commit; a remote
/// PR's commits are display-only, and a commit git cannot diff lists no
/// files, as on the App. Conflicts parse the merge changeset; Hidden lists
/// nothing, since the desktop shows no watched files.
pub(crate) fn load(
    git: &dyn git::Git,
    tab: &ResolvedTab,
    input: &HistoryInput,
    pr_commits: &[CommitInfo],
) -> OwnDiff {
    let mut conflicts = None;
    let (raw, files, section, fetched_head) = match tab.mode {
        DiffMode::History => {
            let section = section(git, tab, input, pr_commits);
            let diff = match section.selected.as_deref() {
                Some(sha) if !tab.identity().is_remote() => {
                    history::commit_diff(git, &source(tab, input.return_mode), sha)
                }
                _ => history::CommitDiff::default(),
            };
            (
                diff.raw,
                diff.files,
                Some(Arc::new(section)),
                diff.fetched_head,
            )
        }
        DiffMode::Conflicts => {
            let merge = diff_load::conflict_files(git, &tab.repo_root);
            conflicts = Some((merge.merge_active, merge.unresolved));
            (merge.raw, merge.files, input.prev.clone(), None)
        }
        _ => (String::new(), Vec::new(), input.prev.clone(), None),
    };
    OwnDiff {
        parsed: ParsedDiff {
            headers: git::parse_diff_headers(&raw),
            files,
            lazy: false,
        },
        raw,
        history: section,
        fetched_head,
        conflicts,
    }
}

fn section(
    git: &dyn git::Git,
    tab: &ResolvedTab,
    input: &HistoryInput,
    pr_commits: &[CommitInfo],
) -> HistorySection {
    let kept = input
        .prev
        .as_deref()
        .filter(|prev| prev.return_mode == input.return_mode);
    let (commits, all_loaded, selected) = match kept {
        Some(prev) => (prev.commits.clone(), prev.all_loaded, prev.selected.clone()),
        None => {
            let source = source(tab, input.return_mode);
            let (commits, all_loaded) = history::history_commits(git, &source, pr_commits);
            let first = commits.first().map(|c| c.hash.clone());
            (commits, all_loaded, first)
        }
    };
    let picked = input
        .select
        .clone()
        .filter(|sha| commits.iter().any(|c| &c.hash == sha));
    HistorySection {
        commits,
        all_loaded,
        selected: picked.or(selected),
        return_mode: input.return_mode,
    }
}

/// History's next page after the commits `held` lists, by the App's rule: a
/// PR branch view lists its PR's commits whole, so it adds none.
pub(crate) fn more(
    git: &dyn git::Git,
    tab: &ResolvedTab,
    held: &HistorySection,
) -> Vec<CommitInfo> {
    if tab.pr_number.is_some() && tab.local_branch_view.is_some() {
        return Vec::new();
    }
    let source = source(tab, held.return_mode);
    history::more_commits(git, &source, held.commits.len())
}

/// The commit scroller's rows for `tab`, read in every view.
pub(crate) fn scroller(
    git: &dyn git::Git,
    tab: &ResolvedTab,
    held: Option<&HistorySection>,
    pr_commits: &[CommitInfo],
) -> Vec<history::ScrollerCommit> {
    let return_mode = held.map_or(DiffMode::Branch, |h| h.return_mode);
    let source = source(tab, return_mode);
    let commits =
        history::scroller_commits(git, &source, held.map(|h| h.commits.as_slice()), pr_commits);
    let meta = if commits.is_empty() || source.is_remote {
        history::CommitMeta::default()
    } else {
        let listed = history::listed_by_github(&source, held.is_some());
        history::commit_meta(git, &source, listed, &commits)
    };
    history::scroller_rows(commits, &meta)
}
