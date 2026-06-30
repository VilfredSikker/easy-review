use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use er_engine::ai::{CommentRef, RiskLevel};
use er_engine::app::{AgentLogSource, App, CommandStatus, DiffMode, InputMode, TabState};
use er_engine::arena::{ArenaRunSnapshot, ArenaRunSummary};
use er_engine::git::{DiffFile, FileStatus, LineType};
use serde::{Deserialize, Serialize};

use crate::inbox::InboxHandle;
use crate::projects::{self, normalize_remote_slug};

/// Mtime/size-cached wrapper around `load_ui_annotations`. `build_snapshot`
/// runs on every poll; the annotations file rarely changes, so skip the disk
/// read + JSON parse unless the file's metadata moved. Writes go through
/// `save_ui_annotations` (tmp+rename), which always bumps the mtime.
fn load_ui_annotations_cached(comments_dir: &str) -> Vec<er_engine::ai::UiAnnotation> {
    type Key = Option<(std::time::SystemTime, u64)>;
    type AnnCache = HashMap<String, (Key, Vec<er_engine::ai::UiAnnotation>)>;
    static CACHE: std::sync::LazyLock<Mutex<AnnCache>> =
        std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

    let path = std::path::Path::new(comments_dir).join("ui-annotations.json");
    let key: Key = std::fs::metadata(&path)
        .ok()
        .and_then(|m| m.modified().ok().map(|t| (t, m.len())));

    let mut cache = match CACHE.lock() {
        Ok(g) => g,
        Err(_) => return er_engine::ai::load_ui_annotations(comments_dir),
    };
    if let Some((cached_key, anns)) = cache.get(comments_dir) {
        if *cached_key == key {
            return anns.clone();
        }
    }
    let anns = er_engine::ai::load_ui_annotations(comments_dir);
    // Bounded: one entry per comments_dir; drop everything if it somehow grows.
    if cache.len() > 64 {
        cache.clear();
    }
    cache.insert(comments_dir.to_string(), (key, anns.clone()));
    anns
}

// ── Differential snapshots ──────────────────────────────────────────────────
//
// Hunk lines dominate snapshot payloads. The backend remembers, per visible
// file path, the `delta_key` of the hunk content most recently delivered to
// the frontend; when a later snapshot would resend identical content the
// hunks are omitted (`FileSnapshot.hunks_omitted = true`) and the frontend
// splices in the hunks it already holds (keyed by the same `delta_key`). If
// the frontend can't match the key it downgrades the file to a lazy stub and
// re-fetches via `request_file_content` — the protocol self-heals.

/// Per-view memory of what file content the frontend currently holds.
#[derive(Default)]
pub struct SentFilesState {
    /// Identifies the (tab, mode, branch, filter…) the keys belong to.
    /// A mismatch clears the map — never omit across view switches.
    view_token: u64,
    /// path → `delta_key` of the full hunks last sent for that path.
    keys: HashMap<String, u64>,
}

impl SentFilesState {
    /// Forget everything — next snapshot sends full content (used when the
    /// frontend re-fetches from scratch via `get_snapshot`).
    pub fn reset(&mut self) {
        self.view_token = 0;
        self.keys.clear();
    }
}

pub type SentFilesHandle = Arc<Mutex<SentFilesState>>;

/// Stable content hash over exactly what `build_hunks` serializes for a file
/// (headers, line kinds/numbers, text). Per-file — unlike the old
/// whole-diff-hash-based `cache_key`, editing one file does not re-key the
/// others, so highlight caches and differential snapshots survive watch
/// refreshes.
fn file_lines_key(f: &DiffFile) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    f.path.hash(&mut h);
    f.compacted.hash(&mut h);
    f.hunks.len().hash(&mut h);
    for hunk in &f.hunks {
        hunk.header.hash(&mut h);
        hunk.old_start.hash(&mut h);
        hunk.new_start.hash(&mut h);
        hunk.lines.len().hash(&mut h);
        for line in &hunk.lines {
            match line.line_type {
                LineType::Add => 1u8.hash(&mut h),
                LineType::Delete => 2u8.hash(&mut h),
                LineType::Context => 3u8.hash(&mut h),
                LineType::Fold(hidden) => {
                    4u8.hash(&mut h);
                    hidden.hash(&mut h);
                }
            }
            line.old_num.hash(&mut h);
            line.new_num.hash(&mut h);
            line.content.hash(&mut h);
        }
    }
    h.finish()
}

/// Fingerprint of the full hunk payload: lines + inline threads. Threads are
/// hashed via their serialized form so any visible change (new reply,
/// resolved toggle, synthetic "thinking" reply) re-sends the file.
fn file_delta_key(lines_key: u64, threads_by_hunk: &[Vec<ThreadSnapshot>]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    lines_key.hash(&mut h);
    if threads_by_hunk.iter().any(|t| !t.is_empty()) {
        if let Ok(json) = serde_json::to_string(threads_by_hunk) {
            json.hash(&mut h);
        }
    }
    h.finish()
}

fn mode_str(mode: DiffMode) -> &'static str {
    match mode {
        DiffMode::Branch => "branch",
        DiffMode::Unstaged => "unstaged",
        DiffMode::Staged => "staged",
        DiffMode::History => "history",
        DiffMode::Conflicts => "conflicts",
        DiffMode::Hidden => "hidden",
        DiffMode::PrDiff => "pr",
        DiffMode::Tour => "tour",
    }
}

/// Record that the frontend now holds full hunks for `snap` (viewport-driven
/// lazy loads bypass `build_snapshot`, so `request_file_content` calls this).
/// No-op when the sent-files map belongs to a different view.
pub(crate) fn record_sent_file(
    app: &App,
    tab: &TabState,
    snap: &FileSnapshot,
    sent_files: &SentFilesHandle,
) {
    if matches!(tab.mode, DiffMode::History) || snap.hunks.is_empty() {
        return;
    }
    let Ok(mut guard) = sent_files.lock() else {
        return;
    };
    if guard.view_token != snapshot_view_token(app, tab, mode_str(tab.mode)) {
        return;
    }
    if let Ok(key) = u64::from_str_radix(&snap.delta_key, 16) {
        guard.keys.insert(snap.path.clone(), key);
    }
}

/// Identity of the current view — anything that changes which files the
/// frontend displays (or their meaning) must be included so the sent-files
/// map is cleared instead of wrongly omitting content across view switches.
fn snapshot_view_token(app: &App, tab: &TabState, mode: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    app.active_tab.hash(&mut h);
    tab.repo_root.hash(&mut h);
    mode.hash(&mut h);
    tab.current_branch.hash(&mut h);
    tab.base_branch.hash(&mut h);
    tab.pr_number.hash(&mut h);
    tab.local_branch_view.hash(&mut h);
    tab.filter_expr.hash(&mut h);
    tab.show_unreviewed_only.hash(&mut h);
    h.finish()
}

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
    /// First diff load of a freshly-selected stub tab (deferred to a
    /// background thread so tab switches return instantly).
    #[serde(default)]
    pub tab_diff: bool,
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

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FeatureFlagsSnapshot {
    pub view_branch: bool,
    pub view_unstaged: bool,
    pub view_staged: bool,
    pub view_history: bool,
    pub view_conflicts: bool,
    pub view_hidden: bool,
    pub view_tour: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DisplayConfigSnapshot {
    pub line_numbers: bool,
    pub wrap_lines: bool,
    pub split_diff: bool,
    pub tab_width: u8,
}

impl From<&er_engine::config::FeatureFlags> for FeatureFlagsSnapshot {
    fn from(f: &er_engine::config::FeatureFlags) -> Self {
        Self {
            view_branch: f.view_branch,
            view_unstaged: f.view_unstaged,
            view_staged: f.view_staged,
            view_history: f.view_history,
            view_conflicts: f.view_conflicts,
            view_hidden: f.view_hidden,
            view_tour: f.view_tour,
        }
    }
}

/// One file reference within a tour pillar (wire form).
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TourFileRef {
    pub path: String,
    pub reason: String,
    pub finding_ids: Vec<String>,
}

/// One tour pillar (wire form) with per-pillar review progress.
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PillarSnapshot {
    pub id: String,
    pub title: String,
    pub description_markdown: String,
    pub importance: u32,
    pub foundation: bool,
    pub files: Vec<TourFileRef>,
    pub reviewed_count: usize,
    pub total_count: usize,
}

/// Guided tour state for the active tab (Guide tab in the desktop).
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TourSnapshot {
    /// True when a guided tour exists for the current view context (PR vs branch).
    pub available: bool,
    /// True when the tour matches the current diff (not stale). When false and
    /// `available` is true, the UI offers a "Re-run guide" affordance.
    pub fresh: bool,
    /// Which diff the tour is attached to: `"pr"` or `"branch"`. Drives the
    /// Diff/PR header toggle state and where the Diff toggle returns to.
    pub scope: String,
    pub title: String,
    pub overview_markdown: String,
    pub pillars: Vec<PillarSnapshot>,
}

impl From<&er_engine::config::DisplayConfig> for DisplayConfigSnapshot {
    fn from(d: &er_engine::config::DisplayConfig) -> Self {
        Self {
            line_numbers: d.line_numbers,
            wrap_lines: d.wrap_lines,
            split_diff: d.split_diff,
            tab_width: d.tab_width,
        }
    }
}

/// +/- summary for a scope (unstaged / staged) so the scope selector can show
/// counts without switching modes.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ScopeStat {
    pub additions: usize,
    pub deletions: usize,
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
    #[serde(default)]
    pub features: FeatureFlagsSnapshot,
    #[serde(default)]
    pub display: DisplayConfigSnapshot,
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
    /// PR number detected for the active branch from the PR-list cache (sidebar
    /// match). Reliable regardless of whether gh-status has been fetched — drives
    /// the Local|PR Diff toggle.
    #[serde(default)]
    pub detected_pr_number: Option<u64>,
    /// Set when the active diff is behind origin (the open diff was computed
    /// against an older head/base than what origin now has). Drives the "stale"
    /// pill + manual Sync. None = up to date / unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff_stale: Option<DiffStaleSnapshot>,
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
    /// Claude Code effort level for the current session (`low` … `max`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_ai_effort: Option<String>,
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
    /// +/- summary for the working-tree (unstaged) and index (staged) diffs, so
    /// the scope selector shows counts without switching modes. Zeros when the
    /// tab isn't a live local checkout.
    #[serde(default)]
    pub unstaged_stat: ScopeStat,
    #[serde(default)]
    pub staged_stat: ScopeStat,
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
    /// AI Review Arena UI (`features.arena`, on by default).
    #[serde(default)]
    pub arena_enabled: bool,
    /// Active arena run id for this tab, if any.
    #[serde(default)]
    pub active_arena_run: Option<String>,
    /// Recent arena runs for the active tab (newest first).
    #[serde(default)]
    pub arena_runs: Vec<ArenaRunSummaryWire>,
    /// Guided tour (Guide tab). `available` drives whether the Guide tab shows.
    #[serde(default)]
    pub tour: TourSnapshot,
    /// Active arena runs across ALL tabs (tab-independent background runs).
    /// Lets the UI keep a run visible/controllable after switching branches.
    #[serde(default)]
    pub background_arena_runs: Vec<ArenaRunSummaryWire>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiffStaleSnapshot {
    /// "pr_head" | "base"
    pub kind: String,
    /// Short human label for the pill tooltip.
    pub message: String,
}

/// Returns Some(stale) only when both oids are known AND differ. None when
/// either is unknown (we don't guess) or they match. Keeps the "no-op when
/// equal / unknown" rule in one tested place.
pub fn compute_oid_staleness(
    latest_oid: Option<&str>,
    used_oid: Option<&str>,
    kind: &str,
    message: &str,
) -> Option<DiffStaleSnapshot> {
    match (latest_oid, used_oid) {
        (Some(latest), Some(used)) if !latest.is_empty() && !used.is_empty() && latest != used => {
            Some(DiffStaleSnapshot {
                kind: kind.to_string(),
                message: message.to_string(),
            })
        }
        _ => None,
    }
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
    pub repo_root: String,
    pub branch_label: String,
    pub pr_number: Option<u64>,
    pub remote_repo: Option<String>,
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
    /// When true, Desktop auto-runs triage on new/updated open PRs while the app is open.
    #[serde(default)]
    pub auto_triage: bool,
    /// When true (and `auto_triage`), also triage your own open PRs.
    #[serde(default)]
    pub auto_triage_own_prs: bool,
    /// When to auto-triage: `new-and-push`, `new-only`, or `review-requested`.
    pub auto_triage_when: String,
    /// Skip auto-triage when filtered diff exceeds this size (KB). `0` = no limit.
    #[serde(default)]
    pub auto_triage_max_diff_kb: u32,
    /// Glob patterns excluded from AI review diffs.
    #[serde(default)]
    pub review_ignore_globs: Vec<String>,
    /// PR numbers hidden via sidebar Ignore (for Unignore in action menu).
    #[serde(default)]
    pub dismissed_prs: Vec<u64>,
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

/// Resolve the `(owner, repo, pr_number)` GitHub-status key for a tab.
///
/// The key drives both the live `github` card in the snapshot and the
/// background `gh status` fetch. The resolution order matters:
///
/// 1. **Remote PR tab** — key directly off `remote_repo` + `pr_number`.
/// 2. **Local PR tab** (`pr_number` set, no remote) — force the tab's OWN
///    `pr_number` and resolve only the owner/repo slug from the PR-list cache.
///    The slug is found by the matching PR number first, falling back to any PR
///    on the viewed head branch. This is the fix for the "Branch view shows
///    another PR" bug: a single head branch can have two open PRs (e.g. one
///    targeting `main`, one targeting a stacked base), and matching purely by
///    head_ref returned an arbitrary one — often not the PR that was opened.
/// 3. **Plain branch / working tab** (no `pr_number`) — match the viewed
///    branch's head_ref, preferring an OPEN PR.
pub(crate) fn resolve_github_status_key(
    tab: &TabState,
    pr_cache: &HashMap<String, Vec<PrInfo>>,
) -> Option<(String, String, u64)> {
    // 1. Remote PR: slug + number directly.
    if let (Some(slug), Some(number)) = (tab.remote_repo.as_ref(), tab.pr_number) {
        return slug
            .split_once('/')
            .map(|(o, r)| (o.to_string(), r.to_string(), number));
    }

    let branch = tab
        .local_branch_view
        .as_deref()
        .unwrap_or(&tab.current_branch);

    // 2. Local PR tab: trust the tab's own pr_number; only resolve the slug.
    if let Some(number) = tab.pr_number {
        return pr_cache
            .iter()
            .find(|(_, prs)| prs.iter().any(|p| p.number == number))
            .or_else(|| {
                pr_cache
                    .iter()
                    .find(|(_, prs)| prs.iter().any(|p| p.head_ref == branch))
            })
            .and_then(|(slug, _)| {
                slug.split_once('/')
                    .map(|(o, r)| (o.to_string(), r.to_string(), number))
            });
    }

    // 3. Plain branch / working tab: match by head_ref, prefer an OPEN PR.
    pr_cache.iter().find_map(|(slug, prs)| {
        prs.iter()
            .filter(|p| p.head_ref == branch)
            .min_by_key(|p| if p.state == "OPEN" { 0 } else { 1 })
            .and_then(|p| {
                slug.split_once('/')
                    .map(|(o, r)| (o.to_string(), r.to_string(), p.number))
            })
    })
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
    /// Per-file: editing one file does not re-key unchanged files.
    pub cache_key: String,
    /// Fingerprint of the full hunk payload (lines + inline threads). The
    /// frontend keeps it alongside hunks so a later `hunks_omitted` snapshot
    /// can verify it still holds matching content before reusing it.
    pub delta_key: String,
    /// Differential snapshot: hunks omitted because the frontend already
    /// holds identical content for `delta_key`. Reuse prior hunks, or fall
    /// back to the lazy-stub fetch when no match is found.
    pub hunks_omitted: bool,
}

/// Lightweight commit metadata for the file viewer's history scroller.
/// Includes "All changes" + last N commits.
#[derive(Debug, Clone, Serialize)]
pub struct CommitSummary {
    pub sha: String,
    pub title: String,
    pub author: String,
    /// ISO 8601 commit timestamp; the frontend renders it as a relative
    /// "time ago" string (uniform across local and remote-PR commits).
    pub committed_at: String,
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
    pub kind: String, // "comment" | "question" | "note"
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
    #[serde(default)]
    pub origin: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub synced: Option<bool>,
    #[serde(default)]
    pub editable: Option<bool>,
    #[serde(default)]
    pub deletable: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FindingResponseSnapshot {
    pub id: String,
    pub author: String,
    pub kind: String,
    pub timestamp: String,
    pub body_markdown: String,
    pub origin: String,
    pub editable: bool,
    pub deletable: bool,
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
    #[serde(default)]
    pub responses: Vec<FindingResponseSnapshot>,
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
    pub notes: usize,
    pub unpushed: usize,
    pub threads: Vec<ThreadSnapshot>,
    pub findings: Vec<FlatFinding>,
    /// Whether `{er_dir}/review.json` exists (batch validate target).
    pub has_review_json: bool,
    /// Top-level GitHub comments eligible for batch validate (!resolved, !outdated).
    pub eligible_comment_count: usize,
    pub triage: Option<TriageSnapshot>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TriagePriorityFileSnapshot {
    pub path: String,
    pub reason: String,
    pub risk: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TriageSnapshot {
    pub fresh: bool,
    pub first_impression: String,
    pub verdict_primary: String,
    pub experts: Vec<String>,
    pub rationale: String,
    pub confidence: String,
    pub priority_files: Vec<TriagePriorityFileSnapshot>,
    pub files_changed: u32,
    pub approx_risk: String,
    pub domains: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckSummary {
    pub name: String,
    pub status: String,
    pub conclusion: String,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GhCommentSummary {
    pub author: String,
    pub body: String,
    pub created_at: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GhReviewSummary {
    pub author: String,
    pub state: String,
    pub body: String,
    pub submitted_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
        er_engine::ai::CommentType::Note => "note",
        er_engine::ai::CommentType::GitHubComment => "comment",
    };
    let source = match c {
        CommentRef::GitHubComment(gc) => gc.source.clone(),
        _ => "local".to_string(),
    };
    let line = match c {
        CommentRef::Question(q) | CommentRef::Note(q) => q.line_start.unwrap_or(0),
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
            CommentRef::Question(q) | CommentRef::Note(q) => q.stale,
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
            origin: None,
            source: None,
            synced: None,
            editable: None,
            deletable: None,
        },
        replies: build_replies(c, tab, pending),
        promoted_to: match c {
            CommentRef::Question(q) | CommentRef::Note(q) => q.promoted_to.clone(),
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
                    origin: Some("thread_reply".to_string()),
                    source: Some("question".to_string()),
                    synced: None,
                    editable: Some(kind == "you"),
                    deletable: Some(true),
                });
            }
        }
    }
    if let Some(ns) = &tab.ai.notes {
        for n in &ns.notes {
            if n.in_reply_to.as_deref() == Some(root_id) {
                let author = display_author(&n.author);
                let kind = reply_kind(&author);
                replies.push(ThreadMessage {
                    id: n.id.clone(),
                    author,
                    kind: kind.to_string(),
                    timestamp: n.timestamp.clone(),
                    body_markdown: n.text.clone(),
                    origin: Some("thread_reply".to_string()),
                    source: Some("note".to_string()),
                    synced: None,
                    editable: Some(kind == "you"),
                    deletable: Some(true),
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
                    origin: Some("thread_reply".to_string()),
                    source: Some(c.source.clone()),
                    synced: Some(c.synced),
                    editable: Some(kind == "you"),
                    deletable: Some(true),
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
                origin: Some("thread_reply".to_string()),
                source: None,
                synced: None,
                editable: None,
                deletable: None,
            });
        }
    }

    replies
}

// ── Builder ──────────────────────────────────────────────────────────────────

pub type PrCache = Arc<Mutex<HashMap<String, Vec<PrInfo>>>>;
pub type GhUser = Arc<Mutex<Option<String>>>;
/// Background-probed remote oids of branch tabs' base branches, keyed by
/// "{repo_root}\0{base_short}". Populated by the 60s ls-remote probe.
pub type BranchBaseRemoteOid = Arc<Mutex<HashMap<String, String>>>;

/// Cache key: (owner, repo, pr_number). Stores the most recent `GithubStatusSnapshot`
/// the background poller fetched.
pub type GhStatusCache = Arc<Mutex<HashMap<(String, String, u64), GithubStatusSnapshot>>>;

/// Map of `thread_id -> started_at_ms`. Used to render a synthetic "…thinking"
/// reply on threads where an AI subprocess is currently running.
pub type PendingAiReplies = Arc<Mutex<HashMap<String, u64>>>;

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

/// One PR-cache entry projected for fingerprinting: `(bucket key, pr count, [(pr number, head_oid)])`.
type PrFingerprintEntry<'a> = (&'a String, usize, Vec<(u64, &'a str)>);

/// Fingerprint of PR list cache + fetch timestamps (chrome-only poll input).
pub fn pr_cache_fingerprint(
    pr_cache: Option<&PrCache>,
    pr_cache_fetched_at: Option<&PrCacheFetchedAt>,
) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hash;

    let mut h = DefaultHasher::new();
    if let Some(cache) = pr_cache.and_then(|m| m.lock().ok()) {
        // Fold each PR's head_oid into the fingerprint so a head change alone
        // forces a snapshot recompute (the stale pill compares against head_oid).
        // Without it, a push that keeps the PR count constant is masked until an
        // unrelated desktop_revision bump — a fragile coupling.
        let mut entries: Vec<PrFingerprintEntry<'_>> = cache
            .iter()
            .map(|(k, v)| {
                (
                    k,
                    v.len(),
                    v.iter().map(|p| (p.number, p.head_oid.as_str())).collect(),
                )
            })
            .collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        for (k, n, mut oids) in entries {
            k.hash(&mut h);
            n.hash(&mut h);
            oids.sort_by_key(|(number, _)| *number);
            for (number, head_oid) in oids {
                number.hash(&mut h);
                head_oid.hash(&mut h);
            }
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

/// Build a single `FileSnapshot`.
///
/// `include_hunks` controls whether the file's diff hunks are serialized:
/// `false` for compacted files or files dropped by the snapshot-wide IPC line
/// budget (`budget_omitted`). Callers that need one file's content regardless of
/// that budget — the viewport-driven lazy loader — pass `true`, which keeps the
/// per-file lazy round-trip from re-serializing the entire diff.
pub(crate) fn build_file_snapshot(
    source_index: usize,
    f: &DiffFile,
    tab: &TabState,
    pending_ai: Option<&PendingAiReplies>,
    include_hunks: bool,
) -> FileSnapshot {
    let budget_omitted = !f.compacted && !include_hunks;
    let hunks = if include_hunks {
        build_hunks(f, tab, pending_ai)
    } else {
        vec![]
    };

    let finding_count = tab
        .ai
        .review
        .as_ref()
        .map(|r| {
            r.files
                .get(&f.path)
                .map(|fr| fr.findings.iter().filter(|fd| fd.is_active()).count())
                .unwrap_or(0)
        })
        .unwrap_or(0);
    let comment_count = tab.ai.file_github_comment_count(&f.path);
    let question_count = tab.ai.file_question_count(&f.path);

    let risk = tab
        .ai
        .review
        .as_ref()
        .and_then(|r| r.files.get(&f.path))
        .map(|fr| severity_str(&fr.risk).to_string());

    let lines_key = file_lines_key(f);
    let delta_key = file_delta_key(lines_key, &build_hunk_threads(f, tab, pending_ai));

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
        is_lazy_stub: (tab.lazy_mode && f.hunks.is_empty() && !f.compacted) || budget_omitted,
        hunks,
        source_index,
        cache_key: format!("{lines_key:016x}"),
        delta_key: format!("{delta_key:016x}"),
        hunks_omitted: false,
    }
}

/// Build the guided-tour wire snapshot from the loaded `tour.json`, including
/// per-pillar reviewed/total counts (shared branch `reviewed` set).
///
/// When the tab shows the branch diff (Branch/Tour/PrDiff — the modes that share
/// the branch bucket and load the branch diff into `tab.files`), this mirrors the
/// TUI's `rebuild_tour_state`: files absent from the diff are dropped, each file
/// appears in only its first pillar, empty pillars are skipped, and unreferenced
/// diff files collect into a trailing "Other changes" pillar. In other scopes
/// (unstaged/staged/history) `tab.files` is a different diff, so the raw pillars
/// are passed through unfiltered — only `available`/pillar-count is consumed
/// there (the pillar nav renders in Tour mode only).
fn build_tour_snapshot(tab: &TabState) -> TourSnapshot {
    let Some(tour) = tab.ai.tour.as_ref() else {
        return TourSnapshot::default();
    };
    if tour.pillars.is_empty() {
        return TourSnapshot::default();
    }

    let branch_scope = matches!(
        tab.mode,
        DiffMode::Branch | DiffMode::Tour | DiffMode::PrDiff
    );

    let make_file = |f: &er_engine::ai::TourFile| TourFileRef {
        path: f.path.clone(),
        reason: f.reason.clone(),
        finding_ids: f.finding_ids.clone(),
    };
    let count_reviewed = |files: &[TourFileRef]| {
        files
            .iter()
            .filter(|f| tab.reviewed.contains_key(&f.path))
            .count()
    };

    let mut pillars: Vec<PillarSnapshot> = Vec::new();

    if branch_scope {
        // Filter to files actually in the diff; dedup so a file shows once.
        let diff_paths: std::collections::HashSet<&str> =
            tab.files.iter().map(|f| f.path.as_str()).collect();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        for p in tour.ordered_pillars() {
            let mut files: Vec<TourFileRef> = Vec::new();
            for f in &p.files {
                if !diff_paths.contains(f.path.as_str()) || !seen.insert(f.path.clone()) {
                    continue;
                }
                files.push(make_file(f));
            }
            if files.is_empty() {
                continue;
            }
            let reviewed_count = count_reviewed(&files);
            let total_count = files.len();
            pillars.push(PillarSnapshot {
                id: p.id.clone(),
                title: p.title.clone(),
                description_markdown: p.description.clone(),
                importance: p.importance,
                foundation: p.foundation,
                files,
                reviewed_count,
                total_count,
            });
        }
        // Trailing "Other changes" pillar for diff files no pillar referenced.
        let other: Vec<TourFileRef> = tab
            .files
            .iter()
            .filter(|f| !seen.contains(&f.path))
            .map(|f| TourFileRef {
                path: f.path.clone(),
                reason: String::new(),
                finding_ids: Vec::new(),
            })
            .collect();
        if !other.is_empty() {
            let reviewed_count = count_reviewed(&other);
            let total_count = other.len();
            pillars.push(PillarSnapshot {
                id: "__other__".to_string(),
                title: "Other changes".to_string(),
                description_markdown: "Files not assigned to a tour pillar.".to_string(),
                importance: 0,
                foundation: false,
                files: other,
                reviewed_count,
                total_count,
            });
        }
    } else {
        for p in tour.ordered_pillars() {
            let files: Vec<TourFileRef> = p.files.iter().map(&make_file).collect();
            let reviewed_count = count_reviewed(&files);
            let total_count = files.len();
            pillars.push(PillarSnapshot {
                id: p.id.clone(),
                title: p.title.clone(),
                description_markdown: p.description.clone(),
                importance: p.importance,
                foundation: p.foundation,
                files,
                reviewed_count,
                total_count,
            });
        }
    }

    TourSnapshot {
        available: true,
        // Staleness is computed per-context in `reload_ai_state` (`tour_stale_for`);
        // `scope` tells the frontend which guide (branch vs PR) is showing so the Guide
        // tab is context-aware and the Diff toggle returns to the right diff.
        fresh: !tab.ai.tour_stale,
        scope: if tab.tour_context_is_pr() {
            "pr".to_string()
        } else {
            "branch".to_string()
        },
        title: tour.title.clone(),
        overview_markdown: tour.overview.clone(),
        pillars,
    }
}

/// Build a full snapshot, with differential-snapshot support: when
/// `sent_files` is provided, files whose hunk content the frontend already
/// holds are sent with `hunks_omitted = true` and no hunk payload.
#[allow(clippy::too_many_arguments)]
pub fn build_snapshot_with_delta(
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
    sent_files: Option<&SentFilesHandle>,
    branch_base_remote_oid: Option<&BranchBaseRemoteOid>,
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
        sent_files,
        branch_base_remote_oid,
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
    branch_base_remote_oid: Option<&BranchBaseRemoteOid>,
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
        None,
        branch_base_remote_oid,
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
    sent_files: Option<&SentFilesHandle>,
    branch_base_remote_oid: Option<&BranchBaseRemoteOid>,
) -> AppSnapshot {
    let t0 = std::time::Instant::now();
    let tab = app.tab();

    let mode = mode_str(tab.mode);

    let input_mode = match &app.input_mode {
        InputMode::Normal => "normal",
        InputMode::Search => "search",
        InputMode::Comment => "comment",
        InputMode::Filter => "filter",
        InputMode::Commit => "commit",
        InputMode::Confirm(_) => "confirm",
        InputMode::RemoteUrl => "remoteurl",
    };

    let (reviewed_count, total_count) = tab.active_reviewed_count();
    let active_selected = tab.active_selected_file_index();

    let visible = tab.visible_files();
    let mut diff_line_budget = std::env::var("ER_DESKTOP_SNAPSHOT_LINE_BUDGET")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(SNAPSHOT_DIFF_LINE_BUDGET);
    // Differential snapshots: lock the sent-files map for the duration of the
    // file loop. History mode rebuilds files per commit selection — skip
    // omission there (per-commit diffs are small anyway).
    let mut sent_guard = if chrome_only || matches!(tab.mode, DiffMode::History) {
        None
    } else {
        sent_files.and_then(|h| h.lock().ok())
    };
    if let Some(guard) = sent_guard.as_mut() {
        let token = snapshot_view_token(app, tab, mode);
        if guard.view_token != token {
            guard.keys.clear();
            guard.view_token = token;
        }
    }
    let files: Vec<FileSnapshot> = if chrome_only {
        Vec::new()
    } else {
        visible
            .iter()
            .map(|(source_index, f)| {
                let source_index = *source_index;

                // Omit hunks when the frontend already holds this exact
                // content. Omitted files don't consume the line budget, so
                // the budget only throttles *changed* files.
                if let Some(guard) = sent_guard.as_mut() {
                    if !f.compacted && !f.hunks.is_empty() {
                        let lines_key = file_lines_key(f);
                        let delta_key =
                            file_delta_key(lines_key, &build_hunk_threads(f, tab, pending_ai));
                        if guard.keys.get(&f.path) == Some(&delta_key) {
                            let mut snap =
                                build_file_snapshot(source_index, f, tab, pending_ai, false);
                            snap.is_lazy_stub = false;
                            snap.hunks_omitted = true;
                            return snap;
                        }
                    }
                }

                let line_count = f.hunks.iter().map(|h| h.lines.len()).sum::<usize>();
                let is_large = f.adds + f.dels > tab.compaction_config.max_lines_before_compact;
                let include_hunks = !f.compacted
                    && (!is_large || source_index == active_selected || diff_line_budget > 0);
                if include_hunks && is_large {
                    diff_line_budget = diff_line_budget.saturating_sub(line_count);
                }
                let snap = build_file_snapshot(source_index, f, tab, pending_ai, include_hunks);
                if let Some(guard) = sent_guard.as_mut() {
                    // Track only paths the frontend now holds full hunks for.
                    if include_hunks && !snap.hunks.is_empty() {
                        if let Ok(key) = u64::from_str_radix(&snap.delta_key, 16) {
                            guard.keys.insert(snap.path.clone(), key);
                        }
                    } else {
                        guard.keys.remove(&snap.path);
                    }
                }
                snap
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

    // Browser-view UI annotations — read from the active tab's comments_dir
    // (mtime-cached: build_snapshot runs on every poll, and the annotations
    // file rarely changes; saves a disk read + JSON parse per poll).
    let ui_annotations: Vec<UiAnnotationSnapshot> = if chrome_only {
        Vec::new()
    } else {
        load_ui_annotations_cached(&tab.comments_dir())
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

    // Resolve the active tab's GitHub status from the cache. The key prefers the
    // tab's own PR number for PR tabs (remote or local) and only falls back to a
    // head_ref match for plain branch/working tabs — see `resolve_github_status_key`.
    // Matching purely by head_ref previously let a branch with two open PRs show
    // an arbitrary one, so a freshly opened PR's Branch card could display a
    // different PR entirely.
    let github = pr_cache
        .and_then(|pc| pc.lock().ok())
        .and_then(|cache| resolve_github_status_key(tab, &cache))
        .and_then(|key| {
            gh_status_cache
                .and_then(|c| c.lock().ok())
                .and_then(|g| g.get(&key).cloned())
        })
        .map(|mut status| {
            if let Some(login) = gh_user.and_then(|g| g.lock().ok().and_then(|v| v.clone())) {
                status.is_authored_by_me = status.author.eq_ignore_ascii_case(&login);
            }
            status
        });

    // Detected PR number for the active branch — taken straight from the PR-list
    // cache (the same branch→PR match the sidebar badge uses). Unlike `github`,
    // this does NOT require a gh-status fetch to have run, so the Local|PR Diff
    // toggle is reliable instead of timing-dependent.
    let detected_pr_number: Option<u64> = if let Some(n) = tab.pr_number {
        Some(n)
    } else if tab.is_remote() {
        None
    } else {
        let branch = tab
            .local_branch_view
            .as_deref()
            .unwrap_or(&tab.current_branch);
        pr_cache.and_then(|pc| pc.lock().ok()).and_then(|cache| {
            cache
                .values()
                .flat_map(|prs| prs.iter())
                .filter(|p| p.head_ref == branch)
                .min_by_key(|p| if p.state == "OPEN" { 0 } else { 1 })
                .map(|p| p.number)
        })
    };

    // Freshness check for the active diff. Cheap: PR case reads the already-fresh
    // PR-list cache (gh pr list, 10-min cadence); branch case reads a background
    // ls-remote cache + a local `git rev-parse`. Never fetches here. Any unknown
    // oid yields None (we don't guess), so the pill never wedges "stale".
    let diff_stale: Option<DiffStaleSnapshot> = {
        // Fire whenever the tab represents a PR — including a local PR tab
        // switched to Branch view (via set_mode), which matches neither
        // `DiffMode::PrDiff` nor the `shows_branch_base_diff` fallback below
        // (that one requires `pr_number.is_none()`), so its pill would otherwise
        // be silently gated out.
        let pr_showing =
            tab.pr_number.is_some() || tab.is_remote() || matches!(tab.mode, DiffMode::PrDiff);
        if pr_showing {
            // PR case (FREE): compare the head_oid the open diff was computed
            // against to the latest head_oid carried by the PR-list cache.
            // Prefer the tab's own PR number — `detected_pr_number` is None for
            // remote PR tabs, so keying off it alone would never light their pill.
            tab.pr_number.or(detected_pr_number).and_then(|pr_number| {
                let cached_head_oid = pr_cache.and_then(|pc| pc.lock().ok()).and_then(|cache| {
                    cache
                        .values()
                        .flat_map(|prs| prs.iter())
                        .find(|p| p.number == pr_number)
                        .map(|p| p.head_oid.clone())
                        .filter(|s| !s.is_empty())
                });
                compute_oid_staleness(
                    cached_head_oid.as_deref(),
                    tab.last_diff_head_oid.as_deref(),
                    "pr_head",
                    "PR head updated on origin — Sync to refresh",
                )
            })
        } else if tab.shows_branch_base_diff() {
            // Branch case (main checkout or read-only branch view): compare the
            // background-probed remote tip of the base branch to the oid the diff's
            // actual base ref currently points at. We resolve `tab.base_branch`
            // directly (not a hardcoded `origin/<base>`) because the main checkout
            // may diff against a local `main` — comparing that local ref to origin's
            // tip is exactly the "behind main" signal.
            let base_short = tab.base_branch.trim_start_matches("origin/");
            let key = format!("{}\u{0}{}", tab.repo_root, base_short);
            let latest = branch_base_remote_oid
                .and_then(|c| c.lock().ok())
                .and_then(|cache| cache.get(&key).cloned());
            let used = er_engine::github::rev_parse_oid(&tab.repo_root, &tab.base_branch);
            compute_oid_staleness(
                latest.as_deref(),
                used.as_deref(),
                "base",
                &format!("{base_short} has new commits on origin — Sync to refresh"),
            )
        } else {
            None
        }
    };

    // Per-scope +/- counters for the scope selector. Only meaningful for a live
    // local checkout (working tree or checked-out branch view); skipped on remote
    // PR tabs, read-only branch views, and chrome-only rebuilds.
    let (unstaged_stat, staged_stat) = if !chrome_only
        && !tab.is_remote()
        && (tab.local_branch_view.is_none() || tab.local_branch_checkout_root.is_some())
    {
        let (ua, ud) = er_engine::git::diff_shortstat(&tab.repo_root, false);
        let (sa, sd) = er_engine::git::diff_shortstat(&tab.repo_root, true);
        (
            ScopeStat {
                additions: ua,
                deletions: ud,
            },
            ScopeStat {
                additions: sa,
                deletions: sd,
            },
        )
    } else {
        (ScopeStat::default(), ScopeStat::default())
    };

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
        theme: app.config.display.theme.clone(),
        features: FeatureFlagsSnapshot::from(&app.config.features),
        display: DisplayConfigSnapshot::from(&app.config.display),
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
        unstaged_stat,
        staged_stat,
        tabs,
        active_tab,
        ui_annotations,
        browser: browser_snapshot_from_tab(tab),
        github,
        detected_pr_number,
        diff_stale,
        bg_loading: loading
            .and_then(|l| l.lock().ok().map(|g| g.clone()))
            .unwrap_or_default(),
        agent_commands: build_agent_commands(app, tab),
        agent_log: build_agent_log(tab),
        active_ai_label: app.active_ai_selection_label(),
        active_ai_effort: app.current_ai_effort.clone(),
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
                repo_root: t.repo_root,
                branch_label: t.branch_label,
                pr_number: t.pr_number,
                remote_repo: t.remote_repo,
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
        arena_enabled: true,
        active_arena_run: app.active_arena_run(),
        arena_runs: {
            let branch = app.arena_branch_ref();
            app.arena_list_summaries(Some(&branch)).unwrap_or_default()
        },
        tour: build_tour_snapshot(tab),
        background_arena_runs: app.background_arena_runs(),
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
        notes: 0,
        unpushed: 0,
        threads: Vec::new(),
        findings: Vec::new(),
        has_review_json: false,
        eligible_comment_count: 0,
        triage: None,
    }
}

/// Load the most recent 10 commits for the file viewer's commit history
/// scroller. PR tabs use cached GitHub PR commits so the list matches
/// GitHub's PR Commits tab and snapshot rendering never shells out to `gh`.
fn build_commits_snapshot(tab: &TabState) -> Vec<CommitSummary> {
    const LIMIT: usize = 10;

    let log_root = tab.commit_log_root();

    let raw: Vec<er_engine::git::CommitInfo> = if let Some(history) = tab.history.as_ref() {
        history.commits.iter().take(LIMIT).cloned().collect()
    } else if tab.pr_number.is_some() {
        tab.pr_commits.iter().take(LIMIT).cloned().collect()
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
            committed_at: c.date,
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

#[derive(PartialEq, Eq)]
struct WorktreesMetaKey {
    repo_root: String,
    base_branch: String,
    fingerprint: u64,
}

/// Per-worktree PR metadata `(is_pr, pr_number, is_merged)` keyed by worktree path.
type WorktreeMetaMap = HashMap<String, (bool, Option<u64>, bool)>;

/// Cached per-worktree PR metadata `(is_pr, pr_number, is_merged)` keyed by path.
///
/// `build_worktrees` runs on every snapshot build (every ~2s poll). The cheap
/// `list_worktrees` call stays live so add/remove/branch-switch is reflected
/// within one poll, but `detect_pr_meta` spawns `git config` + `git merge-base`
/// per worktree — ~2N subprocesses that dominate snapshot construction. The set
/// rarely changes, so cache that meta keyed on a fingerprint of the worktree set:
/// add/remove/switch changes the fingerprint → cache miss → recompute, and a TTL
/// backstops `is_merged` drift when the set is unchanged.
static WORKTREES_META_CACHE: Mutex<
    Option<(WorktreesMetaKey, std::time::Instant, WorktreeMetaMap)>,
> = Mutex::new(None);
const WORKTREES_META_TTL: std::time::Duration = std::time::Duration::from_secs(30);

fn worktrees_meta_cached(
    key: WorktreesMetaKey,
    compute: impl FnOnce() -> WorktreeMetaMap,
) -> WorktreeMetaMap {
    if let Ok(guard) = WORKTREES_META_CACHE.lock() {
        if let Some((cached_key, computed_at, value)) = guard.as_ref() {
            if *cached_key == key && computed_at.elapsed() < WORKTREES_META_TTL {
                return value.clone();
            }
        }
    }
    let value = compute();
    if let Ok(mut guard) = WORKTREES_META_CACHE.lock() {
        *guard = Some((key, std::time::Instant::now(), value.clone()));
    }
    value
}

fn build_worktrees(
    repo_root: &str,
    base_branch: &str,
    current_root: &str,
) -> Vec<WorktreeSnapshot> {
    let wts = er_engine::git::list_worktrees(repo_root).unwrap_or_default();
    let skip_merged = wts.len() > 10;

    let fingerprint = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for wt in &wts {
            wt.path.hash(&mut h);
            wt.branch.hash(&mut h);
        }
        h.finish()
    };
    let key = WorktreesMetaKey {
        repo_root: repo_root.to_string(),
        base_branch: base_branch.to_string(),
        fingerprint,
    };

    let meta = worktrees_meta_cached(key, || {
        wts.iter()
            .map(|wt| {
                (
                    wt.path.clone(),
                    detect_pr_meta(&wt.path, &wt.branch, base_branch, skip_merged),
                )
            })
            .collect()
    });

    wts.into_iter()
        .map(|wt| {
            let (is_pr, pr_number, is_merged) =
                meta.get(&wt.path).copied().unwrap_or((false, None, false));
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
    sorted.sort_by_key(|entry| std::cmp::Reverse(entry.saved_at_ms));
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
    sorted.sort_by_key(|entry| std::cmp::Reverse(entry.viewed_at_ms));
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
                    // already reviewed. Drafts are excluded — they aren't ready
                    // for review yet. Excluding PRs I've already approved or
                    // requested changes on keeps the list to what still needs my
                    // attention (GitHub clears the review request once I review,
                    // but the PR stays open).
                    let to_review: Vec<PrInfo> = all
                        .iter()
                        .filter(|pr| {
                            pr.state == "OPEN"
                                && !pr.is_draft
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
                    all.sort_by_key(|run| std::cmp::Reverse(run.merged_at.clone()));
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
                auto_triage: p.auto_triage,
                auto_triage_own_prs: p.auto_triage_own_prs,
                auto_triage_when: p.auto_triage_when.clone(),
                auto_triage_max_diff_kb: p.auto_triage_max_diff_kb,
                review_ignore_globs: p.review_ignore_globs.clone(),
                dismissed_prs: p.dismissed_prs.clone(),
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

/// Per-hunk old/new line counts (cheap; no allocation).
fn hunk_line_counts(hunk: &er_engine::git::DiffHunk) -> (usize, usize) {
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
    (old_count, new_count)
}

/// Build the comment/question threads for every hunk of a file — the cheap
/// part of `build_hunks`, split out so the differential-snapshot path can
/// fingerprint a file's wire content without serializing its lines.
fn build_hunk_threads(
    file: &DiffFile,
    tab: &TabState,
    pending: Option<&PendingAiReplies>,
) -> Vec<Vec<ThreadSnapshot>> {
    file.hunks
        .iter()
        .enumerate()
        .map(|(hunk_idx, hunk)| {
            let (old_count, new_count) = hunk_line_counts(hunk);
            // Collect threads for this hunk (also matches comments whose hunk_index is
            // missing or stale, by falling back to line-range matching)
            tab.ai
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
                        && !(matches!(c, CommentRef::Question(_) | CommentRef::Note(_))
                            && c.is_resolved())
                })
                .map(|c| comment_ref_to_thread(c, &file.path, hunk_idx, tab, pending))
                .collect()
        })
        .collect()
}

fn build_hunks(
    file: &DiffFile,
    tab: &TabState,
    pending: Option<&PendingAiReplies>,
) -> Vec<HunkSnapshot> {
    let threads_by_hunk = build_hunk_threads(file, tab, pending);
    file.hunks
        .iter()
        .zip(threads_by_hunk)
        .map(|(hunk, threads)| {
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

            let (old_count, new_count) = hunk_line_counts(hunk);

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
    let notes = ai.notes.as_ref().map(|n| n.notes.len()).unwrap_or(0);
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

    // Flat thread list for the Branch comments card and the Notes panel
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
                            origin: None,
                            source: None,
                            synced: None,
                            editable: None,
                            deletable: None,
                        },
                        replies: build_replies(&qref, tab, pending),
                        promoted_to: q.promoted_to.clone(),
                    });
                }
            }
        }
        if let Some(ns) = &ai.notes {
            for n in &ns.notes {
                if n.in_reply_to.is_none() && !n.resolved {
                    let nref = CommentRef::Note(n);
                    result.push(ThreadSnapshot {
                        id: n.id.clone(),
                        kind: "note".to_string(),
                        file: n.file.clone(),
                        line: n.line_start.unwrap_or(0),
                        line_end: n.line_end,
                        side: default_thread_side(),
                        source: "local".to_string(),
                        synced: false,
                        stale: n.stale,
                        resolved: n.resolved,
                        root: ThreadMessage {
                            id: n.id.clone(),
                            author: display_author(&n.author),
                            kind: "you".to_string(),
                            timestamp: n.timestamp.clone(),
                            body_markdown: n.text.clone(),
                            origin: None,
                            source: None,
                            synced: None,
                            editable: None,
                            deletable: None,
                        },
                        replies: build_replies(&nref, tab, pending),
                        promoted_to: n.promoted_to.clone(),
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
                            origin: None,
                            source: None,
                            synced: None,
                            editable: None,
                            deletable: None,
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
                    let mut responses: Vec<FindingResponseSnapshot> = f
                        .responses
                        .iter()
                        .map(|r| FindingResponseSnapshot {
                            id: r.id.clone(),
                            author: "AI".to_string(),
                            kind: "ai".to_string(),
                            timestamp: r.timestamp.clone(),
                            body_markdown: r.text.clone(),
                            origin: "finding_response".to_string(),
                            editable: false,
                            deletable: true,
                        })
                        .collect();
                    if let Some(pmap) = pending {
                        let pending_key = format!("finding:{}", f.id);
                        let is_pending = pmap
                            .lock()
                            .map(|g| g.contains_key(&pending_key))
                            .unwrap_or(false);
                        if is_pending {
                            responses.push(FindingResponseSnapshot {
                                id: String::new(),
                                author: "AI".to_string(),
                                kind: "ai".to_string(),
                                timestamp: String::new(),
                                body_markdown: "…thinking".to_string(),
                                origin: "finding_response".to_string(),
                                editable: false,
                                deletable: false,
                            });
                        }
                    }
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
                        responses,
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
        .map(er_engine::ai::count_eligible_github_comments)
        .unwrap_or(0);

    let triage = ai.triage.as_ref().map(|t| {
        let fresh = er_engine::ai::triage_is_fresh(t, &tab.branch_diff_hash);
        TriageSnapshot {
            fresh,
            first_impression: t.first_impression.clone(),
            verdict_primary: er_engine::ai::verdict_primary_str(&t.verdict.primary).to_string(),
            experts: t.verdict.experts.clone(),
            rationale: t.verdict.rationale.clone(),
            confidence: t.verdict.confidence.clone(),
            priority_files: t
                .priority_files
                .iter()
                .map(|pf| TriagePriorityFileSnapshot {
                    path: pf.path.clone(),
                    reason: pf.reason.clone(),
                    risk: pf.risk.clone(),
                })
                .collect(),
            files_changed: t.diff_stats.files_changed,
            approx_risk: t.diff_stats.approx_risk.clone(),
            domains: t.diff_stats.domains.clone(),
        }
    });

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
        notes,
        unpushed,
        threads,
        findings,
        has_review_json,
        eligible_comment_count,
        triage,
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

    #[test]
    fn compute_oid_staleness_rules() {
        // Equal oids → up to date.
        assert!(compute_oid_staleness(Some("abc"), Some("abc"), "base", "m").is_none());
        // Either side unknown → don't guess.
        assert!(compute_oid_staleness(None, Some("abc"), "base", "m").is_none());
        assert!(compute_oid_staleness(Some("abc"), None, "base", "m").is_none());
        // Empty strings are treated as unknown.
        assert!(compute_oid_staleness(Some(""), Some("abc"), "base", "m").is_none());
        assert!(compute_oid_staleness(Some("abc"), Some(""), "base", "m").is_none());
        // Differing non-empty oids → stale, carrying kind + message.
        let stale = compute_oid_staleness(
            Some("newsha"),
            Some("oldsha"),
            "base",
            "main has new commits on origin — Sync to refresh",
        )
        .expect("differing oids must be stale");
        assert_eq!(stale.kind, "base");
        assert_eq!(
            stale.message,
            "main has new commits on origin — Sync to refresh"
        );

        // PR-head kind threads through unchanged.
        let pr_stale = compute_oid_staleness(Some("head2"), Some("head1"), "pr_head", "msg")
            .expect("differing oids must be stale");
        assert_eq!(pr_stale.kind, "pr_head");
    }

    fn pr_with(number: u64, head_ref: &str, state: &str) -> PrInfo {
        let mut p = minimal_pr_info(number, "t");
        p.head_ref = head_ref.to_string();
        p.state = state.to_string();
        p
    }

    /// A local PR tab (pr_number set, no remote) must key its GitHub status off
    /// its OWN pr_number — even when the same head branch carries a second open
    /// PR. Regression test for the "Branch view shows another PR entirely" bug.
    #[test]
    fn github_key_prefers_local_pr_number_over_head_ref_collision() {
        let branch = "feature/shell-stores";
        // One head branch, two OPEN PRs (e.g. targeting different bases).
        let mut cache: HashMap<String, Vec<PrInfo>> = HashMap::new();
        cache.insert(
            "octo/cat".to_string(),
            vec![pr_with(1308, branch, "OPEN"), pr_with(117, branch, "OPEN")],
        );

        let mut tab = TabState::new_for_test(vec![]);
        tab.local_branch_view = Some(branch.to_string());
        tab.pr_number = Some(117);

        let key = resolve_github_status_key(&tab, &cache);
        assert_eq!(
            key,
            Some(("octo".to_string(), "cat".to_string(), 117)),
            "must resolve the opened PR (#117), not the head_ref collision (#1308)"
        );
    }

    /// Even if the tab's own PR isn't in the cache yet, the slug is recovered
    /// from any PR on the head branch and the tab's number is forced.
    #[test]
    fn github_key_recovers_slug_when_own_pr_absent_from_cache() {
        let branch = "feature/shell-stores";
        let mut cache: HashMap<String, Vec<PrInfo>> = HashMap::new();
        cache.insert("octo/cat".to_string(), vec![pr_with(1308, branch, "OPEN")]);

        let mut tab = TabState::new_for_test(vec![]);
        tab.local_branch_view = Some(branch.to_string());
        tab.pr_number = Some(117);

        assert_eq!(
            resolve_github_status_key(&tab, &cache),
            Some(("octo".to_string(), "cat".to_string(), 117))
        );
    }

    /// Remote PR tabs key straight off remote_repo + pr_number.
    #[test]
    fn github_key_remote_tab_uses_remote_repo() {
        let cache: HashMap<String, Vec<PrInfo>> = HashMap::new();
        let mut tab = TabState::new_for_test(vec![]);
        tab.remote_repo = Some("octo/cat".to_string());
        tab.pr_number = Some(42);

        assert_eq!(
            resolve_github_status_key(&tab, &cache),
            Some(("octo".to_string(), "cat".to_string(), 42))
        );
    }

    /// A plain branch tab (no pr_number) still matches by head_ref, preferring
    /// an OPEN PR over a closed one.
    #[test]
    fn github_key_branch_tab_matches_head_ref_preferring_open() {
        let branch = "feature/x";
        let mut cache: HashMap<String, Vec<PrInfo>> = HashMap::new();
        cache.insert(
            "octo/cat".to_string(),
            vec![pr_with(10, branch, "CLOSED"), pr_with(11, branch, "OPEN")],
        );

        let mut tab = TabState::new_for_test(vec![]);
        tab.local_branch_view = Some(branch.to_string());
        tab.pr_number = None;

        assert_eq!(
            resolve_github_status_key(&tab, &cache),
            Some(("octo".to_string(), "cat".to_string(), 11))
        );
    }

    fn commit_info(hash: &str, subject: &str) -> er_engine::git::CommitInfo {
        er_engine::git::CommitInfo {
            hash: hash.to_string(),
            short_hash: hash.chars().take(7).collect(),
            subject: subject.to_string(),
            author: "octo".to_string(),
            date: "2026-06-01T10:00:00Z".to_string(),
            relative_date: "2026-06-01T10:00:00Z".to_string(),
            file_count: 0,
            adds: 0,
            dels: 0,
            is_merge: false,
        }
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

    fn init_repo_with_feature_commit() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        run_git(root, &["init", "-b", "main"]);
        run_git(root, &["config", "user.email", "test@example.com"]);
        run_git(root, &["config", "user.name", "Test User"]);
        run_git(root, &["config", "commit.gpgsign", "false"]);
        std::fs::write(root.join("file.txt"), "base\n").unwrap();
        run_git(root, &["add", "file.txt"]);
        run_git(root, &["commit", "-m", "base"]);
        run_git(root, &["checkout", "-b", "feature"]);
        std::fs::write(root.join("file.txt"), "base\nfeature\n").unwrap();
        run_git(root, &["commit", "-am", "feature commit"]);
        tmp
    }

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
    fn build_file_snapshot_includes_hunks_only_when_requested() {
        let raw = "diff --git a/src/foo.rs b/src/foo.rs\nindex 0000000..1111111 100644\n--- a/src/foo.rs\n+++ b/src/foo.rs\n@@ -1,2 +1,3 @@\n fn foo() {}\n+fn bar() {}\n fn baz() {}\n";
        let files = er_engine::git::parse_diff(raw);
        assert_eq!(files.len(), 1, "fixture diff should parse one file");
        assert!(!files[0].hunks.is_empty(), "fixture file should have hunks");

        let tab = TabState::new_for_test(files);
        let f = &tab.files[0];

        // include_hunks = true: the lazy-load command path delivers the file's
        // content, so it is no longer a stub.
        let with = build_file_snapshot(0, f, &tab, None, true);
        assert!(
            !with.hunks.is_empty(),
            "requested file must carry its hunks"
        );
        assert!(!with.is_lazy_stub);
        assert_eq!(with.source_index, 0);

        // include_hunks = false (budget-omitted): hunks must NOT be serialized and
        // the file must read as a stub so the UI shows a loading state.
        let without = build_file_snapshot(0, f, &tab, None, false);
        assert!(
            without.hunks.is_empty(),
            "omitted file must not carry hunks"
        );
        assert!(without.is_lazy_stub);
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
    fn pr_commit_snapshot_uses_cached_pr_commits() {
        let mut tab = TabState::new_for_test(vec![]);
        tab.pr_number = Some(42);
        tab.local_branch_view = Some("feature".to_string());
        tab.pr_commits = vec![
            commit_info("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "newest"),
            commit_info("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "older"),
        ];

        let commits = build_commits_snapshot(&tab);

        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].sha, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        assert_eq!(commits[0].title, "newest");
        assert_eq!(commits[1].sha, "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
    }

    #[test]
    fn pr_commit_snapshot_does_not_fallback_to_local_head_history() {
        let tmp = init_repo_with_feature_commit();
        let mut tab = TabState::new_for_test(vec![]);
        tab.repo_root = tmp.path().to_string_lossy().to_string();
        tab.base_branch = "main".to_string();
        tab.current_branch = "feature".to_string();
        tab.local_branch_view = Some("feature".to_string());
        tab.pr_number = Some(42);
        tab.pr_commits = Vec::new();

        let commits = build_commits_snapshot(&tab);

        assert!(commits.is_empty());
    }

    // ── Part B: stale-pill gate for PR tabs ──

    /// Build an App whose single tab is a local PR (not remote) on the given
    /// mode, with `last_diff_head_oid` set, plus a pr_cache entry for that PR.
    fn pr_tab_app_with_cache(
        pr_number: u64,
        mode: DiffMode,
        last_diff_head_oid: Option<&str>,
        cached_head_oid: &str,
    ) -> (App, PrCache) {
        let mut app = App::new_for_test(vec![]);
        {
            let tab = app.tab_mut();
            tab.repo_root = "/tmp/test".to_string();
            tab.remote_repo = None;
            tab.pr_number = Some(pr_number);
            tab.local_branch_view = Some("feature".to_string());
            tab.mode = mode;
            tab.last_diff_head_oid = last_diff_head_oid.map(str::to_string);
        }
        let mut pr = minimal_pr_info(pr_number, "PR");
        pr.head_ref = "feature".to_string();
        pr.state = "OPEN".to_string();
        pr.head_oid = cached_head_oid.to_string();
        let pr_cache: PrCache = Arc::new(Mutex::new(HashMap::from([(
            "owner/repo".to_string(),
            vec![pr],
        )])));
        (app, pr_cache)
    }

    fn diff_stale_for(app: &App, pr_cache: &PrCache) -> Option<DiffStaleSnapshot> {
        build_snapshot_with_delta(
            app,
            Some(pr_cache),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .diff_stale
    }

    /// A local PR tab switched to Branch view (via set_mode) must still light the
    /// stale pill when the PR head advanced on origin. Before the gate was
    /// broadened it matched neither the PrDiff branch nor the
    /// `shows_branch_base_diff` fallback (which requires `pr_number.is_none()`).
    #[test]
    fn pr_tab_in_branch_view_lights_pill_when_head_advanced() {
        let (app, pr_cache) = pr_tab_app_with_cache(42, DiffMode::Branch, Some("head1"), "head2");

        let stale = diff_stale_for(&app, &pr_cache).expect("pill should be lit");
        assert_eq!(stale.kind, "pr_head");
        assert_eq!(stale.message, "PR head updated on origin — Sync to refresh");
    }

    /// Equal oids → no pill (the diff is already at the latest head).
    #[test]
    fn pr_tab_no_pill_when_head_matches() {
        let (app, pr_cache) = pr_tab_app_with_cache(42, DiffMode::Branch, Some("head1"), "head1");

        assert!(diff_stale_for(&app, &pr_cache).is_none());
    }

    /// pr_cache has no entry for this PR → None. Preserves the "don't guess"
    /// rule in `compute_oid_staleness` (an unknown latest oid never wedges stale).
    #[test]
    fn pr_tab_no_pill_when_cache_missing_pr() {
        let (app, _) = pr_tab_app_with_cache(42, DiffMode::Branch, Some("head1"), "head2");
        // pr_cache that only knows a *different* PR number.
        let mut other = minimal_pr_info(99, "other");
        other.head_oid = "headX".to_string();
        let pr_cache: PrCache = Arc::new(Mutex::new(HashMap::from([(
            "owner/repo".to_string(),
            vec![other],
        )])));

        assert!(diff_stale_for(&app, &pr_cache).is_none());
    }

    // ── Part D: pr_cache_fingerprint folds in head_oid ──

    /// A head_oid change alone (same PR count, same fetch timestamps) must move
    /// the fingerprint so a snapshot recompute fires — previously masked because
    /// the fingerprint hashed only count + fetched_at.
    #[test]
    fn pr_cache_fingerprint_changes_when_head_oid_changes() {
        let mut pr = minimal_pr_info(42, "PR");
        pr.head_oid = "head1".to_string();
        let cache_v1: PrCache = Arc::new(Mutex::new(HashMap::from([(
            "owner/repo".to_string(),
            vec![pr.clone()],
        )])));

        let mut pr2 = pr.clone();
        pr2.head_oid = "head2".to_string();
        let cache_v2: PrCache = Arc::new(Mutex::new(HashMap::from([(
            "owner/repo".to_string(),
            vec![pr2],
        )])));

        let fp_v1 = pr_cache_fingerprint(Some(&cache_v1), None);
        let fp_v2 = pr_cache_fingerprint(Some(&cache_v2), None);

        assert_ne!(
            fp_v1, fp_v2,
            "a head_oid change with constant PR count must change the fingerprint"
        );
        // Identical caches still fingerprint equal (no spurious churn).
        assert_eq!(fp_v1, pr_cache_fingerprint(Some(&cache_v1), None));
    }

    #[test]
    fn local_branch_commit_snapshot_still_uses_git_log_range() {
        let tmp = init_repo_with_feature_commit();
        let mut tab = TabState::new_for_test(vec![]);
        tab.repo_root = tmp.path().to_string_lossy().to_string();
        tab.base_branch = "main".to_string();
        tab.current_branch = "feature".to_string();
        tab.local_branch_view = Some("feature".to_string());

        let commits = build_commits_snapshot(&tab);

        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].title, "feature commit");
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
                auto_triage: false,
                auto_triage_own_prs: false,
                auto_triage_when: "new-and-push".to_string(),
                auto_triage_max_diff_kb: 0,
                review_ignore_globs: Vec::new(),
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

    #[test]
    fn projects_snapshot_includes_review_settings_fields() {
        let tab = TabState::new_for_test(vec![]);
        let file = projects::ProjectsFile {
            projects: vec![projects::ProjectRecord {
                id: "discovery".to_string(),
                name: "discovery".to_string(),
                root_path: "/tmp/discovery".to_string(),
                remote: Some("owner/discovery".to_string()),
                dismissed_prs: Vec::new(),
                tracked_prs: Vec::new(),
                tracked_branches: Vec::new(),
                dismissed_branches: Vec::new(),
                recent_prs: Vec::new(),
                saved_prs: Vec::new(),
                auto_triage: true,
                auto_triage_own_prs: true,
                auto_triage_when: "review-requested".to_string(),
                auto_triage_max_diff_kb: 512,
                review_ignore_globs: vec!["**/*.lock".to_string(), "dist/**".to_string()],
            }],
            active_id: None,
        };

        let projects = build_projects_from_file(&file, &tab, None, None, None, None);

        assert_eq!(projects.len(), 1);
        let p = &projects[0];
        assert!(p.auto_triage);
        assert!(p.auto_triage_own_prs);
        assert_eq!(p.auto_triage_when, "review-requested");
        assert_eq!(p.auto_triage_max_diff_kb, 512);
        assert_eq!(
            p.review_ignore_globs,
            vec!["**/*.lock".to_string(), "dist/**".to_string()]
        );
    }

    fn delta_snap(app: &App, sent: &SentFilesHandle) -> AppSnapshot {
        build_snapshot_with_delta(
            app,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(sent),
            None,
        )
    }

    const DELTA_FIXTURE_DIFF: &str = "diff --git a/src/foo.rs b/src/foo.rs\nindex 0000000..1111111 100644\n--- a/src/foo.rs\n+++ b/src/foo.rs\n@@ -1,2 +1,3 @@\n fn foo() {}\n+fn bar() {}\n fn baz() {}\n";
    const DELTA_FIXTURE_DIFF_V2: &str = "diff --git a/src/foo.rs b/src/foo.rs\nindex 0000000..2222222 100644\n--- a/src/foo.rs\n+++ b/src/foo.rs\n@@ -1,2 +1,4 @@\n fn foo() {}\n+fn bar() {}\n+fn qux() {}\n fn baz() {}\n";

    #[test]
    fn differential_snapshot_omits_unchanged_hunks() {
        let files = er_engine::git::parse_diff(DELTA_FIXTURE_DIFF);
        let app = er_engine::app::App::new_for_test(files);
        let sent: SentFilesHandle = Arc::new(Mutex::new(Default::default()));

        // First snapshot sends full hunks.
        let s1 = delta_snap(&app, &sent);
        let f1 = &s1.files[0];
        assert!(!f1.hunks_omitted, "first send must carry hunks");
        assert!(!f1.hunks.is_empty());

        // Unchanged content → hunks omitted, metadata + delta_key still present.
        let s2 = delta_snap(&app, &sent);
        let f2 = &s2.files[0];
        assert!(f2.hunks_omitted, "second send should omit unchanged hunks");
        assert!(f2.hunks.is_empty());
        assert!(!f2.is_lazy_stub, "omitted is not a lazy stub");
        assert_eq!(f2.delta_key, f1.delta_key);
        assert_eq!(f2.additions, f1.additions);
        assert_eq!(f2.cache_key, f1.cache_key);
    }

    #[test]
    fn differential_snapshot_resends_on_content_change() {
        let files = er_engine::git::parse_diff(DELTA_FIXTURE_DIFF);
        let mut app = er_engine::app::App::new_for_test(files);
        let sent: SentFilesHandle = Arc::new(Mutex::new(Default::default()));

        let s1 = delta_snap(&app, &sent);
        assert!(delta_snap(&app, &sent).files[0].hunks_omitted);

        // Diff content changed → full resend with a new delta_key, and the
        // per-file cache_key advances (highlights re-run for this file only).
        app.tab_mut().files = er_engine::git::parse_diff(DELTA_FIXTURE_DIFF_V2);
        let s3 = delta_snap(&app, &sent);
        let f3 = &s3.files[0];
        assert!(!f3.hunks_omitted, "changed file must be resent");
        assert!(!f3.hunks.is_empty());
        assert_ne!(f3.delta_key, s1.files[0].delta_key);
        assert_ne!(f3.cache_key, s1.files[0].cache_key);

        // And the new content is omitted again on the next poll.
        assert!(delta_snap(&app, &sent).files[0].hunks_omitted);
    }

    #[test]
    fn differential_snapshot_resends_when_threads_change() {
        let files = er_engine::git::parse_diff(DELTA_FIXTURE_DIFF);
        let mut app = er_engine::app::App::new_for_test(files);
        let sent: SentFilesHandle = Arc::new(Mutex::new(Default::default()));

        let s1 = delta_snap(&app, &sent);
        assert!(delta_snap(&app, &sent).files[0].hunks_omitted);

        // A new comment thread inside the hunk changes the wire payload even
        // though diff lines are identical — must be resent, not omitted.
        let mut comment = github_comment(false, false);
        comment.file = "src/foo.rs".to_string();
        comment.hunk_index = Some(0);
        comment.line_start = Some(2);
        app.tab_mut().ai.github_comments = Some(ErGitHubComments {
            version: 1,
            diff_hash: "hash".to_string(),
            github: None,
            comments: vec![comment],
        });

        let s3 = delta_snap(&app, &sent);
        let f3 = &s3.files[0];
        assert!(!f3.hunks_omitted, "thread change must resend hunks");
        assert!(
            f3.hunks.iter().any(|h| !h.threads.is_empty()),
            "resent hunks should carry the new thread"
        );
        assert_ne!(f3.delta_key, s1.files[0].delta_key);
        // Lines are unchanged, so the highlight cache key must NOT move.
        assert_eq!(f3.cache_key, s1.files[0].cache_key);
    }

    #[test]
    fn differential_snapshot_clears_on_view_change_and_reset() {
        let files = er_engine::git::parse_diff(DELTA_FIXTURE_DIFF);
        let mut app = er_engine::app::App::new_for_test(files);
        let sent: SentFilesHandle = Arc::new(Mutex::new(Default::default()));

        let _ = delta_snap(&app, &sent);
        assert!(delta_snap(&app, &sent).files[0].hunks_omitted);

        // View switch (mode change) busts the view token — full resend.
        app.tab_mut().mode = DiffMode::Unstaged;
        let s3 = delta_snap(&app, &sent);
        assert!(!s3.files[0].hunks_omitted, "view switch must resend hunks");
        assert!(delta_snap(&app, &sent).files[0].hunks_omitted);

        // Reset (frontend re-fetches from scratch) — full resend.
        sent.lock().unwrap().reset();
        assert!(!delta_snap(&app, &sent).files[0].hunks_omitted);
    }

    #[test]
    fn worktrees_meta_cache_recomputes_only_on_key_change() {
        use std::cell::Cell;

        let calls = Cell::new(0u32);
        let compute = || {
            calls.set(calls.get() + 1);
            let mut m = HashMap::new();
            m.insert("/wt".to_string(), (true, Some(7u64), false));
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
        assert_eq!(v1.get("/wt").copied(), Some((true, Some(7), false)));

        // Same key within TTL: cache hit → no recompute, same value.
        let v2 = worktrees_meta_cached(key(), compute);
        assert_eq!(
            calls.get(),
            1,
            "same worktree set within TTL must reuse cached meta, not respawn git subprocesses"
        );
        assert_eq!(v2.get("/wt").copied(), Some((true, Some(7), false)));

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
}
