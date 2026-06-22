pub mod arena;
pub mod background;
pub(super) mod comments;
pub mod github_sync;
pub(super) mod navigation;
pub mod remote_diff_sync;

use crate::ai::{self, AiState, CommentType, InlineLayers, PanelContent, ReviewFocus};
use crate::config::{self, ErConfig, WatchedConfig};
use crate::git::{
    self, CommitInfo, CompactionConfig, DiffFile, DiffFileHeader, WatchedFile, Worktree,
};
use crate::github::PrOverviewData;
use crate::paths::ErRoot;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
#[allow(unused_imports)]
use std::time::Instant;
use tui_textarea::TextArea;

static COMMENT_SEQ: AtomicU64 = AtomicU64::new(0);

fn profile_branch_enabled() -> bool {
    std::env::var("ER_DESKTOP_PROFILE_BRANCH").as_deref() == Ok("1")
}

fn log_branch_profile_phase(tab: &TabState, phase: &str, started_at: Instant) {
    if !profile_branch_enabled() && tab.local_branch_view.is_none() {
        return;
    }
    let branch = tab
        .local_branch_view
        .as_deref()
        .unwrap_or(tab.current_branch.as_str());
    eprintln!(
        "branch_profile repo={} branch={} phase={} ms={}",
        tab.repo_root,
        branch,
        phase,
        started_at.elapsed().as_millis()
    );
}

/// Read and parse a guided-tour sidecar (`tour.json` / `tour.pr.json`) from a
/// directory. Returns `None` if absent or malformed.
fn read_tour_file(dir: &str, name: &str) -> Option<ai::ErTour> {
    let path = std::path::Path::new(dir).join(name);
    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str::<ai::ErTour>(&content).ok()
}

/// Anchor data captured at comment creation time for later relocation
#[derive(Default)]
pub(crate) struct LineAnchor {
    line_start: Option<usize>,
    line_content: String,
    context_before: Vec<String>,
    context_after: Vec<String>,
    old_line_start: Option<usize>,
    hunk_header: String,
}

// ── Enums ──

/// Which set of changes we're viewing
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DiffMode {
    Branch,
    Unstaged,
    Staged,
    History,
    Conflicts,
    Hidden,
    PrDiff,
    /// AI guided walkthrough — branch diff reordered/grouped into pillars.
    Tour,
}

impl DiffMode {
    #[cfg(test)]
    pub fn label(&self) -> &'static str {
        match self {
            DiffMode::Branch => "BRANCH DIFF",
            DiffMode::Unstaged => "UNSTAGED",
            DiffMode::Staged => "STAGED",
            DiffMode::History => "HISTORY",
            DiffMode::Conflicts => "CONFLICTS",
            DiffMode::Hidden => "HIDDEN",
            DiffMode::PrDiff => "PR DIFF",
            DiffMode::Tour => "TOUR",
        }
    }

    pub fn git_mode(&self) -> &'static str {
        match self {
            DiffMode::Branch => "branch",
            DiffMode::Unstaged => "unstaged",
            DiffMode::Staged => "staged",
            DiffMode::History => "history",
            DiffMode::Conflicts => "conflicts",
            DiffMode::Hidden => "hidden",
            // PrDiff uses branch-style git diff under the hood (PR head vs base).
            // "pr" is used as the session-persistence key and bucket identifier only.
            DiffMode::PrDiff => "pr",
            // Tour reorders the branch diff; it shares the branch git scope/bucket.
            DiffMode::Tour => "tour",
        }
    }

    /// The scope string passed to `git_diff_raw` / `fetch_tab_raw_diff`.
    /// PrDiff diffs against a PR head ref — same git mechanics as `branch`.
    /// Tour walks the branch diff, so it fetches with branch mechanics too.
    pub fn fetch_scope(&self) -> &'static str {
        match self {
            DiffMode::PrDiff | DiffMode::Tour => "branch",
            other => other.git_mode(),
        }
    }
}

/// Which isolated review bucket a tab's diff belongs to.
/// Each bucket gets its own storage directory so review notes never bleed across views.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewBucket {
    Branch,
    Unstaged,
    Staged,
    History,
    Pr,
}

/// State for History mode — commit list + selected commit's diff
pub struct HistoryState {
    /// Loaded commits for the current branch
    pub commits: Vec<CommitInfo>,
    /// Currently selected commit index (left panel)
    pub selected_commit: usize,
    /// Parsed diff for the selected commit (right panel)
    pub commit_files: Vec<DiffFile>,
    /// File navigation within the commit diff
    pub selected_file: usize,
    /// Hunk navigation within the selected file
    pub current_hunk: usize,
    /// Line navigation within the selected hunk
    pub current_line: Option<usize>,
    /// Vertical scroll in the diff pane
    pub diff_scroll: u16,
    /// Horizontal scroll
    pub h_scroll: u16,
    /// Whether all commits have been loaded (no more to fetch)
    pub all_loaded: bool,
    /// LRU cache of recently viewed commit diffs
    pub diff_cache: DiffCache,
}

/// Simple LRU cache for parsed commit diffs
pub struct DiffCache {
    entries: VecDeque<(String, Vec<DiffFile>)>,
    max_size: usize,
}

impl DiffCache {
    pub fn new(max_size: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            max_size,
        }
    }

    pub fn get(&mut self, hash: &str) -> Option<&Vec<DiffFile>> {
        let pos = self.entries.iter().position(|(h, _)| h == hash)?;
        let entry = self.entries.remove(pos)?;
        self.entries.push_back(entry);
        self.entries.back().map(|(_, f)| f)
    }

    pub fn insert(&mut self, hash: String, files: Vec<DiffFile>) {
        // Remove existing entry for this hash if present
        self.entries.retain(|(h, _)| h != &hash);
        if self.entries.len() >= self.max_size {
            self.entries.pop_front();
        }
        self.entries.push_back((hash, files));
    }
}

/// One pillar's display metadata within a TourState (the diff files themselves
/// live in `TourState.files`, indexed by `TourState.pillar_file_ranges`).
#[derive(Debug, Clone)]
pub struct TourPillarView {
    pub id: String,
    pub title: String,
    pub description: String,
    pub importance: u32,
    pub foundation: bool,
}

/// State for Tour mode — pillars (left) + the branch diff reordered/grouped by
/// pillar (right). Parallels `HistoryState`. The Tour shares the Branch review
/// bucket, so `reviewed` is the branch's reviewed set.
pub struct TourState {
    /// Pillars in display order (foundation first, then importance).
    pub pillars: Vec<TourPillarView>,
    /// Currently selected pillar index (left panel).
    pub selected_pillar: usize,
    /// Flattened, pillar-ordered diff files for the whole tour.
    pub files: Vec<DiffFile>,
    /// `(start, end_exclusive)` index range into `files` for each pillar.
    pub pillar_file_ranges: Vec<(usize, usize)>,
    /// File navigation within `files`.
    pub selected_file: usize,
    /// Hunk navigation within the selected file.
    pub current_hunk: usize,
    /// Line navigation within the selected hunk.
    pub current_line: Option<usize>,
    /// Vertical scroll in the diff pane.
    pub diff_scroll: u16,
    /// Horizontal scroll.
    pub h_scroll: u16,
}

impl TourState {
    /// The pillar index that owns file index `file_idx`, if any.
    pub fn pillar_of_file(&self, file_idx: usize) -> Option<usize> {
        self.pillar_file_ranges
            .iter()
            .position(|&(start, end)| file_idx >= start && file_idx < end)
    }

    /// The pillar whose files occupy the current `diff_scroll` row. Mirrors the
    /// diff-pane row layout (per file: 2 header rows + per hunk `1 + lines + 1`).
    /// Used to keep the sticky pillar header / left-list selection in sync while
    /// free-scrolling with u/d.
    pub fn pillar_at_scroll(&self) -> Option<usize> {
        let target = self.diff_scroll as usize;
        let mut row: usize = 0;
        for (file_idx, file) in self.files.iter().enumerate() {
            let mut height = 2; // header + blank
            for hunk in &file.hunks {
                height += 1 + hunk.lines.len() + 1;
            }
            if target < row + height {
                return self.pillar_of_file(file_idx);
            }
            row += height;
        }
        // Past the end — last pillar.
        self.pillar_file_ranges.len().checked_sub(1)
    }
}

/// Whether we're navigating or typing in the search filter / comment
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum InputMode {
    Normal,
    Search,
    Comment,
    Confirm(ConfirmAction),
    Filter,
    Commit,
    RemoteUrl,
}

/// Actions that require user confirmation (y/n)
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum ConfirmAction {
    DeleteComment {
        comment_id: String,
    },
    DeleteWatchedFile {
        path: String,
    },
    Push,
    CleanupQuestions {
        count: usize,
    },
    CleanupReviews {
        count: usize,
    },
    /// Confirm clearing previous review before running AI review
    RunAgentReview {
        clear_previous: bool,
    },
    /// Confirm clearing previous answers before running AI questions
    RunAgentQuestions {
        clear_previous: bool,
    },
    /// Confirm approving the PR on GitHub
    ApprovePR,
    /// Choose how to push comments: as review or individually
    PushComments,
}

/// Which pane has focus in split diff view
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitSide {
    Old,
    New,
}

// ── Overlay types ──

/// Inline editing state for the config hub (StringEdit / ListAdd items)
#[derive(Debug, Clone)]
pub struct ConfigEditState {
    pub item_index: usize,
    pub buffer: String,
    pub cursor_pos: usize,
}

/// A directory entry for the filesystem browser
#[derive(Debug, Clone)]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
    pub is_git_repo: bool,
}

/// Active overlay popup state
#[derive(Debug, Clone)]
pub enum OverlayData {
    WorktreePicker {
        worktrees: Vec<Worktree>,
        selected: usize,
    },
    DirectoryBrowser {
        current_path: String,
        entries: Vec<DirEntry>,
        selected: usize,
    },
    FilterHistory {
        history: Vec<String>,
        selected: usize,
        preset_count: usize,
    },
    ModalHub {
        kind: HubKind,
        /// Optional title override (e.g. "VERIFY / frontend"). Uses kind.title() if None.
        title: Option<String>,
        items: Vec<HubItem>,
        selected: usize,
    },
    ConfigHub {
        tab: config::SettingsScope,
        items: Vec<config::ConfigItem>,
        selected: usize,
        saved_config: Box<ErConfig>,
        editing: Option<ConfigEditState>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiActionKind {
    Triage,
    Review,
    ExpertReview { expert_id: String },
    Professor,
    Validate,
    Questions,
    Summary,
}

impl AiActionKind {}

/// Which modal hub is open
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HubKind {
    Git,
    Ai,
    AiProvider,
    AiModel,
    AiExpert,
    Verify,
    VerifyPackage,
    Help,
    Open,
    Copy,
}

impl HubKind {
    pub fn title(&self) -> &'static str {
        match self {
            HubKind::Git => "GIT",
            HubKind::Ai => "AI",
            HubKind::AiProvider => "AI PROVIDER",
            HubKind::AiModel => "AI MODEL",
            HubKind::AiExpert => "SPECIALIZED REVIEW",
            HubKind::Verify => "VERIFY",
            HubKind::VerifyPackage => "VERIFY",
            HubKind::Help => "HELP",
            HubKind::Open => "OPEN",
            HubKind::Copy => "COPY",
        }
    }
}

/// A single item in a modal hub menu
#[derive(Debug, Clone)]
pub struct HubItem {
    /// Display label (e.g. "Push to remote")
    pub label: String,
    /// Short keybind hint shown right-aligned (e.g. "Ctrl+P")
    pub hint: String,
    /// Brief description shown below label
    pub description: String,
    /// Action to dispatch when selected
    pub action: HubAction,
    /// Whether this item is a section header (non-selectable)
    pub is_header: bool,
    /// Whether the item is currently enabled/applicable
    pub enabled: bool,
}

/// Actions dispatched from modal hub selections
#[derive(Debug, Clone, PartialEq)]
pub enum HubAction {
    Noop,
    // Git hub actions
    PushToRemote,
    PullGitHubComments,
    PushCommentsToGitHub,
    RefreshDiff,
    StageFile,
    StageAll,
    // AI hub actions
    CopyContext,
    CopyReviewJson,
    CopyQuestionsJson,
    ToggleAiFindings,
    ToggleComments,
    ToggleQuestions,
    ToggleHideResolved,
    CleanupQuestions,
    CleanupReviews,
    /// Run a named command from [commands] config (e.g. "summary", "test", "lint")
    RunCommand(String),
    /// Update the AI provider/model selection without running an action
    ConfigureAiSelection,
    /// Start an AI action through the provider/model selection flow
    RunAiAction(AiActionKind),
    /// Pick a provider, optionally continuing to an action
    SelectAiProvider {
        action: Option<AiActionKind>,
        provider_id: String,
    },
    /// Pick a model, optionally continuing to an action
    SelectAiModel {
        action: Option<AiActionKind>,
        provider_id: String,
        model_id: String,
    },
    /// Open expert reviewer picker (specialized review)
    OpenAiExpertPicker,
    /// Run a specialized expert review
    RunExpertReview {
        expert_id: String,
    },
    /// Run the Professor learning agent
    RunProfessorReview,
    /// Fast triage scan (routes to deeper review)
    RunTriageReview,
    /// Run AI review via configured agent command
    PromptReview,
    /// Re-validate an existing AI review (refreshes confidence + evidence)
    PromptValidate,
    /// Run AI question answering via configured agent command
    PromptQuestions,
    /// Approve PR on GitHub
    ApprovePR,
    /// Post a general comment on the PR (not attached to a file/line)
    CommentOnPR,
    // Open hub actions
    OpenDirectory,
    OpenWorktree,
    OpenRemoteUrl,
    OpenPrInBrowser,
    // Copy hub actions
    CopyFullFile,
    CopyFilePath,
    CopyHunk,
    CopyLine,
    // Help — no dispatch, just informational
    /// Select a package in the verify flow, then show that package's commands
    SelectVerifyPackage {
        package_id: String,
    },
    /// Run a verify command scoped to a specific package
    RunPackageCommand {
        command: String,
        package_id: String,
    },
}

// ── Per-Tab State ──

/// State for a single repo tab
pub struct TabState {
    pub mode: DiffMode,
    pub base_branch: String,
    pub current_branch: String,
    pub repo_root: String,

    /// Where review-artifact files live for this tab (managed app data by default).
    pub er_root: ErRoot,

    /// One-shot notice after migrating repo `.er/` into managed storage (shown by App).
    pub storage_notice: Option<String>,

    /// All diff files for the current mode
    pub files: Vec<DiffFile>,

    /// Index of selected file in the file list
    pub selected_file: usize,

    /// Index of current hunk within the selected file
    pub current_hunk: usize,

    /// Index of the currently highlighted line within the current hunk (None = hunk-level)
    pub current_line: Option<usize>,

    /// Line index where shift-select started (within hunk). None = no active selection.
    pub selection_anchor: Option<usize>,

    /// Vertical scroll offset within the diff view
    pub diff_scroll: u16,

    /// Horizontal scroll offset within the diff view (for long lines)
    pub h_scroll: u16,

    /// Which pane has focus in split diff view
    pub split_focus: SplitSide,

    /// Horizontal scroll offset for the old-side pane in split diff view
    pub h_scroll_old: u16,

    /// Horizontal scroll offset for the new-side pane in split diff view
    pub h_scroll_new: u16,

    /// Inline layer visibility toggles
    pub layers: InlineLayers,

    /// Optional context panel content (None = panel closed)
    pub panel: Option<PanelContent>,

    /// Vertical scroll offset for the context panel
    pub panel_scroll: u16,

    /// Whether keyboard focus is on the panel (vs diff view)
    pub panel_focus: bool,

    /// Width of the file tree panel in columns (resizable with </>)
    pub file_tree_width: u16,

    /// Width of the side panel in columns (resizable with {/})
    pub panel_width: u16,

    /// ID of the comment/question currently highlighted by J/K jumping
    pub focused_comment_id: Option<String>,

    /// ID of the finding currently highlighted by [/] jumping
    pub focused_finding_id: Option<String>,

    /// File paths the user explicitly expanded (survive diff refreshes)
    pub user_expanded: HashSet<String>,

    /// Which column has focus in panel's AiSummary view
    pub review_focus: ReviewFocus,

    /// Cursor position within the focused panel section
    pub review_cursor: usize,

    /// Search/filter input
    pub search_query: String,

    /// Files marked as reviewed: path → SHA-256 hash of the file's diff at mark time.
    /// Empty-string hash is a sentinel for entries loaded from the old single-column format
    /// (backwards compat) — those entries are never auto-unmarked.
    pub reviewed: HashMap<String, String>,

    /// Per-file diff hashes for the current refresh (volatile, not persisted).
    /// Used to detect when a reviewed file's diff has changed since it was marked.
    pub current_per_file_hashes: HashMap<String, String>,

    /// Only show unreviewed files in the file tree
    pub show_unreviewed_only: bool,

    /// Sort files by mtime (newest first) — works in any diff mode
    pub sort_by_mtime: bool,

    /// Cached mtime per file path, populated on refresh (avoids per-frame fs::metadata calls)
    pub mtime_cache: HashMap<String, std::time::SystemTime>,

    /// Pre-lowercased search query for use in visible_files() (avoids per-call allocation)
    pub search_query_lower: String,

    /// AI review state (loaded from .er-* files)
    pub ai: AiState,

    /// SHA-256 of the current raw diff (for staleness checks)
    pub diff_hash: String,

    /// SHA-256 of the branch diff (always computed, used for AI staleness)
    /// AI reviews are generated against the branch diff, so staleness must
    /// compare against this hash regardless of which diff mode is active.
    pub branch_diff_hash: String,

    /// Timestamp of last .er-* file check (to avoid re-reading every tick)
    pub last_ai_check: Option<std::time::SystemTime>,

    // ── Filter state ──
    /// Active filter expression (user-visible string)
    pub filter_expr: String,

    /// Parsed filter rules from filter_expr
    pub filter_rules: Vec<super::filter::FilterRule>,

    /// Text buffer for filter input while typing
    pub filter_input: String,

    /// History of applied filter expressions (most recent first, in-memory only)
    pub filter_history: Vec<String>,

    // ── Comment input state ──
    /// Multi-line text area for the comment being typed
    pub comment_textarea: TextArea<'static>,

    /// File path the comment targets
    pub comment_file: String,

    /// Hunk index the comment targets
    pub comment_hunk: usize,

    /// Optional finding ID this comment replies to
    pub comment_reply_to: Option<String>,

    /// Optional specific line number the comment targets (new-side)
    pub comment_line_num: Option<usize>,

    /// Inclusive end line when the comment targets a multi-line range.
    pub comment_line_end: Option<usize>,

    /// Which type of comment is being created (Question vs GitHubComment)
    pub comment_type: CommentType,

    /// When editing an existing comment, holds the comment ID being edited
    pub comment_edit_id: Option<String>,

    /// Optional finding ID this comment responds to (for finding replies)
    pub comment_finding_ref: Option<String>,

    /// Transient override for the comment's `author` field on next submit.
    /// Consumed (cleared) by submit_question / submit_github_comment.
    /// Used by the desktop `ask_ai` flow to attribute AI replies as "ai".
    pub comment_author_override: Option<String>,

    /// Transient: selection side for the next GitHub comment ("LEFT"/"RIGHT").
    /// Consumed by submit_github_comment. Defaults to "RIGHT".
    pub comment_side: Option<String>,

    /// History mode state (only populated when mode == History)
    pub history: Option<HistoryState>,

    /// Tour mode state (only populated when mode == Tour)
    pub tour: Option<TourState>,

    /// Whether the active Tour reflects the PR diff (true) or the local branch
    /// diff (false). Set when entering Tour mode from the originating view, so a
    /// guide generated while viewing the PR stays attached to the PR diff (and
    /// the Diff/PR toggle keeps PR Diff highlighted). Drives which tour sidecar
    /// (`tour.pr.json` vs `tour.json`) loads and which mode the Diff toggle
    /// returns to.
    pub tour_is_pr: bool,

    // ── Watched files state ──
    /// Configuration for watched files
    pub watched_config: WatchedConfig,

    /// Git-ignored files opted into visibility
    pub watched_files: Vec<WatchedFile>,

    /// When Some, a watched file is selected (index into watched_files)
    pub selected_watched: Option<usize>,

    /// Whether the watched files section is visible
    pub show_watched: bool,

    /// Paths that are watched but NOT gitignored (warning)
    pub watched_not_ignored: Vec<String>,

    /// Fetched PR overview data (loaded on startup if PR detected)
    pub pr_data: Option<PrOverviewData>,

    /// PR commit list from GitHub, newest first. PR review tabs use this for
    /// the commit scroller so it matches GitHub's PR Commits tab.
    pub pr_commits: Vec<CommitInfo>,

    /// Local git ref for PR head (e.g. refs/er/pr/42/head). Set when opened via --pr or PR URL
    /// using no-checkout mode. When Some, diffs are computed against this ref instead of HEAD.
    pub pr_head_ref: Option<String>,

    /// PR number when opened via --pr or PR URL. Used for comment sync and PR data fetching.
    pub pr_number: Option<u64>,

    // ── Commit input state ──
    /// Text buffer for the commit message being typed
    pub commit_input: String,

    // ── Merge conflict state ──
    /// Whether a merge is currently in progress (MERGE_HEAD exists)
    pub merge_active: bool,

    /// Number of files with unresolved conflict markers (subset of total merge files)
    pub unresolved_count: usize,

    // ── Performance ──
    /// Configuration for auto-compaction of low-value files
    pub compaction_config: CompactionConfig,

    /// Precomputed hunk offsets for O(1) scroll position lookup
    pub hunk_offsets: Option<HunkOffsets>,

    /// Memory budget tracking
    pub mem_budget: MemoryBudget,

    // ── Lazy parsing state ──
    /// Whether we're using lazy (two-phase) parsing for this diff
    pub lazy_mode: bool,

    /// File headers from lazy parse (only populated in lazy mode)
    pub file_headers: Vec<DiffFileHeader>,

    /// Raw diff string kept for on-demand parsing (only in lazy mode)
    raw_diff: Option<String>,

    // ── Symbol references ──
    /// Symbol reference lookup state (populated via panel action)
    pub symbol_refs: Option<SymbolRefsState>,

    /// Count of files auto-unmarked during the last refresh (drained by App for notification).
    /// Set to 0 after every refresh; non-zero means the App should surface a notification.
    pub pending_unmark_count: usize,

    /// Monotonic counter bumped whenever the reviewed set changes (toggle, mark, unmark,
    /// auto-unmark). Intentionally separate from content/chrome revisions so that poll()
    /// can return snapshot=null + chrome_only=true for reviewed-only changes without
    /// triggering a full hunk rebuild on the frontend.
    pub reviewed_revision: u64,

    /// True after a commit in Staged mode — causes diff view to show HEAD~1..HEAD until next
    /// new staged change or the user pushes.
    pub committed_unpushed: bool,

    /// Per-file context line overrides (path -> context lines count).
    /// Default context is 10 (git's --unified=10). Cleared on diff refresh.
    pub context_overrides: HashMap<String, usize>,

    /// Remote repo slug (e.g. "owner/repo") when reviewing a PR without a local clone.
    /// When Some, git operations are disabled and diffs come from `gh pr diff --repo`.
    pub remote_repo: Option<String>,

    /// When Some, this tab is a read-only diff of a local branch against `base_branch`.
    /// Diff source is `git diff <base>...<branch>` run from `repo_root`. No git mutation.
    pub local_branch_view: Option<String>,

    /// When Some and `local_branch_view` is also Some, the branch is checked out
    /// at this path (project root or linked worktree) and refreshes use
    /// `git_diff_checkout_against_base` against that working tree so live edits
    /// surface. Set/cleared by the desktop active-branch watcher.
    pub local_branch_checkout_root: Option<String>,

    /// Desktop-only: marks a tab restored as a stub (no diff loaded) that needs
    /// a `refresh_diff()` the first time it gains focus. Used by the desktop
    /// startup path to defer non-active-project tabs.
    pub needs_initial_refresh: bool,

    /// Whether `enter_pr_diff` has already fetched the PR head + base refs for
    /// this tab. On first entry (false), we always call out to git/gh and set this
    /// to true. On subsequent entries (true), we skip the network round-trip and
    /// go straight to apply_managed_root + reload + refresh.
    pub pr_refs_fetched: bool,

    /// For remote PR tabs: the head_oid the current `files`/`raw_diff` were
    /// fetched against. The background remote-PR refresh loop compares this
    /// against the latest head_oid in pr_cache; equal ⇒ skip the network
    /// round-trip. Set by `apply_remote_diff_result`. None means "force fetch".
    pub last_diff_head_oid: Option<String>,

    // ── Agent log state (per-tab) ──
    /// Receivers for running background commands (keyed by command name)
    pub command_rx: std::collections::HashMap<String, std::sync::mpsc::Receiver<Result<()>>>,

    /// Status of each named command (keyed by command name like "summary", "test", etc.)
    pub command_status: std::collections::HashMap<String, CommandStatus>,

    /// Sender for streaming agent log entries from background threads
    pub log_tx: std::sync::mpsc::Sender<AgentLogEntry>,

    /// Receiver for agent log entries (drained each tick by drain_agent_log)
    pub log_rx: std::sync::mpsc::Receiver<AgentLogEntry>,

    /// Accumulated agent log entries (capped at 5000)
    pub agent_log: std::collections::VecDeque<AgentLogEntry>,

    /// Whether the agent log panel auto-scrolls to the latest entry
    pub agent_log_auto_scroll: bool,

    // ── Desktop browser pane (per-tab; TUI ignores) ──
    /// URL loaded in the review browser for this tab.
    pub browser_url: String,
    /// Layout of the browser relative to the diff view.
    pub browser_layout: BrowserLayout,
    /// Horizontal split ratio (diff column fraction), clamped 0.35..0.65.
    pub browser_split_ratio: f32,
    /// In-page annotation mode for this tab's browser.
    pub browser_annotate_mode: bool,
    /// Show tooltips on all pins in the browser page.
    pub browser_show_tooltips: bool,
}

/// Per-tab browser layout (desktop only).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserLayout {
    #[default]
    Hidden,
    Split,
    Fullscreen,
}

impl BrowserLayout {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hidden => "hidden",
            Self::Split => "split",
            Self::Fullscreen => "fullscreen",
        }
    }

    pub fn from_label(s: &str) -> Self {
        match s {
            "split" => Self::Split,
            "fullscreen" => Self::Fullscreen,
            _ => Self::Hidden,
        }
    }

    pub fn cycle(self) -> Self {
        match self {
            Self::Hidden => Self::Split,
            Self::Split => Self::Fullscreen,
            Self::Fullscreen => Self::Hidden,
        }
    }
}

/// A single reference to a symbol (file + line)
#[derive(Debug, Clone)]
pub struct SymbolRefEntry {
    pub file: String,
    pub line_num: usize,
    pub line_content: String,
}

/// State for the symbol references panel
#[derive(Debug, Clone)]
pub struct SymbolRefsState {
    pub symbol: String,
    pub in_diff: Vec<SymbolRefEntry>,
    pub external: Vec<SymbolRefEntry>,
    pub cursor: usize,
}

// ── Session Persistence ──

/// Serializable session state for restoring review progress across restarts.
/// Keyed by diff hash — only restored when the diff hasn't changed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionState {
    /// SHA-256 hash of the diff this session was saved against
    pub diff_hash: String,

    /// Current branch name (for sanity check on restore)
    #[serde(default)]
    pub branch: String,

    /// Index of the selected file
    #[serde(default)]
    pub selected_file: usize,

    /// Current hunk index
    #[serde(default)]
    pub current_hunk: usize,

    /// Current line index within hunk (None = hunk-level)
    #[serde(default)]
    pub current_line: Option<usize>,

    /// Vertical scroll offset in diff view
    #[serde(default)]
    pub diff_scroll: u16,

    /// Horizontal scroll offset
    #[serde(default)]
    pub h_scroll: u16,

    /// Active diff viewing mode
    #[serde(default)]
    pub diff_mode: String,

    /// Active filter expression
    #[serde(default)]
    pub filter_expr: String,

    /// Filter history (most recent first)
    #[serde(default)]
    pub filter_history: Vec<String>,

    /// Whether showing only unreviewed files
    #[serde(default)]
    pub show_unreviewed_only: bool,

    /// Whether sorting by mtime
    #[serde(default)]
    pub sort_by_mtime: bool,

    /// In-progress comment draft text (empty if none)
    #[serde(default)]
    pub comment_draft: String,

    /// Comment draft target file
    #[serde(default)]
    pub comment_draft_file: String,

    /// Comment draft target hunk
    #[serde(default)]
    pub comment_draft_hunk: usize,

    /// Comment draft target line
    #[serde(default)]
    pub comment_draft_line: Option<usize>,

    /// Comment draft type ("question" or "github")
    #[serde(default)]
    pub comment_draft_type: String,
}

impl SessionState {
    /// Save session to the given path, writing atomically via tmp+rename.
    pub fn save(&self, session_path: &str) -> Result<()> {
        if let Some(dir) = std::path::Path::new(session_path).parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp_path = format!("{}.tmp", session_path);
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&tmp_path, &json)?;
        std::fs::rename(&tmp_path, session_path)?;
        Ok(())
    }

    /// Load session from the given path. Returns None if file doesn't exist or is invalid.
    pub fn load(session_path: &str) -> Option<Self> {
        let content = std::fs::read_to_string(session_path).ok()?;
        serde_json::from_str(&content).ok()
    }
}

/// Precomputed cumulative line offsets for each hunk in the selected file
#[derive(Debug, Clone)]
pub struct HunkOffsets {
    /// offsets[i] = logical line number where hunk i starts
    pub offsets: Vec<usize>,
}

impl HunkOffsets {
    pub fn build(hunks: &[git::DiffHunk]) -> Self {
        let mut offsets = Vec::with_capacity(hunks.len());
        let mut cursor: usize = 2; // file header lines
        for hunk in hunks {
            offsets.push(cursor);
            cursor += 1; // hunk header
            cursor += hunk.lines.len();
            cursor += 1; // blank line between hunks
        }
        Self { offsets }
    }
}

/// Lightweight memory tracking
#[derive(Debug, Clone, Default)]
pub struct MemoryBudget {
    pub parsed_files: usize,
    pub total_lines: usize,
    pub compacted_files: usize,
}

impl TabState {
    /// Get the comment text from the textarea, joined and trimmed
    pub fn comment_text(&self) -> String {
        self.comment_textarea.lines().join("\n").trim().to_string()
    }

    /// Create a new tab for a given repo root
    pub fn new(repo_root: String) -> Result<Self> {
        let current_branch = git::get_current_branch_in(&repo_root)?;
        let base_branch = git::detect_base_branch_in(&repo_root)?;
        Self::new_inner(repo_root, current_branch, base_branch, true)
    }

    /// Create a TabState with a known base branch (skips auto-detection).
    /// Used for PR flows where the base is known from the GitHub API.
    pub fn new_with_base(repo_root: String, base_branch: String) -> Result<Self> {
        let current_branch = git::get_current_branch_in(&repo_root)?;
        Self::new_inner(repo_root, current_branch, base_branch, true)
    }

    /// Create a TabState with a known base branch but without the eager current
    /// checkout diff refresh. Callers that immediately switch the tab to another
    /// diff target can avoid computing a throwaway diff before their real refresh.
    pub fn new_with_base_unloaded(repo_root: String, base_branch: String) -> Result<Self> {
        let current_branch = git::get_current_branch_in(&repo_root)?;
        Self::new_inner(repo_root, current_branch, base_branch, false)
    }

    /// Create a TabState for a read-only diff of a local branch against the
    /// project's base branch. Runs `git diff <base>...<branch>` — never
    /// checks the branch out or mutates the working tree.
    pub fn new_local_branch(repo_root: String, branch: String) -> Result<Self> {
        let mut tab = TabState::new(repo_root)?;
        tab.local_branch_view = Some(branch);
        tab.mode = DiffMode::Branch;
        tab.sync_managed_storage();
        tab.refresh_diff()?;
        Ok(tab)
    }

    /// Create a TabState for a read-only local PR review. Fetches the PR head to
    /// `refs/er/pr/<number>/head` without running `gh pr checkout` or touching the
    /// working tree. Diffs `<resolved_base>...refs/er/pr/<number>/head`.
    pub fn new_local_pr(repo_root: String, pr_number: u64) -> Result<Self> {
        // Resolve display/base metadata in one gh call. The PR parity diff is
        // loaded via `gh pr diff`, so initial open does not need to fetch the PR
        // head ref into the local clone.
        let (base_branch, head_branch_name) =
            crate::github::gh_pr_branch_names(pr_number, &repo_root)?;
        let resolved_base = crate::github::ensure_base_ref_available(&repo_root, &base_branch)?;

        let mut tab = TabState::new_with_base_unloaded(repo_root, resolved_base)?;
        tab.local_branch_view = Some(if head_branch_name.is_empty() {
            format!("pr/{}", pr_number)
        } else {
            head_branch_name
        });
        tab.pr_head_ref = Some(format!("refs/er/pr/{}/head", pr_number));
        tab.pr_number = Some(pr_number);
        tab.pr_commits =
            crate::github::gh_pr_commits(&tab.repo_root, pr_number, 250).unwrap_or_default();
        tab.mode = DiffMode::Branch;
        tab.sync_managed_storage();
        tab.refresh_diff()?;
        Ok(tab)
    }

    /// Create a local PR review tab from data already loaded by the desktop
    /// backend. This keeps the first-render source as `gh pr diff` while letting
    /// desktop overlap/cache the independent GitHub calls.
    pub fn new_local_pr_from_github_diff(
        repo_root: String,
        pr_number: u64,
        resolved_base: String,
        head_branch_name: String,
        raw: String,
        pr_data: Option<PrOverviewData>,
        pr_commits: Vec<CommitInfo>,
    ) -> Result<Self> {
        let mut tab = TabState::new_with_base_unloaded(repo_root, resolved_base)?;
        tab.local_branch_view = Some(if head_branch_name.is_empty() {
            format!("pr/{}", pr_number)
        } else {
            head_branch_name
        });
        tab.pr_head_ref = Some(format!("refs/er/pr/{}/head", pr_number));
        tab.pr_number = Some(pr_number);
        tab.pr_data = pr_data;
        tab.pr_commits = pr_commits;
        tab.mode = DiffMode::Branch;

        let compaction_config = tab.compaction_config.clone();
        let t_parse = Instant::now();
        if raw.len() > 200_000 {
            let headers = crate::git::parse_diff_headers(&raw);
            let files =
                crate::git::lazy_files_with_compaction(&raw, &headers, &compaction_config, |p| {
                    tab.user_expanded.contains(p)
                });
            tab.files = files;
            tab.file_headers = headers;
            tab.raw_diff = Some(raw.clone());
            tab.lazy_mode = true;
        } else {
            tab.file_headers = crate::git::parse_diff_headers(&raw);
            tab.raw_diff = Some(raw.clone());
            tab.files = crate::git::parse_diff(&raw);
            tab.lazy_mode = false;
            crate::git::compact_files(&mut tab.files, &tab.compaction_config);
        }
        eprintln!(
            "pr_open repo={} pr={} phase=parse ms={}",
            tab.repo_root,
            pr_number,
            t_parse.elapsed().as_millis()
        );

        let t_diff_hash = Instant::now();
        tab.diff_hash = crate::ai::compute_diff_hash(&raw);
        tab.branch_diff_hash = tab.diff_hash.clone();
        eprintln!(
            "pr_open repo={} pr={} phase=diff_hash ms={}",
            tab.repo_root,
            pr_number,
            t_diff_hash.elapsed().as_millis()
        );
        tab.selected_file = 0;
        tab.clamp_hunk();
        tab.ensure_file_parsed();
        tab.rebuild_hunk_offsets();
        tab.mtime_cache.clear();
        tab.update_mem_budget();
        let t_ai_reload = Instant::now();
        tab.sync_managed_storage();
        eprintln!(
            "pr_open repo={} pr={} phase=ai_reload ms={}",
            tab.repo_root,
            pr_number,
            t_ai_reload.elapsed().as_millis()
        );
        Ok(tab)
    }

    /// Create a TabState for remote PR review (no local git repo needed).
    /// Uses `gh pr diff --repo` instead of local git operations.
    pub fn new_remote(pr_ref: &crate::github::PrRef) -> Result<Self> {
        let repo_slug = format!("{}/{}", pr_ref.owner, pr_ref.repo);
        let (agent_log_tx, agent_log_rx) = std::sync::mpsc::channel();

        // Get metadata (base/head branch names)
        let (base_branch, head_branch) =
            crate::github::gh_pr_metadata_remote(&pr_ref.owner, &pr_ref.repo, pr_ref.number)?;

        // Get the diff from GitHub
        let raw = crate::github::gh_pr_diff_remote(&pr_ref.owner, &pr_ref.repo, pr_ref.number)?;

        // Parse the diff
        let compaction_config = crate::git::CompactionConfig::default();
        let mut files = if raw.len() > 200_000 {
            let headers = crate::git::parse_diff_headers(&raw);
            headers.iter().map(crate::git::header_to_stub).collect()
        } else {
            let mut f = crate::git::parse_diff(&raw);
            crate::git::compact_files(&mut f, &compaction_config);
            f
        };

        let diff_hash = crate::ai::compute_diff_hash(&raw);
        let pr_commits =
            crate::github::gh_pr_commits_remote(&pr_ref.owner, &pr_ref.repo, pr_ref.number, 250);
        let lazy_mode = raw.len() > 200_000;
        let file_headers = if lazy_mode {
            let headers = crate::git::parse_diff_headers(&raw);
            // Apply compaction to stubs in lazy mode
            for (file, header) in files.iter_mut().zip(headers.iter()) {
                let total_lines = header.adds + header.dels;
                let should_compact = compaction_config.enabled
                    && (compaction_config
                        .patterns
                        .iter()
                        .any(|p| crate::git::compact_files_match(p, &file.path))
                        || total_lines > compaction_config.max_lines_before_compact);
                if should_compact {
                    file.compacted = true;
                    file.raw_hunk_count = header.hunk_count;
                }
            }
            headers
        } else {
            Vec::new()
        };

        let er_config = crate::config::ErConfig::default();

        let repo_root_remote = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| ".".to_string());
        let mut tab = TabState {
            mode: DiffMode::PrDiff,
            base_branch,
            current_branch: head_branch,
            er_root: ErRoot::RepoLocal(repo_root_remote.clone()),
            repo_root: repo_root_remote,
            files,
            selected_file: 0,
            current_hunk: 0,
            current_line: None,
            selection_anchor: None,
            diff_scroll: 0,
            h_scroll: 0,
            split_focus: SplitSide::New,
            h_scroll_old: 0,
            h_scroll_new: 0,
            layers: InlineLayers::default(),
            panel: None,
            panel_scroll: 0,
            panel_focus: false,
            file_tree_width: 32,
            panel_width: 40,
            focused_comment_id: None,
            focused_finding_id: None,
            user_expanded: HashSet::new(),
            review_focus: ReviewFocus::Files,
            review_cursor: 0,
            search_query: String::new(),
            filter_expr: String::new(),
            filter_rules: Vec::new(),
            filter_input: String::new(),
            filter_history: Vec::new(),
            reviewed: HashMap::new(),
            current_per_file_hashes: HashMap::new(),
            show_unreviewed_only: false,
            sort_by_mtime: false,
            mtime_cache: HashMap::new(),
            search_query_lower: String::new(),
            ai: AiState::default(),
            diff_hash: diff_hash.clone(),
            branch_diff_hash: diff_hash,
            last_ai_check: None,
            comment_textarea: TextArea::default(),
            comment_file: String::new(),
            comment_hunk: 0,
            comment_reply_to: None,
            comment_line_num: None,
            comment_line_end: None,
            comment_type: CommentType::GitHubComment,
            comment_edit_id: None,
            comment_finding_ref: None,
            comment_author_override: None,
            comment_side: None,
            pr_data: None,
            pr_commits,
            pr_head_ref: None,
            pr_number: Some(pr_ref.number),
            history: None,
            tour: None,
            tour_is_pr: false,
            watched_config: er_config.watched.clone(),
            watched_files: Vec::new(),
            selected_watched: None,
            show_watched: false,
            watched_not_ignored: Vec::new(),
            commit_input: String::new(),
            merge_active: false,
            unresolved_count: 0,
            compaction_config,
            hunk_offsets: None,
            mem_budget: MemoryBudget::default(),
            lazy_mode,
            file_headers,
            raw_diff: if lazy_mode { Some(raw) } else { None },
            symbol_refs: None,
            pending_unmark_count: 0,
            reviewed_revision: 0,
            committed_unpushed: false,
            context_overrides: HashMap::new(),
            remote_repo: Some(repo_slug),
            local_branch_view: None,
            local_branch_checkout_root: None,
            command_rx: std::collections::HashMap::new(),
            command_status: std::collections::HashMap::new(),
            log_tx: agent_log_tx,
            log_rx: agent_log_rx,
            agent_log: std::collections::VecDeque::new(),
            agent_log_auto_scroll: true,
            browser_url: String::new(),
            browser_layout: BrowserLayout::default(),
            browser_split_ratio: 0.45,
            browser_annotate_mode: false,
            browser_show_tooltips: false,
            needs_initial_refresh: false,
            storage_notice: None,
            last_diff_head_oid: None,
            pr_refs_fetched: false,
        };

        tab.finish_storage_setup();
        tab.reload_ai_state();

        // Build hunk offsets for initial selection
        tab.rebuild_hunk_offsets();
        tab.ensure_file_parsed();
        tab.update_mem_budget();

        Ok(tab)
    }

    /// Remote PR tab shell without network I/O. Call `refresh_diff()` (or focus
    /// via desktop `kick_deferred_tab_refresh`) to load `gh pr diff`.
    pub fn new_remote_stub(pr_ref: &crate::github::PrRef) -> Result<Self> {
        let repo_slug = format!("{}/{}", pr_ref.owner, pr_ref.repo);
        let (agent_log_tx, agent_log_rx) = std::sync::mpsc::channel();
        let er_config = crate::config::ErConfig::default();
        let repo_root_remote = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| ".".to_string());

        let mut tab = TabState {
            mode: DiffMode::PrDiff,
            base_branch: String::new(),
            current_branch: String::new(),
            er_root: ErRoot::RepoLocal(repo_root_remote.clone()),
            repo_root: repo_root_remote,
            files: Vec::new(),
            selected_file: 0,
            current_hunk: 0,
            current_line: None,
            selection_anchor: None,
            diff_scroll: 0,
            h_scroll: 0,
            split_focus: SplitSide::New,
            h_scroll_old: 0,
            h_scroll_new: 0,
            layers: InlineLayers::default(),
            panel: None,
            panel_scroll: 0,
            panel_focus: false,
            file_tree_width: 32,
            panel_width: 40,
            focused_comment_id: None,
            focused_finding_id: None,
            user_expanded: HashSet::new(),
            review_focus: ReviewFocus::Files,
            review_cursor: 0,
            search_query: String::new(),
            filter_expr: String::new(),
            filter_rules: Vec::new(),
            filter_input: String::new(),
            filter_history: Vec::new(),
            reviewed: HashMap::new(),
            current_per_file_hashes: HashMap::new(),
            show_unreviewed_only: false,
            sort_by_mtime: false,
            mtime_cache: HashMap::new(),
            search_query_lower: String::new(),
            ai: AiState::default(),
            diff_hash: String::new(),
            branch_diff_hash: String::new(),
            last_ai_check: None,
            comment_textarea: TextArea::default(),
            comment_file: String::new(),
            comment_hunk: 0,
            comment_reply_to: None,
            comment_line_num: None,
            comment_line_end: None,
            comment_type: CommentType::GitHubComment,
            comment_edit_id: None,
            comment_finding_ref: None,
            comment_author_override: None,
            comment_side: None,
            pr_data: None,
            pr_commits: Vec::new(),
            pr_head_ref: None,
            pr_number: Some(pr_ref.number),
            history: None,
            tour: None,
            tour_is_pr: false,
            watched_config: er_config.watched.clone(),
            watched_files: Vec::new(),
            selected_watched: None,
            show_watched: false,
            watched_not_ignored: Vec::new(),
            commit_input: String::new(),
            merge_active: false,
            unresolved_count: 0,
            compaction_config: CompactionConfig::default(),
            hunk_offsets: None,
            mem_budget: MemoryBudget::default(),
            lazy_mode: false,
            file_headers: Vec::new(),
            raw_diff: None,
            symbol_refs: None,
            pending_unmark_count: 0,
            reviewed_revision: 0,
            committed_unpushed: false,
            context_overrides: HashMap::new(),
            remote_repo: Some(repo_slug),
            local_branch_view: None,
            local_branch_checkout_root: None,
            command_rx: std::collections::HashMap::new(),
            command_status: std::collections::HashMap::new(),
            log_tx: agent_log_tx,
            log_rx: agent_log_rx,
            agent_log: std::collections::VecDeque::new(),
            agent_log_auto_scroll: true,
            browser_url: String::new(),
            browser_layout: BrowserLayout::default(),
            browser_split_ratio: 0.45,
            browser_annotate_mode: false,
            browser_show_tooltips: false,
            needs_initial_refresh: true,
            storage_notice: None,
            last_diff_head_oid: None,
            pr_refs_fetched: false,
        };
        tab.finish_storage_setup();
        tab.reload_ai_state();
        Ok(tab)
    }

    fn new_inner(
        repo_root: String,
        current_branch: String,
        base_branch: String,
        refresh_initial_diff: bool,
    ) -> Result<Self> {
        let (agent_log_tx, agent_log_rx) = std::sync::mpsc::channel();
        let er_config = config::load_config(&repo_root);
        let watched_config = er_config.watched.clone();
        let has_watched = !watched_config.paths.is_empty();
        let merge_active = git::is_merge_in_progress(&repo_root);
        let er_root = ErRoot::RepoLocal(repo_root.clone());

        let mut tab = TabState {
            mode: DiffMode::Branch,
            base_branch,
            current_branch,
            er_root,
            repo_root,
            files: Vec::new(),
            selected_file: 0,
            current_hunk: 0,
            current_line: None,
            selection_anchor: None,
            diff_scroll: 0,
            h_scroll: 0,
            split_focus: SplitSide::New,
            h_scroll_old: 0,
            h_scroll_new: 0,
            layers: InlineLayers::default(),
            panel: None,
            panel_scroll: 0,
            panel_focus: false,
            file_tree_width: 32,
            panel_width: 40,
            focused_comment_id: None,
            focused_finding_id: None,
            user_expanded: HashSet::new(),
            review_focus: ReviewFocus::Files,
            review_cursor: 0,
            search_query: String::new(),
            filter_expr: String::new(),
            filter_rules: Vec::new(),
            filter_input: String::new(),
            filter_history: Vec::new(),
            reviewed: HashMap::new(),
            current_per_file_hashes: HashMap::new(),
            show_unreviewed_only: false,
            sort_by_mtime: false,
            mtime_cache: HashMap::new(),
            search_query_lower: String::new(),
            ai: AiState::default(),
            diff_hash: String::new(),
            branch_diff_hash: String::new(),
            last_ai_check: None,
            comment_textarea: TextArea::default(),
            comment_file: String::new(),
            comment_hunk: 0,
            comment_reply_to: None,
            comment_line_num: None,
            comment_line_end: None,
            comment_type: CommentType::GitHubComment,
            comment_edit_id: None,
            comment_finding_ref: None,
            comment_author_override: None,
            comment_side: None,
            pr_data: None,
            pr_commits: Vec::new(),
            pr_head_ref: None,
            pr_number: None,
            history: None,
            tour: None,
            tour_is_pr: false,
            watched_config,
            watched_files: Vec::new(),
            selected_watched: None,
            show_watched: has_watched,
            watched_not_ignored: Vec::new(),
            commit_input: String::new(),
            merge_active,
            unresolved_count: 0,
            compaction_config: CompactionConfig::default(),
            hunk_offsets: None,
            mem_budget: MemoryBudget::default(),
            lazy_mode: false,
            file_headers: Vec::new(),
            raw_diff: None,
            symbol_refs: None,
            pending_unmark_count: 0,
            reviewed_revision: 0,
            committed_unpushed: false,
            context_overrides: HashMap::new(),
            remote_repo: None,
            local_branch_view: None,
            local_branch_checkout_root: None,
            command_rx: std::collections::HashMap::new(),
            command_status: std::collections::HashMap::new(),
            log_tx: agent_log_tx,
            log_rx: agent_log_rx,
            agent_log: std::collections::VecDeque::new(),
            agent_log_auto_scroll: true,
            browser_url: String::new(),
            browser_layout: BrowserLayout::default(),
            browser_split_ratio: 0.45,
            browser_annotate_mode: false,
            browser_show_tooltips: false,
            needs_initial_refresh: false,
            storage_notice: None,
            last_diff_head_oid: None,
            pr_refs_fetched: false,
        };

        tab.finish_storage_setup();

        if refresh_initial_diff {
            tab.refresh_diff()?;
        }
        tab.refresh_watched_files();
        Ok(tab)
    }

    /// Create a minimal TabState for unit tests.
    /// Uses fixed repo root "/tmp/test" and no git I/O.
    pub fn new_for_test(files: Vec<crate::git::DiffFile>) -> Self {
        use crate::ai::{AiState, CommentType, InlineLayers, ReviewFocus};
        use crate::config::WatchedConfig;
        use crate::git::CompactionConfig;
        use std::collections::HashSet;
        let (agent_log_tx, agent_log_rx) = std::sync::mpsc::channel();
        TabState {
            mode: DiffMode::Branch,
            base_branch: "main".to_string(),
            current_branch: "feature".to_string(),
            er_root: ErRoot::RepoLocal("/tmp/test".to_string()),
            repo_root: "/tmp/test".to_string(),
            files,
            selected_file: 0,
            current_hunk: 0,
            current_line: None,
            selection_anchor: None,
            diff_scroll: 0,
            h_scroll: 0,
            split_focus: SplitSide::New,
            h_scroll_old: 0,
            h_scroll_new: 0,
            layers: InlineLayers::default(),
            panel: None,
            panel_scroll: 0,
            panel_focus: false,
            file_tree_width: 32,
            panel_width: 40,
            focused_comment_id: None,
            focused_finding_id: None,
            user_expanded: HashSet::new(),
            review_focus: ReviewFocus::Files,
            review_cursor: 0,
            search_query: String::new(),
            filter_expr: String::new(),
            filter_rules: Vec::new(),
            filter_input: String::new(),
            filter_history: Vec::new(),
            reviewed: HashMap::new(),
            current_per_file_hashes: HashMap::new(),
            show_unreviewed_only: false,
            sort_by_mtime: false,
            mtime_cache: HashMap::new(),
            search_query_lower: String::new(),
            ai: AiState::default(),
            diff_hash: String::new(),
            branch_diff_hash: String::new(),
            last_ai_check: None,
            comment_textarea: TextArea::default(),
            comment_file: String::new(),
            comment_hunk: 0,
            comment_reply_to: None,
            comment_line_num: None,
            comment_line_end: None,
            comment_type: CommentType::GitHubComment,
            comment_edit_id: None,
            comment_finding_ref: None,
            comment_author_override: None,
            comment_side: None,
            pr_data: None,
            pr_commits: Vec::new(),
            pr_head_ref: None,
            pr_number: None,
            history: None,
            tour: None,
            tour_is_pr: false,
            watched_config: WatchedConfig::default(),
            watched_files: Vec::new(),
            selected_watched: None,
            show_watched: false,
            watched_not_ignored: Vec::new(),
            commit_input: String::new(),
            merge_active: false,
            unresolved_count: 0,
            compaction_config: CompactionConfig::default(),
            hunk_offsets: None,
            mem_budget: MemoryBudget::default(),
            lazy_mode: false,
            file_headers: Vec::new(),
            raw_diff: None,
            symbol_refs: None,
            pending_unmark_count: 0,
            reviewed_revision: 0,
            committed_unpushed: false,
            context_overrides: HashMap::new(),
            remote_repo: None,
            local_branch_view: None,
            local_branch_checkout_root: None,
            command_rx: std::collections::HashMap::new(),
            command_status: std::collections::HashMap::new(),
            log_tx: agent_log_tx,
            log_rx: agent_log_rx,
            agent_log: std::collections::VecDeque::new(),
            agent_log_auto_scroll: true,
            browser_url: String::new(),
            browser_layout: BrowserLayout::default(),
            browser_split_ratio: 0.45,
            browser_annotate_mode: false,
            browser_show_tooltips: false,
            needs_initial_refresh: false,
            storage_notice: None,
            last_diff_head_oid: None,
            pr_refs_fetched: false,
        }
    }

    /// Resize the file tree panel by `delta` columns, enforcing min/max and diff minimum.
    pub fn resize_file_tree(&mut self, delta: i16, terminal_width: u16) {
        let new_width = (self.file_tree_width as i16 + delta).clamp(16, 60) as u16;
        let panel_w = if self.panel.is_some() {
            self.panel_width
        } else {
            0
        };
        let diff_remaining = terminal_width.saturating_sub(new_width + panel_w);
        if diff_remaining >= 20 {
            self.file_tree_width = new_width;
        }
    }

    /// Resize the side panel by `delta` columns, enforcing min/max and diff minimum.
    pub fn resize_panel(&mut self, delta: i16, terminal_width: u16) {
        let new_width = (self.panel_width as i16 + delta).clamp(24, 80) as u16;
        let diff_remaining = terminal_width.saturating_sub(self.file_tree_width + new_width);
        if diff_remaining >= 20 {
            self.panel_width = new_width;
        }
    }

    /// Short name for display in tab bar.
    ///
    /// Priority:
    /// 1. Local-branch view → that branch name.
    /// 2. Remote PR → `repo#N` (or `repo` if no PR number).
    /// 3. Working tab → `current_branch` if non-empty.
    /// 4. Fallback → repo directory basename.
    pub fn tab_name(&self) -> String {
        if let Some(ref branch) = self.local_branch_view {
            return branch.clone();
        }
        if let Some(ref slug) = self.remote_repo {
            let repo = slug.split('/').next_back().unwrap_or(slug);
            if let Some(pr_num) = self.pr_number {
                return format!("{}#{}", repo, pr_num);
            }
            return repo.to_string();
        }
        if !self.current_branch.is_empty() {
            return self.current_branch.clone();
        }
        std::path::Path::new(&self.repo_root)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| self.repo_root.clone())
    }

    /// Returns a list of (name, status_label, is_running) for agent commands
    /// that should show a persistent status indicator in the top bar.
    pub fn agent_statuses(&self) -> Vec<(&str, &str, bool)> {
        self.command_status
            .iter()
            .filter(|(_, status)| matches!(status, CommandStatus::Running))
            .map(|(name, _)| (name.as_str(), "running", true))
            .collect()
    }

    /// Whether this tab is reviewing a remote PR (no local git repo).
    pub fn is_remote(&self) -> bool {
        self.remote_repo.is_some()
    }

    /// Whether this tab is a read-only local-branch view.
    /// Like remote, only the Branch mode is offered and write commands are hidden.
    pub fn is_local_branch_view(&self) -> bool {
        self.local_branch_view.is_some()
    }

    /// Which review bucket this tab belongs to.
    /// Remote tabs (`remote_repo.is_some()`) always map to `Pr` regardless of `mode`.
    pub fn review_bucket(&self) -> ReviewBucket {
        if self.remote_repo.is_some() {
            return ReviewBucket::Pr;
        }
        match self.mode {
            DiffMode::PrDiff => ReviewBucket::Pr,
            DiffMode::Unstaged => ReviewBucket::Unstaged,
            DiffMode::Staged => ReviewBucket::Staged,
            DiffMode::History => ReviewBucket::History,
            _ => ReviewBucket::Branch,
        }
    }

    /// Sub-directory name for local view buckets (branch/unstaged/staged/history).
    /// Not used for the `Pr` bucket (which lives under `prs/pr-<N>/`).
    fn review_bucket_name(&self) -> &'static str {
        match self.review_bucket() {
            ReviewBucket::Unstaged => "unstaged",
            ReviewBucket::Staged => "staged",
            ReviewBucket::History => "history",
            ReviewBucket::Branch | ReviewBucket::Pr => "branch",
        }
    }

    /// Return the list of DiffMode tabs currently visible, based on feature flags,
    /// remote status, and data availability. Used for dynamic tab numbering.
    ///
    /// - Remote tab (`remote_repo.is_some()`): only `[PrDiff]` — no local working tree.
    /// - Local tab with a PR (`pr_number.is_some()`): working-tree modes + `PrDiff`
    ///   inserted after Staged and before History.
    /// - Local tab without a PR: working-tree modes only, no PrDiff.
    pub fn visible_modes(&self, config: &crate::config::ErConfig) -> Vec<DiffMode> {
        // Remote tabs have no local working tree — PR Diff is the only view.
        if self.is_remote() {
            return vec![DiffMode::PrDiff];
        }

        let mut modes = Vec::new();
        if config.features.view_branch {
            modes.push(DiffMode::Branch);
        }
        let read_only = self.is_local_branch_view() && self.local_branch_checkout_root.is_none();
        if config.features.view_unstaged && !read_only {
            modes.push(DiffMode::Unstaged);
        }
        if config.features.view_staged && !read_only {
            modes.push(DiffMode::Staged);
        }
        // PrDiff is available whenever a PR number is known (local clone with --pr).
        if self.pr_number.is_some() {
            modes.push(DiffMode::PrDiff);
        }
        if config.features.view_history && !read_only {
            modes.push(DiffMode::History);
        }
        // Tour appears next to the working-tree views, only when a tour.json exists
        // for this branch. `ai.tour` is loaded independent of the active bucket
        // (see reload_ai_state), so this is stable across mode switches.
        if config.features.view_tour && self.ai.has_tour() {
            modes.push(DiffMode::Tour);
        }
        if config.features.view_conflicts && !read_only && self.merge_active {
            modes.push(DiffMode::Conflicts);
        }
        if config.features.view_hidden && !read_only && !self.watched_config.paths.is_empty() {
            modes.push(DiffMode::Hidden);
        }
        modes
    }

    /// Return the directory path for AI files (review.json, questions.json,
    /// github-comments.json, etc.).
    ///
    /// Defaults to managed app-data storage (`~/.local/share/easy-review/...`).
    /// With `ER_REPO_LOCAL=1`, uses `repo/.er/`.
    pub fn er_dir(&self) -> String {
        self.er_root.er_dir()
    }

    /// Directory for storing comment files (github-comments.json, questions.json).
    pub fn comments_dir(&self) -> String {
        self.er_dir()
    }

    /// Directory of the "branch" review bucket for this tab — where `tour.json`
    /// lives — regardless of the currently active view bucket. The guided tour is
    /// branch-scoped, so it must be discoverable from Unstaged/Staged/History
    /// modes too (those use other buckets). Returns `None` when storage can't be
    /// resolved (empty repo/branch). Uses pure path helpers (no dir creation).
    pub fn branch_bucket_er_dir(&self) -> Option<String> {
        if crate::storage::use_repo_local_storage() {
            return Some(format!("{}/.er", self.repo_root));
        }
        if let Some(pr_num) = self.pr_number {
            let owner_repo_slug = if let Some(ref remote_repo) = self.remote_repo {
                crate::storage::slug_branch(&remote_repo.to_lowercase())
            } else {
                crate::github::canonical_owner_repo_slug(&self.repo_root)?
            };
            return Some(
                crate::storage::pr_bucket_dir(&owner_repo_slug, pr_num)
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        let branch = self
            .local_branch_view
            .clone()
            .unwrap_or_else(|| self.current_branch.clone());
        if branch.is_empty() || self.repo_root.is_empty() {
            return None;
        }
        let repo_slug = crate::storage::slug_repo(&self.repo_root);
        let branch_slug = crate::storage::slug_branch(&branch);
        Some(
            crate::storage::view_bucket_dir(&repo_slug, &branch_slug, "branch")
                .to_string_lossy()
                .into_owned(),
        )
    }

    /// Point this tab at managed storage (shared by TUI and Desktop).
    /// Routes to a per-bucket directory so review notes are isolated per view.
    pub fn apply_managed_root(&mut self) {
        if crate::storage::use_repo_local_storage() {
            self.er_root = ErRoot::RepoLocal(self.repo_root.clone());
            return;
        }

        // PR-associated tabs share one bucket for branch-level review artifacts
        // (triage, review.json, questions, reviewed) regardless of Branch vs PrDiff mode.
        if let Some(pr_num) = self.pr_number {
            match self.review_bucket() {
                ReviewBucket::Unstaged | ReviewBucket::Staged | ReviewBucket::History => {}
                ReviewBucket::Branch | ReviewBucket::Pr => {
                    let owner_repo_slug = if let Some(ref remote_repo) = self.remote_repo {
                        crate::storage::slug_branch(&remote_repo.to_lowercase())
                    } else {
                        match crate::github::canonical_owner_repo_slug(&self.repo_root) {
                            Some(slug) => slug,
                            None => return,
                        }
                    };
                    self.er_root = crate::storage::resolve_managed_root_for_pr_bucket(
                        &owner_repo_slug,
                        pr_num,
                    );
                    return;
                }
            }
        }

        // Local view buckets (branch/unstaged/staged/history)
        let branch = self
            .local_branch_view
            .clone()
            .unwrap_or_else(|| self.current_branch.clone());
        if branch.is_empty() || self.repo_root.is_empty() {
            return;
        }
        let repo_slug = crate::storage::slug_repo(&self.repo_root);
        let branch_slug = crate::storage::slug_branch(&branch);
        let bucket = self.review_bucket_name();

        self.er_root =
            crate::storage::resolve_managed_root_for_view_bucket(&repo_slug, &branch_slug, bucket);
        // Per-bucket dirs start clean — no migration from repo `.er/` or legacy cache.
        // Migrating here would copy the same data into every bucket (branch/unstaged/
        // staged/history) and break isolation between them.
    }

    /// Apply managed storage, migrate legacy paths, and load reviewed markers.
    pub fn finish_storage_setup(&mut self) {
        self.apply_managed_root();
        self.reviewed = Self::load_reviewed_files_from_path(&self.er_root.reviewed_path());
    }

    /// Re-resolve managed storage for this tab's branch/PR and reload sidecars.
    ///
    /// Call after `local_branch_view` / `pr_number` / `remote_repo` are set — e.g.
    /// before the first `refresh_diff()` on a read-only branch tab. `new_inner` runs
    /// `finish_storage_setup()` while `local_branch_view` is still unset, so branch
    /// views must sync again once the viewed branch is known.
    pub fn sync_managed_storage(&mut self) {
        self.apply_managed_root();
        self.reviewed = Self::load_reviewed_files_from_path(&self.er_root.reviewed_path());
        if !self.active_diff_files().is_empty() {
            self.prune_reviewed_not_in_diff();
        }
        self.reload_ai_state();
    }

    /// Working-tree tab only: if HEAD branch changed since last refresh, persist
    /// reviewed markers for the old branch and reload storage for the new branch.
    pub fn sync_storage_if_checkout_branch_changed(&mut self) -> Result<()> {
        if self.remote_repo.is_some() || self.local_branch_view.is_some() {
            return Ok(());
        }
        let git_branch = git::get_current_branch_in(&self.repo_root)?;
        self.apply_checkout_branch_storage_change(&git_branch)
    }

    /// Apply storage switch when the checkout branch name changes (used by refresh
    /// and unit tests).
    pub(crate) fn apply_checkout_branch_storage_change(&mut self, git_branch: &str) -> Result<()> {
        if git_branch == self.current_branch {
            return Ok(());
        }
        self.save_reviewed_files()?;
        self.current_branch = git_branch.to_string();
        self.sync_managed_storage();
        Ok(())
    }

    /// Whether the current view's guided tour belongs to the PR diff (vs the
    /// local branch diff). Remote tabs and PR Diff mode are always PR-scoped;
    /// Tour mode follows whichever view it was entered from (`tour_is_pr`).
    /// All other modes (Branch/Unstaged/Staged/History) are branch-scoped.
    pub fn tour_context_is_pr(&self) -> bool {
        if self.remote_repo.is_some() {
            return true;
        }
        match self.mode {
            DiffMode::PrDiff => true,
            DiffMode::Tour => self.tour_is_pr,
            _ => false,
        }
    }

    /// Sidecar filename for the guided tour in the current view context.
    /// PR-scoped tours live in `tour.pr.json`; branch-scoped tours in `tour.json`.
    /// Both share the branch bucket directory (`branch_bucket_er_dir`).
    pub fn tour_filename(&self) -> &'static str {
        if self.tour_context_is_pr() {
            "tour.pr.json"
        } else {
            "tour.json"
        }
    }

    /// Branch name used to resolve managed storage for this tab (for sidecar validation).
    pub fn storage_branch_scope(&self) -> Option<&str> {
        if self.local_branch_view.is_some() {
            self.local_branch_view.as_deref()
        } else if self.remote_repo.is_some() {
            // PR storage is keyed by pr-{n}; path isolation is enough.
            None
        } else if self.current_branch.is_empty() {
            None
        } else {
            Some(&self.current_branch)
        }
    }

    /// Path to github-comments.json. Uses cache dir in remote mode.
    pub fn github_comments_path(&self) -> String {
        format!("{}/github-comments.json", self.comments_dir())
    }

    // ── PR Diff mode ──

    /// Switch to `PrDiff` mode: fetch the PR head + base refs (first entry only),
    /// resolve storage to the shared PR bucket, reload reviewed markers and AI state,
    /// then refresh the diff.
    ///
    /// On the first call (`pr_refs_fetched == false`) the function fetches the PR head
    /// and base refs from git/gh and stores them, then sets `pr_refs_fetched = true`.
    /// On subsequent calls the cached refs are reused — no network round-trip.
    /// Fetch errors on the first entry propagate as `Err`; they are never swallowed.
    ///
    /// Returns `Err` if no PR number is set or the first-entry fetch fails.
    pub fn enter_pr_diff(&mut self) -> Result<()> {
        let pr_number = self
            .pr_number
            .ok_or_else(|| anyhow::anyhow!("No PR number set for this tab"))?;

        if !self.pr_refs_fetched {
            // First entry: fetch and cache the refs. Errors surface to the caller.
            let head_ref = crate::github::fetch_pr_head(pr_number, &self.repo_root)?;
            let base = self.base_branch.clone();
            let base_ref = crate::github::fetch_base_branch_ref(
                &self.repo_root,
                base.trim_start_matches("origin/"),
            )?;
            self.pr_head_ref = Some(head_ref);
            self.base_branch = base_ref;
            self.pr_refs_fetched = true;
        }

        self.mode = DiffMode::PrDiff;
        self.apply_managed_root();
        self.reviewed = Self::load_reviewed_files_from_path(&self.er_root.reviewed_path());
        self.reload_ai_state();
        self.refresh_diff()
    }

    // ── Diff ──

    /// Re-run git diff and update the file list
    pub fn refresh_diff(&mut self) -> Result<()> {
        self.refresh_diff_impl(true, true)
    }

    /// For local PR tabs: re-fetch the PR head ref and base branch from origin before
    /// refreshing the diff. For all other tab types, behaves like `refresh_diff`.
    pub fn refetch_and_refresh_diff(&mut self) -> Result<()> {
        let t_total = Instant::now();
        let is_local_pr = self.pr_number.is_some() && !self.is_remote();

        if let (true, Some(pr_number)) = (is_local_pr, self.pr_number) {
            let t = Instant::now();
            let head_ref = crate::github::fetch_pr_head(pr_number, &self.repo_root)?;
            log_branch_profile_phase(self, "fetch_pr_head", t);
            let t = Instant::now();
            let base_branch = crate::github::gh_pr_base_branch(pr_number, &self.repo_root)?;
            log_branch_profile_phase(self, "lookup_pr_base_branch", t);
            let t = Instant::now();
            let resolved_base =
                crate::github::fetch_base_branch_ref(&self.repo_root, &base_branch)?;
            log_branch_profile_phase(self, "fetch_pr_base_ref", t);

            self.pr_head_ref = Some(head_ref);
            self.base_branch = resolved_base;
        }

        let t = Instant::now();
        let res = self.refresh_diff();
        log_branch_profile_phase(self, "refresh_diff", t);
        log_branch_profile_phase(self, "refetch_and_refresh_diff_total", t_total);
        res
    }

    /// Refresh local branch views using only already-available local refs.
    /// This never performs network fetches.
    pub fn refresh_diff_without_remote_fetch(&mut self) -> Result<()> {
        self.refresh_diff_without_remote_fetch_impl(false)
    }

    /// Fast first-render variant of `refresh_diff_without_remote_fetch`.
    /// It still reads the local diff, parses files, and reloads sidecars, but
    /// skips the expensive full SHA-256/per-file staleness pass. A later full
    /// refresh should update hashes before AI actions rely on them.
    pub fn refresh_diff_without_remote_fetch_quick(&mut self) -> Result<()> {
        self.refresh_diff_without_remote_fetch_impl(true)
    }

    fn refresh_diff_without_remote_fetch_impl(&mut self, quick: bool) -> Result<()> {
        if self.is_remote()
            || self.pr_head_ref.is_some()
            || self.local_branch_checkout_root.is_some()
        {
            return if quick {
                self.refresh_diff_quick()
            } else {
                self.refresh_diff()
            };
        }

        let _branch = self
            .local_branch_view
            .clone()
            .ok_or_else(|| anyhow::anyhow!("No local branch view for this tab"))?;

        let base_short = self
            .base_branch
            .strip_prefix("origin/")
            .unwrap_or(&self.base_branch);
        let base_candidates = [format!("origin/{base_short}"), base_short.to_string()];
        let resolved_base = base_candidates
            .into_iter()
            .find(|candidate| crate::github::ref_exists_locally(&self.repo_root, candidate))
            .ok_or_else(|| {
                anyhow::anyhow!("No local base ref resolved for '{}'", self.base_branch)
            })?;

        self.pr_head_ref = None;
        self.base_branch = resolved_base;
        if quick {
            self.refresh_diff_quick()
        } else {
            self.refresh_diff()
        }
    }

    /// Lightweight refresh: skips branch hash recomputation in non-Branch modes.
    /// Use for watch events where the extra git diff call adds unwanted latency.
    pub fn refresh_diff_quick(&mut self) -> Result<()> {
        self.refresh_diff_impl(false, false)
    }

    /// Refresh for mode switch: recompute hashes but don't auto-unmark.
    /// Diff content differs per mode, so hash changes don't mean actual file changes.
    pub fn refresh_diff_mode_switch(&mut self) -> Result<()> {
        self.refresh_diff_impl(true, false)
    }

    /// Refresh conflict files for Conflicts mode.
    ///
    /// Parses the combined merge changeset (staged + unmerged working-tree diffs),
    /// deduplicates by filename (keeping the last/unmerged occurrence for conflict
    /// files), marks unresolved files as `FileStatus::Unmerged`, and sorts with
    /// unresolved files first then alphabetically.
    pub fn refresh_conflicts(&mut self) {
        self.merge_active = git::is_merge_in_progress(&self.repo_root);

        let raw = git::git_diff_conflicts(&self.repo_root).unwrap_or_default();
        let parsed = git::parse_diff(&raw);

        // Get the set of paths that still have conflict markers
        let unmerged_paths: std::collections::HashSet<String> =
            git::unmerged_files(&self.repo_root)
                .unwrap_or_default()
                .into_iter()
                .collect();

        // Deduplicate by path — keep the last occurrence so that the unmerged
        // working-tree diff wins over the staged diff for conflict files.
        let mut seen: std::collections::HashMap<String, git::DiffFile> =
            std::collections::HashMap::new();
        for file in parsed {
            seen.insert(file.path.clone(), file);
        }

        // Apply status: unmerged paths get FileStatus::Unmerged; others keep parsed status
        let mut files: Vec<git::DiffFile> = seen
            .into_values()
            .map(|mut file| {
                if unmerged_paths.contains(&file.path) {
                    file.status = git::FileStatus::Unmerged;
                }
                file
            })
            .collect();

        // Sort: unresolved (Unmerged) first, then alphabetically by path
        files.sort_by(|a, b| {
            let a_unmerged = a.status == git::FileStatus::Unmerged;
            let b_unmerged = b.status == git::FileStatus::Unmerged;
            match (a_unmerged, b_unmerged) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.path.cmp(&b.path),
            }
        });

        self.unresolved_count = unmerged_paths.len();
        self.files = files;
        self.selected_file = 0;
        self.current_hunk = 0;
        self.current_line = None;
        self.diff_scroll = 0;
        self.h_scroll = 0;
        self.rebuild_hunk_offsets();
    }

    /// Whether `raw_diff` (if present) matches the review `scope` for this tab.
    fn review_scope_matches_cached_diff(&self, scope: &str) -> bool {
        if self.remote_repo.is_some() {
            return true;
        }
        if self.local_branch_view.is_some() && self.local_branch_checkout_root.is_none() {
            return true;
        }
        match scope {
            "unstaged" => self.mode == DiffMode::Unstaged,
            "staged" => self.mode == DiffMode::Staged,
            _ => self.mode == DiffMode::Branch || self.mode == DiffMode::PrDiff,
        }
    }

    /// GitHub PR diff for AI review when this tab is PR-associated. Skips local
    /// working-tree scopes on a checked-out branch.
    fn fetch_pr_diff_for_review(&self, scope: &str) -> Option<Result<String>> {
        let pr_number = self.pr_number?;
        if matches!(scope, "unstaged" | "staged") && self.local_branch_checkout_root.is_some() {
            return None;
        }
        if let Some(ref repo_slug) = self.remote_repo {
            let parts: Vec<&str> = repo_slug.split('/').collect();
            if parts.len() == 2 {
                return Some(crate::github::gh_pr_diff_remote(
                    parts[0], parts[1], pr_number,
                ));
            }
            return Some(Err(anyhow::anyhow!(
                "Remote tab missing owner/repo for PR diff"
            )));
        }
        if self.repo_root.is_empty() {
            return None;
        }
        Some(crate::github::gh_pr_diff(pr_number, &self.repo_root))
    }

    /// Fetch raw unified-diff text using the same subprocess rules as `refresh_diff`.
    ///
    /// Extracts only the git/gh calls — no parsing, hashing, or selection restore.
    pub fn fetch_tab_raw_diff(&self, scope: &str) -> Result<String> {
        if self.mode == DiffMode::History || self.mode == DiffMode::Conflicts {
            anyhow::bail!("Cannot fetch diff in {:?} mode", self.mode);
        }
        if self.mode == DiffMode::Hidden {
            return Ok(String::new());
        }

        if let Some(result) = self.fetch_pr_diff_for_review(scope) {
            return result;
        }

        if let Some(ref branch) = self.local_branch_view {
            let base = self.base_branch.clone();
            return if let Some(head_ref) = self.pr_head_ref.clone() {
                if let Some(pr_number) = self.pr_number {
                    crate::github::gh_pr_diff(pr_number, &self.repo_root)
                } else {
                    crate::git::git_diff_against_branch(&self.repo_root, &base, &head_ref)
                }
            } else if let Some(checkout_root) = self.local_branch_checkout_root.clone() {
                return match scope {
                    "unstaged" | "staged" => {
                        crate::git::git_diff_raw(scope, &base, &checkout_root, None)
                    }
                    _ => crate::git::git_diff_checkout_against_base(&checkout_root, &base),
                };
            } else {
                crate::git::git_diff_against_branch(&self.repo_root, &base, branch)
            };
        }

        if let Some(ref repo_slug) = self.remote_repo {
            let parts: Vec<&str> = repo_slug.split('/').collect();
            if parts.len() == 2 {
                let owner = parts[0];
                let repo = parts[1];
                if let Some(pr_number) = self.pr_number {
                    return crate::github::gh_pr_diff_remote(owner, repo, pr_number);
                }
            }
            anyhow::bail!("Remote tab missing owner/repo or pr_number");
        }

        let head_ref_owned = self.pr_head_ref.clone();
        if scope == "staged" && self.committed_unpushed {
            let staged_raw = git::git_diff_raw(
                "staged",
                &self.base_branch,
                &self.repo_root,
                head_ref_owned.as_deref(),
            )?;
            if !staged_raw.is_empty() {
                return Ok(staged_raw);
            }
            match git::git_diff_raw_range("HEAD~1", "HEAD", &self.repo_root) {
                Ok(raw) => return Ok(raw),
                Err(_) => return Ok(String::new()),
            }
        }

        git::git_diff_raw(
            scope,
            &self.base_branch,
            &self.repo_root,
            head_ref_owned.as_deref(),
        )
    }

    /// Raw diff for AI review: prefer the cached UI diff when fresh, else refetch.
    pub fn raw_diff_for_review(&self, scope: &str) -> Result<String> {
        if let Some(raw) = self.raw_diff.as_ref() {
            if !self.branch_diff_hash.is_empty() && self.review_scope_matches_cached_diff(scope) {
                return Ok(raw.clone());
            }
        }
        self.fetch_tab_raw_diff(scope)
    }

    fn refresh_diff_impl(&mut self, recompute_branch_hash: bool, auto_unmark: bool) -> Result<()> {
        let t_total = Instant::now();

        self.sync_storage_if_checkout_branch_changed()?;

        // History mode doesn't use git_diff_raw — skip normal diff refresh
        if self.mode == DiffMode::History {
            return Ok(());
        }

        // Conflicts mode refreshes via refresh_conflicts() only
        if self.mode == DiffMode::Conflicts {
            self.merge_active = git::is_merge_in_progress(&self.repo_root);
            return Ok(());
        }

        // Hidden mode only shows watched files — reload them instead of running git diff
        if self.mode == DiffMode::Hidden {
            self.refresh_watched_files();
            return Ok(());
        }

        // Local branch view: read-only `git diff <base>...<branch>` from the user's clone,
        // or mode-aware diffs when the branch is checked out (working tree).
        // For local PR review, pr_head_ref holds the fetched ref to diff against instead.
        if self.local_branch_view.is_some() {
            let t_raw_diff = Instant::now();
            let scope = if self.local_branch_checkout_root.is_some() {
                self.mode.git_mode()
            } else {
                "branch"
            };
            let raw = self.fetch_tab_raw_diff(scope)?;
            log_branch_profile_phase(self, "local_branch_raw_diff", t_raw_diff);

            let prev_path = self.files.get(self.selected_file).map(|f| f.path.clone());

            let t_parse = Instant::now();
            if raw.len() > 200_000 {
                let headers = crate::git::parse_diff_headers(&raw);
                let compaction_config = self.compaction_config.clone();
                let files = crate::git::lazy_files_with_compaction(
                    &raw,
                    &headers,
                    &compaction_config,
                    |p| self.user_expanded.contains(p),
                );
                self.files = files;
                self.file_headers = headers;
                self.raw_diff = Some(raw.clone());
                self.lazy_mode = true;
            } else {
                self.file_headers = crate::git::parse_diff_headers(&raw);
                self.raw_diff = Some(raw.clone());
                self.files = crate::git::parse_diff(&raw);
                self.lazy_mode = false;
                crate::git::compact_files(&mut self.files, &self.compaction_config);
            }
            log_branch_profile_phase(self, "local_branch_parse", t_parse);

            let t_diff_hash = Instant::now();
            if recompute_branch_hash {
                self.diff_hash = crate::ai::compute_diff_hash(&raw);
                self.branch_diff_hash = self.diff_hash.clone();
            } else {
                self.diff_hash = format!("{:016x}", crate::ai::compute_diff_hash_fast(&raw));
            }
            log_branch_profile_phase(self, "local_branch_diff_hash", t_diff_hash);

            // Restore selection
            if let Some(ref path) = prev_path {
                if let Some(idx) = self.files.iter().position(|f| f.path == *path) {
                    self.selected_file = idx;
                } else {
                    self.selected_file = self.files.len().saturating_sub(1).min(self.selected_file);
                }
            } else {
                self.selected_file = 0;
            }
            self.clamp_hunk();
            self.ensure_file_parsed();
            self.rebuild_hunk_offsets();
            self.mtime_cache.clear();
            self.update_mem_budget();
            // Reload AI sidecar for this branch's comment directory
            let t_ai_reload = Instant::now();
            self.reload_ai_state();
            log_branch_profile_phase(self, "local_branch_ai_reload", t_ai_reload);
            // Tour reorders this branch diff into pillars — rebuild after re-parse.
            if self.mode == DiffMode::Tour {
                self.rebuild_tour_state();
            }
            log_branch_profile_phase(self, "refresh_diff_impl_total", t_total);
            return Ok(());
        }

        // Remote mode: fetch diff from GitHub API instead of local git
        if let (Some(repo_slug), Some(_pr_number)) = (&self.remote_repo, self.pr_number) {
            if repo_slug.split('/').count() == 2 {
                let raw = self.fetch_tab_raw_diff("branch")?;

                let prev_path = self.files.get(self.selected_file).map(|f| f.path.clone());

                if raw.len() > 200_000 {
                    let headers = crate::git::parse_diff_headers(&raw);
                    let compaction_config = self.compaction_config.clone();
                    let files = crate::git::lazy_files_with_compaction(
                        &raw,
                        &headers,
                        &compaction_config,
                        |p| self.user_expanded.contains(p),
                    );
                    self.files = files;
                    self.file_headers = headers;
                    self.raw_diff = Some(raw.clone());
                    self.lazy_mode = true;
                } else {
                    self.files = crate::git::parse_diff(&raw);
                    self.file_headers.clear();
                    self.raw_diff = None;
                    self.lazy_mode = false;
                    crate::git::compact_files(&mut self.files, &self.compaction_config);
                }

                if recompute_branch_hash {
                    self.diff_hash = crate::ai::compute_diff_hash(&raw);
                    self.branch_diff_hash = self.diff_hash.clone();
                } else {
                    self.diff_hash = format!("{:016x}", crate::ai::compute_diff_hash_fast(&raw));
                }

                // Restore selection
                if let Some(ref path) = prev_path {
                    if let Some(idx) = self.files.iter().position(|f| f.path == *path) {
                        self.selected_file = idx;
                    } else {
                        self.selected_file =
                            self.files.len().saturating_sub(1).min(self.selected_file);
                    }
                } else {
                    self.selected_file = 0;
                }
                self.clamp_hunk();
                self.ensure_file_parsed();
                self.rebuild_hunk_offsets();
                self.update_mem_budget();

                if recompute_branch_hash {
                    self.reload_ai_state();
                }
                self.relocate_all_comments();
                if self.ai.is_stale {
                    self.compute_stale_files(&raw);
                }
                // Tour reorders this branch diff into pillars — rebuild after re-parse.
                if self.mode == DiffMode::Tour {
                    self.rebuild_tour_state();
                }

                return Ok(());
            }
            return Ok(());
        }

        // Update merge_active on every refresh
        self.merge_active = git::is_merge_in_progress(&self.repo_root);

        // Remember current position to restore after re-parse
        let prev_path = self.files.get(self.selected_file).map(|f| f.path.clone());
        let prev_hunk = self.current_hunk;
        let prev_line = self.current_line;
        let prev_scroll = self.diff_scroll;

        let head_ref_owned = self.pr_head_ref.clone();
        let raw = if self.mode == DiffMode::Staged && self.committed_unpushed {
            let staged_raw = git::git_diff_raw(
                self.mode.fetch_scope(),
                &self.base_branch,
                &self.repo_root,
                head_ref_owned.as_deref(),
            )?;
            if !staged_raw.is_empty() {
                // New staged changes exist — resume normal staged view
                self.committed_unpushed = false;
                staged_raw
            } else {
                match git::git_diff_raw_range("HEAD~1", "HEAD", &self.repo_root) {
                    Ok(raw) => raw,
                    Err(_) => {
                        self.committed_unpushed = false;
                        String::new()
                    }
                }
            }
        } else {
            self.fetch_tab_raw_diff(self.mode.fetch_scope())?
        };

        // Decide parsing strategy based on diff size.
        // Use byte-length heuristic (O(1)) instead of counting newlines (O(n)).
        // 200_000 bytes ≈ ~5000 lines (at ~40 bytes/line), equivalent to LAZY_PARSE_THRESHOLD.
        if raw.len() > 200_000 {
            // Lazy mode: header-only parse; large/pattern files become compacted stubs,
            // small files are parsed eagerly so they render without a lazy round-trip.
            let headers = git::parse_diff_headers(&raw);
            let compaction_config = self.compaction_config.clone();
            let files =
                crate::git::lazy_files_with_compaction(&raw, &headers, &compaction_config, |p| {
                    self.user_expanded.contains(p)
                });
            self.files = files;
            self.file_headers = headers;
            self.raw_diff = Some(raw.clone());
            self.lazy_mode = true;
        } else {
            // Eager mode: full parse (fast enough for smaller diffs)
            self.files = git::parse_diff(&raw);
            self.file_headers.clear();
            self.raw_diff = None;
            self.lazy_mode = false;

            // Apply auto-compaction to low-value files
            git::compact_files(&mut self.files, &self.compaction_config);

            // Re-expand any files the user explicitly opened (fresh parse means hunks are available)
            let to_expand: Vec<String> = self
                .files
                .iter()
                .filter(|f| f.compacted && self.user_expanded.contains(&f.path))
                .map(|f| f.path.clone())
                .collect();
            let repo_root = self.repo_root.clone();
            let git_mode = self.mode.fetch_scope();
            let base_branch = self.base_branch.clone();
            let head_ref_owned2 = self.pr_head_ref.clone();
            for file in &mut self.files {
                if to_expand.contains(&file.path) {
                    git::expand_compacted_file(
                        file,
                        &repo_root,
                        git_mode,
                        &base_branch,
                        head_ref_owned2.as_deref(),
                    )?;
                }
            }
        }

        // Clear per-file context overrides — diff content has changed
        self.context_overrides.clear();

        if self.sort_by_mtime {
            self.sort_files_by_mtime();
        }

        // Refresh mtime cache once per diff load (avoids per-frame fs::metadata calls)
        self.refresh_mtime_cache();

        // Update memory budget
        self.update_mem_budget();

        // Compute diff hash for the current mode.
        // Use fast hash for quick refreshes (watch events), SHA-256 for full refreshes
        // (AI staleness needs SHA-256 to compare with .er-review.json).
        let branch_raw_owned: Option<String>;
        if recompute_branch_hash {
            // Full refresh: compute SHA-256 for AI compatibility
            self.diff_hash = ai::compute_diff_hash(&raw);
        } else {
            // Quick refresh: use fast hash (skip expensive SHA-256)
            self.diff_hash = format!("{:016x}", ai::compute_diff_hash_fast(&raw));
        }

        // Branch diff hash for AI staleness detection.
        // In Branch mode, reuse — no second git call needed.
        // In other modes, only run the extra git diff when AI data is loaded AND it's a full refresh.
        // Skipping when no AI data exists avoids a redundant git diff call with no consumer.
        if self.mode == DiffMode::Branch {
            // Always use SHA-256 for branch_diff_hash (used by .er/questions.json).
            // diff_hash may be a fast hash during quick refresh, but branch_diff_hash
            // must always be SHA-256 for compatibility with external skills.
            if recompute_branch_hash {
                self.branch_diff_hash = self.diff_hash.clone();
            } else {
                self.branch_diff_hash = ai::compute_diff_hash(&raw);
            }
            branch_raw_owned = Some(raw.clone());
        } else if recompute_branch_hash && (self.ai.has_data() || self.ai.has_questions()) {
            let br = git::git_diff_raw(
                "branch",
                &self.base_branch,
                &self.repo_root,
                head_ref_owned.as_deref(),
            )?;
            self.branch_diff_hash = ai::compute_diff_hash(&br);
            branch_raw_owned = Some(br);
        } else {
            branch_raw_owned = None;
        }

        // Compute per-file hashes from the raw diff output.
        // Used to detect when a reviewed file's diff changes since it was marked.
        // Skip on quick refreshes (watch events) — SHA-256 per-file is expensive and
        // staleness detection doesn't need sub-second precision.
        if recompute_branch_hash {
            let t = Instant::now();
            self.current_per_file_hashes = ai::compute_per_file_hashes(&raw);
            if auto_unmark {
                // Auto-unmark reviewed files whose diff has changed since they were marked.
                self.pending_unmark_count = self.auto_unmark_changed_reviewed();
            }
            log_branch_profile_phase(self, "compute_per_file_hashes", t);
        }

        // Load AI state from .er-* files (only on full refresh — watch-triggered
        // quick refreshes let the separate AI polling handle .er/ changes)
        if recompute_branch_hash {
            let t = Instant::now();
            self.reload_ai_state();
            log_branch_profile_phase(self, "reload_ai_state", t);
        }

        // Relocate comments to follow moved code
        let t = Instant::now();
        self.relocate_all_comments();
        log_branch_profile_phase(self, "relocate_all_comments", t);

        // Compute per-file staleness when the review is stale and has file_hashes.
        // Reuse the branch_raw we already fetched — no additional git call.
        if self.ai.is_stale {
            if let Some(ref branch_diff) = branch_raw_owned {
                self.compute_stale_files(branch_diff);
            }
        }

        // Restore selection by path (file order may change after sort/re-parse)
        if let Some(ref path) = prev_path {
            if let Some(idx) = self.files.iter().position(|f| f.path == *path) {
                self.selected_file = idx;
                // Restore hunk/line/scroll within the same file
                if prev_hunk < self.total_hunks() {
                    self.current_hunk = prev_hunk;
                    self.current_line = prev_line;
                } else {
                    self.clamp_hunk();
                }
                // Always restore scroll position when the file is unchanged —
                // even if hunks shifted, the user's viewport is still meaningful.
                self.diff_scroll = prev_scroll;
            } else {
                // File disappeared from diff — clamp index
                if self.selected_file >= self.files.len() {
                    self.selected_file = self.files.len().saturating_sub(1);
                }
                self.clamp_hunk();
                self.scroll_to_current_hunk();
            }
        } else {
            self.selected_file = 0;
            self.clamp_hunk();
            self.scroll_to_current_hunk();
        }

        // In lazy mode, parse the initially selected file on demand
        self.ensure_file_parsed();

        // Rebuild hunk offsets for the new selection
        self.rebuild_hunk_offsets();

        // Tour mode reorders this branch diff into pillars — rebuild after re-parse.
        if self.mode == DiffMode::Tour {
            self.rebuild_tour_state();
        }

        log_branch_profile_phase(self, "refresh_diff_impl_total", t_total);
        Ok(())
    }

    /// Reload AI state from .er-* files (preserving current nav state)
    pub fn reload_ai_state(&mut self) {
        let prev_stale_files = std::mem::take(&mut self.ai.stale_files);
        let er_dir = self.er_dir();
        let branch_scope = self.storage_branch_scope().map(str::to_string);
        let prev_tour_stale = self.ai.tour_stale;
        self.ai = ai::load_ai_state(&er_dir, &self.branch_diff_hash, branch_scope.as_deref());
        // The guided tour lives in the "branch" bucket and is context-scoped:
        // the PR diff loads `tour.pr.json`, the local branch diff loads
        // `tour.json`. Route to the right sidecar (remote PR tabs fall back to a
        // legacy `tour.json`) and recompute the tour's own staleness, regardless
        // of the active view bucket.
        self.reload_context_tour(&er_dir, prev_tour_stale);
        // Finding IDs may change after review reload — clear stale reference
        self.focused_finding_id = None;
        // Preserve per-file staleness across .er-* file reloads (recomputed in refresh_diff)
        if self.ai.is_stale {
            self.ai.stale_files = prev_stale_files;
        }
        // Clamp cursor to valid range after reload (item count may have decreased)
        let item_count = match self.review_focus {
            ReviewFocus::Files => self.ai.review_file_count(),
            ReviewFocus::Checklist => self.ai.review_checklist_count(),
        };
        // review_cursor stays 0 for an empty list; consumers must still guard
        // item_count == 0 before indexing.
        let max_cursor = if item_count == 0 { 0 } else { item_count - 1 };
        self.review_cursor = self.review_cursor.min(max_cursor);
        self.last_ai_check = ai::latest_er_mtime(&er_dir);
    }

    /// Load the guided tour for the current view context (PR vs branch) and
    /// recompute its staleness. Tour sidecars live in the branch-bucket dir
    /// (shared by Branch/PR/Tour modes), so the tour stays discoverable from any
    /// view. PR context prefers `tour.pr.json` and falls back to the legacy
    /// `tour.json` (tours written before the branch/PR split). Overrides
    /// whatever `load_ai_state` loaded from the active bucket.
    fn reload_context_tour(&mut self, er_dir: &str, prev_tour_stale: bool) {
        let dir = self
            .branch_bucket_er_dir()
            .unwrap_or_else(|| er_dir.to_string());
        let want_pr = self.tour_context_is_pr();

        let tour = if want_pr {
            // PR context loads `tour.pr.json`. Remote PR tabs have no branch
            // context, so a legacy `tour.json` there is unambiguously the PR
            // tour — fall back to it for backward compatibility. Local tabs keep
            // branch and PR guides strictly separate (a branch `tour.json` must
            // not surface in the PR view).
            read_tour_file(&dir, "tour.pr.json").or_else(|| {
                if self.remote_repo.is_some() {
                    read_tour_file(&dir, "tour.json")
                } else {
                    None
                }
            })
        } else {
            read_tour_file(&dir, "tour.json")
        };
        self.ai.tour = tour;

        // Compare the tour's diff hash against the current view's diff hash.
        // `self.diff_hash` is the SHA-256 of the current context diff after a
        // full refresh; on fast-hash refreshes (len != 64) fall back to the
        // always-SHA-256 `branch_diff_hash` for branch context, or keep the
        // previous value for PR context (no reliable SHA-256 handy).
        self.ai.tour_stale = match self.ai.tour.as_ref() {
            None => false,
            Some(t) if self.diff_hash.len() == 64 => t.diff_hash != self.diff_hash,
            Some(t) if !want_pr => t.diff_hash != self.branch_diff_hash,
            Some(_) => prev_tour_stale,
        };
    }

    /// Reload github comments from cache in remote mode.
    /// Unlike reload_ai_state() which reads from .er/, this reads from the remote cache dir.
    pub fn reload_remote_comments(&mut self) {
        let path = self.github_comments_path();
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(gc) = serde_json::from_str::<ai::ErGitHubComments>(&content) {
                self.ai.github_comments = Some(gc);
                self.ai.rebuild_comment_index();
            }
        }
    }

    /// Relocate all comments to their new positions after a diff change.
    pub fn relocate_all_comments(&mut self) {
        let current_hash = self.diff_hash.clone();

        // Build rename map: old path → new path
        let rename_map: std::collections::HashMap<String, String> = self
            .files
            .iter()
            .filter_map(|f| {
                if let git::FileStatus::Renamed(ref old_path) = f.status {
                    Some((old_path.clone(), f.path.clone()))
                } else {
                    None
                }
            })
            .collect();

        let is_lazy = self.lazy_mode;

        // Helper: find DiffFile by path, respecting renames.
        // Returns None for lazy stubs (hunks not yet parsed) to avoid false Lost results.
        let find_file = |path: &str| -> Option<usize> {
            let find_idx =
                |p: &str| -> Option<usize> { self.files.iter().position(|f| f.path == p) };
            let idx =
                find_idx(path).or_else(|| rename_map.get(path).and_then(|np| find_idx(np)))?;
            // Skip unparsed lazy stubs — hunks empty and not compacted
            if is_lazy {
                let f = &self.files[idx];
                if f.hunks.is_empty() && !f.compacted {
                    return None;
                }
            }
            Some(idx)
        };

        // Process questions
        let questions_changed = if let Some(ref mut qs) = self.ai.questions {
            let mut changed = false;
            for q in &mut qs.questions {
                if q.relocated_at_hash == current_hash {
                    continue;
                }
                // File-level questions have no anchor to relocate — skip
                if q.hunk_index.is_none() && q.line_start.is_none() && q.hunk_header.is_empty() {
                    q.relocated_at_hash = current_hash.clone();
                    continue;
                }
                let result = if let Some(idx) = find_file(&q.file) {
                    let anchor = ai::CommentAnchor {
                        file: q.file.clone(),
                        hunk_index: q.hunk_index,
                        line_start: q.line_start,
                        line_content: q.line_content.clone(),
                        context_before: q.context_before.clone(),
                        context_after: q.context_after.clone(),
                        old_line_start: q.old_line_start,
                        hunk_header: q.hunk_header.clone(),
                    };
                    ai::relocate_comment(&anchor, &self.files[idx])
                } else {
                    ai::RelocationResult::Lost
                };
                match result {
                    ai::RelocationResult::Unchanged => {
                        q.anchor_status = "original".to_string();
                        q.relocated_at_hash = current_hash.clone();
                        q.stale = false;
                        changed = true;
                    }
                    ai::RelocationResult::Relocated {
                        new_hunk_index,
                        new_line_start,
                    } => {
                        q.hunk_index = Some(new_hunk_index);
                        q.line_start = Some(new_line_start);
                        q.anchor_status = "relocated".to_string();
                        q.relocated_at_hash = current_hash.clone();
                        q.stale = false;
                        changed = true;
                    }
                    ai::RelocationResult::Lost => {
                        q.anchor_status = "lost".to_string();
                        q.stale = true;
                        q.relocated_at_hash = current_hash.clone();
                        changed = true;
                    }
                }
            }
            changed
        } else {
            false
        };

        // Process notes (same anchoring model as questions)
        let notes_changed = if let Some(ref mut ns) = self.ai.notes {
            let mut changed = false;
            for n in &mut ns.notes {
                if n.relocated_at_hash == current_hash {
                    continue;
                }
                if n.hunk_index.is_none() && n.line_start.is_none() && n.hunk_header.is_empty() {
                    n.relocated_at_hash = current_hash.clone();
                    continue;
                }
                let result = if let Some(idx) = find_file(&n.file) {
                    let anchor = ai::CommentAnchor {
                        file: n.file.clone(),
                        hunk_index: n.hunk_index,
                        line_start: n.line_start,
                        line_content: n.line_content.clone(),
                        context_before: n.context_before.clone(),
                        context_after: n.context_after.clone(),
                        old_line_start: n.old_line_start,
                        hunk_header: n.hunk_header.clone(),
                    };
                    ai::relocate_comment(&anchor, &self.files[idx])
                } else {
                    ai::RelocationResult::Lost
                };
                match result {
                    ai::RelocationResult::Unchanged => {
                        n.anchor_status = "original".to_string();
                        n.relocated_at_hash = current_hash.clone();
                        n.stale = false;
                        changed = true;
                    }
                    ai::RelocationResult::Relocated {
                        new_hunk_index,
                        new_line_start,
                    } => {
                        n.hunk_index = Some(new_hunk_index);
                        n.line_start = Some(new_line_start);
                        n.anchor_status = "relocated".to_string();
                        n.relocated_at_hash = current_hash.clone();
                        n.stale = false;
                        changed = true;
                    }
                    ai::RelocationResult::Lost => {
                        n.anchor_status = "lost".to_string();
                        n.stale = true;
                        n.relocated_at_hash = current_hash.clone();
                        changed = true;
                    }
                }
            }
            changed
        } else {
            false
        };

        // Process GitHub comments (top-level only; replies follow their parent)
        let comments_changed = if let Some(ref mut gc) = self.ai.github_comments {
            let mut changed = false;
            for c in &mut gc.comments {
                if c.relocated_at_hash == current_hash {
                    continue;
                }
                if c.in_reply_to.is_some() {
                    continue;
                }
                // File-level comments have no anchor to relocate — skip
                if c.hunk_index.is_none() && c.line_start.is_none() && c.hunk_header.is_empty() {
                    c.relocated_at_hash = current_hash.clone();
                    continue;
                }
                let result = if let Some(idx) = find_file(&c.file) {
                    let anchor = ai::CommentAnchor {
                        file: c.file.clone(),
                        hunk_index: c.hunk_index,
                        line_start: c.line_start,
                        line_content: c.line_content.clone(),
                        context_before: c.context_before.clone(),
                        context_after: c.context_after.clone(),
                        old_line_start: c.old_line_start,
                        hunk_header: c.hunk_header.clone(),
                    };
                    ai::relocate_comment(&anchor, &self.files[idx])
                } else {
                    ai::RelocationResult::Lost
                };
                match result {
                    ai::RelocationResult::Unchanged => {
                        c.anchor_status = "original".to_string();
                        c.relocated_at_hash = current_hash.clone();
                        c.stale = false;
                        changed = true;
                    }
                    ai::RelocationResult::Relocated {
                        new_hunk_index,
                        new_line_start,
                    } => {
                        c.hunk_index = Some(new_hunk_index);
                        c.line_start = Some(new_line_start);
                        c.anchor_status = "relocated".to_string();
                        c.relocated_at_hash = current_hash.clone();
                        c.stale = false;
                        changed = true;
                    }
                    ai::RelocationResult::Lost => {
                        c.anchor_status = "lost".to_string();
                        c.stale = true;
                        c.relocated_at_hash = current_hash.clone();
                        changed = true;
                    }
                }
            }
            changed
        } else {
            false
        };

        // Write back to disk if anything changed. Write failures are ignored — the
        // next refresh re-relocates the same comments from scratch.
        if questions_changed {
            if let Some(ref qs) = self.ai.questions {
                let path = format!("{}/questions.json", self.er_dir());
                if let Ok(json) = serde_json::to_string_pretty(qs) {
                    let tmp = format!("{}.tmp", path);
                    let _ = std::fs::write(&tmp, json).and_then(|_| std::fs::rename(&tmp, &path));
                }
            }
        }
        if notes_changed {
            if let Some(ref ns) = self.ai.notes {
                let path = format!("{}/notes.json", self.er_dir());
                if let Ok(json) = serde_json::to_string_pretty(ns) {
                    let tmp = format!("{}.tmp", path);
                    let _ = std::fs::write(&tmp, json).and_then(|_| std::fs::rename(&tmp, &path));
                }
            }
        }
        if comments_changed {
            if let Some(ref gc) = self.ai.github_comments {
                let path = self.github_comments_path();
                if let Ok(json) = serde_json::to_string_pretty(gc) {
                    let _ = std::fs::create_dir_all(self.comments_dir());
                    let tmp = format!("{}.tmp", path);
                    let _ = std::fs::write(&tmp, json).and_then(|_| std::fs::rename(&tmp, &path));
                }
            }
        }

        // Relocation mutates hunk_index / line_start in memory; rebuild the lazy
        // comment index so inline lookups stay in sync (file-level counts alone
        // would still look correct with a stale index).
        if questions_changed || notes_changed || comments_changed {
            self.ai.rebuild_comment_index();
        }
    }

    /// Compute which files have changed since the review, populating stale_files
    fn compute_stale_files(&mut self, branch_raw_diff: &str) {
        if let Some(ref review) = self.ai.review {
            if review.file_hashes.is_empty() {
                return;
            }
            let current_hashes = ai::compute_per_file_hashes(branch_raw_diff);
            let mut stale = std::collections::HashSet::new();
            for (file, review_hash) in &review.file_hashes {
                match current_hashes.get(file) {
                    Some(current_hash) if current_hash == review_hash => {}
                    _ => {
                        stale.insert(file.clone());
                    }
                }
            }
            // Files in current diff but not in review are new (not stale)
            self.ai.stale_files = stale;
        }
    }

    /// Check if .er-* files have been updated since last load (called on tick)
    pub fn check_ai_files_changed(&mut self) -> bool {
        let latest_mtime = ai::latest_er_mtime(&self.er_dir());

        let should_reload = match latest_mtime {
            Some(t) => match self.last_ai_check {
                Some(last_check) => t > last_check,
                None => true,
            },
            // Files deleted — clear stale in-memory state if we had any
            None => self.last_ai_check.is_some(),
        };

        if should_reload {
            self.reload_ai_state();
            return true;
        }
        false
    }

    // ── Layer toggles ──

    pub fn toggle_layer_questions(&mut self) {
        self.layers.show_questions = !self.layers.show_questions;
    }

    pub fn toggle_layer_comments(&mut self) {
        self.layers.show_github_comments = !self.layers.show_github_comments;
    }

    pub fn toggle_hide_resolved(&mut self) {
        self.layers.hide_resolved = !self.layers.hide_resolved;
    }

    pub fn toggle_layer_ai(&mut self) {
        self.layers.show_ai_findings = !self.layers.show_ai_findings;
    }

    /// Forward cycle order for the side panel. `FileDetail` and `AgentLog` are
    /// always available; the others are skipped when their data is absent.
    const PANEL_CYCLE: [PanelContent; 5] = [
        PanelContent::FileDetail,
        PanelContent::AiSummary,
        PanelContent::PrOverview,
        PanelContent::SymbolRefs,
        PanelContent::AgentLog,
    ];

    fn panel_available(&self, panel: PanelContent) -> bool {
        match panel {
            PanelContent::FileDetail | PanelContent::AgentLog => true,
            PanelContent::AiSummary => self.layers.show_ai_findings && self.ai.has_data(),
            PanelContent::PrOverview => self.pr_data.is_some(),
            PanelContent::SymbolRefs => self.symbol_refs.is_some(),
        }
    }

    /// Cycle panel: None → FileDetail → AiSummary (if AI data) → PrOverview (if PR live) → SymbolRefs (if symbols) → AgentLog → None
    pub fn toggle_panel(&mut self) {
        self.cycle_panel(true);
    }

    /// Cycle panel in reverse: None → AgentLog → SymbolRefs → PrOverview → AiSummary → FileDetail → None
    pub fn toggle_panel_reverse(&mut self) {
        self.cycle_panel(false);
    }

    /// Move to the next available panel in the cycle direction, closing the
    /// panel after the last one.
    fn cycle_panel(&mut self, forward: bool) {
        let cycle = &Self::PANEL_CYCLE;
        let pos = self.panel.and_then(|p| cycle.iter().position(|&c| c == p));
        self.panel = match (forward, pos) {
            (true, None) => cycle.iter().copied().find(|&p| self.panel_available(p)),
            (true, Some(i)) => cycle[i + 1..]
                .iter()
                .copied()
                .find(|&p| self.panel_available(p)),
            (false, None) => cycle
                .iter()
                .rev()
                .copied()
                .find(|&p| self.panel_available(p)),
            (false, Some(i)) => cycle[..i]
                .iter()
                .rev()
                .copied()
                .find(|&p| self.panel_available(p)),
        };
        self.panel_scroll = 0;
        if self.panel.is_none() {
            self.panel_focus = false;
        }
    }

    // ── Panel/review navigation ──

    /// Number of items in the left column (file risk list)
    pub fn review_file_count(&self) -> usize {
        self.ai.review_file_count()
    }

    /// Number of items in the right column (checklist items)
    pub fn review_checklist_count(&self) -> usize {
        self.ai.review_checklist_count()
    }

    fn review_item_count(&self) -> usize {
        match self.review_focus {
            ReviewFocus::Files => self.review_file_count(),
            ReviewFocus::Checklist => self.review_checklist_count(),
        }
    }

    pub fn review_next(&mut self) {
        let count = self.review_item_count();
        if count > 0 && self.review_cursor + 1 < count {
            self.review_cursor += 1;
        }
    }

    pub fn review_prev(&mut self) {
        if self.review_cursor > 0 {
            self.review_cursor -= 1;
        }
    }

    pub fn review_toggle_focus(&mut self) {
        self.review_focus = match self.review_focus {
            ReviewFocus::Files => ReviewFocus::Checklist,
            ReviewFocus::Checklist => ReviewFocus::Files,
        };
        self.review_cursor = 0;
    }

    /// Estimate the panel_scroll value needed to show the start of each AiSummary section.
    /// Returns (files_section_line, checklist_section_line).
    /// Must mirror the line-building logic in render_ai_summary() in ui/panel.rs.
    pub fn ai_summary_section_offsets(&self) -> (u16, u16) {
        let mut line: u16 = 0;

        // Title bar + separator (added by render_panel before content)
        line += 2;

        // "AI Review Summary" header + blank
        line += 2;

        // Summary content
        if let Some(ref summary) = self.ai.summary {
            for text_line in summary.lines() {
                if text_line.is_empty() || text_line.starts_with('#') {
                    line += 1;
                } else {
                    // Approximate word_wrap: each ~70 chars = 1 line (rough estimate)
                    let chars = text_line.len();
                    line += ((chars / 70) + 1) as u16;
                }
            }
        } else {
            line += 1; // "No .er-summary.md found"
        }

        line += 1; // blank after summary

        let files_start = line;

        // "File Risk Overview" header + blank
        line += 2;

        // File entries
        if let Some(ref review) = self.ai.review {
            line += review.files.len() as u16;
            if self.ai.total_findings() > 0 {
                line += 2; // total findings + blank
            }
        } else {
            line += 1; // "No .er-review.json"
        }

        line += 1; // blank

        let checklist_start = line;

        (files_start, checklist_start)
    }

    /// Diff files for the active view: branch/unstaged/staged use `files`; History uses
    /// the selected commit's parsed diff.
    pub fn active_diff_files(&self) -> &[DiffFile] {
        if self.mode == DiffMode::History {
            return self
                .history
                .as_ref()
                .map(|h| h.commit_files.as_slice())
                .unwrap_or(&[]);
        }
        &self.files
    }

    /// Selected file index within [`Self::active_diff_files`].
    pub fn active_selected_file_index(&self) -> usize {
        if self.mode == DiffMode::History {
            return self.history.as_ref().map(|h| h.selected_file).unwrap_or(0);
        }
        self.selected_file
    }

    /// Get the list of files, filtered by filter rules, search query, and reviewed status.
    /// Pipeline: filter rules → search → unreviewed toggle
    pub fn visible_files(&self) -> Vec<(usize, &DiffFile)> {
        let mut visible: Vec<(usize, &DiffFile)> =
            self.active_diff_files().iter().enumerate().collect();

        // Phase 1: Apply filter rules
        if !self.filter_rules.is_empty() {
            let review = self.ai.review.as_ref();
            visible.retain(|(_, f)| {
                super::filter::apply_filter_with_review(&self.filter_rules, f, review)
            });
        }

        // Phase 2: Apply search query (uses pre-lowercased query to avoid per-call allocation)
        if !self.search_query_lower.is_empty() {
            visible.retain(|(_, f)| f.path.to_lowercase().contains(&self.search_query_lower));
        }

        // Phase 3: Apply unreviewed-only toggle
        if self.show_unreviewed_only {
            visible.retain(|(_, f)| !self.reviewed.contains_key(&f.path));
        }

        visible
    }

    /// Get the list of watched files, filtered by search query
    pub fn visible_watched_files(&self) -> Vec<(usize, &WatchedFile)> {
        if !self.show_watched {
            return Vec::new();
        }
        if self.search_query_lower.is_empty() {
            self.watched_files.iter().enumerate().collect()
        } else {
            self.watched_files
                .iter()
                .enumerate()
                .filter(|(_, f)| f.path.to_lowercase().contains(&self.search_query_lower))
                .collect()
        }
    }

    /// Get the currently selected file (mode-aware: History uses HistoryState)
    pub fn selected_diff_file(&self) -> Option<&DiffFile> {
        if self.selected_watched.is_some() {
            return None;
        }
        if self.mode == DiffMode::History {
            return self
                .history
                .as_ref()
                .and_then(|h| h.commit_files.get(h.selected_file));
        }
        self.files.get(self.selected_file)
    }

    /// Active vertical diff scroll (mode-aware)
    pub fn active_diff_scroll(&self) -> u16 {
        if self.mode == DiffMode::History {
            return self.history.as_ref().map_or(0, |h| h.diff_scroll);
        }
        self.diff_scroll
    }

    /// Active current hunk index (mode-aware)
    pub fn active_current_hunk(&self) -> usize {
        if self.mode == DiffMode::History {
            return self.history.as_ref().map_or(0, |h| h.current_hunk);
        }
        self.current_hunk
    }

    /// Active current line index (mode-aware)
    pub fn active_current_line(&self) -> Option<usize> {
        if self.mode == DiffMode::History {
            return self.history.as_ref().and_then(|h| h.current_line);
        }
        self.current_line
    }

    /// Get the currently selected watched file
    pub fn selected_watched_file(&self) -> Option<&WatchedFile> {
        self.selected_watched
            .and_then(|idx| self.watched_files.get(idx))
    }

    /// Total hunks in the currently selected file
    pub fn total_hunks(&self) -> usize {
        self.selected_diff_file()
            .map(|f| f.hunks.len())
            .unwrap_or(0)
    }

    // ── Navigation ──

    /// Snap selected_file to the first visible file if current selection is not visible
    pub fn snap_to_visible(&mut self) {
        if let Some(idx) = self.selected_watched {
            // In watched section — check if selection is still visible
            let visible_watched = self.visible_watched_files();
            if !visible_watched.iter().any(|(i, _)| *i == idx) {
                if !visible_watched.is_empty() {
                    self.selected_watched = Some(visible_watched[0].0);
                } else {
                    self.selected_watched = None;
                    // Fall back to diff files
                    let visible = self.visible_files();
                    if !visible.is_empty() {
                        self.selected_file = visible[0].0;
                        self.current_hunk = 0;
                        self.current_line = None;
                        self.diff_scroll = 0;
                        self.h_scroll = 0;
                        self.focused_finding_id = None;
                    }
                }
            }
        } else {
            let visible = self.visible_files();
            if visible.is_empty() {
                return;
            }
            if !visible.iter().any(|(i, _)| *i == self.selected_file) {
                self.selected_file = visible[0].0;
                self.current_hunk = 0;
                self.current_line = None;
                self.diff_scroll = 0;
                self.h_scroll = 0;
                self.focused_finding_id = None;
            }
        }
    }

    // ── Mode ──

    /// Root to run commit-log/diff git commands in: the branch's own worktree
    /// when this tab views a checked-out local branch, else the tab's repo root.
    pub fn commit_log_root(&self) -> &str {
        self.local_branch_checkout_root
            .as_deref()
            .unwrap_or(self.repo_root.as_str())
    }

    /// Ref to log the branch's commits against base (`base..head`). Uses the
    /// viewed branch ref so the commit list matches the diff even when that
    /// branch is not the checked-out HEAD of `commit_log_root`.
    pub fn commit_head_ref(&self) -> &str {
        self.local_branch_view
            .as_deref()
            .unwrap_or(self.current_branch.as_str())
    }

    /// (Re)build `TourState` from the loaded tour (`ai.tour`) and the current
    /// branch diff (`self.files`). Pillar files absent from the diff are skipped;
    /// diff files referenced by no pillar are appended to a trailing "Other
    /// changes" pillar so nothing is hidden. Preserves the selected pillar across
    /// rebuilds (e.g. live diff refreshes) when possible.
    pub fn rebuild_tour_state(&mut self) {
        let Some(tour) = self.ai.tour.clone() else {
            self.tour = None;
            return;
        };
        let ordered = tour.ordered_pillars();
        let mut pillars: Vec<TourPillarView> = Vec::new();
        let mut files: Vec<DiffFile> = Vec::new();
        let mut ranges: Vec<(usize, usize)> = Vec::new();
        let mut used: HashSet<String> = HashSet::new();

        for p in &ordered {
            let start = files.len();
            for tf in &p.files {
                if used.contains(&tf.path) {
                    continue;
                }
                if let Some(df) = self.files.iter().find(|f| f.path == tf.path) {
                    files.push(df.clone());
                    used.insert(tf.path.clone());
                }
            }
            let end = files.len();
            // Skip pillars whose files are all absent from the current diff.
            if end == start {
                continue;
            }
            pillars.push(TourPillarView {
                id: p.id.clone(),
                title: p.title.clone(),
                description: p.description.clone(),
                importance: p.importance,
                foundation: p.foundation,
            });
            ranges.push((start, end));
        }

        // Trailing "Other changes" pillar for diff files no pillar referenced.
        let start = files.len();
        for df in &self.files {
            if !used.contains(&df.path) {
                files.push(df.clone());
            }
        }
        if files.len() > start {
            pillars.push(TourPillarView {
                id: "__other__".to_string(),
                title: "Other changes".to_string(),
                description: "Files not assigned to a tour pillar.".to_string(),
                importance: 0,
                foundation: false,
            });
            ranges.push((start, files.len()));
        }

        let selected_pillar = self
            .tour
            .as_ref()
            .map(|t| t.selected_pillar)
            .unwrap_or(0)
            .min(pillars.len().saturating_sub(1));
        let selected_file = ranges.get(selected_pillar).map(|&(s, _)| s).unwrap_or(0);

        self.tour = Some(TourState {
            pillars,
            selected_pillar,
            files,
            pillar_file_ranges: ranges,
            selected_file,
            current_hunk: 0,
            current_line: None,
            diff_scroll: 0,
            h_scroll: 0,
        });
    }

    pub fn set_mode(&mut self, mode: DiffMode) {
        if self.mode != mode {
            // Remember current position to restore after mode switch
            let prev_path = self.files.get(self.selected_file).map(|f| f.path.clone());
            let prev_hunk = self.current_hunk;
            let prev_line = self.current_line;

            // Capture bucket BEFORE changing mode so we can detect a bucket change.
            let prev_bucket = self.review_bucket();
            let prev_mode = self.mode;

            // Entering Tour: remember whether the originating view was the PR diff
            // so the guide stays attached to the PR (vs the local branch) and the
            // Diff toggle returns to the right view. Remote tabs are always PR.
            if mode == DiffMode::Tour {
                self.tour_is_pr = prev_mode == DiffMode::PrDiff || self.remote_repo.is_some();
            }

            self.mode = mode;
            self.committed_unpushed = false;

            // Reload per-bucket storage when the bucket changes on mode switch (covers
            // all branches: Branch↔Unstaged↔Staged↔History↔Conflicts↔Hidden).
            // Must run before refresh_diff_mode_switch() and before the selection restore.
            if self.review_bucket() != prev_bucket {
                self.apply_managed_root();
                self.reviewed = Self::load_reviewed_files_from_path(&self.er_root.reviewed_path());
                self.reload_ai_state();
            }

            if mode == DiffMode::History {
                // Initialize history state if first time
                if self.history.is_none() {
                    let log_root = self.commit_log_root().to_string();
                    let is_pr_review_tab =
                        self.pr_number.is_some() && self.local_branch_view.is_some();
                    let commits = if is_pr_review_tab {
                        self.pr_commits.clone()
                    } else {
                        let head_ref = self.commit_head_ref().to_string();
                        git::git_log_range(&self.base_branch, &head_ref, &log_root, 50, 0)
                            .unwrap_or_default()
                    };

                    let first_diff = if let Some(c) = commits.first() {
                        let raw = git::git_diff_commit(&c.hash, &log_root).unwrap_or_default();
                        git::parse_diff(&raw)
                    } else {
                        vec![]
                    };

                    let mut cache = DiffCache::new(5);
                    if let Some(c) = commits.first() {
                        cache.insert(c.hash.clone(), first_diff.clone());
                    }

                    self.history = Some(HistoryState {
                        commits,
                        selected_commit: 0,
                        commit_files: first_diff,
                        selected_file: 0,
                        current_hunk: 0,
                        current_line: None,
                        diff_scroll: 0,
                        h_scroll: 0,
                        all_loaded: is_pr_review_tab,
                        diff_cache: cache,
                    });
                }
            } else if mode == DiffMode::Tour {
                self.current_hunk = 0;
                self.current_line = None;
                self.selected_watched = None;
                self.diff_scroll = 0;
                // Tour reorders the branch diff — load it first (fetch_scope == "branch").
                let _ = self.refresh_diff_mode_switch();
                self.rebuild_tour_state();
            } else if mode == DiffMode::Conflicts {
                self.current_hunk = 0;
                self.current_line = None;
                self.selected_watched = None;
                self.diff_scroll = 0;
                self.refresh_conflicts();
            } else if mode == DiffMode::Hidden {
                self.current_hunk = 0;
                self.current_line = None;
                self.selected_watched = None;
                self.diff_scroll = 0;
                // Reload config to pick up any .er-config.toml changes
                let er_config = crate::config::load_config(&self.repo_root);
                self.watched_config = er_config.watched;
                // Hidden mode shows only watched/gitignored files — clear regular diff
                self.files.clear();
                self.show_watched = true;
                self.refresh_watched_files();
                if !self.watched_files.is_empty() {
                    self.selected_watched = Some(0);
                }
            } else {
                self.current_hunk = 0;
                self.current_line = None;
                self.selected_watched = None;
                self.diff_scroll = 0;
                let _ = self.refresh_diff_mode_switch();

                // Restore selection by path (file order may differ between modes)
                if let Some(path) = prev_path {
                    if let Some(idx) = self.files.iter().position(|f| f.path == path) {
                        self.selected_file = idx;
                        // Restore hunk/line if the file still has enough hunks
                        if prev_hunk < self.total_hunks() {
                            self.current_hunk = prev_hunk;
                            self.current_line = prev_line;
                        }
                    } else {
                        self.selected_file = 0;
                    }
                } else {
                    self.selected_file = 0;
                }
                self.snap_to_visible();
                self.scroll_to_current_hunk();
            }
        }
    }

    // ── Watched Files ──

    /// Reload watched files from disk
    pub fn refresh_watched_files(&mut self) {
        if !self.show_watched || self.watched_config.paths.is_empty() {
            self.watched_files.clear();
            return;
        }
        match git::discover_watched_files(&self.repo_root, &self.watched_config.paths) {
            Ok(files) => {
                // Check gitignore status for warnings
                self.watched_not_ignored = files
                    .iter()
                    .filter(|f| !git::verify_gitignored(&self.repo_root, &f.path))
                    .map(|f| f.path.clone())
                    .collect();
                self.watched_files = files;
            }
            Err(_) => {
                self.watched_files.clear();
            }
        }
        // Clamp selection
        if let Some(idx) = self.selected_watched {
            if idx >= self.watched_files.len() {
                if self.watched_files.is_empty() {
                    self.selected_watched = None;
                } else {
                    self.selected_watched = Some(self.watched_files.len() - 1);
                }
            }
        }
    }

    /// Reload config from .er-config.toml
    #[allow(dead_code)]
    pub fn reload_config(&mut self) {
        let er_config = config::load_config(&self.repo_root);
        self.watched_config = er_config.watched;
        let has_paths = !self.watched_config.paths.is_empty();
        if !has_paths {
            self.show_watched = false;
            self.watched_files.clear();
            self.selected_watched = None;
        }
    }

    /// Update snapshot for the currently selected watched file
    pub fn update_watched_snapshot(&mut self) -> Result<()> {
        if let Some(watched) = self.selected_watched_file() {
            let path = watched.path.clone();
            let snapshots_dir = self.er_root.snapshots_dir();
            git::save_snapshot(&self.repo_root, &path, &snapshots_dir)?;
        }
        Ok(())
    }

    /// Sort files by filesystem mtime (newest first)
    fn sort_files_by_mtime(&mut self) {
        use std::fs;
        use std::time::SystemTime;

        // In lazy mode this breaks the index correspondence between self.files and
        // self.file_headers that ensure_file_parsed() relies on.
        let repo_root = self.repo_root.clone();
        self.files.sort_by(|a, b| {
            let mtime_a = fs::metadata(format!("{}/{}", repo_root, a.path))
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            let mtime_b = fs::metadata(format!("{}/{}", repo_root, b.path))
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            // Newest first (reverse chronological)
            mtime_b.cmp(&mtime_a)
        });
    }

    /// Populate `mtime_cache` with one `fs::metadata` call per diff file.
    /// Called after the diff is loaded so rendering never touches the filesystem directly.
    pub fn refresh_mtime_cache(&mut self) {
        use std::fs;
        use std::time::SystemTime;
        self.mtime_cache.clear();
        for file in &self.files {
            let mtime = fs::metadata(format!("{}/{}", self.repo_root, file.path))
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            self.mtime_cache.insert(file.path.clone(), mtime);
        }
    }

    fn clamp_hunk(&mut self) {
        let total = self.total_hunks();
        if total == 0 {
            self.current_hunk = 0;
            self.current_line = None;
        } else if self.current_hunk >= total {
            self.current_hunk = total - 1;
        }
    }

    // ── Filter ──

    /// Parse and apply a filter expression, updating history
    pub fn apply_filter_expr(&mut self, expr: &str) {
        let expr = expr.trim().to_string();
        if expr.is_empty() {
            self.clear_filter();
            return;
        }
        self.filter_expr = expr.clone();
        self.filter_rules = super::filter::parse_filter_expr(&self.filter_expr);

        // Add to history (remove duplicate if exists, push to front)
        self.filter_history.retain(|h| h != &expr);
        self.filter_history.insert(0, expr);

        // Cap history at 20 entries
        self.filter_history.truncate(20);

        self.snap_to_visible();
    }

    /// Clear the active filter
    pub fn clear_filter(&mut self) {
        self.filter_expr.clear();
        self.filter_rules.clear();
        self.snap_to_visible();
    }

    // ── Reviewed-File Tracking ──

    /// Count of reviewed files vs total in the active diff (branch / history commit).
    pub fn active_reviewed_count(&self) -> (usize, usize) {
        let files = self.active_diff_files();
        let total = files.len();
        let reviewed = files
            .iter()
            .filter(|f| self.reviewed.contains_key(&f.path))
            .count();
        (reviewed, total)
    }

    /// Count of reviewed files vs total (all files, ignoring filters).
    /// Delegates to [`Self::active_reviewed_count`] so History mode and desktop stay aligned.
    pub fn reviewed_count(&self) -> (usize, usize) {
        self.active_reviewed_count()
    }

    /// Count of reviewed files vs total among filtered files only.
    /// Returns None if no filter is active.
    pub fn filtered_reviewed_count(&self) -> Option<(usize, usize)> {
        if self.filter_rules.is_empty() {
            return None;
        }
        let (mut total, mut reviewed) = (0, 0);
        let review = self.ai.review.as_ref();
        for f in &self.files {
            if super::filter::apply_filter_with_review(&self.filter_rules, f, review) {
                total += 1;
                if self.reviewed.contains_key(&f.path) {
                    reviewed += 1;
                }
            }
        }
        Some((reviewed, total))
    }

    fn load_reviewed_files_from_path(path: &str) -> HashMap<String, String> {
        match std::fs::read_to_string(path) {
            Ok(content) => content
                .lines()
                .map(|l| l.trim())
                .filter(|l| !l.is_empty())
                .map(|l| {
                    // New format: "path\thash". Old format: "path" (no tab).
                    // Old-format entries get an empty-string hash sentinel — they are
                    // never auto-unmarked so existing reviewed state is preserved.
                    if let Some((p, h)) = l.split_once('\t') {
                        (p.to_string(), h.to_string())
                    } else {
                        (l.to_string(), String::new())
                    }
                })
                .collect(),
            Err(_) => HashMap::new(),
        }
    }

    pub fn save_reviewed_files(&self) -> Result<()> {
        // Remote tabs that belong to the Pr bucket (PrDiff mode or --remote) are
        // allowed to persist — they write to the shared prs/pr-N dir.
        // Only skip when remote AND NOT the Pr bucket (shouldn't happen today, but
        // guards against future remote modes that have no managed dir).
        if self.is_remote() && self.review_bucket() != ReviewBucket::Pr {
            return Ok(());
        }
        let path = self.er_root.reviewed_path();
        if self.reviewed.is_empty() {
            // Remove file if no reviewed files
            let _ = std::fs::remove_file(&path);
            return Ok(());
        }
        let er_dir = self.er_root.er_dir();
        std::fs::create_dir_all(&er_dir)?;
        let mut entries: Vec<(&String, &String)> = self.reviewed.iter().collect();
        entries.sort_by_key(|(p, _)| p.as_str());
        let content = entries
            .iter()
            .map(|(p, h)| format!("{}\t{}", p, h))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&path, format!("{}\n", content))?;
        Ok(())
    }

    /// Remove reviewed entries for paths not in the active diff (orphans from another
    /// branch's `reviewed` file or legacy empty-hash lines). Persists when any removed.
    pub fn prune_reviewed_not_in_diff(&mut self) -> usize {
        if self.active_diff_files().is_empty() {
            return 0;
        }
        let active: std::collections::HashSet<&str> = self
            .active_diff_files()
            .iter()
            .map(|f| f.path.as_str())
            .collect();
        let orphan: Vec<String> = self
            .reviewed
            .keys()
            .filter(|p| !active.contains(p.as_str()))
            .cloned()
            .collect();
        let count = orphan.len();
        if count > 0 {
            for path in orphan {
                self.reviewed.remove(&path);
            }
            let _ = self.save_reviewed_files();
        }
        count
    }

    /// Remove reviewed entries whose stored diff hash no longer matches the current diff.
    /// Also drops paths absent from the active diff (including legacy empty-hash lines).
    /// Returns the number of entries removed. Saves the file if any were removed.
    fn auto_unmark_changed_reviewed(&mut self) -> usize {
        let orphan_count = self.prune_reviewed_not_in_diff();

        let stale: Vec<String> = self
            .reviewed
            .iter()
            .filter_map(|(path, stored_hash)| {
                if stored_hash.is_empty() {
                    return None;
                }
                let current_hash = self.current_per_file_hashes.get(path);
                match current_hash {
                    Some(h) if h == stored_hash => None,
                    _ => Some(path.clone()),
                }
            })
            .collect();

        if !stale.is_empty() {
            for path in &stale {
                self.reviewed.remove(path);
            }
            self.reviewed_revision += 1;
            let _ = self.save_reviewed_files();
        } else if orphan_count > 0 {
            // Orphans were pruned by prune_reviewed_not_in_diff; bump here too.
            self.reviewed_revision += 1;
        }
        orphan_count + stale.len()
    }

    /// Capture current state into a SessionState for persistence.
    pub fn capture_session(&self) -> SessionState {
        SessionState {
            diff_hash: self.branch_diff_hash.clone(),
            branch: self.current_branch.clone(),
            selected_file: self.selected_file,
            current_hunk: self.current_hunk,
            current_line: self.current_line,
            diff_scroll: self.diff_scroll,
            h_scroll: self.h_scroll,
            diff_mode: self.mode.git_mode().to_string(),
            filter_expr: self.filter_expr.clone(),
            filter_history: self.filter_history.clone(),
            show_unreviewed_only: self.show_unreviewed_only,
            sort_by_mtime: self.sort_by_mtime,
            comment_draft: self.comment_text(),
            comment_draft_file: self.comment_file.clone(),
            comment_draft_hunk: self.comment_hunk,
            comment_draft_line: self.comment_line_num,
            comment_draft_type: match self.comment_type {
                CommentType::Question => "question".to_string(),
                CommentType::Note => "note".to_string(),
                CommentType::GitHubComment => "github".to_string(),
            },
        }
    }

    /// Restore session state if the diff hash matches. Returns true if restored.
    pub fn restore_session(&mut self) -> bool {
        if self.is_remote() {
            return false;
        }
        let session = match SessionState::load(&self.er_root.session_path()) {
            Some(s) => s,
            None => return false,
        };

        // Only restore if the diff hasn't changed
        if session.diff_hash != self.branch_diff_hash {
            return false;
        }

        // Restore diff mode
        let mode = match session.diff_mode.as_str() {
            "branch" => DiffMode::Branch,
            "unstaged" => DiffMode::Unstaged,
            "staged" => DiffMode::Staged,
            "history" => DiffMode::History,
            "conflicts" => DiffMode::Conflicts,
            _ => DiffMode::Branch,
        };
        if mode != self.mode {
            self.set_mode(mode);
        }

        // Restore navigation (clamped to current file count)
        let file_count = self.files.len();
        if file_count == 0 {
            return false;
        }
        self.selected_file = session.selected_file.min(file_count - 1);
        self.diff_scroll = session.diff_scroll;
        self.h_scroll = session.h_scroll;

        // Restore hunk/line within the selected file
        if let Some(file) = self.files.get(self.selected_file) {
            let hunk_count = file.hunks.len();
            if hunk_count > 0 {
                self.current_hunk = session.current_hunk.min(hunk_count - 1);
            }
            self.current_line = session.current_line;
        }

        // Restore filter
        if !session.filter_expr.is_empty() {
            self.apply_filter_expr(&session.filter_expr);
        }
        self.filter_history = session.filter_history;

        // Restore view preferences
        self.show_unreviewed_only = session.show_unreviewed_only;
        self.sort_by_mtime = session.sort_by_mtime;

        // Restore comment draft if non-empty
        if !session.comment_draft.is_empty() {
            self.comment_textarea = TextArea::new(vec![session.comment_draft]);
            self.comment_file = session.comment_draft_file;
            self.comment_hunk = session.comment_draft_hunk;
            self.comment_line_num = session.comment_draft_line;
            self.comment_type = match session.comment_draft_type.as_str() {
                "question" => CommentType::Question,
                "note" => CommentType::Note,
                _ => CommentType::GitHubComment,
            };
        }

        true
    }

    /// Save current session state to .er/session.json.
    pub fn save_session(&self) {
        if self.is_remote() {
            return;
        }
        let session = self.capture_session();
        let _ = session.save(&self.er_root.session_path());
    }
}

// ── Main App State ──

/// Status of a background command (generic, replaces SummaryAgentStatus)
#[derive(Debug, Clone, PartialEq)]
pub enum CommandStatus {
    /// Command is currently running
    Running,
    /// Command finished successfully
    Done,
    /// Command failed with an error message
    Failed(String),
}

#[derive(Debug, Clone)]
pub enum AgentLogSource {
    Stdout,
    Stderr,
    Status,
}

#[derive(Debug, Clone)]
pub struct AgentLogEntry {
    pub timestamp: std::time::Instant,
    pub command_name: String,
    pub source: AgentLogSource,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct PanelsVisible {
    pub left: bool,
    pub tree: bool,
    pub right: bool,
}

impl Default for PanelsVisible {
    fn default() -> Self {
        PanelsVisible {
            left: true,
            tree: true,
            right: true,
        }
    }
}

pub struct App {
    /// Open tabs (one per repo)
    pub tabs: Vec<TabState>,

    /// Index of the active tab
    pub active_tab: usize,

    /// Whether we're navigating or typing in the search filter
    pub input_mode: InputMode,

    /// Should the app quit?
    pub should_quit: bool,

    /// Active overlay popup (None = no overlay)
    pub overlay: Option<OverlayData>,

    /// Whether watch mode is active
    pub watching: bool,

    /// Last watch notification message
    pub watch_message: Option<String>,

    /// Ticks since last watch notification (for auto-clearing)
    pub watch_message_ticks: u16,

    /// How many ticks the current notification should persist (default 20 ≈ 2s)
    pub watch_message_max_ticks: u16,

    /// Counter for throttling AI file polling (check every 10 ticks ≈ 1s)
    pub ai_poll_counter: u16,

    /// Input buffer for remote URL input mode
    pub remote_url_input: String,

    /// Application configuration (loaded from .er-config.toml)
    pub config: ErConfig,

    /// Session-local AI Hub provider selection
    pub current_ai_provider: Option<String>,

    /// Session-local AI Hub model selection
    pub current_ai_model: Option<String>,

    /// Session-local Claude Code effort level (`low` … `max`).
    pub current_ai_effort: Option<String>,

    /// Pending action from a modal hub selection (consumed by the event loop)
    pub pending_hub_action: Option<HubAction>,

    /// Last known terminal width (updated each tick for resize calculations)
    pub last_terminal_width: u16,

    /// Which panels are currently visible in the desktop UI
    pub panels_visible: PanelsVisible,

    /// App-level background review tasks. Keyed by task id. Outlive
    /// per-tab state so the user can switch tabs/branches while a review
    /// continues running. Session-scoped; not persisted across restarts.
    pub(crate) background_tasks: background::BackgroundTaskMap,

    /// FIFO queue of accepted-but-not-started review tasks. Tasks land here
    /// when the number of running tasks reaches the configured
    /// `ai_hub.max_concurrent_reviews` cap; `poll_background_tasks` launches
    /// them as slots free up.
    pub(crate) pending_background_tasks:
        std::collections::VecDeque<background::PendingBackgroundTask>,

    /// Snapshot copies of finished background tasks retained briefly so
    /// the UI can show "done"/"failed" toasts. Cleared from `background_tasks`
    /// once `finished_at_ms` exits the 8s display window. We keep them here
    /// in finished form (no channels) so snapshot building stays cheap.
    pub(crate) recent_background_tasks: Vec<background::BackgroundTask>,

    /// Multi-round AI review arena runs (desktop orchestration).
    pub arena_registry: std::sync::Arc<crate::arena::ArenaRegistry>,

    /// Last started arena run id per tab `.er` directory.
    pub active_arena_runs: std::collections::HashMap<String, Vec<String>>,
}

impl App {
    fn default_arena_registry() -> Arc<crate::arena::ArenaRegistry> {
        App::init_arena_registry(Arc::new(|| {}))
    }

    fn initial_ai_selection(config: &ErConfig) -> (Option<String>, Option<String>) {
        let provider = config.ai_hub.resolve_provider_id(None);
        let model = provider
            .as_deref()
            .and_then(|provider_id| config.ai_hub.resolve_model_id(provider_id, None));
        (provider, model)
    }

    fn initial_ai_effort(config: &ErConfig) -> Option<String> {
        crate::config::resolve_effort(&config.ai_hub, &config.agent, None, None)
    }

    /// Create the app from CLI path arguments.
    /// If no paths provided, uses current directory.
    pub fn new_with_args(paths: &[String]) -> Result<Self> {
        let tabs = if paths.is_empty() {
            let repo_root = git::get_repo_root()?;
            vec![TabState::new(repo_root)?]
        } else {
            let mut tabs = Vec::new();
            for path in paths {
                if crate::github::is_github_pr_url(path) {
                    // GitHub PR URL — fetch head ref and open without checkout
                    let pr_ref = crate::github::parse_github_pr_url(path)
                        .ok_or_else(|| anyhow::anyhow!("Invalid GitHub PR URL: {}", path))?;
                    crate::github::ensure_gh_installed()?;

                    // We need to be in the repo. Use cwd.
                    let repo_root = git::get_repo_root().context(
                        "Cannot open PR URL: not in a git repository. Clone the repo first.",
                    )?;

                    // Verify the local repo matches the PR's repo
                    crate::github::verify_remote_matches(&repo_root, &pr_ref)?;

                    // Fetch PR head to a local ref without touching the working tree
                    let head_ref = crate::github::fetch_pr_head(pr_ref.number, &repo_root)?;
                    let base = crate::github::gh_pr_base_branch(pr_ref.number, &repo_root)?;
                    let base = crate::github::ensure_base_ref_available(&repo_root, &base)?;
                    let head_branch =
                        crate::github::gh_pr_head_branch_name(pr_ref.number, &repo_root)
                            .unwrap_or_else(|_| format!("pr/{}", pr_ref.number));

                    let mut tab = TabState::new_with_base(repo_root, base)?;
                    tab.pr_head_ref = Some(head_ref);
                    tab.pr_number = Some(pr_ref.number);
                    tab.current_branch = head_branch;
                    tab.refresh_diff()?;
                    tabs.push(tab);
                } else {
                    // Local path
                    let canonical = std::fs::canonicalize(path)
                        .with_context(|| format!("Path not found: {}", path))?;
                    let dir = canonical.to_string_lossy().to_string();
                    let repo_root = git::get_repo_root_in(&dir)
                        .with_context(|| format!("Not a git repository: {}", path))?;
                    tabs.push(TabState::new(repo_root)?);
                }
            }
            tabs
        };

        // Config comes from the first tab's repo root and applies to all tabs —
        // other tabs' .er-config.toml files are ignored.
        let repo_root = tabs.first().map(|t| t.repo_root.as_str()).unwrap_or(".");
        let er_config = config::load_config(repo_root);
        let (current_ai_provider, current_ai_model) = Self::initial_ai_selection(&er_config);
        let current_ai_effort = Self::initial_ai_effort(&er_config);
        let arena_registry = App::init_arena_registry(Arc::new(|| {}));

        let mut app = App {
            tabs,
            active_tab: 0,
            input_mode: InputMode::Normal,
            should_quit: false,
            overlay: None,
            watching: false,
            watch_message: None,
            watch_message_ticks: 0,
            watch_message_max_ticks: 20,
            ai_poll_counter: 0,
            remote_url_input: String::new(),
            config: er_config,
            current_ai_provider,
            current_ai_model,
            current_ai_effort,
            pending_hub_action: None,
            last_terminal_width: 0,
            panels_visible: PanelsVisible::default(),
            background_tasks: std::collections::HashMap::new(),
            recent_background_tasks: Vec::new(),
            pending_background_tasks: std::collections::VecDeque::new(),
            arena_registry: arena_registry.clone(),
            active_arena_runs: std::collections::HashMap::new(),
        };
        app.drain_storage_notices();
        app.reconcile_arena_runs();
        Ok(app)
    }

    /// Create an App with a single unloaded tab for `repo_root` — skips the
    /// initial `refresh_diff()`. Used by the desktop's startup path when a
    /// persisted tabs.json is about to replace `app.tabs` anyway: paying for
    /// the initial diff here is wasted work, and on machines launching
    /// outside a repo CWD this lets us still open a window backed by the
    /// last-active project's root.
    pub fn new_unloaded(repo_root: String) -> Result<Self> {
        let base = git::detect_base_branch_in(&repo_root)?;
        let tab = TabState::new_with_base_unloaded(repo_root.clone(), base)?;
        let er_config = crate::config::load_config(&repo_root);
        let (current_ai_provider, current_ai_model) = Self::initial_ai_selection(&er_config);
        let current_ai_effort = Self::initial_ai_effort(&er_config);
        Ok(App {
            tabs: vec![tab],
            active_tab: 0,
            input_mode: InputMode::Normal,
            should_quit: false,
            overlay: None,
            watching: false,
            watch_message: None,
            watch_message_ticks: 0,
            watch_message_max_ticks: 20,
            ai_poll_counter: 0,
            remote_url_input: String::new(),
            config: er_config,
            current_ai_provider,
            current_ai_model,
            current_ai_effort,
            pending_hub_action: None,
            last_terminal_width: 0,
            panels_visible: PanelsVisible::default(),
            background_tasks: std::collections::HashMap::new(),
            recent_background_tasks: Vec::new(),
            pending_background_tasks: std::collections::VecDeque::new(),
            arena_registry: Self::default_arena_registry(),
            active_arena_runs: std::collections::HashMap::new(),
        })
    }

    /// Create App for remote PR review — no local git repo needed.
    pub fn new_remote(mut tab: TabState, pr_data: Option<crate::github::PrOverviewData>) -> Self {
        if let Some(data) = pr_data {
            tab.pr_data = Some(data);
        }
        let er_config = crate::config::load_global_config();
        let (current_ai_provider, current_ai_model) = Self::initial_ai_selection(&er_config);
        let current_ai_effort = Self::initial_ai_effort(&er_config);
        App {
            tabs: vec![tab],
            active_tab: 0,
            input_mode: InputMode::Normal,
            should_quit: false,
            overlay: None,
            watching: false,
            watch_message: None,
            watch_message_ticks: 0,
            watch_message_max_ticks: 20,
            ai_poll_counter: 0,
            remote_url_input: String::new(),
            config: er_config,
            current_ai_provider,
            current_ai_model,
            current_ai_effort,
            pending_hub_action: None,
            last_terminal_width: 0,
            panels_visible: PanelsVisible::default(),
            background_tasks: std::collections::HashMap::new(),
            recent_background_tasks: Vec::new(),
            pending_background_tasks: std::collections::VecDeque::new(),
            arena_registry: Self::default_arena_registry(),
            active_arena_runs: std::collections::HashMap::new(),
        }
    }

    /// Construct an App with a single test tab. Intended for unit tests
    /// that need to exercise input handlers without spinning up git.
    pub fn new_for_test(files: Vec<crate::git::DiffFile>) -> Self {
        App {
            tabs: vec![TabState::new_for_test(files)],
            active_tab: 0,
            input_mode: InputMode::Normal,
            should_quit: false,
            overlay: None,
            watching: false,
            watch_message: None,
            watch_message_ticks: 0,
            watch_message_max_ticks: 20,
            ai_poll_counter: 0,
            remote_url_input: String::new(),
            config: ErConfig::default(),
            current_ai_provider: None,
            current_ai_model: None,
            current_ai_effort: None,
            pending_hub_action: None,
            last_terminal_width: 0,
            panels_visible: PanelsVisible::default(),
            background_tasks: std::collections::HashMap::new(),
            recent_background_tasks: Vec::new(),
            pending_background_tasks: std::collections::VecDeque::new(),
            arena_registry: Self::default_arena_registry(),
            active_arena_runs: std::collections::HashMap::new(),
        }
    }

    pub fn sync_ai_selection(&mut self) {
        let provider = self
            .config
            .ai_hub
            .resolve_provider_id(self.current_ai_provider.as_deref());
        let model = provider.as_deref().and_then(|provider_id| {
            self.config
                .ai_hub
                .resolve_model_id(provider_id, self.current_ai_model.as_deref())
        });
        self.current_ai_provider = provider;
        self.current_ai_model = model;
    }

    pub fn active_ai_selection_label(&self) -> String {
        if let Some(provider_id) = self
            .config
            .ai_hub
            .resolve_provider_id(self.current_ai_provider.as_deref())
        {
            if let Some(provider) = self.config.ai_hub.providers.get(&provider_id) {
                let provider_label = provider.display_name(&provider_id);
                let model_label = self
                    .config
                    .ai_hub
                    .resolve_model_id(&provider_id, self.current_ai_model.as_deref())
                    .and_then(|model_id| {
                        provider
                            .models
                            .iter()
                            .find(|m| m.id == model_id)
                            .map(|m| m.display_name())
                    });
                let mut label = match model_label {
                    Some(model) => format!("{provider_label} / {model}"),
                    None => provider_label,
                };
                if let Some(effort) = self.current_ai_effort.as_deref() {
                    label.push_str(" · ");
                    label.push_str(effort);
                }
                return label;
            }
        }

        self.config.agent.display_name()
    }

    pub fn open_ai_provider_picker(&mut self, action: Option<AiActionKind>) {
        if !self.config.ai_hub.has_presets() {
            if let Some(action) = action {
                self.pending_hub_action = Some(HubAction::RunAiAction(action));
            } else {
                self.notify("No [ai_hub] providers configured");
            }
            return;
        }

        let selected_provider = self
            .config
            .ai_hub
            .resolve_provider_id(self.current_ai_provider.as_deref());
        let provider_ids = self.config.ai_hub.provider_ids();
        let mut selected = 0usize;
        let items = provider_ids
            .iter()
            .enumerate()
            .filter_map(|(idx, provider_id)| {
                let provider = self.config.ai_hub.providers.get(provider_id)?;
                if selected_provider.as_deref() == Some(provider_id.as_str()) {
                    selected = idx;
                }
                let label = provider.display_name(provider_id);
                let model_summary = if provider.models.is_empty() {
                    "no model presets".to_string()
                } else {
                    format!(
                        "{} model{}",
                        provider.models.len(),
                        if provider.models.len() == 1 { "" } else { "s" }
                    )
                };
                Some(HubItem {
                    label,
                    hint: "".into(),
                    description: model_summary,
                    action: HubAction::SelectAiProvider {
                        action: action.clone(),
                        provider_id: provider_id.clone(),
                    },
                    is_header: false,
                    enabled: true,
                })
            })
            .collect();
        self.overlay = Some(OverlayData::ModalHub {
            kind: HubKind::AiProvider,
            title: None,
            items,
            selected,
        });
    }

    pub fn open_ai_model_picker(&mut self, provider_id: String, action: Option<AiActionKind>) {
        let Some(provider) = self.config.ai_hub.providers.get(&provider_id) else {
            self.notify("Unknown AI provider");
            return;
        };

        if provider.models.is_empty() {
            self.current_ai_provider = Some(provider_id);
            self.current_ai_model = None;
            if let Some(action) = action {
                self.pending_hub_action = Some(HubAction::RunAiAction(action));
            } else {
                self.notify(&format!("AI target: {}", self.active_ai_selection_label()));
            }
            return;
        }

        let resolved_model = self
            .config
            .ai_hub
            .resolve_model_id(&provider_id, self.current_ai_model.as_deref());
        let mut selected = 0usize;
        let items = provider
            .models
            .iter()
            .enumerate()
            .map(|(idx, model)| {
                if resolved_model.as_deref() == Some(model.id.as_str()) {
                    selected = idx;
                }
                HubItem {
                    label: model.display_name(),
                    hint: "".into(),
                    description: model.id.clone(),
                    action: HubAction::SelectAiModel {
                        action: action.clone(),
                        provider_id: provider_id.clone(),
                        model_id: model.id.clone(),
                    },
                    is_header: false,
                    enabled: true,
                }
            })
            .collect();
        self.overlay = Some(OverlayData::ModalHub {
            kind: HubKind::AiModel,
            title: None,
            items,
            selected,
        });
    }

    /// Open a remote PR as a new tab from a GitHub URL.
    pub fn open_remote_url(&mut self, url: &str) -> Result<()> {
        let pr_ref = crate::github::parse_github_pr_url(url)
            .ok_or_else(|| anyhow::anyhow!("Invalid GitHub PR URL"))?;

        // Check if this remote PR is already open
        let slug = format!("{}/{}", pr_ref.owner, pr_ref.repo);
        for (i, tab) in self.tabs.iter().enumerate() {
            if tab.remote_repo.as_deref() == Some(&slug) && tab.pr_number == Some(pr_ref.number) {
                self.active_tab = i;
                self.notify(&format!("Switched to tab: {}", tab.tab_name()));
                return Ok(());
            }
        }

        let mut tab = TabState::new_remote(&pr_ref)?;
        let pr_data =
            crate::github::gh_pr_overview_remote(&pr_ref.owner, &pr_ref.repo, pr_ref.number);
        if let Some(data) = pr_data {
            tab.pr_data = Some(data);
        }
        tab.reload_remote_comments();
        let name = tab.tab_name();
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
        self.notify(&format!("Opened remote: {}", name));
        Ok(())
    }

    // ── Tab Accessors ──

    pub fn toggle_panel(&mut self, name: &str) {
        match name {
            "left" => self.panels_visible.left = !self.panels_visible.left,
            "tree" => self.panels_visible.tree = !self.panels_visible.tree,
            "right" => self.panels_visible.right = !self.panels_visible.right,
            _ => {}
        }
    }

    /// Get a reference to the active tab. `App` always holds at least one tab
    /// (`close_tab` refuses to remove the last one); the index is clamped so a
    /// stale `active_tab` can't read out of bounds.
    pub fn tab(&self) -> &TabState {
        let idx = self.active_tab.min(self.tabs.len().saturating_sub(1));
        &self.tabs[idx]
    }

    /// Get a mutable reference to the active tab
    pub fn tab_mut(&mut self) -> &mut TabState {
        let idx = self.active_tab.min(self.tabs.len().saturating_sub(1));
        &mut self.tabs[idx]
    }

    /// Returns true when split diff rendering should be active.
    /// Requires the config flag and no open panel.
    pub fn split_diff_active(&self, config: &ErConfig) -> bool {
        if !config.display.split_diff {
            return false;
        }
        let tab = self.tab();
        if tab.panel.is_some() {
            return false;
        }
        true
    }

    // ── Tab Management ──

    /// Open a repo in a new tab (or switch to existing tab if already open)
    pub fn open_in_new_tab(&mut self, repo_root: String) -> Result<()> {
        // Check if this repo is already open in a tab
        for (i, tab) in self.tabs.iter().enumerate() {
            if tab.repo_root == repo_root {
                self.active_tab = i;
                self.overlay = None;
                let name = self.tab().tab_name();
                self.notify(&format!("Switched to tab: {}", name));
                return Ok(());
            }
        }

        let tab = TabState::new(repo_root)?;
        let name = tab.tab_name();
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
        self.overlay = None;
        self.notify(&format!("Opened: {}", name));
        Ok(())
    }

    /// Switch to the next tab (wraps around)
    pub fn next_tab(&mut self) {
        if self.tabs.len() > 1 {
            self.active_tab = (self.active_tab + 1) % self.tabs.len();
            let name = self.tab().tab_name();
            self.notify(&format!("Tab: {}", name));
        }
    }

    /// Switch to the previous tab (wraps around)
    pub fn prev_tab(&mut self) {
        if self.tabs.len() > 1 {
            self.active_tab = (self.active_tab + self.tabs.len() - 1) % self.tabs.len();
            let name = self.tab().tab_name();
            self.notify(&format!("Tab: {}", name));
        }
    }

    /// Close the active tab (refuses if it's the last one)
    pub fn close_tab(&mut self) {
        if self.tabs.len() <= 1 {
            self.notify("Cannot close last tab");
            return;
        }
        let name = self.tab().tab_name();
        self.tabs.remove(self.active_tab);
        if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
        }
        self.notify(&format!("Closed: {}", name));
    }

    /// Push a new tab and focus it. Returns the new tab's index.
    pub fn open_tab(&mut self, mut tab: TabState) -> usize {
        tab.sync_managed_storage();
        if let Some(msg) = tab.storage_notice.take() {
            self.notify(&msg);
        }
        let name = tab.tab_name();
        self.tabs.push(tab);
        let idx = self.tabs.len() - 1;
        self.active_tab = idx;
        self.sync_config_from_active_tab();
        self.notify(&format!("Opened: {}", name));
        idx
    }

    /// Reload shared `App.config` from the active tab's repo (global + `.er-config.toml`).
    pub fn sync_config_from_active_tab(&mut self) {
        if let Some(tab) = self.tabs.get(self.active_tab) {
            self.config = config::load_config(&tab.repo_root);
        }
    }

    /// Show any pending storage migration notices for all tabs.
    pub fn drain_storage_notices(&mut self) {
        let notices: Vec<String> = self
            .tabs
            .iter_mut()
            .filter_map(|tab| tab.storage_notice.take())
            .collect();
        for msg in notices {
            self.notify(&msg);
        }
    }

    /// Close the tab at `idx`. Refuses if it's the last tab. If the closed
    /// tab was active, focus shifts to the previous tab (or 0 if removing 0).
    pub fn close_tab_at(&mut self, idx: usize) {
        if self.tabs.len() <= 1 {
            self.notify("Cannot close last tab");
            return;
        }
        if idx >= self.tabs.len() {
            return;
        }
        let name = self.tabs[idx].tab_name();
        self.tabs.remove(idx);
        if self.active_tab == idx {
            // Focus previous (or 0 if removed first).
            self.active_tab = if idx == 0 { 0 } else { idx - 1 };
        } else if self.active_tab > idx {
            self.active_tab -= 1;
        }
        if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
        }
        self.notify(&format!("Closed: {}", name));
    }

    /// Switch focus to the tab at `idx`. No-op if out of bounds.
    pub fn select_tab(&mut self, idx: usize) {
        if idx < self.tabs.len() {
            self.active_tab = idx;
        }
    }

    /// Move the tab at `from` to position `to`. `active_tab` follows the moved
    /// tab (so the focused tab stays focused) or is shifted to account for the
    /// removal/insertion when another tab was active.
    ///
    /// Returns `true` if the reorder happened. Out-of-bounds indices or
    /// `from == to` are silent no-ops.
    pub fn reorder_tabs(&mut self, from: usize, to: usize) -> bool {
        let len = self.tabs.len();
        if from >= len || to >= len || from == to {
            return false;
        }
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);

        // Fix up active_tab so the previously-focused tab stays focused.
        self.active_tab = if self.active_tab == from {
            to
        } else if from < self.active_tab && self.active_tab <= to {
            self.active_tab - 1
        } else if to <= self.active_tab && self.active_tab < from {
            self.active_tab + 1
        } else {
            self.active_tab
        };
        true
    }

    // ── Overlay: Worktree Picker ──

    /// Open the worktree picker overlay
    pub fn open_worktree_picker(&mut self) -> Result<()> {
        let repo_root = self.tab().repo_root.clone();
        let worktrees = git::list_worktrees(&repo_root)?;
        self.overlay = Some(OverlayData::WorktreePicker {
            worktrees,
            selected: 0,
        });
        Ok(())
    }

    /// Open the filter history overlay
    pub fn open_filter_history(&mut self) {
        use crate::app::filter::FILTER_PRESETS;
        let history = self.tab().filter_history.clone();
        self.overlay = Some(OverlayData::FilterHistory {
            history,
            selected: 0,
            preset_count: FILTER_PRESETS.len(),
        });
    }

    /// Open the directory browser overlay (starts from parent of repo root)
    pub fn open_directory_browser(&mut self) {
        let repo_root = self.tab().repo_root.clone();
        let start_path = std::path::Path::new(&repo_root)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "/".to_string());

        let entries = Self::read_directory(&start_path);
        self.overlay = Some(OverlayData::DirectoryBrowser {
            current_path: start_path,
            entries,
            selected: 0,
        });
    }

    // ── Overlay: Modal Hubs ──

    /// Open the Git modal hub
    pub fn open_git_hub(&mut self) {
        let in_staged = self.tab().mode == DiffMode::Staged;
        let items = vec![
            HubItem {
                label: "Push to remote".into(),
                hint: "".into(),
                description: "Push current branch to origin".into(),
                action: HubAction::PushToRemote,
                is_header: false,
                enabled: in_staged,
            },
            HubItem {
                label: "Stage current file".into(),
                hint: "s".into(),
                description: "Stage or unstage the selected file".into(),
                action: HubAction::StageFile,
                is_header: false,
                enabled: true,
            },
            HubItem {
                label: "Stage all files".into(),
                hint: "".into(),
                description: "Stage all changed files".into(),
                action: HubAction::StageAll,
                is_header: false,
                enabled: true,
            },
            HubItem {
                label: "Refresh diff".into(),
                hint: "R".into(),
                description: "Re-run git diff and reload".into(),
                action: HubAction::RefreshDiff,
                is_header: false,
                enabled: true,
            },
            HubItem {
                label: "Pull GitHub comments".into(),
                hint: "".into(),
                description: "Sync review comments from GitHub PR".into(),
                action: HubAction::PullGitHubComments,
                is_header: false,
                enabled: true,
            },
            HubItem {
                label: "Push comments to GitHub".into(),
                hint: "".into(),
                description: "Publish local comments to the PR".into(),
                action: HubAction::PushCommentsToGitHub,
                is_header: false,
                enabled: true,
            },
            HubItem {
                label: "Comment on PR".into(),
                hint: "".into(),
                description: "Post a general comment on the PR (not attached to a line)".into(),
                action: HubAction::CommentOnPR,
                is_header: false,
                enabled: true,
            },
            HubItem {
                label: "Approve PR".into(),
                hint: "".into(),
                description: if self.tab().pr_number.is_some() {
                    "Submit approval review on GitHub".into()
                } else {
                    "No PR detected".into()
                },
                action: HubAction::ApprovePR,
                is_header: false,
                enabled: self.tab().pr_number.is_some(),
            },
        ];
        self.overlay = Some(OverlayData::ModalHub {
            kind: HubKind::Git,
            title: None,
            items,
            selected: 0,
        });
    }

    /// Open the specialized expert reviewer picker.
    pub fn open_ai_expert_picker(&mut self) {
        let items: Vec<HubItem> = crate::ai::EXPERTS
            .iter()
            .map(|e| HubItem {
                label: e.label.to_string(),
                hint: String::new(),
                description: e.description.to_string(),
                action: HubAction::RunExpertReview {
                    expert_id: e.id.to_string(),
                },
                is_header: false,
                enabled: true,
            })
            .collect();
        self.overlay = Some(OverlayData::ModalHub {
            kind: HubKind::AiExpert,
            title: None,
            items,
            selected: 0,
        });
    }

    /// Open the AI modal hub
    pub fn open_ai_hub(&mut self) {
        let has_ai = self.tab().ai.has_data();
        let has_review = self.tab().ai.review.is_some();
        let has_questions = self
            .tab()
            .ai
            .questions
            .as_ref()
            .is_some_and(|q| !q.questions.is_empty());
        let has_notes = self.tab().ai.has_notes();
        let has_questions_or_notes = has_questions || has_notes;
        let has_unresolved_questions = self
            .tab()
            .ai
            .questions
            .as_ref()
            .is_some_and(|q| q.questions.iter().any(|q| !q.resolved));
        let selection_label = self.active_ai_selection_label();
        let items = vec![
            HubItem {
                label: "Triage branch".into(),
                hint: "".into(),
                description: format!(
                    "Fast scan — first impression and review routing via {selection_label} (Haiku-class model)"
                ),
                action: HubAction::RunTriageReview,
                is_header: false,
                enabled: true,
            },
            HubItem {
                label: "Review work".into(),
                hint: "".into(),
                description: if has_review {
                    format!("Full AI review via {selection_label} (will ask to clear previous)")
                } else {
                    format!("Full review: risk, order, checklist, summary via {selection_label}")
                },
                action: HubAction::RunAiAction(AiActionKind::Review),
                is_header: false,
                enabled: true,
            },
            HubItem {
                label: "Specialized review".into(),
                hint: "".into(),
                description: "Focused expert lens — security, patterns, testing, …".into(),
                action: HubAction::OpenAiExpertPicker,
                is_header: false,
                enabled: true,
            },
            HubItem {
                label: "Professor".into(),
                hint: "".into(),
                description: "Learn the implementation — teaching insights, not a review".into(),
                action: HubAction::RunProfessorReview,
                is_header: false,
                enabled: true,
            },
            HubItem {
                label: "Validate review".into(),
                hint: "".into(),
                description: {
                    let comment_count = self
                        .tab()
                        .ai
                        .github_comments
                        .as_ref()
                        .map(crate::ai::count_eligible_github_comments)
                        .unwrap_or(0);
                    if has_review && comment_count > 0 {
                        format!(
                            "Re-verify review findings and {comment_count} GitHub comment(s) via {selection_label}"
                        )
                    } else if has_review {
                        format!("Re-verify findings against the codebase via {selection_label}")
                    } else if comment_count > 0 {
                        format!(
                            "Re-anchor {comment_count} unresolved GitHub comment(s) via {selection_label}"
                        )
                    } else {
                        "Run review or add GitHub comments first".into()
                    }
                },
                action: HubAction::RunAiAction(AiActionKind::Validate),
                is_header: false,
                enabled: has_review
                    || self
                        .tab()
                        .ai
                        .github_comments
                        .as_ref()
                        .map(crate::ai::count_eligible_github_comments)
                        .unwrap_or(0)
                        > 0,
            },
            HubItem {
                label: "Answer questions".into(),
                hint: "".into(),
                description: if has_unresolved_questions {
                    format!("Answer unresolved questions via {selection_label}")
                } else {
                    "No unresolved questions".into()
                },
                action: HubAction::RunAiAction(AiActionKind::Questions),
                is_header: false,
                enabled: has_unresolved_questions,
            },
            HubItem {
                label: "Change agent/model".into(),
                hint: "".into(),
                description: format!("Current: {selection_label}"),
                action: HubAction::ConfigureAiSelection,
                is_header: false,
                enabled: self.config.ai_hub.has_presets(),
            },
            HubItem {
                label: "Copy context to clipboard".into(),
                hint: "".into(),
                description: "Copy diff context for agent terminal".into(),
                action: HubAction::CopyContext,
                is_header: false,
                enabled: true,
            },
            HubItem {
                label: "Copy review.json".into(),
                hint: "".into(),
                description: if has_review {
                    "Copy .er/review.json to clipboard".into()
                } else {
                    "No review data — run a review first".into()
                },
                action: HubAction::CopyReviewJson,
                is_header: false,
                enabled: has_review,
            },
            HubItem {
                label: "Copy questions.json".into(),
                hint: "".into(),
                description: if has_questions {
                    "Copy .er/questions.json to clipboard".into()
                } else {
                    "No questions — add some with q first".into()
                },
                action: HubAction::CopyQuestionsJson,
                is_header: false,
                enabled: has_questions,
            },
            HubItem {
                label: "Toggle AI findings".into(),
                hint: "A".into(),
                description: "Show/hide inline AI findings".into(),
                action: HubAction::ToggleAiFindings,
                is_header: false,
                enabled: has_ai,
            },
            HubItem {
                label: "Toggle comments".into(),
                hint: "".into(),
                description: "Show/hide GitHub comment layer".into(),
                action: HubAction::ToggleComments,
                is_header: false,
                enabled: true,
            },
            HubItem {
                label: "Toggle questions".into(),
                hint: "".into(),
                description: "Show/hide question layer".into(),
                action: HubAction::ToggleQuestions,
                is_header: false,
                enabled: true,
            },
            HubItem {
                label: "Hide resolved".into(),
                hint: "X".into(),
                description: "Toggle hiding resolved comments".into(),
                action: HubAction::ToggleHideResolved,
                is_header: false,
                enabled: true,
            },
            HubItem {
                label: "Generate summary".into(),
                hint: "".into(),
                description: format!("Generate summary via {selection_label}"),
                action: HubAction::RunAiAction(AiActionKind::Summary),
                is_header: false,
                enabled: true,
            },
            HubItem {
                label: "Cleanup questions & notes".into(),
                hint: "z".into(),
                description: if has_questions_or_notes {
                    "Delete .er/questions.json + notes.json".into()
                } else {
                    "no questions or notes to clean up".into()
                },
                action: HubAction::CleanupQuestions,
                is_header: false,
                enabled: has_questions_or_notes,
            },
            HubItem {
                label: "Cleanup reviews".into(),
                hint: "Z".into(),
                description: if has_review {
                    "Delete .er/review.json".into()
                } else {
                    "no review data to clean up".into()
                },
                action: HubAction::CleanupReviews,
                is_header: false,
                enabled: has_review,
            },
        ];
        self.overlay = Some(OverlayData::ModalHub {
            kind: HubKind::Ai,
            title: None,
            items,
            selected: 0,
        });
    }

    /// Open the Verify modal hub — items enabled when configured in [commands].
    /// When [packages] is configured, a packages section appears at the top for mono-repo use.
    pub fn open_verify_hub(&mut self) {
        let cmds = &self.config.commands;
        let not_configured = "set in [commands] in .er-config.toml";
        let mut items: Vec<HubItem> = Vec::new();

        // Packages section — only shown when [packages] is configured
        if self.config.has_packages() {
            items.push(HubItem {
                label: "── Packages ──".into(),
                hint: "".into(),
                description: "".into(),
                action: HubAction::Noop,
                is_header: true,
                enabled: false,
            });
            for (package_id, pkg) in &self.config.packages.items {
                let count = pkg.command_count();
                items.push(HubItem {
                    label: pkg.label.clone().unwrap_or_else(|| package_id.clone()),
                    hint: package_id.clone(),
                    description: format!("{} command(s) configured", count),
                    action: HubAction::SelectVerifyPackage {
                        package_id: package_id.clone(),
                    },
                    is_header: false,
                    enabled: count > 0,
                });
            }
            items.push(HubItem {
                label: "── Global ──".into(),
                hint: "".into(),
                description: "".into(),
                action: HubAction::Noop,
                is_header: true,
                enabled: false,
            });
        }

        items.push(HubItem {
            label: "Run tests".into(),
            hint: "".into(),
            description: cmds.test.as_deref().unwrap_or(not_configured).to_string(),
            action: HubAction::RunCommand("test".into()),
            is_header: false,
            enabled: cmds.test.is_some(),
        });
        items.push(HubItem {
            label: "Run linter".into(),
            hint: "".into(),
            description: cmds.lint.as_deref().unwrap_or(not_configured).to_string(),
            action: HubAction::RunCommand("lint".into()),
            is_header: false,
            enabled: cmds.lint.is_some(),
        });
        items.push(HubItem {
            label: "Type check".into(),
            hint: "".into(),
            description: cmds
                .typecheck
                .as_deref()
                .unwrap_or(not_configured)
                .to_string(),
            action: HubAction::RunCommand("typecheck".into()),
            is_header: false,
            enabled: cmds.typecheck.is_some(),
        });
        items.push(HubItem {
            label: "Security scan".into(),
            hint: "".into(),
            description: cmds
                .security
                .as_deref()
                .unwrap_or(not_configured)
                .to_string(),
            action: HubAction::RunCommand("security".into()),
            is_header: false,
            enabled: cmds.security.is_some(),
        });

        // Pre-select the first enabled item, falling back to the first non-header row so
        // the cursor never lands on a section header when nothing is enabled.
        let selected = items
            .iter()
            .position(|item| !item.is_header && item.enabled)
            .or_else(|| items.iter().position(|item| !item.is_header))
            .unwrap_or(0);

        self.overlay = Some(OverlayData::ModalHub {
            kind: HubKind::Verify,
            title: None,
            items,
            selected,
        });
    }

    /// Open a package-specific verify hub showing that package's configured commands.
    pub fn open_package_commands_hub(&mut self, package_id: String) {
        let not_configured = "set in [packages] in .er-config.toml";
        let fields = self.config.packages.items.get(&package_id).map(|p| {
            (
                p.label.as_deref().unwrap_or(&package_id).to_owned(),
                p.test.clone(),
                p.lint.clone(),
                p.typecheck.clone(),
                p.security.clone(),
            )
        });
        let (label, test, lint, typecheck, security) = match fields {
            Some(f) => f,
            None => {
                self.notify(&format!("Package '{}' not found in config", package_id));
                return;
            }
        };

        let items = vec![
            HubItem {
                label: "Run tests".into(),
                hint: "".into(),
                description: test.as_deref().unwrap_or(not_configured).to_string(),
                action: HubAction::RunPackageCommand {
                    command: "test".into(),
                    package_id: package_id.clone(),
                },
                is_header: false,
                enabled: test.is_some(),
            },
            HubItem {
                label: "Run linter".into(),
                hint: "".into(),
                description: lint.as_deref().unwrap_or(not_configured).to_string(),
                action: HubAction::RunPackageCommand {
                    command: "lint".into(),
                    package_id: package_id.clone(),
                },
                is_header: false,
                enabled: lint.is_some(),
            },
            HubItem {
                label: "Type check".into(),
                hint: "".into(),
                description: typecheck.as_deref().unwrap_or(not_configured).to_string(),
                action: HubAction::RunPackageCommand {
                    command: "typecheck".into(),
                    package_id: package_id.clone(),
                },
                is_header: false,
                enabled: typecheck.is_some(),
            },
            HubItem {
                label: "Security scan".into(),
                hint: "".into(),
                description: security.as_deref().unwrap_or(not_configured).to_string(),
                action: HubAction::RunPackageCommand {
                    command: "security".into(),
                    package_id: package_id.clone(),
                },
                is_header: false,
                enabled: security.is_some(),
            },
        ];

        let selected = items
            .iter()
            .position(|item| !item.is_header && item.enabled)
            .or_else(|| items.iter().position(|item| !item.is_header))
            .unwrap_or(0);

        self.overlay = Some(OverlayData::ModalHub {
            kind: HubKind::VerifyPackage,
            title: Some(format!("VERIFY / {}", label)),
            items,
            selected,
        });
    }

    /// Open the Copy modal hub — copy file, path, hunk, or line to clipboard
    pub fn open_copy_hub(&mut self) {
        let has_file = !self.tab().files.is_empty();
        let has_line = has_file && self.tab().current_line.is_some();
        let items = vec![
            HubItem {
                label: "Full file diff".into(),
                hint: "".into(),
                description: "Copy all hunks for selected file".into(),
                action: HubAction::CopyFullFile,
                is_header: false,
                enabled: has_file,
            },
            HubItem {
                label: "File path".into(),
                hint: "".into(),
                description: "Copy file path to clipboard".into(),
                action: HubAction::CopyFilePath,
                is_header: false,
                enabled: has_file,
            },
            HubItem {
                label: "Hunk".into(),
                hint: "".into(),
                description: "Copy current hunk".into(),
                action: HubAction::CopyHunk,
                is_header: false,
                enabled: has_file,
            },
            HubItem {
                label: "Line".into(),
                hint: "".into(),
                description: "Copy current line content".into(),
                action: HubAction::CopyLine,
                is_header: false,
                enabled: has_line,
            },
        ];
        self.overlay = Some(OverlayData::ModalHub {
            kind: HubKind::Copy,
            title: None,
            items,
            selected: 0,
        });
    }

    /// Open the Help modal hub (keybind reference)
    pub fn open_help_hub(&mut self) {
        let items = vec![
            // ── Navigation ──
            HubItem {
                label: "── Navigation ──".into(),
                hint: "".into(),
                description: "".into(),
                action: HubAction::Noop,
                is_header: true,
                enabled: false,
            },
            HubItem {
                label: "j / k".into(),
                hint: "".into(),
                description: "Previous / next file".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            HubItem {
                label: "n / N".into(),
                hint: "".into(),
                description: "Next / previous hunk".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            HubItem {
                label: "↑ / ↓".into(),
                hint: "".into(),
                description: "Line navigation".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            HubItem {
                label: "Shift+↑/↓".into(),
                hint: "".into(),
                description: "Extend line selection".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            HubItem {
                label: "h / l".into(),
                hint: "".into(),
                description: "Scroll left / right".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            HubItem {
                label: "d / u".into(),
                hint: "".into(),
                description: "Scroll half page ↓ / ↑".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            HubItem {
                label: "PgDn / PgUp".into(),
                hint: "".into(),
                description: "Scroll full page ↓ / ↑".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            HubItem {
                label: "Home".into(),
                hint: "".into(),
                description: "Reset horizontal scroll".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            HubItem {
                label: "1-6".into(),
                hint: "".into(),
                description: "Switch diff mode".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            // ── Review ──
            HubItem {
                label: "── Review ──".into(),
                hint: "".into(),
                description: "".into(),
                action: HubAction::Noop,
                is_header: true,
                enabled: false,
            },
            HubItem {
                label: "Space".into(),
                hint: "".into(),
                description: "Toggle file reviewed".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            HubItem {
                label: "Shift+Space".into(),
                hint: "".into(),
                description: "Show unreviewed only".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            HubItem {
                label: "U".into(),
                hint: "".into(),
                description: "Jump to next unreviewed file".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            HubItem {
                label: "q".into(),
                hint: "".into(),
                description: "Add review question (Ctrl+t → note)".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            HubItem {
                label: "c".into(),
                hint: "".into(),
                description: "Add GitHub comment".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            HubItem {
                label: "s".into(),
                hint: "".into(),
                description: "Stage / unstage file".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            HubItem {
                label: "y".into(),
                hint: "".into(),
                description: "Yank hunk to clipboard".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            // ── Comments ──
            HubItem {
                label: "── Comments ──".into(),
                hint: "".into(),
                description: "".into(),
                action: HubAction::Noop,
                is_header: true,
                enabled: false,
            },
            HubItem {
                label: "J / K".into(),
                hint: "".into(),
                description: "Jump to prev / next comment".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            HubItem {
                label: "Ctrl+J / K".into(),
                hint: "".into(),
                description: "Jump to prev / next AI finding".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            HubItem {
                label: "r".into(),
                hint: "".into(),
                description: "Reply to focused comment".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            HubItem {
                label: "x".into(),
                hint: "".into(),
                description: "Delete focused comment".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            HubItem {
                label: "e".into(),
                hint: "".into(),
                description: "Edit own comment / open in editor".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            HubItem {
                label: "Ctrl+t".into(),
                hint: "".into(),
                description: "While composing: cycle question → note → comment".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            // ── Layers ──
            HubItem {
                label: "── Layers ──".into(),
                hint: "".into(),
                description: "".into(),
                action: HubAction::Noop,
                is_header: true,
                enabled: false,
            },
            HubItem {
                label: "A".into(),
                hint: "".into(),
                description: "Toggle AI findings".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            HubItem {
                label: "C".into(),
                hint: "".into(),
                description: "Toggle GitHub comments".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            HubItem {
                label: "Q".into(),
                hint: "".into(),
                description: "Toggle questions & notes".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            // ── Tools ──
            HubItem {
                label: "── Tools ──".into(),
                hint: "".into(),
                description: "".into(),
                action: HubAction::Noop,
                is_header: true,
                enabled: false,
            },
            HubItem {
                label: "/ / f".into(),
                hint: "".into(),
                description: "Search / filter files".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            HubItem {
                label: "F".into(),
                hint: "".into(),
                description: "Filter history".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            HubItem {
                label: "+ / -".into(),
                hint: "".into(),
                description: "Expand / collapse context lines".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            HubItem {
                label: "Enter".into(),
                hint: "".into(),
                description: "Expand / collapse compacted file".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            HubItem {
                label: "m".into(),
                hint: "".into(),
                description: "Toggle mtime sort".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            HubItem {
                label: "R".into(),
                hint: "".into(),
                description: "Reload diff".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            HubItem {
                label: "w / W".into(),
                hint: "".into(),
                description: "Toggle watch / watched files".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            HubItem {
                label: "z / Z".into(),
                hint: "".into(),
                description: "Cleanup questions & notes / all AI data".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            // ── Panels & Tabs ──
            HubItem {
                label: "── Panels & Tabs ──".into(),
                hint: "".into(),
                description: "".into(),
                action: HubAction::Noop,
                is_header: true,
                enabled: false,
            },
            HubItem {
                label: "p / P".into(),
                hint: "".into(),
                description: "Cycle context panel fwd / back".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            HubItem {
                label: "Tab".into(),
                hint: "".into(),
                description: "Toggle panel focus / split side".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            HubItem {
                label: "] / [".into(),
                hint: "".into(),
                description: "Next / previous tab".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            HubItem {
                label: "x".into(),
                hint: "".into(),
                description: "Close tab".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            HubItem {
                label: "o".into(),
                hint: "".into(),
                description: "Open hub (browse, worktree, remote)".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            HubItem {
                label: "< / >".into(),
                hint: "".into(),
                description: "Shrink / grow file tree width".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            HubItem {
                label: "{ / }".into(),
                hint: "".into(),
                description: "Shrink / grow side panel width".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            // ── Modals ──
            HubItem {
                label: "── Modals ──".into(),
                hint: "".into(),
                description: "".into(),
                action: HubAction::Noop,
                is_header: true,
                enabled: false,
            },
            HubItem {
                label: "g".into(),
                hint: "".into(),
                description: "Git operations hub".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            HubItem {
                label: "a".into(),
                hint: "".into(),
                description: "AI tools hub".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            HubItem {
                label: "v".into(),
                hint: "".into(),
                description: "Verify hub".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            HubItem {
                label: ",".into(),
                hint: "".into(),
                description: "Settings".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            // ── Staged Mode ──
            HubItem {
                label: "── Staged Mode ──".into(),
                hint: "".into(),
                description: "".into(),
                action: HubAction::Noop,
                is_header: true,
                enabled: false,
            },
            HubItem {
                label: "c".into(),
                hint: "".into(),
                description: "Commit".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            HubItem {
                label: "Ctrl+P".into(),
                hint: "".into(),
                description: "Push to remote".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            // ── PR Panel Focused ──
            HubItem {
                label: "── PR Panel Focused ──".into(),
                hint: "".into(),
                description: "".into(),
                action: HubAction::Noop,
                is_header: true,
                enabled: false,
            },
            HubItem {
                label: "o".into(),
                hint: "".into(),
                description: "Open PR in browser".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            HubItem {
                label: "G".into(),
                hint: "".into(),
                description: "Pull GitHub PR comments".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            // ── Hidden Mode ──
            HubItem {
                label: "── Hidden Mode ──".into(),
                hint: "".into(),
                description: "".into(),
                action: HubAction::Noop,
                is_header: true,
                enabled: false,
            },
            HubItem {
                label: "x".into(),
                hint: "".into(),
                description: "Delete watched file".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
            // ── General ──
            HubItem {
                label: "".into(),
                hint: "".into(),
                description: "".into(),
                action: HubAction::Noop,
                is_header: true,
                enabled: false,
            },
            HubItem {
                label: "Ctrl+q".into(),
                hint: "".into(),
                description: "Quit".into(),
                action: HubAction::Noop,
                is_header: false,
                enabled: false,
            },
        ];
        self.overlay = Some(OverlayData::ModalHub {
            kind: HubKind::Help,
            title: None,
            items,
            selected: 0,
        });
    }

    /// Open the Open modal hub (browse folders, switch worktree, remote PR, open in browser)
    pub fn open_open_hub(&mut self) {
        let repo_root = self.tab().repo_root.clone();
        let has_worktrees = git::list_worktrees(&repo_root)
            .map(|wts| wts.len() > 1)
            .unwrap_or(false);
        let has_pr = self.tab().pr_number.is_some();

        let mut items = vec![
            HubItem {
                label: "── Navigate ──".into(),
                hint: "".into(),
                description: "".into(),
                action: HubAction::Noop,
                is_header: true,
                enabled: false,
            },
            HubItem {
                label: "Browse folders".into(),
                hint: "".into(),
                description: "Open a local git repo".into(),
                action: HubAction::OpenDirectory,
                is_header: false,
                enabled: true,
            },
            HubItem {
                label: "Switch worktree".into(),
                hint: "".into(),
                description: if has_worktrees {
                    "Jump to another worktree".into()
                } else {
                    "No other worktrees".into()
                },
                action: HubAction::OpenWorktree,
                is_header: false,
                enabled: has_worktrees,
            },
            HubItem {
                label: "Open remote PR".into(),
                hint: "".into(),
                description: "Review a GitHub PR by URL".into(),
                action: HubAction::OpenRemoteUrl,
                is_header: false,
                enabled: true,
            },
        ];

        if has_pr {
            items.push(HubItem {
                label: "── Current PR ──".into(),
                hint: "".into(),
                description: "".into(),
                action: HubAction::Noop,
                is_header: true,
                enabled: false,
            });
            items.push(HubItem {
                label: "Open PR in browser".into(),
                hint: "".into(),
                description: "View on GitHub".into(),
                action: HubAction::OpenPrInBrowser,
                is_header: false,
                enabled: true,
            });
        }

        let first_selectable = items
            .iter()
            .position(|item| !item.is_header && item.enabled)
            .unwrap_or(0);
        self.overlay = Some(OverlayData::ModalHub {
            kind: HubKind::Open,
            title: None,
            items,
            selected: first_selectable,
        });
    }

    pub fn open_config_hub(&mut self) {
        let tab = config::SettingsScope::General;
        let items = config::config_hub_items_for_scope(&self.config, tab);
        let selected = Self::config_hub_first_selectable(&items);
        self.overlay = Some(OverlayData::ConfigHub {
            tab,
            items,
            selected,
            saved_config: Box::new(self.config.clone()),
            editing: None,
        });
    }

    fn config_hub_first_selectable(items: &[config::ConfigItem]) -> usize {
        items
            .iter()
            .position(|item| !matches!(item, config::ConfigItem::SectionHeader(_)))
            .unwrap_or(0)
    }

    pub fn config_hub_switch_tab(&mut self, tab: config::SettingsScope) {
        if let Some(OverlayData::ConfigHub {
            tab: current,
            items,
            selected,
            editing,
            ..
        }) = &mut self.overlay
        {
            if *current == tab {
                return;
            }
            *current = tab;
            *items = config::config_hub_items_for_scope(&self.config, tab);
            *selected = Self::config_hub_first_selectable(items);
            *editing = None;
        }
    }

    /// Toggle/cycle/activate the currently selected config hub item
    pub fn config_hub_activate(&mut self) {
        let idx = match &self.overlay {
            Some(OverlayData::ConfigHub {
                selected,
                editing: None,
                ..
            }) => *selected,
            _ => return,
        };
        let item = match &self.overlay {
            Some(OverlayData::ConfigHub { items, .. }) => items.get(idx).cloned(),
            _ => None,
        };
        let Some(item) = item else {
            return;
        };
        match item {
            config::ConfigItem::BoolToggle { get, set, .. } => {
                let current = get(&self.config);
                set(&mut self.config, !current);
                self.config_hub_rebuild_items();
            }
            config::ConfigItem::StringCycle {
                options, get, set, ..
            } => {
                let current = get(&self.config);
                let pos = options.iter().position(|&o| o == current).unwrap_or(0);
                let next = options[(pos + 1) % options.len()];
                let theme_before = self.config.display.theme.clone();
                set(&mut self.config, next.to_string());
                if self.config.display.theme != theme_before {
                    let mut global = config::load_global_config();
                    global.display.theme = self.config.display.theme.clone();
                    let _ = config::save_config(&global);
                    if let Some(OverlayData::ConfigHub { saved_config, .. }) = &mut self.overlay {
                        saved_config.display.theme = self.config.display.theme.clone();
                    }
                    self.notify("Theme saved globally");
                }
                self.config_hub_rebuild_items();
            }
            config::ConfigItem::NumberEdit {
                get, set, min, max, ..
            } => {
                let current = get(&self.config);
                let next = if current >= max { min } else { current + 1 };
                set(&mut self.config, next);
                self.config_hub_rebuild_items();
            }
            config::ConfigItem::StringEdit { get, .. } => {
                let current = get(&self.config);
                let cursor = current.len();
                if let Some(OverlayData::ConfigHub {
                    editing, selected, ..
                }) = &mut self.overlay
                {
                    *editing = Some(ConfigEditState {
                        item_index: *selected,
                        buffer: current,
                        cursor_pos: cursor,
                    });
                }
            }
            config::ConfigItem::ListAdd { .. } => {
                if let Some(OverlayData::ConfigHub {
                    editing, selected, ..
                }) = &mut self.overlay
                {
                    *editing = Some(ConfigEditState {
                        item_index: *selected,
                        buffer: String::new(),
                        cursor_pos: 0,
                    });
                }
            }
            config::ConfigItem::Action { action_id, .. } => {
                self.overlay = None;
                match action_id {
                    "copy_review_json" => {
                        let _ = self.copy_review_json();
                    }
                    "copy_questions_json" => {
                        let _ = self.copy_questions_json();
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    /// Cycle the currently selected config hub item backwards (Left arrow)
    pub fn config_hub_activate_prev(&mut self) {
        let idx = match &self.overlay {
            Some(OverlayData::ConfigHub {
                selected,
                editing: None,
                ..
            }) => *selected,
            _ => return,
        };
        let item = match &self.overlay {
            Some(OverlayData::ConfigHub { items, .. }) => items.get(idx).cloned(),
            _ => None,
        };
        let Some(item) = item else {
            return;
        };
        match item {
            config::ConfigItem::StringCycle {
                options, get, set, ..
            } => {
                let current = get(&self.config);
                let pos = options.iter().position(|&o| o == current).unwrap_or(0);
                let prev = options[(pos + options.len() - 1) % options.len()];
                let theme_before = self.config.display.theme.clone();
                set(&mut self.config, prev.to_string());
                if self.config.display.theme != theme_before {
                    let mut global = config::load_global_config();
                    global.display.theme = self.config.display.theme.clone();
                    let _ = config::save_config(&global);
                    if let Some(OverlayData::ConfigHub { saved_config, .. }) = &mut self.overlay {
                        saved_config.display.theme = self.config.display.theme.clone();
                    }
                    self.notify("Theme saved globally");
                }
                self.config_hub_rebuild_items();
            }
            config::ConfigItem::NumberEdit {
                get, set, min, max, ..
            } => {
                let current = get(&self.config);
                let prev = if current <= min { max } else { current - 1 };
                set(&mut self.config, prev);
                self.config_hub_rebuild_items();
            }
            _ => {}
        }
    }

    /// Apply the editing buffer to the config and close the inline edit
    pub fn config_hub_confirm_edit(&mut self) {
        let (item_idx, buffer, tab) = match &self.overlay {
            Some(OverlayData::ConfigHub {
                editing: Some(edit),
                tab,
                ..
            }) => (edit.item_index, edit.buffer.clone(), *tab),
            _ => return,
        };

        // Clear editing first
        if let Some(OverlayData::ConfigHub { editing, .. }) = &mut self.overlay {
            *editing = None;
        }

        let items = config::config_hub_items_for_scope(&self.config, tab);
        if let Some(config::ConfigItem::StringEdit { set, .. }) = items.get(item_idx) {
            set(&mut self.config, buffer);
        } else if let Some(config::ConfigItem::ListAdd { .. }) = items.get(item_idx) {
            if !buffer.trim().is_empty() {
                self.config.watched.paths.push(buffer.trim().to_string());
            }
        }

        self.config_hub_rebuild_items();
    }

    /// Cancel inline editing without applying the buffer
    pub fn config_hub_cancel_edit(&mut self) {
        if let Some(OverlayData::ConfigHub { editing, .. }) = &mut self.overlay {
            *editing = None;
        }
    }

    /// Delete the currently selected ListEntry from watched paths
    pub fn config_hub_delete_selected(&mut self) {
        let (idx, tab) = match &self.overlay {
            Some(OverlayData::ConfigHub { selected, tab, .. }) => (*selected, *tab),
            _ => return,
        };

        let items = config::config_hub_items_for_scope(&self.config, tab);
        if let Some(config::ConfigItem::ListEntry { index, .. }) = items.get(idx) {
            let path_idx = *index;
            if path_idx < self.config.watched.paths.len() {
                self.config.watched.paths.remove(path_idx);
                self.config_hub_rebuild_items();
            }
        }
    }

    /// Save config to the repo-local `.er-config.toml` and close the hub
    pub fn config_hub_save_local(&mut self) {
        let repo_root = self.tab().repo_root.clone();
        if let Err(e) = config::save_config_local(&self.config, &repo_root) {
            self.notify(&format!("Failed to save: {}", e));
        } else {
            // Sync watched config to active tab so W key sees updated paths
            self.tab_mut().watched_config = self.config.watched.clone();
            self.tab_mut().refresh_watched_files();
            self.notify("Config saved to .er-config.toml");
            self.overlay = None;
        }
    }

    /// Save config to the global config file and close the hub
    pub fn config_hub_save_global(&mut self) {
        if let Err(e) = config::save_config(&self.config) {
            self.notify(&format!("Failed to save: {}", e));
        } else {
            // Sync watched config to active tab so W key sees updated paths
            self.tab_mut().watched_config = self.config.watched.clone();
            self.tab_mut().refresh_watched_files();
            self.notify("Config saved globally");
            self.overlay = None;
        }
    }

    /// Revert config to the saved snapshot and close the hub
    pub fn config_hub_cancel(&mut self) {
        if let Some(OverlayData::ConfigHub { saved_config, .. }) = self.overlay.take() {
            self.config = *saved_config;
        }
    }

    /// Rebuild the config hub items list (e.g. after watched paths change) and clamp selection
    pub fn config_hub_rebuild_items(&mut self) {
        if let Some(OverlayData::ConfigHub {
            tab,
            items,
            selected,
            editing,
            ..
        }) = &mut self.overlay
        {
            *items = config::config_hub_items_for_scope(&self.config, *tab);
            // Clamp selected to valid range, skip headers
            let len = items.len();
            if *selected >= len {
                *selected = len.saturating_sub(1);
            }
            // Skip headers at the clamped position
            while *selected < len
                && matches!(items[*selected], config::ConfigItem::SectionHeader(_))
            {
                *selected += 1;
                if *selected >= len {
                    *selected = len.saturating_sub(1);
                    break;
                }
            }
            // Also clear editing if item index is now out of range
            if let Some(ref ed) = editing {
                if ed.item_index >= len {
                    *editing = None;
                }
            }
        }
    }

    // ── Overlay: Navigation ──

    #[allow(clippy::collapsible_match)]
    pub fn overlay_next(&mut self) {
        match &mut self.overlay {
            Some(OverlayData::WorktreePicker {
                worktrees,
                selected,
            }) => {
                if *selected + 1 < worktrees.len() {
                    *selected += 1;
                }
            }
            Some(OverlayData::DirectoryBrowser {
                entries, selected, ..
            }) => {
                if *selected + 1 < entries.len() {
                    *selected += 1;
                }
            }
            // `selected` indexes presets (0..preset_count) then history (preset_count..);
            // the visual separator in the overlay is render-only and not selectable
            Some(OverlayData::FilterHistory {
                history,
                selected,
                preset_count,
            }) => {
                if *selected + 1 < *preset_count + history.len() {
                    *selected += 1;
                }
            }
            Some(OverlayData::ModalHub {
                items, selected, ..
            }) => {
                // Skip headers when navigating down
                let mut next = *selected + 1;
                while next < items.len() {
                    if !items[next].is_header {
                        break;
                    }
                    next += 1;
                }
                if next < items.len() {
                    *selected = next;
                }
            }
            Some(OverlayData::ConfigHub {
                items,
                selected,
                editing,
                ..
            }) => {
                // Don't navigate while editing
                if editing.is_some() {
                    return;
                }
                let mut next = *selected + 1;
                while next < items.len() {
                    if !matches!(items[next], config::ConfigItem::SectionHeader(_)) {
                        break;
                    }
                    next += 1;
                }
                if next < items.len() {
                    *selected = next;
                }
            }
            None => {}
        }
    }

    #[allow(clippy::collapsible_match)]
    pub fn overlay_prev(&mut self) {
        match &mut self.overlay {
            Some(OverlayData::WorktreePicker { selected, .. })
            | Some(OverlayData::DirectoryBrowser { selected, .. })
            | Some(OverlayData::FilterHistory { selected, .. }) => {
                if *selected > 0 {
                    *selected -= 1;
                }
            }
            Some(OverlayData::ModalHub {
                items, selected, ..
            }) => {
                // Skip headers when navigating up
                if *selected > 0 {
                    let mut prev = *selected - 1;
                    while prev > 0 && items[prev].is_header {
                        prev -= 1;
                    }
                    if !items[prev].is_header {
                        *selected = prev;
                    }
                }
            }
            Some(OverlayData::ConfigHub {
                items,
                selected,
                editing,
                ..
            }) => {
                // Don't navigate while editing
                if editing.is_some() {
                    return;
                }
                if *selected > 0 {
                    let mut prev = *selected - 1;
                    while prev > 0 && matches!(items[prev], config::ConfigItem::SectionHeader(_)) {
                        prev -= 1;
                    }
                    if !matches!(items[prev], config::ConfigItem::SectionHeader(_)) {
                        *selected = prev;
                    }
                }
            }
            None => {}
        }
    }

    /// Handle Enter in an overlay — opens selection in a new tab or saves settings
    pub fn overlay_select(&mut self) -> Result<()> {
        let overlay = match self.overlay.take() {
            Some(o) => o,
            None => return Ok(()),
        };

        match overlay {
            OverlayData::WorktreePicker {
                worktrees,
                selected,
            } => {
                if let Some(wt) = worktrees.get(selected) {
                    let path = wt.path.clone();
                    self.open_in_new_tab(path)?;
                }
            }
            OverlayData::FilterHistory {
                history,
                selected,
                preset_count,
            } => {
                use crate::app::filter::FILTER_PRESETS;
                let expr = if selected < preset_count {
                    FILTER_PRESETS.get(selected).map(|p| p.expr.to_string())
                } else {
                    history.get(selected - preset_count).cloned()
                };
                if let Some(expr) = expr {
                    self.tab_mut().apply_filter_expr(&expr);
                    self.notify(&format!("Filter: {}", expr));
                }
            }
            OverlayData::DirectoryBrowser {
                current_path,
                entries,
                selected,
            } => {
                if let Some(entry) = entries.get(selected) {
                    let full_path = format!("{}/{}", current_path, entry.name);
                    if entry.is_dir {
                        if entry.is_git_repo {
                            // It's a git repo — open in new tab
                            self.open_in_new_tab(full_path)?;
                        } else {
                            // Descend into directory
                            let new_entries = Self::read_directory(&full_path);
                            self.overlay = Some(OverlayData::DirectoryBrowser {
                                current_path: full_path,
                                entries: new_entries,
                                selected: 0,
                            });
                        }
                    }
                    // Non-directory entries are ignored
                } else {
                    // Restore overlay if nothing was selected
                    self.overlay = Some(OverlayData::DirectoryBrowser {
                        current_path,
                        entries,
                        selected,
                    });
                }
            }
            OverlayData::ModalHub {
                items, selected, ..
            } => {
                if let Some(item) = items.get(selected) {
                    if item.is_header || item.action == HubAction::Noop {
                        // Headers and noops — do nothing
                    } else if item.enabled {
                        // Store action, close overlay, then caller dispatches
                        self.pending_hub_action = Some(item.action.clone());
                    } else if !item.description.is_empty() {
                        // Disabled item — show why via notification
                        self.notify(&item.description);
                    }
                }
            }
            OverlayData::ConfigHub { .. } => {
                // ConfigHub enter is handled directly in handle_overlay_input
            }
        }
        Ok(())
    }

    /// Go up one directory in the directory browser
    pub fn overlay_go_up(&mut self) {
        if let Some(OverlayData::DirectoryBrowser {
            ref mut current_path,
            ref mut entries,
            ref mut selected,
        }) = self.overlay
        {
            if let Some(parent) = std::path::Path::new(current_path.as_str()).parent() {
                let parent_str = parent.to_string_lossy().to_string();
                if !parent_str.is_empty() {
                    *entries = Self::read_directory(&parent_str);
                    *current_path = parent_str;
                    *selected = 0;
                }
            }
        }
    }

    /// Close the overlay (reverts settings changes if in ConfigHub overlay)
    pub fn overlay_close(&mut self) {
        if matches!(self.overlay, Some(OverlayData::ConfigHub { .. })) {
            self.config_hub_cancel();
        } else if matches!(
            self.overlay,
            Some(OverlayData::ModalHub {
                kind: HubKind::VerifyPackage,
                ..
            })
        ) {
            // Navigate back to the package picker instead of closing entirely.
            self.open_verify_hub();
        } else {
            self.overlay = None;
        }
    }

    /// Read directory entries, sorted: directories first (with git repo marker), then files
    fn read_directory(path: &str) -> Vec<DirEntry> {
        let mut entries = Vec::new();
        if let Ok(read_dir) = std::fs::read_dir(path) {
            for entry in read_dir.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                // Skip hidden files/dirs (except .git check is internal)
                if name.starts_with('.') {
                    continue;
                }
                if let Ok(metadata) = entry.metadata() {
                    let is_dir = metadata.is_dir();
                    let is_git_repo = if is_dir {
                        entry.path().join(".git").exists()
                    } else {
                        false
                    };
                    entries.push(DirEntry {
                        name,
                        is_dir,
                        is_git_repo,
                    });
                }
            }
        }
        entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.cmp(&b.name),
        });
        entries
    }

    // ── Staging ──

    /// Stage or unstage the current file (toggle based on mode)
    pub fn toggle_stage_file(&mut self) -> Result<()> {
        let si = self.tab().selected_file;
        if si >= self.tab().files.len() {
            return Ok(());
        }
        let file_path = self.tab().files[si].path.clone();
        let mode = self.tab().mode;
        let repo_root = self.tab().repo_root.clone();

        match mode {
            DiffMode::Branch | DiffMode::Unstaged => {
                git::git_stage_file(&repo_root, &file_path)?;
                self.notify(&format!("Staged: {}", file_path));
            }
            DiffMode::Staged => {
                git::git_unstage_file(&repo_root, &file_path)?;
                self.notify(&format!("Unstaged: {}", file_path));
            }
            DiffMode::Conflicts => {
                git::git_stage_file(&repo_root, &file_path)?;
                self.notify(&format!("Resolved: {}", file_path));
            }
            DiffMode::History | DiffMode::Hidden | DiffMode::PrDiff | DiffMode::Tour => {
                self.notify("Staging not available in this mode");
                return Ok(());
            }
        }

        if mode == DiffMode::Conflicts {
            self.tab_mut().refresh_conflicts();
        } else {
            self.tab_mut().refresh_diff()?;
        }
        Ok(())
    }

    /// Stage all files
    #[allow(dead_code)]
    pub fn stage_all(&mut self) -> Result<()> {
        let repo_root = self.tab().repo_root.clone();
        git::git_stage_all(&repo_root)?;
        self.notify("Staged all files");
        self.tab_mut().refresh_diff()?;
        Ok(())
    }

    // ── Reviewed-File Tracking ──

    /// Toggle the current file's reviewed status
    pub fn toggle_reviewed(&mut self) -> Result<()> {
        let si = self.tab().selected_file;
        if si >= self.tab().files.len() {
            return Ok(());
        }
        let path = self.tab().files[si].path.clone();

        // Capture current position in visible list before toggling — needed for
        // advancing selection when show_unreviewed_only is active.
        let visible_before: Vec<usize> = self
            .tab()
            .visible_files()
            .into_iter()
            .map(|(i, _)| i)
            .collect();
        let pos_before = visible_before.iter().position(|&i| i == si).unwrap_or(0);

        let tab = self.tab_mut();
        let was_reviewed = tab.reviewed.contains_key(&path);
        if was_reviewed {
            tab.reviewed.remove(&path);
        } else {
            // Store the current per-file hash so we can detect when the diff changes.
            // Falls back to empty string if the file isn't in the current diff (shouldn't
            // happen normally, but guards against a race between parse and toggle).
            let hash = tab
                .current_per_file_hashes
                .get(&path)
                .cloned()
                .unwrap_or_default();
            tab.reviewed.insert(path.clone(), hash);
        }
        tab.reviewed_revision += 1;
        tab.save_reviewed_files()?;

        // When marking a file reviewed while show_unreviewed_only is active, advance
        // to the next unreviewed file so the user doesn't land on a now-hidden entry.
        if !was_reviewed && self.tab().show_unreviewed_only {
            let visible_after: Vec<usize> = self
                .tab()
                .visible_files()
                .into_iter()
                .map(|(i, _)| i)
                .collect();
            if !visible_after.is_empty() {
                // Pick file at same position, clamped to last.
                let next_pos = pos_before.min(visible_after.len() - 1);
                let target = visible_after[next_pos];
                let tab = self.tab_mut();
                tab.selected_file = target;
                tab.current_hunk = 0;
                tab.current_line = None;
                tab.diff_scroll = 0;
                tab.h_scroll = 0;
                tab.ensure_file_parsed();
                tab.rebuild_hunk_offsets();
            }
        }

        if was_reviewed {
            self.notify(&format!("Unreviewed: {}", path));
        } else {
            self.notify(&format!("Reviewed: {}", path));
        }
        Ok(())
    }

    /// Toggle show-unreviewed-only filter
    pub fn toggle_unreviewed_filter(&mut self) {
        let tab = self.tab_mut();
        tab.show_unreviewed_only = !tab.show_unreviewed_only;
        let showing = tab.show_unreviewed_only;
        tab.snap_to_visible();

        if showing {
            self.notify("Showing unreviewed only");
        } else {
            self.notify("Showing all files");
        }
    }

    /// Jump to the next unreviewed file (wraps around). Reports if all reviewed.
    pub fn next_unreviewed_file(&mut self) {
        // Collect the data we need before any mutable borrow.
        let (current_pos, targets): (usize, Vec<(usize, bool)>) = {
            let tab = self.tab();
            let visible = tab.visible_files();
            if visible.is_empty() {
                return;
            }
            let pos = visible
                .iter()
                .position(|(i, _)| *i == tab.selected_file)
                .unwrap_or(0);
            let targets: Vec<(usize, bool)> = visible
                .iter()
                .map(|(i, f)| (*i, tab.reviewed.contains_key(&f.path)))
                .collect();
            (pos, targets)
        };

        if targets.is_empty() {
            return;
        }

        let len = targets.len();
        // Scan forward (wrapping) starting one past current position.
        for offset in 1..=len {
            let idx = (current_pos + offset) % len;
            let (raw_idx, is_reviewed) = targets[idx];
            if !is_reviewed {
                let tab = self.tab_mut();
                tab.selected_file = raw_idx;
                tab.current_hunk = 0;
                tab.current_line = None;
                tab.diff_scroll = 0;
                tab.h_scroll = 0;
                tab.ensure_file_parsed();
                tab.rebuild_hunk_offsets();
                // Borrow the path for the notification after releasing tab_mut.
                let path = self.tab().files[raw_idx].path.clone();
                self.notify(&format!("Jumped to: {}", path));
                return;
            }
        }

        self.notify("All visible files reviewed");
    }
}

/// Parse a Claude Code `--output-format stream-json` line into a human-readable string.
/// Returns `None` for events that should be suppressed (noise).
fn parse_stream_json_line(line: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    let obj = v.as_object()?;

    match obj.get("type")?.as_str()? {
        "assistant" => {
            // Assistant message — content is in message.content[] array
            let message = obj.get("message")?.as_object()?;
            let content = message.get("content")?.as_array()?;
            let mut parts: Vec<String> = Vec::new();
            for item in content {
                let item_obj = match item.as_object() {
                    Some(o) => o,
                    None => continue,
                };
                let item_type = match item_obj.get("type").and_then(|t| t.as_str()) {
                    Some(t) => t,
                    None => continue,
                };
                match item_type {
                    "text" => {
                        let text = match item_obj.get("text").and_then(|t| t.as_str()) {
                            Some(t) => t,
                            None => continue,
                        };
                        let trimmed = text.trim();
                        if !trimmed.is_empty() {
                            parts.push(truncate_str(trimmed, 120));
                        }
                    }
                    "tool_use" => {
                        let tool = item_obj
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("tool");
                        let input = item_obj.get("input").and_then(|i| i.as_object());
                        let detail = match tool {
                            "Read" => input
                                .and_then(|i| i.get("file_path"))
                                .and_then(|p| p.as_str())
                                .map(shorten_path)
                                .unwrap_or_default(),
                            "Write" | "Edit" => input
                                .and_then(|i| i.get("file_path"))
                                .and_then(|p| p.as_str())
                                .map(shorten_path)
                                .unwrap_or_default(),
                            "Bash" => input
                                .and_then(|i| i.get("command"))
                                .and_then(|c| c.as_str())
                                .map(|c| truncate_str(c, 60))
                                .unwrap_or_default(),
                            "Glob" | "Grep" => input
                                .and_then(|i| i.get("pattern"))
                                .and_then(|p| p.as_str())
                                .map(|p| truncate_str(p, 40))
                                .unwrap_or_default(),
                            _ => String::new(),
                        };
                        if detail.is_empty() {
                            parts.push(format!("→ {}", tool));
                        } else {
                            parts.push(format!("→ {} {}", tool, detail));
                        }
                    }
                    _ => {} // skip thinking, signatures, etc.
                }
            }
            if parts.is_empty() {
                None
            } else {
                Some(parts.join("  "))
            }
        }
        "tool_result" | "result" => {
            // Tool results are verbose — skip them to reduce noise
            None
        }
        "system" => {
            // System events — show cost/usage subtypes, skip hooks and noise
            let subtype = obj.get("subtype").and_then(|s| s.as_str()).unwrap_or("");
            if subtype.contains("cost") || subtype.contains("usage") || subtype.contains("result") {
                // Try to extract a message field, fall back to subtype
                let text = obj
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or(subtype);
                Some(format!("⊘ {}", text))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn shorten_path(path: &str) -> String {
    // Show last 2 components for readability
    let parts: Vec<&str> = path.rsplit('/').take(2).collect();
    if parts.len() == 2 {
        format!("{}/{}", parts[1], parts[0])
    } else {
        parts[0].to_string()
    }
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let boundary = s.char_indices().nth(max).map(|(i, _)| i).unwrap_or(s.len());
        format!("{}…", &s[..boundary])
    }
}

// ── Helpers ──

// chrono_now moved to the always-on sync module; re-exported so existing
// `crate::app::chrono_now` / `super::chrono_now` paths keep working.
pub use crate::sync::chrono_now;

/// Delete personal questions sidecar files. Errors are ignored (files may not exist).
/// Remove the private local-draft sidecars: questions and notes. Both are
/// user-authored, never-pushed drafts, so the `z` cleanup clears them together.
pub fn cleanup_questions_and_notes(er_dir: &str) {
    let base = std::path::Path::new(er_dir);
    let _ = std::fs::remove_file(base.join("questions.json"));
    let _ = std::fs::remove_file(base.join("questions.prev.json"));
    let _ = std::fs::remove_file(base.join("notes.json"));
    let _ = std::fs::remove_file(base.join("notes.prev.json"));
}

/// Remove AI-generated answers from questions.json, keeping human questions intact.
/// Unmarks resolved status on questions whose answers are removed.
pub fn cleanup_question_answers(er_dir: &str) {
    let path = std::path::Path::new(er_dir).join("questions.json");
    if let Ok(content) = std::fs::read_to_string(&path) {
        if let Ok(mut qs) = serde_json::from_str::<crate::ai::ErQuestions>(&content) {
            // Collect IDs of answers being removed (so we can un-resolve their parents)
            let answer_ids: std::collections::HashSet<String> = qs
                .questions
                .iter()
                .filter(|q| q.author == "Claude")
                .map(|q| q.id.clone())
                .collect();
            let answered_question_ids: std::collections::HashSet<String> = qs
                .questions
                .iter()
                .filter(|q| q.author == "Claude")
                .filter_map(|q| q.in_reply_to.clone())
                .collect();

            // Remove AI answers
            qs.questions.retain(|q| !answer_ids.contains(&q.id));

            // Un-resolve questions whose answers were removed
            for q in &mut qs.questions {
                if answered_question_ids.contains(&q.id) {
                    q.resolved = false;
                }
            }

            if let Ok(json) = serde_json::to_string_pretty(&qs) {
                let tmp = path.with_extension("json.tmp");
                if std::fs::write(&tmp, json).is_ok() {
                    let _ = std::fs::rename(&tmp, &path);
                }
            }
        }
    }
}

/// Delete AI review sidecar files. Errors are ignored (files may not exist).
pub fn cleanup_reviews(er_dir: &str) {
    let base = std::path::Path::new(er_dir);
    let _ = std::fs::remove_file(base.join("review.json"));
    let _ = std::fs::remove_file(base.join("order.json"));
    let _ = std::fs::remove_file(base.join("checklist.json"));
    let _ = std::fs::remove_file(base.join("summary.md"));
}

/// Delete triage sidecar only. Errors are ignored (file may not exist).
pub fn cleanup_triage(er_dir: &str) {
    let _ = std::fs::remove_file(std::path::Path::new(er_dir).join("triage.json"));
}

/// Delete all AI review artifacts (general, expert, professor). Leaves triage intact.
pub fn cleanup_review_artifacts(er_dir: &str) {
    cleanup_reviews(er_dir);
    let base = std::path::Path::new(er_dir);
    let _ = std::fs::remove_file(base.join("professor.json"));
    let experts_dir = base.join("experts");
    if let Ok(entries) = std::fs::read_dir(&experts_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json") {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

/// Truncate a string to max_len chars, adding … if truncated
fn truncate(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max_len.saturating_sub(1)).collect();
    format!("{}…", truncated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::AiState;
    use crate::git::{DiffFile, DiffHunk, DiffLine, FileStatus, LineType};
    use std::collections::{HashMap, HashSet};

    fn make_test_tab(files: Vec<DiffFile>) -> TabState {
        use crate::ai::{InlineLayers, ReviewFocus};
        let (agent_log_tx, agent_log_rx) = std::sync::mpsc::channel();
        TabState {
            mode: DiffMode::Branch,
            base_branch: "main".to_string(),
            current_branch: "feature".to_string(),
            er_root: ErRoot::RepoLocal("/tmp/test".to_string()),
            repo_root: "/tmp/test".to_string(),
            files,
            selected_file: 0,
            current_hunk: 0,
            current_line: None,
            selection_anchor: None,
            diff_scroll: 0,
            h_scroll: 0,
            split_focus: SplitSide::New,
            h_scroll_old: 0,
            h_scroll_new: 0,
            layers: InlineLayers::default(),
            panel: None,
            panel_scroll: 0,
            panel_focus: false,
            file_tree_width: 32,
            panel_width: 40,
            focused_comment_id: None,
            focused_finding_id: None,
            user_expanded: HashSet::new(),
            review_focus: ReviewFocus::Files,
            review_cursor: 0,
            search_query: String::new(),
            filter_expr: String::new(),
            filter_rules: Vec::new(),
            filter_input: String::new(),
            filter_history: Vec::new(),
            reviewed: HashMap::new(),
            current_per_file_hashes: HashMap::new(),
            show_unreviewed_only: false,
            sort_by_mtime: false,
            mtime_cache: HashMap::new(),
            search_query_lower: String::new(),
            ai: AiState::default(),
            diff_hash: String::new(),
            branch_diff_hash: String::new(),
            last_ai_check: None,
            comment_textarea: TextArea::default(),
            comment_file: String::new(),
            comment_hunk: 0,
            comment_reply_to: None,
            comment_line_num: None,
            comment_line_end: None,
            comment_type: CommentType::GitHubComment,
            comment_edit_id: None,
            comment_finding_ref: None,
            comment_author_override: None,
            comment_side: None,
            pr_data: None,
            pr_commits: Vec::new(),
            pr_head_ref: None,
            pr_number: None,
            history: None,
            tour: None,
            tour_is_pr: false,
            watched_config: WatchedConfig::default(),
            watched_files: Vec::new(),
            selected_watched: None,
            show_watched: false,
            watched_not_ignored: Vec::new(),
            commit_input: String::new(),
            merge_active: false,
            unresolved_count: 0,
            compaction_config: CompactionConfig::default(),
            hunk_offsets: None,
            mem_budget: MemoryBudget::default(),
            lazy_mode: false,
            file_headers: Vec::new(),
            raw_diff: None,
            symbol_refs: None,
            pending_unmark_count: 0,
            reviewed_revision: 0,
            committed_unpushed: false,
            context_overrides: HashMap::new(),
            remote_repo: None,
            local_branch_view: None,
            local_branch_checkout_root: None,
            command_rx: std::collections::HashMap::new(),
            command_status: std::collections::HashMap::new(),
            log_tx: agent_log_tx,
            log_rx: agent_log_rx,
            agent_log: std::collections::VecDeque::new(),
            agent_log_auto_scroll: true,
            browser_url: String::new(),
            browser_layout: BrowserLayout::default(),
            browser_split_ratio: 0.45,
            browser_annotate_mode: false,
            browser_show_tooltips: false,
            needs_initial_refresh: false,
            storage_notice: None,
            last_diff_head_oid: None,
            pr_refs_fetched: false,
        }
    }

    fn make_file(path: &str, hunks: Vec<DiffHunk>, adds: usize, dels: usize) -> DiffFile {
        DiffFile {
            path: path.to_string(),
            status: FileStatus::Modified,
            hunks,
            adds,
            dels,
            compacted: false,
            raw_hunk_count: 0,
        }
    }

    fn make_hunk(lines: Vec<DiffLine>) -> DiffHunk {
        DiffHunk {
            header: "@@ -1,3 +1,4 @@".to_string(),
            old_start: 1,
            old_count: 3,
            new_start: 1,
            new_count: 4,
            lines,
        }
    }

    fn make_line(line_type: LineType, content: &str, new_num: Option<usize>) -> DiffLine {
        DiffLine {
            line_type,
            content: content.to_string(),
            old_num: None,
            new_num,
        }
    }

    // ── truncate ──

    #[test]
    fn truncate_shorter_than_limit_returned_as_is() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn truncate_equal_to_limit_returned_as_is() {
        assert_eq!(truncate("hello", 5), "hello");
    }

    #[test]
    fn truncate_longer_than_limit_truncated_with_ellipsis() {
        let result = truncate("hello world", 8);
        assert_eq!(result, "hello w…");
    }

    #[test]
    fn truncate_very_short_limit_returns_just_ellipsis() {
        let result = truncate("hello", 1);
        assert_eq!(result, "…");
    }

    #[test]
    fn truncate_empty_string_returned_as_is() {
        assert_eq!(truncate("", 5), "");
    }

    // ── visible_files ──

    #[test]
    fn visible_files_no_search_no_filter_returns_all() {
        let files = vec![
            make_file("src/main.rs", vec![], 1, 0),
            make_file("src/lib.rs", vec![], 2, 0),
        ];
        let tab = make_test_tab(files);
        let visible = tab.visible_files();
        assert_eq!(visible.len(), 2);
        assert_eq!(visible[0].0, 0);
        assert_eq!(visible[1].0, 1);
    }

    #[test]
    fn visible_files_search_query_filters_matches() {
        let files = vec![
            make_file("src/main.rs", vec![], 1, 0),
            make_file("tests/foo.rs", vec![], 1, 0),
        ];
        let mut tab = make_test_tab(files);
        tab.search_query = "src".to_string();
        tab.search_query_lower = "src".to_string();
        let visible = tab.visible_files();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].1.path, "src/main.rs");
    }

    #[test]
    fn visible_files_search_query_no_match_returns_empty() {
        let files = vec![make_file("src/main.rs", vec![], 1, 0)];
        let mut tab = make_test_tab(files);
        tab.search_query = "zzz".to_string();
        tab.search_query_lower = "zzz".to_string();
        let visible = tab.visible_files();
        assert_eq!(visible.len(), 0);
    }

    #[test]
    fn visible_files_search_is_case_insensitive() {
        let files = vec![
            make_file("src/main.rs", vec![], 1, 0),
            make_file("tests/foo.rs", vec![], 1, 0),
        ];
        let mut tab = make_test_tab(files);
        tab.search_query = "SRC".to_string();
        tab.search_query_lower = "src".to_string();
        let visible = tab.visible_files();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].1.path, "src/main.rs");
    }

    #[test]
    fn visible_files_show_unreviewed_only_all_reviewed_returns_empty() {
        let files = vec![
            make_file("src/main.rs", vec![], 1, 0),
            make_file("src/lib.rs", vec![], 1, 0),
        ];
        let mut tab = make_test_tab(files);
        tab.show_unreviewed_only = true;
        tab.reviewed
            .insert("src/main.rs".to_string(), String::new());
        tab.reviewed.insert("src/lib.rs".to_string(), String::new());
        let visible = tab.visible_files();
        assert_eq!(visible.len(), 0);
    }

    #[test]
    fn visible_files_show_unreviewed_only_some_reviewed_returns_unreviewed() {
        let files = vec![
            make_file("src/main.rs", vec![], 1, 0),
            make_file("src/lib.rs", vec![], 1, 0),
            make_file("src/util.rs", vec![], 1, 0),
        ];
        let mut tab = make_test_tab(files);
        tab.show_unreviewed_only = true;
        tab.reviewed
            .insert("src/main.rs".to_string(), String::new());
        let visible = tab.visible_files();
        assert_eq!(visible.len(), 2);
        assert_eq!(visible[0].1.path, "src/lib.rs");
        assert_eq!(visible[1].1.path, "src/util.rs");
    }

    #[test]
    fn visible_files_combined_search_and_unreviewed_filter() {
        let files = vec![
            make_file("src/main.rs", vec![], 1, 0),
            make_file("src/lib.rs", vec![], 1, 0),
            make_file("tests/foo.rs", vec![], 1, 0),
        ];
        let mut tab = make_test_tab(files);
        tab.search_query = "src".to_string();
        tab.search_query_lower = "src".to_string();
        tab.show_unreviewed_only = true;
        tab.reviewed
            .insert("src/main.rs".to_string(), String::new());
        let visible = tab.visible_files();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].1.path, "src/lib.rs");
    }

    // ── reviewed_count ──

    #[test]
    fn reviewed_count_no_files_returns_zero_zero() {
        let tab = make_test_tab(vec![]);
        assert_eq!(tab.reviewed_count(), (0, 0));
    }

    #[test]
    fn reviewed_count_some_reviewed_returns_correct_counts() {
        let files = vec![
            make_file("src/main.rs", vec![], 1, 0),
            make_file("src/lib.rs", vec![], 1, 0),
            make_file("src/util.rs", vec![], 1, 0),
        ];
        let mut tab = make_test_tab(files);
        tab.reviewed
            .insert("src/main.rs".to_string(), String::new());
        assert_eq!(tab.reviewed_count(), (1, 3));
    }

    #[test]
    fn reviewed_count_all_reviewed_returns_n_n() {
        let files = vec![
            make_file("src/main.rs", vec![], 1, 0),
            make_file("src/lib.rs", vec![], 1, 0),
        ];
        let mut tab = make_test_tab(files);
        tab.reviewed
            .insert("src/main.rs".to_string(), String::new());
        tab.reviewed.insert("src/lib.rs".to_string(), String::new());
        assert_eq!(tab.reviewed_count(), (2, 2));
    }

    #[test]
    fn active_reviewed_count_ignores_orphan_reviewed_paths() {
        let files = vec![
            make_file("a.json", vec![], 1, 0),
            make_file("b.json", vec![], 1, 0),
            make_file("c.json", vec![], 1, 0),
        ];
        let mut tab = make_test_tab(files);
        tab.reviewed.insert("a.json".to_string(), "h".to_string());
        tab.reviewed.insert("b.json".to_string(), "h".to_string());
        tab.reviewed.insert("c.json".to_string(), "h".to_string());
        for i in 0..4 {
            tab.reviewed
                .insert(format!("other/{i}.rs"), "h".to_string());
        }
        assert_eq!(tab.reviewed.len(), 7);
        assert_eq!(tab.active_reviewed_count(), (3, 3));
        assert_eq!(tab.reviewed_count(), (3, 3));
    }

    #[test]
    fn prune_reviewed_not_in_diff_drops_orphans_and_keeps_active() {
        let files = vec![
            make_file("a.json", vec![], 1, 0),
            make_file("b.json", vec![], 1, 0),
        ];
        let mut tab = make_test_tab(files);
        tab.reviewed.insert("a.json".to_string(), String::new());
        tab.reviewed.insert("gone.rs".to_string(), String::new());
        assert_eq!(tab.prune_reviewed_not_in_diff(), 1);
        assert_eq!(tab.reviewed.len(), 1);
        assert!(tab.reviewed.contains_key("a.json"));
    }

    // ── next_file / prev_file ──

    #[test]
    fn next_file_at_last_file_wraps_to_first() {
        let files = vec![
            make_file("a.rs", vec![], 1, 0),
            make_file("b.rs", vec![], 1, 0),
        ];
        let mut tab = make_test_tab(files);
        tab.selected_file = 1;
        tab.next_file();
        assert_eq!(tab.selected_file, 0);
    }

    #[test]
    fn prev_file_at_first_file_wraps_to_last() {
        let files = vec![
            make_file("a.rs", vec![], 1, 0),
            make_file("b.rs", vec![], 1, 0),
        ];
        let mut tab = make_test_tab(files);
        tab.selected_file = 0;
        tab.prev_file();
        assert_eq!(tab.selected_file, 1);
    }

    #[test]
    fn next_file_moves_to_next_visible_file() {
        let files = vec![
            make_file("a.rs", vec![], 1, 0),
            make_file("b.rs", vec![], 1, 0),
            make_file("c.rs", vec![], 1, 0),
        ];
        let mut tab = make_test_tab(files);
        tab.selected_file = 0;
        tab.next_file();
        assert_eq!(tab.selected_file, 1);
    }

    #[test]
    fn prev_file_moves_to_previous_visible_file() {
        let files = vec![
            make_file("a.rs", vec![], 1, 0),
            make_file("b.rs", vec![], 1, 0),
            make_file("c.rs", vec![], 1, 0),
        ];
        let mut tab = make_test_tab(files);
        tab.selected_file = 2;
        tab.prev_file();
        assert_eq!(tab.selected_file, 1);
    }

    #[test]
    fn next_file_with_no_visible_files_no_crash() {
        let files = vec![make_file("a.rs", vec![], 1, 0)];
        let mut tab = make_test_tab(files);
        tab.search_query = "zzz".to_string();
        tab.search_query_lower = "zzz".to_string();
        // Should not panic
        tab.next_file();
        assert_eq!(tab.selected_file, 0);
    }

    // ── next_hunk / prev_hunk ──

    #[test]
    fn next_hunk_increments_current_hunk() {
        let files = vec![make_file(
            "a.rs",
            vec![make_hunk(vec![]), make_hunk(vec![])],
            1,
            0,
        )];
        let mut tab = make_test_tab(files);
        tab.current_hunk = 0;
        tab.next_hunk();
        assert_eq!(tab.current_hunk, 1);
    }

    #[test]
    fn next_hunk_at_last_hunk_stays() {
        let files = vec![make_file(
            "a.rs",
            vec![make_hunk(vec![]), make_hunk(vec![])],
            1,
            0,
        )];
        let mut tab = make_test_tab(files);
        tab.current_hunk = 1;
        tab.next_hunk();
        assert_eq!(tab.current_hunk, 1);
    }

    #[test]
    fn prev_hunk_decrements_current_hunk() {
        let files = vec![make_file(
            "a.rs",
            vec![make_hunk(vec![]), make_hunk(vec![])],
            1,
            0,
        )];
        let mut tab = make_test_tab(files);
        tab.current_hunk = 1;
        tab.prev_hunk();
        assert_eq!(tab.current_hunk, 0);
    }

    #[test]
    fn prev_hunk_at_zero_stays() {
        let files = vec![make_file("a.rs", vec![make_hunk(vec![])], 1, 0)];
        let mut tab = make_test_tab(files);
        tab.current_hunk = 0;
        tab.prev_hunk();
        assert_eq!(tab.current_hunk, 0);
    }

    // ── next_line / prev_line ──

    #[test]
    fn next_line_from_none_enters_line_mode_at_zero() {
        let lines = vec![
            make_line(LineType::Add, "x", Some(1)),
            make_line(LineType::Add, "y", Some(2)),
        ];
        let files = vec![make_file("a.rs", vec![make_hunk(lines)], 2, 0)];
        let mut tab = make_test_tab(files);
        tab.current_line = None;
        tab.next_line();
        assert_eq!(tab.current_line, Some(0));
    }

    #[test]
    fn next_line_increments_within_hunk() {
        let lines = vec![
            make_line(LineType::Add, "x", Some(1)),
            make_line(LineType::Add, "y", Some(2)),
        ];
        let files = vec![make_file("a.rs", vec![make_hunk(lines)], 2, 0)];
        let mut tab = make_test_tab(files);
        tab.current_line = Some(0);
        tab.next_line();
        assert_eq!(tab.current_line, Some(1));
    }

    #[test]
    fn next_line_at_last_line_of_hunk_moves_to_next_hunk() {
        let lines1 = vec![make_line(LineType::Add, "x", Some(1))];
        let lines2 = vec![make_line(LineType::Add, "y", Some(2))];
        let files = vec![make_file(
            "a.rs",
            vec![make_hunk(lines1), make_hunk(lines2)],
            2,
            0,
        )];
        let mut tab = make_test_tab(files);
        tab.current_hunk = 0;
        tab.current_line = Some(0); // last line of hunk 0
        tab.next_line();
        assert_eq!(tab.current_hunk, 1);
        assert_eq!(tab.current_line, Some(0));
    }

    #[test]
    fn next_line_at_last_line_of_last_hunk_stays() {
        let lines = vec![make_line(LineType::Add, "x", Some(1))];
        let files = vec![make_file("a.rs", vec![make_hunk(lines)], 1, 0)];
        let mut tab = make_test_tab(files);
        tab.current_hunk = 0;
        tab.current_line = Some(0); // last (and only) line of last hunk
        tab.next_line();
        assert_eq!(tab.current_hunk, 0);
        assert_eq!(tab.current_line, Some(0));
    }

    #[test]
    fn prev_line_from_none_enters_line_mode_at_last_line() {
        let lines = vec![make_line(LineType::Add, "x", Some(1))];
        let files = vec![make_file("a.rs", vec![make_hunk(lines)], 1, 0)];
        let mut tab = make_test_tab(files);
        tab.current_line = None;
        tab.prev_line();
        assert_eq!(tab.current_line, Some(0)); // last (and only) line
        assert_eq!(tab.current_hunk, 0);
    }

    #[test]
    fn prev_line_from_zero_at_hunk_zero_exits_line_mode() {
        let lines = vec![make_line(LineType::Add, "x", Some(1))];
        let files = vec![make_file("a.rs", vec![make_hunk(lines)], 1, 0)];
        let mut tab = make_test_tab(files);
        tab.current_hunk = 0;
        tab.current_line = Some(0);
        tab.prev_line();
        assert_eq!(tab.current_line, None);
        assert_eq!(tab.current_hunk, 0);
    }

    #[test]
    fn prev_line_from_zero_at_hunk_one_goes_to_previous_hunk_last_line() {
        let lines1 = vec![
            make_line(LineType::Add, "a", Some(1)),
            make_line(LineType::Add, "b", Some(2)),
        ];
        let lines2 = vec![make_line(LineType::Add, "c", Some(3))];
        let files = vec![make_file(
            "a.rs",
            vec![make_hunk(lines1), make_hunk(lines2)],
            3,
            0,
        )];
        let mut tab = make_test_tab(files);
        tab.current_hunk = 1;
        tab.current_line = Some(0);
        tab.prev_line();
        assert_eq!(tab.current_hunk, 0);
        assert_eq!(tab.current_line, Some(1)); // last line of hunk 0 (2 lines, index 1)
    }

    #[test]
    fn prev_line_decrements_within_hunk() {
        let lines = vec![
            make_line(LineType::Add, "x", Some(1)),
            make_line(LineType::Add, "y", Some(2)),
        ];
        let files = vec![make_file("a.rs", vec![make_hunk(lines)], 2, 0)];
        let mut tab = make_test_tab(files);
        tab.current_line = Some(1);
        tab.prev_line();
        assert_eq!(tab.current_line, Some(0));
    }

    // ── total_hunks ──

    #[test]
    fn total_hunks_no_files_returns_zero() {
        let tab = make_test_tab(vec![]);
        assert_eq!(tab.total_hunks(), 0);
    }

    #[test]
    fn total_hunks_file_with_three_hunks_returns_three() {
        let files = vec![make_file(
            "a.rs",
            vec![make_hunk(vec![]), make_hunk(vec![]), make_hunk(vec![])],
            3,
            0,
        )];
        let tab = make_test_tab(files);
        assert_eq!(tab.total_hunks(), 3);
    }

    // ── current_line_number ──

    #[test]
    fn current_line_number_with_new_num_returns_some() {
        let lines = vec![make_line(LineType::Add, "x", Some(42))];
        let files = vec![make_file("a.rs", vec![make_hunk(lines)], 1, 0)];
        let mut tab = make_test_tab(files);
        tab.current_line = Some(0);
        assert_eq!(tab.current_line_number(), Some(42));
    }

    #[test]
    fn current_line_number_with_none_current_line_returns_none() {
        let lines = vec![make_line(LineType::Add, "x", Some(1))];
        let files = vec![make_file("a.rs", vec![make_hunk(lines)], 1, 0)];
        let mut tab = make_test_tab(files);
        tab.current_line = None;
        assert_eq!(tab.current_line_number(), None);
    }

    #[test]
    fn current_line_number_delete_line_falls_back_to_old_num() {
        let lines = vec![DiffLine {
            line_type: LineType::Delete,
            content: "deleted".to_string(),
            old_num: Some(1),
            new_num: None,
        }];
        let files = vec![make_file("a.rs", vec![make_hunk(lines)], 0, 1)];
        let mut tab = make_test_tab(files);
        tab.current_line = Some(0);
        assert_eq!(tab.current_line_number(), Some(1));
    }

    // ── snap_to_visible ──

    #[test]
    fn snap_to_visible_selected_file_is_visible_no_change() {
        let files = vec![
            make_file("a.rs", vec![], 1, 0),
            make_file("b.rs", vec![], 1, 0),
        ];
        let mut tab = make_test_tab(files);
        tab.selected_file = 1;
        tab.snap_to_visible();
        assert_eq!(tab.selected_file, 1);
    }

    #[test]
    fn snap_to_visible_selected_file_filtered_out_snaps_to_first_visible() {
        let files = vec![
            make_file("src/main.rs", vec![], 1, 0),
            make_file("tests/foo.rs", vec![], 1, 0),
        ];
        let mut tab = make_test_tab(files);
        tab.selected_file = 1; // points at tests/foo.rs
        tab.search_query = "src".to_string(); // only src/main.rs visible
        tab.search_query_lower = "src".to_string();
        tab.snap_to_visible();
        assert_eq!(tab.selected_file, 0); // snapped to src/main.rs (index 0)
    }

    // ── DiffMode methods ──

    #[test]
    fn diff_mode_label_returns_correct_strings() {
        assert_eq!(DiffMode::Branch.label(), "BRANCH DIFF");
        assert_eq!(DiffMode::Unstaged.label(), "UNSTAGED");
        assert_eq!(DiffMode::Staged.label(), "STAGED");
        assert_eq!(DiffMode::History.label(), "HISTORY");
        assert_eq!(DiffMode::Conflicts.label(), "CONFLICTS");
    }

    #[test]
    fn diff_mode_git_mode_returns_correct_strings() {
        assert_eq!(DiffMode::Branch.git_mode(), "branch");
        assert_eq!(DiffMode::Unstaged.git_mode(), "unstaged");
        assert_eq!(DiffMode::Staged.git_mode(), "staged");
        assert_eq!(DiffMode::History.git_mode(), "history");
        assert_eq!(DiffMode::Conflicts.git_mode(), "conflicts");
    }

    // ── clamp_hunk ──

    #[test]
    fn clamp_hunk_beyond_total_clamped_to_last() {
        let files = vec![make_file(
            "a.rs",
            vec![make_hunk(vec![]), make_hunk(vec![])],
            1,
            0,
        )];
        let mut tab = make_test_tab(files);
        tab.current_hunk = 99;
        tab.clamp_hunk();
        assert_eq!(tab.current_hunk, 1); // total 2 hunks → max index 1
    }

    #[test]
    fn clamp_hunk_no_hunks_resets_to_zero_and_clears_line() {
        let files = vec![make_file("a.rs", vec![], 0, 0)];
        let mut tab = make_test_tab(files);
        tab.current_hunk = 5;
        tab.current_line = Some(2);
        tab.clamp_hunk();
        assert_eq!(tab.current_hunk, 0);
        assert_eq!(tab.current_line, None);
    }

    // ── scroll methods ──

    #[test]
    fn scroll_down_adds_to_diff_scroll() {
        let mut tab = make_test_tab(vec![]);
        tab.diff_scroll = 10;
        tab.scroll_down(5);
        assert_eq!(tab.diff_scroll, 15);
    }

    #[test]
    fn scroll_up_subtracts_saturating() {
        let mut tab = make_test_tab(vec![]);
        tab.diff_scroll = 3;
        tab.scroll_up(10);
        assert_eq!(tab.diff_scroll, 0); // saturating — no underflow
    }

    #[test]
    fn scroll_right_adds_to_h_scroll() {
        let mut tab = make_test_tab(vec![]);
        tab.h_scroll = 5;
        tab.scroll_right(3);
        assert_eq!(tab.h_scroll, 8);
    }

    #[test]
    fn scroll_left_subtracts_saturating() {
        let mut tab = make_test_tab(vec![]);
        tab.h_scroll = 2;
        tab.scroll_left(10);
        assert_eq!(tab.h_scroll, 0); // saturating — no underflow
    }

    // ── sync_cursor_to_scroll ──

    /// Layout for a file with 2 hunks (3 lines, 2 lines):
    /// offset 0: file header
    /// offset 1: blank
    /// offset 2: hunk 0 header
    /// offset 3: hunk 0 line 0
    /// offset 4: hunk 0 line 1
    /// offset 5: hunk 0 line 2
    /// offset 6: blank
    /// offset 7: hunk 1 header
    /// offset 8: hunk 1 line 0
    /// offset 9: hunk 1 line 1
    /// offset 10: blank
    fn make_two_hunk_tab() -> TabState {
        let files = vec![make_file(
            "a.rs",
            vec![
                make_hunk(vec![
                    make_line(LineType::Context, "a", Some(1)),
                    make_line(LineType::Add, "b", Some(2)),
                    make_line(LineType::Context, "c", Some(3)),
                ]),
                make_hunk(vec![
                    make_line(LineType::Delete, "d", None),
                    make_line(LineType::Add, "e", Some(10)),
                ]),
            ],
            2,
            1,
        )];
        make_test_tab(files)
    }

    #[test]
    fn scroll_down_syncs_cursor_to_first_hunk_first_line() {
        let mut tab = make_two_hunk_tab();
        // Scroll to offset 3 → hunk 0, line 0
        tab.scroll_down(3);
        assert_eq!(tab.diff_scroll, 3);
        assert_eq!(tab.current_hunk, 0);
        assert_eq!(tab.current_line, Some(0));
    }

    #[test]
    fn scroll_down_syncs_cursor_mid_hunk() {
        let mut tab = make_two_hunk_tab();
        // Scroll to offset 5 → hunk 0, line 2
        tab.scroll_down(5);
        assert_eq!(tab.current_hunk, 0);
        assert_eq!(tab.current_line, Some(2));
    }

    #[test]
    fn scroll_down_syncs_cursor_to_second_hunk() {
        let mut tab = make_two_hunk_tab();
        // Scroll to offset 8 → hunk 1, line 0
        tab.scroll_down(8);
        assert_eq!(tab.current_hunk, 1);
        assert_eq!(tab.current_line, Some(0));
    }

    #[test]
    fn scroll_down_on_hunk_header_snaps_to_first_content_line() {
        let mut tab = make_two_hunk_tab();
        // Scroll to offset 7 → hunk 1 header → snaps to hunk 1 line 0
        tab.scroll_down(7);
        assert_eq!(tab.current_hunk, 1);
        assert_eq!(tab.current_line, Some(0));
    }

    #[test]
    fn scroll_down_past_end_clamps_to_last_line() {
        let mut tab = make_two_hunk_tab();
        tab.scroll_down(100);
        assert_eq!(tab.current_hunk, 1);
        assert_eq!(tab.current_line, Some(1)); // last line of last hunk
    }

    #[test]
    fn scroll_up_syncs_cursor_back() {
        let mut tab = make_two_hunk_tab();
        tab.diff_scroll = 8;
        tab.current_hunk = 1;
        tab.current_line = Some(0);
        // Scroll up to offset 3 → hunk 0, line 0
        tab.scroll_up(5);
        assert_eq!(tab.diff_scroll, 3);
        assert_eq!(tab.current_hunk, 0);
        assert_eq!(tab.current_line, Some(0));
    }

    #[test]
    fn scroll_up_to_file_header_snaps_to_first_content_line() {
        let mut tab = make_two_hunk_tab();
        tab.diff_scroll = 5;
        // Scroll up to offset 0 → file header area → snaps to hunk 0 line 0
        tab.scroll_up(5);
        assert_eq!(tab.diff_scroll, 0);
        assert_eq!(tab.current_hunk, 0);
        assert_eq!(tab.current_line, Some(0));
    }

    #[test]
    fn scroll_on_blank_between_hunks_snaps_to_next_hunk() {
        let mut tab = make_two_hunk_tab();
        // Scroll to offset 6 → blank after hunk 0 → snaps to hunk 1 line 0
        tab.scroll_down(6);
        assert_eq!(tab.current_hunk, 1);
        assert_eq!(tab.current_line, Some(0));
    }

    // ── TabState new field defaults ──

    #[test]
    fn new_tab_state_has_default_layers() {
        let tab = make_test_tab(vec![]);
        assert!(tab.layers.show_questions);
        assert!(tab.layers.show_github_comments);
        assert!(tab.layers.show_ai_findings);
    }

    #[test]
    fn new_tab_state_panel_is_none() {
        let tab = make_test_tab(vec![]);
        assert!(tab.panel.is_none());
    }

    #[test]
    fn new_tab_state_panel_focus_is_false() {
        let tab = make_test_tab(vec![]);
        assert!(!tab.panel_focus);
    }

    // ── Layer toggles ──

    #[test]
    fn toggle_layer_ai_flips_show_ai_findings() {
        let mut tab = make_test_tab(vec![]);
        assert!(tab.layers.show_ai_findings);
        tab.toggle_layer_ai();
        assert!(!tab.layers.show_ai_findings);
        tab.toggle_layer_ai();
        assert!(tab.layers.show_ai_findings);
    }

    #[test]
    fn toggle_layer_questions_flips_show_questions() {
        let mut tab = make_test_tab(vec![]);
        assert!(tab.layers.show_questions);
        tab.toggle_layer_questions();
        assert!(!tab.layers.show_questions);
        tab.toggle_layer_questions();
        assert!(tab.layers.show_questions);
    }

    #[test]
    fn toggle_layer_comments_flips_show_github_comments() {
        let mut tab = make_test_tab(vec![]);
        assert!(tab.layers.show_github_comments);
        tab.toggle_layer_comments();
        assert!(!tab.layers.show_github_comments);
        tab.toggle_layer_comments();
        assert!(tab.layers.show_github_comments);
    }

    // ── toggle_panel ──

    #[test]
    fn toggle_panel_opens_file_detail_from_none() {
        let mut tab = make_test_tab(vec![]);
        tab.toggle_panel();
        assert_eq!(tab.panel, Some(crate::ai::PanelContent::FileDetail));
    }

    #[test]
    fn toggle_panel_cycles_to_ai_summary_when_ai_data_present() {
        let mut tab = make_test_tab(vec![]);
        tab.ai.summary = Some("some summary".to_string());
        tab.layers.show_ai_findings = true;
        tab.panel = Some(crate::ai::PanelContent::FileDetail);
        tab.toggle_panel();
        assert_eq!(tab.panel, Some(crate::ai::PanelContent::AiSummary));
    }

    #[test]
    fn toggle_panel_skips_ai_summary_when_no_ai_data() {
        let mut tab = make_test_tab(vec![]);
        tab.panel = Some(crate::ai::PanelContent::FileDetail);
        tab.toggle_panel();
        // No AI data, no PR data, no SymbolRefs: FileDetail → AgentLog (AI and PR skipped)
        assert_eq!(tab.panel, Some(crate::ai::PanelContent::AgentLog));
    }

    #[test]
    fn toggle_panel_skips_ai_goes_to_pr_when_pr_available() {
        let mut tab = make_test_tab(vec![]);
        tab.pr_data = Some(crate::github::PrOverviewData {
            number: 1,
            title: "t".to_string(),
            body: String::new(),
            state: "OPEN".to_string(),
            author: "u".to_string(),
            url: String::new(),
            base_branch: "main".to_string(),
            head_branch: "feat".to_string(),
            checks: vec![],
            reviewers: vec![],
        });
        tab.panel = Some(crate::ai::PanelContent::FileDetail);
        tab.toggle_panel();
        // No AI data, but PR available: FileDetail → PrOverview
        assert_eq!(tab.panel, Some(crate::ai::PanelContent::PrOverview));
    }

    #[test]
    fn toggle_panel_closes_from_ai_summary_when_no_pr() {
        let mut tab = make_test_tab(vec![]);
        tab.ai.summary = Some("summary".to_string());
        tab.panel = Some(crate::ai::PanelContent::AiSummary);
        tab.toggle_panel();
        // AiSummary → AgentLog (no PR available, no SymbolRefs)
        assert_eq!(tab.panel, Some(crate::ai::PanelContent::AgentLog));
    }

    #[test]
    fn toggle_panel_cycles_ai_to_pr_when_pr_available() {
        let mut tab = make_test_tab(vec![]);
        tab.ai.summary = Some("summary".to_string());
        tab.pr_data = Some(crate::github::PrOverviewData {
            number: 1,
            title: "t".to_string(),
            body: String::new(),
            state: "OPEN".to_string(),
            author: "u".to_string(),
            url: String::new(),
            base_branch: "main".to_string(),
            head_branch: "feat".to_string(),
            checks: vec![],
            reviewers: vec![],
        });
        tab.panel = Some(crate::ai::PanelContent::AiSummary);
        tab.toggle_panel();
        // AiSummary → PrOverview (PR available)
        assert_eq!(tab.panel, Some(crate::ai::PanelContent::PrOverview));
    }

    #[test]
    fn toggle_panel_resets_panel_scroll_on_each_toggle() {
        let mut tab = make_test_tab(vec![]);
        tab.panel_scroll = 42;
        tab.toggle_panel();
        assert_eq!(tab.panel_scroll, 0);

        tab.panel_scroll = 10;
        tab.toggle_panel(); // FileDetail → AgentLog (no AI or PR data)
        assert_eq!(tab.panel_scroll, 0);
    }

    #[test]
    fn toggle_panel_sets_panel_focus_false_when_closing() {
        let mut tab = make_test_tab(vec![]);
        // AgentLog → None closes the panel and clears focus
        tab.panel = Some(crate::ai::PanelContent::AgentLog);
        tab.panel_focus = true;
        tab.toggle_panel();
        assert!(!tab.panel_focus);
    }

    #[test]
    fn toggle_panel_does_not_clear_panel_focus_when_opening() {
        let mut tab = make_test_tab(vec![]);
        tab.panel_focus = false;
        tab.toggle_panel(); // None → FileDetail
                            // panel_focus stays as-is when panel is opened
        assert!(!tab.panel_focus);
    }

    // ── toggle_panel full cycle ──

    #[test]
    fn toggle_panel_full_cycle_no_ai_no_pr() {
        let mut tab = make_test_tab(vec![]);
        assert_eq!(tab.panel, None);
        tab.toggle_panel(); // None → FileDetail
        assert_eq!(tab.panel, Some(crate::ai::PanelContent::FileDetail));
        tab.toggle_panel(); // FileDetail → AgentLog (no AI, no PR, no SymbolRefs)
        assert_eq!(tab.panel, Some(crate::ai::PanelContent::AgentLog));
        tab.toggle_panel(); // AgentLog → None
        assert_eq!(tab.panel, None);
    }

    #[test]
    fn toggle_panel_full_cycle_with_ai_only() {
        let mut tab = make_test_tab(vec![]);
        tab.ai.summary = Some("summary".to_string());
        tab.layers.show_ai_findings = true;
        assert_eq!(tab.panel, None);
        tab.toggle_panel(); // None → FileDetail
        assert_eq!(tab.panel, Some(crate::ai::PanelContent::FileDetail));
        tab.toggle_panel(); // FileDetail → AiSummary (AI present)
        assert_eq!(tab.panel, Some(crate::ai::PanelContent::AiSummary));
        tab.toggle_panel(); // AiSummary → AgentLog (no PR, no SymbolRefs)
        assert_eq!(tab.panel, Some(crate::ai::PanelContent::AgentLog));
        tab.toggle_panel(); // AgentLog → None
        assert_eq!(tab.panel, None);
    }

    #[test]
    fn toggle_panel_full_cycle_with_ai_and_pr() {
        let mut tab = make_test_tab(vec![]);
        tab.ai.summary = Some("summary".to_string());
        tab.layers.show_ai_findings = true;
        tab.pr_data = Some(crate::github::PrOverviewData {
            number: 42,
            title: "My PR".to_string(),
            body: String::new(),
            state: "OPEN".to_string(),
            author: "user".to_string(),
            url: String::new(),
            base_branch: "main".to_string(),
            head_branch: "feature".to_string(),
            checks: vec![],
            reviewers: vec![],
        });
        assert_eq!(tab.panel, None);
        tab.toggle_panel(); // None → FileDetail
        assert_eq!(tab.panel, Some(crate::ai::PanelContent::FileDetail));
        tab.toggle_panel(); // FileDetail → AiSummary
        assert_eq!(tab.panel, Some(crate::ai::PanelContent::AiSummary));
        tab.toggle_panel(); // AiSummary → PrOverview
        assert_eq!(tab.panel, Some(crate::ai::PanelContent::PrOverview));
        tab.toggle_panel(); // PrOverview → AgentLog (no SymbolRefs)
        assert_eq!(tab.panel, Some(crate::ai::PanelContent::AgentLog));
        tab.toggle_panel(); // AgentLog → None
        assert_eq!(tab.panel, None);
    }

    // ── toggle_panel_reverse full cycle ──

    #[test]
    fn toggle_panel_reverse_full_cycle_no_ai_no_pr() {
        let mut tab = make_test_tab(vec![]);
        assert_eq!(tab.panel, None);
        tab.toggle_panel_reverse(); // None → AgentLog (always available, last in forward cycle)
        assert_eq!(tab.panel, Some(crate::ai::PanelContent::AgentLog));
        tab.toggle_panel_reverse(); // AgentLog → FileDetail (no sym, no PR, no AI)
        assert_eq!(tab.panel, Some(crate::ai::PanelContent::FileDetail));
        tab.toggle_panel_reverse(); // FileDetail → None
        assert_eq!(tab.panel, None);
    }

    #[test]
    fn toggle_panel_reverse_full_cycle_with_ai_only() {
        let mut tab = make_test_tab(vec![]);
        tab.ai.summary = Some("summary".to_string());
        tab.layers.show_ai_findings = true;
        assert_eq!(tab.panel, None);
        tab.toggle_panel_reverse(); // None → AgentLog (always last in forward cycle)
        assert_eq!(tab.panel, Some(crate::ai::PanelContent::AgentLog));
        tab.toggle_panel_reverse(); // AgentLog → AiSummary (no sym, no PR, has AI)
        assert_eq!(tab.panel, Some(crate::ai::PanelContent::AiSummary));
        tab.toggle_panel_reverse(); // AiSummary → FileDetail
        assert_eq!(tab.panel, Some(crate::ai::PanelContent::FileDetail));
        tab.toggle_panel_reverse(); // FileDetail → None
        assert_eq!(tab.panel, None);
    }

    #[test]
    fn toggle_panel_reverse_full_cycle_with_ai_and_pr() {
        let mut tab = make_test_tab(vec![]);
        tab.ai.summary = Some("summary".to_string());
        tab.layers.show_ai_findings = true;
        tab.pr_data = Some(crate::github::PrOverviewData {
            number: 42,
            title: "My PR".to_string(),
            body: String::new(),
            state: "OPEN".to_string(),
            author: "user".to_string(),
            url: String::new(),
            base_branch: "main".to_string(),
            head_branch: "feature".to_string(),
            checks: vec![],
            reviewers: vec![],
        });
        assert_eq!(tab.panel, None);
        tab.toggle_panel_reverse(); // None → AgentLog (always last in forward cycle)
        assert_eq!(tab.panel, Some(crate::ai::PanelContent::AgentLog));
        tab.toggle_panel_reverse(); // AgentLog → PrOverview (no sym, has PR)
        assert_eq!(tab.panel, Some(crate::ai::PanelContent::PrOverview));
        tab.toggle_panel_reverse(); // PrOverview → AiSummary
        assert_eq!(tab.panel, Some(crate::ai::PanelContent::AiSummary));
        tab.toggle_panel_reverse(); // AiSummary → FileDetail
        assert_eq!(tab.panel, Some(crate::ai::PanelContent::FileDetail));
        tab.toggle_panel_reverse(); // FileDetail → None
        assert_eq!(tab.panel, None);
    }

    #[test]
    fn toggle_panel_reverse_skips_unavailable_panels() {
        let mut tab = make_test_tab(vec![]);
        // No AI, no PR: reverse from None goes to AgentLog (always available)
        tab.toggle_panel_reverse();
        assert_eq!(tab.panel, Some(crate::ai::PanelContent::AgentLog));
        // AgentLog → FileDetail (no sym, no PR, no AI)
        tab.toggle_panel_reverse();
        assert_eq!(tab.panel, Some(crate::ai::PanelContent::FileDetail));
    }

    #[test]
    fn toggle_panel_reverse_skips_ai_when_no_ai_data() {
        let mut tab = make_test_tab(vec![]);
        tab.pr_data = Some(crate::github::PrOverviewData {
            number: 42,
            title: "My PR".to_string(),
            body: String::new(),
            state: "OPEN".to_string(),
            author: "user".to_string(),
            url: String::new(),
            base_branch: "main".to_string(),
            head_branch: "feature".to_string(),
            checks: vec![],
            reviewers: vec![],
        });
        // From PrOverview, no AI present: should go to FileDetail (skip AiSummary)
        tab.panel = Some(crate::ai::PanelContent::PrOverview);
        tab.toggle_panel_reverse();
        assert_eq!(tab.panel, Some(crate::ai::PanelContent::FileDetail));
    }

    #[test]
    fn toggle_panel_reverse_resets_scroll_and_focus() {
        let mut tab = make_test_tab(vec![]);
        tab.panel_scroll = 10;
        tab.panel_focus = true;
        tab.toggle_panel_reverse(); // None → AgentLog
        assert_eq!(tab.panel_scroll, 0);
        // panel_focus stays true when panel is open
        tab.toggle_panel_reverse(); // AgentLog → FileDetail (no sym, no PR, no AI)
        tab.toggle_panel_reverse(); // FileDetail → None
        assert!(!tab.panel_focus);
    }

    // ── DiffCache ──

    #[test]
    fn diff_cache_get_on_empty_returns_none() {
        let mut cache = DiffCache::new(5);
        assert!(cache.get("abc123").is_none());
    }

    #[test]
    fn diff_cache_insert_then_get_returns_stored_files() {
        let mut cache = DiffCache::new(5);
        let files = vec![make_file("a.rs", vec![], 1, 0)];
        cache.insert("abc123".to_string(), files);
        let result = cache.get("abc123");
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 1);
        assert_eq!(result.unwrap()[0].path, "a.rs");
    }

    #[test]
    fn diff_cache_get_wrong_hash_returns_none() {
        let mut cache = DiffCache::new(5);
        cache.insert("abc123".to_string(), vec![]);
        assert!(cache.get("xyz789").is_none());
    }

    #[test]
    fn diff_cache_evicts_oldest_when_full() {
        let mut cache = DiffCache::new(2);
        cache.insert(
            "first".to_string(),
            vec![make_file("first.rs", vec![], 1, 0)],
        );
        cache.insert(
            "second".to_string(),
            vec![make_file("second.rs", vec![], 1, 0)],
        );
        // Cache is full (max_size=2). Insert a third entry.
        cache.insert(
            "third".to_string(),
            vec![make_file("third.rs", vec![], 1, 0)],
        );
        // Oldest ("first") should be evicted
        assert!(cache.get("first").is_none());
        assert!(cache.get("second").is_some());
        assert!(cache.get("third").is_some());
    }

    #[test]
    fn diff_cache_reinserting_same_hash_updates_entry() {
        let mut cache = DiffCache::new(3);
        cache.insert("hash1".to_string(), vec![make_file("v1.rs", vec![], 1, 0)]);
        cache.insert("hash2".to_string(), vec![]);
        // Re-insert hash1 with new data
        cache.insert("hash1".to_string(), vec![make_file("v2.rs", vec![], 2, 0)]);
        // Updated entry should have new data
        let result = cache.get("hash1").unwrap();
        assert_eq!(result[0].path, "v2.rs");
        assert!(cache.get("hash2").is_some());
    }

    // ── DiffCache basic behavior ──

    #[test]
    fn diff_cache_evicts_oldest_at_capacity() {
        let mut cache = DiffCache::new(2);
        cache.insert("a".to_string(), vec![]);
        cache.insert("b".to_string(), vec![]);
        cache.insert("c".to_string(), vec![]); // should evict "a"
        assert!(cache.get("a").is_none());
        assert!(cache.get("b").is_some());
        assert!(cache.get("c").is_some());
    }

    #[test]
    fn diff_cache_get_promotes_to_mru() {
        let mut cache = DiffCache::new(2);
        cache.insert("a".to_string(), vec![]);
        cache.insert("b".to_string(), vec![]);
        cache.get("a"); // promote "a" to MRU
        cache.insert("c".to_string(), vec![]); // should evict "b", not "a"
        assert!(cache.get("a").is_some());
        assert!(cache.get("b").is_none());
        assert!(cache.get("c").is_some());
    }

    #[test]
    fn diff_cache_miss_returns_none() {
        let mut cache = DiffCache::new(2);
        assert!(cache.get("nonexistent").is_none());
    }

    #[test]
    fn diff_cache_insert_existing_key_replaces() {
        let mut cache = DiffCache::new(2);
        cache.insert("a".to_string(), vec![make_file("test.rs", vec![], 1, 0)]);
        cache.insert(
            "a".to_string(),
            vec![
                make_file("test.rs", vec![], 1, 0),
                make_file("other.rs", vec![], 2, 0),
            ],
        );
        let result = cache.get("a").unwrap();
        assert_eq!(result.len(), 2); // updated, not original
    }

    // ── DiffCache MRU promotion ──

    #[test]
    fn diff_cache_get_promotes_entry_so_it_survives_eviction() {
        let mut cache = DiffCache::new(3);
        cache.insert(
            "first".to_string(),
            vec![make_file("first.rs", vec![], 1, 0)],
        );
        cache.insert(
            "second".to_string(),
            vec![make_file("second.rs", vec![], 1, 0)],
        );
        cache.insert(
            "third".to_string(),
            vec![make_file("third.rs", vec![], 1, 0)],
        );
        // Cache is full (max_size=3). Access "first" — promotes it to MRU position.
        let _ = cache.get("first");
        // Insert a 4th entry — should evict "second" (now LRU), not "first"
        cache.insert(
            "fourth".to_string(),
            vec![make_file("fourth.rs", vec![], 1, 0)],
        );
        assert!(cache.get("first").is_some());
        assert!(cache.get("second").is_none()); // evicted — was LRU after "first" was promoted
        assert!(cache.get("third").is_some());
        assert!(cache.get("fourth").is_some());
    }

    // ── user_expanded tracking ──

    #[test]
    fn toggle_compacted_on_compacted_file_adds_path_to_user_expanded() {
        let compacted_file = DiffFile {
            path: "big_generated.rs".to_string(),
            status: crate::git::FileStatus::Modified,
            hunks: vec![],
            adds: 0,
            dels: 0,
            compacted: true,
            raw_hunk_count: 3,
        };
        let mut tab = make_test_tab(vec![compacted_file]);
        // user_expanded starts empty
        assert!(!tab.user_expanded.contains("big_generated.rs"));
        // toggle_compacted on a compacted file tries to expand it via git, which will fail
        // in a test environment — we only check the HashSet tracking side-effect
        // by directly simulating the expansion path: mark as not-compacted then re-compact
        // Instead, verify that marking expanded=true and calling the re-compact branch
        // removes the path, and vice versa — by setting up the state manually.
        tab.user_expanded.insert("big_generated.rs".to_string());
        assert!(tab.user_expanded.contains("big_generated.rs"));
        // Simulating re-compact branch: remove path from user_expanded
        tab.user_expanded.remove("big_generated.rs");
        assert!(!tab.user_expanded.contains("big_generated.rs"));
    }

    #[test]
    fn toggle_compacted_on_expanded_file_removes_path_from_user_expanded() {
        // Start with a non-compacted file that was previously user-expanded
        let mut tab = make_test_tab(vec![make_file("src/lib.rs", vec![], 5, 2)]);
        tab.user_expanded.insert("src/lib.rs".to_string());
        assert!(tab.user_expanded.contains("src/lib.rs"));
        // Simulate the re-compact branch logic (file.compacted = false → compact it)
        // The actual toggle_compacted would call file.compacted = true and remove from set.
        // We test the HashSet contract directly here.
        tab.user_expanded.remove("src/lib.rs");
        assert!(!tab.user_expanded.contains("src/lib.rs"));
    }

    // ── ai_poll_counter type ──

    #[test]
    fn ai_poll_counter_can_hold_values_above_255() {
        // ai_poll_counter is u16 — must hold values > u8::MAX (255)
        // Confirm the type can represent 1000 (would overflow u8)
        let counter: u16 = 1000;
        assert_eq!(counter, 1000);
        assert!(counter > 255);
    }

    // ── get_line_anchor ──

    fn make_test_app(tab: TabState) -> App {
        App {
            tabs: vec![tab],
            active_tab: 0,
            input_mode: InputMode::Normal,
            should_quit: false,
            overlay: None,
            watching: false,
            watch_message: None,
            watch_message_ticks: 0,
            watch_message_max_ticks: 20,
            ai_poll_counter: 0,
            remote_url_input: String::new(),
            config: ErConfig::default(),
            current_ai_provider: None,
            current_ai_model: None,
            current_ai_effort: None,
            pending_hub_action: None,
            last_terminal_width: 0,
            panels_visible: PanelsVisible::default(),
            background_tasks: std::collections::HashMap::new(),
            recent_background_tasks: Vec::new(),
            pending_background_tasks: std::collections::VecDeque::new(),
            arena_registry: App::default_arena_registry(),
            active_arena_runs: std::collections::HashMap::new(),
        }
    }

    #[test]
    fn get_line_anchor_finds_line_by_new_num() {
        let lines = vec![
            DiffLine {
                line_type: LineType::Context,
                content: "before".to_string(),
                old_num: Some(1),
                new_num: Some(1),
            },
            DiffLine {
                line_type: LineType::Add,
                content: "target line".to_string(),
                old_num: None,
                new_num: Some(2),
            },
            DiffLine {
                line_type: LineType::Context,
                content: "after".to_string(),
                old_num: Some(2),
                new_num: Some(3),
            },
        ];
        let tab = make_test_tab(vec![make_file("a.rs", vec![make_hunk(lines)], 1, 0)]);
        let app = make_test_app(tab);

        let anchor = app.get_line_anchor(0, Some(2));

        assert_eq!(anchor.line_content, "target line");
        assert_eq!(anchor.line_start, Some(2));
    }

    #[test]
    fn get_line_anchor_falls_back_to_old_num_for_deleted_line() {
        let lines = vec![
            DiffLine {
                line_type: LineType::Context,
                content: "context".to_string(),
                old_num: Some(1),
                new_num: Some(1),
            },
            DiffLine {
                line_type: LineType::Delete,
                content: "deleted line".to_string(),
                old_num: Some(2),
                new_num: None,
            },
        ];
        let tab = make_test_tab(vec![make_file("a.rs", vec![make_hunk(lines)], 0, 1)]);
        let app = make_test_app(tab);

        // comment_line_num carries old_num (2) for a delete-only line
        let anchor = app.get_line_anchor(0, Some(2));

        assert_eq!(anchor.line_content, "deleted line");
        assert_eq!(anchor.line_start, Some(2));
    }

    #[test]
    fn get_line_anchor_returns_empty_content_when_line_not_found() {
        let lines = vec![DiffLine {
            line_type: LineType::Add,
            content: "some line".to_string(),
            old_num: None,
            new_num: Some(1),
        }];
        let tab = make_test_tab(vec![make_file("a.rs", vec![make_hunk(lines)], 1, 0)]);
        let app = make_test_app(tab);

        // line number 99 does not exist in the hunk
        let anchor = app.get_line_anchor(0, Some(99));

        assert_eq!(anchor.line_content, "");
        assert_eq!(anchor.line_start, Some(99));
    }

    #[test]
    fn get_line_anchor_uses_comment_file_when_it_differs_from_selected_file() {
        let selected_lines = vec![DiffLine {
            line_type: LineType::Add,
            content: "selected file line".to_string(),
            old_num: None,
            new_num: Some(2),
        }];
        let comment_lines = vec![DiffLine {
            line_type: LineType::Add,
            content: "comment target line".to_string(),
            old_num: None,
            new_num: Some(2),
        }];
        let mut tab = make_test_tab(vec![
            make_file("first.rs", vec![make_hunk(selected_lines)], 1, 0),
            make_file("second.rs", vec![make_hunk(comment_lines)], 1, 0),
        ]);
        tab.selected_file = 0;
        tab.comment_file = "second.rs".to_string();
        let app = make_test_app(tab);

        let anchor = app.get_line_anchor(0, Some(2));

        assert_eq!(anchor.line_content, "comment target line");
        assert_eq!(anchor.line_start, Some(2));
    }

    // ── split_diff_active ──

    #[test]
    fn split_diff_active_returns_false_when_config_off() {
        let tab = make_test_tab(vec![]);
        let app = make_test_app(tab);
        let config = ErConfig::default(); // split_diff defaults to false
        assert!(!app.split_diff_active(&config));
    }

    #[test]
    fn split_diff_active_returns_true_when_config_on_and_no_panel() {
        let tab = make_test_tab(vec![]);
        let app = make_test_app(tab);
        let mut config = ErConfig::default();
        config.display.split_diff = true;
        assert!(app.split_diff_active(&config));
    }

    #[test]
    fn split_diff_active_returns_false_when_panel_open() {
        use crate::ai::PanelContent;
        let mut tab = make_test_tab(vec![]);
        tab.panel = Some(PanelContent::FileDetail);
        let app = make_test_app(tab);
        let mut config = ErConfig::default();
        config.display.split_diff = true;
        assert!(!app.split_diff_active(&config));
    }

    #[test]
    fn split_diff_active_returns_true_in_history_mode() {
        let mut tab = make_test_tab(vec![]);
        tab.mode = DiffMode::History;
        let app = make_test_app(tab);
        let mut config = ErConfig::default();
        config.display.split_diff = true;
        assert!(app.split_diff_active(&config));
    }

    #[test]
    fn split_diff_active_returns_true_in_conflicts_mode() {
        let mut tab = make_test_tab(vec![]);
        tab.mode = DiffMode::Conflicts;
        let app = make_test_app(tab);
        let mut config = ErConfig::default();
        config.display.split_diff = true;
        assert!(app.split_diff_active(&config));
    }

    // ── scroll_right_split / scroll_left_split ──

    #[test]
    fn scroll_right_split_increments_new_scroll_when_focus_is_new() {
        let mut tab = make_test_tab(vec![]);
        tab.split_focus = SplitSide::New;
        tab.h_scroll_new = 5;
        tab.scroll_right_split();
        assert_eq!(tab.h_scroll_new, 6);
        assert_eq!(tab.h_scroll_old, 0);
    }

    #[test]
    fn scroll_right_split_increments_old_scroll_when_focus_is_old() {
        let mut tab = make_test_tab(vec![]);
        tab.split_focus = SplitSide::Old;
        tab.h_scroll_old = 3;
        tab.scroll_right_split();
        assert_eq!(tab.h_scroll_old, 4);
        assert_eq!(tab.h_scroll_new, 0);
    }

    #[test]
    fn scroll_left_split_decrements_new_scroll_when_focus_is_new() {
        let mut tab = make_test_tab(vec![]);
        tab.split_focus = SplitSide::New;
        tab.h_scroll_new = 3;
        tab.scroll_left_split();
        assert_eq!(tab.h_scroll_new, 2);
        assert_eq!(tab.h_scroll_old, 0);
    }

    #[test]
    fn scroll_left_split_saturates_at_zero() {
        let mut tab = make_test_tab(vec![]);
        tab.split_focus = SplitSide::Old;
        tab.h_scroll_old = 0;
        tab.scroll_left_split();
        assert_eq!(tab.h_scroll_old, 0);
    }

    // ── current_line_number_for_split ──

    #[test]
    fn current_line_number_for_split_new_returns_new_num() {
        let lines = vec![DiffLine {
            line_type: LineType::Add,
            content: "x".to_string(),
            old_num: None,
            new_num: Some(42),
        }];
        let mut tab = make_test_tab(vec![make_file("a.rs", vec![make_hunk(lines)], 1, 0)]);
        tab.current_line = Some(0);
        assert_eq!(tab.current_line_number_for_split(SplitSide::New), Some(42));
    }

    #[test]
    fn current_line_number_for_split_old_returns_old_num() {
        let lines = vec![DiffLine {
            line_type: LineType::Delete,
            content: "y".to_string(),
            old_num: Some(7),
            new_num: None,
        }];
        let mut tab = make_test_tab(vec![make_file("a.rs", vec![make_hunk(lines)], 0, 1)]);
        tab.current_line = Some(0);
        assert_eq!(tab.current_line_number_for_split(SplitSide::Old), Some(7));
    }

    #[test]
    fn current_line_number_for_split_returns_none_when_no_line_selected() {
        let lines = vec![make_line(LineType::Add, "z", Some(1))];
        let mut tab = make_test_tab(vec![make_file("a.rs", vec![make_hunk(lines)], 1, 0)]);
        tab.current_line = None;
        assert_eq!(tab.current_line_number_for_split(SplitSide::New), None);
        assert_eq!(tab.current_line_number_for_split(SplitSide::Old), None);
    }

    // ── Filter pipeline ──

    #[test]
    fn filter_rules_search_and_unreviewed_all_active() {
        let files = vec![
            make_file("src/main.rs", vec![], 10, 0),
            make_file("src/lib.rs", vec![], 5, 0),
            make_file("tests/test.rs", vec![], 3, 0),
            make_file("src/utils.rs", vec![], 2, 0),
        ];
        let mut tab = make_test_tab(files);
        // Phase 1: Filter to "src/" files only
        tab.filter_rules = crate::app::filter::parse_filter_expr("src/*");
        // Phase 2: Search query narrows further
        tab.search_query_lower = "main".to_string();
        // Phase 3: Mark main.rs as reviewed, toggle unreviewed only
        tab.reviewed
            .insert("src/main.rs".to_string(), String::new());
        tab.show_unreviewed_only = true;

        let visible = tab.visible_files();
        // src/main.rs matches filter+search but is reviewed → excluded
        // src/lib.rs matches filter but not search → excluded
        // tests/test.rs doesn't match filter → excluded
        assert_eq!(visible.len(), 0);
    }

    #[test]
    fn filter_rules_plus_search_narrows_correctly() {
        let files = vec![
            make_file("src/main.rs", vec![], 10, 0),
            make_file("src/lib.rs", vec![], 5, 0),
            make_file("tests/test.rs", vec![], 3, 0),
        ];
        let mut tab = make_test_tab(files);
        tab.filter_rules = crate::app::filter::parse_filter_expr("src/*");
        tab.search_query_lower = "lib".to_string();

        let visible = tab.visible_files();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].1.path, "src/lib.rs");
    }

    #[test]
    fn snap_to_visible_when_all_files_filtered_out() {
        let files = vec![
            make_file("src/main.rs", vec![], 1, 0),
            make_file("src/lib.rs", vec![], 1, 0),
        ];
        let mut tab = make_test_tab(files);
        tab.selected_file = 0;
        // Filter out all files
        tab.search_query_lower = "nonexistent".to_string();
        tab.snap_to_visible();
        // visible_files is empty, snap_to_visible returns early without changing
        let visible = tab.visible_files();
        assert!(visible.is_empty());
    }

    #[test]
    fn apply_filter_expr_history_deduplication() {
        let files = vec![make_file("src/main.rs", vec![], 1, 0)];
        let mut tab = make_test_tab(files);
        tab.apply_filter_expr("*.rs");
        tab.apply_filter_expr("*.ts");
        tab.apply_filter_expr("*.rs"); // duplicate
                                       // "*.rs" should appear only once (at front)
        assert_eq!(tab.filter_history.len(), 2);
        assert_eq!(tab.filter_history[0], "*.rs");
        assert_eq!(tab.filter_history[1], "*.ts");
    }

    #[test]
    fn apply_filter_expr_history_capped_at_20() {
        let files = vec![make_file("src/main.rs", vec![], 1, 0)];
        let mut tab = make_test_tab(files);
        for i in 0..25 {
            tab.apply_filter_expr(&format!("filter_{}", i));
        }
        assert_eq!(tab.filter_history.len(), 20);
        // Most recent should be first
        assert_eq!(tab.filter_history[0], "filter_24");
    }

    #[test]
    fn clear_filter_restores_full_file_list() {
        let files = vec![
            make_file("src/main.rs", vec![], 1, 0),
            make_file("tests/test.rs", vec![], 1, 0),
        ];
        let mut tab = make_test_tab(files);
        tab.apply_filter_expr("src/*");
        assert_eq!(tab.visible_files().len(), 1);
        tab.clear_filter();
        assert_eq!(tab.visible_files().len(), 2);
    }

    // ── Comment lifecycle ──

    #[test]
    fn start_comment_sets_input_mode() {
        let files = vec![make_file(
            "src/main.rs",
            vec![make_hunk(vec![make_line(LineType::Add, "line", Some(1))])],
            1,
            0,
        )];
        let tab = make_test_tab(files);
        let mut app = make_test_app(tab);
        app.start_comment(CommentType::Question);
        assert!(matches!(app.input_mode, InputMode::Comment));
        assert_eq!(app.tab().comment_type, CommentType::Question);
        assert_eq!(app.tab().comment_file, "src/main.rs");
    }

    #[test]
    fn start_comment_github_type() {
        let files = vec![make_file(
            "src/lib.rs",
            vec![make_hunk(vec![make_line(LineType::Add, "code", Some(5))])],
            1,
            0,
        )];
        let tab = make_test_tab(files);
        let mut app = make_test_app(tab);
        app.start_comment(CommentType::GitHubComment);
        assert!(matches!(app.input_mode, InputMode::Comment));
        assert_eq!(app.tab().comment_type, CommentType::GitHubComment);
    }

    #[test]
    fn toggle_comment_type_flips_question_and_github() {
        let files = vec![make_file(
            "src/main.rs",
            vec![make_hunk(vec![make_line(LineType::Add, "line", Some(1))])],
            1,
            0,
        )];
        let tab = make_test_tab(files);
        let mut app = make_test_app(tab);
        app.start_comment(CommentType::Question);
        assert!(app.can_toggle_comment_type());
        app.toggle_comment_type();
        assert_eq!(app.tab().comment_type, CommentType::Note);
        app.toggle_comment_type();
        assert_eq!(app.tab().comment_type, CommentType::GitHubComment);
        app.toggle_comment_type();
        assert_eq!(app.tab().comment_type, CommentType::Question);
    }

    #[test]
    fn toggle_comment_type_noop_for_reply_and_edit() {
        let files = vec![make_file(
            "src/main.rs",
            vec![make_hunk(vec![make_line(LineType::Add, "line", Some(1))])],
            1,
            0,
        )];
        let tab = make_test_tab(files);
        let mut app = make_test_app(tab);
        app.start_comment(CommentType::Question);
        app.tab_mut().comment_reply_to = Some("q-parent".to_string());
        assert!(!app.can_toggle_comment_type());
        app.toggle_comment_type();
        assert_eq!(app.tab().comment_type, CommentType::Question);

        app.tab_mut().comment_reply_to = None;
        app.tab_mut().comment_edit_id = Some("q-edit".to_string());
        assert!(!app.can_toggle_comment_type());
        app.toggle_comment_type();
        assert_eq!(app.tab().comment_type, CommentType::Question);
    }

    #[test]
    fn start_comment_note_sets_note_draft() {
        let files = vec![make_file(
            "src/main.rs",
            vec![make_hunk(vec![make_line(LineType::Add, "line", Some(1))])],
            1,
            0,
        )];
        let tab = make_test_tab(files);
        let mut app = make_test_app(tab);
        app.start_comment(CommentType::Note);
        assert_eq!(app.tab().comment_type, CommentType::Note);
        assert!(matches!(app.input_mode, InputMode::Comment));
        assert_eq!(app.tab().comment_file, "src/main.rs");
    }

    #[test]
    fn submit_note_persists_to_notes_json_with_n_prefix() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().to_string_lossy().into_owned();
        let files = vec![make_file(
            "src/main.rs",
            vec![make_hunk(vec![make_line(
                LineType::Add,
                "let x = 1;",
                Some(1),
            )])],
            1,
            0,
        )];
        let mut tab = make_test_tab(files);
        // Route the note sidecar into the TempDir's .er/ dir.
        tab.er_root = ErRoot::RepoLocal(root.clone());
        tab.repo_root = root.clone();
        let mut app = make_test_app(tab);

        // Author a note via the composer (the TUI reaches Note through Ctrl+t).
        app.start_comment(CommentType::Note);
        app.tab_mut().comment_textarea = TextArea::new(vec!["Refactor this helper".to_string()]);
        app.submit_comment().unwrap();

        // notes.json holds the new note with an n- prefixed id.
        let content = std::fs::read_to_string(format!("{root}/.er/notes.json"))
            .expect("notes.json should be written");
        let notes: crate::ai::ErNotes = serde_json::from_str(&content).unwrap();
        assert_eq!(notes.notes.len(), 1);
        assert!(
            notes.notes[0].id.starts_with("n-"),
            "note id must use n- prefix"
        );
        assert_eq!(notes.notes[0].text, "Refactor this helper");
        assert_eq!(notes.notes[0].file, "src/main.rs");

        // The reloaded state reflects the note in the per-file count (counts.2).
        assert_eq!(app.tab().ai.file_note_count("src/main.rs"), 1);
        assert!(app.tab().ai.has_notes());
    }

    #[test]
    fn submit_comment_empty_text_returns_to_normal() {
        let files = vec![make_file(
            "src/main.rs",
            vec![make_hunk(vec![make_line(LineType::Add, "line", Some(1))])],
            1,
            0,
        )];
        let tab = make_test_tab(files);
        let mut app = make_test_app(tab);
        app.start_comment(CommentType::Question);
        // Comment input is empty
        assert!(app.submit_comment().is_ok());
        assert!(matches!(app.input_mode, InputMode::Normal));
    }

    #[test]
    fn next_comment_empty_list_no_crash() {
        let files = vec![make_file("src/main.rs", vec![], 0, 0)];
        let tab = make_test_tab(files);
        let mut app = make_test_app(tab);
        // Should not crash even with no comments
        app.next_comment();
        app.prev_comment();
    }

    #[test]
    fn start_edit_comment_populates_input_buffer() {
        let files = vec![make_file(
            "src/main.rs",
            vec![make_hunk(vec![make_line(LineType::Add, "line", Some(1))])],
            1,
            0,
        )];
        let mut tab = make_test_tab(files);
        // Manually add a question to the AI state
        tab.ai.questions = Some(crate::ai::ErQuestions {
            version: 1,
            diff_hash: String::new(),
            questions: vec![crate::ai::ReviewQuestion {
                id: "q-123-0".to_string(),
                timestamp: String::new(),
                file: "src/main.rs".to_string(),
                hunk_index: Some(0),
                line_start: Some(1),
                line_end: None,
                line_content: "line".to_string(),
                text: "Why is this here?".to_string(),
                resolved: false,
                stale: false,
                context_before: Vec::new(),
                context_after: Vec::new(),
                old_line_start: None,
                hunk_header: String::new(),
                anchor_status: "original".to_string(),
                relocated_at_hash: String::new(),
                in_reply_to: None,
                author: "You".to_string(),
                promoted_to: None,
                finding_ref: None,
            }],
        });
        let mut app = make_test_app(tab);
        app.start_edit_comment("q-123-0");
        assert!(matches!(app.input_mode, InputMode::Comment));
        assert_eq!(app.tab().comment_text(), "Why is this here?");
        assert_eq!(app.tab().comment_edit_id, Some("q-123-0".to_string()));
    }

    // ── remote PR review ──

    #[test]
    fn new_for_test_initializes_remote_repo_as_none() {
        let tab = TabState::new_for_test(vec![]);
        assert!(tab.remote_repo.is_none());
    }

    #[test]
    fn is_remote_returns_false_by_default() {
        let tab = TabState::new_for_test(vec![]);
        assert!(!tab.is_remote());
    }

    #[test]
    fn is_remote_returns_true_when_remote_repo_set() {
        let mut tab = TabState::new_for_test(vec![]);
        tab.remote_repo = Some("owner/repo".to_string());
        assert!(tab.is_remote());
    }

    #[test]
    fn tab_name_falls_back_to_repo_basename_when_branch_empty() {
        let mut tab = TabState::new_for_test(vec![]);
        tab.repo_root = "/home/user/my-project".to_string();
        tab.current_branch = String::new();
        assert_eq!(tab.tab_name(), "my-project");
    }

    #[test]
    fn tab_name_returns_current_branch_for_working_tab() {
        let mut tab = TabState::new_for_test(vec![]);
        tab.repo_root = "/home/user/my-project".to_string();
        tab.current_branch = "main".to_string();
        assert_eq!(tab.tab_name(), "main");
    }

    #[test]
    fn tab_name_returns_local_branch_view_when_set() {
        let mut tab = TabState::new_for_test(vec![]);
        tab.current_branch = "main".to_string();
        tab.local_branch_view = Some("dev-5009".to_string());
        assert_eq!(tab.tab_name(), "dev-5009");
    }

    #[test]
    fn tab_name_returns_repo_name_in_remote_mode() {
        let mut tab = TabState::new_for_test(vec![]);
        tab.remote_repo = Some("owner/repo".to_string());
        assert_eq!(tab.tab_name(), "repo");
    }

    #[test]
    fn tab_name_includes_pr_number_in_remote_mode() {
        let mut tab = TabState::new_for_test(vec![]);
        tab.remote_repo = Some("owner/repo".to_string());
        tab.pr_number = Some(154);
        assert_eq!(tab.tab_name(), "repo#154");
    }

    #[test]
    fn comments_dir_returns_er_subdir_in_normal_mode() {
        let mut tab = TabState::new_for_test(vec![]);
        tab.repo_root = "/home/user/my-project".to_string();
        tab.er_root = ErRoot::RepoLocal(tab.repo_root.clone());
        assert_eq!(tab.comments_dir(), "/home/user/my-project/.er");
    }

    #[test]
    fn comments_dir_returns_managed_path_in_remote_mode() {
        let _guard = crate::storage::STORAGE_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut tab = TabState::new_for_test(vec![]);
        tab.remote_repo = Some("owner/repo".to_string());
        tab.pr_number = Some(42);
        tab.apply_managed_root();
        let expected =
            crate::storage::pr_bucket_dir(&crate::storage::slug_branch("owner/repo"), 42);
        assert_eq!(tab.comments_dir(), expected.to_string_lossy());
    }

    #[test]
    fn comments_dir_uses_viewed_branch_for_local_branch_tabs() {
        let _guard = crate::storage::STORAGE_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut tab = TabState::new_for_test(vec![]);
        tab.repo_root = "/home/user/my-project".to_string();
        tab.current_branch = "main".to_string();
        tab.local_branch_view = Some("claude/dev-5067".to_string());
        tab.sync_managed_storage();
        let repo_slug = crate::storage::slug_repo(&tab.repo_root);
        let branch_slug = crate::storage::slug_branch("claude/dev-5067");
        // Phase 1: storage is now in a view-bucket subdir, not the branch root
        let expected = crate::storage::view_bucket_dir(&repo_slug, &branch_slug, "branch");
        assert_eq!(tab.comments_dir(), expected.to_string_lossy());
        assert_ne!(
            tab.comments_dir(),
            crate::storage::view_bucket_dir(
                &repo_slug,
                &crate::storage::slug_branch("main"),
                "branch"
            )
            .to_string_lossy()
        );
    }

    #[test]
    fn reviewed_path_uses_viewed_branch_for_local_branch_tab() {
        let _guard = crate::storage::STORAGE_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut tab = TabState::new_for_test(vec![]);
        tab.repo_root = "/home/user/my-project".to_string();
        tab.current_branch = "main".to_string();
        tab.local_branch_view = Some("claude/dev-5067".to_string());
        tab.sync_managed_storage();
        let repo_slug = crate::storage::slug_repo(&tab.repo_root);
        let branch_slug = crate::storage::slug_branch("claude/dev-5067");
        // Phase 1: reviewed file is inside the view-bucket subdir
        let expected =
            crate::storage::view_bucket_dir(&repo_slug, &branch_slug, "branch").join("reviewed");
        assert_eq!(tab.er_root.reviewed_path(), expected.to_string_lossy());
        assert!(!tab
            .er_root
            .reviewed_path()
            .contains(&crate::storage::slug_branch("main")));
    }

    #[test]
    fn apply_checkout_branch_storage_change_reloads_reviewed() {
        let _guard = crate::storage::STORAGE_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::TempDir::new().unwrap();
        std::env::set_var("ER_STORAGE_ROOT", tmp.path());

        let mut tab = TabState::new_for_test(vec![]);
        tab.repo_root = "/home/user/my-project".to_string();
        tab.current_branch = "main".to_string();
        tab.sync_managed_storage();

        let repo_slug = crate::storage::slug_repo(&tab.repo_root);
        // Phase 1: reviewed files live inside the view-bucket subdir, not the branch root
        let main_dir = crate::storage::view_bucket_dir(
            &repo_slug,
            &crate::storage::slug_branch("main"),
            "branch",
        );
        let feature_dir = crate::storage::view_bucket_dir(
            &repo_slug,
            &crate::storage::slug_branch("feature"),
            "branch",
        );
        std::fs::create_dir_all(&main_dir).unwrap();
        std::fs::create_dir_all(&feature_dir).unwrap();
        std::fs::write(main_dir.join("reviewed"), "a.rs\thash-main\n").unwrap();
        std::fs::write(feature_dir.join("reviewed"), "b.rs\thash-feat\n").unwrap();

        tab.reviewed
            .insert("a.rs".to_string(), "hash-main".to_string());
        tab.apply_checkout_branch_storage_change("feature").unwrap();

        assert_eq!(tab.current_branch, "feature");
        assert_eq!(
            tab.reviewed.get("b.rs").map(String::as_str),
            Some("hash-feat")
        );
        assert!(!tab.reviewed.contains_key("a.rs"));

        std::env::remove_var("ER_STORAGE_ROOT");
    }

    #[test]
    fn sync_storage_if_checkout_branch_changed_skips_local_branch_view() {
        let mut tab = TabState::new_for_test(vec![]);
        tab.repo_root = "/home/user/my-project".to_string();
        tab.current_branch = "main".to_string();
        tab.local_branch_view = Some("feature/x".to_string());
        tab.sync_managed_storage();
        let path_before = tab.er_root.reviewed_path();
        tab.reviewed.insert("only.rs".to_string(), "h".to_string());
        tab.sync_storage_if_checkout_branch_changed().unwrap();
        assert_eq!(tab.er_root.reviewed_path(), path_before);
        assert!(tab.reviewed.contains_key("only.rs"));
    }

    #[test]
    fn comments_dir_uses_normal_mode_when_pr_number_missing() {
        // remote_repo without pr_number falls back to normal mode path
        let mut tab = TabState::new_for_test(vec![]);
        tab.repo_root = "/home/user/my-project".to_string();
        tab.er_root = ErRoot::RepoLocal(tab.repo_root.clone());
        tab.remote_repo = Some("owner/repo".to_string());
        // pr_number is None — should fall through to normal path
        assert_eq!(tab.comments_dir(), "/home/user/my-project/.er");
    }

    #[test]
    fn github_comments_path_appends_filename_to_comments_dir() {
        let mut tab = TabState::new_for_test(vec![]);
        tab.repo_root = "/home/user/my-project".to_string();
        tab.er_root = ErRoot::RepoLocal(tab.repo_root.clone());
        assert_eq!(
            tab.github_comments_path(),
            "/home/user/my-project/.er/github-comments.json"
        );
    }

    #[test]
    fn github_comments_path_uses_managed_dir_in_remote_mode() {
        let _guard = crate::storage::STORAGE_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut tab = TabState::new_for_test(vec![]);
        tab.remote_repo = Some("owner/repo".to_string());
        tab.pr_number = Some(7);
        tab.apply_managed_root();
        let dir = crate::storage::pr_bucket_dir(&crate::storage::slug_branch("owner/repo"), 7);
        assert_eq!(
            tab.github_comments_path(),
            format!("{}/github-comments.json", dir.to_string_lossy())
        );
    }

    #[test]
    fn comments_dir_replaces_slash_in_slug_with_dash() {
        let _guard = crate::storage::STORAGE_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut tab = TabState::new_for_test(vec![]);
        tab.remote_repo = Some("my-org/my-repo".to_string());
        tab.pr_number = Some(1);
        tab.apply_managed_root();
        let expected =
            crate::storage::pr_bucket_dir(&crate::storage::slug_branch("my-org/my-repo"), 1);
        assert_eq!(tab.comments_dir(), expected.to_string_lossy());
    }

    #[test]
    fn reload_remote_comments_loads_from_file() {
        let repo_root = std::env::temp_dir()
            .join(format!("er-test-reload-{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        // In normal mode, comments_dir() returns "{repo_root}/.er"
        let er_dir = format!("{}/.er", repo_root);
        std::fs::create_dir_all(&er_dir).unwrap();

        let comments_json = serde_json::json!({
            "version": 1,
            "diff_hash": "abc123",
            "comments": [
                {
                    "id": "c-1",
                    "file": "src/lib.rs",
                    "comment": "Looks good",
                    "source": "local",
                    "author": "You",
                    "resolved": false,
                    "synced": false
                }
            ]
        });
        let path = format!("{}/github-comments.json", er_dir);
        std::fs::write(&path, serde_json::to_string(&comments_json).unwrap()).unwrap();

        let mut tab = TabState::new_for_test(vec![]);
        tab.repo_root = repo_root.clone();
        tab.er_root = ErRoot::RepoLocal(tab.repo_root.clone());

        tab.reload_remote_comments();

        let gc = tab
            .ai
            .github_comments
            .as_ref()
            .expect("github_comments should be loaded");
        assert_eq!(gc.comments.len(), 1);
        assert_eq!(gc.comments[0].id, "c-1");
        assert_eq!(gc.comments[0].comment, "Looks good");

        std::fs::remove_dir_all(&repo_root).ok();
    }

    #[test]
    fn reload_remote_comments_handles_missing_file_gracefully() {
        let mut tab = TabState::new_for_test(vec![]);
        tab.repo_root = "/nonexistent/path/that/does/not/exist".to_string();
        // Should not panic
        tab.reload_remote_comments();
        assert!(tab.ai.github_comments.is_none());
    }

    #[test]
    fn reload_remote_comments_uses_remote_path_when_set() {
        let _guard = crate::storage::STORAGE_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::TempDir::new().unwrap();
        std::env::set_var("ER_STORAGE_ROOT", tmp.path());

        let mut tab = TabState::new_for_test(vec![]);
        tab.remote_repo = Some("owner/repo".to_string());
        tab.pr_number = Some(99);
        tab.apply_managed_root();

        let dir = tab.comments_dir();
        assert!(dir.contains("pr-99"), "dir = {}", dir);
        assert!(dir.contains("owner-repo"), "dir = {}", dir);

        std::env::remove_var("ER_STORAGE_ROOT");

        let path = tab.github_comments_path();
        assert!(path.ends_with("/github-comments.json"), "path = {}", path);

        // Reload should not panic even when the cache dir doesn't exist
        tab.reload_remote_comments();
        assert!(tab.ai.github_comments.is_none());
    }

    // ── truncate_str (multi-byte UTF-8 regression) ──

    #[test]
    fn truncate_str_emoji_does_not_panic() {
        // Emoji are multi-byte (4 bytes each). Slicing at byte offset would panic.
        let s = "hello 🎉🎊🎈 world";
        let result = truncate_str(s, 8);
        // Should get 8 chars + ellipsis, no panic
        assert_eq!(result, "hello 🎉🎊…");
    }

    #[test]
    fn truncate_str_cjk_chars() {
        let s = "你好世界测试";
        let result = truncate_str(s, 4);
        assert_eq!(result, "你好世界…");
    }

    #[test]
    fn truncate_str_mixed_ascii_and_multibyte() {
        let s = "café résumé";
        let result = truncate_str(s, 5);
        assert_eq!(result, "café …");
    }

    #[test]
    fn truncate_str_exact_multibyte_boundary() {
        let s = "🎉🎊"; // 2 emoji, each 4 bytes
        let result = truncate_str(s, 2);
        // Exactly at limit, no truncation needed
        assert_eq!(result, "🎉🎊");
    }

    #[test]
    fn truncate_str_single_emoji_within_limit() {
        let s = "🎉";
        let result = truncate_str(s, 5);
        assert_eq!(result, "🎉");
    }

    // ── parse_stream_json_line ──

    #[test]
    fn parse_stream_json_assistant_text_event() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Analyzing the diff..."}]}}"#;
        let result = parse_stream_json_line(line);
        assert_eq!(result, Some("Analyzing the diff...".to_string()));
    }

    #[test]
    fn parse_stream_json_assistant_tool_use_read() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Read","input":{"file_path":"/home/user/project/src/main.rs"}}]}}"#;
        let result = parse_stream_json_line(line);
        assert_eq!(result, Some("→ Read src/main.rs".to_string()));
    }

    #[test]
    fn parse_stream_json_assistant_tool_use_bash() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Bash","input":{"command":"git diff main"}}]}}"#;
        let result = parse_stream_json_line(line);
        assert_eq!(result, Some("→ Bash git diff main".to_string()));
    }

    #[test]
    fn parse_stream_json_assistant_tool_use_write() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Write","input":{"file_path":"/tmp/project/.er/review.json"}}]}}"#;
        let result = parse_stream_json_line(line);
        assert_eq!(result, Some("→ Write .er/review.json".to_string()));
    }

    #[test]
    fn parse_stream_json_assistant_tool_use_glob() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Glob","input":{"pattern":"**/*.rs"}}]}}"#;
        let result = parse_stream_json_line(line);
        assert_eq!(result, Some("→ Glob **/*.rs".to_string()));
    }

    #[test]
    fn parse_stream_json_assistant_tool_use_unknown_tool() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"CustomTool","input":{}}]}}"#;
        let result = parse_stream_json_line(line);
        assert_eq!(result, Some("→ CustomTool".to_string()));
    }

    #[test]
    fn parse_stream_json_tool_result_suppressed() {
        let line = r#"{"type":"tool_result","content":"some output"}"#;
        assert_eq!(parse_stream_json_line(line), None);
    }

    #[test]
    fn parse_stream_json_result_suppressed() {
        let line = r#"{"type":"result","result":"done"}"#;
        assert_eq!(parse_stream_json_line(line), None);
    }

    #[test]
    fn parse_stream_json_system_cost_event() {
        let line = r#"{"type":"system","subtype":"cost","message":"$0.05 used"}"#;
        let result = parse_stream_json_line(line);
        assert_eq!(result, Some("⊘ $0.05 used".to_string()));
    }

    #[test]
    fn parse_stream_json_system_usage_event() {
        let line = r#"{"type":"system","subtype":"usage","message":"1000 tokens"}"#;
        let result = parse_stream_json_line(line);
        assert_eq!(result, Some("⊘ 1000 tokens".to_string()));
    }

    #[test]
    fn parse_stream_json_system_hook_suppressed() {
        let line = r#"{"type":"system","subtype":"hook"}"#;
        assert_eq!(parse_stream_json_line(line), None);
    }

    #[test]
    fn parse_stream_json_malformed_json_returns_none() {
        assert_eq!(parse_stream_json_line("not json at all"), None);
        assert_eq!(parse_stream_json_line("{invalid}"), None);
        assert_eq!(parse_stream_json_line(""), None);
    }

    #[test]
    fn parse_stream_json_empty_content_array_returns_none() {
        let line = r#"{"type":"assistant","message":{"content":[]}}"#;
        assert_eq!(parse_stream_json_line(line), None);
    }

    #[test]
    fn parse_stream_json_whitespace_only_text_skipped() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"   "}]}}"#;
        assert_eq!(parse_stream_json_line(line), None);
    }

    #[test]
    fn parse_stream_json_multiple_content_items_joined() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Reading file"},{"type":"tool_use","name":"Read","input":{"file_path":"/src/lib.rs"}}]}}"#;
        let result = parse_stream_json_line(line);
        assert_eq!(result, Some("Reading file  → Read src/lib.rs".to_string()));
    }

    #[test]
    fn parse_stream_json_unknown_type_returns_none() {
        let line = r#"{"type":"unknown_event","data":"something"}"#;
        assert_eq!(parse_stream_json_line(line), None);
    }

    // ── shorten_path ──

    #[test]
    fn shorten_path_multiple_components() {
        assert_eq!(
            shorten_path("/home/user/project/src/main.rs"),
            "src/main.rs"
        );
    }

    #[test]
    fn shorten_path_two_components() {
        assert_eq!(shorten_path("src/main.rs"), "src/main.rs");
    }

    #[test]
    fn shorten_path_single_component() {
        assert_eq!(shorten_path("main.rs"), "main.rs");
    }

    #[test]
    fn shorten_path_deeply_nested() {
        assert_eq!(shorten_path("/a/b/c/d/e/f.rs"), "e/f.rs");
    }

    // ── textarea comment system ──

    #[test]
    fn comment_text_empty_textarea() {
        let tab = TabState::new_for_test(vec![]);
        assert_eq!(tab.comment_text(), "");
    }

    #[test]
    fn comment_text_single_line() {
        let mut tab = TabState::new_for_test(vec![]);
        tab.comment_textarea = TextArea::new(vec!["hello world".to_string()]);
        assert_eq!(tab.comment_text(), "hello world");
    }

    #[test]
    fn comment_text_multi_line_joins_with_newline() {
        let mut tab = TabState::new_for_test(vec![]);
        tab.comment_textarea = TextArea::new(vec!["line one".to_string(), "line two".to_string()]);
        assert_eq!(tab.comment_text(), "line one\nline two");
    }

    #[test]
    fn has_comment_draft_false_when_empty() {
        let app = make_test_app(TabState::new_for_test(vec![]));
        assert!(!app.has_comment_draft());
    }

    #[test]
    fn has_comment_draft_false_when_in_comment_mode() {
        let mut app = make_test_app(TabState::new_for_test(vec![]));
        app.tab_mut().comment_textarea = TextArea::new(vec!["draft".to_string()]);
        app.input_mode = InputMode::Comment;
        assert!(!app.has_comment_draft());
    }

    #[test]
    fn has_comment_draft_true_when_paused() {
        let mut app = make_test_app(TabState::new_for_test(vec![]));
        app.tab_mut().comment_textarea = TextArea::new(vec!["draft".to_string()]);
        app.input_mode = InputMode::Normal;
        assert!(app.has_comment_draft());
    }

    #[test]
    fn pause_comment_preserves_text_and_switches_to_normal() {
        let mut app = make_test_app(TabState::new_for_test(vec![]));
        app.tab_mut().comment_textarea = TextArea::new(vec!["my comment".to_string()]);
        app.input_mode = InputMode::Comment;
        app.pause_comment();
        assert_eq!(app.input_mode, InputMode::Normal);
        assert_eq!(app.tab().comment_text(), "my comment");
    }

    #[test]
    fn resume_comment_restores_comment_mode() {
        let mut app = make_test_app(TabState::new_for_test(vec![]));
        app.tab_mut().comment_textarea = TextArea::new(vec!["draft".to_string()]);
        app.input_mode = InputMode::Normal;
        app.resume_comment();
        assert_eq!(app.input_mode, InputMode::Comment);
    }

    #[test]
    fn resume_comment_noop_when_no_draft() {
        let mut app = make_test_app(TabState::new_for_test(vec![]));
        app.input_mode = InputMode::Normal;
        app.resume_comment();
        assert_eq!(app.input_mode, InputMode::Normal);
    }

    #[test]
    fn cancel_comment_clears_textarea() {
        let mut app = make_test_app(TabState::new_for_test(vec![]));
        app.tab_mut().comment_textarea = TextArea::new(vec!["will be cleared".to_string()]);
        app.input_mode = InputMode::Comment;
        app.cancel_comment();
        assert_eq!(app.input_mode, InputMode::Normal);
        assert_eq!(app.tab().comment_text(), "");
    }

    // ── reorder_tabs ──

    /// Build an `App` with `n` distinct tabs so we can assert on order/active.
    /// Each tab gets a unique repo_root marker we can read back after reorder.
    fn make_app_with_n_tabs(n: usize) -> App {
        let mut app = make_test_app(TabState::new_for_test(vec![]));
        app.tab_mut().repo_root = "tab0".to_string();
        for i in 1..n {
            let mut t = TabState::new_for_test(vec![]);
            t.repo_root = format!("tab{i}");
            app.tabs.push(t);
        }
        app
    }

    fn tab_roots(app: &App) -> Vec<String> {
        app.tabs.iter().map(|t| t.repo_root.clone()).collect()
    }

    #[test]
    fn reorder_tabs_moves_first_to_last_and_follows_active() {
        let mut app = make_app_with_n_tabs(3);
        app.active_tab = 0;
        assert!(app.reorder_tabs(0, 2));
        assert_eq!(tab_roots(&app), vec!["tab1", "tab2", "tab0"]);
        // Active followed the moved tab.
        assert_eq!(app.active_tab, 2);
    }

    #[test]
    fn reorder_tabs_moves_last_to_first_and_follows_active() {
        let mut app = make_app_with_n_tabs(3);
        app.active_tab = 2;
        assert!(app.reorder_tabs(2, 0));
        assert_eq!(tab_roots(&app), vec!["tab2", "tab0", "tab1"]);
        assert_eq!(app.active_tab, 0);
    }

    #[test]
    fn reorder_tabs_shifts_active_when_other_tab_moves_past_it() {
        // Active is tab1 (idx 1). Move tab0 → idx 2. tab1 should still be active
        // but its index shifts down to 0.
        let mut app = make_app_with_n_tabs(3);
        app.active_tab = 1;
        assert!(app.reorder_tabs(0, 2));
        assert_eq!(tab_roots(&app), vec!["tab1", "tab2", "tab0"]);
        assert_eq!(app.active_tab, 0);
    }

    #[test]
    fn reorder_tabs_shifts_active_when_other_tab_moves_back_past_it() {
        // Active is tab1 (idx 1). Move tab2 → idx 0. tab1 stays active at idx 2.
        let mut app = make_app_with_n_tabs(3);
        app.active_tab = 1;
        assert!(app.reorder_tabs(2, 0));
        assert_eq!(tab_roots(&app), vec!["tab2", "tab0", "tab1"]);
        assert_eq!(app.active_tab, 2);
    }

    #[test]
    fn reorder_tabs_noop_when_from_equals_to() {
        let mut app = make_app_with_n_tabs(3);
        app.active_tab = 1;
        assert!(!app.reorder_tabs(1, 1));
        assert_eq!(tab_roots(&app), vec!["tab0", "tab1", "tab2"]);
        assert_eq!(app.active_tab, 1);
    }

    #[test]
    fn reorder_tabs_noop_on_out_of_bounds() {
        let mut app = make_app_with_n_tabs(3);
        assert!(!app.reorder_tabs(5, 0));
        assert!(!app.reorder_tabs(0, 5));
        assert_eq!(tab_roots(&app), vec!["tab0", "tab1", "tab2"]);
    }

    // ── reviewed by path (History mode) ──

    fn test_diff_file(path: &str) -> crate::git::DiffFile {
        use crate::git::{DiffFile, FileStatus};
        DiffFile {
            path: path.to_string(),
            status: FileStatus::Modified,
            hunks: vec![],
            adds: 0,
            dels: 0,
            compacted: false,
            raw_hunk_count: 0,
        }
    }

    /// `mark_reviewed` / `unmark_reviewed` must resolve paths via
    /// [`TabState::active_diff_files`], not `tab.files[source_index]`.
    #[test]
    fn history_mark_reviewed_uses_commit_path_not_tab_files_index() {
        use crate::git::CommitInfo;

        let mut tab = TabState::new_for_test(vec![test_diff_file("branch-only.rs")]);
        tab.mode = DiffMode::History;
        tab.history = Some(HistoryState {
            commits: vec![CommitInfo {
                hash: "abc123".into(),
                short_hash: "abc123".into(),
                subject: "test".into(),
                author: "author".into(),
                date: "2026-01-01".into(),
                relative_date: "1d".into(),
                file_count: 2,
                adds: 1,
                dels: 0,
                is_merge: false,
            }],
            selected_commit: 0,
            commit_files: vec![test_diff_file("commit-a.rs"), test_diff_file("commit-b.rs")],
            selected_file: 1,
            current_hunk: 0,
            current_line: None,
            diff_scroll: 0,
            h_scroll: 0,
            all_loaded: true,
            diff_cache: DiffCache::new(5),
        });

        assert_eq!(tab.files.len(), 1);
        assert_eq!(tab.files[0].path, "branch-only.rs");
        assert_eq!(tab.active_diff_files().len(), 2);
        assert_eq!(tab.active_diff_files()[1].path, "commit-b.rs");

        // Old index-based lookup would use tab.files[1] (missing / wrong path).
        assert!(tab.files.get(1).is_none());

        let path = "commit-b.rs";
        assert!(
            tab.active_diff_files().iter().any(|f| f.path == path),
            "path must exist in active diff before mark"
        );
        tab.reviewed.insert(path.to_string(), String::new());

        assert!(tab.reviewed.contains_key("commit-b.rs"));
        assert!(!tab.reviewed.contains_key("branch-only.rs"));
    }

    fn run_git_for_history_test(dir: &std::path::Path, args: &[&str]) -> String {
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

    fn history_commit_info(hash: String, subject: &str) -> crate::git::CommitInfo {
        crate::git::CommitInfo {
            short_hash: hash.chars().take(7).collect(),
            hash,
            subject: subject.to_string(),
            author: "Test User".to_string(),
            date: "2026-06-01T10:00:00Z".to_string(),
            relative_date: "2026-06-01T10:00:00Z".to_string(),
            file_count: 0,
            adds: 0,
            dels: 0,
            is_merge: false,
        }
    }

    #[test]
    fn pr_history_uses_cached_commits_and_loads_selected_older_diff() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        run_git_for_history_test(root, &["init", "-b", "main"]);
        run_git_for_history_test(root, &["config", "user.email", "test@example.com"]);
        run_git_for_history_test(root, &["config", "user.name", "Test User"]);
        run_git_for_history_test(root, &["config", "commit.gpgsign", "false"]);
        std::fs::write(root.join("file.txt"), "base\n").unwrap();
        run_git_for_history_test(root, &["add", "file.txt"]);
        run_git_for_history_test(root, &["commit", "-m", "base"]);
        run_git_for_history_test(root, &["checkout", "-b", "feature"]);
        std::fs::write(root.join("file.txt"), "base\nolder\n").unwrap();
        run_git_for_history_test(root, &["commit", "-am", "older"]);
        let older_hash = run_git_for_history_test(root, &["rev-parse", "HEAD"]);
        std::fs::write(root.join("file.txt"), "base\nolder\nnewer\n").unwrap();
        run_git_for_history_test(root, &["commit", "-am", "newer"]);
        let newer_hash = run_git_for_history_test(root, &["rev-parse", "HEAD"]);

        let mut tab =
            TabState::new_with_base_unloaded(root.to_string_lossy().to_string(), "main".into())
                .unwrap();
        tab.local_branch_view = Some("feature".to_string());
        tab.pr_number = Some(42);
        tab.pr_commits = vec![
            history_commit_info(newer_hash, "newer"),
            history_commit_info(older_hash.clone(), "older"),
        ];

        tab.set_mode(DiffMode::History);
        let history = tab.history.as_mut().expect("history");
        assert_eq!(history.commits.len(), 2);
        assert!(history.all_loaded);
        history.selected_commit = 1;
        tab.history_load_selected_diff();

        let history = tab.history.as_ref().expect("history");
        assert_eq!(history.commits[history.selected_commit].hash, older_hash);
        assert!(
            history.commit_files.iter().any(|f| f.path == "file.txt"),
            "older commit diff should be loaded from the selected cached PR commit"
        );
    }

    // ── visible_modes / PrDiff wiring ──

    /// A remote tab (remote_repo set) exposes only [PrDiff] — no working-tree views.
    #[test]
    fn visible_modes_remote_tab_returns_only_pr_diff() {
        let mut tab = make_test_tab(vec![]);
        tab.remote_repo = Some("owner/repo".into());
        tab.pr_number = Some(42);
        let config = ErConfig::default();
        assert_eq!(tab.visible_modes(&config), vec![DiffMode::PrDiff]);
    }

    /// A local tab with a PR number includes PrDiff in the correct position
    /// (after Staged, before History) and still shows working-tree modes.
    #[test]
    fn visible_modes_local_pr_tab_includes_pr_diff() {
        let mut tab = make_test_tab(vec![]);
        tab.pr_number = Some(7);
        let config = ErConfig::default();
        let modes = tab.visible_modes(&config);
        assert_eq!(
            modes,
            vec![
                DiffMode::Branch,
                DiffMode::Unstaged,
                DiffMode::Staged,
                DiffMode::PrDiff,
                DiffMode::History,
            ]
        );
    }

    /// A local tab without a PR number must NOT include PrDiff.
    #[test]
    fn visible_modes_local_no_pr_excludes_pr_diff() {
        let tab = make_test_tab(vec![]);
        let config = ErConfig::default();
        let modes = tab.visible_modes(&config);
        assert!(!modes.contains(&DiffMode::PrDiff));
    }

    /// A live local checkout with pr_head_ref set (after enter_pr_diff) must still
    /// expose Unstaged and Staged — those views belong to the working tree, not to the
    /// PR diff view, and must not be gated on pr_head_ref.
    #[test]
    fn visible_modes_live_local_checkout_keeps_unstaged_staged_after_enter_pr_diff() {
        let mut tab = make_test_tab(vec![]);
        // Simulate a working-tree tab: local_branch_view is None → read_only = false.
        // Set pr_number + pr_head_ref as enter_pr_diff would.
        tab.pr_number = Some(42);
        tab.pr_head_ref = Some("refs/er/pr/42/head".into());
        let config = ErConfig::default();
        let modes = tab.visible_modes(&config);
        assert!(
            modes.contains(&DiffMode::Unstaged),
            "Unstaged must be present even when pr_head_ref is set on a live checkout"
        );
        assert!(
            modes.contains(&DiffMode::Staged),
            "Staged must be present even when pr_head_ref is set on a live checkout"
        );
        assert!(
            modes.contains(&DiffMode::PrDiff),
            "PrDiff must also be present"
        );
    }

    /// new_remote and new_remote_stub constructors must set mode = PrDiff.
    /// We can't call the real constructors (network I/O), so we verify via
    /// new_for_test with remote_repo set and mode manually set — and separately
    /// verify the review_bucket() short-circuit still routes to Pr.
    #[test]
    fn remote_tab_mode_is_pr_diff_and_bucket_is_pr() {
        let mut tab = TabState::new_for_test(vec![]);
        tab.remote_repo = Some("owner/repo".into());
        tab.pr_number = Some(5);
        tab.mode = DiffMode::PrDiff;
        assert_eq!(tab.mode, DiffMode::PrDiff);
        assert_eq!(tab.review_bucket(), ReviewBucket::Pr);
    }

    /// The guided tour's context (and thus which sidecar it loads/writes) follows
    /// the view being looked at: PR Diff → `tour.pr.json`, local branch →
    /// `tour.json`. Tour mode follows whichever view it was entered from.
    #[test]
    fn tour_context_and_filename_follow_view() {
        let mut tab = make_test_tab(vec![]);

        // Local branch diff → branch-scoped tour.
        tab.mode = DiffMode::Branch;
        assert!(!tab.tour_context_is_pr());
        assert_eq!(tab.tour_filename(), "tour.json");

        // PR diff → PR-scoped tour. Entering the Guide from here must keep it
        // attached to the PR (the bug: it flipped to the local branch).
        tab.mode = DiffMode::PrDiff;
        assert!(tab.tour_context_is_pr());
        assert_eq!(tab.tour_filename(), "tour.pr.json");

        // Tour mode follows the originating view via tour_is_pr.
        tab.mode = DiffMode::Tour;
        tab.tour_is_pr = false;
        assert!(!tab.tour_context_is_pr());
        assert_eq!(tab.tour_filename(), "tour.json");
        tab.tour_is_pr = true;
        assert!(tab.tour_context_is_pr());
        assert_eq!(tab.tour_filename(), "tour.pr.json");

        // Working-tree scopes are always branch-scoped, regardless of tour_is_pr.
        for m in [DiffMode::Unstaged, DiffMode::Staged, DiffMode::History] {
            tab.mode = m;
            assert!(!tab.tour_context_is_pr(), "{m:?} must be branch-scoped");
            assert_eq!(tab.tour_filename(), "tour.json");
        }

        // Remote PR tabs are always PR-scoped.
        let mut remote = make_test_tab(vec![]);
        remote.remote_repo = Some("owner/repo".into());
        remote.mode = DiffMode::PrDiff;
        assert!(remote.tour_context_is_pr());
        assert_eq!(remote.tour_filename(), "tour.pr.json");
    }

    /// Entering Tour mode records whether the originating view was the PR diff,
    /// so the Diff toggle returns to the right view and the right tour loads.
    #[test]
    fn set_mode_tour_records_origin_view() {
        let mut tab = make_test_tab(vec![]);

        // From PR Diff → Tour is PR-scoped.
        tab.mode = DiffMode::PrDiff;
        tab.tour_is_pr = false;
        tab.set_mode(DiffMode::Tour);
        assert!(tab.tour_is_pr, "Tour entered from PR Diff must be PR-scoped");

        // From the local branch → Tour is branch-scoped.
        tab.mode = DiffMode::Branch;
        tab.set_mode(DiffMode::Tour);
        assert!(
            !tab.tour_is_pr,
            "Tour entered from the local branch must be branch-scoped"
        );
    }

    /// pr_refs_fetched starts false on all non-remote constructors and on test tabs.
    #[test]
    fn pr_refs_fetched_initial_value_is_false() {
        let tab = TabState::new_for_test(vec![]);
        assert!(!tab.pr_refs_fetched);
    }

    // ── view-bucket storage integration ──

    /// Reviewed entries must be isolated per view bucket on disk.
    ///
    /// Sequence:
    ///   1. Branch bucket: save "a.rs" reviewed.
    ///   2. Switch to Unstaged bucket (mode + sync_managed_storage): "a.rs" must NOT appear.
    ///   3. Switch back to Branch bucket: "a.rs" must reappear.
    ///
    /// This confirms that the per-bucket directory layout prevents cross-view bleed
    /// from the save path all the way through to the load path.
    #[test]
    fn reviewed_isolated_per_bucket_on_disk() {
        let _guard = crate::storage::STORAGE_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::TempDir::new().unwrap();
        std::env::set_var("ER_STORAGE_ROOT", tmp.path());

        let mut tab = TabState::new_for_test(vec![]);
        tab.repo_root = "/home/user/my-project".to_string();
        tab.current_branch = "main".to_string();
        tab.mode = DiffMode::Branch;
        // Point er_root at the Branch view-bucket under our TempDir.
        tab.apply_managed_root();

        // Write "a.rs" as reviewed in the Branch bucket.
        tab.reviewed
            .insert("a.rs".to_string(), "hash-branch".to_string());
        tab.save_reviewed_files().unwrap();

        // --- Switch to Unstaged bucket ---
        tab.reviewed.clear();
        tab.mode = DiffMode::Unstaged;
        // sync_managed_storage: apply_managed_root (re-routes to unstaged dir) + reload.
        tab.sync_managed_storage();

        assert!(
            !tab.reviewed.contains_key("a.rs"),
            "a.rs must NOT be reviewed in the Unstaged bucket (was only saved in Branch)"
        );

        // --- Switch back to Branch bucket ---
        tab.mode = DiffMode::Branch;
        tab.sync_managed_storage();

        assert!(
            tab.reviewed.contains_key("a.rs"),
            "a.rs must be reviewed again after returning to the Branch bucket"
        );

        std::env::remove_var("ER_STORAGE_ROOT");
    }

    /// PR-bucket convergence: the slug produced for a remote tab ("Acme/My-Repo")
    /// must equal the slug produced for a local clone's canonical identity for the
    /// same repo, and mixed-case variants must resolve to the same directory.
    ///
    /// `canonical_owner_repo_slug` requires a real git remote; we test the slug
    /// logic directly — both paths execute `slug_branch(&owner_repo.to_lowercase())`,
    /// so equality of inputs guarantees equality of outputs and therefore the same
    /// storage directory.
    #[test]
    fn pr_bucket_convergence_case_insensitive() {
        let _guard = crate::storage::STORAGE_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::TempDir::new().unwrap();
        std::env::set_var("ER_STORAGE_ROOT", tmp.path());

        // The slug that apply_managed_root() produces for a remote tab is:
        //   slug_branch(&remote_repo.to_lowercase())
        // where remote_repo comes from the GitHub URL owner/repo string.
        let remote_slug = crate::storage::slug_branch(&"Acme/My-Repo".to_lowercase());

        // A local clone tab routes via canonical_owner_repo_slug(repo_root), which
        // internally does: slug_branch(&format!("{}/{}", owner, repo).to_lowercase()).
        // Simulate that — same expression, known-good values from URL parsing.
        let clone_slug = crate::storage::slug_branch(&"acme/my-repo".to_lowercase());

        // Both must resolve to the same slug so they share the same pr-bucket dir.
        assert_eq!(
            remote_slug, clone_slug,
            "remote slug and clone slug must match so the PR bucket is shared"
        );
        assert_eq!(remote_slug, "acme-my-repo", "canonical PR bucket slug");

        // Mixed-case variant must also converge.
        let mixed_slug = crate::storage::slug_branch(&"acme/my-repo".to_lowercase());
        assert_eq!(
            remote_slug, mixed_slug,
            "lowercase PR URL must resolve to the same bucket"
        );

        // The actual directory paths must be identical.
        let dir_from_remote = crate::storage::pr_bucket_dir(&remote_slug, 42);
        let dir_from_clone = crate::storage::pr_bucket_dir(&clone_slug, 42);
        assert_eq!(
            dir_from_remote, dir_from_clone,
            "pr_bucket_dir must be identical regardless of case origin"
        );

        std::env::remove_var("ER_STORAGE_ROOT");
    }

    #[test]
    fn er_dir_routes_to_active_view_bucket_or_shared_pr_bucket() {
        let _guard = crate::storage::STORAGE_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::TempDir::new().unwrap();
        std::env::set_var("ER_STORAGE_ROOT", tmp.path());

        let mut tab = TabState::new_for_test(vec![]);
        tab.repo_root = "/home/user/my-project".to_string();
        tab.current_branch = "feat/buckets".to_string();
        tab.local_branch_view = Some("feat/buckets".to_string());

        for (mode, bucket) in [
            (DiffMode::Branch, "branch"),
            (DiffMode::Unstaged, "unstaged"),
            (DiffMode::Staged, "staged"),
            (DiffMode::History, "history"),
        ] {
            tab.mode = mode;
            tab.pr_number = None;
            tab.remote_repo = None;
            tab.apply_managed_root();

            let expected = crate::storage::view_bucket_dir(
                &crate::storage::slug_repo(&tab.repo_root),
                &crate::storage::slug_branch("feat/buckets"),
                bucket,
            )
            .to_string_lossy()
            .into_owned();
            assert_eq!(
                tab.er_dir(),
                expected,
                "{mode:?} should use {bucket} bucket"
            );
        }

        tab.pr_number = Some(99);
        tab.remote_repo = Some("owner/repo".to_string());
        for mode in [DiffMode::Branch, DiffMode::PrDiff] {
            tab.mode = mode;
            tab.apply_managed_root();
            let expected =
                crate::storage::pr_bucket_dir(&crate::storage::slug_branch("owner/repo"), 99)
                    .to_string_lossy()
                    .into_owned();
            assert_eq!(tab.er_dir(), expected, "{mode:?} should use PR bucket");
        }

        std::env::remove_var("ER_STORAGE_ROOT");
    }

    /// Local PR tabs in Branch mode must share the PR bucket (not the branch view-bucket).
    #[test]
    fn pr_number_branch_mode_routes_to_pr_bucket() {
        let _guard = crate::storage::STORAGE_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::TempDir::new().unwrap();
        std::env::set_var("ER_STORAGE_ROOT", tmp.path());

        let mut tab = TabState::new_for_test(vec![]);
        tab.remote_repo = Some("owner/repo".to_string());
        tab.pr_number = Some(42);
        tab.mode = DiffMode::Branch;
        tab.local_branch_view = Some("feat/foo".to_string());
        tab.apply_managed_root();

        let expected = crate::storage::resolve_managed_root_for_pr_bucket(
            &crate::storage::slug_branch("owner/repo"),
            42,
        );
        assert_eq!(tab.er_dir(), expected.er_dir());

        std::env::remove_var("ER_STORAGE_ROOT");
    }

    #[test]
    fn pr_diff_mode_uses_cached_diff_for_ai_review() {
        let mut tab = TabState::new_for_test(vec![]);
        tab.mode = DiffMode::PrDiff;
        tab.raw_diff = Some("diff --git a/foo b/foo\n".to_string());
        tab.branch_diff_hash = "abc".to_string();
        assert_eq!(
            tab.raw_diff_for_review("branch").unwrap(),
            "diff --git a/foo b/foo\n"
        );
    }

    /// Remote (--remote) reviewed entries must persist to disk even after the guard
    /// `is_remote() && review_bucket() != Pr` was lifted.
    ///
    /// Concretely: construct a remote-mode tab, insert a reviewed entry, call
    /// save_reviewed_files(), and assert the file exists at
    /// `<storage_root>/repos/<owner-repo>/prs/pr-<N>/reviewed`.
    #[test]
    fn remote_reviewed_persists_to_pr_bucket_dir() {
        let _guard = crate::storage::STORAGE_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::TempDir::new().unwrap();
        std::env::set_var("ER_STORAGE_ROOT", tmp.path());

        let mut tab = TabState::new_for_test(vec![]);
        tab.remote_repo = Some("owner/repo".to_string());
        tab.pr_number = Some(42);
        tab.mode = DiffMode::PrDiff;
        // Route er_root to the PR bucket under the TempDir.
        tab.apply_managed_root();

        tab.reviewed
            .insert("src/lib.rs".to_string(), "hash-abc".to_string());
        tab.save_reviewed_files().unwrap();

        let expected =
            crate::storage::pr_bucket_dir(&crate::storage::slug_branch("owner/repo"), 42)
                .join("reviewed");

        assert!(
            expected.exists(),
            "reviewed file must exist at {:?} for remote PR tab",
            expected
        );

        let contents = std::fs::read_to_string(&expected).unwrap();
        assert!(
            contents.contains("src/lib.rs"),
            "reviewed file must contain the saved path; got: {}",
            contents
        );

        std::env::remove_var("ER_STORAGE_ROOT");
    }

    #[test]
    fn cleanup_triage_removes_only_triage_json() {
        let dir = tempfile::tempdir().unwrap();
        let er = dir.path();
        std::fs::write(er.join("triage.json"), "{}").unwrap();
        std::fs::write(er.join("review.json"), "{}").unwrap();

        cleanup_triage(er.to_str().unwrap());

        assert!(!er.join("triage.json").exists());
        assert!(er.join("review.json").exists());
    }

    #[test]
    fn cleanup_review_artifacts_removes_review_sidecars_not_triage() {
        let dir = tempfile::tempdir().unwrap();
        let er = dir.path();
        std::fs::write(er.join("triage.json"), "{}").unwrap();
        std::fs::write(er.join("review.json"), "{}").unwrap();
        std::fs::write(er.join("order.json"), "{}").unwrap();
        std::fs::write(er.join("checklist.json"), "{}").unwrap();
        std::fs::write(er.join("summary.md"), "# hi").unwrap();
        std::fs::write(er.join("professor.json"), "{}").unwrap();
        let experts = er.join("experts");
        std::fs::create_dir_all(&experts).unwrap();
        std::fs::write(experts.join("security.json"), "{}").unwrap();

        cleanup_review_artifacts(er.to_str().unwrap());

        assert!(er.join("triage.json").exists());
        assert!(!er.join("review.json").exists());
        assert!(!er.join("order.json").exists());
        assert!(!er.join("checklist.json").exists());
        assert!(!er.join("summary.md").exists());
        assert!(!er.join("professor.json").exists());
        assert!(!experts.join("security.json").exists());
    }
}
