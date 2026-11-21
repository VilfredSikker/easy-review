//! The render seam: one read model each frame draws from. `from_app` fills
//! the `App` variant from the App; the service path fills `Sections` from the
//! tab content the workspace publishes.

mod from_app;
pub mod view;

pub use from_app::{from_app, ViewCache};

use er_engine::ai::{AiState, CommentType, InlineLayers, PanelContent, ReviewFocus};
use er_engine::app::App;
use er_engine::config::ErConfig;
use er_engine::git::{CommitInfo, DiffFile, ProdDiffStats};
use er_engine::github::PrOverviewData;
use er_engine::tab_identity::DiffMode;
use tui_textarea::TextArea;

pub struct Screen<'a> {
    pub chrome: Chrome<'a>,
    pub tab: TabView<'a>,
    pub data: TabData<'a>,
    /// Read by the diff view, the file tree and the panel, which do not draw
    /// from the read model yet.
    pub app: &'a App,
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
    pub comment_reply_to: Option<&'a str>,
    pub comment_finding_ref: Option<&'a str>,
    pub comment_file: &'a str,
    pub comment_line_num: Option<usize>,
    pub comment_hunk: usize,
    pub comment_textarea: &'a TextArea<'static>,
    pub filter_input: &'a str,
    pub commit_input: &'a str,
}

/// The active tab's content, one variant per path. A renderer that reads the
/// AI state or the reviewed count matches on it.
pub enum TabData<'a> {
    App(AppData<'a>),
    #[cfg(feature = "v1-service")]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the service path builds it from the published tab content"
        )
    )]
    Sections(SectionData<'a>),
}

/// The App's content for the tab, borrowed as the App holds it.
pub struct AppData<'a> {
    pub files: &'a [DiffFile],
    pub ai: &'a AiState,
    pub pr_data: Option<&'a PrOverviewData>,
    pub history: Option<HistoryView<'a>>,
    pub reviewed_count: (usize, usize),
    pub filtered_reviewed_count: Option<(usize, usize)>,
    pub running_commands: Vec<&'a str>,
    pub mem_budget: MemoryBudget,
}

/// History mode's commit list and the selected commit's diff.
#[derive(Clone, Copy)]
pub struct HistoryView<'a> {
    pub commits: &'a [CommitInfo],
    pub selected_commit: usize,
    pub commit_files: &'a [DiffFile],
    pub selected_file: usize,
}

/// How much of the diff is parsed, for the `ER_DEBUG` status line.
#[derive(Clone, Copy, Default)]
pub struct MemoryBudget {
    pub parsed_files: usize,
    pub total_lines: usize,
    pub compacted_files: usize,
}

/// The service's published sections: the section data only. Values a
/// service builder computes (the reviewed count, the AI projections) join
/// as each area is ported; until then those areas draw `UNPORTED`.
#[cfg(feature = "v1-service")]
pub struct SectionData<'a> {
    pub files: &'a [std::sync::Arc<DiffFile>],
    pub pr: Option<&'a PrOverviewData>,
}

/// What an area the service path does not draw yet shows instead.
pub const UNPORTED: &str = "not on this build yet";

impl<'a> TabData<'a> {
    /// The AI state, or `None` where the path does not carry it yet.
    pub fn ai(&self) -> Option<&'a AiState> {
        match self {
            Self::App(data) => Some(data.ai),
            #[cfg(feature = "v1-service")]
            Self::Sections(_) => None,
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
            Self::Sections(_) => &[],
        }
    }

    /// Reviewed and total files, or `None` where the path does not count them yet.
    pub const fn reviewed_count(&self) -> Option<(usize, usize)> {
        match self {
            Self::App(data) => Some(data.reviewed_count),
            #[cfg(feature = "v1-service")]
            Self::Sections(_) => None,
        }
    }

    pub const fn filtered_reviewed_count(&self) -> Option<(usize, usize)> {
        match self {
            Self::App(data) => data.filtered_reviewed_count,
            #[cfg(feature = "v1-service")]
            Self::Sections(_) => None,
        }
    }

    pub fn mem_budget(&self) -> MemoryBudget {
        match self {
            Self::App(data) => data.mem_budget,
            #[cfg(feature = "v1-service")]
            Self::Sections(_) => MemoryBudget::default(),
        }
    }

    pub const fn history(&self) -> Option<HistoryView<'a>> {
        match self {
            Self::App(data) => data.history,
            #[cfg(feature = "v1-service")]
            Self::Sections(_) => None,
        }
    }
}

#[cfg(all(test, feature = "v1-service"))]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    /// The service path draws the reviewed count's area as unported until a
    /// builder computes the count.
    #[test]
    fn sections_draw_the_reviewed_count_as_unported() {
        let app = crate::golden::base_app();
        let files: Vec<_> = app
            .tab()
            .files
            .iter()
            .cloned()
            .map(std::sync::Arc::new)
            .collect();
        let mut cache = ViewCache::default();
        let mut screen = from_app(&app, &mut cache);
        screen.data = TabData::Sections(SectionData {
            files: &files,
            pr: None,
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
        assert!(top.contains(&format!("reviewed count {UNPORTED}")), "{top}");
        assert!(!top.contains("/2 reviewed"), "{top}");
    }
}
