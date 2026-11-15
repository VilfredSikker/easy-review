//! The desktop side of the workspace seam (ADR 0067).
//!
//! One actor per repo root, started on first use, plus one settings actor for
//! the whole process (ADR 0069). The desktop resolves app-scoped records — the
//! project list — to a repo root and a remote before sending, so the service
//! never reads the store: those records stay on the caller's side for now.
//!
//! Only compiled with `v1-service`. The default build keeps the existing
//! command path untouched.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use er_engine::ports::Ports;
use er_service::{
    spawn, spawn_settings, Command, OpenTarget, Reply, SettingsHandle, WorkspaceDeps,
    WorkspaceError, WorkspaceHandle, WorkspaceView,
};

/// The actors this desktop process has started.
pub struct ServiceState {
    deps: WorkspaceDeps,
    handles: Mutex<HashMap<String, WorkspaceHandle>>,
    /// The one settings actor. A fixed slot, never keyed by a tab: the config
    /// is app-scoped, so a per-repo owner would race its saves.
    settings: OnceLock<SettingsHandle>,
    /// `config.toml` for the settings actor; the managed path when unset.
    settings_path: Option<PathBuf>,
}

impl ServiceState {
    pub fn new(ports: Ports) -> Self {
        Self {
            deps: WorkspaceDeps::new(ports),
            handles: Mutex::new(HashMap::new()),
            settings: OnceLock::new(),
            settings_path: None,
        }
    }

    /// A service whose settings actor owns `config_path` instead of the
    /// managed config, so a test never reaches the owner's files.
    pub fn with_settings_path(ports: Ports, config_path: PathBuf) -> Self {
        Self {
            settings_path: Some(config_path),
            ..Self::new(ports)
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

    /// The settings actor, started on first use. It loads the config before
    /// its loop starts, so the first settings command pays that read.
    pub fn settings(&self) -> SettingsHandle {
        self.settings
            .get_or_init(|| {
                let path = self
                    .settings_path
                    .clone()
                    .unwrap_or_else(er_engine::config::managed_config_path);
                spawn_settings(self.deps.clone(), path)
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
}
