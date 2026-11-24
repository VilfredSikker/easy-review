//! One process's service: a workspace actor per workspace key, the settings
//! actor and one app registry, each started on first use.
//!
//! A front end that runs the service in its own process holds one host. Its
//! registry lives in memory: that front end's tabs come from its own start,
//! so the host never reads or writes another front end's `tabs.json` or
//! `pr-open-cache.json`. Every actor gets the registry in its deps, so a load
//! for a key the registry does not list is refused on every tab.
//!
//! Nothing here waits on an actor. A command goes out with [`ServiceHost::post`],
//! which answers the reply receiver at once; the caller drains it when it
//! next looks. Events from every actor, the settings actor, the registry and
//! the task host reach one sender, so a front end's loop can wake on any of
//! them. A run or a flow command goes out the same way, through
//! [`ServiceHost::run`] and [`ServiceHost::flow`]: the host plans it on its
//! worker, since planning reads git (ADR 0074).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use er_engine::ports::Ports;
use er_engine::TabKey;

use crate::actor::{spawn, WorkspaceDeps};
use crate::app_registry::AppRegistry;
use crate::content::TabContent;
use crate::handle::{WorkspaceError, WorkspaceHandle};
use crate::protocol::{Command, Event, Reply};
use crate::run_worker::{self, FlowInputs, FlowRequest, RunReceiver, RunRequest, Worker};
use crate::settings::{spawn_settings, SettingsHandle};
use crate::tasks::{TaskHooks, TaskHost};
use crate::workspace::WorkspaceView;

/// What a posted command answers on, once.
pub type ReplyReceiver = Receiver<Result<Reply, WorkspaceError>>;

/// The actors one process has started, and its one registry.
pub struct ServiceHost {
    deps: WorkspaceDeps,
    registry: Arc<AppRegistry>,
    handles: Arc<Mutex<HashMap<String, WorkspaceHandle>>>,
    settings: OnceLock<SettingsHandle>,
    settings_path: PathBuf,
    events: Option<Sender<Event>>,
    tasks: OnceLock<TaskHost>,
    worker: OnceLock<Worker>,
    /// The sender the task host's events go to, read at each send, so a host
    /// whose tasks started before `with_events` still sends them.
    task_events: Arc<Mutex<Option<Sender<Event>>>>,
}

impl ServiceHost {
    /// A host whose settings actor owns `settings_path`. The caller names the
    /// config file, so a test never reaches the managed one.
    pub fn new(ports: Ports, settings_path: PathBuf) -> Self {
        Self::with_deps(WorkspaceDeps::new(ports), settings_path)
    }

    /// The same, over deps the caller built, such as a test's job sink. A
    /// registry the deps already carry is replaced by the host's own, which
    /// never persists.
    pub fn with_deps(deps: WorkspaceDeps, settings_path: PathBuf) -> Self {
        let registry = Arc::new(AppRegistry::in_memory(deps.ports().clone()));
        Self {
            deps: deps.with_app_registry(Arc::clone(&registry)),
            registry,
            handles: Arc::default(),
            settings: OnceLock::new(),
            settings_path,
            events: None,
            tasks: OnceLock::new(),
            worker: OnceLock::new(),
            task_events: Arc::default(),
        }
    }

    /// Sends every event from the registry, and from each actor started from
    /// now on, to `events`. Set it before any actor starts.
    #[must_use]
    pub fn with_events(mut self, events: Sender<Event>) -> Self {
        forward(self.registry.subscribe(), events.clone());
        *lock(&self.task_events) = Some(events.clone());
        self.events = Some(events);
        self
    }

    /// The ports every actor reads through, for a front end's own reads that
    /// no command covers, such as the TUI's start-up PR probe.
    pub fn ports(&self) -> &Ports {
        self.deps.ports()
    }

    /// The one app registry. Its commands run on the caller's thread and
    /// touch no file.
    pub fn registry(&self) -> &Arc<AppRegistry> {
        &self.registry
    }

    /// The actor for a workspace key, started on first use. Two tabs on one
    /// repo share it.
    pub fn handle_for(&self, workspace_key: &str) -> WorkspaceHandle {
        lock(&self.handles)
            .entry(workspace_key.to_string())
            .or_insert_with(|| {
                let handle = spawn(self.deps.clone());
                if let Some(events) = &self.events {
                    forward(handle.subscribe(), events.clone());
                }
                handle
            })
            .clone()
    }

    /// Sends `command` to the actor for `workspace_key` and answers at once
    /// with the receiver its reply lands on.
    pub fn post(&self, workspace_key: &str, command: Command) -> ReplyReceiver {
        self.handle_for(workspace_key).send(command)
    }

    /// The last view the actor for `workspace_key` published, when it is up.
    /// Starts no actor.
    pub fn published_view(&self, workspace_key: &str) -> Option<Arc<WorkspaceView>> {
        lock(&self.handles)
            .get(workspace_key)
            .map(WorkspaceHandle::view)
    }

    /// The content loaded under `key` on the actor for `workspace_key`.
    /// Starts no actor.
    pub fn content(&self, workspace_key: &str, key: TabKey) -> Option<Arc<TabContent>> {
        self.published_view(workspace_key)?.tab(key).cloned()
    }

    /// The settings actor, started on first use. It reads the config before
    /// its loop starts.
    pub fn settings(&self) -> SettingsHandle {
        self.settings
            .get_or_init(|| {
                let handle = spawn_settings(self.deps.clone(), self.settings_path.clone());
                if let Some(events) = &self.events {
                    forward(handle.subscribe(), events.clone());
                }
                handle
            })
            .clone()
    }

    /// The settings actor when it is up. Never starts it.
    pub fn settings_if_started(&self) -> Option<SettingsHandle> {
        self.settings.get().cloned()
    }

    /// How many workspace actors this host has started.
    pub fn actor_count(&self) -> usize {
        lock(&self.handles).len()
    }

    /// The app's one task host. Each row change sends `TasksChanged`.
    pub fn tasks(&self) -> &TaskHost {
        self.tasks.get_or_init(|| {
            let events = Arc::clone(&self.task_events);
            TaskHost::new(move |revision| send(&events, Event::TasksChanged { revision }))
        })
    }

    /// What a task on this host reads and tells: the settings view's config,
    /// and `TaskNotice` events for its messages. A committed importance run
    /// reloads its repo's risk rules and refreshes the loaded tabs on it.
    pub fn task_hooks(&self) -> TaskHooks {
        let settings = self.settings();
        let events = Arc::clone(&self.task_events);
        let notify = Arc::new(move |message, long| {
            send(&events, Event::TaskNotice { message, long });
        });
        let (registry, handles) = (Arc::clone(&self.registry), Arc::clone(&self.handles));
        let (rules, told) = (settings.clone(), Arc::clone(&notify));
        TaskHooks {
            ports: self.deps.ports().clone(),
            config: Arc::new(move || Arc::clone(&settings.view().config)),
            notify,
            finished: Arc::new(move |task| {
                if !crate::tasks::committed_importance(task.row) {
                    return;
                }
                let handle = |entry: &crate::TabEntry| {
                    lock(&handles)
                        .get(&entry.descriptor.workspace_key())
                        .cloned()
                };
                let reloaded = crate::tasks::reload_risk_rules(
                    &registry.tabs_view(),
                    task.target,
                    &rules,
                    |entry| handle(entry)?.view().tab(entry.key).cloned(),
                    |entry| {
                        if let Some(handle) = handle(entry) {
                            let _ = handle.send(Command::RefreshDiff {
                                key: entry.key,
                                force: false,
                                adopt_head_oid: None,
                            });
                        }
                    },
                );
                let message = match reloaded {
                    Ok(()) => "Risk rules committed — see the task log for its report".into(),
                    Err(error) => format!("Risk rules committed, but not reloaded: {error}"),
                };
                told(message, true);
            }),
        }
    }

    /// Plans on the worker; the content is the tab's when asked, whatever is
    /// active when it plans.
    pub fn run(&self, key: TabKey, request: RunRequest) -> RunReceiver {
        let Some(content) = self.listed_content(key) else {
            return run_worker::refused_now("The tab has not loaded yet");
        };
        run_worker::plan(
            self.worker(),
            self.tasks().clone(),
            self.task_hooks(),
            content,
            request,
        )
    }

    pub fn flow(&self, key: TabKey, request: FlowRequest) -> ReplyReceiver {
        let tabs = self.registry.tabs_view();
        let Some(entry) = tabs.tabs.iter().find(|entry| entry.key == key) else {
            return failed_now("no open tab has that key");
        };
        let workspace_key = entry.descriptor.workspace_key();
        let Some(content) = self.content(&workspace_key, key) else {
            return failed_now("The tab has not loaded yet");
        };
        if request
            .controlled_bucket()
            .is_some_and(|bucket| bucket != content.context.er_dir)
        {
            return failed_now(
                "The active review bucket changed. Refresh before controlling this review.",
            );
        }
        let actor = self.flow_actor(&workspace_key, &request);
        let contents = tabs
            .tabs
            .iter()
            .filter_map(|entry| self.content(&entry.descriptor.workspace_key(), entry.key))
            .collect();
        let inputs = FlowInputs {
            ports: self.deps.ports().clone(),
            config: Arc::clone(&self.settings().view().config),
            content,
            handle: self.handle_for(&actor),
            contents,
        };
        run_worker::flow(self.worker(), inputs, request)
    }

    /// An acceptance goes to the actor holding the flow, since the user may
    /// have switched to another project's tab.
    pub(crate) fn flow_actor(&self, workspace_key: &str, request: &FlowRequest) -> String {
        let FlowRequest::Accept { er_dir, .. } = request else {
            return workspace_key.to_string();
        };
        lock(&self.handles)
            .iter()
            .find(|(_, handle)| handle.view().review_flow(er_dir).is_some())
            .map_or_else(|| workspace_key.to_string(), |(key, _)| key.clone())
    }

    fn worker(&self) -> &Worker {
        self.worker.get_or_init(Worker::start)
    }

    fn listed_content(&self, key: TabKey) -> Option<Arc<TabContent>> {
        let tabs = self.registry.tabs_view();
        let entry = tabs.tabs.iter().find(|entry| entry.key == key)?;
        self.content(&entry.descriptor.workspace_key(), key)
    }
}

fn failed_now(message: &str) -> ReplyReceiver {
    let (tx, rx) = std::sync::mpsc::channel();
    let _ = tx.send(Err(WorkspaceError::Failed(message.to_string())));
    rx
}

fn send(events: &Mutex<Option<Sender<Event>>>, event: Event) {
    if let Some(events) = lock(events).as_ref() {
        let _ = events.send(event);
    }
}

/// One thread per subscription, alive until the subscription ends or nothing
/// reads `events`.
fn forward(subscription: Receiver<Event>, events: Sender<Event>) {
    let _ = std::thread::Builder::new()
        .name("er-host-events".into())
        .spawn(move || {
            for event in subscription {
                if events.send(event).is_err() {
                    return;
                }
            }
        });
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use er_engine::pr_open_cache::save_persisted_pr_open_cache;
    use er_engine::pr_open_entry::{PrOpenCacheEntry, PrOpenCacheKey, PrOpenFreshness};
    use er_engine::tabs_record::{save_tabs, TabDescriptor, TabKind, TabsFile};

    use super::*;
    use crate::app_registry::AppCommand;

    const WAIT: Duration = Duration::from_secs(5);

    fn working(root: &str) -> TabDescriptor {
        TabDescriptor {
            kind: TabKind::Working,
            repo_root: root.to_string(),
            branch: None,
            pr_owner: None,
            pr_repo: None,
            pr_number: None,
            pr_head_ref: None,
            base_ref: None,
            extra: serde_json::Map::new(),
        }
    }

    fn host(dir: &std::path::Path) -> ServiceHost {
        ServiceHost::new(Ports::for_test(), dir.join("config.toml"))
    }

    fn new_tab(host: &ServiceHost, tab: TabDescriptor) -> anyhow::Result<TabKey> {
        match host.registry().send(AppCommand::NewTab { tab })? {
            Reply::Tabs(view) => view.active.ok_or_else(|| anyhow::anyhow!("no active tab")),
            other => anyhow::bail!("a new tab answered {other:?}"),
        }
    }

    /// Two tabs on one repo share one actor; a tab on another repo starts a
    /// second. Nothing starts before its first use.
    #[test]
    fn two_tabs_on_one_repo_share_one_actor() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let host = host(dir.path());
        assert_eq!(host.actor_count(), 0);
        assert!(host.settings_if_started().is_none());

        let (first, second) = (working("/repo"), working("/repo"));
        host.handle_for(&first.workspace_key());
        host.handle_for(&second.workspace_key());
        assert_eq!(host.actor_count(), 1, "one repo started two actors");

        host.handle_for(&working("/other").workspace_key());
        assert_eq!(host.actor_count(), 2);
        assert!(host.settings_if_started().is_none());
        Ok(())
    }

    /// A load for a key the host's registry does not list is refused at
    /// dispatch, on every actor the host starts.
    #[test]
    fn a_load_for_an_unlisted_key_is_refused() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        er_engine::storage::with_test_storage_root(dir.path(), || -> anyhow::Result<()> {
            let host = host(dir.path());
            let tab = working("/repo");
            new_tab(&host, tab.clone())?;
            let unlisted = TabKey::next();
            let reply = host
                .post(
                    &tab.workspace_key(),
                    Command::LoadTab {
                        key: unlisted,
                        tab,
                        mode: None,
                    },
                )
                .recv_timeout(WAIT)?;
            assert_eq!(
                reply,
                Err(WorkspaceError::Failed(
                    "the tab closed before its load".into()
                )),
                "an unlisted key reached the load"
            );
            Ok(())
        })
    }

    /// The host's registry neither opens the saved tab list or open cache
    /// nor writes either: both files stay as seeded through tab commands and
    /// a prune.
    #[test]
    fn the_host_registry_reads_and_writes_no_app_record() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        er_engine::storage::with_test_storage_root(dir.path(), || -> anyhow::Result<()> {
            save_tabs(&[working("/saved")], 0)?;
            let cache_key = PrOpenCacheKey {
                project_id: "project".into(),
                repo_root: "/saved".into(),
                pr_number: 7,
            };
            let seeded = Arc::new(Mutex::new(HashMap::from([(
                cache_key,
                PrOpenCacheEntry {
                    freshness: PrOpenFreshness {
                        head_oid: "head".into(),
                        base_branch: "main".into(),
                        head_branch: "feature".into(),
                        updated_at: "2026-11-21T00:00:00Z".into(),
                    },
                    raw_diff: String::new(),
                    pr_data: None,
                    pr_commits: None,
                    last_touched: 0,
                },
            )])));
            save_persisted_pr_open_cache(&seeded);
            let tabs_file = er_engine::store_registry::app_path::<TabsFile>()
                .ok_or_else(|| anyhow::anyhow!("no tabs path"))?;
            let cache_file = tabs_file.with_file_name("pr-open-cache.json");
            let files = [tabs_file, cache_file];
            let before = files
                .iter()
                .map(std::fs::read_to_string)
                .collect::<Result<Vec<_>, _>>()?;

            let host = host(dir.path());
            assert!(
                host.registry().tabs_view().tabs.is_empty(),
                "the host opened the saved tabs"
            );
            assert!(
                host.registry().cached_pr(None, "/saved", 7).is_none(),
                "the host opened the saved open cache"
            );
            let first = new_tab(&host, working("/a"))?;
            new_tab(&host, working("/b"))?;
            host.registry().send(AppCommand::CloseTab { key: first })?;
            host.registry()
                .prune_pr_open_cache(&std::collections::HashSet::new());

            let after = files
                .iter()
                .map(std::fs::read_to_string)
                .collect::<Result<Vec<_>, _>>()?;
            assert_eq!(
                before, after,
                "a tab command or a prune wrote an app record"
            );
            Ok(())
        })
    }

    /// Every actor's events and the registry's reach the one sender, so a
    /// front end's loop can wake on either.
    #[test]
    fn actor_and_registry_events_reach_the_sender() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        er_engine::storage::with_test_storage_root(dir.path(), || -> anyhow::Result<()> {
            let (tx, events) = std::sync::mpsc::channel();
            let github = er_github::FakeGitHub::default().with_pr_state(Ok("OPEN".to_string()));
            let ports = Ports::with_github(Arc::new(github));
            let host = ServiceHost::new(ports, dir.path().join("config.toml")).with_events(tx);
            new_tab(&host, working("/repo"))?;
            assert!(matches!(
                events.recv_timeout(WAIT)?,
                Event::TabsChanged { revision: 1 }
            ));
            let target = crate::protocol::OpenTarget::RemotePr {
                owner: "acme".to_string(),
                repo: "widgets".to_string(),
                number: 7,
                repo_root: "/repo".to_string(),
            };
            let opened = host.post(
                "/repo",
                Command::OpenTarget {
                    target,
                    replace: false,
                },
            );
            assert!(opened.recv_timeout(WAIT)?.is_ok());
            let published = std::iter::from_fn(|| events.recv_timeout(WAIT).ok())
                .any(|event| matches!(event, Event::ViewPublished { .. }));
            assert!(published, "the actor's events did not reach the sender");
            Ok(())
        })
    }
}
