//! The desktop's shared state. Every field is an `Arc`, so a clone shares
//! the state.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};

use crate::line_commits_cache::LineCommitsCache;
use crate::page_view::{GhStatusCache, GhUser, PrInfo, WatchStatusSnapshot, WatchStatusState};
use er_service::app_records::inbox::InboxHandle;

#[derive(Clone)]
pub struct DesktopState {
    pub ports: er_engine::ports::Ports,
    /// Workspace actors for the command path (ADR 0067).
    pub service: Arc<crate::service::ServiceState>,
    /// Desktop-only view state (ADR 0068).
    pub view: crate::view_state::ViewStateHandle,
    pub pr_cache: Arc<Mutex<HashMap<String, Vec<PrInfo>>>>,
    /// The `origin/<base>` oid a sync last fetched, keyed by
    /// "{repo_root}\0{base_short}". The page snapshot that compared it is gone,
    /// so nothing reads it now.
    pub branch_base_remote_oid: Arc<Mutex<HashMap<String, String>>>,
    /// The sidebar's resources and the metadata cache's one writer.
    pub app_owner: Arc<er_service::app_records::AppOwner>,
    pub gh_user: GhUser,
    /// Active PTY sessions keyed by frontend session_id (e.g. `tab-<idx>`).
    /// Dropping an entry kills its child shell via `PtySession::drop`.
    pub terminals: Arc<Mutex<HashMap<String, crate::terminal::PtySession>>>,
    /// Threads with an in-flight `ask_ai` subprocess. Snapshot reads this
    /// to render a synthetic "…thinking" reply until the real reply lands.
    pub pending_ai_replies: er_service::card_asks::AiReplyJobs,
    /// The desktop's writes to the tabs' browser annotations.
    pub ui_annotations: WriteClock,
    /// Held by each flag-path write to `ui-annotations.json` from its view
    /// check to its bump. Each write loads the file and saves it whole, and
    /// `er_send` runs commands side by side, so without it two writes sent
    /// together each save over the other's change.
    pub ui_annotations_io: Arc<Mutex<()>>,
    /// Per-PR GitHub status (review decision, mergeable, checks, etc).
    /// Populated by a background thread on a 60s cadence and on explicit refresh.
    pub gh_status_cache: GhStatusCache,
    /// Whether a PR-list refresh is running, which `jobs` shows.
    pub pr_list_running: Arc<std::sync::atomic::AtomicBool>,
    /// Moves when a PR-list refresh starts or ends, so `jobs` pushes it.
    pub pr_list_clock: WriteClock,
    /// Moves when a GitHub status fetch starts or ends, so `pr_status`
    /// pushes it.
    pub status_fetch_clock: WriteClock,
    /// Keys with an in-flight gh_status fetch. Prevents duplicate concurrent fetches.
    pub gh_status_in_flight: Arc<Mutex<HashSet<(String, String, u64)>>>,
    /// Every key a status fetch has started for. The relay keeps watching
    /// them after the fetch ends, so the end of a first fetch that stored
    /// nothing still reaches the page as `fetching: false`.
    pub gh_status_fetched: Arc<Mutex<HashSet<(String, String, u64)>>>,
    /// Monotonic counter bumped whenever background-owned durable state changes
    /// so that poll() can detect changes not visible in App state.
    pub desktop_revision: Arc<AtomicU64>,
    /// Active-branch watcher status. Read by `build_snapshot` so the UI can
    /// show `Watching` when the desktop watcher is following a checkout.
    pub watch_status: WatchStatusState,
    pub inbox: InboxHandle,
    pub tauri_app_handle: Arc<Mutex<Option<tauri::AppHandle>>>,
    /// Dedupes concurrent auto-triage workers per remote/pr/head.
    pub auto_triage_in_flight: crate::auto_triage::AutoTriageInFlight,
    /// Per-file "Mark lines" attribution, outside the snapshot on purpose.
    pub line_commits_cache: LineCommitsCache,
    /// The desktop's writes to `session` and `panels`.
    pub session: SessionClock,
    /// How many times the live runs `jobs` lists have moved.
    pub live_jobs: LiveJobs,
    /// How many times the tab rows' values from outside their descriptors
    /// have moved.
    pub live_tabs: LiveTabs,
    /// The background agent tasks the flag path runs, which `jobs` lists.
    pub tasks: er_service::TaskHost,
}

impl DesktopState {
    /// Marks a PR-list refresh as running and moves `jobs`; None when one
    /// already runs. The check and the mark are one lock. Dropping the guard
    /// ends the refresh, so a panic or an early return cannot leave it on.
    pub fn begin_pr_list_refresh(&self) -> Option<PrListRefresh> {
        if self.pr_list_running.swap(true, Ordering::AcqRel) {
            return None;
        }
        self.pr_list_clock.bump();
        Some(PrListRefresh(self.clone()))
    }

    /// Marks the status fetch of `(owner, repo, number)` as running and
    /// moves `pr_status`; None when it already runs. Dropping the guard ends
    /// the fetch.
    pub fn begin_status_fetch(&self, key: &(String, String, u64)) -> Option<StatusFetch> {
        let added = self
            .gh_status_in_flight
            .lock()
            .is_ok_and(|mut in_flight| in_flight.insert(key.clone()));
        if !added {
            return None;
        }
        if let Ok(mut fetched) = self.gh_status_fetched.lock() {
            fetched.insert(key.clone());
        }
        self.status_fetch_clock.bump();
        Some(StatusFetch {
            desktop: self.clone(),
            key: key.clone(),
        })
    }
}

/// A running PR-list refresh. Dropping it marks the refresh as done and
/// moves `jobs`.
#[must_use = "dropping the guard ends the refresh"]
pub struct PrListRefresh(DesktopState);

impl Drop for PrListRefresh {
    fn drop(&mut self) {
        self.0.pr_list_running.store(false, Ordering::Release);
        self.0.pr_list_clock.bump();
    }
}

/// A running status fetch. Dropping it marks the fetch as done and moves
/// `pr_status`.
#[must_use = "dropping the guard ends the fetch"]
pub struct StatusFetch {
    desktop: DesktopState,
    key: (String, String, u64),
}

impl Drop for StatusFetch {
    fn drop(&mut self) {
        if let Ok(mut in_flight) = self.desktop.gh_status_in_flight.lock() {
            in_flight.remove(&self.key);
        }
        self.desktop.status_fetch_clock.bump();
    }
}

/// A count of the changes seen in the live runs and review flows `jobs`
/// lists. The actors publish them in their views, which move for every diff
/// or annotation write too, so the count moves only when the runs or flows
/// themselves differ from the last ones seen.
#[derive(Clone, Default)]
pub struct LiveJobs {
    seen: Arc<Mutex<Vec<er_protocol::ArenaRunJob>>>,
    flows: Arc<Mutex<Vec<er_protocol::ReviewFlowJob>>>,
    revision: Arc<AtomicU64>,
}

impl LiveJobs {
    /// The count after comparing `now` with the runs seen last.
    pub fn observe(&self, now: &[er_protocol::ArenaRunJob]) -> u64 {
        self.compare(&self.seen, now)
    }

    /// The count after comparing `now` with the flows seen last.
    pub fn observe_flows(&self, now: &[er_protocol::ReviewFlowJob]) -> u64 {
        self.compare(&self.flows, now)
    }

    fn compare<T: Clone + PartialEq>(&self, seen: &Mutex<Vec<T>>, now: &[T]) -> u64 {
        count_change(seen, &self.revision, now)
    }
}

/// A count of the changes seen in the tab rows' values that come from the
/// tabs' content and repos, which no registry revision moves: a load that
/// changes a Working tab's branch or a diff's hash moves this count, so
/// `tabs` moves with it.
#[derive(Clone, Default)]
pub struct LiveTabs {
    seen: Arc<Mutex<Vec<er_protocol::TabRow>>>,
    revision: Arc<AtomicU64>,
}

impl LiveTabs {
    /// The count after comparing `now` with the rows seen last.
    pub fn observe(&self, now: &[er_protocol::TabRow]) -> u64 {
        count_change(&self.seen, &self.revision, now)
    }
}

/// `revision` after comparing `now` with `seen`, moved by one when they differ.
fn count_change<T: Clone + PartialEq>(
    seen: &Mutex<Vec<T>>,
    revision: &AtomicU64,
    now: &[T],
) -> u64 {
    let mut seen = match seen.lock() {
        Ok(seen) => seen,
        Err(poisoned) => poisoned.into_inner(),
    };
    if seen.as_slice() != now {
        *seen = now.to_vec();
        revision.fetch_add(1, Ordering::AcqRel);
    }
    revision.load(Ordering::Acquire)
}

/// The desktop's own writes to `session` and `panels`: the watcher's status,
/// the update check and the panel mirror. Each write moves a counter and wakes
/// the resource relay, so the push follows it as it follows an actor's
/// publish. The default build has no relay and only counts.
#[derive(Clone, Default)]
pub struct SessionClock {
    revision: Arc<AtomicU64>,
    wake: Arc<OnceLock<Sender<()>>>,
    /// The last update check's answer.
    pub update: Arc<Mutex<Option<er_protocol::AppUpdate>>>,
}

impl SessionClock {
    /// How many times the desktop's half of `session` moved.
    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }

    /// The desktop's half of `session` moved.
    pub fn bump(&self) {
        self.revision.fetch_add(1, Ordering::AcqRel);
        self.wake();
    }

    /// Asks the relay to read the versions again.
    pub fn wake(&self) {
        if let Some(wake) = self.wake.get() {
            let _ = wake.send(());
        }
    }

    /// Wakes `wake` after each write from now on; the first call wins.
    pub fn wake_with(&self, wake: Sender<()>) {
        let _ = self.wake.set(wake);
    }

    /// A count of another desktop writer's writes, waking the relay this
    /// clock wakes.
    pub fn clock(&self) -> WriteClock {
        WriteClock {
            revision: Arc::default(),
            wake: Arc::clone(&self.wake),
        }
    }

    /// Writes the watcher's status and moves `session` when it changed.
    pub fn set_watch_status(&self, status: &WatchStatusState, next: WatchStatusSnapshot) {
        let changed = match status.lock() {
            Ok(mut current) => {
                let changed = *current != next;
                *current = next;
                changed
            }
            Err(_) => false,
        };
        if changed {
            self.bump();
        }
    }
}

/// One desktop writer's count of its writes. Each write moves it and wakes
/// the resource relay, so the push follows the write.
#[derive(Clone, Default)]
pub struct WriteClock {
    revision: Arc<AtomicU64>,
    wake: Arc<OnceLock<Sender<()>>>,
}

impl WriteClock {
    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }

    pub fn bump(&self) {
        self.revision.fetch_add(1, Ordering::AcqRel);
        if let Some(wake) = self.wake.get() {
            let _ = wake.send(());
        }
    }
}

/// Run a blocking command body on the async runtime's blocking pool.
///
/// Sync Tauri commands execute on the main thread — any command that waits
/// on the `App` mutex or shells out to git can freeze the window while it
/// runs. Heavy commands are declared `async` and wrap their original body
/// with this helper so the main thread stays responsive; the bodies remain
/// plain blocking code (the state is all `Arc`s, so it is cloned into the
/// closure).
pub async fn run_blocking<T, F>(f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| format!("blocking task failed: {e}"))?
}
