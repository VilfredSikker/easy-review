use crate::git::{self, DiffFile, Worktree};
use anyhow::{Context, Result};
use std::collections::HashSet;
use std::io::Write;

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

/// Whether we're navigating or typing in the search filter
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputMode {
    Normal,
    Search,
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
            diff_scroll: 0,
            h_scroll: 0,
            search_query: String::new(),
            reviewed,
            show_unreviewed_only: false,
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
                self.diff_scroll = 0;
                self.h_scroll = 0;
            }
        } else {
            // Current selection not in visible set — snap to first
            self.selected_file = visible[0].0;
            self.current_hunk = 0;
            self.diff_scroll = 0;
            self.h_scroll = 0;
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
                self.diff_scroll = 0;
                self.h_scroll = 0;
            }
        } else {
            // Current selection not in visible set — snap to first
            self.selected_file = visible[0].0;
            self.current_hunk = 0;
            self.diff_scroll = 0;
            self.h_scroll = 0;
        }
    }

    pub fn next_hunk(&mut self) {
        let total = self.total_hunks();
        if total > 0 && self.current_hunk < total - 1 {
            self.current_hunk += 1;
            self.scroll_to_current_hunk();
        }
    }

    pub fn prev_hunk(&mut self) {
        if self.current_hunk > 0 {
            self.current_hunk -= 1;
            self.scroll_to_current_hunk();
        }
    }

    fn scroll_to_current_hunk(&mut self) {
        if let Some(file) = self.selected_diff_file() {
            let mut line_offset: u16 = 2;
            for (i, hunk) in file.hunks.iter().enumerate() {
                if i == self.current_hunk {
                    self.diff_scroll = line_offset.saturating_sub(1);
                    return;
                }
                line_offset += 1 + hunk.lines.len() as u16 + 1;
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
            self.diff_scroll = 0;
            let _ = self.refresh_diff();
        }
    }

    fn clamp_hunk(&mut self) {
        let total = self.total_hunks();
        if total == 0 {
            self.current_hunk = 0;
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
