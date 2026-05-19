use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::State;
use tauri_plugin_notification::NotificationExt;

use er_engine::ai::CommentType;
use er_engine::app::{App, DiffMode, InputMode};
use er_engine::highlight::Highlighter;

use crate::pr_cache::PrCacheFetchedAtMap;
use crate::projects;
use crate::snapshot::{
    build_snapshot, AgentLogSnapshot, AppSnapshot, CheckSummary, GhCommentSummary,
    GhReviewSummary, GhStatusCache, GhUser, GithubStatusSnapshot, LoadingState, MetaCache,
    PendingAiReplies, PrInfo, WatchStatusState,
};
use crate::inbox::{InboxHandle, InboxItem, InboxTarget};

const DEFAULT_ASK_AI_PROMPT: &str = "Elaborate on this and answer any question directly.";
const REQUESTED_KINDS: &[&str] = &[
    "ai_review_done",
    "ai_review_failed",
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
    pub revision: u64,
    /// Full snapshot — `None` when the revision is unchanged since the last poll.
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

pub struct AppState {
    pub app: Arc<Mutex<App>>,
    pub highlighter: Mutex<Highlighter>,
    pub pr_cache: Arc<Mutex<HashMap<String, Vec<PrInfo>>>>,
    pub pr_cache_fetched_at: PrCacheFetchedAtMap,
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
    /// Monotonic counter bumped whenever background-owned state changes (caches,
    /// loading flags) so that poll() can detect changes not visible in App state.
    pub desktop_revision: Arc<AtomicU64>,
    /// Last revision included in a poll response. Used to skip snapshot builds
    /// when the app and desktop state are unchanged since the previous poll.
    pub last_sent_revision: Arc<AtomicU64>,
    /// Active-branch watcher status. Read by `build_snapshot` so the UI can
    /// show `Watching` when the desktop watcher is following a checkout.
    pub watch_status: WatchStatusState,
    pub inbox: InboxHandle,
    pub tauri_app_handle: Arc<Mutex<Option<tauri::AppHandle>>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PrOpenCacheKey {
    project_id: String,
    repo_root: String,
    pr_number: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrOpenFreshness {
    base_branch: String,
    head_branch: String,
    head_oid: String,
    updated_at: String,
}

#[derive(Debug, Clone)]
pub struct PrOpenCacheEntry {
    freshness: PrOpenFreshness,
    raw_diff: String,
}

#[derive(Debug, Clone)]
struct PrOpenMetadata {
    freshness: PrOpenFreshness,
    pr_data: er_engine::github::PrOverviewData,
}

struct PrOpenInputs {
    repo_root: String,
    metadata: PrOpenMetadata,
    resolved_base: String,
    raw_diff: String,
    cache_hit: bool,
}

#[tauri::command]
pub fn start_window_drag(window: tauri::Window) -> Result<(), String> {
    window.start_dragging().map_err(|e| e.to_string())
}

macro_rules! snap {
    ($state:expr) => {{
        let app = $state.app.lock().map_err(|e| e.to_string())?;
        let mut hl = $state.highlighter.lock().map_err(|e| e.to_string())?;
        Ok(build_snapshot(
            &app,
            &mut hl,
            Some(&$state.pr_cache),
            Some(&$state.pr_cache_fetched_at),
            Some(&$state.meta_cache),
            Some(&$state.gh_user),
            Some(&$state.pending_ai_replies),
            Some(&$state.gh_status_cache),
        Some(&$state.loading),
        Some(&$state.watch_status),
        Some(&$state.inbox),
    ))
    }};
}

/// Build a snapshot using the lock guards directly (when callers already hold them).
fn snap_from(app: &App, hl: &mut Highlighter, state: &AppState) -> AppSnapshot {
    build_snapshot(
        app,
        hl,
        Some(&state.pr_cache),
        Some(&state.pr_cache_fetched_at),
        Some(&state.meta_cache),
        Some(&state.gh_user),
        Some(&state.pending_ai_replies),
        Some(&state.gh_status_cache),
        Some(&state.loading),
        Some(&state.watch_status),
        Some(&state.inbox),
    )
}

fn log_branch_open_phase(
    project_id: &str,
    branch: &str,
    phase: &str,
    started_at: std::time::Instant,
) {
    log::info!(
        "branch_open project={} branch={} phase={} ms={}",
        project_id,
        branch,
        phase,
        started_at.elapsed().as_millis()
    );
}

fn now_ms() -> u64 {
    crate::inbox::now_epoch_ms()
}

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
    let handle = app_handle_state
        .lock()
        .ok()
        .and_then(|g| g.clone());
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
    desktop_revision.fetch_add(1, Ordering::Relaxed); // in-flight started
    let in_flight_clone = Arc::clone(&in_flight);
    std::thread::spawn(move || {
        let snap = fetch_github_status(&owner, &repo, number);
        if let Some(snap) = snap {
            if let Ok(mut g) = cache.lock() {
                g.insert((owner, repo, number), snap);
            }
        }
        if let Some(loading) = &loading {
            if let Ok(mut flags) = loading.lock() {
                flags.gh_status = false;
            }
        }
        desktop_revision.fetch_add(1, Ordering::Relaxed); // completed (success or miss)
        if let Ok(mut set) = in_flight_clone.lock() {
            set.remove(&key);
        }
    });
}

fn active_github_key(app: &App, state: &AppState) -> Option<(String, String, u64)> {
    let tab = app.tab();
    if let (Some(slug), Some(n)) = (tab.remote_repo.as_ref(), tab.pr_number) {
        return slug
            .split_once('/')
            .map(|(o, r)| (o.to_string(), r.to_string(), n));
    }

    let branch = tab
        .local_branch_view
        .as_deref()
        .unwrap_or(&tab.current_branch)
        .to_string();
    state.pr_cache.lock().ok().and_then(|cache| {
        cache.iter().find_map(|(slug, prs)| {
            prs.iter()
                .filter(|p| p.head_ref == branch)
                .min_by_key(|p| if p.state == "OPEN" { 0 } else { 1 })
                .and_then(|p| {
                    slug.split_once('/')
                        .map(|(o, r)| (o.to_string(), r.to_string(), p.number))
                })
        })
    })
}

fn process_ai_task_inbox(app: &App, state: &AppState) {
    let now = now_ms();
    let tasks = app.background_task_snapshots();
    let mut emitted_any = false;
    if let Ok(mut inbox) = state.inbox.lock() {
        for task in tasks {
            let (kind, severity, title, body) = match task.status.as_str() {
                "done" => (
                    "ai_review_done".to_string(),
                    "success".to_string(),
                    format!("AI review completed ({})", task.target_label),
                    task.label.clone(),
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
                kind: kind.clone(),
                severity,
                title,
                body,
                source: "ai".to_string(),
                target: InboxTarget {
                    project_id: None,
                    repo_root: Some(
                        app.tab()
                            .repo_root
                            .clone(),
                    ),
                    remote: app.tab().remote_repo.clone(),
                    pr_number: app.tab().pr_number,
                    branch: Some(
                        app.tab()
                            .local_branch_view
                            .clone()
                            .unwrap_or_else(|| app.tab().current_branch.clone()),
                    ),
                    url: None,
                },
                created_at_ms: now,
                read_at_ms: None,
                dedupe_key: format!("ai:{}:{}", task.id, task.status),
            };
            if inbox.add_item(item.clone()) {
                emitted_any = true;
                maybe_send_native_notification(&state.inbox, &state.tauri_app_handle, &item);
            }
        }
    }
    if emitted_any {
        crate::inbox::save_inbox_state(&state.inbox);
        state.desktop_revision.fetch_add(1, Ordering::Relaxed);
    }
}

/// Fetch all GitHub status data for a PR. Runs the 4 gh calls in parallel.
/// Returns None when the PR overview fetch fails (e.g. no network, gh not authed).
/// Comments/reviews/checks failures are non-fatal — the snapshot still populates.
pub fn fetch_github_status(owner: &str, repo: &str, number: u64) -> Option<GithubStatusSnapshot> {
    let t = std::time::Instant::now();
    // Run 4 independent gh calls concurrently — cuts wall time from ~3.5s to ~1s.
    let (overview_res, checks, comments, reviews) = std::thread::scope(|s| {
        let o = s.spawn(|| er_engine::github::gh_pr_overview_remote_full(owner, repo, number));
        let c = s.spawn(|| {
            er_engine::github::gh_pr_checks_remote(owner, repo, number).unwrap_or_default()
        });
        let cm = s.spawn(|| {
            er_engine::github::gh_pr_comments_overview(owner, repo, number).unwrap_or_default()
        });
        let r =
            s.spawn(|| er_engine::github::gh_pr_reviews(owner, repo, number).unwrap_or_default());
        (
            o.join().ok(),
            c.join().unwrap_or_default(),
            cm.join().unwrap_or_default(),
            r.join().unwrap_or_default(),
        )
    });
    let overview = overview_res?.ok()?;
    log::info!(
        "gh_status fetch {owner}/{repo}#{number} in {}ms",
        t.elapsed().as_millis()
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
    })
}

/// Kick a background refresh of the active tab's GitHub status.
/// Works for remote PR tabs (remote_repo + pr_number) and for working-tree /
/// local-branch tabs where the viewed branch has an open PR in pr_cache.
fn kick_active_gh_status(app: &App, state: &AppState) {
    if let Some((owner, repo, number)) = active_github_key(app, state) {
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
pub fn get_snapshot(state: State<AppState>) -> Result<AppSnapshot, String> {
    snap!(state)
}

#[tauri::command]
pub fn toggle_panel(panel: String, state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    app.toggle_panel(&panel);
    Ok(snap_from(&app, &mut hl, &*state))
}

// ── Navigation ────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn select_file(idx: usize, state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    {
        let tab = app.tab_mut();
        if idx < tab.files.len() {
            tab.selected_file = idx;
            tab.current_hunk = 0;
            tab.current_line = None;
            tab.diff_scroll = 0;
            tab.h_scroll = 0;
            tab.ensure_file_parsed();
            tab.rebuild_hunk_offsets();
        }
    }
    Ok(snap_from(&app, &mut hl, &*state))
}

#[tauri::command]
pub fn next_file(state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    app.tab_mut().next_file();
    Ok(snap_from(&app, &mut hl, &*state))
}

#[tauri::command]
pub fn prev_file(state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    app.tab_mut().prev_file();
    Ok(snap_from(&app, &mut hl, &*state))
}

#[tauri::command]
pub fn jump_to_unreviewed(state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
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
    Ok(snap_from(&app, &mut hl, &*state))
}

// ── Hunk navigation ──────────────────────────────────────────────────────────

#[tauri::command]
pub fn next_hunk(state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    app.tab_mut().next_hunk();
    Ok(snap_from(&app, &mut hl, &*state))
}

#[tauri::command]
pub fn prev_hunk(state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    app.tab_mut().prev_hunk();
    Ok(snap_from(&app, &mut hl, &*state))
}

#[tauri::command]
pub fn toggle_compacted(state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    app.tab_mut()
        .toggle_compacted()
        .map_err(|e| e.to_string())?;
    Ok(snap_from(&app, &mut hl, &*state))
}

// ── Mode ──────────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn set_mode(mode: String, state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    let diff_mode = match mode.as_str() {
        "unstaged" => DiffMode::Unstaged,
        "staged" => DiffMode::Staged,
        "history" => DiffMode::History,
        _ => DiffMode::Branch,
    };
    app.tab_mut().set_mode(diff_mode);
    Ok(snap_from(&app, &mut hl, &*state))
}

// ── Reviewed state ────────────────────────────────────────────────────────────

#[tauri::command]
pub fn toggle_reviewed(state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    app.toggle_reviewed().map_err(|e| e.to_string())?;
    Ok(snap_from(&app, &mut hl, &*state))
}

#[tauri::command]
pub fn mark_reviewed(file_idx: usize, state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    {
        let tab = app.tab_mut();
        if let Some(file) = tab.files.get(file_idx) {
            let path = file.path.clone();
            let hash = tab
                .current_per_file_hashes
                .get(&path)
                .cloned()
                .unwrap_or_default();
            tab.reviewed.insert(path.clone(), hash);
            let _ = tab.save_reviewed_files();
        }
    }
    Ok(snap_from(&app, &mut hl, &*state))
}

#[tauri::command]
pub fn unmark_reviewed(file_idx: usize, state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    {
        let tab = app.tab_mut();
        if let Some(file) = tab.files.get(file_idx) {
            let path = file.path.clone();
            tab.reviewed.remove(&path);
            let _ = tab.save_reviewed_files();
        }
    }
    Ok(snap_from(&app, &mut hl, &*state))
}

// ── Editor ────────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn open_in_editor(state: State<AppState>) -> Result<OpenSourceResult, String> {
    open_source(state)
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
        target: "No local checkout found for this source. Create editable worktree first.".to_string(),
    })
}

fn local_source_root<'a>(tab: &'a er_engine::app::TabState) -> Option<&'a str> {
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

#[tauri::command]
pub fn open_url_in_browser(url: String) -> Result<(), String> {
    let result = if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(&url).spawn()
    } else if cfg!(target_os = "linux") {
        std::process::Command::new("xdg-open").arg(&url).spawn()
    } else {
        std::process::Command::new("cmd")
            .args(["/c", "start", &url])
            .spawn()
    };
    result.map(|_| ()).map_err(|e| e.to_string())
}

fn github_file_url_for_tab(tab: &er_engine::app::TabState, file_path: &str, line_num: usize) -> Option<String> {
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
        let conclusion = c
            .conclusion
            .as_str()
            .to_ascii_uppercase();
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
) {
    let now = now_ms();
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
            desktop_revision.fetch_add(1, Ordering::Relaxed);
        }
        return;
    };

    let projects_file = projects::load();
    let mut project_by_remote: HashMap<String, (String, String)> = HashMap::new();
    for p in projects_file.projects {
        if let Some(remote) = p.remote {
            project_by_remote.insert(remote, (p.id, p.root_path));
        }
    }
    let cache = pr_cache
        .lock()
        .ok()
        .map(|g| g.clone())
        .unwrap_or_default();

    let mut new_items: Vec<InboxItem> = Vec::new();
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
                    project_id: project_by_remote.get(&remote).map(|p| p.0.clone()),
                    repo_root: project_by_remote.get(&remote).map(|p| p.1.clone()),
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
                                project_id: project_by_remote.get(&remote).map(|p| p.0.clone()),
                                repo_root: project_by_remote.get(&remote).map(|p| p.1.clone()),
                                remote: Some(remote.clone()),
                                pr_number: Some(pr.number),
                                branch: Some(pr.head_ref.clone()),
                                url: None,
                            },
                            created_at_ms: now,
                            read_at_ms: None,
                            dedupe_key: format!("github:{remote}:{}:review_decision:APPROVED", pr.number),
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
                                project_id: project_by_remote.get(&remote).map(|p| p.0.clone()),
                                repo_root: project_by_remote.get(&remote).map(|p| p.1.clone()),
                                remote: Some(remote.clone()),
                                pr_number: Some(pr.number),
                                branch: Some(pr.head_ref.clone()),
                                url: None,
                            },
                            created_at_ms: now,
                            read_at_ms: None,
                            dedupe_key: format!("github:{remote}:{}:review_decision:CHANGES_REQUESTED", pr.number),
                        });
                    }
                }
                if !is_my_pr {
                    let prev_requested = prev_state.requested_reviewers.iter().any(|r| r == &gh_user);
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
                                project_id: project_by_remote.get(&remote).map(|p| p.0.clone()),
                                repo_root: project_by_remote.get(&remote).map(|p| p.1.clone()),
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
                                project_id: project_by_remote.get(&remote).map(|p| p.0.clone()),
                                repo_root: project_by_remote.get(&remote).map(|p| p.1.clone()),
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
                                project_id: project_by_remote.get(&remote).map(|p| p.0.clone()),
                                repo_root: project_by_remote.get(&remote).map(|p| p.1.clone()),
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
                    project_id: project_by_remote.get(&remote).map(|p| p.0.clone()),
                    repo_root: project_by_remote.get(&remote).map(|p| p.1.clone()),
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
    if let Ok(mut inbox) = inbox_handle.lock() {
        for item in new_items {
            if inbox.add_item(item.clone()) {
                emitted_any = true;
                maybe_send_native_notification(inbox_handle, app_handle_state, &item);
            }
        }
    }
    crate::inbox::save_inbox_state(inbox_handle);
    if emitted_any {
        desktop_revision.fetch_add(1, Ordering::Relaxed);
    }
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
pub fn list_review_revisions(state: State<AppState>) -> Result<Vec<ReviewRevisionSummary>, String> {
    let app = state.app.lock().map_err(|e| e.to_string())?;
    let tab = app.tab();
    if tab.repo_root.is_empty() {
        return Ok(Vec::new());
    }
    let branch = tab
        .local_branch_view
        .as_deref()
        .unwrap_or(&tab.current_branch)
        .to_string();
    if branch.is_empty() {
        return Ok(Vec::new());
    }
    let repo_slug = crate::er_storage::slug_repo(&tab.repo_root);
    let branch_slug = crate::er_storage::slug_branch(&branch);
    let active = crate::er_storage::active_revision(&repo_slug, &branch_slug);
    let active_id = active.as_ref().map(|a| a.revision_id.as_str()).unwrap_or("");
    let revisions = crate::er_storage::list_revisions(&repo_slug, &branch_slug);
    let mut out = Vec::with_capacity(revisions.len());
    for rev in revisions {
        let agents_root = crate::er_storage::revision_root(&repo_slug, &branch_slug, &rev.revision_id).join("agents");
        let mut agents = Vec::new();
        if let Ok(entries) = std::fs::read_dir(agents_root) {
            for entry in entries.flatten() {
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    if let Some(name) = entry.file_name().to_str() {
                        agents.push(name.to_string());
                    }
                }
            }
        }
        out.push(ReviewRevisionSummary {
            revision_id: rev.revision_id.clone(),
            created_at: rev.created_at.clone(),
            scope: rev.scope.clone(),
            diff_hash: rev.diff_hash.clone(),
            active: rev.revision_id == active_id,
            agents,
        });
    }
    Ok(out)
}

#[tauri::command]
pub fn read_review_json(state: State<AppState>, revision_id: Option<String>) -> Result<String, String> {
    let app = state.app.lock().map_err(|e| e.to_string())?;
    let tab = app.tab();
    let branch = tab
        .local_branch_view
        .as_deref()
        .unwrap_or(&tab.current_branch)
        .to_string();
    let repo_slug = crate::er_storage::slug_repo(&tab.repo_root);
    let branch_slug = crate::er_storage::slug_branch(&branch);
    let active = crate::er_storage::active_revision(&repo_slug, &branch_slug);
    let rev_id = revision_id
        .or_else(|| active.map(|a| a.revision_id))
        .ok_or_else(|| "No active revision".to_string())?;
    let review_path = crate::er_storage::revision_root(&repo_slug, &branch_slug, &rev_id)
        .join("agents")
        .join("claude")
        .join("review.json");
    if !review_path.exists() {
        return Err("No review.json found for selected revision".to_string());
    }
    std::fs::read_to_string(&review_path).map_err(|e| e.to_string())
}

// ── Filter / search ───────────────────────────────────────────────────────────

#[tauri::command]
pub fn set_filter(query: String, state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    app.tab_mut().apply_filter_expr(&query);
    Ok(snap_from(&app, &mut hl, &*state))
}

#[tauri::command]
pub fn clear_filter(state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    app.tab_mut().apply_filter_expr("");
    Ok(snap_from(&app, &mut hl, &*state))
}

// ── Threads ───────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn add_comment(
    file: String,
    hunk_idx: usize,
    line_num: Option<usize>,
    text: String,
    side: Option<String>,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    // Set side before submit so submit_github_comment can consume it
    if let Some(ref s) = side {
        app.tab_mut().comment_side = Some(s.clone());
    }
    app.submit_comment_text(
        file,
        hunk_idx,
        line_num,
        text,
        CommentType::GitHubComment,
        None,
        None,
    )
    .map_err(|e| e.to_string())?;
    Ok(snap_from(&app, &mut hl, &*state))
}

#[tauri::command]
pub fn add_question(
    file: String,
    hunk_idx: usize,
    line_num: Option<usize>,
    text: String,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    app.submit_comment_text(file, hunk_idx, line_num, text, CommentType::Question, None, None)
        .map_err(|e| e.to_string())?;
    Ok(snap_from(&app, &mut hl, &*state))
}

#[tauri::command]
pub fn reply_to_thread(
    parent_id: String,
    text: String,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
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
        text,
        comment_type,
        Some(parent_id),
        None,
    )
    .map_err(|e| e.to_string())?;
    Ok(snap_from(&app, &mut hl, &*state))
}

#[tauri::command]
pub fn delete_thread(id: String, state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    app.delete_comment_direct(&id).map_err(|e| e.to_string())?;
    Ok(snap_from(&app, &mut hl, &*state))
}

fn write_json_atomic<T: serde::Serialize>(path: &str, value: &T) -> Result<(), String> {
    let tmp = format!("{path}.tmp");
    let body = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, body).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

fn mark_thread_resolved_in_files(id: &str, q_path: &str, gc_path: &str) -> Result<bool, String> {
    use er_engine::ai::{ErGitHubComments, ErQuestions};
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
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;

    let tab = app.tab();
    let q_path = format!("{}/questions.json", tab.er_dir());
    let gc_path = tab.github_comments_path();
    let changed = mark_thread_resolved_in_files(&id, &q_path, &gc_path)?;
    if !changed {
        return Err(format!("Thread not found or already resolved: {id}"));
    }
    app.tab_mut().reload_ai_state();
    Ok(snap_from(&app, &mut hl, &*state))
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

// ── GitHub sync ───────────────────────────────────────────────────────────────

#[tauri::command]
pub fn refresh_diff(state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    app.tab_mut().refresh_diff().map_err(|e| e.to_string())?;
    Ok(snap_from(&app, &mut hl, &*state))
}

#[tauri::command]
pub fn force_refresh_diff(state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    app.tab_mut()
        .refetch_and_refresh_diff()
        .map_err(|e| e.to_string())?;
    kick_meta_refresh(&*state, app.tab().repo_root.clone());
    Ok(snap_from(&app, &mut hl, &*state))
}

/// Trigger an immediate background refresh of the GitHub status for the active tab.
/// Returns the current snapshot without waiting — the next poll will pick up the fresh data.
#[tauri::command]
pub fn refresh_github_status(state: State<AppState>) -> Result<AppSnapshot, String> {
    let key = {
        let app = state.app.lock().map_err(|e| e.to_string())?;
        active_github_key(&app, &*state)
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
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    app.sync_github_comments().map_err(|e| e.to_string())?;
    Ok(snap_from(&app, &mut hl, &*state))
}

#[tauri::command]
pub fn push_github_comments(state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    app.push_all_comments_to_github()
        .map_err(|e| e.to_string())?;
    Ok(snap_from(&app, &mut hl, &*state))
}

/// Submit pending local comments as a GitHub PR review with an explicit decision.
/// `mode` must be "COMMENT", "APPROVE", or "REQUEST_CHANGES".
/// `summary` is the top-level review body sent to GitHub.
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
            let pr_info = github::get_pr_info(&repo_root).map_err(|e| e.to_string())?;
            (pr_info.0, pr_info.1, pr_info.2)
        };
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
        line: usize,
        old_line: Option<usize>,
        body: String,
        side: String,
    }

    // Collect all line-anchored pending candidates.
    let candidates: Vec<BatchEntry> = gc
        .comments
        .iter()
        .filter(|c| c.source == "local" && !c.synced && !c.file.is_empty())
        .filter_map(|c| {
            c.line_start.map(|l| BatchEntry {
                id: c.id.clone(),
                file: c.file.clone(),
                line: l,
                old_line: c.old_line_start,
                body: c.comment.clone(),
                side: c.side.clone(),
            })
        })
        .collect();

    // Partition into valid (anchor in current diff) and stale.
    // LEFT-side comments (deleted lines) validate against old-side hunk ranges;
    // RIGHT-side comments validate against new-side ranges.
    let mut invalid_anchors: Vec<(String, usize, String)> = Vec::new();
    let mut batch_entries: Vec<BatchEntry> = Vec::new();
    for e in candidates {
        let in_diff = if e.side == "LEFT" {
            let anchor = e.old_line.unwrap_or(e.line);
            is_anchor_in_current_diff(&old_file_anchors, &e.file, anchor)
        } else {
            is_anchor_in_current_diff(&file_anchors, &e.file, e.line)
        };
        if in_diff {
            batch_entries.push(e);
        } else {
            invalid_anchors.push((e.id, e.line, e.file));
        }
    }

    let batch: Vec<er_engine::github::ReviewBatchEntry> = batch_entries
        .iter()
        .map(|e| {
            let line = if e.side == "LEFT" {
                e.old_line.unwrap_or(e.line)
            } else {
                e.line
            };
            er_engine::github::ReviewBatchEntry {
                file: e.file.clone(),
                line,
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

    let submit_err = |e: anyhow::Error| -> String {
        let raw = e.to_string();
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
    };

    let submit_result = if is_remote {
        github::gh_pr_submit_review_remote(
            &owner,
            &repo_name,
            pr_number,
            &batch,
            event,
            &summary_trimmed,
        )
    } else {
        github::gh_pr_submit_review(
            &owner,
            &repo_name,
            pr_number,
            &batch,
            &repo_root,
            event,
            &summary_trimmed,
        )
    };
    submit_result.map_err(submit_err)?;

    // Mark only comments that were actually submitted in this review as synced.
    if !submitted_ids.is_empty() {
        let mut gc_to_write = gc;
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
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    Ok(snap_from(&app, &mut hl, &*state))
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
        let (owner, repo, number) = active_github_key(&app, &*state)
            .ok_or_else(|| "No GitHub PR detected for the active tab".to_string())?;
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
    submit_result.map_err(|e| format!("GitHub review submission failed: {e}"))?;

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
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    Ok(snap_from(&app, &mut hl, &*state))
}

/// Post a PR-wide (issue-stream) comment on the active tab's PR. Used by the
/// GitHub card's "Comment / Review" action — distinct from line-anchored
/// review comments handled by `submit_github_review`.
#[tauri::command]
pub fn post_github_pr_comment(
    body: String,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return Err("Comment body cannot be empty".to_string());
    }

    let (owner, repo, number) = {
        let app = state.app.lock().map_err(|e| e.to_string())?;
        active_github_key(&app, &*state).ok_or_else(|| {
            "No GitHub PR detected for the active tab".to_string()
        })?
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
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    Ok(snap_from(&app, &mut hl, &*state))
}

// ── AI integration ───────────────────────────────────────────────────────────

#[tauri::command]
pub fn run_ai_review(scope: String, state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;

    let is_remote = app.tab().remote_repo.is_some();

    let (prompt, target) = if is_remote {
        // Remote PR review: use existing relative-path prompt; cwd = er_dir (cache dir).
        let base_branch = app.tab().base_branch.clone();
        let prompt = er_engine::ai::prompts::build_review_prompt(&base_branch, &scope);
        let tab = app.tab();
        let branch_label = tab
            .local_branch_view
            .clone()
            .unwrap_or_else(|| tab.current_branch.clone());
        let target = er_engine::app::BackgroundTaskTarget {
            repo_root: tab.repo_root.clone(),
            er_dir: tab.er_dir(),
            branch_label,
            base_branch: tab.base_branch.clone(),
            scope: scope.clone(),
            pr_number: tab.pr_number,
            remote_repo: tab.remote_repo.clone(),
            managed_local: false,
        };
        (prompt, target)
    } else {
        // Local managed review: create a fresh revision in managed storage.
        let (repo_root, branch, base_branch, diff_hash) = {
            let tab = app.tab();
            let branch = tab
                .local_branch_view
                .clone()
                .unwrap_or_else(|| tab.current_branch.clone());
            (
                tab.repo_root.clone(),
                branch,
                tab.base_branch.clone(),
                tab.branch_diff_hash.clone(),
            )
        };

        let repo_slug = crate::er_storage::slug_repo(&repo_root);
        let branch_slug = crate::er_storage::slug_branch(&branch);
        let rev_id = crate::er_storage::new_revision_id(&diff_hash);

        let meta = crate::er_storage::RevisionMeta {
            revision_id: rev_id.clone(),
            base_branch: base_branch.clone(),
            head_branch: branch.clone(),
            diff_hash: diff_hash.clone(),
            commit_hash: String::new(),
            created_at: crate::er_storage::iso_now(),
            scope: scope.clone(),
        };
        crate::er_storage::create_revision(&repo_slug, &branch_slug, meta)
            .map_err(|e| format!("Failed to create managed review revision: {e}"))?;

        let agent_dir = crate::er_storage::agent_dir(&repo_slug, &branch_slug, &rev_id, "claude");
        // Ensure the agent dir exists (create_revision builds it, but be explicit).
        std::fs::create_dir_all(&agent_dir)
            .map_err(|e| format!("Failed to create agent directory: {e}"))?;
        let session_dir = crate::er_storage::revision_root(&repo_slug, &branch_slug, &rev_id);
        let agent_dir_str = agent_dir.to_string_lossy().into_owned();
        let session_dir_str = session_dir.to_string_lossy().into_owned();

        // Update the tab's er_root so the UI loads from the new revision.
        app.tab_mut().er_root = er_engine::ErRoot::Managed {
            agent_dir: agent_dir_str.clone(),
            session_dir: session_dir_str,
        };

        let prompt = er_engine::ai::prompts::build_review_prompt_local_managed(
            &base_branch,
            &scope,
            &agent_dir_str,
        );

        let target = er_engine::app::BackgroundTaskTarget {
            repo_root: repo_root.clone(),
            er_dir: agent_dir_str,
            branch_label: branch,
            base_branch,
            scope: scope.clone(),
            pr_number: None,
            remote_repo: None,
            managed_local: true,
        };
        (prompt, target)
    };

    app.spawn_background_review(target, prompt)
        .map_err(|e| e.to_string())?;

    state
        .desktop_revision
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Ok(snap_from(&app, &mut hl, &*state))
}

#[tauri::command]
pub fn run_ai_validate(scope: String, state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;

    if app.tab().is_remote() {
        return Err("Validate review is local-only. Check out the PR locally first.".to_string());
    }

    let er_dir = app.tab().er_dir();
    let review_path = std::path::Path::new(&er_dir).join("review.json");
    if !review_path.exists() {
        return Err("No review to validate. Run AI review first.".to_string());
    }

    let base_branch = app.tab().base_branch.clone();
    let prompt = er_engine::ai::prompts::build_validate_prompt(&base_branch, &scope);
    app.spawn_agent_prompt("validate", &prompt)
        .map_err(|e| e.to_string())?;
    app.notify("AI review validation started");

    state
        .desktop_revision
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Ok(snap_from(&app, &mut hl, &*state))
}

#[tauri::command]
pub fn set_ai_model(model: String, state: State<AppState>) -> Result<AppSnapshot, String> {
    let app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;

    let repo_root = app.tab().repo_root.clone();
    let mut cfg = er_engine::config::load_config(&repo_root);
    cfg.agent.model = model;
    er_engine::config::save_config(&cfg).map_err(|e| e.to_string())?;

    Ok(snap_from(&app, &mut hl, &*state))
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
}

#[tauri::command]
pub fn list_ai_providers(state: State<AppState>) -> Result<Vec<AiProviderInfo>, String> {
    let app = state.app.lock().map_err(|e| e.to_string())?;
    let hub = &app.config.ai_hub;
    let current_provider = app.current_ai_provider.as_deref();
    let current_model = app.current_ai_model.as_deref();
    let resolved_provider = hub.resolve_provider_id(current_provider);

    let providers = hub
        .providers
        .iter()
        .map(|(id, cfg)| {
            let resolved_model = hub.resolve_model_id(id, current_model);
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
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;

    let hub = &app.config.ai_hub;
    if !hub.providers.contains_key(&provider_id) {
        return Err(format!("Unknown provider: {provider_id}"));
    }
    if let Some(ref mid) = model_id {
        let provider = hub.providers.get(&provider_id).unwrap();
        if !provider.models.is_empty() && !provider.models.iter().any(|m| &m.id == mid) {
            return Err(format!(
                "Unknown model '{mid}' for provider '{provider_id}'"
            ));
        }
    }

    app.current_ai_provider = Some(provider_id);
    app.current_ai_model = model_id;

    state
        .desktop_revision
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Ok(snap_from(&app, &mut hl, &*state))
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
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;

    // 1. Resolve the source question + already-promoted guard.
    let (file, hunk_idx, line_start, default_body, questions_path) = {
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
        if let Some(existing) = q.promoted_to.as_deref() {
            return Err(format!("Already promoted to {existing}"));
        }

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
            format!("{}/questions.json", tab.er_dir()),
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

    // 5. Persist `promoted_to` back into questions.json and reload.
    if let Some(new_id) = new_id.as_deref() {
        if let Ok(content) = std::fs::read_to_string(&questions_path) {
            if let Ok(mut qs) = serde_json::from_str::<er_engine::ai::ErQuestions>(&content) {
                if let Some(q) = qs.questions.iter_mut().find(|q| q.id == id) {
                    q.promoted_to = Some(new_id.to_string());
                }
                if let Ok(json) = serde_json::to_string_pretty(&qs) {
                    let tmp = format!("{questions_path}.tmp");
                    if std::fs::write(&tmp, json).is_ok() {
                        let _ = std::fs::rename(&tmp, &questions_path);
                    }
                }
            }
        }
        app.tab_mut().reload_ai_state();
    }

    Ok(snap_from(&app, &mut hl, &*state))
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
    let app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;

    let (file, hunk_idx, line_num, comment_type, context) = {
        let tab = app.tab();
        if thread_id.starts_with("q-") {
            let q = tab
                .ai
                .questions
                .as_ref()
                .and_then(|qs| qs.questions.iter().find(|q| q.id == thread_id))
                .ok_or_else(|| "Question not found".to_string())?;
            let mut ctx = String::new();
            ctx.push_str(&format!("{}:{}\n", q.file, q.line_start.unwrap_or(0)));
            ctx.push_str(&q.text);
            // Append replies in order
            if let Some(qs) = &tab.ai.questions {
                for r in qs
                    .questions
                    .iter()
                    .filter(|r| r.in_reply_to.as_deref() == Some(thread_id.as_str()))
                {
                    ctx.push_str(&format!("\n\n**{}** replied:\n{}", r.author, r.text));
                }
            }
            (
                q.file.clone(),
                q.hunk_index.unwrap_or(0),
                q.line_start,
                CommentType::Question,
                ctx,
            )
        } else {
            let c = tab
                .ai
                .github_comments
                .as_ref()
                .and_then(|gc| gc.comments.iter().find(|c| c.id == thread_id))
                .ok_or_else(|| "Comment not found".to_string())?;
            let mut ctx = String::new();
            ctx.push_str(&format!("{}:{}\n", c.file, c.line_start.unwrap_or(0)));
            ctx.push_str(&c.comment);
            if let Some(gc) = &tab.ai.github_comments {
                for r in gc
                    .comments
                    .iter()
                    .filter(|r| r.in_reply_to.as_deref() == Some(thread_id.as_str()))
                {
                    ctx.push_str(&format!("\n\n**{}** replied:\n{}", r.author, r.comment));
                }
            }
            (
                c.file.clone(),
                c.hunk_index.unwrap_or(0),
                c.line_start,
                CommentType::GitHubComment,
                ctx,
            )
        }
    };

    let repo_root = app.tab().repo_root.clone();
    let cfg = er_engine::config::load_config(&repo_root);
    let model = if cfg.agent.model.trim().is_empty() {
        "sonnet".to_string()
    } else {
        cfg.agent.model.clone()
    };

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

    // Build snapshot before releasing locks (test path expects synchronous
    // visibility of the pending state).
    let snap = snap_from(&app, &mut hl, &*state);

    // Release locks before spawning so the subprocess runs without holding
    // the App mutex.
    drop(hl);
    drop(app);

    std::thread::spawn(move || {
        let body = run_ask_ai_subprocess(&model, &context, &user_prompt);
        // Take App lock to submit the reply.
        if let Ok(mut app) = app_arc.lock() {
            let _ = app.submit_comment_text_as_author(
                file,
                hunk_idx,
                line_num,
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

/// Invoke the `claude` CLI (or a sentinel for tests) and return the body to
/// attach as a reply. Capped at 8KB. Never panics — any failure becomes a
/// fallback error message in the returned body.
fn run_ask_ai_subprocess(model: &str, context: &str, user_prompt: &str) -> String {
    // Test sentinel — bypasses subprocess entirely so unit tests do not need
    // the `claude` binary installed.
    if let Ok(fake) = std::env::var("ER_FAKE_CLAUDE") {
        return match fake.as_str() {
            "fail" => "Pending — invoke via CLI (error: ER_FAKE_CLAUDE=fail)".to_string(),
            "ok" => "mocked ok".to_string(),
            other if !other.is_empty() => other.to_string(),
            _ => "mocked ok".to_string(),
        };
    }

    use std::process::Command;
    let result = Command::new("claude")
        .arg("--print")
        .arg("--model")
        .arg(model)
        .arg("--append-system-prompt")
        .arg(context)
        .arg(user_prompt)
        .output();

    match result {
        Ok(out) if out.status.success() => {
            let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
            const MAX: usize = 8 * 1024;
            if s.len() > MAX {
                s.truncate(MAX);
            }
            let trimmed = s.trim().to_string();
            if trimmed.is_empty() {
                "Pending — invoke via CLI (empty response)".to_string()
            } else {
                trimmed
            }
        }
        Ok(out) => {
            let err = String::from_utf8_lossy(&out.stderr);
            format!(
                "Pending — invoke via CLI (claude exited {}: {})",
                out.status.code().unwrap_or(-1),
                err.trim()
            )
        }
        Err(e) => format!("Pending — invoke via CLI (failed to spawn claude: {e})"),
    }
}

// ── PR URL open ──────────────────────────────────────────────────────────────

/// Place `tab` into the app: replace the active slot when `replace` is true
/// (Cmd-click / middle-click semantics), otherwise push a new tab.
pub(crate) fn place_tab(app: &mut App, tab: er_engine::app::TabState, replace: bool) {
    if replace && !app.tabs.is_empty() {
        let idx = app.active_tab.min(app.tabs.len() - 1);
        let name = tab.tab_name();
        app.tabs[idx] = tab;
        app.active_tab = idx;
        app.notify(&format!("Opened: {}", name));
    } else {
        app.open_tab(tab);
    }
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

fn normalize_remote_slug(remote: &str) -> String {
    let trimmed = remote.trim();
    let without_scheme = trimmed
        .strip_prefix("https://github.com/")
        .or_else(|| trimmed.strip_prefix("http://github.com/"))
        .unwrap_or(trimmed);
    without_scheme
        .trim_end_matches(".git")
        .trim_matches('/')
        .to_ascii_lowercase()
}

fn find_project_id_for_remote(file: &projects::ProjectsFile, remote_slug: &str) -> Option<String> {
    let target = normalize_remote_slug(remote_slug);
    file.projects
        .iter()
        .find_map(|p| p.remote.as_ref().filter(|r| normalize_remote_slug(r) == target).map(|_| p.id.clone()))
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
            "number,title,headRefName,state,isDraft,author,assignees,reviewRequests,reviewDecision,mergedAt,latestReviews",
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
    let raw: RawPr = serde_json::from_slice(&out.stdout).map_err(|e| {
        format!("Failed to parse gh pr view output for {remote}#{pr_number}: {e}")
    })?;
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
        latest_reviewer_states,
    })
}

#[tauri::command]
pub fn open_remote_pr(
    owner: String,
    repo: String,
    number: u64,
    replace: Option<bool>,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    do_open_remote_pr(&mut app, &owner, &repo, number, replace.unwrap_or(false))?;
    kick_meta_refresh(&*state, app.tab().repo_root.clone());
    kick_github_status_refresh(
        state.gh_status_cache.clone(),
        Arc::clone(&state.gh_status_in_flight),
        Arc::clone(&state.desktop_revision),
        Some(Arc::clone(&state.loading)),
        owner,
        repo,
        number,
    );
    Ok(snap_from(&app, &mut hl, &*state))
}

#[tauri::command]
pub fn open_pr_url(
    url: String,
    replace: Option<bool>,
    state: State<AppState>,
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
            let fetched_pr = fetch_single_pr_for_remote(&remote, pr_ref.number)?;
            if let Ok(mut cache) = state.pr_cache.lock() {
                let entry = cache.entry(remote.clone()).or_default();
                if let Some(idx) = entry.iter().position(|pr| pr.number == pr_ref.number) {
                    entry[idx] = fetched_pr;
                } else {
                    entry.push(fetched_pr);
                }
            }
            if let Ok(mut fetched) = state.pr_cache_fetched_at.lock() {
                fetched.insert(remote.clone(), now_epoch_ms());
            }
            crate::pr_cache::save_persisted_pr_cache(&state.pr_cache, &state.pr_cache_fetched_at);
        }
        projects::track_pr(&project_id, pr_ref.number).map_err(|e| e.to_string())?;
        return open_pr_review(project_id, pr_ref.number, replace, state);
    }

    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    do_open_remote_pr(
        &mut app,
        &pr_ref.owner,
        &pr_ref.repo,
        pr_ref.number,
        replace.unwrap_or(false),
    )?;
    kick_meta_refresh(&*state, app.tab().repo_root.clone());
    kick_github_status_refresh(
        state.gh_status_cache.clone(),
        Arc::clone(&state.gh_status_in_flight),
        Arc::clone(&state.desktop_revision),
        Some(Arc::clone(&state.loading)),
        pr_ref.owner,
        pr_ref.repo,
        pr_ref.number,
    );
    Ok(snap_from(&app, &mut hl, &*state))
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
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    app.open_tab(new_tab);
    let _ = projects::auto_register(&path_str);
    kick_meta_refresh(&*state, app.tab().repo_root.clone());
    Ok(snap_from(&app, &mut hl, &*state))
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
            log::info!("branch open local-first miss; falling back to local branch diff: {local_err}");
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
        let er_ref_result =
            er_engine::github::fetch_branch_upstream_into_er_ref(&repo_root, &branch_name);
        let base_ref_result =
            er_engine::github::fetch_remote_base_ref_for_diff(&repo_root, &base_branch);

        match (er_ref_result, base_ref_result) {
            (Ok(er_ref), Ok(base_ref)) => {
                let mut refreshed_active_tab = false;
                if let Ok(mut app) = app_state.lock() {
                    let active_tab = app.active_tab;
                    if let Some(tab) = app.tabs.get_mut(active_tab).filter(|tab| {
                        tab.repo_root == repo_root
                            && tab.local_branch_view.as_deref() == Some(branch_name.as_str())
                    }) {
                        tab.local_branch_diff_ref = Some(er_ref);
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
            (Err(err), _) => {
                log::warn!("background branch upstream refresh failed for {branch_name}: {err}");
                refresh_active_branch_after_background_miss(
                    &app_state,
                    &desktop_revision,
                    &repo_root,
                    &branch_name,
                );
            }
            (_, Err(err)) => {
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
pub fn open_local_branch(
    project_id: String,
    name: String,
    replace: Option<bool>,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let t_total = std::time::Instant::now();
    let branch_name = name.clone();
    let t_tab_build = std::time::Instant::now();
    let (new_tab, open_path) = build_local_branch_tab(&project_id, name)?;
    log_branch_open_phase(&project_id, &branch_name, "tab_build", t_tab_build);
    let t_app_lock = std::time::Instant::now();
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
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
    kick_meta_refresh(&*state, app.tab().repo_root.clone());
    kick_active_gh_status(&app, &*state);
    let repo_root = app.tab().repo_root.clone();
    let base_branch = app.tab().base_branch.clone();
    let t_snapshot = std::time::Instant::now();
    let snapshot = snap_from(&app, &mut hl, &*state);
    log_branch_open_phase(&project_id, &branch_name, "snapshot_build", t_snapshot);
    log_branch_open_phase(&project_id, &branch_name, "total", t_total);
    drop(hl);
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

fn cached_pr_open_diff(
    cache: &Arc<Mutex<HashMap<PrOpenCacheKey, PrOpenCacheEntry>>>,
    key: &PrOpenCacheKey,
    freshness: &PrOpenFreshness,
) -> Option<String> {
    cache
        .lock()
        .ok()
        .and_then(|guard| guard.get(key).cloned())
        .filter(|entry| entry.freshness == *freshness)
        .map(|entry| entry.raw_diff)
}

fn remember_pr_open_diff(
    cache: &Arc<Mutex<HashMap<PrOpenCacheKey, PrOpenCacheEntry>>>,
    key: PrOpenCacheKey,
    freshness: PrOpenFreshness,
    raw_diff: String,
) {
    if let Ok(mut guard) = cache.lock() {
        guard.insert(
            key,
            PrOpenCacheEntry {
                freshness,
                raw_diff,
            },
        );
        const MAX_PR_OPEN_CACHE_ENTRIES: usize = 32;
        if guard.len() > MAX_PR_OPEN_CACHE_ENTRIES {
            if let Some(first_key) = guard.keys().next().cloned() {
                guard.remove(&first_key);
            }
        }
    }
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
            "number,title,body,state,author,url,baseRefName,headRefName,headRefOid,updatedAt,reviews",
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
    })
}

fn run_gh_pr_diff_for_open(repo_root: &str, pr_number: u64) -> Result<String, String> {
    er_engine::github::gh_pr_diff(pr_number, repo_root).map_err(|e| e.to_string())
}

fn load_pr_open_inputs(
    project_id: &str,
    pr_number: u64,
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
    let branch_label = format!("pr-{}", pr_number);

    let key = pr_open_cache_key(project_id, &repo_root, pr_number);
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
        if let Some(raw_diff) =
            cached_pr_open_diff(&state.pr_open_cache, &key, &metadata.freshness)
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
        let t_diff = std::time::Instant::now();
        let raw_diff = run_gh_pr_diff_for_open(&repo_root, pr_number)?;
        log::info!(
            "branch_open project={} branch={} phase=gh_pr_diff ms={} cache=refresh",
            project_id,
            branch_label,
            t_diff.elapsed().as_millis()
        );
        let t_base = std::time::Instant::now();
        let resolved_base = er_engine::github::ensure_base_ref_available(
            &repo_root,
            &metadata.freshness.base_branch,
        )
        .map_err(|e| e.to_string())?;
        log_branch_open_phase(project_id, &branch_label, "base_ref_check", t_base);
        remember_pr_open_diff(
            &state.pr_open_cache,
            key,
            metadata.freshness.clone(),
            raw_diff.clone(),
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
    remember_pr_open_diff(
        &state.pr_open_cache,
        key,
        metadata.freshness.clone(),
        raw_diff.clone(),
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
pub fn open_pr_review(
    project_id: String,
    pr_number: u64,
    replace: Option<bool>,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let t_total = std::time::Instant::now();
    let branch_label = format!("pr-{}", pr_number);
    let t_tab_build = std::time::Instant::now();
    let inputs = load_pr_open_inputs(&project_id, pr_number, &*state).map_err(|e| {
        log::error!("open_pr_review: pr=#{pr_number} project_id={project_id} err={e}");
        e
    })?;
    let cache_hit = inputs.cache_hit;
    let new_tab = er_engine::app::TabState::new_local_pr_from_github_diff(
        inputs.repo_root,
        pr_number,
        inputs.resolved_base,
        inputs.metadata.freshness.head_branch,
        inputs.raw_diff,
        Some(inputs.metadata.pr_data),
    )
    .map_err(|e| {
        log::error!("open_pr_review: pr=#{pr_number} project_id={project_id} err={e}");
        e.to_string()
    })?;
    log_branch_open_phase(&project_id, &branch_label, "pr_tab_build", t_tab_build);
    log::info!(
        "branch_open project={} branch={} phase=pr_open_cache hit={}",
        project_id,
        branch_label,
        cache_hit
    );
    let t_app_lock = std::time::Instant::now();
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    log_branch_open_phase(&project_id, &branch_label, "app_lock", t_app_lock);
    let t_place_tab = std::time::Instant::now();
    place_tab(&mut app, new_tab, replace.unwrap_or(false));
    log_branch_open_phase(&project_id, &branch_label, "tab_place", t_place_tab);
    kick_meta_refresh(&*state, app.tab().repo_root.clone());
    let t_snapshot = std::time::Instant::now();
    let snapshot = snap_from(&app, &mut hl, &*state);
    log_branch_open_phase(&project_id, &branch_label, "snapshot_build", t_snapshot);
    log_branch_open_phase(&project_id, &branch_label, "total", t_total);
    kick_active_gh_status(&app, &*state);
    Ok(snapshot)
}

/// Kept for backwards compatibility — delegates to the no-checkout PR review flow.
#[tauri::command]
pub fn open_pr_branch(
    project_id: String,
    pr_number: u64,
    head_ref: String,
    replace: Option<bool>,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let _ = head_ref; // ignored; PR head is fetched directly from origin
    open_pr_review(project_id, pr_number, replace, state)
}

/// Switch the active tab to a different diff source.
/// Valid sources: "pr", "origin", "local".
#[tauri::command]
pub fn set_diff_source(source: String, state: State<AppState>) -> Result<AppSnapshot, String> {
    use er_engine::app::DiffSource;
    let diff_source = match source.as_str() {
        "pr" => DiffSource::Pr,
        "origin" => DiffSource::Origin,
        "local" => DiffSource::Local,
        other => return Err(format!("Invalid diff source: {other}")),
    };
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    {
        let available = app.tab().available_diff_sources();
        if !available.contains(&diff_source) {
            return Err(format!(
                "Diff source '{source}' is not available for this tab"
            ));
        }
    }
    app.tab_mut()
        .set_diff_source(diff_source)
        .map_err(|e| e.to_string())?;
    Ok(snap_from(&app, &mut hl, &*state))
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
        desktop_rev.fetch_add(1, Ordering::Relaxed); // loading started
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to build tokio runtime");
            let failed = rt.block_on(async move { crate::pr_cache::refresh_pr_cache(&cache, &fetched_at).await });
            for remote in failed {
                process_inbox_after_pr_refresh(
                    &pr_cache,
                    &gh_user,
                    &inbox,
                    &desktop_rev,
                    &app_handle_state,
                    Some(remote),
                );
            }
            process_inbox_after_pr_refresh(
                &pr_cache,
                &gh_user,
                &inbox,
                &desktop_rev,
                &app_handle_state,
                None,
            );
            if let Ok(mut f) = loading.lock() {
                f.pr_list = false;
            }
            desktop_rev.fetch_add(1, Ordering::Relaxed); // loading finished / cache updated
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
pub fn open_inbox_item(id: String, state: State<AppState>) -> Result<AppSnapshot, String> {
    let now = now_ms();
    let target = {
        let mut inbox = state.inbox.lock().map_err(|e| e.to_string())?;
        let target = inbox.items.iter().find(|i| i.id == id).map(|i| i.target.clone());
        inbox.mark_item_read(&id, now);
        target
    };
    crate::inbox::save_inbox_state(&state.inbox);
    state.desktop_revision.fetch_add(1, Ordering::Relaxed);

    if let Some(target) = target {
        if let (Some(project_id), Some(pr_number)) = (target.project_id.clone(), target.pr_number) {
            return open_pr_review(project_id, pr_number, Some(true), state);
        }
        if let (Some(project_id), Some(branch)) = (target.project_id, target.branch) {
            return open_local_branch(project_id, branch, Some(true), state);
        }
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
    kick_meta_refresh(&*state, root);
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
    kick_meta_refresh(&*state, root);
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
    refresh_branch_open_diff(&mut new_tab)?;

    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    place_tab(&mut app, new_tab, replace.unwrap_or(false));
    projects::set_active(&project_id);
    kick_meta_refresh(&*state, app.tab().repo_root.clone());
    kick_active_gh_status(&app, &*state);
    Ok(snap_from(&app, &mut hl, &*state))
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
    kick_meta_refresh(&*state, root);
    snap!(state)
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
    kick_meta_refresh(&*state, root);
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
    kick_meta_refresh(&*state, root);
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
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
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
    projects::set_active(&id);
    kick_meta_refresh(&*state, app.tab().repo_root.clone());
    kick_active_gh_status(&app, &*state);
    Ok(snap_from(&app, &mut hl, &*state))
}

// ── Findings: dismiss / promote / reply (v1 stubs) ──────────────────────────

#[tauri::command]
pub fn dismiss_finding(finding_id: String, state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    // TODO: persist to .er/review.json. For v1, mutate in-memory by marking resolved.
    {
        let tab = app.tab_mut();
        if let Some(review) = tab.ai.review.as_mut() {
            for file in review.files.values_mut() {
                for f in file.findings.iter_mut() {
                    if f.id == finding_id {
                        f.resolved = true;
                        f.resolved_note = "Dismissed by reviewer".to_string();
                    }
                }
            }
        }
    }
    Ok(snap_from(&app, &mut hl, &*state))
}

#[tauri::command]
pub fn promote_finding_to_comment(
    finding_id: String,
    body: Option<String>,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;

    let er_dir = app.tab().er_dir();

    // Already-promoted guard via sidecar map.
    let mut promotions = load_finding_promotions(&er_dir);
    if let Some(existing) = promotions.get(&finding_id) {
        return Err(format!("Already promoted to {existing}"));
    }

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
    let text = body.unwrap_or(default_body);

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

    if let Some(new_id) = new_id {
        promotions.insert(finding_id, new_id);
        if let Err(e) = save_finding_promotions(&er_dir, &promotions) {
            eprintln!("warn: failed to persist finding-promotions.json: {e}");
        }
        app.tab_mut().reload_ai_state();
    }

    Ok(snap_from(&app, &mut hl, &*state))
}

#[tauri::command]
pub fn reply_to_finding(
    finding_id: String,
    body: String,
    _ai_assist: bool,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;

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
        let default_root = "AI follow-up requested for this finding.".to_string();
        app.submit_comment_text(
            file,
            hunk_idx,
            line_start,
            default_root,
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
            .and_then(|gc| gc.comments.last().map(|c| c.id.clone()))
            .ok_or_else(|| "Failed to create finding comment thread".to_string())?;
        // Prepend finding title + description so the AI subprocess has full context.
        let enriched_prompt = if let Some((title, desc)) = finding_text {
            format!("Finding: {title}\n\n{desc}\n\n---\n\n{prompt}")
        } else {
            prompt
        };
        drop(hl);
        drop(app);
        return ask_ai(root_id, enriched_prompt, state);
    } else {
        app.submit_comment_text(
            file,
            hunk_idx,
            line_start,
            body,
            CommentType::GitHubComment,
            None,
            None,
        )
        .map_err(|e| e.to_string())?;
    }

    Ok(snap_from(&app, &mut hl, &*state))
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
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    Ok(snap_from(&app, &mut hl, &*state))
}

// ── Commit composer ──────────────────────────────────────────────────────────

#[tauri::command]
pub fn open_commit_composer(state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    app.input_mode = InputMode::Commit;
    Ok(snap_from(&app, &mut hl, &*state))
}

// ── History: select commit ──────────────────────────────────────────────────

#[tauri::command]
pub fn select_commit(sha: String, state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
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
                if history.selected_commit != pos {
                    history.selected_commit = pos;
                    // Reuse the engine's commit-load path so the right panel
                    // updates without forcing the user to keystroke.
                    tab.history_load_selected_diff();
                }
            }
        }
    }
    Ok(snap_from(&app, &mut hl, &*state))
}

// ── Tab management ──────────────────────────────────────────────────────────

#[tauri::command]
pub fn new_tab(state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    // Spawn a working-tree tab cloned from the active tab's repo root.
    // If that fails (e.g. deleted repo), fall back to the first tab's root.
    let root = app.tab().repo_root.clone();
    let tab = er_engine::app::TabState::new(root.clone())
        .or_else(|_| er_engine::app::TabState::new(app.tabs[0].repo_root.clone()))
        .map_err(|e| format!("Failed to open new tab: {e}"))?;
    app.open_tab(tab);
    kick_meta_refresh(&*state, root);
    Ok(snap_from(&app, &mut hl, &*state))
}

#[tauri::command]
pub fn close_tab(idx: usize, state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    app.close_tab_at(idx);
    Ok(snap_from(&app, &mut hl, &*state))
}

#[tauri::command]
pub fn select_tab(idx: usize, state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    app.select_tab(idx);
    kick_active_gh_status(&app, &*state);
    Ok(snap_from(&app, &mut hl, &*state))
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn reorder_tabs(
    fromIdx: usize,
    toIdx: usize,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    app.reorder_tabs(fromIdx, toIdx);
    // Persistence: the exit hook in main.rs flushes tab descriptors on app
    // exit, which captures any reorders. No mid-session save needed.
    Ok(snap_from(&app, &mut hl, &*state))
}

// ── UI annotations (browser view) ───────────────────────────────────────────

#[tauri::command]
#[allow(non_snake_case)]
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
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
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
    Ok(snap_from(&app, &mut hl, &*state))
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
    let mut out = String::with_capacity((input.len() + 2) / 3 * 4);
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
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    let dir = app.tab().comments_dir();
    let mut anns = er_engine::ai::load_ui_annotations(&dir);
    anns.retain(|a| a.id != id);
    er_engine::ai::save_ui_annotations(&dir, &anns).map_err(|e| e.to_string())?;
    Ok(snap_from(&app, &mut hl, &*state))
}

#[tauri::command]
pub fn clear_ui_annotations(state: State<AppState>) -> Result<AppSnapshot, String> {
    let app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    let dir = app.tab().comments_dir();
    let anns = er_engine::ai::load_ui_annotations(&dir);
    for path in anns.iter().filter_map(|a| a.screenshot_path.as_deref()) {
        let _ = std::fs::remove_file(path);
    }
    er_engine::ai::save_ui_annotations(&dir, &[]).map_err(|e| e.to_string())?;
    Ok(snap_from(&app, &mut hl, &*state))
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
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    let dir = app.tab().comments_dir();
    apply_anchor_updates(&dir, &updates).map_err(|e| e.to_string())?;
    Ok(snap_from(&app, &mut hl, &*state))
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
pub fn poll(state: State<AppState>) -> Result<PollResponse, String> {
    let t0 = std::time::Instant::now();
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    // Drain pending agent log entries and check for completed commands.
    app.drain_agent_log();
    // Consume completed command receivers — updates command_status to done/failed
    // and emits completion log entries; also resets last_ai_check on successful
    // review so the .er reload below picks up freshly written files.
    app.check_commands();
    // Same lifecycle for app-level background tasks (cross-tab reviews).
    app.poll_background_tasks();
    process_ai_task_inbox(&app, &*state);
    // Drain again so completion/failure log entries are visible in this poll.
    app.drain_agent_log();
    // Check if .er/ AI files changed — cheap mtime check, reloads AI state if yes
    app.tab_mut().check_ai_files_changed();

    let desktop_rev = state.desktop_revision.load(Ordering::Relaxed);
    let revision = compute_poll_revision(&app, desktop_rev);
    let last_sent = state.last_sent_revision.load(Ordering::Relaxed);

    if revision == last_sent {
        // Nothing changed — skip the expensive snapshot build.
        return Ok(PollResponse {
            revision,
            snapshot: None,
        });
    }

    let snapshot = snap_from(&app, &mut hl, &*state);
    state.last_sent_revision.store(revision, Ordering::Relaxed);

    if std::env::var("ER_DESKTOP_PROFILE_POLL").as_deref() == Ok("1") {
        eprintln!(
            "er-desktop poll_ms={} files={} threads={}",
            t0.elapsed().as_millis(),
            snapshot.files.len(),
            snapshot.ai.threads.len()
        );
    }
    Ok(PollResponse {
        revision,
        snapshot: Some(snapshot),
    })
}

fn compute_poll_revision(app: &App, desktop_revision: u64) -> u64 {
    let tab = app.tab();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    // Desktop-side background state (PR cache, gh status, loading flags)
    desktop_revision.hash(&mut h);
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
    h.finish()
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

    // If a session with this id already exists, drop it first (the old shell
    // dies via RAII). Lets the frontend re-mount cleanly without leaking PTYs.
    {
        let mut map = state.terminals.lock().map_err(|e| e.to_string())?;
        map.remove(&session_id);
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
    fn pr_open_cache_returns_matching_fresh_diff() {
        let cache = Arc::new(Mutex::new(HashMap::new()));
        let key = pr_open_cache_key("p1", "/repo", 1112);
        let freshness = PrOpenFreshness {
            base_branch: "main".into(),
            head_branch: "feature".into(),
            head_oid: "abc".into(),
            updated_at: "2026-05-17T10:00:00Z".into(),
        };
        remember_pr_open_diff(&cache, key.clone(), freshness.clone(), "diff --git".into());

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
        remember_pr_open_diff(&cache, key.clone(), freshness.clone(), "old diff".into());

        let stale_probe = PrOpenFreshness {
            head_oid: "def".into(),
            ..freshness
        };
        assert!(cached_pr_open_diff(&cache, &key, &stale_probe).is_none());
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
    fn ask_ai_subprocess_honors_fake_sentinel_ok() {
        let body = with_fake_claude("ok", || run_ask_ai_subprocess("sonnet", "ctx", "prompt"));
        assert_eq!(body, "mocked ok");
    }

    #[test]
    fn ask_ai_subprocess_honors_fake_sentinel_fail() {
        let body = with_fake_claude("fail", || run_ask_ai_subprocess("sonnet", "ctx", "prompt"));
        assert!(
            body.starts_with("Pending — invoke via CLI"),
            "expected fallback message, got: {body}"
        );
    }

    #[test]
    fn ask_ai_subprocess_returns_custom_sentinel_value() {
        let body = with_fake_claude("custom-response-text", || {
            run_ask_ai_subprocess("sonnet", "ctx", "prompt")
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
            }],
        };
        std::fs::write(&q_path, serde_json::to_string_pretty(&questions).unwrap()).unwrap();
        std::fs::write(&gc_path, r#"{"version":1,"diff_hash":"x","comments":[]}"#).unwrap();

        let changed = mark_thread_resolved_in_files(
            "q-1",
            &q_path.to_string_lossy(),
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
}
