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
pub mod arena;
mod effect;
mod github_comments;
mod github_sync;
mod guard;
mod handle;
mod jobs;
mod pending_review;
mod protocol;
mod workspace;

pub use actor::{spawn, WorkspaceDeps};
pub use handle::{WorkspaceError, WorkspaceHandle, REPLY_TIMEOUT};
pub use pending_review::OWN_PR_APPROVAL_REFUSED;
pub use protocol::{
    AnnotationKind, AnnotationTarget, ArenaCommand, ArenaContext, Command, Event, JobOutcome,
    OpenTarget, PrRef, Reply, ReviewEvent, ReviewLineTarget, SyncSource, TabContext,
};
pub use workspace::{LiveArenaRun, WorkspaceTarget, WorkspaceView};

#[cfg(test)]
mod job_tests;
#[cfg(test)]
mod tests;
