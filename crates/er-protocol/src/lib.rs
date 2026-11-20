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

mod command;
mod error;
mod notice;
mod resource;
mod risk;
mod session;
mod settings;
mod sidebar;
mod tabs;

pub use command::{Ack, Command, ProjectReviewSettingsPatch};
pub use error::{ErError, ErErrorKind};
pub use notice::{Notice, NoticeLevel};
pub use resource::{
    Resource, ResourceChanged, ResourceKey, ResourceVersion, TabKey, Tabs, Versioned,
};
pub use risk::{MatchedRiskRule, Risk, RiskFile, RiskRule, RiskRuleSource, RiskTier};
pub use session::{AppUpdate, Panel, Panels, Reviewer, Session, WatchStatus};
pub use settings::{
    AiModel, AiProvider, ConfigFieldValue, ConfigHubField, DisplayConfig, FeatureFlags,
    ModelUpsert, ProviderUpsert, ReviewModelGroups, ReviewModelRef, Settings, SettingsRows,
    UninstallOptions, UninstallPreview, UninstallTarget,
};
pub use sidebar::{
    AvailableBranches, AvailablePrs, Branch, Branches, PrRow, Project, ProjectPrs, Projects, Prs,
    Worktree, Worktrees,
};
pub use tabs::{TabKind, TabRow};
