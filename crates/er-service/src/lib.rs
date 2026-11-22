//! The Workspace actor: commands in, events out, one actor per repo/worktree.
//!
//! The actor owns the workspace state for one repository. A command names what
//! to do; the actor reads that state on its own thread, hands any subprocess
//! work to an effect thread through [`WorkspaceDeps::ports`], and applies the
//! typed result back on the actor thread. Readers never touch the state: the
//! actor publishes an immutable [`WorkspaceView`] per revision behind an
//! `RwLock<Arc<_>>`, and [`WorkspaceHandle::view`] clones the current one.
//!
//! The engine dependency is deliberately headless (`default-features =
//! false`): the actor is the seam the desktop and the TUI will share, so it
//! must not pull a front end into the build.
//!
//! See ADR 0067 for the crate graph and the actor's position in it.

mod actor;
mod annotations;
mod app_registry;
pub mod arena;
mod content;
mod diff_resource;
mod effect;
mod flows;
mod freshness;
mod github_comments;
mod github_sync;
mod guard;
mod handle;
mod host;
mod hunks;
mod jobs;
mod pending_review;
mod pr_posts;
mod protocol;
mod reviewed_resource;
mod settings;
mod settings_wire;
mod sidecar_reload;
mod tab_pr;
mod workspace;

pub use actor::{spawn, WorkspaceDeps};
pub use app_registry::{
    resolve_tab_key, tabs_resource, wire_tab_key, AppCommand, AppRegistry, CachedPr, TabEntry,
    TabsView,
};
pub use content::{
    AiSection, DiffMeta, DiffSection, FreshnessSection, PrSection, ReviewedSection, ScopeStats,
    SidecarStamp, TabContent,
};
pub use diff_resource::{diff_load_error, diff_resource, diff_version, DiffInputs};
pub use flows::read_flow_run;
pub use freshness::{BaseBehind, Divergence, Freshness, RemoteTips};
pub use handle::{WorkspaceError, WorkspaceHandle, REPLY_TIMEOUT};
pub use host::{ReplyReceiver, ServiceHost};
pub use hunks::{read_hunks, HunkRead};
pub use pending_review::OWN_PR_APPROVAL_REFUSED;
pub use protocol::{
    AcceptanceDiff, AnnotationKind, AnnotationTarget, ArenaCommand, ArenaContext, Command, Event,
    FlowCommand, FlowContext, JobOutcome, OpenTarget, PrRef, Reply, ReviewEvent, ReviewLineTarget,
    SyncSource, TabContext,
};
pub use reviewed_resource::{
    content_view, painted_view_matches, reviewed_resource, reviewed_version,
};
pub use settings::{spawn_settings, SettingsCommand, SettingsHandle, SettingsReply, SettingsView};
pub use settings_wire::{risk_resource, settings_command, settings_resource};
pub use workspace::{LiveArenaRun, LiveReviewFlow, LoadError, WorkspaceTarget, WorkspaceView};

#[cfg(test)]
mod flow_tests;
#[cfg(test)]
mod job_tests;
#[cfg(test)]
mod settings_tests;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod content_tests;
