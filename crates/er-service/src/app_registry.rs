//! The app registry: the one service-level owner of the tab list and the PR
//! open caches.
//!
//! `tabs.json` and `pr-open-cache.json` are app records, so no workspace actor
//! can own them: two workspaces would be two writers of one file. The service
//! creates one registry per process at a fixed place, never chosen from a tab;
//! every write to either file goes through it, and workspace actors read or
//! request through it (ADR 0067). App-scoped state has one service-level
//! owner; workspace actors own per-repo state.
//!
//! Its locks cover in-memory changes only. A save takes its own turn lock,
//! snapshots the state under a brief lock and writes after releasing it, so
//! saves land in order; port calls run on effect threads.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, RwLock};
use std::thread::{self, ScopedJoinHandle};

use er_engine::github::GhError;
use er_engine::ports::Ports;
use er_engine::pr_open_cache::{
    load_persisted_pr_open_cache, prune_pr_open_cache, save_persisted_pr_open_cache,
};
use er_engine::pr_open_entry::{
    remember_pr_open_entry, PrOpenCacheEntry, PrOpenCacheKey, PrOpenFreshness,
    PR_COMMIT_CACHE_LIMIT,
};
use er_engine::remote_pr_open_cache::{
    claim_remote_pr_open, get_remote_pr_open_entry, insert_remote_pr_open_entry,
    release_remote_pr_open, RemotePrOpenCache, RemotePrOpenEntry, RemotePrOpenInFlight,
};
use er_engine::tabs_record::{load_tabs, save_tabs, TabDescriptor};
use er_engine::TabKey;

use crate::guard;
use crate::handle::WorkspaceError;
use crate::protocol::{Event, Reply};

/// One open tab: its process-local key and the record it saves as.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabEntry {
    pub key: TabKey,
    pub descriptor: TabDescriptor,
}

/// The tab list as readers see it, published per revision.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TabsView {
    pub revision: u64,
    pub tabs: Vec<TabEntry>,
    pub active: Option<TabKey>,
}

impl TabsView {
    /// The key of the tab at `idx`, for a front end that addresses tabs by
    /// position.
    pub fn key_at(&self, idx: usize) -> Option<TabKey> {
        self.tabs.get(idx).map(|tab| tab.key)
    }

    pub fn active_entry(&self) -> Option<&TabEntry> {
        let active = self.active?;
        self.tabs.iter().find(|tab| tab.key == active)
    }

    fn position(&self, key: TabKey) -> Result<usize, WorkspaceError> {
        self.tabs
            .iter()
            .position(|tab| tab.key == key)
            .ok_or_else(|| WorkspaceError::Failed("no open tab has that key".into()))
    }

    /// The saved record's active index.
    fn active_idx(&self) -> usize {
        self.active
            .and_then(|key| self.position(key).ok())
            .unwrap_or(0)
    }
}

/// A write or warm-up the registry runs. Commands that act on an existing tab
/// name it by key; a position travels beside the key only where the command
/// needs one.
#[derive(Clone, Debug)]
pub enum AppCommand {
    /// Append a tab and make it active.
    NewTab {
        tab: TabDescriptor,
    },
    CloseTab {
        key: TabKey,
    },
    /// Put `tab` in `key`'s place and make it active. It gets a new key, so
    /// content loaded for the old tab is never read as the new one's.
    ReplaceTab {
        key: TabKey,
        tab: TabDescriptor,
    },
    SelectTab {
        key: TabKey,
    },
    /// Move the tab to `to_idx`; the active tab stays active.
    ReorderTabs {
        key: TabKey,
        to_idx: usize,
    },
    /// Warm the local open cache for a project's PR.
    PrefetchOpenTarget {
        project_id: String,
        repo_root: String,
        pr_number: u64,
        freshness: PrOpenFreshness,
    },
    /// Warm the in-memory cache for a PR with no local clone.
    PrefetchRemoteOpenTarget {
        owner: String,
        repo: String,
        number: u64,
    },
}

type OpenCache = Arc<Mutex<HashMap<PrOpenCacheKey, PrOpenCacheEntry>>>;

/// The registry. Shared as an `Arc`; the prefetch effects hold a clone.
pub struct AppRegistry {
    ports: Ports,
    /// Loaded from `tabs.json` on first use.
    tabs: OnceLock<Mutex<TabsView>>,
    view: RwLock<Arc<TabsView>>,
    subscribers: Mutex<Vec<Sender<Event>>>,
    tabs_save: Mutex<()>,
    /// Loaded from `pr-open-cache.json` on first use.
    pr_open: OnceLock<OpenCache>,
    pr_open_in_flight: Mutex<HashSet<(String, u64)>>,
    pr_open_save: Mutex<()>,
    /// Never persisted: remote opens are rare and freshness matters more.
    remote: RemotePrOpenCache,
    remote_in_flight: RemotePrOpenInFlight,
}

impl AppRegistry {
    pub fn new(ports: Ports) -> Self {
        Self {
            ports,
            tabs: OnceLock::new(),
            view: RwLock::new(Arc::new(TabsView::default())),
            subscribers: Mutex::new(Vec::new()),
            tabs_save: Mutex::new(()),
            pr_open: OnceLock::new(),
            pr_open_in_flight: Mutex::new(HashSet::new()),
            pr_open_save: Mutex::new(()),
            remote: Arc::default(),
            remote_in_flight: Arc::default(),
        }
    }

    /// Runs one command. Tab commands answer with the new [`TabsView`] after
    /// the save is tried; a prefetch answers at once and warms on its own
    /// thread.
    pub fn send(self: &Arc<Self>, command: AppCommand) -> Result<Reply, WorkspaceError> {
        match command {
            AppCommand::PrefetchOpenTarget {
                project_id,
                repo_root,
                pr_number,
                freshness,
            } => {
                let key = PrOpenCacheKey {
                    project_id,
                    repo_root,
                    pr_number,
                };
                self.prefetch_local(key, freshness);
                Ok(Reply::Acknowledged)
            }
            AppCommand::PrefetchRemoteOpenTarget {
                owner,
                repo,
                number,
            } => {
                self.prefetch_remote(owner, repo, number);
                Ok(Reply::Acknowledged)
            }
            tab_command => self.apply_tabs(tab_command).map(Reply::Tabs),
        }
    }

    /// The last published tab list. Loads `tabs.json` on the first call.
    pub fn tabs_view(&self) -> Arc<TabsView> {
        self.loaded_tabs();
        match self.view.read() {
            Ok(view) => Arc::clone(&view),
            Err(poisoned) => Arc::clone(&poisoned.into_inner()),
        }
    }

    /// Subscribes to `TabsChanged` and `CommandFailed` from now on.
    pub fn subscribe(&self) -> Receiver<Event> {
        let (tx, rx) = mpsc::channel();
        lock(&self.subscribers).push(tx);
        rx
    }

    /// A warmed local entry, for the open path that consumes it.
    pub fn pr_open_entry(&self, key: &PrOpenCacheKey) -> Option<PrOpenCacheEntry> {
        lock(self.pr_open()).get(key).cloned()
    }

    /// A warmed remote entry; a hit counts as a use for the LRU.
    pub fn remote_pr_open_entry(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Option<RemotePrOpenEntry> {
        get_remote_pr_open_entry(&self.remote, owner, repo, number)
    }

    /// Keeps only the `(project_id, pr_number)` entries in `keep`, then saves.
    pub fn prune_pr_open_cache(&self, keep: &HashSet<(String, u64)>) {
        prune_pr_open_cache(&mut lock(self.pr_open()), keep);
        self.save_pr_open();
    }

    fn loaded_tabs(&self) -> &Mutex<TabsView> {
        self.tabs.get_or_init(|| {
            let loaded = load_tabs().map(restored_view).unwrap_or_default();
            self.publish(&loaded);
            Mutex::new(loaded)
        })
    }

    /// A change that applied is published even when its save fails: the list
    /// in memory already holds it, and the next save writes it. The failure
    /// goes out as `CommandFailed`, as the flag-off path logs and carries on.
    fn apply_tabs(&self, command: AppCommand) -> Result<TabsView, WorkspaceError> {
        let view = {
            let mut state = lock(self.loaded_tabs());
            apply_tab_command(&mut state, command)?;
            state.revision += 1;
            state.clone()
        };
        if let Err(error) = self.save_tabs_record() {
            self.report(&error);
        }
        self.publish(&view);
        self.broadcast(Event::TabsChanged {
            revision: view.revision,
        });
        Ok(view)
    }

    /// Saves the current list. The turn lock orders saves, and the state is
    /// read inside the turn, so the last save always writes the newest list.
    fn save_tabs_record(&self) -> Result<(), WorkspaceError> {
        let _turn = lock(&self.tabs_save);
        let (descriptors, active_idx) = {
            let state = lock(self.loaded_tabs());
            let descriptors: Vec<TabDescriptor> = state
                .tabs
                .iter()
                .map(|tab| tab.descriptor.clone())
                .collect();
            (descriptors, state.active_idx())
        };
        save_tabs(&descriptors, active_idx).map_err(|e| WorkspaceError::Failed(e.to_string()))
    }

    /// Publishes `view` unless a newer revision is already out.
    fn publish(&self, view: &TabsView) {
        let mut current = match self.view.write() {
            Ok(current) => current,
            Err(poisoned) => poisoned.into_inner(),
        };
        if current.revision <= view.revision {
            *current = Arc::new(view.clone());
        }
    }

    fn broadcast(&self, event: Event) {
        lock(&self.subscribers).retain(|sender| sender.send(event.clone()).is_ok());
    }

    fn pr_open(&self) -> &OpenCache {
        self.pr_open.get_or_init(|| {
            let loaded = load_persisted_pr_open_cache().ok().flatten();
            Arc::new(Mutex::new(loaded.unwrap_or_default()))
        })
    }

    fn save_pr_open(&self) {
        let _turn = lock(&self.pr_open_save);
        save_persisted_pr_open_cache(self.pr_open());
    }

    fn prefetch_local(self: &Arc<Self>, key: PrOpenCacheKey, freshness: PrOpenFreshness) {
        let fresh = lock(self.pr_open())
            .get(&key)
            .is_some_and(|entry| entry.freshness == freshness);
        let claim = (key.project_id.clone(), key.pr_number);
        if fresh || !lock(&self.pr_open_in_flight).insert(claim.clone()) {
            return;
        }
        let (registry, release) = (Arc::clone(self), claim.clone());
        // Unnamed, like every effect thread (see `actor::spawn_effect`).
        let spawned = thread::Builder::new().spawn(move || {
            match fetch_local(&registry.ports, &key, &freshness) {
                Ok((raw_diff, commits)) => {
                    remember_pr_open_entry(
                        &mut lock(registry.pr_open()),
                        key,
                        freshness,
                        raw_diff,
                        None,
                        Some(commits),
                    );
                    registry.save_pr_open();
                }
                Err(error) => registry.report(&error),
            }
            lock(&registry.pr_open_in_flight).remove(&claim);
        });
        if spawned.is_err() {
            lock(&self.pr_open_in_flight).remove(&release);
        }
    }

    fn prefetch_remote(self: &Arc<Self>, owner: String, repo: String, number: u64) {
        if get_remote_pr_open_entry(&self.remote, &owner, &repo, number).is_some()
            || !claim_remote_pr_open(&self.remote_in_flight, &owner, &repo, number)
        {
            return;
        }
        let registry = Arc::clone(self);
        let (claim_owner, claim_repo) = (owner.clone(), repo.clone());
        let spawned = thread::Builder::new().spawn(move || {
            match fetch_remote(&registry.ports, &owner, &repo, number) {
                Ok(entry) => {
                    insert_remote_pr_open_entry(&registry.remote, &owner, &repo, number, entry);
                }
                Err(error) => registry.report(&error),
            }
            release_remote_pr_open(&registry.remote_in_flight, &owner, &repo, number);
        });
        if spawned.is_err() {
            release_remote_pr_open(&self.remote_in_flight, &claim_owner, &claim_repo, number);
        }
    }

    fn report(&self, error: &WorkspaceError) {
        self.broadcast(Event::CommandFailed {
            message: error.to_string(),
        });
    }
}

/// The local open inputs, through the ports: the diff, the base ref and the
/// commits in parallel. The comment bundle warms on its own thread.
fn fetch_local(
    ports: &Ports,
    key: &PrOpenCacheKey,
    freshness: &PrOpenFreshness,
) -> Result<(String, Vec<er_engine::git::CommitInfo>), WorkspaceError> {
    guard::assert_off_actor();
    let (git, github) = (&*ports.git, &*ports.github);
    let (root, number) = (key.repo_root.as_str(), key.pr_number);
    warm_comment_bundle(ports, Some(root.to_string()), None, number);
    thread::scope(|s| {
        let diff = s.spawn(|| github.pr_diff(git, number, root));
        let base = s.spawn(|| {
            er_engine::github::ensure_base_ref_available(git, root, &freshness.base_branch).map_err(
                |e| GhError::Local {
                    message: e.to_string(),
                },
            )
        });
        let commits = s.spawn(|| github.pr_commits(root, number, PR_COMMIT_CACHE_LIMIT));
        let raw_diff = joined(diff)?;
        joined(base)?;
        Ok((raw_diff, joined(commits).unwrap_or_default()))
    })
}

/// The remote open inputs, through the GitHub port. The diff is kept only with
/// the head oid it was fetched at, and only when the head did not move during
/// the fetch.
fn fetch_remote(
    ports: &Ports,
    owner: &str,
    repo: &str,
    number: u64,
) -> Result<RemotePrOpenEntry, WorkspaceError> {
    guard::assert_off_actor();
    let (git, github) = (&*ports.git, &*ports.github);
    warm_comment_bundle(
        ports,
        None,
        Some((owner.to_string(), repo.to_string())),
        number,
    );
    let head_before = github.pr_head_oid_remote(owner, repo, number).ok();
    let (overview, diff, commits) = thread::scope(|s| {
        let overview = s.spawn(|| github.overview_remote(owner, repo, number));
        let diff = s.spawn(|| github.pr_diff_remote(git, owner, repo, number));
        let commits = s.spawn(|| github.commits_remote(owner, repo, number, PR_COMMIT_CACHE_LIMIT));
        (joined(overview), joined(diff), joined(commits))
    });
    let overview = overview?.ok_or_else(|| {
        WorkspaceError::Failed(format!("{owner}/{repo}#{number} returned no PR overview"))
    })?;
    let raw_diff = diff?;
    let head_after = github.pr_head_oid_remote(owner, repo, number).ok();
    Ok(RemotePrOpenEntry {
        base_branch: overview.base_branch.clone(),
        head_branch: overview.head_branch.clone(),
        raw_diff,
        pr_data: Some(overview),
        pr_commits: commits.unwrap_or_default(),
        head_oid: if head_before == head_after {
            head_before
        } else {
            None
        },
        last_touched: 0,
    })
}

/// Warms the PR comment cache the post-open sync reads, detached so the
/// prefetch claim releases as soon as its own inputs are cached.
fn warm_comment_bundle(
    ports: &Ports,
    repo_root: Option<String>,
    slug: Option<(String, String)>,
    number: u64,
) {
    let ports = ports.clone();
    let _ = thread::Builder::new().spawn(move || {
        let slug = slug.or_else(|| {
            let root = repo_root.as_deref()?;
            ports.github.get_repo_info(&*ports.git, root).ok()
        });
        if let Some((owner, repo)) = slug {
            let _ =
                ports
                    .github
                    .pr_comment_bundle_cached(&owner, &repo, number, repo_root.as_deref());
        }
    });
}

/// A loaded record with a fresh key per tab. Keys are never saved (ADR 0068).
fn restored_view(file: er_engine::tabs_record::TabsFile) -> TabsView {
    let tabs: Vec<TabEntry> = file
        .tabs
        .into_iter()
        .map(|descriptor| TabEntry {
            key: TabKey::next(),
            descriptor,
        })
        .collect();
    let active = tabs
        .get(file.active_idx.min(tabs.len().saturating_sub(1)))
        .map(|tab| tab.key);
    TabsView {
        revision: 0,
        tabs,
        active,
    }
}

/// Applies one tab command to the list. Closing keeps the App's rule: the
/// last tab stays, and closing the active tab focuses its left neighbour.
fn apply_tab_command(state: &mut TabsView, command: AppCommand) -> Result<(), WorkspaceError> {
    match command {
        AppCommand::NewTab { tab } => {
            let key = TabKey::next();
            state.tabs.push(TabEntry {
                key,
                descriptor: tab,
            });
            state.active = Some(key);
        }
        AppCommand::CloseTab { key } => {
            let idx = state.position(key)?;
            if state.tabs.len() <= 1 {
                return Err(WorkspaceError::Failed("cannot close the last tab".into()));
            }
            state.tabs.remove(idx);
            if state.active == Some(key) {
                state.active = state.key_at(idx.saturating_sub(1));
            }
        }
        AppCommand::ReplaceTab { key, tab } => {
            let idx = state.position(key)?;
            let new = TabKey::next();
            state.tabs[idx] = TabEntry {
                key: new,
                descriptor: tab,
            };
            state.active = Some(new);
        }
        AppCommand::SelectTab { key } => {
            state.position(key)?;
            state.active = Some(key);
        }
        AppCommand::ReorderTabs { key, to_idx } => {
            let from = state.position(key)?;
            if to_idx >= state.tabs.len() {
                return Err(WorkspaceError::Failed(format!("no tab position {to_idx}")));
            }
            let tab = state.tabs.remove(from);
            state.tabs.insert(to_idx, tab);
        }
        AppCommand::PrefetchOpenTarget { .. } | AppCommand::PrefetchRemoteOpenTarget { .. } => {}
    }
    Ok(())
}

fn joined<T>(handle: ScopedJoinHandle<'_, Result<T, GhError>>) -> Result<T, WorkspaceError> {
    match handle.join() {
        Ok(result) => result.map_err(WorkspaceError::from),
        Err(_) => Err(WorkspaceError::Failed("a prefetch thread panicked".into())),
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// What an open cache holds about a PR beside its diff. A local entry warmed
/// by a prefetch may carry commits and no overview.
#[derive(Clone, Debug, Default)]
pub struct CachedPr {
    pub overview: Option<er_engine::github::PrOverviewData>,
    pub commits: Option<Vec<er_engine::git::CommitInfo>>,
    pub head_oid: Option<String>,
}

impl AppRegistry {
    /// The cached PR `number`: the remote cache's entry for `remote`, or a
    /// local entry for `repo_root` under any project. The diff stays in the
    /// cache.
    pub fn cached_pr(
        &self,
        remote: Option<(&str, &str)>,
        repo_root: &str,
        number: u64,
    ) -> Option<CachedPr> {
        if let Some((owner, repo)) = remote {
            let key = (owner.to_string(), repo.to_string(), number);
            return lock(&self.remote).get(&key).map(|entry| CachedPr {
                overview: entry.pr_data.clone(),
                commits: Some(entry.pr_commits.clone()),
                head_oid: entry.head_oid.clone(),
            });
        }
        lock(self.pr_open())
            .iter()
            .find(|(key, _)| key.repo_root == repo_root && key.pr_number == number)
            .map(|(_, entry)| CachedPr {
                overview: entry.pr_data.clone(),
                commits: entry.pr_commits.clone(),
                head_oid: Some(entry.freshness.head_oid.clone()).filter(|oid| !oid.is_empty()),
            })
    }
}
