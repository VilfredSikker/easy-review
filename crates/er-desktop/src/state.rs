//! The desktop's shared state. Every field is an `Arc`, so a clone shares
//! the state.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};

use crate::line_commits_cache::LineCommitsCache;
use crate::page_view::{PendingAiReplies, PendingAiReply, WatchStatusSnapshot, WatchStatusState};
use er_service::app_records::inbox::InboxHandle;

#[derive(Clone)]
pub struct DesktopState {
    pub ports: er_engine::ports::Ports,
    /// Workspace actors for the command path (ADR 0067).
    pub service: Arc<crate::service::ServiceState>,
    /// Desktop-only view state (ADR 0068).
    pub view: crate::view_state::ViewStateHandle,
    /// The `origin/<base>` oid a sync last fetched, keyed by
    /// "{repo_root}\0{base_short}". The page snapshot that compared it is gone,
    /// so nothing reads it now.
    pub branch_base_remote_oid: Arc<Mutex<HashMap<String, String>>>,
    /// The sidebar's owner: the records, the caches, and the fetches that
    /// fill them with their bookkeeping.
    pub app_owner: Arc<er_service::app_records::AppOwner>,
    /// Active PTY sessions keyed by frontend session_id (e.g. `tab-<idx>`).
    /// Dropping an entry kills its child shell via `PtySession::drop`.
    pub terminals: Arc<Mutex<HashMap<String, crate::terminal::PtySession>>>,
    /// Threads with an in-flight `ask_ai` subprocess. Snapshot reads this
    /// to render a synthetic "…thinking" reply until the real reply lands.
    pub pending_ai_replies: AiReplyJobs,
    /// The desktop's writes to the tabs' browser annotations.
    pub ui_annotations: WriteClock,
    /// Held by each flag-path write to `ui-annotations.json` from its view
    /// check to its bump. Each write loads the file and saves it whole, and
    /// `er_send` runs commands side by side, so without it two writes sent
    /// together each save over the other's change.
    pub ui_annotations_io: Arc<Mutex<()>>,
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

/// The in-flight ask-AI replies, by thread id or `finding:<id>`: what
/// `jobs` serves. Every insert and removal goes through here and moves the
/// clock, so no writer leaves the page on a stale "…thinking" reply.
#[derive(Clone, Default)]
pub struct AiReplyJobs {
    replies: PendingAiReplies,
    pub clock: WriteClock,
}

impl AiReplyJobs {
    pub fn new(clock: WriteClock) -> Self {
        Self {
            replies: Arc::default(),
            clock,
        }
    }

    /// The map the snapshot joins into its threads and findings.
    pub fn replies(&self) -> &PendingAiReplies {
        &self.replies
    }

    pub fn insert(&self, key: String, reply: PendingAiReply) {
        if let Ok(mut replies) = self.replies.lock() {
            replies.insert(key, reply);
        }
        self.clock.bump();
    }

    /// Removes each key that is pending; moves the clock when one was.
    pub fn remove<'a>(&self, keys: impl IntoIterator<Item = &'a str>) {
        let mut removed = false;
        if let Ok(mut replies) = self.replies.lock() {
            for key in keys {
                removed |= replies.remove(key).is_some();
            }
        }
        if removed {
            self.clock.bump();
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
