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
use std::sync::{Arc, Mutex};

use er_engine::ports::Ports;
use er_service::{
    spawn, Command, OpenTarget, Reply, WorkspaceDeps, WorkspaceError, WorkspaceHandle,
    WorkspaceView,
};

/// The actors this desktop process has started.
pub struct ServiceState {
    deps: WorkspaceDeps,
    handles: Mutex<HashMap<String, WorkspaceHandle>>,
}

impl ServiceState {
    pub fn new(ports: Ports) -> Self {
        Self {
            deps: WorkspaceDeps::new(ports),
            handles: Mutex::new(HashMap::new()),
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
            .or_insert_with(|| spawn(self.deps.clone()))
            .clone()
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
}
