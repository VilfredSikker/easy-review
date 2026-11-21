use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The registered projects in sidebar order.
///
/// Whether a project is the active one is left out: the frontend derives it
/// from the `tabs` resource, so a tab switch moves no sidebar version.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Projects {
    pub projects: Vec<Project>,
}

/// One project record, without its pull requests.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    /// Empty for a project known only by its GitHub remote.
    pub root_path: String,
    pub remote: Option<String>,
    pub remote_only: bool,
    /// Skip triage when the filtered diff is larger than this, in KB; `0` sets no limit.
    pub auto_triage_max_diff_kb: u32,
    /// Globs left out of AI review diffs.
    pub review_ignore_globs: Vec<String>,
}

/// One project's branches, from the metadata cache.
///
/// Which branch is current is left out: for the project the active tab
/// shows it is the branch that tab views, and otherwise the checkout's
/// branch, which `current_branch` carries.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Branches {
    /// The branch the project's checkout is on; empty when unknown.
    pub current_branch: String,
    /// The checkout's branch, the tracked ones and those with a worktree.
    pub local_branches: Vec<Branch>,
    /// The most recently committed branches beyond those, up to ten.
    pub auto_branches: Vec<Branch>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Branch {
    pub name: String,
    pub upstream: Option<String>,
    pub is_merged: bool,
    /// The branch's worktree on disk, when one is checked out.
    pub worktree_path: Option<String>,
    /// The open pull request whose head is this branch, when there is one.
    // JSON over IPC delivers a number, never a BigInt; PR numbers stay below 2^53.
    #[ts(type = "number | null")]
    pub pr_number: Option<u64>,
}

/// One remote's cached pull requests, and each project's lists of them.
///
/// The value moves whenever any row field moves, `head_oid` included. How
/// old the fetch is, and so whether it is stale, is left to the frontend:
/// an age in the value would move it on every read.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Prs {
    /// One row per number: the remote's cached pull requests, dismissed ones
    /// included, then a title-only row for each saved or recent entry the
    /// cache does not hold.
    pub rows: Vec<PrRow>,
    /// When the remote's list was last fetched, in ms since the epoch.
    // JSON over IPC delivers a number, never a BigInt; epoch ms stay below 2^53.
    #[ts(type = "number | null")]
    pub fetched_at_ms: Option<u64>,
    /// The lists of each project on this remote, in the record's order.
    pub projects: Vec<ProjectPrs>,
}

/// One project's pull request lists, as numbers into `Prs::rows`. Its
/// dismissed numbers are already left out of mine, to review and recently
/// merged.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPrs {
    pub project: String,
    /// Open pull requests the signed-in user wrote.
    #[ts(type = "number[]")]
    pub mine: Vec<u64>,
    /// Open pull requests by others that still wait on the user's review.
    #[ts(type = "number[]")]
    pub to_review: Vec<u64>,
    /// Merged pull requests, newest merge first, up to 25.
    #[ts(type = "number[]")]
    pub recently_merged: Vec<u64>,
    /// Saved pull requests, newest save first.
    #[ts(type = "number[]")]
    pub saved: Vec<u64>,
    /// Pull requests opened for review, newest first.
    #[ts(type = "number[]")]
    pub recent: Vec<u64>,
    /// The numbers the sidebar's Ignore hid, which Unignore lists.
    #[ts(type = "number[]")]
    pub dismissed: Vec<u64>,
}

/// One pull request as the remote's list holds it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PrRow {
    #[ts(type = "number")]
    pub number: u64,
    pub title: String,
    pub head_ref: String,
    pub base_ref: String,
    /// The head commit; empty in a title-only row.
    pub head_oid: String,
    /// `OPEN`, `CLOSED` or `MERGED`; empty in a title-only row.
    pub state: String,
    pub is_draft: bool,
    pub author: String,
    pub assignees: Vec<String>,
    pub reviewers: Vec<String>,
    pub checks_state: Option<String>,
    pub review_decision: Option<String>,
    pub merged_at: Option<String>,
    pub updated_at: String,
    /// The signed-in user's latest review approves it.
    pub approved_by_me: bool,
}

/// A project's cached pull requests it neither tracks nor dismissed, and
/// that the signed-in user did not write, is not assigned and was not asked
/// to review.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AvailablePrs {
    pub prs: Vec<PrRow>,
}

/// The worktrees of the repository at one root, as `git worktree list`
/// gives them, dismissed branches and detached checkouts included.
///
/// Which one is current is left out: the frontend compares each path with
/// the root its tab shows.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Worktrees {
    pub worktrees: Vec<Worktree>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Worktree {
    pub path: String,
    /// `(detached)` for a checkout with no branch.
    pub branch: String,
    /// The branch merges from a pull request's head (`refs/pull/<n>/head`).
    pub is_pr: bool,
    #[ts(type = "number | null")]
    pub pr_number: Option<u64>,
    pub is_merged: bool,
    /// The repository's `owner/repo`; every row of a list carries the same.
    pub remote: Option<String>,
}

/// Branch names a project could start tracking.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AvailableBranches {
    pub branches: Vec<String>,
}

/// The inbox, built on the server: the items newest first with the kinds the
/// person hid left out, how many of those are unread, and when the PR sweep
/// last ran. The frontend shows the count and never counts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Inbox {
    pub items: Vec<InboxItem>,
    /// The unread items among `items`.
    // JSON over IPC delivers a number, never a BigInt; the inbox keeps 200 items.
    #[ts(type = "number")]
    pub unread_count: u64,
    /// When the PR sweep last ran, in ms since the epoch; 0 before the first.
    #[ts(type = "number")]
    pub last_refresh_ms: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct InboxItem {
    pub id: String,
    /// What happened, as the sweep names it (`ci_failed`, `pr_merged`, …).
    pub kind: String,
    /// The filter group of `kind` (`ci`, `lifecycle`, …).
    pub category: String,
    pub severity: String,
    pub title: String,
    pub body: String,
    pub source: String,
    pub target: InboxTarget,
    #[ts(type = "number")]
    pub created_at_ms: u64,
    /// None while the item is unread.
    #[ts(type = "number | null")]
    pub read_at_ms: Option<u64>,
    pub dedupe_key: String,
}

/// What an inbox item opens; each part is what the event named.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct InboxTarget {
    pub project_id: Option<String>,
    pub repo_root: Option<String>,
    pub remote: Option<String>,
    #[ts(type = "number | null")]
    pub pr_number: Option<u64>,
    pub branch: Option<String>,
    pub url: Option<String>,
}
