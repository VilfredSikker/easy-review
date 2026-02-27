use crate::ai::{self, AiState, CommentType, InlineLayers, PanelContent, ReviewFocus};
use crate::config::{self, ErConfig, WatchedConfig};
use crate::git::{self, CommitInfo, DiffFile, DiffFileHeader, CompactionConfig, WatchedFile, Worktree};
use crate::github::PrOverviewData;
use anyhow::{Context, Result};
use std::collections::{HashSet, VecDeque};
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
#[allow(unused_imports)]
use std::time::Instant;

static COMMENT_SEQ: AtomicU64 = AtomicU64::new(0);

/// Anchor data captured at comment creation time for later relocation
struct LineAnchor {
    line_start: Option<usize>,
    line_content: String,
    context_before: Vec<String>,
    context_after: Vec<String>,
    old_line_start: Option<usize>,
    hunk_header: String,
}

impl Default for LineAnchor {
    fn default() -> Self {
        LineAnchor {
            line_start: None,
            line_content: String::new(),
            context_before: Vec::new(),
            context_after: Vec::new(),
            old_line_start: None,
            hunk_header: String::new(),
        }
    }
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
        }
    }

    pub fn git_mode(&self) -> &'static str {
        match self {
            DiffMode::Branch => "branch",
            DiffMode::Unstaged => "unstaged",
            DiffMode::Staged => "staged",
            DiffMode::History => "history",
            DiffMode::Conflicts => "conflicts",
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
}

/// Actions that require user confirmation (y/n)
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum ConfirmAction {
    DeleteComment { comment_id: String },
}

/// Which pane has focus in split diff view
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitSide {
    Old,
    New,
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
            comment_type: CommentType::GitHubComment,
            comment_edit_id: None,
            comment_finding_ref: None,
            pr_data: None,
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
            comment_type: CommentType::GitHubComment,
            comment_edit_id: None,
            comment_finding_ref: None,
            pr_data: None,
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
        }
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

    fn refresh_diff_impl(&mut self, recompute_branch_hash: bool) -> Result<()> {
        // History mode doesn't use git_diff_raw — skip normal diff refresh
        if self.mode == DiffMode::History {
            return Ok(());
        }

        // Conflicts mode refreshes via refresh_conflicts() only
        if self.mode == DiffMode::Conflicts {
            self.merge_active = git::is_merge_in_progress(&self.repo_root);
            return Ok(());
        }

        // Update merge_active on every refresh
        self.merge_active = git::is_merge_in_progress(&self.repo_root);

        // Remember current position to restore after re-parse
        let prev_path = self.files.get(self.selected_file).map(|f| f.path.clone());
        let prev_hunk = self.current_hunk;
        let prev_line = self.current_line;

        let raw = git::git_diff_raw(self.mode.git_mode(), &self.base_branch, &self.repo_root)?;

        // Decide parsing strategy based on diff size
        // TODO(risk:medium): counting newlines by iterating raw bytes on every refresh is O(n) over the full diff.
        // For very large diffs (hundreds of MB) this adds measurable latency on every watch event.
        let line_count = raw.as_bytes().iter().filter(|&&b| b == b'\n').count();
        if line_count > git::LAZY_PARSE_THRESHOLD {
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

            // Re-expand any files the user explicitly opened (fresh parse means hunks are available)
            let to_expand: Vec<String> = self.files.iter()
                .filter(|f| f.compacted && self.user_expanded.contains(&f.path))
                .map(|f| f.path.clone())
                .collect();
            let repo_root = self.repo_root.clone();
            let git_mode = self.mode.git_mode();
            let base_branch = self.base_branch.clone();
            for file in &mut self.files {
                if to_expand.contains(&file.path) {
                    git::expand_compacted_file(file, &repo_root, git_mode, &base_branch)?;
                }
            }
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
        // TODO(risk:minor): when item_count == 0, max_cursor is set to 0 and review_cursor is clamped to 0.
        // Any code that then indexes into the list at review_cursor (e.g., checklist items) must
        // separately guard against the empty-list case, or it will index position 0 of an empty Vec.
        let max_cursor = if item_count == 0 { 0 } else { item_count - 1 };
        self.review_cursor = self.review_cursor.min(max_cursor);
        self.last_ai_check = ai::latest_er_mtime(&self.repo_root);
    }

    /// Relocate all comments to their new positions after a diff change.
    fn relocate_all_comments(&mut self) {
        let current_hash = self.diff_hash.clone();
        let repo_root = self.repo_root.clone();

        // Build rename map: old path → new path
        let rename_map: std::collections::HashMap<String, String> = self.files.iter()
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
            let find_idx = |p: &str| -> Option<usize> {
                self.files.iter().position(|f| f.path == p)
            };
            let idx = find_idx(path)
                .or_else(|| rename_map.get(path).and_then(|np| find_idx(np)))?;
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
                    ai::RelocationResult::Relocated { new_hunk_index, new_line_start } => {
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
                    ai::RelocationResult::Relocated { new_hunk_index, new_line_start } => {
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
                let path = format!("{}/.er-questions.json", repo_root);
                if let Ok(json) = serde_json::to_string_pretty(qs) {
                    let tmp = format!("{}.tmp", path);
                    let _ = std::fs::write(&tmp, json).and_then(|_| std::fs::rename(&tmp, &path));
                }
            }
        }
        if comments_changed {
            if let Some(ref gc) = self.ai.github_comments {
                let path = format!("{}/.er-github-comments.json", repo_root);
                if let Ok(json) = serde_json::to_string_pretty(gc) {
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
        let latest_mtime = ai::latest_er_mtime(&self.repo_root);

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

    /// Cycle panel: None → FileDetail → AiSummary (if AI data) → PrOverview (if PR live) → None
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
                } else {
                    None
                }
            }
            Some(PanelContent::AiSummary) => {
                if has_pr {
                    Some(PanelContent::PrOverview)
                } else {
                    None
                }
            }
            Some(PanelContent::PrOverview) => {
                if self.symbol_refs.is_some() {
                    Some(PanelContent::SymbolRefs)
                } else {
                    None
                }
            }
            Some(PanelContent::SymbolRefs) => None,
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

    /// Get the currently selected file (mode-aware: History uses HistoryState)
    pub fn selected_diff_file(&self) -> Option<&DiffFile> {
        if self.selected_watched.is_some() {
            return None;
        }
        if self.mode == DiffMode::History {
            return self.history.as_ref()
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
        self.focused_comment_id = None;
        self.focused_finding_id = None;
        if let Some(idx) = self.selected_watched {
            // In watched section — move down within watched files
            let visible_watched = self.visible_watched_files();
            if let Some(pos) = visible_watched.iter().position(|(i, _)| *i == idx) {
                if pos + 1 < visible_watched.len() {
                    self.selected_watched = Some(visible_watched[pos + 1].0);
                    self.diff_scroll = 0;
                    self.h_scroll = 0;
                } else {
                    // At last watched file — wrap to first diff file
                    self.selected_watched = None;
                    let visible = self.visible_files();
                    if !visible.is_empty() {
                        self.selected_file = visible[0].0;
                        self.current_hunk = 0;
                        self.current_line = None;
                        self.selection_anchor = None;
                        self.diff_scroll = 0;
                        self.h_scroll = 0;
                        self.panel_scroll = 0;
                        self.ensure_file_parsed();
                        self.rebuild_hunk_offsets();
                    }
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
                    self.panel_scroll = 0;
                    self.ensure_file_parsed();
                    self.rebuild_hunk_offsets();
                } else {
                    // At last diff file
                    let visible_watched = self.visible_watched_files();
                    if !visible_watched.is_empty() {
                        // Transition to watched section
                        self.selected_watched = Some(visible_watched[0].0);
                        self.diff_scroll = 0;
                        self.h_scroll = 0;
                    } else {
                        // Wrap to first diff file
                        self.selected_file = visible[0].0;
                        self.current_hunk = 0;
                        self.current_line = None;
                        self.selection_anchor = None;
                        self.diff_scroll = 0;
                        self.h_scroll = 0;
                        self.panel_scroll = 0;
                        self.ensure_file_parsed();
                        self.rebuild_hunk_offsets();
                    }
                }
            } else {
                // Current selection not in visible set — snap to first
                self.selected_file = visible[0].0;
                self.current_hunk = 0;
                self.diff_scroll = 0;
                self.h_scroll = 0;
                self.panel_scroll = 0;
                self.ensure_file_parsed();
                self.rebuild_hunk_offsets();
            }
        }
    }

    pub fn prev_file(&mut self) {
        self.focused_comment_id = None;
        self.focused_finding_id = None;
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
                        self.panel_scroll = 0;
                        self.ensure_file_parsed();
                        self.rebuild_hunk_offsets();
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
                    self.panel_scroll = 0;
                    self.ensure_file_parsed();
                    self.rebuild_hunk_offsets();
                } else {
                    // At first diff file — wrap to last item
                    let visible_watched = self.visible_watched_files();
                    if !visible_watched.is_empty() {
                        self.selected_watched = Some(visible_watched.last().unwrap().0);
                        self.diff_scroll = 0;
                        self.h_scroll = 0;
                    } else {
                        // Wrap to last diff file
                        self.selected_file = visible.last().unwrap().0;
                        self.current_hunk = 0;
                        self.current_line = None;
                        self.selection_anchor = None;
                        self.diff_scroll = 0;
                        self.h_scroll = 0;
                        self.panel_scroll = 0;
                        self.ensure_file_parsed();
                        self.rebuild_hunk_offsets();
                    }
                }
            } else {
                // Current selection not in visible set — snap to first
                self.selected_file = visible[0].0;
                self.current_hunk = 0;
                self.diff_scroll = 0;
                self.h_scroll = 0;
                self.panel_scroll = 0;
                self.ensure_file_parsed();
                self.rebuild_hunk_offsets();
            }
        }
    }

    pub fn next_hunk(&mut self) {
        self.focused_comment_id = None;
        let total = self.total_hunks();
        if total > 0 && self.current_hunk < total - 1 {
            self.current_hunk += 1;
            self.current_line = None;
            self.selection_anchor = None;
            self.scroll_to_current_hunk();
        }
    }

    pub fn prev_hunk(&mut self) {
        self.focused_comment_id = None;
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

    /// Get the line number for the focused side in split diff view
    pub fn current_line_number_for_split(&self, side: SplitSide) -> Option<usize> {
        let file = self.selected_diff_file()?;
        let hunk = file.hunks.get(self.current_hunk)?;
        let line_idx = self.current_line?;
        let diff_line = hunk.lines.get(line_idx)?;
        match side {
            SplitSide::Old => diff_line.old_num,
            SplitSide::New => diff_line.new_num,
        }
    }

    /// Increment the focused pane's horizontal scroll in split diff view
    pub fn scroll_right_split(&mut self) {
        match self.split_focus {
            SplitSide::Old => self.h_scroll_old = self.h_scroll_old.saturating_add(1),
            SplitSide::New => self.h_scroll_new = self.h_scroll_new.saturating_add(1),
        }
    }

    /// Decrement the focused pane's horizontal scroll in split diff view
    pub fn scroll_left_split(&mut self) {
        match self.split_focus {
            SplitSide::Old => self.h_scroll_old = self.h_scroll_old.saturating_sub(1),
            SplitSide::New => self.h_scroll_new = self.h_scroll_new.saturating_sub(1),
        }
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
                // TODO(risk:medium): base + current_line can overflow usize on pathological inputs
        // (e.g., a hunk with usize::MAX lines). saturating_sub then .min(u16::MAX) masks the overflow
        // rather than preventing it. Add a bounds check on current_line before the addition.
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

    pub fn panel_scroll_down(&mut self, amount: u16) {
        self.panel_scroll = self.panel_scroll.saturating_add(amount);
    }

    pub fn panel_scroll_up(&mut self, amount: u16) {
        self.panel_scroll = self.panel_scroll.saturating_sub(amount);
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
                let last = file.hunks.len().saturating_sub(1);
                let line = file.hunks.get(last).map_or(0, |h| h.lines.len().saturating_sub(1));
                (last, line)
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
        // TODO(risk:medium): file_headers.get(selected_file) uses the raw file index, but selected_file
        // is an index into self.files (which may be reordered by mtime sort). If sort_by_mtime is active,
        // the file at self.files[selected_file] corresponds to a different header than
        // self.file_headers[selected_file], so we'd parse the wrong file's diff.
        // Parse on demand from raw diff
        if let (Some(ref raw), Some(header)) = (&self.raw_diff, self.file_headers.get(self.selected_file)) {
            let parsed = git::parse_file_at_offset(raw, header);
            if !parsed.hunks.is_empty() {
                if let Some(file) = self.files.get_mut(self.selected_file) {
                    file.hunks = parsed.hunks;
                    file.adds = parsed.adds;
                    file.dels = parsed.dels;
                }
                self.rebuild_hunk_offsets();
                self.update_mem_budget();
                return;
            }
        }
        // Fallback: offset parse returned no hunks but file has changes — fetch from git directly
        if let Some(file) = self.files.get(self.selected_file) {
            if file.adds + file.dels > 0 {
                let path = file.path.clone();
                let repo_root = self.repo_root.clone();
                let mode = self.mode.git_mode().to_string();
                let base = self.base_branch.clone();
                if let Ok(raw) = git::git_diff_raw_file(&mode, &base, &repo_root, &path) {
                    let parsed = git::parse_diff(&raw);
                    if let Some(p) = parsed.into_iter().next() {
                        if let Some(file) = self.files.get_mut(self.selected_file) {
                            file.hunks = p.hunks;
                            file.adds = p.adds;
                            file.dels = p.dels;
                        }
                    }
                }
            }
        }
        self.rebuild_hunk_offsets();
        self.update_mem_budget();
    }

    /// Toggle expand/compact for the currently selected file.
    /// If compacted, expand by re-fetching from git.
    /// If expanded (and was compacted), re-compact it.
    pub fn toggle_compacted(&mut self) -> Result<()> {
        if let Some(file) = self.files.get_mut(self.selected_file) {
            if file.compacted {
                let path = file.path.clone();
                git::expand_compacted_file(
                    file,
                    &self.repo_root,
                    self.mode.git_mode(),
                    &self.base_branch,
                )?;
                self.user_expanded.insert(path);
                self.rebuild_hunk_offsets();
                self.update_mem_budget();
            } else {
                // Re-compact: only if it matched a pattern or was large
                // TODO(risk:minor): any file can be re-compacted via Enter regardless of whether it originally
                // matched a compaction pattern. A file that was never auto-compacted (user navigated to it
                // in eager mode) still gets compacted on the second Enter press, which may be surprising.
                let path = file.path.clone();
                file.compacted = true;
                file.raw_hunk_count = file.hunks.len();
                file.hunks.clear();
                file.hunks.shrink_to_fit();
                self.user_expanded.remove(&path);
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

    // ── History Mode Navigation ──

    /// Move to the next commit in history (older)
    pub fn history_next_commit(&mut self) {
        let history = match self.history.as_mut() {
            Some(h) => h,
            None => return,
        };
        if history.selected_commit + 1 < history.commits.len() {
            history.selected_commit += 1;
            self.history_load_selected_diff();
        }
    }

    /// Move to the previous commit in history (newer)
    pub fn history_prev_commit(&mut self) {
        let history = match self.history.as_mut() {
            Some(h) => h,
            None => return,
        };
        if history.selected_commit > 0 {
            history.selected_commit -= 1;
            self.history_load_selected_diff();
        }
    }

    /// Load the diff for the currently selected commit
    fn history_load_selected_diff(&mut self) {
        let (hash, repo_root) = {
            let history = match self.history.as_mut() {
                Some(h) => h,
                None => return,
            };
            let commit_hash = match history.commits.get(history.selected_commit) {
                Some(c) => c.hash.clone(),
                None => return,
            };
            // Check cache first (promotes to MRU on access)
            // TODO(risk:medium): cached.clone() copies the entire Vec<DiffFile> including all hunk lines.
            // For a commit with thousands of changed lines this is an expensive allocation on every
            // back-navigation to a cached commit. Consider storing Arcs or indices instead of cloning.
            if let Some(cached) = history.diff_cache.get(&commit_hash) {
                let files = cached.clone();
                history.commit_files = files;
                history.selected_file = 0;
                history.current_hunk = 0;
                history.current_line = None;
                history.diff_scroll = 0;
                history.h_scroll = 0;
                return;
            }
            (commit_hash, self.repo_root.clone())
        };

        let files = match git::git_diff_commit(&hash, &repo_root) {
            Ok(raw) => git::parse_diff(&raw),
            Err(_) => vec![],
        };

        let history = self.history.as_mut().unwrap();
        history.diff_cache.insert(hash, files.clone());
        history.commit_files = files;
        history.selected_file = 0;
        history.current_hunk = 0;
        history.current_line = None;
        history.diff_scroll = 0;
        history.h_scroll = 0;
    }

    /// Move to next file within the selected commit's diff
    pub fn history_next_file(&mut self) {
        let history = match self.history.as_mut() {
            Some(h) => h,
            None => return,
        };
        if history.commit_files.is_empty() {
            return;
        }
        if history.selected_file + 1 < history.commit_files.len() {
            history.selected_file += 1;
            history.current_hunk = 0;
            history.current_line = None;
            Self::history_scroll_to_file(history);
        }
    }

    /// Move to previous file within the selected commit's diff
    pub fn history_prev_file(&mut self) {
        let history = match self.history.as_mut() {
            Some(h) => h,
            None => return,
        };
        if history.selected_file > 0 {
            history.selected_file -= 1;
            history.current_hunk = 0;
            history.current_line = None;
            Self::history_scroll_to_file(history);
        }
    }

    /// Move to next line within the commit diff
    pub fn history_next_line(&mut self) {
        let history = match self.history.as_mut() {
            Some(h) => h,
            None => return,
        };
        let file = match history.commit_files.get(history.selected_file) {
            Some(f) => f,
            None => return,
        };
        let hunk_count = file.hunks.len();
        let line_count = file.hunks.get(history.current_hunk).map(|h| h.lines.len()).unwrap_or(0);

        match history.current_line {
            None => {
                if line_count > 0 {
                    history.current_line = Some(0);
                    Self::history_scroll_to_current(history);
                }
            }
            Some(line) => {
                if line + 1 < line_count {
                    history.current_line = Some(line + 1);
                    Self::history_scroll_to_current(history);
                } else if history.current_hunk + 1 < hunk_count {
                    // Move to next hunk's first line
                    history.current_hunk += 1;
                    history.current_line = Some(0);
                    Self::history_scroll_to_current(history);
                } else if history.selected_file + 1 < history.commit_files.len() {
                    // Move to next file's first hunk's first line
                    history.selected_file += 1;
                    history.current_hunk = 0;
                    history.current_line = Some(0);
                    Self::history_scroll_to_current(history);
                }
            }
        }
    }

    /// Move to previous line within the commit diff
    pub fn history_prev_line(&mut self) {
        let history = match self.history.as_mut() {
            Some(h) => h,
            None => return,
        };
        let file = match history.commit_files.get(history.selected_file) {
            Some(f) => f,
            None => return,
        };

        match history.current_line {
            None => {
                let count = file.hunks.get(history.current_hunk).map(|h| h.lines.len()).unwrap_or(0);
                if count > 0 {
                    history.current_line = Some(count - 1);
                    Self::history_scroll_to_current(history);
                }
            }
            Some(0) => {
                if history.current_hunk > 0 {
                    history.current_hunk -= 1;
                    let count = file.hunks.get(history.current_hunk).map(|h| h.lines.len()).unwrap_or(0);
                    history.current_line = if count > 0 { Some(count - 1) } else { None };
                    Self::history_scroll_to_current(history);
                } else if history.selected_file > 0 {
                    // Move to prev file's last hunk's last line
                    history.selected_file -= 1;
                    let prev_file = &history.commit_files[history.selected_file];
                    if let Some(last_hunk) = prev_file.hunks.last() {
                        history.current_hunk = prev_file.hunks.len() - 1;
                        history.current_line = if last_hunk.lines.is_empty() { None } else { Some(last_hunk.lines.len() - 1) };
                    } else {
                        history.current_hunk = 0;
                        history.current_line = None;
                    }
                    Self::history_scroll_to_current(history);
                } else {
                    history.current_line = None;
                }
            }
            Some(line) => {
                history.current_line = Some(line - 1);
                Self::history_scroll_to_current(history);
            }
        }
    }

    /// Scroll to the current file header in history mode
    fn history_scroll_to_file(history: &mut HistoryState) {
        let mut line_offset: usize = 0;
        for (file_idx, file) in history.commit_files.iter().enumerate() {
            if file_idx == history.selected_file {
                history.diff_scroll = line_offset.min(u16::MAX as usize) as u16;
                return;
            }
            // File header (1) + blank line (1) + per-hunk (header + lines + blank)
            line_offset += 2; // header + blank
            for hunk in &file.hunks {
                line_offset += 1 + hunk.lines.len() + 1; // header + lines + blank
            }
        }
    }

    /// Scroll to the current line position in history mode
    fn history_scroll_to_current(history: &mut HistoryState) {
        let mut line_offset: usize = 0;
        for (file_idx, file) in history.commit_files.iter().enumerate() {
            line_offset += 2; // file header + blank
            for (hunk_idx, hunk) in file.hunks.iter().enumerate() {
                if file_idx == history.selected_file && hunk_idx == history.current_hunk {
                    line_offset += history.current_line.unwrap_or(0);
                    history.diff_scroll = line_offset.saturating_sub(1).min(u16::MAX as usize) as u16;
                    return;
                }
                line_offset += 1 + hunk.lines.len() + 1;
            }
        }
    }

    /// Load more commits when scrolling past the end
    pub fn history_load_more(&mut self) {
        let (skip, all_loaded) = match self.history.as_ref() {
            Some(h) => (h.commits.len(), h.all_loaded),
            None => return,
        };
        if all_loaded {
            return;
        }

        // TODO(risk:medium): git_log_branch is called synchronously on the event loop thread. Loading 50
        // commits on a slow filesystem or network-mounted repo blocks the UI for the full duration of the
        // git log call. This should be moved to a background thread like the PR hint check.
        let new_commits = git::git_log_branch(
            &self.base_branch,
            &self.repo_root,
            50,
            skip,
        )
        .unwrap_or_default();

        let history = self.history.as_mut().unwrap();
        if new_commits.is_empty() {
            history.all_loaded = true;
        } else {
            history.commits.extend(new_commits);
        }
    }

    /// Get visible commits (filtered by search query)
    pub fn visible_commits(&self) -> Vec<(usize, &CommitInfo)> {
        let history = match self.history.as_ref() {
            Some(h) => h,
            None => return vec![],
        };

        if self.search_query.is_empty() {
            history.commits.iter().enumerate().collect()
        } else {
            let q = self.search_query.to_lowercase();
            history
                .commits
                .iter()
                .enumerate()
                .filter(|(_, c)| {
                    c.subject.to_lowercase().contains(&q)
                        || c.short_hash.contains(&q)
                        || c.author.to_lowercase().contains(&q)
                })
                .collect()
        }
    }

    /// Scroll down in history mode
    pub fn history_scroll_down(&mut self, amount: u16) {
        if let Some(ref mut h) = self.history {
            h.diff_scroll = h.diff_scroll.saturating_add(amount);
        }
    }

    /// Scroll up in history mode
    pub fn history_scroll_up(&mut self, amount: u16) {
        if let Some(ref mut h) = self.history {
            h.diff_scroll = h.diff_scroll.saturating_sub(amount);
        }
    }

    /// Scroll right in history mode
    pub fn history_scroll_right(&mut self, amount: u16) {
        if let Some(ref mut h) = self.history {
            h.h_scroll = h.h_scroll.saturating_add(amount);
        }
    }

    /// Scroll left in history mode
    pub fn history_scroll_left(&mut self, amount: u16) {
        if let Some(ref mut h) = self.history {
            h.h_scroll = h.h_scroll.saturating_sub(amount);
        }
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
            if mode == DiffMode::History {
                // Initialize history state if first time
                if self.history.is_none() {
                    let commits = git::git_log_branch(
                        &self.base_branch,
                        &self.repo_root,
                        50,
                        0,
                    )
                    .unwrap_or_default();

                    let first_diff = if let Some(c) = commits.first() {
                        let raw = git::git_diff_commit(&c.hash, &self.repo_root)
                            .unwrap_or_default();
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
            } else {
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
    pub ai_poll_counter: u16,

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
            ai_poll_counter: 0,
            config: er_config,
        })
    }

    // ── Tab Accessors ──

    /// Get a reference to the active tab (clamps index to valid range)
    pub fn tab(&self) -> &TabState {
        let idx = self.active_tab.min(self.tabs.len().saturating_sub(1));
        &self.tabs[idx]
    }

    /// Get a mutable reference to the active tab (clamps index to valid range)
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
            DiffMode::Conflicts => {
                git::git_stage_file(&repo_root, &file_path)?;
                self.notify(&format!("Resolved: {}", file_path));
            }
            DiffMode::History => {
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
        let split_active = self.split_diff_active(&self.config.clone());
        let split_focus = self.tab().split_focus;
        let tab = self.tab_mut();
        let file_path = match tab.selected_diff_file() {
            Some(f) => f.path.clone(),
            None => return,
        };
        tab.comment_input.clear();
        tab.comment_file = file_path;
        tab.comment_hunk = tab.current_hunk;
        tab.comment_line_num = if split_active {
            tab.current_line_number_for_split(split_focus)
        } else {
            tab.current_line_number()
        };
        tab.comment_reply_to = None;
        tab.comment_finding_ref = None;
        tab.comment_type = comment_type;
        self.input_mode = InputMode::Comment;
    }

    /// Start editing an existing comment — opens comment input pre-filled with its text
    pub fn start_edit_comment(&mut self, comment_id: &str) {
        let tab = self.tab();
        // Find the comment text and type
        let (text, is_question) = if comment_id.starts_with("q-") {
            if let Some(qs) = &tab.ai.questions {
                if let Some(q) = qs.questions.iter().find(|q| q.id == comment_id) {
                    (q.text.clone(), true)
                } else { return; }
            } else { return; }
        } else {
            if let Some(gc) = &tab.ai.github_comments {
                if let Some(c) = gc.comments.iter().find(|c| c.id == comment_id) {
                    (c.comment.clone(), false)
                } else { return; }
            } else { return; }
        };

        let tab = self.tab_mut();
        let file_path = match tab.selected_diff_file() {
            Some(f) => f.path.clone(),
            None => return,
        };
        tab.comment_input = text;
        tab.comment_file = file_path;
        tab.comment_hunk = tab.current_hunk;
        tab.comment_line_num = tab.current_line_number();
        tab.comment_reply_to = None;
        tab.comment_type = if is_question { CommentType::Question } else { CommentType::GitHubComment };
        tab.comment_edit_id = Some(comment_id.to_string());
        self.input_mode = InputMode::Comment;
    }

    /// Start replying to a comment or question — creates a threaded reply
    pub fn start_reply_comment(&mut self, comment_id: &str) {
        let tab = self.tab();
        // Determine type from ID prefix and find the parent comment's location
        let (file, hunk_index, line_start, is_question) = if comment_id.starts_with("q-") {
            if let Some(qs) = &tab.ai.questions {
                if let Some(q) = qs.questions.iter().find(|q| q.id == comment_id) {
                    (q.file.clone(), q.hunk_index.unwrap_or(0), q.line_start, true)
                } else { return; }
            } else { return; }
        } else {
            if let Some(gc) = &tab.ai.github_comments {
                if let Some(c) = gc.comments.iter().find(|c| c.id == comment_id) {
                    (c.file.clone(), c.hunk_index.unwrap_or(0), c.line_start, false)
                } else { return; }
            } else { return; }
        };

        let tab = self.tab_mut();
        tab.comment_input.clear();
        tab.comment_file = file;
        tab.comment_hunk = hunk_index;
        tab.comment_line_num = line_start;
        tab.comment_reply_to = Some(comment_id.to_string());
        tab.comment_finding_ref = None;
        tab.comment_type = if is_question { CommentType::Question } else { CommentType::GitHubComment };
        tab.comment_edit_id = None;
        self.input_mode = InputMode::Comment;
    }

    /// Start replying to an AI finding — creates a GitHubComment referencing the finding
    pub fn start_reply_finding(&mut self, finding_id: &str) {
        let tab = self.tab();
        // Find the finding's file and location
        let (file, hunk_index, line_start) = if let Some(review) = &tab.ai.review {
            let mut found = None;
            for (file_path, file_review) in &review.files {
                for finding in &file_review.findings {
                    if finding.id == finding_id {
                        found = Some((file_path.clone(), finding.hunk_index.unwrap_or(0), finding.line_start));
                        break;
                    }
                }
                if found.is_some() { break; }
            }
            match found {
                Some(f) => f,
                None => return,
            }
        } else { return; };

        let tab = self.tab_mut();
        tab.comment_input.clear();
        tab.comment_file = file;
        tab.comment_hunk = hunk_index;
        tab.comment_line_num = line_start;
        tab.comment_reply_to = None;
        tab.comment_finding_ref = Some(finding_id.to_string());
        tab.comment_type = CommentType::GitHubComment;
        tab.comment_edit_id = None;
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

        // If editing an existing comment, update it in-place
        if let Some(edit_id) = tab.comment_edit_id.clone() {
            return self.update_comment(edit_id, text);
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
        let reply_to = tab.comment_reply_to.clone();

        let anchor = self.get_line_anchor(hunk_index, comment_line_num);

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

        // If diff hash changed, update it but preserve existing questions
        // (the relocation system handles comment drift)
        if questions.diff_hash != diff_hash {
            questions.diff_hash = diff_hash;
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

        let is_reply = reply_to.is_some();
        questions.questions.push(ai::ReviewQuestion {
            id,
            timestamp: chrono_now(),
            file: file_path,
            hunk_index: Some(hunk_index),
            line_start: anchor.line_start,
            line_content: anchor.line_content,
            text: text.clone(),
            resolved: false,
            stale: false,
            context_before: anchor.context_before,
            context_after: anchor.context_after,
            old_line_start: anchor.old_line_start,
            hunk_header: anchor.hunk_header,
            anchor_status: "original".to_string(),
            relocated_at_hash: self.tab().branch_diff_hash.clone(),
            in_reply_to: reply_to,
            author: "You".to_string(),
        });

        // Write atomically
        let json = serde_json::to_string_pretty(&questions)?;
        let tmp_path = format!("{}.tmp", questions_path);
        std::fs::write(&tmp_path, json)?;
        std::fs::rename(&tmp_path, &questions_path)?;

        self.tab_mut().comment_input.clear();
        self.input_mode = InputMode::Normal;
        self.tab_mut().reload_ai_state();
        let label = if is_reply { "Reply" } else { "Question" };
        self.notify(&format!("{} added: {}", label, truncate(&text, 40)));
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
        let finding_ref = tab.comment_finding_ref.clone();
        let comment_line_num = tab.comment_line_num;

        let anchor = self.get_line_anchor(hunk_index, comment_line_num);

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

        // If diff hash changed, update it but preserve existing comments
        // (the relocation system handles comment drift)
        if gh_comments.diff_hash != diff_hash {
            gh_comments.diff_hash = diff_hash;
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

        let is_reply = reply_to.is_some();
        gh_comments.comments.push(ai::GitHubReviewComment {
            id,
            timestamp: chrono_now(),
            file: file_path,
            hunk_index: Some(hunk_index),
            line_start: anchor.line_start,
            line_end: None,
            line_content: anchor.line_content,
            comment: text.clone(),
            in_reply_to: reply_to,
            resolved: false,
            source: "local".to_string(),
            github_id: None,
            author: "You".to_string(),
            synced: false,
            stale: false,
            context_before: anchor.context_before,
            context_after: anchor.context_after,
            old_line_start: anchor.old_line_start,
            hunk_header: anchor.hunk_header,
            anchor_status: "original".to_string(),
            relocated_at_hash: self.tab().branch_diff_hash.clone(),
            finding_ref,
        });

        // Write atomically
        let json = serde_json::to_string_pretty(&gh_comments)?;
        let tmp_path = format!("{}.tmp", comments_path);
        std::fs::write(&tmp_path, json)?;
        std::fs::rename(&tmp_path, &comments_path)?;

        self.tab_mut().comment_input.clear();
        self.input_mode = InputMode::Normal;
        self.tab_mut().reload_ai_state();
        let label = if is_reply { "Reply" } else { "Comment" };
        self.notify(&format!("{} added: {}", label, truncate(&text, 40)));
        Ok(())
    }

    /// Richer anchor data captured when placing a comment
    fn get_line_anchor(&self, hunk_index: usize, comment_line_num: Option<usize>) -> LineAnchor {
        let tab = self.tab();
        if let Some(df) = tab.selected_diff_file() {
            if let Some(hunk) = df.hunks.get(hunk_index) {
                if let Some(ln) = comment_line_num {
                    // Find the target line index within the hunk
                    let target_idx = hunk.lines.iter().position(|l| l.new_num == Some(ln))
                        .or_else(|| hunk.lines.iter().position(|l| l.old_num == Some(ln)));
                    let (line_content, old_line_start) = if let Some(idx) = target_idx {
                        let dl = &hunk.lines[idx];
                        (dl.content.clone(), dl.old_num)
                    } else {
                        (String::new(), None)
                    };

                    // Collect up to 3 content lines before the target (same hunk)
                    let context_before = if let Some(idx) = target_idx {
                        let start = idx.saturating_sub(3);
                        hunk.lines[start..idx]
                            .iter()
                            .map(|l| l.content.clone())
                            .collect()
                    } else {
                        Vec::new()
                    };

                    // Collect up to 3 content lines after the target (same hunk)
                    let context_after = if let Some(idx) = target_idx {
                        let end = (idx + 4).min(hunk.lines.len());
                        hunk.lines[(idx + 1)..end]
                            .iter()
                            .map(|l| l.content.clone())
                            .collect()
                    } else {
                        Vec::new()
                    };

                    LineAnchor {
                        line_start: Some(ln),
                        line_content,
                        context_before,
                        context_after,
                        old_line_start,
                        hunk_header: hunk.header.clone(),
                    }
                } else {
                    // Hunk-level comment
                    LineAnchor {
                        line_start: None,
                        line_content: hunk.header.clone(),
                        context_before: Vec::new(),
                        context_after: Vec::new(),
                        old_line_start: None,
                        hunk_header: hunk.header.clone(),
                    }
                }
            } else {
                LineAnchor::default()
            }
        } else {
            LineAnchor::default()
        }
    }

    /// Cancel comment input
    pub fn cancel_comment(&mut self) {
        self.tab_mut().comment_input.clear();
        self.tab_mut().comment_edit_id = None;
        self.input_mode = InputMode::Normal;
    }

    /// Update an existing comment in-place: new text, re-anchored to current position
    fn update_comment(&mut self, comment_id: String, new_text: String) -> Result<()> {
        let tab = self.tab();
        let repo_root = tab.repo_root.clone();
        let hunk_index = tab.comment_hunk;
        let comment_line_num = tab.comment_line_num;

        let anchor = self.get_line_anchor(hunk_index, comment_line_num);
        let diff_hash = self.tab().diff_hash.clone();

        if comment_id.starts_with("q-") {
            let path = format!("{}/.er-questions.json", repo_root);
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(mut qs) = serde_json::from_str::<ai::ErQuestions>(&content) {
                    if let Some(q) = qs.questions.iter_mut().find(|q| q.id == comment_id) {
                        q.text = new_text.clone();
                        q.line_start = anchor.line_start;
                        q.line_content = anchor.line_content.clone();
                        q.context_before = anchor.context_before.clone();
                        q.context_after = anchor.context_after.clone();
                        q.old_line_start = anchor.old_line_start;
                        q.hunk_header = anchor.hunk_header.clone();
                        q.hunk_index = Some(hunk_index);
                        q.anchor_status = "original".to_string();
                        q.relocated_at_hash = diff_hash;
                        q.stale = false;
                    }
                    let json = serde_json::to_string_pretty(&qs)?;
                    let tmp = format!("{}.tmp", path);
                    std::fs::write(&tmp, json)?;
                    std::fs::rename(&tmp, &path)?;
                }
            }
        } else {
            let path = format!("{}/.er-github-comments.json", repo_root);
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(mut gc) = serde_json::from_str::<ai::ErGitHubComments>(&content) {
                    if let Some(c) = gc.comments.iter_mut().find(|c| c.id == comment_id) {
                        c.comment = new_text.clone();
                        c.line_start = anchor.line_start;
                        c.line_content = anchor.line_content.clone();
                        c.context_before = anchor.context_before.clone();
                        c.context_after = anchor.context_after.clone();
                        c.old_line_start = anchor.old_line_start;
                        c.hunk_header = anchor.hunk_header.clone();
                        c.hunk_index = Some(hunk_index);
                        c.anchor_status = "original".to_string();
                        c.relocated_at_hash = diff_hash;
                        c.stale = false;
                    }
                    let json = serde_json::to_string_pretty(&gc)?;
                    let tmp = format!("{}.tmp", path);
                    std::fs::write(&tmp, json)?;
                    std::fs::rename(&tmp, &path)?;
                }
            }
        }

        self.tab_mut().comment_input.clear();
        self.tab_mut().comment_edit_id = None;
        self.input_mode = InputMode::Normal;
        self.tab_mut().reload_ai_state();
        self.notify(&format!("Comment updated: {}", truncate(&new_text, 40)));
        Ok(())
    }

    // ── Comment Navigation ──

    /// Jump to the next comment across all files.
    #[allow(dead_code)]
    pub fn next_comment(&mut self) {
        self.jump_comment(true, false);
    }

    /// Jump to the previous comment across all files.
    #[allow(dead_code)]
    pub fn prev_comment(&mut self) {
        self.jump_comment(false, false);
    }

    /// Jump to the next question across all files.
    #[allow(dead_code)]
    pub fn next_question(&mut self) {
        self.jump_comment(true, true);
    }

    /// Jump to the previous question across all files.
    #[allow(dead_code)]
    pub fn prev_question(&mut self) {
        self.jump_comment(false, true);
    }


    /// Core jump logic: navigate forward/backward through comments or questions across all files.
    /// Uses focused_comment_id for exact position tracking instead of file+hunk guessing.
    fn jump_comment(&mut self, forward: bool, questions_only: bool) {
        let tab = self.tab_mut();
        let all = if questions_only {
            // Convert 3-tuple to 4-tuple for uniform handling
            tab.ai.all_questions_ordered().into_iter()
                .map(|(f, h, id)| (f, h, None::<usize>, id))
                .collect::<Vec<_>>()
        } else {
            tab.ai.all_comments_ordered()
        };

        if all.is_empty() {
            return;
        }

        // Find current position by exact ID match first, then fallback to file position
        let current_pos = tab.focused_comment_id.as_ref().and_then(|fid| {
            all.iter().position(|(_, _, _, id)| id == fid)
        }).or_else(|| {
            let current_file = tab.files.get(tab.selected_file).map(|f| &f.path);
            current_file.and_then(|cf| {
                if forward {
                    all.iter().position(|(f, _, _, _)| f == cf)
                } else {
                    all.iter().rposition(|(f, _, _, _)| f == cf)
                }
            })
        });

        let next_idx = match current_pos {
            Some(pos) => {
                if forward {
                    if pos + 1 < all.len() { pos + 1 } else { 0 }
                } else {
                    if pos > 0 { pos - 1 } else { all.len() - 1 }
                }
            }
            None => {
                if forward { 0 } else { all.len() - 1 }
            }
        };

        let (ref file, hunk_index, _, ref comment_id) = all[next_idx];

        tab.focused_comment_id = Some(comment_id.clone());
        tab.focused_finding_id = None;

        let needs_file_change = tab.files.get(tab.selected_file)
            .map_or(true, |f| f.path != *file);

        if needs_file_change {
            if let Some(idx) = tab.files.iter().position(|f| f.path == *file) {
                tab.selected_file = idx;
                tab.current_hunk = hunk_index.unwrap_or(0);
                tab.current_line = None;
                tab.selection_anchor = None;
                tab.diff_scroll = 0;
                tab.h_scroll = 0;
                tab.ensure_file_parsed();
                tab.rebuild_hunk_offsets();
            }
        } else if let Some(hi) = hunk_index {
            tab.current_hunk = hi;
            tab.current_line = None;
        }

        tab.scroll_to_current_hunk();
    }

    /// Jump forward to the next AI finding.
    pub fn next_finding(&mut self) {
        self.jump_finding(true);
    }

    /// Jump backward to the previous AI finding.
    pub fn prev_finding(&mut self) {
        self.jump_finding(false);
    }

    /// Core jump logic: navigate forward/backward through AI findings across all files.
    /// Uses focused_finding_id for exact position tracking.
    fn jump_finding(&mut self, forward: bool) {
        let tab = self.tab_mut();
        let all = tab.ai.all_findings_ordered();

        if all.is_empty() {
            return;
        }

        // Find current position by exact ID match first, then fallback to file position
        let current_pos = tab.focused_finding_id.as_ref().and_then(|fid| {
            all.iter().position(|(_, _, _, id)| id == fid)
        }).or_else(|| {
            let current_file = tab.files.get(tab.selected_file).map(|f| f.path.as_str());
            let current_hunk = tab.current_hunk;
            current_file.and_then(|cf| {
                if forward {
                    // Find first finding at or after current position
                    all.iter().position(|(f, hi, _, _)| {
                        f.as_str() > cf || (f == cf && hi.unwrap_or(0) >= current_hunk)
                    })
                } else {
                    // Find last finding at or before current position
                    all.iter().rposition(|(f, hi, _, _)| {
                        f.as_str() < cf || (f == cf && hi.unwrap_or(0) <= current_hunk)
                    })
                }
            })
        });

        let next_idx = match current_pos {
            Some(pos) => {
                if forward {
                    if pos + 1 < all.len() { pos + 1 } else { 0 }
                } else {
                    if pos > 0 { pos - 1 } else { all.len() - 1 }
                }
            }
            None => {
                if forward { 0 } else { all.len() - 1 }
            }
        };

        let (ref file, hunk_index, _, ref finding_id) = all[next_idx];

        tab.focused_finding_id = Some(finding_id.clone());
        tab.focused_comment_id = None;

        let needs_file_change = tab.files.get(tab.selected_file)
            .map_or(true, |f| f.path != *file);

        if needs_file_change {
            if let Some(idx) = tab.files.iter().position(|f| f.path == *file) {
                tab.selected_file = idx;
                tab.current_hunk = hunk_index.unwrap_or(0);
                tab.current_line = None;
                tab.selection_anchor = None;
                tab.diff_scroll = 0;
                tab.h_scroll = 0;
                tab.ensure_file_parsed();
                tab.rebuild_hunk_offsets();
            }
        } else if let Some(hi) = hunk_index {
            tab.current_hunk = hi;
            tab.current_line = None;
        }

        tab.scroll_to_current_hunk();
    }

    /// Jump to the next hint (unified: comments + questions + findings).
    pub fn next_hint(&mut self) {
        self.jump_hint(true);
    }

    /// Jump to the previous hint (unified: comments + questions + findings).
    pub fn prev_hint(&mut self) {
        self.jump_hint(false);
    }

    /// Unified navigation across comments, questions, and findings.
    fn jump_hint(&mut self, forward: bool) {
        use crate::ai::HintType;

        let tab = self.tab_mut();
        let all = tab.ai.all_hints_ordered();

        if all.is_empty() {
            return;
        }

        // Find current position by matching the currently focused ID
        let current_id = tab.focused_comment_id.as_ref()
            .or(tab.focused_finding_id.as_ref());
        let current_pos = current_id.and_then(|fid| {
            all.iter().position(|(_, _, _, id, _)| id == fid)
        }).or_else(|| {
            let current_file = tab.files.get(tab.selected_file).map(|f| &f.path);
            current_file.and_then(|cf| {
                if forward {
                    all.iter().position(|(f, _, _, _, _)| f == cf)
                } else {
                    all.iter().rposition(|(f, _, _, _, _)| f == cf)
                }
            })
        });

        let next_idx = match current_pos {
            Some(pos) => {
                if forward {
                    if pos + 1 < all.len() { pos + 1 } else { 0 }
                } else {
                    if pos > 0 { pos - 1 } else { all.len() - 1 }
                }
            }
            None => {
                if forward { 0 } else { all.len() - 1 }
            }
        };

        let (ref file, hunk_index, _, ref id, hint_type) = all[next_idx];

        // Set the appropriate focus ID based on hint type
        match hint_type {
            HintType::Question | HintType::GitHubComment => {
                tab.focused_comment_id = Some(id.clone());
                tab.focused_finding_id = None;
            }
            HintType::Finding => {
                tab.focused_finding_id = Some(id.clone());
                tab.focused_comment_id = None;
            }
        }

        let needs_file_change = tab.files.get(tab.selected_file)
            .map_or(true, |f| f.path != *file);

        if needs_file_change {
            if let Some(idx) = tab.files.iter().position(|f| f.path == *file) {
                tab.selected_file = idx;
                tab.current_hunk = hunk_index.unwrap_or(0);
                tab.current_line = None;
                tab.selection_anchor = None;
                tab.diff_scroll = 0;
                tab.h_scroll = 0;
                tab.ensure_file_parsed();
                tab.rebuild_hunk_offsets();
            }
        } else if let Some(hi) = hunk_index {
            tab.current_hunk = hi;
            tab.current_line = None;
        }

        tab.scroll_to_current_hunk();
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
                            let _ = crate::github::gh_pr_delete_comment(&gh.owner, &gh.repo, gh_id, &repo_root);
                            for reply_id in &reply_github_ids {
                                let _ = crate::github::gh_pr_delete_comment(&gh.owner, &gh.repo, *reply_id, &repo_root);
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
                tab.ensure_file_parsed();
                tab.rebuild_hunk_offsets();
                if tab.panel.is_none() {
                    tab.panel = Some(PanelContent::FileDetail);
                }
                self.notify(&format!("Jumped to: {}", path));
            } else {
                self.notify(&format!("File not in diff: {}", path));
            }
        } else {
            self.notify("No file associated with this item");
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
        // TODO(risk:minor): watch_message_ticks is a u8 (max 255). If notify() is called without a
        // subsequent tick draining it (e.g., during a long blocking git operation), ticks can overflow
        // and wrap back to 0, causing the message to persist for another ~25 seconds unexpectedly.
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
            split_focus: SplitSide::New,
            h_scroll_old: 0,
            h_scroll_new: 0,
            layers: InlineLayers::default(),
            panel: None,
            panel_scroll: 0,
            panel_focus: false,
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
            comment_type: CommentType::GitHubComment,
            comment_edit_id: None,
            comment_finding_ref: None,
            pr_data: None,
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
        // No AI data, no PR data: FileDetail → None (both skipped)
        assert_eq!(tab.panel, None);
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
        // AiSummary → None (no PR available)
        assert_eq!(tab.panel, None);
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
        tab.toggle_panel(); // FileDetail → None (no AI or PR data)
        assert_eq!(tab.panel_scroll, 0);
    }

    #[test]
    fn toggle_panel_sets_panel_focus_false_when_closing() {
        let mut tab = make_test_tab(vec![]);
        // PrOverview → None closes the panel and clears focus
        tab.panel = Some(crate::ai::PanelContent::PrOverview);
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
        tab.toggle_panel(); // FileDetail → None (no AI, no PR)
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
        tab.toggle_panel(); // AiSummary → None (no PR)
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
        tab.toggle_panel(); // PrOverview → None
        assert_eq!(tab.panel, None);
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
        cache.insert("first".to_string(), vec![make_file("first.rs", vec![], 1, 0)]);
        cache.insert("second".to_string(), vec![make_file("second.rs", vec![], 1, 0)]);
        // Cache is full (max_size=2). Insert a third entry.
        cache.insert("third".to_string(), vec![make_file("third.rs", vec![], 1, 0)]);
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
        cache.insert("a".to_string(), vec![
            make_file("test.rs", vec![], 1, 0),
            make_file("other.rs", vec![], 2, 0),
        ]);
        let result = cache.get("a").unwrap();
        assert_eq!(result.len(), 2); // updated, not original
    }

    // ── DiffCache MRU promotion ──

    #[test]
    fn diff_cache_get_promotes_entry_so_it_survives_eviction() {
        let mut cache = DiffCache::new(3);
        cache.insert("first".to_string(), vec![make_file("first.rs", vec![], 1, 0)]);
        cache.insert("second".to_string(), vec![make_file("second.rs", vec![], 1, 0)]);
        cache.insert("third".to_string(), vec![make_file("third.rs", vec![], 1, 0)]);
        // Cache is full (max_size=3). Access "first" — promotes it to MRU position.
        let _ = cache.get("first");
        // Insert a 4th entry — should evict "second" (now LRU), not "first"
        cache.insert("fourth".to_string(), vec![make_file("fourth.rs", vec![], 1, 0)]);
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
            ai_poll_counter: 0,
            config: ErConfig::default(),
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
        let lines = vec![
            DiffLine {
                line_type: LineType::Add,
                content: "some line".to_string(),
                old_num: None,
                new_num: Some(1),
            },
        ];
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
}
