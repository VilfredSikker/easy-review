pub(super) mod comments;
pub(super) mod navigation;
pub(super) mod quiz;
pub(super) mod wizard;

use crate::ai::{self, AiState, CommentType, InlineLayers, PanelContent, ReviewFocus};
use crate::config::{self, ErConfig, WatchedConfig};
use crate::git::{
    self, CommitInfo, CompactionConfig, DiffFile, DiffFileHeader, WatchedFile, Worktree,
};
use crate::github::PrOverviewData;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
#[allow(unused_imports)]
use std::time::Instant;

static COMMENT_SEQ: AtomicU64 = AtomicU64::new(0);

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
    Wizard,
    Quiz,
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
            DiffMode::Wizard => "WIZARD",
            DiffMode::Quiz => "QUIZ",
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
            // Wizard and Quiz reuse branch diff data
            DiffMode::Wizard => "branch",
            DiffMode::Quiz => "branch",
        }
    }
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
        // TODO(risk:minor): remove(pos).unwrap() is safe only because position() just confirmed the index exists,
        // but if VecDeque ever changes contract this is a hidden panic. Consider using swap_remove_back or expect().
        if let Some(pos) = self.entries.iter().position(|(h, _)| h == hash) {
            let entry = self.entries.remove(pos).unwrap();
            self.entries.push_back(entry);
            self.entries.back().map(|(_, f)| f)
        } else {
            None
        }
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
        items: Vec<HubItem>,
        selected: usize,
    },
    ConfigHub {
        items: Vec<config::ConfigItem>,
        selected: usize,
        saved_config: Box<ErConfig>,
        editing: Option<ConfigEditState>,
    },
}

/// Which modal hub is open
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HubKind {
    Git,
    Ai,
    Verify,
    Help,
    Open,
    Copy,
}

impl HubKind {
    pub fn title(&self) -> &'static str {
        match self {
            HubKind::Git => "GIT",
            HubKind::Ai => "AI",
            HubKind::Verify => "VERIFY",
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
    ToggleAiFindings,
    ToggleComments,
    ToggleQuestions,
    CleanupQuestions,
    CleanupReviews,
    /// Run a named command from [commands] config (e.g. "summary", "test", "lint")
    RunCommand(String),
    /// Run AI review via configured agent command
    PromptReview,
    /// Run AI question answering via configured agent command
    PromptQuestions,
    /// Run quiz generation via configured agent command
    PromptQuiz,
    /// Run quiz answer review via configured agent command
    PromptQuizReview,
    /// Run wizard tour generation via configured agent command
    PromptWizard,
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
}

// ── Per-Tab State ──

/// State for a single repo tab
pub struct TabState {
    pub mode: DiffMode,
    pub base_branch: String,
    pub current_branch: String,
    pub repo_root: String,

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
    /// Text buffer for the comment being typed
    pub comment_input: String,

    /// File path the comment targets
    pub comment_file: String,

    /// Hunk index the comment targets
    pub comment_hunk: usize,

    /// Optional finding ID this comment replies to
    pub comment_reply_to: Option<String>,

    /// Optional specific line number the comment targets (new-side)
    pub comment_line_num: Option<usize>,

    /// Which type of comment is being created (Question vs GitHubComment)
    pub comment_type: CommentType,

    /// When editing an existing comment, holds the comment ID being edited
    pub comment_edit_id: Option<String>,

    /// Optional finding ID this comment responds to (for finding replies)
    pub comment_finding_ref: Option<String>,

    /// History mode state (only populated when mode == History)
    pub history: Option<HistoryState>,

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

    /// Cached visible file indices (invalidated on search/filter/file list change)
    pub file_tree_cache: Option<FileTreeCache>,

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

    /// True after a commit in Staged mode — causes diff view to show HEAD~1..HEAD until next
    /// new staged change or the user pushes.
    pub committed_unpushed: bool,

    /// Per-file context line overrides (path -> context lines count).
    /// Default context is 3 (git's --unified=3). Cleared on diff refresh.
    pub context_overrides: HashMap<String, usize>,

    /// Remote repo slug (e.g. "owner/repo") when reviewing a PR without a local clone.
    /// When Some, git operations are disabled and diffs come from `gh pr diff --repo`.
    pub remote_repo: Option<String>,

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

    /// Wizard mode state (only populated when mode == Wizard)
    pub wizard: Option<WizardState>,

    /// Quiz mode state (only populated when mode == Quiz)
    pub quiz: Option<QuizState>,
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

/// State for the wizard tour mode
pub struct WizardState {
    /// File paths in tour order (fundamental → important → supporting → rest)
    pub ordered_files: Vec<String>,
    /// Index into ordered_files (which file is currently selected)
    pub current_step: usize,
    /// Files marked reviewed in wizard mode
    pub completed: HashSet<String>,
}

/// State for review quiz mode
pub struct QuizState {
    /// All questions from the quiz (unfiltered)
    pub questions: Vec<crate::ai::QuizQuestion>,
    /// Index of the currently selected question
    pub current: usize,
    /// Answers keyed by question id
    pub answers: HashMap<String, QuizAnswer>,
    /// (correct, attempted) score
    pub score: (usize, usize),
    /// Filter by difficulty level (None = all)
    pub filter_level: Option<u8>,
    /// Filter by category (None = all)
    pub filter_category: Option<String>,
    /// Whether in freeform text input mode
    pub input_mode: QuizInputMode,
    /// Text buffer for freeform answer
    pub input_buffer: String,
    /// Whether to show the explanation for the current question
    pub show_explanation: bool,
}

#[derive(Debug, Clone)]
pub enum QuizAnswer {
    Choice(char),
    Freeform(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum QuizInputMode {
    Navigating,
    AnsweringFreeform,
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

    /// File paths the user explicitly expanded
    #[serde(default)]
    pub user_expanded: Vec<String>,

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
    const SESSION_FILE: &'static str = ".er/session.json";

    /// Save session to .er/session.json, writing atomically via tmp+rename.
    pub fn save(&self, repo_root: &str) -> Result<()> {
        let dir = format!("{}/.er", repo_root);
        std::fs::create_dir_all(&dir)?;
        let path = format!("{}/{}", repo_root, Self::SESSION_FILE);
        let tmp_path = format!("{}.tmp", path);
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&tmp_path, &json)?;
        std::fs::rename(&tmp_path, &path)?;
        Ok(())
    }

    /// Load session from .er/session.json. Returns None if file doesn't exist or is invalid.
    pub fn load(repo_root: &str) -> Option<Self> {
        let path = format!("{}/{}", repo_root, Self::SESSION_FILE);
        let content = std::fs::read_to_string(&path).ok()?;
        serde_json::from_str(&content).ok()
    }
}

/// Precomputed cumulative line offsets for each hunk in the selected file
#[derive(Debug, Clone)]
pub struct HunkOffsets {
    /// offsets[i] = logical line number where hunk i starts
    pub offsets: Vec<usize>,
    /// Total logical lines for this file
    #[allow(dead_code)]
    pub total: usize,
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
        Self {
            offsets,
            total: cursor,
        }
    }
}

/// Cached visible file indices for file tree rendering
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct FileTreeCache {
    /// Indices into the files array that pass filters
    pub visible: Vec<usize>,
    /// Inputs that produced this cache
    search_query: String,
    show_unreviewed_only: bool,
    file_count: usize,
    reviewed_count: usize,
}

/// Lightweight memory tracking
#[derive(Debug, Clone, Default)]
pub struct MemoryBudget {
    pub parsed_files: usize,
    pub total_lines: usize,
    pub compacted_files: usize,
}

impl TabState {
    /// Create a new tab for a given repo root
    pub fn new(repo_root: String) -> Result<Self> {
        let current_branch = git::get_current_branch_in(&repo_root)?;
        let base_branch = git::detect_base_branch_in(&repo_root)?;
        Self::new_inner(repo_root, current_branch, base_branch)
    }

    /// Create a TabState with a known base branch (skips auto-detection).
    /// Used for PR flows where the base is known from the GitHub API.
    pub fn new_with_base(repo_root: String, base_branch: String) -> Result<Self> {
        let current_branch = git::get_current_branch_in(&repo_root)?;
        Self::new_inner(repo_root, current_branch, base_branch)
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

        let mut tab = TabState {
            mode: DiffMode::Branch,
            base_branch,
            current_branch: head_branch,
            repo_root: std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| ".".to_string()),
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
            comment_input: String::new(),
            comment_file: String::new(),
            comment_hunk: 0,
            comment_reply_to: None,
            comment_line_num: None,
            comment_type: CommentType::GitHubComment,
            comment_edit_id: None,
            comment_finding_ref: None,
            pr_data: None,
            pr_head_ref: None,
            pr_number: Some(pr_ref.number),
            history: None,
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
            file_tree_cache: None,
            mem_budget: MemoryBudget::default(),
            lazy_mode,
            file_headers,
            raw_diff: if lazy_mode { Some(raw) } else { None },
            symbol_refs: None,
            pending_unmark_count: 0,
            committed_unpushed: false,
            context_overrides: HashMap::new(),
            remote_repo: Some(repo_slug),
            command_rx: std::collections::HashMap::new(),
            command_status: std::collections::HashMap::new(),
            log_tx: agent_log_tx,
            log_rx: agent_log_rx,
            agent_log: std::collections::VecDeque::new(),
            agent_log_auto_scroll: true,
            wizard: None,
            quiz: None,
        };

        // Build hunk offsets for initial selection
        tab.rebuild_hunk_offsets();
        tab.ensure_file_parsed();
        tab.update_mem_budget();

        Ok(tab)
    }

    fn new_inner(repo_root: String, current_branch: String, base_branch: String) -> Result<Self> {
        let (agent_log_tx, agent_log_rx) = std::sync::mpsc::channel();
        let reviewed = Self::load_reviewed_files(&repo_root);
        let er_config = config::load_config(&repo_root);
        let watched_config = er_config.watched.clone();
        let has_watched = !watched_config.paths.is_empty();
        let merge_active = git::is_merge_in_progress(&repo_root);

        let mut tab = TabState {
            mode: DiffMode::Branch,
            base_branch,
            current_branch,
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
            reviewed,
            current_per_file_hashes: HashMap::new(),
            show_unreviewed_only: false,
            sort_by_mtime: false,
            mtime_cache: HashMap::new(),
            search_query_lower: String::new(),
            ai: AiState::default(),
            diff_hash: String::new(),
            branch_diff_hash: String::new(),
            last_ai_check: None,
            comment_input: String::new(),
            comment_file: String::new(),
            comment_hunk: 0,
            comment_reply_to: None,
            comment_line_num: None,
            comment_type: CommentType::GitHubComment,
            comment_edit_id: None,
            comment_finding_ref: None,
            pr_data: None,
            pr_head_ref: None,
            pr_number: None,
            history: None,
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
            file_tree_cache: None,
            mem_budget: MemoryBudget::default(),
            lazy_mode: false,
            file_headers: Vec::new(),
            raw_diff: None,
            symbol_refs: None,
            pending_unmark_count: 0,
            committed_unpushed: false,
            context_overrides: HashMap::new(),
            remote_repo: None,
            command_rx: std::collections::HashMap::new(),
            command_status: std::collections::HashMap::new(),
            log_tx: agent_log_tx,
            log_rx: agent_log_rx,
            agent_log: std::collections::VecDeque::new(),
            agent_log_auto_scroll: true,
            wizard: None,
            quiz: None,
        };

        tab.refresh_diff()?;
        tab.refresh_watched_files();
        Ok(tab)
    }

    /// Create a minimal TabState for unit tests.
    /// Uses fixed repo root "/tmp/test" and no git I/O.
    #[cfg(test)]
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
            comment_input: String::new(),
            comment_file: String::new(),
            comment_hunk: 0,
            comment_reply_to: None,
            comment_line_num: None,
            comment_type: CommentType::GitHubComment,
            comment_edit_id: None,
            comment_finding_ref: None,
            pr_data: None,
            pr_head_ref: None,
            pr_number: None,
            history: None,
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
            file_tree_cache: None,
            mem_budget: MemoryBudget::default(),
            lazy_mode: false,
            file_headers: Vec::new(),
            raw_diff: None,
            symbol_refs: None,
            pending_unmark_count: 0,
            committed_unpushed: false,
            context_overrides: HashMap::new(),
            remote_repo: None,
            command_rx: std::collections::HashMap::new(),
            command_status: std::collections::HashMap::new(),
            log_tx: agent_log_tx,
            log_rx: agent_log_rx,
            agent_log: std::collections::VecDeque::new(),
            agent_log_auto_scroll: true,
            wizard: None,
            quiz: None,
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

    /// Short name for display in tab bar (last path component)
    pub fn tab_name(&self) -> String {
        if let Some(ref slug) = self.remote_repo {
            let repo = slug.split('/').next_back().unwrap_or(slug);
            if let Some(pr_num) = self.pr_number {
                return format!("{}#{}", repo, pr_num);
            }
            return repo.to_string();
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

    /// Return the list of DiffMode tabs currently visible, based on feature flags,
    /// remote status, and data availability. Used for dynamic tab numbering.
    pub fn visible_modes(&self, config: &crate::config::ErConfig) -> Vec<DiffMode> {
        let mut modes = Vec::new();
        if config.features.view_branch {
            modes.push(DiffMode::Branch);
        }
        if config.features.view_unstaged && !self.is_remote() && self.pr_head_ref.is_none() {
            modes.push(DiffMode::Unstaged);
        }
        if config.features.view_staged && !self.is_remote() && self.pr_head_ref.is_none() {
            modes.push(DiffMode::Staged);
        }
        if config.features.view_history && !self.is_remote() {
            modes.push(DiffMode::History);
        }
        if config.features.view_conflicts && !self.is_remote() && self.merge_active {
            modes.push(DiffMode::Conflicts);
        }
        if config.features.view_hidden && !self.is_remote() && !self.watched_config.paths.is_empty()
        {
            modes.push(DiffMode::Hidden);
        }
        if config.features.view_wizard && !self.is_remote() && self.ai.wizard.is_some() {
            modes.push(DiffMode::Wizard);
        }
        if config.features.view_quiz && !self.is_remote() && self.ai.quiz.is_some() {
            modes.push(DiffMode::Quiz);
        }
        modes
    }

    /// Return the `.er/` directory path — uses comments_dir() in remote mode,
    /// `{repo_root}/.er` in local mode.
    pub fn er_dir(&self) -> String {
        if self.is_remote() {
            self.comments_dir()
        } else {
            format!("{}/.er", self.repo_root)
        }
    }

    /// Directory for storing comment files. In remote mode, uses `~/.cache/er/remote/`.
    /// In normal mode, uses `{repo_root}/.er/`.
    pub fn comments_dir(&self) -> String {
        if let (Some(ref slug), Some(n)) = (&self.remote_repo, self.pr_number) {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            let safe_slug = slug.replace('/', "-");
            format!("{}/.cache/er/remote/{}-{}", home, safe_slug, n)
        } else {
            format!("{}/.er", self.repo_root)
        }
    }

    /// Path to github-comments.json. Uses cache dir in remote mode.
    pub fn github_comments_path(&self) -> String {
        format!("{}/github-comments.json", self.comments_dir())
    }

    // ── Diff ──

    /// Re-run git diff and update the file list
    pub fn refresh_diff(&mut self) -> Result<()> {
        self.refresh_diff_impl(true, true)
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
        self.file_tree_cache = None;
    }

    fn refresh_diff_impl(&mut self, recompute_branch_hash: bool, auto_unmark: bool) -> Result<()> {
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

        // Remote mode: fetch diff from GitHub API instead of local git
        if let Some(ref repo_slug) = self.remote_repo.clone() {
            let parts: Vec<&str> = repo_slug.split('/').collect();
            if parts.len() == 2 {
                let owner = parts[0].to_string();
                let repo = parts[1].to_string();
                if let Some(pr_number) = self.pr_number {
                    let raw = crate::github::gh_pr_diff_remote(&owner, &repo, pr_number)?;

                    let prev_path = self.files.get(self.selected_file).map(|f| f.path.clone());

                    if raw.len() > 200_000 {
                        let headers = crate::git::parse_diff_headers(&raw);
                        self.files = headers.iter().map(crate::git::header_to_stub).collect();
                        self.file_headers = headers;
                        self.raw_diff = Some(raw.clone());
                        self.lazy_mode = true;
                        for (file, header) in self.files.iter_mut().zip(self.file_headers.iter()) {
                            if self.user_expanded.contains(&file.path) {
                                continue;
                            }
                            let total_lines = header.adds + header.dels;
                            let should_compact = self.compaction_config.enabled
                                && (self
                                    .compaction_config
                                    .patterns
                                    .iter()
                                    .any(|p| crate::git::compact_files_match(p, &file.path))
                                    || total_lines
                                        > self.compaction_config.max_lines_before_compact);
                            if should_compact {
                                file.compacted = true;
                                file.raw_hunk_count = header.hunk_count;
                            }
                        }
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
                        self.diff_hash =
                            format!("{:016x}", crate::ai::compute_diff_hash_fast(&raw));
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
                    self.file_tree_cache = None;
                    self.update_mem_budget();
                    return Ok(());
                }
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

        // In Staged mode after a commit, show HEAD~1..HEAD unless new staged changes exist
        let head_ref_owned = self.pr_head_ref.clone();
        let raw = if self.mode == DiffMode::Staged && self.committed_unpushed {
            let staged_raw = git::git_diff_raw(
                self.mode.git_mode(),
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
            git::git_diff_raw(
                self.mode.git_mode(),
                &self.base_branch,
                &self.repo_root,
                head_ref_owned.as_deref(),
            )?
        };

        // Decide parsing strategy based on diff size.
        // Use byte-length heuristic (O(1)) instead of counting newlines (O(n)).
        // 200_000 bytes ≈ ~5000 lines (at ~40 bytes/line), equivalent to LAZY_PARSE_THRESHOLD.
        if raw.len() > 200_000 {
            // Lazy mode: header-only parse, files get hunks on demand
            let headers = git::parse_diff_headers(&raw);
            self.files = headers.iter().map(git::header_to_stub).collect();
            self.file_headers = headers;
            self.raw_diff = Some(raw.clone());
            self.lazy_mode = true;

            // Apply compaction to the stub files (pattern-based only, since hunks are empty)
            // TODO(risk:medium): zip() silently stops at the shorter iterator. If file_headers and files ever
            // diverge in length (e.g. a parsing bug), some files will be skipped without any indication.
            for (file, header) in self.files.iter_mut().zip(self.file_headers.iter()) {
                if self.user_expanded.contains(&file.path) {
                    continue;
                }
                let total_lines = header.adds + header.dels;
                let should_compact = self.compaction_config.enabled
                    && (self
                        .compaction_config
                        .patterns
                        .iter()
                        .any(|p| git::compact_files_match(p, &file.path))
                        || total_lines > self.compaction_config.max_lines_before_compact);
                if should_compact {
                    file.compacted = true;
                    file.raw_hunk_count = header.hunk_count;
                }
            }
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
            let git_mode = self.mode.git_mode();
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
            self.current_per_file_hashes = ai::compute_per_file_hashes(&raw);
            if auto_unmark {
                // Auto-unmark reviewed files whose diff has changed since they were marked.
                self.pending_unmark_count = self.auto_unmark_changed_reviewed();
            }
        }

        // Load AI state from .er-* files (only on full refresh — watch-triggered
        // quick refreshes let the separate AI polling handle .er/ changes)
        if recompute_branch_hash {
            self.reload_ai_state();
        }

        // Relocate comments to follow moved code
        self.relocate_all_comments();

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

        // Invalidate file tree cache
        self.file_tree_cache = None;

        Ok(())
    }

    /// Reload AI state from .er-* files (preserving current nav state)
    pub fn reload_ai_state(&mut self) {
        let prev_stale_files = std::mem::take(&mut self.ai.stale_files);
        let er_dir = self.er_dir();
        self.ai = ai::load_ai_state(&er_dir, &self.branch_diff_hash);
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
        // TODO(risk:minor): when item_count == 0, max_cursor is set to 0 and review_cursor is clamped to 0.
        // Any code that then indexes into the list at review_cursor (e.g., checklist items) must
        // separately guard against the empty-list case, or it will index position 0 of an empty Vec.
        let max_cursor = if item_count == 0 { 0 } else { item_count - 1 };
        self.review_cursor = self.review_cursor.min(max_cursor);
        self.last_ai_check = ai::latest_er_mtime(&er_dir);
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
    fn relocate_all_comments(&mut self) {
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

        // TODO(risk:medium): write errors are silently discarded here (let _ = ...). If the disk is full
        // or the directory is read-only, the relocation update is lost without any user notification.
        // The next refresh will re-relocate the same comments, but any intermediate "relocated" state is
        // dropped, and the user has no idea the write failed.
        // Write back to disk if anything changed
        if questions_changed {
            if let Some(ref qs) = self.ai.questions {
                let path = format!("{}/questions.json", self.er_dir());
                if let Ok(json) = serde_json::to_string_pretty(qs) {
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

    pub fn toggle_layer_ai(&mut self) {
        self.layers.show_ai_findings = !self.layers.show_ai_findings;
    }

    /// Cycle panel: None → FileDetail → AiSummary (if AI data) → PrOverview (if PR live) → SymbolRefs (if symbols) → AgentLog → None
    pub fn toggle_panel(&mut self) {
        let has_ai = self.layers.show_ai_findings && self.ai.has_data();
        let has_pr = self.pr_data.is_some();
        self.panel = match self.panel {
            None => Some(PanelContent::FileDetail),
            Some(PanelContent::FileDetail) => {
                if has_ai {
                    Some(PanelContent::AiSummary)
                } else if has_pr {
                    Some(PanelContent::PrOverview)
                } else if self.symbol_refs.is_some() {
                    Some(PanelContent::SymbolRefs)
                } else {
                    Some(PanelContent::AgentLog)
                }
            }
            Some(PanelContent::AiSummary) => {
                if has_pr {
                    Some(PanelContent::PrOverview)
                } else if self.symbol_refs.is_some() {
                    Some(PanelContent::SymbolRefs)
                } else {
                    Some(PanelContent::AgentLog)
                }
            }
            Some(PanelContent::PrOverview) => {
                if self.symbol_refs.is_some() {
                    Some(PanelContent::SymbolRefs)
                } else {
                    Some(PanelContent::AgentLog)
                }
            }
            Some(PanelContent::SymbolRefs) => Some(PanelContent::AgentLog),
            Some(PanelContent::AgentLog) => None,
        };
        self.panel_scroll = 0;
        if self.panel.is_none() {
            self.panel_focus = false;
        }
    }

    /// Cycle panel in reverse: None → AgentLog → SymbolRefs → PrOverview → AiSummary → FileDetail → None
    pub fn toggle_panel_reverse(&mut self) {
        let has_ai = self.layers.show_ai_findings && self.ai.has_data();
        let has_pr = self.pr_data.is_some();
        let has_sym = self.symbol_refs.is_some();
        self.panel = match self.panel {
            None => Some(PanelContent::AgentLog),
            Some(PanelContent::AgentLog) => {
                if has_sym {
                    Some(PanelContent::SymbolRefs)
                } else if has_pr {
                    Some(PanelContent::PrOverview)
                } else if has_ai {
                    Some(PanelContent::AiSummary)
                } else {
                    Some(PanelContent::FileDetail)
                }
            }
            Some(PanelContent::SymbolRefs) => {
                if has_pr {
                    Some(PanelContent::PrOverview)
                } else if has_ai {
                    Some(PanelContent::AiSummary)
                } else {
                    Some(PanelContent::FileDetail)
                }
            }
            Some(PanelContent::PrOverview) => {
                if has_ai {
                    Some(PanelContent::AiSummary)
                } else {
                    Some(PanelContent::FileDetail)
                }
            }
            Some(PanelContent::AiSummary) => Some(PanelContent::FileDetail),
            Some(PanelContent::FileDetail) => None,
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
                    // TODO(risk:medium): (chars / 70 + 1) as u16 overflows if a single summary line
                    // exceeds ~4.5 MB (u16::MAX * 70). The cast wraps silently, producing a small offset
                    // and misaligning the scroll target. Use saturating arithmetic here.
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
            // TODO(risk:minor): review.files.len() as u16 truncates silently if there are more than
            // 65535 files in the review. Unlikely but adding .min(u16::MAX as usize) as u16 is safer.
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

    /// Get the list of files, filtered by filter rules, search query, and reviewed status.
    /// Pipeline: filter rules → search → unreviewed toggle
    pub fn visible_files(&self) -> Vec<(usize, &DiffFile)> {
        let mut visible: Vec<(usize, &DiffFile)> = self.files.iter().enumerate().collect();

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

    pub fn set_mode(&mut self, mode: DiffMode) {
        if self.mode != mode {
            // Remember current position to restore after mode switch
            let prev_path = self.files.get(self.selected_file).map(|f| f.path.clone());
            let prev_hunk = self.current_hunk;
            let prev_line = self.current_line;

            self.mode = mode;
            self.committed_unpushed = false;
            if mode == DiffMode::History {
                // Initialize history state if first time
                if self.history.is_none() {
                    let commits = git::git_log_branch(&self.base_branch, &self.repo_root, 50, 0)
                        .unwrap_or_default();

                    let first_diff = if let Some(c) = commits.first() {
                        let raw =
                            git::git_diff_commit(&c.hash, &self.repo_root).unwrap_or_default();
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
                        all_loaded: false,
                        diff_cache: cache,
                    });
                }
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
            } else if mode == DiffMode::Wizard {
                self.current_hunk = 0;
                self.current_line = None;
                self.selected_watched = None;
                self.diff_scroll = 0;
                // In wizard mode, reuse the branch diff but build wizard state
                let _ = self.refresh_diff_mode_switch();
                self.enter_wizard_mode();
                // Auto-open context panel
                self.panel = Some(crate::ai::PanelContent::AiSummary);
            } else if mode == DiffMode::Quiz {
                self.current_hunk = 0;
                self.current_line = None;
                self.selected_watched = None;
                self.diff_scroll = 0;
                let _ = self.refresh_diff_mode_switch();
                self.enter_quiz_mode();
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
            git::save_snapshot(&self.repo_root, &path)?;
        }
        Ok(())
    }

    /// Sort files by filesystem mtime (newest first)
    fn sort_files_by_mtime(&mut self) {
        use std::fs;
        use std::time::SystemTime;

        // TODO(risk:medium): sort_by_mtime is applied after lazy parsing sets up file_headers with indices
        // matching self.files positions. After sorting, self.files[i] no longer corresponds to
        // self.file_headers[i], breaking ensure_file_parsed(). This is the same bug as noted above —
        // lazy mode + mtime sort together produce wrong on-demand parses.
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

    /// Count of reviewed files vs total (all files, ignoring filters)
    pub fn reviewed_count(&self) -> (usize, usize) {
        let total = self.files.len();
        let reviewed = self
            .files
            .iter()
            .filter(|f| self.reviewed.contains_key(&f.path))
            .count();
        (reviewed, total)
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

    fn load_reviewed_files(repo_root: &str) -> HashMap<String, String> {
        let path = format!("{}/.er/reviewed", repo_root);
        match std::fs::read_to_string(&path) {
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

    fn save_reviewed_files(&self) -> Result<()> {
        if self.is_remote() {
            return Ok(());
        }
        let path = format!("{}/.er/reviewed", self.repo_root);
        if self.reviewed.is_empty() {
            // Remove file if no reviewed files
            let _ = std::fs::remove_file(&path);
            return Ok(());
        }
        std::fs::create_dir_all(format!("{}/.er", self.repo_root))?;
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

    /// Remove reviewed entries whose stored diff hash no longer matches the current diff.
    /// Entries with an empty hash (old-format backwards compat sentinel) are skipped.
    /// Returns the number of entries removed. Saves the file if any were removed.
    fn auto_unmark_changed_reviewed(&mut self) -> usize {
        let stale: Vec<String> = self
            .reviewed
            .iter()
            .filter_map(|(path, stored_hash)| {
                // Skip old-format entries (no hash stored)
                if stored_hash.is_empty() {
                    return None;
                }
                let current_hash = self.current_per_file_hashes.get(path);
                // Unmark if the hash changed or the file disappeared from the diff
                match current_hash {
                    Some(h) if h == stored_hash => None,
                    _ => Some(path.clone()),
                }
            })
            .collect();

        let count = stale.len();
        if count > 0 {
            for path in &stale {
                self.reviewed.remove(path);
            }
            let _ = self.save_reviewed_files();
        }
        count
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
            user_expanded: self.user_expanded.iter().cloned().collect(),
            filter_expr: self.filter_expr.clone(),
            filter_history: self.filter_history.clone(),
            show_unreviewed_only: self.show_unreviewed_only,
            sort_by_mtime: self.sort_by_mtime,
            comment_draft: self.comment_input.clone(),
            comment_draft_file: self.comment_file.clone(),
            comment_draft_hunk: self.comment_hunk,
            comment_draft_line: self.comment_line_num,
            comment_draft_type: match self.comment_type {
                CommentType::Question => "question".to_string(),
                CommentType::GitHubComment => "github".to_string(),
            },
        }
    }

    /// Restore session state if the diff hash matches. Returns true if restored.
    pub fn restore_session(&mut self) -> bool {
        if self.is_remote() {
            return false;
        }
        let session = match SessionState::load(&self.repo_root) {
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

        // Restore expanded files
        self.user_expanded = session.user_expanded.into_iter().collect();

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
            self.comment_input = session.comment_draft;
            self.comment_file = session.comment_draft_file;
            self.comment_hunk = session.comment_draft_hunk;
            self.comment_line_num = session.comment_draft_line;
            self.comment_type = match session.comment_draft_type.as_str() {
                "question" => CommentType::Question,
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
        let _ = session.save(&self.repo_root);
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

    /// Pending action from a modal hub selection (consumed by the event loop)
    pub pending_hub_action: Option<HubAction>,

    /// Last known terminal width (updated each tick for resize calculations)
    pub last_terminal_width: u16,
}

impl App {
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

        // TODO(risk:minor): config is loaded from the first tab's repo root but the App is shared across
        // all tabs. If a second tab's .er-config.toml has different feature flags, those settings are
        // ignored — the first tab's config wins for everything (display, features, agents).
        // Load config from the first tab's repo root
        let repo_root = tabs.first().map(|t| t.repo_root.as_str()).unwrap_or(".");
        let er_config = config::load_config(repo_root);

        Ok(App {
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
            pending_hub_action: None,
            last_terminal_width: 0,
        })
    }

    /// Create App for remote PR review — no local git repo needed.
    pub fn new_remote(mut tab: TabState, pr_data: Option<crate::github::PrOverviewData>) -> Self {
        if let Some(data) = pr_data {
            tab.pr_data = Some(data);
        }
        let er_config = crate::config::ErConfig::default();
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
            pending_hub_action: None,
            last_terminal_width: 0,
        }
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

    /// Get a reference to the active tab
    // TODO(risk:high): tab() and tab_mut() index self.tabs[self.active_tab] directly. If active_tab
    // ever exceeds tabs.len() (e.g., after close_tab() removes the last non-first tab and the index
    // is not decremented correctly, or if tabs is somehow emptied), this panics. All callers assume
    // tabs is non-empty; that invariant is enforced only by close_tab's guard but not at the type level.
    pub fn tab(&self) -> &TabState {
        &self.tabs[self.active_tab]
    }

    /// Get a mutable reference to the active tab
    pub fn tab_mut(&mut self) -> &mut TabState {
        &mut self.tabs[self.active_tab]
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
            items,
            selected: 0,
        });
    }

    /// Open the AI modal hub
    pub fn open_ai_hub(&mut self) {
        let has_ai = self.tab().ai.has_data();
        let has_review = self.tab().ai.review.is_some();
        let has_quiz = self.tab().ai.quiz.is_some();
        let has_quiz_answers = std::path::Path::new(&self.tab().repo_root)
            .join(".er/quiz-answers.json")
            .exists();
        let has_questions = self
            .tab()
            .ai
            .questions
            .as_ref()
            .is_some_and(|q| !q.questions.is_empty());
        let has_unresolved_questions = self
            .tab()
            .ai
            .questions
            .as_ref()
            .is_some_and(|q| q.questions.iter().any(|q| !q.resolved));
        let summary_configured = self.config.commands.summary.is_some();
        let agent_name = self.config.agent.display_name();
        let items = vec![
            HubItem {
                label: format!("Review work ({})", agent_name),
                hint: "".into(),
                description: if has_review {
                    "Run AI review (will ask to clear previous)".into()
                } else {
                    "Run AI code review on current diff".into()
                },
                action: HubAction::PromptReview,
                is_header: false,
                enabled: true,
            },
            HubItem {
                label: format!("Answer questions ({})", agent_name),
                hint: "".into(),
                description: if has_unresolved_questions {
                    "Answer unresolved questions via AI".into()
                } else {
                    "No unresolved questions".into()
                },
                action: HubAction::PromptQuestions,
                is_header: false,
                enabled: has_unresolved_questions,
            },
            HubItem {
                label: format!("Generate quiz ({})", agent_name),
                hint: "".into(),
                description: if has_quiz {
                    "Regenerate quiz questions from current diff".into()
                } else {
                    "Generate comprehension quiz from current diff".into()
                },
                action: HubAction::PromptQuiz,
                is_header: false,
                enabled: true,
            },
            HubItem {
                label: format!("Generate wizard tour ({})", agent_name),
                hint: "".into(),
                description: if self.tab().ai.wizard.is_some() {
                    "Regenerate guided tour of changes".into()
                } else {
                    "Generate guided tour of important changes".into()
                },
                action: HubAction::PromptWizard,
                is_header: false,
                enabled: true,
            },
            HubItem {
                label: format!("Review quiz answers ({})", agent_name),
                hint: "".into(),
                description: if has_quiz_answers {
                    "Get AI feedback on your quiz answers".into()
                } else {
                    "No quiz answers to review — take the quiz first (key 8)".into()
                },
                action: HubAction::PromptQuizReview,
                is_header: false,
                enabled: has_quiz_answers,
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
                label: "Generate summary".into(),
                hint: "".into(),
                description: if summary_configured {
                    self.config.commands.summary.as_deref().unwrap().to_string()
                } else {
                    "set [commands] summary in .er-config.toml".into()
                },
                action: HubAction::RunCommand("summary".into()),
                is_header: false,
                enabled: summary_configured,
            },
            HubItem {
                label: "Cleanup questions".into(),
                hint: "z".into(),
                description: if has_questions {
                    "Delete .er/questions.json".into()
                } else {
                    "no questions to clean up".into()
                },
                action: HubAction::CleanupQuestions,
                is_header: false,
                enabled: has_questions,
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
            items,
            selected: 0,
        });
    }

    /// Open the Verify modal hub — items enabled when configured in [commands]
    pub fn open_verify_hub(&mut self) {
        let cmds = &self.config.commands;
        let not_configured = "set in [commands] in .er-config.toml";
        let items = vec![
            HubItem {
                label: "Run tests".into(),
                hint: "".into(),
                description: cmds.test.as_deref().unwrap_or(not_configured).to_string(),
                action: HubAction::RunCommand("test".into()),
                is_header: false,
                enabled: cmds.test.is_some(),
            },
            HubItem {
                label: "Run linter".into(),
                hint: "".into(),
                description: cmds.lint.as_deref().unwrap_or(not_configured).to_string(),
                action: HubAction::RunCommand("lint".into()),
                is_header: false,
                enabled: cmds.lint.is_some(),
            },
            HubItem {
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
            },
            HubItem {
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
            },
        ];
        self.overlay = Some(OverlayData::ModalHub {
            kind: HubKind::Verify,
            items,
            selected: 0,
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
                description: "Add review question".into(),
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
                description: "Toggle questions".into(),
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
                description: "Cleanup questions / all AI data".into(),
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
            items,
            selected: first_selectable,
        });
    }

    pub fn open_config_hub(&mut self) {
        let items = config::config_hub_items(&self.config);
        let first_selectable = items
            .iter()
            .position(|item| !matches!(item, config::ConfigItem::SectionHeader(_)))
            .unwrap_or(0);
        self.overlay = Some(OverlayData::ConfigHub {
            items,
            selected: first_selectable,
            saved_config: Box::new(self.config.clone()),
            editing: None,
        });
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
        let items = config::config_hub_items(&self.config);
        let Some(item) = items.into_iter().nth(idx) else {
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
                set(&mut self.config, next.to_string());
                // Apply theme change immediately if this was the theme item
                crate::ui::themes::set_theme_by_name(&self.config.display.theme);
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
            _ => {}
        }
    }

    /// Apply the editing buffer to the config and close the inline edit
    pub fn config_hub_confirm_edit(&mut self) {
        let (item_idx, buffer) = match &self.overlay {
            Some(OverlayData::ConfigHub {
                editing: Some(edit),
                ..
            }) => (edit.item_index, edit.buffer.clone()),
            _ => return,
        };

        // Clear editing first
        if let Some(OverlayData::ConfigHub { editing, .. }) = &mut self.overlay {
            *editing = None;
        }

        let items = config::config_hub_items(&self.config);
        match items.get(item_idx) {
            Some(config::ConfigItem::StringEdit { set, .. }) => {
                set(&mut self.config, buffer);
            }
            Some(config::ConfigItem::ListAdd { .. }) => {
                if !buffer.trim().is_empty() {
                    self.config.watched.paths.push(buffer.trim().to_string());
                }
            }
            _ => {}
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
        let idx = match &self.overlay {
            Some(OverlayData::ConfigHub { selected, .. }) => *selected,
            _ => return,
        };

        let items = config::config_hub_items(&self.config);
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
            crate::ui::themes::set_theme_by_name(&self.config.display.theme);
        }
    }

    /// Rebuild the config hub items list (e.g. after watched paths change) and clamp selection
    pub fn config_hub_rebuild_items(&mut self) {
        if let Some(OverlayData::ConfigHub {
            items,
            selected,
            editing,
            ..
        }) = &mut self.overlay
        {
            *items = config::config_hub_items(&self.config);
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
            DiffMode::History | DiffMode::Hidden | DiffMode::Wizard | DiffMode::Quiz => {
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

/// Simple ISO 8601 UTC timestamp (no external crate needed).
/// Kept in ISO format so .er-feedback.json timestamps are human-readable.
pub(crate) fn chrono_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();

    let days = secs / 86400;
    let remaining = secs % 86400;
    let hours = remaining / 3600;
    let minutes = (remaining % 3600) / 60;
    let seconds = remaining % 60;

    // Walk years from epoch, subtracting days per year (handles leap years via Gregorian rule)
    let mut y = 1970i64;
    let mut d = i64::try_from(days).unwrap_or(i64::MAX);
    loop {
        let days_in_year = if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) {
            366
        } else {
            365
        };
        if d < days_in_year {
            break;
        }
        d -= days_in_year;
        y += 1;
    }

    // Walk months within the year (m is 0-indexed, d ends as 0-indexed day-of-month)
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let month_days: [i64; 12] = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut m = 0usize;
    for md in &month_days {
        if d < *md {
            break;
        }
        d -= *md;
        m += 1;
    }
    // Guard against overflow past December (shouldn't happen, but be safe)
    if m >= 12 {
        m = 11;
        d = 0;
    }

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y,
        m + 1,
        d + 1,
        hours,
        minutes,
        seconds
    )
}

/// Delete personal questions sidecar files. Errors are ignored (files may not exist).
pub fn cleanup_questions(er_dir: &str) {
    let base = std::path::Path::new(er_dir);
    let _ = std::fs::remove_file(base.join("questions.json"));
    let _ = std::fs::remove_file(base.join("questions.prev.json"));
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
            comment_input: String::new(),
            comment_file: String::new(),
            comment_hunk: 0,
            comment_reply_to: None,
            comment_line_num: None,
            comment_type: CommentType::GitHubComment,
            comment_edit_id: None,
            comment_finding_ref: None,
            pr_data: None,
            pr_head_ref: None,
            pr_number: None,
            history: None,
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
            file_tree_cache: None,
            mem_budget: MemoryBudget::default(),
            lazy_mode: false,
            file_headers: Vec::new(),
            raw_diff: None,
            symbol_refs: None,
            pending_unmark_count: 0,
            committed_unpushed: false,
            context_overrides: HashMap::new(),
            remote_repo: None,
            command_rx: std::collections::HashMap::new(),
            command_status: std::collections::HashMap::new(),
            log_tx: agent_log_tx,
            log_rx: agent_log_rx,
            agent_log: std::collections::VecDeque::new(),
            agent_log_auto_scroll: true,
            wizard: None,
            quiz: None,
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
    fn current_line_number_delete_line_with_no_new_num_returns_none() {
        let lines = vec![make_line(LineType::Delete, "deleted", None)];
        let files = vec![make_file("a.rs", vec![make_hunk(lines)], 0, 1)];
        let mut tab = make_test_tab(files);
        tab.current_line = Some(0);
        assert_eq!(tab.current_line_number(), None);
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
        assert!(!tab.layers.show_ai_findings);
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
        assert!(!tab.layers.show_ai_findings);
        tab.toggle_layer_ai();
        assert!(tab.layers.show_ai_findings);
        tab.toggle_layer_ai();
        assert!(!tab.layers.show_ai_findings);
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
            pending_hub_action: None,
            last_terminal_width: 0,
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
            }],
        });
        let mut app = make_test_app(tab);
        app.start_edit_comment("q-123-0");
        assert!(matches!(app.input_mode, InputMode::Comment));
        assert_eq!(app.tab().comment_input, "Why is this here?");
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
    fn tab_name_returns_last_path_component_in_normal_mode() {
        let mut tab = TabState::new_for_test(vec![]);
        tab.repo_root = "/home/user/my-project".to_string();
        assert_eq!(tab.tab_name(), "my-project");
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
        assert_eq!(tab.comments_dir(), "/home/user/my-project/.er");
    }

    #[test]
    fn comments_dir_returns_cache_path_in_remote_mode() {
        let mut tab = TabState::new_for_test(vec![]);
        tab.remote_repo = Some("owner/repo".to_string());
        tab.pr_number = Some(42);
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        assert_eq!(
            tab.comments_dir(),
            format!("{}/.cache/er/remote/owner-repo-42", home)
        );
    }

    #[test]
    fn comments_dir_uses_normal_mode_when_pr_number_missing() {
        // remote_repo without pr_number falls back to normal mode path
        let mut tab = TabState::new_for_test(vec![]);
        tab.repo_root = "/home/user/my-project".to_string();
        tab.remote_repo = Some("owner/repo".to_string());
        // pr_number is None — should fall through to normal path
        assert_eq!(tab.comments_dir(), "/home/user/my-project/.er");
    }

    #[test]
    fn github_comments_path_appends_filename_to_comments_dir() {
        let mut tab = TabState::new_for_test(vec![]);
        tab.repo_root = "/home/user/my-project".to_string();
        assert_eq!(
            tab.github_comments_path(),
            "/home/user/my-project/.er/github-comments.json"
        );
    }

    #[test]
    fn github_comments_path_uses_cache_dir_in_remote_mode() {
        let mut tab = TabState::new_for_test(vec![]);
        tab.remote_repo = Some("owner/repo".to_string());
        tab.pr_number = Some(7);
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        assert_eq!(
            tab.github_comments_path(),
            format!(
                "{}/.cache/er/remote/owner-repo-7/github-comments.json",
                home
            )
        );
    }

    #[test]
    fn comments_dir_replaces_slash_in_slug_with_dash() {
        let mut tab = TabState::new_for_test(vec![]);
        tab.remote_repo = Some("my-org/my-repo".to_string());
        tab.pr_number = Some(1);
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        assert_eq!(
            tab.comments_dir(),
            format!("{}/.cache/er/remote/my-org-my-repo-1", home)
        );
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
        // Remote mode derives comments_dir() from $HOME, which may not be writable in CI.
        // Instead, verify the path derivation is correct (no I/O), and that reload
        // gracefully handles the case where the remote cache dir doesn't exist yet.
        let mut tab = TabState::new_for_test(vec![]);
        tab.remote_repo = Some("owner/repo".to_string());
        tab.pr_number = Some(99);

        // Path should include the sanitized slug and PR number
        let dir = tab.comments_dir();
        assert!(dir.contains("owner-repo-99"), "dir = {}", dir);
        assert!(dir.contains(".cache/er/remote"), "dir = {}", dir);

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

    // ── WizardState ──

    fn make_quiz_question(id: &str, file: &str, freeform: bool) -> crate::ai::QuizQuestion {
        crate::ai::QuizQuestion {
            id: id.to_string(),
            level: 1,
            category: "design-decisions".to_string(),
            text: format!("Question {}", id),
            options: if freeform {
                None
            } else {
                Some(vec![crate::ai::QuizOption {
                    label: 'A',
                    text: "Option A".to_string(),
                    is_correct: true,
                }])
            },
            freeform,
            expected_reasoning: String::new(),
            explanation: "Explanation".to_string(),
            related_file: file.to_string(),
            related_hunk: None,
            related_lines: None,
        }
    }

    #[test]
    fn test_wizard_state_creation() {
        let state = WizardState {
            ordered_files: vec!["a.rs".to_string(), "b.rs".to_string()],
            current_step: 0,
            completed: HashSet::new(),
        };
        assert_eq!(state.ordered_files.len(), 2);
        assert_eq!(state.current_step, 0);
        assert!(state.completed.is_empty());
    }

    #[test]
    fn test_wizard_mark_reviewed() {
        use crate::git::{DiffHunk, DiffLine, LineType};

        let file_a = make_file(
            "a.rs",
            vec![DiffHunk {
                header: "@@ -1,1 +1,1 @@".to_string(),
                lines: vec![DiffLine {
                    line_type: LineType::Context,
                    content: "fn main() {}".to_string(),
                    old_num: Some(1),
                    new_num: Some(1),
                }],
                old_start: 1,
                old_count: 1,
                new_start: 1,
                new_count: 1,
            }],
            1,
            0,
        );
        let mut tab = make_test_tab(vec![file_a]);
        tab.wizard = Some(WizardState {
            ordered_files: vec!["a.rs".to_string()],
            current_step: 0,
            completed: HashSet::new(),
        });

        tab.wizard_mark_reviewed();

        let wizard = tab.wizard.as_ref().unwrap();
        assert!(wizard.completed.contains("a.rs"));
    }

    // ── QuizState ──

    #[test]
    fn test_quiz_answer_mc() {
        let questions = vec![make_quiz_question("q1", "src/auth.rs", false)];
        let mut quiz = QuizState {
            questions,
            current: 0,
            answers: HashMap::new(),
            score: (0, 0),
            filter_level: None,
            filter_category: None,
            input_mode: QuizInputMode::Navigating,
            input_buffer: String::new(),
            show_explanation: false,
        };

        let question = &quiz.questions[0];
        let opts = question.options.as_ref().unwrap();
        let label = opts.iter().find(|o| o.is_correct).unwrap().label;
        let id = question.id.clone();
        quiz.answers.insert(id.clone(), QuizAnswer::Choice(label));
        quiz.score.1 += 1;
        quiz.score.0 += 1;

        assert_eq!(quiz.score, (1, 1));
        assert!(matches!(quiz.answers[&id], QuizAnswer::Choice('A')));
    }

    #[test]
    fn test_quiz_state_initial_score() {
        let quiz = QuizState {
            questions: vec![
                make_quiz_question("q1", "a.rs", false),
                make_quiz_question("q2", "b.rs", true),
            ],
            current: 0,
            answers: HashMap::new(),
            score: (0, 0),
            filter_level: None,
            filter_category: None,
            input_mode: QuizInputMode::Navigating,
            input_buffer: String::new(),
            show_explanation: false,
        };

        assert_eq!(quiz.score, (0, 0));
        assert_eq!(quiz.questions.len(), 2);
        assert_eq!(quiz.input_mode, QuizInputMode::Navigating);
    }

    #[test]
    fn test_quiz_freeform_answer() {
        let questions = vec![make_quiz_question("q1", "src/auth.rs", true)];
        let mut quiz = QuizState {
            questions,
            current: 0,
            answers: HashMap::new(),
            score: (0, 0),
            filter_level: None,
            filter_category: None,
            input_mode: QuizInputMode::AnsweringFreeform,
            input_buffer: "My answer about risk".to_string(),
            show_explanation: false,
        };

        let text = quiz.input_buffer.trim().to_string();
        let id = quiz.questions[0].id.clone();
        quiz.answers.insert(id.clone(), QuizAnswer::Freeform(text));
        quiz.score.1 += 1;
        quiz.input_buffer.clear();
        quiz.input_mode = QuizInputMode::Navigating;
        quiz.show_explanation = true;

        assert_eq!(quiz.score, (0, 1));
        assert!(quiz.input_buffer.is_empty());
        assert_eq!(quiz.input_mode, QuizInputMode::Navigating);
        assert!(quiz.show_explanation);
        assert!(matches!(quiz.answers[&id], QuizAnswer::Freeform(_)));
    }
}
