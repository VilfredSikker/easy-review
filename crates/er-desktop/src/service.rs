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
    spawn, Command, Event, OpenTarget, Reply, WorkspaceDeps, WorkspaceError, WorkspaceHandle,
    WorkspaceView,
};

/// Told the revision of every view an actor publishes.
type RevisionRelay = Arc<dyn Fn(u64) + Send + Sync>;

/// The actors this desktop process has started.
pub struct ServiceState {
    deps: WorkspaceDeps,
    handles: Mutex<HashMap<String, WorkspaceHandle>>,
    relay: Option<RevisionRelay>,
}

impl ServiceState {
    pub fn new(ports: Ports) -> Self {
        Self {
            deps: WorkspaceDeps::new(ports),
            handles: Mutex::new(HashMap::new()),
            relay: None,
        }
    }

    /// Like [`Self::new`], with every actor's published revisions forwarded to
    /// `relay`. The desktop bumps its revision from it, so a flag-path change
    /// reaches the front end as an `er://revision` event instead of waiting
    /// for a poll; a live arena run's progress reaches the page this way.
    pub fn with_revision_relay(ports: Ports, relay: impl Fn(u64) + Send + Sync + 'static) -> Self {
        Self::with_deps(WorkspaceDeps::new(ports), relay)
    }

    /// The same, over deps the caller built, such as a test's job sink.
    pub fn with_deps(deps: WorkspaceDeps, relay: impl Fn(u64) + Send + Sync + 'static) -> Self {
        Self {
            deps,
            handles: Mutex::new(HashMap::new()),
            relay: Some(Arc::new(relay)),
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

/// One thread per actor, alive as long as the actor: each event that changes
/// what the front end reads goes to the relay with the revision to re-read.
/// A panel record carries no revision of its own, so it re-sends the latest.
fn forward_revisions(handle: &WorkspaceHandle, relay: RevisionRelay) {
    let events = handle.subscribe();
    let _ = std::thread::Builder::new()
        .name("er-workspace-relay".into())
        .spawn(move || {
            let mut latest = 0;
            for event in events {
                if !bumps_revision(&event) {
                    continue;
                }
                if let Event::ViewPublished { revision } | Event::WorkflowChanged { revision, .. } =
                    event
                {
                    latest = revision;
                }
                relay(latest);
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
    use std::sync::Arc;
    use std::time::Duration;

    use er_engine::github::FakeGitHub;
    use er_engine::ports::Ports;

    use er_service::Event;

    use super::{bumps_revision, ServiceState};

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
