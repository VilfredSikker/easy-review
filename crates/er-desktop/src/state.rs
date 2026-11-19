//! The desktop's shared state, minus the App.
//!
//! `AppState` wraps this with the App and the counters only the snapshot poll
//! reads. A module that takes `DesktopState` cannot reach the App, so the
//! modules that render from the service take this one, and the compiler holds
//! them off the App. Every field is an `Arc`, so a clone shares the state.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Condvar, Mutex};

use er_engine::pr_open_entry::{PrOpenCacheEntry, PrOpenCacheKey, PrOpenFreshness};

use crate::inbox::InboxHandle;
use crate::line_commits_cache::LineCommitsCache;
use crate::pr_cache::PrCacheFetchedAtMap;
use crate::snapshot::{
    GhStatusCache, GhUser, LoadingState, MetaCache, PendingAiReplies, PrInfo, WatchStatusState,
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
    pub gh_user: GhUser,
    /// Active PTY sessions keyed by frontend session_id (e.g. `tab-<idx>`).
    /// Dropping an entry kills its child shell via `PtySession::drop`.
    pub terminals: Arc<Mutex<HashMap<String, crate::terminal::PtySession>>>,
    /// Threads with an in-flight `ask_ai` subprocess. Snapshot reads this
    /// to render a synthetic "…thinking" reply until the real reply lands.
    pub pending_ai_replies: PendingAiReplies,
    /// Per-PR GitHub status (review decision, mergeable, checks, etc).
    /// Populated by a background thread on a 60s cadence and on explicit refresh.
    pub gh_status_cache: GhStatusCache,
    /// Which background fetches are currently in-flight — surfaced in every snapshot.
    pub loading: LoadingState,
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
