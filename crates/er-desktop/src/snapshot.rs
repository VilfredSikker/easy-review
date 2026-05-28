use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use er_engine::ai::{CommentRef, RiskLevel};
use er_engine::app::{AgentLogSource, App, CommandStatus, DiffMode, InputMode, TabState};
use er_engine::arena::{ArenaRunSnapshot, ArenaRunSummary};
use er_engine::git::{DiffFile, FileStatus, LineType};
use serde::Serialize;

use crate::inbox::InboxHandle;
use crate::projects;

// ── Wire types ──────────────────────────────────────────────────────────────

/// Full arena run + projections (`arena_get` / `arena_override`).
pub type ArenaRunSnapshotWire = ArenaRunSnapshot;

/// Arena run list entry for the review tab (`arena_list` / poll snapshot).
pub type ArenaRunSummaryWire = ArenaRunSummary;

/// Which background fetches are currently in-flight. Included in every snapshot
/// so the frontend can show loading indicators without adding its own timers.
#[derive(Debug, Clone, Default, Serialize)]
pub struct LoadingFlags {
    /// `gh pr list` refresh running across all project remotes.
    pub pr_list: bool,
    /// GitHub status fetch (checks, reviews) for the active tab.
    pub gh_status: bool,
    /// Inline GitHub comment sync for the active tab.
    pub gh_comments: bool,
    /// Background remote-PR diff refresh for the active tab.
    #[serde(default)]
    pub remote_pr_diff: bool,
}

pub type LoadingState = Arc<Mutex<LoadingFlags>>;
pub type PrCacheFetchedAt = Arc<Mutex<HashMap<String, u64>>>;

/// Active-branch watcher status. Set by the desktop background thread that
/// watches the working tree of the currently active local-branch tab.
#[derive(Debug, Clone, Default, Serialize)]
pub struct WatchStatusSnapshot {
    pub active: bool,
    pub branch: Option<String>,
    pub root_path: Option<String>,
}

pub type WatchStatusState = Arc<Mutex<WatchStatusSnapshot>>;

/// Safety valve for continuous-diff snapshots. The Svelte view virtualizes DOM
/// rows, but the wire snapshot can still become large if every non-compacted
/// file serializes every highlighted line on every poll.
const SNAPSHOT_DIFF_LINE_BUDGET: usize = 15_000;

/// Snapshot of the current diff source and what's available.
#[derive(Debug, Clone, Serialize)]
pub struct DiffSourceSnapshot {
    /// "pr" | "origin" | "local"
    pub active: String,
    /// Subset of ["pr", "origin", "local"] — only sources valid for this tab.
    pub available: Vec<String>,
    pub branch: String,
    pub upstream: Option<String>,
    pub base: String,
    pub pr_number: Option<u64>,
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
    /// Short status phrase for UI display.
    pub status: String,
    /// Suggestion to the user about what to do.
    pub suggestion: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppSnapshot {
    pub mode: String,
    pub branch: String,
    pub base: String,
    pub input_mode: String,
    pub files: Vec<FileSnapshot>,
    pub selected_file: usize,
    pub current_hunk: Option<usize>,
    pub filter: Option<String>,
    pub reviewed_count: usize,
    pub total_count: usize,
    pub ai: AiSnapshot,
    pub pr: Option<PrSnapshot>,
    pub panels: Panels,
    pub theme: String,
    pub watch_active: bool,
    pub watch_status: WatchStatusSnapshot,
    pub worktrees: Vec<WorktreeSnapshot>,
    pub projects: Vec<ProjectSnapshot>,
    pub notification: Option<String>,
    /// When Some, the active tab is a read-only diff of this local branch.
    pub local_branch: Option<String>,
    /// True when the viewed local branch is checked out (project root or worktree).
    #[serde(default)]
    pub local_branch_checked_out: bool,
    pub tabs: Vec<TabSummary>,
    pub active_tab: usize,
    /// Browser-view annotations for the active tab. Freshly read from disk
    /// each snapshot — keeps the source of truth in `ui-annotations.json`.
    #[serde(default)]
    pub ui_annotations: Vec<UiAnnotationSnapshot>,
    /// Per-tab browser pane state for the active tab.
    #[serde(default)]
    pub browser: BrowserSnapshot,
    /// Live GitHub status for the active tab when it's a remote PR with cached data.
    pub github: Option<GithubStatusSnapshot>,
    /// Diff source state for the active tab. None for working-tree tabs.
    #[serde(default)]
    pub diff_source: Option<DiffSourceSnapshot>,
    /// Which background fetches are currently in-flight.
    pub bg_loading: LoadingFlags,
    /// Running/done/failed background AI commands for the active tab.
    #[serde(default)]
    pub agent_commands: Vec<AgentCommandSnapshot>,
    /// Recent agent log output for the active tab (last 200 entries).
    #[serde(default)]
    pub agent_log: Vec<AgentLogSnapshot>,
    /// Human-readable label for the currently selected AI provider/model.
    #[serde(default)]
    pub active_ai_label: String,
    /// Filter presets + recent filter history for the active tab. Presets
    /// come first to mirror the TUI's filter overlay ordering.
    #[serde(default)]
    pub filter_suggestions: Vec<FilterSuggestionSnapshot>,
    /// Last 10 commits on the active tab's branch (vs base). Powers the file
    /// viewer's commit history scroller. Empty for remote-only tabs.
    #[serde(default)]
    pub commits: Vec<CommitSummary>,
    /// SHA of the currently-selected commit when in History mode.
    /// None when viewing a non-history scope ("All changes", unstaged, staged).
    #[serde(default)]
    pub selected_commit_sha: Option<String>,
    /// Session-scoped background review tasks across all tabs. Includes
    /// Running tasks and Done/Failed tasks within the last 8 seconds so
    /// the frontend can render transient toasts.
    #[serde(default)]
    pub background_tasks: Vec<BackgroundTaskSnapshotWire>,
    #[serde(default)]
    pub inbox_items: Vec<InboxItemSnapshot>,
    #[serde(default)]
    pub inbox_unread_count: usize,
    #[serde(default)]
    pub inbox_last_refresh_ms: u64,
    /// `features.arena` — when false, arena commands are rejected.
    #[serde(default)]
    pub arena_enabled: bool,
    /// Active arena run id for this tab, if any.
    #[serde(default)]
    pub active_arena_run: Option<String>,
    /// Recent arena runs for the active tab (newest first).
    #[serde(default)]
    pub arena_runs: Vec<ArenaRunSummaryWire>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InboxItemSnapshot {
    pub id: String,
    pub kind: String,
    pub severity: String,
    pub title: String,
    pub body: String,
    pub source: String,
    pub target: serde_json::Value,
    pub created_at_ms: u64,
    pub read_at_ms: Option<u64>,
    pub dedupe_key: String,
}

/// Wire representation of an app-level background task.
#[derive(Debug, Clone, Serialize)]
pub struct BackgroundTaskSnapshotWire {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub target_label: String,
    pub scope: String,
    /// "running" | "done" | "failed"
    pub status: String,
    pub error: Option<String>,
    pub started_at_ms: u128,
    pub finished_at_ms: Option<u128>,
    /// Last 40 log entries from the task's ring buffer.
    #[serde(default)]
    pub recent_log: Vec<AgentLogSnapshot>,
    /// Path to the task's debug log file, if available.
    #[serde(default)]
    pub debug_log_path: Option<String>,
}

/// Status of a background AI command.
#[derive(Debug, Clone, Serialize)]
pub struct AgentCommandSnapshot {
    pub name: String,
    /// "running" | "done" | "failed"
    pub status: String,
    /// Error message when status == "failed"
    pub error: Option<String>,
}

/// A single line of agent output.
#[derive(Debug, Clone, Serialize)]
pub struct AgentLogSnapshot {
    pub command_name: String,
    /// "stdout" | "stderr" | "status"
    pub source: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct BrowserSnapshot {
    pub url: String,
    pub layout: String,
    pub split_ratio: f32,
    pub annotate_mode: bool,
    pub show_tooltips: bool,
}

fn browser_snapshot_from_tab(tab: &TabState) -> BrowserSnapshot {
    BrowserSnapshot {
        url: tab.browser_url.clone(),
        layout: tab.browser_layout.as_str().to_string(),
        split_ratio: tab.browser_split_ratio.clamp(0.35, 0.65),
        annotate_mode: tab.browser_annotate_mode,
        show_tooltips: tab.browser_show_tooltips,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct UiAnnotationSnapshot {
    pub id: String,
    pub url: String,
    pub selector: Option<String>,
    pub box_x: f64,
    pub box_y: f64,
    pub box_w: f64,
    pub box_h: f64,
    pub viewport_w: u32,
    pub viewport_h: u32,
    pub text: String,
    pub timestamp: String,
    pub author: String,
    pub screenshot_path: Option<String>,
    pub stale: bool,
    pub element_context: Option<String>,
    pub dom_context: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TabSummary {
    pub idx: usize,
    pub label: String,
    pub kind: String, // "working" | "local_branch" | "remote_pr"
    pub branch: Option<String>,
    pub pr_number: Option<u64>,
    pub repo_root: String,
    pub is_active: bool,
    pub change_token: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectSnapshot {
    pub id: String,
    pub name: String,
    pub root_path: String,
    pub remote: Option<String>,
    #[serde(default)]
    pub remote_only: bool,
    pub is_active: bool,
    pub local_branches: Vec<BranchInfo>,
    pub auto_branches: Vec<BranchInfo>,
    /// Manually bookmarked PRs (sorted by saved_at desc).
    #[serde(default)]
    pub saved_prs: Vec<PrInfo>,
    /// Open PRs authored by the current user.
    pub my_prs: Vec<PrInfo>,
    /// Open PRs from others that the current user hasn't approved yet (max 5).
    pub prs_to_review: Vec<PrInfo>,
    /// PRs opened for review recently (sorted by viewed_at desc).
    #[serde(default)]
    pub recent_prs: Vec<PrInfo>,
    /// Most recently merged PRs (max 5, sorted by merged_at desc).
    pub recently_merged: Vec<PrInfo>,
    #[serde(default)]
    pub pr_cache_stale: bool,
    #[serde(default)]
    pub pr_cache_age_ms: Option<u64>,
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

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
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

#[derive(Debug, Clone, Serialize)]
pub struct Panels {
    pub left: bool,
    pub tree: bool,
    pub right: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileSnapshot {
    pub path: String,
    pub status: String,
    pub additions: usize,
    pub deletions: usize,
    pub reviewed: bool,
    pub compacted: bool,
    pub risk: Option<String>,
    pub finding_count: usize,
    pub comment_count: usize,
    pub question_count: usize,
    pub hunks: Vec<HunkSnapshot>,
    /// True when the file exists in the diff but its hunks haven't been parsed
    /// yet (lazy mode, large diff). The UI should show a loading state rather
    /// than "No changes."
    pub is_lazy_stub: bool,
    /// Index into the backend's full `tab.files` list. Use this when calling
    /// `select_file` — `files` may be filtered, so positional indices into the
    /// frontend snapshot do not match the engine's selection index.
    pub source_index: usize,
    /// Stable content hash for the desktop highlight cache. Advances when the
    /// diff changes. Frontend uses this to detect stale highlight responses.
    pub cache_key: String,
}

/// Lightweight commit metadata for the file viewer's history scroller.
/// Includes "All changes" + last N commits.
#[derive(Debug, Clone, Serialize)]
pub struct CommitSummary {
    pub sha: String,
    pub title: String,
    pub author: String,
    pub age: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FilterSuggestionSnapshot {
    /// "preset" | "history"
    pub kind: String,
    pub name: String,
    pub expr: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct HunkSnapshot {
    pub header: String,
    pub old_start: usize,
    pub old_count: usize,
    pub new_start: usize,
    pub new_count: usize,
    pub lines: Vec<LineSnapshot>,
    pub threads: Vec<ThreadSnapshot>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LineSnapshot {
    pub old_num: Option<usize>,
    pub new_num: Option<usize>,
    pub kind: String,
    /// Always-present plain text for the line (no syntax coloring).
    /// Used directly when spans are absent; also feeds word-diff.
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ThreadSnapshot {
    pub id: String,
    pub kind: String, // "comment" | "question"
    pub file: String,
    pub line: usize,
    /// Inclusive end line when the thread spans multiple diff lines (`None` = single line).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_end: Option<usize>,
    /// Review side for range matching: "LEFT" | "RIGHT".
    #[serde(default = "default_thread_side")]
    pub side: String,
    pub source: String, // "local" | "github"
    pub synced: bool,
    pub stale: bool,
    pub resolved: bool,
    pub root: ThreadMessage,
    pub replies: Vec<ThreadMessage>,
    /// For questions: the GitHub comment id this was promoted to (if any).
    pub promoted_to: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ThreadMessage {
    pub id: String,
    pub author: String,
    pub kind: String, // "you" | "human" | "ai"
    pub timestamp: String,
    pub body_markdown: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FlatFinding {
    pub id: String,
    pub file: String,
    pub line: Option<usize>,
    pub hunk_index: Option<usize>,
    pub severity: String, // "high" | "med" | "low"
    /// Set when finding comes from a specialized expert (`category` = expert id).
    pub expert_label: Option<String>,
    /// Agent that produced this finding (pill label): General, Security, Professor, …
    pub agent_label: String,
    pub title: String,
    pub message_markdown: String,
    /// GitHub comment id this finding was promoted to (if any).
    pub promoted_to: Option<String>,
    /// ID of the root GitHub comment thread created via "Ask AI" for this finding.
    pub thread_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AiSnapshot {
    pub fresh: bool,
    pub stale_reason: Option<String>,
    pub summary_markdown: Option<String>,
    /// Per-agent markdown summaries (Security, Testing, Professor, …) from expert/professor sidecars.
    pub agent_summaries: std::collections::HashMap<String, String>,
    pub high: usize,
    pub med: usize,
    pub low: usize,
    pub local_comment_count: usize,
    pub github_comment_count: usize,
    pub comments: usize,
    pub questions: usize,
    pub unpushed: usize,
    pub threads: Vec<ThreadSnapshot>,
    pub findings: Vec<FlatFinding>,
    /// Whether `{er_dir}/review.json` exists (batch validate target).
    pub has_review_json: bool,
    /// Top-level GitHub comments eligible for batch validate (!resolved, !outdated).
    pub eligible_comment_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckSummary {
    pub name: String,
    pub status: String,
    pub conclusion: String,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GhCommentSummary {
    pub author: String,
    pub body: String,
    pub created_at: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct GhReviewSummary {
    pub author: String,
    pub state: String,
    pub body: String,
    pub submitted_at: String,
}

#[derive(Debug, Clone, Serialize)]
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
}

#[derive(Debug, Clone, Serialize)]
pub struct PrSnapshot {
    pub number: u64,
    pub title: String,
    pub state: String,
    pub base: String,
    pub head: String,
    pub url: String,
    pub author: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorktreeSnapshot {
    pub path: String,
    pub branch: String,
    pub is_current: bool,
    pub is_pr: bool,
    pub pr_number: Option<u64>,
    pub is_merged: bool,
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn default_thread_side() -> String {
    "RIGHT".to_string()
}

fn severity_str(r: &RiskLevel) -> &'static str {
    match r {
        RiskLevel::High => "high",
        RiskLevel::Medium => "med",
        RiskLevel::Low | RiskLevel::Info => "low",
    }
}

fn comment_ref_to_thread(
    c: &CommentRef<'_>,
    file: &str,
    _hunk_idx: usize,
    tab: &TabState,
    pending: Option<&PendingAiReplies>,
) -> ThreadSnapshot {
    let kind = match c.comment_type() {
        er_engine::ai::CommentType::Question => "question",
        er_engine::ai::CommentType::GitHubComment => "comment",
    };
    let source = match c {
        CommentRef::GitHubComment(gc) => gc.source.clone(),
        _ => "local".to_string(),
    };
    let line = match c {
        CommentRef::Question(q) => q.line_start.unwrap_or(0),
        CommentRef::GitHubComment(gc) => gc.line_start.unwrap_or(0),
        CommentRef::Legacy(lc) => lc.line_start.unwrap_or(0),
    };
    let line_end = c.line_end();
    let side = match c {
        CommentRef::GitHubComment(gc) => gc.side.clone(),
        _ => default_thread_side(),
    };
    let author_kind = if c.author() == "You" { "you" } else { "human" };
    ThreadSnapshot {
        id: c.id().to_string(),
        kind: kind.to_string(),
        file: file.to_string(),
        line,
        line_end,
        side,
        source,
        synced: c.is_synced(),
        stale: match c {
            CommentRef::Question(q) => q.stale,
            CommentRef::GitHubComment(gc) => gc.stale || gc.outdated,
            CommentRef::Legacy(_) => false,
        },
        resolved: c.is_resolved(),
        root: ThreadMessage {
            id: c.id().to_string(),
            author: c.author().to_string(),
            kind: author_kind.to_string(),
            timestamp: c.timestamp().to_string(),
            body_markdown: c.text().to_string(),
        },
        replies: build_replies(c, tab, pending),
        promoted_to: match c {
            CommentRef::Question(q) => q.promoted_to.clone(),
            _ => None,
        },
    }
}

/// Author label for a thread message — an empty raw author means the local user.
fn display_author(raw: &str) -> String {
    if raw.is_empty() {
        "You".to_string()
    } else {
        raw.to_string()
    }
}

/// Reply kind from a resolved display author (shared by question + GitHub replies).
fn reply_kind(author: &str) -> &'static str {
    if author == "You" {
        "you"
    } else if author == "ai" {
        "ai"
    } else {
        "human"
    }
}

fn build_replies(
    root: &CommentRef<'_>,
    tab: &TabState,
    pending: Option<&PendingAiReplies>,
) -> Vec<ThreadMessage> {
    let root_id = root.id();
    let mut replies: Vec<ThreadMessage> = Vec::new();

    if let Some(qs) = &tab.ai.questions {
        for q in &qs.questions {
            if q.in_reply_to.as_deref() == Some(root_id) {
                let author = display_author(&q.author);
                let kind = reply_kind(&author);
                replies.push(ThreadMessage {
                    id: q.id.clone(),
                    author,
                    kind: kind.to_string(),
                    timestamp: q.timestamp.clone(),
                    body_markdown: q.text.clone(),
                });
            }
        }
    }
    if let Some(gc) = &tab.ai.github_comments {
        for c in &gc.comments {
            if c.in_reply_to.as_deref() == Some(root_id) {
                let author = display_author(&c.author);
                let kind = reply_kind(&author);
                replies.push(ThreadMessage {
                    id: c.id.clone(),
                    author,
                    kind: kind.to_string(),
                    timestamp: c.timestamp.clone(),
                    body_markdown: c.comment.clone(),
                });
            }
        }
    }

    replies.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

    // Append synthetic "…thinking" reply when an ask_ai subprocess is in
    // flight for this thread. Injected after the sort so it always renders at
    // the bottom regardless of timestamps.
    if let Some(pmap) = pending {
        let is_pending = pmap
            .lock()
            .map(|g| g.contains_key(root.id()))
            .unwrap_or(false);
        if is_pending {
            replies.push(ThreadMessage {
                id: String::new(),
                author: "ai".to_string(),
                kind: "ai".to_string(),
                timestamp: String::new(),
                body_markdown: "…thinking".to_string(),
            });
        }
    }

    replies
}

// ── Builder ──────────────────────────────────────────────────────────────────

pub type PrCache = Arc<Mutex<HashMap<String, Vec<PrInfo>>>>;
pub type GhUser = Arc<Mutex<Option<String>>>;

/// Cache key: (owner, repo, pr_number). Stores the most recent `GithubStatusSnapshot`
/// the background poller fetched.
pub type GhStatusCache = Arc<Mutex<HashMap<(String, String, u64), GithubStatusSnapshot>>>;

/// Map of `thread_id -> started_at_ms`. Used to render a synthetic "…thinking"
/// reply on threads where an AI subprocess is currently running.
pub type PendingAiReplies = Arc<Mutex<HashMap<String, u64>>>;

#[derive(Clone, Default)]
pub struct ProjectMeta {
    #[allow(dead_code)]
    pub current_branch: String,
    #[allow(dead_code)]
    pub base_branch: String,
    pub local_branches: Vec<BranchInfo>,
    pub auto_branches: Vec<BranchInfo>,
}

pub type MetaCache = std::sync::Arc<Mutex<HashMap<String, ProjectMeta>>>;

/// Refresh the per-project metadata cache by shelling out to git for each
/// known project. MUST NOT hold `AppState.app` — runs on a background thread.
pub fn refresh_meta_cache(active_root: &str, cache: &MetaCache) -> bool {
    refresh_meta_cache_filtered(active_root, cache, None)
}

/// Variant that refreshes a single project (by id), leaving entries for other
/// projects untouched. Used at startup so the active project's branches show
/// up immediately without paying for `git branch / worktree list / base
/// detection` on every other registered project first.
pub fn refresh_meta_cache_for_project(project_id: &str, cache: &MetaCache) -> bool {
    refresh_meta_cache_filtered("", cache, Some(project_id))
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
            let current_branch = detect_current_branch(&p.root_path);
            let base_branch = detect_base_branch(&p.root_path);
            let raw_worktrees = er_engine::git::list_worktrees(&p.root_path).unwrap_or_default();
            let local_branches = build_tracked_branches(
                &p.root_path,
                &base_branch,
                &current_branch,
                &p.tracked_branches,
                &p.dismissed_branches,
                &raw_worktrees,
            );
            let auto_branches = build_auto_branches(
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

/// Fingerprint of PR list cache + fetch timestamps (chrome-only poll input).
pub fn pr_cache_fingerprint(
    pr_cache: Option<&PrCache>,
    pr_cache_fetched_at: Option<&PrCacheFetchedAt>,
) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hash;

    let mut h = DefaultHasher::new();
    if let Some(cache) = pr_cache.and_then(|m| m.lock().ok()) {
        let mut entries: Vec<(&String, usize)> = cache.iter().map(|(k, v)| (k, v.len())).collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        for (k, n) in entries {
            k.hash(&mut h);
            n.hash(&mut h);
        }
    }
    if let Some(fetched) = pr_cache_fetched_at.and_then(|m| m.lock().ok()) {
        let mut entries: Vec<(&String, u64)> = fetched.iter().map(|(k, v)| (k, *v)).collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        for (k, ts) in entries {
            k.hash(&mut h);
            ts.hash(&mut h);
        }
    }
    crate::profile_log::finish_hash(h)
}

#[allow(clippy::too_many_arguments)]
pub fn build_snapshot(
    app: &App,
    pr_cache: Option<&PrCache>,
    pr_cache_fetched_at: Option<&PrCacheFetchedAt>,
    meta_cache: Option<&MetaCache>,
    gh_user: Option<&GhUser>,
    pending_ai: Option<&PendingAiReplies>,
    gh_status_cache: Option<&GhStatusCache>,
    loading: Option<&LoadingState>,
    watch_status: Option<&WatchStatusState>,
    inbox: Option<&InboxHandle>,
) -> AppSnapshot {
    build_snapshot_inner(
        app,
        pr_cache,
        pr_cache_fetched_at,
        meta_cache,
        gh_user,
        pending_ai,
        gh_status_cache,
        loading,
        watch_status,
        inbox,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn build_chrome_snapshot(
    app: &App,
    pr_cache: Option<&PrCache>,
    pr_cache_fetched_at: Option<&PrCacheFetchedAt>,
    meta_cache: Option<&MetaCache>,
    gh_user: Option<&GhUser>,
    pending_ai: Option<&PendingAiReplies>,
    gh_status_cache: Option<&GhStatusCache>,
    loading: Option<&LoadingState>,
    watch_status: Option<&WatchStatusState>,
    inbox: Option<&InboxHandle>,
) -> AppSnapshot {
    build_snapshot_inner(
        app,
        pr_cache,
        pr_cache_fetched_at,
        meta_cache,
        gh_user,
        pending_ai,
        gh_status_cache,
        loading,
        watch_status,
        inbox,
        true,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_snapshot_inner(
    app: &App,
    pr_cache: Option<&PrCache>,
    pr_cache_fetched_at: Option<&PrCacheFetchedAt>,
    meta_cache: Option<&MetaCache>,
    gh_user: Option<&GhUser>,
    pending_ai: Option<&PendingAiReplies>,
    gh_status_cache: Option<&GhStatusCache>,
    loading: Option<&LoadingState>,
    watch_status: Option<&WatchStatusState>,
    inbox: Option<&InboxHandle>,
    chrome_only: bool,
) -> AppSnapshot {
    let t0 = std::time::Instant::now();
    let tab = app.tab();

    let mode = match tab.mode {
        DiffMode::Branch => "branch",
        DiffMode::Unstaged => "unstaged",
        DiffMode::Staged => "staged",
        DiffMode::History => "history",
        DiffMode::Conflicts => "conflicts",
        DiffMode::Hidden => "hidden",
    };

    let input_mode = match &app.input_mode {
        InputMode::Normal => "normal",
        InputMode::Search => "search",
        InputMode::Comment => "comment",
        InputMode::Filter => "filter",
        InputMode::Commit => "commit",
        InputMode::Confirm(_) => "confirm",
        InputMode::RemoteUrl => "remoteurl",
    };

    let reviewed_count = tab.reviewed.len();
    let total_count = tab.active_diff_files().len();
    let active_selected = tab.active_selected_file_index();

    let visible = tab.visible_files();
    let mut diff_line_budget = std::env::var("ER_DESKTOP_SNAPSHOT_LINE_BUDGET")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(SNAPSHOT_DIFF_LINE_BUDGET);
    let files: Vec<FileSnapshot> = if chrome_only {
        Vec::new()
    } else {
        visible
            .iter()
            .map(|(source_index, f)| {
                let source_index = *source_index;
                let line_count = f.hunks.iter().map(|h| h.lines.len()).sum::<usize>();
                let include_hunks =
                    !f.compacted && (source_index == active_selected || diff_line_budget > 0);
                let budget_omitted = !f.compacted && !include_hunks;
                let hunks = if include_hunks {
                    diff_line_budget = diff_line_budget.saturating_sub(line_count);
                    build_hunks(f, tab, pending_ai)
                } else {
                    vec![]
                };

                // Per-file counts from AI state
                let finding_count = tab
                    .ai
                    .review
                    .as_ref()
                    .map(|r| {
                        r.files
                            .get(&f.path)
                            .map(|fr| fr.findings.len())
                            .unwrap_or(0)
                    })
                    .unwrap_or(0);
                let comment_count = tab.ai.file_github_comment_count(&f.path);
                let question_count = tab.ai.file_question_count(&f.path);

                // File-level risk from AI review
                let risk = tab
                    .ai
                    .review
                    .as_ref()
                    .and_then(|r| r.files.get(&f.path))
                    .map(|fr| severity_str(&fr.risk).to_string());

                FileSnapshot {
                    path: f.path.clone(),
                    status: status_str(&f.status),
                    additions: f.adds,
                    deletions: f.dels,
                    reviewed: tab.reviewed.contains_key(&f.path),
                    compacted: f.compacted,
                    risk,
                    finding_count,
                    comment_count,
                    question_count,
                    is_lazy_stub: (tab.lazy_mode && f.hunks.is_empty() && !f.compacted)
                        || budget_omitted,
                    hunks,
                    source_index,
                    cache_key: er_engine::cache::file_cache_key(&tab.branch_diff_hash, &f.path),
                }
            })
            .collect()
    };

    // Translate backend selection (index into active diff files) into a
    // visible-list index. If the selected file is filtered out, fall back to 0.
    let selected_file = visible
        .iter()
        .position(|(idx, _)| *idx == active_selected)
        .unwrap_or(0);

    let filter_suggestions: Vec<FilterSuggestionSnapshot> = {
        use er_engine::app::filter::FILTER_PRESETS;
        let mut out: Vec<FilterSuggestionSnapshot> = FILTER_PRESETS
            .iter()
            .map(|p| FilterSuggestionSnapshot {
                kind: "preset".to_string(),
                name: p.name.to_string(),
                expr: p.expr.to_string(),
            })
            .collect();
        for expr in &tab.filter_history {
            out.push(FilterSuggestionSnapshot {
                kind: "history".to_string(),
                name: expr.clone(),
                expr: expr.clone(),
            });
        }
        out
    };

    let ai = if chrome_only {
        empty_ai_snapshot()
    } else {
        build_ai_snapshot(tab, pending_ai)
    };
    let pr = if chrome_only {
        None
    } else {
        build_pr_snapshot(tab)
    };
    let commits = if chrome_only {
        Vec::new()
    } else {
        build_commits_snapshot(tab)
    };
    let selected_commit_sha = if !chrome_only && matches!(tab.mode, DiffMode::History) {
        tab.history
            .as_ref()
            .and_then(|h| h.commits.get(h.selected_commit))
            .map(|c| c.hash.clone())
    } else {
        None
    };

    let active_tab = app.active_tab;
    let tabs: Vec<TabSummary> = app
        .tabs
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let kind = if t.is_remote() {
                "remote_pr"
            } else if t.is_local_branch_view() {
                "local_branch"
            } else {
                "working"
            };
            TabSummary {
                idx: i,
                label: t.tab_name(),
                kind: kind.to_string(),
                branch: t.local_branch_view.clone(),
                pr_number: t.pr_number,
                repo_root: t.repo_root.clone(),
                is_active: i == active_tab,
                change_token: t.branch_diff_hash.clone(),
            }
        })
        .collect();

    let filter = if tab.filter_expr.is_empty() {
        None
    } else {
        Some(tab.filter_expr.clone())
    };

    // Browser-view UI annotations — read freshly from the active tab's
    // comments_dir so writes flow back to the UI on the next snapshot.
    let ui_annotations: Vec<UiAnnotationSnapshot> = if chrome_only {
        Vec::new()
    } else {
        er_engine::ai::load_ui_annotations(&tab.comments_dir())
            .into_iter()
            .map(|a| UiAnnotationSnapshot {
                id: a.id,
                url: a.url,
                selector: a.selector,
                box_x: a.box_x,
                box_y: a.box_y,
                box_w: a.box_w,
                box_h: a.box_h,
                viewport_w: a.viewport_w,
                viewport_h: a.viewport_h,
                text: a.text,
                timestamp: a.timestamp,
                author: a.author,
                screenshot_path: a.screenshot_path,
                stale: a.stale,
                element_context: a.element_context,
                dom_context: a.dom_context,
            })
            .collect()
    };

    // Resolve the active tab's GitHub status from the cache.
    // For remote PR tabs: use remote_repo + pr_number directly.
    // For working-tree / local-branch tabs: look up the current branch in pr_cache.
    let github = if let (Some(slug), Some(number)) = (tab.remote_repo.as_ref(), tab.pr_number) {
        slug.split_once('/').and_then(|(o, r)| {
            let key = (o.to_string(), r.to_string(), number);
            gh_status_cache
                .and_then(|c| c.lock().ok())
                .and_then(|g| g.get(&key).cloned())
        })
    } else {
        // Find a PR whose head_ref matches the viewed branch. Prefer open PRs,
        // but keep merged/closed matches so the Sources card can show terminal
        // PR state instead of looking disconnected.
        let branch = tab
            .local_branch_view
            .as_deref()
            .unwrap_or(&tab.current_branch);
        let pr_key = pr_cache.and_then(|pc| pc.lock().ok()).and_then(|cache| {
            cache.iter().find_map(|(slug, prs)| {
                prs.iter()
                    .filter(|p| p.head_ref == branch)
                    .min_by_key(|p| if p.state == "OPEN" { 0 } else { 1 })
                    .and_then(|p| {
                        slug.split_once('/')
                            .map(|(o, r)| (o.to_string(), r.to_string(), p.number))
                    })
            })
        });
        pr_key.and_then(|(o, r, n)| {
            let key = (o, r, n);
            gh_status_cache
                .and_then(|c| c.lock().ok())
                .and_then(|g| g.get(&key).cloned())
        })
    }
    .map(|mut status| {
        if let Some(login) = gh_user.and_then(|g| g.lock().ok().and_then(|v| v.clone())) {
            status.is_authored_by_me = status.author.eq_ignore_ascii_case(&login);
        }
        status
    });

    let diff_source = build_diff_source_snapshot(tab, pr_cache, meta_cache);

    let out = AppSnapshot {
        mode: mode.to_string(),
        branch: tab
            .local_branch_view
            .clone()
            .unwrap_or_else(|| tab.current_branch.clone()),
        base: tab.base_branch.clone(),
        input_mode: input_mode.to_string(),
        files,
        selected_file,
        current_hunk: Some(tab.current_hunk),
        filter,
        reviewed_count,
        total_count,
        ai,
        pr,
        panels: Panels {
            left: app.panels_visible.left,
            tree: app.panels_visible.tree,
            right: app.panels_visible.right,
        },
        theme: "dark".to_string(),
        watch_active: {
            let ws = watch_status
                .and_then(|w| w.lock().ok().map(|g| g.clone()))
                .unwrap_or_default();
            ws.active || app.watching
        },
        watch_status: watch_status
            .and_then(|w| w.lock().ok().map(|g| g.clone()))
            .unwrap_or_default(),
        worktrees: if chrome_only {
            Vec::new()
        } else {
            build_worktrees(&tab.repo_root, &tab.base_branch, &tab.repo_root)
        },
        projects: build_projects(tab, pr_cache, pr_cache_fetched_at, meta_cache, gh_user),
        notification: app.watch_message.clone(),
        local_branch: tab.local_branch_view.clone(),
        local_branch_checked_out: tab.local_branch_checkout_root.is_some(),
        tabs,
        active_tab,
        ui_annotations,
        browser: browser_snapshot_from_tab(tab),
        github,
        diff_source,
        bg_loading: loading
            .and_then(|l| l.lock().ok().map(|g| g.clone()))
            .unwrap_or_default(),
        agent_commands: build_agent_commands(app, tab),
        agent_log: build_agent_log(tab),
        active_ai_label: app.active_ai_selection_label(),
        filter_suggestions,
        commits,
        selected_commit_sha,
        background_tasks: app
            .background_task_snapshots()
            .into_iter()
            .map(|t| BackgroundTaskSnapshotWire {
                recent_log: t
                    .recent_log
                    .iter()
                    .map(|e| AgentLogSnapshot {
                        command_name: e.command_name.clone(),
                        source: match &e.source {
                            AgentLogSource::Stdout => "stdout".to_string(),
                            AgentLogSource::Stderr => "stderr".to_string(),
                            AgentLogSource::Status => "status".to_string(),
                        },
                        text: e.text.clone(),
                    })
                    .collect(),
                debug_log_path: None,
                id: t.id,
                kind: t.kind,
                label: t.label,
                target_label: t.target_label,
                scope: t.scope,
                status: t.status,
                error: t.error,
                started_at_ms: t.started_at_ms,
                finished_at_ms: t.finished_at_ms,
            })
            .collect(),
        inbox_items: inbox
            .and_then(|h| h.lock().ok().map(|g| g.items.clone()))
            .unwrap_or_default()
            .into_iter()
            .map(|i| InboxItemSnapshot {
                id: i.id,
                kind: i.kind,
                severity: i.severity,
                title: i.title,
                body: i.body,
                source: i.source,
                target: serde_json::to_value(i.target).unwrap_or(serde_json::Value::Null),
                created_at_ms: i.created_at_ms,
                read_at_ms: i.read_at_ms,
                dedupe_key: i.dedupe_key,
            })
            .collect(),
        inbox_unread_count: inbox
            .and_then(|h| h.lock().ok().map(|g| g.unread_count()))
            .unwrap_or(0),
        inbox_last_refresh_ms: inbox
            .and_then(|h| h.lock().ok().map(|g| g.last_refresh_ms))
            .unwrap_or(0),
        arena_enabled: app.config.features.arena,
        active_arena_run: if app.config.features.arena {
            app.active_arena_run()
        } else {
            None
        },
        arena_runs: if app.config.features.arena {
            app.arena_list_summaries().unwrap_or_default()
        } else {
            Vec::new()
        },
    };
    if crate::profile_log::profile_enabled() {
        let total_lines: usize = out
            .files
            .iter()
            .flat_map(|f| f.hunks.iter())
            .map(|h| h.lines.len())
            .sum();
        let max_file_lines: usize = out
            .files
            .iter()
            .map(|f| f.hunks.iter().map(|h| h.lines.len()).sum::<usize>())
            .max()
            .unwrap_or(0);
        let budget_omitted = out
            .files
            .iter()
            .filter(|f| f.is_lazy_stub && !f.compacted)
            .count();
        let rendered_hunks = out.files.iter().map(|f| f.hunks.len()).sum::<usize>();
        let meta_fp = meta_cache.map(meta_cache_fingerprint).unwrap_or(0);
        crate::profile_log::profile_log(
            "build_snapshot",
            &[
                ("build_ms", t0.elapsed().as_millis().to_string()),
                ("files", out.files.len().to_string()),
                ("rendered_hunks", rendered_hunks.to_string()),
                ("lines_in_ipc", total_lines.to_string()),
                ("max_file_lines", max_file_lines.to_string()),
                ("budget_omitted", budget_omitted.to_string()),
                ("meta_fp", meta_fp.to_string()),
                (
                    "chrome_only",
                    if chrome_only { "1" } else { "0" }.to_string(),
                ),
            ],
        );
    }
    out
}

fn empty_ai_snapshot() -> AiSnapshot {
    AiSnapshot {
        fresh: true,
        stale_reason: None,
        summary_markdown: None,
        agent_summaries: std::collections::HashMap::new(),
        high: 0,
        med: 0,
        low: 0,
        local_comment_count: 0,
        github_comment_count: 0,
        comments: 0,
        questions: 0,
        unpushed: 0,
        threads: Vec::new(),
        findings: Vec::new(),
        has_review_json: false,
        eligible_comment_count: 0,
    }
}

/// Load the most recent 10 commits on the current branch for the file viewer's
/// commit history scroller. Reuses history-mode commits if already loaded.
/// For local tabs shells out to `git log`. For remote PR tabs (no local repo),
/// fetches commits via `gh pr view --json commits`.
fn build_commits_snapshot(tab: &TabState) -> Vec<CommitSummary> {
    const LIMIT: usize = 10;

    let log_root = tab.commit_log_root();

    let raw: Vec<er_engine::git::CommitInfo> = if let Some(history) = tab.history.as_ref() {
        history.commits.iter().take(LIMIT).cloned().collect()
    } else if let Some(ref repo_slug) = tab.remote_repo {
        // Remote PR tab: no local git repo. Fetch commits via `gh pr view --json commits`.
        // Parse the "owner/repo" slug back into owner + repo.
        if let Some(pr_number) = tab.pr_number {
            if let Some((owner, repo)) = repo_slug.split_once('/') {
                er_engine::github::gh_pr_commits_remote(owner, repo, pr_number, LIMIT)
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        }
    } else {
        // Log the VIEWED branch's commits (base..branch), not base..HEAD — when
        // the branch isn't the checked-out HEAD of log_root, base..HEAD logs the
        // wrong branch (e.g. main). The branch ref resolves in the main clone too.
        // Matches the source History mode uses so clicking a commit resolves it.
        let head_ref = tab.commit_head_ref();
        let ranged = er_engine::git::git_log_range(&tab.base_branch, head_ref, log_root, LIMIT, 0)
            .unwrap_or_default();
        if ranged.is_empty() {
            // On the base branch itself `base..HEAD` is empty — fall back to
            // recent HEAD history so the commit scroller still shows something.
            er_engine::git::git_log_head(log_root, LIMIT).unwrap_or_default()
        } else {
            ranged
        }
    };

    raw.into_iter()
        .map(|c| CommitSummary {
            sha: c.hash,
            title: c.subject,
            author: c.author,
            age: c.relative_date,
        })
        .collect()
}

fn build_agent_commands(app: &App, tab: &TabState) -> Vec<AgentCommandSnapshot> {
    let mut out: Vec<AgentCommandSnapshot> = tab
        .command_status
        .iter()
        .map(|(name, status)| AgentCommandSnapshot {
            name: name.clone(),
            status: match status {
                CommandStatus::Running => "running".to_string(),
                CommandStatus::Done => "done".to_string(),
                CommandStatus::Failed(_) => "failed".to_string(),
            },
            error: match status {
                CommandStatus::Failed(msg) => Some(msg.clone()),
                _ => None,
            },
        })
        .collect();

    // Merge in app-level background tasks targeting this tab so existing
    // per-tab UI (status badges, agent-output card) keeps working.
    for task in app.background_tasks_for_tab(tab) {
        // Skip if a tab-local entry with the same name already exists
        // (avoids duplicate "review" badges if the user runs both layers).
        if out.iter().any(|c| c.name == task.kind) {
            continue;
        }
        out.push(AgentCommandSnapshot {
            name: task.kind,
            status: task.status,
            error: task.error,
        });
    }
    out
}

fn build_agent_log(tab: &TabState) -> Vec<AgentLogSnapshot> {
    tab.agent_log
        .iter()
        .rev()
        .take(200)
        .rev()
        .map(|e| AgentLogSnapshot {
            command_name: e.command_name.clone(),
            source: match &e.source {
                AgentLogSource::Stdout => "stdout".to_string(),
                AgentLogSource::Stderr => "stderr".to_string(),
                AgentLogSource::Status => "status".to_string(),
            },
            text: e.text.clone(),
        })
        .collect()
}

fn build_worktrees(
    repo_root: &str,
    base_branch: &str,
    current_root: &str,
) -> Vec<WorktreeSnapshot> {
    let wts = er_engine::git::list_worktrees(repo_root).unwrap_or_default();
    let skip_merged = wts.len() > 10;
    wts.into_iter()
        .map(|wt| {
            let (is_pr, pr_number, is_merged) =
                detect_pr_meta(&wt.path, &wt.branch, base_branch, skip_merged);
            WorktreeSnapshot {
                is_current: wt.path == current_root,
                branch: wt.branch,
                path: wt.path,
                is_pr,
                pr_number,
                is_merged,
            }
        })
        .collect()
}

fn build_tracked_branches(
    repo_root: &str,
    base_branch: &str,
    current_branch: &str,
    tracked: &[String],
    dismissed: &[String],
    worktrees: &[er_engine::git::Worktree],
) -> Vec<BranchInfo> {
    let out = std::process::Command::new("git")
        .args([
            "for-each-ref",
            "--format=%(refname:short)|%(upstream:short)",
            "refs/heads/",
        ])
        .current_dir(repo_root)
        .output();
    let Ok(out) = out else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&out.stdout);

    let skip_merged = worktrees.len() > 10 || base_branch.is_empty();

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
            let is_merged = if skip_merged || name == base_branch {
                false
            } else {
                std::process::Command::new("git")
                    .args(["merge-base", "--is-ancestor", &name, base_branch])
                    .current_dir(repo_root)
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false)
            };
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

fn build_auto_branches(
    repo_root: &str,
    base_branch: &str,
    current_branch: &str,
    tracked: &[String],
    dismissed: &[String],
    limit: usize,
    worktrees: &[er_engine::git::Worktree],
) -> Vec<BranchInfo> {
    let out = std::process::Command::new("git")
        .args([
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname:short)|%(upstream:short)",
            "refs/heads/",
        ])
        .current_dir(repo_root)
        .output();
    let Ok(out) = out else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&out.stdout);

    let skip_merged = worktrees.len() > 10 || base_branch.is_empty();

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
        let is_merged = if skip_merged || name == base_branch {
            false
        } else {
            std::process::Command::new("git")
                .args(["merge-base", "--is-ancestor", &name, base_branch])
                .current_dir(repo_root)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        };
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

fn minimal_pr_info(number: u64, title: &str) -> PrInfo {
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

fn resolve_saved_prs(
    entries: &[projects::SavedPrEntry],
    cache_prs: Option<&[PrInfo]>,
) -> Vec<PrInfo> {
    let mut out = Vec::new();
    let mut sorted: Vec<&projects::SavedPrEntry> = entries.iter().collect();
    sorted.sort_by(|a, b| b.saved_at_ms.cmp(&a.saved_at_ms));
    for entry in sorted {
        if let Some(cache) = cache_prs {
            if let Some(pr) = cache.iter().find(|p| p.number == entry.number) {
                out.push(pr.clone());
                continue;
            }
        }
        if !entry.title.is_empty() {
            out.push(minimal_pr_info(entry.number, &entry.title));
        }
    }
    out
}

fn resolve_recent_prs(
    entries: &[projects::RecentPrEntry],
    cache_prs: Option<&[PrInfo]>,
) -> Vec<PrInfo> {
    let mut out = Vec::new();
    let mut sorted: Vec<&projects::RecentPrEntry> = entries.iter().collect();
    sorted.sort_by(|a, b| b.viewed_at_ms.cmp(&a.viewed_at_ms));
    for entry in sorted {
        if let Some(cache) = cache_prs {
            if let Some(pr) = cache.iter().find(|p| p.number == entry.number) {
                out.push(pr.clone());
                continue;
            }
        }
        if !entry.title.is_empty() {
            out.push(minimal_pr_info(entry.number, &entry.title));
        }
    }
    out
}

fn normalize_remote_slug(remote: &str) -> String {
    let trimmed = remote.trim();
    let without_scheme = trimmed
        .strip_prefix("https://github.com/")
        .or_else(|| trimmed.strip_prefix("http://github.com/"))
        .unwrap_or(trimmed);
    without_scheme
        .trim_end_matches(".git")
        .trim_matches('/')
        .to_ascii_lowercase()
}

fn build_projects(
    tab: &TabState,
    pr_cache: Option<&PrCache>,
    pr_cache_fetched_at: Option<&PrCacheFetchedAt>,
    meta_cache: Option<&MetaCache>,
    gh_user: Option<&GhUser>,
) -> Vec<ProjectSnapshot> {
    // Cache layer: the per-project iteration in build_projects_from_file
    // dominates the snapshot cost on machines with several projects (full
    // clones of pr_cache + meta_cache). We avoid paying it twice when the
    // inputs are unchanged between consecutive snapshots.
    let key = build_projects_cache_key(tab, pr_cache, pr_cache_fetched_at, meta_cache, gh_user);
    if let Some(cached) = projects_cache_lookup(&key) {
        return cached;
    }
    let file = projects::load();
    let value = build_projects_from_file(
        &file,
        tab,
        pr_cache,
        pr_cache_fetched_at,
        meta_cache,
        gh_user,
    );
    projects_cache_store(key, &value);
    value
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProjectsCacheKey {
    projects_mtime_ns: u128,
    pr_cache_fingerprint: u64,
    meta_cache_fingerprint: u64,
    active_root: String,
    active_remote: Option<String>,
    viewed_branch: String,
    gh_user: Option<String>,
}

fn build_projects_cache_key(
    tab: &TabState,
    pr_cache: Option<&PrCache>,
    pr_cache_fetched_at: Option<&PrCacheFetchedAt>,
    meta_cache: Option<&MetaCache>,
    gh_user: Option<&GhUser>,
) -> ProjectsCacheKey {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let projects_mtime_ns = std::fs::metadata(projects::config_path())
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);

    // pr_cache fingerprint: combine remote names, len per remote, and last
    // fetched timestamps. Any fetch bumps the timestamp, so this catches all
    // mutations without needing a separate revision counter.
    let mut h = DefaultHasher::new();
    if let Some(cache) = pr_cache.and_then(|m| m.lock().ok()) {
        let mut entries: Vec<(&String, usize)> = cache.iter().map(|(k, v)| (k, v.len())).collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        for (k, n) in entries {
            k.hash(&mut h);
            n.hash(&mut h);
        }
    }
    if let Some(fetched) = pr_cache_fetched_at.and_then(|m| m.lock().ok()) {
        let mut entries: Vec<(&String, u64)> = fetched.iter().map(|(k, v)| (k, *v)).collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        for (k, ts) in entries {
            k.hash(&mut h);
            ts.hash(&mut h);
        }
    }
    let pr_cache_fingerprint = h.finish();

    // meta_cache fingerprint: project id + branch counts. Branch list changes
    // bump the count or the current_branch string; both included.
    let mut h = DefaultHasher::new();
    if let Some(meta) = meta_cache.and_then(|m| m.lock().ok()) {
        let mut entries: Vec<(&String, &ProjectMeta)> = meta.iter().collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        for (k, v) in entries {
            k.hash(&mut h);
            v.current_branch.hash(&mut h);
            v.base_branch.hash(&mut h);
            v.local_branches.len().hash(&mut h);
            v.auto_branches.len().hash(&mut h);
        }
    }
    let meta_cache_fingerprint = h.finish();

    let me: Option<String> = gh_user.and_then(|g| g.lock().ok().and_then(|v| v.clone()));
    let viewed_branch = tab
        .local_branch_view
        .clone()
        .unwrap_or_else(|| tab.current_branch.clone());

    ProjectsCacheKey {
        projects_mtime_ns,
        pr_cache_fingerprint,
        meta_cache_fingerprint,
        active_root: tab.repo_root.clone(),
        active_remote: tab.remote_repo.clone(),
        viewed_branch,
        gh_user: me,
    }
}

static PROJECTS_CACHE: Mutex<Option<(ProjectsCacheKey, Vec<ProjectSnapshot>)>> = Mutex::new(None);

fn projects_cache_lookup(key: &ProjectsCacheKey) -> Option<Vec<ProjectSnapshot>> {
    let guard = PROJECTS_CACHE.lock().ok()?;
    let (cached_key, value) = guard.as_ref()?;
    if cached_key == key {
        Some(value.clone())
    } else {
        None
    }
}

fn projects_cache_store(key: ProjectsCacheKey, value: &[ProjectSnapshot]) {
    if let Ok(mut guard) = PROJECTS_CACHE.lock() {
        *guard = Some((key, value.to_vec()));
    }
}

fn build_projects_from_file(
    file: &projects::ProjectsFile,
    tab: &TabState,
    pr_cache: Option<&PrCache>,
    pr_cache_fetched_at: Option<&PrCacheFetchedAt>,
    meta_cache: Option<&MetaCache>,
    gh_user: Option<&GhUser>,
) -> Vec<ProjectSnapshot> {
    let active_root = &tab.repo_root;
    let active_remote = tab.remote_repo.as_deref().map(normalize_remote_slug);

    let pr_map: Option<HashMap<String, Vec<PrInfo>>> =
        pr_cache.and_then(|m| m.lock().ok().map(|g| g.clone()));
    let pr_fetched_map: HashMap<String, u64> = pr_cache_fetched_at
        .and_then(|m| m.lock().ok().map(|g| g.clone()))
        .unwrap_or_default();

    // Snapshot the meta cache once, then drop the lock immediately.
    let meta_map: HashMap<String, ProjectMeta> = meta_cache
        .and_then(|m| m.lock().ok().map(|g| g.clone()))
        .unwrap_or_default();

    let me: Option<String> = gh_user.and_then(|g| g.lock().ok().and_then(|v| v.clone()));

    // The branch the user is currently viewing — for read-only tabs this is the
    // local_branch_view; otherwise the working tree's HEAD. Drives the "active"
    // dot in the sidebar so it tracks the tab, not the working tree.
    let viewed_branch: String = tab
        .local_branch_view
        .clone()
        .unwrap_or_else(|| tab.current_branch.clone());

    file.projects
        .iter()
        .map(|p| {
            let remote_only = p.root_path.is_empty() && p.remote.is_some();
            let project_remote = p.remote.as_deref().map(normalize_remote_slug);
            let is_active = if remote_only {
                active_remote.as_deref().is_some()
                    && active_remote.as_deref() == project_remote.as_deref()
            } else {
                &p.root_path == active_root
            };
            let mut meta = if remote_only {
                ProjectMeta::default()
            } else {
                meta_map.get(&p.id).cloned().unwrap_or_default()
            };

            // For the active project, recompute is_current per branch using the
            // viewed branch instead of the worktree's HEAD.
            if is_active {
                for b in meta.local_branches.iter_mut() {
                    b.is_current = b.name == viewed_branch;
                }
                for b in meta.auto_branches.iter_mut() {
                    b.is_current = b.name == viewed_branch;
                }
            }

            let cache_slice = p.remote.as_ref().and_then(|remote| {
                pr_map
                    .as_ref()
                    .and_then(|m| m.get(remote).map(|v| v.as_slice()))
            });

            let saved_prs = resolve_saved_prs(&p.saved_prs, cache_slice);
            let recent_prs = resolve_recent_prs(&p.recent_prs, cache_slice);

            let (my_prs, prs_to_review, recently_merged, pr_cache_stale, pr_cache_age_ms) =
                if remote_only {
                    (Vec::new(), Vec::new(), Vec::new(), false, None)
                } else if let (Some(remote), Some(ref cache)) = (&p.remote, &pr_map) {
                    let mut all: Vec<PrInfo> = cache
                        .get(remote)
                        .cloned()
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|pr| !p.dismissed_prs.contains(&pr.number))
                        .map(|mut pr| {
                            // Compute approved_by_me from transient latest_reviewer_states.
                            if let Some(ref login) = me {
                                pr.approved_by_me = pr
                                    .latest_reviewer_states
                                    .iter()
                                    .any(|(l, s)| l == login && s == "APPROVED");
                            }
                            pr
                        })
                        .collect();

                    let my: Vec<PrInfo> = all
                        .iter()
                        .filter(|pr| {
                            pr.state == "OPEN"
                                && me.as_deref().is_some_and(|login| pr.author == login)
                        })
                        .cloned()
                        .collect();

                    // "To review" = open PRs not authored by me that I haven't
                    // already reviewed. Excluding PRs I've already approved or
                    // requested changes on keeps the list to what still needs my
                    // attention (GitHub clears the review request once I review,
                    // but the PR stays open).
                    let to_review: Vec<PrInfo> = all
                        .iter()
                        .filter(|pr| {
                            pr.state == "OPEN"
                                && me.as_deref().is_none_or(|login| pr.author != login)
                                && !pr.approved_by_me
                                && me.as_deref().is_none_or(|login| {
                                    !pr.latest_reviewer_states.iter().any(|(l, s)| {
                                        l == login && (s == "APPROVED" || s == "CHANGES_REQUESTED")
                                    })
                                })
                        })
                        .cloned()
                        .collect();

                    all.retain(|pr| pr.state == "MERGED");
                    all.sort_by(|a, b| b.merged_at.cmp(&a.merged_at));
                    all.truncate(5);

                    let now_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0);
                    let age_ms = pr_fetched_map
                        .get(remote)
                        .copied()
                        .map(|fetched| now_ms.saturating_sub(fetched));
                    let stale = age_ms.map(|age| age > 10 * 60 * 1000).unwrap_or(true);
                    (my, to_review, all, stale, age_ms)
                } else {
                    (Vec::new(), Vec::new(), Vec::new(), false, None)
                };

            // Populate pr_number on branches by matching against open PRs.
            let all_open_prs: Vec<&PrInfo> = my_prs
                .iter()
                .chain(prs_to_review.iter())
                .filter(|p| p.state == "OPEN")
                .collect();

            for b in meta.local_branches.iter_mut() {
                if let Some(pr) = all_open_prs.iter().find(|p| p.head_ref == b.name) {
                    b.pr_number = Some(pr.number);
                }
            }
            for b in meta.auto_branches.iter_mut() {
                if let Some(pr) = all_open_prs.iter().find(|p| p.head_ref == b.name) {
                    b.pr_number = Some(pr.number);
                }
            }

            ProjectSnapshot {
                id: p.id.clone(),
                name: p.name.clone(),
                root_path: p.root_path.clone(),
                remote: p.remote.clone(),
                remote_only,
                is_active,
                local_branches: meta.local_branches,
                auto_branches: meta.auto_branches,
                saved_prs,
                my_prs,
                prs_to_review,
                recent_prs,
                recently_merged,
                pr_cache_stale,
                pr_cache_age_ms,
            }
        })
        .collect()
}

fn detect_current_branch(repo_root: &str) -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(repo_root)
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_default()
}

fn detect_base_branch(repo_root: &str) -> String {
    // Cheap fallback: try common defaults
    for candidate in ["main", "master", "develop", "dev"] {
        let out = std::process::Command::new("git")
            .args([
                "rev-parse",
                "--verify",
                &format!("refs/heads/{}", candidate),
            ])
            .current_dir(repo_root)
            .output();
        if let Ok(o) = out {
            if o.status.success() {
                return candidate.to_string();
            }
        }
    }
    String::new()
}

fn build_hunks(
    file: &DiffFile,
    tab: &TabState,
    pending: Option<&PendingAiReplies>,
) -> Vec<HunkSnapshot> {
    file.hunks
        .iter()
        .enumerate()
        .map(|(hunk_idx, hunk)| {
            let lines = hunk
                .lines
                .iter()
                .map(|line| {
                    let (kind, text) = match line.line_type {
                        LineType::Add => ("add", line.content.clone()),
                        LineType::Delete => ("del", line.content.clone()),
                        LineType::Context => ("context", line.content.clone()),
                        LineType::Fold(hidden) => {
                            ("fold", format!("··· {hidden} unchanged lines ···"))
                        }
                    };
                    LineSnapshot {
                        old_num: line.old_num,
                        new_num: line.new_num,
                        kind: kind.to_string(),
                        text,
                    }
                })
                .collect();

            let old_count = hunk
                .lines
                .iter()
                .filter(|l| matches!(l.line_type, LineType::Context | LineType::Delete))
                .count();
            let new_count = hunk
                .lines
                .iter()
                .filter(|l| matches!(l.line_type, LineType::Context | LineType::Add))
                .count();

            // Collect threads for this hunk (also matches comments whose hunk_index is
            // missing or stale, by falling back to line-range matching)
            let threads: Vec<ThreadSnapshot> = tab
                .ai
                .comments_for_hunk_or_line_range(
                    &file.path,
                    hunk_idx,
                    hunk.new_start,
                    new_count,
                    hunk.old_start,
                    old_count,
                )
                .iter()
                .filter(|c| {
                    c.in_reply_to().is_none()
                        && !(matches!(c, CommentRef::Question(_)) && c.is_resolved())
                })
                .map(|c| comment_ref_to_thread(c, &file.path, hunk_idx, tab, pending))
                .collect();

            HunkSnapshot {
                header: hunk.header.clone(),
                old_start: hunk.old_start,
                old_count,
                new_start: hunk.new_start,
                new_count,
                lines,
                threads,
            }
        })
        .collect()
}

fn build_ai_snapshot(tab: &TabState, pending: Option<&PendingAiReplies>) -> AiSnapshot {
    let ai = &tab.ai;
    let stale_reason = if !ai.is_stale {
        None
    } else if tab.branch_diff_hash.is_empty() {
        Some("Current diff hash is unavailable; refresh the diff.".to_string())
    } else {
        let review_mismatch = ai
            .review
            .as_ref()
            .map(|r| r.diff_hash != tab.branch_diff_hash)
            .unwrap_or(false);
        let order_mismatch = ai
            .order
            .as_ref()
            .map(|o| o.diff_hash != tab.branch_diff_hash)
            .unwrap_or(false);
        let checklist_mismatch = ai
            .checklist
            .as_ref()
            .map(|c| c.diff_hash != tab.branch_diff_hash)
            .unwrap_or(false);
        if review_mismatch {
            Some(
                "Review was generated for an older diff. Re-run or validate the review."
                    .to_string(),
            )
        } else if order_mismatch || checklist_mismatch {
            Some("Review metadata is out of date for the current diff.".to_string())
        } else {
            Some("Review artifacts do not match the current diff.".to_string())
        }
    };

    let (high, med, low) = if let Some(review) = &ai.review {
        let mut h = 0usize;
        let mut m = 0usize;
        let mut l = 0usize;
        for file_review in review.files.values() {
            for finding in file_review.findings.iter().filter(|f| f.is_active()) {
                match finding.severity {
                    RiskLevel::High => h += 1,
                    RiskLevel::Medium => m += 1,
                    RiskLevel::Low | RiskLevel::Info => l += 1,
                }
            }
        }
        (h, m, l)
    } else {
        (0, 0, 0)
    };

    let questions = ai
        .questions
        .as_ref()
        .map(|q| q.questions.len())
        .unwrap_or(0);
    let comments = ai
        .github_comments
        .as_ref()
        .map(|c| c.comments.len())
        .unwrap_or(0);
    let unpushed = ai
        .github_comments
        .as_ref()
        .map(|c| c.comments.iter().filter(|c| !c.synced).count())
        .unwrap_or(0);
    let local_comment_count = ai
        .github_comments
        .as_ref()
        .map(|c| {
            c.comments
                .iter()
                .filter(|comment| {
                    comment.in_reply_to.is_none() && comment.source == "local" && !comment.synced
                })
                .count()
        })
        .unwrap_or(0);
    let github_comment_count = ai
        .github_comments
        .as_ref()
        .map(|c| {
            c.comments
                .iter()
                .filter(|comment| {
                    comment.in_reply_to.is_none() && (comment.source == "github" || comment.synced)
                })
                .count()
        })
        .unwrap_or(0);

    // Flat thread list for CommentsCard / QuestionsCard
    let threads: Vec<ThreadSnapshot> = {
        let mut result = Vec::new();
        if let Some(qs) = &ai.questions {
            for q in &qs.questions {
                if q.in_reply_to.is_none() && !q.resolved {
                    let qref = CommentRef::Question(q);
                    result.push(ThreadSnapshot {
                        id: q.id.clone(),
                        kind: "question".to_string(),
                        file: q.file.clone(),
                        line: q.line_start.unwrap_or(0),
                        line_end: q.line_end,
                        side: default_thread_side(),
                        source: "local".to_string(),
                        synced: false,
                        stale: q.stale,
                        resolved: q.resolved,
                        root: ThreadMessage {
                            id: q.id.clone(),
                            author: display_author(&q.author),
                            kind: "you".to_string(),
                            timestamp: q.timestamp.clone(),
                            body_markdown: q.text.clone(),
                        },
                        replies: build_replies(&qref, tab, pending),
                        promoted_to: q.promoted_to.clone(),
                    });
                }
            }
        }
        if let Some(gc) = &ai.github_comments {
            for c in &gc.comments {
                if c.in_reply_to.is_none() {
                    let author_kind = if c.author.is_empty() || c.author == "You" {
                        "you"
                    } else {
                        "human"
                    };
                    let cref = CommentRef::GitHubComment(c);
                    result.push(ThreadSnapshot {
                        id: c.id.clone(),
                        kind: "comment".to_string(),
                        file: c.file.clone(),
                        line: c.line_start.unwrap_or(0),
                        line_end: c.line_end,
                        side: c.side.clone(),
                        source: c.source.clone(),
                        synced: c.synced,
                        stale: c.stale || c.outdated,
                        resolved: c.resolved,
                        root: ThreadMessage {
                            id: c.id.clone(),
                            author: display_author(&c.author),
                            kind: author_kind.to_string(),
                            timestamp: c.timestamp.clone(),
                            body_markdown: c.comment.clone(),
                        },
                        replies: build_replies(&cref, tab, pending),
                        promoted_to: None,
                    });
                }
            }
        }
        result
    };

    // Flat findings list for AiReviewCard. Merge promoted_to from the sibling
    // `.er/finding-promotions.json` so the UI can show "Promoted to #N".
    let promotions = crate::commands::load_finding_promotions(&tab.er_dir());
    let findings: Vec<FlatFinding> = if let Some(review) = &ai.review {
        review
            .files
            .iter()
            .flat_map(|(path, fr)| {
                let promotions = &promotions;
                let gh = ai.github_comments.as_ref();
                fr.findings.iter().filter(|f| f.is_active()).map(move |f| {
                    let thread_id = gh
                        .and_then(|gc| {
                            gc.comments
                                .iter()
                                .find(|c| {
                                    c.finding_ref.as_deref() == Some(f.id.as_str())
                                        && c.in_reply_to.is_none()
                                })
                                .map(|c| c.id.clone())
                        })
                        .or_else(|| {
                            ai.questions.as_ref().and_then(|qs| {
                                qs.questions
                                    .iter()
                                    .find(|q| {
                                        q.finding_ref.as_deref() == Some(f.id.as_str())
                                            && q.in_reply_to.is_none()
                                    })
                                    .map(|q| q.id.clone())
                            })
                        });
                    FlatFinding {
                        id: f.id.clone(),
                        file: path.clone(),
                        line: f.line_start,
                        hunk_index: f.hunk_index,
                        severity: severity_str(&f.severity).to_string(),
                        expert_label: er_engine::ai::expert_label_for_category(&f.category)
                            .map(|s| s.to_string()),
                        agent_label: er_engine::ai::agent_label_for_category(&f.category)
                            .to_string(),
                        title: f.title.clone(),
                        message_markdown: f.description.clone(),
                        promoted_to: promotions
                            .get(&f.id)
                            .cloned()
                            .or_else(|| f.promoted_to.clone()),
                        thread_id,
                    }
                })
            })
            .collect()
    } else {
        vec![]
    };

    let er_dir = tab.er_dir();
    let has_review_json = std::path::Path::new(&er_dir).join("review.json").exists();
    let eligible_comment_count = ai
        .github_comments
        .as_ref()
        .map(|gc| er_engine::ai::count_eligible_github_comments(gc))
        .unwrap_or(0);

    AiSnapshot {
        fresh: !ai.is_stale,
        stale_reason,
        summary_markdown: ai.summary.clone(),
        agent_summaries: ai.agent_summaries.clone(),
        high,
        med,
        low,
        local_comment_count,
        github_comment_count,
        comments,
        questions,
        unpushed,
        threads,
        findings,
        has_review_json,
        eligible_comment_count,
    }
}

fn build_diff_source_snapshot(
    tab: &er_engine::app::TabState,
    _pr_cache: Option<&PrCache>,
    meta_cache: Option<&MetaCache>,
) -> Option<DiffSourceSnapshot> {
    use er_engine::app::DiffSource;

    // Working-tree tabs (no local_branch_view, no remote_repo) don't show the card.
    if tab.local_branch_view.is_none() && tab.remote_repo.is_none() {
        return None;
    }

    let branch = tab
        .local_branch_view
        .clone()
        .unwrap_or_else(|| tab.current_branch.clone());

    // Look up upstream from meta_cache if available.
    let upstream = meta_cache.and_then(|mc| mc.lock().ok()).and_then(|cache| {
        cache.values().find_map(|entry| {
            entry
                .local_branches
                .iter()
                .chain(entry.auto_branches.iter())
                .find(|b| b.name == branch)
                .and_then(|b| b.upstream.clone())
        })
    });

    let active = tab.diff_source();
    let available = tab.available_diff_sources();

    let active_str = match active {
        DiffSource::Pr => "pr",
        DiffSource::Origin => "origin",
        DiffSource::Local => "local",
    }
    .to_string();

    let available_strs: Vec<String> = available
        .iter()
        .map(|s| match s {
            DiffSource::Pr => "pr",
            DiffSource::Origin => "origin",
            DiffSource::Local => "local",
        })
        .map(|s| s.to_string())
        .collect();

    let ahead_behind = tab.ahead_behind_vs_upstream();
    let (ahead, behind) = match ahead_behind {
        Some((a, b)) => (Some(a), Some(b)),
        None => (None, None),
    };

    let has_upstream = upstream.is_some();

    let (status, suggestion) = build_diff_source_copy(active, ahead, behind, has_upstream);

    Some(DiffSourceSnapshot {
        active: active_str,
        available: available_strs,
        branch,
        upstream,
        base: tab.base_branch.clone(),
        pr_number: tab.pr_number,
        ahead,
        behind,
        status,
        suggestion,
    })
}

fn build_diff_source_copy(
    source: er_engine::app::DiffSource,
    ahead: Option<u32>,
    behind: Option<u32>,
    has_upstream: bool,
) -> (String, String) {
    use er_engine::app::DiffSource;
    if !has_upstream && source != DiffSource::Pr {
        return (
            "No upstream configured. Only Local diff is available.".into(),
            String::new(),
        );
    }
    match source {
        DiffSource::Pr => (
            "Showing GitHub PR diff. This should match Files changed on GitHub.".into(),
            String::new(),
        ),
        DiffSource::Origin => {
            let ahead = ahead.unwrap_or(0);
            let behind = behind.unwrap_or(0);
            if ahead > 0 && behind > 0 {
                (
                    format!(
                        "Showing pushed branch. Local and origin have both moved ({ahead} ahead, {behind} behind)."
                    ),
                    "Prefer PR or Origin for review parity.".into(),
                )
            } else if ahead > 0 {
                (
                    format!("Showing pushed branch. Local has {ahead} unpushed commit(s)."),
                    "Switch to Local to inspect unpushed work.".into(),
                )
            } else if behind > 0 {
                (
                    format!("Showing pushed branch. Local is behind origin by {behind} commit(s)."),
                    String::new(),
                )
            } else {
                (
                    "Showing pushed branch. Local is up to date with origin.".into(),
                    String::new(),
                )
            }
        }
        DiffSource::Local => {
            let ahead = ahead.unwrap_or(0);
            let behind = behind.unwrap_or(0);
            if ahead > 0 && behind > 0 {
                (
                    format!("Showing local branch. Local and origin have both moved ({ahead} ahead, {behind} behind)."),
                    "Prefer PR or Origin for review parity.".into(),
                )
            } else if ahead > 0 {
                (
                    format!("Showing local branch with {ahead} unpushed commit(s)."),
                    String::new(),
                )
            } else if behind > 0 {
                (
                    format!("Showing local branch, but origin is {behind} commit(s) ahead."),
                    "Switch to Origin or PR for current review.".into(),
                )
            } else {
                (
                    "Showing local branch. In sync with origin.".into(),
                    String::new(),
                )
            }
        }
    }
}

fn build_pr_snapshot(tab: &TabState) -> Option<PrSnapshot> {
    let pr = tab.pr_data.as_ref()?;
    Some(PrSnapshot {
        number: pr.number,
        title: pr.title.clone(),
        state: pr.state.clone(),
        base: pr.base_branch.clone(),
        head: pr.head_branch.clone(),
        url: pr.url.clone(),
        author: pr.author.clone(),
    })
}

fn detect_pr_meta(
    worktree_path: &str,
    branch: &str,
    base: &str,
    skip_merged: bool,
) -> (bool, Option<u64>, bool) {
    let mut is_pr = false;
    let mut pr_number: Option<u64> = None;
    if let Ok(out) = std::process::Command::new("git")
        .args(["config", "--get", &format!("branch.{}.merge", branch)])
        .current_dir(worktree_path)
        .output()
    {
        if out.status.success() {
            let val = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if let Some(rest) = val.strip_prefix("refs/pull/") {
                if let Some(num_str) = rest.strip_suffix("/head") {
                    if let Ok(n) = num_str.parse::<u64>() {
                        is_pr = true;
                        pr_number = Some(n);
                    }
                }
            }
        }
    }

    let is_merged = if skip_merged || base.is_empty() || branch == base {
        false
    } else {
        std::process::Command::new("git")
            .args(["merge-base", "--is-ancestor", branch, base])
            .current_dir(worktree_path)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    };

    (is_pr, pr_number, is_merged)
}

fn status_str(status: &FileStatus) -> String {
    match status {
        FileStatus::Added => "added",
        FileStatus::Modified => "modified",
        FileStatus::Deleted => "deleted",
        FileStatus::Renamed(_) => "renamed",
        FileStatus::Copied(_) => "copied",
        FileStatus::Unmerged => "unmerged",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use er_engine::ai::{ErGitHubComments, GitHubReviewComment};

    fn github_comment(outdated: bool, stale: bool) -> GitHubReviewComment {
        GitHubReviewComment {
            id: "gh-1".to_string(),
            timestamp: "2026-01-01T00:00:00Z".to_string(),
            file: "src/foo.rs".to_string(),
            hunk_index: Some(0),
            line_start: Some(10),
            line_end: None,
            line_content: "fn foo() {}".to_string(),
            comment: "This thread is outdated on GitHub".to_string(),
            in_reply_to: None,
            resolved: false,
            source: "github".to_string(),
            github_id: Some(1),
            author: "octo".to_string(),
            synced: true,
            outdated,
            stale,
            context_before: vec![],
            context_after: vec![],
            old_line_start: None,
            hunk_header: String::new(),
            anchor_status: "original".to_string(),
            relocated_at_hash: String::new(),
            finding_ref: None,
            side: "RIGHT".to_string(),
        }
    }

    #[test]
    fn ai_snapshot_marks_github_outdated_comment_stale_for_ui() {
        let mut tab = TabState::new_for_test(vec![]);
        tab.ai.github_comments = Some(ErGitHubComments {
            version: 1,
            diff_hash: "hash".to_string(),
            github: None,
            comments: vec![github_comment(true, false)],
        });

        let snapshot = build_ai_snapshot(&tab, None);

        assert_eq!(snapshot.threads.len(), 1);
        assert!(snapshot.threads[0].stale);
    }

    #[test]
    fn thread_conversion_marks_github_outdated_comment_stale_for_ui() {
        let tab = TabState::new_for_test(vec![]);
        let comment = github_comment(true, false);

        let thread = comment_ref_to_thread(
            &CommentRef::GitHubComment(&comment),
            "src/foo.rs",
            0,
            &tab,
            None,
        );

        assert!(thread.stale);
    }

    #[test]
    fn pr_snapshot_includes_url_and_author_from_pr_data() {
        let mut tab = TabState::new_for_test(vec![]);
        tab.pr_data = Some(er_engine::github::PrOverviewData {
            number: 878,
            title: "Data table sorting".to_string(),
            body: String::new(),
            state: "OPEN".to_string(),
            author: "VilfredSikker".to_string(),
            url: "https://github.com/reshapebiotech/discovery/pull/878".to_string(),
            base_branch: "main".to_string(),
            head_branch: "DEV-3884/data-table-sorting".to_string(),
            checks: Vec::new(),
            reviewers: Vec::new(),
        });

        let pr = build_pr_snapshot(&tab).expect("pr snapshot");

        assert_eq!(pr.number, 878);
        assert_eq!(pr.author, "VilfredSikker");
        assert_eq!(
            pr.url,
            "https://github.com/reshapebiotech/discovery/pull/878"
        );
        assert_eq!(pr.head, "DEV-3884/data-table-sorting");
    }

    #[test]
    fn projects_snapshot_includes_remote_only_project_with_recent_pr() {
        let mut tab = TabState::new_for_test(vec![]);
        tab.remote_repo = Some("owner/repo".to_string());
        tab.pr_number = Some(123);

        let file = projects::ProjectsFile {
            projects: vec![projects::ProjectRecord {
                id: "remote-owner-repo".to_string(),
                name: "owner/repo".to_string(),
                root_path: String::new(),
                remote: Some("owner/repo".to_string()),
                dismissed_prs: Vec::new(),
                tracked_prs: Vec::new(),
                tracked_branches: Vec::new(),
                dismissed_branches: Vec::new(),
                recent_prs: vec![projects::RecentPrEntry {
                    number: 123,
                    viewed_at_ms: 10,
                    title: "Cached title fallback".to_string(),
                }],
                saved_prs: Vec::new(),
            }],
            active_id: None,
        };
        let cached_recent = PrInfo {
            number: 123,
            title: "Remote PR title".to_string(),
            head_ref: "feature".to_string(),
            state: "OPEN".to_string(),
            is_draft: false,
            author: "octo".to_string(),
            assignees: Vec::new(),
            reviewers: Vec::new(),
            checks_state: None,
            review_decision: None,
            merged_at: None,
            approved_by_me: false,
            base_ref: "main".to_string(),
            head_oid: "abc123".to_string(),
            updated_at: "2026-05-22T00:00:00Z".to_string(),
            latest_reviewer_states: Vec::new(),
        };
        let pr_cache = Arc::new(Mutex::new(HashMap::from([(
            "owner/repo".to_string(),
            vec![
                cached_recent.clone(),
                PrInfo {
                    number: 456,
                    title: "Another cached repo PR".to_string(),
                    ..cached_recent
                },
            ],
        )])));

        let projects = build_projects_from_file(&file, &tab, Some(&pr_cache), None, None, None);

        assert_eq!(projects.len(), 1);
        assert!(projects[0].remote_only);
        assert!(projects[0].is_active);
        assert!(projects[0].local_branches.is_empty());
        assert!(projects[0].auto_branches.is_empty());
        assert!(projects[0].my_prs.is_empty());
        assert!(projects[0].prs_to_review.is_empty());
        assert!(projects[0].recently_merged.is_empty());
        assert_eq!(projects[0].recent_prs.len(), 1);
        assert_eq!(projects[0].recent_prs[0].title, "Remote PR title");
    }
}
