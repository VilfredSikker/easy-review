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
mod effect;
mod flows;
mod github_comments;
mod github_sync;
mod guard;
mod handle;
mod jobs;
mod pending_review;
mod pr_posts;
mod protocol;
mod settings;
mod workspace;

pub use actor::{spawn, WorkspaceDeps};
pub use app_registry::{AppCommand, AppRegistry, TabEntry, TabsView};
pub use flows::read_flow_run;
pub use handle::{WorkspaceError, WorkspaceHandle, REPLY_TIMEOUT};
pub use pending_review::OWN_PR_APPROVAL_REFUSED;
pub use protocol::{
    AcceptanceDiff, AnnotationKind, AnnotationTarget, ArenaCommand, ArenaContext, Command, Event,
    FlowCommand, FlowContext, JobOutcome, OpenTarget, PrRef, Reply, ReviewEvent, ReviewLineTarget,
    SyncSource, TabContext,
};
pub use settings::{spawn_settings, SettingsCommand, SettingsHandle, SettingsReply, SettingsView};
pub use workspace::{LiveArenaRun, LiveReviewFlow, WorkspaceTarget, WorkspaceView};

#[cfg(test)]
mod flow_tests;
#[cfg(test)]
mod job_tests;
#[cfg(test)]
mod settings_tests;
#[cfg(test)]
mod tests;
