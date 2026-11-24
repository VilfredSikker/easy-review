//! The caller's side of the actor: send, subscribe, read the view.
//!
//! A handle is cheap to clone and thread-safe. It carries the command sender,
//! the subscriber list and the published view; it never owns the state itself.

use std::fmt;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use er_agent::job::JobProgress;
use er_engine::jobs::JobId;
use er_engine::TabKey;

use crate::content::ContentPatch;
use crate::protocol::{Command, Event, JobOutcome, Reply};
use crate::workspace::WorkspaceView;

/// How long a caller waits for a reply before deciding the actor is stuck. The
/// effects P40 runs are one GitHub call; 30s covers a slow `gh` without
/// hanging a UI command forever.
pub const REPLY_TIMEOUT: Duration = Duration::from_secs(30);

/// Why a command did not produce a reply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkspaceError {
    /// The actor thread is gone (panicked or shut down).
    Disconnected,
    /// No reply within the caller's deadline.
    Timeout(Duration),
    /// The effect failed; the message is the port's error.
    Failed(String),
    /// A GitHub call failed. The variant is the port's own classification
    /// (ADR 0055), so a caller matches it instead of reading text.
    Github(er_github::GhError),
}

impl fmt::Display for WorkspaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disconnected => write!(f, "workspace actor disconnected"),
            Self::Timeout(timeout) => {
                write!(f, "workspace actor did not reply within {timeout:?}")
            }
            Self::Failed(message) => write!(f, "{message}"),
            Self::Github(error) => write!(f, "{error}"),
        }
    }
}

impl From<er_github::GhError> for WorkspaceError {
    fn from(error: er_github::GhError) -> Self {
        Self::Github(error)
    }
}

impl std::error::Error for WorkspaceError {}

/// The kind the frontend branches on. A failed effect is the backend
/// declining the command, with the port's message as the reason.
impl From<WorkspaceError> for er_protocol::ErError {
    fn from(error: WorkspaceError) -> Self {
        use er_protocol::ErErrorKind;

        let kind = match &error {
            WorkspaceError::Disconnected => ErErrorKind::Disconnected,
            WorkspaceError::Timeout(_) => ErErrorKind::Timeout,
            WorkspaceError::Failed(_) => ErErrorKind::Refused,
            WorkspaceError::Github(_) => ErErrorKind::Github,
        };
        Self {
            kind,
            message: error.to_string(),
        }
    }
}

pub(crate) type ReplySender = Sender<Result<Reply, WorkspaceError>>;

/// What an effect hands back: the caller's reply, and what it changed in the
/// tabs' content for the actor to apply first. The patch never reaches the
/// caller, so the public replies keep their shapes.
pub(crate) struct Effected {
    pub reply: Reply,
    pub patch: Option<ContentPatch>,
}

impl From<Reply> for Effected {
    fn from(reply: Reply) -> Self {
        Self { reply, patch: None }
    }
}

impl Effected {
    pub(crate) fn patched(reply: Reply, patch: ContentPatch) -> Self {
        Self {
            reply,
            patch: Some(patch),
        }
    }
}

/// The actor's inbox.
pub(crate) enum ActorMsg {
    Command {
        command: Command,
        reply: ReplySender,
    },
    /// A tab's first load failed. The actor records it for the key before
    /// the failed effect's answer, which publishes it.
    LoadFailed {
        key: TabKey,
        message: String,
    },
    /// An off-actor effect finished; the actor applies it and answers.
    EffectDone {
        result: Result<Effected, WorkspaceError>,
        reply: ReplySender,
    },
    /// An off-actor effect read GitHub; the actor applies what it read (and
    /// writes it, for a sync) before answering.
    Fetched {
        result: Result<crate::github_sync::Fetched, WorkspaceError>,
        reply: ReplySender,
    },
    JobProgress {
        job: JobId,
        line: JobProgress,
    },
    /// A job's effect returned; the actor drops its stop handle and tells
    /// subscribers.
    JobFinished {
        job: JobId,
        outcome: JobOutcome,
    },
    /// A live arena run wrote progress; readers refresh.
    ArenaChanged,
    /// A flow call finished; the actor publishes what it changed.
    FlowsChanged(crate::flows::FlowsUpdate),
    /// A sidecar reload read the disk; the actor applies what moved.
    Reloaded(Box<crate::sidecar_reload::Reloaded>),
    /// A flow stage finished; the flows advance.
    FlowTick,
    /// A working-tree write ran, or its reload finished.
    Worktree(Box<crate::worktree::Step>),
}

/// A handle to one workspace actor.
#[derive(Clone)]
pub struct WorkspaceHandle {
    pub(crate) tx: Sender<ActorMsg>,
    pub(crate) subscribers: Arc<Mutex<Vec<Sender<Event>>>>,
    pub(crate) view: Arc<RwLock<Arc<WorkspaceView>>>,
}

impl WorkspaceHandle {
    /// Sends a command and returns the reply channel immediately.
    pub fn send(&self, command: Command) -> Receiver<Result<Reply, WorkspaceError>> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.send_to(command, reply_tx);
        reply_rx
    }

    /// Sends a command whose reply lands on `reply`.
    pub(crate) fn send_to(&self, command: Command, reply: Sender<Result<Reply, WorkspaceError>>) {
        let _ = self.tx.send(ActorMsg::Command { command, reply });
    }

    /// Sends a command and waits up to [`REPLY_TIMEOUT`]. Callers that own a
    /// tighter deadline use [`Self::send_and_wait_with`].
    pub fn send_and_wait(&self, command: Command) -> Result<Reply, WorkspaceError> {
        self.send_and_wait_with(command, REPLY_TIMEOUT)
    }

    /// Sends a command and waits up to `timeout`. A timeout leaves the command
    /// running; the reply, if it ever arrives, has nowhere to go.
    pub fn send_and_wait_with(
        &self,
        command: Command,
        timeout: Duration,
    ) -> Result<Reply, WorkspaceError> {
        let replies = self.send(command);
        match replies.recv_timeout(timeout) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => Err(WorkspaceError::Timeout(timeout)),
            Err(RecvTimeoutError::Disconnected) => Err(WorkspaceError::Disconnected),
        }
    }

    /// Subscribes to events from now on. The returned receiver is removed from
    /// the list after its first failed read, i.e. once the caller drops it.
    pub fn subscribe(&self) -> Receiver<Event> {
        let (tx, rx) = mpsc::channel();
        if let Ok(mut subscribers) = self.subscribers.lock() {
            subscribers.push(tx);
        }
        rx
    }

    /// The last published view. Never blocks on the actor.
    pub fn view(&self) -> Arc<WorkspaceView> {
        match self.view.read() {
            Ok(view) => Arc::clone(&view),
            // A poisoned lock still holds the last complete Arc; a reader
            // that gives up here would blank the UI over a writer panic.
            Err(poisoned) => Arc::clone(&poisoned.into_inner()),
        }
    }
}
