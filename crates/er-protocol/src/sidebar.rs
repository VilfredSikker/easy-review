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
