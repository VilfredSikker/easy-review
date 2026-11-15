//! The desktop side of the workspace seam (ADR 0067).
//!
//! One actor per repo root, started on first use. The desktop resolves
//! app-scoped records — the project list — to a repo root and a remote before
//! sending, so the service never reads the store: those records stay on the
//! caller's side for now.
//!
//! Only compiled with `v1-service`. The default build keeps the existing
//! command path untouched.

use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};

use er_engine::ports::Ports;
use er_service::{
    spawn, Command, Event, OpenTarget, Reply, WorkspaceDeps, WorkspaceError, WorkspaceHandle,
    WorkspaceView,
};

/// The actors this desktop process has started.
pub struct ServiceState {
    deps: WorkspaceDeps,
    handles: Mutex<HashMap<String, WorkspaceHandle>>,
    /// The desktop revision. Every view an actor publishes bumps it, so the
    /// front end re-reads on `er://revision` (ADR 0011) with no poll of its
    /// own: a live arena run's progress reaches the page this way.
    desktop_revision: Arc<AtomicU64>,
}

impl ServiceState {
    pub fn new(ports: Ports, desktop_revision: Arc<AtomicU64>) -> Self {
        Self::with_deps(WorkspaceDeps::new(ports), desktop_revision)
    }

    /// The same, over deps the caller built, such as a test's job sink.
    pub fn with_deps(deps: WorkspaceDeps, desktop_revision: Arc<AtomicU64>) -> Self {
        Self {
            deps,
            handles: Mutex::new(HashMap::new()),
            desktop_revision,
        }
    }

    /// The actor for `key`, started on first use. The key is the repo root, or
    /// `remote:<slug>` for a PR with no local project.
    pub fn handle_for(&self, key: &str) -> WorkspaceHandle {
        let mut handles = match self.handles.lock() {
            Ok(handles) => handles,
            Err(poisoned) => poisoned.into_inner(),
        };
        handles
            .entry(key.to_string())
            .or_insert_with(|| {
                let handle = spawn(self.deps.clone());
                // Subscribed before the handle is handed out, so the first
                // command's publish is not missed.
                forward_views(&handle, Arc::clone(&self.desktop_revision));
                handle
            })
            .clone()
    }

    /// Sends one command and waits for the actor's bounded reply.
    pub fn send(&self, key: &str, command: Command) -> Result<Reply, WorkspaceError> {
        self.handle_for(key).send_and_wait(command)
    }

    /// Sends one open command and waits for the actor's bounded reply.
    pub fn open_target(
        &self,
        key: &str,
        target: OpenTarget,
        replace: bool,
    ) -> Result<Reply, WorkspaceError> {
        self.handle_for(key)
            .send_and_wait(Command::OpenTarget { target, replace })
    }

    /// The last published view for `key`. Starts the actor if it is not up, so
    /// a reader never sees a missing handle as an error.
    pub fn view(&self, key: &str) -> Arc<WorkspaceView> {
        self.handle_for(key).view()
    }

    /// Whether any actor this process started is running `run_id`, from the
    /// published views. The App asks before it marks a run stale.
    pub fn arena_run_live(&self, run_id: &str) -> bool {
        let handles: Vec<WorkspaceHandle> = match self.handles.lock() {
            Ok(handles) => handles.values().cloned().collect(),
            Err(poisoned) => poisoned.into_inner().values().cloned().collect(),
        };
        handles
            .iter()
            .any(|handle| handle.view().arena_run(run_id).is_some())
    }
}

/// Bumps the desktop revision for each event that changes what the front end
/// reads. The thread touches only the counter, and ends when the actor and its
/// handles are gone.
fn forward_views(handle: &WorkspaceHandle, desktop_revision: Arc<AtomicU64>) {
    let events = handle.subscribe();
    std::thread::spawn(move || {
        for event in events {
            if bumps_revision(&event) {
                crate::profile_log::bump_desktop_revision(&desktop_revision, "service_view");
            }
        }
    });
}

/// A published view, a flow's record change, and a panel record the view does
/// not carry all change what the page shows, so each makes it re-read.
fn bumps_revision(event: &Event) -> bool {
    matches!(
        event,
        Event::ViewPublished { .. } | Event::WorkflowChanged { .. } | Event::PanelRunChanged { .. }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The flow events reach the page through the same revision as a view,
    /// with no poll of their own.
    #[test]
    fn flow_events_bump_the_desktop_revision() {
        assert!(bumps_revision(&Event::WorkflowChanged {
            er_dir: "bucket".into(),
            revision: 3,
        }));
        assert!(bumps_revision(&Event::PanelRunChanged {
            er_dir: "bucket".into(),
            run_id: "run".into(),
        }));
        assert!(bumps_revision(&Event::ViewPublished { revision: 3 }));
        assert!(!bumps_revision(&Event::CommandFailed {
            message: "no".into(),
        }));
    }
}
