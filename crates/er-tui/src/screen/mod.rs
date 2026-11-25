//! The render seam: one read model each frame draws from. `from_app` fills
//! the `App` variant from the App; the service path fills `Sections` from the
//! tab content the workspace publishes.

mod from_app;
pub mod view;
pub mod watched;

pub use from_app::{from_app, ViewCache};

use er_engine::ai::{AiState, CommentType, InlineLayers, PanelContent, ReviewFocus};
use er_engine::config::ErConfig;
use er_engine::git::{CommitInfo, DiffFile, ProdDiffStats, WatchedFile};
use er_engine::github::PrOverviewData;
use er_engine::tab_identity::DiffMode;
use std::collections::HashMap;
use tui_textarea::TextArea;
use watched::SnapshotDiff;

pub struct Screen<'a> {
    pub chrome: Chrome<'a>,
    pub tab: TabView<'a>,
    pub data: TabData<'a>,
}

/// Everything around the active tab's content.
pub struct Chrome<'a> {
    pub tab_names: Vec<String>,
    pub active_tab: usize,
    pub theme: &'a str,
    pub config: &'a ErConfig,
    pub available_update: Option<&'a str>,
    pub notification: Option<&'a str>,
    pub config_load_error: Option<&'a str>,
    pub watching: bool,
    pub input_mode: view::InputMode<'a>,
    pub overlay: Option<view::OverlayData<'a>>,
    pub remote_url_input: &'a str,
    pub ai_selection_label: String,
    pub has_comment_draft: bool,
    pub can_toggle_comment_type: bool,
    pub split_diff: bool,
}

/// The active tab's view fields: what it shows, and what is selected, focused
/// or typed in it.
pub struct TabView<'a> {
    pub mode: DiffMode,
    pub name: String,
    pub remote_repo: Option<&'a str>,
    pub is_remote: bool,
    pub current_branch: &'a str,
    pub base_branch: &'a str,
    pub pr_number: Option<u64>,
    pub merge_active: bool,
    pub unresolved_count: usize,
    pub committed_unpushed: bool,
    pub sort_by_mtime: bool,
    pub modes: Vec<DiffMode>,
    pub line_stats: ProdDiffStats,
    pub layers: &'a InlineLayers,
    pub panel: Option<PanelContent>,
    pub panel_focus: bool,
    pub file_tree_width: u16,
    pub panel_width: u16,
    pub review_focus: ReviewFocus,
    pub selected_file: usize,
    pub focused_comment_id: Option<&'a str>,
    pub focused_finding_id: Option<&'a str>,
    pub filter_expr: &'a str,
    pub search_query: &'a str,
    pub show_unreviewed_only: bool,
    pub show_watched: bool,
    pub watched_count: usize,
    pub lazy_mode: bool,
    pub comment_type: CommentType,
    /// The comment joins the pending review; only the flag path drafts one.
    pub comment_review: bool,
    pub comment_reply_to: Option<&'a str>,
    pub comment_finding_ref: Option<&'a str>,
    pub comment_file: &'a str,
    pub comment_line_num: Option<usize>,
    pub comment_hunk: usize,
    pub comment_textarea: &'a TextArea<'static>,
    pub filter_input: &'a str,
    pub commit_input: &'a str,
    /// The files the tree lists, after the filter, the search and the
    /// unreviewed toggle, with their index in the tab's files.
    pub visible_files: Vec<(usize, &'a DiffFile)>,
    /// The file the diff view shows; History mode's is the commit's.
    pub selected_diff_file: Option<&'a DiffFile>,
    /// Scroll and cursor of the diff view, History mode's when in History.
    pub diff_scroll: u16,
    pub current_hunk: usize,
    pub current_line: Option<usize>,
    pub h_scroll: u16,
    pub h_scroll_old: u16,
    pub h_scroll_new: u16,
    pub split_focus: view::SplitSide,
    pub context_overrides: &'a HashMap<String, usize>,
    pub mtime_cache: &'a HashMap<String, std::time::SystemTime>,
    pub panel_scroll: u16,
    pub review_cursor: usize,
    pub branch_diff_hash: &'a str,
    pub repo_root: &'a str,
    pub visible_watched: Vec<(usize, &'a WatchedFile)>,
    pub selected_watched: Option<&'a WatchedFile>,
    pub selected_watched_index: Option<usize>,
    pub watched_not_ignored: &'a [String],
    /// The selected watched file's snapshot diff; `None` in content mode.
    pub watched_snapshot: Option<&'a SnapshotDiff>,
}

/// The active tab's content, one variant per path. A renderer that reads the
/// AI state or the reviewed count matches on it.
pub enum TabData<'a> {
    App(AppData<'a>),
    #[cfg(feature = "v1-service")]
    Sections(SectionData<'a>),
}

/// The App's content for the tab, borrowed as the App holds it.
pub struct AppData<'a> {
    pub files: &'a [DiffFile],
    pub reviewed: &'a HashMap<String, String>,
    pub ai: &'a AiState,
    pub pr_data: Option<&'a PrOverviewData>,
    pub history: Option<HistoryView<'a>>,
    pub tour: Option<TourView<'a>>,
    pub symbol_refs: Option<SymbolRefsView<'a>>,
    pub agent_log: &'a [view::AgentLogEntry],
    pub agent_log_auto_scroll: bool,
    pub running_command_count: usize,
    pub reviewed_count: (usize, usize),
    pub filtered_reviewed_count: Option<(usize, usize)>,
    pub running_commands: Vec<&'a str>,
    pub mem_budget: MemoryBudget,
}

/// History mode's commit list and the selected commit's diff.
pub struct HistoryView<'a> {
    pub commits: &'a [CommitInfo],
    /// The commits the search leaves, with their index in `commits`.
    pub visible_commits: Vec<(usize, &'a CommitInfo)>,
    pub selected_commit: usize,
    pub commit_files: Vec<&'a DiffFile>,
    pub selected_file: usize,
    pub current_hunk: usize,
    pub current_line: Option<usize>,
    pub diff_scroll: u16,
    pub h_scroll: u16,
}

/// Tour mode's pillars and the diff regrouped by pillar.
pub struct TourView<'a> {
    pub pillars: &'a [view::TourPillar],
    pub selected_pillar: usize,
    pub files: Vec<&'a DiffFile>,
    pub file_is_related: &'a [bool],
    pub pillar_file_ranges: &'a [(usize, usize)],
    pub selected_file: usize,
    pub current_hunk: usize,
    pub current_line: Option<usize>,
    pub diff_scroll: u16,
    pub h_scroll: u16,
}

/// The symbol references panel.
pub struct SymbolRefsView<'a> {
    pub symbol: &'a str,
    pub in_diff: &'a [view::SymbolRef],
    pub external: &'a [view::SymbolRef],
    pub cursor: usize,
}

/// How much of the diff is parsed, for the `ER_DEBUG` status line.
#[derive(Clone, Copy, Default)]
pub struct MemoryBudget {
    pub parsed_files: usize,
    pub total_lines: usize,
    pub compacted_files: usize,
}

/// The service's published sections, and what the ported builders computed
/// from them. The Refs panel draws `UNPORTED` until the entry that ports
/// it.
#[cfg(feature = "v1-service")]
pub struct SectionData<'a> {
    pub files: &'a [std::sync::Arc<DiffFile>],
    pub reviewed: &'a HashMap<String, String>,
    pub pr: Option<&'a PrOverviewData>,
    /// The published AI section's state: the threads, their order and counts
    /// come from its engine methods, as on the App path.
    pub ai: &'a AiState,
    /// Reviewed and total files, from the reviewed builder.
    pub reviewed_count: (usize, usize),
    /// The same among the files the filter keeps; `None` with no filter.
    pub filtered_reviewed_count: Option<(usize, usize)>,
    /// History's commits and the selected commit's files, in History only.
    pub history: Option<HistoryView<'a>>,
    /// The tour's pillars and files, from the service's tour builder, in
    /// Tour only.
    pub tour: Option<TourView<'a>>,
    /// The tab's bucket log, from the service's task host.
    pub agent_log: &'a [view::AgentLogEntry],
    /// The commands running on the tab's bucket.
    pub running_commands: Vec<&'a str>,
}

/// What an area the service path does not draw yet shows instead.
pub const UNPORTED: &str = "not on this build yet";

impl<'a> TabData<'a> {
    /// The AI state, or `None` where the path does not carry it.
    pub fn ai(&self) -> Option<&'a AiState> {
        match self {
            Self::App(data) => Some(data.ai),
            #[cfg(feature = "v1-service")]
            Self::Sections(data) => Some(data.ai),
        }
    }

    /// The AI state for inline marks: an empty one where the path has none,
    /// so nothing is marked.
    pub fn ai_or_empty(&self) -> &'a AiState {
        static EMPTY: std::sync::LazyLock<AiState> = std::sync::LazyLock::new(AiState::default);
        self.ai().unwrap_or(&EMPTY)
    }

    pub fn file_count(&self) -> usize {
        match self {
            Self::App(data) => data.files.len(),
            #[cfg(feature = "v1-service")]
            Self::Sections(data) => data.files.len(),
        }
    }

    pub fn file(&self, index: usize) -> Option<&'a DiffFile> {
        match self {
            Self::App(data) => data.files.get(index),
            #[cfg(feature = "v1-service")]
            Self::Sections(data) => data.files.get(index).map(|file| &**file),
        }
    }

    pub fn pr_data(&self) -> Option<&'a PrOverviewData> {
        match self {
            Self::App(data) => data.pr_data,
            #[cfg(feature = "v1-service")]
            Self::Sections(data) => data.pr,
        }
    }

    pub fn running_commands(&self) -> &[&'a str] {
        match self {
            Self::App(data) => &data.running_commands,
            #[cfg(feature = "v1-service")]
            Self::Sections(data) => &data.running_commands,
        }
    }

    /// The agent log the Log panel draws, and how many commands run. The
    /// flag path's log always follows its tail, as the App's does.
    pub fn agent_log(&self) -> (&'a [view::AgentLogEntry], bool, usize) {
        match self {
            Self::App(data) => (
                data.agent_log,
                data.agent_log_auto_scroll,
                data.running_command_count,
            ),
            #[cfg(feature = "v1-service")]
            Self::Sections(data) => (data.agent_log, true, data.running_commands.len()),
        }
    }

    /// Reviewed and total files.
    pub const fn reviewed_count(&self) -> (usize, usize) {
        match self {
            Self::App(data) => data.reviewed_count,
            #[cfg(feature = "v1-service")]
            Self::Sections(data) => data.reviewed_count,
        }
    }

    pub const fn filtered_reviewed_count(&self) -> Option<(usize, usize)> {
        match self {
            Self::App(data) => data.filtered_reviewed_count,
            #[cfg(feature = "v1-service")]
            Self::Sections(data) => data.filtered_reviewed_count,
        }
    }

    pub fn mem_budget(&self) -> MemoryBudget {
        match self {
            Self::App(data) => data.mem_budget,
            #[cfg(feature = "v1-service")]
            Self::Sections(_) => MemoryBudget::default(),
        }
    }

    pub const fn history(&self) -> Option<&HistoryView<'a>> {
        match self {
            Self::App(data) => data.history.as_ref(),
            #[cfg(feature = "v1-service")]
            Self::Sections(data) => data.history.as_ref(),
        }
    }

    pub const fn tour(&self) -> Option<&TourView<'a>> {
        match self {
            Self::App(data) => data.tour.as_ref(),
            #[cfg(feature = "v1-service")]
            Self::Sections(data) => data.tour.as_ref(),
        }
    }

    pub const fn reviewed(&self) -> &'a HashMap<String, String> {
        match self {
            Self::App(data) => data.reviewed,
            #[cfg(feature = "v1-service")]
            Self::Sections(data) => data.reviewed,
        }
    }

    /// The App's panel-only data, or `None` where the path does not carry it yet.
    pub const fn app(&self) -> Option<&AppData<'a>> {
        match self {
            Self::App(data) => Some(data),
            #[cfg(feature = "v1-service")]
            Self::Sections(_) => None,
        }
    }
}

#[cfg(all(test, feature = "v1-service"))]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    /// The service path draws the reviewed count it was given.
    #[test]
    fn sections_draw_their_reviewed_count() {
        let scene = crate::service::golden_tests::section_scene();
        let files = scene.content.diff.files.clone();
        let mut screen = crate::service::golden_tests::screen_of(&scene);
        let reviewed = std::collections::HashMap::new();
        screen.data = TabData::Sections(SectionData {
            files: &files,
            reviewed: &reviewed,
            pr: None,
            ai: &scene.content.ai.state,
            reviewed_count: (1, 2),
            filtered_reviewed_count: None,
            history: None,
            tour: None,
            agent_log: &[],
            running_commands: Vec::new(),
        });
        let mut terminal = Terminal::new(TestBackend::new(120, 12)).unwrap();
        let mut hl = crate::ui::highlight::Highlighter::new();
        terminal
            .draw(|f| crate::ui::draw(f, &screen, &mut hl))
            .unwrap();
        let top: String = terminal.backend().buffer().content[120..240]
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(top.contains("1/2 reviewed"), "{top}");
        assert!(!top.contains(UNPORTED), "{top}");
    }

    /// The service path draws the AI Summary panel from the AI state it was
    /// given.
    #[test]
    fn sections_draw_the_ai_summary_panel() {
        let mut scene = crate::service::golden_tests::section_scene();
        scene.ai(|ai| ai.summary = Some("Reads the audience early.".into()));
        scene.state.panel = Some(PanelContent::AiSummary);
        let files = scene.content.diff.files.clone();
        let reviewed = std::collections::HashMap::new();
        let mut screen = crate::service::golden_tests::screen_of(&scene);
        screen.data = TabData::Sections(SectionData {
            files: &files,
            reviewed: &reviewed,
            pr: None,
            ai: &scene.content.ai.state,
            reviewed_count: (0, 0),
            filtered_reviewed_count: None,
            history: None,
            tour: None,
            agent_log: &[],
            running_commands: Vec::new(),
        });
        let mut terminal = Terminal::new(TestBackend::new(140, 12)).unwrap();
        let mut hl = crate::ui::highlight::Highlighter::new();
        terminal
            .draw(|f| crate::ui::draw(f, &screen, &mut hl))
            .unwrap();
        let drawn: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(!drawn.contains(UNPORTED), "{drawn}");
        assert!(drawn.contains("audience early"), "{drawn}");
    }
}
