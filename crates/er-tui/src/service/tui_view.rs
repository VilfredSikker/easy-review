//! The flag-on TUI's own view state, and the frame drawn from it.
//!
//! The service publishes each tab's content; what the user selected, scrolled
//! or typed stays here, per tab key, as the desktop keeps its own in
//! desktop-ui (ADR 0068). Each frame fills the `Screen` from the active tab's
//! content and this state, and calls no builder: an area a builder computes
//! draws the unported message until the entry that ports it.

use std::collections::HashMap;
use std::time::SystemTime;

use er_engine::ai::{CommentType, InlineLayers, PanelContent, ReviewFocus};
use er_engine::config::{ErConfig, FileKindRepoConfig, RiskRules};
use er_engine::git::{DiffFile, DiffHunk, LineType, ProdDiffStats};
use er_engine::navigation::{self, FileMove, LineCursor, LineMove};
use er_engine::tab_identity::DiffMode;
use er_engine::visible_files::{visible_files, FileListView};
use er_engine::TabKey;
use er_service::TabContent;
use tui_textarea::TextArea;

use crate::screen::view::{InputMode, SplitSide};
use crate::screen::{Chrome, Screen, SectionData, TabData, TabView};

/// What the flag-on loop shows: per tab its view state, and app wide the
/// input mode and the status line. The overlay joins with the first entry
/// that opens one on this path.
#[derive(Default)]
pub struct TuiView {
    tabs: HashMap<TabKey, TabViewState>,
    pub input_mode: Mode,
    pub notification: Option<String>,
}

/// The input modes the flag-on path can be in. Only Normal has a service
/// route yet.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Normal,
}

impl TuiView {
    /// The view state of `key`'s tab. Made on its first frame, scrolled to
    /// the first hunk as a load leaves it.
    pub fn tab_mut(&mut self, key: TabKey, content: &TabContent) -> &mut TabViewState {
        self.tabs.entry(key).or_insert_with(|| {
            let mut state = TabViewState::default();
            state.scroll_to_cursor(content.diff.files.first().map(|file| &**file));
            state
        })
    }

    pub fn tab(&self, key: TabKey) -> Option<&TabViewState> {
        self.tabs.get(&key)
    }
}

/// One tab's view state: what is selected, scrolled, shown and typed.
pub struct TabViewState {
    pub selected_file: usize,
    pub current_hunk: usize,
    pub current_line: Option<usize>,
    pub diff_scroll: u16,
    pub h_scroll: u16,
    pub h_scroll_old: u16,
    pub h_scroll_new: u16,
    pub panel: Option<PanelContent>,
    pub panel_focus: bool,
    pub panel_scroll: u16,
    /// The layer toggles and the Gate.
    pub layers: InlineLayers,
    pub filter_expr: String,
    pub search_query: String,
    pub show_unreviewed_only: bool,
    pub sort_by_mtime: bool,
    pub file_tree_width: u16,
    pub panel_width: u16,
    pub draft: Draft,
    pub filter_input: String,
    pub commit_input: String,
    context_overrides: HashMap<String, usize>,
    mtime_cache: HashMap<String, SystemTime>,
}

impl Default for TabViewState {
    fn default() -> Self {
        Self {
            selected_file: 0,
            current_hunk: 0,
            current_line: None,
            diff_scroll: 0,
            h_scroll: 0,
            h_scroll_old: 0,
            h_scroll_new: 0,
            panel: None,
            panel_focus: false,
            panel_scroll: 0,
            layers: InlineLayers::default(),
            filter_expr: String::new(),
            search_query: String::new(),
            show_unreviewed_only: false,
            sort_by_mtime: false,
            file_tree_width: 32,
            panel_width: 40,
            draft: Draft::default(),
            filter_input: String::new(),
            commit_input: String::new(),
            context_overrides: HashMap::new(),
            mtime_cache: HashMap::new(),
        }
    }
}

/// The comment being written, and where it lands.
pub struct Draft {
    pub textarea: TextArea<'static>,
    pub file: String,
    pub hunk: usize,
    pub line: Option<usize>,
    pub kind: CommentType,
    pub reply_to: Option<String>,
    pub finding_ref: Option<String>,
}

impl Default for Draft {
    fn default() -> Self {
        Self {
            textarea: TextArea::default(),
            file: String::new(),
            hunk: 0,
            line: None,
            kind: CommentType::GitHubComment,
            reply_to: None,
            finding_ref: None,
        }
    }
}

/// Where a jump to the next unreviewed file lands.
pub enum Jump {
    NoFiles,
    AllReviewed,
    To(usize),
}

/// A file the diff lists by header only, whose hunks a parse fills.
pub fn is_stub(file: &DiffFile) -> bool {
    file.hunks.is_empty() && !file.compacted && file.raw_hunk_count > 0
}

impl TabViewState {
    /// The files the tree lists, with their index in `files`.
    pub fn visible<'f>(
        &self,
        files: &'f [std::sync::Arc<DiffFile>],
        reviewed: &HashMap<String, String>,
    ) -> Vec<(usize, &'f DiffFile)> {
        let search = self.search_query.to_lowercase();
        visible_files(
            files.iter().map(|file| &**file),
            &FileListView {
                rules: &[],
                review: None,
                risk_rules: &RiskRules::default(),
                file_kinds: &FileKindRepoConfig::default(),
                search_lower: &search,
                unreviewed_only: self.show_unreviewed_only,
                reviewed,
            },
        )
    }

    /// Moves to the next or previous visible file. Answers the file it
    /// selected, which may still be a stub to parse.
    pub fn move_file(&mut self, content: &TabContent, forward: bool) -> Option<usize> {
        let visible: Vec<usize> = self
            .visible(&content.diff.files, &content.reviewed.marks)
            .iter()
            .map(|(idx, _)| *idx)
            .collect();
        let step = if forward {
            navigation::next_file
        } else {
            navigation::prev_file
        };
        match step(self.selected_file, None, &visible, &[]) {
            FileMove::File(idx) => {
                self.current_line = None;
                self.select_file_top(idx);
                Some(idx)
            }
            FileMove::Snap(idx) => {
                self.select_file_top(idx);
                Some(idx)
            }
            FileMove::Stay | FileMove::Watched(_) | FileMove::LeaveWatched => None,
        }
    }

    /// Jumps to the next unreviewed visible file, wrapping. The jump keeps
    /// the panel scroll, unlike a file move.
    pub fn next_unreviewed(&mut self, content: &TabContent) -> Jump {
        let marks = &content.reviewed.marks;
        let visible: Vec<(usize, bool)> = self
            .visible(&content.diff.files, marks)
            .iter()
            .map(|(idx, file)| (*idx, marks.contains_key(&file.path)))
            .collect();
        if visible.is_empty() {
            return Jump::NoFiles;
        }
        let Some(idx) = navigation::next_unreviewed_file(self.selected_file, &visible) else {
            return Jump::AllReviewed;
        };
        self.selected_file = idx;
        self.current_hunk = 0;
        self.current_line = None;
        self.diff_scroll = 0;
        self.h_scroll = 0;
        Jump::To(idx)
    }

    fn select_file_top(&mut self, idx: usize) {
        self.selected_file = idx;
        self.current_hunk = 0;
        self.diff_scroll = 0;
        self.h_scroll = 0;
        self.panel_scroll = 0;
    }

    pub fn move_hunk(&mut self, file: Option<&DiffFile>, forward: bool) {
        let count = file.map_or(0, |file| file.hunks.len());
        let to = if forward {
            navigation::next_hunk(self.current_hunk, count)
        } else {
            navigation::prev_hunk(self.current_hunk)
        };
        if let Some(hunk) = to {
            self.current_hunk = hunk;
            self.current_line = None;
            self.scroll_to_cursor(file);
        }
    }

    pub fn move_line(&mut self, file: Option<&DiffFile>, forward: bool) {
        let hunks: &[DiffHunk] = file.map_or(&[], |file| &file.hunks);
        let counts: Vec<usize> = hunks.iter().map(|hunk| hunk.lines.len()).collect();
        let is_fold = |hunk: usize, line: usize| {
            hunks
                .get(hunk)
                .and_then(|h| h.lines.get(line))
                .is_some_and(|l| matches!(l.line_type, LineType::Fold(_)))
        };
        let cursor = LineCursor {
            hunk: self.current_hunk,
            line: self.current_line,
        };
        let to = if forward {
            navigation::next_line(cursor, &counts, is_fold)
        } else {
            navigation::prev_line(cursor, &counts, is_fold)
        };
        match to {
            LineMove::Stay => {}
            LineMove::To(cursor) => {
                self.current_hunk = cursor.hunk;
                self.current_line = cursor.line;
                self.scroll_to_cursor(file);
            }
            LineMove::Exit => self.current_line = None,
        }
    }

    /// Scrolls the diff to the cursor: two file header rows, then per hunk a
    /// header row, its lines and a blank row, as the diff view lays it out.
    fn scroll_to_cursor(&mut self, file: Option<&DiffFile>) {
        let Some(file) = file else { return };
        let mut offset: usize = 2;
        for (idx, hunk) in file.hunks.iter().enumerate() {
            if idx == self.current_hunk {
                let row = offset + self.current_line.unwrap_or(0);
                self.diff_scroll = u16::try_from(row.saturating_sub(1)).unwrap_or(u16::MAX);
                return;
            }
            offset += 1 + hunk.lines.len() + 1;
        }
    }

    /// Scrolls the diff by `rows` and puts the cursor on the line at the top.
    pub fn scroll(&mut self, file: Option<&DiffFile>, rows: i32) {
        self.diff_scroll = if rows < 0 {
            self.diff_scroll
                .saturating_sub(u16::try_from(-rows).unwrap_or(u16::MAX))
        } else {
            self.diff_scroll
                .saturating_add(u16::try_from(rows).unwrap_or(u16::MAX))
        };
        let Some(file) = file.filter(|file| !file.hunks.is_empty()) else {
            return;
        };
        let target = usize::from(self.diff_scroll);
        let mut offset: usize = 2;
        for (idx, hunk) in file.hunks.iter().enumerate() {
            offset += 1;
            if target < offset + hunk.lines.len() {
                self.current_hunk = idx;
                self.current_line = Some(target.saturating_sub(offset));
                return;
            }
            offset += hunk.lines.len() + 1;
        }
        if let Some(last) = file.hunks.last() {
            self.current_hunk = file.hunks.len() - 1;
            self.current_line = Some(last.lines.len().saturating_sub(1));
        }
    }
}

/// What a frame reads beside the view state: the open tabs' names, the
/// active tab's content and the config.
pub struct Frame<'a> {
    pub tab_names: Vec<String>,
    pub active_tab: usize,
    pub content: &'a TabContent,
    pub config: &'a ErConfig,
}

/// The frame's read model, from the active tab's published content and its
/// view state. Draws the section data only; the reviewed count and the AI
/// panels show the unported message.
pub fn screen<'a>(view: &'a TuiView, state: &'a TabViewState, frame: Frame<'a>) -> Screen<'a> {
    let content = frame.content;
    let tab = &content.tab;
    let files = &content.diff.files;
    let reviewed = &content.reviewed.marks;
    let is_remote = tab.identity().is_remote();
    let file_kinds = FileKindRepoConfig::default();
    Screen {
        chrome: Chrome {
            tab_names: frame.tab_names,
            active_tab: frame.active_tab,
            theme: &frame.config.display.theme,
            config: frame.config,
            available_update: None,
            notification: view.notification.as_deref(),
            config_load_error: None,
            watching: false,
            input_mode: match view.input_mode {
                Mode::Normal => InputMode::Normal,
            },
            overlay: None,
            remote_url_input: "",
            ai_selection_label: crate::screen::UNPORTED.to_string(),
            has_comment_draft: !state.draft.textarea.is_empty(),
            can_toggle_comment_type: false,
            split_diff: false,
        },
        tab: TabView {
            mode: tab.mode,
            name: tab_name(content),
            remote_repo: tab.remote_repo.as_deref(),
            is_remote,
            current_branch: &tab.current_branch,
            base_branch: &tab.base_branch,
            pr_number: tab.pr_number,
            merge_active: false,
            unresolved_count: 0,
            committed_unpushed: false,
            sort_by_mtime: state.sort_by_mtime,
            modes: modes(content, frame.config),
            line_stats: ProdDiffStats::summarize(
                files
                    .iter()
                    .map(|file| (file.path.as_str(), file.adds, file.dels)),
                |path| file_kinds.classify(path),
            ),
            layers: &state.layers,
            panel: state.panel,
            panel_focus: state.panel_focus,
            file_tree_width: state.file_tree_width,
            panel_width: state.panel_width,
            review_focus: ReviewFocus::Files,
            selected_file: state.selected_file,
            focused_comment_id: None,
            focused_finding_id: None,
            filter_expr: &state.filter_expr,
            search_query: &state.search_query,
            show_unreviewed_only: state.show_unreviewed_only,
            show_watched: false,
            watched_count: 0,
            lazy_mode: content.diff.meta.lazy,
            comment_type: state.draft.kind,
            comment_reply_to: state.draft.reply_to.as_deref(),
            comment_finding_ref: state.draft.finding_ref.as_deref(),
            comment_file: &state.draft.file,
            comment_line_num: state.draft.line,
            comment_hunk: state.draft.hunk,
            comment_textarea: &state.draft.textarea,
            filter_input: &state.filter_input,
            commit_input: &state.commit_input,
            visible_files: state.visible(files, reviewed),
            selected_diff_file: files.get(state.selected_file).map(|file| &**file),
            diff_scroll: state.diff_scroll,
            current_hunk: state.current_hunk,
            current_line: state.current_line,
            h_scroll: state.h_scroll,
            h_scroll_old: state.h_scroll_old,
            h_scroll_new: state.h_scroll_new,
            split_focus: SplitSide::New,
            context_overrides: &state.context_overrides,
            mtime_cache: &state.mtime_cache,
            panel_scroll: state.panel_scroll,
            review_cursor: 0,
            branch_diff_hash: &content.diff.meta.branch_diff_hash,
            repo_root: &tab.repo_root,
            visible_watched: Vec::new(),
            selected_watched: None,
            selected_watched_index: None,
            watched_not_ignored: &[],
            watched_snapshot: None,
        },
        data: TabData::Sections(SectionData {
            files,
            reviewed,
            pr: content.pr.overview.as_ref(),
        }),
    }
}

/// The tab's name, as the App names it: a branch view by its branch, a
/// remote PR by its repo and number, else the checked-out branch, else the
/// folder.
pub fn tab_name(content: &TabContent) -> String {
    let tab = &content.tab;
    if let Some(branch) = &tab.local_branch_view {
        return branch.clone();
    }
    if let Some(slug) = &tab.remote_repo {
        let repo = slug.rsplit('/').next().unwrap_or(slug);
        return match tab.pr_number {
            Some(number) => format!("{repo}#{number}"),
            None => repo.to_string(),
        };
    }
    if !tab.current_branch.is_empty() {
        return tab.current_branch.clone();
    }
    std::path::Path::new(&tab.repo_root)
        .file_name()
        .map_or_else(
            || tab.repo_root.clone(),
            |name| name.to_string_lossy().into_owned(),
        )
}

/// The views the mode bar offers. A PR tab with no checkout offers its PR
/// diff alone; the views that need a builder or the watcher join as they
/// are ported.
fn modes(content: &TabContent, config: &ErConfig) -> Vec<DiffMode> {
    let tab = &content.tab;
    if tab.identity().is_remote() || (tab.pr_number.is_some() && tab.checkout_root.is_none()) {
        return vec![DiffMode::PrDiff];
    }
    let features = &config.features;
    let mut modes = Vec::new();
    if features.view_branch {
        modes.push(DiffMode::Branch);
    }
    let read_only = tab.local_branch_view.is_some() && tab.checkout_root.is_none();
    if features.view_unstaged && !read_only {
        modes.push(DiffMode::Unstaged);
    }
    if features.view_staged && !read_only {
        modes.push(DiffMode::Staged);
    }
    if features.view_history && !read_only {
        modes.push(DiffMode::History);
    }
    modes
}
