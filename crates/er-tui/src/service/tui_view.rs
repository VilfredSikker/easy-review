//! The flag-on TUI's own view state, and the frame drawn from it.
//!
//! The service publishes each tab's content; what the user selected, scrolled
//! or typed stays here, per tab key, as the desktop keeps its own in
//! desktop-ui (ADR 0068). Each frame fills the `Screen` from the active tab's
//! content and this state, and calls no builder itself: a builder's value
//! comes in on the `Frame`, and an area no ported builder computes draws the
//! unported message until the entry that ports it.

use std::collections::HashMap;
use std::sync::Arc;

use er_engine::ai::{CommentType, InlineLayers, PanelContent, ReviewFocus};
use er_engine::config::{ErConfig, FileKindRepoConfig, RiskRules};
use er_engine::filter::{parse_filter_expr, FilterRule, FILTER_PRESETS};
use er_engine::git::{DiffFile, DiffHunk, LineType, ProdDiffStats};
use er_engine::navigation::{self, FileMove, LineCursor, LineMove};
use er_engine::tab_identity::DiffMode;
use er_engine::visible_files::{filtered_reviewed_count, FileListView};
use er_engine::TabKey;
use er_service::{SettingsView, TabContent};
use tui_textarea::TextArea;

use super::ai_picker::Picker;
use super::config_hub::ConfigHub;
use super::modes::Follow;
use super::open_hub::OpenHub;
use super::session::Restore;
use super::watched::TabWatched;
use super::worktree::GitHub;
use crate::screen::view::{
    ConfigEditState, ConfirmAction, HubKind, InputMode, OverlayData, SplitSide,
};
use crate::screen::{Chrome, Screen, SectionData, TabData, TabView};

/// What the flag-on loop shows: per tab its view state, and app wide the
/// input mode, the overlay and the status line.
#[derive(Default)]
pub struct TuiView {
    tabs: HashMap<TabKey, TabViewState>,
    pub input_mode: Mode,
    pub overlay: Option<Overlay>,
    pub notification: Option<String>,
    /// The settings the rule tables come from; a new tab reads them before
    /// its session restores a filter.
    pub settings: Option<Arc<SettingsView>>,
    /// `--filter`, for the first tab's view state once it is made.
    pub start_filter: Option<(TabKey, String)>,
    pub base_input: String,
    /// What the start's probe or a pull learned about a tab's PR.
    pr_known: HashMap<TabKey, PrKnown>,
}

/// Whether a tab's branch has a pull request, as far as the TUI knows. Only
/// GitHub's own "no PR" answer makes it `Absent` (ADR 0053).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrKnown {
    Present,
    Absent,
    Unknown,
}

/// The overlays the flag-on path holds; each entry that ports an overlay's
/// actions adds its variant.
pub enum Overlay {
    ConfigHub(ConfigHub),
    /// The AI hub and its provider, model and effort pickers.
    Picker(Picker),
    GitHub(GitHub),
    Open(OpenHub),
    /// The filter presets, then the tab's recent filters.
    FilterHistory {
        history: Vec<String>,
        selected: usize,
    },
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "no key opens the export picker on this path yet")
    )]
    ExportPicker {
        include_comments: bool,
        include_findings: bool,
        include_questions: bool,
        include_notes: bool,
        selected: usize,
    },
}

impl Overlay {
    /// Rebuilds a hub's rows from a newer settings view.
    pub fn sync(&mut self, view: &er_service::SettingsView, ai_label: &str) {
        match self {
            Self::ConfigHub(hub) => hub.sync(view),
            Self::Picker(picker) => picker.sync(view, ai_label),
            Self::ExportPicker { .. }
            | Self::GitHub(_)
            | Self::Open(_)
            | Self::FilterHistory { .. } => {}
        }
    }

    fn data(&self) -> OverlayData<'_> {
        match *self {
            Self::ConfigHub(ref hub) => OverlayData::ConfigHub {
                tab: hub.scope,
                items: &hub.items,
                selected: hub.selected,
                editing: hub.editing.as_ref().map(|edit| ConfigEditState {
                    item_index: edit.item_index,
                    buffer: &edit.buffer,
                }),
            },
            Self::Picker(ref picker) => OverlayData::ModalHub {
                kind: picker.kind(),
                title: picker.title.as_deref(),
                items: &picker.rows,
                selected: picker.selected,
            },
            Self::GitHub(ref hub) => OverlayData::ModalHub {
                kind: HubKind::Git,
                title: None,
                items: &hub.rows,
                selected: hub.selected,
            },
            Self::Open(ref hub) => OverlayData::ModalHub {
                kind: HubKind::Open,
                title: None,
                items: &hub.rows,
                selected: hub.selected,
            },
            Self::FilterHistory {
                ref history,
                selected,
            } => OverlayData::FilterHistory {
                history,
                selected,
                preset_count: FILTER_PRESETS.len(),
            },
            Self::ExportPicker {
                include_comments,
                include_findings,
                include_questions,
                include_notes,
                selected,
            } => OverlayData::ExportPicker {
                include_comments,
                include_findings,
                include_questions,
                include_notes,
                selected,
            },
        }
    }
}

/// The input modes the flag-on path can be in; each entry that ports one
/// adds its variant.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Normal,
    Search,
    Filter,
    Commit,
    /// The composer: the active tab's draft takes the keys.
    Comment,
    Confirm(Ask),
    /// The compare-base input, opened from the Open hub.
    BaseBranch,
}

/// What a y/n prompt asks before its write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ask {
    Push,
    DeleteWatchedFile,
    ApprovePr,
    DeleteComment,
}

impl TuiView {
    /// The view state of `key`'s tab, its rule tables current. Made on its
    /// first frame, scrolled to the first hunk as a load leaves it; then
    /// `--filter` applies, then the session, in the App's order.
    pub fn tab_mut(&mut self, key: TabKey, content: &TabContent) -> &mut TabViewState {
        let settings = self.settings.as_deref();
        let state = self.tabs.entry(key).or_insert_with(|| {
            let mut state = TabViewState::default();
            state.sync_rules(content, settings);
            state.scroll_to_cursor(content.diff.files.first().map(|file| &**file));
            if let Some((_, expr)) = self.start_filter.take_if(|(first, _)| *first == key) {
                state.apply_filter(content, &expr, false);
            }
            if let Some(session) = super::session::load(content) {
                super::session::restore(&mut state, session, content);
            }
            state
        });
        state.sync_rules(content, settings);
        state
    }

    /// The view state of `key`'s tab, once it has drawn.
    pub fn tab_mut_if_open(&mut self, key: TabKey) -> Option<&mut TabViewState> {
        self.tabs.get_mut(&key)
    }

    pub fn tab(&self, key: TabKey) -> Option<&TabViewState> {
        self.tabs.get(&key)
    }

    /// What is known of `content`'s PR: a tab opened on one has it.
    pub fn pr_known(&self, key: TabKey, content: &TabContent) -> PrKnown {
        if content.tab.pr_number.is_some() {
            return PrKnown::Present;
        }
        self.pr_known.get(&key).copied().unwrap_or(PrKnown::Unknown)
    }

    pub fn learn_pr(&mut self, key: TabKey, known: PrKnown) {
        self.pr_known.insert(key, known);
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
    /// `filter_expr`, parsed.
    pub filter: Vec<FilterRule>,
    /// Recent filters, newest first.
    pub filter_history: Vec<String>,
    /// The repo's rule tables, and the settings and diff revisions they were
    /// read at.
    tables: (Option<(u64, u64)>, FileKindRepoConfig, RiskRules),
    pub search_query: String,
    pub show_unreviewed_only: bool,
    pub sort_by_mtime: bool,
    pub file_tree_width: u16,
    pub panel_width: u16,
    pub draft: Draft,
    /// The question, note or comment J and K last landed on.
    pub focused_comment_id: Option<String>,
    /// The AI revision the Gate last followed; a new one sets it again
    /// unless it was set by hand.
    pub gate_followed: Option<u64>,
    /// The notice of the draft's write in flight; the composer takes no
    /// edit until its reply lands.
    pub sent: Option<String>,
    /// The comment the delete prompt asks about.
    pub deleting_comment: Option<String>,
    /// A J or K landed on a stub: once it parses, the diff scrolls to the
    /// hint's hunk, as the App's in-place parse lets it.
    pub scroll_on_parse: bool,
    pub filter_input: String,
    pub commit_input: String,
    pub watched: TabWatched,
    context_overrides: HashMap<String, usize>,
    /// Where the selection stood when a mode switch or base change went out.
    pub follow: Option<Follow>,
    /// The same for a refresh, kept apart so one leaves a switch's alone.
    pub refresh_follow: Option<Follow>,
    pub restore: Option<Restore>,
    pub base_sent: Option<String>,
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
            filter: Vec::new(),
            filter_history: Vec::new(),
            tables: Default::default(),
            search_query: String::new(),
            show_unreviewed_only: false,
            sort_by_mtime: false,
            file_tree_width: 32,
            panel_width: 40,
            draft: Draft::default(),
            focused_comment_id: None,
            gate_followed: None,
            sent: None,
            deleting_comment: None,
            scroll_on_parse: false,
            filter_input: String::new(),
            commit_input: String::new(),
            watched: TabWatched::default(),
            context_overrides: HashMap::new(),
            follow: None,
            refresh_follow: None,
            restore: None,
            base_sent: None,
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
    pub edit_id: Option<String>,
    /// The review side, `LEFT` or `RIGHT`.
    pub side: Option<String>,
    pub line_end: Option<usize>,
    /// The id a retried GitHub post carries, so it finds the comment it
    /// already made (ADR 0053), and the id a question or note create
    /// carries, so a retry after a refusal writes the same row (ADR 0072).
    pub client_id: Option<String>,
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
            edit_id: None,
            side: None,
            line_end: None,
            client_id: None,
        }
    }
}

impl Draft {
    /// Whether Ctrl+T may change the kind: a new draft on a file, as the
    /// App's rule has it.
    pub const fn can_toggle_kind(&self) -> bool {
        self.reply_to.is_none()
            && self.edit_id.is_none()
            && self.finding_ref.is_none()
            && !self.file.is_empty()
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
    /// Reads the repo's rule tables again when the settings or the diff
    /// moved since they were read.
    fn sync_rules(&mut self, content: &TabContent, settings: Option<&SettingsView>) {
        let Some(settings) = settings else { return };
        let at = Some((settings.revision, content.diff.revision));
        if self.tables.0 != at {
            let (kinds, risk) = er_service::file_rule_tables(content, &settings.config);
            self.tables = (at, kinds, risk);
        }
    }

    fn list_view<'a>(&'a self, content: &'a TabContent, search: &'a str) -> FileListView<'a> {
        FileListView {
            rules: &self.filter,
            review: content.ai.state.review.as_ref(),
            risk_rules: &self.tables.2,
            file_kinds: &self.tables.1,
            search_lower: search,
            unreviewed_only: self.show_unreviewed_only,
            reviewed: &content.reviewed.marks,
        }
    }

    /// The files the tree lists, with their index in the diff: the filter,
    /// then the search, then unreviewed-only, in the service's order.
    pub fn visible<'f>(&self, content: &'f TabContent) -> Vec<(usize, &'f DiffFile)> {
        let search = self.search_query.to_lowercase();
        let view = self.list_view(content, &search);
        er_service::listed_files(content, &view, self.sort_by_mtime)
    }

    /// Reviewed and total files among those the filter keeps, or `None`
    /// with no filter, from the engine's count as the App's.
    pub fn filtered_reviewed_count(&self, content: &TabContent) -> Option<(usize, usize)> {
        let files = content.diff.files.iter().map(|file| &**file);
        filtered_reviewed_count(files, &self.list_view(content, ""))
    }

    /// Applies a filter expression, keeps it at the top of the history (20
    /// at most), and snaps; an empty one clears the filter. Answers the file
    /// the snap selected.
    pub fn apply_filter(&mut self, content: &TabContent, expr: &str, shown: bool) -> Option<usize> {
        let expr = expr.trim();
        if expr.is_empty() {
            return self.clear_filter(content, shown);
        }
        self.filter_expr = expr.to_string();
        self.filter = parse_filter_expr(expr);
        self.filter_history.retain(|old| old != expr);
        self.filter_history.insert(0, expr.to_string());
        self.filter_history.truncate(20);
        self.snap(content, shown)
    }

    pub fn clear_filter(&mut self, content: &TabContent, shown: bool) -> Option<usize> {
        self.filter_expr.clear();
        self.filter.clear();
        self.snap(content, shown)
    }

    fn visible_indices(&self, content: &TabContent) -> Vec<usize> {
        self.visible(content).iter().map(|(idx, _)| *idx).collect()
    }

    /// The selected file's place among the visible files, or the first.
    pub fn visible_position(&self, content: &TabContent) -> usize {
        let visible = self.visible_indices(content);
        visible
            .iter()
            .position(|&idx| idx == self.selected_file)
            .unwrap_or(0)
    }

    /// Selects the first visible file when the list hides the selected one,
    /// as the App's snap does: a hidden watched file moves to the first
    /// watched file shown, else to the first diff file. Answers the diff file
    /// it selected.
    pub fn snap(&mut self, content: &TabContent, watched_shown: bool) -> Option<usize> {
        if let Some(at) = self.watched.selected {
            let watched = self.watched.visible(watched_shown, &self.search_query);
            if watched.iter().any(|(idx, _)| *idx == at) {
                return None;
            }
            let first_watched = watched.first().map(|(idx, _)| *idx);
            if first_watched.is_some() {
                self.watched.selected = first_watched;
                return None;
            }
            self.watched.selected = None;
            let first = *self.visible_indices(content).first()?;
            self.select_file_cursor(first);
            return Some(first);
        }
        let visible = self.visible_indices(content);
        let first = *visible.first()?;
        if visible.contains(&self.selected_file) {
            return None;
        }
        self.select_file_cursor(first);
        Some(first)
    }

    /// Selects the visible file at `position`, or the last, as the App does
    /// once a mark hides the selected file under unreviewed-only.
    pub fn advance_to(&mut self, content: &TabContent, position: usize) -> Option<usize> {
        let visible = self.visible_indices(content);
        let last = visible.len().checked_sub(1)?;
        let idx = *visible.get(position.min(last))?;
        self.select_file_cursor(idx);
        Some(idx)
    }

    fn select_file_cursor(&mut self, idx: usize) {
        self.selected_file = idx;
        self.current_hunk = 0;
        self.current_line = None;
        self.diff_scroll = 0;
        self.h_scroll = 0;
    }

    /// Moves to the next or previous visible file, on through the watched
    /// files when they show. Answers the diff file it selected, which may
    /// still be a stub to parse.
    pub fn move_file(
        &mut self,
        content: &TabContent,
        watched_shown: bool,
        forward: bool,
    ) -> Option<usize> {
        let visible = self.visible_indices(content);
        let watched: Vec<usize> = self
            .watched
            .visible(watched_shown, &self.search_query)
            .iter()
            .map(|(idx, _)| *idx)
            .collect();
        let step = if forward {
            navigation::next_file
        } else {
            navigation::prev_file
        };
        match step(
            self.selected_file,
            self.watched.selected,
            &visible,
            &watched,
        ) {
            FileMove::File(idx) => {
                self.watched.selected = None;
                self.current_line = None;
                self.select_file_top(idx);
                Some(idx)
            }
            FileMove::Snap(idx) => {
                self.select_file_top(idx);
                Some(idx)
            }
            FileMove::Watched(idx) => {
                self.watched.selected = Some(idx);
                self.diff_scroll = 0;
                self.h_scroll = 0;
                None
            }
            FileMove::LeaveWatched => {
                self.watched.selected = None;
                None
            }
            FileMove::Stay => None,
        }
    }

    /// Jumps to the next unreviewed visible file, wrapping. The jump keeps
    /// the panel scroll, unlike a file move.
    pub fn next_unreviewed(&mut self, content: &TabContent) -> Jump {
        let marks = &content.reviewed.marks;
        let visible: Vec<(usize, bool)> = self
            .visible(content)
            .iter()
            .map(|(idx, file)| (*idx, marks.contains_key(&file.path)))
            .collect();
        if visible.is_empty() {
            return Jump::NoFiles;
        }
        let Some(idx) = navigation::next_unreviewed_file(self.selected_file, &visible) else {
            return Jump::AllReviewed;
        };
        self.select_file_cursor(idx);
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
    pub fn scroll_to_cursor(&mut self, file: Option<&DiffFile>) {
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
/// active tab's content, and the settings view's config, banner and label.
pub struct Frame<'a> {
    pub tab_names: Vec<String>,
    pub active_tab: usize,
    pub content: &'a TabContent,
    pub config: &'a ErConfig,
    pub config_load_error: Option<&'a str>,
    pub ai_selection_label: String,
    /// Reviewed and total files, from the reviewed builder.
    pub reviewed_count: (usize, usize),
    pub watching: bool,
}

/// The frame's read model, from the active tab's published content and its
/// view state. Draws the section data, the threads and the reviewed count;
/// the AI Summary panel shows the unported message.
pub fn screen<'a>(view: &'a TuiView, state: &'a TabViewState, frame: Frame<'a>) -> Screen<'a> {
    let content = frame.content;
    let tab = &content.tab;
    let files = &content.diff.files;
    let reviewed = &content.reviewed.marks;
    let is_remote = tab.identity().is_remote();
    let file_kinds = &state.tables.1;
    let watched = &state.watched;
    let watched_shown = watched.is_shown(&frame.config.watched);
    let selected_watched = watched.selected_file();
    Screen {
        chrome: Chrome {
            tab_names: frame.tab_names,
            active_tab: frame.active_tab,
            theme: &frame.config.display.theme,
            config: frame.config,
            available_update: None,
            notification: view.notification.as_deref(),
            config_load_error: frame.config_load_error,
            watching: frame.watching,
            input_mode: match view.input_mode {
                Mode::Normal => InputMode::Normal,
                Mode::Search => InputMode::Search,
                Mode::Filter => InputMode::Filter,
                Mode::Commit => InputMode::Commit,
                Mode::Comment => InputMode::Comment,
                Mode::BaseBranch => InputMode::BaseBranch,
                Mode::Confirm(Ask::Push) => InputMode::Confirm(ConfirmAction::Push),
                Mode::Confirm(Ask::ApprovePr) => InputMode::Confirm(ConfirmAction::ApprovePR),
                Mode::Confirm(Ask::DeleteComment) => {
                    InputMode::Confirm(ConfirmAction::DeleteComment)
                }
                Mode::Confirm(Ask::DeleteWatchedFile) => {
                    InputMode::Confirm(ConfirmAction::DeleteWatchedFile {
                        path: watched.deleting.as_deref().unwrap_or_default(),
                    })
                }
            },
            overlay: view.overlay.as_ref().map(Overlay::data),
            remote_url_input: &view.base_input,
            ai_selection_label: frame.ai_selection_label,
            has_comment_draft: !state.draft.textarea.is_empty() && view.input_mode != Mode::Comment,
            can_toggle_comment_type: state.draft.can_toggle_kind(),
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
            committed_unpushed: content.diff.meta.committed_unpushed,
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
            focused_comment_id: state.focused_comment_id.as_deref(),
            focused_finding_id: None,
            filter_expr: &state.filter_expr,
            search_query: &state.search_query,
            show_unreviewed_only: state.show_unreviewed_only,
            show_watched: watched_shown,
            watched_count: watched.files.len(),
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
            visible_files: state.visible(content),
            selected_diff_file: files
                .get(state.selected_file)
                .filter(|_| selected_watched.is_none())
                .map(|file| &**file),
            diff_scroll: state.diff_scroll,
            current_hunk: state.current_hunk,
            current_line: state.current_line,
            h_scroll: state.h_scroll,
            h_scroll_old: state.h_scroll_old,
            h_scroll_new: state.h_scroll_new,
            split_focus: SplitSide::New,
            context_overrides: &state.context_overrides,
            mtime_cache: &content.diff.meta.stamps.mtimes,
            panel_scroll: state.panel_scroll,
            review_cursor: 0,
            branch_diff_hash: &content.diff.meta.branch_diff_hash,
            repo_root: &tab.repo_root,
            visible_watched: watched.visible(watched_shown, &state.search_query),
            selected_watched,
            selected_watched_index: watched.selected,
            watched_not_ignored: &watched.not_ignored,
            watched_snapshot: watched
                .snapshot()
                .filter(|_| frame.config.watched.diff_mode == "snapshot"),
        },
        data: TabData::Sections(SectionData {
            files,
            reviewed,
            pr: content.pr.overview.as_ref(),
            ai: &content.ai.state,
            reviewed_count: frame.reviewed_count,
            filtered_reviewed_count: state.filtered_reviewed_count(content),
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

/// The App's mode list, so number keys match; Conflicts waits for P71.2.
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
    if tab.pr_number.is_some() {
        modes.push(DiffMode::PrDiff);
    }
    if features.view_history && !read_only {
        modes.push(DiffMode::History);
    }
    if features.view_tour && content.ai.state.has_tour() {
        modes.push(DiffMode::Tour);
    }
    if features.view_hidden && !read_only && !config.watched.paths.is_empty() {
        modes.push(DiffMode::Hidden);
    }
    modes
}
