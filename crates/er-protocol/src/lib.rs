//! The wire between the desktop backend and its frontend.
//!
//! Types here derive serde and `ts_rs::TS`, and the TypeScript the frontend
//! imports is generated from them by the `protocol-gen` binary into
//! `desktop-ui/src/lib/protocol/generated/`. Export runs only through that
//! binary: a type that exported itself would be written by any `cargo test`,
//! so what the committed files hold would depend on which test ran last.
//!
//! The crate names nothing from the workspace. Builders that turn service or
//! desktop state into these types live beside that state, so the wire does not
//! move when an internal type does.

mod annotations;
mod command;
mod diff;
mod direct;
mod error;
mod hunks;
mod jobs;
mod notice;
mod pr;
mod preview;
mod resource;
mod reviewed;
mod risk;
mod session;
mod settings;
mod sidebar;
mod tabs;

pub use annotations::{
    AiAnnotations, AnnotationScreenshot, Annotations, Checklist, ChecklistItem, Diagram,
    DiagramPreset, FileRisk, Finding, FindingResponse, OtherBucket, ReviewJson, ReviewRevision,
    ReviewRevisions, Thread, ThreadMessage, Tour, TourFile, TourPillar, TourRelatedFile, Triage,
    TriageGuard, TriagePriorityFile, TriageReviewUpdate, TriageSensitiveReason, UiAnnotation,
    UiAnnotations,
};
pub use command::{
    Ack, AnchorUpdate, Command, LineTarget, NewUiAnnotation, ProjectReviewSettingsPatch,
};
pub use diff::{
    BaseBehind, BranchDivergence, CommitRow, DiffLineStats, FileRow, FilterSuggestion, ScopeStat,
    TabDiff,
};
pub use direct::{BlameSpan, ExportOptions, LineCommits, OpenSource, OpenSourceKind};
pub use error::{ErError, ErErrorKind};
pub use hunks::{FileHunks, Hunk, HunkLine};
pub use jobs::{AiReplyJob, Jobs};
pub use notice::{Notice, NoticeLevel};
pub use pr::{
    CheckRun, GithubStatus, MergeMethod, PendingReview, PendingReviewComment, PrAction, PrCard,
    PrStatus, PrTarget, RepoMerge, ReviewLine, ReviewRequest, Stack, StackLayer, StatusComment,
    StatusReview, StatusReviewer, TabPr,
};
pub use preview::{DiffPaths, FilePreview, ImagePreview};
pub use resource::{
    Resource, ResourceChanged, ResourceKey, ResourceVersion, TabKey, Tabs, Versioned,
};
pub use reviewed::{PaintedView, TabReviewed};
pub use risk::{MatchedRiskRule, Risk, RiskFile, RiskRule, RiskRuleSource, RiskTier};
pub use session::{AppUpdate, Panel, Panels, Reviewer, Session, WatchStatus};
pub use settings::{
    AiModel, AiProvider, ConfigFieldValue, ConfigHubField, DisplayConfig, FeatureFlags,
    ModelUpsert, ProviderUpsert, ReviewModelGroups, ReviewModelRef, Settings, SettingsRows,
    UninstallOptions, UninstallPreview, UninstallTarget,
};
pub use sidebar::{
    AvailableBranches, AvailablePrs, Branch, Branches, Inbox, InboxItem, InboxTarget, PrRow,
    Project, ProjectPrs, Projects, Prs, Worktree, Worktrees,
};
pub use tabs::{BrowserLayout, BrowserView, PrOpenHint, TabKind, TabRow, TabView};
