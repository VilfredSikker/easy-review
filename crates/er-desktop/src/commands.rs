use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::State;
use tauri_plugin_notification::NotificationExt;

use crate::inbox::{InboxHandle, InboxItem, InboxTarget};
use crate::pr_cache::PrCacheFetchedAtMap;
use crate::projects::{self, normalize_remote_slug};
use crate::snapshot::{
    build_chrome_snapshot, build_file_snapshot, AgentLogSnapshot, AppSnapshot, CheckSummary,
    FileSnapshot, GhCommentSummary, GhReviewSummary, GhStatusCache, GhUser, GithubStatusSnapshot,
    LoadingState, MetaCache, PendingAiReplies, PrInfo, WatchStatusState,
};
use er_engine::ai::CommentType;
#[cfg(test)]
use er_engine::app::CardAiInvocation;
use er_engine::app::{
    build_card_ai_system_context, plan_card_ai_invocation, run_card_ai_subprocess, App,
    BrowserLayout, CardAiContextParams, DiffMode, InputMode,
};

const DEFAULT_ASK_AI_PROMPT: &str = "Elaborate on this and answer any question directly.";

/// Per-card validation: check whether a comment, question, or finding still holds.
const VALIDATE_CARD_AI_PROMPT: &str = r#"Validate whether this review note is still accurate against the current code context provided.

Use Read / grep / rg in the repository (see system context for repo_root and diff excerpt) before concluding. Up to ~10 reads for this finding; cite file:line evidence.

Reply in markdown with:
1. **Verdict**: Confirmed | Outdated | Needs context | Unclear
2. **Evidence**: What in the code supports your verdict (cite file:line when possible)
3. **Recommendation**: What the reviewer should do next

Be concise. If the concern is already addressed in the current diff, say so clearly."#;

/// Per-question elaboration: answer / expand on a reviewer's question by reading
/// the surrounding code. Used by the `elaborate_with_ai` command (questions),
/// where "validate" doesn't fit — there is nothing to confirm/refute, the
/// reviewer wants the AI to investigate and explain.
const ELABORATE_CARD_AI_PROMPT: &str = r#"Answer and elaborate on this review question against the current code.

Use Read / grep / rg in the repository (see system context for repo_root and diff excerpt) before answering. Up to ~10 reads; cite file:line evidence.

Reply in markdown with:
1. **Answer**: A direct answer to the question.
2. **Details**: Relevant context from the code (cite file:line when possible).
3. **Follow-ups**: Anything the reviewer should double-check or decide.

Be concise and concrete."#;
const REQUESTED_KINDS: &[&str] = &[
    "ai_review_done",
    "ai_review_failed",
    "ai_triage_done",
    "ai_triage_failed",
    "ai_review_cancelled",
    "pr_review_approved",
    "pr_review_changes_requested",
    "ci_failed",
    "review_requested",
    "review_rerequested",
    "pr_comment_or_mention",
    "pr_merged",
    "pr_closed",
    "github_refresh_failed",
    "pr_cache_stale",
];

#[derive(Debug, Clone, serde::Serialize)]
pub struct PollResponse {
    /// Legacy combined revision (`max(content_revision, chrome_revision)`).
    pub revision: u64,
    pub content_revision: u64,
    pub chrome_revision: u64,
    /// Monotonic counter bumped only when the reviewed set changes. Kept out of
    /// content_revision and chrome_revision so reviewed-only changes can return
    /// snapshot=None + chrome_only=true without triggering a full hunk rebuild.
    pub reviewed_revision: u64,
    /// When true, the frontend should merge chrome fields and keep existing file hunks/spans.
    pub chrome_only: bool,
    /// Full snapshot — `None` when both revisions are unchanged since the last poll.
    pub snapshot: Option<AppSnapshot>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ReviewRevisionSummary {
    pub revision_id: String,
    pub created_at: String,
    pub scope: String,
    pub diff_hash: String,
    pub active: bool,
    pub agents: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct OpenSourceResult {
    pub kind: String, // opened_local | opened_url | needs_checkout
    pub target: String,
}

#[derive(Clone)]
pub struct AppState {
    pub app: Arc<Mutex<App>>,
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
    /// Keys (project_id, pr_number) with an in-flight PR-open prefetch.
    /// Prevents duplicate background `gh` invocations when the user hovers
    /// the same row repeatedly.
    pub pr_open_prefetch_in_flight: Arc<Mutex<HashSet<(String, u64)>>>,
    /// Monotonic counter bumped whenever background-owned durable state changes
    /// so that poll() can detect changes not visible in App state.
    pub desktop_revision: Arc<AtomicU64>,
    /// Last content revision included in a poll response.
    pub last_sent_content_revision: Arc<AtomicU64>,
    /// Last chrome revision included in a poll response.
    pub last_sent_chrome_revision: Arc<AtomicU64>,
    /// Last reviewed_revision included in a poll response.
    pub last_sent_reviewed_revision: Arc<AtomicU64>,
    /// Active-branch watcher status. Read by `build_snapshot` so the UI can
    /// show `Watching` when the desktop watcher is following a checkout.
    pub watch_status: WatchStatusState,
    pub inbox: InboxHandle,
    pub tauri_app_handle: Arc<Mutex<Option<tauri::AppHandle>>>,
    /// Dedupes concurrent auto-triage workers per remote/pr/head.
    pub auto_triage_in_flight: crate::auto_triage::AutoTriageInFlight,
    /// Differential snapshots: per-file content keys the frontend currently
    /// holds, so unchanged hunks can be omitted from later snapshots.
    pub sent_files: crate::snapshot::SentFilesHandle,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct PrOpenCacheKey {
    pub(crate) project_id: String,
    pub(crate) repo_root: String,
    pub(crate) pr_number: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PrOpenFreshness {
    pub(crate) base_branch: String,
    pub(crate) head_branch: String,
    pub(crate) head_oid: String,
    pub(crate) updated_at: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PrOpenCacheEntry {
    pub(crate) freshness: PrOpenFreshness,
    pub(crate) raw_diff: String,
    /// Cached PR overview so a click after a hover-prefetch can render the
    /// right panel without re-running `gh pr view`.
    pub(crate) pr_data: Option<er_engine::github::PrOverviewData>,
    /// Cached GitHub PR commits, newest first, keyed by the same freshness.
    pub(crate) pr_commits: Option<Vec<er_engine::git::CommitInfo>>,
    /// Monotonic tick from `pr_open_clock()` recording the last read or write.
    /// Drives LRU eviction (`evict_lru`) when the cache exceeds its entry cap.
    /// Older persisted entries deserialize to 0 (treated as least-recent).
    #[serde(default)]
    pub(crate) last_touched: u64,
}

#[derive(Debug, Clone)]
struct PrOpenMetadata {
    freshness: PrOpenFreshness,
    pr_data: er_engine::github::PrOverviewData,
    pr_commits: Vec<er_engine::git::CommitInfo>,
}

struct PrOpenInputs {
    repo_root: String,
    metadata: PrOpenMetadata,
    resolved_base: String,
    raw_diff: String,
    cache_hit: bool,
}

/// Hint passed from the frontend sidebar when opening or prefetching a PR.
/// Carries the freshness fields the sidebar already has from `gh pr list`,
/// so we can skip a `gh pr view` round-trip.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrOpenHint {
    pub base_ref: String,
    pub head_ref: String,
    pub head_oid: String,
    pub updated_at: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub author: String,
}

#[tauri::command]
pub fn start_window_drag(window: tauri::Window) -> Result<(), String> {
    window.start_dragging().map_err(|e| e.to_string())
}

/// Run a blocking command body on the async runtime's blocking pool.
///
/// Sync Tauri commands execute on the main thread — any command that waits
/// on the `App` mutex or shells out to git can freeze the window while it
/// runs. Heavy commands are declared `async` and wrap their original body
/// with this helper so the main thread stays responsive; the bodies remain
/// plain blocking code (`AppState` is all `Arc`s, so it is cloned into the
/// closure).
pub(crate) async fn run_blocking<T, F>(f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| format!("blocking task failed: {e}"))?
}

macro_rules! snap {
    ($state:expr) => {{
        let app = $state.app.lock().map_err(|e| e.to_string())?;
        Ok(snap_from(&app, &$state))
    }};
}

/// Build a snapshot using the lock guards directly (when callers already hold them).
/// Differential: hunks the frontend already holds are omitted (`hunks_omitted`).
pub(crate) fn snap_from(app: &App, state: &AppState) -> AppSnapshot {
    crate::snapshot::build_snapshot_with_delta(
        app,
        Some(&state.pr_cache),
        Some(&state.pr_cache_fetched_at),
        Some(&state.meta_cache),
        Some(&state.gh_user),
        Some(&state.pending_ai_replies),
        Some(&state.gh_status_cache),
        Some(&state.loading),
        Some(&state.watch_status),
        Some(&state.inbox),
        Some(&state.sent_files),
        Some(&state.branch_base_remote_oid),
    )
}

fn chrome_snap_from(app: &App, state: &AppState) -> AppSnapshot {
    build_chrome_snapshot(
        app,
        Some(&state.pr_cache),
        Some(&state.pr_cache_fetched_at),
        Some(&state.meta_cache),
        Some(&state.gh_user),
        Some(&state.pending_ai_replies),
        Some(&state.gh_status_cache),
        Some(&state.loading),
        Some(&state.watch_status),
        Some(&state.inbox),
        Some(&state.branch_base_remote_oid),
    )
}

fn log_branch_open_phase(
    project_id: &str,
    branch: &str,
    phase: &str,
    started_at: std::time::Instant,
) {
    crate::profile_log::profile_log(
        "branch_open",
        &[
            ("project_id", project_id.to_string()),
            ("branch", branch.to_string()),
            ("phase", phase.to_string()),
            ("ms", started_at.elapsed().as_millis().to_string()),
        ],
    );
}

fn now_ms() -> u64 {
    crate::inbox::now_epoch_ms()
}

/// Show native notifications for inbox items that were created before the Tauri
/// `AppHandle` was stored (startup PR refresh races setup in release builds).
pub fn flush_pending_native_notifications(
    inbox_handle: &InboxHandle,
    app_handle_state: &Arc<Mutex<Option<tauri::AppHandle>>>,
) {
    let pending: Vec<InboxItem> = {
        let Ok(inbox) = inbox_handle.lock() else {
            return;
        };
        inbox
            .items
            .iter()
            .filter(|item| {
                (REQUESTED_KINDS.contains(&item.kind.as_str())
                    || item.severity == "warning"
                    || item.severity == "error")
                    && !inbox.notified_item_ids.contains(&item.id)
            })
            .cloned()
            .collect()
    };
    for item in &pending {
        maybe_send_native_notification(inbox_handle, app_handle_state, item);
    }
}

/// Release builds deliver notifications under the app bundle id (not Terminal).
#[cfg(target_os = "macos")]
pub fn prepare_macos_notifications(app: &tauri::AppHandle) {
    if tauri::is_dev() {
        return;
    }
    let ident = app.config().identifier.clone();
    match notify_rust::set_application(&ident) {
        Ok(()) => log::info!("macOS notifications: registered bundle id {ident}"),
        Err(e) => log::warn!(
            "macOS notifications: could not use bundle id {ident} ({e}). \
             Launch the installed Easy Review.app and enable notifications in System Settings."
        ),
    }
}

#[cfg(not(target_os = "macos"))]
pub fn prepare_macos_notifications(_app: &tauri::AppHandle) {}

fn maybe_send_native_notification(
    inbox_handle: &InboxHandle,
    app_handle_state: &Arc<Mutex<Option<tauri::AppHandle>>>,
    item: &InboxItem,
) {
    if !REQUESTED_KINDS.contains(&item.kind.as_str())
        && item.severity != "warning"
        && item.severity != "error"
    {
        return;
    }
    let Ok(mut inbox) = inbox_handle.lock() else {
        return;
    };
    if inbox.notified_item_ids.contains(&item.id) {
        return;
    }
    let handle = app_handle_state.lock().ok().and_then(|g| g.clone());
    if let Some(app) = handle {
        let shown = app
            .notification()
            .builder()
            .title(&item.title)
            .body(&item.body)
            .show()
            .is_ok();
        if shown {
            inbox.notified_item_ids.insert(item.id.clone());
        }
    }
}

/// Spawn a background fetch of the GitHub status for the given (owner, repo, number).
/// Returns immediately. The cache is updated on success; failures are logged.
/// Deduplicates: if a fetch for the same key is already in-flight, this is a no-op.
pub fn kick_github_status_refresh(
    cache: GhStatusCache,
    in_flight: Arc<Mutex<HashSet<(String, String, u64)>>>,
    desktop_revision: Arc<AtomicU64>,
    loading: Option<LoadingState>,
    owner: String,
    repo: String,
    number: u64,
) {
    let key = (owner.clone(), repo.clone(), number);
    if let Ok(mut set) = in_flight.lock() {
        if !set.insert(key.clone()) {
            return; // already fetching
        }
    }
    if let Some(loading) = &loading {
        if let Ok(mut flags) = loading.lock() {
            flags.gh_status = true;
        }
    }
    let in_flight_clone = Arc::clone(&in_flight);
    std::thread::spawn(move || {
        let snap = fetch_github_status(&owner, &repo, number);
        if let Some(snap) = snap {
            if let Ok(mut g) = cache.lock() {
                g.insert((owner.clone(), repo.clone(), number), snap);
            }
            // Persist after the lock is released (the save helper re-locks).
            crate::gh_status_cache::save_persisted_gh_status_cache(&cache);
            crate::profile_log::bump_desktop_revision(&desktop_revision, "gh_status_cache");
        }
        if let Some(loading) = &loading {
            if let Ok(mut flags) = loading.lock() {
                flags.gh_status = false;
            }
        }
        if let Ok(mut set) = in_flight_clone.lock() {
            set.remove(&key);
        }
    });
}

fn active_github_key(app: &App, state: &AppState) -> Option<(String, String, u64)> {
    let tab = app.tab();
    // Prefer the tab's own PR number (remote or local PR tab) so the background
    // gh-status fetch targets the PR that was actually opened, not an arbitrary
    // head_ref match when a branch carries more than one open PR. Plain branch /
    // working tabs fall back to head_ref matching inside the resolver.
    let cache = state.pr_cache.lock().ok()?;
    crate::snapshot::resolve_github_status_key(tab, &cache)
}

fn active_pr_author(
    app: &App,
    state: &AppState,
    owner: &str,
    repo: &str,
    number: u64,
) -> Option<String> {
    let tab = app.tab();
    if tab.pr_number == Some(number) {
        if let Some(author) = tab
            .pr_data
            .as_ref()
            .map(|pr| pr.author.trim())
            .filter(|author| !author.is_empty())
        {
            return Some(author.to_string());
        }
    }

    let key = (owner.to_string(), repo.to_string(), number);
    state
        .gh_status_cache
        .lock()
        .ok()
        .and_then(|cache| cache.get(&key).map(|status| status.author.clone()))
        .filter(|author| !author.trim().is_empty())
}

fn own_pr_approval_error() -> String {
    "GitHub does not allow approving your own pull request.".to_string()
}

fn is_own_pr_approval_error(raw: &str) -> bool {
    let lower = raw.to_lowercase();
    lower.contains("can not approve your own pull request")
        || lower.contains("cannot approve your own pull request")
}

fn process_ai_task_inbox(app: &App, state: &AppState) {
    let now = now_ms();
    let tasks = app.background_task_snapshots();
    let tab = app.tab();
    let repo_root = tab.repo_root.clone();
    let remote = tab.remote_repo.clone();
    let pr_number = tab.pr_number;
    let branch = tab
        .local_branch_view
        .clone()
        .unwrap_or_else(|| tab.current_branch.clone());

    let project_id =
        projects::resolve_project_id_for_inbox(Some(repo_root.as_str()), remote.as_deref());

    let mut emitted_any = false;
    let mut just_added: Vec<InboxItem> = Vec::new();
    if let Ok(mut inbox) = state.inbox.lock() {
        for task in tasks {
            let (kind, severity, title, body) = match task.status.as_str() {
                "done" if task.kind == "triage" => (
                    "ai_triage_done".to_string(),
                    "success".to_string(),
                    format!("Triage completed ({})", task.target_label),
                    task.label.clone(),
                ),
                "done" => (
                    "ai_review_done".to_string(),
                    "success".to_string(),
                    format!("AI review completed ({})", task.target_label),
                    task.label.clone(),
                ),
                "failed" if task.kind == "triage" => (
                    "ai_triage_failed".to_string(),
                    "error".to_string(),
                    format!("Triage failed ({})", task.target_label),
                    task.error
                        .clone()
                        .unwrap_or_else(|| "Triage failed".to_string()),
                ),
                "failed" => (
                    "ai_review_failed".to_string(),
                    "error".to_string(),
                    format!("AI review failed ({})", task.target_label),
                    task.error
                        .clone()
                        .unwrap_or_else(|| "Review failed".to_string()),
                ),
                _ => continue,
            };
            let item = InboxItem {
                id: format!("inbox-ai-{}-{}", task.id, task.status),
                kind,
                severity,
                title,
                body,
                source: "ai".to_string(),
                target: InboxTarget {
                    project_id: project_id.clone(),
                    repo_root: Some(repo_root.clone()),
                    remote: remote.clone(),
                    pr_number,
                    branch: Some(branch.clone()),
                    url: None,
                },
                created_at_ms: now,
                read_at_ms: None,
                dedupe_key: format!("ai:{}:{}", task.id, task.status),
            };
            if inbox.add_item(item.clone()) {
                emitted_any = true;
                just_added.push(item);
            }
        }
    }
    for item in &just_added {
        maybe_send_native_notification(&state.inbox, &state.tauri_app_handle, item);
    }
    if emitted_any {
        crate::inbox::save_inbox_state(&state.inbox);
        state.desktop_revision.fetch_add(1, Ordering::Relaxed);
    }
}

/// Fetch all GitHub status data for a PR. Runs 2 gh calls in parallel: one
/// combined `gh pr view --json` for overview+comments+reviews, one `gh pr
/// checks` for CI status (separate subcommand, can't be merged into `pr view`).
/// Returns None when the overview/status-bundle fetch fails (e.g. no network,
/// gh not authed). Comments/reviews parse failures inside the bundle are
/// non-fatal; checks failures are non-fatal — the snapshot still populates.
pub fn fetch_github_status(owner: &str, repo: &str, number: u64) -> Option<GithubStatusSnapshot> {
    let t = std::time::Instant::now();
    // Run 2 independent gh calls concurrently — cuts wall time and gh
    // subprocess count from 4 to 2.
    let (bundle_res, checks) = std::thread::scope(|s| {
        let b = s.spawn(|| er_engine::github::gh_pr_status_remote(owner, repo, number));
        let c = s.spawn(|| {
            er_engine::github::gh_pr_checks_remote(owner, repo, number).unwrap_or_default()
        });
        (b.join().ok(), c.join().unwrap_or_default())
    });
    let bundle = bundle_res?.ok()?;
    let overview = bundle.overview;
    let comments = bundle.comments;
    let reviews = bundle.reviews;
    crate::profile_log::profile_log(
        "gh_status_fetch",
        &[
            ("owner", owner.to_string()),
            ("repo", repo.to_string()),
            ("number", number.to_string()),
            ("ms", t.elapsed().as_millis().to_string()),
        ],
    );

    // Most recent 5, newest last in the source — keep the trailing 5.
    let recent_comments: Vec<GhCommentSummary> = comments
        .iter()
        .rev()
        .take(5)
        .map(|c| GhCommentSummary {
            author: c.author.clone(),
            body: c.body.clone(),
            created_at: c.created_at.clone(),
            url: c.url.clone(),
        })
        .collect();
    let recent_reviews: Vec<GhReviewSummary> = reviews
        .iter()
        .rev()
        .take(5)
        .map(|r| GhReviewSummary {
            author: r.author.clone(),
            state: r.state.clone(),
            body: r.body.clone(),
            submitted_at: r.submitted_at.clone(),
        })
        .collect();

    let check_summaries: Vec<CheckSummary> = checks
        .into_iter()
        .map(|c| CheckSummary {
            name: c.name,
            status: c.status,
            conclusion: c.conclusion,
            url: c.url,
        })
        .collect();

    // ISO-8601-ish timestamp using only std. Format: seconds since epoch as
    // string — frontend renders as "x sec ago". Avoids pulling in chrono.
    let last_updated = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs().to_string());

    Some(GithubStatusSnapshot {
        owner: owner.to_string(),
        repo: repo.to_string(),
        number,
        url: overview.url,
        state: overview.state,
        is_draft: overview.is_draft,
        title: overview.title,
        body: overview.body,
        author: overview.author,
        head_ref: overview.head_ref_name,
        base_ref: overview.base_ref_name,
        review_decision: overview.review_decision,
        mergeable: overview.mergeable,
        labels: overview.labels,
        checks: check_summaries,
        comments_count: comments.len(),
        reviews_count: reviews.len(),
        recent_comments,
        recent_reviews,
        last_updated,
        is_authored_by_me: false,
    })
}

/// Kick a background refresh of the active tab's GitHub status.
/// Works for remote PR tabs (remote_repo + pr_number) and for working-tree /
/// local-branch tabs where the viewed branch has an open PR in pr_cache.
fn kick_active_gh_status(app: &App, state: &AppState) {
    if let Some((owner, repo, number)) = active_github_key(app, state) {
        let last_updated = state.gh_status_cache.lock().ok().and_then(|cache| {
            cache
                .get(&(owner.clone(), repo.clone(), number))
                .and_then(|snap| snap.last_updated.clone())
        });
        // The active tab's status is already fresh (<10s) — the 30s
        // background loop or a recent kick already covered it. Don't fire a
        // redundant 4-subprocess fan-out on every tab switch / open / close.
        if crate::gh_status_cache::status_is_fresh(
            last_updated.as_deref(),
            crate::gh_status_cache::now_epoch_secs(),
            10,
        ) {
            return;
        }
        kick_github_status_refresh(
            state.gh_status_cache.clone(),
            Arc::clone(&state.gh_status_in_flight),
            Arc::clone(&state.desktop_revision),
            Some(Arc::clone(&state.loading)),
            owner,
            repo,
            number,
        );
    }
}

/// Kick off an async meta-cache refresh so the *next* poll reflects the
/// mutation that just happened. The refresh runs on a background thread and
/// does NOT touch the App mutex.
fn kick_meta_refresh(state: &AppState, root: String) {
    let cache = state.meta_cache.clone();
    let desktop_revision = Arc::clone(&state.desktop_revision);
    desktop_revision.fetch_add(1, Ordering::Relaxed);
    std::thread::spawn(move || {
        crate::snapshot::refresh_meta_cache(&root, &cache);
        desktop_revision.fetch_add(1, Ordering::Relaxed);
    });
}

#[tauri::command]
pub async fn get_snapshot(state: State<'_, AppState>) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let t0 = std::time::Instant::now();
        // Full fetch — the frontend is (re)building its state from scratch,
        // so forget what was previously sent and serialize everything.
        if let Ok(mut sent) = state.sent_files.lock() {
            sent.reset();
        }
        let app = state.app.lock().map_err(|e| e.to_string())?;
        let snap = snap_from(&app, &state);
        crate::profile_log::profile_log(
            "get_snapshot",
            &[
                ("build_ms", t0.elapsed().as_millis().to_string()),
                ("files", snap.files.len().to_string()),
            ],
        );
        Ok(snap)
    })
    .await
}

#[tauri::command]
pub fn toggle_panel(panel: String, state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    app.toggle_panel(&panel);
    Ok(snap_from(&app, &state))
}

// ── Navigation ────────────────────────────────────────────────────────────────

/// Parse one or more lazy-stub files and return *only those* `FileSnapshot`s
/// (not the full `AppSnapshot`), without changing the navigation selection. The
/// frontend merges each returned file into its existing snapshot in place.
///
/// Returning per-file payloads instead of re-serializing the entire diff via
/// `snap_from` keeps the viewport-driven lazy round-trip cheap on large diffs —
/// a fast-scroll burst that reveals several stubs is one call, not N full
/// snapshots. History mode has no lazy stubs, so it returns an empty vec.
#[tauri::command]
pub async fn request_file_content(
    source_indices: Vec<usize>,
    state: State<'_, AppState>,
) -> Result<Vec<FileSnapshot>, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        if app.tab().mode == er_engine::app::DiffMode::History {
            return Ok(vec![]);
        }
        {
            let tab = app.tab_mut();
            for &source_index in &source_indices {
                if source_index < tab.files.len() {
                    tab.ensure_file_parsed_at(source_index);
                }
            }
        }
        let tab = app.tab();
        let pending_ai = Some(&state.pending_ai_replies);
        let out: Vec<FileSnapshot> = source_indices
            .iter()
            .filter_map(|&source_index| {
                tab.files
                    .get(source_index)
                    .map(|f| build_file_snapshot(source_index, f, tab, pending_ai, true))
            })
            .collect();
        // These hunks bypass build_snapshot — record them so later polls can
        // omit the same content.
        for snap in &out {
            crate::snapshot::record_sent_file(&app, tab, snap, &state.sent_files);
        }
        Ok(out)
    })
    .await
}

#[tauri::command]
pub async fn select_file(idx: usize, state: State<'_, AppState>) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        {
            let tab = app.tab_mut();
            if tab.mode == er_engine::app::DiffMode::History {
                tab.history_select_file(idx);
            } else if idx < tab.files.len() {
                tab.selected_file = idx;
                tab.current_hunk = 0;
                tab.current_line = None;
                tab.diff_scroll = 0;
                tab.h_scroll = 0;
                tab.ensure_file_parsed();
                tab.rebuild_hunk_offsets();
            }
        }
        Ok(snap_from(&app, &state))
    })
    .await
}

#[tauri::command]
pub fn next_file(state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    app.tab_mut().next_file();
    Ok(snap_from(&app, &state))
}

#[tauri::command]
pub fn prev_file(state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    app.tab_mut().prev_file();
    Ok(snap_from(&app, &state))
}

#[tauri::command]
pub fn jump_to_unreviewed(state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    {
        let tab = app.tab_mut();
        let visible: Vec<usize> = tab
            .visible_files()
            .into_iter()
            .filter(|(_, f)| !tab.reviewed.contains_key(&f.path))
            .map(|(i, _)| i)
            .collect();
        if let Some(&first) = visible.first() {
            tab.selected_file = first;
            tab.current_hunk = 0;
            tab.current_line = None;
            tab.diff_scroll = 0;
            tab.h_scroll = 0;
            tab.ensure_file_parsed();
            tab.rebuild_hunk_offsets();
        }
    }
    Ok(snap_from(&app, &state))
}

// ── Hunk navigation ──────────────────────────────────────────────────────────

#[tauri::command]
pub fn next_hunk(state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    app.tab_mut().next_hunk();
    Ok(snap_from(&app, &state))
}

#[tauri::command]
pub fn prev_hunk(state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    app.tab_mut().prev_hunk();
    Ok(snap_from(&app, &state))
}

#[tauri::command]
pub fn toggle_compacted(state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    app.tab_mut()
        .toggle_compacted()
        .map_err(|e| e.to_string())?;
    Ok(snap_from(&app, &state))
}

// ── Mode ──────────────────────────────────────────────────────────────────────

fn feature_allows_mode_str(features: &er_engine::config::FeatureFlags, mode: &str) -> bool {
    match mode {
        "unstaged" => features.view_unstaged,
        "staged" => features.view_staged,
        "history" => features.view_history,
        "conflicts" => features.view_conflicts,
        "hidden" => features.view_hidden,
        "tour" => features.view_tour,
        "pr" | "pr_diff" => features.view_branch,
        _ => features.view_branch,
    }
}

#[tauri::command]
pub async fn set_mode(
    mode: String,
    pr_number: Option<u64>,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        if !feature_allows_mode_str(&app.config.features, mode.as_str()) {
            return Err(format!("'{mode}' view is disabled in settings"));
        }
        if matches!(mode.as_str(), "pr" | "pr_diff") {
            // Only enter PrDiff when not already there (avoids re-fetching refs
            // on a tab that is already in PrDiff from construction).
            if app.tab().mode != DiffMode::PrDiff {
                // Seed a detected PR number (from the header toggle) onto a local
                // branch tab that wasn't opened via --pr, so enter_pr_diff can fetch
                // the head and resolve the shared `pr` bucket.
                if app.tab().pr_number.is_none() {
                    if let Some(n) = pr_number {
                        app.tab_mut().pr_number = Some(n);
                    }
                }
                app.tab_mut().enter_pr_diff().map_err(|e| e.to_string())?;
            }
            return Ok(snap_from(&app, &state));
        }
        let diff_mode = match mode.as_str() {
            "unstaged" => DiffMode::Unstaged,
            "staged" => DiffMode::Staged,
            "history" => DiffMode::History,
            "conflicts" => DiffMode::Conflicts,
            "hidden" => DiffMode::Hidden,
            "tour" => DiffMode::Tour,
            _ => DiffMode::Branch,
        };
        app.tab_mut().set_mode(diff_mode);
        Ok(snap_from(&app, &state))
    })
    .await
}

// ── Reviewed state ────────────────────────────────────────────────────────────

#[tauri::command]
pub fn toggle_reviewed(state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    // toggle_reviewed bumps tab.reviewed_revision internally
    app.toggle_reviewed().map_err(|e| e.to_string())?;
    // Return chrome-only: counts update, no hunk rebuild needed
    Ok(chrome_snap_from(&app, &state))
}

#[tauri::command]
pub fn mark_reviewed(path: String, state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    {
        let tab = app.tab_mut();
        if tab.active_diff_files().iter().any(|f| f.path == path) {
            let hash = tab
                .current_per_file_hashes
                .get(&path)
                .cloned()
                .unwrap_or_default();
            tab.reviewed.insert(path, hash);
            tab.reviewed_revision += 1;
            let _ = tab.save_reviewed_files();
        }
    }
    // Return chrome-only: counts update, no hunk rebuild needed
    Ok(chrome_snap_from(&app, &state))
}

#[tauri::command]
pub fn unmark_reviewed(path: String, state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    {
        let tab = app.tab_mut();
        if tab.active_diff_files().iter().any(|f| f.path == path) {
            tab.reviewed.remove(&path);
            tab.reviewed_revision += 1;
            let _ = tab.save_reviewed_files();
        }
    }
    // Return chrome-only: counts update, no hunk rebuild needed
    Ok(chrome_snap_from(&app, &state))
}

/// Paths of a tour pillar's files that are present in the current diff. The
/// synthetic `__other__` pillar resolves to diff files no pillar references.
/// Filtering to in-diff files keeps stale-tour entries out of the shared branch
/// `reviewed` set (mirrors `mark_reviewed`'s `active_diff_files` guard).
fn pillar_file_paths(tab: &er_engine::app::TabState, pillar_id: &str) -> Vec<String> {
    let diff_paths: std::collections::HashSet<&str> = tab
        .active_diff_files()
        .iter()
        .map(|f| f.path.as_str())
        .collect();
    let Some(tour) = tab.ai.tour.as_ref() else {
        return Vec::new();
    };
    // Use the shared ownership rule (`ErTour::pillar_ownership`) so bulk-review
    // attributes each file to exactly the pillar the desktop snapshot displays
    // it under — including the global cross-pillar dedup that gives a shared
    // related file to the first pillar that references it. Keeping these in sync
    // is what lets a pillar's "Review all" reach 100%.
    let ownership = tour.pillar_ownership(|p| diff_paths.contains(p));
    if pillar_id == "__other__" {
        let assigned: std::collections::HashSet<&str> = ownership
            .iter()
            .flat_map(|(_, paths)| paths.iter().map(String::as_str))
            .collect();
        return tab
            .active_diff_files()
            .iter()
            .map(|f| f.path.clone())
            .filter(|p| !assigned.contains(p.as_str()))
            .collect();
    }
    ownership
        .into_iter()
        .find(|(id, _)| id == pillar_id)
        .map(|(_, paths)| paths)
        .unwrap_or_default()
}

#[tauri::command]
pub fn bulk_review_pillar(
    pillar_id: String,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    {
        let tab = app.tab_mut();
        let paths = pillar_file_paths(tab, &pillar_id);
        let mut changed = false;
        for path in paths {
            let hash = tab
                .current_per_file_hashes
                .get(&path)
                .cloned()
                .unwrap_or_default();
            tab.reviewed.insert(path, hash);
            changed = true;
        }
        if changed {
            tab.reviewed_revision += 1;
            let _ = tab.save_reviewed_files();
        }
    }
    // Full snapshot (not chrome-only): this command is invoked via the generic
    // `app.cmd` path, which replaces the whole snapshot. A chrome-only snapshot
    // carries `files: []`, which would blank the diff. snap_from keeps files
    // (hunks differential-omitted + spliced by the frontend).
    Ok(snap_from(&app, &state))
}

#[tauri::command]
pub fn unbulk_review_pillar(
    pillar_id: String,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    {
        let tab = app.tab_mut();
        let paths = pillar_file_paths(tab, &pillar_id);
        let mut changed = false;
        for path in paths {
            if tab.reviewed.remove(&path).is_some() {
                changed = true;
            }
        }
        if changed {
            tab.reviewed_revision += 1;
            let _ = tab.save_reviewed_files();
        }
    }
    Ok(snap_from(&app, &state))
}

// ── Editor ────────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn open_in_editor(state: State<AppState>) -> Result<OpenSourceResult, String> {
    open_source(state)
}

/// Open the selected file in VS Code (`code -g path:line`) when a local checkout exists.
/// No GitHub/browser fallback — desktop `e` key uses this exclusively.
#[tauri::command]
pub fn open_in_vscode(state: State<AppState>) -> Result<OpenSourceResult, String> {
    let app = state.app.lock().map_err(|e| e.to_string())?;
    let tab = app.tab();
    let file = match tab.selected_diff_file() {
        Some(f) => f,
        None => {
            return Ok(OpenSourceResult {
                kind: "needs_checkout".to_string(),
                target: "No selected file".to_string(),
            });
        }
    };
    let line_num = file
        .hunks
        .get(tab.current_hunk)
        .map(|h| h.new_start)
        .unwrap_or(1);

    if let Some(local_root) = local_source_root(tab) {
        let file_path = Path::new(local_root).join(&file.path);
        if file_path.exists() {
            open_vscode_at(local_root, &file_path, line_num).map_err(|e| e.to_string())?;
            return Ok(OpenSourceResult {
                kind: "opened_local".to_string(),
                target: file_path.to_string_lossy().into_owned(),
            });
        }
    }

    Ok(OpenSourceResult {
        kind: "needs_checkout".to_string(),
        target: "No local checkout found for this file. Check out the branch in a worktree first."
            .to_string(),
    })
}

#[tauri::command]
pub fn open_source(state: State<AppState>) -> Result<OpenSourceResult, String> {
    let app = state.app.lock().map_err(|e| e.to_string())?;
    let tab = app.tab();
    let file = match tab.selected_diff_file() {
        Some(f) => f,
        None => {
            return Ok(OpenSourceResult {
                kind: "needs_checkout".to_string(),
                target: "No selected file".to_string(),
            });
        }
    };
    let line_num = file
        .hunks
        .get(tab.current_hunk)
        .map(|h| h.new_start)
        .unwrap_or(1);

    // Local editable source is only valid when this tab represents a checked-out
    // local context (working tree, or local-branch view with checkout root).
    if let Some(local_root) = local_source_root(tab) {
        let file_path = Path::new(local_root).join(&file.path);
        if file_path.exists() {
            open_editor_at(local_root, &file_path, line_num).map_err(|e| e.to_string())?;
            return Ok(OpenSourceResult {
                kind: "opened_local".to_string(),
                target: file_path.to_string_lossy().into_owned(),
            });
        }
    }

    // Fallback: open GitHub URL for PR/tab-backed sources.
    if let Some(url) = github_file_url_for_tab(tab, &file.path, line_num) {
        drop(app);
        open_url_in_browser(url.clone())?;
        return Ok(OpenSourceResult {
            kind: "opened_url".to_string(),
            target: url,
        });
    }

    Ok(OpenSourceResult {
        kind: "needs_checkout".to_string(),
        target: "No local checkout found for this source. Create editable worktree first."
            .to_string(),
    })
}

fn local_source_root(tab: &er_engine::app::TabState) -> Option<&str> {
    if !allows_local_open(
        tab.is_remote(),
        tab.local_branch_view.is_some(),
        tab.local_branch_checkout_root.is_some(),
    ) {
        return None;
    }
    // Local PR tabs (pr_head_ref set) are read-only review contexts unless the
    // branch is explicitly checked out in a working tree/worktree.
    if tab.local_branch_view.is_some() {
        return tab.local_branch_checkout_root.as_deref();
    }
    Some(tab.repo_root.as_str())
}

fn allows_local_open(
    is_remote: bool,
    has_local_branch_view: bool,
    has_checkout_root: bool,
) -> bool {
    if is_remote {
        return false;
    }
    if has_local_branch_view {
        return has_checkout_root;
    }
    true
}

fn open_editor_at(repo_root: &str, file_path: &Path, line_num: usize) -> anyhow::Result<()> {
    use anyhow::Context;
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "code".to_string());
    let mut cmd = std::process::Command::new(&editor);
    if editor.contains("code") || editor.contains("cursor") {
        cmd.arg(repo_root)
            .arg("-g")
            .arg(format!("{}:{}", file_path.display(), line_num));
    } else if editor.contains("zed") {
        cmd.arg(repo_root)
            .arg(format!("{}:{}", file_path.display(), line_num));
    } else {
        cmd.arg(format!("+{}", line_num)).arg(file_path);
    }
    cmd.spawn().context("Failed to open editor")?;
    Ok(())
}

fn open_vscode_at(repo_root: &str, file_path: &Path, line_num: usize) -> anyhow::Result<()> {
    use anyhow::Context;
    std::process::Command::new("code")
        .arg(repo_root)
        .arg("-g")
        .arg(format!("{}:{}", file_path.display(), line_num))
        .spawn()
        .context("Failed to open VS Code (is `code` on PATH?)")?;
    Ok(())
}

/// Open an http(s) URL in the system default browser (shared by Tauri command + nav policy).
pub fn open_external_url(url: &str) -> Result<(), String> {
    let result = if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(url).spawn()
    } else if cfg!(target_os = "linux") {
        std::process::Command::new("xdg-open").arg(url).spawn()
    } else {
        std::process::Command::new("cmd")
            .args(["/c", "start", url])
            .spawn()
    };
    result.map(|_| ()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_url_in_browser(url: String) -> Result<(), String> {
    open_external_url(&url)
}

fn github_file_url_for_tab(
    tab: &er_engine::app::TabState,
    file_path: &str,
    line_num: usize,
) -> Option<String> {
    let pr_head = tab
        .pr_data
        .as_ref()
        .map(|pr| pr.head_branch.trim().to_string())
        .filter(|b| !b.is_empty());

    let mut branch_ref = pr_head
        .or_else(|| tab.local_branch_view.clone())
        .unwrap_or_else(|| tab.current_branch.clone())
        .trim()
        .to_string();

    // PR tab whose overview hasn't loaded yet: synchronously ask gh for the head branch.
    if tab.pr_data.is_none() {
        if let Some(n) = tab.pr_number {
            if let Ok(name) = er_engine::github::gh_pr_head_branch_name(n, &tab.repo_root) {
                let name = name.trim().to_string();
                if !name.is_empty() {
                    branch_ref = name;
                }
            }
        }
    }

    if branch_ref.is_empty() {
        return None;
    }
    if let Some(slug) = tab.remote_repo.as_ref() {
        return Some(format!(
            "https://github.com/{slug}/blob/{branch_ref}/{file_path}#L{line_num}"
        ));
    }

    let remote = std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(&tab.repo_root)
        .output()
        .ok()?;
    if !remote.status.success() {
        return None;
    }
    let remote = String::from_utf8_lossy(&remote.stdout).trim().to_string();
    let slug = parse_github_slug(&remote)?;
    Some(format!(
        "https://github.com/{slug}/blob/{branch_ref}/{file_path}#L{line_num}"
    ))
}

fn parse_github_slug(remote: &str) -> Option<String> {
    let normalized = remote.trim_end_matches(".git");
    if let Some(rest) = normalized.strip_prefix("git@github.com:") {
        return Some(rest.to_string());
    }
    if let Some(pos) = normalized.find("github.com/") {
        return Some(normalized[(pos + "github.com/".len())..].to_string());
    }
    None
}

fn normalize_check_state(checks: &[er_engine::github::CheckRun]) -> (String, Vec<String>) {
    if checks.is_empty() {
        return ("unknown".to_string(), Vec::new());
    }
    let mut has_pending = false;
    let mut failing = Vec::new();
    for c in checks {
        let status = c.status.to_ascii_uppercase();
        let conclusion = c.conclusion.as_str().to_ascii_uppercase();
        if status == "PENDING" || status == "IN_PROGRESS" || status == "QUEUED" {
            has_pending = true;
        }
        if conclusion == "FAILURE" || conclusion == "TIMED_OUT" || conclusion == "CANCELLED" {
            failing.push(c.name.clone());
        }
    }
    if !failing.is_empty() {
        ("failure".to_string(), failing)
    } else if has_pending {
        ("pending".to_string(), Vec::new())
    } else {
        ("success".to_string(), Vec::new())
    }
}

pub fn process_inbox_after_pr_refresh(
    pr_cache: &Arc<Mutex<HashMap<String, Vec<PrInfo>>>>,
    gh_user_state: &GhUser,
    inbox_handle: &InboxHandle,
    desktop_revision: &Arc<AtomicU64>,
    app_handle_state: &Arc<Mutex<Option<tauri::AppHandle>>>,
    refresh_failed_remote: Option<String>,
    auto_triage: Option<&crate::auto_triage::AutoTriageContext>,
) {
    let now = now_ms();
    if let Ok(mut inbox) = inbox_handle.lock() {
        inbox.last_refresh_ms = now;
    }
    let gh_user = gh_user_state.lock().ok().and_then(|g| g.clone());
    let Some(gh_user) = gh_user else {
        if let Some(remote) = refresh_failed_remote {
            let mut inbox = match inbox_handle.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            let last = inbox.refresh_error_at_ms.get(&remote).copied().unwrap_or(0);
            if now.saturating_sub(last) >= crate::inbox::REFRESH_ERROR_TTL_MS {
                let _ = inbox.add_item(InboxItem {
                    id: format!("inbox-gh-refresh-failed-{remote}-{now}"),
                    kind: "github_refresh_failed".to_string(),
                    severity: "info".to_string(),
                    title: format!("GitHub refresh failed for {remote}"),
                    body: "Could not refresh PR data; using stale cache.".to_string(),
                    source: "github".to_string(),
                    target: InboxTarget {
                        project_id: None,
                        repo_root: None,
                        remote: Some(remote.clone()),
                        pr_number: None,
                        branch: None,
                        url: None,
                    },
                    created_at_ms: now,
                    read_at_ms: None,
                    dedupe_key: format!("github:{remote}:refresh_failed"),
                });
                inbox.refresh_error_at_ms.insert(remote, now);
            }
            drop(inbox);
            crate::inbox::save_inbox_state(inbox_handle);
            crate::profile_log::bump_desktop_revision(desktop_revision, "inbox_refresh_failed");
        }
        return;
    };

    let projects_file = projects::load();
    let mut project_by_remote: HashMap<String, projects::ProjectRecord> = HashMap::new();
    for p in projects_file.projects {
        if let Some(remote) = p.remote.clone() {
            project_by_remote.insert(remote, p);
        }
    }
    let cache = pr_cache.lock().ok().map(|g| g.clone()).unwrap_or_default();

    let mut new_items: Vec<InboxItem> = Vec::new();
    let mut auto_triage_requests: Vec<crate::auto_triage::AutoTriageRequest> = Vec::new();
    let mut ci_work: Vec<(String, String, u64, String, String, String)> = Vec::new();
    let mut inbox = match inbox_handle.lock() {
        Ok(g) => g,
        Err(_) => return,
    };

    if let Some(remote) = refresh_failed_remote {
        let last = inbox.refresh_error_at_ms.get(&remote).copied().unwrap_or(0);
        if now.saturating_sub(last) >= crate::inbox::REFRESH_ERROR_TTL_MS {
            new_items.push(InboxItem {
                id: format!("inbox-gh-refresh-failed-{remote}-{now}"),
                kind: "github_refresh_failed".to_string(),
                severity: "info".to_string(),
                title: format!("GitHub refresh failed for {remote}"),
                body: "Could not refresh PR data; using stale cache.".to_string(),
                source: "github".to_string(),
                target: InboxTarget {
                    project_id: project_by_remote.get(&remote).map(|p| p.id.clone()),
                    repo_root: project_by_remote.get(&remote).map(|p| p.root_path.clone()),
                    remote: Some(remote.clone()),
                    pr_number: None,
                    branch: None,
                    url: None,
                },
                created_at_ms: now,
                read_at_ms: None,
                dedupe_key: format!("github:{remote}:refresh_failed"),
            });
            inbox.refresh_error_at_ms.insert(remote, now);
        }
    }

    for (remote, prs) in cache {
        for pr in prs {
            let key = format!("{remote}#{}", pr.number);
            let requested_me = pr.reviewers.iter().any(|r| r == &gh_user);
            let requested_reviewers = pr.reviewers.clone();
            let is_my_pr = pr.author == gh_user;
            let prev = inbox.observed_pr.get(&key).cloned();

            if let Some(prev_state) = &prev {
                if is_my_pr {
                    if pr.review_decision.as_deref() == Some("APPROVED")
                        && prev_state.review_decision.as_deref() != Some("APPROVED")
                    {
                        new_items.push(InboxItem {
                            id: format!("inbox-pr-approved-{remote}-{}-{now}", pr.number),
                            kind: "pr_review_approved".to_string(),
                            severity: "success".to_string(),
                            title: format!("PR #{} approved", pr.number),
                            body: pr.title.clone(),
                            source: "github".to_string(),
                            target: InboxTarget {
                                project_id: project_by_remote.get(&remote).map(|p| p.id.clone()),
                                repo_root: project_by_remote
                                    .get(&remote)
                                    .map(|p| p.root_path.clone()),
                                remote: Some(remote.clone()),
                                pr_number: Some(pr.number),
                                branch: Some(pr.head_ref.clone()),
                                url: None,
                            },
                            created_at_ms: now,
                            read_at_ms: None,
                            dedupe_key: format!(
                                "github:{remote}:{}:review_decision:APPROVED",
                                pr.number
                            ),
                        });
                    }
                    if pr.review_decision.as_deref() == Some("CHANGES_REQUESTED")
                        && prev_state.review_decision.as_deref() != Some("CHANGES_REQUESTED")
                    {
                        new_items.push(InboxItem {
                            id: format!("inbox-pr-changes-{remote}-{}-{now}", pr.number),
                            kind: "pr_review_changes_requested".to_string(),
                            severity: "warning".to_string(),
                            title: format!("Changes requested on PR #{}", pr.number),
                            body: pr.title.clone(),
                            source: "github".to_string(),
                            target: InboxTarget {
                                project_id: project_by_remote.get(&remote).map(|p| p.id.clone()),
                                repo_root: project_by_remote
                                    .get(&remote)
                                    .map(|p| p.root_path.clone()),
                                remote: Some(remote.clone()),
                                pr_number: Some(pr.number),
                                branch: Some(pr.head_ref.clone()),
                                url: None,
                            },
                            created_at_ms: now,
                            read_at_ms: None,
                            dedupe_key: format!(
                                "github:{remote}:{}:review_decision:CHANGES_REQUESTED",
                                pr.number
                            ),
                        });
                    }
                }
                if !is_my_pr {
                    let prev_requested =
                        prev_state.requested_reviewers.iter().any(|r| r == &gh_user);
                    if requested_me && !prev_requested {
                        let kind = if prev_state.requested_reviewers.contains(&gh_user) {
                            "review_rerequested"
                        } else {
                            "review_requested"
                        };
                        new_items.push(InboxItem {
                            id: format!("inbox-{kind}-{remote}-{}-{now}", pr.number),
                            kind: kind.to_string(),
                            severity: "info".to_string(),
                            title: format!("Review requested: PR #{}", pr.number),
                            body: pr.title.clone(),
                            source: "github".to_string(),
                            target: InboxTarget {
                                project_id: project_by_remote.get(&remote).map(|p| p.id.clone()),
                                repo_root: project_by_remote
                                    .get(&remote)
                                    .map(|p| p.root_path.clone()),
                                remote: Some(remote.clone()),
                                pr_number: Some(pr.number),
                                branch: Some(pr.head_ref.clone()),
                                url: None,
                            },
                            created_at_ms: now,
                            read_at_ms: None,
                            dedupe_key: format!("github:{remote}:{}:{kind}", pr.number),
                        });
                    }
                }
                if prev_state.pr_state != pr.state {
                    if pr.state == "MERGED" {
                        new_items.push(InboxItem {
                            id: format!("inbox-pr-merged-{remote}-{}-{now}", pr.number),
                            kind: "pr_merged".to_string(),
                            severity: "success".to_string(),
                            title: format!("PR #{} merged", pr.number),
                            body: pr.title.clone(),
                            source: "github".to_string(),
                            target: InboxTarget {
                                project_id: project_by_remote.get(&remote).map(|p| p.id.clone()),
                                repo_root: project_by_remote
                                    .get(&remote)
                                    .map(|p| p.root_path.clone()),
                                remote: Some(remote.clone()),
                                pr_number: Some(pr.number),
                                branch: Some(pr.head_ref.clone()),
                                url: None,
                            },
                            created_at_ms: now,
                            read_at_ms: None,
                            dedupe_key: format!("github:{remote}:{}:merged", pr.number),
                        });
                    } else if pr.state == "CLOSED" {
                        new_items.push(InboxItem {
                            id: format!("inbox-pr-closed-{remote}-{}-{now}", pr.number),
                            kind: "pr_closed".to_string(),
                            severity: "info".to_string(),
                            title: format!("PR #{} closed", pr.number),
                            body: pr.title.clone(),
                            source: "github".to_string(),
                            target: InboxTarget {
                                project_id: project_by_remote.get(&remote).map(|p| p.id.clone()),
                                repo_root: project_by_remote
                                    .get(&remote)
                                    .map(|p| p.root_path.clone()),
                                remote: Some(remote.clone()),
                                pr_number: Some(pr.number),
                                branch: Some(pr.head_ref.clone()),
                                url: None,
                            },
                            created_at_ms: now,
                            read_at_ms: None,
                            dedupe_key: format!("github:{remote}:{}:closed", pr.number),
                        });
                    }
                }
            }

            if is_my_pr && pr.state == "OPEN" {
                let ci_key = format!("{remote}#{}", pr.number);
                let should_fetch_ci = inbox
                    .ci_state
                    .get(&ci_key)
                    .map(|c| now.saturating_sub(c.fetched_at_ms) >= crate::inbox::CI_TTL_MS)
                    .unwrap_or(true);
                if should_fetch_ci {
                    if let Some((owner, repo_name)) = remote.split_once('/') {
                        ci_work.push((
                            remote.clone(),
                            owner.to_string(),
                            pr.number,
                            repo_name.to_string(),
                            pr.title.clone(),
                            pr.head_ref.clone(),
                        ));
                    }
                }
            }

            let triaged_head_oid = prev.as_ref().and_then(|p| p.triaged_head_oid.clone());
            let queue_ctx = crate::auto_triage::AutoTriageQueueContext {
                is_my_pr,
                requested_me,
                is_new_pr: prev.is_none(),
                triaged_head_oid: triaged_head_oid.as_deref(),
            };
            let queue_auto_triage = auto_triage.is_some()
                && project_by_remote.get(&remote).is_some_and(|project| {
                    crate::auto_triage::should_queue_auto_triage(project, &pr, queue_ctx)
                });
            if queue_auto_triage {
                if let Some(project) = project_by_remote.get(&remote) {
                    auto_triage_requests.push(crate::auto_triage::AutoTriageRequest {
                        project_id: project.id.clone(),
                        remote: remote.clone(),
                        repo_root: project.root_path.clone(),
                        pr_number: pr.number,
                        head_oid: pr.head_oid.clone(),
                        base_ref: pr.base_ref.clone(),
                        review_ignore_globs: project.review_ignore_globs.clone(),
                        auto_triage_max_diff_kb: project.auto_triage_max_diff_kb,
                    });
                }
            }

            inbox.observed_pr.insert(
                key,
                crate::inbox::ObservedPrState {
                    review_decision: pr.review_decision.clone(),
                    requested_reviewers,
                    pr_state: pr.state.clone(),
                    is_my_pr,
                    check_state: prev.as_ref().and_then(|p| p.check_state.clone()),
                    failing_checks: prev
                        .as_ref()
                        .map(|p| p.failing_checks.clone())
                        .unwrap_or_default(),
                    head_oid: pr.head_oid.clone(),
                    triaged_head_oid: if queue_auto_triage {
                        Some(pr.head_oid.clone())
                    } else {
                        triaged_head_oid
                    },
                },
            );
        }
    }
    drop(inbox);

    for (remote, owner, pr_number, repo_name, pr_title, head_ref) in ci_work {
        let checks = er_engine::github::gh_pr_checks_remote(&owner, &repo_name, pr_number)
            .unwrap_or_default();
        let (state_name, failing_checks) = normalize_check_state(&checks);
        let ci_key = format!("{remote}#{pr_number}");
        let mut inbox = match inbox_handle.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        let prev = inbox.ci_state.get(&ci_key).cloned();
        let prev_state = prev
            .as_ref()
            .map(|c| c.check_state.as_str())
            .unwrap_or("unknown");
        if state_name == "failure" && prev_state != "failure" {
            let body = if failing_checks.is_empty() {
                pr_title.clone()
            } else {
                format!("{pr_title} — failing: {}", failing_checks.join(", "))
            };
            new_items.push(InboxItem {
                id: format!("inbox-ci-failed-{remote}-{pr_number}-{now}"),
                kind: "ci_failed".to_string(),
                severity: "warning".to_string(),
                title: format!("CI failed on PR #{pr_number}"),
                body,
                source: "github".to_string(),
                target: InboxTarget {
                    project_id: project_by_remote.get(&remote).map(|p| p.id.clone()),
                    repo_root: project_by_remote.get(&remote).map(|p| p.root_path.clone()),
                    remote: Some(remote.clone()),
                    pr_number: Some(pr_number),
                    branch: Some(head_ref.clone()),
                    url: None,
                },
                created_at_ms: now,
                read_at_ms: None,
                dedupe_key: format!("github:{remote}:{pr_number}:ci_failed"),
            });
        }
        inbox.ci_state.insert(
            ci_key.clone(),
            crate::inbox::ObservedCiState {
                fetched_at_ms: now,
                check_state: state_name.clone(),
                failing_checks: failing_checks.clone(),
            },
        );
        if let Some(pr_state) = inbox.observed_pr.get_mut(&ci_key) {
            pr_state.check_state = Some(state_name);
            pr_state.failing_checks = failing_checks;
        }
    }

    let mut emitted_any = false;
    let mut just_added: Vec<InboxItem> = Vec::new();
    if let Ok(mut inbox) = inbox_handle.lock() {
        for item in new_items {
            if inbox.add_item(item.clone()) {
                emitted_any = true;
                just_added.push(item);
            }
        }
    }
    for item in &just_added {
        maybe_send_native_notification(inbox_handle, app_handle_state, item);
    }
    crate::inbox::save_inbox_state(inbox_handle);
    if emitted_any {
        crate::profile_log::bump_desktop_revision(desktop_revision, "inbox_items");
    }
    if let Some(ctx) = auto_triage {
        if !auto_triage_requests.is_empty() {
            crate::auto_triage::dispatch_auto_triage(ctx, auto_triage_requests);
        }
    }
}

#[tauri::command]
pub fn set_project_auto_triage(
    project_id: String,
    enabled: bool,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    projects::set_auto_triage(&project_id, enabled).map_err(|e| e.to_string())?;
    crate::profile_log::bump_desktop_revision(&state.desktop_revision, "project_auto_triage");
    let app = state.app.lock().map_err(|e| e.to_string())?;
    Ok(snap_from(&app, &state))
}

#[tauri::command]
pub fn set_project_auto_triage_own_prs(
    project_id: String,
    enabled: bool,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    projects::set_auto_triage_own_prs(&project_id, enabled).map_err(|e| e.to_string())?;
    crate::profile_log::bump_desktop_revision(&state.desktop_revision, "project_auto_triage_own");
    let app = state.app.lock().map_err(|e| e.to_string())?;
    Ok(snap_from(&app, &state))
}

fn auto_triage_ctx(state: &AppState) -> crate::auto_triage::AutoTriageContext {
    crate::auto_triage::AutoTriageContext {
        app: Arc::clone(&state.app),
        in_flight: Arc::clone(&state.auto_triage_in_flight),
        desktop_revision: Arc::clone(&state.desktop_revision),
    }
}

fn pr_from_cache(
    pr_cache: &Arc<Mutex<HashMap<String, Vec<crate::snapshot::PrInfo>>>>,
    remote: &str,
    pr_number: u64,
) -> Option<crate::snapshot::PrInfo> {
    pr_cache.lock().ok().and_then(|cache| {
        cache
            .get(remote)
            .and_then(|prs| prs.iter().find(|p| p.number == pr_number).cloned())
    })
}

fn open_pr_for_branch(
    pr_cache: &Arc<Mutex<HashMap<String, Vec<crate::snapshot::PrInfo>>>>,
    remote: &str,
    branch: &str,
) -> Option<crate::snapshot::PrInfo> {
    pr_cache.lock().ok().and_then(|cache| {
        cache.get(remote).and_then(|prs| {
            prs.iter()
                .find(|p| p.state == "OPEN" && !p.is_draft && p.head_ref == branch)
                .cloned()
        })
    })
}

fn mark_pr_triaged(inbox_handle: &InboxHandle, remote: &str, pr_number: u64, head_oid: &str) {
    let key = format!("{remote}#{pr_number}");
    if let Ok(mut inbox) = inbox_handle.lock() {
        if let Some(state) = inbox.observed_pr.get_mut(&key) {
            state.triaged_head_oid = Some(head_oid.to_string());
        }
        drop(inbox);
        crate::inbox::save_inbox_state(inbox_handle);
    }
}

/// Start triage for a single PR from the sidebar (does not open the review tab).
#[tauri::command]
pub fn run_pr_triage(
    project_id: String,
    pr_number: u64,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let file = projects::load();
    let project = file
        .projects
        .iter()
        .find(|p| p.id == project_id)
        .ok_or_else(|| "Project not found".to_string())?;
    let remote = project
        .remote
        .clone()
        .ok_or_else(|| "Project has no GitHub remote".to_string())?;
    let pr = pr_from_cache(&state.pr_cache, &remote, pr_number)
        .ok_or_else(|| format!("PR #{pr_number} not in cache — try Sync first"))?;

    let req = crate::auto_triage::AutoTriageRequest {
        project_id: project.id.clone(),
        remote: remote.clone(),
        repo_root: project.root_path.clone(),
        pr_number: pr.number,
        head_oid: pr.head_oid.clone(),
        base_ref: pr.base_ref.clone(),
        review_ignore_globs: project.review_ignore_globs.clone(),
        auto_triage_max_diff_kb: project.auto_triage_max_diff_kb,
    };
    let ctx = auto_triage_ctx(&state);
    crate::auto_triage::dispatch_auto_triage(&ctx, vec![req]);
    if !pr.head_oid.is_empty() {
        mark_pr_triaged(&state.inbox, &remote, pr_number, &pr.head_oid);
    }
    let app = state.app.lock().map_err(|e| e.to_string())?;
    Ok(snap_from(&app, &state))
}

/// Triage a tracked branch: uses the open PR when `branch` is a PR head, else local branch diff.
#[tauri::command]
pub fn run_branch_triage(
    project_id: String,
    branch: String,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let file = projects::load();
    let project = file
        .projects
        .iter()
        .find(|p| p.id == project_id)
        .ok_or_else(|| "Project not found".to_string())?;
    let remote = project
        .remote
        .clone()
        .ok_or_else(|| "Project has no GitHub remote".to_string())?;

    if let Some(pr) = open_pr_for_branch(&state.pr_cache, &remote, &branch) {
        return run_pr_triage(project_id, pr.number, state);
    }

    let ctx = auto_triage_ctx(&state);
    crate::auto_triage::dispatch_branch_triage(
        &ctx,
        &project.id,
        &remote,
        &project.root_path,
        &branch,
        &project.review_ignore_globs,
    );
    let app = state.app.lock().map_err(|e| e.to_string())?;
    Ok(snap_from(&app, &state))
}

/// Remove a queued (not yet started) review from the AI review queue.
/// Running reviews are unaffected.
#[tauri::command]
pub fn cancel_queued_review(id: String, state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    if !app.cancel_queued_background_task(&id) {
        return Err("Queued review not found (it may have started)".to_string());
    }
    state.desktop_revision.fetch_add(1, Ordering::Relaxed);
    Ok(snap_from(&app, &state))
}

#[tauri::command]
pub fn patch_project_review_settings(
    project_id: String,
    patch: projects::ProjectReviewSettingsPatch,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    projects::patch_project_review_settings(&project_id, patch).map_err(|e| e.to_string())?;
    crate::profile_log::bump_desktop_revision(&state.desktop_revision, "project_review_settings");
    let app = state.app.lock().map_err(|e| e.to_string())?;
    Ok(snap_from(&app, &state))
}

#[tauri::command]
pub fn reveal_er_folder(state: State<AppState>) -> Result<(), String> {
    let er_dir = {
        let app = state.app.lock().map_err(|e| e.to_string())?;
        app.tab().er_dir()
    };
    let target = std::path::Path::new(&er_dir);
    // Ensure the directory exists so reveal never fails on a fresh or empty revision.
    let _ = std::fs::create_dir_all(target);
    let result = if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(target).spawn()
    } else if cfg!(target_os = "linux") {
        std::process::Command::new("xdg-open").arg(target).spawn()
    } else {
        std::process::Command::new("explorer").arg(target).spawn()
    };
    result.map(|_| ()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn reveal_path(path: String) -> Result<(), String> {
    let target = std::path::Path::new(&path);
    let result = if cfg!(target_os = "macos") {
        std::process::Command::new("open")
            .arg("-R")
            .arg(target)
            .spawn()
    } else if cfg!(target_os = "linux") {
        let parent = target.parent().unwrap_or(target);
        std::process::Command::new("xdg-open").arg(parent).spawn()
    } else {
        let arg = format!("/select,{}", target.display());
        std::process::Command::new("explorer").arg(arg).spawn()
    };
    result.map(|_| ()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_review_revisions(state: State<AppState>) -> Result<Vec<ReviewRevisionSummary>, String> {
    // Branch-level managed storage no longer keeps multiple revisions per
    // branch — re-running review overwrites the same files in place. The
    // returned list is now at most one entry representing the current branch
    // state, so the existing UI (ExportModal, AgentOutputView) keeps working
    // without a revision picker.
    let app = state.app.lock().map_err(|e| e.to_string())?;
    let tab = app.tab();
    if tab.repo_root.is_empty() {
        return Ok(Vec::new());
    }
    let er_dir = tab.er_dir();
    let review_path = std::path::Path::new(&er_dir).join("review.json");
    if !review_path.exists() {
        return Ok(Vec::new());
    }
    Ok(vec![ReviewRevisionSummary {
        revision_id: "current".to_string(),
        created_at: String::new(),
        scope: String::new(),
        diff_hash: tab.branch_diff_hash.clone(),
        active: true,
        agents: vec!["claude".to_string()],
    }])
}

#[tauri::command]
pub fn read_review_json(
    state: State<AppState>,
    revision_id: Option<String>,
) -> Result<String, String> {
    let _ = revision_id; // single-revision model — ignored
    let app = state.app.lock().map_err(|e| e.to_string())?;
    let er_dir = app.tab().er_dir();
    let review_path = std::path::Path::new(&er_dir).join("review.json");
    if !review_path.exists() {
        return Err("No review.json found".to_string());
    }
    std::fs::read_to_string(&review_path).map_err(|e| e.to_string())
}

// ── Filter / search ───────────────────────────────────────────────────────────

// The file-tree filter applies live while the user types (debounced in
// FileTree.svelte). Sync commands run on the main thread, and re-filtering +
// rebuilding the snapshot on a large diff is heavy enough to visibly freeze
// the window per apply — so both commands run via `run_blocking`.
#[tauri::command]
pub async fn set_filter(query: String, state: State<'_, AppState>) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        app.tab_mut().apply_filter_expr(&query);
        Ok(snap_from(&app, &state))
    })
    .await
}

#[tauri::command]
pub async fn clear_filter(state: State<'_, AppState>) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        app.tab_mut().apply_filter_expr("");
        Ok(snap_from(&app, &state))
    })
    .await
}

// ── Threads ───────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn add_comment(
    file: String,
    hunk_idx: usize,
    line_num: Option<usize>,
    line_num_end: Option<usize>,
    text: String,
    side: Option<String>,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    // Set side before submit so submit_github_comment can consume it
    if let Some(ref s) = side {
        app.tab_mut().comment_side = Some(s.clone());
    }
    app.submit_comment_text(
        file,
        hunk_idx,
        line_num,
        line_num_end,
        text,
        CommentType::GitHubComment,
        None,
        None,
    )
    .map_err(|e| e.to_string())?;
    Ok(snap_from(&app, &state))
}

#[tauri::command]
pub fn add_question(
    file: String,
    hunk_idx: usize,
    line_num: Option<usize>,
    line_num_end: Option<usize>,
    text: String,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    app.submit_comment_text(
        file,
        hunk_idx,
        line_num,
        line_num_end,
        text,
        CommentType::Question,
        None,
        None,
    )
    .map_err(|e| e.to_string())?;
    Ok(snap_from(&app, &state))
}

#[tauri::command]
pub fn add_note(
    file: String,
    hunk_idx: usize,
    line_num: Option<usize>,
    line_num_end: Option<usize>,
    text: String,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    app.submit_comment_text(
        file,
        hunk_idx,
        line_num,
        line_num_end,
        text,
        CommentType::Note,
        None,
        None,
    )
    .map_err(|e| e.to_string())?;
    Ok(snap_from(&app, &state))
}

#[tauri::command]
pub fn reply_to_thread(
    parent_id: String,
    text: String,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let (file, hunk_idx, line_num, comment_type) = {
        let tab = app.tab();
        if parent_id.starts_with("q-") {
            let q = tab
                .ai
                .questions
                .as_ref()
                .and_then(|qs| qs.questions.iter().find(|q| q.id == parent_id))
                .map(|q| {
                    (
                        q.file.clone(),
                        q.hunk_index.unwrap_or(0),
                        q.line_start,
                        CommentType::Question,
                    )
                });
            q.ok_or_else(|| "Question not found".to_string())?
        } else if parent_id.starts_with("n-") {
            let n = tab
                .ai
                .notes
                .as_ref()
                .and_then(|ns| ns.notes.iter().find(|n| n.id == parent_id))
                .map(|n| {
                    (
                        n.file.clone(),
                        n.hunk_index.unwrap_or(0),
                        n.line_start,
                        CommentType::Note,
                    )
                });
            n.ok_or_else(|| "Note not found".to_string())?
        } else {
            let c = tab
                .ai
                .github_comments
                .as_ref()
                .and_then(|gc| gc.comments.iter().find(|c| c.id == parent_id))
                .map(|c| {
                    (
                        c.file.clone(),
                        c.hunk_index.unwrap_or(0),
                        c.line_start,
                        CommentType::GitHubComment,
                    )
                });
            c.ok_or_else(|| "Comment not found".to_string())?
        }
    };
    app.submit_comment_text(
        file,
        hunk_idx,
        line_num,
        None,
        text,
        comment_type,
        Some(parent_id),
        None,
    )
    .map_err(|e| e.to_string())?;
    Ok(snap_from(&app, &state))
}

#[tauri::command]
pub fn delete_thread(id: String, state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    app.delete_comment_direct(&id).map_err(|e| e.to_string())?;
    if let Ok(mut p) = state.pending_ai_replies.lock() {
        p.remove(&id);
    }
    state
        .desktop_revision
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Ok(snap_from(&app, &state))
}

/// Remove all question/github threads linked to a finding (`finding_ref`), keeping the finding.
fn finding_linked_thread_ids(ai: &er_engine::ai::AiState, finding_id: &str) -> Vec<String> {
    let mut ids = Vec::new();
    if let Some(qs) = ai.questions.as_ref() {
        for q in &qs.questions {
            if q.finding_ref.as_deref() == Some(finding_id) {
                ids.push(q.id.clone());
            }
        }
    }
    if let Some(ns) = ai.notes.as_ref() {
        for n in &ns.notes {
            if n.finding_ref.as_deref() == Some(finding_id) {
                ids.push(n.id.clone());
            }
        }
    }
    if let Some(gc) = ai.github_comments.as_ref() {
        for c in &gc.comments {
            if c.finding_ref.as_deref() == Some(finding_id) {
                ids.push(c.id.clone());
            }
        }
    }
    ids
}

#[tauri::command]
pub fn remove_finding_thread(
    finding_id: String,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let er_dir = app.tab().er_dir();

    let pending_ids = finding_linked_thread_ids(&app.tab().ai, &finding_id);
    er_engine::ai::delete_threads_linked_to_finding(&er_dir, &finding_id)
        .map_err(|e| format!("Failed to remove finding thread: {e}"))?;

    if let Ok(mut p) = state.pending_ai_replies.lock() {
        for id in pending_ids {
            p.remove(&id);
        }
        if let Some(root) = er_engine::ai::find_finding_thread_root(&app.tab().ai, &finding_id) {
            p.remove(&root);
        }
    }

    app.tab_mut().reload_ai_state();
    state
        .desktop_revision
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Ok(snap_from(&app, &state))
}

fn write_json_atomic<T: serde::Serialize>(path: &str, value: &T) -> Result<(), String> {
    let tmp = format!("{path}.tmp");
    let body = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, body).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

fn mark_thread_resolved_in_files(
    id: &str,
    q_path: &str,
    notes_path: &str,
    gc_path: &str,
) -> Result<bool, String> {
    use er_engine::ai::{ErGitHubComments, ErNotes, ErQuestions};
    if let Ok(text) = std::fs::read_to_string(q_path) {
        if let Ok(mut qs) = serde_json::from_str::<ErQuestions>(&text) {
            if let Some(q) = qs.questions.iter_mut().find(|q| q.id == id) {
                if !q.resolved {
                    q.resolved = true;
                    write_json_atomic(q_path, &qs)?;
                }
                return Ok(true);
            }
        }
    }

    if let Ok(text) = std::fs::read_to_string(notes_path) {
        if let Ok(mut ns) = serde_json::from_str::<ErNotes>(&text) {
            if let Some(n) = ns.notes.iter_mut().find(|n| n.id == id) {
                if !n.resolved {
                    n.resolved = true;
                    write_json_atomic(notes_path, &ns)?;
                }
                return Ok(true);
            }
        }
    }

    if let Ok(text) = std::fs::read_to_string(gc_path) {
        if let Ok(mut gc) = serde_json::from_str::<ErGitHubComments>(&text) {
            if let Some(c) = gc.comments.iter_mut().find(|c| c.id == id) {
                if !c.resolved {
                    c.resolved = true;
                    write_json_atomic(gc_path, &gc)?;
                }
                return Ok(true);
            }
        }
    }
    Ok(false)
}

#[tauri::command]
pub fn resolve_thread(id: String, state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;

    let tab = app.tab();
    let q_path = format!("{}/questions.json", tab.er_dir());
    let notes_path = format!("{}/notes.json", tab.er_dir());
    let gc_path = tab.github_comments_path();
    let changed = mark_thread_resolved_in_files(&id, &q_path, &notes_path, &gc_path)?;
    if !changed {
        return Err(format!("Thread not found or already resolved: {id}"));
    }
    app.tab_mut().reload_ai_state();
    Ok(snap_from(&app, &state))
}

fn validate_review_submission(
    pending_line_comment_count: usize,
    summary: &str,
) -> Result<(), String> {
    if pending_line_comment_count == 0 && summary.trim().is_empty() {
        return Err(
            "Nothing to submit. Add at least one unpushed GitHub comment or enter a review summary. Local questions are private and are not submitted.".to_string()
        );
    }
    Ok(())
}

fn is_anchor_in_current_diff(
    file_anchors: &std::collections::HashMap<String, Vec<(usize, usize)>>,
    file: &str,
    line: usize,
) -> bool {
    file_anchors.get(file).is_some_and(|ranges| {
        ranges
            .iter()
            .any(|(start, end)| line >= *start && line < *end)
    })
}

fn anchor_range_in_current_diff(
    file_anchors: &std::collections::HashMap<String, Vec<(usize, usize)>>,
    file: &str,
    line_start: usize,
    line_end: usize,
) -> bool {
    is_anchor_in_current_diff(file_anchors, file, line_start)
        && is_anchor_in_current_diff(file_anchors, file, line_end)
}

// ── GitHub sync ───────────────────────────────────────────────────────────────

#[tauri::command]
pub fn refresh_diff(state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    app.tab_mut().refresh_diff().map_err(|e| e.to_string())?;
    Ok(snap_from(&app, &state))
}

/// Latest known PR `head_oid` for `pr_number` from the PR-list cache. This is
/// the exact source the stale-pill compares against (see `build_snapshot`), so
/// callers can align a tab's `last_diff_head_oid` with it to clear the pill.
fn pr_cache_head_oid_for_pr(state: &AppState, pr_number: u64) -> Option<String> {
    state.pr_cache.lock().ok().and_then(|cache| {
        cache
            .values()
            .flat_map(|prs| prs.iter())
            .find(|p| p.number == pr_number)
            .map(|p| p.head_oid.clone())
            .filter(|s| !s.is_empty())
    })
}

#[tauri::command]
pub async fn force_refresh_diff(state: State<'_, AppState>) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    // Heavy: shells out to git/gh fetches. Run off the main thread so the
    // window stays responsive (it previously froze for the whole fetch).
    run_blocking(move || {
        let root = {
            let mut app = state.app.lock().map_err(|e| e.to_string())?;
            app.tab_mut()
                .refetch_and_refresh_diff()
                .map_err(|e| e.to_string())?;

            // Clear the stale pill on a completed sync by aligning the tab's
            // freshness baseline with the exact source the pill compares against
            // (see `build_snapshot`'s `diff_stale`). Without this the pill stays
            // stuck "stale" after a successful sync, because the value recorded
            // during refresh comes from a different source than the comparison.
            let pr_number = app.tab().pr_number;
            let is_branch_diff = app.tab().shows_branch_base_diff();
            let base_branch = app.tab().base_branch.clone();
            let repo_root = app.tab().repo_root.clone();

            if let Some(pr_number) = pr_number {
                // PR pill compares pr_cache `head_oid` (gh) vs `last_diff_head_oid`,
                // but refresh records the latter from the local pull/<n>/head ref.
                // Mirror the remote loop and adopt the cache value.
                if let Some(oid) = pr_cache_head_oid_for_pr(&state, pr_number) {
                    app.tab_mut().last_diff_head_oid = Some(oid);
                }
            } else if is_branch_diff {
                // Branch pill compares the ls-remote probe cache vs the local
                // `origin/<base>` oid. After the sync just fetched `origin/<base>`,
                // adopt that oid as the probed value so the two agree.
                let base_short = base_branch.trim_start_matches("origin/").to_string();
                if let Some(oid) = er_engine::github::rev_parse_oid(&repo_root, &base_branch) {
                    let key = format!("{repo_root}\u{0}{base_short}");
                    if let Ok(mut cache) = state.branch_base_remote_oid.lock() {
                        cache.insert(key, oid);
                    }
                }
            }

            crate::tabs::persist_app_tabs(&app);
            repo_root
        };
        kick_meta_refresh(&state, root);
        snap!(state)
    })
    .await
}

/// Trigger an immediate background refresh of the GitHub status for the active tab.
/// Returns the current snapshot without waiting — the next poll will pick up the fresh data.
#[tauri::command]
pub fn refresh_github_status(state: State<AppState>) -> Result<AppSnapshot, String> {
    let key = {
        let app = state.app.lock().map_err(|e| e.to_string())?;
        active_github_key(&app, &state)
    };
    if let Some((owner, repo, number)) = key {
        kick_github_status_refresh(
            state.gh_status_cache.clone(),
            Arc::clone(&state.gh_status_in_flight),
            Arc::clone(&state.desktop_revision),
            Some(Arc::clone(&state.loading)),
            owner,
            repo,
            number,
        );
    }
    snap!(state)
}

#[tauri::command]
pub fn pull_github_comments(state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    app.sync_github_comments().map_err(|e| e.to_string())?;
    Ok(snap_from(&app, &state))
}

#[tauri::command]
pub fn push_github_comments(state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    app.push_all_comments_to_github()
        .map_err(|e| e.to_string())?;
    Ok(snap_from(&app, &state))
}

/// Push a single local comment thread (root + replies) to GitHub.
#[tauri::command]
pub fn push_github_comment_thread(
    id: String,
    pr_number: Option<u64>,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    app.push_github_comment_thread(&id, pr_number)
        .map_err(|e| e.to_string())?;
    Ok(snap_from(&app, &state))
}

/// Push a single unsynced local GitHub comment reply (parent must already be on GitHub).
#[tauri::command]
pub fn push_github_comment_reply(
    reply_id: String,
    pr_number: Option<u64>,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    if reply_id.starts_with("fr-") {
        return Err("Finding validation replies cannot be pushed individually — promote the finding instead.".to_string());
    }
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    app.push_github_comment_reply(&reply_id, pr_number)
        .map_err(|e| e.to_string())?;
    Ok(snap_from(&app, &state))
}

/// Submit pending local comments as a GitHub PR review with an explicit decision.
/// `mode` must be "COMMENT", "APPROVE", or "REQUEST_CHANGES".
/// `summary` is the top-level review body sent to GitHub.
fn gh_review_submit_err(e: anyhow::Error) -> String {
    let raw = e.to_string();
    if is_own_pr_approval_error(&raw) {
        return own_pr_approval_error();
    }
    let mut hints = Vec::new();
    if raw.contains("422") || raw.to_lowercase().contains("unprocessable") {
        hints.push("blank review payload");
        hints.push("invalid or stale line anchor");
        hints.push("comment position no longer matches PR head");
    }
    if hints.is_empty() {
        format!("GitHub review submission failed: {raw}")
    } else {
        format!(
            "GitHub review submission failed: {raw}\nLikely causes: {}",
            hints.join("; ")
        )
    }
}

fn is_gh_review_422(err: &anyhow::Error) -> bool {
    let raw = err.to_string();
    raw.contains("422") || raw.to_lowercase().contains("unprocessable")
}

#[tauri::command]
pub fn submit_github_review(
    mode: String,
    summary: String,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    use er_engine::ai::ErGitHubComments;
    use er_engine::github;

    let event = match mode.as_str() {
        "APPROVE" | "REQUEST_CHANGES" | "COMMENT" => mode.as_str(),
        _ => {
            return Err(format!(
                "Invalid review mode: {mode}. Use COMMENT, APPROVE, or REQUEST_CHANGES."
            ))
        }
    };

    // Fast preflight for the common desktop PR-review path: if the active tab
    // already carries PR metadata, avoid refreshing the diff and waiting for
    // GitHub only to get the known own-PR approval rejection.
    if event == "APPROVE" {
        let app = state.app.lock().map_err(|e| e.to_string())?;
        if let Some((owner, repo, number)) = active_github_key(&app, &state) {
            let me = state.gh_user.lock().ok().and_then(|login| login.clone());
            if let (Some(me), Some(author)) =
                (me, active_pr_author(&app, &state, &owner, &repo, number))
            {
                if author.eq_ignore_ascii_case(&me) {
                    return Err(own_pr_approval_error());
                }
            }
        }
    }

    // Refresh the local diff so line anchors are checked against the latest tree.
    {
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        if !app.tab().is_remote() {
            app.tab_mut()
                .refetch_and_refresh_diff()
                .map_err(|e| e.to_string())?;
        }
    }

    let (
        owner,
        repo_name,
        pr_number,
        is_remote,
        repo_root,
        comments_path,
        file_anchors,
        old_file_anchors,
    ) = {
        let app = state.app.lock().map_err(|e| e.to_string())?;
        let tab = app.tab();
        let is_remote = tab.is_remote();
        let repo_root = tab.repo_root.clone();
        let comments_path = tab.github_comments_path();
        let mut file_anchors: std::collections::HashMap<String, Vec<(usize, usize)>> =
            std::collections::HashMap::new();
        let mut old_file_anchors: std::collections::HashMap<String, Vec<(usize, usize)>> =
            std::collections::HashMap::new();
        for f in &tab.files {
            let mut new_ranges = Vec::new();
            let mut old_ranges = Vec::new();
            for h in &f.hunks {
                let new_start = h.new_start;
                let new_end = h.new_start + h.new_count.max(1);
                new_ranges.push((new_start, new_end));
                let old_start = h.old_start;
                let old_end = h.old_start + h.old_count.max(1);
                old_ranges.push((old_start, old_end));
            }
            file_anchors.insert(f.path.clone(), new_ranges);
            old_file_anchors.insert(f.path.clone(), old_ranges);
        }

        let (owner, repo_name, pr_number) = if is_remote {
            if let (Some(ref slug), Some(n)) = (&tab.remote_repo, tab.pr_number) {
                let mut parts = slug.split('/');
                let o = parts.next().unwrap_or("").to_string();
                let r = parts.next().unwrap_or("").to_string();
                (o, r, n)
            } else {
                return Err("No PR info for this tab".to_string());
            }
        } else {
            // Prefer the tab's explicit PR number (set when a local PR review was
            // opened) over branch detection. `get_pr_info` alone re-runs `gh pr
            // view` for the current branch and bails when no PR is detected, even
            // though the tab already knows which PR these comments belong to —
            // which is why individual pushes (which use `local_pr_target`) work
            // but the batch review submit failed with "No PR associated with
            // current branch".
            let pr_info = er_engine::sync::local_pr_target(&repo_root, tab.pr_number)
                .map_err(|e| e.to_string())?;
            (pr_info.0, pr_info.1, pr_info.2)
        };
        if event == "APPROVE" {
            let me = state.gh_user.lock().ok().and_then(|login| login.clone());
            if let (Some(me), Some(author)) = (
                me,
                active_pr_author(&app, &state, &owner, &repo_name, pr_number),
            ) {
                if author.eq_ignore_ascii_case(&me) {
                    return Err(own_pr_approval_error());
                }
            }
        }
        (
            owner,
            repo_name,
            pr_number,
            is_remote,
            repo_root,
            comments_path,
            file_anchors,
            old_file_anchors,
        )
    };

    // Collect pending line comments into the batch format.
    let gc: ErGitHubComments = std::fs::read_to_string(&comments_path)
        .ok()
        .and_then(|s| serde_json::from_str::<ErGitHubComments>(&s).ok())
        .unwrap_or(ErGitHubComments {
            version: 1,
            diff_hash: String::new(),
            github: None,
            comments: vec![],
        });

    // Reject early if any unsynced local comment has no line anchor — those can
    // never be part of a GitHub review batch and would silently get marked synced
    // without actually being sent.
    let unsubmittable_count = gc
        .comments
        .iter()
        .filter(|c| {
            c.source == "local" && !c.synced && !c.file.is_empty() && c.line_start.is_none()
        })
        .count();
    if unsubmittable_count > 0 {
        return Err(format!(
            "{unsubmittable_count} pending comment(s) have no line anchor and cannot be included \
             in a batch GitHub review. Add them on a specific diff line or delete them first."
        ));
    }

    struct BatchEntry {
        id: String,
        file: String,
        line_start: usize,
        line_end: Option<usize>,
        old_line: Option<usize>,
        body: String,
        side: String,
    }

    // Only top-level line comments belong in a review batch — replies use the reply API.
    let candidates: Vec<BatchEntry> = gc
        .comments
        .iter()
        .filter(|c| {
            c.source == "local"
                && !c.synced
                && !c.file.is_empty()
                && c.in_reply_to.is_none()
                && c.anchor_status != "lost"
                && !c.outdated
        })
        .filter_map(|c| {
            c.line_start.map(|start| {
                let end = c.line_end.filter(|e| *e > start).unwrap_or(start);
                BatchEntry {
                    id: c.id.clone(),
                    file: c.file.clone(),
                    line_start: start,
                    line_end: if end > start { Some(end) } else { None },
                    old_line: c.old_line_start,
                    body: c.comment.clone(),
                    side: c.side.clone(),
                }
            })
        })
        .collect();

    // Partition into valid (anchor in current diff) and stale.
    // LEFT-side comments (deleted lines) validate against old-side hunk ranges;
    // RIGHT-side comments validate against new-side ranges.
    let mut invalid_anchors: Vec<(String, usize, String)> = Vec::new();
    let mut batch_entries: Vec<BatchEntry> = Vec::new();
    for e in candidates {
        let end = e.line_end.unwrap_or(e.line_start);
        let in_diff = if e.side == "LEFT" {
            let start = e.old_line.unwrap_or(e.line_start);
            let old_end = e
                .line_end
                .and_then(|le| e.old_line.map(|ol| ol + (le - e.line_start)))
                .unwrap_or(start);
            anchor_range_in_current_diff(&old_file_anchors, &e.file, start, old_end)
        } else {
            anchor_range_in_current_diff(&file_anchors, &e.file, e.line_start, end)
        };
        if in_diff {
            batch_entries.push(e);
        } else {
            invalid_anchors.push((e.id, e.line_start, e.file));
        }
    }

    let batch: Vec<er_engine::github::ReviewBatchEntry> = batch_entries
        .iter()
        .map(|e| {
            let end = e.line_end.unwrap_or(e.line_start);
            let (line, start_line) = if e.side == "LEFT" {
                let start = e.old_line.unwrap_or(e.line_start);
                let old_end = e
                    .line_end
                    .and_then(|le| e.old_line.map(|ol| ol + (le - e.line_start)))
                    .unwrap_or(start);
                (old_end, if old_end > start { Some(start) } else { None })
            } else {
                (
                    end,
                    if end > e.line_start {
                        Some(e.line_start)
                    } else {
                        None
                    },
                )
            };
            er_engine::github::ReviewBatchEntry {
                file: e.file.clone(),
                line,
                start_line,
                body: e.body.clone(),
                side: e.side.clone(),
            }
        })
        .collect();
    let submitted_ids: Vec<String> = batch_entries.iter().map(|e| e.id.clone()).collect();

    let summary_trimmed = summary.trim().to_string();
    validate_review_submission(batch.len(), &summary_trimmed)?;
    if !invalid_anchors.is_empty() {
        let sample = invalid_anchors
            .iter()
            .take(3)
            .map(|(id, line, file)| format!("{id} ({file}:{line}) — stale line anchor"))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!(
            "Some local comments reference lines/files not present in the current diff and cannot be submitted. Resolve/delete or re-anchor them first. Examples: {sample}"
        ));
    }

    let submit_review = |comments: &[github::ReviewBatchEntry]| -> Result<(), anyhow::Error> {
        if is_remote {
            github::gh_pr_submit_review_remote(
                &owner,
                &repo_name,
                pr_number,
                comments,
                event,
                &summary_trimmed,
            )
        } else {
            github::gh_pr_submit_review(
                &owner,
                &repo_name,
                pr_number,
                comments,
                &repo_root,
                event,
                &summary_trimmed,
            )
        }
    };

    let mut decision_only_fallback = false;
    match submit_review(&batch) {
        Ok(()) => {}
        Err(e)
            if !batch.is_empty()
                && is_gh_review_422(&e)
                && (event == "APPROVE" || event == "REQUEST_CHANGES") =>
        {
            submit_review(&[]).map_err(gh_review_submit_err)?;
            decision_only_fallback = true;
        }
        Err(e) => return Err(gh_review_submit_err(e)),
    }

    let mut gc_to_write = gc;
    if decision_only_fallback {
        let skipped = batch.len();
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        app.notify(&format!(
            "{event} submitted, but {skipped} inline comment(s) could not be bundled (stale vs PR head). Refresh the diff, then push them individually."
        ));
    } else if !submitted_ids.is_empty() {
        let submitted: std::collections::HashSet<String> = submitted_ids.into_iter().collect();
        let mut touched = false;
        for c in &mut gc_to_write.comments {
            if submitted.contains(&c.id) {
                c.synced = true;
                touched = true;
            }
        }
        if touched {
            write_json_atomic(&comments_path, &gc_to_write)?;
        }
    }

    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    app.tab_mut().reload_ai_state();
    Ok(snap_from(&app, &state))
}

/// Submit a bare PR review decision (APPROVE / REQUEST_CHANGES / COMMENT) from
/// the GitHub card. Unlike `submit_github_review`, this **does not** bundle any
/// pending line-anchored comments — it sends only the body + event. This avoids
/// HTTP 422s when local drafts have stale line anchors vs the remote PR head
/// (the GitHub card is for decisions, not for pushing inline drafts — that's
/// what the Comments card is for).
#[tauri::command]
pub fn submit_github_pr_decision(
    mode: String,
    summary: String,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    use er_engine::github;

    let event = match mode.as_str() {
        "APPROVE" | "REQUEST_CHANGES" | "COMMENT" => mode.as_str(),
        _ => {
            return Err(format!(
                "Invalid review mode: {mode}. Use COMMENT, APPROVE, or REQUEST_CHANGES."
            ))
        }
    };

    let summary_trimmed = summary.trim().to_string();
    // GitHub itself rejects REQUEST_CHANGES and COMMENT reviews with a blank body.
    // APPROVE is fine without a body.
    if event != "APPROVE" && summary_trimmed.is_empty() {
        return Err(format!(
            "GitHub requires a comment for {event} reviews. Add a summary first."
        ));
    }

    let (owner, repo, number, is_remote, repo_root) = {
        let app = state.app.lock().map_err(|e| e.to_string())?;
        let tab = app.tab();
        let is_remote = tab.is_remote();
        let repo_root = tab.repo_root.clone();
        let (owner, repo, number) = active_github_key(&app, &state)
            .ok_or_else(|| "No GitHub PR detected for the active tab".to_string())?;
        if event == "APPROVE" {
            let me = state.gh_user.lock().ok().and_then(|login| login.clone());
            if let (Some(me), Some(author)) =
                (me, active_pr_author(&app, &state, &owner, &repo, number))
            {
                if author.eq_ignore_ascii_case(&me) {
                    return Err(own_pr_approval_error());
                }
            }
        }
        (owner, repo, number, is_remote, repo_root)
    };

    let submit_result = if is_remote {
        github::gh_pr_submit_review_remote(&owner, &repo, number, &[], event, &summary_trimmed)
    } else {
        github::gh_pr_submit_review(
            &owner,
            &repo,
            number,
            &[],
            &repo_root,
            event,
            &summary_trimmed,
        )
    };
    submit_result.map_err(gh_review_submit_err)?;

    kick_github_status_refresh(
        state.gh_status_cache.clone(),
        Arc::clone(&state.gh_status_in_flight),
        Arc::clone(&state.desktop_revision),
        Some(Arc::clone(&state.loading)),
        owner,
        repo,
        number,
    );

    let app = state.app.lock().map_err(|e| e.to_string())?;
    Ok(snap_from(&app, &state))
}

/// Post a PR-wide (issue-stream) comment on the active tab's PR. Used by the
/// GitHub card's "Comment / Review" action — distinct from line-anchored
/// review comments handled by `submit_github_review`.
#[tauri::command]
pub fn post_github_pr_comment(body: String, state: State<AppState>) -> Result<AppSnapshot, String> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return Err("Comment body cannot be empty".to_string());
    }

    let (owner, repo, number) = {
        let app = state.app.lock().map_err(|e| e.to_string())?;
        active_github_key(&app, &state)
            .ok_or_else(|| "No GitHub PR detected for the active tab".to_string())?
    };

    er_engine::github::gh_pr_general_comment_remote(&owner, &repo, number, trimmed)
        .map_err(|e| format!("Failed to post comment: {e}"))?;

    // Refresh the cached GitHub status so the comment count + recent list update.
    kick_github_status_refresh(
        state.gh_status_cache.clone(),
        Arc::clone(&state.gh_status_in_flight),
        Arc::clone(&state.desktop_revision),
        Some(Arc::clone(&state.loading)),
        owner,
        repo,
        number,
    );

    let app = state.app.lock().map_err(|e| e.to_string())?;
    Ok(snap_from(&app, &state))
}

// ── AI integration ───────────────────────────────────────────────────────────

fn resolve_review_scope(scope: &str, tab: &er_engine::app::TabState) -> Result<String, String> {
    let resolved = if scope == "current" {
        match tab.mode {
            DiffMode::Branch | DiffMode::Unstaged | DiffMode::Staged => {
                tab.mode.git_mode().to_string()
            }
            // Guide (tour) reviews the branch diff it was built from.
            DiffMode::PrDiff | DiffMode::Tour => "branch".to_string(),
            _ => {
                return Err(format!(
                    "AI review not available in {} view — switch to All changes, PR Diff, Unstaged, or Staged",
                    tab.mode.git_mode()
                ));
            }
        }
    } else if scope == "pr" {
        "branch".to_string()
    } else if matches!(scope, "branch" | "unstaged" | "staged") {
        if matches!(scope, "unstaged" | "staged") && tab.mode == DiffMode::PrDiff {
            return Err(
                "Unstaged/Staged review not available in PR Diff — switch to Unstaged or Staged view"
                    .to_string(),
            );
        }
        scope.to_string()
    } else {
        return Err(format!("Invalid review scope: {scope}"));
    };
    Ok(resolved)
}

#[tauri::command]
pub fn list_diff_paths(state: State<AppState>) -> Result<Vec<String>, String> {
    let app = state.app.lock().map_err(|e| e.to_string())?;
    let tab = app.tab();
    match tab.mode {
        DiffMode::Branch
        | DiffMode::Unstaged
        | DiffMode::Staged
        | DiffMode::PrDiff
        | DiffMode::Tour => {}
        _ => {
            return Err(format!(
                "File list not available in {} view",
                tab.mode.git_mode()
            ));
        }
    }
    Ok(tab
        .active_diff_files()
        .iter()
        .map(|f| f.path.clone())
        .collect())
}

#[tauri::command]
pub fn run_ai_review(scope: String, state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let scope = resolve_review_scope(&scope, app.tab())?;

    let (repo_root, branch_label, base_branch, er_dir, pr_number, remote_repo, is_remote) = {
        let tab = app.tab();
        let branch_label = tab
            .local_branch_view
            .clone()
            .unwrap_or_else(|| tab.current_branch.clone());
        (
            tab.repo_root.clone(),
            branch_label,
            tab.base_branch.clone(),
            tab.er_dir(),
            tab.pr_number,
            tab.remote_repo.clone(),
            tab.remote_repo.is_some(),
        )
    };

    std::fs::create_dir_all(&er_dir)
        .map_err(|e| format!("Failed to create branch managed directory: {e}"))?;

    let mut raw = app
        .tab()
        .raw_diff_for_review(&scope)
        .map_err(|e| e.to_string())?;
    let ignore = projects::review_ignore_globs_for_repo(&repo_root, remote_repo.as_deref());
    if !ignore.is_empty() {
        raw = er_engine::git::filter_raw_diff_exclude_globs(&raw, &ignore);
    }
    spawn_ai_review_with_diff(
        &mut app,
        &state,
        &scope,
        &er_dir,
        repo_root,
        branch_label,
        base_branch,
        pr_number,
        remote_repo,
        is_remote,
        raw,
    )?;
    Ok(snap_from(&app, &state))
}

#[allow(clippy::too_many_arguments)]
fn spawn_ai_review_with_diff(
    app: &mut er_engine::app::App,
    state: &AppState,
    scope: &str,
    er_dir: &str,
    repo_root: String,
    branch_label: String,
    base_branch: String,
    pr_number: Option<u64>,
    remote_repo: Option<String>,
    is_remote: bool,
    raw: String,
) -> Result<(), String> {
    if raw.trim().is_empty() {
        return Err("Nothing to review".to_string());
    }
    std::fs::write(std::path::Path::new(er_dir).join("diff-tmp"), &raw)
        .map_err(|e| format!("Failed to write diff-tmp: {e}"))?;

    let prompt = er_engine::ai::prompts::build_review_prompt_prepared_diff(
        scope,
        er_dir,
        &base_branch,
        &branch_label,
    );

    let target = er_engine::app::BackgroundTaskTarget {
        repo_root,
        er_dir: er_dir.to_string(),
        branch_label,
        base_branch,
        scope: scope.to_string(),
        pr_number,
        remote_repo,
        managed_local: !is_remote,
    };

    app.spawn_background_review(target, prompt, true)
        .map_err(|e| e.to_string())?;

    let debug_bg = er_engine::app::debug_bg_enabled();
    if debug_bg {
        eprintln!(
            "[bg] run_ai_review post-spawn snapshots={}",
            app.background_task_snapshots().len()
        );
    }

    state
        .desktop_revision
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if debug_bg {
        let snap = snap_from(app, state);
        eprintln!(
            "[bg] run_ai_review snapshot.background_tasks.len()={}",
            snap.background_tasks.len()
        );
    }
    Ok(())
}

/// Generate a guided Tour with AI: captures the active view's diff and spawns the
/// er-tour agent, which writes `tour.json` into that view's bucket. The PR Diff view
/// tours the PR head-vs-base diff (PR bucket); the Local branch / working-tree views
/// tour the branch diff (branch bucket). The mtime poll reloads it automatically on
/// completion, surfacing the Guide tab.
#[tauri::command]
pub fn generate_tour(state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    // `raw_diff_for_review("branch")` returns the active view's diff (the PR
    // head-vs-base diff in PrDiff mode), so only the destination bucket differs.
    let scope = "branch".to_string();

    let (
        repo_root,
        branch_label,
        base_branch,
        er_dir,
        pr_number,
        remote_repo,
        is_remote,
        tour_file,
        is_pr,
    ) = {
        let tab = app.tab();
        let branch_label = tab
            .local_branch_view
            .clone()
            .unwrap_or_else(|| tab.current_branch.clone());
        // Route to the active context's tour bucket (PR vs branch), matching where
        // `resolve_view_tour` reads — including a PR guide regenerated from the Guide tab.
        let er_dir = tab.tour_bucket_er_dir().unwrap_or_else(|| tab.er_dir());
        (
            tab.repo_root.clone(),
            branch_label,
            tab.base_branch.clone(),
            er_dir,
            tab.pr_number,
            tab.remote_repo.clone(),
            tab.remote_repo.is_some(),
            // Per-view buckets disambiguate, so the sidecar is always `tour.json`.
            "tour.json".to_string(),
            tab.tour_context_is_pr(),
        )
    };

    std::fs::create_dir_all(&er_dir)
        .map_err(|e| format!("Failed to create tour managed directory: {e}"))?;

    let mut raw = app
        .tab()
        .raw_diff_for_review(&scope)
        .map_err(|e| e.to_string())?;
    let ignore = projects::review_ignore_globs_for_repo(&repo_root, remote_repo.as_deref());
    if !ignore.is_empty() {
        raw = er_engine::git::filter_raw_diff_exclude_globs(&raw, &ignore);
    }
    if raw.trim().is_empty() {
        return Err("Nothing to tour".to_string());
    }
    std::fs::write(std::path::Path::new(&er_dir).join("diff-tmp"), &raw)
        .map_err(|e| format!("Failed to write diff-tmp: {e}"))?;

    let scope_label = if is_pr { "PR diff" } else { "branch diff" };
    let prompt =
        er_engine::ai::prompts::build_tour_prompt_prepared_diff(scope_label, &er_dir, &tour_file);
    let target = er_engine::app::BackgroundTaskTarget {
        repo_root,
        er_dir: er_dir.clone(),
        branch_label,
        base_branch,
        scope,
        pr_number,
        remote_repo,
        managed_local: !is_remote,
    };
    app.spawn_background_tour(target, prompt, true)
        .map_err(|e| e.to_string())?;
    state
        .desktop_revision
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Ok(snap_from(&app, &state))
}

pub use er_engine::ai::{ExpertInfo, ReviewerInfo};

#[tauri::command]
pub fn list_ai_experts() -> Vec<ExpertInfo> {
    er_engine::ai::list_expert_info()
}

#[tauri::command]
pub fn list_ai_reviewers() -> Vec<ReviewerInfo> {
    er_engine::ai::list_ai_reviewers()
}

#[tauri::command]
pub fn run_ai_expert_review(
    scope: String,
    expert_id: String,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    if er_engine::ai::expert_by_id(&expert_id).is_none() {
        return Err(format!("Unknown expert: {expert_id}"));
    }
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let scope = resolve_review_scope(&scope, app.tab())?;

    let (repo_root, branch_label, base_branch, er_dir, pr_number, remote_repo, is_remote) = {
        let tab = app.tab();
        let branch_label = tab
            .local_branch_view
            .clone()
            .unwrap_or_else(|| tab.current_branch.clone());
        (
            tab.repo_root.clone(),
            branch_label,
            tab.base_branch.clone(),
            tab.er_dir(),
            tab.pr_number,
            tab.remote_repo.clone(),
            tab.remote_repo.is_some(),
        )
    };

    std::fs::create_dir_all(&er_dir)
        .map_err(|e| format!("Failed to create branch managed directory: {e}"))?;

    let mut raw = app
        .tab()
        .raw_diff_for_review(&scope)
        .map_err(|e| e.to_string())?;
    if raw.trim().is_empty() {
        return Err("Nothing to review".to_string());
    }
    let ignore = projects::review_ignore_globs_for_repo(&repo_root, remote_repo.as_deref());
    if !ignore.is_empty() {
        raw = er_engine::git::filter_raw_diff_exclude_globs(&raw, &ignore);
    }
    std::fs::write(std::path::Path::new(&er_dir).join("diff-tmp"), &raw)
        .map_err(|e| format!("Failed to write diff-tmp: {e}"))?;

    let prompt = er_engine::ai::prompts::build_expert_review_prompt_prepared_diff(
        &scope, &er_dir, &expert_id,
    );

    let target = er_engine::app::BackgroundTaskTarget {
        repo_root,
        er_dir: er_dir.clone(),
        branch_label,
        base_branch,
        scope: scope.to_string(),
        pr_number,
        remote_repo,
        managed_local: !is_remote,
    };

    app.spawn_background_expert_review(&expert_id, target, prompt, true)
        .map_err(|e| e.to_string())?;

    state
        .desktop_revision
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Ok(snap_from(&app, &state))
}

#[tauri::command]
pub fn run_ai_professor_review(
    scope: String,
    focus_prompt: Option<String>,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    run_ai_scoped_review(
        scope,
        vec![],
        vec!["professor".to_string()],
        focus_prompt,
        state,
    )
}

#[tauri::command]
pub fn run_ai_triage_review(scope: String, state: State<AppState>) -> Result<AppSnapshot, String> {
    run_ai_scoped_review(scope, vec![], vec!["triage".to_string()], None, state)
}

#[tauri::command]
pub fn run_ai_review_files(
    scope: String,
    paths: Vec<String>,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    run_ai_scoped_review(scope, paths, vec!["general".to_string()], None, state)
}

#[tauri::command]
pub fn run_ai_scoped_review(
    scope: String,
    paths: Vec<String>,
    reviewer_kinds: Vec<String>,
    focus_prompt: Option<String>,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    if reviewer_kinds.is_empty() {
        return Err("No reviewers selected".to_string());
    }

    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let scope = resolve_review_scope(&scope, app.tab())?;

    let (repo_root, branch_label, base_branch, er_dir, pr_number, remote_repo, is_remote) = {
        let tab = app.tab();
        let branch_label = tab
            .local_branch_view
            .clone()
            .unwrap_or_else(|| tab.current_branch.clone());
        (
            tab.repo_root.clone(),
            branch_label,
            tab.base_branch.clone(),
            tab.er_dir(),
            tab.pr_number,
            tab.remote_repo.clone(),
            tab.remote_repo.is_some(),
        )
    };

    std::fs::create_dir_all(&er_dir)
        .map_err(|e| format!("Failed to create branch managed directory: {e}"))?;

    let raw = app
        .tab()
        .raw_diff_for_review(&scope)
        .map_err(|e| e.to_string())?;
    if raw.trim().is_empty() {
        return Err("Nothing to review".to_string());
    }

    let scoped_files = !paths.is_empty();
    let file_count = paths.len();
    let diff_body = if scoped_files {
        let filtered = er_engine::git::filter_raw_diff_by_paths(&raw, &paths);
        if filtered.trim().is_empty() {
            return Err("No diff for selected files".to_string());
        }
        let mut sorted_paths = paths;
        sorted_paths.sort();
        let manifest = sorted_paths.join("\n");
        std::fs::write(
            std::path::Path::new(&er_dir).join("review-files.txt"),
            format!("{manifest}\n"),
        )
        .map_err(|e| format!("Failed to write review-files.txt: {e}"))?;
        filtered
    } else {
        // Full-diff multi-reviewer run: no file manifest.
        let _ = std::fs::remove_file(std::path::Path::new(&er_dir).join("review-files.txt"));
        let ignore = projects::review_ignore_globs_for_repo(&repo_root, remote_repo.as_deref());
        if ignore.is_empty() {
            raw
        } else {
            er_engine::git::filter_raw_diff_exclude_globs(&raw, &ignore)
        }
    };

    std::fs::write(std::path::Path::new(&er_dir).join("diff-tmp"), &diff_body)
        .map_err(|e| format!("Failed to write diff-tmp: {e}"))?;

    let target = er_engine::app::BackgroundTaskTarget {
        repo_root,
        er_dir: er_dir.clone(),
        branch_label,
        base_branch,
        scope: scope.to_string(),
        pr_number,
        remote_repo,
        managed_local: !is_remote,
    };

    let (started, skipped) = spawn_scoped_reviewers(
        &mut app,
        &scope,
        &er_dir,
        target,
        &reviewer_kinds,
        focus_prompt.as_deref(),
        scoped_files,
    )?;

    state
        .desktop_revision
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    if started.is_empty() && !skipped.is_empty() {
        return Err(format!(
            "All selected reviewers already running: {}",
            skipped.join(", ")
        ));
    }

    let file_note = if scoped_files {
        format!(
            " for {file_count} file{}",
            if file_count == 1 { "" } else { "s" }
        )
    } else {
        String::new()
    };

    let msg = if skipped.is_empty() {
        format!(
            "Started {} reviewer(s){file_note}: {}",
            started.len(),
            started.join(", ")
        )
    } else {
        format!(
            "Started {} reviewer(s){file_note}: {} (skipped: {})",
            started.len(),
            started.join(", "),
            skipped.join(", ")
        )
    };
    app.notify(&msg);

    Ok(snap_from(&app, &state))
}

fn spawn_scoped_reviewers(
    app: &mut er_engine::app::App,
    scope: &str,
    er_dir: &str,
    target: er_engine::app::BackgroundTaskTarget,
    reviewer_kinds: &[String],
    focus_prompt: Option<&str>,
    scoped_files: bool,
) -> Result<(Vec<String>, Vec<String>), String> {
    use er_engine::ai::{prompts, ReviewerKind};

    let mut started = Vec::new();
    let mut skipped = Vec::new();

    for kind in reviewer_kinds {
        let parsed = er_engine::ai::parse_reviewer_kind(kind)
            .ok_or_else(|| format!("Unknown reviewer: {kind}"))?;
        let label = er_engine::ai::list_ai_reviewers()
            .into_iter()
            .find(|r| r.kind == *kind)
            .map(|r| r.label)
            .unwrap_or_else(|| kind.clone());

        let spawn_result = match &parsed {
            ReviewerKind::Triage => {
                let prompt = prompts::build_triage_review_prompt_prepared_diff(scope, er_dir);
                app.spawn_background_triage_review(target.clone(), prompt, true)
            }
            ReviewerKind::General => {
                let mut prompt = prompts::build_review_prompt_prepared_diff(
                    scope,
                    er_dir,
                    &target.base_branch,
                    &target.branch_label,
                );
                if scoped_files {
                    prompt = prompts::append_file_scope_if_present(prompt, er_dir);
                }
                app.spawn_background_review(target.clone(), prompt, true)
            }
            ReviewerKind::Expert(id) => {
                let mut prompt =
                    prompts::build_expert_review_prompt_prepared_diff(scope, er_dir, id);
                if scoped_files {
                    prompt = prompts::append_file_scope_if_present(prompt, er_dir);
                }
                app.spawn_background_expert_review(id, target.clone(), prompt, true)
            }
            ReviewerKind::Professor => {
                let prompt = prompts::build_professor_review_prompt_prepared_diff(
                    scope,
                    er_dir,
                    focus_prompt,
                    scoped_files,
                );
                app.spawn_background_professor_review(target.clone(), prompt, true)
            }
        };

        match spawn_result {
            Ok(()) => started.push(label),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("already running") {
                    skipped.push(label);
                } else {
                    return Err(msg);
                }
            }
        }
    }

    Ok((started, skipped))
}

fn eligible_github_comment_count(tab: &er_engine::app::TabState) -> usize {
    if let Some(gc) = tab.ai.github_comments.as_ref() {
        return er_engine::ai::count_eligible_github_comments(gc);
    }
    let path = std::path::Path::new(&tab.er_dir()).join("github-comments.json");
    if let Ok(content) = std::fs::read_to_string(&path) {
        if let Ok(gc) = serde_json::from_str::<er_engine::ai::ErGitHubComments>(&content) {
            return er_engine::ai::count_eligible_github_comments(&gc);
        }
    }
    0
}

#[tauri::command]
pub fn run_ai_validate(scope: String, state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let scope = resolve_review_scope(&scope, app.tab())?;

    if app.tab().is_remote() {
        return Err("Validate review is local-only. Check out the PR locally first.".to_string());
    }

    let er_dir = app.tab().er_dir();
    let review_path = std::path::Path::new(&er_dir).join("review.json");
    let has_review = review_path.exists();
    let comment_count = eligible_github_comment_count(app.tab());
    if !has_review && comment_count == 0 {
        return Err("Nothing to validate. Run AI review or add GitHub comments first.".to_string());
    }

    let raw = app
        .tab()
        .raw_diff_for_review(&scope)
        .map_err(|e| e.to_string())?;
    if raw.trim().is_empty() {
        return Err("Nothing to validate".to_string());
    }
    std::fs::write(std::path::Path::new(&er_dir).join("diff-tmp"), &raw)
        .map_err(|e| format!("Failed to write diff-tmp: {e}"))?;

    app.tab_mut().relocate_all_comments();

    if has_review {
        let prompt = er_engine::ai::prompts::build_validate_prompt_prepared_diff(&scope, &er_dir);
        app.spawn_agent_prompt("validate", &prompt)
            .map_err(|e| e.to_string())?;
    }
    if comment_count > 0 {
        let prompt =
            er_engine::ai::prompts::build_validate_github_comments_prompt_prepared_diff(&er_dir);
        app.spawn_agent_prompt("validate-comments", &prompt)
            .map_err(|e| e.to_string())?;
    }

    let msg = match (has_review, comment_count) {
        (true, n) if n > 0 => format!("Validation started (review + {n} comments)"),
        (true, _) => "Validation started (review)".to_string(),
        (false, n) => format!("Validation started ({n} comments)"),
    };
    app.notify(&msg);

    state
        .desktop_revision
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Ok(snap_from(&app, &state))
}

#[tauri::command]
pub fn set_ai_model(model: String, state: State<AppState>) -> Result<AppSnapshot, String> {
    let app = state.app.lock().map_err(|e| e.to_string())?;

    let mut cfg = er_engine::config::load_global_config();
    cfg.agent.model = model;
    er_engine::config::save_config(&cfg).map_err(|e| e.to_string())?;

    Ok(snap_from(&app, &state))
}

// ── AI provider / model selection ───────────────────────────────────────────

#[derive(serde::Serialize)]
pub struct AiProviderInfo {
    pub id: String,
    pub label: String,
    pub models: Vec<AiModelInfo>,
    pub is_selected: bool,
}

#[derive(serde::Serialize)]
pub struct AiModelInfo {
    pub id: String,
    pub label: String,
    pub is_selected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_per_1k_in: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_per_1k_out: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_latency_ms: Option<u32>,
    pub effort_levels: Vec<String>,
}

#[tauri::command]
pub fn list_ai_providers(state: State<AppState>) -> Result<Vec<AiProviderInfo>, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    app.sync_config_from_active_tab();
    let hub = &app.config.ai_hub;
    // Palette / session highlight: use live current_* (not persisted defaults).
    let current_provider = app.current_ai_provider.as_deref();
    let current_model = app.current_ai_model.as_deref();
    let resolved_provider = hub.resolve_provider_id(current_provider);

    let providers = hub
        .providers
        .iter()
        .map(|(id, cfg)| {
            let resolved_model = if resolved_provider.as_deref() == Some(id.as_str()) {
                hub.resolve_model_id(id, current_model)
            } else {
                None
            };
            AiProviderInfo {
                id: id.clone(),
                label: cfg.display_name(id),
                is_selected: resolved_provider.as_deref() == Some(id.as_str()),
                models: cfg
                    .models
                    .iter()
                    .map(|m| AiModelInfo {
                        id: m.id.clone(),
                        label: m.display_name(),
                        is_selected: resolved_model.as_deref() == Some(m.id.as_str()),
                        description: m.description.clone(),
                        cost_per_1k_in: m.cost_per_1k_in,
                        cost_per_1k_out: m.cost_per_1k_out,
                        avg_latency_ms: m.avg_latency_ms,
                        effort_levels: m.effort_levels.clone(),
                    })
                    .collect(),
            }
        })
        .collect();

    Ok(providers)
}

#[tauri::command]
pub fn set_ai_selection(
    provider_id: String,
    model_id: Option<String>,
    persist: Option<bool>,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let persist = persist.unwrap_or(false);
    let agent = app.config.agent.clone();

    let selection = if persist {
        let selection = app
            .config
            .ai_hub
            .set_default_selection(&provider_id, model_id.as_deref(), &agent)
            .map_err(|e| e.to_string())?;
        er_engine::config::save_config(&app.config).map_err(|e| e.to_string())?;
        selection
    } else {
        // Session-only: keep current effort when the new model still supports it.
        let runtime_effort = app.current_ai_effort.clone();
        app.config
            .ai_hub
            .resolve_selection(
                &provider_id,
                model_id.as_deref(),
                &agent,
                runtime_effort.as_deref(),
            )
            .map_err(|e| e.to_string())?
    };
    app.current_ai_provider = selection.provider_id;
    app.current_ai_model = selection.model_id;
    app.current_ai_effort = selection.effort;

    state
        .desktop_revision
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Ok(snap_from(&app, &state))
}

#[tauri::command]
pub fn set_ai_effort(
    effort: Option<String>,
    persist: Option<bool>,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let persist = persist.unwrap_or(false);
    let default_selection = app
        .config
        .ai_hub
        .resolve_default_selection(&app.config.agent);

    let (provider_id, model_id) = if persist {
        // Settings: normalize against persisted defaults, not a session palette pick.
        (default_selection.provider_id, default_selection.model_id)
    } else {
        (
            app.current_ai_provider
                .clone()
                .or(default_selection.provider_id),
            app.current_ai_model.clone().or(default_selection.model_id),
        )
    };

    let normalized = er_engine::config::normalize_effort(
        &app.config.ai_hub,
        provider_id.as_deref(),
        model_id.as_deref(),
        effort.as_deref(),
    );
    if effort.is_some() && normalized.is_none() {
        return Err("Effort is unsupported for the selected model".into());
    }

    if persist {
        app.config.ai_hub.default_effort = normalized.clone();
        // Keep session aligned with the defaults being edited in Settings.
        app.current_ai_provider = provider_id;
        app.current_ai_model = model_id;
        app.current_ai_effort = normalized;
        er_engine::config::save_config(&app.config).map_err(|e| e.to_string())?;
    } else {
        app.current_ai_effort = normalized;
    }

    state
        .desktop_revision
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Ok(snap_from(&app, &state))
}

// ── Promote question to GitHub comment ──────────────────────────────────────

/// Compose a markdown body that includes the question's text plus any replies
/// rendered as quoted blocks. Mirrors what the user sees in the inline thread.
fn build_promoted_body(root_text: &str, replies: &[(&str, &str)]) -> String {
    let mut out = root_text.trim().to_string();
    for (author, body) in replies {
        out.push_str("\n\n");
        let quoted: String = body
            .lines()
            .map(|l| format!("> {l}"))
            .collect::<Vec<_>>()
            .join("\n");
        out.push_str(&format!("> **{author}** replied:\n{quoted}"));
    }
    out
}

#[tauri::command]
pub fn promote_to_comment(
    id: String,
    body: Option<String>,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;

    // 1. Resolve the source question or note + already-promoted guard.
    let (file, hunk_idx, line_start, default_body) = {
        let tab = app.tab();
        // Both questions and notes use the `ReviewQuestion` shape; pick the
        // collection by id prefix so notes can be promoted too.
        let (items, item): (
            &[er_engine::ai::ReviewQuestion],
            &er_engine::ai::ReviewQuestion,
        ) = if id.starts_with("n-") {
            let ns = tab
                .ai
                .notes
                .as_ref()
                .ok_or_else(|| "No notes loaded".to_string())?;
            let n = ns
                .notes
                .iter()
                .find(|n| n.id == id)
                .ok_or_else(|| format!("Note not found: {id}"))?;
            (ns.notes.as_slice(), n)
        } else {
            let qs = tab
                .ai
                .questions
                .as_ref()
                .ok_or_else(|| "No questions loaded".to_string())?;
            let q = qs
                .questions
                .iter()
                .find(|q| q.id == id)
                .ok_or_else(|| format!("Question not found: {id}"))?;
            (qs.questions.as_slice(), q)
        };

        let replies: Vec<(&str, &str)> = items
            .iter()
            .filter(|r| r.in_reply_to.as_deref() == Some(&id))
            .map(|r| (r.author.as_str(), r.text.as_str()))
            .collect();
        let default = build_promoted_body(&item.text, &replies);

        (
            item.file.clone(),
            item.hunk_index.unwrap_or(0),
            item.line_start,
            default,
        )
    };

    let text = body.unwrap_or(default_body);

    // 2. Snapshot existing comment ids to detect the new one.
    let existing_ids: std::collections::HashSet<String> = {
        let tab = app.tab();
        tab.ai
            .github_comments
            .as_ref()
            .map(|gc| gc.comments.iter().map(|c| c.id.clone()).collect())
            .unwrap_or_default()
    };

    // 3. Create the new comment.
    app.submit_comment_text(
        file,
        hunk_idx,
        line_start,
        None,
        text,
        CommentType::GitHubComment,
        None,
        None,
    )
    .map_err(|e| e.to_string())?;

    // 4. Find the new comment id (anything not in the pre-existing set).
    let new_id: Option<String> = {
        let tab = app.tab();
        tab.ai.github_comments.as_ref().and_then(|gc| {
            gc.comments
                .iter()
                .find(|c| !existing_ids.contains(&c.id))
                .map(|c| c.id.clone())
        })
    };

    // 5. Remove the source question thread (replaced by the new GitHub comment).
    if new_id.is_some() {
        app.delete_comment_direct(&id).map_err(|e| e.to_string())?;
    }

    Ok(snap_from(&app, &state))
}

/// Promote a question to a local note. Notes are still private but framed as
/// actionable hand-offs to a coding agent. Mirrors `promote_to_comment` but the
/// target is `notes.json`. The source question (and its replies) is removed.
#[tauri::command]
pub fn promote_to_note(
    id: String,
    body: Option<String>,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;

    let (file, hunk_idx, line_start, default_body) = {
        let tab = app.tab();
        let qs = tab
            .ai
            .questions
            .as_ref()
            .ok_or_else(|| "No questions loaded".to_string())?;
        let q = qs
            .questions
            .iter()
            .find(|q| q.id == id)
            .ok_or_else(|| format!("Question not found: {id}"))?;
        let replies: Vec<(&str, &str)> = qs
            .questions
            .iter()
            .filter(|r| r.in_reply_to.as_deref() == Some(&id))
            .map(|r| (r.author.as_str(), r.text.as_str()))
            .collect();
        let default = build_promoted_body(&q.text, &replies);
        (
            q.file.clone(),
            q.hunk_index.unwrap_or(0),
            q.line_start,
            default,
        )
    };

    let text = body.unwrap_or(default_body);

    let existing_ids: std::collections::HashSet<String> = {
        let tab = app.tab();
        tab.ai
            .notes
            .as_ref()
            .map(|ns| ns.notes.iter().map(|n| n.id.clone()).collect())
            .unwrap_or_default()
    };

    app.submit_comment_text(
        file,
        hunk_idx,
        line_start,
        None,
        text,
        CommentType::Note,
        None,
        None,
    )
    .map_err(|e| e.to_string())?;

    let new_id: Option<String> = {
        let tab = app.tab();
        tab.ai.notes.as_ref().and_then(|ns| {
            ns.notes
                .iter()
                .find(|n| !existing_ids.contains(&n.id))
                .map(|n| n.id.clone())
        })
    };

    if new_id.is_some() {
        app.delete_comment_direct(&id).map_err(|e| e.to_string())?;
    }

    Ok(snap_from(&app, &state))
}

// ── Finding promotions sidecar ───────────────────────────────────────────────
// Map of `finding_id -> comment_id`. Stored alongside `.er/review.json` so
// `er` does not have to write into AI-owned files.

fn finding_promotions_path(er_dir: &str) -> String {
    format!("{er_dir}/finding-promotions.json")
}

pub(crate) fn load_finding_promotions(er_dir: &str) -> std::collections::HashMap<String, String> {
    let path = finding_promotions_path(er_dir);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_finding_promotions(
    er_dir: &str,
    map: &std::collections::HashMap<String, String>,
) -> std::io::Result<()> {
    std::fs::create_dir_all(er_dir)?;
    let path = finding_promotions_path(er_dir);
    let tmp = format!("{path}.tmp");
    let json = serde_json::to_string_pretty(map).unwrap_or_else(|_| "{}".to_string());
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, &path)
}

// ── Ask AI ──────────────────────────────────────────────────────────────────

/// Validate a comment, question, or finding with AI — adds a local reply on the card
/// (finding responses in review.json; thread replies in sidecars).
#[tauri::command]
pub fn validate_with_ai(
    thread_id: Option<String>,
    finding_id: Option<String>,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let resolved_finding = {
        let app = state.app.lock().map_err(|e| e.to_string())?;
        finding_id.or_else(|| {
            thread_id
                .as_ref()
                .and_then(|tid| finding_id_for_thread(app.tab(), tid))
        })
    };
    if let Some(fid) = resolved_finding {
        return ask_ai_for_finding(fid, VALIDATE_CARD_AI_PROMPT.to_string(), state);
    }
    let resolved_thread =
        thread_id.ok_or_else(|| "thread_id or finding_id is required".to_string())?;
    ask_ai(resolved_thread, VALIDATE_CARD_AI_PROMPT.to_string(), state)
}

/// Ask AI to elaborate on / answer a question thread — adds a local reply.
/// This is the question-flavored counterpart to `validate_with_ai`: questions
/// get "Elaborate" (investigate + answer) rather than "Validate" (confirm/refute).
#[tauri::command]
pub fn elaborate_with_ai(thread_id: String, state: State<AppState>) -> Result<AppSnapshot, String> {
    ask_ai(thread_id, ELABORATE_CARD_AI_PROMPT.to_string(), state)
}

/// Invoke the configured AI agent (`claude` CLI by default) on a question or
/// comment thread. The subprocess runs on a background thread so this command
/// returns immediately — the reply is added asynchronously and picked up by
/// the next snapshot poll. While the subprocess is running a synthetic
/// "…thinking" reply is rendered (see `pending_ai_replies` in `AppState`).
#[tauri::command]
pub fn ask_ai(
    thread_id: String,
    prompt: String,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    app.sync_ai_selection();

    let (
        file,
        hunk_idx,
        line_num,
        comment_type,
        thread_body,
        repo_root,
        base_branch,
        current_branch,
        work_dir,
        files,
        raw_diff,
        hunk_index,
        line_content,
        finding_title,
        finding_description,
        provider_id,
        model_id,
        agent_model_fallback,
    ) = {
        let tab = app.tab();
        let scope = match tab.mode {
            DiffMode::Branch | DiffMode::Unstaged | DiffMode::Staged => {
                tab.mode.git_mode().to_string()
            }
            _ => "branch".to_string(),
        };
        let raw_diff = tab.raw_diff_for_review(&scope).ok();
        let work_dir = if tab.is_remote() {
            tab.er_dir()
        } else {
            tab.repo_root.clone()
        };

        if thread_id.starts_with("q-") || thread_id.starts_with("n-") {
            // Questions and notes share the `ReviewQuestion` shape; pick the
            // collection (and resulting CommentType) by id prefix.
            let is_note = thread_id.starts_with("n-");
            let items: &[er_engine::ai::ReviewQuestion] = if is_note {
                tab.ai
                    .notes
                    .as_ref()
                    .map(|ns| ns.notes.as_slice())
                    .unwrap_or(&[])
            } else {
                tab.ai
                    .questions
                    .as_ref()
                    .map(|qs| qs.questions.as_slice())
                    .unwrap_or(&[])
            };
            let q = items
                .iter()
                .find(|q| q.id == thread_id)
                .ok_or(if is_note {
                    "Note not found"
                } else {
                    "Question not found"
                })
                .map_err(|e| e.to_string())?;
            let mut thread_body = String::new();
            thread_body.push_str(&format!("{}:{}\n", q.file, q.line_start.unwrap_or(0)));
            thread_body.push_str(&q.text);
            for r in items
                .iter()
                .filter(|r| r.in_reply_to.as_deref() == Some(thread_id.as_str()))
            {
                thread_body.push_str(&format!("\n\n**{}** replied:\n{}", r.author, r.text));
            }
            let (finding_title, finding_description) =
                finding_fields_for_ref(tab, q.finding_ref.as_deref());
            let line_content = if q.line_content.trim().is_empty() {
                None
            } else {
                Some(q.line_content.as_str())
            };
            (
                q.file.clone(),
                q.hunk_index.unwrap_or(0),
                q.line_start,
                if is_note {
                    CommentType::Note
                } else {
                    CommentType::Question
                },
                thread_body,
                tab.repo_root.clone(),
                tab.base_branch.clone(),
                tab.current_branch.clone(),
                work_dir,
                tab.files.clone(),
                raw_diff,
                q.hunk_index.unwrap_or(0),
                line_content,
                finding_title,
                finding_description,
                app.current_ai_provider.clone(),
                app.current_ai_model.clone(),
                app.config.agent.model.clone(),
            )
        } else {
            let c = tab
                .ai
                .github_comments
                .as_ref()
                .and_then(|gc| gc.comments.iter().find(|c| c.id == thread_id))
                .ok_or_else(|| "Comment not found".to_string())?;
            let mut thread_body = String::new();
            thread_body.push_str(&format!("{}:{}\n", c.file, c.line_start.unwrap_or(0)));
            thread_body.push_str(&c.comment);
            if let Some(gc) = &tab.ai.github_comments {
                for r in gc
                    .comments
                    .iter()
                    .filter(|r| r.in_reply_to.as_deref() == Some(thread_id.as_str()))
                {
                    thread_body.push_str(&format!("\n\n**{}** replied:\n{}", r.author, r.comment));
                }
            }
            let (finding_title, finding_description) =
                finding_fields_for_ref(tab, c.finding_ref.as_deref());
            let line_content = if c.line_content.trim().is_empty() {
                None
            } else {
                Some(c.line_content.as_str())
            };
            (
                c.file.clone(),
                c.hunk_index.unwrap_or(0),
                c.line_start,
                CommentType::GitHubComment,
                thread_body,
                tab.repo_root.clone(),
                tab.base_branch.clone(),
                tab.current_branch.clone(),
                work_dir,
                tab.files.clone(),
                raw_diff,
                c.hunk_index.unwrap_or(0),
                line_content,
                finding_title,
                finding_description,
                app.current_ai_provider.clone(),
                app.current_ai_model.clone(),
                app.config.agent.model.clone(),
            )
        }
    };

    let system_context = build_card_ai_system_context(&CardAiContextParams {
        repo_root: &repo_root,
        base_branch: &base_branch,
        current_branch: &current_branch,
        files: &files,
        raw_diff: raw_diff.as_deref(),
        file: &file,
        hunk_index,
        line_start: line_num,
        line_content,
        thread_body: &thread_body,
        finding_title: finding_title.as_deref(),
        finding_description: finding_description.as_deref(),
    });

    let cfg = er_engine::config::load_global_config();
    let invocation = plan_card_ai_invocation(
        &cfg,
        provider_id.as_deref(),
        model_id.as_deref(),
        app.current_ai_effort.as_deref(),
        work_dir,
    );
    let model_for_subprocess: Option<String> = model_id.or_else(|| {
        if agent_model_fallback.trim().is_empty() {
            None
        } else {
            Some(agent_model_fallback)
        }
    });

    // Mark thread as pending BEFORE spawning. Snapshot reads this map to
    // inject the "…thinking" placeholder reply.
    let started_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    if let Ok(mut p) = state.pending_ai_replies.lock() {
        p.insert(thread_id.clone(), started_at);
    }
    state.desktop_revision.fetch_add(1, Ordering::Relaxed);

    let user_prompt = if prompt.trim().is_empty() {
        DEFAULT_ASK_AI_PROMPT.to_string()
    } else {
        prompt
    };
    let app_arc = Arc::clone(&state.app);
    let pending_arc = Arc::clone(&state.pending_ai_replies);
    let meta_cache = state.meta_cache.clone();
    let desktop_revision = Arc::clone(&state.desktop_revision);
    let thread_id_for_thread = thread_id.clone();
    let repo_root_for_thread = repo_root.clone();
    let inv_for_thread = invocation;
    let model_for_thread = model_for_subprocess;

    // Build snapshot before releasing locks (test path expects synchronous
    // visibility of the pending state).
    let snap = snap_from(&app, &state);

    // Release lock before spawning so the subprocess runs without holding the App mutex.
    drop(app);

    std::thread::spawn(move || {
        let body = run_card_ai_subprocess(
            &inv_for_thread,
            &system_context,
            &user_prompt,
            model_for_thread.as_deref(),
        );
        // Take App lock to submit the reply.
        if let Ok(mut app) = app_arc.lock() {
            let _ = app.submit_comment_text_as_author(
                file,
                hunk_idx,
                line_num,
                None,
                body,
                comment_type,
                Some(thread_id_for_thread.clone()),
                None,
                "ai".to_string(),
            );
        }
        if let Ok(mut p) = pending_arc.lock() {
            p.remove(&thread_id_for_thread);
        }
        crate::snapshot::refresh_meta_cache(&repo_root_for_thread, &meta_cache);
        desktop_revision.fetch_add(1, Ordering::Relaxed);
    });

    Ok(snap)
}

fn finding_id_for_thread(tab: &er_engine::app::TabState, thread_id: &str) -> Option<String> {
    if thread_id.starts_with("q-") {
        tab.ai
            .questions
            .as_ref()
            .and_then(|qs| qs.questions.iter().find(|q| q.id == thread_id))
            .and_then(|q| q.finding_ref.clone())
    } else if thread_id.starts_with("n-") {
        tab.ai
            .notes
            .as_ref()
            .and_then(|ns| ns.notes.iter().find(|n| n.id == thread_id))
            .and_then(|n| n.finding_ref.clone())
    } else {
        tab.ai
            .github_comments
            .as_ref()
            .and_then(|gc| gc.comments.iter().find(|c| c.id == thread_id))
            .and_then(|c| c.finding_ref.clone())
    }
}

fn lookup_finding_fields(
    tab: &er_engine::app::TabState,
    finding_id: &str,
) -> Result<(String, usize, Option<usize>, String, String), String> {
    let review = tab
        .ai
        .review
        .as_ref()
        .ok_or_else(|| "No review loaded".to_string())?;
    for (path, fr) in review.files.iter() {
        for f in fr.findings.iter() {
            if f.id == finding_id {
                return Ok((
                    path.clone(),
                    f.hunk_index.unwrap_or(0),
                    f.line_start,
                    f.title.clone(),
                    f.description.clone(),
                ));
            }
        }
    }
    Err(format!("Finding not found: {finding_id}"))
}

/// Run AI validation on a finding; reply is stored in `Finding.responses`.
fn ask_ai_for_finding(
    finding_id: String,
    prompt: String,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    app.sync_ai_selection();

    let (
        file,
        _hunk_idx,
        line_num,
        finding_title,
        finding_description,
        repo_root,
        base_branch,
        current_branch,
        work_dir,
        files,
        raw_diff,
        hunk_index,
        line_content,
        provider_id,
        model_id,
        agent_model_fallback,
        er_dir,
    ) = {
        let tab = app.tab();
        let (file, hunk_idx, line_num, finding_title, finding_description) =
            lookup_finding_fields(tab, &finding_id)?;
        let scope = match tab.mode {
            DiffMode::Branch | DiffMode::Unstaged | DiffMode::Staged => {
                tab.mode.git_mode().to_string()
            }
            _ => "branch".to_string(),
        };
        let raw_diff = tab.raw_diff_for_review(&scope).ok();
        let work_dir = if tab.is_remote() {
            tab.er_dir()
        } else {
            tab.repo_root.clone()
        };
        let line_content = line_num.and_then(|ls| {
            tab.files
                .iter()
                .find(|f| f.path == file)
                .and_then(|df| df.hunks.get(hunk_idx))
                .and_then(|h| h.lines.iter().find(|l| l.new_num == Some(ls)))
                .map(|l| l.content.as_str())
        });
        (
            file,
            hunk_idx,
            line_num,
            finding_title,
            finding_description,
            tab.repo_root.clone(),
            tab.base_branch.clone(),
            tab.current_branch.clone(),
            work_dir,
            tab.files.clone(),
            raw_diff,
            hunk_idx,
            line_content,
            app.current_ai_provider.clone(),
            app.current_ai_model.clone(),
            app.config.agent.model.clone(),
            tab.er_dir(),
        )
    };

    let mut thread_body = format!("**Finding:** {finding_title}\n\n{finding_description}");
    if let Some(review) = app.tab().ai.review.as_ref() {
        for fr in review.files.values() {
            if let Some(f) = fr.findings.iter().find(|f| f.id == finding_id) {
                for r in &f.responses {
                    thread_body.push_str(&format!("\n\n**AI** replied:\n{}", r.text));
                }
                break;
            }
        }
    }

    let system_context = build_card_ai_system_context(&CardAiContextParams {
        repo_root: &repo_root,
        base_branch: &base_branch,
        current_branch: &current_branch,
        files: &files,
        raw_diff: raw_diff.as_deref(),
        file: &file,
        hunk_index,
        line_start: line_num,
        line_content,
        thread_body: &thread_body,
        finding_title: Some(finding_title.as_str()),
        finding_description: Some(finding_description.as_str()),
    });

    let cfg = er_engine::config::load_global_config();
    let invocation = plan_card_ai_invocation(
        &cfg,
        provider_id.as_deref(),
        model_id.as_deref(),
        app.current_ai_effort.as_deref(),
        work_dir,
    );
    let model_for_subprocess: Option<String> = model_id.or_else(|| {
        if agent_model_fallback.trim().is_empty() {
            None
        } else {
            Some(agent_model_fallback)
        }
    });

    let pending_key = format!("finding:{finding_id}");
    let started_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    if let Ok(mut p) = state.pending_ai_replies.lock() {
        p.insert(pending_key.clone(), started_at);
    }
    state.desktop_revision.fetch_add(1, Ordering::Relaxed);

    let user_prompt = if prompt.trim().is_empty() {
        DEFAULT_ASK_AI_PROMPT.to_string()
    } else {
        prompt
    };
    let app_arc = Arc::clone(&state.app);
    let pending_arc = Arc::clone(&state.pending_ai_replies);
    let meta_cache = state.meta_cache.clone();
    let desktop_revision = Arc::clone(&state.desktop_revision);
    let finding_id_for_thread = finding_id.clone();
    let er_dir_for_thread = er_dir.clone();
    let repo_root_for_thread = repo_root.clone();
    let inv_for_thread = invocation;
    let model_for_thread = model_for_subprocess;

    let snap = snap_from(&app, &state);
    drop(app);

    std::thread::spawn(move || {
        let body = run_card_ai_subprocess(
            &inv_for_thread,
            &system_context,
            &user_prompt,
            model_for_thread.as_deref(),
        );
        if let Ok(mut app) = app_arc.lock() {
            if let Err(e) = er_engine::ai::append_finding_response(
                &er_dir_for_thread,
                &finding_id_for_thread,
                &body,
            ) {
                app.notify(&format!("Failed to save finding validation: {e}"));
            } else {
                app.tab_mut().reload_ai_state();
            }
        }
        if let Ok(mut p) = pending_arc.lock() {
            p.remove(&format!("finding:{finding_id_for_thread}"));
        }
        crate::snapshot::refresh_meta_cache(&repo_root_for_thread, &meta_cache);
        desktop_revision.fetch_add(1, Ordering::Relaxed);
    });

    Ok(snap)
}

#[tauri::command]
pub fn update_finding_response(
    finding_id: String,
    response_id: String,
    body: String,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let er_dir = app.tab().er_dir();
    er_engine::ai::update_finding_response(&er_dir, &finding_id, &response_id, &body)
        .map_err(|e| e.to_string())?;
    app.tab_mut().reload_ai_state();
    Ok(snap_from(&app, &state))
}

#[tauri::command]
pub fn delete_finding_response(
    finding_id: String,
    response_id: String,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let er_dir = app.tab().er_dir();
    er_engine::ai::delete_finding_response(&er_dir, &finding_id, &response_id)
        .map_err(|e| e.to_string())?;
    app.tab_mut().reload_ai_state();
    Ok(snap_from(&app, &state))
}

fn finding_fields_for_ref(
    tab: &er_engine::app::TabState,
    finding_ref: Option<&str>,
) -> (Option<String>, Option<String>) {
    let Some(fid) = finding_ref else {
        return (None, None);
    };
    let Some(review) = tab.ai.review.as_ref() else {
        return (None, None);
    };
    for fr in review.files.values() {
        for f in &fr.findings {
            if f.id == fid {
                return (Some(f.title.clone()), Some(f.description.clone()));
            }
        }
    }
    (None, None)
}

// ── PR URL open ──────────────────────────────────────────────────────────────

/// Resolve the working-tree path where `branch` is checked out: the project
/// root (if `git rev-parse --abbrev-ref HEAD` == `branch`) or a linked
/// worktree. Returns `None` when the branch isn't checked out anywhere.
///
/// Shared by the PR-open path (`open_pr_review_impl`) and the desktop
/// active-branch watcher (`main.rs::desired_local_branch_watch`) so both
/// attach a checkout root using the same logic — making Saved/My PRs/Recent
/// behave like Tracked when the PR's head branch is checked out.
pub(crate) fn resolve_head_checkout(repo_root: &str, branch: &str) -> Option<String> {
    if branch.is_empty() {
        return None;
    }
    // Project root checkout?
    let head_out = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(repo_root)
        .output()
        .ok()?;
    if head_out.status.success() {
        let head = String::from_utf8_lossy(&head_out.stdout).trim().to_string();
        if head == branch {
            return Some(repo_root.to_string());
        }
    }
    // Linked worktree checkout?
    let worktrees = er_engine::git::list_worktrees(repo_root).ok()?;
    let wt = worktrees.into_iter().find(|w| w.branch == branch)?;
    Some(wt.path)
}

/// Place `tab` into the app: replace the active slot when `replace` is true
/// (Cmd-click / middle-click semantics), otherwise push a new tab.
pub(crate) fn place_tab(app: &mut App, tab: er_engine::app::TabState, replace: bool) {
    let mut tab = tab;
    if replace && !app.tabs.is_empty() {
        tab.sync_managed_storage();
        if let Some(msg) = tab.storage_notice.take() {
            app.notify(&msg);
        }
        let idx = app.active_tab.min(app.tabs.len() - 1);
        let name = tab.tab_name();
        app.tabs[idx] = tab;
        app.active_tab = idx;
        app.sync_config_from_active_tab();
        app.notify(&format!("Opened: {}", name));
    } else {
        app.open_tab(tab);
    }
    crate::tabs::persist_app_tabs(app);
    projects::sync_projects_from_tabs(&app.tabs);
}

/// Internal helper: open a remote PR view. If the same PR is already open,
/// just focus it. Otherwise place it via `replace` semantics.
fn do_open_remote_pr(
    app: &mut App,
    owner: &str,
    repo: &str,
    number: u64,
    replace: bool,
) -> Result<(), String> {
    let slug = format!("{owner}/{repo}");
    for (i, t) in app.tabs.iter().enumerate() {
        if t.remote_repo.as_deref() == Some(&slug) && t.pr_number == Some(number) {
            app.active_tab = i;
            return Ok(());
        }
    }
    let pr_ref = er_engine::github::PrRef {
        owner: owner.to_string(),
        repo: repo.to_string(),
        number,
    };
    let mut tab = er_engine::app::TabState::new_remote(&pr_ref).map_err(|e| e.to_string())?;
    let pr_data = er_engine::github::gh_pr_overview_remote(owner, repo, number);
    if let Some(data) = pr_data {
        tab.pr_data = Some(data);
    }
    tab.reload_remote_comments();
    place_tab(app, tab, replace);
    Ok(())
}

fn find_project_id_for_remote(file: &projects::ProjectsFile, remote_slug: &str) -> Option<String> {
    let target = normalize_remote_slug(remote_slug);
    file.projects.iter().find_map(|p| {
        if p.root_path.is_empty() {
            return None;
        }
        p.remote
            .as_ref()
            .filter(|r| normalize_remote_slug(r) == target)
            .map(|_| p.id.clone())
    })
}

fn now_epoch_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn fetch_single_pr_for_remote(remote: &str, pr_number: u64) -> Result<PrInfo, String> {
    #[derive(serde::Deserialize)]
    struct RawAuthor {
        login: Option<String>,
    }
    #[derive(serde::Deserialize)]
    struct RawLogin {
        login: String,
    }
    #[derive(serde::Deserialize)]
    struct RawReviewRequest {
        #[serde(default)]
        login: Option<String>,
    }
    #[derive(serde::Deserialize)]
    struct RawReview {
        author: RawAuthor,
        state: String,
    }
    #[derive(serde::Deserialize)]
    struct RawPr {
        number: u64,
        title: String,
        #[serde(rename = "headRefName")]
        head_ref_name: String,
        #[serde(default, rename = "baseRefName")]
        base_ref_name: String,
        #[serde(default, rename = "headRefOid")]
        head_ref_oid: String,
        #[serde(default, rename = "updatedAt")]
        updated_at: String,
        state: String,
        #[serde(rename = "isDraft")]
        is_draft: bool,
        author: RawAuthor,
        #[serde(default)]
        assignees: Vec<RawLogin>,
        #[serde(default, rename = "reviewRequests")]
        review_requests: Vec<RawReviewRequest>,
        #[serde(default, rename = "reviewDecision")]
        review_decision: Option<String>,
        #[serde(default, rename = "mergedAt")]
        merged_at: Option<String>,
        #[serde(default, rename = "latestReviews")]
        latest_reviews: Vec<RawReview>,
    }

    let out = std::process::Command::new("gh")
        .args([
            "pr",
            "view",
            &pr_number.to_string(),
            "--repo",
            remote,
            "--json",
            "number,title,headRefName,baseRefName,headRefOid,updatedAt,state,isDraft,author,assignees,reviewRequests,reviewDecision,mergedAt,latestReviews",
        ])
        .output()
        .map_err(|e| format!("Failed to run gh pr view for {remote}#{pr_number}: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!(
            "Failed to fetch PR #{pr_number} for {remote}: {}",
            stderr.trim()
        ));
    }
    let raw: RawPr = serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("Failed to parse gh pr view output for {remote}#{pr_number}: {e}"))?;
    let latest_reviewer_states = raw
        .latest_reviews
        .into_iter()
        .filter_map(|rv| rv.author.login.map(|l| (l, rv.state)))
        .collect();
    Ok(PrInfo {
        number: raw.number,
        title: raw.title,
        head_ref: raw.head_ref_name,
        state: raw.state,
        is_draft: raw.is_draft,
        author: raw.author.login.unwrap_or_default(),
        assignees: raw.assignees.into_iter().map(|a| a.login).collect(),
        reviewers: raw
            .review_requests
            .into_iter()
            .filter_map(|rr| rr.login)
            .collect(),
        checks_state: None,
        review_decision: raw.review_decision,
        merged_at: raw.merged_at,
        approved_by_me: false,
        base_ref: raw.base_ref_name,
        head_oid: raw.head_ref_oid,
        updated_at: raw.updated_at,
        latest_reviewer_states,
    })
}

fn cache_single_pr_for_remote(
    state: &AppState,
    remote: &str,
    pr_number: u64,
) -> Result<PrInfo, String> {
    let fetched_pr = fetch_single_pr_for_remote(remote, pr_number)?;
    if let Ok(mut cache) = state.pr_cache.lock() {
        let entry = cache.entry(remote.to_string()).or_default();
        if let Some(idx) = entry.iter().position(|pr| pr.number == pr_number) {
            entry[idx] = fetched_pr.clone();
        } else {
            entry.push(fetched_pr.clone());
        }
    }
    if let Ok(mut fetched) = state.pr_cache_fetched_at.lock() {
        fetched.insert(remote.to_string(), now_epoch_ms());
    }
    crate::pr_cache::save_persisted_pr_cache(&state.pr_cache, &state.pr_cache_fetched_at);
    Ok(fetched_pr)
}

fn record_remote_recent_pr(
    state: &AppState,
    remote: &str,
    pr_number: u64,
) -> Result<String, String> {
    let fetched_pr = cache_single_pr_for_remote(state, remote, pr_number)?;
    let project_id = projects::ensure_remote_project(remote).map_err(|e| e.to_string())?;
    projects::record_recent_pr(&project_id, pr_number, &fetched_pr.title)
        .map_err(|e| e.to_string())?;
    Ok(project_id)
}

#[tauri::command]
pub async fn open_remote_pr(
    owner: String,
    repo: String,
    number: u64,
    replace: Option<bool>,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || open_remote_pr_impl(owner, repo, number, replace, &state)).await
}

fn open_remote_pr_impl(
    owner: String,
    repo: String,
    number: u64,
    replace: Option<bool>,
    state: &AppState,
) -> Result<AppSnapshot, String> {
    let remote = format!("{owner}/{repo}");
    let _project_id = record_remote_recent_pr(state, &remote, number)?;
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    do_open_remote_pr(&mut app, &owner, &repo, number, replace.unwrap_or(false))?;
    kick_meta_refresh(state, app.tab().repo_root.clone());
    kick_github_status_refresh(
        state.gh_status_cache.clone(),
        Arc::clone(&state.gh_status_in_flight),
        Arc::clone(&state.desktop_revision),
        Some(Arc::clone(&state.loading)),
        owner,
        repo,
        number,
    );
    Ok(snap_from(&app, state))
}

#[tauri::command]
pub async fn open_pr_url(
    url: String,
    replace: Option<bool>,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || open_pr_url_impl(url, replace, &state)).await
}

fn open_pr_url_impl(
    url: String,
    replace: Option<bool>,
    state: &AppState,
) -> Result<AppSnapshot, String> {
    let pr_ref = er_engine::github::parse_github_pr_url(&url)
        .ok_or_else(|| format!("Not a valid GitHub PR URL: {url}"))?;
    let remote = format!("{}/{}", pr_ref.owner, pr_ref.repo);
    let file = projects::load();
    if let Some(project_id) = find_project_id_for_remote(&file, &remote) {
        let mut has_cached = false;
        if let Ok(cache) = state.pr_cache.lock() {
            has_cached = cache
                .get(&remote)
                .map(|prs| prs.iter().any(|pr| pr.number == pr_ref.number))
                .unwrap_or(false);
        }
        if !has_cached {
            cache_single_pr_for_remote(state, &remote, pr_ref.number)?;
        }
        projects::track_pr(&project_id, pr_ref.number).map_err(|e| e.to_string())?;
        return open_pr_review_impl(project_id, pr_ref.number, replace, None, state);
    }

    let _project_id = record_remote_recent_pr(state, &remote, pr_ref.number)?;
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    do_open_remote_pr(
        &mut app,
        &pr_ref.owner,
        &pr_ref.repo,
        pr_ref.number,
        replace.unwrap_or(false),
    )?;
    kick_meta_refresh(state, app.tab().repo_root.clone());
    kick_github_status_refresh(
        state.gh_status_cache.clone(),
        Arc::clone(&state.gh_status_in_flight),
        Arc::clone(&state.desktop_revision),
        Some(Arc::clone(&state.loading)),
        pr_ref.owner,
        pr_ref.repo,
        pr_ref.number,
    );
    Ok(snap_from(&app, state))
}

// ── Worktree picker (stub — no dialog dep) ──────────────────────────────────

#[tauri::command]
pub fn open_worktree(state: State<AppState>) -> Result<AppSnapshot, String> {
    let picked = rfd::FileDialog::new()
        .set_title("Select a git repository")
        .pick_folder();
    let Some(path) = picked else {
        // User cancelled — return current snapshot, no-op.
        return snap!(state);
    };
    let path_str = path.to_string_lossy().to_string();

    let mut new_tab = er_engine::app::TabState::new(path_str.clone())
        .map_err(|e| format!("Failed to open {path_str}: {e}"))?;
    new_tab
        .refresh_diff()
        .map_err(|e| format!("Failed to refresh {path_str}: {e}"))?;

    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    app.open_tab(new_tab);
    crate::tabs::persist_app_tabs(&app);
    let _ = projects::auto_register(&path_str);
    kick_meta_refresh(&state, app.tab().repo_root.clone());
    Ok(snap_from(&app, &state))
}

// ── Project commands ─────────────────────────────────────────────────────────

enum LocalBranchOpenPath {
    LocalFirst,
    LocalOnlyFallback,
}

fn build_local_branch_tab(
    project_id: &str,
    name: String,
) -> Result<(er_engine::app::TabState, LocalBranchOpenPath), String> {
    let branch_name = name.clone();
    let t_project = std::time::Instant::now();
    let file = projects::load();
    let proj = file
        .projects
        .iter()
        .find(|p| p.id == project_id)
        .ok_or_else(|| format!("Project not found: {project_id}"))?
        .clone();
    log_branch_open_phase(project_id, &branch_name, "project_lookup", t_project);

    let t_base = std::time::Instant::now();
    let base_branch =
        er_engine::git::detect_base_branch_in(&proj.root_path).map_err(|e| e.to_string())?;
    log_branch_open_phase(project_id, &branch_name, "base_detect", t_base);

    let t_tab_init = std::time::Instant::now();
    let mut new_tab =
        er_engine::app::TabState::new_with_base_unloaded(proj.root_path.clone(), base_branch)
            .map_err(|e| e.to_string())?;
    log_branch_open_phase(project_id, &branch_name, "tab_init", t_tab_init);

    new_tab.local_branch_view = Some(name);
    new_tab.mode = er_engine::app::DiffMode::Branch;
    new_tab.sync_managed_storage();
    let t_local_refresh = std::time::Instant::now();
    match new_tab.refresh_diff_without_remote_fetch_quick() {
        Ok(()) => {
            log_branch_open_phase(
                project_id,
                &branch_name,
                "local_first_refresh",
                t_local_refresh,
            );
            Ok((new_tab, LocalBranchOpenPath::LocalFirst))
        }
        Err(local_err) => {
            log::info!(
                "branch open local-first miss; falling back to local branch diff: {local_err}"
            );
            let t_local_fallback = std::time::Instant::now();
            new_tab.refresh_diff_quick().map_err(|e| e.to_string())?;
            log_branch_open_phase(
                project_id,
                &branch_name,
                "local_fallback_refresh",
                t_local_fallback,
            );
            Ok((new_tab, LocalBranchOpenPath::LocalOnlyFallback))
        }
    }
}

fn refresh_branch_open_diff(tab: &mut er_engine::app::TabState) -> Result<(), String> {
    match tab.refetch_and_refresh_diff() {
        Ok(()) => Ok(()),
        Err(err) if er_engine::github::is_no_upstream_to_refresh(&err) => {
            log::info!("branch open falling back to local diff: {err}");
            tab.refresh_diff().map_err(|e| e.to_string())
        }
        Err(err) => Err(format!("Failed to refresh branch from upstream: {err}")),
    }
}

fn kick_background_branch_refresh(
    app_state: Arc<Mutex<App>>,
    desktop_revision: Arc<AtomicU64>,
    repo_root: String,
    branch_name: String,
    base_branch: String,
) {
    std::thread::spawn(move || {
        // Fetch the base branch from origin so the local diff is up-to-date.
        let base_strip = base_branch.strip_prefix("origin/").unwrap_or(&base_branch);
        match er_engine::github::fetch_base_branch_ref(&repo_root, base_strip) {
            Ok(base_ref) => {
                let mut refreshed_active_tab = false;
                if let Ok(mut app) = app_state.lock() {
                    let active_tab = app.active_tab;
                    if let Some(tab) = app.tabs.get_mut(active_tab).filter(|tab| {
                        tab.repo_root == repo_root
                            && tab.local_branch_view.as_deref() == Some(branch_name.as_str())
                    }) {
                        tab.base_branch = base_ref;
                        if let Err(err) = tab.refresh_diff() {
                            log::warn!(
                                "background branch refresh diff failed for {branch_name}: {err}"
                            );
                        } else {
                            refreshed_active_tab = true;
                        }
                    }
                }
                if refreshed_active_tab {
                    desktop_revision.fetch_add(1, Ordering::Relaxed);
                }
            }
            Err(err) => {
                log::warn!("background branch base refresh failed for {branch_name}: {err}");
                refresh_active_branch_after_background_miss(
                    &app_state,
                    &desktop_revision,
                    &repo_root,
                    &branch_name,
                );
            }
        }
    });
}

fn refresh_active_branch_after_background_miss(
    app_state: &Arc<Mutex<App>>,
    desktop_revision: &Arc<AtomicU64>,
    repo_root: &str,
    branch_name: &str,
) {
    let mut refreshed_active_tab = false;
    if let Ok(mut app) = app_state.lock() {
        let active_tab = app.active_tab;
        if let Some(tab) = app.tabs.get_mut(active_tab).filter(|tab| {
            tab.repo_root == repo_root && tab.local_branch_view.as_deref() == Some(branch_name)
        }) {
            if let Err(err) = tab.refresh_diff() {
                log::warn!("background branch local full refresh failed for {branch_name}: {err}");
            } else {
                refreshed_active_tab = true;
            }
        }
    }
    if refreshed_active_tab {
        desktop_revision.fetch_add(1, Ordering::Relaxed);
    }
}

#[tauri::command]
pub async fn open_local_branch(
    project_id: String,
    name: String,
    replace: Option<bool>,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || open_local_branch_impl(project_id, name, replace, &state)).await
}

fn open_local_branch_impl(
    project_id: String,
    name: String,
    replace: Option<bool>,
    state: &AppState,
) -> Result<AppSnapshot, String> {
    let t_total = std::time::Instant::now();
    let branch_name = name.clone();
    let t_tab_build = std::time::Instant::now();
    let (new_tab, open_path) = build_local_branch_tab(&project_id, name)?;
    log_branch_open_phase(&project_id, &branch_name, "tab_build", t_tab_build);
    let t_app_lock = std::time::Instant::now();
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    log_branch_open_phase(&project_id, &branch_name, "app_lock", t_app_lock);
    let t_place_tab = std::time::Instant::now();
    place_tab(&mut app, new_tab, replace.unwrap_or(false));
    log_branch_open_phase(&project_id, &branch_name, "tab_place", t_place_tab);
    let open_path_label = match open_path {
        LocalBranchOpenPath::LocalFirst => "local_first",
        LocalBranchOpenPath::LocalOnlyFallback => "local_only_fallback",
    };
    log::info!(
        "branch_open project={} branch={} phase=initial_path mode={}",
        project_id,
        branch_name,
        open_path_label
    );
    kick_meta_refresh(state, app.tab().repo_root.clone());
    kick_active_gh_status(&app, state);
    let repo_root = app.tab().repo_root.clone();
    let base_branch = app.tab().base_branch.clone();
    let t_snapshot = std::time::Instant::now();
    let snapshot = snap_from(&app, state);
    log_branch_open_phase(&project_id, &branch_name, "snapshot_build", t_snapshot);
    log_branch_open_phase(&project_id, &branch_name, "total", t_total);
    drop(app);
    kick_background_branch_refresh(
        Arc::clone(&state.app),
        Arc::clone(&state.desktop_revision),
        repo_root,
        branch_name.clone(),
        base_branch,
    );
    Ok(snapshot)
}

fn pr_open_cache_key(project_id: &str, repo_root: &str, pr_number: u64) -> PrOpenCacheKey {
    PrOpenCacheKey {
        project_id: project_id.to_string(),
        repo_root: repo_root.to_string(),
        pr_number,
    }
}

const PR_COMMIT_CACHE_LIMIT: usize = 250;

fn cached_pr_open_diff(
    cache: &Arc<Mutex<HashMap<PrOpenCacheKey, PrOpenCacheEntry>>>,
    key: &PrOpenCacheKey,
    freshness: &PrOpenFreshness,
) -> Option<String> {
    let mut guard = cache.lock().ok()?;
    let entry = guard.get(key)?;
    if entry.freshness != *freshness {
        return None;
    }
    // Clone the payload before the mutable recency bump below (can't hold an
    // immutable borrow across `get_mut`).
    let raw_diff = entry.raw_diff.clone();
    // Bump recency so a read counts toward LRU ordering (a hit is a use).
    let tick = pr_open_clock();
    if let Some(e) = guard.get_mut(key) {
        e.last_touched = tick;
    }
    Some(raw_diff)
}

/// Cached PR-open entry: `(raw diff, freshness, optional overview, optional commits)`.
type CachedPrOpenEntry = (
    String,
    PrOpenFreshness,
    Option<er_engine::github::PrOverviewData>,
    Option<Vec<er_engine::git::CommitInfo>>,
);

/// Look up a cached open-diff for the hint path, treating it as a hit when the
/// **base branch** matches — head/`updated_at` drift is allowed (J1: render the
/// diff we already hold instantly; the 30s `pr_head_probe` lights the stale pill,
/// Sync refreshes). A base **retarget** is the one hard miss: the staleness pill
/// (`compute_oid_staleness`) watches only `head_oid`, so a silently re-based diff
/// would render with no warning — re-fetch instead.
///
/// Returns the cached diff together with the entry's **own** freshness (and cached
/// `pr_data`/`pr_commits` when present). The caller seeds the staleness probe with
/// this freshness's `head_oid` — the oid the diff was actually built against — so a
/// head that moved since lights the pill. Returning the newer *requested* oid would
/// suppress the pill and silently show a stale diff (the bug this fixes).
fn cached_pr_open_entry(
    cache: &Arc<Mutex<HashMap<PrOpenCacheKey, PrOpenCacheEntry>>>,
    key: &PrOpenCacheKey,
    requested: &PrOpenFreshness,
) -> Option<CachedPrOpenEntry> {
    let mut guard = cache.lock().ok()?;
    let entry = guard.get(key)?;
    if entry.freshness.base_branch != requested.base_branch {
        return None;
    }
    // Clone the payload before the mutable recency bump below (can't hold an
    // immutable borrow across `get_mut`).
    let out = (
        entry.raw_diff.clone(),
        entry.freshness.clone(),
        entry.pr_data.clone(),
        entry.pr_commits.clone(),
    );
    // Bump recency so a read counts toward LRU ordering (a hit is a use).
    let tick = pr_open_clock();
    if let Some(e) = guard.get_mut(key) {
        e.last_touched = tick;
    }
    Some(out)
}

/// Monotonic counter for `PrOpenCacheEntry::last_touched`. Strictly increasing
/// per process; only relative ordering matters for LRU eviction, never the
/// absolute value.
fn pr_open_clock() -> u64 {
    static CLOCK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    CLOCK.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// Per-process cap on the open-diff cache. Over this, the least-recently-touched
/// entry is evicted (LRU) so a hot PR survives a churn of one-off opens.
pub(crate) const MAX_PR_OPEN_CACHE_ENTRIES: usize = 32;

/// Evict the least-recently-touched entry when the map exceeds `MAX_PR_OPEN_CACHE_ENTRIES`.
/// Pure (operates on the borrowed map) so the LRU policy is unit-testable without a lock.
pub(crate) fn evict_lru(map: &mut HashMap<PrOpenCacheKey, PrOpenCacheEntry>) {
    if map.len() <= MAX_PR_OPEN_CACHE_ENTRIES {
        return;
    }
    // Remove the entry with the smallest `last_touched`. Touch times from
    // `pr_open_clock()` are process-unique and increasing, so there is a strict
    // minimum; the `HashMap` iteration order does not affect which key is chosen.
    let evict_key = map
        .iter()
        .min_by_key(|(_, e)| e.last_touched)
        .map(|(k, _)| k.clone());
    if let Some(key) = evict_key {
        map.remove(&key);
    }
}

fn remember_pr_open_entry(
    cache: &Arc<Mutex<HashMap<PrOpenCacheKey, PrOpenCacheEntry>>>,
    key: PrOpenCacheKey,
    freshness: PrOpenFreshness,
    raw_diff: String,
    pr_data: Option<er_engine::github::PrOverviewData>,
    pr_commits: Option<Vec<er_engine::git::CommitInfo>>,
) {
    if let Ok(mut guard) = cache.lock() {
        // Preserve existing metadata if the new entry doesn't bring it (lets a
        // hint-based prefetch keep data fetched by an earlier full open).
        let pr_data = pr_data.or_else(|| {
            guard
                .get(&key)
                .filter(|e| e.freshness == freshness)
                .and_then(|e| e.pr_data.clone())
        });
        let pr_commits = pr_commits.or_else(|| {
            guard
                .get(&key)
                .filter(|e| e.freshness == freshness)
                .and_then(|e| e.pr_commits.clone())
        });
        guard.insert(
            key,
            PrOpenCacheEntry {
                freshness,
                raw_diff,
                pr_data,
                pr_commits,
                last_touched: pr_open_clock(),
            },
        );
        evict_lru(&mut guard);
    }
    crate::pr_open_cache::save_persisted_pr_open_cache(cache);
}

fn run_gh_pr_view_for_open(repo_root: &str, pr_number: u64) -> Result<PrOpenMetadata, String> {
    #[derive(serde::Deserialize)]
    struct RawAuthor {
        login: Option<String>,
    }
    #[derive(serde::Deserialize)]
    struct RawReview {
        author: RawAuthor,
        state: String,
    }
    #[derive(serde::Deserialize)]
    struct RawView {
        number: u64,
        title: String,
        #[serde(default)]
        body: String,
        state: String,
        author: RawAuthor,
        url: String,
        #[serde(rename = "baseRefName")]
        base_ref_name: String,
        #[serde(rename = "headRefName")]
        head_ref_name: String,
        #[serde(default, rename = "headRefOid")]
        head_ref_oid: String,
        #[serde(default, rename = "updatedAt")]
        updated_at: String,
        #[serde(default)]
        reviews: Vec<RawReview>,
    }

    let out = std::process::Command::new("gh")
        .args([
            "pr",
            "view",
            &pr_number.to_string(),
            "--json",
            "number,title,body,state,author,url,baseRefName,headRefName,headRefOid,updatedAt,reviews,commits",
        ])
        .current_dir(repo_root)
        .output()
        .map_err(|e| format!("Failed to run gh pr view: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("Failed to get PR #{pr_number}: {}", stderr.trim()));
    }
    let raw: RawView = serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("Failed to parse gh pr view for PR #{pr_number}: {e}"))?;
    let pr_commits =
        er_engine::github::parse_pr_commits_view_json(&out.stdout, PR_COMMIT_CACHE_LIMIT)
            .unwrap_or_default();
    let reviewers = raw
        .reviews
        .into_iter()
        .filter_map(|review| {
            review
                .author
                .login
                .map(|login| er_engine::github::ReviewerStatus {
                    login,
                    state: review.state,
                })
        })
        .collect();
    let freshness = PrOpenFreshness {
        base_branch: raw.base_ref_name.clone(),
        head_branch: raw.head_ref_name.clone(),
        head_oid: raw.head_ref_oid,
        updated_at: raw.updated_at,
    };
    Ok(PrOpenMetadata {
        freshness,
        pr_data: er_engine::github::PrOverviewData {
            number: raw.number,
            title: raw.title,
            body: raw.body,
            state: raw.state,
            author: raw.author.login.unwrap_or_default(),
            url: raw.url,
            base_branch: raw.base_ref_name,
            head_branch: raw.head_ref_name,
            checks: Vec::new(),
            reviewers,
        },
        pr_commits,
    })
}

fn run_gh_pr_diff_for_open(repo_root: &str, pr_number: u64) -> Result<String, String> {
    er_engine::github::gh_pr_diff(pr_number, repo_root).map_err(|e| e.to_string())
}

fn run_gh_pr_commits_for_open(repo_root: &str, pr_number: u64) -> Vec<er_engine::git::CommitInfo> {
    er_engine::github::gh_pr_commits(repo_root, pr_number, PR_COMMIT_CACHE_LIMIT)
        .unwrap_or_default()
}

/// Build a minimal `PrOverviewData` from a sidebar hint. Used when opening a PR
/// without first fetching `gh pr view` — the panel renders immediately with the
/// fields the sidebar already had, and a background refresh fills in body/checks/reviews.
fn pr_overview_from_hint(
    hint: &PrOpenHint,
    pr_number: u64,
    repo_slug: Option<&str>,
) -> er_engine::github::PrOverviewData {
    let url = repo_slug
        .map(|slug| format!("https://github.com/{slug}/pull/{pr_number}"))
        .unwrap_or_default();
    er_engine::github::PrOverviewData {
        number: pr_number,
        title: hint.title.clone(),
        body: String::new(),
        state: "OPEN".to_string(),
        author: hint.author.clone(),
        url,
        base_branch: hint.base_ref.clone(),
        head_branch: hint.head_ref.clone(),
        checks: Vec::new(),
        reviewers: Vec::new(),
    }
}

fn freshness_from_hint(hint: &PrOpenHint) -> PrOpenFreshness {
    PrOpenFreshness {
        base_branch: hint.base_ref.clone(),
        head_branch: hint.head_ref.clone(),
        head_oid: hint.head_oid.clone(),
        updated_at: hint.updated_at.clone(),
    }
}

/// Build the freshness key from a freshly-fetched `PrInfo` (the same metadata the
/// sidebar carries). `sync_pr` uses this to persist the refreshed diff so the next
/// open is a cache hit. It must agree field-for-field with `freshness_from_hint`
/// (sidebar-hint open) and `run_gh_pr_view_for_open` (no-hint open) — otherwise a
/// freshness mismatch makes every post-sync open a (safe) miss. `diff_head_oid` is
/// the oid the cached diff was actually computed against (`tab.last_diff_head_oid`,
/// set in `refetch_and_refresh_diff`); it falls back to the PR head oid. Pinning the
/// diff's own oid keeps the post-Sync reopen a clean hit — the persisted oid matches
/// the diff it was built against, so the staleness pill stays dark until the head
/// actually moves again.
fn freshness_from_pr_info(pr: &PrInfo, diff_head_oid: Option<&str>) -> PrOpenFreshness {
    PrOpenFreshness {
        base_branch: pr.base_ref.clone(),
        head_branch: pr.head_ref.clone(),
        head_oid: diff_head_oid
            .filter(|oid| !oid.trim().is_empty())
            .map(|oid| oid.to_string())
            .unwrap_or_else(|| pr.head_oid.clone()),
        updated_at: pr.updated_at.clone(),
    }
}

/// Sidebar hints can omit fields (e.g. Recent entries resolved without pr_cache).
/// Incomplete hints must not skip `gh pr view` — that leaves `base_branch` empty.
fn pr_open_hint_is_complete(hint: &PrOpenHint) -> bool {
    !hint.base_ref.trim().is_empty()
        && !hint.head_ref.trim().is_empty()
        && !hint.head_oid.trim().is_empty()
}

fn load_pr_open_inputs(
    project_id: &str,
    pr_number: u64,
    hint: Option<&PrOpenHint>,
    state: &AppState,
) -> Result<PrOpenInputs, String> {
    let file = projects::load();
    let proj = file
        .projects
        .iter()
        .find(|p| p.id == project_id)
        .ok_or_else(|| format!("Project not found: {project_id}"))?
        .clone();
    let repo_root = proj.root_path;
    let repo_slug = proj.remote.clone();
    let branch_label = format!("pr-{}", pr_number);
    let key = pr_open_cache_key(project_id, &repo_root, pr_number);
    let hint = hint.filter(|h| pr_open_hint_is_complete(h));

    // ── Hint path: skip the `gh pr view` round-trip entirely ──
    if let Some(hint) = hint {
        let freshness = freshness_from_hint(hint);

        // Cache hit when the base branch matches (J1: head/`updated_at` drift is
        // allowed — render the diff we already hold instantly, the 30s stale pill
        // catches a moved head). Reuse the **entry's own** freshness, not the
        // hint's: it pins the oid the cached diff was built against, so seeding the
        // staleness probe with it lights the pill when the live head has advanced.
        // Backfill commits only if this older cache entry does not have them yet.
        if let Some((raw_diff, entry_freshness, cached_pr_data, cached_pr_commits)) =
            cached_pr_open_entry(&state.pr_open_cache, &key, &freshness)
        {
            log::info!(
                "branch_open project={} branch={} phase=gh_pr_diff ms=0 cache=hit_hint",
                project_id,
                branch_label
            );
            let t_base = std::time::Instant::now();
            let resolved_base = er_engine::github::ensure_base_ref_available(
                &repo_root,
                &entry_freshness.base_branch,
            )
            .map_err(|e| e.to_string())?;
            log_branch_open_phase(project_id, &branch_label, "base_ref_check", t_base);
            let pr_data = cached_pr_data
                .unwrap_or_else(|| pr_overview_from_hint(hint, pr_number, repo_slug.as_deref()));
            let pr_commits = cached_pr_commits
                .unwrap_or_else(|| run_gh_pr_commits_for_open(&repo_root, pr_number));
            // Re-write with the entry's own freshness so the diff↔oid pairing is
            // preserved; remember_pr_open_entry keeps existing pr_data because the
            // freshness still matches the stored entry.
            remember_pr_open_entry(
                &state.pr_open_cache,
                key,
                entry_freshness.clone(),
                raw_diff.clone(),
                None,
                Some(pr_commits.clone()),
            );
            return Ok(PrOpenInputs {
                repo_root,
                metadata: PrOpenMetadata {
                    freshness: entry_freshness,
                    pr_data,
                    pr_commits,
                },
                resolved_base,
                raw_diff,
                cache_hit: true,
            });
        }

        // Cache miss with hint: run `gh pr diff` and `ensure_base_ref_available`
        // in parallel. Skip `gh pr view` — overview is rendered from the hint
        // and a background refresh can fill in body/reviews later.
        let (diff_res, base_res, commits, diff_ms, base_ms) = std::thread::scope(|s| {
            let diff_root = repo_root.clone();
            let base_root = repo_root.clone();
            let commits_root = repo_root.clone();
            let base_branch = freshness.base_branch.clone();
            let diff_h = s.spawn(move || {
                let t = std::time::Instant::now();
                let res = run_gh_pr_diff_for_open(&diff_root, pr_number);
                (res, t.elapsed().as_millis())
            });
            let base_h = s.spawn(move || {
                let t = std::time::Instant::now();
                let res = er_engine::github::ensure_base_ref_available(&base_root, &base_branch)
                    .map_err(|e| e.to_string());
                (res, t.elapsed().as_millis())
            });
            let commits_h = s.spawn(move || run_gh_pr_commits_for_open(&commits_root, pr_number));
            let (diff_res, diff_ms) = diff_h
                .join()
                .unwrap_or_else(|_| (Err("gh pr diff thread panicked".to_string()), 0));
            let (base_res, base_ms) = base_h
                .join()
                .unwrap_or_else(|_| (Err("base ref fetch thread panicked".to_string()), 0));
            let commits = commits_h.join().unwrap_or_default();
            (diff_res, base_res, commits, diff_ms, base_ms)
        });
        log::info!(
            "branch_open project={} branch={} phase=gh_pr_diff ms={} cache=miss_hint",
            project_id,
            branch_label,
            diff_ms
        );
        log::info!(
            "branch_open project={} branch={} phase=base_ref_check ms={} parallel=true",
            project_id,
            branch_label,
            base_ms
        );
        let raw_diff = diff_res?;
        let resolved_base = base_res?;
        let pr_data = pr_overview_from_hint(hint, pr_number, repo_slug.as_deref());
        remember_pr_open_entry(
            &state.pr_open_cache,
            key,
            freshness.clone(),
            raw_diff.clone(),
            None, // intentionally None — pr_data is a placeholder; full view will fill it later
            Some(commits.clone()),
        );
        return Ok(PrOpenInputs {
            repo_root,
            metadata: PrOpenMetadata {
                freshness,
                pr_data,
                pr_commits: commits,
            },
            resolved_base,
            raw_diff,
            cache_hit: false,
        });
    }

    // ── No hint: original behavior (probe `gh pr view` for freshness) ──
    let has_cache_entry = state
        .pr_open_cache
        .lock()
        .ok()
        .map(|guard| guard.contains_key(&key))
        .unwrap_or(false);
    if has_cache_entry {
        let t_view = std::time::Instant::now();
        let metadata = run_gh_pr_view_for_open(&repo_root, pr_number)?;
        log::info!(
            "branch_open project={} branch={} phase=gh_pr_view ms={} cache=probe",
            project_id,
            branch_label,
            t_view.elapsed().as_millis()
        );
        if let Some(raw_diff) = cached_pr_open_diff(&state.pr_open_cache, &key, &metadata.freshness)
        {
            log::info!(
                "branch_open project={} branch={} phase=gh_pr_diff ms=0 cache=hit",
                project_id,
                branch_label
            );
            let t_base = std::time::Instant::now();
            let resolved_base = er_engine::github::ensure_base_ref_available(
                &repo_root,
                &metadata.freshness.base_branch,
            )
            .map_err(|e| e.to_string())?;
            log_branch_open_phase(project_id, &branch_label, "base_ref_check", t_base);
            // Refresh cached pr_data with the freshly-fetched overview.
            remember_pr_open_entry(
                &state.pr_open_cache,
                key,
                metadata.freshness.clone(),
                raw_diff.clone(),
                Some(metadata.pr_data.clone()),
                Some(metadata.pr_commits.clone()),
            );
            return Ok(PrOpenInputs {
                repo_root,
                metadata,
                resolved_base,
                raw_diff,
                cache_hit: true,
            });
        }
        log::info!(
            "branch_open project={} branch={} phase=gh_pr_diff cache=stale",
            project_id,
            branch_label
        );
        let (diff_res, base_res, diff_ms, base_ms) = std::thread::scope(|s| {
            let diff_root = repo_root.clone();
            let base_root = repo_root.clone();
            let base_branch = metadata.freshness.base_branch.clone();
            let diff_h = s.spawn(move || {
                let t = std::time::Instant::now();
                let res = run_gh_pr_diff_for_open(&diff_root, pr_number);
                (res, t.elapsed().as_millis())
            });
            let base_h = s.spawn(move || {
                let t = std::time::Instant::now();
                let res = er_engine::github::ensure_base_ref_available(&base_root, &base_branch)
                    .map_err(|e| e.to_string());
                (res, t.elapsed().as_millis())
            });
            let (diff_res, diff_ms) = diff_h
                .join()
                .unwrap_or_else(|_| (Err("gh pr diff thread panicked".to_string()), 0));
            let (base_res, base_ms) = base_h
                .join()
                .unwrap_or_else(|_| (Err("base ref fetch thread panicked".to_string()), 0));
            (diff_res, base_res, diff_ms, base_ms)
        });
        log::info!(
            "branch_open project={} branch={} phase=gh_pr_diff ms={} cache=refresh",
            project_id,
            branch_label,
            diff_ms
        );
        log::info!(
            "branch_open project={} branch={} phase=base_ref_check ms={} parallel=true",
            project_id,
            branch_label,
            base_ms
        );
        let raw_diff = diff_res?;
        let resolved_base = base_res?;
        remember_pr_open_entry(
            &state.pr_open_cache,
            key,
            metadata.freshness.clone(),
            raw_diff.clone(),
            Some(metadata.pr_data.clone()),
            Some(metadata.pr_commits.clone()),
        );
        return Ok(PrOpenInputs {
            repo_root,
            metadata,
            resolved_base,
            raw_diff,
            cache_hit: false,
        });
    }

    let (metadata_res, diff_res, view_ms, diff_ms) = std::thread::scope(|s| {
        let view_root = repo_root.clone();
        let diff_root = repo_root.clone();
        let view = s.spawn(move || {
            let t = std::time::Instant::now();
            let res = run_gh_pr_view_for_open(&view_root, pr_number);
            (res, t.elapsed().as_millis())
        });
        let diff = s.spawn(move || {
            let t = std::time::Instant::now();
            let res = run_gh_pr_diff_for_open(&diff_root, pr_number);
            (res, t.elapsed().as_millis())
        });
        let (metadata_res, view_ms) = view
            .join()
            .unwrap_or_else(|_| (Err("gh pr view thread panicked".to_string()), 0));
        let (diff_res, diff_ms) = diff
            .join()
            .unwrap_or_else(|_| (Err("gh pr diff thread panicked".to_string()), 0));
        (metadata_res, diff_res, view_ms, diff_ms)
    });
    log::info!(
        "branch_open project={} branch={} phase=gh_pr_view ms={} cache=miss",
        project_id,
        branch_label,
        view_ms
    );
    log::info!(
        "branch_open project={} branch={} phase=gh_pr_diff ms={} cache=miss",
        project_id,
        branch_label,
        diff_ms
    );
    let metadata = metadata_res?;
    let raw_diff = diff_res?;
    let t_base = std::time::Instant::now();
    let resolved_base =
        er_engine::github::ensure_base_ref_available(&repo_root, &metadata.freshness.base_branch)
            .map_err(|e| e.to_string())?;
    log_branch_open_phase(project_id, &branch_label, "base_ref_check", t_base);
    remember_pr_open_entry(
        &state.pr_open_cache,
        key,
        metadata.freshness.clone(),
        raw_diff.clone(),
        Some(metadata.pr_data.clone()),
        Some(metadata.pr_commits.clone()),
    );
    Ok(PrOpenInputs {
        repo_root,
        metadata,
        resolved_base,
        raw_diff,
        cache_hit: false,
    })
}

/// Open a PR for read-only review. Fetches the PR head to a local ref without
/// running `gh pr checkout` and without touching the working tree or requiring
/// the repo to be clean.
#[tauri::command]
pub async fn open_pr_review(
    project_id: String,
    pr_number: u64,
    replace: Option<bool>,
    hint: Option<PrOpenHint>,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    let t_cmd = std::time::Instant::now();
    run_blocking(move || {
        // TEMP diagnostic: spawn_blocking dispatch latency (candidate 2 — queue wait).
        log::info!(
            "open_pr_review pr={} phase=queue_wait ms={}",
            pr_number,
            t_cmd.elapsed().as_millis()
        );
        open_pr_review_impl(project_id, pr_number, replace, hint, &state)
    })
    .await
}

fn open_pr_review_impl(
    project_id: String,
    pr_number: u64,
    replace: Option<bool>,
    hint: Option<PrOpenHint>,
    state: &AppState,
) -> Result<AppSnapshot, String> {
    let t_total = std::time::Instant::now();
    let branch_label = format!("pr-{}", pr_number);
    let t_tab_build = std::time::Instant::now();
    let inputs =
        load_pr_open_inputs(&project_id, pr_number, hint.as_ref(), state).map_err(|e| {
            log::error!("open_pr_review: pr=#{pr_number} project_id={project_id} err={e}");
            e
        })?;
    let cache_hit = inputs.cache_hit;
    let recent_title = hint
        .as_ref()
        .map(|h| h.title.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| inputs.metadata.pr_data.title.clone());
    // Head branch name for checkout detection. `new_local_pr_from_github_diff`
    // moves `head_branch` into the tab's `local_branch_view`, so capture it now.
    let head_branch_for_checkout = inputs.metadata.freshness.head_branch.clone();
    // The oid the cached diff was computed against. On a cache hit we seed it into
    // the tab via `enter_pr_diff_preloaded` so the staleness probe has a baseline
    // without re-fetching. Captured before `inputs.metadata` is moved below.
    let head_oid_for_preload = inputs.metadata.freshness.head_oid.clone();
    let repo_root_for_checkout = inputs.repo_root.clone();
    let new_tab = er_engine::app::TabState::new_local_pr_from_github_diff(
        inputs.repo_root,
        pr_number,
        inputs.resolved_base,
        inputs.metadata.freshness.head_branch,
        inputs.raw_diff,
        Some(inputs.metadata.pr_data),
        inputs.metadata.pr_commits,
    )
    .map_err(|e| {
        log::error!("open_pr_review: pr=#{pr_number} project_id={project_id} err={e}");
        e.to_string()
    })?;
    // If the PR's head branch is checked out locally (project root or a linked
    // worktree), attach the checkout root now so Unstaged/Staged/working-tree
    // Branch views are immediately available — matching the Tracked-branch
    // entry point. Uses the same `resolve_head_checkout` the active-branch
    // watcher uses, so subsequent focus switches re-attach consistently.
    // Falls back to the tab's `local_branch_view` (a `pr/<n>` placeholder when
    // the head branch name is unknown) which won't match a checkout → None.
    let checkout_branch = if head_branch_for_checkout.is_empty() {
        new_tab.local_branch_view.clone().unwrap_or_default()
    } else {
        head_branch_for_checkout
    };
    let checkout_root = resolve_head_checkout(&repo_root_for_checkout, &checkout_branch);
    let tab_build_ms = t_tab_build.elapsed().as_millis();
    log_branch_open_phase(&project_id, &branch_label, "pr_tab_build", t_tab_build);
    log::info!(
        "branch_open project={} branch={} phase=pr_open_cache hit={}",
        project_id,
        branch_label,
        cache_hit
    );
    let t_app_lock = std::time::Instant::now();
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let app_lock_ms = t_app_lock.elapsed().as_millis();
    log_branch_open_phase(&project_id, &branch_label, "app_lock", t_app_lock);
    let t_place_tab = std::time::Instant::now();
    place_tab(&mut app, new_tab, replace.unwrap_or(false));
    let tab_place_ms = t_place_tab.elapsed().as_millis();
    log_branch_open_phase(&project_id, &branch_label, "tab_place", t_place_tab);
    // Attach the checkout root (if any) to the now-active tab before entering
    // PR Diff, so the first snapshot already reflects the working-tree views.
    if let Some(root) = checkout_root {
        app.tab_mut().local_branch_checkout_root = Some(root);
    }
    let t_pr_diff = std::time::Instant::now();
    // On a fresh cache hit the tab already holds the exact `gh pr diff` output
    // (placed by `new_local_pr_from_github_diff` from `inputs.raw_diff`), so trust
    // it and skip the redundant `gh pr diff` that `enter_pr_diff()` would run via
    // `refresh_diff()`. A miss — or a hit whose head oid is unknown (empty), where
    // seeding it would mislead the staleness probe — still skips the redundant
    // refetch via `enter_pr_diff_freshly_loaded()`: `new_local_pr_from_github_diff`
    // (above) already parsed `inputs.raw_diff` into `tab.files`/`raw_diff`/
    // `diff_hash`, the exact content a plain `enter_pr_diff()` would re-fetch.
    if cache_hit && !head_oid_for_preload.trim().is_empty() {
        app.tab_mut()
            .enter_pr_diff_preloaded(head_oid_for_preload)
            .map_err(|e| e.to_string())?;
    } else {
        app.tab_mut()
            .enter_pr_diff_freshly_loaded()
            .map_err(|e| e.to_string())?;
    }
    let pr_diff_enter_ms = t_pr_diff.elapsed().as_millis();
    log_branch_open_phase(&project_id, &branch_label, "pr_diff_enter", t_pr_diff);
    let t_recent = std::time::Instant::now();
    let _ = projects::record_recent_pr(&project_id, pr_number, &recent_title);
    let record_recent_ms = t_recent.elapsed().as_millis();
    kick_meta_refresh(state, app.tab().repo_root.clone());
    let t_snapshot = std::time::Instant::now();
    let snapshot = snap_from(&app, state);
    let snap_build_ms = t_snapshot.elapsed().as_millis();
    log_branch_open_phase(&project_id, &branch_label, "snapshot_build", t_snapshot);
    log_branch_open_phase(&project_id, &branch_label, "total", t_total);
    kick_active_gh_status(&app, state);
    // TEMP diagnostic: serialize cost + payload size (candidate 1 — snapshot serialize/IPC).
    // `ser_ms`/`ser_bytes` estimate Tauri's post-return serialization; the IPC transfer +
    // JS parse is then `invoke_ms - queue_wait - total - ser_ms`. Remove after diagnosis.
    let t_ser = std::time::Instant::now();
    let ser_bytes = serde_json::to_vec(&snapshot).map(|v| v.len()).unwrap_or(0);
    log::info!(
        "open_pr_review pr={} phase=summary cache_hit={} files={} app_lock_ms={} tab_build_ms={} tab_place_ms={} pr_diff_enter_ms={} record_recent_ms={} snap_build_ms={} ser_bytes={} ser_ms={} total_ms={}",
        pr_number,
        cache_hit,
        snapshot.files.len(),
        app_lock_ms,
        tab_build_ms,
        tab_place_ms,
        pr_diff_enter_ms,
        record_recent_ms,
        snap_build_ms,
        ser_bytes,
        t_ser.elapsed().as_millis(),
        t_total.elapsed().as_millis(),
    );
    Ok(snapshot)
}

/// Kept for backwards compatibility — delegates to the no-checkout PR review flow.
#[tauri::command]
pub async fn open_pr_branch(
    project_id: String,
    pr_number: u64,
    head_ref: String,
    replace: Option<bool>,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let _ = head_ref; // ignored; PR head is fetched directly from origin
    let state = state.inner().clone();
    run_blocking(move || open_pr_review_impl(project_id, pr_number, replace, None, &state)).await
}

/// Fire-and-forget background warmup of the PR-open cache. Invoked from the
/// sidebar's `onmouseenter` (after a short debounce). Returns immediately —
/// the actual fetch runs on a worker thread. If the cache is already fresh
/// for the hint or another prefetch is already in flight, this is a no-op.
#[tauri::command]
pub fn prefetch_pr_open(
    project_id: String,
    pr_number: u64,
    hint: PrOpenHint,
    state: State<AppState>,
) -> Result<(), String> {
    let file = projects::load();
    let Some(proj) = file.projects.iter().find(|p| p.id == project_id).cloned() else {
        return Ok(());
    };
    let repo_root = proj.root_path;
    let key = pr_open_cache_key(&project_id, &repo_root, pr_number);
    let freshness = freshness_from_hint(&hint);

    // Skip if we already have a fresh cached entry matching the hint's freshness.
    if cached_pr_open_diff(&state.pr_open_cache, &key, &freshness).is_some() {
        return Ok(());
    }

    // Dedupe: claim the in-flight slot atomically.
    let claim_key = (project_id.clone(), pr_number);
    {
        let mut guard = state
            .pr_open_prefetch_in_flight
            .lock()
            .map_err(|e| e.to_string())?;
        if guard.contains(&claim_key) {
            return Ok(());
        }
        guard.insert(claim_key.clone());
    }

    let cache = Arc::clone(&state.pr_open_cache);
    let in_flight = Arc::clone(&state.pr_open_prefetch_in_flight);
    let branch_label = format!("pr-{}", pr_number);
    std::thread::spawn(move || {
        let t = std::time::Instant::now();
        let diff_root = repo_root.clone();
        let base_root = repo_root.clone();
        let commits_root = repo_root.clone();
        let base_branch = freshness.base_branch.clone();
        let (diff_res, base_res, commits) = std::thread::scope(|s| {
            let diff_h = s.spawn(move || run_gh_pr_diff_for_open(&diff_root, pr_number));
            let base_h = s.spawn(move || {
                er_engine::github::ensure_base_ref_available(&base_root, &base_branch)
                    .map_err(|e| e.to_string())
            });
            let commits_h = s.spawn(move || run_gh_pr_commits_for_open(&commits_root, pr_number));
            let diff_res = diff_h
                .join()
                .unwrap_or_else(|_| Err("gh pr diff thread panicked".to_string()));
            let base_res = base_h
                .join()
                .unwrap_or_else(|_| Err("base ref fetch thread panicked".to_string()));
            let commits = commits_h.join().unwrap_or_default();
            (diff_res, base_res, commits)
        });
        match (diff_res, base_res) {
            (Ok(raw_diff), Ok(_)) => {
                remember_pr_open_entry(
                    &cache,
                    key,
                    freshness.clone(),
                    raw_diff,
                    None,
                    Some(commits),
                );
                log::info!(
                    "pr_open_prefetch project={} branch={} ok ms={}",
                    claim_key.0,
                    branch_label,
                    t.elapsed().as_millis()
                );
            }
            (Err(e), _) | (_, Err(e)) => {
                log::warn!(
                    "pr_open_prefetch project={} branch={} failed ms={} err={}",
                    claim_key.0,
                    branch_label,
                    t.elapsed().as_millis(),
                    e
                );
            }
        }
        if let Ok(mut guard) = in_flight.lock() {
            guard.remove(&claim_key);
        }
    });
    Ok(())
}

/// Trigger a manual PR-list refresh. Returns the current snapshot immediately
/// (the refresh runs in the background). Deduplicates: if a refresh is already
/// running, this is a no-op.
#[tauri::command]
pub fn refresh_pr_list(state: State<AppState>) -> Result<AppSnapshot, String> {
    // Atomic check-and-set under a single lock acquisition to avoid TOCTOU.
    let already_running = {
        let mut flags = state.loading.lock().map_err(|e| e.to_string())?;
        if flags.pr_list {
            true
        } else {
            flags.pr_list = true;
            false
        }
    };

    if !already_running {
        let cache = Arc::clone(&state.pr_cache);
        let fetched_at = Arc::clone(&state.pr_cache_fetched_at);
        let loading = Arc::clone(&state.loading);
        let desktop_rev = Arc::clone(&state.desktop_revision);
        let pr_cache = Arc::clone(&state.pr_cache);
        let gh_user = Arc::clone(&state.gh_user);
        let inbox = Arc::clone(&state.inbox);
        let app_handle_state = Arc::clone(&state.tauri_app_handle);
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to build tokio runtime");
            let failed =
                rt.block_on(
                    async move { crate::pr_cache::refresh_pr_cache(&cache, &fetched_at).await },
                );
            for remote in failed {
                process_inbox_after_pr_refresh(
                    &pr_cache,
                    &gh_user,
                    &inbox,
                    &desktop_rev,
                    &app_handle_state,
                    Some(remote),
                    None,
                );
            }
            process_inbox_after_pr_refresh(
                &pr_cache,
                &gh_user,
                &inbox,
                &desktop_rev,
                &app_handle_state,
                None,
                None,
            );
            if let Ok(mut f) = loading.lock() {
                f.pr_list = false;
            }
            crate::profile_log::bump_desktop_revision(&desktop_rev, "pr_cache_refresh_manual");
        });
    }

    snap!(state)
}

/// Trigger a PR-list refresh scoped to a single project's remote. Returns the
/// current snapshot immediately (the refresh runs in the background).
/// Deduplicates: if a full PR refresh is already running, this is a no-op.
/// If the project has no remote configured, returns the current snapshot without error.
#[tauri::command]
pub fn refresh_project_pr_list(
    project_id: String,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let file = crate::projects::load();
    let remote = match file.projects.iter().find(|p| p.id == project_id) {
        Some(p) => match p.remote.clone() {
            Some(r) => r,
            None => return snap!(state),
        },
        None => return snap!(state),
    };

    let already_running = {
        let mut flags = state.loading.lock().map_err(|e| e.to_string())?;
        if flags.pr_list {
            true
        } else {
            flags.pr_list = true;
            false
        }
    };

    if !already_running {
        let cache = Arc::clone(&state.pr_cache);
        let fetched_at = Arc::clone(&state.pr_cache_fetched_at);
        let loading = Arc::clone(&state.loading);
        let desktop_rev = Arc::clone(&state.desktop_revision);
        let pr_cache = Arc::clone(&state.pr_cache);
        let gh_user = Arc::clone(&state.gh_user);
        let inbox = Arc::clone(&state.inbox);
        let app_handle_state = Arc::clone(&state.tauri_app_handle);
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to build tokio runtime");
            let remote_clone = remote.clone();
            let success = rt.block_on(async move {
                crate::pr_cache::refresh_pr_cache_for_remote(&remote_clone, &cache, &fetched_at)
                    .await
            });
            if !success {
                process_inbox_after_pr_refresh(
                    &pr_cache,
                    &gh_user,
                    &inbox,
                    &desktop_rev,
                    &app_handle_state,
                    Some(remote),
                    None,
                );
            }
            process_inbox_after_pr_refresh(
                &pr_cache,
                &gh_user,
                &inbox,
                &desktop_rev,
                &app_handle_state,
                None,
                None,
            );
            if let Ok(mut f) = loading.lock() {
                f.pr_list = false;
            }
            crate::profile_log::bump_desktop_revision(
                &desktop_rev,
                "pr_cache_refresh_project_manual",
            );
        });
    }

    snap!(state)
}

#[tauri::command]
pub fn mark_inbox_item_read(id: String, state: State<AppState>) -> Result<AppSnapshot, String> {
    let now = now_ms();
    if let Ok(mut inbox) = state.inbox.lock() {
        inbox.mark_item_read(&id, now);
    }
    crate::inbox::save_inbox_state(&state.inbox);
    state.desktop_revision.fetch_add(1, Ordering::Relaxed);
    snap!(state)
}

#[tauri::command]
pub fn mark_all_inbox_read(state: State<AppState>) -> Result<AppSnapshot, String> {
    let now = now_ms();
    if let Ok(mut inbox) = state.inbox.lock() {
        inbox.mark_all_read(now);
    }
    crate::inbox::save_inbox_state(&state.inbox);
    state.desktop_revision.fetch_add(1, Ordering::Relaxed);
    snap!(state)
}

#[tauri::command]
pub fn clear_read_inbox_items(state: State<AppState>) -> Result<AppSnapshot, String> {
    if let Ok(mut inbox) = state.inbox.lock() {
        inbox.clear_read();
    }
    crate::inbox::save_inbox_state(&state.inbox);
    state.desktop_revision.fetch_add(1, Ordering::Relaxed);
    snap!(state)
}

#[tauri::command]
pub fn refresh_notifications(state: State<AppState>) -> Result<AppSnapshot, String> {
    refresh_pr_list(state)
}

#[tauri::command]
pub async fn open_inbox_item(
    id: String,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || open_inbox_item_impl(id, &state)).await
}

fn open_inbox_item_impl(id: String, state: &AppState) -> Result<AppSnapshot, String> {
    let now = now_ms();
    let mut target = {
        let mut inbox = state.inbox.lock().map_err(|e| e.to_string())?;
        let target = inbox
            .items
            .iter()
            .find(|i| i.id == id)
            .map(|i| i.target.clone());
        inbox.mark_item_read(&id, now);
        target
    };
    crate::inbox::save_inbox_state(&state.inbox);
    state.desktop_revision.fetch_add(1, Ordering::Relaxed);

    if let Some(mut target) = target.take() {
        if target.project_id.is_none() {
            target.project_id = projects::resolve_project_id_for_inbox(
                target.repo_root.as_deref(),
                target.remote.as_deref(),
            );
        }
        if let (Some(project_id), Some(pr_number)) = (target.project_id.clone(), target.pr_number) {
            return open_pr_review_impl(project_id, pr_number, Some(true), None, state);
        }
        if let (Some(project_id), Some(branch)) = (target.project_id, target.branch) {
            return open_local_branch_impl(project_id, branch, Some(true), state);
        }
    }

    if let Ok(mut app) = state.app.lock() {
        app.notify(
            "Could not open notification target — add or select the project in Easy Review first",
        );
    }
    snap!(state)
}

#[tauri::command]
pub fn add_tracked_branch(
    project_id: String,
    name: String,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let file = projects::load();
    let proj = file
        .projects
        .iter()
        .find(|p| p.id == project_id)
        .ok_or_else(|| format!("Project not found: {project_id}"))?;

    // Confirm the branch actually exists locally before tracking it.
    let exists = std::process::Command::new("git")
        .args([
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{}", name),
        ])
        .current_dir(&proj.root_path)
        .status()
        .map_err(|e| format!("git show-ref failed: {e}"))?
        .success();
    if !exists {
        return Err(format!("Branch '{name}' does not exist locally"));
    }

    projects::add_tracked_branch(&project_id, &name).map_err(|e| e.to_string())?;
    let root = state
        .app
        .lock()
        .ok()
        .map(|a| a.tab().repo_root.clone())
        .unwrap_or_default();
    kick_meta_refresh(&state, root);
    snap!(state)
}

#[tauri::command]
pub fn remove_tracked_branch(
    project_id: String,
    name: String,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let file = projects::load();
    let proj = file
        .projects
        .iter()
        .find(|p| p.id == project_id)
        .ok_or_else(|| format!("Project not found: {project_id}"))?;

    let current = std::process::Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(&proj.root_path)
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_default();
    if !current.is_empty() && current == name {
        return Err(
            "Cannot remove the currently-checked-out branch from view; switch first.".to_string(),
        );
    }

    projects::remove_tracked_branch(&project_id, &name).map_err(|e| e.to_string())?;
    let root = state
        .app
        .lock()
        .ok()
        .map(|a| a.tab().repo_root.clone())
        .unwrap_or_default();
    kick_meta_refresh(&state, root);
    snap!(state)
}

#[tauri::command]
pub fn list_available_branches(project_id: String) -> Result<Vec<String>, String> {
    let file = projects::load();
    let proj = file
        .projects
        .iter()
        .find(|p| p.id == project_id)
        .ok_or_else(|| format!("Project not found: {project_id}"))?;

    let out = std::process::Command::new("git")
        .args(["for-each-ref", "--format=%(refname:short)", "refs/heads/"])
        .current_dir(&proj.root_path)
        .output()
        .map_err(|e| format!("git for-each-ref failed: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git for-each-ref failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let text = String::from_utf8_lossy(&out.stdout);

    let current = std::process::Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(&proj.root_path)
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_default();

    let names: Vec<String> = text
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|n| !n.is_empty())
        .filter(|n| n != &current && !proj.tracked_branches.iter().any(|t| t == n))
        .collect();
    Ok(names)
}

#[tauri::command]
pub fn delete_project(project_id: String, state: State<AppState>) -> Result<AppSnapshot, String> {
    let before = projects::load();
    let deleted_remote = before
        .projects
        .iter()
        .find(|p| p.id == project_id)
        .and_then(|p| p.remote.clone());
    projects::delete_project(&project_id).map_err(|e| e.to_string())?;
    if let Some(remote) = deleted_remote {
        let remaining = projects::load();
        let target = normalize_remote_slug(&remote);
        let remote_still_used = remaining.projects.iter().any(|p| {
            p.remote
                .as_ref()
                .is_some_and(|r| normalize_remote_slug(r) == target)
        });
        if !remote_still_used {
            if let Ok(mut cache) = state.pr_cache.lock() {
                cache.retain(|r, _| normalize_remote_slug(r) != target);
            }
            if let Ok(mut fetched_at) = state.pr_cache_fetched_at.lock() {
                fetched_at.retain(|r, _| normalize_remote_slug(r) != target);
            }
            crate::pr_cache::save_persisted_pr_cache(&state.pr_cache, &state.pr_cache_fetched_at);
        }
    }
    state.desktop_revision.fetch_add(1, Ordering::Relaxed);
    snap!(state)
}

#[tauri::command]
pub fn open_project_branch(
    project_id: String,
    branch: String,
    replace: Option<bool>,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    // Read-only: same logic as open_local_branch. We also mark the project active.
    let file = projects::load();
    let proj = file
        .projects
        .iter()
        .find(|p| p.id == project_id)
        .ok_or_else(|| format!("Project not found: {project_id}"))?
        .clone();
    let mut new_tab = er_engine::app::TabState::new_with_base(
        proj.root_path.clone(),
        er_engine::git::detect_base_branch_in(&proj.root_path).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    new_tab.local_branch_view = Some(branch);
    new_tab.mode = er_engine::app::DiffMode::Branch;
    new_tab.sync_managed_storage();
    refresh_branch_open_diff(&mut new_tab)?;

    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    place_tab(&mut app, new_tab, replace.unwrap_or(false));
    projects::set_active(&project_id);
    kick_meta_refresh(&state, app.tab().repo_root.clone());
    kick_active_gh_status(&app, &state);
    Ok(snap_from(&app, &state))
}

#[tauri::command]
pub fn dismiss_remote_pr(
    project_id: String,
    pr_number: u64,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    projects::dismiss_pr(&project_id, pr_number);
    let root = state
        .app
        .lock()
        .ok()
        .map(|a| a.tab().repo_root.clone())
        .unwrap_or_default();
    kick_meta_refresh(&state, root);
    state
        .desktop_revision
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    snap!(state)
}

#[tauri::command]
pub fn undismiss_remote_pr(
    project_id: String,
    pr_number: u64,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    projects::undismiss_pr(&project_id, pr_number).map_err(|e| e.to_string())?;
    let root = state
        .app
        .lock()
        .ok()
        .map(|a| a.tab().repo_root.clone())
        .unwrap_or_default();
    kick_meta_refresh(&state, root);
    state
        .desktop_revision
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    snap!(state)
}

#[tauri::command]
pub async fn sync_pr(
    project_id: String,
    pr_number: u64,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let proj = projects::load()
            .projects
            .into_iter()
            .find(|p| p.id == project_id);
        let (remote, root_path) = match proj {
            Some(p) => (
                p.remote
                    .ok_or_else(|| "Project has no remote configured".to_string())?,
                p.root_path,
            ),
            None => return Err("Project not found".to_string()),
        };
        let fetched_pr = cache_single_pr_for_remote(&state, &remote, pr_number)?;

        // sync_pr alone only refreshes the metadata cache. If this PR is open in
        // any tab, force-refresh that tab's diff so the view reflects origin.
        // Capture a successfully-refreshed *local* PR tab's diff oid so we can
        // persist the new diff into pr_open_cache below — making the next open of
        // this PR an instant cache hit instead of another `gh pr diff`.
        let mut refreshed_local_diff_oid: Option<Option<String>> = None;
        if let Ok(mut app) = state.app.lock() {
            let indices: Vec<usize> = app
                .tabs
                .iter()
                .enumerate()
                .filter(|(_, t)| {
                    t.pr_number == Some(pr_number)
                        && (t.remote_repo.as_deref() == Some(remote.as_str())
                            || t.repo_root == root_path)
                })
                .map(|(i, _)| i)
                .collect();
            for i in indices {
                if let Some(tab) = app.tabs.get_mut(i) {
                    match tab.refetch_and_refresh_diff() {
                        Ok(()) => {
                            // Remote tabs can't form the local open key (their
                            // repo_root is the launch CWD, not the checkout) — and
                            // the remote open path doesn't read the cache yet, so
                            // only local PR tabs are worth persisting here.
                            if !tab.is_remote() && refreshed_local_diff_oid.is_none() {
                                refreshed_local_diff_oid = Some(tab.last_diff_head_oid.clone());
                            }
                        }
                        Err(e) => log::warn!("sync_pr: diff refresh failed for tab {i}: {e}"),
                    }
                }
            }
        }

        // Persist the refreshed diff for the local PR tab (lock released). The key
        // matches the local open path (`load_pr_open_inputs`: project_id + the
        // project root + pr_number); the freshness matches a future open's, so the
        // reopen hits. We re-run `gh pr diff` here rather than reuse the tab's
        // view-dependent `raw_diff` (which may be the Local-Branch diff, not the PR
        // diff) to store exactly the canonical bytes the open path caches. An
        // `updated_at`/oid mismatch on a later open is a safe miss, never a stale hit.
        if let Some(diff_oid) = refreshed_local_diff_oid {
            let key = pr_open_cache_key(&project_id, &root_path, pr_number);
            let freshness = freshness_from_pr_info(&fetched_pr, diff_oid.as_deref());
            match run_gh_pr_diff_for_open(&root_path, pr_number) {
                Ok(raw_diff) => {
                    remember_pr_open_entry(
                        &state.pr_open_cache,
                        key,
                        freshness,
                        raw_diff,
                        None,
                        None,
                    );
                }
                Err(e) => {
                    log::warn!("sync_pr: cache-persist diff fetch failed for #{pr_number}: {e}")
                }
            }
        }

        state
            .desktop_revision
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        snap!(state)
    })
    .await
}

#[tauri::command]
pub async fn sync_branch(
    project_id: String,
    branch: String,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let root_path = projects::load()
            .projects
            .into_iter()
            .find(|p| p.id == project_id)
            .map(|p| p.root_path)
            .ok_or_else(|| "Project not found".to_string())?;

        if let Ok(mut app) = state.app.lock() {
            let indices: Vec<usize> = app
                .tabs
                .iter()
                .enumerate()
                .filter(|(_, t)| {
                    !t.is_remote()
                        && t.pr_number.is_none()
                        && t.repo_root == root_path
                        && (t.local_branch_view.as_deref() == Some(branch.as_str())
                            || (t.local_branch_view.is_none() && t.current_branch == branch))
                })
                .map(|(i, _)| i)
                .collect();
            for i in indices {
                if let Some(tab) = app.tabs.get_mut(i) {
                    if let Err(e) = tab.refetch_and_refresh_diff() {
                        log::warn!("sync_branch: diff refresh failed for tab {i}: {e}");
                    }
                }
            }
        }

        state
            .desktop_revision
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        snap!(state)
    })
    .await
}

#[tauri::command]
pub fn track_pr(
    project_id: String,
    pr_number: u64,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    projects::track_pr(&project_id, pr_number).map_err(|e| e.to_string())?;
    let root = state
        .app
        .lock()
        .ok()
        .map(|a| a.tab().repo_root.clone())
        .unwrap_or_default();
    kick_meta_refresh(&state, root);
    snap!(state)
}

fn resolve_pr_title_for_project(
    project_id: &str,
    pr_number: u64,
    title: Option<String>,
    state: &AppState,
) -> Result<String, String> {
    if let Some(t) = title {
        let trimmed = t.trim().to_string();
        if !trimmed.is_empty() {
            return Ok(trimmed);
        }
    }
    let file = projects::load();
    let proj = file
        .projects
        .iter()
        .find(|p| p.id == project_id)
        .ok_or_else(|| format!("Project not found: {project_id}"))?;
    if let Some(remote) = proj.remote.as_ref() {
        if let Ok(cache) = state.pr_cache.lock() {
            if let Some(prs) = cache.get(remote) {
                if let Some(pr) = prs.iter().find(|p| p.number == pr_number) {
                    if !pr.title.is_empty() {
                        return Ok(pr.title.clone());
                    }
                }
            }
        }
    }
    Ok(format!("PR #{pr_number}"))
}

#[tauri::command]
pub fn save_pr(
    project_id: String,
    pr_number: u64,
    title: Option<String>,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let title = resolve_pr_title_for_project(&project_id, pr_number, title, &state)?;
    projects::save_pr(&project_id, pr_number, &title).map_err(|e| e.to_string())?;
    let root = state
        .app
        .lock()
        .ok()
        .map(|a| a.tab().repo_root.clone())
        .unwrap_or_default();
    kick_meta_refresh(&state, root);
    snap!(state)
}

#[tauri::command]
pub fn unsave_pr(
    project_id: String,
    pr_number: u64,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    projects::unsave_pr(&project_id, pr_number).map_err(|e| e.to_string())?;
    let root = state
        .app
        .lock()
        .ok()
        .map(|a| a.tab().repo_root.clone())
        .unwrap_or_default();
    kick_meta_refresh(&state, root);
    snap!(state)
}

#[tauri::command]
pub fn untrack_pr(
    project_id: String,
    pr_number: u64,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    projects::untrack_pr(&project_id, pr_number).map_err(|e| e.to_string())?;
    let root = state
        .app
        .lock()
        .ok()
        .map(|a| a.tab().repo_root.clone())
        .unwrap_or_default();
    kick_meta_refresh(&state, root);
    snap!(state)
}

#[tauri::command]
pub fn list_available_prs(
    project_id: String,
    state: State<AppState>,
) -> Result<Vec<PrInfo>, String> {
    let file = projects::load();
    let proj = file
        .projects
        .iter()
        .find(|p| p.id == project_id)
        .ok_or_else(|| format!("Project not found: {project_id}"))?
        .clone();

    let Some(remote) = proj.remote.as_ref() else {
        return Ok(Vec::new());
    };

    let cache_prs: Vec<PrInfo> = state
        .pr_cache
        .lock()
        .ok()
        .and_then(|g| g.get(remote).cloned())
        .unwrap_or_default();

    let me: Option<String> = state.gh_user.lock().ok().and_then(|v| v.clone());

    // Return PRs that would NOT be visible today: not dismissed, not tracked,
    // and not matching the author/assignee/reviewer filter.
    let avail: Vec<PrInfo> = cache_prs
        .into_iter()
        .filter(|pr| {
            if proj.dismissed_prs.contains(&pr.number) {
                return false;
            }
            if proj.tracked_prs.contains(&pr.number) {
                return false;
            }
            if let Some(ref login) = me {
                if &pr.author == login {
                    return false;
                }
                if pr.assignees.iter().any(|a| a == login) {
                    return false;
                }
                if pr.reviewers.iter().any(|r| r == login) {
                    return false;
                }
            }
            true
        })
        .collect();
    Ok(avail)
}

#[tauri::command]
pub fn set_active_project(id: String, state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let file = projects::load();
    let proj = file
        .projects
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| format!("Project not found: {id}"))?
        .clone();
    let new_tab = er_engine::app::TabState::new(proj.root_path.clone())
        .map_err(|e| format!("Failed to open {}: {e}", proj.root_path))?;
    app.open_tab(new_tab);
    app.tab_mut().refresh_diff().map_err(|e| e.to_string())?;
    crate::tabs::persist_app_tabs(&app);
    projects::set_active(&id);
    kick_meta_refresh(&state, app.tab().repo_root.clone());
    kick_active_gh_status(&app, &state);
    Ok(snap_from(&app, &state))
}

#[tauri::command]
pub fn delete_review_artifact(kind: String, state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let er_dir = app.tab().er_dir();

    match kind.as_str() {
        "triage" => er_engine::app::cleanup_triage(&er_dir),
        "review" => er_engine::app::cleanup_review_artifacts(&er_dir),
        other => return Err(format!("Unknown review artifact kind: {other}")),
    }

    app.tab_mut().reload_ai_state();
    state
        .desktop_revision
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Ok(snap_from(&app, &state))
}

// ── Findings: dismiss / promote / reply (v1 stubs) ──────────────────────────

#[tauri::command]
pub fn dismiss_finding(finding_id: String, state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;

    let er_dir = app.tab().er_dir();
    let removed = er_engine::ai::remove_finding_from_sidecars(&er_dir, &finding_id)
        .map_err(|e| format!("Failed to remove finding: {e}"))?;
    if !removed {
        return Err(format!("Finding not found: {finding_id}"));
    }

    let _ = er_engine::ai::delete_threads_linked_to_finding(&er_dir, &finding_id);

    let mut promotions = load_finding_promotions(&er_dir);
    if promotions.remove(&finding_id).is_some() {
        save_finding_promotions(&er_dir, &promotions)
            .map_err(|e| format!("Failed to update finding promotions: {e}"))?;
    }

    app.tab_mut().reload_ai_state();

    Ok(snap_from(&app, &state))
}

#[tauri::command]
pub fn promote_finding_to_comment(
    finding_id: String,
    body: Option<String>,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;

    let er_dir = app.tab().er_dir();

    let found = {
        let tab = app.tab();
        let mut result: Option<(String, usize, Option<usize>, String)> = None;
        if let Some(review) = tab.ai.review.as_ref() {
            'outer: for (path, file) in review.files.iter() {
                for f in file.findings.iter() {
                    if f.id == finding_id {
                        let default = if f.description.is_empty() {
                            f.title.clone()
                        } else {
                            format!("{}\n\n{}", f.title, f.description)
                        };
                        result = Some((
                            path.clone(),
                            f.hunk_index.unwrap_or(0),
                            f.line_start,
                            default,
                        ));
                        break 'outer;
                    }
                }
            }
        }
        result
    };

    let (file, hunk_idx, line_start, default_body) =
        found.ok_or_else(|| format!("Finding not found: {finding_id}"))?;
    let text = match body {
        Some(b) => b,
        None => {
            let promote_replies =
                er_engine::ai::collect_finding_promote_replies(&app.tab().ai, &finding_id);
            er_engine::ai::append_promote_replies(default_body, &promote_replies)
        }
    };

    let existing_ids: std::collections::HashSet<String> = {
        let tab = app.tab();
        tab.ai
            .github_comments
            .as_ref()
            .map(|gc| gc.comments.iter().map(|c| c.id.clone()).collect())
            .unwrap_or_default()
    };

    app.submit_comment_text(
        file,
        hunk_idx,
        line_start,
        None,
        text,
        CommentType::GitHubComment,
        None,
        None,
    )
    .map_err(|e| e.to_string())?;

    let new_id: Option<String> = {
        let tab = app.tab();
        tab.ai.github_comments.as_ref().and_then(|gc| {
            gc.comments
                .iter()
                .find(|c| !existing_ids.contains(&c.id))
                .map(|c| c.id.clone())
        })
    };

    if new_id.is_some() {
        er_engine::ai::remove_finding_from_sidecars(&er_dir, &finding_id)
            .map_err(|e| format!("Failed to remove finding after promote: {e}"))?;
        let _ = er_engine::ai::delete_threads_linked_to_finding(&er_dir, &finding_id);
        let mut promotions = load_finding_promotions(&er_dir);
        if promotions.remove(&finding_id).is_some() {
            let _ = save_finding_promotions(&er_dir, &promotions);
        }
        app.tab_mut().reload_ai_state();
    }

    Ok(snap_from(&app, &state))
}

const FINDING_THREAD_STUB: &str = "Follow-up on this finding.";

#[tauri::command]
pub fn reply_to_finding(
    finding_id: String,
    body: String,
    _ai_assist: bool,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;

    // v1: create a github comment that references the finding's location.
    let (target, finding_text) = {
        let tab = app.tab();
        let mut result: Option<(String, usize, Option<usize>)> = None;
        let mut text: Option<(String, String)> = None;
        if let Some(review) = tab.ai.review.as_ref() {
            'outer: for (path, file) in review.files.iter() {
                for f in file.findings.iter() {
                    if f.id == finding_id {
                        result = Some((path.clone(), f.hunk_index.unwrap_or(0), f.line_start));
                        text = Some((f.title.clone(), f.description.clone()));
                        break 'outer;
                    }
                }
            }
        }
        (result, text)
    };

    let (file, hunk_idx, line_start) =
        target.ok_or_else(|| format!("Finding not found: {finding_id}"))?;

    if _ai_assist {
        let prompt = if body.trim().is_empty() {
            DEFAULT_ASK_AI_PROMPT.to_string()
        } else {
            body
        };
        let enriched_prompt = if let Some((title, desc)) = finding_text {
            format!("Finding: {title}\n\n{desc}\n\n---\n\n{prompt}")
        } else {
            prompt
        };

        let root_id = if let Some(root) =
            er_engine::ai::find_finding_thread_root(&app.tab().ai, &finding_id)
        {
            root
        } else {
            app.submit_comment_text(
                file.clone(),
                hunk_idx,
                line_start,
                None,
                "AI follow-up requested for this finding.".to_string(),
                CommentType::GitHubComment,
                None,
                Some(finding_id.clone()),
            )
            .map_err(|e| e.to_string())?;
            app.tab()
                .ai
                .github_comments
                .as_ref()
                .and_then(|gc| {
                    gc.comments
                        .iter()
                        .find(|c| {
                            c.finding_ref.as_deref() == Some(finding_id.as_str())
                                && c.in_reply_to.is_none()
                        })
                        .map(|c| c.id.clone())
                })
                .ok_or_else(|| "Failed to create finding comment thread".to_string())?
        };
        drop(app);
        return ask_ai(root_id, enriched_prompt, state);
    }

    let root_id = er_engine::ai::find_finding_thread_root(&app.tab().ai, &finding_id);
    if let Some(root_id) = root_id {
        app.submit_comment_text(
            file,
            hunk_idx,
            line_start,
            None,
            body,
            CommentType::GitHubComment,
            Some(root_id),
            None,
        )
        .map_err(|e| e.to_string())?;
    } else {
        app.submit_comment_text(
            file.clone(),
            hunk_idx,
            line_start,
            None,
            FINDING_THREAD_STUB.to_string(),
            CommentType::GitHubComment,
            None,
            Some(finding_id.clone()),
        )
        .map_err(|e| e.to_string())?;
        let root_id = app
            .tab()
            .ai
            .github_comments
            .as_ref()
            .and_then(|gc| {
                gc.comments
                    .iter()
                    .find(|c| {
                        c.finding_ref.as_deref() == Some(finding_id.as_str())
                            && c.in_reply_to.is_none()
                    })
                    .map(|c| c.id.clone())
            })
            .ok_or_else(|| "Failed to create finding comment thread".to_string())?;
        app.submit_comment_text(
            file,
            hunk_idx,
            line_start,
            None,
            body,
            CommentType::GitHubComment,
            Some(root_id),
            None,
        )
        .map_err(|e| e.to_string())?;
    }

    Ok(snap_from(&app, &state))
}

#[tauri::command]
pub fn update_thread_message(
    id: String,
    body: String,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let text = body.trim();
    if text.is_empty() {
        return Err("Message cannot be empty".to_string());
    }

    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let author = {
        let tab = app.tab();
        if id.starts_with("q-") {
            tab.ai
                .questions
                .as_ref()
                .and_then(|qs| qs.questions.iter().find(|q| q.id == id))
                .map(|q| q.author.clone())
        } else if id.starts_with("n-") {
            tab.ai
                .notes
                .as_ref()
                .and_then(|ns| ns.notes.iter().find(|n| n.id == id))
                .map(|n| n.author.clone())
        } else {
            tab.ai
                .github_comments
                .as_ref()
                .and_then(|gc| gc.comments.iter().find(|c| c.id == id))
                .map(|c| c.author.clone())
        }
    };
    let author = author.ok_or_else(|| format!("Thread message not found: {id}"))?;
    if author == "ai" {
        return Err("Cannot edit AI-generated text".to_string());
    }
    if !author.is_empty() && author != "You" {
        return Err("Can only edit your own messages".to_string());
    }

    app.update_comment_text(&id, text)
        .map_err(|e| e.to_string())?;
    Ok(snap_from(&app, &state))
}

// ── Review export (markdown) ─────────────────────────────────────────────────

use crate::export::{render_markdown, ExportOpts};

/// Render the active tab's annotations as markdown and return the body to
/// the UI for clipboard copy / preview.
#[tauri::command]
pub fn export_review(opts: ExportOpts, state: State<AppState>) -> Result<String, String> {
    let app = state.app.lock().map_err(|e| e.to_string())?;
    Ok(render_markdown(app.tab(), &opts))
}

/// Render and write to disk. Empty `path` writes to `<comments_dir>/export.md`.
/// Returns the resolved absolute path so the UI can show it in a toast.
#[tauri::command]
pub fn export_review_to_file(
    opts: ExportOpts,
    path: String,
    state: State<AppState>,
) -> Result<String, String> {
    let app = state.app.lock().map_err(|e| e.to_string())?;
    let tab = app.tab();
    let body = render_markdown(tab, &opts);
    let target = if path.trim().is_empty() {
        let dir = tab.comments_dir();
        std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create {dir}: {e}"))?;
        format!("{dir}/export.md")
    } else {
        path
    };
    std::fs::write(&target, body).map_err(|e| format!("Failed to write {target}: {e}"))?;
    Ok(target)
}

/// Back-compat shim: delegate to `export_review_to_file` with all-defaults
/// opts. Kept so older bindings / CommandPalette entries don't break.
#[tauri::command]
pub fn export_to_agent(state: State<AppState>) -> Result<AppSnapshot, String> {
    let opts = ExportOpts::default();
    let path = {
        let app = state.app.lock().map_err(|e| e.to_string())?;
        let tab = app.tab();
        let body = render_markdown(tab, &opts);
        let dir = tab.comments_dir();
        std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create {dir}: {e}"))?;
        let path = format!("{dir}/export.md");
        std::fs::write(&path, body).map_err(|e| format!("Failed to write {path}: {e}"))?;
        path
    };
    let _ = path;
    let app = state.app.lock().map_err(|e| e.to_string())?;
    Ok(snap_from(&app, &state))
}

// ── Commit composer ──────────────────────────────────────────────────────────

#[tauri::command]
pub fn open_commit_composer(state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    app.input_mode = InputMode::Commit;
    Ok(snap_from(&app, &state))
}

// ── History: select commit ──────────────────────────────────────────────────

#[tauri::command]
pub fn select_commit(sha: String, state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    {
        let tab = app.tab_mut();
        // Switching into History mode lazily initializes HistoryState with the
        // first 50 commits — needed when the user clicks a commit from
        // Branch/Unstaged/Staged where history was never loaded.
        if tab.mode != DiffMode::History {
            tab.set_mode(DiffMode::History);
        }
        if let Some(history) = tab.history.as_mut() {
            if let Some(pos) = history.commits.iter().position(|c| c.hash == sha) {
                history.selected_commit = pos;
                tab.history_load_selected_diff();
            }
        }
    }
    Ok(snap_from(&app, &state))
}

// ── Tab management ──────────────────────────────────────────────────────────

#[tauri::command]
pub fn new_tab(state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    // Spawn a working-tree tab cloned from the active tab's repo root.
    // If that fails (e.g. deleted repo), fall back to the first tab's root.
    let root = app.tab().repo_root.clone();
    let tab = er_engine::app::TabState::new(root.clone())
        .or_else(|_| er_engine::app::TabState::new(app.tabs[0].repo_root.clone()))
        .map_err(|e| format!("Failed to open new tab: {e}"))?;
    app.open_tab(tab);
    crate::tabs::persist_app_tabs(&app);
    kick_meta_refresh(&state, root);
    Ok(snap_from(&app, &state))
}

#[tauri::command]
pub fn close_tab(
    idx: usize,
    app_handle: tauri::AppHandle,
    state: State<AppState>,
    browser_state: State<'_, crate::browser_webview::BrowserWebviewState>,
) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    app.close_tab_at(idx);
    kick_deferred_tab_refresh(&mut app, &state);
    crate::browser_webview::reset_all_tab_webviews(&app_handle, &browser_state)?;
    let active = app.active_tab;
    crate::browser_webview::on_tab_selected(&app_handle, &browser_state, &app, active)?;
    crate::tabs::persist_app_tabs(&app);
    kick_active_gh_status(&app, &state);
    Ok(snap_from(&app, &state))
}

#[tauri::command]
pub async fn select_tab(
    idx: usize,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        use tauri::Manager;
        let browser_state = app_handle.state::<crate::browser_webview::BrowserWebviewState>();
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        app.select_tab(idx);
        kick_deferred_tab_refresh(&mut app, &state);
        kick_active_gh_status(&app, &state);
        crate::browser_webview::on_tab_selected(&app_handle, &browser_state, &app, idx)?;
        crate::tabs::persist_app_tabs(&app);
        Ok(snap_from(&app, &state))
    })
    .await
}

/// If the active tab was restored as a lazy stub, kick its first
/// `refresh_diff()` to a background thread and return immediately. The
/// caller's snapshot shows the stub with `loading.tab_diff = true`; the
/// loaded diff arrives via the revision-event poll when the worker finishes.
/// (This used to run inline while holding the App lock — a tab switch onto a
/// large stub tab serialized every other command behind a multi-second git
/// diff + parse.)
pub(crate) fn kick_deferred_tab_refresh(app: &mut App, state: &AppState) {
    let idx = app.active_tab;
    let tab = app.tab_mut();
    if !tab.needs_initial_refresh {
        return;
    }
    tab.needs_initial_refresh = false;
    let expect_root = tab.repo_root.clone();
    if let Ok(mut l) = state.loading.lock() {
        l.tab_diff = true;
    }
    let app_arc = Arc::clone(&state.app);
    let loading = Arc::clone(&state.loading);
    let desktop_revision = Arc::clone(&state.desktop_revision);
    std::thread::spawn(move || {
        let t = std::time::Instant::now();
        if let Ok(mut app) = app_arc.lock() {
            // Re-resolve the tab by index + repo_root in case tabs changed
            // while this worker waited for the lock.
            if let Some(tab) = app.tabs.get_mut(idx).filter(|t| t.repo_root == expect_root) {
                let is_local_pr = tab.pr_number.is_some() && !tab.is_remote();
                let result = if is_local_pr {
                    tab.refetch_and_refresh_diff()
                } else {
                    tab.refresh_diff()
                };
                if let Err(e) = result {
                    log::error!("er-desktop: deferred tab refresh failed: {e}");
                }
            }
        }
        if let Ok(mut l) = loading.lock() {
            l.tab_diff = false;
        }
        desktop_revision.fetch_add(1, Ordering::Relaxed);
        crate::profile_log::profile_log(
            "lazy_tab_refresh",
            &[("ms", t.elapsed().as_millis().to_string())],
        );
    });
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub fn update_tab_browser(
    layout: Option<String>,
    url: Option<String>,
    // When true with `url`, drive the child webview to that URL. Default: persist only.
    navigate: Option<bool>,
    annotate: Option<bool>,
    tooltips: Option<bool>,
    split_ratio: Option<f32>,
    app_handle: tauri::AppHandle,
    browser_state: State<'_, crate::browser_webview::BrowserWebviewState>,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let tab_idx = app.active_tab;
    let tab = app.tab_mut();
    if let Some(l) = layout.as_deref() {
        tab.browser_layout = BrowserLayout::from_label(l);
    }
    let had_url = url.is_some();
    if let Some(u) = url {
        tab.browser_url = u;
    }
    let url_nav_needed = layout.is_some() || navigate.unwrap_or(false);
    if let Some(a) = annotate {
        tab.browser_annotate_mode = a;
    }
    if let Some(t) = tooltips {
        tab.browser_show_tooltips = t;
    }
    if let Some(r) = split_ratio {
        tab.browser_split_ratio = r.clamp(0.35, 0.65);
    }
    if url_nav_needed {
        crate::browser_webview::on_tab_selected(&app_handle, &browser_state, &app, tab_idx)?;
    } else if layout.is_none() && !had_url {
        crate::browser_webview::sync_tab_browser_chrome(
            &app_handle,
            &browser_state,
            &app,
            tab_idx,
            annotate.is_some(),
        )?;
    }
    state.desktop_revision.fetch_add(1, Ordering::Relaxed);
    Ok(snap_from(&app, &state))
}

#[tauri::command]
pub fn cycle_tab_browser_layout(
    app_handle: tauri::AppHandle,
    browser_state: State<'_, crate::browser_webview::BrowserWebviewState>,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let tab_idx = app.active_tab;
    let tab = app.tab_mut();
    tab.browser_layout = tab.browser_layout.cycle();
    crate::browser_webview::on_tab_selected(&app_handle, &browser_state, &app, tab_idx)?;
    state.desktop_revision.fetch_add(1, Ordering::Relaxed);
    Ok(snap_from(&app, &state))
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn reorder_tabs(
    fromIdx: usize,
    toIdx: usize,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    app.reorder_tabs(fromIdx, toIdx);
    crate::tabs::persist_app_tabs(&app);
    Ok(snap_from(&app, &state))
}

// ── UI annotations (browser view) ───────────────────────────────────────────

#[tauri::command]
#[allow(non_snake_case, clippy::too_many_arguments)]
pub fn add_ui_annotation(
    url: String,
    selector: Option<String>,
    bbox: [f64; 4],
    viewport: [u32; 2],
    text: String,
    screenshotDataUrl: Option<String>,
    elementContext: Option<String>,
    domContext: Option<serde_json::Value>,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let app = state.app.lock().map_err(|e| e.to_string())?;
    let dir = app.tab().comments_dir();
    let mut anns = er_engine::ai::load_ui_annotations(&dir);
    let id = format!(
        "ui-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    );

    // If a screenshot data URL was provided, decode and persist it under
    // `<comments_dir>/screenshots/<id>.png`. Failure to decode is non-fatal:
    // the annotation is still saved without a screenshot path.
    let screenshot_path = match screenshotDataUrl.as_deref() {
        Some(data_url) => decode_data_url_png(data_url)
            .and_then(|bytes| save_screenshot_bytes(&dir, &id, &bytes).ok()),
        None => None,
    };

    let ts = chrono_like_timestamp();
    anns.push(er_engine::ai::UiAnnotation {
        id,
        url,
        selector,
        box_x: bbox[0],
        box_y: bbox[1],
        box_w: bbox[2],
        box_h: bbox[3],
        viewport_w: viewport[0],
        viewport_h: viewport[1],
        text,
        timestamp: ts,
        author: "You".to_string(),
        screenshot_path,
        stale: false,
        element_context: elementContext,
        dom_context: domContext,
    });
    er_engine::ai::save_ui_annotations(&dir, &anns).map_err(|e| e.to_string())?;
    state.desktop_revision.fetch_add(1, Ordering::Relaxed);
    Ok(snap_from(&app, &state))
}

/// Decode a `data:image/png;base64,<payload>` URL into raw PNG bytes. Returns
/// `None` if the prefix is missing or base64 is malformed. We accept any
/// `data:image/*;base64,` MIME — the caller is trusted to produce PNG.
fn decode_data_url_png(data_url: &str) -> Option<Vec<u8>> {
    let comma = data_url.find(',')?;
    let header = &data_url[..comma];
    if !header.starts_with("data:image/") || !header.ends_with(";base64") {
        return None;
    }
    let payload = &data_url[comma + 1..];
    base64_decode(payload).ok()
}

/// Minimal standard-base64 decoder (RFC 4648). Skips whitespace, requires the
/// canonical alphabet. Avoids adding a base64 crate dep just for screenshots.
fn base64_decode(input: &str) -> Result<Vec<u8>, &'static str> {
    fn val(b: u8) -> Result<u8, &'static str> {
        match b {
            b'A'..=b'Z' => Ok(b - b'A'),
            b'a'..=b'z' => Ok(b - b'a' + 26),
            b'0'..=b'9' => Ok(b - b'0' + 52),
            b'+' => Ok(62),
            b'/' => Ok(63),
            _ => Err("invalid base64 char"),
        }
    }
    let bytes: Vec<u8> = input.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    let mut stripped: &[u8] = &bytes;
    while stripped.last() == Some(&b'=') {
        stripped = &stripped[..stripped.len() - 1];
    }
    let mut out = Vec::with_capacity(stripped.len() * 3 / 4);
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;
    for &b in stripped {
        let v = val(b)? as u32;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1u32 << bits) - 1;
        }
    }
    Ok(out)
}

/// Atomically save PNG bytes to `<comments_dir>/screenshots/<id>.png`. Returns
/// the absolute path written. Atomic: tmp file → rename.
fn save_screenshot_bytes(comments_dir: &str, id: &str, bytes: &[u8]) -> std::io::Result<String> {
    let screenshots_dir = format!("{comments_dir}/screenshots");
    std::fs::create_dir_all(&screenshots_dir)?;
    let final_path = format!("{screenshots_dir}/{id}.png");
    let tmp_path = format!("{final_path}.tmp");
    std::fs::write(&tmp_path, bytes)?;
    std::fs::rename(&tmp_path, &final_path)?;
    // Best-effort canonicalize for absolute path; fall back to the raw join.
    let abs = std::fs::canonicalize(&final_path)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or(final_path);
    Ok(abs)
}

/// Save raw PNG bytes for an existing annotation. Returns the absolute path.
/// Used when the frontend captures a screenshot AFTER the annotation row
/// already exists (e.g. via a separate "Capture" action).
#[tauri::command]
pub fn save_annotation_screenshot(
    id: String,
    png_bytes: Vec<u8>,
    state: State<AppState>,
) -> Result<String, String> {
    let app = state.app.lock().map_err(|e| e.to_string())?;
    let dir = app.tab().comments_dir();
    let path = save_screenshot_bytes(&dir, &id, &png_bytes).map_err(|e| e.to_string())?;

    // Patch the annotation row's `screenshot_path` so the UI picks it up.
    let mut anns = er_engine::ai::load_ui_annotations(&dir);
    if let Some(a) = anns.iter_mut().find(|a| a.id == id) {
        a.screenshot_path = Some(path.clone());
        er_engine::ai::save_ui_annotations(&dir, &anns).map_err(|e| e.to_string())?;
        state.desktop_revision.fetch_add(1, Ordering::Relaxed);
    }
    Ok(path)
}

/// Read a saved screenshot back as a `data:image/png;base64,<...>` URL so the
/// frontend can render it without configuring the Tauri asset protocol.
#[tauri::command]
pub fn read_annotation_screenshot(path: String) -> Result<String, String> {
    let bytes = std::fs::read(&path).map_err(|e| format!("read {path}: {e}"))?;
    Ok(format!("data:image/png;base64,{}", base64_encode(&bytes)))
}

/// Minimal standard-base64 encoder. Mirrors `base64_decode`.
fn base64_encode(input: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    let chunks = input.chunks(3);
    for c in chunks {
        let b0 = c[0];
        let b1 = c.get(1).copied().unwrap_or(0);
        let b2 = c.get(2).copied().unwrap_or(0);
        out.push(A[(b0 >> 2) as usize] as char);
        out.push(A[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        if c.len() > 1 {
            out.push(A[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if c.len() > 2 {
            out.push(A[(b2 & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

#[tauri::command]
pub fn delete_ui_annotation(id: String, state: State<AppState>) -> Result<AppSnapshot, String> {
    let app = state.app.lock().map_err(|e| e.to_string())?;
    let dir = app.tab().comments_dir();
    let mut anns = er_engine::ai::load_ui_annotations(&dir);
    anns.retain(|a| a.id != id);
    er_engine::ai::save_ui_annotations(&dir, &anns).map_err(|e| e.to_string())?;
    state.desktop_revision.fetch_add(1, Ordering::Relaxed);
    Ok(snap_from(&app, &state))
}

#[tauri::command]
pub fn clear_ui_annotations_for_page(
    page_url: String,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let app = state.app.lock().map_err(|e| e.to_string())?;
    let dir = app.tab().comments_dir();
    let anns = er_engine::ai::load_ui_annotations(&dir);
    let removed: Vec<_> = anns
        .iter()
        .filter(|a| a.url == page_url)
        .filter_map(|a| a.screenshot_path.as_deref())
        .collect();
    for path in removed {
        let _ = std::fs::remove_file(path);
    }
    let kept: Vec<_> = anns.into_iter().filter(|a| a.url != page_url).collect();
    er_engine::ai::save_ui_annotations(&dir, &kept).map_err(|e| e.to_string())?;
    state.desktop_revision.fetch_add(1, Ordering::Relaxed);
    Ok(snap_from(&app, &state))
}

#[tauri::command]
pub fn clear_ui_annotations(state: State<AppState>) -> Result<AppSnapshot, String> {
    let app = state.app.lock().map_err(|e| e.to_string())?;
    let dir = app.tab().comments_dir();
    let anns = er_engine::ai::load_ui_annotations(&dir);
    for path in anns.iter().filter_map(|a| a.screenshot_path.as_deref()) {
        let _ = std::fs::remove_file(path);
    }
    er_engine::ai::save_ui_annotations(&dir, &[]).map_err(|e| e.to_string())?;
    state.desktop_revision.fetch_add(1, Ordering::Relaxed);
    Ok(snap_from(&app, &state))
}

#[derive(serde::Deserialize)]
pub struct AnchorUpdate {
    pub id: String,
    pub fresh: bool,
    #[serde(default)]
    pub new_box: Option<[f64; 4]>,
}

/// Apply a list of anchor updates to the annotations file in `dir`. Pure I/O
/// helper exposed for tests; the Tauri command is a thin wrapper.
pub(crate) fn apply_anchor_updates(dir: &str, updates: &[AnchorUpdate]) -> std::io::Result<()> {
    let mut anns = er_engine::ai::load_ui_annotations(dir);
    for upd in updates {
        if let Some(a) = anns.iter_mut().find(|a| a.id == upd.id) {
            a.stale = !upd.fresh;
            if let Some(b) = upd.new_box {
                a.box_x = b[0];
                a.box_y = b[1];
                a.box_w = b[2];
                a.box_h = b[3];
            }
        }
    }
    er_engine::ai::save_ui_annotations(dir, &anns)
}

#[tauri::command]
pub fn update_ui_annotation_anchors(
    updates: Vec<AnchorUpdate>,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let app = state.app.lock().map_err(|e| e.to_string())?;
    let dir = app.tab().comments_dir();
    apply_anchor_updates(&dir, &updates).map_err(|e| e.to_string())?;
    Ok(snap_from(&app, &state))
}

#[tauri::command]
pub fn list_ui_annotations(
    url: Option<String>,
    state: State<AppState>,
) -> Result<Vec<er_engine::ai::UiAnnotation>, String> {
    let app = state.app.lock().map_err(|e| e.to_string())?;
    let dir = app.tab().comments_dir();
    let mut anns = er_engine::ai::load_ui_annotations(&dir);
    if let Some(filter_url) = url {
        anns.retain(|a| a.url == filter_url);
    }
    Ok(anns)
}

/// Minimal RFC3339-ish timestamp. Avoids pulling chrono just for this.
fn chrono_like_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Best-effort UTC: seconds-since-epoch wrapped in a sortable string.
    format!("{secs}")
}

#[tauri::command]
pub async fn poll(state: State<'_, AppState>) -> Result<PollResponse, String> {
    let state = state.inner().clone();
    run_blocking(move || poll_impl(&state)).await
}

fn poll_impl(state: &AppState) -> Result<PollResponse, String> {
    let t0 = std::time::Instant::now();
    let lock_t0 = std::time::Instant::now();
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let lock_wait_ms = lock_t0.elapsed().as_millis();
    // Drain pending agent log entries and check for completed commands.
    app.drain_agent_log();
    // Consume completed command receivers — updates command_status to done/failed
    // and emits completion log entries; also resets last_ai_check on successful
    // review so the .er reload below picks up freshly written files.
    app.check_commands();
    // Same lifecycle for app-level background tasks (cross-tab reviews).
    // Only log poll diagnostics when there's actually a task in flight to avoid
    // flooding stderr every 2 seconds during normal use.
    let pre = app.background_task_snapshots().len();
    let debug_bg = er_engine::app::debug_bg_enabled() && pre > 0;
    if debug_bg {
        eprintln!("[bg] poll: pre poll_background_tasks snapshots={pre}");
    }
    app.poll_background_tasks();
    let post = app.background_task_snapshots().len();
    if debug_bg || (er_engine::app::debug_bg_enabled() && post > 0) {
        eprintln!("[bg] poll: post poll_background_tasks snapshots={post}");
    }
    process_ai_task_inbox(&app, state);
    // Drain again so completion/failure log entries are visible in this poll.
    app.drain_agent_log();
    // Check if .er/ AI files changed — cheap mtime check, reloads AI state if yes
    app.tab_mut().check_ai_files_changed();

    let desktop_rev = state.desktop_revision.load(Ordering::Relaxed);
    let content_revision = compute_content_revision(&app);
    let chrome_revision = compute_chrome_revision(state);
    let reviewed_revision = app.tab().reviewed_revision;
    let revision = content_revision.max(chrome_revision);
    let last_content = state.last_sent_content_revision.load(Ordering::Relaxed);
    let last_chrome = state.last_sent_chrome_revision.load(Ordering::Relaxed);
    let last_reviewed = state.last_sent_reviewed_revision.load(Ordering::Relaxed);

    // Nothing changed — return early without a snapshot.
    if content_revision == last_content
        && chrome_revision == last_chrome
        && reviewed_revision == last_reviewed
    {
        crate::profile_log::profile_log(
            "poll_skip",
            &[
                ("revision", revision.to_string()),
                ("content_revision", content_revision.to_string()),
                ("chrome_revision", chrome_revision.to_string()),
                ("reviewed_revision", reviewed_revision.to_string()),
                ("desktop_rev", desktop_rev.to_string()),
                ("lock_wait_ms", lock_wait_ms.to_string()),
                ("poll_ms", t0.elapsed().as_millis().to_string()),
            ],
        );
        return Ok(PollResponse {
            revision,
            content_revision,
            chrome_revision,
            reviewed_revision,
            chrome_only: false,
            snapshot: None,
        });
    }

    // Reviewed-only change: return snapshot=null + chrome_only=true so the frontend
    // knows the count updated without replacing hunk data (avoids jank on checkmarks).
    if content_revision == last_content
        && chrome_revision == last_chrome
        && reviewed_revision != last_reviewed
    {
        state
            .last_sent_reviewed_revision
            .store(reviewed_revision, Ordering::Relaxed);
        crate::profile_log::profile_log(
            "poll_reviewed_only",
            &[
                ("reviewed_revision", reviewed_revision.to_string()),
                ("poll_ms", t0.elapsed().as_millis().to_string()),
            ],
        );
        return Ok(PollResponse {
            revision,
            content_revision,
            chrome_revision,
            reviewed_revision,
            chrome_only: true,
            snapshot: None,
        });
    }

    let chrome_only = content_revision == last_content && chrome_revision != last_chrome;

    crate::profile_log::profile_log(
        "poll_revision_change",
        &[
            ("old_content", last_content.to_string()),
            ("new_content", content_revision.to_string()),
            ("old_chrome", last_chrome.to_string()),
            ("new_chrome", chrome_revision.to_string()),
            (
                "chrome_only",
                if chrome_only { "1" } else { "0" }.to_string(),
            ),
            ("reviewed_revision", reviewed_revision.to_string()),
            ("desktop_rev", desktop_rev.to_string()),
            (
                "diff_hash",
                if app.tab().diff_hash.is_empty() {
                    "empty".to_string()
                } else {
                    app.tab().diff_hash.chars().take(12).collect()
                },
            ),
        ],
    );

    let snapshot = if chrome_only {
        chrome_snap_from(&app, state)
    } else {
        snap_from(&app, state)
    };
    state
        .last_sent_content_revision
        .store(content_revision, Ordering::Relaxed);
    state
        .last_sent_chrome_revision
        .store(chrome_revision, Ordering::Relaxed);
    // Always sync reviewed_revision so a simultaneous content+reviewed change
    // doesn't fire a spurious reviewed-only poll next tick.
    state
        .last_sent_reviewed_revision
        .store(reviewed_revision, Ordering::Relaxed);

    crate::profile_log::profile_log(
        "poll",
        &[
            ("poll_ms", t0.elapsed().as_millis().to_string()),
            ("revision", revision.to_string()),
            ("content_revision", content_revision.to_string()),
            ("chrome_revision", chrome_revision.to_string()),
            ("reviewed_revision", reviewed_revision.to_string()),
            (
                "chrome_only",
                if chrome_only { "1" } else { "0" }.to_string(),
            ),
            ("desktop_rev", desktop_rev.to_string()),
            ("lock_wait_ms", lock_wait_ms.to_string()),
            ("files", snapshot.files.len().to_string()),
            ("threads", snapshot.ai.threads.len().to_string()),
        ],
    );
    Ok(PollResponse {
        revision,
        content_revision,
        chrome_revision,
        reviewed_revision,
        chrome_only,
        snapshot: Some(snapshot),
    })
}

fn compute_chrome_revision(state: &AppState) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hash;

    let mut h = DefaultHasher::new();
    crate::snapshot::meta_cache_fingerprint(&state.meta_cache).hash(&mut h);
    crate::snapshot::pr_cache_fingerprint(Some(&state.pr_cache), Some(&state.pr_cache_fetched_at))
        .hash(&mut h);
    if let Ok(g) = state.gh_status_cache.lock() {
        let mut keys: Vec<_> = g.keys().collect();
        keys.sort();
        for k in keys {
            k.hash(&mut h);
            if let Some(v) = g.get(k) {
                v.review_decision.hash(&mut h);
                v.mergeable.hash(&mut h);
                v.checks.len().hash(&mut h);
                v.state.hash(&mut h);
            }
        }
    }
    if let Ok(w) = state.watch_status.lock() {
        w.active.hash(&mut h);
        w.branch.hash(&mut h);
        w.root_path.hash(&mut h);
    }
    if let Ok(inbox) = state.inbox.lock() {
        inbox.unread_count().hash(&mut h);
        inbox.last_refresh_ms.hash(&mut h);
    }
    state.desktop_revision.load(Ordering::Relaxed).hash(&mut h);
    crate::profile_log::finish_hash(h)
}

fn compute_content_revision(app: &App) -> u64 {
    let tab = app.tab();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    app.active_tab.hash(&mut h);
    tab.diff_hash.hash(&mut h);
    tab.branch_diff_hash.hash(&mut h);
    tab.current_branch.hash(&mut h);
    tab.base_branch.hash(&mut h);
    tab.selected_file.hash(&mut h);
    tab.current_hunk.hash(&mut h);
    tab.files.len().hash(&mut h);
    tab.filter_expr.hash(&mut h);
    tab.ai
        .questions
        .as_ref()
        .map(|q| q.questions.len())
        .unwrap_or(0)
        .hash(&mut h);
    tab.ai
        .notes
        .as_ref()
        .map(|n| n.notes.len())
        .unwrap_or(0)
        .hash(&mut h);
    tab.ai
        .github_comments
        .as_ref()
        .map(|g| g.comments.len())
        .unwrap_or(0)
        .hash(&mut h);
    if let Some(qs) = &tab.ai.questions {
        if let Some(last) = qs.questions.last() {
            last.id.hash(&mut h);
            last.timestamp.hash(&mut h);
            last.resolved.hash(&mut h);
        }
    }
    if let Some(ns) = &tab.ai.notes {
        if let Some(last) = ns.notes.last() {
            last.id.hash(&mut h);
            last.timestamp.hash(&mut h);
            last.resolved.hash(&mut h);
        }
    }
    if let Some(gc) = &tab.ai.github_comments {
        if let Some(last) = gc.comments.last() {
            last.id.hash(&mut h);
            last.timestamp.hash(&mut h);
            last.synced.hash(&mut h);
            last.resolved.hash(&mut h);
        }
    }
    if let Some(review) = &tab.ai.review {
        review.diff_hash.hash(&mut h);
        review.files.len().hash(&mut h);
    }
    // Agent command status changes (e.g. running → done) must trigger a snapshot.
    for (name, status) in &tab.command_status {
        name.hash(&mut h);
        match status {
            er_engine::app::CommandStatus::Running => 0u8.hash(&mut h),
            er_engine::app::CommandStatus::Done => 1u8.hash(&mut h),
            er_engine::app::CommandStatus::Failed(_) => 2u8.hash(&mut h),
        }
    }
    tab.agent_log.len().hash(&mut h);
    if let Some(last) = tab.agent_log.back() {
        last.text.hash(&mut h);
    }
    crate::profile_log::finish_hash(h)
}

// ── Terminal (in-app shell drawer) ───────────────────────────────────────────
//
// Each frontend `Terminal.svelte` instance owns a `session_id` (typically
// `tab-<idx>`). On mount it calls `terminal_spawn`, then streams keystrokes via
// `terminal_write` and resize events via `terminal_resize`. Output is pushed
// back as a Tauri event (`terminal-output`) so we don't poll. On unmount,
// `terminal_close` removes the session from the map — `PtySession::drop` then
// kills the child shell.

#[derive(serde::Serialize, Clone)]
struct TerminalOutputPayload {
    session_id: String,
    bytes: Vec<u8>,
}

#[derive(serde::Serialize, Clone)]
struct TerminalExitPayload {
    session_id: String,
}

#[tauri::command]
pub fn terminal_spawn(
    session_id: String,
    cwd: String,
    app_handle: tauri::AppHandle,
    state: State<AppState>,
) -> Result<(), String> {
    use std::io::Read;
    use tauri::Emitter;

    // Idempotent spawn: keep an existing PTY alive (hide/show must not kill shells).
    {
        let map = state.terminals.lock().map_err(|e| e.to_string())?;
        if map.contains_key(&session_id) {
            return Ok(());
        }
    }

    let (session, mut reader) =
        crate::terminal::PtySession::spawn(&cwd).map_err(|e| e.to_string())?;
    {
        let mut map = state.terminals.lock().map_err(|e| e.to_string())?;
        map.insert(session_id.clone(), session);
    }

    let handle = app_handle.clone();
    let sid = session_id.clone();
    let terminals_for_thread = Arc::clone(&state.terminals);
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => {
                    let _ = handle.emit(
                        "terminal-exit",
                        TerminalExitPayload {
                            session_id: sid.clone(),
                        },
                    );
                    if let Ok(mut map) = terminals_for_thread.lock() {
                        map.remove(&sid);
                    }
                    break;
                }
                Ok(n) => {
                    let _ = handle.emit(
                        "terminal-output",
                        TerminalOutputPayload {
                            session_id: sid.clone(),
                            bytes: buf[..n].to_vec(),
                        },
                    );
                }
                Err(_) => {
                    let _ = handle.emit(
                        "terminal-exit",
                        TerminalExitPayload {
                            session_id: sid.clone(),
                        },
                    );
                    if let Ok(mut map) = terminals_for_thread.lock() {
                        map.remove(&sid);
                    }
                    break;
                }
            }
        }
    });

    Ok(())
}

#[tauri::command]
pub fn terminal_write(
    session_id: String,
    bytes: Vec<u8>,
    state: State<AppState>,
) -> Result<(), String> {
    let mut map = state.terminals.lock().map_err(|e| e.to_string())?;
    let session = map
        .get_mut(&session_id)
        .ok_or_else(|| format!("no terminal session: {session_id}"))?;
    session.write(&bytes).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn terminal_resize(
    session_id: String,
    rows: u16,
    cols: u16,
    state: State<AppState>,
) -> Result<(), String> {
    let mut map = state.terminals.lock().map_err(|e| e.to_string())?;
    let session = map
        .get_mut(&session_id)
        .ok_or_else(|| format!("no terminal session: {session_id}"))?;
    session.resize(rows, cols).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn terminal_close(session_id: String, state: State<AppState>) -> Result<(), String> {
    let mut map = state.terminals.lock().map_err(|e| e.to_string())?;
    map.remove(&session_id);
    Ok(())
}

// ── Dev-URL detection ─────────────────────────────────────────────────────────

/// Pure helper: given the textual contents of a `package.json`, return the
/// best-guess local dev-server URL. Returns the Vite default if a
/// `package.json` was found but no signal matched.
fn detect_dev_url_from_package_json(text: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(text).ok()?;
    let scripts = json.get("scripts").and_then(|v| v.as_object());
    let script = scripts
        .and_then(|s| s.get("dev").and_then(|v| v.as_str()))
        .or_else(|| scripts.and_then(|s| s.get("start").and_then(|v| v.as_str())));

    if let Some(cmd) = script {
        // Explicit port flag wins.
        if let Some(port) = explicit_port(cmd) {
            return Some(format!("http://localhost:{port}"));
        }
        if let Some(port) = port_from_command(cmd) {
            return Some(format!("http://localhost:{port}"));
        }
    }
    // package.json present but no signal — fall back to Vite default.
    Some("http://localhost:5173".to_string())
}

/// Scan a script string for `--port N` or `-p N` and return N.
fn explicit_port(cmd: &str) -> Option<u16> {
    let tokens: Vec<&str> = cmd.split_whitespace().collect();
    let mut i = 0;
    while i < tokens.len() {
        let t = tokens[i];
        // --port=N
        if let Some(rest) = t.strip_prefix("--port=") {
            if let Ok(n) = rest.parse() {
                return Some(n);
            }
        }
        if let Some(rest) = t.strip_prefix("-p=") {
            if let Ok(n) = rest.parse() {
                return Some(n);
            }
        }
        // --port N  /  -p N
        if (t == "--port" || t == "-p") && i + 1 < tokens.len() {
            if let Ok(n) = tokens[i + 1].parse() {
                return Some(n);
            }
        }
        i += 1;
    }
    None
}

/// Recognize common dev-server commands and return their conventional port.
fn port_from_command(cmd: &str) -> Option<u16> {
    let c = cmd.to_lowercase();
    // Order matters: check longer/more-specific patterns first.
    if c.contains("next dev") {
        return Some(3000);
    }
    if c.contains("astro dev") {
        return Some(4321);
    }
    if c.contains("webpack-dev-server") || c.contains("webpack serve") {
        return Some(8080);
    }
    if c.contains("rails server") || c.contains("bin/dev") {
        return Some(3000);
    }
    if c.contains("manage.py runserver") {
        return Some(8000);
    }
    if c.contains("fastapi run") {
        return Some(8000);
    }
    if c.contains("bun --hot") || c.contains("bun run --hot") {
        return Some(3000);
    }
    if c.contains("vite") {
        return Some(5173);
    }
    None
}

#[tauri::command]
pub fn detect_dev_url(repo_root: String) -> Result<Option<String>, String> {
    if repo_root.is_empty() {
        return Ok(None);
    }
    let pkg = std::path::Path::new(&repo_root).join("package.json");
    if pkg.exists() {
        let text = std::fs::read_to_string(&pkg).map_err(|e| e.to_string())?;
        return Ok(detect_dev_url_from_package_json(&text));
    }
    // No package.json — caller may extend later (pyproject.toml / Cargo.toml).
    Ok(None)
}

/// Return the recent log entries for a specific background task.
/// Returns an empty list if the task is not found (may have been reaped).
#[tauri::command]
pub fn get_background_task_log(
    task_id: String,
    state: State<AppState>,
) -> Result<Vec<AgentLogSnapshot>, String> {
    let app = state.app.lock().map_err(|e| e.to_string())?;
    let entries = app.background_task_log_tail(&task_id);
    let log: Vec<AgentLogSnapshot> = entries
        .iter()
        .map(|e| AgentLogSnapshot {
            command_name: e.command_name.clone(),
            source: match &e.source {
                er_engine::app::AgentLogSource::Stdout => "stdout".to_string(),
                er_engine::app::AgentLogSource::Stderr => "stderr".to_string(),
                er_engine::app::AgentLogSource::Status => "status".to_string(),
            },
            text: e.text.clone(),
        })
        .collect();
    Ok(log)
}

#[cfg(test)]
mod tests {
    use super::*;
    use er_engine::ai::{
        load_ui_annotations, save_ui_annotations, ErGitHubComments, ErQuestions,
        GitHubReviewComment, ReviewQuestion, UiAnnotation,
    };

    fn ann(id: &str) -> UiAnnotation {
        UiAnnotation {
            id: id.into(),
            url: "/x".into(),
            selector: Some(format!("#{id}")),
            box_x: 1.0,
            box_y: 2.0,
            box_w: 10.0,
            box_h: 20.0,
            viewport_w: 800,
            viewport_h: 600,
            text: "hi".into(),
            timestamp: "t".into(),
            author: "You".into(),
            screenshot_path: None,
            stale: false,
            element_context: None,
            dom_context: None,
        }
    }

    // Serialize env-var-touching tests to avoid races on parallel runners.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn with_fake_claude<R>(value: &str, f: impl FnOnce() -> R) -> R {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev = std::env::var("ER_FAKE_CLAUDE").ok();
        std::env::set_var("ER_FAKE_CLAUDE", value);
        let out = f();
        match prev {
            Some(v) => std::env::set_var("ER_FAKE_CLAUDE", v),
            None => std::env::remove_var("ER_FAKE_CLAUDE"),
        }
        out
    }

    #[test]
    fn resolve_review_scope_accepts_pr_diff_mode() {
        let mut tab = er_engine::app::TabState::new_for_test(vec![]);
        tab.mode = er_engine::app::DiffMode::PrDiff;
        tab.pr_number = Some(42);
        assert_eq!(resolve_review_scope("branch", &tab).unwrap(), "branch");
        assert_eq!(resolve_review_scope("current", &tab).unwrap(), "branch");
        assert_eq!(resolve_review_scope("pr", &tab).unwrap(), "branch");
    }

    #[test]
    fn resolve_review_scope_accepts_tour_mode() {
        // Guide (tour) reviews the branch diff it was regrouped from.
        let mut tab = er_engine::app::TabState::new_for_test(vec![]);
        tab.mode = er_engine::app::DiffMode::Tour;
        assert_eq!(resolve_review_scope("branch", &tab).unwrap(), "branch");
        assert_eq!(resolve_review_scope("current", &tab).unwrap(), "branch");
    }

    /// Insert directly into the cache map for read-side gating tests, bypassing
    /// `remember_pr_open_entry` so the test never triggers a real disk write.
    fn insert_pr_open_test_entry(
        cache: &Arc<Mutex<HashMap<PrOpenCacheKey, PrOpenCacheEntry>>>,
        key: PrOpenCacheKey,
        freshness: PrOpenFreshness,
        raw_diff: String,
    ) {
        cache.lock().unwrap().insert(
            key,
            PrOpenCacheEntry {
                freshness,
                raw_diff,
                pr_data: None,
                pr_commits: None,
                last_touched: 0,
            },
        );
    }

    #[test]
    fn pr_open_cache_returns_matching_fresh_diff() {
        let cache = Arc::new(Mutex::new(HashMap::new()));
        let key = pr_open_cache_key("p1", "/repo", 1112);
        let freshness = PrOpenFreshness {
            base_branch: "main".into(),
            head_branch: "feature".into(),
            head_oid: "abc".into(),
            updated_at: "2026-05-17T10:00:00Z".into(),
        };
        insert_pr_open_test_entry(&cache, key.clone(), freshness.clone(), "diff --git".into());

        assert_eq!(
            cached_pr_open_diff(&cache, &key, &freshness).as_deref(),
            Some("diff --git")
        );
    }

    #[test]
    fn pr_open_cache_rejects_stale_freshness() {
        let cache = Arc::new(Mutex::new(HashMap::new()));
        let key = pr_open_cache_key("p1", "/repo", 1112);
        let freshness = PrOpenFreshness {
            base_branch: "main".into(),
            head_branch: "feature".into(),
            head_oid: "abc".into(),
            updated_at: "2026-05-17T10:00:00Z".into(),
        };
        insert_pr_open_test_entry(&cache, key.clone(), freshness.clone(), "old diff".into());

        let stale_probe = PrOpenFreshness {
            head_oid: "def".into(),
            ..freshness
        };
        assert!(cached_pr_open_diff(&cache, &key, &stale_probe).is_none());
    }

    #[test]
    fn pr_open_entry_renders_stale_head_but_rejects_rebase() {
        // J1: when the head moved on origin (stale-by-head), the hint open is still a
        // cache hit — we render the diff we already hold and let the 30s pill flag it.
        // The lookup must return the ENTRY's own freshness (the oid the cached diff
        // was built against), not the newer requested oid; seeding the staleness probe
        // with the requested oid would suppress the pill and show a stale diff silently.
        let cache = Arc::new(Mutex::new(HashMap::new()));
        let key = pr_open_cache_key("p1", "/repo", 1112);
        let entry_freshness = PrOpenFreshness {
            base_branch: "main".into(),
            head_branch: "feature".into(),
            head_oid: "OLD".into(),
            updated_at: "2026-05-17T10:00:00Z".into(),
        };
        insert_pr_open_test_entry(
            &cache,
            key.clone(),
            entry_freshness.clone(),
            "cached diff".into(),
        );

        // Head advanced (OLD → NEW), same base → HIT returning the entry's OLD oid.
        let requested_new_head = PrOpenFreshness {
            head_oid: "NEW".into(),
            ..entry_freshness.clone()
        };
        let (raw_diff, returned_freshness, pr_data, pr_commits) =
            cached_pr_open_entry(&cache, &key, &requested_new_head)
                .expect("stale-by-head with matching base must be a cache hit");
        assert_eq!(raw_diff, "cached diff");
        assert_eq!(
            returned_freshness.head_oid, "OLD",
            "must return the entry's own oid so the staleness pill lights on the moved head"
        );
        assert_eq!(returned_freshness.base_branch, "main");
        assert!(pr_data.is_none(), "no pr_data was cached");
        assert!(pr_commits.is_none(), "no pr_commits were cached");

        // Base retargeted (main → develop) → MISS: the pill watches only head_oid, so
        // a silently re-based diff would render with no warning. Re-fetch instead.
        let requested_rebased = PrOpenFreshness {
            base_branch: "develop".into(),
            ..entry_freshness
        };
        assert!(
            cached_pr_open_entry(&cache, &key, &requested_rebased).is_none(),
            "a base retarget must be a hard cache miss"
        );
    }

    #[test]
    fn freshness_from_pr_info_matches_hint_open() {
        // `sync_pr` persists the refreshed diff under `freshness_from_pr_info`. The
        // hint open (base-only gate) renders it from cache and seeds the staleness
        // pill with the entry's `head_oid`, so that oid must equal what the open path
        // computes — `freshness_from_hint` (sidebar) and the equivalent `gh pr view`
        // (no-hint). This test pins that field-for-field contract: divergence would
        // falsely light the pill on a clean post-sync reopen.
        let pr = PrInfo {
            number: 1112,
            title: "Add feature".into(),
            head_ref: "feature".into(),
            state: "OPEN".into(),
            is_draft: false,
            author: "alice".into(),
            assignees: vec![],
            reviewers: vec![],
            checks_state: None,
            review_decision: None,
            merged_at: None,
            approved_by_me: false,
            base_ref: "main".into(),
            head_oid: "headoid123".into(),
            updated_at: "2026-05-17T10:00:00Z".into(),
            latest_reviewer_states: vec![],
        };
        let hint = PrOpenHint {
            base_ref: pr.base_ref.clone(),
            head_ref: pr.head_ref.clone(),
            head_oid: pr.head_oid.clone(),
            updated_at: pr.updated_at.clone(),
            title: pr.title.clone(),
            author: String::new(),
        };

        // Same head → identical freshness → the post-sync reopen renders from cache
        // with the staleness pill dark.
        assert_eq!(
            freshness_from_pr_info(&pr, Some(&pr.head_oid)),
            freshness_from_hint(&hint),
            "post-sync freshness must match the sidebar-hint open freshness"
        );

        // The diff's own oid wins (the cached diff was computed against it); an
        // empty/whitespace oid falls back to the PR head oid.
        assert_eq!(
            freshness_from_pr_info(&pr, Some("diffoid999")).head_oid,
            "diffoid999"
        );
        assert_eq!(freshness_from_pr_info(&pr, None).head_oid, "headoid123");
        assert_eq!(
            freshness_from_pr_info(&pr, Some("   ")).head_oid,
            "headoid123"
        );

        // Negative (timestamp): an advanced `updated_at` must be reflected in the
        // freshness value. (The head is unchanged, so the cached diff is identical —
        // a benign clean hit; the pill, which watches head_oid, stays dark.)
        let mut moved = pr.clone();
        moved.updated_at = "2026-05-17T11:00:00Z".into();
        assert_ne!(
            freshness_from_pr_info(&moved, Some(&moved.head_oid)),
            freshness_from_hint(&hint),
            "an advanced updated_at must change the freshness value"
        );

        // Negative (oid): a moved head with the SAME timestamp must still change the
        // freshness — it pins `head_oid`, so a post-sync entry carries the new oid and
        // a reopen lights the stale pill if the head moves again. Guards against a
        // regression that only compared `updated_at`.
        assert_ne!(
            freshness_from_pr_info(&pr, Some("newheadoid456")),
            freshness_from_hint(&hint),
            "a changed head_oid alone must change the freshness"
        );
    }

    #[test]
    fn evict_lru_removes_oldest_touched_when_over_cap() {
        // Fill the cache to the cap with distinct, ascending touch times so the
        // first-inserted key (touch = 1) is the least-recent.
        let mut map: HashMap<PrOpenCacheKey, PrOpenCacheEntry> = HashMap::new();
        let mk = |n: u64| pr_open_cache_key("p1", "/repo", n);
        for n in 0..MAX_PR_OPEN_CACHE_ENTRIES as u64 {
            map.insert(
                mk(n),
                PrOpenCacheEntry {
                    freshness: PrOpenFreshness {
                        base_branch: "main".into(),
                        head_branch: "feature".into(),
                        head_oid: format!("oid-{n}"),
                        updated_at: "2026-05-17T10:00:00Z".into(),
                    },
                    raw_diff: format!("diff-{n}"),
                    pr_data: None,
                    pr_commits: None,
                    last_touched: n + 1,
                },
            );
        }
        assert_eq!(map.len(), MAX_PR_OPEN_CACHE_ENTRIES);
        evict_lru(&mut map); // at cap → no eviction
        assert_eq!(map.len(), MAX_PR_OPEN_CACHE_ENTRIES);

        // One more entry pushes over the cap; the oldest (touch=1, key=0) is evicted.
        map.insert(
            mk(99),
            PrOpenCacheEntry {
                freshness: PrOpenFreshness {
                    base_branch: "main".into(),
                    head_branch: "feature".into(),
                    head_oid: "oid-99".into(),
                    updated_at: "2026-05-17T10:00:00Z".into(),
                },
                raw_diff: "diff-99".into(),
                pr_data: None,
                pr_commits: None,
                last_touched: 1000,
            },
        );
        evict_lru(&mut map);
        assert_eq!(
            map.len(),
            MAX_PR_OPEN_CACHE_ENTRIES,
            "back to cap after one eviction"
        );
        assert!(
            !map.contains_key(&mk(0)),
            "least-recently-touched entry evicted"
        );
        assert!(map.contains_key(&mk(99)), "newly-inserted entry retained");
        assert!(
            map.contains_key(&mk(31)),
            "most-recent of the originals retained"
        );
    }

    #[test]
    fn evict_lru_keeps_recently_read_entry_over_untouched_ones() {
        // An entry that was *read* (touch bumped) must outlive one only written.
        let mut map: HashMap<PrOpenCacheKey, PrOpenCacheEntry> = HashMap::new();
        let mk = |n: u64| pr_open_cache_key("p1", "/repo", n);
        for n in 0..MAX_PR_OPEN_CACHE_ENTRIES as u64 {
            map.insert(
                mk(n),
                PrOpenCacheEntry {
                    freshness: PrOpenFreshness {
                        base_branch: "main".into(),
                        head_branch: "feature".into(),
                        head_oid: format!("oid-{n}"),
                        updated_at: "2026-05-17T10:00:00Z".into(),
                    },
                    raw_diff: format!("diff-{n}"),
                    pr_data: None,
                    pr_commits: None,
                    last_touched: n + 1,
                },
            );
        }
        // Bump key 0 to be the most-recent (simulating a read hit), then overflow.
        map.get_mut(&mk(0)).unwrap().last_touched = 10_000;
        map.insert(
            mk(99),
            PrOpenCacheEntry {
                freshness: PrOpenFreshness {
                    base_branch: "main".into(),
                    head_branch: "feature".into(),
                    head_oid: "oid-99".into(),
                    updated_at: "2026-05-17T10:00:00Z".into(),
                },
                raw_diff: "diff-99".into(),
                pr_data: None,
                pr_commits: None,
                last_touched: 10_001,
            },
        );
        evict_lru(&mut map);
        assert!(
            map.contains_key(&mk(0)),
            "recently-read entry survives eviction"
        );
        // The least-recent untouched entry (touch=2, key=1) is the one evicted.
        assert!(!map.contains_key(&mk(1)), "oldest untouched entry evicted");
    }

    #[test]
    fn open_source_policy_allows_only_checked_out_local_contexts() {
        // Working tree tab
        assert!(allows_local_open(false, false, false));
        // Remote PR tab
        assert!(!allows_local_open(true, false, false));
        // Local branch/PR view without checkout root
        assert!(!allows_local_open(false, true, false));
        // Local branch view with checkout root (tracked branch checked out)
        assert!(allows_local_open(false, true, true));
    }

    #[test]
    fn card_ai_subprocess_honors_fake_sentinel_ok() {
        let inv = CardAiInvocation {
            command: "claude".into(),
            args: vec![],
            work_dir: "/tmp".into(),
            is_claude_compatible: true,
            uses_stream_json: false,
            env: vec![],
        };
        let body = with_fake_claude("ok", || {
            run_card_ai_subprocess(&inv, "ctx", "prompt", Some("sonnet"))
        });
        assert_eq!(body, "mocked ok");
    }

    #[test]
    fn card_ai_subprocess_honors_fake_sentinel_fail() {
        let inv = CardAiInvocation {
            command: "claude".into(),
            args: vec![],
            work_dir: "/tmp".into(),
            is_claude_compatible: true,
            uses_stream_json: false,
            env: vec![],
        };
        let body = with_fake_claude("fail", || {
            run_card_ai_subprocess(&inv, "ctx", "prompt", Some("sonnet"))
        });
        assert!(
            body.starts_with("Pending — invoke via CLI"),
            "expected fallback message, got: {body}"
        );
    }

    #[test]
    fn card_ai_subprocess_returns_custom_sentinel_value() {
        let inv = CardAiInvocation {
            command: "claude".into(),
            args: vec![],
            work_dir: "/tmp".into(),
            is_claude_compatible: true,
            uses_stream_json: false,
            env: vec![],
        };
        let body = with_fake_claude("custom-response-text", || {
            run_card_ai_subprocess(&inv, "ctx", "prompt", Some("sonnet"))
        });
        assert_eq!(body, "custom-response-text");
    }

    #[test]
    fn review_submit_validation_rejects_blank() {
        let err = validate_review_submission(0, "").unwrap_err();
        assert!(err.contains("Nothing to submit"));
        assert!(err.contains("private"));
    }

    #[test]
    fn review_submit_validation_accepts_summary_only() {
        validate_review_submission(0, "Looks good overall").unwrap();
    }

    #[test]
    fn review_submit_validation_accepts_comment_batch() {
        validate_review_submission(2, "").unwrap();
    }

    #[test]
    fn own_pr_approval_error_is_detected_from_github_422() {
        let raw = r#"Failed to submit review: gh: Unprocessable Entity (HTTP 422) ({"errors":["Review Can not approve your own pull request"]})"#;
        assert!(is_own_pr_approval_error(raw));
        assert_eq!(
            gh_review_submit_err(anyhow::anyhow!(raw)),
            "GitHub does not allow approving your own pull request."
        );
    }

    #[test]
    fn resolve_thread_updates_question_file() {
        let tmp = tempfile::tempdir().unwrap();
        let q_path = tmp.path().join("questions.json");
        let gc_path = tmp.path().join("github-comments.json");
        let questions = ErQuestions {
            version: 1,
            diff_hash: "x".to_string(),
            questions: vec![ReviewQuestion {
                id: "q-1".to_string(),
                timestamp: "t".to_string(),
                file: "a.rs".to_string(),
                hunk_index: Some(0),
                line_start: Some(1),
                line_end: None,
                line_content: String::new(),
                text: "question".to_string(),
                resolved: false,
                stale: false,
                context_before: vec![],
                context_after: vec![],
                old_line_start: None,
                hunk_header: String::new(),
                anchor_status: "original".to_string(),
                relocated_at_hash: String::new(),
                in_reply_to: None,
                author: "You".to_string(),
                promoted_to: None,
                finding_ref: None,
            }],
        };
        std::fs::write(&q_path, serde_json::to_string_pretty(&questions).unwrap()).unwrap();
        std::fs::write(&gc_path, r#"{"version":1,"diff_hash":"x","comments":[]}"#).unwrap();

        let changed = mark_thread_resolved_in_files(
            "q-1",
            &q_path.to_string_lossy(),
            &tmp.path().join("notes.json").to_string_lossy(),
            &gc_path.to_string_lossy(),
        )
        .unwrap();
        assert!(changed);
        let updated: ErQuestions =
            serde_json::from_str(&std::fs::read_to_string(&q_path).unwrap()).unwrap();
        assert!(updated.questions[0].resolved);
    }

    #[test]
    fn resolve_thread_updates_github_comment_file() {
        let tmp = tempfile::tempdir().unwrap();
        let q_path = tmp.path().join("questions.json");
        let gc_path = tmp.path().join("github-comments.json");
        std::fs::write(&q_path, r#"{"version":1,"diff_hash":"x","questions":[]}"#).unwrap();
        let comments = ErGitHubComments {
            version: 1,
            diff_hash: "x".to_string(),
            github: None,
            comments: vec![GitHubReviewComment {
                id: "c-1".to_string(),
                timestamp: "t".to_string(),
                file: "a.rs".to_string(),
                hunk_index: Some(0),
                line_start: Some(2),
                line_end: None,
                line_content: String::new(),
                comment: "note".to_string(),
                in_reply_to: None,
                resolved: false,
                source: "local".to_string(),
                github_id: None,
                author: "You".to_string(),
                synced: false,
                outdated: false,
                stale: false,
                context_before: vec![],
                context_after: vec![],
                old_line_start: None,
                hunk_header: String::new(),
                anchor_status: "original".to_string(),
                relocated_at_hash: String::new(),
                finding_ref: None,
                side: "RIGHT".to_string(),
            }],
        };
        std::fs::write(&gc_path, serde_json::to_string_pretty(&comments).unwrap()).unwrap();

        let changed = mark_thread_resolved_in_files(
            "c-1",
            &q_path.to_string_lossy(),
            &tmp.path().join("notes.json").to_string_lossy(),
            &gc_path.to_string_lossy(),
        )
        .unwrap();
        assert!(changed);
        let updated: ErGitHubComments =
            serde_json::from_str(&std::fs::read_to_string(&gc_path).unwrap()).unwrap();
        assert!(updated.comments[0].resolved);
    }

    #[test]
    fn save_annotation_screenshot_writes_bytes_and_returns_path() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_string_lossy().to_string();

        // Minimal 1x1 transparent PNG (8-byte signature + IHDR + IDAT + IEND).
        // Bytes don't need to be a valid image for the I/O test — just round-trip.
        let png_bytes: Vec<u8> = vec![
            0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0xDE, 0xAD, 0xBE, 0xEF,
        ];

        let path = save_screenshot_bytes(&dir, "ui-test-42", &png_bytes).unwrap();
        assert!(
            path.ends_with("ui-test-42.png"),
            "path should be the id.png: {path}"
        );
        let read_back = std::fs::read(&path).unwrap();
        assert_eq!(read_back, png_bytes, "saved bytes must match input");

        // Ensure tmp file was cleaned up by the rename.
        let tmp_path = format!("{path}.tmp");
        assert!(
            !std::path::Path::new(&tmp_path).exists(),
            "tmp file must be gone after rename"
        );
    }

    #[test]
    fn decode_data_url_png_roundtrips_through_base64() {
        let payload = b"\x89PNG\r\n\x1a\n hello";
        let encoded = base64_encode(payload);
        let data_url = format!("data:image/png;base64,{encoded}");
        let decoded = decode_data_url_png(&data_url).expect("should decode");
        assert_eq!(decoded, payload);
    }

    #[test]
    fn decode_data_url_png_rejects_non_base64_data_url() {
        assert!(decode_data_url_png("not-a-data-url").is_none());
        assert!(decode_data_url_png("data:text/plain,hello").is_none());
        assert!(decode_data_url_png("data:image/png,plainstuff").is_none());
    }

    fn write_pkg(dir: &std::path::Path, body: &str) {
        std::fs::write(dir.join("package.json"), body).unwrap();
    }

    #[test]
    fn detect_dev_url_vite_default() {
        let tmp = tempfile::tempdir().unwrap();
        write_pkg(tmp.path(), r#"{ "scripts": { "dev": "vite" } }"#);
        let got = detect_dev_url(tmp.path().to_string_lossy().to_string()).unwrap();
        assert_eq!(got.as_deref(), Some("http://localhost:5173"));
    }

    #[test]
    fn detect_dev_url_next() {
        let tmp = tempfile::tempdir().unwrap();
        write_pkg(tmp.path(), r#"{ "scripts": { "dev": "next dev" } }"#);
        let got = detect_dev_url(tmp.path().to_string_lossy().to_string()).unwrap();
        assert_eq!(got.as_deref(), Some("http://localhost:3000"));
    }

    #[test]
    fn detect_dev_url_explicit_port() {
        let tmp = tempfile::tempdir().unwrap();
        write_pkg(
            tmp.path(),
            r#"{ "scripts": { "dev": "vite --port 4200" } }"#,
        );
        let got = detect_dev_url(tmp.path().to_string_lossy().to_string()).unwrap();
        assert_eq!(
            got.as_deref(),
            Some("http://localhost:4200"),
            "explicit --port N should win over framework default"
        );
    }

    #[test]
    fn detect_dev_url_no_package_json() {
        let tmp = tempfile::tempdir().unwrap();
        let got = detect_dev_url(tmp.path().to_string_lossy().to_string()).unwrap();
        assert_eq!(got, None);
    }

    #[test]
    fn detect_dev_url_unknown_command() {
        let tmp = tempfile::tempdir().unwrap();
        write_pkg(
            tmp.path(),
            r#"{ "scripts": { "dev": "node scripts/start.js" } }"#,
        );
        let got = detect_dev_url(tmp.path().to_string_lossy().to_string()).unwrap();
        assert_eq!(
            got.as_deref(),
            Some("http://localhost:5173"),
            "unknown command should fall back to vite default when package.json exists"
        );
    }

    fn make_app_with_n_tabs(n: usize) -> App {
        use er_engine::app::TabState;
        let mut seed = TabState::new_for_test(vec![]);
        seed.repo_root = "tab0".into();
        let mut app = App::new_remote(seed, None);
        for i in 1..n {
            let mut t = TabState::new_for_test(vec![]);
            t.repo_root = format!("tab{i}");
            app.tabs.push(t);
        }
        app
    }

    /// `open_local_branch` with `replace = Some(true)` swaps the active tab's
    /// slot instead of pushing — `tabs.len()` stays constant. We exercise the
    /// shared placement helper (`place_tab`) used by the Tauri command, which
    /// avoids needing a full `State<AppState>` plus on-disk projects.json.
    #[test]
    fn open_local_branch_replace_swaps_active_slot() {
        use er_engine::app::TabState;

        let mut app = make_app_with_n_tabs(2);
        app.active_tab = 1;

        let mut incoming = TabState::new_for_test(vec![]);
        incoming.repo_root = "new".into();
        place_tab(&mut app, incoming, true);

        assert_eq!(app.tabs.len(), 2, "replace must not grow tabs");
        assert_eq!(app.active_tab, 1, "active stays on the replaced slot");
        assert_eq!(app.tabs[1].repo_root, "new", "active slot got new tab");
        assert_eq!(app.tabs[0].repo_root, "tab0", "other tab is untouched");
    }

    #[test]
    fn place_tab_append_pushes_and_focuses() {
        use er_engine::app::TabState;

        let mut app = make_app_with_n_tabs(1);
        app.active_tab = 0;

        let mut incoming = TabState::new_for_test(vec![]);
        incoming.repo_root = "new".into();
        place_tab(&mut app, incoming, false);

        assert_eq!(app.tabs.len(), 2, "append grows tabs by one");
        assert_eq!(app.active_tab, 1, "new tab is focused");
        assert_eq!(app.tabs[1].repo_root, "new");
    }

    #[test]
    fn update_ui_annotation_anchors_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_string_lossy().to_string();
        save_ui_annotations(&dir, &[ann("a1"), ann("a2")]).unwrap();

        let updates = vec![
            AnchorUpdate {
                id: "a1".into(),
                fresh: true,
                new_box: Some([5.0, 6.0, 7.0, 8.0]),
            },
            AnchorUpdate {
                id: "a2".into(),
                fresh: false,
                new_box: None,
            },
        ];
        apply_anchor_updates(&dir, &updates).unwrap();

        let back = load_ui_annotations(&dir);
        let a1 = back.iter().find(|a| a.id == "a1").unwrap();
        assert!(!a1.stale, "a1 should be fresh");
        assert_eq!(
            (a1.box_x, a1.box_y, a1.box_w, a1.box_h),
            (5.0, 6.0, 7.0, 8.0),
            "a1 box should be updated"
        );

        let a2 = back.iter().find(|a| a.id == "a2").unwrap();
        assert!(a2.stale, "a2 should be marked stale");
        assert_eq!(
            (a2.box_x, a2.box_y, a2.box_w, a2.box_h),
            (1.0, 2.0, 10.0, 20.0),
            "a2 box should be unchanged when new_box is None"
        );
    }

    fn make_gh_comment(
        id: &str,
        file: &str,
        line_start: Option<usize>,
        synced: bool,
    ) -> GitHubReviewComment {
        GitHubReviewComment {
            id: id.to_string(),
            timestamp: "t".to_string(),
            file: file.to_string(),
            hunk_index: None,
            line_start,
            line_end: None,
            line_content: String::new(),
            comment: "body".to_string(),
            in_reply_to: None,
            resolved: false,
            source: "local".to_string(),
            github_id: None,
            author: "You".to_string(),
            synced,
            outdated: false,
            stale: false,
            context_before: vec![],
            context_after: vec![],
            old_line_start: None,
            hunk_header: String::new(),
            anchor_status: "original".to_string(),
            relocated_at_hash: String::new(),
            finding_ref: None,
            side: "RIGHT".to_string(),
        }
    }

    /// Verifies that comments without a line anchor are detected as unsubmittable
    /// before any GitHub API call would be made.
    #[test]
    fn submit_review_detects_unanchored_local_comments() {
        let gc = ErGitHubComments {
            version: 1,
            diff_hash: "abc".to_string(),
            github: None,
            comments: vec![
                make_gh_comment("c-1", "src/main.rs", Some(10), false), // has anchor, unsynced
                make_gh_comment("c-2", "src/lib.rs", None, false), // NO anchor, unsynced — the problem
                make_gh_comment("c-3", "src/foo.rs", None, true), // no anchor but already synced — OK
            ],
        };

        let unsubmittable_count = gc
            .comments
            .iter()
            .filter(|c| {
                c.source == "local" && !c.synced && !c.file.is_empty() && c.line_start.is_none()
            })
            .count();

        assert_eq!(
            unsubmittable_count, 1,
            "only the unsynced comment without a line anchor should be flagged"
        );
    }

    /// Verifies that all-anchored unsynced comments produce a zero unsubmittable count,
    /// meaning the validation passes and submission proceeds normally.
    #[test]
    fn submit_review_no_false_positive_when_all_comments_have_anchors() {
        let gc = ErGitHubComments {
            version: 1,
            diff_hash: "abc".to_string(),
            github: None,
            comments: vec![
                make_gh_comment("c-1", "src/main.rs", Some(5), false),
                make_gh_comment("c-2", "src/lib.rs", Some(20), false),
            ],
        };

        let unsubmittable_count = gc
            .comments
            .iter()
            .filter(|c| {
                c.source == "local" && !c.synced && !c.file.is_empty() && c.line_start.is_none()
            })
            .count();

        assert_eq!(
            unsubmittable_count, 0,
            "no unsubmittable comments when all have line anchors"
        );
    }

    /// Regression: `process_ai_task_inbox` must not call `maybe_send_native_notification`
    /// while holding the inbox mutex (non-reentrant lock → permanent deadlock on review done).
    #[test]
    fn ai_review_done_inbox_notify_does_not_deadlock() {
        use std::sync::mpsc;
        use std::time::Duration;

        let inbox: InboxHandle = Arc::new(Mutex::new(crate::inbox::InboxState::default()));
        let app_handle: Arc<Mutex<Option<tauri::AppHandle>>> = Arc::new(Mutex::new(None));

        let item = InboxItem {
            id: "inbox-ai-test:done".to_string(),
            kind: "ai_review_done".to_string(),
            severity: "success".to_string(),
            title: "AI review completed (test-branch)".to_string(),
            body: "test-branch".to_string(),
            source: "ai".to_string(),
            target: InboxTarget {
                project_id: None,
                repo_root: Some("/tmp/repo".to_string()),
                remote: None,
                pr_number: None,
                branch: Some("test-branch".to_string()),
                url: None,
            },
            created_at_ms: 0,
            read_at_ms: None,
            dedupe_key: "ai:test-task:done".to_string(),
        };

        let inbox_thread = Arc::clone(&inbox);
        let handle_thread = Arc::clone(&app_handle);
        let item_thread = item.clone();
        let (tx, rx) = mpsc::channel();

        std::thread::spawn(move || {
            let mut just_added = Vec::new();
            if let Ok(mut guard) = inbox_thread.lock() {
                if guard.add_item(item_thread.clone()) {
                    just_added.push(item_thread);
                }
            }
            for added in &just_added {
                maybe_send_native_notification(&inbox_thread, &handle_thread, added);
            }
            let _ = tx.send(());
        });

        rx.recv_timeout(Duration::from_millis(500))
            .expect("inbox notify path deadlocked (re-entrant lock on ai_review_done)");
        assert_eq!(inbox.lock().unwrap().items.len(), 1);
    }
}
