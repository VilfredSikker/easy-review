use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::projects::{self};

// ── Wire types ──────────────────────────────────────────────────────────────

/// Active-branch watcher status. Set by the desktop background thread that
/// watches the working tree of the currently active local-branch tab.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct WatchStatusSnapshot {
    pub active: bool,
    pub branch: Option<String>,
    pub root_path: Option<String>,
}

pub type WatchStatusState = Arc<Mutex<WatchStatusSnapshot>>;

#[derive(Debug, Clone, Serialize)]
pub struct ReviewFlowSnapshot {
    pub state: String,
    pub tasks: Vec<er_engine::ai::review_flow::FlowTaskProgress>,
    pub depth: Option<String>,
    pub diff_hash: String,
    pub lenses: Vec<String>,
    pub pending_finders: Vec<String>,
    pub failed_finders: Vec<FailedReviewFlowFinderSnapshot>,
    pub arena_run_id: Option<String>,
    /// Providers with successful finder output for this flow.
    pub successful_finder_providers: Vec<String>,
    /// Provider that successfully completed arbitration, if any.
    pub successful_arbiter_provider: Option<String>,
    /// Sorted unique providers that actually completed finder or arbiter work.
    pub providers_used: Vec<String>,
    pub estimated_cost: Option<f64>,
    pub estimated_latency_seconds: Option<f64>,
    pub kept: usize,
    pub dropped: usize,
    pub awaiting_accept: usize,
    pub reason: Option<String>,
    pub draining: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct FailedReviewFlowFinderSnapshot {
    pub lens: String,
    pub reason: String,
}

impl ReviewFlowSnapshot {
    pub(crate) fn from_flow(flow: &er_engine::ai::review_flow::ReviewFlow, draining: bool) -> Self {
        use er_engine::ai::review_flow::{FlowDepth, FlowState};

        let (state, kept, dropped, awaiting_accept, reason) = match &flow.state {
            FlowState::Triaging => ("triaging", 0, 0, 0, None),
            FlowState::Skipped => ("skipped", 0, 0, 0, None),
            FlowState::AwaitingConfirmation => ("awaiting_confirmation", 0, 0, 0, None),
            FlowState::Finding => ("finding", 0, 0, 0, None),
            FlowState::Verifying => ("verifying", 0, 0, 0, None),
            FlowState::Done {
                kept,
                dropped,
                awaiting_accept,
            } => ("done", *kept, *dropped, *awaiting_accept, None),
            FlowState::Failed { reason } => ("failed", 0, 0, 0, Some(reason.clone())),
            FlowState::Cancelled => ("cancelled", 0, 0, 0, None),
            FlowState::Interrupted { unfinished } => (
                "interrupted",
                0,
                0,
                0,
                Some(format!("{unfinished} unfinished stage(s) can be re-run")),
            ),
        };

        Self {
            state: state.to_string(),
            tasks: flow.tasks.clone(),
            depth: flow.depth.map(|depth| {
                match depth {
                    FlowDepth::Quick => "quick",
                    FlowDepth::Standard => "standard",
                    FlowDepth::Deep => "deep",
                }
                .to_string()
            }),
            diff_hash: flow.diff_hash.clone(),
            lenses: flow.lenses.clone(),
            pending_finders: flow.pending_finders.clone(),
            failed_finders: flow
                .failed_finders
                .iter()
                .map(|(lens, reason)| FailedReviewFlowFinderSnapshot {
                    lens: lens.clone(),
                    reason: reason.clone(),
                })
                .collect(),
            arena_run_id: flow.arena_run_id.clone(),
            successful_finder_providers: flow.successful_finder_providers.clone(),
            successful_arbiter_provider: flow.successful_arbiter_provider.clone(),
            providers_used: flow.providers_used.clone(),
            estimated_cost: flow.estimated_cost,
            estimated_latency_seconds: flow.estimated_latency_seconds,
            kept,
            dropped,
            awaiting_accept,
            reason,
            draining,
        }
    }
}

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

/// [`resolve_github_status_key`] over a tab's identity, which the flag-on
/// desktop reads from the tab's published content.
pub fn resolve_github_status_key_for(
    tab: &er_engine::tab_identity::TabIdentity<'_>,
    pr_cache: &HashMap<String, Vec<PrInfo>>,
) -> Option<(String, String, u64)> {
    // 1. Remote PR: slug + number directly.
    if let (Some(slug), Some(number)) = (tab.remote_repo, tab.pr_number) {
        return slug
            .split_once('/')
            .map(|(o, r)| (o.to_string(), r.to_string(), number));
    }

    let branch = tab.local_branch_view.unwrap_or(tab.current_branch);

    // 2. Local PR tab: trust the tab's own pr_number; only resolve the slug.
    //    When the tab knows its own remote (local PR tabs resolve it from the
    //    repo's git remote), restrict the slug search to that repo — otherwise
    //    a PR number that exists in several repos (e.g. #73 in both easy-review
    //    and design-system) matches an arbitrary repo's PR.
    if let Some(number) = tab.pr_number {
        let matches = |slug: &str, prs: &[PrInfo]| {
            let in_repo = tab
                .remote_repo
                .map(|r| r.eq_ignore_ascii_case(slug))
                .unwrap_or(true);
            in_repo && prs.iter().any(|p| p.number == number)
        };
        return pr_cache
            .iter()
            .find(|(slug, prs)| matches(slug, prs))
            .or_else(|| {
                pr_cache
                    .iter()
                    .filter(|(slug, _)| {
                        tab.remote_repo
                            .map(|r| r.eq_ignore_ascii_case(slug))
                            .unwrap_or(true)
                    })
                    .find(|(_, prs)| prs.iter().any(|p| p.head_ref == branch))
            })
            .and_then(|(slug, _)| {
                slug.split_once('/')
                    .map(|(o, r)| (o.to_string(), r.to_string(), number))
            });
    }

    // 3. Plain branch / working tab: match by head_ref, preferring an OPEN PR.
    //    When the tab knows its own remote (branch tabs resolve it from the
    //    repo's git remote), restrict the match to that repo — otherwise a
    //    branch name that exists in several repos matches an arbitrary PR in
    //    the wrong one.
    pr_cache
        .iter()
        .filter(|(slug, _)| {
            tab.remote_repo
                .map(|r| r.eq_ignore_ascii_case(slug))
                .unwrap_or(true)
        })
        .find_map(|(slug, prs)| {
            prs.iter()
                .filter(|p| p.head_ref == branch)
                .min_by_key(|p| if p.state == "OPEN" { 0 } else { 1 })
                .and_then(|p| {
                    slug.split_once('/')
                        .map(|(o, r)| (o.to_string(), r.to_string(), p.number))
                })
        })
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

/// The reviewer list for a PR. The author is left out: their reviews are
/// replies on their own PR, not reviews of it.
pub fn github_reviewers(
    overview: &er_engine::github::PrOverviewFull,
    reviews: &[er_engine::github::PrReview],
) -> Vec<GhReviewerSummary> {
    er_engine::github::pr_reviewers(&overview.author, &overview.review_requests, reviews)
        .into_iter()
        .map(|r| GhReviewerSummary {
            reviewer: r.reviewer,
            is_team: r.is_team,
            requested: r.requested,
            state: r.state,
        })
        .collect()
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

// ── Helpers ──────────────────────────────────────────────────────────────────

// ── Builder ──────────────────────────────────────────────────────────────────

pub type GhUser = Arc<Mutex<Option<String>>>;

/// Cache key: (owner, repo, pr_number). Stores the most recent `GithubStatusSnapshot`
/// the background poller fetched.
pub type GhStatusCache = Arc<Mutex<HashMap<(String, String, u64), GithubStatusSnapshot>>>;

/// Map of `thread_id -> PendingAiReply`. Used to render a synthetic "…thinking"
/// reply on threads where an AI subprocess is currently running.
pub type PendingAiReplies = Arc<Mutex<HashMap<String, PendingAiReply>>>;

/// One AI subprocess answering a thread or finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingAiReply {
    pub started_at_ms: u64,
    /// Which action is running (`AI_ACTION_ASK` / `AI_ACTION_ELABORATE`); the
    /// placeholder carries it so the card can say "Elaborating" before the
    /// rewrite lands, rather than guessing from the thread.
    pub ai_action: Option<&'static str>,
}

#[derive(Clone, Default)]
pub struct ProjectMeta {
    pub current_branch: String,
    pub base_branch: String,
    pub local_branches: Vec<BranchInfo>,
    pub auto_branches: Vec<BranchInfo>,
}

pub type MetaCache = std::sync::Arc<Mutex<HashMap<String, ProjectMeta>>>;

/// Refresh the per-project metadata cache by shelling out to git for each
/// known project. MUST NOT hold `AppState.app` — runs on a background thread.
pub fn refresh_meta_cache(
    git: &dyn er_engine::git::Git,
    active_root: &str,
    cache: &MetaCache,
) -> bool {
    refresh_meta_cache_filtered(git, active_root, cache, None)
}

/// Variant that refreshes a single project (by id), leaving entries for other
///
/// projects untouched. Used at startup so the active project's branches show
/// up immediately without paying for `git branch / worktree list / base
/// detection` on every other registered project first.
pub fn refresh_meta_cache_for_project(
    git: &dyn er_engine::git::Git,
    project_id: &str,
    cache: &MetaCache,
) -> bool {
    refresh_meta_cache_filtered(git, "", cache, Some(project_id))
}

pub fn meta_cache_fingerprint(cache: &MetaCache) -> u64 {
    use std::hash::Hash;
    let Ok(g) = cache.lock() else {
        return 0;
    };
    let mut h = std::collections::hash_map::DefaultHasher::new();
    g.len().hash(&mut h);
    for (id, m) in g.iter() {
        id.hash(&mut h);
        m.current_branch.hash(&mut h);
        m.local_branches.len().hash(&mut h);
        m.auto_branches.len().hash(&mut h);
    }
    crate::profile_log::finish_hash(h)
}

fn refresh_meta_cache_filtered(
    git: &dyn er_engine::git::Git,
    active_root: &str,
    cache: &MetaCache,
    only_project_id: Option<&str>,
) -> bool {
    let t0 = std::time::Instant::now();
    let fp_before = meta_cache_fingerprint(cache);
    let file = projects::load();
    let updates: Vec<(String, ProjectMeta)> = file
        .projects
        .iter()
        .filter(|p| !p.root_path.is_empty())
        .filter(|p| only_project_id.is_none_or(|id| p.id == id))
        .map(|p| {
            let current_branch = detect_current_branch(git, &p.root_path);
            let base_branch = detect_base_branch(git, &p.root_path);
            let raw_worktrees = git.list_worktrees(&p.root_path).unwrap_or_default();
            let local_branches = build_tracked_branches(
                git,
                &p.root_path,
                &base_branch,
                &current_branch,
                &p.tracked_branches,
                &p.dismissed_branches,
                &raw_worktrees,
            );
            let auto_branches = build_auto_branches(
                git,
                &p.root_path,
                &base_branch,
                &current_branch,
                &p.tracked_branches,
                &p.dismissed_branches,
                10,
                &raw_worktrees,
            );
            (
                p.id.clone(),
                ProjectMeta {
                    current_branch,
                    base_branch,
                    local_branches,
                    auto_branches,
                },
            )
        })
        .collect();
    let _ = active_root; // active_root is unused now that ProjectMeta drops worktrees.
    if let Ok(mut g) = cache.lock() {
        if only_project_id.is_some() {
            // Partial update — merge into the existing cache.
            for (id, meta) in updates {
                g.insert(id, meta);
            }
        } else {
            // Full sweep — replace, so deleted projects drop out.
            let mut next: HashMap<String, ProjectMeta> = HashMap::new();
            for (id, meta) in updates {
                next.insert(id, meta);
            }
            *g = next;
        }
    }
    let fp_after = meta_cache_fingerprint(cache);
    let projects_count = file
        .projects
        .iter()
        .filter(|p| !p.root_path.is_empty())
        .filter(|p| only_project_id.is_none_or(|id| p.id == id))
        .count();
    crate::profile_log::profile_log(
        "meta_refresh",
        &[
            ("refresh_ms", t0.elapsed().as_millis().to_string()),
            ("projects", projects_count.to_string()),
            (
                "changed",
                if fp_before != fp_after {
                    "1".to_string()
                } else {
                    "0".to_string()
                },
            ),
            (
                "full_sweep",
                if only_project_id.is_none() {
                    "1".to_string()
                } else {
                    "0".to_string()
                },
            ),
        ],
    );
    fp_before != fp_after
}

#[derive(PartialEq, Eq)]
struct WorktreesMetaKey {
    repo_root: String,
    base_branch: String,
    fingerprint: u64,
}

/// Per-worktree PR metadata `(is_pr, pr_number, is_merged, remote)` keyed by worktree path.
type WorktreeMetaMap = HashMap<String, (bool, Option<u64>, bool, Option<String>)>;

/// Cached per-worktree PR metadata `(is_pr, pr_number, is_merged, remote)` keyed
/// by path.
///
/// `build_worktrees` runs on every snapshot build that carries content. The
/// cheap `list_worktrees` call stays live so add/remove/branch-switch is
/// reflected within one poll, while the per-worktree metadata is cached keyed on
/// a fingerprint of the worktree set: add/remove/switch changes the fingerprint
/// → cache miss → recompute, and a TTL backstops `is_merged` drift when the set
/// is unchanged.
///
/// A miss costs four git processes for the whole list — `list_worktrees`, the
/// remote, the branch merges, and `git branch --merged` — and no network. What
/// the builder must not do is move any of those inside the per-worktree loop:
/// every worktree of a repository shares its config and refs, so per-row
/// lookups ask the same question N times, on the snapshot path, with the App
/// lock held. See `build_worktrees`.
///
/// More than one entry, because reviewing a few PRs at once means alternating
/// between tabs whose `base_branch` differs, and `base_branch` is part of the
/// key: with a single slot, every switch drops the entry the other tab needs and
/// rebuilds it on the way back. Entries are about a kilobyte for a nine-worktree
/// repo, so the cap costs nothing worth measuring.
static WORKTREES_META_CACHE: Mutex<Vec<(WorktreesMetaKey, std::time::Instant, WorktreeMetaMap)>> =
    Mutex::new(Vec::new());

/// Entries kept before the least recently used is dropped.
const WORKTREES_META_CACHE_CAP: usize = 8;

/// How long an entry may go unrefreshed before the next use rebuilds it.
///
/// Generous on purpose, because the fingerprint carries each worktree's tip: a
/// commit, a rebase, or a branch switch invalidates the exact key immediately,
/// with no help from this timer. All the TTL still covers is drift the
/// fingerprint cannot see — the base branch advancing from somewhere other than
/// this machine — and a few minutes of staleness in a merged-branch colouring
/// costs far less than a rebuild on every tab switch.
const WORKTREES_META_TTL: std::time::Duration = std::time::Duration::from_secs(300);

fn worktrees_meta_cached(
    key: WorktreesMetaKey,
    compute: impl FnOnce() -> WorktreeMetaMap,
) -> WorktreeMetaMap {
    if let Ok(mut guard) = WORKTREES_META_CACHE.lock() {
        if let Some(idx) = guard.iter().position(|(cached_key, computed_at, _)| {
            *cached_key == key && computed_at.elapsed() < WORKTREES_META_TTL
        }) {
            // Move to the back so the cap drops the least recently used entry,
            // not merely the oldest.
            let entry = guard.remove(idx);
            let value = entry.2.clone();
            guard.push(entry);
            return value;
        }
    }
    let value = compute();
    if let Ok(mut guard) = WORKTREES_META_CACHE.lock() {
        guard.retain(|(cached_key, _, _)| *cached_key != key);
        guard.push((key, std::time::Instant::now(), value.clone()));
        while guard.len() > WORKTREES_META_CACHE_CAP {
            guard.remove(0);
        }
    }
    value
}

/// The worktree list, plus the cache key its contents fingerprint to.
fn worktrees_list_and_key(
    git: &dyn er_engine::git::Git,
    repo_root: &str,
    base_branch: &str,
) -> (Vec<er_engine::git::Worktree>, WorktreesMetaKey) {
    let wts = git.list_worktrees(repo_root).unwrap_or_default();

    let fingerprint = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for wt in &wts {
            wt.path.hash(&mut h);
            wt.branch.hash(&mut h);
            // The tip, not only the branch name. `is_merged` compares tips, so
            // without this a commit or a rebase moves the answer while path and
            // branch stay put, and nothing in the key changes.
            //
            // Free: the oid arrives in the same `git worktree list --porcelain`
            // output this function already runs. The base branch's own tip is
            // deliberately NOT probed here — that would need a `rev-parse` on
            // every build (this block runs before the cache check, so per build
            // and not per miss) to save a miss that costs less than the probe.
            // Base drift stays on the TTL floor.
            wt.head.hash(&mut h);
        }
        h.finish()
    };
    let key = WorktreesMetaKey {
        repo_root: repo_root.to_string(),
        base_branch: base_branch.to_string(),
        fingerprint,
    };
    (wts, key)
}

/// Per-worktree metadata for a worktree list, from repository-wide lookups.
///
/// Every worktree `git worktree list` reports belongs to this repository and
/// reads the same `.git/config` and the same refs, so the remote and the branch
/// lookups are all per-repo values: each is read once here and shared across the
/// rows, rather than asked once per row. A per-worktree loop over these is N
/// identical subprocesses for one answer, on the snapshot path with the App lock
/// held.
fn compute_worktrees_meta(
    github: &dyn er_engine::github::GitHub,
    git: &dyn er_engine::git::Git,
    wts: &[er_engine::git::Worktree],
    repo_root: &str,
    base_branch: &str,
) -> WorktreeMetaMap {
    let repo_remote = crate::projects::resolve_repo_remote(github, git, repo_root);
    let merges = pr_branch_merges(git, repo_root);
    let merged = merged_branches(git, repo_root, base_branch, wts.len());
    wts.iter()
        .map(|wt| {
            let (is_pr, pr_number, is_merged) =
                detect_pr_meta(&merges, &merged, &wt.branch, base_branch);
            (
                wt.path.clone(),
                (is_pr, pr_number, is_merged, repo_remote.clone()),
            )
        })
        .collect()
}

pub(crate) fn build_worktrees(
    github: &dyn er_engine::github::GitHub,
    git: &dyn er_engine::git::Git,
    repo_root: &str,
    base_branch: &str,
    current_root: &str,
) -> Vec<WorktreeSnapshot> {
    let (wts, key) = worktrees_list_and_key(git, repo_root, base_branch);

    let meta = worktrees_meta_cached(key, || {
        compute_worktrees_meta(github, git, &wts, repo_root, base_branch)
    });

    wts.into_iter()
        .map(|wt| {
            let (is_pr, pr_number, is_merged, remote) = meta
                .get(&wt.path)
                .cloned()
                .unwrap_or((false, None, false, None));
            WorktreeSnapshot {
                is_current: wt.path == current_root,
                branch: wt.branch,
                path: wt.path,
                is_pr,
                pr_number,
                is_merged,
                remote,
            }
        })
        .collect()
}

/// Local branches merged into `base_branch`, from one `git branch --merged`.
///
/// Replaces a `merge-base --is-ancestor` per branch, which spawned a git
/// process for every local branch on every meta refresh — 100+ on a repo with
/// 100 branches — all asking the same question of the same base. Empty when the
/// answer cannot be established, which callers read as "nothing is merged",
/// matching the per-branch form's `.unwrap_or(false)`.
fn merged_into_base(
    git: &dyn er_engine::git::Git,
    repo_root: &str,
    base_branch: &str,
) -> std::collections::HashSet<String> {
    if base_branch.is_empty() {
        return std::collections::HashSet::new();
    }
    // `--format` drops the `* `/`+ ` prefixes `git branch` adds for the current
    // and worktree branches, so the output is bare names.
    git.branch_merged(repo_root, base_branch)
        .map(|stdout| {
            stdout
                .lines()
                .map(|line| line.trim().to_string())
                .filter(|line| !line.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// How many worktrees a repo may have before the merged-branch check is skipped.
///
/// `git branch --merged` lists every local branch, so on a repo with many
/// worktrees the check costs more than the colouring it drives is worth. Above
/// this, callers see "nothing merged" and render every branch uncoloured.
const MAX_WORKTREES_FOR_MERGED_CHECK: usize = 10;

/// Branches merged into `base_branch`, for the callers that colour a branch
/// list.
///
/// Empty when the check is skipped (too many worktrees) or the answer cannot be
/// established (no base branch, git failed) — callers treat all three alike, so
/// they only need `contains`.
fn merged_branches(
    git: &dyn er_engine::git::Git,
    repo_root: &str,
    base_branch: &str,
    worktree_count: usize,
) -> std::collections::HashSet<String> {
    if worktree_count > MAX_WORKTREES_FOR_MERGED_CHECK {
        return std::collections::HashSet::new();
    }
    merged_into_base(git, repo_root, base_branch)
}

fn build_tracked_branches(
    git: &dyn er_engine::git::Git,
    repo_root: &str,
    base_branch: &str,
    current_branch: &str,
    tracked: &[String],
    dismissed: &[String],
    worktrees: &[er_engine::git::Worktree],
) -> Vec<BranchInfo> {
    let Ok(text) = git.for_each_ref(
        repo_root,
        "%(refname:short)|%(upstream:short)",
        None,
        "refs/heads/",
    ) else {
        return Vec::new();
    };

    let merged = merged_branches(git, repo_root, base_branch, worktrees.len());

    // Build the full list first, then filter to the curated set (tracked ∪ {current}).
    let all: Vec<BranchInfo> = text
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(2, '|');
            let name = parts.next()?.trim().to_string();
            if name.is_empty() {
                return None;
            }
            let upstream_raw = parts.next().unwrap_or("").trim().to_string();
            let upstream = if upstream_raw.is_empty() {
                None
            } else {
                Some(upstream_raw)
            };
            let is_current = name == current_branch;
            let is_merged = name != base_branch && merged.contains(&name);
            let worktree_path = worktrees
                .iter()
                .find(|w| w.branch == name)
                .map(|w| w.path.clone());
            Some(BranchInfo {
                name,
                upstream,
                is_current,
                is_merged,
                worktree_path,
                pr_number: None,
            })
        })
        .collect();

    let worktree_branches: std::collections::HashSet<&str> =
        worktrees.iter().map(|w| w.branch.as_str()).collect();
    let dismissed_set: std::collections::HashSet<&str> =
        dismissed.iter().map(|s| s.as_str()).collect();

    // Visibility set: {current} ∪ tracked ∪ {branches with an active worktree},
    // minus dismissed (current branch is always shown).
    let mut visible: Vec<BranchInfo> = Vec::new();
    let mut seen = std::collections::HashSet::<String>::new();

    if let Some(cur) = all.iter().find(|b| b.is_current) {
        visible.push(cur.clone());
        seen.insert(cur.name.clone());
    }
    for b in &all {
        if seen.contains(&b.name) {
            continue;
        }
        if dismissed_set.contains(b.name.as_str()) {
            continue;
        }
        let in_tracked = tracked.iter().any(|t| t == &b.name);
        let has_worktree = worktree_branches.contains(b.name.as_str());
        if in_tracked || has_worktree {
            visible.push(b.clone());
            seen.insert(b.name.clone());
        }
    }
    visible
}

#[expect(
    clippy::too_many_arguments,
    reason = "one argument per branch-list input"
)]
fn build_auto_branches(
    git: &dyn er_engine::git::Git,
    repo_root: &str,
    base_branch: &str,
    current_branch: &str,
    tracked: &[String],
    dismissed: &[String],
    limit: usize,
    worktrees: &[er_engine::git::Worktree],
) -> Vec<BranchInfo> {
    let Ok(text) = git.for_each_ref(
        repo_root,
        "%(refname:short)|%(upstream:short)",
        Some("-committerdate"),
        "refs/heads/",
    ) else {
        return Vec::new();
    };

    let merged = merged_branches(git, repo_root, base_branch, worktrees.len());

    let tracked_set: std::collections::HashSet<&str> = tracked.iter().map(|s| s.as_str()).collect();
    let dismissed_set: std::collections::HashSet<&str> =
        dismissed.iter().map(|s| s.as_str()).collect();

    let mut result: Vec<BranchInfo> = Vec::new();
    for line in text.lines() {
        if result.len() >= limit {
            break;
        }
        let mut parts = line.splitn(2, '|');
        let Some(name) = parts.next().map(|s| s.trim().to_string()) else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        if name == current_branch {
            continue;
        }
        if dismissed_set.contains(name.as_str()) {
            continue;
        }
        if tracked_set.contains(name.as_str()) {
            continue;
        }
        let upstream_raw = parts.next().unwrap_or("").trim().to_string();
        let upstream = if upstream_raw.is_empty() {
            None
        } else {
            Some(upstream_raw)
        };
        let is_merged = name != base_branch && merged.contains(&name);
        let worktree_path = worktrees
            .iter()
            .find(|w| w.branch == name)
            .map(|w| w.path.clone());
        result.push(BranchInfo {
            name,
            upstream,
            is_current: false,
            is_merged,
            worktree_path,
            pr_number: None,
        });
    }
    result
}

pub(crate) fn minimal_pr_info(number: u64, title: &str) -> PrInfo {
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
const RECENTLY_MERGED_LIMIT: usize = 25;

pub(crate) fn recently_merged_prs(mut prs: Vec<PrInfo>) -> Vec<PrInfo> {
    prs.retain(|pr| pr.state == "MERGED");
    prs.sort_by_key(|pr| std::cmp::Reverse(pr.merged_at.clone()));
    prs.truncate(RECENTLY_MERGED_LIMIT);
    prs
}

fn detect_current_branch(git: &dyn er_engine::git::Git, repo_root: &str) -> String {
    git.current_branch(repo_root).unwrap_or_default()
}

fn detect_base_branch(git: &dyn er_engine::git::Git, repo_root: &str) -> String {
    // Cheap fallback: try common defaults
    for candidate in ["main", "master", "develop", "dev"] {
        let spec = format!("refs/heads/{candidate}");
        if git
            .rev_parse_verify(repo_root, &spec)
            .is_ok_and(|oid| oid.is_some())
        {
            return candidate.to_string();
        }
    }
    String::new()
}

/// Every `branch.<name>.merge` value in the repository, in one git call.
///
/// Branch config is repository-wide, so this answers for every worktree at once
/// and belongs outside any per-worktree loop.
fn pr_branch_merges(git: &dyn er_engine::git::Git, repo_root: &str) -> HashMap<String, String> {
    let Ok(stdout) = git.config_get_regexp(repo_root, r"^branch\..*\.merge$") else {
        return HashMap::new();
    };
    stdout
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split_once(' ')?;
            let name = key.strip_prefix("branch.")?.strip_suffix(".merge")?;
            Some((name.to_string(), value.trim().to_string()))
        })
        .collect()
}

/// Per-worktree PR metadata, from repository-wide lookups.
///
/// `merges` and `merged` are read once for the repository rather than per
/// worktree: refs and branch config both live in the shared `.git`, so every
/// worktree would get the same answer anyway. The per-worktree form spawned two
/// git processes per row — eighteen on a nine-worktree repo — on the snapshot
/// path, with the App lock held.
fn detect_pr_meta(
    merges: &HashMap<String, String>,
    merged: &std::collections::HashSet<String>,
    branch: &str,
    base: &str,
) -> (bool, Option<u64>, bool) {
    let pr_number = merges
        .get(branch)
        .and_then(|val| val.strip_prefix("refs/pull/"))
        .and_then(|rest| rest.strip_suffix("/head"))
        .and_then(|n| n.parse::<u64>().ok());

    // `merged` is already empty when the check is skipped or the base is
    // unknown — see `merged_branches` — so only the self-compare needs a guard.
    let is_merged = !base.is_empty() && branch != base && merged.contains(branch);

    (pr_number.is_some(), pr_number, is_merged)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_flow_snapshot_keeps_captured_task_identity_and_wire_status() {
        use er_engine::ai::review_flow::{
            FlowState, FlowTaskProgress, FlowTaskStage, FlowTaskStatus, ReviewFlow,
        };

        let mut flow = ReviewFlow::new("captured-hash");
        flow.state = FlowState::Failed {
            reason: "provider failed".into(),
        };
        flow.tasks.push(FlowTaskProgress {
            id: "finder:security".into(),
            stage: FlowTaskStage::Finding,
            lens: Some("security".into()),
            label: "Security review".into(),
            purpose: "Check permissions and sensitive data".into(),
            provider_id: "captured-provider".into(),
            provider_label: "Captured provider".into(),
            model_id: "captured-model".into(),
            model_label: "Captured model".into(),
            status: FlowTaskStatus::Failed,
            error: Some("provider failed".into()),
            background_task_id: Some("exact-background-task".into()),
            round: None,
            total_rounds: None,
        });
        let snapshot = ReviewFlowSnapshot::from_flow(&flow, false);
        let wire = serde_json::to_value(snapshot).unwrap();
        assert_eq!(wire["tasks"][0]["provider_id"], "captured-provider");
        assert_eq!(wire["tasks"][0]["model_id"], "captured-model");
        assert_eq!(wire["tasks"][0]["stage"], "finding");
        assert_eq!(wire["tasks"][0]["status"], "failed");
        assert_eq!(wire["tasks"][0]["error"], "provider failed");
        assert_eq!(
            wire["tasks"][0]["background_task_id"],
            "exact-background-task"
        );
    }

    fn pr_with(number: u64, head_ref: &str, state: &str) -> PrInfo {
        let mut p = minimal_pr_info(number, "t");
        p.head_ref = head_ref.to_string();
        p.state = state.to_string();
        p
    }

    #[test]
    fn recently_merged_keeps_more_than_the_first_page_newest_first() {
        let mut prs: Vec<PrInfo> = (1..=30)
            .map(|n| {
                let mut p = pr_with(n, "b", "MERGED");
                p.merged_at = Some(format!("2026-09-{n:02}T00:00:00Z"));
                p
            })
            .collect();
        prs.push(pr_with(99, "open", "OPEN"));
        prs.push(pr_with(98, "closed", "CLOSED"));

        let merged = recently_merged_prs(prs);
        let numbers: Vec<u64> = merged.iter().map(|p| p.number).collect();
        assert_eq!(numbers.len(), RECENTLY_MERGED_LIMIT);
        assert!(numbers.len() > 5, "Show more needs more than one page");
        assert_eq!(numbers[0], 30);
        assert!(numbers.windows(2).all(|w| w[0] > w[1]));
    }

    fn run_git(dir: &std::path::Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap_or_else(|e| panic!("failed to run git {args:?}: {e}"));
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    // ── stale-pill gate for PR tabs ──

    // ── pr_cache_fingerprint folds in head_oid ──

    // The differential snapshot is the flag-off path: with the flag on the
    // page fetches hunks by hash and nothing records what it holds.

    // The differential snapshot is the flag-off path: with the flag on the
    // page fetches hunks by hash and nothing records what it holds.

    // The differential snapshot is the flag-off path: with the flag on the
    // page fetches hunks by hash and nothing records what it holds.

    // The differential snapshot is the flag-off path: with the flag on the
    // page fetches hunks by hash and nothing records what it holds.

    #[test]
    fn worktrees_meta_cache_recomputes_only_on_key_change() {
        use std::cell::Cell;

        let calls = Cell::new(0u32);
        let compute = || {
            calls.set(calls.get() + 1);
            let mut m = HashMap::new();
            m.insert("/wt".to_string(), (true, Some(7u64), false, None));
            m
        };
        let key = || WorktreesMetaKey {
            repo_root: "/repo".to_string(),
            base_branch: "main".to_string(),
            // Unique fingerprint so this test never collides with cached state
            // left by other suites touching the shared static.
            fingerprint: 0xDEAD_BEEF,
        };

        // First call: cache miss → compute runs once, value flows through.
        let v1 = worktrees_meta_cached(key(), compute);
        assert_eq!(calls.get(), 1);
        assert_eq!(v1.get("/wt").cloned(), Some((true, Some(7), false, None)));

        // Same key within TTL: cache hit → no recompute, same value.
        let v2 = worktrees_meta_cached(key(), compute);
        assert_eq!(
            calls.get(),
            1,
            "same worktree set within TTL must reuse cached meta, not respawn git subprocesses"
        );
        assert_eq!(v2.get("/wt").cloned(), Some((true, Some(7), false, None)));

        // Changed fingerprint (worktree added/removed/switched): miss → recompute.
        let changed = WorktreesMetaKey {
            repo_root: "/repo".to_string(),
            base_branch: "main".to_string(),
            fingerprint: 0xFEED_FACE,
        };
        let _ = worktrees_meta_cached(changed, compute);
        assert_eq!(
            calls.get(),
            2,
            "a changed worktree set must invalidate the cache and recompute"
        );
    }

    #[test]
    fn worktrees_meta_cache_holds_entries_for_different_base_branches() {
        use std::cell::Cell;

        // The workflow this cap exists for: a few PRs open at once, alternating
        // between tabs whose base branch differs. `base_branch` is part of the
        // key, so a single-slot cache evicted the other tab on every switch and
        // rebuilt it on the way back.
        let calls = Cell::new(0u32);
        let key_for = |base: &str| WorktreesMetaKey {
            repo_root: "/two-base-repo".to_string(),
            base_branch: base.to_string(),
            // Distinct fingerprints so this test never collides with cached
            // state other suites left in the shared static.
            fingerprint: 0x0BAD_F00D,
        };

        let _ = worktrees_meta_cached(key_for("main"), || {
            calls.set(calls.get() + 1);
            HashMap::from([("/wt".to_string(), (false, None, false, None))])
        });
        let _ = worktrees_meta_cached(key_for("release/v1"), || {
            calls.set(calls.get() + 1);
            HashMap::from([("/wt".to_string(), (false, None, false, None))])
        });
        assert_eq!(calls.get(), 2);

        // Both keys must still hit: neither evicted the other.
        let _ = worktrees_meta_cached(key_for("main"), || {
            calls.set(calls.get() + 1);
            HashMap::new()
        });
        let _ = worktrees_meta_cached(key_for("release/v1"), || {
            calls.set(calls.get() + 1);
            HashMap::new()
        });
        assert_eq!(
            calls.get(),
            2,
            "switching between two base branches must not rebuild either entry"
        );
    }

    /// A repo with one branch merged back into `main` and one left open —
    /// the two cases `merged_into_base` has to tell apart.
    fn init_repo_with_a_merged_and_an_open_branch() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        run_git(root, &["init", "-b", "main"]);
        run_git(root, &["config", "user.email", "t@example.com"]);
        run_git(root, &["config", "user.name", "t"]);
        run_git(root, &["config", "commit.gpgsign", "false"]);
        std::fs::write(root.join("f.txt"), "one\n").unwrap();
        run_git(root, &["add", "f.txt"]);
        run_git(root, &["commit", "-qm", "base"]);

        // Merged back into main.
        run_git(root, &["checkout", "-q", "-b", "merged-branch"]);
        run_git(
            root,
            &["commit", "-q", "--allow-empty", "-m", "merged work"],
        );
        run_git(root, &["checkout", "-q", "main"]);
        run_git(
            root,
            &["merge", "-q", "--no-ff", "-m", "merge", "merged-branch"],
        );

        // Branched from main and left alone.
        run_git(root, &["checkout", "-q", "-b", "open-branch"]);
        run_git(root, &["commit", "-q", "--allow-empty", "-m", "open work"]);
        run_git(root, &["checkout", "-q", "main"]);
        tmp
    }

    #[test]
    fn merged_into_base_lists_only_merged_branches() {
        let dir = init_repo_with_a_merged_and_an_open_branch();
        let root = dir.path().to_string_lossy().to_string();

        let merged = merged_into_base(&*er_engine::ports::Ports::cli().git, &root, "main");
        assert!(merged.contains("merged-branch"), "got {merged:?}");
        assert!(merged.contains("main"), "the base is merged into itself");
        assert!(
            !merged.contains("open-branch"),
            "an unmerged branch must not be listed: {merged:?}"
        );

        // An unresolvable base yields nothing rather than a wrong answer —
        // the same outcome the per-branch form's `.unwrap_or(false)` gave.
        let git = &*er_engine::ports::Ports::cli().git;
        assert!(merged_into_base(git, &root, "no-such-base").is_empty());
        assert!(merged_into_base(git, &root, "").is_empty());
    }

    #[test]
    fn merged_branches_skips_the_check_above_the_worktree_threshold() {
        // Above the threshold every branch renders uncoloured, which is the
        // same outcome as "nothing is merged" — the guard belongs where it is
        // now, not duplicated at both call sites.
        let dir = init_repo_with_a_merged_and_an_open_branch();
        let root = dir.path().to_string_lossy().to_string();

        let git = &*er_engine::ports::Ports::cli().git;
        let at_limit = merged_branches(git, &root, "main", MAX_WORKTREES_FOR_MERGED_CHECK);
        assert!(
            at_limit.contains("merged-branch"),
            "at the threshold the check still runs: {at_limit:?}"
        );

        let over_limit = merged_branches(git, &root, "main", MAX_WORKTREES_FOR_MERGED_CHECK + 1);
        assert!(
            over_limit.is_empty(),
            "past the threshold the check is skipped: {over_limit:?}"
        );
        assert!(
            merged_branches(git, &root, "", 0).is_empty(),
            "an empty base has nothing to compare against"
        );
    }

    #[test]
    fn check_summary_carries_timing_and_duration() {
        let run = er_engine::github::CheckRun {
            name: "ci".into(),
            status: "COMPLETED".into(),
            conclusion: "SUCCESS".into(),
            url: None,
            started_at: Some("2026-10-30T10:00:00Z".into()),
            completed_at: Some("2026-10-30T10:01:52Z".into()),
            description: None,
        };
        let summary = CheckSummary::from(run);
        assert_eq!(summary.duration_secs, Some(112));
        assert_eq!(summary.started_at.as_deref(), Some("2026-10-30T10:00:00Z"));
    }

    #[test]
    fn detect_current_branch_reads_head_through_the_handle() {
        let fake = std::sync::Arc::new(
            er_git::FakeGit::default().with_current_branch(Ok("feature".into())),
        );
        let ports = er_engine::ports::Ports::with_git(fake.clone());

        let branch = detect_current_branch(&*ports.git, "/nonexistent-repo");

        assert_eq!(branch, "feature");
        assert!(
            fake.calls()
                .iter()
                .any(|call| call.contains("current_branch")),
            "the helper must read HEAD through the handle: {:?}",
            fake.calls()
        );
    }
}
