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
    spawn, AppCommand, AppRegistry, Command, Event, OpenTarget, Reply, WorkspaceDeps,
    WorkspaceError, WorkspaceHandle, WorkspaceView,
};

/// Told the revision of every view an actor publishes.
type RevisionRelay = Arc<dyn Fn(u64) + Send + Sync>;

/// The actors this desktop process has started.
pub struct ServiceState {
    deps: WorkspaceDeps,
    handles: Mutex<HashMap<String, WorkspaceHandle>>,
    relay: Option<RevisionRelay>,
    /// The one app registry: the tab list and the PR open caches. A fixed slot,
    /// never keyed by a tab, since its records are app-scoped.
    app_registry: Arc<AppRegistry>,
}

impl ServiceState {
    pub fn new(ports: Ports) -> Self {
        let app_registry = Arc::new(AppRegistry::new(ports.clone()));
        Self {
            deps: WorkspaceDeps::new(ports).with_app_registry(Arc::clone(&app_registry)),
            handles: Mutex::new(HashMap::new()),
            relay: None,
            app_registry,
        }
    }

    /// Like [`Self::new`], with every actor's published revisions forwarded to
    /// `relay`. The desktop emits `er://revision` from it, so a flag-path
    /// change reaches the front end as an event instead of waiting for a poll.
    pub fn with_revision_relay(ports: Ports, relay: impl Fn(u64) + Send + Sync + 'static) -> Self {
        let relay: RevisionRelay = Arc::new(relay);
        let service = Self {
            relay: Some(Arc::clone(&relay)),
            ..Self::new(ports)
        };
        forward_tab_revisions(&service.app_registry, relay);
        service
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
                if let Some(relay) = &self.relay {
                    forward_revisions(&handle, Arc::clone(relay));
                }
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

    /// The app registry. Tab and open-cache commands go here, never to a
    /// workspace actor picked from a tab.
    pub fn app_registry(&self) -> &Arc<AppRegistry> {
        &self.app_registry
    }

    /// Sends one command to the app registry.
    pub fn send_app(&self, command: AppCommand) -> Result<Reply, WorkspaceError> {
        self.app_registry.send(command)
    }

    /// The last published view for `key`. Starts the actor if it is not up, so
    /// a reader never sees a missing handle as an error.
    pub fn view(&self, key: &str) -> Arc<WorkspaceView> {
        self.handle_for(key).view()
    }
}

/// One thread per actor, alive as long as the actor: each published view's
/// revision goes to the relay.
fn forward_revisions(handle: &WorkspaceHandle, relay: RevisionRelay) {
    let events = handle.subscribe();
    let _ = std::thread::Builder::new()
        .name("er-workspace-relay".into())
        .spawn(move || {
            for event in events {
                if let Event::ViewPublished { revision } = event {
                    relay(revision);
                }
            }
        });
}

/// Like [`forward_revisions`], for the app registry's tab list.
fn forward_tab_revisions(registry: &AppRegistry, relay: RevisionRelay) {
    let events = registry.subscribe();
    let _ = std::thread::Builder::new()
        .name("er-app-registry-relay".into())
        .spawn(move || {
            for event in events {
                if let Event::TabsChanged { revision } = event {
                    relay(revision);
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use er_engine::github::FakeGitHub;
    use er_engine::ports::Ports;

    use super::ServiceState;

    /// A flag-path change reaches the relay as an event; nothing polls the
    /// desktop revision for it.
    #[test]
    fn an_actor_revision_reaches_the_relay() {
        let github = Arc::new(FakeGitHub::default().with_pr_state(Ok("OPEN".to_string())));
        let (tx, revisions) = std::sync::mpsc::channel();
        let tx = std::sync::Mutex::new(tx);
        let service = ServiceState::with_revision_relay(Ports::with_github(github), move |rev| {
            if let Ok(tx) = tx.lock() {
                let _ = tx.send(rev);
            }
        });
        let target = er_service::OpenTarget::RemotePr {
            owner: "acme".to_string(),
            repo: "widgets".to_string(),
            number: 7,
            repo_root: "/repo".to_string(),
        };
        let opened = service.open_target("/repo", target, false);
        assert!(opened.is_ok(), "{opened:?}");
        assert_eq!(revisions.recv_timeout(Duration::from_secs(5)), Ok(1));
    }

    /// A tab command's new list reaches the relay the same way, so the tab
    /// strip refreshes on the event rather than a poll.
    #[test]
    fn a_tab_revision_reaches_the_relay() {
        let tmp = tempfile::tempdir().expect("temp dir");
        er_engine::storage::with_test_storage_root(tmp.path(), || {
            let (tx, revisions) = std::sync::mpsc::channel();
            let tx = std::sync::Mutex::new(tx);
            let service = ServiceState::with_revision_relay(Ports::for_test(), move |rev| {
                if let Ok(tx) = tx.lock() {
                    let _ = tx.send(rev);
                }
            });
            let tab = er_engine::tabs_record::TabDescriptor {
                kind: er_engine::tabs_record::TabKind::Working,
                repo_root: "/repo".to_string(),
                branch: None,
                pr_owner: None,
                pr_repo: None,
                pr_number: None,
                pr_head_ref: None,
                base_ref: None,
                browser_url: None,
                browser_layout: None,
            };
            let added = service.send_app(er_service::AppCommand::NewTab { tab });
            assert!(added.is_ok(), "{added:?}");
            assert_eq!(revisions.recv_timeout(Duration::from_secs(5)), Ok(1));
        });
    }
}
