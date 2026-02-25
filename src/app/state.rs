use crate::ai::agent::{
    self, AgentContext, AgentConfigInner, AgentMessage, MessageRole,
};
use crate::ai::{self, AiState, ReviewFocus, ViewMode};
use crate::git::{self, DiffFile, Worktree};
use anyhow::{Context, Result};
use std::collections::HashSet;
use std::io::Write;
use std::process::{Command, Stdio};

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

/// Whether we're navigating or typing in the search filter / comment / agent prompt
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputMode {
    Normal,
    Search,
    Comment,
    AgentPrompt,
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
            search_query: String::new(),
            reviewed,
            show_unreviewed_only: false,
            ai: AiState::default(),
            diff_hash: String::new(),
            last_ai_check: None,
            comment_input: String::new(),
            comment_file: String::new(),
            comment_hunk: 0,
            comment_reply_to: None,
            comment_line_num: None,
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
        let raw = git::git_diff_raw(self.mode.git_mode(), &self.base_branch, &self.repo_root)?;
        self.files = git::parse_diff(&raw);

        // Compute diff hash for staleness detection
        self.diff_hash = ai::compute_diff_hash(&raw);

        // Load AI state from .er-* files
        self.reload_ai_state();

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
        let current_panel_tab = self.ai.panel_tab;
        // Take ownership of agent state to preserve across reload
        let current_agent = std::mem::replace(&mut self.ai.agent, agent::AgentState::new());
        self.ai = ai::load_ai_state(&self.repo_root, &self.diff_hash);
        self.ai.view_mode = current_mode;
        self.ai.review_focus = current_focus;
        self.ai.review_cursor = current_cursor;
        self.ai.panel_tab = current_panel_tab;
        self.ai.agent = current_agent;
        // Clamp cursor to valid range after reload (item count may have decreased)
        let item_count = match current_focus {
            ReviewFocus::Files => self.ai.review_file_count(),
            ReviewFocus::Checklist => self.ai.review_checklist_count(),
        };
        let max_cursor = item_count.saturating_sub(1);
        if self.ai.review_cursor > max_cursor {
            self.ai.review_cursor = max_cursor;
        }
        // If the current mode requires AI data that's not available, fall back
        if self.ai.view_mode != ViewMode::Default && !self.ai.overlay_available() {
            self.ai.view_mode = ViewMode::Default;
        }
        self.last_ai_check = Some(std::time::SystemTime::now());
    }

    /// Check if .er-* files have been updated since last load (called on tick)
    pub fn check_ai_files_changed(&mut self) -> bool {
        let latest_mtime = match ai::latest_er_mtime(&self.repo_root) {
            Some(t) => t,
            None => return false,
        };

        if let Some(last_check) = self.last_ai_check {
            if latest_mtime > last_check {
                self.reload_ai_state();
                return true;
            }
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
            }
        } else {
            self.selected_file = visible[0].0;
            self.current_hunk = 0;
            self.diff_scroll = 0;
            self.h_scroll = 0;
        }
        self.update_agent_context();
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
            }
        } else {
            self.selected_file = visible[0].0;
            self.current_hunk = 0;
            self.diff_scroll = 0;
            self.h_scroll = 0;
        }
        self.update_agent_context();
    }

    pub fn next_hunk(&mut self) {
        let total = self.total_hunks();
        if total > 0 && self.current_hunk < total - 1 {
            self.current_hunk += 1;
            self.current_line = None;
            self.scroll_to_current_hunk();
        }
        self.update_agent_context();
    }

    pub fn prev_hunk(&mut self) {
        if self.current_hunk > 0 {
            self.current_hunk -= 1;
            self.current_line = None;
            self.scroll_to_current_hunk();
        }
        self.update_agent_context();
    }

    /// Move to the next line within the current hunk (arrow down)
    pub fn next_line(&mut self) {
        let total_lines = self.current_hunk_line_count();
        if total_lines == 0 {
            return;
        }
        match self.current_line {
            None => {
                self.current_line = Some(0);
            }
            Some(line) => {
                if line + 1 < total_lines {
                    self.current_line = Some(line + 1);
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
        self.update_agent_context();
    }

    /// Move to the previous line within the current hunk (arrow up)
    pub fn prev_line(&mut self) {
        match self.current_line {
            None => {}
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
            }
        }
        self.update_agent_context();
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
        if let Some(file) = self.selected_diff_file() {
            let mut line_offset: usize = 2;
            for (i, hunk) in file.hunks.iter().enumerate() {
                if i == self.current_hunk {
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

    // ── Mode ──

    pub fn set_mode(&mut self, mode: DiffMode) {
        if self.mode != mode {
            self.mode = mode;
            self.selected_file = 0;
            self.current_hunk = 0;
            self.current_line = None;
            self.diff_scroll = 0;
            let _ = self.refresh_diff();
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

    // ── Agent ──

    /// Rebuild AgentContext from current navigation state
    pub fn update_agent_context(&mut self) {
        let ctx = &mut self.ai.agent.context;

        ctx.base_branch = self.base_branch.clone();
        ctx.head_branch = self.current_branch.clone();

        let file = match self.files.get(self.selected_file) {
            Some(f) => f,
            None => {
                *ctx = AgentContext {
                    base_branch: ctx.base_branch.clone(),
                    head_branch: ctx.head_branch.clone(),
                    ..Default::default()
                };
                return;
            }
        };
        ctx.file = Some(file.path.clone());

        if let Some(hunk) = file.hunks.get(self.current_hunk) {
            ctx.hunk_index = Some(self.current_hunk);
            ctx.hunk_header = Some(hunk.header.clone());
            ctx.hunk_diff = Some(
                hunk.lines
                    .iter()
                    .map(|l| l.content.as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
            );

            if let Some(line_idx) = self.current_line {
                if let Some(line) = hunk.lines.get(line_idx) {
                    ctx.line_number = line.new_num;
                    ctx.line_content = Some(line.content.clone());
                } else {
                    ctx.line_number = None;
                    ctx.line_content = None;
                }
            } else {
                ctx.line_number = None;
                ctx.line_content = None;
            }
        } else {
            ctx.hunk_index = None;
            ctx.hunk_header = None;
            ctx.hunk_diff = None;
            ctx.line_number = None;
            ctx.line_content = None;
        }

        // Finding from AI review data
        let finding = self
            .ai
            .review
            .as_ref()
            .and_then(|r| r.files.get(&file.path))
            .and_then(|f| {
                f.findings
                    .iter()
                    .find(|f| f.hunk_index == ctx.hunk_index)
            });

        if let Some(f) = finding {
            ctx.finding_title = Some(f.title.clone());
            ctx.finding_severity = Some(format!("{:?}", f.severity));
            ctx.finding_description = Some(f.description.clone());
            ctx.finding_suggestion = Some(f.suggestion.clone());
        } else {
            ctx.finding_title = None;
            ctx.finding_severity = None;
            ctx.finding_description = None;
            ctx.finding_suggestion = None;
        }
    }

    /// Spawn an agent process with the current prompt and context
    pub fn spawn_agent(&mut self, config: &AgentConfigInner) -> Result<()> {
        let prompt = self.ai.agent.input.trim().to_string();
        if prompt.is_empty() {
            return Ok(());
        }

        // Expand slash commands
        let prompt = agent::expand_slash_command(&prompt).unwrap_or(prompt);

        // Write context to temp file
        let ctx_path = format!("{}/.er-agent-context.json", self.repo_root);
        let ctx_json = serde_json::to_string_pretty(&self.ai.agent.context)?;
        std::fs::write(&ctx_path, &ctx_json)?;

        // Build full prompt with context preamble
        let full_prompt = agent::build_full_prompt(&prompt, &self.ai.agent.context);

        // Resolve command + args from config
        let (cmd, args) =
            agent::resolve_command(config, &full_prompt, &ctx_path, &self.ai.agent.context);

        // Spawn child process
        let child = Command::new(&cmd)
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .current_dir(&self.repo_root)
            .spawn()
            .with_context(|| format!("Failed to run agent: {cmd}"))?;

        // Set stdout to non-blocking
        #[cfg(unix)]
        if let Some(ref stdout) = child.stdout {
            use std::os::unix::io::AsRawFd;
            set_nonblocking(stdout.as_raw_fd());
        }

        // Record user message
        self.ai.agent.messages.push(AgentMessage {
            role: MessageRole::User,
            text: prompt,
            timestamp: chrono::Utc::now().to_rfc3339(),
        });
        self.ai.agent.child = Some(child);
        self.ai.agent.is_running = true;
        self.ai.agent.partial_response.clear();
        self.ai.agent.input.clear();

        Ok(())
    }
}

#[cfg(unix)]
fn set_nonblocking(fd: i32) {
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
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
}

impl App {
    pub fn new() -> Result<Self> {
        let repo_root = git::get_repo_root()?;
        let tab = TabState::new(repo_root)?;

        Ok(App {
            tabs: vec![tab],
            active_tab: 0,
            input_mode: InputMode::Normal,
            should_quit: false,
            overlay: None,
            watching: false,
            watch_message: None,
            watch_message_ticks: 0,
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
        let diff_hash = tab.diff_hash.clone();
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
                        // Hunk-level comment
                        (Some(hunk.new_start as usize), hunk.header.clone())
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

        // Generate comment ID using epoch millis to avoid collisions after clear()
        let comment_id = format!(
            "fb-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
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

        // Write the file
        let json = serde_json::to_string_pretty(&feedback)?;
        std::fs::write(&feedback_path, json)?;

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

        // Persist to disk
        if let Some(ref checklist) = tab.ai.checklist {
            let checklist_path = format!("{}/.er-checklist.json", tab.repo_root);
            let json = serde_json::to_string_pretty(checklist)?;
            std::fs::write(&checklist_path, json)?;
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
    let mut d = days as i64;
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

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y, m + 1, d + 1, hours, minutes, seconds
    )
}

/// Truncate a string to max_len chars, adding … if truncated
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max_len.saturating_sub(1)).collect();
    format!("{}…", truncated)
}
