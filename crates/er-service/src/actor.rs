//! The actor loop.
//!
//! One thread owns the state and processes one message at a time. Anything
//! that can block (the ports) runs on a fresh effect thread; its typed result
//! comes back as another message, so the actor never waits on a subprocess.
//! The guard in [`crate::guard`] turns a port call on this thread into a
//! panic at the call site.

use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;

use er_engine::ports::Ports;

use crate::effect::resolve_target;
use crate::guard::{self, ActorThread};
use crate::handle::{ActorMsg, ReplySender, WorkspaceError, WorkspaceHandle};
use crate::protocol::{Command, Event, Reply};
use crate::workspace::{WorkspaceState, WorkspaceTarget, WorkspaceView};

/// The actor's dependencies. This is the seam P45 extends: the actor takes
/// this value rather than the ports directly, so a new dependency (the shared
/// app registry) is added here without touching callers of [`spawn`].
#[derive(Clone)]
pub struct WorkspaceDeps {
    ports: Ports,
}

impl WorkspaceDeps {
    pub fn new(ports: Ports) -> Self {
        Self { ports }
    }

    /// The ports, for use inside an off-actor effect. Every effect runs on its
    /// own unmarked thread, so this is the path a port call must take.
    ///
    /// # Panics
    ///
    /// Panics on a workspace actor thread: a handler that reaches a port here
    /// would block the actor on a subprocess and serialize the workspace.
    pub fn ports(&self) -> &Ports {
        guard::assert_off_actor();
        &self.ports
    }
}

/// Starts the actor for one workspace and returns its handle.
///
/// The actor thread runs until every sender is dropped. A command whose
/// effect thread cannot be started answers with a dropped reply channel.
pub fn spawn(deps: WorkspaceDeps) -> WorkspaceHandle {
    let (tx, rx) = std::sync::mpsc::channel();
    let subscribers = Arc::new(Mutex::new(Vec::new()));
    let view = Arc::new(RwLock::new(Arc::new(WorkspaceView::default())));
    let handle = WorkspaceHandle {
        tx: tx.clone(),
        subscribers: Arc::clone(&subscribers),
        view: Arc::clone(&view),
    };
    #[expect(
        clippy::expect_used,
        reason = "the actor thread is process setup: if it cannot start, no command can ever run"
    )]
    let _ = thread::Builder::new()
        .name("er-workspace-actor".into())
        .spawn(move || run(deps, tx, rx, subscribers, view))
        .expect("spawn the workspace actor thread");
    handle
}

fn run(
    deps: WorkspaceDeps,
    tx: Sender<ActorMsg>,
    rx: Receiver<ActorMsg>,
    subscribers: Arc<Mutex<Vec<Sender<Event>>>>,
    view: Arc<RwLock<Arc<WorkspaceView>>>,
) {
    let _actor = ActorThread::enter();
    let mut state = WorkspaceState::new();
    while let Ok(message) = rx.recv() {
        match message {
            ActorMsg::Command { command, reply } => {
                handle_command(&deps, &tx, command, reply);
            }
            ActorMsg::EffectDone { result, reply } => {
                apply(&mut state, result, reply, &subscribers, &view);
            }
        }
    }
}

/// Dispatches one command on the actor thread. This function must not touch a
/// port: only the effect closure below may, and it runs on a new thread.
fn handle_command(
    deps: &WorkspaceDeps,
    tx: &Sender<ActorMsg>,
    command: Command,
    reply: ReplySender,
) {
    match command {
        Command::OpenTarget { target, replace: _ } => {
            let deps = deps.clone();
            let tx = tx.clone();
            let _ = thread::Builder::new()
                .name("er-workspace-effect".into())
                .spawn(move || {
                    let result = resolve_target(&deps, &target);
                    let _ = tx.send(ActorMsg::EffectDone { result, reply });
                });
        }
        #[cfg(test)]
        Command::Ping => {
            let _ = reply.send(Ok(Reply::Pong));
        }
        #[cfg(test)]
        Command::PortProbe => {
            // Deliberate: the guard is the thing under test.
            let _ = deps.ports();
        }
        #[cfg(test)]
        Command::Stall => {
            // Hold the reply sender so the caller's own timeout fires. Leaked
            // on purpose; this command exists only in tests.
            std::mem::forget(reply);
        }
    }
}

/// Applies an effect result on the actor thread, publishes the new view, and
/// tells subscribers and the caller.
fn apply(
    state: &mut WorkspaceState,
    result: Result<WorkspaceTarget, WorkspaceError>,
    reply: ReplySender,
    subscribers: &Mutex<Vec<Sender<Event>>>,
    view: &RwLock<Arc<WorkspaceView>>,
) {
    match result {
        Ok(target) => {
            let revision = state.apply_opened(target.clone());
            publish(view, state);
            broadcast(subscribers, Event::ViewPublished { revision });
            broadcast(
                subscribers,
                Event::TargetOpened {
                    target: target.clone(),
                },
            );
            let _ = reply.send(Ok(Reply::TargetOpened(target)));
        }
        Err(error) => {
            let message = error.to_string();
            let revision = state.apply_failure(message.clone());
            publish(view, state);
            broadcast(subscribers, Event::ViewPublished { revision });
            broadcast(subscribers, Event::CommandFailed { message });
            let _ = reply.send(Err(error));
        }
    }
}

fn publish(view: &RwLock<Arc<WorkspaceView>>, state: &WorkspaceState) {
    let next = Arc::new(state.view().clone());
    match view.write() {
        Ok(mut current) => *current = next,
        // A poisoned lock still holds a complete view; refuse to blank the
        // workspace over a writer panic.
        Err(poisoned) => *poisoned.into_inner() = next,
    }
}

/// Sends one event to every subscriber, dropping those whose receiver is gone.
fn broadcast(subscribers: &Mutex<Vec<Sender<Event>>>, event: Event) {
    if let Ok(mut subscribers) = subscribers.lock() {
        subscribers.retain(|sender| sender.send(event.clone()).is_ok());
    }
}
