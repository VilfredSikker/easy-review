//! The messages crossing the actor boundary.
//!
//! Every field an effect needs travels with the command: the actor never reads
//! app-scoped records, so the desktop resolves a project to a repo root and a
//! remote before sending. That keeps the store on the caller's side of the
//! seam and makes the service testable with a scripted port alone.

use crate::workspace::WorkspaceTarget;

/// A command sent to the workspace actor.
#[derive(Clone, Debug)]
pub enum Command {
    /// Open a PR target. The actor resolves it off-actor and applies it.
    OpenTarget {
        target: OpenTarget,
        /// Whether the target replaces the active one rather than joining the
        /// list. The actor applies either by target identity; the flag is
        /// carried so the desktop route keeps its `replace` parameter.
        replace: bool,
    },
    /// Test support: reply without touching any port.
    #[cfg(test)]
    Ping,
    /// Test support: call a port on the actor thread, which the guard refuses.
    #[cfg(test)]
    PortProbe,
    /// Test support: hold the reply open forever, so a caller can prove its
    /// own timeout fires.
    #[cfg(test)]
    Stall,
}

/// What to open. See the module docs for why the repo root is a parameter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpenTarget {
    /// A GitHub PR URL, e.g. from a link in the UI.
    PrUrl { url: String, repo_root: String },
    /// An owner/repo/number given directly.
    RemotePr {
        owner: String,
        repo: String,
        number: u64,
        repo_root: String,
    },
    /// A project's PR, by project id and the project's remote (empty when the
    /// project has no remote; the target still opens, just without state).
    PrReview {
        project_id: String,
        remote: String,
        pr_number: u64,
        repo_root: String,
    },
}

impl OpenTarget {
    /// The repo root the caller resolved for this target; empty when the
    /// caller has no local project. Used as the actor's workspace key.
    pub fn repo_root(&self) -> &str {
        match self {
            Self::PrUrl { repo_root, .. }
            | Self::RemotePr { repo_root, .. }
            | Self::PrReview { repo_root, .. } => repo_root,
        }
    }
}

/// The actor's answer to one command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    TargetOpened(WorkspaceTarget),
    #[cfg(test)]
    Pong,
}

/// Pushed to subscribers after the actor applies a change. An event names the
/// revision, not the state: the reader fetches the view, so a slow subscriber
/// never holds the actor or pins an old copy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// A new view revision is published; readers fetch `handle.view()`.
    ViewPublished {
        revision: u64,
    },
    TargetOpened {
        target: WorkspaceTarget,
    },
    CommandFailed {
        message: String,
    },
}
