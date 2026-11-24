//! The view types the renderers read. Each mirrors an App type of the same
//! name, so a renderer reads the same fields on either path; `screen::from_app`
//! converts the App's.

use er_engine::config::{ConfigItem, SettingsScope};
use er_engine::git::Worktree;
use er_engine::review_flow_driver::ReviewFlowResults;
use std::collections::HashSet;

/// Which side of the split diff has focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitSide {
    Old,
    New,
}

/// One pillar of the guided tour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TourPillar {
    pub id: String,
    pub title: String,
    pub description: String,
    pub importance: u32,
    pub foundation: bool,
}

/// One reference in the symbol references panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolRef {
    pub file: String,
    pub line_num: usize,
    pub line_content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentLogSource {
    Stdout,
    Stderr,
    Status,
}

/// One line of agent output.
#[derive(Debug, Clone)]
pub struct AgentLogEntry {
    pub timestamp: std::time::Instant,
    pub command_name: std::sync::Arc<str>,
    pub source: AgentLogSource,
    pub text: String,
}

/// Whether the user is navigating or typing, and into what.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode<'a> {
    Normal,
    Search,
    Comment,
    Confirm(ConfirmAction<'a>),
    Filter,
    Commit,
    RemoteUrl,
    BaseBranch,
    /// The pending review's summary; only the flag path's review hub asks
    /// for it.
    #[cfg_attr(
        all(not(feature = "v1-service"), not(test)),
        expect(dead_code, reason = "only the flag path's review hub opens it")
    )]
    ReviewSummary,
}

/// The action a y/n prompt is asking about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmAction<'a> {
    DeleteComment,
    DeleteWatchedFile {
        path: &'a str,
    },
    Push,
    CleanupQuestions {
        count: usize,
    },
    CleanupReviews {
        count: usize,
    },
    RunAgentReview,
    RunAgentQuestions,
    RunAgentNotes,
    ApprovePR,
    /// A comment of the pending review; only the flag path asks.
    #[cfg_attr(
        all(not(feature = "v1-service"), not(test)),
        expect(dead_code, reason = "only the flag path's review hub asks it")
    )]
    RemoveReviewComment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HubKind {
    Git,
    Ai,
    AiProvider,
    AiModel,
    AiEffort,
    AiExpert,
    Verify,
    VerifyPackage,
    Help,
    Open,
    Copy,
}

impl HubKind {
    pub const fn title(self) -> &'static str {
        match self {
            Self::Git => "GIT",
            Self::Ai => "AI",
            Self::AiProvider => "AI PROVIDER",
            Self::AiModel => "AI MODEL",
            Self::AiEffort => "EFFORT",
            Self::AiExpert => "SPECIALIZED REVIEW",
            Self::Verify | Self::VerifyPackage => "VERIFY",
            Self::Help => "HELP",
            Self::Open => "OPEN",
            Self::Copy => "COPY",
        }
    }
}

/// One row of a modal hub. The renderer needs to know only whether a row
/// opens a stacked PR, which has a second key, so the action itself stays in
/// the App.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HubItem {
    pub label: String,
    pub hint: String,
    pub description: String,
    pub is_header: bool,
    pub enabled: bool,
    pub opens_stack_pr: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
    pub is_git_repo: bool,
}

/// The config hub's inline edit.
#[derive(Debug, Clone, Copy)]
pub struct ConfigEditState<'a> {
    pub item_index: usize,
    pub buffer: &'a String,
}

/// The open popup. Lists whose rows are App types are held as converted rows
/// by the view cache; every other list is borrowed.
#[derive(Debug, Clone, Copy)]
pub enum OverlayData<'a> {
    WorktreePicker {
        worktrees: &'a [Worktree],
        selected: usize,
    },
    DirectoryBrowser {
        current_path: &'a str,
        entries: &'a [DirEntry],
        selected: usize,
    },
    FilterHistory {
        history: &'a [String],
        selected: usize,
        preset_count: usize,
    },
    ModalHub {
        kind: HubKind,
        title: Option<&'a str>,
        items: &'a [HubItem],
        selected: usize,
    },
    ConfigHub {
        tab: SettingsScope,
        items: &'a [ConfigItem],
        selected: usize,
        editing: Option<ConfigEditState<'a>>,
    },
    ExportPicker {
        include_comments: bool,
        include_findings: bool,
        include_questions: bool,
        include_notes: bool,
        selected: usize,
    },
    ReviewFlowProgress {
        er_dir: &'a str,
        flow: &'a er_engine::ai::review_flow::ReviewFlow,
        selected: usize,
        detail_scroll: usize,
    },
    ReviewFlowResults {
        results: &'a ReviewFlowResults,
        selected: usize,
        selected_ids: &'a HashSet<String>,
        detail_scroll: usize,
    },
}
