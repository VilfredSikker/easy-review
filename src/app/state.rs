use crate::ai::{self, AiState, CommentType, InlineLayers, PanelContent, ReviewFocus};
use crate::config::{self, ErConfig, WatchedConfig};
use crate::git::{self, DiffFile, DiffFileHeader, CompactionConfig, WatchedFile, Worktree};
use anyhow::{Context, Result};
use std::collections::HashSet;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
#[allow(unused_imports)]
use std::time::Instant;

static COMMENT_SEQ: AtomicU64 = AtomicU64::new(0);

// ── Enums ──

/// Which set of changes we're viewing
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DiffMode {
    Branch,
    Unstaged,
    Staged,
}

impl DiffMode {
    #[allow(dead_code)]
    pub fn label(&self) -> &'static str {
        match self {
            DiffMode::Branch => "BRANCH DIFF",
            DiffMode::Unstaged => "UNSTAGED",
            DiffMode::Staged => "STAGED",
        }
    }

    pub fn git_mode(&self) -> &'static str {
        match self {
            DiffMode::Branch => "branch",
            DiffMode::Unstaged => "unstaged",
            DiffMode::Staged => "staged",
        }
    }
}

/// Whether we're navigating or typing in the search filter / comment
#[derive(Debug, Clone, PartialEq)]
pub enum InputMode {
    Normal,
    Search,
    Comment,
    Confirm(ConfirmAction),
    Filter,
    Commit,
}

/// Actions that require user confirmation (y/n)
#[derive(Debug, Clone, PartialEq)]
pub enum ConfirmAction {
    DeleteComment { comment_id: String },
}

/// Tracks which comment is focused for reply/delete operations
#[derive(Debug, Clone)]
pub struct CommentFocus {
    pub file: String,
    pub hunk_index: Option<usize>,
    pub comment_id: String,
}

// ── Overlay types ──

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
    Settings {
        selected: usize,
        /// Snapshot of config at overlay open time, for Cancel revert
        saved_config: ErConfig,
    },
    FilterHistory {
        history: Vec<String>,
        selected: usize,
        preset_count: usize,
    },
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

    /// Vertical scroll offset for the AI side panel (independent of diff_scroll)
    pub ai_panel_scroll: u16,

    /// Inline layer visibility toggles
    pub layers: InlineLayers,

    /// Optional context panel content (None = panel closed)
    pub panel: Option<PanelContent>,

    /// Vertical scroll offset for the context panel
    pub panel_scroll: u16,

    /// Whether keyboard focus is on the panel (vs diff view)
    pub panel_focus: bool,

    /// Which column has focus in panel's AiSummary view
    pub review_focus: ReviewFocus,

    /// Cursor position within the focused panel section
    pub review_cursor: usize,

    /// Search/filter input
    pub search_query: String,

    /// Files marked as reviewed (paths relative to repo root)
    pub reviewed: HashSet<String>,

    /// Only show unreviewed files in the file tree
    pub show_unreviewed_only: bool,

    /// Sort files by mtime (newest first) — works in any diff mode
    pub sort_by_mtime: bool,

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

    /// Currently focused comment (for reply/delete operations)
    pub comment_focus: Option<CommentFocus>,

    /// Which type of comment is being created (Question vs GitHubComment)
    pub comment_type: CommentType,

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

    // ── Commit input state ──

    /// Text buffer for the commit message being typed
    pub commit_input: String,

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

    fn new_inner(repo_root: String, current_branch: String, base_branch: String) -> Result<Self> {
        let reviewed = Self::load_reviewed_files(&repo_root);
        let er_config = config::load_config(&repo_root);
        let watched_config = er_config.watched.clone();
        let has_watched = !watched_config.paths.is_empty();

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
            ai_panel_scroll: 0,
            layers: InlineLayers::default(),
            panel: None,
            panel_scroll: 0,
            panel_focus: false,
            review_focus: ReviewFocus::Files,
            review_cursor: 0,
            search_query: String::new(),
            filter_expr: String::new(),
            filter_rules: Vec::new(),
            filter_input: String::new(),
            filter_history: Vec::new(),
            reviewed,
            show_unreviewed_only: false,
            sort_by_mtime: false,
            ai: AiState::default(),
            diff_hash: String::new(),
            branch_diff_hash: String::new(),
            last_ai_check: None,
            comment_input: String::new(),
            comment_file: String::new(),
            comment_hunk: 0,
            comment_reply_to: None,
            comment_line_num: None,
            comment_focus: None,
            comment_type: CommentType::GitHubComment,
            watched_config,
            watched_files: Vec::new(),
            selected_watched: None,
            show_watched: has_watched,
            watched_not_ignored: Vec::new(),
            commit_input: String::new(),
            compaction_config: CompactionConfig::default(),
            hunk_offsets: None,
            file_tree_cache: None,
            mem_budget: MemoryBudget::default(),
            lazy_mode: false,
            file_headers: Vec::new(),
            raw_diff: None,
        };

        tab.refresh_diff()?;
        tab.refresh_watched_files();
        Ok(tab)
    }

    /// Short name for display in tab bar (last path component)
    pub fn tab_name(&self) -> String {
        std::path::Path::new(&self.repo_root)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| self.repo_root.clone())
    }

    // ── Diff ──

    /// Re-run git diff and update the file list
    pub fn refresh_diff(&mut self) -> Result<()> {
        self.refresh_diff_impl(true)
    }

    /// Lightweight refresh: skips branch hash recomputation in non-Branch modes.
    /// Use for watch events where the extra git diff call adds unwanted latency.
    pub fn refresh_diff_quick(&mut self) -> Result<()> {
        self.refresh_diff_impl(false)
    }

    fn refresh_diff_impl(&mut self, recompute_branch_hash: bool) -> Result<()> {
        // Remember current position to restore after re-parse
        let prev_path = self.files.get(self.selected_file).map(|f| f.path.clone());
        let prev_hunk = self.current_hunk;
        let prev_line = self.current_line;

        let raw = git::git_diff_raw(self.mode.git_mode(), &self.base_branch, &self.repo_root)?;

        // Decide parsing strategy based on diff size
        let line_count = raw.as_bytes().iter().filter(|&&b| b == b'\n').count();
        if line_count > git::LAZY_PARSE_THRESHOLD {
            // Lazy mode: header-only parse, files get hunks on demand
            let headers = git::parse_diff_headers(&raw);
            self.files = headers.iter().map(git::header_to_stub).collect();
            self.file_headers = headers;
            self.raw_diff = Some(raw.clone());
            self.lazy_mode = true;

            // Apply compaction to the stub files (pattern-based only, since hunks are empty)
            for (file, header) in self.files.iter_mut().zip(self.file_headers.iter()) {
                let total_lines = header.adds + header.dels;
                let should_compact = self.compaction_config.enabled
                    && (self.compaction_config.patterns.iter().any(|p| git::compact_files_match(p, &file.path))
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
        }

        if self.sort_by_mtime {
            self.sort_files_by_mtime();
        }

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
        // In other modes, only run the extra git diff on explicit refresh.
        // This guarantees at most 2 git diff calls (down from 3).
        if self.mode == DiffMode::Branch {
            self.branch_diff_hash = self.diff_hash.clone();
            branch_raw_owned = Some(raw.clone());
        } else if recompute_branch_hash {
            let br = git::git_diff_raw("branch", &self.base_branch, &self.repo_root)?;
            self.branch_diff_hash = ai::compute_diff_hash(&br);
            branch_raw_owned = Some(br);
        } else {
            branch_raw_owned = None;
        }

        // Load AI state from .er-* files
        self.reload_ai_state();

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
                // Restore hunk/line if the file still has enough hunks
                if prev_hunk < self.total_hunks() {
                    self.current_hunk = prev_hunk;
                    self.current_line = prev_line;
                } else {
                    self.clamp_hunk();
                }
            } else {
                // File disappeared from diff — clamp index
                if self.selected_file >= self.files.len() {
                    self.selected_file = self.files.len().saturating_sub(1);
                }
                self.clamp_hunk();
            }
        } else {
            self.selected_file = 0;
            self.clamp_hunk();
        }
        self.scroll_to_current_hunk();

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
        self.ai = ai::load_ai_state(&self.repo_root, &self.branch_diff_hash);
        // Preserve per-file staleness across .er-* file reloads (recomputed in refresh_diff)
        if self.ai.is_stale {
            self.ai.stale_files = prev_stale_files;
        }
        // Clamp cursor to valid range after reload (item count may have decreased)
        let item_count = match self.review_focus {
            ReviewFocus::Files => self.ai.review_file_count(),
            ReviewFocus::Checklist => self.ai.review_checklist_count(),
        };
        let max_cursor = if item_count == 0 { 0 } else { item_count - 1 };
        self.review_cursor = self.review_cursor.min(max_cursor);
        self.last_ai_check = ai::latest_er_mtime(&self.repo_root);
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
        let latest_mtime = match ai::latest_er_mtime(&self.repo_root) {
            Some(t) => t,
            None => return false,
        };
        let should_reload = match self.last_ai_check {
            Some(last_check) => latest_mtime > last_check,
            None => true,
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

    /// Cycle panel: None → FileDetail → AiSummary (if AI data) → None
    pub fn toggle_panel(&mut self) {
        self.panel = match self.panel {
            None => Some(PanelContent::FileDetail),
            Some(PanelContent::FileDetail) => {
                if self.ai.has_data() {
                    Some(PanelContent::AiSummary)
                } else {
                    None
                }
            }
            Some(PanelContent::AiSummary) => None,
            Some(PanelContent::PrOverview) => None,
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

    /// Get the list of files, filtered by filter rules, search query, and reviewed status.
    /// Pipeline: filter rules → search → unreviewed toggle
    pub fn visible_files(&self) -> Vec<(usize, &DiffFile)> {
        let mut visible: Vec<(usize, &DiffFile)> = self.files.iter().enumerate().collect();

        // Phase 1: Apply filter rules
        if !self.filter_rules.is_empty() {
            visible.retain(|(_, f)| super::filter::apply_filter(&self.filter_rules, f));
        }

        // Phase 2: Apply search query
        if !self.search_query.is_empty() {
            let q = self.search_query.to_lowercase();
            visible.retain(|(_, f)| f.path.to_lowercase().contains(&q));
        }

        // Phase 3: Apply unreviewed-only toggle
        if self.show_unreviewed_only {
            visible.retain(|(_, f)| !self.reviewed.contains(&f.path));
        }

        visible
    }

    /// Get the list of watched files, filtered by search query
    pub fn visible_watched_files(&self) -> Vec<(usize, &WatchedFile)> {
        if !self.show_watched {
            return Vec::new();
        }
        if self.search_query.is_empty() {
            self.watched_files.iter().enumerate().collect()
        } else {
            let q = self.search_query.to_lowercase();
            self.watched_files
                .iter()
                .enumerate()
                .filter(|(_, f)| f.path.to_lowercase().contains(&q))
                .collect()
        }
    }

    /// Get the currently selected file
    pub fn selected_diff_file(&self) -> Option<&DiffFile> {
        if self.selected_watched.is_some() {
            return None;
        }
        self.files.get(self.selected_file)
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
            }
        }
    }

    pub fn next_file(&mut self) {
        if let Some(idx) = self.selected_watched {
            // In watched section — move down within watched files
            let visible_watched = self.visible_watched_files();
            if let Some(pos) = visible_watched.iter().position(|(i, _)| *i == idx) {
                if pos + 1 < visible_watched.len() {
                    self.selected_watched = Some(visible_watched[pos + 1].0);
                    self.diff_scroll = 0;
                    self.h_scroll = 0;
                }
            }
        } else {
            // In diff section
            let visible = self.visible_files();
            if visible.is_empty() {
                // No diff files — jump to watched if available
                let visible_watched = self.visible_watched_files();
                if !visible_watched.is_empty() {
                    self.selected_watched = Some(visible_watched[0].0);
                    self.diff_scroll = 0;
                    self.h_scroll = 0;
                }
                return;
            }
            if let Some(pos) = visible.iter().position(|(i, _)| *i == self.selected_file) {
                if pos + 1 < visible.len() {
                    self.selected_file = visible[pos + 1].0;
                    self.current_hunk = 0;
                    self.current_line = None;
                    self.selection_anchor = None;
                    self.diff_scroll = 0;
                    self.h_scroll = 0;
                    self.ai_panel_scroll = 0;
                } else {
                    // At last diff file — transition to watched section
                    let visible_watched = self.visible_watched_files();
                    if !visible_watched.is_empty() {
                        self.selected_watched = Some(visible_watched[0].0);
                        self.diff_scroll = 0;
                        self.h_scroll = 0;
                    }
                }
            } else {
                // Current selection not in visible set — snap to first
                self.selected_file = visible[0].0;
                self.current_hunk = 0;
                self.diff_scroll = 0;
                self.h_scroll = 0;
                self.ai_panel_scroll = 0;
                self.ensure_file_parsed();
                self.rebuild_hunk_offsets();
            }
        }
    }

    pub fn prev_file(&mut self) {
        if let Some(idx) = self.selected_watched {
            // In watched section — move up within watched files
            let visible_watched = self.visible_watched_files();
            if let Some(pos) = visible_watched.iter().position(|(i, _)| *i == idx) {
                if pos > 0 {
                    self.selected_watched = Some(visible_watched[pos - 1].0);
                    self.diff_scroll = 0;
                    self.h_scroll = 0;
                } else {
                    // At first watched file — transition back to diff section
                    self.selected_watched = None;
                    let visible = self.visible_files();
                    if !visible.is_empty() {
                        self.selected_file = visible.last().unwrap().0;
                        self.current_hunk = 0;
                        self.current_line = None;
                        self.selection_anchor = None;
                        self.diff_scroll = 0;
                        self.h_scroll = 0;
                        self.ai_panel_scroll = 0;
                    }
                }
            }
        } else {
            // In diff section — normal navigation
            let visible = self.visible_files();
            if visible.is_empty() {
                return;
            }
            if let Some(pos) = visible.iter().position(|(i, _)| *i == self.selected_file) {
                if pos > 0 {
                    self.selected_file = visible[pos - 1].0;
                    self.current_hunk = 0;
                    self.current_line = None;
                    self.selection_anchor = None;
                    self.diff_scroll = 0;
                    self.h_scroll = 0;
                    self.ai_panel_scroll = 0;
                }
            } else {
                // Current selection not in visible set — snap to first
                self.selected_file = visible[0].0;
                self.current_hunk = 0;
                self.diff_scroll = 0;
                self.h_scroll = 0;
                self.ai_panel_scroll = 0;
                self.ensure_file_parsed();
                self.rebuild_hunk_offsets();
            }
        }
    }

    pub fn next_hunk(&mut self) {
        let total = self.total_hunks();
        if total > 0 && self.current_hunk < total - 1 {
            self.current_hunk += 1;
            self.current_line = None;
            self.selection_anchor = None;
            self.scroll_to_current_hunk();
        }
    }

    pub fn prev_hunk(&mut self) {
        if self.current_hunk > 0 {
            self.current_hunk -= 1;
            self.current_line = None;
            self.selection_anchor = None;
            self.scroll_to_current_hunk();
        }
    }

    /// Move to the next line within the current hunk (arrow down)
    pub fn next_line(&mut self) {
        self.selection_anchor = None;
        let total_lines = self.current_hunk_line_count();
        if total_lines == 0 {
            return;
        }
        match self.current_line {
            None => {
                self.current_line = Some(0);
                self.scroll_to_current_hunk();
            }
            Some(line) => {
                if line + 1 < total_lines {
                    self.current_line = Some(line + 1);
                    self.scroll_to_current_hunk();
                } else {
                    let total_hunks = self.total_hunks();
                    if self.current_hunk + 1 < total_hunks {
                        self.current_hunk += 1;
                        self.current_line = Some(0);
                        self.scroll_to_current_hunk();
                    }
                }
            }
        }
    }

    /// Move to the previous line within the current hunk (arrow up)
    pub fn prev_line(&mut self) {
        self.selection_anchor = None;
        match self.current_line {
            None => {
                // Enter line mode at the last line of the current hunk
                let count = self.current_hunk_line_count();
                if count > 0 {
                    self.current_line = Some(count - 1);
                    self.scroll_to_current_hunk();
                }
            }
            Some(0) => {
                if self.current_hunk > 0 {
                    self.current_hunk -= 1;
                    let count = self.current_hunk_line_count();
                    self.current_line = if count > 0 { Some(count - 1) } else { None };
                    self.scroll_to_current_hunk();
                } else {
                    self.current_line = None;
                }
            }
            Some(line) => {
                self.current_line = Some(line - 1);
                self.scroll_to_current_hunk();
            }
        }
    }

    /// Get the number of lines in the current hunk
    pub fn current_hunk_line_count(&self) -> usize {
        self.selected_diff_file()
            .and_then(|f| f.hunks.get(self.current_hunk))
            .map(|h| h.lines.len())
            .unwrap_or(0)
    }

    /// Get the new-side line number for the currently selected line
    pub fn current_line_number(&self) -> Option<usize> {
        let file = self.selected_diff_file()?;
        let hunk = file.hunks.get(self.current_hunk)?;
        let line_idx = self.current_line?;
        let diff_line = hunk.lines.get(line_idx)?;
        diff_line.new_num
    }

    /// Get the selected line range within the current hunk (from shift+arrow selection)
    pub fn selected_range(&self) -> Option<std::ops::RangeInclusive<usize>> {
        let anchor = self.selection_anchor?;
        let current = self.current_line?;
        Some(anchor.min(current)..=anchor.max(current))
    }

    pub fn scroll_to_current_hunk(&mut self) {
        // Use precomputed hunk offsets if available (O(1) lookup)
        if let Some(ref offsets) = self.hunk_offsets {
            if let Some(&base) = offsets.offsets.get(self.current_hunk) {
                let line_offset = base + self.current_line.unwrap_or(0);
                self.diff_scroll = line_offset.saturating_sub(1).min(u16::MAX as usize) as u16;
                return;
            }
        }
        // Fallback: compute from hunks (for Overlay mode where offsets are approximate)
        if let Some(file) = self.selected_diff_file() {
            let mut line_offset: usize = 2;
            for (i, hunk) in file.hunks.iter().enumerate() {
                if i == self.current_hunk {
                    line_offset += self.current_line.unwrap_or(0);
                    self.diff_scroll = line_offset.saturating_sub(1).min(u16::MAX as usize) as u16;
                    return;
                }
                line_offset += 1 + hunk.lines.len() + 1;
            }
        }
    }

    pub fn scroll_down(&mut self, amount: u16) {
        self.diff_scroll = self.diff_scroll.saturating_add(amount);
        self.sync_cursor_to_scroll();
    }

    pub fn scroll_up(&mut self, amount: u16) {
        self.diff_scroll = self.diff_scroll.saturating_sub(amount);
        self.sync_cursor_to_scroll();
    }

    /// Move the cursor (current_hunk + current_line) to match the current
    /// diff_scroll position.  Uses the same layout model as the renderer:
    /// 2 header lines, then per hunk: 1 header + N content lines + 1 blank.
    fn sync_cursor_to_scroll(&mut self) {
        // Compute target (hunk, line) from the scroll offset without
        // holding a borrow across the mutation.
        let result = {
            let file = match self.selected_diff_file() {
                Some(f) => f,
                None => return,
            };
            if file.hunks.is_empty() {
                return;
            }

            let target = self.diff_scroll as usize;
            let mut offset: usize = 2; // file header + blank

            let mut found: Option<(usize, usize)> = None;
            for (i, hunk) in file.hunks.iter().enumerate() {
                offset += 1; // hunk header line
                let content_start = offset;
                let content_end = offset + hunk.lines.len();

                if target < content_end {
                    let line_idx = if target >= content_start {
                        target - content_start
                    } else {
                        0 // target is on/before hunk header — snap to first line
                    };
                    found = Some((i, line_idx));
                    break;
                }

                offset = content_end + 1; // blank line after hunk
            }

            found.unwrap_or_else(|| {
                // Past the end — clamp to last line of last hunk
                let last = file.hunks.len() - 1;
                (last, file.hunks[last].lines.len().saturating_sub(1))
            })
        };

        self.current_hunk = result.0;
        self.current_line = Some(result.1);
    }

    pub fn scroll_right(&mut self, amount: u16) {
        self.h_scroll = self.h_scroll.saturating_add(amount);
    }

    pub fn scroll_left(&mut self, amount: u16) {
        self.h_scroll = self.h_scroll.saturating_sub(amount);
    }

    // ── Performance helpers ──

    /// Rebuild hunk offsets for the currently selected file
    fn rebuild_hunk_offsets(&mut self) {
        self.hunk_offsets = self.selected_diff_file().map(|f| HunkOffsets::build(&f.hunks));
    }

    /// Update memory budget counters
    fn update_mem_budget(&mut self) {
        let mut total_lines = 0usize;
        let mut compacted = 0usize;
        let mut parsed = 0usize;
        for file in &self.files {
            if file.compacted {
                compacted += 1;
            } else {
                parsed += 1;
                total_lines += file.hunks.iter().map(|h| h.lines.len()).sum::<usize>();
            }
        }
        self.mem_budget = MemoryBudget {
            parsed_files: parsed,
            total_lines,
            compacted_files: compacted,
        };
    }

    /// In lazy mode, ensure the currently selected file has its hunks parsed.
    /// No-op in eager mode or if already parsed.
    pub fn ensure_file_parsed(&mut self) {
        if !self.lazy_mode {
            return;
        }
        if let Some(file) = self.files.get(self.selected_file) {
            // Already parsed (has hunks) or is compacted — skip
            if !file.hunks.is_empty() || file.compacted {
                return;
            }
        }
        // Parse on demand from raw diff
        if let (Some(ref raw), Some(header)) = (&self.raw_diff, self.file_headers.get(self.selected_file)) {
            let parsed = git::parse_file_at_offset(raw, header);
            if let Some(file) = self.files.get_mut(self.selected_file) {
                file.hunks = parsed.hunks;
                // Update adds/dels from actual parse
                file.adds = parsed.adds;
                file.dels = parsed.dels;
            }
            self.rebuild_hunk_offsets();
            self.update_mem_budget();
        }
    }

    /// Toggle expand/compact for the currently selected file.
    /// If compacted, expand by re-fetching from git.
    /// If expanded (and was compacted), re-compact it.
    pub fn toggle_compacted(&mut self) -> Result<()> {
        if let Some(file) = self.files.get_mut(self.selected_file) {
            if file.compacted {
                git::expand_compacted_file(
                    file,
                    &self.repo_root,
                    self.mode.git_mode(),
                    &self.base_branch,
                )?;
                self.rebuild_hunk_offsets();
                self.update_mem_budget();
            } else {
                // Re-compact: only if it matched a pattern or was large
                file.compacted = true;
                file.raw_hunk_count = file.hunks.len();
                file.hunks.clear();
                file.hunks.shrink_to_fit();
                self.current_hunk = 0;
                self.current_line = None;
                self.diff_scroll = 0;
                self.hunk_offsets = None;
                self.update_mem_budget();
            }
        }
        Ok(())
    }

    /// Get cached visible files, rebuilding cache if needed
    #[allow(dead_code)]
    pub fn visible_files_cached(&mut self) -> Vec<usize> {
        let reviewed_count = self.reviewed.len();
        let needs_rebuild = match &self.file_tree_cache {
            Some(cache) => {
                cache.search_query != self.search_query
                    || cache.show_unreviewed_only != self.show_unreviewed_only
                    || cache.file_count != self.files.len()
                    || cache.reviewed_count != reviewed_count
            }
            None => true,
        };

        if needs_rebuild {
            let visible = self.visible_files().iter().map(|(i, _)| *i).collect::<Vec<_>>();
            self.file_tree_cache = Some(FileTreeCache {
                visible: visible.clone(),
                search_query: self.search_query.clone(),
                show_unreviewed_only: self.show_unreviewed_only,
                file_count: self.files.len(),
                reviewed_count,
            });
            visible
        } else {
            self.file_tree_cache.as_ref().unwrap().visible.clone()
        }
    }

    // ── Editor ──

    pub fn open_in_editor(&self) -> Result<()> {
        let file = match self.selected_diff_file() {
            Some(f) => f,
            None => return Ok(()),
        };

        let editor = std::env::var("EDITOR").unwrap_or_else(|_| "code".to_string());
        let file_path = std::path::Path::new(&self.repo_root).join(&file.path);
        let line_num = file
            .hunks
            .get(self.current_hunk)
            .map(|h| h.new_start)
            .unwrap_or(1);

        let mut cmd = std::process::Command::new(&editor);
        if editor.contains("code") || editor.contains("cursor") {
            cmd.arg(&self.repo_root)
                .arg("-g")
                .arg(format!("{}:{}", file_path.display(), line_num));
        } else if editor.contains("zed") {
            cmd.arg(&self.repo_root)
                .arg(format!("{}:{}", file_path.display(), line_num));
        } else {
            cmd.arg(format!("+{}", line_num)).arg(&file_path);
        }

        cmd.spawn().context("Failed to open editor")?;
        Ok(())
    }

    // ── Mode ──

    pub fn set_mode(&mut self, mode: DiffMode) {
        if self.mode != mode {
            // Remember current position to restore after mode switch
            let prev_path = self.files
                .get(self.selected_file)
                .map(|f| f.path.clone());
            let prev_hunk = self.current_hunk;
            let prev_line = self.current_line;

            self.mode = mode;
            self.current_hunk = 0;
            self.current_line = None;
            self.selected_watched = None;
            self.diff_scroll = 0;
            let _ = self.refresh_diff();

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
        let reviewed = self.files.iter().filter(|f| self.reviewed.contains(&f.path)).count();
        (reviewed, total)
    }

    /// Count of reviewed files vs total among filtered files only.
    /// Returns None if no filter is active.
    pub fn filtered_reviewed_count(&self) -> Option<(usize, usize)> {
        if self.filter_rules.is_empty() {
            return None;
        }
        let (mut total, mut reviewed) = (0, 0);
        for f in &self.files {
            if super::filter::apply_filter(&self.filter_rules, f) {
                total += 1;
                if self.reviewed.contains(&f.path) {
                    reviewed += 1;
                }
            }
        }
        Some((reviewed, total))
    }

    fn load_reviewed_files(repo_root: &str) -> HashSet<String> {
        let path = format!("{}/.er-reviewed", repo_root);
        match std::fs::read_to_string(&path) {
            Ok(content) => content
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect(),
            Err(_) => HashSet::new(),
        }
    }

    fn save_reviewed_files(&self) -> Result<()> {
        let path = format!("{}/.er-reviewed", self.repo_root);
        if self.reviewed.is_empty() {
            // Remove file if no reviewed files
            let _ = std::fs::remove_file(&path);
            return Ok(());
        }
        let mut lines: Vec<&String> = self.reviewed.iter().collect();
        lines.sort();
        let content = lines.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n");
        std::fs::write(&path, format!("{}\n", content))?;
        Ok(())
    }

}

// ── Main App State ──

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
    pub watch_message_ticks: u8,

    /// Counter for throttling AI file polling (check every 10 ticks ≈ 1s)
    pub ai_poll_counter: u8,

    /// Application configuration (loaded from .er-config.toml)
    pub config: ErConfig,
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
                    // GitHub PR URL — checkout and open
                    let pr_ref = crate::github::parse_github_pr_url(path)
                        .ok_or_else(|| anyhow::anyhow!("Invalid GitHub PR URL: {}", path))?;
                    crate::github::ensure_gh_installed()?;

                    // We need to be in the repo to checkout. Use cwd.
                    let repo_root = git::get_repo_root()
                        .context("Cannot open PR URL: not in a git repository. Clone the repo first.")?;

                    // Verify the local repo matches the PR's repo
                    crate::github::verify_remote_matches(&repo_root, &pr_ref)?;

                    crate::github::gh_pr_checkout(pr_ref.number, &repo_root)?;
                    let base = crate::github::gh_pr_base_branch(pr_ref.number, &repo_root)?;
                    let base = crate::github::ensure_base_ref_available(&repo_root, &base)?;

                    let tab = TabState::new_with_base(repo_root, base)?;
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
            ai_poll_counter: 0,
            config: er_config,
        })
    }

    // ── Tab Accessors ──

    /// Get a reference to the active tab
    pub fn tab(&self) -> &TabState {
        &self.tabs[self.active_tab]
    }

    /// Get a mutable reference to the active tab
    pub fn tab_mut(&mut self) -> &mut TabState {
        &mut self.tabs[self.active_tab]
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

    /// Switch to the next tab (circular)
    pub fn next_tab(&mut self) {
        if self.tabs.len() > 1 {
            self.active_tab = (self.active_tab + 1) % self.tabs.len();
        }
    }

    /// Switch to the previous tab (circular)
    pub fn prev_tab(&mut self) {
        if self.tabs.len() > 1 {
            self.active_tab = if self.active_tab == 0 {
                self.tabs.len() - 1
            } else {
                self.active_tab - 1
            };
        }
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

    // ── Overlay: Settings ──

    /// Open the settings overlay
    pub fn open_settings(&mut self) {
        let items = config::settings_items();
        // Find the first selectable (non-header) item
        let first_selectable = items.iter().position(|item| {
            !matches!(item, config::SettingsItem::SectionHeader(_))
        }).unwrap_or(0);

        self.overlay = Some(OverlayData::Settings {
            selected: first_selectable,
            saved_config: self.config.clone(),
        });
    }

    /// Toggle the currently selected boolean setting
    pub fn settings_toggle(&mut self) {
        let items = config::settings_items();
        if let Some(OverlayData::Settings { selected, .. }) = &self.overlay {
            let idx = *selected;
            if let Some(config::SettingsItem::BoolToggle { get, set, .. }) = items.get(idx) {
                let current = get(&self.config);
                set(&mut self.config, !current);
            }
        }
    }

    /// Save settings to disk and close the overlay
    pub fn settings_save(&mut self) {
        if let Err(e) = config::save_config(&self.config) {
            self.notify(&format!("Failed to save: {}", e));
        } else {
            self.notify("Settings saved");
        }
        self.overlay = None;
    }

    /// Revert settings to the saved snapshot and close the overlay
    pub fn settings_cancel(&mut self) {
        if let Some(OverlayData::Settings { saved_config, .. }) = self.overlay.take() {
            self.config = saved_config;
        }
    }

    // ── Overlay: Navigation ──

    pub fn overlay_next(&mut self) {
        match &mut self.overlay {
            Some(OverlayData::WorktreePicker { worktrees, selected }) => {
                if *selected + 1 < worktrees.len() {
                    *selected += 1;
                }
            }
            Some(OverlayData::DirectoryBrowser { entries, selected, .. }) => {
                if *selected + 1 < entries.len() {
                    *selected += 1;
                }
            }
            Some(OverlayData::Settings { selected, .. }) => {
                let items = config::settings_items();
                // Skip section headers when navigating down
                let mut next = *selected + 1;
                while next < items.len() {
                    if !matches!(items[next], config::SettingsItem::SectionHeader(_)) {
                        break;
                    }
                    next += 1;
                }
                if next < items.len() {
                    *selected = next;
                }
            }
            // `selected` indexes presets (0..preset_count) then history (preset_count..);
            // the visual separator in the overlay is render-only and not selectable
            Some(OverlayData::FilterHistory { history, selected, preset_count }) => {
                if *selected + 1 < *preset_count + history.len() {
                    *selected += 1;
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
            Some(OverlayData::Settings { selected, .. }) => {
                let items = config::settings_items();
                // Skip section headers when navigating up
                if *selected > 0 {
                    let mut prev = *selected - 1;
                    while prev > 0 && matches!(items[prev], config::SettingsItem::SectionHeader(_)) {
                        prev -= 1;
                    }
                    if !matches!(items[prev], config::SettingsItem::SectionHeader(_)) {
                        *selected = prev;
                    }
                }
            }
            None => {}
        }
    }

    /// Handle Enter in an overlay — opens selection in a new tab or saves settings
    pub fn overlay_select(&mut self) -> Result<()> {
        // Settings overlay: Enter on a toggle item toggles it; otherwise treat as Save
        if let Some(OverlayData::Settings { selected, .. }) = &self.overlay {
            let items = config::settings_items();
            if let Some(item) = items.get(*selected) {
                match item {
                    config::SettingsItem::BoolToggle { .. } => {
                        self.settings_toggle();
                        return Ok(());
                    }
                    _ => {
                        // Non-toggleable item — treat Enter as Save
                        self.settings_save();
                        return Ok(());
                    }
                }
            }
            return Ok(());
        }

        let overlay = match self.overlay.take() {
            Some(o) => o,
            None => return Ok(()),
        };

        match overlay {
            OverlayData::WorktreePicker { worktrees, selected } => {
                if let Some(wt) = worktrees.get(selected) {
                    let path = wt.path.clone();
                    self.open_in_new_tab(path)?;
                }
            }
            OverlayData::FilterHistory { history, selected, preset_count } => {
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
            OverlayData::DirectoryBrowser { current_path, entries, selected } => {
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
            OverlayData::Settings { .. } => {
                // Already handled above
            }
        }
        Ok(())
    }

    /// Go up one directory in the directory browser
    pub fn overlay_go_up(&mut self) {
        if let Some(OverlayData::DirectoryBrowser { ref mut current_path, ref mut entries, ref mut selected }) = self.overlay {
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

    /// Close the overlay (reverts settings changes if in Settings overlay)
    pub fn overlay_close(&mut self) {
        if matches!(self.overlay, Some(OverlayData::Settings { .. })) {
            self.settings_cancel();
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
                    entries.push(DirEntry { name, is_dir, is_git_repo });
                }
            }
        }
        entries.sort_by(|a, b| {
            match (a.is_dir, b.is_dir) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.name.cmp(&b.name),
            }
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
        }

        self.tab_mut().refresh_diff()?;
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

    /// Stage just the current hunk
    pub fn stage_current_hunk(&mut self) -> Result<()> {
        let si = self.tab().selected_file;
        let hi = self.tab().current_hunk;

        if si >= self.tab().files.len() {
            return Ok(());
        }
        if hi >= self.tab().files[si].hunks.len() {
            return Ok(());
        }

        let file_path = self.tab().files[si].path.clone();
        let hunk_clone = self.tab().files[si].hunks[hi].clone();
        let repo_root = self.tab().repo_root.clone();

        git::git_stage_hunk(&repo_root, &file_path, &hunk_clone)?;
        self.notify("Staged hunk");
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

        let tab = self.tab_mut();
        let was_reviewed = tab.reviewed.contains(&path);
        if was_reviewed {
            tab.reviewed.remove(&path);
        } else {
            tab.reviewed.insert(path.clone());
        }
        tab.save_reviewed_files()?;

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

    // ── Comment System ──

    /// Enter comment mode for the current file + hunk (and optionally line)
    pub fn start_comment(&mut self, comment_type: CommentType) {
        let tab = self.tab_mut();
        let file_path = match tab.selected_diff_file() {
            Some(f) => f.path.clone(),
            None => return,
        };
        tab.comment_input.clear();
        tab.comment_file = file_path;
        tab.comment_hunk = tab.current_hunk;
        tab.comment_line_num = tab.current_line_number();
        tab.comment_reply_to = None;
        tab.comment_type = comment_type;
        self.input_mode = InputMode::Comment;
    }

    /// Submit the current comment/question to the appropriate file
    pub fn submit_comment(&mut self) -> Result<()> {
        let tab = self.tab();
        let text = tab.comment_input.trim().to_string();
        if text.is_empty() {
            self.input_mode = InputMode::Normal;
            return Ok(());
        }

        let comment_type = tab.comment_type;
        match comment_type {
            CommentType::Question => self.submit_question(text),
            CommentType::GitHubComment => self.submit_github_comment(text),
        }
    }

    /// Submit a personal review question to .er-questions.json
    fn submit_question(&mut self, text: String) -> Result<()> {
        let tab = self.tab();
        let repo_root = tab.repo_root.clone();
        let diff_hash = tab.branch_diff_hash.clone();
        let file_path = tab.comment_file.clone();
        let hunk_index = tab.comment_hunk;
        let comment_line_num = tab.comment_line_num;

        let (line_start, line_content) = self.get_line_context(hunk_index, comment_line_num);

        // Load or create .er-questions.json
        let questions_path = format!("{}/.er-questions.json", repo_root);
        let mut questions: ai::ErQuestions = match std::fs::read_to_string(&questions_path) {
            Ok(content) => match serde_json::from_str(&content) {
                Ok(qs) => qs,
                Err(_) => {
                    self.notify("Warning: .er-questions.json is invalid JSON — starting fresh");
                    ai::ErQuestions {
                        version: 1,
                        diff_hash: diff_hash.clone(),
                        questions: Vec::new(),
                    }
                }
            },
            Err(_) => ai::ErQuestions {
                version: 1,
                diff_hash: diff_hash.clone(),
                questions: Vec::new(),
            },
        };

        // If diff hash changed, start fresh
        if questions.diff_hash != diff_hash {
            questions.diff_hash = diff_hash;
            questions.questions.clear();
        }

        let seq = COMMENT_SEQ.fetch_add(1, Ordering::Relaxed);
        let id = format!(
            "q-{}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0),
            seq
        );

        questions.questions.push(ai::ReviewQuestion {
            id,
            timestamp: chrono_now(),
            file: file_path,
            hunk_index: Some(hunk_index),
            line_start,
            line_content,
            text: text.clone(),
            resolved: false,
            stale: false,
        });

        // Write atomically
        let json = serde_json::to_string_pretty(&questions)?;
        let tmp_path = format!("{}.tmp", questions_path);
        std::fs::write(&tmp_path, json)?;
        std::fs::rename(&tmp_path, &questions_path)?;

        self.tab_mut().comment_input.clear();
        self.input_mode = InputMode::Normal;
        self.tab_mut().reload_ai_state();
        self.notify(&format!("Question added: {}", truncate(&text, 40)));
        Ok(())
    }

    /// Submit a GitHub PR comment to .er-github-comments.json
    fn submit_github_comment(&mut self, text: String) -> Result<()> {
        let tab = self.tab();
        let repo_root = tab.repo_root.clone();
        let diff_hash = tab.branch_diff_hash.clone();
        let file_path = tab.comment_file.clone();
        let hunk_index = tab.comment_hunk;
        let reply_to = tab.comment_reply_to.clone();
        let comment_line_num = tab.comment_line_num;

        let (line_start, line_content) = self.get_line_context(hunk_index, comment_line_num);

        // Load or create .er-github-comments.json
        let comments_path = format!("{}/.er-github-comments.json", repo_root);
        let mut gh_comments: ai::ErGitHubComments = match std::fs::read_to_string(&comments_path) {
            Ok(content) => match serde_json::from_str(&content) {
                Ok(gc) => gc,
                Err(_) => {
                    self.notify("Warning: .er-github-comments.json is invalid JSON — starting fresh");
                    ai::ErGitHubComments {
                        version: 1,
                        diff_hash: diff_hash.clone(),
                        github: None,
                        comments: Vec::new(),
                    }
                }
            },
            Err(_) => ai::ErGitHubComments {
                version: 1,
                diff_hash: diff_hash.clone(),
                github: None,
                comments: Vec::new(),
            },
        };

        // If diff hash changed, start fresh for local comments only
        if gh_comments.diff_hash != diff_hash {
            gh_comments.diff_hash = diff_hash;
            // Keep synced GitHub comments, clear local-only ones
            gh_comments.comments.retain(|c| c.source == "github");
        }

        let seq = COMMENT_SEQ.fetch_add(1, Ordering::Relaxed);
        let id = format!(
            "c-{}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0),
            seq
        );

        gh_comments.comments.push(ai::GitHubReviewComment {
            id,
            timestamp: chrono_now(),
            file: file_path,
            hunk_index: Some(hunk_index),
            line_start,
            line_end: None,
            line_content,
            comment: text.clone(),
            in_reply_to: reply_to,
            resolved: false,
            source: "local".to_string(),
            github_id: None,
            author: "You".to_string(),
            synced: false,
            stale: false,
        });

        // Write atomically
        let json = serde_json::to_string_pretty(&gh_comments)?;
        let tmp_path = format!("{}.tmp", comments_path);
        std::fs::write(&tmp_path, json)?;
        std::fs::rename(&tmp_path, &comments_path)?;

        self.tab_mut().comment_input.clear();
        self.input_mode = InputMode::Normal;
        self.tab_mut().reload_ai_state();
        self.notify(&format!("Comment added: {}", truncate(&text, 40)));
        Ok(())
    }

    /// Helper: get line context for a comment target
    fn get_line_context(&self, hunk_index: usize, comment_line_num: Option<usize>) -> (Option<usize>, String) {
        let tab = self.tab();
        if let Some(df) = tab.selected_diff_file() {
            if let Some(hunk) = df.hunks.get(hunk_index) {
                if let Some(ln) = comment_line_num {
                    let content = hunk.lines.iter()
                        .find(|l| l.new_num == Some(ln))
                        .map(|l| l.content.clone())
                        .unwrap_or_default();
                    (Some(ln), content)
                } else {
                    (None, hunk.header.clone())
                }
            } else {
                (None, String::new())
            }
        } else {
            (None, String::new())
        }
    }

    /// Cancel comment input
    pub fn cancel_comment(&mut self) {
        self.tab_mut().comment_input.clear();
        self.input_mode = InputMode::Normal;
    }

    // ── Comment Focus & Navigation ──

    /// Toggle comment focus mode. When in a hunk with comments, Tab enters
    /// comment focus; pressing Tab again exits it.
    pub fn toggle_comment_focus(&mut self) {
        let tab = self.tab_mut();
        if tab.comment_focus.is_some() {
            // Exit comment focus
            tab.comment_focus = None;
            return;
        }

        // Enter comment focus: find first comment in the current hunk
        let file_path = match tab.selected_diff_file() {
            Some(f) => f.path.clone(),
            None => return,
        };
        let hunk_idx = tab.current_hunk;

        // Collect all top-level comments for this hunk (line + hunk-level)
        let comments = tab.ai.comments_for_hunk(&file_path, hunk_idx);
        let top_level: Vec<_> = comments.iter()
            .filter(|c| c.in_reply_to().is_none())
            .collect();

        if let Some(first) = top_level.first() {
            tab.comment_focus = Some(CommentFocus {
                file: file_path,
                hunk_index: Some(hunk_idx),
                comment_id: first.id().to_string(),
            });
        }
    }

    /// Navigate to next comment in the current hunk
    pub fn next_comment(&mut self) {
        let tab = self.tab_mut();
        let focus = match &tab.comment_focus {
            Some(f) => f.clone(),
            None => return,
        };

        // All comments in this hunk (including replies)
        let hunk_idx = focus.hunk_index.unwrap_or(0);
        let all_comments = tab.ai.comments_for_hunk(&focus.file, hunk_idx);
        let ids: Vec<String> = all_comments.iter().map(|c| c.id().to_string()).collect();

        if let Some(pos) = ids.iter().position(|id| *id == focus.comment_id) {
            if pos + 1 < ids.len() {
                tab.comment_focus = Some(CommentFocus {
                    file: focus.file,
                    hunk_index: focus.hunk_index,
                    comment_id: ids[pos + 1].clone(),
                });
            }
        }
    }

    /// Navigate to previous comment in the current hunk
    pub fn prev_comment(&mut self) {
        let tab = self.tab_mut();
        let focus = match &tab.comment_focus {
            Some(f) => f.clone(),
            None => return,
        };

        let hunk_idx = focus.hunk_index.unwrap_or(0);
        let all_comments = tab.ai.comments_for_hunk(&focus.file, hunk_idx);
        let ids: Vec<String> = all_comments.iter().map(|c| c.id().to_string()).collect();

        if let Some(pos) = ids.iter().position(|id| *id == focus.comment_id) {
            if pos > 0 {
                tab.comment_focus = Some(CommentFocus {
                    file: focus.file,
                    hunk_index: focus.hunk_index,
                    comment_id: ids[pos - 1].clone(),
                });
            }
        }
    }

    // ── Reply System ──

    /// Start replying to the focused comment (GitHub comments only)
    pub fn start_reply(&mut self) {
        let tab = self.tab();
        let focus = match &tab.comment_focus {
            Some(f) => f.clone(),
            None => return,
        };

        // Find the focused comment in the unified view
        let hunk_idx = focus.hunk_index.unwrap_or(0);
        let all_comments = tab.ai.comments_for_hunk(&focus.file, hunk_idx);
        let comment = all_comments.iter().find(|c| c.id() == focus.comment_id);

        match comment {
            Some(c) => {
                // Can only reply to GitHub comments, not questions
                if c.comment_type() == CommentType::Question {
                    self.notify("Cannot reply to a question — use /er-questions instead");
                    return;
                }
                // Block nested replies
                if !c.can_reply() {
                    self.notify("Cannot reply to a reply");
                    return;
                }
            }
            None => return,
        }

        let tab = self.tab_mut();
        tab.comment_input.clear();
        tab.comment_file = focus.file;
        tab.comment_hunk = focus.hunk_index.unwrap_or(0);
        tab.comment_line_num = None;
        tab.comment_reply_to = Some(focus.comment_id);
        tab.comment_type = CommentType::GitHubComment;
        self.input_mode = InputMode::Comment;
    }

    // ── Comment Deletion ──

    /// Initiate comment deletion (enters confirm mode)
    pub fn start_delete_comment(&mut self) {
        let tab = self.tab();
        let focus = match &tab.comment_focus {
            Some(f) => f.clone(),
            None => return,
        };

        // Find the comment in the unified view to check deletability
        let hunk_idx = focus.hunk_index.unwrap_or(0);
        let all_comments = tab.ai.comments_for_hunk(&focus.file, hunk_idx);
        let comment = all_comments.iter().find(|c| c.id() == focus.comment_id);

        match comment {
            Some(c) => {
                if !c.can_delete() {
                    self.notify("Cannot delete others' comments");
                    return;
                }
            }
            None => return,
        }

        self.input_mode = InputMode::Confirm(ConfirmAction::DeleteComment {
            comment_id: focus.comment_id,
        });
    }

    /// Execute comment deletion after confirmation
    pub fn confirm_delete_comment(&mut self, comment_id: &str) -> Result<()> {
        let tab = self.tab();
        let repo_root = tab.repo_root.clone();

        // Determine which file this comment lives in
        let is_question = comment_id.starts_with("q-");

        if is_question {
            // Delete from .er-questions.json
            let path = format!("{}/.er-questions.json", repo_root);
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(mut qs) = serde_json::from_str::<ai::ErQuestions>(&content) {
                    qs.questions.retain(|q| q.id != comment_id);
                    let json = serde_json::to_string_pretty(&qs)?;
                    let tmp_path = format!("{}.tmp", path);
                    std::fs::write(&tmp_path, &json)?;
                    std::fs::rename(&tmp_path, &path)?;
                }
            }
        } else {
            // Delete from .er-github-comments.json
            let path = format!("{}/.er-github-comments.json", repo_root);
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(mut gc) = serde_json::from_str::<ai::ErGitHubComments>(&content) {
                    // Check if the comment has a github_id for API deletion
                    let github_id = gc.comments.iter()
                        .find(|c| c.id == comment_id)
                        .and_then(|c| c.github_id);

                    let reply_github_ids: Vec<u64> = gc.comments.iter()
                        .filter(|c| c.in_reply_to.as_deref() == Some(comment_id) && c.github_id.is_some())
                        .filter_map(|c| c.github_id)
                        .collect();

                    // Delete from GitHub if applicable
                    if let Some(gh_id) = github_id {
                        if let Some(ref gh) = gc.github {
                            let _ = crate::github::gh_pr_delete_comment(&gh.owner, &gh.repo, gh_id);
                            for reply_id in &reply_github_ids {
                                let _ = crate::github::gh_pr_delete_comment(&gh.owner, &gh.repo, *reply_id);
                            }
                        }
                    }

                    // Remove comment and cascade replies
                    gc.comments.retain(|c| {
                        c.id != comment_id && c.in_reply_to.as_deref() != Some(comment_id)
                    });

                    let json = serde_json::to_string_pretty(&gc)?;
                    let tmp_path = format!("{}.tmp", path);
                    std::fs::write(&tmp_path, &json)?;
                    std::fs::rename(&tmp_path, &path)?;
                }
            }
        }

        // Clear focus and return to normal
        self.tab_mut().comment_focus = None;
        self.input_mode = InputMode::Normal;
        self.tab_mut().reload_ai_state();
        self.notify("Comment deleted");
        Ok(())
    }

    /// Cancel the confirm dialog
    pub fn cancel_confirm(&mut self) {
        self.input_mode = InputMode::Normal;
    }

    // ── Hunk Comment (Shift-C) ──

    /// Enter comment mode for a hunk-level comment (no line_start)
    pub fn start_hunk_comment(&mut self, comment_type: CommentType) {
        let tab = self.tab_mut();
        let file_path = match tab.selected_diff_file() {
            Some(f) => f.path.clone(),
            None => return,
        };
        tab.comment_input.clear();
        tab.comment_file = file_path;
        tab.comment_hunk = tab.current_hunk;
        tab.comment_line_num = None; // Force hunk-level
        tab.comment_reply_to = None;
        tab.comment_type = comment_type;
        self.input_mode = InputMode::Comment;
    }

    // ── Commit ──

    /// Start commit input (only in Staged mode)
    pub fn start_commit(&mut self) {
        self.tab_mut().commit_input.clear();
        self.input_mode = InputMode::Commit;
    }

    /// Run git commit with the typed message
    pub fn submit_commit(&mut self) -> Result<()> {
        let message = self.tab().commit_input.trim().to_string();
        if message.is_empty() {
            self.input_mode = InputMode::Normal;
            return Ok(());
        }
        let repo_root = self.tab().repo_root.clone();
        git::git_commit(&repo_root, &message)?;
        self.tab_mut().commit_input.clear();
        self.input_mode = InputMode::Normal;
        let _ = self.tab_mut().refresh_diff();
        self.notify("Committed!");
        Ok(())
    }

    /// Cancel commit input
    pub fn cancel_commit(&mut self) {
        self.tab_mut().commit_input.clear();
        self.input_mode = InputMode::Normal;
    }

    // ── AiReview Navigation ──

    /// Jump from AiSummary panel to the selected file in FileDetail mode
    pub fn review_jump_to_file(&mut self) {
        let file_path = {
            let tab = self.tab();
            match tab.review_focus {
                ReviewFocus::Files => tab.ai.review_file_at(tab.review_cursor),
                ReviewFocus::Checklist => tab.ai.checklist_file_at(tab.review_cursor),
            }
        };

        if let Some(path) = file_path {
            let file_idx = self.tab().files.iter().position(|f| f.path == path);
            if let Some(idx) = file_idx {
                let tab = self.tab_mut();
                tab.selected_file = idx;
                tab.current_hunk = 0;
                tab.current_line = None;
                tab.diff_scroll = 0;
                tab.h_scroll = 0;
                tab.panel = Some(PanelContent::FileDetail);
                self.notify(&format!("Jumped to: {}", path));
            } else {
                self.notify(&format!("File not in diff: {}", path));
            }
        }
    }

    /// Toggle the checklist item at cursor and persist to .er-checklist.json
    pub fn review_toggle_checklist(&mut self) -> Result<()> {
        let tab = self.tab_mut();
        if tab.review_focus != ReviewFocus::Checklist {
            return Ok(());
        }

        let cursor = tab.review_cursor;
        tab.ai.toggle_checklist_item(cursor);

        // Persist atomically via temp file + rename
        if let Some(ref checklist) = tab.ai.checklist {
            let checklist_path = format!("{}/.er-checklist.json", tab.repo_root);
            let tmp_path = format!("{}.tmp", checklist_path);
            let json = serde_json::to_string_pretty(checklist)?;
            std::fs::write(&tmp_path, json)?;
            std::fs::rename(&tmp_path, &checklist_path)?;
        }

        let checked = tab.ai.checklist.as_ref()
            .and_then(|c| c.items.get(cursor))
            .map(|i| i.checked)
            .unwrap_or(false);

        if checked {
            self.notify("✓ Item checked");
        } else {
            self.notify("○ Item unchecked");
        }
        Ok(())
    }

    // ── Clipboard ──

    /// Copy the current hunk to the system clipboard
    pub fn yank_hunk(&mut self) -> Result<()> {
        let si = self.tab().selected_file;
        let hi = self.tab().current_hunk;

        if si >= self.tab().files.len() {
            self.notify("No file selected");
            return Ok(());
        }
        if hi >= self.tab().files[si].hunks.len() {
            self.notify("No hunk selected");
            return Ok(());
        }

        let text = self.tab().files[si].hunks[hi].to_text();
        Self::copy_to_clipboard(&text)?;
        self.notify("Hunk copied to clipboard");
        Ok(())
    }

    /// Copy rich context to clipboard for pasting into an agent terminal.
    ///
    /// What gets copied depends on navigation state:
    /// - Selection active (shift+arrow): selected lines only
    /// - Line-level nav (arrow keys): current line only
    /// - Hunk-level nav (n/N keys): full hunk
    pub fn copy_context(&mut self) -> Result<()> {
        let tab = self.tab();
        let file = match tab.selected_diff_file() {
            Some(f) => f,
            None => {
                self.notify("No file selected");
                return Ok(());
            }
        };
        let hunk = match file.hunks.get(tab.current_hunk) {
            Some(h) => h,
            None => {
                self.notify("No hunk selected");
                return Ok(());
            }
        };

        let mut text = String::new();

        // Header
        text.push_str(&format!("File: {}\n", file.path));
        text.push_str(&format!("Branch: {} (vs {})\n", tab.current_branch, tab.base_branch));

        // Determine what to copy based on navigation state
        let (lines_to_copy, line_label) = if let Some(range) = tab.selected_range() {
            // Shift+arrow selection: copy selected lines
            let selected: Vec<_> = hunk.lines.iter().enumerate()
                .filter(|(i, _)| range.contains(i))
                .map(|(_, l)| l)
                .collect();
            let start = selected.first().and_then(|l| l.new_num).unwrap_or(0);
            let end = selected.last().and_then(|l| l.new_num).unwrap_or(0);
            let label = if start == end {
                format!("Line {}", start)
            } else {
                format!("Lines {}-{}", start, end)
            };
            (selected, label)
        } else if let Some(line_idx) = tab.current_line {
            // Line-level navigation: copy current line only
            if let Some(line) = hunk.lines.get(line_idx) {
                let ln = line.new_num.unwrap_or(0);
                (vec![line], format!("Line {}", ln))
            } else {
                let all: Vec<_> = hunk.lines.iter().collect();
                (all, format!("Hunk #{}", tab.current_hunk + 1))
            }
        } else {
            // Hunk-level navigation: copy full hunk
            let all: Vec<_> = hunk.lines.iter().collect();
            (all, format!("Hunk #{}", tab.current_hunk + 1))
        };

        text.push_str(&format!("{}:\n\n", line_label));

        // Hunk header
        text.push_str(&format!(" {}\n", hunk.header));

        // Diff lines
        for line in &lines_to_copy {
            let prefix = match line.line_type {
                crate::git::LineType::Add => "+",
                crate::git::LineType::Delete => "-",
                crate::git::LineType::Context => " ",
            };
            text.push_str(&format!("{}{}\n", prefix, line.content));
        }

        // AI finding if present
        let findings = tab.ai.findings_for_hunk(&file.path, tab.current_hunk);
        if let Some(finding) = findings.first() {
            text.push_str(&format!("\nFinding: [{:?}] {}\n", finding.severity, finding.title));
            if !finding.suggestion.is_empty() {
                text.push_str(&format!("Suggestion: {}\n", finding.suggestion));
            }
        }

        let line_count = lines_to_copy.len();
        let scope = if tab.selected_range().is_some() {
            "selection"
        } else if tab.current_line.is_some() {
            "line"
        } else {
            "hunk"
        };
        Self::copy_to_clipboard(&text)?;
        self.notify(&format!("Copied {} ({} lines)", scope, line_count));
        Ok(())
    }

    fn copy_to_clipboard(text: &str) -> Result<()> {
        let (cmd, args): (&str, Vec<&str>) = if cfg!(target_os = "macos") {
            ("pbcopy", vec![])
        } else if cfg!(target_os = "windows") {
            ("clip", vec![])
        } else {
            // Linux — try xclip, fall back to xsel
            if std::process::Command::new("which")
                .arg("xclip")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
            {
                ("xclip", vec!["-selection", "clipboard"])
            } else {
                ("xsel", vec!["--clipboard", "--input"])
            }
        };

        let mut child = std::process::Command::new(cmd)
            .args(&args)
            .stdin(std::process::Stdio::piped())
            .spawn()
            .context("Failed to open clipboard command")?;

        if let Some(ref mut stdin) = child.stdin {
            stdin.write_all(text.as_bytes())?;
        }

        child.wait().context("Clipboard command failed")?;
        Ok(())
    }

    // ── Notifications ──

    pub fn notify(&mut self, msg: &str) {
        self.watch_message = Some(msg.to_string());
        self.watch_message_ticks = 0;
    }

    /// Tick called on every event loop iteration — used for notification auto-clear
    pub fn tick(&mut self) {
        if self.watch_message.is_some() {
            self.watch_message_ticks += 1;
            if self.watch_message_ticks > 20 {
                self.watch_message = None;
                self.watch_message_ticks = 0;
            }
        }
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
        let days_in_year = if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) { 366 } else { 365 };
        if d < days_in_year {
            break;
        }
        d -= days_in_year;
        y += 1;
    }

    // Walk months within the year (m is 0-indexed, d ends as 0-indexed day-of-month)
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let month_days: [i64; 12] = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
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
        y, m + 1, d + 1, hours, minutes, seconds
    )
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
    use std::collections::HashSet;

    fn make_test_tab(files: Vec<DiffFile>) -> TabState {
        use crate::ai::{InlineLayers, ReviewFocus};
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
            ai_panel_scroll: 0,
            layers: InlineLayers::default(),
            panel: None,
            panel_scroll: 0,
            panel_focus: false,
            review_focus: ReviewFocus::Files,
            review_cursor: 0,
            search_query: String::new(),
            filter_expr: String::new(),
            filter_rules: Vec::new(),
            filter_input: String::new(),
            filter_history: Vec::new(),
            reviewed: HashSet::new(),
            show_unreviewed_only: false,
            sort_by_mtime: false,
            ai: AiState::default(),
            diff_hash: String::new(),
            branch_diff_hash: String::new(),
            last_ai_check: None,
            comment_input: String::new(),
            comment_file: String::new(),
            comment_hunk: 0,
            comment_reply_to: None,
            comment_line_num: None,
            comment_focus: None,
            comment_type: CommentType::GitHubComment,
            watched_config: WatchedConfig::default(),
            watched_files: Vec::new(),
            selected_watched: None,
            show_watched: false,
            watched_not_ignored: Vec::new(),
            commit_input: String::new(),
            compaction_config: CompactionConfig::default(),
            hunk_offsets: None,
            file_tree_cache: None,
            mem_budget: MemoryBudget::default(),
            lazy_mode: false,
            file_headers: Vec::new(),
            raw_diff: None,
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
        let visible = tab.visible_files();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].1.path, "src/main.rs");
    }

    #[test]
    fn visible_files_search_query_no_match_returns_empty() {
        let files = vec![
            make_file("src/main.rs", vec![], 1, 0),
        ];
        let mut tab = make_test_tab(files);
        tab.search_query = "zzz".to_string();
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
        tab.reviewed.insert("src/main.rs".to_string());
        tab.reviewed.insert("src/lib.rs".to_string());
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
        tab.reviewed.insert("src/main.rs".to_string());
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
        tab.show_unreviewed_only = true;
        tab.reviewed.insert("src/main.rs".to_string());
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
        tab.reviewed.insert("src/main.rs".to_string());
        assert_eq!(tab.reviewed_count(), (1, 3));
    }

    #[test]
    fn reviewed_count_all_reviewed_returns_n_n() {
        let files = vec![
            make_file("src/main.rs", vec![], 1, 0),
            make_file("src/lib.rs", vec![], 1, 0),
        ];
        let mut tab = make_test_tab(files);
        tab.reviewed.insert("src/main.rs".to_string());
        tab.reviewed.insert("src/lib.rs".to_string());
        assert_eq!(tab.reviewed_count(), (2, 2));
    }

    // ── next_file / prev_file ──

    #[test]
    fn next_file_at_last_file_stays_at_last() {
        let files = vec![
            make_file("a.rs", vec![], 1, 0),
            make_file("b.rs", vec![], 1, 0),
        ];
        let mut tab = make_test_tab(files);
        tab.selected_file = 1;
        tab.next_file();
        assert_eq!(tab.selected_file, 1);
    }

    #[test]
    fn prev_file_at_first_file_stays_at_first() {
        let files = vec![
            make_file("a.rs", vec![], 1, 0),
            make_file("b.rs", vec![], 1, 0),
        ];
        let mut tab = make_test_tab(files);
        tab.selected_file = 0;
        tab.prev_file();
        assert_eq!(tab.selected_file, 0);
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
        let files = vec![
            make_file("a.rs", vec![], 1, 0),
        ];
        let mut tab = make_test_tab(files);
        tab.search_query = "zzz".to_string();
        // Should not panic
        tab.next_file();
        assert_eq!(tab.selected_file, 0);
    }

    // ── next_hunk / prev_hunk ──

    #[test]
    fn next_hunk_increments_current_hunk() {
        let files = vec![
            make_file("a.rs", vec![make_hunk(vec![]), make_hunk(vec![])], 1, 0),
        ];
        let mut tab = make_test_tab(files);
        tab.current_hunk = 0;
        tab.next_hunk();
        assert_eq!(tab.current_hunk, 1);
    }

    #[test]
    fn next_hunk_at_last_hunk_stays() {
        let files = vec![
            make_file("a.rs", vec![make_hunk(vec![]), make_hunk(vec![])], 1, 0),
        ];
        let mut tab = make_test_tab(files);
        tab.current_hunk = 1;
        tab.next_hunk();
        assert_eq!(tab.current_hunk, 1);
    }

    #[test]
    fn prev_hunk_decrements_current_hunk() {
        let files = vec![
            make_file("a.rs", vec![make_hunk(vec![]), make_hunk(vec![])], 1, 0),
        ];
        let mut tab = make_test_tab(files);
        tab.current_hunk = 1;
        tab.prev_hunk();
        assert_eq!(tab.current_hunk, 0);
    }

    #[test]
    fn prev_hunk_at_zero_stays() {
        let files = vec![
            make_file("a.rs", vec![make_hunk(vec![])], 1, 0),
        ];
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
        let files = vec![make_file("a.rs", vec![make_hunk(lines1), make_hunk(lines2)], 2, 0)];
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
        let files = vec![make_file("a.rs", vec![make_hunk(lines1), make_hunk(lines2)], 3, 0)];
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
        let files = vec![
            make_file("a.rs", vec![make_hunk(vec![]), make_hunk(vec![]), make_hunk(vec![])], 3, 0),
        ];
        let tab = make_test_tab(files);
        assert_eq!(tab.total_hunks(), 3);
    }

    // ── current_line_number ──

    #[test]
    fn current_line_number_with_new_num_returns_some() {
        let lines = vec![
            make_line(LineType::Add, "x", Some(42)),
        ];
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
        let lines = vec![
            make_line(LineType::Delete, "deleted", None),
        ];
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
        tab.snap_to_visible();
        assert_eq!(tab.selected_file, 0); // snapped to src/main.rs (index 0)
    }

    // ── DiffMode methods ──

    #[test]
    fn diff_mode_label_returns_correct_strings() {
        assert_eq!(DiffMode::Branch.label(), "BRANCH DIFF");
        assert_eq!(DiffMode::Unstaged.label(), "UNSTAGED");
        assert_eq!(DiffMode::Staged.label(), "STAGED");
    }

    #[test]
    fn diff_mode_git_mode_returns_correct_strings() {
        assert_eq!(DiffMode::Branch.git_mode(), "branch");
        assert_eq!(DiffMode::Unstaged.git_mode(), "unstaged");
        assert_eq!(DiffMode::Staged.git_mode(), "staged");
    }

    // ── clamp_hunk ──

    #[test]
    fn clamp_hunk_beyond_total_clamped_to_last() {
        let files = vec![
            make_file("a.rs", vec![make_hunk(vec![]), make_hunk(vec![])], 1, 0),
        ];
        let mut tab = make_test_tab(files);
        tab.current_hunk = 99;
        tab.clamp_hunk();
        assert_eq!(tab.current_hunk, 1); // total 2 hunks → max index 1
    }

    #[test]
    fn clamp_hunk_no_hunks_resets_to_zero_and_clears_line() {
        let files = vec![
            make_file("a.rs", vec![], 0, 0),
        ];
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
        tab.panel = Some(crate::ai::PanelContent::FileDetail);
        tab.toggle_panel();
        assert_eq!(tab.panel, Some(crate::ai::PanelContent::AiSummary));
    }

    #[test]
    fn toggle_panel_skips_ai_summary_when_no_ai_data() {
        let mut tab = make_test_tab(vec![]);
        tab.panel = Some(crate::ai::PanelContent::FileDetail);
        tab.toggle_panel();
        assert_eq!(tab.panel, None);
    }

    #[test]
    fn toggle_panel_closes_from_ai_summary() {
        let mut tab = make_test_tab(vec![]);
        tab.ai.summary = Some("summary".to_string());
        tab.panel = Some(crate::ai::PanelContent::AiSummary);
        tab.toggle_panel();
        assert_eq!(tab.panel, None);
    }

    #[test]
    fn toggle_panel_resets_panel_scroll_on_each_toggle() {
        let mut tab = make_test_tab(vec![]);
        tab.panel_scroll = 42;
        tab.toggle_panel();
        assert_eq!(tab.panel_scroll, 0);

        tab.panel_scroll = 10;
        tab.toggle_panel(); // FileDetail → None (no AI data)
        assert_eq!(tab.panel_scroll, 0);
    }

    #[test]
    fn toggle_panel_sets_panel_focus_false_when_closing() {
        let mut tab = make_test_tab(vec![]);
        tab.panel = Some(crate::ai::PanelContent::FileDetail);
        tab.panel_focus = true;
        tab.toggle_panel(); // FileDetail → None (no AI data)
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
}
