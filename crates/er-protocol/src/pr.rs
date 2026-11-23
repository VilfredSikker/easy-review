use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One open tab's pull request: the card and the stack. Field names follow
/// the snapshot's `pr` and `stack`, so the flag-off and flag-on reads have
/// one shape.
///
/// A key the registry lists but no workspace has loaded answers this empty at
/// version 0, as `diff(tab)` does.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TabPr {
    /// None for a tab with no PR, or one GitHub could not describe.
    pub pr: Option<PrCard>,
    /// None when the tab has nowhere to read a stack from: a local PR or
    /// branch view whose head is not checked out and that has no PR chain.
    pub stack: Option<Stack>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PrCard {
    #[ts(type = "number")]
    pub number: u64,
    pub title: String,
    pub state: String,
    pub base: String,
    pub head: String,
    pub url: String,
    pub author: String,
}

/// The tab's `gh stack`. Before the first lookup `layers` is empty and
/// `unavailable` none, which the page reads as "not looked up yet".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Stack {
    pub trunk: String,
    /// Top of the stack first.
    pub layers: Vec<StackLayer>,
    /// 1-based from the top; none when the viewed branch is not a layer.
    #[ts(type = "number | null")]
    pub position: Option<u64>,
    #[ts(type = "number")]
    pub size: u64,
    pub unavailable: Option<String>,
    /// `unavailable` is a failed lookup the page can retry.
    pub retryable: bool,
    /// The page holds its own in-flight flag, so this is false.
    pub loading: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct StackLayer {
    pub branch: String,
    #[ts(type = "number | null")]
    pub pr_number: Option<u64>,
    pub pr_url: Option<String>,
    pub state: String,
    pub is_current: bool,
    pub enabled: bool,
    pub needs_rebase: bool,
}

/// One pull request's GitHub status, as the status cache last stored it;
/// none until a fetch stores one. Field names follow the snapshot's
/// `github`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PrStatus {
    pub status: Option<GithubStatus>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct GithubStatus {
    pub owner: String,
    pub repo: String,
    #[ts(type = "number")]
    pub number: u64,
    pub url: String,
    pub state: String,
    pub is_draft: bool,
    pub title: String,
    pub body: String,
    pub author: String,
    pub head_ref: String,
    pub base_ref: String,
    pub review_decision: Option<String>,
    pub mergeable: Option<String>,
    pub labels: Vec<String>,
    pub checks: Vec<CheckRun>,
    #[ts(type = "number")]
    pub comments_count: u64,
    #[ts(type = "number")]
    pub reviews_count: u64,
    pub recent_comments: Vec<StatusComment>,
    pub recent_reviews: Vec<StatusReview>,
    pub last_updated: Option<String>,
    pub is_authored_by_me: bool,
    pub merge_state_status: Option<String>,
    pub head_oid: String,
    pub is_cross_repository: bool,
    pub auto_merge_method: Option<String>,
    pub head_branch_exists: Option<bool>,
    pub repo_merge: Option<RepoMerge>,
    pub in_merge_queue: bool,
    pub base_has_merge_queue: bool,
    pub can_bypass_rules: bool,
    pub created_at: Option<String>,
    #[ts(type = "number")]
    pub commit_count: u64,
    pub reviewers: Vec<StatusReviewer>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct CheckRun {
    pub name: String,
    pub status: String,
    pub conclusion: String,
    pub url: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    #[ts(type = "number | null")]
    pub duration_secs: Option<u64>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct StatusComment {
    pub author: String,
    pub body: String,
    pub created_at: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct StatusReview {
    pub author: String,
    pub state: String,
    pub body: String,
    pub submitted_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct StatusReviewer {
    pub reviewer: String,
    pub is_team: bool,
    pub requested: bool,
    pub state: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RepoMerge {
    pub merge_commit_allowed: bool,
    pub squash_merge_allowed: bool,
    pub rebase_merge_allowed: bool,
    pub delete_branch_on_merge: bool,
    pub auto_merge_allowed: bool,
    pub viewer_permission: Option<String>,
}
