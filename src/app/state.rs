use crate::ai::{self, AiState, ReviewFocus, ViewMode};
use crate::git::{self, CommitInfo, DiffFile, Worktree};
use anyhow::{Context, Result};
use std::collections::{HashSet, VecDeque};
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};

static COMMENT_SEQ: AtomicU64 = AtomicU64::new(0);

// ── Enums ──

/// Which set of changes we're viewing
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DiffMode {
    Branch,
    Unstaged,
    Staged,
    History,
}

impl DiffMode {
    #[allow(dead_code)]
    pub fn label(&self) -> &'static str {
        match self {
            DiffMode::Branch => "BRANCH DIFF",
            DiffMode::Unstaged => "UNSTAGED",
            DiffMode::Staged => "STAGED",
            DiffMode::History => "HISTORY",
        }
    }

    pub fn git_mode(&self) -> &'static str {
        match self {
            DiffMode::Branch => "branch",
            DiffMode::Unstaged => "unstaged",
            DiffMode::Staged => "staged",
            DiffMode::History => "history",
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

    pub fn get(&self, hash: &str) -> Option<&Vec<DiffFile>> {
        self.entries.iter().find(|(h, _)| h == hash).map(|(_, f)| f)
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
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputMode {
    Normal,
    Search,
    Comment,
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

    /// Vertical scroll offset within the diff view
    pub diff_scroll: u16,

    /// Horizontal scroll offset within the diff view (for long lines)
    pub h_scroll: u16,

    /// Vertical scroll offset for the AI side panel (independent of diff_scroll)
    pub ai_panel_scroll: u16,

    /// Search/filter input
    pub search_query: String,

    /// Files marked as reviewed (paths relative to repo root)
    pub reviewed: HashSet<String>,

    /// Only show unreviewed files in the file tree
    pub show_unreviewed_only: bool,

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

    /// History mode state (only populated when mode == History)
    pub history: Option<HistoryState>,
}

impl TabState {
    /// Create a new tab for a given repo root
    pub fn new(repo_root: String) -> Result<Self> {
        let current_branch = git::get_current_branch_in(&repo_root)?;
        let base_branch = git::detect_base_branch_in(&repo_root)?;
        let reviewed = Self::load_reviewed_files(&repo_root);

        let mut tab = TabState {
            mode: DiffMode::Branch,
            base_branch,
            current_branch,
            repo_root,
            files: Vec::new(),
            selected_file: 0,
            current_hunk: 0,
            current_line: None,
            diff_scroll: 0,
            h_scroll: 0,
            ai_panel_scroll: 0,
            search_query: String::new(),
            reviewed,
            show_unreviewed_only: false,
            ai: AiState::default(),
            diff_hash: String::new(),
            branch_diff_hash: String::new(),
            last_ai_check: None,
            comment_input: String::new(),
            comment_file: String::new(),
            comment_hunk: 0,
            comment_reply_to: None,
            comment_line_num: None,
            history: None,
        };

        tab.refresh_diff()?;
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
        // History mode doesn't use git_diff_raw — skip normal diff refresh
        if self.mode == DiffMode::History {
            return Ok(());
        }
        let raw = git::git_diff_raw(self.mode.git_mode(), &self.base_branch, &self.repo_root)?;
        self.files = git::parse_diff(&raw);

        // Compute diff hash for the current mode
        self.diff_hash = ai::compute_diff_hash(&raw);

        // Branch diff hash for AI staleness detection.
        // In Branch mode it's always the same as diff_hash (free).
        // In other modes, only recompute on explicit actions — watch events
        // skip this to avoid running two git diffs per file save.
        if self.mode == DiffMode::Branch {
            self.branch_diff_hash = self.diff_hash.clone();
        } else if recompute_branch_hash {
            let branch_raw = git::git_diff_raw("branch", &self.base_branch, &self.repo_root)?;
            self.branch_diff_hash = ai::compute_diff_hash(&branch_raw);
        }

        // Load AI state from .er-* files
        self.reload_ai_state();

        // Compute per-file staleness when the review is stale and has file_hashes.
        // Use the branch diff for comparison (AI reviews are generated against branch diff).
        if self.ai.is_stale {
            let branch_raw = if self.mode == DiffMode::Branch {
                Some(raw.as_str())
            } else if recompute_branch_hash {
                // branch_raw was already fetched above for hash computation — re-fetch for per-file
                // (stored in branch_diff_hash; re-running git diff here is acceptable since
                // recompute_branch_hash is only true on explicit actions, not watch events)
                None
            } else {
                None
            };
            if let Some(branch_diff) = branch_raw {
                self.compute_stale_files(branch_diff);
            } else if recompute_branch_hash {
                // Non-branch mode with explicit refresh: fetch branch diff for per-file staleness
                if let Ok(branch_raw) =
                    git::git_diff_raw("branch", &self.base_branch, &self.repo_root)
                {
                    self.compute_stale_files(&branch_raw);
                }
            }
        }

        // Clamp selection
        if self.selected_file >= self.files.len() && !self.files.is_empty() {
            self.selected_file = self.files.len() - 1;
        }
        if self.files.is_empty() {
            self.selected_file = 0;
        }
        self.clamp_hunk();
        self.diff_scroll = 0;

        Ok(())
    }

    /// Reload AI state from .er-* files (preserving current view/nav state)
    pub fn reload_ai_state(&mut self) {
        let current_mode = self.ai.view_mode;
        let current_focus = self.ai.review_focus;
        let current_cursor = self.ai.review_cursor;
        let prev_stale_files = std::mem::take(&mut self.ai.stale_files);
        self.ai = ai::load_ai_state(&self.repo_root, &self.branch_diff_hash);
        self.ai.view_mode = current_mode;
        self.ai.review_focus = current_focus;
        self.ai.review_cursor = current_cursor;
        // Preserve per-file staleness across .er-* file reloads (recomputed in refresh_diff)
        if self.ai.is_stale {
            self.ai.stale_files = prev_stale_files;
        }
        // Clamp cursor to valid range after reload (item count may have decreased)
        let item_count = match current_focus {
            ReviewFocus::Files => self.ai.review_file_count(),
            ReviewFocus::Checklist => self.ai.review_checklist_count(),
        };
        // cursor 0 on empty is safe — all access methods are bounds-checked
        let max_cursor = if item_count == 0 { 0 } else { item_count - 1 };
        self.ai.review_cursor = self.ai.review_cursor.min(max_cursor);
        // If the current mode requires AI data that's not available, fall back
        if self.ai.view_mode != ViewMode::Default && !self.ai.overlay_available() {
            self.ai.view_mode = ViewMode::Default;
        }
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

    /// Get the list of files, filtered by search query and reviewed status
    pub fn visible_files(&self) -> Vec<(usize, &DiffFile)> {
        let mut visible: Vec<(usize, &DiffFile)> = if self.search_query.is_empty() {
            self.files.iter().enumerate().collect()
        } else {
            let q = self.search_query.to_lowercase();
            self.files
                .iter()
                .enumerate()
                .filter(|(_, f)| f.path.to_lowercase().contains(&q))
                .collect()
        };

        if self.show_unreviewed_only {
            visible.retain(|(_, f)| !self.reviewed.contains(&f.path));
        }

        visible
    }

    /// Get the currently selected file
    pub fn selected_diff_file(&self) -> Option<&DiffFile> {
        self.files.get(self.selected_file)
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

    pub fn next_file(&mut self) {
        let visible = self.visible_files();
        if visible.is_empty() {
            return;
        }
        if let Some(pos) = visible.iter().position(|(i, _)| *i == self.selected_file) {
            if pos + 1 < visible.len() {
                self.selected_file = visible[pos + 1].0;
                self.current_hunk = 0;
                self.current_line = None;
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
        }
    }

    pub fn prev_file(&mut self) {
        let visible = self.visible_files();
        if visible.is_empty() {
            return;
        }
        if let Some(pos) = visible.iter().position(|(i, _)| *i == self.selected_file) {
            if pos > 0 {
                self.selected_file = visible[pos - 1].0;
                self.current_hunk = 0;
                self.current_line = None;
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
        }
    }

    pub fn next_hunk(&mut self) {
        let total = self.total_hunks();
        if total > 0 && self.current_hunk < total - 1 {
            self.current_hunk += 1;
            self.current_line = None;
            self.scroll_to_current_hunk();
        }
    }

    pub fn prev_hunk(&mut self) {
        if self.current_hunk > 0 {
            self.current_hunk -= 1;
            self.current_line = None;
            self.scroll_to_current_hunk();
        }
    }

    /// Move to the next line within the current hunk (arrow down)
    pub fn next_line(&mut self) {
        let total_lines = self.current_hunk_line_count();
        if total_lines == 0 {
            return;
        }
        match self.current_line {
            None => {
                // Enter line mode at first line
                self.current_line = Some(0);
                self.scroll_to_current_hunk();
            }
            Some(line) => {
                if line + 1 < total_lines {
                    self.current_line = Some(line + 1);
                    self.scroll_to_current_hunk();
                } else {
                    // At last line of this hunk — move to next hunk's first line
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
                // At first line — move to prev hunk's last line
                if self.current_hunk > 0 {
                    self.current_hunk -= 1;
                    let count = self.current_hunk_line_count();
                    self.current_line = if count > 0 { Some(count - 1) } else { None };
                    self.scroll_to_current_hunk();
                } else {
                    // Already at top — exit line mode
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
    fn current_hunk_line_count(&self) -> usize {
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

    fn scroll_to_current_hunk(&mut self) {
        // Note: In Overlay mode, scroll position is approximate — banner lines
        // (AI findings, comments) inserted by the renderer inflate the actual
        // offset but aren't counted here. Acceptable for v1.
        if let Some(file) = self.selected_diff_file() {
            let mut line_offset: usize = 2;
            for (i, hunk) in file.hunks.iter().enumerate() {
                if i == self.current_hunk {
                    // Account for current line position within the hunk
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
    }

    pub fn scroll_up(&mut self, amount: u16) {
        self.diff_scroll = self.diff_scroll.saturating_sub(amount);
    }

    pub fn scroll_right(&mut self, amount: u16) {
        self.h_scroll = self.h_scroll.saturating_add(amount);
    }

    pub fn scroll_left(&mut self, amount: u16) {
        self.h_scroll = self.h_scroll.saturating_sub(amount);
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
            let history = match self.history.as_ref() {
                Some(h) => h,
                None => return,
            };
            let commit = match history.commits.get(history.selected_commit) {
                Some(c) => c,
                None => return,
            };
            // Check cache first
            if let Some(cached) = history.diff_cache.get(&commit.hash) {
                let files = cached.clone();
                let history = self.history.as_mut().unwrap();
                history.commit_files = files;
                history.selected_file = 0;
                history.current_hunk = 0;
                history.current_line = None;
                history.diff_scroll = 0;
                history.h_scroll = 0;
                return;
            }
            (commit.hash.clone(), self.repo_root.clone())
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
            } else {
                self.selected_file = 0;
                self.current_hunk = 0;
                self.current_line = None;
                self.diff_scroll = 0;
                let _ = self.refresh_diff();
            }
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

    // ── Reviewed-File Tracking ──

    /// Count of reviewed files vs total
    pub fn reviewed_count(&self) -> (usize, usize) {
        let total = self.files.len();
        let reviewed = self.files.iter().filter(|f| self.reviewed.contains(&f.path)).count();
        (reviewed, total)
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

                    let mut tab = TabState::new(repo_root)?;
                    tab.base_branch = base;
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
            None => {}
        }
    }

    pub fn overlay_prev(&mut self) {
        match &mut self.overlay {
            Some(OverlayData::WorktreePicker { selected, .. })
            | Some(OverlayData::DirectoryBrowser { selected, .. }) => {
                if *selected > 0 {
                    *selected -= 1;
                }
            }
            None => {}
        }
    }

    /// Handle Enter in an overlay — opens selection in a new tab
    pub fn overlay_select(&mut self) -> Result<()> {
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

    /// Close the overlay
    pub fn overlay_close(&mut self) {
        self.overlay = None;
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
            DiffMode::History => {
                self.notify("Staging not available in History mode");
                return Ok(());
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
    pub fn start_comment(&mut self) {
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
        self.input_mode = InputMode::Comment;
    }

    /// Submit the current comment: append to .er-feedback.json
    pub fn submit_comment(&mut self) -> Result<()> {
        let tab = self.tab();
        let text = tab.comment_input.trim().to_string();
        if text.is_empty() {
            self.input_mode = InputMode::Normal;
            return Ok(());
        }

        let repo_root = tab.repo_root.clone();
        let diff_hash = tab.branch_diff_hash.clone();
        let file_path = tab.comment_file.clone();
        let hunk_index = tab.comment_hunk;
        let reply_to = tab.comment_reply_to.clone();

        // Get line context — use specific line if available, else hunk start
        let comment_line_num = tab.comment_line_num;
        let (line_start, line_content) = {
            let tab = self.tab();
            if let Some(df) = tab.selected_diff_file() {
                if let Some(hunk) = df.hunks.get(hunk_index) {
                    if let Some(ln) = comment_line_num {
                        // Line-level comment: find the content at that line
                        let content = hunk.lines.iter()
                            .find(|l| l.new_num == Some(ln))
                            .map(|l| l.content.clone())
                            .unwrap_or_default();
                        (Some(ln), content)
                    } else {
                        // Hunk-level comment: hunk_index identifies the hunk; line_start is not meaningful here
                        (None, hunk.header.clone())
                    }
                } else {
                    (None, String::new())
                }
            } else {
                (None, String::new())
            }
        };

        // Load existing feedback or create new
        let feedback_path = format!("{}/.er-feedback.json", repo_root);
        let mut feedback: ai::ErFeedback = match std::fs::read_to_string(&feedback_path) {
            Ok(content) => match serde_json::from_str(&content) {
                Ok(fb) => fb,
                Err(_) => {
                    self.notify("Warning: .er-feedback.json is invalid JSON — starting fresh");
                    ai::ErFeedback {
                        version: 1,
                        diff_hash: diff_hash.clone(),
                        comments: Vec::new(),
                    }
                }
            },
            Err(_) => ai::ErFeedback {
                version: 1,
                diff_hash: diff_hash.clone(),
                comments: Vec::new(),
            },
        };

        // If diff hash changed, start fresh
        if feedback.diff_hash != diff_hash {
            feedback.diff_hash = diff_hash;
            feedback.comments.clear();
        }

        // Generate comment ID using epoch millis + sequence counter to avoid collisions
        let seq = COMMENT_SEQ.fetch_add(1, Ordering::Relaxed);
        let comment_id = format!(
            "fb-{}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0),
            seq
        );

        // Create the comment
        let comment = ai::FeedbackComment {
            id: comment_id,
            timestamp: chrono_now(),
            file: file_path,
            hunk_index: Some(hunk_index),
            line_start,
            line_end: None,
            line_content,
            comment: text.clone(),
            in_reply_to: reply_to,
            resolved: false,
        };

        feedback.comments.push(comment);

        // Write atomically via temp file + rename
        let json = serde_json::to_string_pretty(&feedback)?;
        let tmp_path = format!("{}.tmp", feedback_path);
        std::fs::write(&tmp_path, json)?;
        std::fs::rename(&tmp_path, &feedback_path)?;

        // Clear state and return to normal
        self.tab_mut().comment_input.clear();
        self.input_mode = InputMode::Normal;

        // Reload AI state to pick up the new feedback
        self.tab_mut().reload_ai_state();

        self.notify(&format!("Comment added: {}", truncate(&text, 40)));
        Ok(())
    }

    /// Cancel comment input
    pub fn cancel_comment(&mut self) {
        self.tab_mut().comment_input.clear();
        self.input_mode = InputMode::Normal;
    }

    // ── AiReview Navigation ──

    /// Jump from AiReview to the selected file in SidePanel mode
    pub fn review_jump_to_file(&mut self) {
        let file_path = {
            let ai = &self.tab().ai;
            match ai.review_focus {
                ReviewFocus::Files => ai.review_file_at(ai.review_cursor),
                ReviewFocus::Checklist => ai.checklist_file_at(ai.review_cursor),
            }
        };

        if let Some(path) = file_path {
            // Find the file index in the file list
            let file_idx = self.tab().files.iter().position(|f| f.path == path);
            if let Some(idx) = file_idx {
                let tab = self.tab_mut();
                tab.selected_file = idx;
                tab.current_hunk = 0;
                tab.current_line = None;
                tab.diff_scroll = 0;
                tab.h_scroll = 0;
                tab.ai.view_mode = ViewMode::SidePanel;
                self.notify(&format!("Jumped to: {}", path));
            } else {
                self.notify(&format!("File not in diff: {}", path));
            }
        }
    }

    /// Toggle the checklist item at cursor and persist to .er-checklist.json
    pub fn review_toggle_checklist(&mut self) -> Result<()> {
        let tab = self.tab_mut();
        if tab.ai.review_focus != ReviewFocus::Checklist {
            return Ok(());
        }

        let cursor = tab.ai.review_cursor;
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
fn chrono_now() -> String {
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
        TabState {
            mode: DiffMode::Branch,
            base_branch: "main".to_string(),
            current_branch: "feature".to_string(),
            repo_root: "/tmp/test".to_string(),
            files,
            selected_file: 0,
            current_hunk: 0,
            current_line: None,
            diff_scroll: 0,
            h_scroll: 0,
            ai_panel_scroll: 0,
            search_query: String::new(),
            reviewed: HashSet::new(),
            show_unreviewed_only: false,
            ai: AiState::default(),
            diff_hash: String::new(),
            branch_diff_hash: String::new(),
            last_ai_check: None,
            comment_input: String::new(),
            comment_file: String::new(),
            comment_hunk: 0,
            comment_reply_to: None,
            comment_line_num: None,
            history: None,
        }
    }

    fn make_file(path: &str, hunks: Vec<DiffHunk>, adds: usize, dels: usize) -> DiffFile {
        DiffFile {
            path: path.to_string(),
            status: FileStatus::Modified,
            hunks,
            adds,
            dels,
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
        assert_eq!(DiffMode::History.label(), "HISTORY");
    }

    #[test]
    fn diff_mode_git_mode_returns_correct_strings() {
        assert_eq!(DiffMode::Branch.git_mode(), "branch");
        assert_eq!(DiffMode::Unstaged.git_mode(), "unstaged");
        assert_eq!(DiffMode::Staged.git_mode(), "staged");
        assert_eq!(DiffMode::History.git_mode(), "history");
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
}
