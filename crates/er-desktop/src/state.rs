//! The desktop's shared state, minus the App.
//!
//! `AppState` wraps this with the App and the counters only the snapshot poll
//! reads. A module that takes `DesktopState` cannot reach the App, so the
//! modules that render from the service take this one, and the compiler holds
//! them off the App. Every field is an `Arc`, so a clone shares the state.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Condvar, Mutex, OnceLock};

use er_engine::pr_open_entry::{PrOpenCacheEntry, PrOpenCacheKey, PrOpenFreshness};

use crate::inbox::InboxHandle;
use crate::line_commits_cache::LineCommitsCache;
use crate::pr_cache::PrCacheFetchedAtMap;
use crate::snapshot::{
    GhStatusCache, GhUser, LoadingState, MetaCache, PendingAiReplies, PendingAiReply, PrInfo,
    WatchStatusSnapshot, WatchStatusState,
};

#[derive(Clone)]
pub struct DesktopState {
    pub ports: er_engine::ports::Ports,
    /// Workspace actors for the `v1-service` command path (ADR 0067). The
    /// default build has no service and no field.
    #[cfg(feature = "v1-service")]
    pub service: Arc<crate::service::ServiceState>,
    /// Desktop-only view state (ADR 0068). Lock it after `app`, never before.
    pub view: crate::view_state::ViewStateHandle,
    pub pr_cache: Arc<Mutex<HashMap<String, Vec<PrInfo>>>>,
    pub pr_cache_fetched_at: PrCacheFetchedAtMap,
    /// Background-probed remote oid of the active branch tab's base branch,
    /// keyed by "{repo_root}\0{base_short}". Compared in build_snapshot against
    /// the local origin/<base> oid to detect a stale ("behind main") branch diff.
    pub branch_base_remote_oid: Arc<Mutex<HashMap<String, String>>>,
    pub pr_open_cache: Arc<Mutex<HashMap<PrOpenCacheKey, PrOpenCacheEntry>>>,
    pub meta_cache: MetaCache,
    /// The sidebar's resources and the metadata cache's one writer.
    pub app_owner: Arc<crate::app_owner::AppOwner>,
    pub gh_user: GhUser,
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
    /// Per-PR GitHub status (review decision, mergeable, checks, etc).
    /// Populated by a background thread on a 60s cadence and on explicit refresh.
    pub gh_status_cache: GhStatusCache,
    /// Which background fetches are currently in-flight — surfaced in every snapshot.
    pub loading: LoadingState,
    /// Moves when a PR-list refresh starts or ends, so `jobs` pushes it.
    pub pr_list_clock: WriteClock,
    /// Moves when a GitHub status fetch starts or ends, so `pr_status`
    /// pushes it.
    pub status_fetch_clock: WriteClock,
    /// Keys with an in-flight gh_status fetch. Prevents duplicate concurrent fetches.
    pub gh_status_in_flight: Arc<Mutex<HashSet<(String, String, u64)>>>,
    /// Keys (project_id, pr_number) with an in-flight PR-open prefetch,
    /// mapped to a claim the open path can wait on. Prevents duplicate
    /// background `gh` invocations when the user hovers the same row
    /// repeatedly, and lets a click during a hover-prefetch join the
    /// in-flight fetch instead of running a duplicate `gh pr diff`.
    pub pr_open_prefetch_in_flight: Arc<Mutex<PrOpenPrefetchMap>>,
    /// Keys (repo-or-slug, pr_number) with an in-flight branch-scope preload
    /// (`kick_branch_preload`). Prevents duplicate background `gh`/git calls
    /// when the same PR is opened repeatedly.
    pub branch_preload_in_flight: Arc<Mutex<HashSet<(String, u64)>>>,
    /// Keys (repo-root, pr_number) with an in-flight background PR ref fetch
    /// (`kick_pr_ref_fetch`). Prevents duplicate `git fetch` pairs when the
    /// same PR is opened repeatedly.
    pub pr_ref_fetch_in_flight: Arc<Mutex<HashSet<(String, u64)>>>,
    /// Remote-only PR open cache + in-flight claims (see remote_pr_open_cache.rs).
    /// Warms on sidebar hover (`prefetch_remote_pr_open`); a hit opens the PR
    /// with zero `gh` calls.
    pub remote_pr_open_cache: crate::remote_pr_open_cache::RemotePrOpenCache,
    pub remote_pr_open_in_flight: crate::remote_pr_open_cache::RemotePrOpenInFlight,
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
    #[cfg(feature = "v1-service")]
    pub tasks: crate::tasks::TaskHost,
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
    /// The engine's sequence number of the last App notification sent as a
    /// notice, so each is sent once.
    pub app_notice_seq: Arc<AtomicU64>,
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

/// In-flight PR-open prefetch claim. The open path (`load_pr_open_inputs`)
///
/// waits on `cv` (bounded) when the claim's freshness matches its own, so a
/// click during a hover-prefetch consumes the prefetched cache entry instead
/// of running a duplicate `gh pr diff`. `freshness` is the prefetch's own, so
/// a waiter whose hint changed mid-flight skips the wait entirely.
pub type PrOpenPrefetchMap = HashMap<(String, u64), Arc<PrOpenPrefetchClaim>>;

pub struct PrOpenPrefetchClaim {
    pub freshness: PrOpenFreshness,
    pub done: Mutex<bool>,
    pub cv: Condvar,
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
