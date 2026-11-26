//! The sidebar's cached rows: the branch and PR rows the metadata and PR
//! caches hold, and the GitHub status each PR's merge box reads. The owner
//! builds the wire resources from these.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct BranchInfo {
    pub name: String,
    pub upstream: Option<String>,
    pub is_current: bool,
    pub is_merged: bool,
    /// When the branch has a checked-out worktree on disk, its absolute path.
    /// Informational only — clicking a branch never navigates here.
    #[serde(default)]
    pub worktree_path: Option<String>,
    /// PR number for the open PR whose head branch matches this branch name, if any.
    #[serde(default)]
    pub pr_number: Option<u64>,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct PrInfo {
    pub number: u64,
    pub title: String,
    pub head_ref: String,
    pub state: String,
    pub is_draft: bool,
    pub author: String,
    #[serde(default)]
    pub assignees: Vec<String>,
    #[serde(default)]
    pub reviewers: Vec<String>,
    /// "PASSING" | "FAILING" | "PENDING" | null
    #[serde(default)]
    pub checks_state: Option<String>,
    /// "APPROVED" | "CHANGES_REQUESTED" | "REVIEW_REQUIRED" | null
    #[serde(default)]
    pub review_decision: Option<String>,
    /// ISO 8601 timestamp — used to sort recently merged PRs.
    #[serde(default)]
    pub merged_at: Option<String>,
    /// True when the current gh user has an APPROVED latest review on this PR.
    #[serde(default)]
    pub approved_by_me: bool,
    /// Base branch (e.g. "main"). Plumbed from `gh pr list` so callers can skip a
    /// second `gh pr view` round-trip when opening the PR.
    #[serde(default)]
    pub base_ref: String,
    /// Head commit SHA — used as the cache freshness key for `pr_open_cache`.
    #[serde(default)]
    pub head_oid: String,
    /// PR `updatedAt` ISO timestamp — part of the freshness key.
    #[serde(default)]
    pub updated_at: String,
    /// Transient: latest review per reviewer (login, state). Not serialized to frontend.
    #[serde(skip)]
    pub latest_reviewer_states: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CheckSummary {
    pub name: String,
    pub status: String,
    pub conclusion: String,
    pub url: Option<String>,
    /// ISO-8601 start and end; `None` until reached.
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub completed_at: Option<String>,
    /// Wall time of a finished check, computed once here so the UI does not
    /// re-parse GitHub's timestamps. `None` while running or untimed.
    #[serde(default)]
    pub duration_secs: Option<u64>,
    /// Status-context text (a deploy preview's "Ready"), shown where a check
    /// has no duration.
    #[serde(default)]
    pub description: Option<String>,
}

impl From<er_engine::github::CheckRun> for CheckSummary {
    fn from(c: er_engine::github::CheckRun) -> Self {
        let duration_secs = c.duration_secs();
        Self {
            name: c.name,
            status: c.status,
            conclusion: c.conclusion,
            url: c.url,
            started_at: c.started_at,
            completed_at: c.completed_at,
            duration_secs,
            description: c.description,
        }
    }
}

/// One person (or team) on the PR's reviewer list: a pending request, a
/// submitted review, or both. Folded by `er_engine::github::pr_reviewers`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GhReviewerSummary {
    pub reviewer: String,
    pub is_team: bool,
    /// A review is requested from them now (first ask or re-ask).
    pub requested: bool,
    /// The review that counts for them (`APPROVED`, `CHANGES_REQUESTED`,
    /// `DISMISSED`, `COMMENTED`); `None` until they review.
    pub state: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GhCommentSummary {
    pub author: String,
    pub body: String,
    pub created_at: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GhReviewSummary {
    pub author: String,
    pub state: String,
    pub body: String,
    pub submitted_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GithubStatusSnapshot {
    pub owner: String,
    pub repo: String,
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
    pub checks: Vec<CheckSummary>,
    pub comments_count: usize,
    pub reviews_count: usize,
    pub recent_comments: Vec<GhCommentSummary>,
    pub recent_reviews: Vec<GhReviewSummary>,
    pub last_updated: Option<String>,
    #[serde(default)]
    pub is_authored_by_me: bool,
    /// GitHub's merge-box verdict (`CLEAN`, `BEHIND`, `BLOCKED`, `DIRTY`, …).
    #[serde(default)]
    pub merge_state_status: Option<String>,
    #[serde(default)]
    pub head_oid: String,
    #[serde(default)]
    pub is_cross_repository: bool,
    /// Method of a pending auto-merge; `None` when auto-merge is off.
    #[serde(default)]
    pub auto_merge_method: Option<String>,
    /// Whether the head branch still exists. Looked up only once a same-repo
    /// PR is merged or closed; `None` while open, for a fork PR, or when
    /// GitHub could not answer.
    #[serde(default)]
    pub head_branch_exists: Option<bool>,
    #[serde(default)]
    pub repo_merge: Option<RepoMergeSnapshot>,
    /// Queued on a merge-queue branch: `gh pr merge` there only enqueues, and
    /// the PR stays open until the queue merges it. Looked up only for bases
    /// that have a queue.
    #[serde(default)]
    pub in_merge_queue: bool,
    /// The base branch has a merge queue, so merging enqueues and the queue,
    /// not this PR, picks the method. Looked up only for open PRs.
    #[serde(default)]
    pub base_has_merge_queue: bool,
    /// The viewer may merge past the base branch's unmet requirements
    /// (`gh pr merge --admin`). Looked up only while the PR is blocked.
    #[serde(default)]
    pub can_bypass_rules: bool,
    /// When the PR was opened (ISO-8601). `gh pr view` has no timeline, so the
    /// "opened" event is built from this instead (ADR 0026).
    #[serde(default)]
    pub created_at: Option<String>,
    /// Commits on the PR.
    #[serde(default)]
    pub commit_count: usize,
    /// Requested reviewers first, then everyone else who reviewed.
    #[serde(default)]
    pub reviewers: Vec<GhReviewerSummary>,
    /// Which fetch produced this entry (`gh_status_cache::store_fetched`).
    /// Process-local, so never sent or persisted.
    #[serde(skip)]
    pub fetch_ticket: u64,
}

/// Repository merge settings the merge box needs. Cached per repo in the
/// engine, so this costs a `gh` call once per TTL, not once per poll.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, schemars::JsonSchema)]
pub struct RepoMergeSnapshot {
    pub merge_commit_allowed: bool,
    pub squash_merge_allowed: bool,
    pub rebase_merge_allowed: bool,
    pub delete_branch_on_merge: bool,
    #[serde(default)]
    pub auto_merge_allowed: bool,
    pub viewer_permission: Option<String>,
}

impl From<er_engine::gh_pr_actions::RepoMergeSettings> for RepoMergeSnapshot {
    fn from(s: er_engine::gh_pr_actions::RepoMergeSettings) -> Self {
        Self {
            merge_commit_allowed: s.merge_commit_allowed,
            squash_merge_allowed: s.squash_merge_allowed,
            rebase_merge_allowed: s.rebase_merge_allowed,
            delete_branch_on_merge: s.delete_branch_on_merge,
            auto_merge_allowed: s.auto_merge_allowed,
            viewer_permission: s.viewer_permission,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct WorktreeSnapshot {
    pub path: String,
    pub branch: String,
    pub is_current: bool,
    pub is_pr: bool,
    pub pr_number: Option<u64>,
    pub is_merged: bool,
    /// The repo (owner/repo slug) this worktree belongs to. Worktrees of one
    /// repository share a `.git/config`, so every row of a given list carries
    /// the same value; it may still differ from the active *project's* remote,
    /// which can point at a different repository entirely.
    #[serde(default)]
    pub remote: Option<String>,
}

pub type GhUser = Arc<Mutex<Option<String>>>;

/// Cache key: (owner, repo, pr_number). Stores the most recent `GithubStatusSnapshot`
/// the background poller fetched.
pub type GhStatusCache = Arc<Mutex<HashMap<(String, String, u64), GithubStatusSnapshot>>>;

#[derive(Clone, Default)]
pub struct ProjectMeta {
    pub current_branch: String,
    pub local_branches: Vec<BranchInfo>,
    pub auto_branches: Vec<BranchInfo>,
}

pub type MetaCache = std::sync::Arc<Mutex<HashMap<String, ProjectMeta>>>;

pub fn minimal_pr_info(number: u64, title: &str) -> PrInfo {
    PrInfo {
        number,
        title: title.to_string(),
        head_ref: String::new(),
        state: String::new(),
        is_draft: false,
        author: String::new(),
        assignees: Vec::new(),
        reviewers: Vec::new(),
        checks_state: None,
        review_decision: None,
        merged_at: None,
        approved_by_me: false,
        base_ref: String::new(),
        head_oid: String::new(),
        updated_at: String::new(),
        latest_reviewer_states: Vec::new(),
    }
}

/// The sidebar shows five and reveals the rest with "Show more"; the PR cache
/// keeps 50 closed PRs, so this bounds the snapshot payload, not the fetch.
pub const RECENTLY_MERGED_LIMIT: usize = 25;

pub fn recently_merged_prs(mut prs: Vec<PrInfo>) -> Vec<PrInfo> {
    prs.retain(|pr| pr.state == "MERGED");
    prs.sort_by_key(|pr| std::cmp::Reverse(pr.merged_at.clone()));
    prs.truncate(RECENTLY_MERGED_LIMIT);
    prs
}
