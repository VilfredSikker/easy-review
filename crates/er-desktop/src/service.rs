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
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};

use er_engine::ports::Ports;
use er_engine::tabs_record::TabDescriptor;
use er_engine::TabKey;
use er_service::{
    spawn, spawn_settings, AppCommand, AppRegistry, Command, Event, OpenTarget, Reply,
    SettingsHandle, TabContent, WorkspaceDeps, WorkspaceError, WorkspaceHandle, WorkspaceView,
};

/// Told the revision of every view an actor publishes.
type RevisionRelay = Arc<dyn Fn(u64) + Send + Sync>;

/// The actors this desktop process has started.
pub struct ServiceState {
    deps: WorkspaceDeps,
    handles: Mutex<HashMap<String, WorkspaceHandle>>,
    relay: Option<RevisionRelay>,
    /// Woken after each view an actor publishes and each tab-list change, so
    /// the resource relay reads the versions again.
    resource_wake: Option<Sender<()>>,
    /// The one settings actor. A fixed slot, never keyed by a tab: the config
    /// is app-scoped, so a per-repo owner would race its saves.
    settings: OnceLock<SettingsHandle>,
    /// `config.toml` for the settings actor; the managed path when unset.
    settings_path: Option<PathBuf>,
    /// The one app registry: the tab list and the PR open caches. A fixed slot,
    /// never keyed by a tab, since its records are app-scoped.
    app_registry: Arc<AppRegistry>,
}

impl ServiceState {
    pub fn new(ports: Ports) -> Self {
        Self::from_deps(WorkspaceDeps::new(ports), None)
    }

    /// A service whose settings actor owns `config_path` instead of the
    /// managed config, so a test never reaches the owner's files.
    pub fn with_settings_path(ports: Ports, config_path: PathBuf) -> Self {
        Self {
            settings_path: Some(config_path),
            ..Self::new(ports)
        }
    }

    /// Like [`Self::new`], with every actor's published revisions forwarded to
    /// `relay`. The desktop bumps its revision from it, so a flag-path change
    /// reaches the front end as an `er://revision` event instead of waiting
    /// for a poll; a live arena run's progress reaches the page this way.
    pub fn with_revision_relay(ports: Ports, relay: impl Fn(u64) + Send + Sync + 'static) -> Self {
        Self::with_deps(WorkspaceDeps::new(ports), relay)
    }

    /// The same, over deps the caller built, such as a test's job sink. The
    /// app registry's tab revisions reach `relay` too.
    pub fn with_deps(deps: WorkspaceDeps, relay: impl Fn(u64) + Send + Sync + 'static) -> Self {
        let relay: RevisionRelay = Arc::new(relay);
        let service = Self::from_deps(deps, Some(Arc::clone(&relay)));
        forward_tab_revisions(&service.app_registry, relay);
        service
    }

    /// The same service, waking `wake` after each view an actor publishes and
    /// each tab-list change. Set it before any actor starts: an actor started
    /// earlier is not followed.
    pub fn with_resource_wake(mut self, wake: Sender<()>) -> Self {
        forward_wakes(self.app_registry.subscribe(), wake.clone());
        self.resource_wake = Some(wake);
        self
    }

    /// Gives `deps` the one app registry: the one it already carries, or a new
    /// one over its ports.
    fn from_deps(deps: WorkspaceDeps, relay: Option<RevisionRelay>) -> Self {
        let app_registry = match deps.app_registry() {
            Some(registry) => Arc::clone(registry),
            None => Arc::new(AppRegistry::new(deps.ports().clone())),
        };
        Self {
            deps: deps.with_app_registry(Arc::clone(&app_registry)),
            handles: Mutex::new(HashMap::new()),
            relay,
            resource_wake: None,
            settings: OnceLock::new(),
            settings_path: None,
            app_registry,
        }
    }

    /// The actor for `key`, started on first use. The key is the tab's
    /// `workspace_key`.
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
                if let Some(wake) = &self.resource_wake {
                    forward_wakes(handle.subscribe(), wake.clone());
                }
                handle
            })
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

    /// The app registry. Tab and open-cache commands go here, never to a
    /// workspace actor picked from a tab.
    pub fn app_registry(&self) -> &Arc<AppRegistry> {
        &self.app_registry
    }

    /// Runs `follow` on its own thread after each tab-list change, so the App
    /// follows the registry without a tab command waiting on the App lock.
    pub fn follow_tabs(&self, follow: impl Fn() + Send + 'static) {
        let events = self.app_registry.subscribe();
        let _ = std::thread::Builder::new()
            .name("er-tab-follower".into())
            .spawn(move || {
                for event in events {
                    if matches!(event, Event::TabsChanged { .. }) {
                        follow();
                    }
                }
            });
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

    /// The last published view for `key` when its actor is up. Never starts
    /// one, so the poll can read it under the engine lock.
    pub fn published_view(&self, key: &str) -> Option<Arc<WorkspaceView>> {
        self.started(key).as_ref().map(WorkspaceHandle::view)
    }

    /// The content `key` holds on `tab`'s actor, when that actor is up and has
    /// loaded it. Never starts an actor.
    pub fn content(&self, key: TabKey, tab: &TabDescriptor) -> Option<Arc<TabContent>> {
        self.published_view(&actor_key(tab))?.tab(key).cloned()
    }

    /// Sends `command` to `key`'s actor when it is up; `None` when it is not,
    /// so a command with nothing to act on starts no actor.
    pub fn send_if_started(
        &self,
        key: &str,
        command: Command,
    ) -> Option<Result<Reply, WorkspaceError>> {
        Some(self.started(key)?.send_and_wait(command))
    }

    /// Sends `command` to `key`'s actor when it is up, without waiting for
    /// the reply. Answers whether it was sent.
    pub fn post_if_started(&self, key: &str, command: Command) -> bool {
        self.started(key)
            .map(|handle| drop(handle.send(command)))
            .is_some()
    }

    fn started(&self, key: &str) -> Option<WorkspaceHandle> {
        let handles = match self.handles.lock() {
            Ok(handles) => handles,
            Err(poisoned) => poisoned.into_inner(),
        };
        handles.get(key).cloned()
    }

    /// The last published view of every actor that is up. Starts none.
    pub fn published_views(&self) -> Vec<Arc<WorkspaceView>> {
        let handles: Vec<WorkspaceHandle> = match self.handles.lock() {
            Ok(handles) => handles.values().cloned().collect(),
            Err(poisoned) => poisoned.into_inner().values().cloned().collect(),
        };
        handles.iter().map(WorkspaceHandle::view).collect()
    }

    /// How many workspace actors this process has started.
    #[cfg(test)]
    pub(crate) fn started_actors(&self) -> usize {
        match self.handles.lock() {
            Ok(handles) => handles.len(),
            Err(poisoned) => poisoned.into_inner().len(),
        }
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

    /// The key of the started actor whose published view holds the review
    /// flow on `er_dir`. Starts no actor.
    pub fn flow_key(&self, er_dir: &str) -> Option<String> {
        let handles: Vec<(String, WorkspaceHandle)> = match self.handles.lock() {
            Ok(handles) => handles
                .iter()
                .map(|(k, h)| (k.clone(), h.clone()))
                .collect(),
            Err(poisoned) => {
                let handles = poisoned.into_inner();
                handles
                    .iter()
                    .map(|(k, h)| (k.clone(), h.clone()))
                    .collect()
            }
        };
        handles
            .into_iter()
            .find(|(_, handle)| handle.view().review_flow(er_dir).is_some())
            .map(|(key, _)| key)
    }
}

/// The actor a tab's content lives on. The open routes key the same way.
pub fn actor_key(tab: &TabDescriptor) -> String {
    tab.workspace_key()
}

/// One thread per actor, alive as long as the actor, relaying its events.
fn forward_revisions(handle: &WorkspaceHandle, relay: RevisionRelay) {
    let events = handle.subscribe();
    let _ = std::thread::Builder::new()
        .name("er-workspace-relay".into())
        .spawn(move || relay_events(events, relay.as_ref()));
}

/// Each event that changes what the front end reads goes to the relay with the
/// revision to re-read. A panel record carries no revision of its own, so it
/// re-sends the latest.
fn relay_events(events: impl IntoIterator<Item = Event>, relay: &dyn Fn(u64)) {
    let mut latest = 0;
    for event in events {
        if !bumps_revision(&event) {
            continue;
        }
        if let Event::ViewPublished { revision } | Event::WorkflowChanged { revision, .. } = event {
            latest = revision;
        }
        relay(latest);
    }
}

/// A published view, a flow's record change, and a panel record the view does
/// not carry all change what the page shows, so each makes it re-read.
fn bumps_revision(event: &Event) -> bool {
    matches!(
        event,
        Event::ViewPublished { .. } | Event::WorkflowChanged { .. } | Event::PanelRunChanged { .. }
    )
}

/// One thread per subscription, waking `wake` on each published view or tab
/// list. It ends with the subscription, or when nothing reads the wakes.
fn forward_wakes(events: std::sync::mpsc::Receiver<Event>, wake: Sender<()>) {
    let _ = std::thread::Builder::new()
        .name("er-resource-wake".into())
        .spawn(move || {
            for event in events {
                let publishes = matches!(
                    event,
                    Event::ViewPublished { .. } | Event::TabsChanged { .. }
                );
                if publishes && wake.send(()).is_err() {
                    return;
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

    use er_service::Event;

    use super::{bumps_revision, relay_events, ServiceState};

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

    fn relayed(events: Vec<Event>) -> Vec<u64> {
        let seen = std::sync::Mutex::new(Vec::new());
        relay_events(events, &|revision| {
            if let Ok(mut seen) = seen.lock() {
                seen.push(revision);
            }
        });
        seen.into_inner().unwrap_or_default()
    }

    /// A panel record the view does not carry makes the page re-read on its
    /// own, with no view published beside it, at the latest revision.
    #[test]
    fn a_lone_panel_record_reaches_the_relay() {
        let panel = || Event::PanelRunChanged {
            er_dir: "bucket".into(),
            run_id: "run".into(),
        };
        assert_eq!(relayed(vec![panel()]), vec![0]);
        assert_eq!(
            relayed(vec![
                Event::ViewPublished { revision: 5 },
                Event::CommandFailed {
                    message: "no".into(),
                },
                panel(),
            ]),
            vec![5, 5]
        );
    }

    /// Reading a tab's content, or dropping it, for a repo with no actor
    /// starts none: a reader must never pay an actor's start.
    #[test]
    fn reading_content_starts_no_actor() {
        let service = ServiceState::new(Ports::for_test());
        let tab = er_engine::tabs_record::TabDescriptor {
            kind: er_engine::tabs_record::TabKind::Working,
            repo_root: "/nowhere".to_string(),
            branch: None,
            pr_owner: None,
            pr_repo: None,
            pr_number: None,
            pr_head_ref: None,
            base_ref: None,
            extra: serde_json::Map::new(),
        };
        let key = er_engine::TabKey::next();

        assert!(service.content(key, &tab).is_none());
        let dropped = service.send_if_started("/nowhere", er_service::Command::DropTab { key });

        assert!(dropped.is_none());
        assert_eq!(service.started_actors(), 0, "a read started an actor");
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
                extra: serde_json::Map::new(),
            };
            let added = service.send_app(er_service::AppCommand::NewTab { tab });
            assert!(added.is_ok(), "{added:?}");
            assert_eq!(revisions.recv_timeout(Duration::from_secs(5)), Ok(1));
        });
    }

    /// A tab change runs the follower on its own thread, which is how the
    /// flag-on App follows the registry without a tab command waiting on it.
    #[test]
    fn a_tab_change_runs_the_follower() {
        let tmp = tempfile::tempdir().expect("temp dir");
        er_engine::storage::with_test_storage_root(tmp.path(), || {
            let service = ServiceState::new(Ports::for_test());
            let (tx, followed) = std::sync::mpsc::channel();
            let tx = std::sync::Mutex::new(tx);
            service.follow_tabs(move || {
                if let Ok(tx) = tx.lock() {
                    let _ = tx.send(());
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
                extra: serde_json::Map::new(),
            };
            let added = service.send_app(er_service::AppCommand::NewTab { tab });
            assert!(added.is_ok(), "{added:?}");
            assert_eq!(followed.recv_timeout(Duration::from_secs(5)), Ok(()));
        });
    }
}
