use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::State;
#[cfg(not(target_os = "macos"))]
use tauri_plugin_notification::NotificationExt;

pub(crate) mod content_routes;

use crate::inbox::{
    inbox_item_from_notification, inbox_items_from_pr_transition, is_review_edge_kind, InboxHandle,
    InboxItem, InboxNotification, InboxState, InboxTarget, NotificationInboxCtx, PrInboxView,
    PrTransitionCtx,
};
use crate::projects::{self, normalize_remote_slug};
use crate::snapshot::{
    build_chrome_snapshot, AgentLogSnapshot, AppSnapshot, CheckSummary, FileSnapshot,
    GhCommentSummary, GhReviewSummary, GhUser, GithubStatusSnapshot, PrInfo,
};
pub use crate::state::{run_blocking, DesktopState, PrOpenPrefetchClaim, PrOpenPrefetchMap};
#[cfg(test)]
use er_engine::app::run_card_ai_subprocess;
#[cfg(test)]
use er_engine::app::CardAiInvocation;
use er_engine::app::{App, DiffMode, InputMode};
use er_engine::config::InboxConfig;

pub(crate) const DEFAULT_ASK_AI_PROMPT: &str =
    "Elaborate on this and answer any question directly.";

/// Per-card validation: check whether a comment, question, or finding still holds.
pub(crate) const VALIDATE_CARD_AI_PROMPT: &str = r#"Validate whether this review note is still accurate against the current code context provided.

Use Read / grep / rg in the repository (see system context for repo_root and diff excerpt) before concluding. Up to ~10 reads for this finding; cite file:line evidence.

Reply in markdown with:
1. **Verdict**: Confirmed | Outdated | Needs context | Unclear
2. **Evidence**: What in the code supports your verdict (cite file:line when possible)
3. **Recommendation**: What the reviewer should do next

Be concise. If the concern is already addressed in the current diff, say so clearly."#;

/// One-click Ask AI on a question: investigate and answer. "Validate" does not
/// fit a question — there is nothing to confirm or refute.
const ANSWER_QUESTION_AI_PROMPT: &str = r#"Answer and elaborate on this review question against the current code.

Use Read / grep / rg in the repository (see system context for repo_root and diff excerpt) before answering. Up to ~10 reads; cite file:line evidence.

Reply in markdown with:
1. **Answer**: A direct answer to the question.
2. **Details**: Relevant context from the code (cite file:line when possible).
3. **Follow-ups**: Anything the reviewer should double-check or decide.

Be concise and concrete."#;

/// One-click Ask AI on a note. A note is an instruction for a coding agent, so
/// the useful answer is whether the change is right and what it would touch —
/// not the change itself, which stays the agent's job once the note is sent.
const ANSWER_NOTE_AI_PROMPT: &str = r#"Assess this review note against the current code. The note is an instruction the reviewer intends to hand to a coding agent.

Use Read / grep / rg in the repository (see system context for repo_root and diff excerpt) before answering. Up to ~10 reads; cite file:line evidence. Do not edit any file.

Reply in markdown with:
1. **Assessment**: Whether the requested change is right, and why.
2. **Details**: What the change would touch (cite file:line when possible).
3. **Risks**: Anything that could break, or that the note leaves undecided.

Be concise and concrete."#;

/// Elaborate on a question: rewrite it, do not answer it. A terse question
/// written mid-review ("why fetch-depth?") only makes sense next to the line it
/// sits on; the rewrite has to survive export and being read by someone, or
/// some agent, who never saw that line.
const ELABORATE_QUESTION_AI_PROMPT: &str = r#"Rewrite this review question so it stands on its own, without the diff beside it.

Use Read / grep / rg in the repository (see system context for repo_root and diff excerpt) to learn what the question refers to. Up to ~10 reads.

The rewritten question must:
- name the file and the lines or symbol it is about;
- state the relevant facts from the code that make the question worth asking;
- say what a good answer has to settle.

Do not answer the question. Keep the reviewer's intent and tone; do not add concerns they did not raise.
Reply with only the rewritten question in markdown — no preamble, no heading."#;

/// Elaborate on a note: rewrite it into an instruction a coding agent can carry
/// out without the review open. Same reason as the question variant; the note
/// variant names the change and its outcome instead of what to settle.
const ELABORATE_NOTE_AI_PROMPT: &str = r#"Rewrite this review note into a self-contained instruction for a coding agent that will not see the diff.

Use Read / grep / rg in the repository (see system context for repo_root and diff excerpt) to learn what the note refers to. Up to ~10 reads. Do not edit any file.

The instruction must:
- name the file and the lines, job, function or other symbol to change;
- state the exact change to make;
- state the expected outcome, and anything that must stay unchanged when the note implies it.

Keep the reviewer's intent; do not widen the change beyond what the note asks.
Reply with only the instruction in markdown — no preamble, no heading."#;

/// The prompt for a one-click AI action on a question or note, keyed on the
/// thread id prefix the stores use (`q-` question, `n-` note). `None` for any
/// other thread: Ask AI and Elaborate act on the private stores only.
pub(crate) fn card_action_prompt(thread_id: &str, ai_action: &str) -> Option<&'static str> {
    let is_note = thread_id.starts_with("n-");
    if !is_note && !thread_id.starts_with("q-") {
        return None;
    }
    match (ai_action, is_note) {
        (er_engine::ai::AI_ACTION_ELABORATE, false) => Some(ELABORATE_QUESTION_AI_PROMPT),
        (er_engine::ai::AI_ACTION_ELABORATE, true) => Some(ELABORATE_NOTE_AI_PROMPT),
        (er_engine::ai::AI_ACTION_ASK, false) => Some(ANSWER_QUESTION_AI_PROMPT),
        (er_engine::ai::AI_ACTION_ASK, true) => Some(ANSWER_NOTE_AI_PROMPT),
        _ => None,
    }
}

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

/// The desktop state plus what only the App path reads: the App, and the
/// snapshot poll's record of what it last sent. Field reads reach the
/// [`DesktopState`] it wraps through `Deref`.
#[derive(Clone)]
pub struct AppState {
    pub desktop: DesktopState,
    pub app: Arc<Mutex<App>>,
    /// Last content revision included in a poll response.
    pub last_sent_content_revision: Arc<AtomicU64>,
    /// Last chrome revision included in a poll response.
    pub last_sent_chrome_revision: Arc<AtomicU64>,
    /// Last reviewed_revision included in a poll response.
    pub last_sent_reviewed_revision: Arc<AtomicU64>,
    /// Differential snapshots: per-file content keys the frontend currently
    /// holds, so unchanged hunks can be omitted from later snapshots.
    pub sent_files: crate::snapshot::SentFilesHandle,
}

impl AppState {
    /// `desktop` beside `app`, with nothing sent to the frontend yet.
    pub fn new(desktop: DesktopState, app: Arc<Mutex<App>>) -> Self {
        Self {
            desktop,
            app,
            last_sent_content_revision: Arc::new(AtomicU64::new(u64::MAX)),
            last_sent_chrome_revision: Arc::new(AtomicU64::new(u64::MAX)),
            last_sent_reviewed_revision: Arc::new(AtomicU64::new(u64::MAX)),
            sent_files: Arc::default(),
        }
    }
}

impl std::ops::Deref for AppState {
    type Target = DesktopState;

    fn deref(&self) -> &DesktopState {
        &self.desktop
    }
}

impl std::ops::DerefMut for AppState {
    fn deref_mut(&mut self) -> &mut DesktopState {
        &mut self.desktop
    }
}

pub use er_engine::pr_open_entry::{
    evict_lru, pr_open_clock, PrOpenCacheEntry, PrOpenCacheKey, PrOpenFreshness,
    MAX_PR_OPEN_CACHE_ENTRIES, PR_COMMIT_CACHE_LIMIT,
};

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

macro_rules! snap {
    ($state:expr) => {{
        let app = $state.app.lock().map_err(|e| e.to_string())?;
        Ok(snap_from(&app, &$state))
    }};
}

/// Build a snapshot using the lock guards directly (when callers already hold them).
/// Differential: hunks the frontend already holds are omitted (`hunks_omitted`).
pub fn snap_from(app: &App, state: &AppState) -> AppSnapshot {
    send_app_notice(app, state);
    crate::snapshot::build_snapshot_with_delta(app, &snapshot_sources(state))
}

fn snap_from_with_arbiter(app: &App, state: &AppState, overlay: Option<&[u8]>) -> AppSnapshot {
    send_app_notice(app, state);
    let mut sources = snapshot_sources(state);
    sources.other_bucket_arbiter = Some(overlay);
    crate::snapshot::build_snapshot_with_delta(app, &sources)
}

fn snapshot_sources(state: &AppState) -> crate::snapshot::SnapshotSources<'_> {
    crate::snapshot::SnapshotSources {
        other_bucket_arbiter: None,
        pr_cache: Some(&state.pr_cache),
        pr_cache_fetched_at: Some(&state.pr_cache_fetched_at),
        meta_cache: Some(&state.meta_cache),
        gh_user: Some(&state.gh_user),
        pending_ai: Some(state.pending_ai_replies.replies()),
        gh_status_cache: Some(&state.gh_status_cache),
        loading: Some(&state.loading),
        watch_status: Some(&state.watch_status),
        inbox: Some(&state.inbox),
        // With the flag on the page fetches hunks by hash, so nothing records
        // which hunks it holds.
        sent_files: None,
        branch_base_remote_oid: Some(&state.branch_base_remote_oid),
        view: Some(&state.view),
    }
}

/// Skip the sidecar write when the active view is no longer the one the
/// frontend painted. Returns a snapshot of the *current* view and does not
/// toast; the client rolls back the origin paint.
fn abort_wrong_view(
    app: &App,
    state: &AppState,
    view: Option<&crate::snapshot::OptimisticView>,
) -> Option<Result<AppSnapshot, String>> {
    let expected = view?;
    if crate::snapshot::optimistic_view_matches(app, expected) {
        return None;
    }
    Some(Ok(snap_from(app, state)))
}

/// Build a full snapshot for a tab-switch / open command and invalidate poll
///
/// `last_sent_*` so the next poll emits a clean full content snapshot for the
/// new view (does not merely align to the current revision).
pub fn snap_from_command(app: &App, state: &AppState) -> AppSnapshot {
    let snap = snap_from(app, state);
    let content = with_service_view(state, app, compute_content_revision(app));
    let chrome = compute_chrome_revision(state);
    // wrapping_add(1) guarantees content != last_sent on the next poll even
    // when the hash is unchanged, forcing a full content snapshot.
    state
        .last_sent_content_revision
        .store(content.wrapping_add(1), Ordering::Relaxed);
    state
        .last_sent_chrome_revision
        .store(chrome.wrapping_add(1), Ordering::Relaxed);
    state
        .last_sent_reviewed_revision
        .store(app.tab().reviewed_revision, Ordering::Relaxed);
    snap
}

/// Full snapshot for a mutation the frontend already painted optimistically
/// (thread and finding writes). Records the revisions as sent, the same way
/// `poll_impl` does after a send, so the follow-up poll is a `poll_skip`
/// instead of rebuilding and re-sending the identical content a second time.
pub fn snap_from_confirmed(app: &App, state: &AppState) -> AppSnapshot {
    let t0 = std::time::Instant::now();
    let overlay = other_bucket_arbiter(app);
    let snap = snap_from_with_arbiter(app, state, overlay.as_deref());
    state.last_sent_content_revision.store(
        with_service_view(
            state,
            app,
            compute_content_revision_with_arbiter(app, overlay.as_deref()),
        ),
        Ordering::Relaxed,
    );
    state
        .last_sent_chrome_revision
        .store(compute_chrome_revision(state), Ordering::Relaxed);
    state
        .last_sent_reviewed_revision
        .store(app.tab().reviewed_revision, Ordering::Relaxed);
    // Attributes a `build_snapshot` line to a thread/finding write so the
    // cost of confirming one is visible next to the poll's own builds.
    crate::profile_log::profile_log(
        "thread_write_confirm",
        &[
            ("snap_ms", t0.elapsed().as_millis().to_string()),
            ("files", snap.files.len().to_string()),
        ],
    );
    snap
}

/// First-paint snapshot for hot open paths (two-phase open): full chrome
/// (tabs/projects/mode/branch/base) + PR card, but no diff files, AI, or
/// annotations, with `bg_loading.tab_diff` set so the frontend renders the
/// "Loading diff…" pane. The background offload worker
/// ([`kick_post_open_offload`]) then bumps the revision and the poll delivers
/// the full snapshot within ~40–120 ms.
pub fn lite_snap_from_command(app: &App, state: &AppState) -> AppSnapshot {
    let mut snap = chrome_snap_from(app, state);
    snap.pr = crate::snapshot::build_pr_snapshot(app.tab());
    snap.bg_loading.tab_diff = true;
    snap_from_command_invalidate(app, state);
    snap
}

fn snap_from_command_invalidate(app: &App, state: &AppState) {
    let content = with_service_view(state, app, compute_content_revision(app));
    let chrome = compute_chrome_revision(state);
    state
        .last_sent_content_revision
        .store(content.wrapping_add(1), Ordering::Relaxed);
    state
        .last_sent_chrome_revision
        .store(chrome.wrapping_add(1), Ordering::Relaxed);
    state
        .last_sent_reviewed_revision
        .store(app.tab().reviewed_revision, Ordering::Relaxed);
}

fn chrome_snap_from(app: &App, state: &AppState) -> AppSnapshot {
    send_app_notice(app, state);
    build_chrome_snapshot(app, &snapshot_sources(state))
}

/// With the flag on the page shows the App's notification only as a notice,
/// so every snapshot an App command builds sends it, under the lock the
/// command holds, and answers what it sent. P62.2 deletes this with the
/// App's commands.
fn send_app_notice(app: &App, state: &AppState) -> Option<er_protocol::Notice> {
    let notice = app_notice(app.notification.as_ref(), &state.session.app_notice_seq)?;
    let handle = state
        .tauri_app_handle
        .lock()
        .ok()
        .and_then(|handle| handle.clone());
    if let Some(handle) = handle {
        crate::resources::notice::emit(&handle, &notice);
    }
    Some(notice)
}

/// The App's notification as a notice, once: `sent` holds the engine's
/// sequence number of the last one sent, and the App keeps its last
/// notification set, so every later snapshot sees it again.
pub(crate) fn app_notice(
    notification: Option<&er_engine::app::Notification>,
    sent: &AtomicU64,
) -> Option<er_protocol::Notice> {
    let notification = notification?;
    let before = sent.fetch_max(notification.seq, Ordering::AcqRel);
    (before < notification.seq).then(|| {
        crate::resources::notice::engine_notice(notification.message.clone(), notification.long)
    })
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

pub(crate) fn now_ms() -> u64 {
    crate::inbox::now_epoch_ms()
}

/// Show native notifications for inbox items that were created before the Tauri
/// `AppHandle` was stored (startup PR refresh races setup in release builds).
pub fn flush_pending_native_notifications(
    inbox_handle: &InboxHandle,
    app_handle_state: &Arc<Mutex<Option<tauri::AppHandle>>>,
    prefs: &InboxConfig,
) {
    let pending: Vec<InboxItem> = {
        let Ok(inbox) = inbox_handle.lock() else {
            return;
        };
        inbox
            .items
            .iter()
            .filter(|item| {
                prefs.notifies(&item.kind) && !inbox.notified_item_ids.contains(&item.id)
            })
            .cloned()
            .collect()
    };
    for item in &pending {
        maybe_send_native_notification(inbox_handle, app_handle_state, prefs, item);
    }
}

fn show_native_notification(app: Option<&tauri::AppHandle>, title: &str, body: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        let _ = app;
        crate::native_notify::show(title, body)
    }
    #[cfg(not(target_os = "macos"))]
    {
        match app {
            Some(app) => app
                .notification()
                .builder()
                .title(title)
                .body(body)
                .show()
                .is_ok(),
            None => {
                log::debug!("native notification skipped (no AppHandle yet)");
                false
            }
        }
    }
}

fn maybe_send_native_notification(
    inbox_handle: &InboxHandle,
    app_handle_state: &Arc<Mutex<Option<tauri::AppHandle>>>,
    prefs: &InboxConfig,
    item: &InboxItem,
) {
    if !prefs.notifies(&item.kind) {
        return;
    }
    let Ok(mut inbox) = inbox_handle.lock() else {
        return;
    };
    if inbox.notified_item_ids.contains(&item.id) {
        return;
    }
    let handle = app_handle_state.lock().ok().and_then(|g| g.clone());
    let shown = show_native_notification(handle.as_ref(), &item.title, &item.body);
    if shown {
        inbox.notified_item_ids.insert(item.id.clone());
    } else {
        log::debug!("native notification not delivered for {}", item.id);
    }
}

/// Fetch a PR's status and store it, bumping the revision when stored. Blocks
/// on `gh`, so call it off the main thread. Every writer of the status cache
/// goes through here: the ticket taken before the fetch is what keeps an
/// older fetch from overwriting a newer one, and a writer that inserted
/// directly would bring that race back. Persisting is left to the caller.
/// Returns whether the entry was stored.
pub fn fetch_and_store_github_status(
    cache: &Mutex<HashMap<(String, String, u64), GithubStatusSnapshot>>,
    desktop_revision: &AtomicU64,
    owner: &str,
    repo: &str,
    number: u64,
) -> bool {
    let ticket = crate::gh_status_cache::begin_fetch();
    let Some(snap) = fetch_github_status(owner, repo, number) else {
        return false;
    };
    let key = (owner.to_string(), repo.to_string(), number);
    let stored = crate::gh_status_cache::store_fetched(cache, key, ticket, snap);
    if stored {
        crate::profile_log::bump_desktop_revision(desktop_revision, "gh_status_cache");
    }
    stored
}

/// Spawn a background fetch of the GitHub status for the given (owner, repo, number).
///
/// Returns immediately. The cache is updated on success; failures are logged.
/// Deduplicates: if a fetch for the same key is already in-flight, this is a no-op.
pub fn kick_github_status_refresh(
    desktop: &DesktopState,
    owner: String,
    repo: String,
    number: u64,
) {
    let key = (owner.clone(), repo.clone(), number);
    let Some(fetch) = desktop.begin_status_fetch(&key) else {
        return; // already fetching
    };
    let desktop = desktop.clone();
    std::thread::spawn(move || {
        let _fetch = fetch;
        let cache = &desktop.gh_status_cache;
        if fetch_and_store_github_status(cache, &desktop.desktop_revision, &owner, &repo, number) {
            // Persist after the lock is released (the save helper re-locks).
            crate::gh_status_cache::save_persisted_gh_status_cache(cache);
        }
    });
}

pub(crate) fn active_github_key(app: &App, state: &AppState) -> Option<(String, String, u64)> {
    let tab = app.tab();
    // Prefer the tab's own PR number (remote or local PR tab) so the background
    // gh-status fetch targets the PR that was actually opened, not an arbitrary
    // head_ref match when a branch carries more than one open PR. Plain branch /
    // working tabs fall back to head_ref matching inside the resolver.
    let cache = state.pr_cache.lock().ok()?;
    crate::snapshot::resolve_github_status_key(tab, &cache)
}

#[cfg(test)]
fn own_pr_approval_error() -> String {
    "GitHub does not allow approving your own pull request.".to_string()
}

#[cfg(test)]
fn is_own_pr_approval_error(raw: &str) -> bool {
    let lower = raw.to_lowercase();
    lower.contains("can not approve your own pull request")
        || lower.contains("cannot approve your own pull request")
}

#[cfg(test)]
fn is_one_pending_review_error(raw: &str) -> bool {
    raw.to_lowercase().contains("one pending review")
}

/// Files finished background tasks of the flag path in the inbox, each
/// under its own target's project.
pub(crate) fn file_tasks_in_inbox(state: &DesktopState, tasks: &[er_protocol::TaskJob]) {
    let config = crate::arena_service::config(state);
    let rows = tasks.iter().map(|task| {
        let row = InboxTask {
            id: &task.id,
            kind: &task.kind,
            status: &task.status,
            target_label: &task.target_label,
            label: &task.label,
            error: task.error.as_deref(),
        };
        let target = InboxTarget {
            project_id: projects::resolve_project_id_for_inbox(
                None,
                Some(task.repo_root.as_str()),
                task.remote_repo.as_deref(),
            ),
            repo_root: Some(task.repo_root.clone()),
            remote: task.remote_repo.clone(),
            pr_number: task.pr_number,
            branch: Some(task.branch_label.clone()),
            url: None,
        };
        (row, target)
    });
    file_in_inbox(state, &config.inbox, rows);
}

/// One background task as the inbox files it.
pub(crate) struct InboxTask<'a> {
    id: &'a str,
    kind: &'a str,
    status: &'a str,
    target_label: &'a str,
    label: &'a str,
    error: Option<&'a str>,
}

fn file_in_inbox<'a>(
    state: &DesktopState,
    config: &er_engine::config::InboxConfig,
    tasks: impl Iterator<Item = (InboxTask<'a>, InboxTarget)>,
) {
    let now = now_ms();
    let mut emitted_any = false;
    let mut just_added: Vec<InboxItem> = Vec::new();
    if let Ok(mut inbox) = state.inbox.lock() {
        for (task, target) in tasks {
            let failure = |fallback: &str| task.error.unwrap_or(fallback).to_string();
            let (kind, severity, title, body) = match task.status {
                "done" if task.kind == "triage" => (
                    "ai_triage_done".to_string(),
                    "success".to_string(),
                    format!("Triage completed ({})", task.target_label),
                    task.label.to_string(),
                ),
                "done" => (
                    "ai_review_done".to_string(),
                    "success".to_string(),
                    format!("AI review completed ({})", task.target_label),
                    task.label.to_string(),
                ),
                "failed" if task.kind == "triage" => (
                    "ai_triage_failed".to_string(),
                    "error".to_string(),
                    format!("Triage failed ({})", task.target_label),
                    failure("Triage failed"),
                ),
                "failed" => (
                    "ai_review_failed".to_string(),
                    "error".to_string(),
                    format!("AI review failed ({})", task.target_label),
                    failure("Review failed"),
                ),
                _ => continue,
            };
            if !config.stores(&kind) {
                continue;
            }
            let item = InboxItem {
                id: format!("inbox-ai-{}-{}", task.id, task.status),
                kind,
                severity,
                title,
                body,
                source: "ai".to_string(),
                target,
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
        maybe_send_native_notification(&state.inbox, &state.tauri_app_handle, config, item);
    }
    if emitted_any {
        crate::inbox::save_inbox_state(&state.inbox);
        state.desktop_revision.fetch_add(1, Ordering::Relaxed);
    }
}

/// Fetch all GitHub status data for a PR. Runs 2 gh calls in parallel: one
///
/// combined `gh pr view --json` for overview+comments+reviews, one `gh pr
/// checks` for CI status (separate subcommand, can't be merged into `pr view`).
/// Returns None when the overview/status-bundle fetch fails (e.g. no network,
/// gh not authed). Comments/reviews parse failures inside the bundle are
/// non-fatal; checks failures are non-fatal — the snapshot still populates.
pub fn fetch_github_status(owner: &str, repo: &str, number: u64) -> Option<GithubStatusSnapshot> {
    let t = std::time::Instant::now();
    // The independent lookups run concurrently. The branch and merge-queue
    // lookups after this need the PR state, so they wait for the bundle.
    let (bundle_res, checks, repo_merge) = std::thread::scope(|s| {
        let b = s.spawn(|| er_engine::github::gh_pr_status_remote(owner, repo, number));
        let c = s.spawn(|| {
            er_engine::github::gh_pr_checks_remote(owner, repo, number).unwrap_or_default()
        });
        // Cached per repo, so this is usually no subprocess at all.
        let r = s.spawn(|| er_engine::gh_pr_actions::cached_repo_merge_settings(owner, repo));
        (
            b.join().ok(),
            c.join().unwrap_or_default(),
            r.join().ok().flatten(),
        )
    });
    let bundle = bundle_res?.ok()?;
    let overview = bundle.overview;
    let head_branch_exists =
        head_branch_exists_after_close(owner, repo, &overview, repo_merge.as_ref());
    let (base_has_merge_queue, in_merge_queue) = merge_queue_status(owner, repo, number, &overview);
    let can_bypass_rules = can_bypass_rules(owner, repo, number, &overview, base_has_merge_queue);
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

    let check_summaries: Vec<CheckSummary> = checks.into_iter().map(CheckSummary::from).collect();
    let reviewers = crate::snapshot::github_reviewers(&overview, &reviews);

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
        merge_state_status: overview.merge_state_status,
        head_oid: overview.head_ref_oid,
        is_cross_repository: overview.is_cross_repository,
        auto_merge_method: overview.auto_merge_method,
        head_branch_exists,
        repo_merge: repo_merge.map(Into::into),
        in_merge_queue,
        base_has_merge_queue,
        can_bypass_rules,
        created_at: overview.created_at,
        commit_count: overview.commit_count,
        reviewers,
        fetch_ticket: 0,
    })
}

/// Delete/Restore branch only make sense once a same-repo PR is merged or
/// closed, so the extra `gh api` lookup runs only then. A fork's branch is
/// never looked up in the base repo: a same-named branch there is a different
/// branch (ADR 0040).
fn head_branch_exists_after_close(
    owner: &str,
    repo: &str,
    overview: &er_engine::github::PrOverviewFull,
    repo_merge: Option<&er_engine::gh_pr_actions::RepoMergeSettings>,
) -> Option<bool> {
    if overview.state == "OPEN" || overview.head_ref_name.is_empty() || overview.is_cross_repository
    {
        return None;
    }
    // GitHub deletes the branch a moment after the merge on such repos.
    let deleting_itself =
        overview.state == "MERGED" && repo_merge.is_some_and(|s| s.delete_branch_on_merge);
    er_engine::gh_pr_actions::cached_remote_branch_exists(
        owner,
        repo,
        &overview.head_ref_name,
        deleting_itself,
    )
}

/// `(base has a merge queue, PR is in it)` for an open PR. Queue presence is
/// cached per repo and base; the membership lookup costs a `gh` call, so it
/// runs only on a base that has a queue.
fn merge_queue_status(
    owner: &str,
    repo: &str,
    number: u64,
    overview: &er_engine::github::PrOverviewFull,
) -> (bool, bool) {
    use er_engine::gh_pr_actions::{cached_base_has_merge_queue, gh_pr_in_merge_queue};
    let has_queue = overview.state == "OPEN"
        && cached_base_has_merge_queue(owner, repo, &overview.base_ref_name) == Some(true);
    let queued = has_queue && gh_pr_in_merge_queue(owner, repo, number).unwrap_or(false);
    (has_queue, queued)
}

/// Only a blocked PR can use a bypass, so other states skip the lookup. A
/// merge-queue base is skipped too: `--admin` there merges past the queue.
fn can_bypass_rules(
    owner: &str,
    repo: &str,
    number: u64,
    overview: &er_engine::github::PrOverviewFull,
    base_has_merge_queue: bool,
) -> bool {
    overview.state == "OPEN"
        && !base_has_merge_queue
        && overview.merge_state_status.as_deref() == Some("BLOCKED")
        && er_engine::gh_pr_actions::cached_viewer_can_bypass(
            owner,
            repo,
            &overview.base_ref_name,
            number,
        ) == Some(true)
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
        kick_github_status_refresh(state, owner, repo, number);
    }
}

/// Kick off an async meta-cache refresh so the *next* poll reflects the
/// mutation that just happened. The refresh runs on a background thread and
/// does NOT touch the App mutex.
pub(crate) fn kick_meta_refresh(state: &DesktopState, root: String) {
    let owner = Arc::clone(&state.app_owner);
    let desktop_revision = Arc::clone(&state.desktop_revision);
    let ports = state.ports.clone();
    desktop_revision.fetch_add(1, Ordering::Relaxed);
    std::thread::spawn(move || {
        owner.refresh_meta(&*ports.git, &root);
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
pub async fn toggle_panel(panel: String, state: State<'_, AppState>) -> Result<(), String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let mut view = state.view.lock().map_err(|e| e.to_string())?;
        view.toggle_panel(&panel);
        Ok(())
    })
    .await
}

// ── Navigation ────────────────────────────────────────────────────────────────

/// Parse one or more lazy-stub files and return *only those* `FileSnapshot`s
///
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
    // Flag-on, the parsed files reach the page with the snapshot that reads
    // the content, so this answers no files.
    run_blocking(move || {
        content_routes::request_file_content(&state, &source_indices).map(|()| Vec::new())
    })
    .await
}

#[tauri::command]
pub async fn select_file(idx: usize, state: State<'_, AppState>) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || content_routes::select_file(&state, idx)).await
}

#[derive(serde::Serialize)]
pub struct FilePreviewResponse {
    path: String,
    text: String,
    preview_context_key: String,
    preview_key: String,
}

#[tauri::command]
pub async fn request_file_preview(
    path: String,
    expected_preview_context_key: String,
    state: State<'_, AppState>,
) -> Result<FilePreviewResponse, String> {
    let state = state.inner().clone();
    run_blocking(move || file_preview(&state, path, expected_preview_context_key)).await
}

/// A document's text in the active view. With the flag on, a view the
/// workspace loads is read from its content, never the App's tab.
fn file_preview(
    state: &AppState,
    path: String,
    expected_preview_context_key: String,
) -> Result<FilePreviewResponse, String> {
    if let Some(entry) = crate::resources::preview::active(state) {
        let preview = crate::resources::preview::document(
            state,
            &entry,
            &path,
            &expected_preview_context_key,
        )
        .map_err(|e| e.message)?;
        return Ok(FilePreviewResponse {
            path: preview.path,
            text: preview.text,
            preview_context_key: preview.preview_context_key,
            preview_key: preview.preview_key,
        });
    }
    {
        let (active_tab, request) = {
            let app = state.app.lock().map_err(|e| e.to_string())?;
            if app.tab().preview_context_key() != expected_preview_context_key {
                return Err("Document preview context changed".to_string());
            }
            (
                app.active_tab,
                app.tab()
                    .capture_file_preview(&path)
                    .map_err(|e| e.to_string())?,
            )
        };
        let text = request.read(&*state.ports.git).map_err(|e| e.to_string())?;
        {
            let app = state.app.lock().map_err(|e| e.to_string())?;
            if app.active_tab != active_tab
                || app.tab().preview_context_key() != request.preview_context_key
                || app.tab().file_preview_key(&path) != request.preview_key
            {
                return Err("Document preview context changed".to_string());
            }
        }
        Ok(FilePreviewResponse {
            path,
            text,
            preview_context_key: request.preview_context_key,
            preview_key: request.preview_key,
        })
    }
}

#[derive(serde::Serialize)]
pub struct ImagePreviewResponse {
    path: String,
    /// `data:` URLs, so the webview needs no file access to show them.
    before: Option<String>,
    after: Option<String>,
    preview_context_key: String,
    preview_key: String,
}

pub(crate) fn image_data_url(mime: &str, bytes: &[u8]) -> String {
    use base64::Engine as _;
    format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

#[tauri::command]
pub async fn request_image_preview(
    path: String,
    expected_preview_context_key: String,
    state: State<'_, AppState>,
) -> Result<ImagePreviewResponse, String> {
    let state = state.inner().clone();
    run_blocking(move || image_preview(&state, path, expected_preview_context_key)).await
}

/// Both sides of an image change in the active view, read as
/// `file_preview` reads a document.
fn image_preview(
    state: &AppState,
    path: String,
    expected_preview_context_key: String,
) -> Result<ImagePreviewResponse, String> {
    if let Some(entry) = crate::resources::preview::active(state) {
        let preview =
            crate::resources::preview::image(state, &entry, &path, &expected_preview_context_key)
                .map_err(|e| e.message)?;
        return Ok(ImagePreviewResponse {
            path: preview.path,
            before: preview.before,
            after: preview.after,
            preview_context_key: preview.preview_context_key,
            preview_key: preview.preview_key,
        });
    }
    {
        let mime = er_engine::app::image_mime(&path).ok_or("This file is not an image")?;
        let (active_tab, request) = {
            let app = state.app.lock().map_err(|e| e.to_string())?;
            if app.tab().preview_context_key() != expected_preview_context_key {
                return Err("Image preview context changed".to_string());
            }
            (
                app.active_tab,
                app.tab()
                    .capture_image_preview(&path)
                    .map_err(|e| e.to_string())?,
            )
        };
        let read = |side: &Option<er_engine::app::FilePreviewRequest>| {
            side.as_ref()
                .map(|r| r.read_bytes(&*state.ports.git, er_engine::app::MAX_IMAGE_PREVIEW_BYTES))
                .transpose()
        };
        let after = read(&request.after).map_err(|e| e.to_string())?;
        // The earlier side is context. A failed read leaves the current side alone.
        let before = match read(&request.before) {
            Ok(bytes) => bytes,
            Err(e) if after.is_some() => {
                log::debug!("image preview: earlier side of {path} unreadable: {e}");
                None
            }
            Err(e) => return Err(e.to_string()),
        };
        let keys = request
            .after
            .as_ref()
            .or(request.before.as_ref())
            .ok_or("Image has no side to show")?;
        let (preview_context_key, preview_key) =
            (keys.preview_context_key.clone(), keys.preview_key.clone());
        {
            let app = state.app.lock().map_err(|e| e.to_string())?;
            if app.active_tab != active_tab
                || app.tab().preview_context_key() != preview_context_key
                || app.tab().file_preview_key(&path) != preview_key
            {
                return Err("Image preview context changed".to_string());
            }
        }
        Ok(ImagePreviewResponse {
            before: before.map(|b| image_data_url(mime, &b)),
            after: after.map(|b| image_data_url(mime, &b)),
            path,
            preview_context_key,
            preview_key,
        })
    }
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
    tab_idx: Option<usize>,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || set_mode_impl(&state, &mode, pr_number, tab_idx)).await
}

fn set_mode_impl(
    state: &AppState,
    mode: &str,
    pr_number: Option<u64>,
    tab_idx: Option<usize>,
) -> Result<AppSnapshot, String> {
    // Every view the workspace loads goes to the active tab's actor.
    if let Some(diff_mode) = content_routes::routed_mode(mode) {
        return content_routes::set_mode(state, mode, diff_mode, pr_number, tab_idx);
    }
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    if !feature_allows_mode_str(&app.config.features, mode) {
        return Err(format!("'{mode}' view is disabled in settings"));
    }
    if let Some(idx) = tab_idx {
        if app.active_tab != idx {
            return Ok(snap_from_command(&app, state));
        }
    }
    let t_switch = std::time::Instant::now();
    if matches!(mode, "pr" | "pr_diff") {
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
        return Ok(set_mode_snapshot(&app, state, mode, t_switch));
    }
    let diff_mode = match mode {
        "unstaged" => DiffMode::Unstaged,
        "staged" => DiffMode::Staged,
        "history" => DiffMode::History,
        "conflicts" => DiffMode::Conflicts,
        "hidden" => DiffMode::Hidden,
        "tour" => DiffMode::Tour,
        _ => DiffMode::Branch,
    };
    app.tab_mut().set_mode(diff_mode);
    Ok(set_mode_snapshot(&app, state, mode, t_switch))
}

/// The snapshot a mode switch returns, with the switch and the snapshot build
/// logged apart: a slow switch is either a re-fetched diff or a large snapshot,
/// and the fix for each is different.
fn set_mode_snapshot(
    app: &App,
    state: &AppState,
    mode: &str,
    t_switch: std::time::Instant,
) -> AppSnapshot {
    let switch_ms = t_switch.elapsed().as_millis();
    let t_snap = std::time::Instant::now();
    let snap = snap_from_command(app, state);
    crate::profile_log::profile_log(
        "set_mode",
        &[
            ("mode", mode.to_string()),
            ("switch_ms", switch_ms.to_string()),
            ("snap_ms", t_snap.elapsed().as_millis().to_string()),
            ("files", snap.files.len().to_string()),
        ],
    );
    snap
}

/// Change the compare/base branch the active tab diffs against. Only valid on
/// local working-tree / local-branch / local-PR tabs; remote PR diffs come from
/// GitHub and cannot be re-based client-side. Persists the new base so a
/// restart restores the same comparison.
#[tauri::command]
pub async fn change_base(
    branch: String,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        app.tab_mut()
            .set_base_branch(&branch)
            .map_err(|e| e.to_string())?;
        crate::tabs::persist_app_tabs(&app, &state.view);
        Ok(snap_from_command(&app, &state))
    })
    .await
}

// ── Reviewed state ────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn toggle_reviewed(
    path: String,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        content_routes::write_reviewed(&state, |context| er_service::Command::ToggleReviewed {
            context,
            path,
        })
    })
    .await
}

#[tauri::command]
pub async fn mark_reviewed(
    path: String,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        content_routes::write_reviewed(&state, |context| er_service::Command::MarkReviewed {
            context,
            path,
            reviewed: true,
        })
    })
    .await
}

#[tauri::command]
pub async fn unmark_reviewed(
    path: String,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        content_routes::write_reviewed(&state, |context| er_service::Command::MarkReviewed {
            context,
            path,
            reviewed: false,
        })
    })
    .await
}

#[tauri::command]
pub async fn bulk_review_pillar(
    pillar_id: String,
    view: Option<crate::snapshot::OptimisticView>,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        route_annotation(&state, view.as_ref(), |context| {
            er_service::Command::BulkReviewPillar {
                context,
                pillar_id,
                reviewed: true,
            }
        })
    })
    .await
}

#[tauri::command]
pub async fn unbulk_review_pillar(
    pillar_id: String,
    view: Option<crate::snapshot::OptimisticView>,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        route_annotation(&state, view.as_ref(), |context| {
            er_service::Command::BulkReviewPillar {
                context,
                pillar_id,
                reviewed: false,
            }
        })
    })
    .await
}

/// Result of comparing the running desktop build to GitHub Releases latest.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AppUpdateInfo {
    pub current: String,
    pub latest: Option<String>,
    pub update_available: bool,
    pub release_url: Option<String>,
}

const UPDATE_CACHE_TTL: std::time::Duration = std::time::Duration::from_hours(1);

fn fetch_latest_release() -> Result<er_engine::release_update::LatestRelease, String> {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(8)))
        .build();
    let agent = ureq::Agent::new_with_config(config);
    let mut response = agent
        .get(er_engine::release_update::RELEASES_LATEST_API)
        .header("User-Agent", "easy-review-desktop")
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("GitHub releases request failed: {e}"))?;
    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("GitHub releases body read failed: {e}"))?;
    er_engine::release_update::parse_latest_release(&text).map_err(|e| e.to_string())
}

fn check_app_update_inner() -> AppUpdateInfo {
    let current = env!("CARGO_PKG_VERSION").to_string();
    match fetch_latest_release() {
        Ok(release) => {
            let latest = release.version().to_string();
            let update_available = er_engine::release_update::version_is_newer(&latest, &current);
            AppUpdateInfo {
                current,
                latest: Some(latest),
                update_available,
                release_url: Some(release.html_url),
            }
        }
        Err(err) => {
            log::debug!("check_app_update: {err}");
            AppUpdateInfo {
                current,
                latest: None,
                update_available: false,
                release_url: None,
            }
        }
    }
}

/// Compare the running app version to GitHub Releases latest (1h cache).
#[tauri::command]
pub async fn check_app_update() -> Result<AppUpdateInfo, String> {
    tauri::async_runtime::spawn_blocking(app_update_cached)
        .await
        .map_err(|e| format!("check_app_update join: {e}"))
}

/// The update check behind a 1 h cache, shared by the command and the wire's
/// `check_app_update`. Blocking: a miss asks GitHub.
pub(crate) fn app_update_cached() -> AppUpdateInfo {
    use std::sync::Mutex;
    use std::time::Instant;

    struct Cache {
        at: Instant,
        info: AppUpdateInfo,
    }
    static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

    if let Ok(guard) = CACHE.lock() {
        if let Some(c) = guard.as_ref() {
            if c.at.elapsed() < UPDATE_CACHE_TTL {
                return c.info.clone();
            }
        }
    }

    let info = check_app_update_inner();

    if let Ok(mut guard) = CACHE.lock() {
        *guard = Some(Cache {
            at: Instant::now(),
            info: info.clone(),
        });
    }
    info
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

/// The shared state a PR-list refresh hands to [`process_inbox_after_pr_refresh`].
#[derive(Clone, Copy)]
pub struct InboxRefreshHandles<'a> {
    pub pr_cache: &'a Arc<Mutex<HashMap<String, Vec<PrInfo>>>>,
    pub gh_user: &'a GhUser,
    pub inbox: &'a InboxHandle,
    pub desktop_revision: &'a Arc<AtomicU64>,
    pub app_handle: &'a Arc<Mutex<Option<tauri::AppHandle>>>,
}

pub fn process_inbox_after_pr_refresh(
    handles: InboxRefreshHandles<'_>,
    prefs: &InboxConfig,
    refresh_failed_remote: Option<String>,
) {
    let InboxRefreshHandles {
        pr_cache,
        gh_user: gh_user_state,
        inbox: inbox_handle,
        desktop_revision,
        app_handle: app_handle_state,
    } = handles;
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
                if prefs.stores("github_refresh_failed") {
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
                }
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
    let mut ci_work: Vec<(String, String, u64, String, String, String)> = Vec::new();
    let mut inbox = match inbox_handle.lock() {
        Ok(g) => g,
        Err(_) => return,
    };

    let should_ingest_notifications = refresh_failed_remote.is_none();
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
            let requested_reviewers = pr.reviewers.clone();
            let is_my_pr = pr.author == gh_user;
            let prev = inbox.observed_pr.get(&key).cloned();

            if prev.is_some() {
                let view = PrInboxView {
                    number: pr.number,
                    title: pr.title.clone(),
                    head_ref: pr.head_ref.clone(),
                    state: pr.state.clone(),
                    author: pr.author.clone(),
                    requested_reviewers: requested_reviewers.clone(),
                    review_decision: pr.review_decision.clone(),
                    latest_reviewer_states: pr.latest_reviewer_states.clone(),
                };
                let project = project_by_remote.get(&remote);
                let ctx = PrTransitionCtx {
                    remote: &remote,
                    gh_user: &gh_user,
                    now_ms: now,
                    project_id: project.map(|p| p.id.clone()),
                    repo_root: project.map(|p| p.root_path.clone()),
                };
                new_items.extend(inbox_items_from_pr_transition(prev.as_ref(), &view, &ctx));
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
                    head_oid: pr.head_oid.clone(),
                    latest_reviewer_states: crate::inbox::merge_reviewer_states(
                        prev.as_ref(),
                        pr.latest_reviewer_states.clone(),
                    ),
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

    let skip_review_prs: HashSet<(String, u64)> = new_items
        .iter()
        .filter(|i| is_review_edge_kind(&i.kind))
        .filter_map(|i| Some((i.target.remote.clone()?, i.target.pr_number?)))
        .collect();

    let mut emitted_any = false;
    let mut just_added: Vec<InboxItem> = Vec::new();
    if let Ok(mut inbox) = inbox_handle.lock() {
        for item in new_items {
            if !prefs.stores(&item.kind) {
                continue;
            }
            if inbox.add_item(item.clone()) {
                emitted_any = true;
                just_added.push(item);
            }
        }
    }
    for item in &just_added {
        maybe_send_native_notification(inbox_handle, app_handle_state, prefs, item);
    }
    crate::inbox::save_inbox_state(inbox_handle);
    if emitted_any {
        crate::profile_log::bump_desktop_revision(desktop_revision, "inbox_items");
    }
    if should_ingest_notifications {
        ingest_github_notifications(
            inbox_handle,
            &project_by_remote,
            &skip_review_prs,
            now,
            desktop_revision,
            app_handle_state,
            prefs,
        );
    }
}

fn ingest_github_notifications(
    inbox_handle: &InboxHandle,
    project_by_remote: &HashMap<String, projects::ProjectRecord>,
    skip_review_prs: &HashSet<(String, u64)>,
    now: u64,
    desktop_revision: &Arc<AtomicU64>,
    app_handle_state: &Arc<Mutex<Option<tauri::AppHandle>>>,
    prefs: &InboxConfig,
) {
    let notes = match er_engine::github::gh_list_participating_notifications() {
        Ok(n) => n,
        Err(e) => {
            log::error!("[inbox] GitHub notifications unavailable: {e}");
            return;
        }
    };
    let allowed_remotes: HashSet<String> = project_by_remote
        .keys()
        .map(|r| projects::normalize_remote_slug(r))
        .collect();
    let mut emitted_any = false;
    let mut remembered_any = false;
    let mut just_added: Vec<InboxItem> = Vec::new();
    if let Ok(mut inbox) = inbox_handle.lock() {
        for gh_note in notes {
            if inbox.has_seen_notification(&gh_note.id, &gh_note.updated_at) {
                continue;
            }
            let remote = projects::normalize_remote_slug(&gh_note.repository.full_name);
            if !allowed_remotes.contains(&remote) {
                continue;
            }
            let project = project_by_remote.get(&remote).or_else(|| {
                project_by_remote.iter().find_map(|(key, p)| {
                    if projects::normalize_remote_slug(key) == remote {
                        Some(p)
                    } else {
                        None
                    }
                })
            });
            let note = InboxNotification {
                id: gh_note.id.clone(),
                reason: gh_note.reason,
                title: gh_note.subject.title,
                remote,
                pr_number: er_engine::github::pr_number_from_notification_url(&gh_note.subject.url),
                subject_type: gh_note.subject.subject_type,
            };
            let ctx = NotificationInboxCtx {
                now_ms: now,
                allowed_remotes: &allowed_remotes,
                project_id: project.map(|p| p.id.clone()),
                repo_root: project.map(|p| p.root_path.clone()),
                skip_review_prs,
            };
            if let Some(item) = inbox_item_from_notification(&note, &ctx) {
                inbox.remember_notification(&gh_note.id, &gh_note.updated_at);
                remembered_any = true;
                if prefs.stores(&item.kind) && inbox.add_item(item.clone()) {
                    emitted_any = true;
                    just_added.push(item);
                }
            } else if crate::inbox::should_remember_skipped_author(&note, skip_review_prs) {
                inbox.remember_notification(&gh_note.id, &gh_note.updated_at);
                remembered_any = true;
            }
        }
    }
    for item in &just_added {
        maybe_send_native_notification(inbox_handle, app_handle_state, prefs, item);
    }
    if emitted_any || remembered_any {
        crate::inbox::save_inbox_state(inbox_handle);
    }
    if emitted_any {
        crate::profile_log::bump_desktop_revision(desktop_revision, "inbox_notifications");
    }
}

/// Sidebar triage on the flag path: queued on the task host, with the
/// request set shown in `jobs`.
pub(crate) fn triage_ctx(desktop: &DesktopState) -> crate::auto_triage::AutoTriageContext {
    let (spawn_desktop, notify_desktop, clock) = (
        desktop.clone(),
        desktop.clone(),
        desktop.tasks.clock.clone(),
    );
    crate::auto_triage::AutoTriageContext {
        ports: desktop.ports.clone(),
        spawn: Arc::new(move |target, prompt| {
            crate::runs::spawn_reviewer(
                &spawn_desktop,
                &er_engine::ai::ReviewerKind::Triage,
                target,
                prompt,
                None,
                true,
            )
        }),
        notify: Arc::new(move |message| {
            crate::resources::notice::tell(&notify_desktop, message, false);
        }),
        in_flight: Arc::clone(&desktop.auto_triage_in_flight),
        queue_moved: Arc::new(move || clock.bump()),
        desktop_revision: Arc::clone(&desktop.desktop_revision),
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

/// Start triage for a single PR from the sidebar (does not open the review tab).
#[tauri::command]
pub fn run_pr_triage(
    project_id: String,
    pr_number: u64,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    ask_on_desktop(&state, |desktop| {
        pr_triage(desktop, &triage_ctx(desktop), &project_id, pr_number)
            .map_err(crate::tasks::refused)
    })
}

/// Queues triage of one project's PR on `ctx`.
pub(crate) fn pr_triage(
    desktop: &DesktopState,
    ctx: &crate::auto_triage::AutoTriageContext,
    project_id: &str,
    pr_number: u64,
) -> Result<(), String> {
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
    let pr = pr_from_cache(&desktop.pr_cache, &remote, pr_number)
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
    crate::auto_triage::dispatch_auto_triage(ctx, vec![req]);
    Ok(())
}

/// Triage a tracked branch: uses the open PR when `branch` is a PR head, else local branch diff.
#[tauri::command]
pub fn run_branch_triage(
    project_id: String,
    branch: String,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    ask_on_desktop(&state, |desktop| {
        branch_triage(desktop, &triage_ctx(desktop), &project_id, &branch)
            .map_err(crate::tasks::refused)
    })
}

/// Queues triage of one project's branch on `ctx`: its open PR when it has
/// one, else the branch diff.
pub(crate) fn branch_triage(
    desktop: &DesktopState,
    ctx: &crate::auto_triage::AutoTriageContext,
    project_id: &str,
    branch: &str,
) -> Result<(), String> {
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

    if let Some(pr) = open_pr_for_branch(&desktop.pr_cache, &remote, branch) {
        return pr_triage(desktop, ctx, project_id, pr.number);
    }
    crate::auto_triage::dispatch_branch_triage(
        ctx,
        &project.id,
        &remote,
        &project.root_path,
        branch,
        &project.review_ignore_globs,
    );
    Ok(())
}

/// Remove a queued (not yet started) review from the AI review queue.
/// Running reviews are unaffected.
#[tauri::command]
pub fn cancel_queued_review(id: String, state: State<AppState>) -> Result<AppSnapshot, String> {
    ask_on_desktop(&state, |desktop| crate::tasks::cancel_queued(desktop, &id))
}

/// Stop a running review.
///
/// The handle is cloned and the lock released *before* signalling: `kill`
/// forks a process, and holding the app mutex across a fork would block every
/// other command — including the poll that would show the stop took effect.
#[tauri::command]
pub fn cancel_running_review(id: String, state: State<AppState>) -> Result<AppSnapshot, String> {
    ask_on_desktop(&state, |desktop| crate::tasks::cancel_running(desktop, &id))
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
pub fn list_review_revisions(state: State<AppState>) -> Result<Vec<ReviewRevisionSummary>, String> {
    // Branch-level managed storage keeps no revisions: re-running review
    // overwrites the same files in place. The returned list is therefore at
    // most one entry representing the current branch state, so the existing UI
    // (ExportModal, AgentOutputView) works without a revision picker.
    let app = state.app.lock().map_err(|e| e.to_string())?;
    let tab = app.tab();
    Ok(review_revisions(
        &tab.repo_root,
        &tab.er_dir(),
        &tab.branch_diff_hash,
    ))
}

/// The one revision a bucket with a review holds; none without one.
pub(crate) fn review_revisions(
    repo_root: &str,
    er_dir: &str,
    branch_diff_hash: &str,
) -> Vec<ReviewRevisionSummary> {
    if repo_root.is_empty() || !er_engine::ai::review_file_exists(er_dir) {
        return Vec::new();
    }
    vec![ReviewRevisionSummary {
        revision_id: "current".to_string(),
        created_at: String::new(),
        scope: String::new(),
        diff_hash: branch_diff_hash.to_string(),
        active: true,
        agents: vec!["claude".to_string()],
    }]
}

#[tauri::command]
pub fn read_review_json(
    state: State<AppState>,
    revision_id: Option<String>,
) -> Result<String, String> {
    let _ = revision_id; // single-revision model — ignored
    let app = state.app.lock().map_err(|e| e.to_string())?;
    let tab = app.tab();
    projected_review_json(
        &tab.er_dir(),
        &tab.branch_diff_hash,
        tab.storage_branch_scope(),
    )
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
#[expect(
    clippy::too_many_arguments,
    reason = "the parameters are the JS IPC payload; a params struct would change the command contract"
)]
pub async fn add_comment(
    file: String,
    hunk_idx: usize,
    line_num: Option<usize>,
    line_num_end: Option<usize>,
    text: String,
    side: Option<String>,
    id: Option<String>,
    view: Option<crate::snapshot::OptimisticView>,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        route_annotation(&state, view.as_ref(), |context| {
            er_service::Command::AddComment {
                context,
                target: er_service::AnnotationTarget {
                    file,
                    hunk_idx,
                    line_num,
                    line_num_end,
                },
                body: text,
                side,
                client_id: id,
            }
        })
    })
    .await
}

#[tauri::command]
#[expect(
    clippy::too_many_arguments,
    reason = "the parameters are the JS IPC payload; a params struct would change the command contract"
)]
pub async fn add_question(
    file: String,
    hunk_idx: usize,
    line_num: Option<usize>,
    line_num_end: Option<usize>,
    text: String,
    side: Option<String>,
    id: Option<String>,
    view: Option<crate::snapshot::OptimisticView>,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        route_annotation(&state, view.as_ref(), |context| {
            er_service::Command::AddQuestion {
                context,
                target: er_service::AnnotationTarget {
                    file,
                    hunk_idx,
                    line_num,
                    line_num_end,
                },
                body: text,
                side,
                client_id: id,
                reply_to: None,
                finding_ref: None,
            }
        })
    })
    .await
}

#[tauri::command]
#[expect(
    clippy::too_many_arguments,
    reason = "the parameters are the JS IPC payload; a params struct would change the command contract"
)]
pub async fn add_note(
    file: String,
    hunk_idx: usize,
    line_num: Option<usize>,
    line_num_end: Option<usize>,
    text: String,
    side: Option<String>,
    id: Option<String>,
    view: Option<crate::snapshot::OptimisticView>,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        route_annotation(&state, view.as_ref(), |context| {
            er_service::Command::AddNote {
                context,
                target: er_service::AnnotationTarget {
                    file,
                    hunk_idx,
                    line_num,
                    line_num_end,
                },
                body: text,
                side,
                client_id: id,
                reply_to: None,
                finding_ref: None,
            }
        })
    })
    .await
}

/// A question or note on a document line the diff does not show, written from
/// the preview. The file text is read outside the lock, as the preview does,
/// since a remote review fetches it over the network. `preview_key` names the
/// text the line was picked in; a document that changed since would put the
/// line on different text, so the save is refused.
#[tauri::command]
pub async fn add_document_thread(
    file: String,
    kind: String,
    line_num: usize,
    line_num_end: Option<usize>,
    text: String,
    preview_key: String,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        service_add_document_thread(
            &state,
            DocumentThread {
                file,
                kind,
                line_num,
                line_num_end,
                text,
                preview_key,
            },
        )
    })
    .await
}

/// A document thread as the preview sends it.
pub(crate) struct DocumentThread {
    pub file: String,
    pub kind: String,
    pub line_num: usize,
    pub line_num_end: Option<usize>,
    pub text: String,
    pub preview_key: String,
}

/// The flag path of `add_document_thread`. The preview and its text come from
/// the App's tab, which renders it; the context comes from the registry's
/// active tab, as every annotation route's does. The App's tab key is read
/// with the preview, so while the App lags the registry the text is refused
/// rather than written to the registry's other tab.
pub(crate) fn service_add_document_thread(
    state: &AppState,
    thread: DocumentThread,
) -> Result<AppSnapshot, String> {
    let DocumentThread {
        file,
        kind,
        line_num,
        line_num_end,
        text,
        preview_key,
    } = thread;
    let kind = match kind.as_str() {
        "question" => er_service::AnnotationKind::Question,
        "note" => er_service::AnnotationKind::Note,
        other => return Err(format!("A document anchor cannot hold a {other}")),
    };
    let changed = || "The document changed before the comment was saved".to_string();
    let (active_tab, key, request) = {
        let app = state.app.lock().map_err(|e| e.to_string())?;
        (
            app.active_tab,
            app.tab().key(),
            app.tab()
                .capture_file_preview(&file)
                .map_err(|e| e.to_string())?,
        )
    };
    if request.preview_key != preview_key {
        return Err(changed());
    }
    let source = request.read(&*state.ports.git).map_err(|e| e.to_string())?;
    {
        let app = state.app.lock().map_err(|e| e.to_string())?;
        if app.active_tab != active_tab
            || app.tab().key() != key
            || app.tab().file_preview_key(&file) != request.preview_key
        {
            return Err(changed());
        }
    }
    content_routes::annotate(state, Some(key), |context| {
        er_service::Command::AddDocumentThread {
            context,
            file,
            line: line_num,
            line_end: line_num_end,
            kind,
            body: text,
            source,
        }
    })?;
    Ok(AppSnapshot::default())
}

#[tauri::command]
pub async fn reply_to_thread(
    parent_id: String,
    text: String,
    view: Option<crate::snapshot::OptimisticView>,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        route_annotation(&state, view.as_ref(), |context| {
            er_service::Command::ReplyToThread {
                context,
                parent_id,
                body: text,
                client_id: None,
            }
        })
    })
    .await
}

/// Reply to a comment that is already on GitHub: post the reply now and
/// record it. `id` is the client id the composer holds, so a retry after an
/// unknown outcome reconciles instead of posting twice.
#[tauri::command]
pub async fn reply_to_github_thread(
    parent_id: String,
    text: String,
    id: String,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        route_annotation(&state, None, |context| {
            er_service::Command::ReplyToGithubThread {
                context,
                parent_id,
                body: text,
                client_id: Some(id),
            }
        })
    })
    .await
}

#[tauri::command]
pub async fn delete_thread(
    id: String,
    view: Option<crate::snapshot::OptimisticView>,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        route_annotation(&state, view.as_ref(), |context| {
            er_service::Command::DeleteThread { context, id }
        })
    })
    .await
}

/// Remove all question/github threads linked to a finding (`finding_ref`), keeping the finding.
pub(crate) fn finding_linked_thread_ids(
    ai: &er_engine::ai::AiState,
    finding_id: &str,
) -> Vec<String> {
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
pub async fn remove_finding_thread(
    finding_id: String,
    view: Option<crate::snapshot::OptimisticView>,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        if let Some(early) = abort_wrong_view(&app, &state, view.as_ref()) {
            return early;
        }
        let er_dir = app.tab().er_dir();

        let pending_ids = finding_linked_thread_ids(&app.tab().ai, &finding_id);
        er_engine::ai::delete_threads_linked_to_finding(
            &er_dir,
            &app.tab().github_comments_dir(),
            &finding_id,
        )
        .map_err(|e| format!("Failed to remove finding thread: {e}"))?;

        let root = er_engine::ai::find_finding_thread_root(&app.tab().ai, &finding_id);
        let pending = pending_ids.iter().chain(&root).map(String::as_str);
        state.pending_ai_replies.remove(pending);

        app.tab_mut().reload_ai_state();
        crate::profile_log::bump_desktop_revision(&state.desktop_revision, "remove_finding_thread");
        Ok(snap_from_confirmed(&app, &state))
    })
    .await
}

#[tauri::command]
pub async fn resolve_thread(
    id: String,
    view: Option<crate::snapshot::OptimisticView>,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        route_annotation(&state, view.as_ref(), |context| {
            er_service::Command::ResolveThread { context, id }
        })
    })
    .await
}

// ── GitHub sync ───────────────────────────────────────────────────────────────

// Sync commands run on the main thread; `refresh_diff` shells out to git and
// rebuilds the snapshot — heavy enough to freeze the window from ⌘K.
#[tauri::command]
pub async fn refresh_diff(state: State<'_, AppState>) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || content_routes::refresh(&state, false).map(|_| AppSnapshot::default()))
        .await
}

/// Fetch the `gh stack` (github/gh-stack) stack for the active tab's branch and
/// return the refreshed snapshot.
///
/// `gh stack view` talks to GitHub, so the subprocess runs with the `App` lock
/// released (the heavy-command rule) and the result is written back through
/// `App::apply_stack_result`, which caches it on the tab for `build_snapshot`.
/// Called lazily when the BranchCard stack control is first opened.
#[tauri::command]
pub async fn refresh_stack(state: State<'_, AppState>) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || refresh_stack_impl(&state)).await
}

fn refresh_stack_impl(state: &AppState) -> Result<AppSnapshot, String> {
    // Under the lock: drop any cached stack so the claim below re-runs the
    // lookup, request it, and take the claim (the repo root to query). A tab
    // whose viewed branch isn't checked out requests nothing — `gh stack view`
    // there would describe an unrelated branch.
    let request = {
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        app.tab_mut().stack = er_engine::app::StackState::default();
        app.request_stack_load();
        app.take_stack_load_request()
    };

    // Lock released: the lookup hits the network.
    let result = request.map(|(tab_index, source, lookup_seq)| {
        let info = er_engine::gh_stack::load_from(&source);
        (tab_index, source, lookup_seq, info)
    });

    let snapshot = {
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        if let Some((tab_index, source, lookup_seq, info)) = result {
            // The expected steady states (not in a stack, extension missing) are
            // not errors. Anything else is a real failure — the control surfaces
            // the reason so the user can retry, and this leaves a durable record
            // for diagnosing a broken `gh` setup.
            if let er_engine::gh_stack::StackInfo::Failed(reason) = &info {
                let branch = app
                    .tabs
                    .get(tab_index)
                    .map(|t| t.current_branch.as_str())
                    .unwrap_or("");
                log::error!(
                    "er-desktop: stack lookup failed ({}) branch={branch}: {reason}",
                    source.describe()
                );
            }
            app.apply_stack_result(tab_index, lookup_seq, info);
        }
        snap_from_command(&app, state)
    };

    // The cached stack lives in engine state that the content hash doesn't
    // cover, so invalidate polling explicitly.
    state.desktop_revision.fetch_add(1, Ordering::Relaxed);
    Ok(snapshot)
}

/// Latest known PR `head_oid` for `pr_number` from the PR-list cache. This is
/// the exact source the stale-pill compares against (see `build_snapshot`), so
/// callers can align a tab's `last_diff_head_oid` with it to clear the pill.
fn pr_cache_head_oid_for_pr(state: &DesktopState, pr_number: u64) -> Option<String> {
    state.pr_cache.lock().ok().and_then(|cache| {
        cache
            .values()
            .flat_map(|prs| prs.iter())
            .find(|p| p.number == pr_number)
            .map(|p| p.head_oid.clone())
            .filter(|s| !s.is_empty())
    })
}

/// The branch pill compares the ls-remote probe cache with the local
/// `origin/<base>` oid. A sync has just fetched `origin/<base>`, so its oid
/// becomes the probed value and the two agree.
fn align_branch_base_probe(state: &DesktopState, repo_root: &str, base_branch: &str) {
    let base_short = base_branch.trim_start_matches("origin/");
    if let Some(oid) = er_engine::github::rev_parse_oid(&*state.ports.git, repo_root, base_branch) {
        let key = format!("{repo_root}\u{0}{base_short}");
        if let Ok(mut cache) = state.branch_base_remote_oid.lock() {
            cache.insert(key, oid);
        }
    }
}

/// A Sync's snapshot plus how many Reviewed marks it cleared.
///
/// The snapshot is flattened so the reply still applies as a plain
/// `AppSnapshot`; `reviewed_cleared` rides alongside for the "Reviewed cleared
/// on N changed files" toast. The count cannot be recovered afterwards: the
/// marks are gone from the reviewed set by the time the snapshot is built.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SyncOutcome {
    #[serde(flatten)]
    pub snapshot: AppSnapshot,
    pub reviewed_cleared: usize,
}

#[tauri::command]
pub async fn force_refresh_diff(state: State<'_, AppState>) -> Result<SyncOutcome, String> {
    let state = state.inner().clone();
    // Flag-on, the active tab's actor refetches and the route lines the stale
    // pill's baselines up, as the App path below does.
    run_blocking(move || {
        let synced = content_routes::sync(&state)?;
        kick_meta_refresh(&state, synced.repo_root);
        refresh_status_after_sync(&state, synced.status_key);
        Ok(SyncOutcome {
            snapshot: AppSnapshot::default(),
            reviewed_cleared: synced.reviewed_cleared,
        })
    })
    .await
    // Heavy: shells out to git/gh fetches. Run off the main thread so the
    // window stays responsive for the whole fetch.
}

/// A Sync moves the diff on screen to a newer PR head, but the cached status
/// can be up to a poll older. The merge box compares the two and blocks a
/// merge when they differ, so a Sync with no status refresh would leave the
/// merge blocked, telling the user to sync again. Ignores the freshness skip
/// for that reason.
fn refresh_active_status_after_sync(state: &AppState) {
    let key = state
        .app
        .lock()
        .ok()
        .and_then(|app| active_github_key(&app, state));
    refresh_status_after_sync(state, key);
}

pub(crate) fn refresh_status_after_sync(state: &DesktopState, key: Option<(String, String, u64)>) {
    if let Some((owner, repo, number)) = key {
        kick_github_status_refresh(state, owner, repo, number);
    }
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
        kick_github_status_refresh(&state, owner, repo, number);
    }
    snap!(state)
}

/// Pull GitHub review comments for the active tab.
///
/// Runs off the main thread and does not hold the App mutex during `gh`
/// (same three-phase pattern as the background 45s comment sync). Pass
/// `force: true` to bypass the 60s hover-prefetch cache (the Comments
/// panel re-fetch button); auto-pull omits it so a warm cache stays instant.
#[tauri::command]
pub async fn pull_github_comments(
    force: Option<bool>,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    let force = force.unwrap_or(false);
    run_blocking(move || service_pull_github_comments(&state, force)).await
}

#[cfg(test)]
fn gh_review_submit_err(e: anyhow::Error) -> String {
    let raw = e.to_string();
    if is_own_pr_approval_error(&raw) {
        return own_pr_approval_error();
    }
    // Typed, not textual: a PR number or SHA containing "422" used to earn
    // these hints for a failure GitHub never called unprocessable.
    let unprocessable = matches!(
        e.downcast_ref::<er_engine::github::GhError>(),
        Some(er_engine::github::GhError::Unprocessable { .. })
    );
    with_review_hints(raw, unprocessable)
}

/// A failed review write, with the likely causes of an unprocessable one.
fn with_review_hints(raw: String, unprocessable: bool) -> String {
    if !unprocessable {
        return format!("GitHub review submission failed: {raw}");
    }
    let hints = [
        "blank review payload",
        "invalid or stale line anchor",
        "comment position no longer matches PR head",
    ];
    format!(
        "GitHub review submission failed: {raw}\nLikely causes: {}",
        hints.join("; ")
    )
}

/// Submit a bare PR review decision (APPROVE / REQUEST_CHANGES / COMMENT) from
/// the GitHub card. It sends only the body + event; inline comments post as
/// they are created (ADR 0053), so there is no draft to bundle here.
///
/// The flag path is async: its reply waits on `gh`, which must not block the
/// main thread.
#[tauri::command]
pub async fn submit_github_pr_decision(
    mode: String,
    summary: String,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || service_submit_github_pr_decision(&state, &mode, &summary)).await
}

/// Post a PR-wide (issue-stream) comment on the active tab's PR. Used by the
/// GitHub card's "Comment / Review" action — distinct from line-anchored
/// comments, which post where they are created (ADR 0053).
#[tauri::command]
pub async fn post_github_pr_comment(
    body: String,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || service_post_github_pr_comment(&state, &body)).await
}

// ── Pending review ───────────────────────────────────────────────────────────
//
// The composer's "Start a review" collects line comments in the reviewer's
// GitHub pending review; the Review popover submits them with one decision.
// GitHub holds the draft, so every command here is a `gh` round trip and runs
// off the lock, which is held only to read which PR the tab is on (ADR 0015).

/// The pending review as the Review popover reads it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PendingReviewView {
    pub pr_number: u64,
    #[serde(flatten)]
    pub review: er_engine::github::PendingReview,
}

/// A line the composer selected, in the numbering of its side.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewLineTarget {
    pub file: String,
    pub line_num: usize,
    pub line_num_end: Option<usize>,
    /// "LEFT" | "RIGHT"
    pub side: Option<String>,
}

impl ReviewLineTarget {
    fn into_service(self) -> er_service::ReviewLineTarget {
        er_service::ReviewLineTarget {
            file: self.file,
            line_num: self.line_num,
            line_num_end: self.line_num_end,
            side: self.side,
        }
    }
}

pub(crate) fn review_comment_body(text: &str) -> Result<String, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("Comment body cannot be empty".to_string());
    }
    Ok(trimmed.to_string())
}

/// A posted or submitted comment changes what `pull_github_comments` should
/// return, so drop its cache and let the status card recount.
pub(crate) fn after_github_write(state: &DesktopState, owner: String, repo: String, number: u64) {
    er_engine::github::invalidate_pr_comments_cache();
    kick_github_status_refresh(state, owner, repo, number);
}

#[tauri::command]
pub async fn get_pending_review(state: State<'_, AppState>) -> Result<PendingReviewView, String> {
    let state = state.inner().clone();
    run_blocking(move || service_get_pending_review(&state)).await
}

#[tauri::command]
pub async fn add_pending_review_comment(
    target: ReviewLineTarget,
    text: String,
    state: State<'_, AppState>,
) -> Result<PendingReviewView, String> {
    let state = state.inner().clone();
    run_blocking(move || service_add_pending_review_comment(&state, target, &text)).await
}

#[tauri::command]
pub async fn remove_pending_review_comment(
    comment_id: String,
    state: State<'_, AppState>,
) -> Result<PendingReviewView, String> {
    let state = state.inner().clone();
    run_blocking(move || service_remove_pending_review_comment(&state, comment_id)).await
}

/// Submit the pending review with a decision. Without pending comments this
/// is the bare decision the Branch card's Approve used to send.
#[tauri::command]
pub async fn submit_pending_review(
    mode: String,
    summary: String,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || service_submit_pending_review(&state, &mode, summary)).await
}

/// "Add single comment": one line comment posted to the PR now, outside any
/// review.
#[tauri::command]
pub async fn post_github_line_comment(
    target: ReviewLineTarget,
    text: String,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || service_post_github_line_comment(&state, target, &text)).await
}

// ── AI integration ───────────────────────────────────────────────────────────

/// The git scope a review of `scope` reads on a tab showing `mode`.
pub(crate) fn resolve_review_scope_in(scope: &str, mode: DiffMode) -> Result<String, String> {
    let resolved = if scope == "current" {
        match mode {
            DiffMode::Branch | DiffMode::Unstaged | DiffMode::Staged => mode.git_mode().to_string(),
            // Guide (tour) reviews the branch diff it was built from.
            DiffMode::PrDiff | DiffMode::Tour => "branch".to_string(),
            _ => {
                return Err(format!(
                    "AI review not available in {} view — switch to All changes, PR Diff, Unstaged, or Staged",
                    mode.git_mode()
                ));
            }
        }
    } else if scope == "pr" {
        "branch".to_string()
    } else if matches!(scope, "branch" | "unstaged" | "staged") {
        if matches!(scope, "unstaged" | "staged") && mode == DiffMode::PrDiff {
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

// ⌘K / AI Hub: may shell out for the review diff and writes artifacts, then
// rebuilds a snapshot — must not run on the main thread. Keep admission,
// prepared-artifact writes, and enqueue under the App lock so an Advanced
// producer cannot race a flow lease between its check and its write.
#[tauri::command]
pub async fn run_ai_review(
    scope: String,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        ask_on_desktop(&state, |desktop| crate::runs::review(desktop, None, scope))
    })
    .await
}

/// Generate a guided Tour with AI: captures the active view's diff and spawns the
///
/// er-tour agent, which writes `tour.json` into that view's bucket. The PR Diff view
/// tours the PR head-vs-base diff (PR bucket); the Local branch / working-tree views
/// tour the branch diff (branch bucket). The mtime poll reloads it automatically on
/// completion, surfacing the Guide tab.
#[tauri::command]
pub async fn generate_tour(state: State<'_, AppState>) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || ask_on_desktop(&state, |desktop| crate::runs::tour(desktop, None))).await
}

/// Run the agent that proposes this repo's importance rules.
///
/// This only enqueues. The worker validates the rule table the agent prints and
/// merges it into the global config, replacing one repo's table and leaving
/// every other section alone; the task poll then lifts the new rules onto the
/// open tabs, so the file filter and the settings view both see them without a
/// restart (`docs/adr/0037-config-reaches-a-tab-by-sync.md`).
#[tauri::command]
pub async fn run_importance_agent(state: State<'_, AppState>) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || ask_on_desktop(&state, |desktop| crate::runs::importance(desktop, None)))
        .await
}

/// Generate a mermaid diagram of the active view's diff (`kind` =
///
/// `mental-model` | `subsystems` | `flows` | `custom`). The agent writes one
/// sidecar, `{er_dir}/diagrams/<kind>.json` (presets overwrite on re-run;
/// customs get a timestamped id and accumulate). Diagrams are per-view-bucket,
/// like triage: the active bucket is both write target and (via
/// `load_ai_state`) read source. The mtime poll picks up the result.
#[tauri::command]
pub async fn generate_diagram(
    kind: String,
    custom_prompt: Option<String>,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        ask_on_desktop(&state, |desktop| {
            crate::runs::diagram(desktop, None, kind, custom_prompt)
        })
    })
    .await
}

/// Delete one diagram sidecar (`{er_dir}/diagrams/<id>.json`) from the active
/// view bucket.
#[tauri::command]
pub async fn delete_diagram(id: String, state: State<'_, AppState>) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        if !er_engine::ai::is_safe_diagram_id(&id) {
            return Err(format!("Invalid diagram id: {id}"));
        }
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        let path = std::path::Path::new(&app.tab().er_dir())
            .join("diagrams")
            .join(format!("{id}.json"));
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| format!("Failed to delete diagram: {e}"))?;
        }
        app.tab_mut().reload_ai_state();
        state
            .desktop_revision
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(snap_from(&app, &state))
    })
    .await
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
pub async fn run_ai_expert_review(
    scope: String,
    expert_id: String,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        ask_on_desktop(&state, |desktop| {
            crate::runs::expert(desktop, None, scope, expert_id)
        })
    })
    .await
}

#[tauri::command]
pub async fn run_ai_professor_review(
    scope: String,
    focus_prompt: Option<String>,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    run_ai_scoped_review(
        scope,
        vec![],
        vec!["professor".to_string()],
        focus_prompt,
        state,
    )
    .await
}

#[tauri::command]
pub async fn run_ai_triage_review(
    scope: String,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    run_ai_scoped_review(scope, vec![], vec!["triage".to_string()], None, state).await
}

#[tauri::command]
pub async fn run_ai_review_files(
    scope: String,
    paths: Vec<String>,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    run_ai_scoped_review(scope, paths, vec!["general".to_string()], None, state).await
}

#[tauri::command]
pub async fn run_ai_scoped_review(
    scope: String,
    paths: Vec<String>,
    reviewer_kinds: Vec<String>,
    focus_prompt: Option<String>,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        ask_on_desktop(&state, |desktop| {
            crate::runs::scoped(desktop, None, scope, paths, reviewer_kinds, focus_prompt)
        })
    })
    .await
}

/// The diff a scoped run reviews: `raw` cut to `paths` (sorted, without
/// duplicates) with the file manifest and the overlay directory written into
/// the bucket, or the whole diff without the project's ignored files when
/// `paths` is empty. Answers whether it is scoped, the paths and the diff.
pub(crate) fn scoped_diff(
    er_dir: &str,
    raw: String,
    paths: Vec<String>,
    repo_root: &str,
    remote_repo: Option<&str>,
) -> Result<(bool, Vec<String>, String), String> {
    let scoped_files = !paths.is_empty();
    let mut scope_paths = paths;
    scope_paths.sort();
    scope_paths.dedup();
    let er_path = std::path::Path::new(er_dir);
    let diff_body = if scoped_files {
        let filtered = er_engine::git::filter_raw_diff_by_paths(&raw, &scope_paths);
        if filtered.trim().is_empty() {
            return Err("No diff for selected files".to_string());
        }
        let manifest = scope_paths.join("\n");
        std::fs::write(er_path.join("review-files.txt"), format!("{manifest}\n"))
            .map_err(|e| format!("Failed to write review-files.txt: {e}"))?;
        // The agent writes only its lens's overlay; the host stamps the
        // scope and the base identity on exit.
        std::fs::create_dir_all(er_path.join("scoped/experts"))
            .map_err(|e| format!("Failed to create scoped output directory: {e}"))?;
        filtered
    } else {
        let ignore = er_engine::projects_pins::review_ignore_globs_for_repo(repo_root, remote_repo);
        if ignore.is_empty() {
            raw
        } else {
            er_engine::git::filter_raw_diff_exclude_globs(&raw, &ignore)
        }
    };
    Ok((scoped_files, scope_paths, diff_body))
}

/// What a scoped run tells the person: who started, over how many files,
/// and who was already running.
pub(crate) fn scoped_message(
    started: &[String],
    skipped: &[String],
    file_count: Option<usize>,
) -> String {
    let file_note = match file_count {
        Some(count) => format!(" for {count} file{}", if count == 1 { "" } else { "s" }),
        None => String::new(),
    };
    if skipped.is_empty() {
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
    }
}

/// Per-lens stamps for a scoped run: the sorted selected paths, the base
/// file's identity captured before the agent starts, and the overlay bytes.
pub(crate) fn scope_stamps(
    er_dir: &str,
    scope_paths: &[String],
    reviewer_kinds: &[String],
) -> std::collections::HashMap<String, er_engine::ai::ScopedRunStamp> {
    use er_engine::ai::ReviewerKind;

    let er_path = std::path::Path::new(er_dir);
    let store = er_engine::store_registry::store();
    let Ok(scope) = er_store::Scope::dir(er_dir) else {
        return std::collections::HashMap::new();
    };
    let mut stamps = std::collections::HashMap::new();
    for kind in reviewer_kinds {
        let Some(parsed) = er_engine::ai::parse_reviewer_kind(kind) else {
            continue;
        };
        let (command_name, base) = match &parsed {
            ReviewerKind::Triage => continue,
            ReviewerKind::General => (
                "review".to_string(),
                base_identity(&store, &scope, (), |review: &er_engine::ai::ErReview| {
                    (&review.diff_hash, &review.created_at)
                }),
            ),
            ReviewerKind::Professor => (
                "professor".to_string(),
                base_identity(
                    &store,
                    &scope,
                    (),
                    |review: &er_engine::ai::ProfessorReview| {
                        (&review.diff_hash, &review.created_at)
                    },
                ),
            ),
            ReviewerKind::Expert(id) => {
                let Ok(slot) = er_store::ExpertId::new(id) else {
                    continue;
                };
                (
                    format!("expert-{id}"),
                    base_identity(
                        &store,
                        &scope,
                        slot,
                        |review: &er_engine::ai::ExpertReview| {
                            (&review.diff_hash, &review.created_at)
                        },
                    ),
                )
            }
        };
        let Some(overlay_path) = er_engine::ai::scoped_overlay_path(&command_name) else {
            continue;
        };
        stamps.insert(
            kind.clone(),
            er_engine::ai::ScopedRunStamp {
                scope_paths: scope_paths.to_vec(),
                base_diff_hash: base.0,
                base_created_at: base.1,
                previous_overlay: std::fs::read(er_path.join(&overlay_path)).ok(),
            },
        );
    }
    stamps
}

/// The `(diff_hash, created_at)` of a lens's full file, empty when absent.
/// Read through the store so an old enveloped file resolves the same way the
/// loader will read it.
fn base_identity<T: er_store::Artifact>(
    store: &er_store::ArtifactStore,
    scope: &er_store::Scope,
    slot: T::Slot,
    pick: impl Fn(&T) -> (&str, &str),
) -> (String, String) {
    let Ok(key) = er_store::ArtifactKey::<T>::new(scope.clone(), slot) else {
        return (String::new(), String::new());
    };
    let Ok(er_store::Loaded::Found(stored)) = store.read(&key) else {
        return (String::new(), String::new());
    };
    let (diff_hash, created_at) = pick(&stored.value);
    (diff_hash.to_string(), created_at.to_string())
}

/// Starts one scoped reviewer through `spawn`, which takes its kind, its
/// target, its prompt and its stamp.
pub(crate) type SpawnReviewer<'a> = dyn FnMut(
        &er_engine::ai::ReviewerKind,
        er_engine::task_target::BackgroundTaskTarget,
        String,
        Option<er_engine::ai::ScopedRunStamp>,
    ) -> Result<(), String>
    + 'a;

pub(crate) fn spawn_scoped_reviewers(
    spawn: &mut SpawnReviewer<'_>,
    target: er_engine::task_target::BackgroundTaskTarget,
    reviewer_kinds: &[String],
    focus_prompt: Option<&str>,
    diff_hash: &str,
    stamps: &std::collections::HashMap<String, er_engine::ai::ScopedRunStamp>,
) -> Result<(Vec<String>, Vec<String>), String> {
    use er_engine::ai::{prompts, ReviewerKind};

    let scope = target.scope.as_str();
    let er_dir = target.er_dir.as_str();

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
        let stamp = stamps.get(kind).cloned();

        let spawn_result = match &parsed {
            ReviewerKind::Triage => {
                let prompt =
                    prompts::build_triage_review_prompt_prepared_diff(scope, er_dir, diff_hash);
                spawn(&parsed, target.clone(), prompt, None)
            }
            ReviewerKind::General => {
                let overlay = er_engine::ai::scoped_overlay_path("review");
                let prompt = prompts::append_file_scope_if_present(
                    prompts::build_review_prompt_prepared_diff(
                        scope,
                        er_dir,
                        &target.base_branch,
                        &target.branch_label,
                        diff_hash,
                        overlay.as_deref(),
                    ),
                    er_dir,
                    overlay.as_deref(),
                );
                spawn(&parsed, target.clone(), prompt, stamp)
            }
            ReviewerKind::Expert(id) => {
                let overlay = er_engine::ai::scoped_overlay_path(&format!("expert-{id}"));
                let prompt = prompts::append_file_scope_if_present(
                    prompts::build_expert_review_prompt_prepared_diff(
                        scope,
                        er_dir,
                        id,
                        diff_hash,
                        overlay.as_deref(),
                    ),
                    er_dir,
                    overlay.as_deref(),
                );
                spawn(&parsed, target.clone(), prompt, stamp)
            }
            ReviewerKind::Professor => {
                let overlay = er_engine::ai::scoped_overlay_path("professor");
                let prompt = prompts::append_file_scope_if_present(
                    prompts::build_professor_review_prompt_prepared_diff(
                        scope,
                        er_dir,
                        focus_prompt,
                        overlay.as_deref(),
                        diff_hash,
                    ),
                    er_dir,
                    overlay.as_deref(),
                );
                spawn(&parsed, target.clone(), prompt, stamp)
            }
        };

        match spawn_result {
            Ok(()) => started.push(label),
            Err(msg) => {
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

/// The GitHub comments a validation checks: from the loaded sidecar, else
/// read from `comments_dir`.
pub(crate) fn github_comment_count(
    loaded: Option<&er_engine::ai::ErGithubComments>,
    comments_dir: &str,
) -> usize {
    if let Some(gc) = loaded {
        return er_engine::ai::count_eligible_github_comments(gc);
    }
    let store = er_engine::store_registry::store();
    if let Ok(scope) = er_store::Scope::dir(comments_dir) {
        if let Ok(er_store::Loaded::Found(stored)) =
            er_engine::ai::annotation_store::load_github_comments(&store, &scope)
        {
            return er_engine::ai::count_eligible_github_comments(&stored.value);
        }
    }
    0
}

#[tauri::command]
pub async fn run_ai_validate(
    scope: String,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        ask_on_desktop(&state, |desktop| {
            crate::runs::validate(desktop, None, scope)
        })
    })
    .await
}

#[tauri::command]
pub async fn set_ai_model(
    model: String,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || ai_model(&state, model)).await
}

/// With `v1-service` the AI picker's saves go to the settings actor, the one
/// writer of the config files (ADR 0069); the flag-off bodies save the App's.
pub(crate) fn ai_model(state: &AppState, model: String) -> Result<AppSnapshot, String> {
    {
        crate::ai_selection_service::ai_model(&state.desktop, model)?;
        let app = state.app.lock().map_err(|e| e.to_string())?;
        Ok(snap_from(&app, state))
    }
}

// ── AI provider / model selection ───────────────────────────────────────────

pub use er_engine::config::{map_ai_providers, AiModelInfo, AiProviderInfo};

#[tauri::command]
pub async fn list_ai_providers(state: State<'_, AppState>) -> Result<Vec<AiProviderInfo>, String> {
    let state = state.inner().clone();
    run_blocking(move || ai_providers(&state)).await
}

/// The picker's providers with the current pick marked. With `v1-service`
/// every pick saves, so the pick is the settings actor's default selection,
/// which its view already marks; the App lock is never taken.
pub(crate) fn ai_providers(state: &AppState) -> Result<Vec<AiProviderInfo>, String> {
    Ok(state.service.settings().view().hub.providers.clone())
}

#[tauri::command]
pub async fn set_ai_selection(
    provider_id: String,
    model_id: Option<String>,
    persist: Option<bool>,
    effort: Option<String>,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || ai_selection(&state, provider_id, model_id, persist, effort)).await
}

pub(crate) fn ai_selection(
    state: &AppState,
    provider_id: String,
    model_id: Option<String>,
    persist: Option<bool>,
    effort: Option<String>,
) -> Result<AppSnapshot, String> {
    // Palette and title-bar picks are the user's default. Settings also
    // passes true. Session-only remains available via persist: false.
    let persist = persist.unwrap_or(true);
    if persist {
        crate::ai_selection_service::ai_selection(&state.desktop, provider_id, model_id, effort)?;
        let app = state.app.lock().map_err(|e| e.to_string())?;
        return Ok(snap_from(&app, state));
    }
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let agent = app.config.agent.clone();

    let selection = if persist {
        let selection = app
            .config
            .ai_hub
            .set_default_selection_with_effort(
                &provider_id,
                model_id.as_deref(),
                effort.as_deref(),
                &agent,
            )
            .map_err(|e| e.to_string())?;
        er_engine::config::save_config(&app.config).map_err(|e| e.to_string())?;
        selection
    } else {
        // Session-only: keep current effort when the new model still supports
        // it, unless the picker supplied an explicit level.
        let runtime_effort = effort.clone().or_else(|| app.current_ai_effort.clone());
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
    Ok(snap_from(&app, state))
}

#[tauri::command]
pub async fn set_ai_effort(
    effort: Option<String>,
    persist: Option<bool>,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || ai_effort(&state, effort, persist)).await
}

pub(crate) fn ai_effort(
    state: &AppState,
    effort: Option<String>,
    persist: Option<bool>,
) -> Result<AppSnapshot, String> {
    let persist = persist.unwrap_or(false);
    if persist {
        crate::ai_selection_service::ai_effort(&state.desktop, effort)?;
        let app = state.app.lock().map_err(|e| e.to_string())?;
        return Ok(snap_from(&app, state));
    }
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
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
    Ok(snap_from(&app, state))
}

// ── Promote question to GitHub comment ──────────────────────────────────────

#[tauri::command]
pub async fn promote_to_comment(
    id: String,
    body: Option<String>,
    view: Option<crate::snapshot::OptimisticView>,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        route_annotation(&state, view.as_ref(), |context| {
            er_service::Command::PromoteToComment { context, id, body }
        })
    })
    .await
}

/// Promote a question to a local note. Notes are still private but framed as
///
/// actionable hand-offs to a coding agent. Mirrors `promote_to_comment` but the
/// target is `notes.json`. The source question (and its replies) is removed.
#[tauri::command]
pub async fn promote_to_note(
    id: String,
    body: Option<String>,
    view: Option<crate::snapshot::OptimisticView>,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        route_annotation(&state, view.as_ref(), |context| {
            er_service::Command::PromoteToNote { context, id, body }
        })
    })
    .await
}

// ── Finding promotions sidecar ───────────────────────────────────────────────
// Map of `finding_id -> comment_id`. Stored alongside `.er/review.json` so
// `er` does not have to write into AI-owned files.

pub fn load_finding_promotions(er_dir: &str) -> std::collections::HashMap<String, String> {
    er_engine::ai::load_finding_promotions(er_dir)
}

fn save_finding_promotions(
    er_dir: &str,
    map: &std::collections::HashMap<String, String>,
) -> std::io::Result<()> {
    er_engine::ai::save_finding_promotions(er_dir, map)
}

// ── Ask AI ──────────────────────────────────────────────────────────────────

/// Validate a comment, question, or finding with AI — adds a local reply on the card
/// (finding responses in review.json; thread replies in sidecars).
#[tauri::command]
pub async fn validate_with_ai(
    thread_id: Option<String>,
    finding_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        ask_on_desktop(&state, |desktop| {
            crate::ai_ask::validate(desktop, None, thread_id, finding_id)
        })
    })
    .await
}

/// Elaborate on a question or note: the AI rewrites it into a self-contained
/// question or instruction and adds that as a local reply. It does not answer;
/// a one-click `ask_ai` does.
#[tauri::command]
pub async fn elaborate_with_ai(
    thread_id: String,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        ask_on_desktop(&state, |desktop| {
            crate::ai_ask::elaborate(desktop, None, thread_id)
        })
    })
    .await
}

/// Tracked agent-command name for Send to agent. Single-flight per tab: two
/// agents editing one working tree would race over the same files.
pub(crate) const SEND_NOTE_COMMAND: &str = "send-to-agent";

/// What the Send-to-agent prompt says about the note and where it sits.
struct SendNotePromptParams<'a> {
    repo_root: &'a str,
    base_branch: &'a str,
    current_branch: &'a str,
    file: &'a str,
    line_start: Option<usize>,
    line_end: Option<usize>,
    line_content: &'a str,
    /// The note followed by its replies, oldest first. An Elaborate rewrite is
    /// one of those replies, so the agent gets the expanded instruction too.
    thread_body: &'a str,
}

/// "L8", "L8–9", or `None` for a file-level note.
fn note_range_label(line_start: Option<usize>, line_end: Option<usize>) -> Option<String> {
    let a = line_start?;
    Some(match line_end {
        Some(b) if b > a => format!("L{a}–{b}"),
        _ => format!("L{a}"),
    })
}

/// The prompt that hands a note to a coding agent, compiled in like every other
/// prompt (ADR 0023). The agent edits the working tree and nothing else: the
/// review's sidecars stay `er`'s to write, and committing stays the reviewer's
/// call, since the diff under review is the working tree being changed.
fn build_send_note_prompt(p: &SendNotePromptParams<'_>) -> String {
    let location = match note_range_label(p.line_start, p.line_end) {
        Some(range) => format!("{}, {range}", p.file),
        None => format!("{} (the file as a whole)", p.file),
    };
    let anchor = if p.line_content.trim().is_empty() {
        String::new()
    } else {
        format!(
            "The anchored line currently reads:\n```\n{}\n```\n\n",
            p.line_content
        )
    };
    format!(
        "You are a coding agent. A reviewer left the note below on a change under review and is handing it to you to carry out.

Repository: {repo_root}
Branch: {current_branch} (compared against {base_branch})
Location: {location}

{anchor}Note thread (the reviewer's note first, then any replies; a reply may restate the note as a fuller instruction):
---
{thread_body}
---

Do this:
1. Read {file} and what it depends on before changing anything.
2. Make the change the note asks for, in the working tree of {repo_root}. Keep to what the note asks; do not refactor around it.
3. If the note no longer applies to the current code, or is too ambiguous to act on without guessing, change nothing and say why.

Do not commit, push, stash, switch branches or reset. Do not write anything under the review tool's storage directory.

Finish with a short markdown summary: the files you changed and what changed in each, or why you changed nothing.",
        repo_root = p.repo_root,
        current_branch = p.current_branch,
        base_branch = p.base_branch,
        file = p.file,
        thread_body = p.thread_body.trim(),
    )
}

/// Why a note cannot go to the agent: a remote tab, or a branch checked out
/// elsewhere or nowhere, so there is no working tree here to edit.
pub(crate) fn send_note_blocked(
    is_remote: bool,
    checked_out: Option<&str>,
    repo_root: &str,
) -> Result<(), String> {
    if is_remote {
        return Err(
            "Send to agent edits the working tree, so it is off on a remote PR tab. Check the PR out locally to use it."
                .to_string(),
        );
    }
    match checked_out {
        Some(root) if root == repo_root => Ok(()),
        Some(root) => Err(format!(
            "This branch is checked out at {root}. Open that worktree to send notes to an agent."
        )),
        None => Err(
            "This branch is not checked out, so there is no working tree for an agent to edit."
                .to_string(),
        ),
    }
}

/// "Run standard instead" on a pending Deep confirmation: the triaged lenses
/// run as Standard finders, each queued for an agent slot (ADR 0021), with the
/// prompts the flow already builds in Rust (ADR 0023). Declining cancels, and
/// starting a new flow re-triages, which is why neither stands in for this.
#[tauri::command]
pub async fn run_standard_review_flow(
    er_dir: String,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        crate::arena_commands::flow_on_active(
            &state,
            er_dir,
            crate::flow_service::Control::RunStandard,
        )
    })
    .await
}

/// Hand a note to the configured agent CLI to carry out (ADR 0031). It runs as
/// a tracked agent command, so it shows in the running-jobs pill and the agent
/// log, can be stopped, and waits for an agent slot like a review agent
/// (ADR 0021). Its edits land in the working tree and reach the diff through
/// the watcher; nothing is written back to the note.
#[tauri::command]
pub async fn send_note_to_agent(
    note_id: String,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        ask_on_desktop(&state, |desktop| {
            crate::runs::send_note(desktop, None, note_id)
        })
    })
    .await
}

/// The prompt that sends note `note_id` and its replies from `notes` to the
/// agent, and the toast that names its context.
pub(crate) fn note_prompt(
    notes: Option<&er_engine::ai::ErNotes>,
    note_id: &str,
    repo_root: &str,
    base_branch: &str,
    current_branch: &str,
) -> Result<(String, String), String> {
    let notes: &[er_engine::ai::PrivateAnnotation] =
        notes.map(|ns| ns.notes.as_slice()).unwrap_or(&[]);
    let note = notes
        .iter()
        .find(|n| n.id == note_id)
        .ok_or_else(|| "Note not found".to_string())?;
    let mut thread_body = note.text.clone();
    for r in notes
        .iter()
        .filter(|r| r.in_reply_to.as_deref() == Some(note_id))
    {
        thread_body.push_str(&format!("\n\n**{}** replied:\n{}", r.author, r.text));
    }
    let prompt = build_send_note_prompt(&SendNotePromptParams {
        repo_root,
        base_branch,
        current_branch,
        file: &note.file,
        line_start: note.line_start,
        line_end: note.line_end,
        line_content: &note.line_content,
        thread_body: &thread_body,
    });
    let name = note.file.rsplit('/').next().unwrap_or(&note.file);
    let toast = match note_range_label(note.line_start, note.line_end) {
        Some(range) => format!("Note sent to the agent with {range} of {name} as context"),
        None => format!("Note sent to the agent with {name} as context"),
    };
    Ok((prompt, toast))
}

/// Invoke the configured AI agent (`claude` CLI by default) on a question or
///
/// comment thread. The subprocess runs on a background thread so this command
/// returns immediately — the reply is added asynchronously and picked up by
/// the next snapshot poll. While the subprocess is running a synthetic
/// "…thinking" reply is rendered (see `pending_ai_replies` in `AppState`).
#[tauri::command]
pub async fn ask_ai(
    thread_id: String,
    prompt: String,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        ask_on_desktop(&state, |desktop| {
            crate::ai_ask::ask(desktop, None, thread_id, prompt)
        })
    })
    .await
}

/// A card ask on the flag path, on the registry's active tab. It reads the
/// tab's content and the settings view, never the App, and the reply being
/// written shows in `jobs`; the page reads that, so no snapshot is built.
fn ask_on_desktop(
    state: &AppState,
    ask: impl FnOnce(&crate::state::DesktopState) -> Result<(), er_protocol::ErError>,
) -> Result<AppSnapshot, String> {
    ask(&state.desktop).map_err(|e| e.message)?;
    Ok(AppSnapshot::default())
}

/// The model a card ask runs: the one picked, else the agent's.
pub(crate) fn card_model(picked: Option<String>, agent_model: String) -> Option<String> {
    picked.or_else(|| (!agent_model.trim().is_empty()).then_some(agent_model))
}

/// Where a card ask's progress lines go: the card UI has no log panel, so
/// they go to the debug log, drained on their own thread while the job
/// blocks.
pub(crate) fn card_progress() -> er_engine::app::ProgressSink {
    Arc::new(|line: &str| {
        log::debug!("card ai: {line}");
    })
}

#[tauri::command]
pub async fn update_finding_response(
    finding_id: String,
    response_id: String,
    body: String,
    view: Option<crate::snapshot::OptimisticView>,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        if let Some(early) = abort_wrong_view(&app, &state, view.as_ref()) {
            return early;
        }
        let er_dir = app.tab().er_dir();
        er_engine::ai::update_finding_response(&er_dir, &finding_id, &response_id, &body)
            .map_err(|e| e.to_string())?;
        app.tab_mut().reload_ai_state();
        Ok(snap_from_confirmed(&app, &state))
    })
    .await
}

#[tauri::command]
pub async fn delete_finding_response(
    finding_id: String,
    response_id: String,
    view: Option<crate::snapshot::OptimisticView>,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        if let Some(early) = abort_wrong_view(&app, &state, view.as_ref()) {
            return early;
        }
        let er_dir = app.tab().er_dir();
        er_engine::ai::delete_finding_response(&er_dir, &finding_id, &response_id)
            .map_err(|e| e.to_string())?;
        app.tab_mut().reload_ai_state();
        Ok(snap_from_confirmed(&app, &state))
    })
    .await
}

// ── PR URL open ──────────────────────────────────────────────────────────────

/// Shared by the PR-open path (`open_pr_review_impl`), the desktop
/// active-branch watcher and the workspace's load, so all three attach a
/// checkout root by the same rule.
pub use er_engine::checkout::resolve_head_checkout;

/// Place `tab` into the app: replace the active slot when `replace` is true
/// (Cmd-click / middle-click semantics), otherwise push a new tab.
pub fn place_tab(
    app: &mut App,
    view: &crate::view_state::ViewStateHandle,
    tab: er_engine::app::TabState,
    replace: bool,
    skip_storage_sync: bool,
) {
    let mut tab = tab;
    if replace && !app.tabs.is_empty() {
        if !skip_storage_sync {
            tab.sync_managed_storage();
        }
        if let Some(msg) = tab.storage_notice.take() {
            app.notify(&msg);
        }
        let idx = app.active_tab.min(app.tabs.len() - 1);
        let name = tab.tab_name();
        // Replacing in place skips `push_tab`, where a tab gets its repo's rule
        // tables.
        app.install_repo_rules(&mut tab);
        app.tabs[idx] = tab;
        app.active_tab = idx;
        app.sync_config_from_active_tab();
        app.notify(&format!("Opened: {}", name));
    } else {
        app.open_tab(tab);
    }
    crate::tabs::persist_app_tabs(app, view);
    projects::sync_projects_from_tabs(&app.tabs);
}

/// Internal helper: open a remote PR view. If the same PR is already open,
/// just focus it. Otherwise place it via `replace` semantics.
/// Everything [`build_remote_pr_tab`] needs from the network, fetched once and
/// reused for both the tab and the cache entry.
struct RemotePrOpenInputs {
    base_branch: String,
    head_branch: String,
    raw_diff: String,
    pr_commits: Vec<er_engine::git::CommitInfo>,
    pr_data: Option<er_engine::github::PrOverviewData>,
    head_oid: Option<String>,
}

fn fetch_remote_pr_open_inputs(
    git: &dyn er_engine::git::Git,
    owner: &str,
    repo: &str,
    number: u64,
) -> Result<RemotePrOpenInputs, String> {
    let preview_head_before = er_engine::github::gh_pr_head_oid_remote(owner, repo, number).ok();
    // Fetch the payloads together, then confirm that their source did not move.
    let (metadata, diff, commits, overview) = std::thread::scope(|s| {
        let (owner_a, repo_a) = (owner.to_string(), repo.to_string());
        let metadata_h = s.spawn(move || {
            er_engine::github::gh_pr_metadata_remote(&owner_a, &repo_a, number)
                .map_err(|e| e.to_string())
        });
        let (owner_b, repo_b) = (owner.to_string(), repo.to_string());
        let diff_h = s.spawn(move || {
            er_engine::github::gh_pr_diff_remote(git, &owner_b, &repo_b, number)
                .map_err(|e| e.to_string())
        });
        let (owner_c, repo_c) = (owner.to_string(), repo.to_string());
        let commits_h = s
            .spawn(move || er_engine::github::gh_pr_commits_remote(&owner_c, &repo_c, number, 250));
        let overview_h =
            s.spawn(move || er_engine::github::gh_pr_overview_remote(owner, repo, number));
        (
            metadata_h
                .join()
                .unwrap_or_else(|_| Err("gh pr metadata thread panicked".to_string())),
            diff_h
                .join()
                .unwrap_or_else(|_| Err("gh pr diff thread panicked".to_string())),
            commits_h.join().unwrap_or_default(),
            overview_h.join().unwrap_or_default(),
        )
    });
    let (base_branch, head_branch) = metadata?;
    let raw_diff = diff?;
    let confirmed_head = er_engine::github::gh_pr_head_oid_remote(owner, repo, number).ok();
    let head_oid = if preview_head_before == confirmed_head {
        preview_head_before
    } else {
        None
    };
    Ok(RemotePrOpenInputs {
        base_branch,
        head_branch,
        raw_diff,
        pr_commits: commits,
        pr_data: overview,
        head_oid,
    })
}

/// Build a remote PR tab from a cached open entry — no network, no subprocess.
/// Pure (only disk reads for managed storage), so it is unit-testable. Seeds
/// the tab's branch preload from the entry (for remote tabs the branch-scope
/// diff IS `raw_diff`), so the first Branch-view switch needs no `gh` call.
fn remote_pr_tab_from_entry(
    pr_ref: &er_engine::github::PrRef,
    entry: crate::remote_pr_open_cache::RemotePrOpenEntry,
    ports: &er_engine::ports::Ports,
) -> Result<er_engine::app::TabState, String> {
    let mut tab = er_engine::app::TabState::new_remote_with_data(
        pr_ref,
        entry.base_branch,
        entry.head_branch,
        entry.raw_diff.clone(),
        entry.pr_commits,
        ports.clone(),
    )
    .map_err(|e| e.to_string())?;
    if let Some(data) = entry.pr_data {
        tab.pr_data = Some(data);
    }
    // Staleness baseline = the oid the cached diff was fetched at;
    // `build_remote_pr_tab` falls back to the PR-list cache when the entry has
    // no oid (pre-upgrade / failed fetch).
    tab.last_diff_head_oid = entry.head_oid.clone();
    tab.preview_head_oid = entry.head_oid.clone();
    tab.preloaded_branch_raw = Some(er_engine::app::PreloadedBranchRaw {
        raw: entry.raw_diff,
        preview_head_oid: entry.head_oid.clone(),
        base_branch: tab.base_branch.clone(),
        pr_number: Some(pr_ref.number),
        local_branch_view: tab.local_branch_view.clone(),
        checkout_root: None,
        remote_repo: tab.remote_repo.clone(),
        pr_head_ref: None,
        parity: true,
    });
    tab.reload_remote_comments();
    Ok(tab)
}

/// Build a remote PR tab outside the App lock: cache hit → zero `gh` calls;
/// miss → fetch metadata/diff/commits/overview, then warm the cache so the
/// next open (or a hover prefetch landing later) is instant.
fn build_remote_pr_tab(
    owner: &str,
    repo: &str,
    number: u64,
    state: &AppState,
) -> Result<er_engine::app::TabState, String> {
    let pr_ref = er_engine::github::PrRef {
        owner: owner.to_string(),
        repo: repo.to_string(),
        number,
    };
    // Same 3 s budget as the local open's `join_in_flight_pr_prefetch`.
    crate::remote_pr_open_cache::wait_for_remote_pr_open(
        &state.remote_pr_open_in_flight,
        owner,
        repo,
        number,
        std::time::Duration::from_secs(3),
    );
    if let Some(entry) = crate::remote_pr_open_cache::get_remote_pr_open_entry(
        &state.remote_pr_open_cache,
        owner,
        repo,
        number,
    ) {
        let mut tab = remote_pr_tab_from_entry(&pr_ref, entry.clone(), &state.ports)?;
        // Seed the staleness probe's baseline with the oid the cached diff was
        // fetched at: equal oid ⇒ pill stays off; advanced ⇒ it lights. Fall
        // back to the PR-list cache only when the entry has no oid (pre-upgrade
        // entries / failed oid fetch).
        tab.last_diff_head_oid = entry.head_oid.or_else(|| {
            let guard = state.pr_cache.lock().ok()?;
            let pr = guard
                .get(&format!("{owner}/{repo}"))?
                .iter()
                .find(|p| p.number == number)?;
            (!pr.head_oid.is_empty()).then(|| pr.head_oid.clone())
        });
        log::info!("remote_pr_open {owner}/{repo}#{number} cache=hit");
        return Ok(tab);
    }
    let inputs = fetch_remote_pr_open_inputs(&*state.ports.git, owner, repo, number)?;
    let mut tab = er_engine::app::TabState::new_remote_with_data(
        &pr_ref,
        inputs.base_branch.clone(),
        inputs.head_branch.clone(),
        inputs.raw_diff.clone(),
        inputs.pr_commits.clone(),
        state.ports.clone(),
    )
    .map_err(|e| e.to_string())?;
    if let Some(data) = inputs.pr_data.clone() {
        tab.pr_data = Some(data);
    }
    // Seed the branch preload from the diff we just fetched (branch scope ==
    // raw_diff for remote tabs) so `kick_branch_preload` skips its refetch.
    tab.preloaded_branch_raw = Some(er_engine::app::PreloadedBranchRaw {
        raw: inputs.raw_diff.clone(),
        preview_head_oid: inputs.head_oid.clone(),
        base_branch: tab.base_branch.clone(),
        pr_number: Some(number),
        local_branch_view: tab.local_branch_view.clone(),
        checkout_root: None,
        remote_repo: tab.remote_repo.clone(),
        pr_head_ref: None,
        parity: true,
    });
    tab.reload_remote_comments();
    // Seed the staleness baseline with the oid the freshly fetched diff was
    // computed at.
    tab.last_diff_head_oid = inputs.head_oid.clone();
    tab.preview_head_oid = inputs.head_oid.clone();
    crate::remote_pr_open_cache::insert_remote_pr_open_entry(
        &state.remote_pr_open_cache,
        owner,
        repo,
        number,
        crate::remote_pr_open_cache::RemotePrOpenEntry {
            base_branch: inputs.base_branch,
            head_branch: inputs.head_branch,
            raw_diff: inputs.raw_diff,
            pr_data: inputs.pr_data,
            pr_commits: inputs.pr_commits,
            head_oid: inputs.head_oid,
            last_touched: 0,
        },
    );
    log::info!("remote_pr_open {owner}/{repo}#{number} cache=miss");
    Ok(tab)
}

/// Open (or activate, if already open) a remote-only PR. The heavy path
/// (`gh` calls + diff parse) runs WITHOUT the App lock: holding the app mutex
/// across the network calls blocks every other command. On a cache hit there
/// is no network at all.
fn activate_or_open_remote_pr(
    owner: &str,
    repo: &str,
    number: u64,
    replace: bool,
    state: &AppState,
) -> Result<AppSnapshot, String> {
    let remote = format!("{owner}/{repo}");
    // Fast path: PR already open — activate without any network.
    {
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        if let Some(idx) = app
            .tabs
            .iter()
            .position(|t| t.remote_repo.as_deref() == Some(&remote) && t.pr_number == Some(number))
        {
            app.active_tab = idx;
            kick_meta_refresh(state, app.tab().repo_root.clone());
            kick_github_status_refresh(state, owner.to_string(), repo.to_string(), number);
            kick_branch_preload(&app, state);
            return Ok(snap_from_command(&app, state));
        }
    }
    // Build outside the lock (network on miss, cache on hit).
    let tab = build_remote_pr_tab(owner, repo, number, state)?;
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    // Re-check under the lock: a concurrent open of the same PR may have
    // landed while we were fetching — activate it instead of placing a
    // duplicate tab.
    if let Some(idx) = app
        .tabs
        .iter()
        .position(|t| t.remote_repo.as_deref() == Some(&remote) && t.pr_number == Some(number))
    {
        app.active_tab = idx;
        kick_meta_refresh(state, app.tab().repo_root.clone());
        kick_github_status_refresh(state, owner.to_string(), repo.to_string(), number);
        kick_branch_preload(&app, state);
        return Ok(snap_from_command(&app, state));
    }
    place_tab(&mut app, &state.view, tab, replace, false);
    app.tab_mut().needs_initial_refresh = false;
    kick_meta_refresh(state, app.tab().repo_root.clone());
    kick_github_status_refresh(state, owner.to_string(), repo.to_string(), number);
    kick_branch_preload(&app, state);
    Ok(snap_from_command(&app, state))
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

fn fetch_single_pr_for_remote(
    github: &dyn er_engine::github::GitHub,
    remote: &str,
    pr_number: u64,
) -> Result<PrInfo, String> {
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

    let out = github
        .pr_view_json(
            Some(remote),
            None,
            pr_number,
            "number,title,headRefName,baseRefName,headRefOid,updatedAt,state,isDraft,author,assignees,reviewRequests,reviewDecision,mergedAt,latestReviews",
        )
        .map_err(|e| format!("Failed to fetch PR #{pr_number} for {remote}: {e}"))?;
    let raw: RawPr = serde_json::from_slice(&out)
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

pub(crate) fn cache_single_pr_for_remote(
    state: &DesktopState,
    remote: &str,
    pr_number: u64,
) -> Result<PrInfo, String> {
    // Cache hit → reuse without a gh round-trip. The sidebar PR list comes
    // from this same pr_cache, so a hover-prefetched remote open pays zero
    // network on the click path (freshness parity is exact at open time).
    if let Ok(cache) = state.pr_cache.lock() {
        if let Some(pr) = cache
            .get(remote)
            .and_then(|prs| prs.iter().find(|p| p.number == pr_number))
        {
            return Ok(pr.clone());
        }
    }
    let fetched_pr = fetch_single_pr_for_remote(&*state.ports.github, remote, pr_number)?;
    state.app_owner.cache_pr(remote, fetched_pr.clone());
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

/// The local project for `remote`, which decides whether a PR URL opens as a
/// local review or a remote PR. It never picks an actor: the opened tab's own
/// descriptor does.
fn project_root_for_remote(remote: &str) -> String {
    let file = projects::load();
    find_project_id_for_remote(&file, remote)
        .and_then(|id| {
            file.projects
                .iter()
                .find(|project| project.id == id)
                .map(|project| project.root_path.clone())
        })
        .unwrap_or_default()
}

/// The desktop side of the `v1-service` open path. App-scoped records are
/// resolved here, where they live, so the actor's command carries the repo
/// root and the remote and the service never reads the store. The actor is
/// the one the opened tab keys, so the open and the tab's content meet there.
pub(crate) fn service_open(
    state: &DesktopState,
    target: er_service::OpenTarget,
    replace: Option<bool>,
) -> Result<(), String> {
    let tab = content_routes::pr_tab(&target)?;
    let reply = state
        .service
        .open_target(&tab.workspace_key(), target, replace.unwrap_or(false))
        .map_err(|error| error.to_string())?;
    // The open path answers with the opened target; the other replies belong
    // to the annotation commands.
    if let er_service::Reply::TargetOpened(_) = reply {}
    // The PR becomes a registry tab whose content the actor loads. The
    // flag-on desktop does not render from the service yet, so the command
    // acknowledges the open without moving the view.
    content_routes::open(state, tab, replace.unwrap_or(false))
}

/// Sends one annotation command through the service and acknowledges it.
///
/// The flag-on desktop does not render from the service yet (D21), so the
/// returned snapshot is empty; the open commands are tab changes and blank
/// until the next poll, while annotation commands keep the painted row
/// (ADR 0017).
///
/// The context and the key come from the app registry's active tab and its
/// published content. The view the user painted on is still checked on the
/// App, which renders it. Every command here carries something the page took
/// from the App, a painted view or a path or thread id on screen, so the App
/// tab's key goes to `annotate`, which refuses the command when the
/// registry's active tab is another one.
fn route_annotation(
    state: &AppState,
    view: Option<&crate::snapshot::OptimisticView>,
    build: impl FnOnce(er_service::TabContext) -> er_service::Command,
) -> Result<AppSnapshot, String> {
    let expected = {
        let app = state.app.lock().map_err(|e| e.to_string())?;
        // The flag-on desktop does not render the service snapshot (D21), so a
        // wrong-view snapshot would read as success and leave the paint in
        // place. Err is what makes the frontend roll the paint back.
        if abort_wrong_view(&app, state, view).is_some() {
            return Err(content_routes::MOVED.to_string());
        }
        app.tab().key()
    };
    content_routes::annotate(state, Some(expected), build)?;
    Ok(AppSnapshot::default())
}

/// Sends one GitHub command for the registry's active tab and waits for the
/// reply. The tab and its context come from the registry and the tab's
/// content, so a held App never holds the command; the `gh` work runs in the
/// actor's effect.
fn send_for_active_tab(
    state: &AppState,
    build: impl FnOnce(er_service::TabContext) -> er_service::Command,
) -> Result<er_service::Reply, er_service::WorkspaceError> {
    let (tab, content) =
        content_routes::active_tab_content(state).map_err(er_service::WorkspaceError::Failed)?;
    let workspace = crate::service::actor_key(&tab);
    state
        .service
        .send(&workspace, build(content.context.clone()))
}

fn unexpected_reply(reply: &er_service::Reply) -> String {
    format!("The workspace answered with an unexpected reply: {reply:?}")
}

/// The flag path's wording for a failed review write. GitHub's classification
/// picks the hints; no error text is read.
pub(crate) fn review_write_message(error: er_service::WorkspaceError) -> String {
    match error {
        er_service::WorkspaceError::Github(gh) => {
            let unprocessable = matches!(gh, er_engine::github::GhError::Unprocessable { .. });
            with_review_hints(gh.to_string(), unprocessable)
        }
        other => other.to_string(),
    }
}

/// Sends a pending-review command and returns the PR it acted on and the view
/// the popover renders.
fn service_pending_review(
    state: &AppState,
    build: impl FnOnce(er_service::TabContext) -> er_service::Command,
    message: fn(er_service::WorkspaceError) -> String,
) -> Result<(er_service::PrRef, PendingReviewView), String> {
    match send_for_active_tab(state, build).map_err(message)? {
        er_service::Reply::PendingReview { pr, review } => {
            let view = PendingReviewView {
                pr_number: pr.number,
                review,
            };
            Ok((pr, view))
        }
        other => Err(unexpected_reply(&other)),
    }
}

fn service_get_pending_review(state: &AppState) -> Result<PendingReviewView, String> {
    service_pending_review(
        state,
        |context| er_service::Command::GetPendingReview { context },
        |error| error.to_string(),
    )
    .map(|(_, view)| view)
}

fn service_add_pending_review_comment(
    state: &AppState,
    target: ReviewLineTarget,
    text: &str,
) -> Result<PendingReviewView, String> {
    let body = review_comment_body(text)?;
    let target = target.into_service();
    let (pr, view) = service_pending_review(
        state,
        |context| er_service::Command::AddPendingReviewComment {
            context,
            target,
            body,
        },
        review_write_message,
    )?;
    after_github_write(state, pr.owner, pr.repo, pr.number);
    Ok(view)
}

fn service_remove_pending_review_comment(
    state: &AppState,
    comment_node_id: String,
) -> Result<PendingReviewView, String> {
    service_pending_review(
        state,
        |context| er_service::Command::RemovePendingReviewComment {
            context,
            comment_node_id,
        },
        |error| error.to_string(),
    )
    .map(|(_, view)| view)
}

fn service_pull_github_comments(state: &AppState, force: bool) -> Result<AppSnapshot, String> {
    let source = er_service::SyncSource::Manual { force };
    let reply = send_for_active_tab(state, |context| er_service::Command::SyncGithubComments {
        context,
        source,
    })
    .map_err(|error| format!("GitHub sync error: {error}"))?;
    match reply {
        er_service::Reply::CommentsSynced { .. } => Ok(AppSnapshot::default()),
        other => Err(unexpected_reply(&other)),
    }
}

fn service_submit_pending_review(
    state: &AppState,
    mode: &str,
    summary: String,
) -> Result<AppSnapshot, String> {
    let event = er_service::ReviewEvent::parse(mode).ok_or_else(|| {
        format!("Invalid review mode: {mode}. Use COMMENT, APPROVE, or REQUEST_CHANGES.")
    })?;
    let reply = send_for_active_tab(state, |context| er_service::Command::SubmitPendingReview {
        context,
        event,
        summary,
    })
    .map_err(review_write_message)?;
    match reply {
        er_service::Reply::ReviewSubmitted { pr } => {
            after_github_write(state, pr.owner, pr.repo, pr.number);
            Ok(AppSnapshot::default())
        }
        other => Err(unexpected_reply(&other)),
    }
}

fn service_post_github_pr_comment(state: &AppState, body: &str) -> Result<AppSnapshot, String> {
    let body = body.trim().to_string();
    if body.is_empty() {
        return Err("Comment body cannot be empty".to_string());
    }
    let reply = send_for_active_tab(state, |context| er_service::Command::PostGithubPrComment {
        context,
        body,
    })
    .map_err(|error| format!("Failed to post comment: {error}"))?;
    match reply {
        er_service::Reply::CommentPosted { pr } => {
            // The comment count and the recent list come from the status card.
            kick_github_status_refresh(state, pr.owner, pr.repo, pr.number);
            Ok(AppSnapshot::default())
        }
        other => Err(unexpected_reply(&other)),
    }
}

fn service_post_github_line_comment(
    state: &AppState,
    target: ReviewLineTarget,
    text: &str,
) -> Result<AppSnapshot, String> {
    let body = review_comment_body(text)?;
    let target = target.into_service();
    let reply = send_for_active_tab(state, |context| {
        er_service::Command::PostGithubLineComment {
            context,
            target,
            body,
        }
    })
    .map_err(|error| error.to_string())?;
    match reply {
        er_service::Reply::CommentPosted { pr } => {
            after_github_write(state, pr.owner, pr.repo, pr.number);
            Ok(AppSnapshot::default())
        }
        other => Err(unexpected_reply(&other)),
    }
}

fn service_submit_github_pr_decision(
    state: &AppState,
    mode: &str,
    summary: &str,
) -> Result<AppSnapshot, String> {
    let event = er_service::ReviewEvent::parse(mode).ok_or_else(|| {
        format!("Invalid review mode: {mode}. Use COMMENT, APPROVE, or REQUEST_CHANGES.")
    })?;
    let summary = summary.trim().to_string();
    // GitHub rejects a REQUEST_CHANGES or COMMENT review with a blank body.
    if event != er_service::ReviewEvent::Approve && summary.is_empty() {
        let event = event.as_str();
        return Err(format!(
            "GitHub requires a comment for {event} reviews. Add a summary first."
        ));
    }
    let reply = send_for_active_tab(state, |context| {
        er_service::Command::SubmitGithubPrDecision {
            context,
            event,
            summary,
        }
    })
    .map_err(review_write_message)?;
    match reply {
        er_service::Reply::ReviewSubmitted { pr } => {
            kick_github_status_refresh(state, pr.owner, pr.repo, pr.number);
            Ok(AppSnapshot::default())
        }
        other => Err(unexpected_reply(&other)),
    }
}

pub(crate) fn service_open_pr_url(
    state: &DesktopState,
    url: String,
    replace: Option<bool>,
) -> Result<(), String> {
    let pr_ref = er_engine::github::parse_github_pr_url(&url)
        .ok_or_else(|| format!("Not a valid GitHub PR URL: {url}"))?;
    let remote = format!("{}/{}", pr_ref.owner, pr_ref.repo);
    let repo_root = project_root_for_remote(&remote);
    service_open(
        state,
        er_service::OpenTarget::PrUrl { url, repo_root },
        replace,
    )
}

pub(crate) fn service_open_remote_pr(
    state: &DesktopState,
    owner: String,
    repo: String,
    number: u64,
    replace: Option<bool>,
) -> Result<(), String> {
    // A remote PR opens as a remote tab even when a project tracks its repo,
    // as the flag-off path does, so it carries no local root.
    service_open(
        state,
        er_service::OpenTarget::RemotePr {
            owner,
            repo,
            number,
            repo_root: String::new(),
        },
        replace,
    )
}

pub(crate) fn service_open_pr_review(
    state: &DesktopState,
    project_id: String,
    pr_number: u64,
    replace: Option<bool>,
) -> Result<(), String> {
    let file = projects::load();
    let project = file
        .projects
        .iter()
        .find(|project| project.id == project_id);
    let repo_root = project
        .map(|project| project.root_path.clone())
        .unwrap_or_default();
    let remote = project
        .and_then(|project| project.remote.clone())
        .unwrap_or_default();
    service_open(
        state,
        er_service::OpenTarget::PrReview {
            project_id,
            remote,
            pr_number,
            repo_root,
        },
        replace,
    )
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
    run_blocking(move || {
        service_open_remote_pr(&state, owner, repo, number, replace)
            .map(|()| AppSnapshot::default())
    })
    .await
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
    activate_or_open_remote_pr(&owner, &repo, number, replace.unwrap_or(false), state)
}

#[tauri::command]
pub async fn open_pr_url(
    url: String,
    replace: Option<bool>,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || service_open_pr_url(&state, url, replace).map(|()| AppSnapshot::default()))
        .await
}

// ── Worktree picker (stub — no dialog dep) ──────────────────────────────────

#[tauri::command]
pub fn open_worktree(state: State<AppState>) -> Result<AppSnapshot, String> {
    let picked = rfd::FileDialog::new()
        .set_title("Select a git repository")
        .pick_folder();
    let Some(path) = picked else {
        // User cancelled — return current snapshot, no-op.
        return Ok(AppSnapshot::default());
    };
    let path_str = path.to_string_lossy().to_string();
    // The picked folder is the only input the workspace needs.
    service_open_folder(&state, path_str).map(|()| AppSnapshot::default())
}

/// Opens a Working tab on a picked folder, registering it as a project.
pub(crate) fn service_open_folder(state: &DesktopState, path: String) -> Result<(), String> {
    let _ = projects::auto_register(&*state.ports.github, &*state.ports.git, &path);
    content_routes::open(state, content_routes::working_tab(path), false)
}

// ── Project commands ─────────────────────────────────────────────────────────

enum LocalBranchOpenPath {
    LocalFirst,
    LocalOnlyFallback,
}

fn build_local_branch_tab(
    project_id: &str,
    name: String,
    ports: &er_engine::ports::Ports,
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
    let base_branch = ports
        .git
        .detect_base_branch(&proj.root_path)
        .map_err(|e| e.to_string())?;
    log_branch_open_phase(project_id, &branch_name, "base_detect", t_base);

    let t_tab_init = std::time::Instant::now();
    let mut new_tab =
        build_local_branch_tab_for_repo(&proj.root_path, base_branch, name.clone(), ports)?;
    log_branch_open_phase(project_id, &branch_name, "tab_init", t_tab_init);
    // Light: re-roots managed storage and reloads the reviewed markers without
    // the AI reload. The refresh below reaches its own AI gate once
    // `branch_diff_hash` is set, so the full sync would read every sidecar
    // twice on the open path.
    new_tab.sync_managed_storage_light();
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

fn build_local_branch_tab_for_repo(
    repo_root: &str,
    base_branch: String,
    name: String,
    ports: &er_engine::ports::Ports,
) -> Result<er_engine::app::TabState, String> {
    let mut tab = er_engine::app::TabState::new_with_base_unloaded(
        repo_root.to_string(),
        base_branch,
        ports.clone(),
    )
    .map_err(|e| e.to_string())?;
    tab.local_branch_view = Some(name.clone());
    // Resolve the working tree before the first diff refresh. The watcher only
    // applies later changes, so it cannot repair a same-branch tab whose root
    // was left unset at construction.
    tab.local_branch_checkout_root = resolve_head_checkout(&*ports.git, repo_root, &name);
    tab.mode = er_engine::app::DiffMode::Branch;
    // The tab's own remote is the repo actually being viewed, not whichever
    // project's remote happens to be active in the app.
    tab.remote_repo = projects::resolve_repo_remote(&*ports.github, &*ports.git, repo_root);
    Ok(tab)
}

/// Background branch-refresh workers currently running, keyed
/// `(repo_root, branch)`.
///
/// The frontend dedups repeats of the *same* branch, but nothing there stops a
/// run of clicks across different branches stacking one worker per click, each
/// paying its own network fetch — and every worker whose tab is no longer
/// active still pays, then discards the result.
static BRANCH_REFRESH_IN_FLIGHT: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

fn branch_refresh_finished(key: &(String, String)) {
    if let Ok(mut g) = BRANCH_REFRESH_IN_FLIGHT.lock() {
        g.retain(|k| k != key);
    }
}

fn kick_background_branch_refresh(
    app_state: Arc<Mutex<App>>,
    desktop_revision: Arc<AtomicU64>,
    repo_root: String,
    branch_name: String,
    base_branch: String,
) {
    if er_engine::github::is_pinned_pr_base(&base_branch) {
        return; // a deleted base's commit: nothing on origin to fetch
    }
    let flight_key = (repo_root.clone(), branch_name.clone());
    {
        let Ok(mut g) = BRANCH_REFRESH_IN_FLIGHT.lock() else {
            return;
        };
        if g.contains(&flight_key) {
            return; // this branch is already being refreshed
        }
        g.push(flight_key.clone());
    }
    let ports = app_state
        .lock()
        .ok()
        .map(|a| a.ports.clone())
        .unwrap_or_else(er_engine::ports::Ports::cli);
    std::thread::spawn(move || {
        // Fetch the base branch from origin so the local diff is up-to-date.
        let base_strip = base_branch.strip_prefix("origin/").unwrap_or(&base_branch);
        // Where the local remote-tracking ref pointed before the fetch. If the
        // fetch does not move it, the diff cannot have changed and the
        // lock-held full refresh below is pure cost. That is the steady state:
        // most opens find origin's base tip exactly where it already was.
        let remote_ref = format!("origin/{base_strip}");
        let oid_before = er_engine::github::rev_parse_oid(&*ports.git, &repo_root, &remote_ref);
        let t_worker = std::time::Instant::now();
        match er_engine::github::fetch_base_branch_ref(&*ports.git, &repo_root, base_strip) {
            Ok(base_ref) => {
                let oid_after =
                    er_engine::github::rev_parse_oid(&*ports.git, &repo_root, &remote_ref);
                // A first materialisation (`before` was None) counts as moved.
                let moved = oid_before.is_none() || oid_before != oid_after;
                // This worker was the one leg of a branch open with no phase
                // log, so its duration was invisible in profile output.
                log::info!(
                    "branch_open branch={} phase=background_branch_refresh ms={} moved={}",
                    branch_name,
                    t_worker.elapsed().as_millis(),
                    moved
                );
                if !moved {
                    branch_refresh_finished(&flight_key);
                    return;
                }
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
        branch_refresh_finished(&flight_key);
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
    run_blocking(move || {
        let tab = content_routes::branch_tab(&project_id, name)?;
        content_routes::open(&state, tab, replace.unwrap_or(false))?;
        Ok(AppSnapshot::default())
    })
    .await
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
    let (new_tab, open_path) = build_local_branch_tab(&project_id, name, &state.ports)?;
    log_branch_open_phase(&project_id, &branch_name, "tab_build", t_tab_build);
    let t_app_lock = std::time::Instant::now();
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    log_branch_open_phase(&project_id, &branch_name, "app_lock", t_app_lock);
    let t_place_tab = std::time::Instant::now();
    place_tab(
        &mut app,
        &state.view,
        new_tab,
        replace.unwrap_or(false),
        false,
    );
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
    let snapshot = snap_from_command(&app, state);
    log_branch_open_phase(&project_id, &branch_name, "snapshot_build", t_snapshot);
    log_branch_open_phase(&project_id, &branch_name, "total", t_total);
    drop(app);
    kick_background_branch_refresh(
        Arc::clone(&state.app),
        Arc::clone(&state.desktop_revision),
        repo_root,
        branch_name,
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
/// **base branch** matches — head/`updated_at` drift is allowed: render the
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

fn remember_pr_open_entry(
    cache: &Arc<Mutex<HashMap<PrOpenCacheKey, PrOpenCacheEntry>>>,
    key: PrOpenCacheKey,
    freshness: PrOpenFreshness,
    raw_diff: String,
    pr_data: Option<er_engine::github::PrOverviewData>,
    pr_commits: Option<Vec<er_engine::git::CommitInfo>>,
) {
    if let Ok(mut guard) = cache.lock() {
        er_engine::pr_open_entry::remember_pr_open_entry(
            &mut guard, key, freshness, raw_diff, pr_data, pr_commits,
        );
    }
    // With `v1-service` the service's app registry is the file's one writer.
}

fn run_gh_pr_view_for_open(
    github: &dyn er_engine::github::GitHub,
    repo_root: &str,
    pr_number: u64,
) -> Result<PrOpenMetadata, String> {
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

    let out = github
        .pr_view_json(
            None,
            Some(repo_root),
            pr_number,
            "number,title,body,state,author,url,baseRefName,headRefName,headRefOid,updatedAt,reviews,commits",
        )
        .map_err(|e| format!("Failed to get PR #{pr_number}: {e}"))?;
    let raw: RawView = serde_json::from_slice(&out)
        .map_err(|e| format!("Failed to parse gh pr view for PR #{pr_number}: {e}"))?;
    let pr_commits = er_engine::github::parse_pr_commits_view_json(&out, PR_COMMIT_CACHE_LIMIT)
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

fn run_gh_pr_diff_for_open(
    git: &dyn er_engine::git::Git,
    repo_root: &str,
    pr_number: u64,
) -> Result<String, String> {
    er_engine::github::gh_pr_diff(git, pr_number, repo_root).map_err(|e| e.to_string())
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
pub(crate) fn freshness_from_pr_info(pr: &PrInfo, diff_head_oid: Option<&str>) -> PrOpenFreshness {
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

/// If a hover-prefetch is in flight for `(project_id, pr_number)` with a
/// freshness matching `freshness`, wait (bounded) for it to populate the
/// cache and return the fresh entry. Returns `None` when there is nothing to
/// join (no claim, freshness mismatch, or the wait timed out) — the caller
/// falls back to fetching the diff itself.
fn join_in_flight_pr_prefetch(
    state: &AppState,
    project_id: &str,
    pr_number: u64,
    key: &PrOpenCacheKey,
    freshness: &PrOpenFreshness,
) -> Option<CachedPrOpenEntry> {
    let claim = {
        let guard = state.pr_open_prefetch_in_flight.lock().ok()?;
        guard.get(&(project_id.to_string(), pr_number)).cloned()
    };
    let claim = match claim {
        Some(c) if c.freshness == *freshness => c,
        _ => return None,
    };
    // Bounded wait: the prefetch typically completes in ~1s; give it 3s.
    // The worker notifies on success *and* failure, so a failed prefetch
    // wakes us to fall through to our own fetch rather than sleeping out the
    // timeout.
    let mut done = claim.done.lock().ok()?;
    // Re-check `done` after acquiring the lock: the worker may have set it and
    // notified before we locked, in which case `wait_timeout` would otherwise
    // sleep out the full 3 s (lost wakeup). Bounded: each wait_timeout has its
    // own 3 s budget and the predicate is re-checked on every wakeup.
    let mut timed_out = false;
    while !*done {
        let (guard, timeout) = claim
            .cv
            .wait_timeout(done, std::time::Duration::from_secs(3))
            .ok()?;
        done = guard;
        if timeout.timed_out() {
            timed_out = true;
            break;
        }
    }
    drop(done);
    if timed_out {
        // Narrow race: the worker may have finished exactly as the budget
        // expired — one final check before falling back to our own fetch.
        if !*claim.done.lock().ok()? {
            return None;
        }
    }
    // The prefetch populated the cache with its own freshness — serve it when
    // the base branch still matches (head/`updated_at` drift is allowed, same
    // as the hint-miss path). A freshness mismatch mid-flight re-fetches.
    cached_pr_open_entry(&state.pr_open_cache, key, freshness)
}

/// Cheap pre-check: does the PR-open cache already hold a fresh-enough entry
/// for this PR (complete hint + matching base)? Lets `open_pr_review_impl`
/// branch hit (inputs are in memory — the existing fast flow) vs miss (place
/// a stub, fetch in the offload worker) BEFORE touching the network.
fn pr_open_cache_is_hit(
    project_id: &str,
    pr_number: u64,
    hint: Option<&PrOpenHint>,
    state: &AppState,
) -> bool {
    let Some(hint) = hint.filter(|h| pr_open_hint_is_complete(h)) else {
        return false;
    };
    let file = projects::load();
    let Some(proj) = file.projects.iter().find(|p| p.id == project_id) else {
        return false;
    };
    let key = pr_open_cache_key(project_id, &proj.root_path, pr_number);
    cached_pr_open_entry(&state.pr_open_cache, &key, &freshness_from_hint(hint)).is_some()
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
    let repo_slug = proj.remote;
    let branch_label = format!("pr-{}", pr_number);
    let key = pr_open_cache_key(project_id, &repo_root, pr_number);
    let hint = hint.filter(|h| pr_open_hint_is_complete(h));

    // ── Hint path: skip the `gh pr view` round-trip entirely ──
    if let Some(hint) = hint {
        let freshness = freshness_from_hint(hint);

        // Cache hit when the base branch matches — head/`updated_at` drift is
        // allowed: render the diff we already hold instantly, and the 30s stale
        // pill catches a moved head. Reuse the **entry's own** freshness, not the
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
                &*state.ports.git,
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
        // First try to join an in-flight hover-prefetch (a click landing
        // mid-prefetch should consume its result, not run a duplicate
        // `gh pr diff`). Bounded wait — on timeout or a freshness change we
        // fall through to the parallel fetch below.
        if let Some((raw_diff, entry_freshness, cached_pr_data, cached_pr_commits)) =
            join_in_flight_pr_prefetch(state, project_id, pr_number, &key, &freshness)
        {
            log::info!(
                "branch_open project={} branch={} phase=gh_pr_diff ms=0 cache=joined_prefetch",
                project_id,
                branch_label
            );
            let t_base = std::time::Instant::now();
            let resolved_base = er_engine::github::ensure_base_ref_available(
                &*state.ports.git,
                &repo_root,
                &entry_freshness.base_branch,
            )
            .map_err(|e| e.to_string())?;
            log_branch_open_phase(project_id, &branch_label, "base_ref_check", t_base);
            let pr_data = cached_pr_data
                .unwrap_or_else(|| pr_overview_from_hint(hint, pr_number, repo_slug.as_deref()));
            let pr_commits = cached_pr_commits
                .unwrap_or_else(|| run_gh_pr_commits_for_open(&repo_root, pr_number));
            // Re-write with the entry's own freshness so the diff↔oid pairing
            // is preserved; remember_pr_open_entry keeps existing pr_data.
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
        let (diff_res, base_res, commits, diff_ms, base_ms) = std::thread::scope(|s| {
            let diff_root = repo_root.clone();
            let base_root = repo_root.clone();
            let commits_root = repo_root.clone();
            let base_branch = freshness.base_branch.clone();
            let diff_h = s.spawn(move || {
                let t = std::time::Instant::now();
                let res = run_gh_pr_diff_for_open(&*state.ports.git, &diff_root, pr_number);
                (res, t.elapsed().as_millis())
            });
            let base_h = s.spawn(move || {
                let t = std::time::Instant::now();
                let res = er_engine::github::ensure_base_ref_available(
                    &*state.ports.git,
                    &base_root,
                    &base_branch,
                )
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
        let metadata = run_gh_pr_view_for_open(&*state.ports.github, &repo_root, pr_number)?;
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
                &*state.ports.git,
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
                let res = run_gh_pr_diff_for_open(&*state.ports.git, &diff_root, pr_number);
                (res, t.elapsed().as_millis())
            });
            let base_h = s.spawn(move || {
                let t = std::time::Instant::now();
                let res = er_engine::github::ensure_base_ref_available(
                    &*state.ports.git,
                    &base_root,
                    &base_branch,
                )
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
        let view_github = Arc::clone(&state.ports.github);
        let view = s.spawn(move || {
            let t = std::time::Instant::now();
            let res = run_gh_pr_view_for_open(&*view_github, &view_root, pr_number);
            (res, t.elapsed().as_millis())
        });
        let diff = s.spawn(move || {
            let t = std::time::Instant::now();
            let res = run_gh_pr_diff_for_open(&*state.ports.git, &diff_root, pr_number);
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
    let resolved_base = er_engine::github::ensure_base_ref_available(
        &*state.ports.git,
        &repo_root,
        &metadata.freshness.base_branch,
    )
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
    {
        // The open path is service-owned now; the hint is not consumed until
        // the desktop renders from the view.
        drop(hint);
        run_blocking(move || {
            service_open_pr_review(&state, project_id, pr_number, replace)?;
            Ok(AppSnapshot::default())
        })
        .await
    }
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
    // Fast-path decision BEFORE any network: cache hit → the inputs are in
    // memory and the existing two-phase flow applies (~150–250 ms command).
    // Miss → place a stub instantly and let the offload worker run the
    // network phase (gh pr view + gh pr diff + commits, ~1.5–2.5 s) outside
    // the command; the full diff arrives via the revision-event poll.
    if !pr_open_cache_is_hit(&project_id, pr_number, hint.as_ref(), state) {
        return open_pr_review_miss_async(
            &project_id,
            pr_number,
            hint.as_ref(),
            replace,
            state,
            &branch_label,
            t_total,
        );
    }
    let t_tab_build = std::time::Instant::now();
    let inputs =
        load_pr_open_inputs(&project_id, pr_number, hint.as_ref(), state).map_err(|e| {
            log::error!("open_pr_review: pr=#{pr_number} project_id={project_id} err={e}");
            e
        })?;
    let cache_hit = inputs.cache_hit;
    // Branch-view preload seed for the miss path: the diff was just fetched,
    // and for a local PR tab without a local checkout the branch scope IS the
    // same `gh pr diff` — seed it so the first Branch switch needs no network.
    let raw_diff_for_preload = if cache_hit {
        None
    } else {
        Some(inputs.raw_diff.clone())
    };
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
        state.ports.clone(),
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
    let checkout_root =
        resolve_head_checkout(&*state.ports.git, &repo_root_for_checkout, &checkout_branch);
    log_branch_open_phase(&project_id, &branch_label, "pr_tab_build", t_tab_build);
    log::info!(
        "branch_open project={} branch={} phase=pr_open_cache hit={}",
        project_id,
        branch_label,
        cache_hit
    );
    let t_app_lock = std::time::Instant::now();
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    log_branch_open_phase(&project_id, &branch_label, "app_lock", t_app_lock);
    let t_place_tab = std::time::Instant::now();
    // Skip the storage sync: `enter_pr_diff_*` below performs the authoritative
    // apply_managed_root + AI reload for the PR bucket, so syncing here would
    // make three full reloads per open instead of one.
    place_tab(
        &mut app,
        &state.view,
        new_tab,
        replace.unwrap_or(false),
        true,
    );
    log_branch_open_phase(&project_id, &branch_label, "tab_place", t_place_tab);
    // Attach the checkout root (if any) to the now-active tab before entering
    // PR Diff, so the first snapshot already reflects the working-tree views.
    if let Some(root) = checkout_root {
        app.tab_mut().local_branch_checkout_root = Some(root);
    }
    let t_pr_diff = std::time::Instant::now();
    // The tab already holds the exact `gh pr diff` output (placed by
    // `new_local_pr_from_github_diff` from `inputs.raw_diff`), so trust it and
    // skip the redundant `gh pr diff` that `enter_pr_diff()` would run via
    // `refresh_diff()`. When the diff's head oid is known — cache hit, or a
    // miss with a complete hint / fresh `gh pr view` — `enter_pr_diff_preloaded`
    // also seeds the staleness baseline from that oid and skips the two
    // sequential network ref-fetches (`fetch_pr_head` + `fetch_base_branch_ref`)
    // that `enter_pr_diff_freshly_loaded()` would run on first entry; those
    // refs only serve the local Branch view and are fetched in the background
    // by `kick_pr_ref_fetch` below. Fall back to `enter_pr_diff_freshly_loaded`
    // (which still skips the redundant refresh, but does the first-entry ref
    // fetch and falls back to a full refresh when `files` is empty) for a
    // 0-file PR or an unknown head oid, where seeding would mislead the probe.
    let has_loaded_files = !app.tab().files.is_empty();
    let two_phase = if !head_oid_for_preload.trim().is_empty() && has_loaded_files {
        // Two-phase open: enter PR Diff without the AI sidecar reload —
        // `kick_post_open_offload` performs the single authoritative reload
        // right after the command returns, and the chrome-only snapshot below
        // paints immediately with "Loading diff…".
        app.tab_mut()
            .enter_pr_diff_preloaded(head_oid_for_preload, true)
            .map_err(|e| e.to_string())?;
        app.tab_mut().needs_initial_refresh = false;
        true
    } else {
        // Rare fallback (0-file PR / unknown head oid): keep the synchronous
        // first-entry ref fetch + reload — correctness first, the diff fetch
        // dominates anyway and the overlay already paints during it.
        app.tab_mut()
            .enter_pr_diff_freshly_loaded()
            .map_err(|e| e.to_string())?;
        app.tab_mut().needs_initial_refresh = false;
        false
    };
    // Seed the branch preload from the freshly fetched diff (miss only — a
    // cache hit may predate the current head, so kick_branch_preload below
    // re-fetches fresh content in the background instead). Skipped when the
    // head branch is checked out locally (working-tree scope, staleness-prone).
    if let Some(raw) = raw_diff_for_preload {
        if app.tab().local_branch_checkout_root.is_none() {
            let tab = app.tab_mut();
            tab.preloaded_branch_raw = Some(er_engine::app::PreloadedBranchRaw {
                raw,
                preview_head_oid: tab.preview_head_oid.clone(),
                base_branch: tab.base_branch.clone(),
                pr_number: tab.pr_number,
                local_branch_view: tab.local_branch_view.clone(),
                checkout_root: None,
                remote_repo: None,
                pr_head_ref: tab.pr_head_ref.clone(),
                parity: true,
            });
        }
    }
    log_branch_open_phase(&project_id, &branch_label, "pr_diff_enter", t_pr_diff);
    let _ = projects::record_recent_pr(&project_id, pr_number, &recent_title);
    kick_meta_refresh(state, app.tab().repo_root.clone());
    let t_snapshot = std::time::Instant::now();
    let snapshot = if two_phase {
        // Offload the full snapshot + AI reload to the background worker; the
        // poll delivers it via the revision event in ~40–120 ms.
        kick_post_open_offload(&app, state);
        lite_snap_from_command(&app, state)
    } else {
        snap_from_command(&app, state)
    };
    log_branch_open_phase(&project_id, &branch_label, "snapshot_build", t_snapshot);
    log_branch_open_phase(&project_id, &branch_label, "total", t_total);
    kick_active_gh_status(&app, state);
    // Background-preload the Branch-view diff so the first switch to the
    // Branch view doesn't run `gh pr diff` synchronously under the App lock.
    kick_branch_preload(&app, state);
    // Background-fetch the PR's local git refs (skipped by the fast
    // `enter_pr_diff_preloaded` path) so later local-ref consumers find them.
    kick_pr_ref_fetch(&app, state);
    Ok(snapshot)
}

/// Async-miss PR open: place a stub tab (chrome-only, "Loading diff…") and
/// return immediately; `kick_miss_open_offload` fetches the inputs and
/// populates the tab in the background. Mirrors the cache-hit two-phase flow
/// so misses get the same instant-command behavior — the network phase
/// (~1.5–2.5 s for `gh pr view` + `gh pr diff` + commits) leaves the command.
fn open_pr_review_miss_async(
    project_id: &str,
    pr_number: u64,
    hint: Option<&PrOpenHint>,
    replace: Option<bool>,
    state: &AppState,
    branch_label: &str,
    t_total: std::time::Instant,
) -> Result<AppSnapshot, String> {
    let t_tab_build = std::time::Instant::now();
    let file = projects::load();
    let proj = file
        .projects
        .iter()
        .find(|p| p.id == project_id)
        .ok_or_else(|| format!("Project not found: {project_id}"))?
        .clone();
    let base = hint
        .map(|h| h.base_ref.clone())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "main".to_string());
    let head = hint.map(|h| h.head_ref.clone());
    let head_oid = hint.map(|h| h.head_oid.clone());
    let pr_data = hint.map(|h| pr_overview_from_hint(h, pr_number, proj.remote.as_deref()));
    let recent_title = hint
        .map(|h| h.title.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("#{pr_number}"));
    let stub = er_engine::app::TabState::new_stub_pr_tab(
        proj.root_path.clone(),
        pr_number,
        base,
        head,
        pr_data,
        head_oid,
        state.ports.clone(),
    )
    .map_err(|e| e.to_string())?;
    let tab_build_ms = t_tab_build.elapsed().as_millis();
    log_branch_open_phase(project_id, branch_label, "pr_tab_build", t_tab_build);
    log::info!(
        "branch_open project={} branch={} phase=pr_open_cache hit=false",
        project_id,
        branch_label
    );

    let t_app_lock = std::time::Instant::now();
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let app_lock_ms = t_app_lock.elapsed().as_millis();
    log_branch_open_phase(project_id, branch_label, "app_lock", t_app_lock);
    let t_place_tab = std::time::Instant::now();
    place_tab(&mut app, &state.view, stub, replace.unwrap_or(false), true);
    let tab_place_ms = t_place_tab.elapsed().as_millis();
    log_branch_open_phase(project_id, branch_label, "tab_place", t_place_tab);
    log_branch_open_phase(
        project_id,
        branch_label,
        "pr_diff_enter",
        std::time::Instant::now(),
    );
    let t_recent = std::time::Instant::now();
    let _ = projects::record_recent_pr(project_id, pr_number, &recent_title);
    let record_recent_ms = t_recent.elapsed().as_millis();
    kick_meta_refresh(state, app.tab().repo_root.clone());
    let t_snapshot = std::time::Instant::now();
    let stub_idx = app.active_tab;
    let expect_local_view = app.tab().local_branch_view.clone();
    kick_miss_open_offload(
        state,
        project_id,
        pr_number,
        hint.cloned(),
        proj.root_path,
        stub_idx,
        expect_local_view,
    );
    let snapshot = lite_snap_from_command(&app, state);
    let snap_build_ms = t_snapshot.elapsed().as_millis();
    log_branch_open_phase(project_id, branch_label, "snapshot_build", t_snapshot);
    log_branch_open_phase(project_id, branch_label, "total", t_total);
    kick_active_gh_status(&app, state);
    // No `kick_branch_preload` here: the offload worker seeds the branch
    // preload from the freshly fetched parity diff — a second `gh pr diff`
    // would duplicate the network call.
    kick_pr_ref_fetch(&app, state);
    let t_ser = std::time::Instant::now();
    // Measuring the snapshot means serializing the whole thing — every file and
    // hunk — and Tauri already serializes the same value again for IPC. That is
    // a second full pass over the payload on the open path, and this one runs
    // while the App lock is still held. Only the profiled build pays for it.
    let ser_bytes = if crate::profile_log::profile_enabled() {
        serde_json::to_vec(&snapshot).map(|v| v.len()).unwrap_or(0)
    } else {
        0
    };
    log::info!(
        "open_pr_review pr={} phase=summary cache_hit=false files={} app_lock_ms={} tab_build_ms={} tab_place_ms={} pr_diff_enter_ms=0 record_recent_ms={} snap_build_ms={} ser_bytes={} ser_ms={} total_ms={}",
        pr_number,
        snapshot.files.len(),
        app_lock_ms,
        tab_build_ms,
        tab_place_ms,
        record_recent_ms,
        snap_build_ms,
        ser_bytes,
        t_ser.elapsed().as_millis(),
        t_total.elapsed().as_millis(),
    );
    Ok(snapshot)
}

/// Background offload for the async-miss PR open: runs the network phase
/// (`load_pr_open_inputs` — parallel `gh pr view` + `gh pr diff` + commits)
/// WITHOUT the app lock, populates the stub under a brief lock (parse + AI
/// reload), seeds the branch preload from the fetched parity diff, and bumps
/// `desktop_revision` so the poll delivers the full diff. On failure the tab
/// stays `needs_initial_refresh = true` with a notify — a later click on the
/// tab retries via the deferred-refresh worker.
fn kick_miss_open_offload(
    state: &AppState,
    project_id: &str,
    pr_number: u64,
    hint: Option<PrOpenHint>,
    expect_root: String,
    expect_idx: usize,
    expect_local_view: Option<String>,
) {
    if let Ok(mut l) = state.loading.lock() {
        l.tab_diff = true;
    }
    let project_id = project_id.to_string();
    let app_arc = Arc::clone(&state.app);
    let loading = Arc::clone(&state.loading);
    let desktop_revision = Arc::clone(&state.desktop_revision);
    let state_ref = state.clone();
    std::thread::spawn(move || {
        let t = std::time::Instant::now();
        match load_pr_open_inputs(&project_id, pr_number, hint.as_ref(), &state_ref) {
            Ok(inputs) => {
                let head_branch = inputs.metadata.freshness.head_branch.clone();
                let checkout_root = if head_branch.is_empty() {
                    None
                } else {
                    resolve_head_checkout(&*state_ref.ports.git, &inputs.repo_root, &head_branch)
                };
                if let Ok(mut app) = app_arc.lock() {
                    // Re-resolve the stub by index + identity (sibling worker
                    // pattern); if a tab closed during the fetch window
                    // shifted the index, fall back to a scan so the populated
                    // diff still lands (mirrors the error path).
                    let target = match app.tabs.get_mut(expect_idx).filter(|t| {
                        t.repo_root == expect_root
                            && t.pr_number == Some(pr_number)
                            && t.local_branch_view == expect_local_view
                    }) {
                        Some(tab) => Some(tab),
                        None => app.tabs.iter_mut().find(|t| {
                            t.repo_root == expect_root
                                && t.pr_number == Some(pr_number)
                                && t.needs_initial_refresh
                        }),
                    };
                    if let Some(tab) = target {
                        tab.populate_pr_tab(
                            &inputs.raw_diff,
                            Some(inputs.metadata.pr_data),
                            inputs.metadata.pr_commits,
                            inputs.resolved_base.clone(),
                            Some(inputs.metadata.freshness.head_oid.clone()),
                        );
                        if let Some(root) = checkout_root {
                            tab.local_branch_checkout_root = Some(root);
                        }
                        // Seed the branch preload (parity) so the first Branch
                        // switch needs no network.
                        if tab.local_branch_checkout_root.is_none() {
                            tab.preloaded_branch_raw = Some(er_engine::app::PreloadedBranchRaw {
                                raw: inputs.raw_diff,
                                preview_head_oid: tab.preview_head_oid.clone(),
                                base_branch: inputs.resolved_base,
                                pr_number: Some(pr_number),
                                local_branch_view: tab.local_branch_view.clone(),
                                checkout_root: None,
                                remote_repo: None,
                                pr_head_ref: tab.pr_head_ref.clone(),
                                parity: true,
                            });
                        }
                    }
                }
            }
            Err(e) => {
                log::error!(
                    "er-desktop: async PR open failed pr={pr_number} project={project_id}: {e}"
                );
                if let Ok(mut app) = app_arc.lock() {
                    // Re-resolve by index + identity; if a tab closed during
                    // the fetch window shifted the index, fall back to a scan
                    // so the notify + retry flag still land on the stub.
                    let target = if let Some(tab) = app.tabs.get_mut(expect_idx).filter(|t| {
                        t.repo_root == expect_root
                            && t.pr_number == Some(pr_number)
                            && t.local_branch_view == expect_local_view
                    }) {
                        Some(tab)
                    } else {
                        // Narrow to un-populated stubs so the retry flag lands
                        // on a stub, not on a populated duplicate tab.
                        app.tabs.iter_mut().find(|t| {
                            t.repo_root == expect_root
                                && t.pr_number == Some(pr_number)
                                && t.needs_initial_refresh
                        })
                    };
                    if let Some(tab) = target {
                        tab.needs_initial_refresh = true;
                        app.notify(&format!("Failed to open PR #{pr_number}: {e}"));
                    }
                }
            }
        }
        if let Ok(mut l) = loading.lock() {
            l.tab_diff = false;
        }
        desktop_revision.fetch_add(1, Ordering::Relaxed);
        crate::profile_log::profile_log(
            "miss_open_offload",
            &[("ms", t.elapsed().as_millis().to_string())],
        );
    });
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
    run_blocking(move || {
        service_open_pr_review(&state, project_id, pr_number, replace)?;
        Ok(AppSnapshot::default())
    })
    .await
}

/// Fire-and-forget background warmup of the PR-open cache. Invoked from the
///
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
    prefetch_pr_open_impl(project_id, pr_number, hint, state.inner())
}

/// With `v1-service` the app registry warms its own cache through the ports.
pub(crate) fn prefetch_pr_open_impl(
    project_id: String,
    pr_number: u64,
    hint: PrOpenHint,
    state: &DesktopState,
) -> Result<(), String> {
    let file = projects::load();
    let Some(proj) = file.projects.iter().find(|p| p.id == project_id) else {
        return Ok(());
    };
    let command = er_service::AppCommand::PrefetchOpenTarget {
        repo_root: proj.root_path.clone(),
        project_id,
        pr_number,
        freshness: freshness_from_hint(&hint),
    };
    state
        .service
        .send_app(command)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Fire-and-forget background warmup of the remote-only PR open cache.
///
/// Invoked from the sidebar's `onmouseenter` (after a short debounce) for
/// remote-only projects, whose open path (`open_remote_pr`) has no local git
/// clone to fall back on — a cache hit opens with zero `gh` calls.
#[tauri::command]
pub fn prefetch_remote_pr_open(
    owner: String,
    repo: String,
    number: u64,
    state: State<AppState>,
) -> Result<(), String> {
    prefetch_remote_pr_open_impl(owner, repo, number, state.inner())
}

pub(crate) fn prefetch_remote_pr_open_impl(
    owner: String,
    repo: String,
    number: u64,
    state: &DesktopState,
) -> Result<(), String> {
    let command = er_service::AppCommand::PrefetchRemoteOpenTarget {
        owner,
        repo,
        number,
    };
    state
        .service
        .send_app(command)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Trigger a manual PR-list refresh. Returns the current snapshot immediately
/// (the refresh runs in the background). Deduplicates: if a refresh is already
/// running, this is a no-op.
#[tauri::command]
pub fn refresh_pr_list(state: State<AppState>) -> Result<AppSnapshot, String> {
    let prefs_state = state.inner().clone();
    let prefs = move || crate::app_owner::startup::inbox_prefs(&prefs_state);
    kick_pr_list_refresh(&state, None, prefs)?;
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
    let prefs_state = state.inner().clone();
    let prefs = move || crate::app_owner::startup::inbox_prefs(&prefs_state);
    kick_pr_list_refresh(&state, Some(remote), prefs)?;
    snap!(state)
}

/// Starts a PR-list fetch through the sidebar's owner, of every project
/// remote or of `remote` alone, and runs the inbox over the result. A fetch
/// already running absorbs it. `prefs` reads the inbox prefs once the fetch
/// is done.
pub(crate) fn kick_pr_list_refresh(
    desktop: &DesktopState,
    remote: Option<String>,
    prefs: impl FnOnce() -> InboxConfig + Send + 'static,
) -> Result<(), String> {
    let Some(refresh) = desktop.begin_pr_list_refresh() else {
        return Ok(());
    };
    let desktop = desktop.clone();
    std::thread::spawn(move || {
        #[expect(
            clippy::expect_used,
            reason = "a worker thread that cannot build its runtime has nothing to fall back to"
        )]
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime");
        let github = Arc::clone(&desktop.ports.github);
        let owner = &desktop.app_owner;
        let (failed, reason) = match &remote {
            None => (
                rt.block_on(owner.refresh_prs(github)),
                "pr_cache_refresh_manual",
            ),
            Some(remote) => {
                let fetched = rt.block_on(owner.refresh_remote_prs(github, remote));
                let failed = if fetched {
                    Vec::new()
                } else {
                    vec![remote.clone()]
                };
                (failed, "pr_cache_refresh_project_manual")
            }
        };
        let prefs = prefs();
        let handles = InboxRefreshHandles {
            pr_cache: &desktop.pr_cache,
            gh_user: &desktop.gh_user,
            inbox: &desktop.inbox,
            desktop_revision: &desktop.desktop_revision,
            app_handle: &desktop.tauri_app_handle,
        };
        for remote in failed {
            process_inbox_after_pr_refresh(handles, &prefs, Some(remote));
        }
        process_inbox_after_pr_refresh(handles, &prefs, None);
        drop(refresh);
        crate::profile_log::bump_desktop_revision(&desktop.desktop_revision, reason);
    });
    Ok(())
}

/// Apply an inbox mutation the frontend has already painted optimistically.
///
/// Returns a chrome-only snapshot: the inbox is app-wide chrome, so there is no
/// reason to rebuild files/AI/commits (git subprocesses included) for a
/// read-state flip. The frontend merges it onto whatever view is showing.
/// Runs on the blocking pool — callers wrap it in `run_blocking`.
fn mutate_inbox(
    state: &AppState,
    command: &'static str,
    f: impl FnOnce(&mut InboxState),
) -> Result<AppSnapshot, String> {
    state.app_owner.write_inbox(f).map_err(|e| {
        log::error!("[inbox] command={command} failed to lock inbox: {e}");
        e
    })?;
    // Clearing read items changes neither hashed inbox input (unread count,
    // last refresh), so the bump is what lets the poll notice the change.
    crate::profile_log::bump_desktop_revision(&state.desktop_revision, command);
    let app = state.app.lock().map_err(|e| e.to_string())?;
    Ok(chrome_snap_from(&app, state))
}

#[tauri::command]
pub async fn mark_inbox_item_read(
    id: String,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let now = now_ms();
        mutate_inbox(&state, "mark_inbox_item_read", |inbox| {
            inbox.mark_item_read(&id, now);
        })
    })
    .await
}

#[tauri::command]
pub async fn mark_all_inbox_read(state: State<'_, AppState>) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let now = now_ms();
        mutate_inbox(&state, "mark_all_inbox_read", |inbox| {
            inbox.mark_all_read(now);
        })
    })
    .await
}

#[tauri::command]
pub async fn mark_inbox_items_read(
    ids: Vec<String>,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let now = now_ms();
        mutate_inbox(&state, "mark_inbox_items_read", |inbox| {
            inbox.mark_items_read(&ids, now);
        })
    })
    .await
}

#[tauri::command]
pub async fn clear_read_inbox_items(state: State<'_, AppState>) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        mutate_inbox(&state, "clear_read_inbox_items", |inbox| {
            inbox.clear_read();
        })
    })
    .await
}

#[tauri::command]
pub async fn clear_inbox_items(
    ids: Vec<String>,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        // Same contract as global trash: only already-read items are removed.
        mutate_inbox(&state, "clear_inbox_items", |inbox| {
            inbox.clear_read_items(&ids);
        })
    })
    .await
}

#[tauri::command]
pub fn refresh_notifications(state: State<AppState>) -> Result<AppSnapshot, String> {
    refresh_pr_list(state)
}

#[tauri::command]
pub fn test_native_notification(state: State<AppState>) -> Result<(), String> {
    show_test_notification(&state)
}

/// Shows a test banner, or says how to get one when the system would not.
pub(crate) fn show_test_notification(state: &DesktopState) -> Result<(), String> {
    let handle = state.tauri_app_handle.lock().ok().and_then(|g| g.clone());
    if show_native_notification(handle.as_ref(), "Easy Review", "Test notification") {
        Ok(())
    } else {
        Err(
            "Could not send a native notification. Launch the installed Easy Review.app \
             and enable notifications in System Settings."
                .into(),
        )
    }
}

/// Open a notification's target. `new_tab` opens it in a fresh tab; the
/// default replaces the active tab (the inbox dialog offers both).
#[tauri::command]
pub async fn open_inbox_item(
    id: String,
    new_tab: Option<bool>,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || open_inbox_item_impl(id, new_tab.unwrap_or(false), &state)).await
}

/// Build a `PrOpenHint` for an inbox PR open from the cached PR list. Returns
/// `None` if the PR is not cached or a required field is missing; the open
/// then falls back to the no-hint (async-miss) path.
///
/// The cache is keyed by each project's `remote` as written in projects.json
/// (`VilfredSikker/easy-review`), while inbox targets carry the lowercased
/// slug, so the key has to be matched after normalizing both sides.
fn inbox_pr_hint_from_cache(
    cache: &HashMap<String, Vec<crate::snapshot::PrInfo>>,
    pr_number: u64,
    remote: &str,
) -> Option<PrOpenHint> {
    let slug = normalize_remote_slug(remote);
    let pr = cache
        .iter()
        .filter(|(key, _)| normalize_remote_slug(key) == slug)
        .find_map(|(_, prs)| prs.iter().find(|p| p.number == pr_number))?;
    if pr.base_ref.trim().is_empty()
        || pr.head_ref.trim().is_empty()
        || pr.head_oid.trim().is_empty()
    {
        return None;
    }
    Some(PrOpenHint {
        base_ref: pr.base_ref.clone(),
        head_ref: pr.head_ref.clone(),
        head_oid: pr.head_oid.clone(),
        updated_at: pr.updated_at.clone(),
        title: pr.title.clone(),
        author: pr.author.clone(),
    })
}

/// What an inbox item opens, resolved the way the sidebar picks its command,
/// so a notification open and a sidebar click take the same path.
enum InboxOpenTarget {
    LocalPr {
        project_id: String,
        pr_number: u64,
        hint: Option<PrOpenHint>,
    },
    /// A remote-only project has no clone for `open_pr_review` to run in.
    RemotePr {
        owner: String,
        repo: String,
        number: u64,
    },
    Branch {
        project_id: String,
        branch: String,
    },
}

fn resolve_inbox_open_target(target: InboxTarget, state: &DesktopState) -> Option<InboxOpenTarget> {
    let file = projects::load();
    // A poisoned cache only costs the hint, not the open.
    let empty = HashMap::new();
    let guard = state.pr_cache.lock().ok();
    inbox_open_target_in(&file, guard.as_deref().unwrap_or(&empty), target)
}

fn inbox_open_target_in(
    file: &projects::ProjectsFile,
    cache: &HashMap<String, Vec<crate::snapshot::PrInfo>>,
    target: InboxTarget,
) -> Option<InboxOpenTarget> {
    // Always re-resolve: a stored id can name a project that no longer exists,
    // and an unchecked id makes the open a hard "Project not found" error.
    let project_id = projects::resolve_inbox_project_in_file(
        file,
        target.project_id.as_deref(),
        target.repo_root.as_deref(),
        target.remote.as_deref(),
    )?;
    let Some(pr_number) = target.pr_number else {
        return target
            .branch
            .map(|branch| InboxOpenTarget::Branch { project_id, branch });
    };
    let proj = file.projects.iter().find(|p| p.id == project_id)?;
    if proj.root_path.is_empty() {
        let remote = proj.remote.as_deref().or(target.remote.as_deref())?;
        let (owner, repo) = crate::projects::remote_owner_repo(remote)?;
        return Some(InboxOpenTarget::RemotePr {
            owner,
            repo,
            number: pr_number,
        });
    }
    let hint = target
        .remote
        .as_deref()
        .or(proj.remote.as_deref())
        .and_then(|remote| inbox_pr_hint_from_cache(cache, pr_number, remote));
    Some(InboxOpenTarget::LocalPr {
        project_id,
        pr_number,
        hint,
    })
}

fn inbox_item_target(id: &str, state: &DesktopState) -> Option<InboxTarget> {
    let inbox = state.inbox.lock().ok()?;
    inbox
        .items
        .iter()
        .find(|i| i.id == id)
        .map(|i| i.target.clone())
}

/// Warm the open cache for an inbox item while its message dialog is up, the
/// same warmup the sidebar runs on hover. A cold open pays the full
/// `gh pr diff` round-trip that a warmed sidebar click skips.
#[tauri::command]
pub fn prefetch_inbox_item(id: String, state: State<AppState>) -> Result<(), String> {
    prefetch_inbox_target(&id, state.inner())
}

/// What the open-cache prefetches take: the desktop's state with the flag
/// on, the App's without.
type PrefetchState = DesktopState;

/// Warms the open cache for the inbox item's PR; an item with no PR, or one
/// gone from the inbox, warms nothing.
pub(crate) fn prefetch_inbox_target(id: &str, state: &PrefetchState) -> Result<(), String> {
    let Some(target) = inbox_item_target(id, state) else {
        return Ok(());
    };
    match resolve_inbox_open_target(target, state) {
        Some(InboxOpenTarget::LocalPr {
            project_id,
            pr_number,
            hint: Some(hint),
        }) => prefetch_pr_open_impl(project_id, pr_number, hint, state),
        Some(InboxOpenTarget::RemotePr {
            owner,
            repo,
            number,
        }) => prefetch_remote_pr_open_impl(owner, repo, number, state),
        _ => Ok(()),
    }
}

/// Marks the inbox item read through the sidebar's owner, and answers what
/// it targets; none for an id the inbox does not hold.
fn read_inbox_item(id: &str, state: &DesktopState) -> Result<Option<InboxTarget>, String> {
    let now = now_ms();
    let mut target = None;
    state
        .app_owner
        .write_inbox(|inbox| {
            target = inbox
                .items
                .iter()
                .find(|i| i.id == id)
                .map(|i| i.target.clone());
            inbox.mark_item_read(id, now);
        })
        .map_err(|e| {
            log::error!("[inbox] command=open_inbox_item failed to lock inbox: {e}");
            e
        })?;
    Ok(target)
}

const NO_INBOX_TARGET: &str =
    "Could not open notification target — add or select the project in Easy Review first";

/// `er_send`'s inbox open: the read, then the open a sidebar click takes. A
/// target no project holds is refused after the read has landed.
pub(crate) fn service_open_inbox_item(
    state: &DesktopState,
    id: &str,
    replace: bool,
) -> Result<(), String> {
    match read_inbox_item(id, state)?.and_then(|t| resolve_inbox_open_target(t, state)) {
        Some(target) => service_open_inbox_target(state, target, Some(replace)),
        None => Err(NO_INBOX_TARGET.into()),
    }
}

fn open_inbox_item_impl(
    id: String,
    new_tab: bool,
    state: &AppState,
) -> Result<AppSnapshot, String> {
    let replace = Some(!new_tab);
    let target = read_inbox_item(&id, state)?;
    state.desktop_revision.fetch_add(1, Ordering::Relaxed);

    if let Some(target) = target
        .clone()
        .and_then(|t| resolve_inbox_open_target(t, state))
    {
        service_open_inbox_target(state, target, replace)?;
        return Ok(AppSnapshot::default());
    }
    match target.and_then(|t| resolve_inbox_open_target(t, state)) {
        Some(InboxOpenTarget::LocalPr {
            project_id,
            pr_number,
            hint,
        }) => return open_pr_review_impl(project_id, pr_number, replace, hint, state),
        Some(InboxOpenTarget::RemotePr {
            owner,
            repo,
            number,
        }) => return open_remote_pr_impl(owner, repo, number, replace, state),
        Some(InboxOpenTarget::Branch { project_id, branch }) => {
            return open_local_branch_impl(project_id, branch, replace, state);
        }
        None => {}
    }

    if let Ok(mut app) = state.app.lock() {
        app.notify(NO_INBOX_TARGET);
    }
    snap!(state)
}

/// An inbox open through the same flag-on helpers as a sidebar click.
fn service_open_inbox_target(
    state: &DesktopState,
    target: InboxOpenTarget,
    replace: Option<bool>,
) -> Result<(), String> {
    match target {
        InboxOpenTarget::LocalPr {
            project_id,
            pr_number,
            ..
        } => service_open_pr_review(state, project_id, pr_number, replace),
        InboxOpenTarget::RemotePr {
            owner,
            repo,
            number,
        } => service_open_remote_pr(state, owner, repo, number, replace),
        InboxOpenTarget::Branch { project_id, branch } => {
            let tab = content_routes::branch_tab(&project_id, branch)?;
            content_routes::open(state, tab, replace.unwrap_or(false))
        }
    }
}

#[tauri::command]
pub fn add_tracked_branch(
    project_id: String,
    name: String,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    track_branch(&state, &project_id, &name)?;
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
    untrack_branch(&state, &project_id, &name)?;
    let root = state
        .app
        .lock()
        .ok()
        .map(|a| a.tab().repo_root.clone())
        .unwrap_or_default();
    kick_meta_refresh(&state, root);
    snap!(state)
}

/// Tracks `name` under its project once git confirms the branch exists
/// locally.
pub(crate) fn track_branch(
    desktop: &DesktopState,
    project_id: &str,
    name: &str,
) -> Result<(), String> {
    let file = projects::load();
    let proj = file
        .projects
        .iter()
        .find(|p| p.id == project_id)
        .ok_or_else(|| format!("Project not found: {project_id}"))?;

    // Confirm the branch actually exists locally before tracking it.
    let exists = desktop
        .ports
        .git
        .show_ref_verify(&proj.root_path, &format!("refs/heads/{name}"))
        .map_err(|e| format!("git show-ref failed: {e}"))?;
    if !exists {
        return Err(format!("Branch '{name}' does not exist locally"));
    }

    projects::add_tracked_branch(project_id, name).map_err(|e| e.to_string())
}

/// Hides `name` from its project's list; the checked-out branch stays.
pub(crate) fn untrack_branch(
    desktop: &DesktopState,
    project_id: &str,
    name: &str,
) -> Result<(), String> {
    let file = projects::load();
    let proj = file
        .projects
        .iter()
        .find(|p| p.id == project_id)
        .ok_or_else(|| format!("Project not found: {project_id}"))?;

    let current = desktop
        .ports
        .git
        .current_branch_show(&proj.root_path)
        .unwrap_or_default();
    if !current.is_empty() && current == name {
        return Err(
            "Cannot remove the currently-checked-out branch from view; switch first.".to_string(),
        );
    }

    projects::remove_tracked_branch(project_id, name).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_available_branches(
    project_id: String,
    state: State<AppState>,
) -> Result<Vec<String>, String> {
    project_available_branches(&state, &project_id)
}

/// The project's local branches neither checked out nor tracked.
pub(crate) fn project_available_branches(
    desktop: &DesktopState,
    project_id: &str,
) -> Result<Vec<String>, String> {
    let file = projects::load();
    let proj = file
        .projects
        .iter()
        .find(|p| p.id == project_id)
        .ok_or_else(|| format!("Project not found: {project_id}"))?;

    available_branches(&*desktop.ports.git, &proj.root_path, &proj.tracked_branches)
}

/// Branch names not checked out and not already tracked.
fn available_branches(
    git: &dyn er_engine::git::Git,
    root_path: &str,
    tracked: &[String],
) -> Result<Vec<String>, String> {
    // `GitError` already words a failed call, context included; adding the
    // context again here would print it twice.
    let text = git
        .for_each_ref(root_path, "%(refname:short)", None, "refs/heads/")
        .map_err(|e| e.to_string())?;

    let current = git.current_branch_show(root_path).unwrap_or_default();

    let names: Vec<String> = text
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|n| !n.is_empty())
        .filter(|n| n != &current && !tracked.iter().any(|t| t == n))
        .collect();
    Ok(names)
}

#[tauri::command]
pub fn delete_project(project_id: String, state: State<AppState>) -> Result<AppSnapshot, String> {
    delete_project_and_its_prs(&state, &project_id)?;
    snap!(state)
}

/// Deletes the project, and the cached PRs of its remote when no other
/// project shares it.
pub(crate) fn delete_project_and_its_prs(
    desktop: &DesktopState,
    project_id: &str,
) -> Result<(), String> {
    let state = desktop;
    let before = projects::load();
    let deleted_remote = before
        .projects
        .iter()
        .find(|p| p.id == project_id)
        .and_then(|p| p.remote.clone());
    projects::delete_project(project_id).map_err(|e| e.to_string())?;
    if let Some(remote) = deleted_remote {
        let remaining = projects::load();
        let target = normalize_remote_slug(&remote);
        let remote_still_used = remaining.projects.iter().any(|p| {
            p.remote
                .as_ref()
                .is_some_and(|r| normalize_remote_slug(r) == target)
        });
        if !remote_still_used {
            state.app_owner.forget_remote_prs(&remote);
        }
    }
    state.desktop_revision.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
#[expect(
    non_snake_case,
    reason = "camelCase parameters match the keys of the JS IPC payload"
)]
pub fn reorder_projects(
    orderedIds: Vec<String>,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    if let Err(e) = projects::reorder_projects(&orderedIds) {
        log::error!(
            "reorder_projects failed id_count={} err={e}",
            orderedIds.len()
        );
        return Err(e.to_string());
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
    service_open_project_branch(&state, project_id, branch, replace.unwrap_or(false))
        .map(|()| AppSnapshot::default())
}

pub(crate) fn service_open_project_branch(
    state: &DesktopState,
    project_id: String,
    branch: String,
    replace: bool,
) -> Result<(), String> {
    let tab = content_routes::branch_tab(&project_id, branch)?;
    content_routes::open(state, tab, replace)?;
    projects::set_active(&project_id);
    Ok(())
}

#[tauri::command]
pub async fn dismiss_remote_pr(
    project_id: String,
    pr_number: u64,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        projects::dismiss_pr(&project_id, pr_number);
        confirm_projects_chrome(&state, "projects_dismiss")
    })
    .await
}

#[tauri::command]
pub async fn undismiss_remote_pr(
    project_id: String,
    pr_number: u64,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        projects::undismiss_pr(&project_id, pr_number).map_err(|e| e.to_string())?;
        confirm_projects_chrome(&state, "projects_undismiss")
    })
    .await
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
                let Some(tab) = app.tabs.get_mut(i) else {
                    continue;
                };
                if let Err(e) = tab.refetch_and_refresh_diff() {
                    log::warn!("sync_pr: diff refresh failed for tab {i}: {e}");
                    continue;
                }
                // Remote tabs can't form the local open key (their
                // repo_root is the launch CWD, not the checkout) — and
                // the remote open path doesn't read the cache yet, so
                // only local PR tabs are worth persisting here.
                if tab.is_remote() {
                    // Realign the stale-pill baseline after a legit
                    // sync: refetch_and_refresh_diff's remote branch
                    // never updates last_diff_head_oid, so without
                    // this the pill stays lit forever.
                    if let Some(oid) = tab
                        .pr_number
                        .and_then(|pr_number| pr_cache_head_oid_for_pr(&state, pr_number))
                    {
                        tab.last_diff_head_oid = Some(oid);
                    }
                } else if refreshed_local_diff_oid.is_none() {
                    refreshed_local_diff_oid = Some(tab.last_diff_head_oid.clone());
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
            match run_gh_pr_diff_for_open(&*state.ports.git, &root_path, pr_number) {
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
                    log::warn!("sync_pr: cache-persist diff fetch failed for #{pr_number}: {e}");
                }
            }
        }

        refresh_active_status_after_sync(&state);
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

pub(crate) fn resolve_pr_title_for_project(
    project_id: &str,
    pr_number: u64,
    title: Option<String>,
    state: &DesktopState,
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

/// Confirm a projects.json mutation the frontend already painted (pin/ignore).
///
/// Chrome-only: the sidebar lists are chrome, so no files/AI/commits rebuild.
/// The bump is required — `compute_chrome_revision` does not hash
/// projects.json, so without it the poll would never notice the change.
/// No `kick_meta_refresh`: that repopulates branch metadata (a git sweep over
/// every project) which pinning never touches.
fn confirm_projects_chrome(state: &AppState, reason: &'static str) -> Result<AppSnapshot, String> {
    crate::profile_log::bump_desktop_revision(&state.desktop_revision, reason);
    let app = state.app.lock().map_err(|e| e.to_string())?;
    Ok(chrome_snap_from(&app, state))
}

#[tauri::command]
pub async fn save_pr(
    project_id: String,
    pr_number: u64,
    title: Option<String>,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let title = resolve_pr_title_for_project(&project_id, pr_number, title, &state)?;
        projects::save_pr(&project_id, pr_number, &title).map_err(|e| e.to_string())?;
        confirm_projects_chrome(&state, "projects_pin")
    })
    .await
}

#[tauri::command]
pub async fn unsave_pr(
    project_id: String,
    pr_number: u64,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        projects::unsave_pr(&project_id, pr_number).map_err(|e| e.to_string())?;
        confirm_projects_chrome(&state, "projects_unpin")
    })
    .await
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
    project_available_prs(&state, &project_id)
}

/// The project's cached PRs that no sidebar list shows.
pub(crate) fn project_available_prs(
    state: &DesktopState,
    project_id: &str,
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
    service_set_active_project(&state, &id).map(|()| AppSnapshot::default())
}

/// Opens a Working tab on the project's root through the registry, as the
/// other flag-on opens do.
pub(crate) fn service_set_active_project(state: &DesktopState, id: &str) -> Result<(), String> {
    let root = projects::load()
        .projects
        .iter()
        .find(|p| p.id == id)
        .map(|p| p.root_path.clone())
        .ok_or_else(|| format!("Project not found: {id}"))?;
    content_routes::open(state, content_routes::working_tab(root), false)?;
    projects::set_active(id);
    Ok(())
}

#[tauri::command]
pub fn delete_review_artifact(kind: String, state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    delete_review_artifact_from_app(&mut app, &kind)?;
    state
        .desktop_revision
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Ok(snap_from(&app, &state))
}

fn delete_review_artifact_from_app(app: &mut App, kind: &str) -> Result<(), String> {
    let er_dir = app.tab().er_dir();
    app.check_review_bucket_admission(&er_dir)
        .map_err(|e| e.to_string())?;

    match kind {
        "triage" => er_engine::app::cleanup_triage(&er_dir).map_err(|e| e.to_string())?,
        "review" => er_engine::app::cleanup_review_artifacts(&er_dir).map_err(|e| e.to_string())?,
        other => return Err(format!("Unknown review artifact kind: {other}")),
    }

    app.tab_mut().reload_ai_state();
    Ok(())
}

// ── Review checklist ────────────────────────────────────────────────────────

/// Toggle the checklist item at `index` and persist it to the view bucket.
///
/// The index addresses the flat `checklist.json` item list — the same address
/// the TUI cursor resolves to — and `view` gates the write so a click that
/// lands after the reviewer switched views does not write to the new bucket.
#[tauri::command]
pub async fn toggle_checklist_item(
    index: usize,
    view: Option<crate::snapshot::OptimisticView>,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        if let Some(early) = abort_wrong_view(&app, &state, view.as_ref()) {
            return early;
        }
        // An index the checklist no longer has writes nothing; the returned
        // snapshot is what the frontend reconciles its optimistic paint against.
        app.toggle_checklist_item_at(index)
            .map_err(|e| format!("Failed to toggle checklist item: {e}"))?;
        Ok(snap_from_confirmed(&app, &state))
    })
    .await
}

// ── Findings: dismiss / promote / reply (v1 stubs) ──────────────────────────

#[tauri::command]
pub async fn dismiss_finding(
    finding_id: String,
    view: Option<crate::snapshot::OptimisticView>,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        if let Some(early) = abort_wrong_view(&app, &state, view.as_ref()) {
            return early;
        }

        let er_dir = app.tab().er_dir();
        let removed = er_engine::ai::remove_finding_from_sidecars(&er_dir, &finding_id)
            .map_err(|e| format!("Failed to remove finding: {e}"))?;
        if !removed {
            return Err(format!("Finding not found: {finding_id}"));
        }

        let _ = er_engine::ai::delete_threads_linked_to_finding(
            &er_dir,
            &app.tab().github_comments_dir(),
            &finding_id,
        );

        let mut promotions = load_finding_promotions(&er_dir);
        if promotions.remove(&finding_id).is_some() {
            save_finding_promotions(&er_dir, &promotions)
                .map_err(|e| format!("Failed to update finding promotions: {e}"))?;
        }

        app.tab_mut().reload_ai_state();

        Ok(snap_from(&app, &state))
    })
    .await
}

/// A person marks a finding resolved: it stays on record, dimmed, instead of
/// being removed as `dismiss_finding` does. ADR 0047.
#[tauri::command]
pub async fn resolve_finding(
    finding_id: String,
    view: Option<crate::snapshot::OptimisticView>,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        if let Some(early) = abort_wrong_view(&app, &state, view.as_ref()) {
            return early;
        }
        write_finding_resolved(&mut app, &finding_id, true)?;
        Ok(snap_from(&app, &state))
    })
    .await
}

/// Reopen a finding a person, or the validate pass, resolved. ADR 0047.
#[tauri::command]
pub async fn unresolve_finding(
    finding_id: String,
    view: Option<crate::snapshot::OptimisticView>,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        if let Some(early) = abort_wrong_view(&app, &state, view.as_ref()) {
            return early;
        }
        write_finding_resolved(&mut app, &finding_id, false)?;
        Ok(snap_from(&app, &state))
    })
    .await
}

fn write_finding_resolved(app: &mut App, finding_id: &str, resolved: bool) -> Result<(), String> {
    let er_dir = app.tab().er_dir();
    let found = er_engine::ai::set_finding_resolved(&er_dir, finding_id, resolved)
        .map_err(|e| format!("Failed to update finding: {e}"))?;
    if !found {
        return Err(format!("Finding not found: {finding_id}"));
    }
    app.tab_mut().reload_ai_state();
    Ok(())
}

#[tauri::command]
pub async fn promote_finding_to_comment(
    finding_id: String,
    body: Option<String>,
    view: Option<crate::snapshot::OptimisticView>,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        route_annotation(&state, view.as_ref(), |context| {
            er_service::Command::PromoteFindingToComment {
                context,
                finding_id,
                body,
            }
        })
    })
    .await
}

#[tauri::command]
pub async fn reply_to_finding(
    finding_id: String,
    body: String,
    ai_assist: bool,
    id: Option<String>,
    view: Option<crate::snapshot::OptimisticView>,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        reply_to_finding_on_service(&state, finding_id, body, ai_assist, id, view.as_ref())
    })
    .await
}

/// The flag path of a finding reply. The plain reply is the workspace's
/// write; an AI-assisted one finds or creates the finding's thread there and
/// asks in it, with the reply being written shown in `jobs`.
fn reply_to_finding_on_service(
    state: &AppState,
    finding_id: String,
    body: String,
    ai_assist: bool,
    id: Option<String>,
    view: Option<&crate::snapshot::OptimisticView>,
) -> Result<AppSnapshot, String> {
    if ai_assist {
        crate::ai_ask::ask_on_finding(&state.desktop, None, None, finding_id, body, id)
            .map_err(|e| e.message)?;
        return Ok(AppSnapshot::default());
    }
    route_annotation(state, view, |context| er_service::Command::ReplyToFinding {
        context,
        finding_id,
        body,
        ai_assist: false,
        client_id: id,
    })
}

#[tauri::command]
pub async fn update_thread_message(
    id: String,
    body: String,
    view: Option<crate::snapshot::OptimisticView>,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let text = body.trim().to_string();
    if text.is_empty() {
        return Err("Message cannot be empty".to_string());
    }

    let state = state.inner().clone();
    run_blocking(move || {
        route_annotation(&state, view.as_ref(), |context| {
            er_service::Command::UpdateThreadMessage { context, id, body }
        })
    })
    .await
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
pub async fn select_commit(sha: String, state: State<'_, AppState>) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || content_routes::select_commit(&state, &sha)).await
    // A commit row is a primary click target, and loading it runs git log and
    // git diff under the App lock — on the main thread that freezes the window.
}

/// "All changes" from the single-commit view. The engine recorded which view
/// History was entered from, so the frontend does not have to guess between
/// `pr_diff` and `branch`.
#[tauri::command]
pub async fn exit_history(state: State<'_, AppState>) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || content_routes::exit_history(&state)).await
    // Leaving History re-runs the view's git diff under the App lock.
}

// ── Tab management ──────────────────────────────────────────────────────────

// With `v1-service` the four tab commands go to the app registry, the tab
// record's one writer. The desktop maps the strip's index to the registry's
// tab key here. The App keeps its own tab list and keys until the service
// builds the desktop's snapshot; the flag-on desktop does not render from the
// registry yet, so each command answers with an empty snapshot.

fn service_tab_key(state: &DesktopState, idx: usize) -> Result<er_engine::TabKey, String> {
    let view = state.service.app_registry().tabs_view();
    view.key_at(idx)
        .ok_or_else(|| format!("no tab at position {idx}"))
}

fn service_tab_command(
    state: &DesktopState,
    command: er_service::AppCommand,
) -> Result<AppSnapshot, String> {
    state.service.send_app(command).map_err(|e| e.to_string())?;
    Ok(AppSnapshot::default())
}

/// A Working tab on the active tab's repo, read from the registry's view
/// rather than the App, so the command never waits on the App lock.
pub(crate) fn service_new_tab(state: &DesktopState) -> Result<AppSnapshot, String> {
    let view = state.service.app_registry().tabs_view();
    let root = new_tab_root(&view, project_root_for_remote)
        .ok_or_else(|| "Failed to open new tab: no open tab names a repo".to_string())?;
    let tab = content_routes::working_tab(root);
    service_tab_command(state, er_service::AppCommand::NewTab { tab })
}

/// The root a new Working tab opens on: the active tab's. A remote PR has no
/// local root, so it takes the root of the project that tracks its repo, and
/// failing that the first open tab that has one.
fn new_tab_root(
    view: &er_service::TabsView,
    project_root: impl Fn(&str) -> String,
) -> Option<String> {
    use er_engine::tabs_record::TabKind;

    let local = |tab: &crate::tabs::TabDescriptor| {
        (tab.kind != TabKind::RemotePr && !tab.repo_root.is_empty()).then(|| tab.repo_root.clone())
    };
    let active = &view.active_entry().or(view.tabs.first())?.descriptor;
    local(active)
        .or_else(|| {
            let (owner, repo) = active.pr_owner.as_ref().zip(active.pr_repo.as_ref())?;
            Some(project_root(&format!("{owner}/{repo}"))).filter(|root| !root.is_empty())
        })
        .or_else(|| view.tabs.iter().find_map(|tab| local(&tab.descriptor)))
}

/// Closing a tab also drops its content on the actor that loaded it.
fn service_close_tab(state: &DesktopState, idx: usize) -> Result<AppSnapshot, String> {
    content_routes::close(state, idx)
}

fn service_select_tab(state: &DesktopState, idx: usize) -> Result<AppSnapshot, String> {
    let key = service_tab_key(state, idx)?;
    service_tab_command(state, er_service::AppCommand::SelectTab { key })
}

fn service_reorder_tabs(
    state: &DesktopState,
    from: usize,
    to: usize,
) -> Result<AppSnapshot, String> {
    let key = service_tab_key(state, from)?;
    service_tab_command(
        state,
        er_service::AppCommand::ReorderTabs { key, to_idx: to },
    )
}

#[tauri::command]
pub fn new_tab(state: State<AppState>) -> Result<AppSnapshot, String> {
    service_new_tab(&state)
}

#[tauri::command]
pub fn close_tab(idx: usize, state: State<AppState>) -> Result<AppSnapshot, String> {
    service_close_tab(&state, idx)
}

#[tauri::command]
pub async fn select_tab(idx: usize, state: State<'_, AppState>) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || service_select_tab(&state, idx)).await
}

#[tauri::command]
#[expect(
    non_snake_case,
    reason = "camelCase parameters match the keys of the JS IPC payload"
)]
pub fn reorder_tabs(
    fromIdx: usize,
    toIdx: usize,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    service_reorder_tabs(&state, fromIdx, toIdx)
}

/// If the active tab was restored as a lazy stub, kick its first
/// `refresh_diff()` to a background thread and return immediately. The
/// caller's snapshot shows the stub with `loading.tab_diff = true`; the
/// loaded diff arrives via the revision-event poll when the worker finishes.
///
/// Running this inline would hold the App lock across the git diff + parse,
/// serializing every other command behind it — seconds, for a large stub tab.
pub fn kick_deferred_tab_refresh(app: &mut App, state: &AppState) {
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
    let ports = state.ports.clone();
    std::thread::spawn(move || {
        // Phase 1 — brief lock: capture the fetch inputs and release, so the
        // network work below runs outside the critical section. Re-resolve the
        // tab by index + repo_root in case tabs changed while this worker was
        // starting.
        let inputs = app_arc.lock().ok().and_then(|app| {
            app.tabs
                .get(idx)
                .filter(|tab| tab.repo_root == expect_root)
                .and_then(|tab| tab.pr_refresh_inputs())
        });

        // Phase 2 — no lock: the head fetch, the gh lookup, and the base fetch.
        // Timed on its own so `lazy_tab_refresh` can say whether a slow load was
        // the network or the local rebuild.
        let mut fetched = None;
        let mut fetch_failed = false;
        let t_fetch = std::time::Instant::now();
        if let Some(inputs) = inputs {
            // A restored stub normally already has its refs on disk, and the
            // fetches would re-download them. Probed here, where no lock is
            // held, so the check costs a `git rev-parse` and nothing else.
            let already_present = er_engine::github::ref_exists_locally(
                &*ports.git,
                &inputs.repo_root,
                &format!("refs/er/pr/{}/head", inputs.pr_number),
            );
            if !already_present {
                match er_engine::app::TabState::fetch_pr_refresh(&*ports.git, &inputs) {
                    Ok(result) => fetched = Some(result),
                    Err(e) => {
                        log::error!("er-desktop: deferred tab pr fetch failed: {e}");
                        fetch_failed = true;
                    }
                }
            }
        }

        let fetch_ms = t_fetch.elapsed().as_millis();

        // Phase 3 — brief lock: apply, then rebuild the diff.
        let t_refresh = std::time::Instant::now();
        if let Ok(mut app) = app_arc.lock() {
            if let Some(tab) = app.tabs.get_mut(idx).filter(|t| t.repo_root == expect_root) {
                // A failed fetch leaves the tab as it was, rather than rebuilding
                // the diff from refs that could not be refreshed.
                if !fetch_failed {
                    match fetched {
                        Some(result) => tab.apply_pr_refresh(result),
                        // Skipped the fetch because the refs are already on disk,
                        // so nothing has recorded the oid this diff is about to be
                        // built against. Without it the staleness probe has an
                        // unknown to compare and stays silent, showing a diff that
                        // is behind the PR head with no pill.
                        None => tab.seed_last_diff_head_oid_from_local_ref(),
                    }
                    if let Err(e) = tab.refresh_diff() {
                        log::error!("er-desktop: deferred tab refresh failed: {e}");
                    }
                }
            }
        }
        let refresh_ms = t_refresh.elapsed().as_millis();
        if let Ok(mut l) = loading.lock() {
            l.tab_diff = false;
        }
        desktop_revision.fetch_add(1, Ordering::Relaxed);
        crate::profile_log::profile_log(
            "lazy_tab_refresh",
            &[
                // `fetch_ms` is the network legs, `refresh_ms` the local rebuild.
                // Without the split a slow load gives no clue which it was.
                ("fetch_ms", fetch_ms.to_string()),
                ("refresh_ms", refresh_ms.to_string()),
                ("ms", (fetch_ms + refresh_ms).to_string()),
            ],
        );
    });
}

/// Background offload for the two-phase PR open.
///
/// Phase 1 (`open_pr_review_impl`) returns a chrome-only snapshot with
/// `bg_loading.tab_diff` set; this worker then performs the deferred work the
/// full snapshot needs — the single authoritative AI reload (or the deferred
/// first diff fetch when the fallback path flagged `needs_initial_refresh`),
/// tab persistence, clearing the loading flag — and bumps `desktop_revision`
/// so the revision-event poll delivers the full snapshot in ~40–120 ms.
/// Mirrors `kick_deferred_tab_refresh`: never holds the app lock during the
/// slow part beyond the re-resolution, and re-checks tab identity.
pub fn kick_post_open_offload(app: &App, state: &AppState) {
    let idx = app.active_tab;
    let tab = app.tab();
    let expect_root = tab.repo_root.clone();
    let expect_pr = tab.pr_number;
    let expect_local_view = tab.local_branch_view.clone();
    if let Ok(mut l) = state.loading.lock() {
        l.tab_diff = true;
    }
    let app_arc = Arc::clone(&state.app);
    let loading = Arc::clone(&state.loading);
    let desktop_revision = Arc::clone(&state.desktop_revision);
    let ports = state.ports.clone();
    std::thread::spawn(move || {
        let t = std::time::Instant::now();

        /// What this worker owes, decided under the first brief lock.
        enum Work {
            /// The tab is gone, or is no longer the one we were asked to load.
            Nothing,
            /// Cache-hit path: the AI reload ran in phase 1 and nothing is left.
            Done,
            /// Fallback path with nothing to fetch: refresh the diff as it stands.
            RefreshOnly,
            /// Fallback path on a local PR tab: fetch these first.
            Fetch(er_engine::app::PrRefreshInputs),
        }

        // Phase 1 — brief lock: decide what to do, and capture the fetch inputs
        // if there are any. The network legs run below with the guard released,
        // because holding it across them blocks every command the user can see.
        let work = if let Ok(mut app) = app_arc.lock() {
            // Re-resolve the tab by index + identity in case tabs changed while
            // the worker waited for the lock.
            match app.tabs.get_mut(idx).filter(|t| {
                t.repo_root == expect_root
                    && t.pr_number == expect_pr
                    && t.local_branch_view == expect_local_view
            }) {
                Some(tab) if tab.needs_initial_refresh => {
                    // Fallback path (no head oid / 0-file PR): the diff was not
                    // loaded synchronously — fetch it here, same as the
                    // deferred-tab-refresh worker.
                    tab.needs_initial_refresh = false;
                    match tab.pr_refresh_inputs() {
                        Some(inputs) => Work::Fetch(inputs),
                        None => Work::RefreshOnly,
                    }
                }
                Some(tab) => {
                    // Common cache-hit path: the diff is already loaded; only
                    // the AI sidecar reload was deferred.
                    tab.reload_ai_state();
                    Work::Done
                }
                None => Work::Nothing,
            }
        } else {
            Work::Nothing
        };

        // Phase 2 — no lock: the PR's network legs, when the fallback path needs them.
        let mut fetch_failed = false;
        let fetched = match &work {
            Work::Fetch(inputs) => {
                match er_engine::app::TabState::fetch_pr_refresh(&*ports.git, inputs) {
                    Ok(result) => Some(result),
                    Err(e) => {
                        log::error!(
                            "er-desktop: post-open offload fetch failed for pr={:?} root={}: {e}",
                            expect_pr,
                            expect_root
                        );
                        fetch_failed = true;
                        None
                    }
                }
            }
            _ => None,
        };

        // Phase 3 — brief lock: apply and rebuild the diff. Skipped when the fetch
        // failed, which leaves the tab as it was rather than rebuilding against
        // refs that could not be refreshed.
        if !matches!(work, Work::Nothing | Work::Done) && !fetch_failed {
            if let Ok(mut app) = app_arc.lock() {
                if let Some(tab) = app.tabs.get_mut(idx).filter(|t| {
                    t.repo_root == expect_root
                        && t.pr_number == expect_pr
                        && t.local_branch_view == expect_local_view
                }) {
                    match fetched {
                        Some(result) => tab.apply_pr_refresh(result),
                        // Rebuild from the local PR ref without fetching, so
                        // record the oid it is built against — otherwise the
                        // staleness probe has an unknown on one side and shows
                        // no pill for a diff that is behind the PR head.
                        None => tab.seed_last_diff_head_oid_from_local_ref(),
                    }
                    if let Err(e) = tab.refresh_diff() {
                        log::error!(
                            "er-desktop: post-open offload failed for pr={:?} root={}: {e}",
                            expect_pr,
                            expect_root
                        );
                    }
                }
            }
        }
        // No persist here: `place_tab` already wrote tabs.json on the open
        // critical path and the reload changes no persisted field
        // (TabDescriptor has no mode/AI state).
        if let Ok(mut l) = loading.lock() {
            l.tab_diff = false;
        }
        desktop_revision.fetch_add(1, Ordering::Relaxed);
        crate::profile_log::profile_log(
            "post_open_offload",
            &[("ms", t.elapsed().as_millis().to_string())],
        );
    });
}

/// Decide whether tab `idx` is a PR tab whose branch-scope diff is worth
/// prefetching in the background. Returns the captured fetch inputs, or `None`.
///
/// Rules:
/// - Requires a PR number (local PR tab or remote tab).
/// - Local PR tabs whose head branch is checked out locally are skipped: the
///   working-tree diff is a local `git` call (fast) and preloading it would
///   risk serving a stale diff after the user edits the working tree.
/// - Remote tabs are eligible — their branch scope is `gh pr diff --repo`, a
///   network call with no cache.
pub fn branch_preload_target(
    app: &App,
    idx: usize,
) -> Option<er_engine::app::BranchScopeFetchInputs> {
    let tab = app.tabs.get(idx)?;
    let pr_number = tab.pr_number?;
    if tab.local_branch_view.is_none() && tab.remote_repo.is_none() {
        return None;
    }
    if tab.local_branch_checkout_root.is_some() {
        return None;
    }
    // A seeded preload (open-time cache raw) still triggers the worker: it
    // skips the fetch but preloads the branch-view AI sidecars so the first
    // Branch click is instant even when the view was collapsed at open.
    Some(er_engine::app::BranchScopeFetchInputs {
        ports: app.ports.clone(),
        repo_root: tab.repo_root.clone(),
        base_branch: tab.base_branch.clone(),
        local_branch_view: tab.local_branch_view.clone(),
        pr_head_ref: tab.pr_head_ref.clone(),
        pr_number: Some(pr_number),
        checkout_root: None,
        remote_repo: tab.remote_repo.clone(),
    })
}

/// Kick a background fetch of the active tab's branch-scope raw diff so the
///
/// first switch to the Branch view consumes the preload (`TabState`'s
/// `preloaded_branch_raw`) instead of running `gh pr diff` / `git diff`
/// synchronously under the App lock.
///
/// Invisible to the frontend: no revision bump, no loading flag. The preload
/// is consumed one-shot by the next branch-scope refresh, and dropped if any
/// input moved (base, PR, branch, checkout, remote). Errors are logged and
/// otherwise ignored; a second kick for the same PR while one is in flight is
/// a no-op.
pub fn kick_branch_preload(app: &App, state: &AppState) {
    let idx = app.active_tab;
    let Some(inputs) = branch_preload_target(app, idx) else {
        return;
    };
    let key = inputs.dedupe_key();
    {
        let Ok(mut g) = state.branch_preload_in_flight.lock() else {
            return;
        };
        if !g.insert(key.clone()) {
            return; // already in flight for this PR
        }
    }
    let app_arc = Arc::clone(&state.app);
    let in_flight = Arc::clone(&state.branch_preload_in_flight);
    std::thread::spawn(move || {
        // The worker decides whether a fetch is needed: a seeded raw (the
        // open-time cache/entry diff) skips the fetch — but the branch-view
        // AI sidecar preload always runs, so the first Branch click is fast
        // even when the view was collapsed at open.
        let seeded_raw = app_arc.lock().ok().and_then(|app| {
            app.tabs
                .get(idx)
                .filter(|t| {
                    t.repo_root == inputs.repo_root
                        && t.pr_number == inputs.pr_number
                        && t.remote_repo == inputs.remote_repo
                        && t.local_branch_view == inputs.local_branch_view
                        && t.local_branch_checkout_root == inputs.checkout_root
                })
                .and_then(|t| {
                    t.preloaded_branch_raw
                        .as_ref()
                        .map(|p| (p.raw.clone(), p.preview_head_oid.clone()))
                })
        });
        let fetch_preview_head = || {
            let slug = inputs.remote_repo.clone().or_else(|| {
                er_engine::github::get_repo_info(&*inputs.ports.git, &inputs.repo_root)
                    .ok()
                    .map(|(owner, repo)| format!("{owner}/{repo}"))
            })?;
            let (owner, repo) = slug.split_once('/')?;
            er_engine::github::gh_pr_head_oid_remote(owner, repo, inputs.pr_number?).ok()
        };
        let (raw, preview_head_oid) = match seeded_raw {
            Some((raw, oid)) => (Some(raw), oid),
            None => {
                let head = fetch_preview_head();
                let raw = match er_engine::app::fetch_branch_scope_raw("branch", &inputs) {
                    Ok(raw) => Some(raw),
                    Err(e) => {
                        log::warn!(
                            "er-desktop: branch preload failed for pr={:?}: {e}",
                            inputs.pr_number
                        );
                        None
                    }
                };
                let confirmed = fetch_preview_head();
                (raw, if head == confirmed { head } else { None })
            }
        };
        // Branch-view AI sidecar preload (local PR tabs — their Branch view is
        // reachable; remote tabs are PrDiff-only and their bucket is the PR
        // bucket, already loaded). Inputs captured under a brief lock, sidecar
        // disk reads run OUTSIDE it, write-back under a fresh lock.
        let ai_preload = raw.as_ref().and_then(|raw| {
            // Brief lock: capture the load inputs only.
            let captured = {
                let app = app_arc.lock().ok()?;
                let tab = app.tabs.get(idx).filter(|t| {
                    t.repo_root == inputs.repo_root
                        && t.pr_number == inputs.pr_number
                        && t.remote_repo == inputs.remote_repo
                        && t.local_branch_view == inputs.local_branch_view
                        && t.local_branch_checkout_root == inputs.checkout_root
                })?;
                tab.local_branch_view.as_ref()?;
                let bucket_dir = tab.branch_bucket_er_dir()?;
                // Skip when an existing slot already targets this bucket (it
                // may be fresher); a slot for a DIFFERENT bucket is a dead
                // letter (branch moved) and will be replaced on write-back.
                if let Some(existing) = &tab.preloaded_branch_ai {
                    if existing.bucket_dir == bucket_dir {
                        return None;
                    }
                }
                Some((bucket_dir, tab.storage_branch_scope().map(str::to_string)))
            };
            let (bucket_dir, scope) = captured?;
            // Outside the lock: pure CPU + sidecar disk reads.
            let hash = er_engine::ai::compute_diff_hash(raw);
            let arbiter_overlay = er_engine::ai::arbiter_overlay_stamp(&bucket_dir);
            let ai = er_engine::ai::load_ai_state_with_arbiter(
                &bucket_dir,
                &hash,
                scope.as_deref(),
                arbiter_overlay.as_deref(),
            );
            Some((bucket_dir, hash, arbiter_overlay, ai))
        });
        if let Some((bucket_dir, hash, arbiter_overlay, ai)) = ai_preload {
            if let Ok(mut app) = app_arc.lock() {
                if let Some(tab) = app.tabs.get_mut(idx).filter(|t| {
                    t.repo_root == inputs.repo_root
                        && t.pr_number == inputs.pr_number
                        && t.remote_repo == inputs.remote_repo
                        && t.local_branch_view == inputs.local_branch_view
                        && t.local_branch_checkout_root == inputs.checkout_root
                }) {
                    // The capture guaranteed no same-bucket slot exists; the
                    // write-back installs only when the slot is absent or is a
                    // dead letter (branch moved between capture and write-back).
                    let can_install = match &tab.preloaded_branch_ai {
                        None => true,
                        Some(existing) => existing.bucket_dir != bucket_dir,
                    };
                    if can_install {
                        tab.preloaded_branch_ai = Some(er_engine::app::BranchAiPreload {
                            bucket_dir,
                            diff_hash: hash,
                            arbiter_overlay,
                            ai,
                        });
                    }
                }
            }
        }
        if let Some(raw) = raw {
            if let Ok(mut app) = app_arc.lock() {
                // Re-resolve the tab by index + identity in case tabs
                // changed while the worker waited for the lock. Remote
                // tabs share repo_root (current dir), so include the
                // distinguishing fields; base_branch/pr_head_ref drift is
                // still caught by consume-side validation (the
                // authoritative gate, tested).
                if let Some(tab) = app.tabs.get_mut(idx).filter(|t| {
                    t.repo_root == inputs.repo_root
                        && t.pr_number == inputs.pr_number
                        && t.remote_repo == inputs.remote_repo
                        && t.local_branch_view == inputs.local_branch_view
                        && t.local_branch_checkout_root == inputs.checkout_root
                }) {
                    if tab.preloaded_branch_raw.is_none() {
                        tab.preloaded_branch_raw = Some(er_engine::app::PreloadedBranchRaw {
                            raw,
                            preview_head_oid: preview_head_oid.clone(),
                            base_branch: inputs.base_branch.clone(),
                            pr_number: inputs.pr_number,
                            local_branch_view: inputs.local_branch_view.clone(),
                            checkout_root: inputs.checkout_root.clone(),
                            remote_repo: inputs.remote_repo.clone(),
                            pr_head_ref: inputs.pr_head_ref.clone(),
                            parity: true,
                        });
                    }
                }
            }
        }
        if let Ok(mut g) = in_flight.lock() {
            g.remove(&key);
        }
    });
}

/// Kick a background fetch of a local PR tab's git refs
///
/// (`refs/er/pr/<n>/head` and a fresh `origin/<base>`) so a tab opened via the
/// fast `enter_pr_diff_preloaded` path — which deliberately skips the two
/// sequential network ref-fetches on the open critical path — ends up with
/// real local refs shortly after placement. The PR Diff view renders from the
/// already-fetched `gh pr diff` text and never needs these refs; they serve
/// local-ref consumers: the Branch-scope staged diff for `committed_unpushed`,
/// History-mode commit diffs, `refetch_and_refresh_diff`, and the TUI.
///
/// Applying the results under a brief lock replicates exactly the state the
/// synchronous first-entry block of `enter_pr_diff_impl` would have set
/// (`pr_head_ref`, `base_branch`, `last_diff_head_oid`, `pr_refs_fetched`), so
/// behavior is identical whether the refs land on the critical path or in the
/// background. Invisible to the frontend until applied: no loading flag; a
/// `desktop_revision` bump on success so the next poll reflects the applied
/// refs. Errors are logged and otherwise ignored — every consumer that needs
/// the refs either fetches on demand (History mode) or re-fetches anyway
/// (sync). Skipped when the refs already exist locally (e.g. an earlier sync
/// materialized them). Deduped via `pr_ref_fetch_in_flight`; a second kick for
/// the same PR while one is in flight is a no-op.
pub fn kick_pr_ref_fetch(app: &App, state: &AppState) {
    let idx = app.active_tab;
    let Some(tab) = app.tabs.get(idx) else {
        return;
    };
    let Some(pr_number) = tab.pr_number else {
        return;
    };
    if tab.is_remote() {
        return; // remote-only tabs have no local clone to fetch into
    }
    if tab.local_branch_checkout_root.is_some() {
        return; // working-tree views; the local PR refs are not on the diff path
    }
    let repo_root = tab.repo_root.clone();
    let base_branch = tab.base_branch.clone();
    let head_ref = format!("refs/er/pr/{}/head", pr_number);
    let key = (repo_root.clone(), pr_number);
    {
        let Ok(mut g) = state.pr_ref_fetch_in_flight.lock() else {
            return;
        };
        if !g.insert(key.clone()) {
            return; // already in flight for this PR
        }
    }
    let app_arc = Arc::clone(&state.app);
    let in_flight = Arc::clone(&state.pr_ref_fetch_in_flight);
    let desktop_rev = Arc::clone(&state.desktop_revision);
    let ports = state.ports.clone();
    std::thread::spawn(move || {
        let t = std::time::Instant::now();
        // Probed here rather than on the caller's thread. The in-flight set
        // above dedupes by (repo_root, pr_number), so this `git rev-parse`
        // costs one subprocess per unique PR instead of one per open, and the
        // open path stops paying for it at all.
        if er_engine::github::ref_exists_locally(&*ports.git, &repo_root, &head_ref) {
            if let Ok(mut g) = in_flight.lock() {
                g.remove(&key);
            }
            return; // refs already materialized (e.g. by an earlier sync)
        }
        // Independent fetches — run them in parallel like the open path does.
        let (head_res, base_res) = std::thread::scope(|s| {
            let head_root = repo_root.clone();
            let base_root = repo_root.clone();
            let base_branch = base_branch.clone();
            let head_git = ports.git.clone();
            let head_h = s.spawn(move || {
                er_engine::github::fetch_pr_head(&*head_git, pr_number, &head_root)
                    .map_err(|e| e.to_string())
            });
            let base_git = ports.git.clone();
            let base_h = s.spawn(move || {
                er_engine::github::fetch_base_branch_ref(
                    &*base_git,
                    &base_root,
                    base_branch.trim_start_matches("origin/"),
                )
                .map_err(|e| e.to_string())
            });
            let head_res = head_h
                .join()
                .unwrap_or_else(|_| Err("pr head fetch thread panicked".to_string()));
            let base_res = base_h
                .join()
                .unwrap_or_else(|_| Err("base ref fetch thread panicked".to_string()));
            (head_res, base_res)
        });
        match (head_res, base_res) {
            (Ok(head_ref), Ok(base_ref)) => {
                if let Ok(mut app) = app_arc.lock() {
                    // Re-resolve the tab by index + identity in case tabs
                    // changed while the worker waited for the lock.
                    if let Some(tab) = app
                        .tabs
                        .get_mut(idx)
                        .filter(|t| t.repo_root == repo_root && t.pr_number == Some(pr_number))
                    {
                        tab.pr_head_ref = Some(head_ref);
                        tab.base_branch = base_ref;
                        // Deliberately NOT updating `last_diff_head_oid`: the
                        // two-phase open serves the open-time diff, and this
                        // background fetch would otherwise suppress the stale
                        // pill while the displayed diff is at an older head.
                        // Manual Sync realigns it.
                        tab.pr_refs_fetched = true;
                        desktop_rev.fetch_add(1, Ordering::Relaxed);
                    }
                }
                log::info!(
                    "pr_ref_fetch repo={} pr={} ok ms={}",
                    repo_root,
                    pr_number,
                    t.elapsed().as_millis()
                );
            }
            (Err(e), _) | (_, Err(e)) => log::warn!(
                "pr_ref_fetch repo={} pr={} failed ms={} err={}",
                repo_root,
                pr_number,
                t.elapsed().as_millis(),
                e
            ),
        }
        if let Ok(mut g) = in_flight.lock() {
            g.remove(&key);
        }
    });
}

#[tauri::command]
#[expect(
    clippy::too_many_arguments,
    reason = "the parameters are the JS IPC payload; a params struct would change the command contract"
)]
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
    let update = crate::view_state::BrowserUpdate {
        layout,
        url,
        annotate,
        tooltips,
        split_ratio,
    };
    let navigate = navigate.unwrap_or(false);
    {
        let _ = &browser_state;
        update_tab_browser_routed(&state, &app_handle, update, navigate)
    }
}

/// The flag-on body of `update_tab_browser`: the registry's active tab, as
/// `er_send` writes it.
pub(crate) fn update_tab_browser_routed(
    state: &AppState,
    webviews: &dyn crate::browser_webview::TabWebviews,
    update: crate::view_state::BrowserUpdate,
    navigate: bool,
) -> Result<AppSnapshot, String> {
    crate::resources::tabs::update_browser(state, webviews, update, navigate)
        .map_err(|e| e.message)?;
    Ok(AppSnapshot::default())
}

#[tauri::command]
pub fn cycle_tab_browser_layout(
    app_handle: tauri::AppHandle,
    browser_state: State<'_, crate::browser_webview::BrowserWebviewState>,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    {
        let _ = &browser_state;
        cycle_tab_browser_layout_routed(&state, &app_handle)
    }
}

/// The flag-on body of `cycle_tab_browser_layout`, as `er_send` runs it.
pub(crate) fn cycle_tab_browser_layout_routed(
    state: &AppState,
    webviews: &dyn crate::browser_webview::TabWebviews,
) -> Result<AppSnapshot, String> {
    crate::resources::tabs::cycle_browser_layout(state, webviews).map_err(|e| e.message)?;
    Ok(AppSnapshot::default())
}

// ── UI annotations (browser view) ───────────────────────────────────────────

#[tauri::command]
#[expect(
    non_snake_case,
    clippy::too_many_arguments,
    reason = "the parameters, camelCase keys included, are the JS IPC payload"
)]
pub async fn add_ui_annotation(
    url: String,
    selector: Option<String>,
    bbox: [f64; 4],
    viewport: [u32; 2],
    text: String,
    screenshotDataUrl: Option<String>,
    elementContext: Option<String>,
    domContext: Option<serde_json::Value>,
    id: Option<String>,
    view: Option<crate::snapshot::OptimisticView>,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let app = state.app.lock().map_err(|e| e.to_string())?;
        if let Some(early) = abort_wrong_view(&app, &state, view.as_ref()) {
            return early;
        }
        let new = NewUiAnnotation {
            url,
            selector,
            bbox,
            viewport,
            text,
            screenshot_data_url: screenshotDataUrl,
            element_context: elementContext,
            dom_context: domContext,
            id,
        };
        write_ui_annotation(&app.tab().comments_dir(), new)?;
        state.ui_annotations.bump();
        state.desktop_revision.fetch_add(1, Ordering::Relaxed);
        Ok(snap_from(&app, &state))
    })
    .await
}

/// A browser annotation as the page submits it.
pub(crate) struct NewUiAnnotation {
    pub url: String,
    pub selector: Option<String>,
    pub bbox: [f64; 4],
    pub viewport: [u32; 2],
    pub text: String,
    pub screenshot_data_url: Option<String>,
    pub element_context: Option<String>,
    pub dom_context: Option<serde_json::Value>,
    pub id: Option<String>,
}

/// Adds `new` to the annotations file in `dir`, its screenshot beside it.
/// Answers the row's id.
pub(crate) fn write_ui_annotation(dir: &str, new: NewUiAnnotation) -> Result<String, String> {
    let NewUiAnnotation {
        url,
        selector,
        bbox,
        viewport,
        text,
        screenshot_data_url,
        element_context,
        dom_context,
        id,
    } = new;
    let mut anns = er_engine::ai::load_ui_annotations(dir);
    let id = id.filter(|s| !s.is_empty()).unwrap_or_else(|| {
        format!(
            "ui-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        )
    });
    if !id.starts_with("ui-")
        || id.contains(['/', '\\'])
        || id.contains("..")
        || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err("invalid annotation id".to_string());
    }

    // If a screenshot data URL was provided, decode and persist it under
    // `<dir>/screenshots/<id>.png`. Failure to decode is non-fatal:
    // the annotation is still saved without a screenshot path.
    let screenshot_path = match screenshot_data_url.as_deref() {
        Some(data_url) => decode_data_url_png(data_url)
            .and_then(|bytes| save_screenshot_bytes(dir, &id, &bytes).ok()),
        None => None,
    };

    let ts = chrono_like_timestamp();
    anns.push(er_engine::ai::UiAnnotation {
        id: id.clone(),
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
        element_context,
        dom_context,
    });
    er_engine::ai::save_ui_annotations(dir, &anns).map_err(|e| e.to_string())?;
    Ok(id)
}

pub(crate) fn delete_ui_annotation_in(dir: &str, id: &str) -> Result<(), String> {
    let mut anns = er_engine::ai::load_ui_annotations(dir);
    anns.retain(|a| a.id != id);
    er_engine::ai::save_ui_annotations(dir, &anns).map_err(|e| e.to_string())
}

/// Removes the annotations on `page_url`, or every one with none, and their
/// screenshots.
pub(crate) fn clear_ui_annotations_in(dir: &str, page_url: Option<&str>) -> Result<(), String> {
    let anns = er_engine::ai::load_ui_annotations(dir);
    let (removed, kept): (Vec<_>, Vec<_>) = anns
        .into_iter()
        .partition(|a| page_url.is_none_or(|url| a.url == url));
    for path in removed.iter().filter_map(|a| a.screenshot_path.as_deref()) {
        let _ = std::fs::remove_file(path);
    }
    er_engine::ai::save_ui_annotations(dir, &kept).map_err(|e| e.to_string())
}

/// Saves `bytes` as `id`'s screenshot in `dir` and points its row at it.
/// Answers the path, and whether a row took it.
pub(crate) fn save_annotation_screenshot_in(
    dir: &str,
    id: &str,
    bytes: &[u8],
) -> Result<(String, bool), String> {
    let path = save_screenshot_bytes(dir, id, bytes).map_err(|e| e.to_string())?;
    let mut anns = er_engine::ai::load_ui_annotations(dir);
    let Some(a) = anns.iter_mut().find(|a| a.id == id) else {
        return Ok((path, false));
    };
    a.screenshot_path = Some(path.clone());
    er_engine::ai::save_ui_annotations(dir, &anns).map_err(|e| e.to_string())?;
    Ok((path, true))
}

/// Decode a `data:image/png;base64,<payload>` URL into raw PNG bytes. Returns
/// `None` if the prefix is missing or base64 is malformed. We accept any
/// `data:image/*;base64,` MIME — the caller is trusted to produce PNG.
pub(crate) fn decode_data_url_png(data_url: &str) -> Option<Vec<u8>> {
    let (header, payload) = data_url.split_once(',')?;
    if !header.starts_with("data:image/") || !header.ends_with(";base64") {
        return None;
    }
    base64_decode(payload).ok()
}

/// Minimal standard-base64 decoder (RFC 4648). Skips whitespace, requires the
/// canonical alphabet. Avoids adding a base64 crate dep just for screenshots.
fn base64_decode(input: &str) -> Result<Vec<u8>, &'static str> {
    const fn val(b: u8) -> Result<u8, &'static str> {
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
        let v = u32::from(val(b)?);
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
    let final_path = format!("{screenshots_dir}/{id}.png");
    er_store::atomic::write_atomic(std::path::Path::new(&final_path), bytes)?;
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
    let (path, patched) =
        save_annotation_screenshot_in(&app.tab().comments_dir(), &id, &png_bytes)?;
    // The row's `screenshot_path` now names it, so the UI picks it up.
    if patched {
        state.ui_annotations.bump();
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
pub async fn delete_ui_annotation(
    id: String,
    view: Option<crate::snapshot::OptimisticView>,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let app = state.app.lock().map_err(|e| e.to_string())?;
        if let Some(early) = abort_wrong_view(&app, &state, view.as_ref()) {
            return early;
        }
        delete_ui_annotation_in(&app.tab().comments_dir(), &id)?;
        state.ui_annotations.bump();
        state.desktop_revision.fetch_add(1, Ordering::Relaxed);
        Ok(snap_from(&app, &state))
    })
    .await
}

#[tauri::command]
pub fn clear_ui_annotations_for_page(
    page_url: String,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let app = state.app.lock().map_err(|e| e.to_string())?;
    clear_ui_annotations_in(&app.tab().comments_dir(), Some(&page_url))?;
    state.ui_annotations.bump();
    state.desktop_revision.fetch_add(1, Ordering::Relaxed);
    Ok(snap_from(&app, &state))
}

#[tauri::command]
pub fn clear_ui_annotations(state: State<AppState>) -> Result<AppSnapshot, String> {
    let app = state.app.lock().map_err(|e| e.to_string())?;
    clear_ui_annotations_in(&app.tab().comments_dir(), None)?;
    state.ui_annotations.bump();
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
pub fn apply_anchor_updates(dir: &str, updates: &[AnchorUpdate]) -> std::io::Result<()> {
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
    state.ui_annotations.bump();
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
    let lock_wait_ms = lock_t0.elapsed().as_millis();

    // Pass two: re-take for the revisions and the snapshot, so the two agree
    // with each other. Nothing between here and the snapshot releases it.
    let app = state.app.lock().map_err(|e| e.to_string())?;
    let desktop_rev = state.desktop_revision.load(Ordering::Relaxed);
    let overlay = other_bucket_arbiter(&app);
    let content_revision = with_service_view(
        state,
        &app,
        compute_content_revision_with_arbiter(&app, overlay.as_deref()),
    );
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
        snap_from_with_arbiter(&app, state, overlay.as_deref())
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
                v.is_draft.hash(&mut h);
                v.is_cross_repository.hash(&mut h);
                v.merge_state_status.hash(&mut h);
                v.head_oid.hash(&mut h);
                v.auto_merge_method.hash(&mut h);
                v.head_branch_exists.hash(&mut h);
                v.repo_merge.hash(&mut h);
                v.in_merge_queue.hash(&mut h);
                v.base_has_merge_queue.hash(&mut h);
                v.can_bypass_rules.hash(&mut h);
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

/// On the service path the active tab's workspace view rides on the content
/// revision, so a view its actor publishes rebuilds the content. The relay
/// bumps the desktop revision too, which wakes the poll but moves only the
/// chrome revision. Every writer of `last_sent_content_revision` goes through
/// here, or a confirmed write's follow-up poll would resend the content.
fn with_service_view(state: &AppState, app: &App, content: u64) -> u64 {
    use std::hash::{Hash, Hasher};
    let key = app.tab().identity().workspace_key();
    let Some(view) = state.service.published_view(&key) else {
        return content;
    };
    let mut h = std::collections::hash_map::DefaultHasher::new();
    content.hash(&mut h);
    view.revision.hash(&mut h);
    h.finish()
}

fn compute_content_revision(app: &App) -> u64 {
    let overlay = other_bucket_arbiter(app);
    compute_content_revision_with_arbiter(app, overlay.as_deref())
}

fn other_bucket_arbiter(app: &App) -> Option<Vec<u8>> {
    crate::snapshot::other_bucket_dir(app.tab())
        .and_then(|(_, dir)| er_engine::ai::arbiter_overlay_stamp(&dir))
}

fn compute_content_revision_with_arbiter(app: &App, other_overlay: Option<&[u8]>) -> u64 {
    let tab = app.tab();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    app.active_tab.hash(&mut h);
    let review_bucket_er_dir = tab.er_dir();
    review_bucket_er_dir.hash(&mut h);
    // Hash the bytes used by the loaded AI state. Arbiter metadata can change
    // without changing the verdict, rationale, confidence, or finding count.
    tab.last_arbiter_overlay.hash(&mut h);
    if crate::snapshot::other_bucket_dir(tab).is_some() {
        other_overlay.hash(&mut h);
    }
    app.review_flow_revision().hash(&mut h);
    if let Some(flow) = app.review_flow_for(&review_bucket_er_dir) {
        format!("{:?}", flow.state).hash(&mut h);
        format!("{:?}", flow.depth).hash(&mut h);
        flow.diff_hash.hash(&mut h);
        flow.lenses.hash(&mut h);
        flow.pending_finders.hash(&mut h);
        flow.failed_finders.hash(&mut h);
        flow.arena_run_id.hash(&mut h);
        flow.successful_finder_providers.hash(&mut h);
        flow.successful_arbiter_provider.hash(&mut h);
        flow.providers_used.hash(&mut h);
        flow.estimated_cost.map(f64::to_bits).hash(&mut h);
        flow.estimated_latency_seconds
            .map(f64::to_bits)
            .hash(&mut h);
        app.review_flow_is_draining(&review_bucket_er_dir)
            .hash(&mut h);
    } else {
        false.hash(&mut h);
    }
    tab.preview_context_key().hash(&mut h);
    for (_, file) in tab.visible_files() {
        tab.file_preview_key(&file.path).hash(&mut h);
    }
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
    // The checklist moves no other hash input: a toggle rewrites one file, and
    // a checklist that appears or is regenerated changes the item count. Drop
    // this and the card shows the state from the last poll forever.
    if let Some(checklist) = &tab.ai.checklist {
        checklist.items.len().hash(&mut h);
        checklist
            .items
            .iter()
            .filter(|i| i.checked)
            .count()
            .hash(&mut h);
    }
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
            last.resolved.hash(&mut h);
            last.outdated.hash(&mut h);
            last.stale.hash(&mut h);
            last.anchor_status.hash(&mut h);
        }
        gc.comments
            .iter()
            .filter(|c| c.stale || c.outdated)
            .count()
            .hash(&mut h);
    }
    if let Some(review) = &tab.ai.review {
        review.diff_hash.hash(&mut h);
        review.files.len().hash(&mut h);
        // Per-finding state that moves no file count. Resolving a finding or the
        // arbiter dropping one leaves `files.len()` alone, so without these the
        // row would never reach the UI.
        let mut total = 0usize;
        let mut resolved = 0usize;
        let mut dropped = 0usize;
        for fr in review.files.values() {
            total += fr.findings.len();
            resolved += fr.findings.iter().filter(|f| f.resolved).count();
            dropped += fr
                .findings
                .iter()
                .filter(|f| matches!(f.confidence, er_engine::ai::Confidence::Dropped))
                .count();
        }
        total.hash(&mut h);
        resolved.hash(&mut h);
        dropped.hash(&mut h);
        let mut grades: Vec<_> = review
            .files
            .iter()
            .flat_map(|(path, file)| {
                file.findings.iter().map(move |finding| {
                    (path.as_str(), finding.id.as_str(), finding.confidence as u8)
                })
            })
            .collect();
        grades.sort_unstable();
        grades.hash(&mut h);
    }
    // A verdict can change without changing any finding counts or confidence.
    // Sort the map so equivalent overlays always produce the same revision.
    let mut verdicts: Vec<_> = tab.ai.arbiter_verdicts.iter().collect();
    verdicts.sort_unstable_by_key(|(key, _)| *key);
    verdicts.hash(&mut h);
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

/// Return the recent log entries for a specific background task.
/// Returns an empty list if the task is not found (may have been reaped).
#[tauri::command]
pub fn get_background_task_log(
    task_id: String,
    state: State<AppState>,
) -> Result<Vec<AgentLogSnapshot>, String> {
    Ok(state
        .desktop
        .tasks
        .log(&task_id)
        .into_iter()
        .map(|line| AgentLogSnapshot {
            command_name: line.command_name,
            source: line.source,
            text: line.text,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use er_engine::ai::{load_ui_annotations, save_ui_annotations, UiAnnotation};

    mod ai_ask_tests;
    mod annotation_write_tests;
    mod annotations_resource_tests;
    mod arena_resource_tests;
    mod arena_wire_tests;
    mod ask_context_tests;
    mod content_route_tests;
    mod diff_command_tests;
    mod diff_resource_tests;
    mod direct_tests;
    mod finding_write_tests;
    mod flow_resource_tests;
    mod flow_wire_tests;
    mod github_sender_tests;
    mod hunks_tests;
    mod pr_resource_tests;
    mod preview_tests;
    mod resource_tests;
    mod reviewed_resource_tests;
    mod session_resource_tests;
    mod settings_resource_tests;
    mod sidebar_inbox_tests;
    mod sidebar_pr_tests;
    mod sidebar_tests;
    mod sync_command_tests;
    mod tab_key_tests;
    mod tab_load_tests;
    mod tab_resource_tests;
    mod task_tests;
    mod ui_annotation_write_tests;
    mod watcher_tests;
    mod workspace_key_tests;

    #[test]
    fn an_available_branch_error_names_the_command_once() {
        let fake = er_git::FakeGit::default().with_for_each_ref(Err(er_git::GitError::Failed {
            context: "git for-each-ref failed".to_string(),
            stderr: "fatal: not a git repository".to_string(),
        }));

        let text = available_branches(&fake, "/nonexistent-repo", &[])
            .expect_err("a failed for-each-ref must surface");

        assert_eq!(text.matches("git for-each-ref failed").count(), 1, "{text}");
        assert!(text.contains("fatal: not a git repository"), "{text}");
    }

    #[test]
    fn card_action_prompt_picks_the_kind_and_action() {
        use er_engine::ai::{AI_ACTION_ASK, AI_ACTION_ELABORATE};
        assert_eq!(
            card_action_prompt("q-1", AI_ACTION_ELABORATE),
            Some(ELABORATE_QUESTION_AI_PROMPT)
        );
        assert_eq!(
            card_action_prompt("n-1", AI_ACTION_ELABORATE),
            Some(ELABORATE_NOTE_AI_PROMPT)
        );
        assert_eq!(
            card_action_prompt("q-1", AI_ACTION_ASK),
            Some(ANSWER_QUESTION_AI_PROMPT)
        );
        assert_eq!(
            card_action_prompt("n-1", AI_ACTION_ASK),
            Some(ANSWER_NOTE_AI_PROMPT)
        );
        // GitHub comments are shared, so the one-click private actions skip them.
        assert_eq!(card_action_prompt("c-1", AI_ACTION_ELABORATE), None);
        assert_eq!(card_action_prompt("q-1", "unknown"), None);
    }

    fn send_note_params<'a>(
        thread_body: &'a str,
        line_content: &'a str,
    ) -> SendNotePromptParams<'a> {
        SendNotePromptParams {
            repo_root: "/repo",
            base_branch: "main",
            current_branch: "feat/x",
            file: ".github/workflows/ci.yml",
            line_start: Some(8),
            line_end: Some(9),
            line_content,
            thread_body,
        }
    }

    #[test]
    fn send_note_prompt_carries_the_note_its_replies_and_the_location() {
        let body = "pin python\n\n**Claude** replied:\nAdd actions/setup-python@v5";
        let prompt = build_send_note_prompt(&send_note_params(body, "  - run: python x.py"));
        assert!(prompt.contains("Location: .github/workflows/ci.yml, L8–9"));
        assert!(prompt.contains("Branch: feat/x (compared against main)"));
        assert!(prompt.contains("pin python"));
        assert!(prompt.contains("Add actions/setup-python@v5"));
        assert!(prompt.contains("```\n  - run: python x.py\n```"));
        // The reviewer commits; the agent only edits the working tree.
        assert!(prompt.contains("Do not commit, push"));
        assert!(prompt.contains("change nothing and say why"));
    }

    #[test]
    fn send_note_prompt_handles_file_level_notes_without_an_anchor() {
        let mut p = send_note_params("tidy this file", "   ");
        p.line_start = None;
        p.line_end = None;
        let prompt = build_send_note_prompt(&p);
        assert!(prompt.contains("Location: .github/workflows/ci.yml (the file as a whole)"));
        assert!(!prompt.contains("anchored line"));
    }

    #[test]
    fn note_range_label_matches_the_card() {
        assert_eq!(note_range_label(Some(8), Some(9)).as_deref(), Some("L8–9"));
        assert_eq!(note_range_label(Some(8), Some(8)).as_deref(), Some("L8"));
        assert_eq!(note_range_label(Some(8), None).as_deref(), Some("L8"));
        assert_eq!(note_range_label(None, Some(3)), None);
    }

    #[test]
    fn send_note_needs_the_viewed_branch_checked_out_here() {
        assert_eq!(send_note_blocked(false, Some("/repo"), "/repo"), Ok(()));
        assert!(send_note_blocked(true, Some("/repo"), "/repo")
            .unwrap_err()
            .contains("remote PR"));
        // A PR or branch view is only a working tree once its head is checked out.
        assert!(send_note_blocked(false, None, "/repo")
            .unwrap_err()
            .contains("not checked out"));
        // Checked out in another worktree: the command would run in the wrong one.
        assert!(send_note_blocked(false, Some("/repo-wt"), "/repo")
            .unwrap_err()
            .contains("/repo-wt"));
    }

    #[test]
    fn elaborate_prompts_rewrite_rather_than_answer() {
        for prompt in [ELABORATE_QUESTION_AI_PROMPT, ELABORATE_NOTE_AI_PROMPT] {
            assert!(prompt.starts_with("Rewrite"));
            assert!(prompt.contains("Reply with only"));
        }
        assert!(ELABORATE_QUESTION_AI_PROMPT.contains("Do not answer"));
        assert!(ELABORATE_NOTE_AI_PROMPT.contains("Do not edit any file"));
    }

    struct ReviewFlowTestDir(tempfile::TempDir);

    impl ReviewFlowTestDir {
        fn new() -> Self {
            Self(tempfile::tempdir().expect("review flow test dir"))
        }

        fn path(&self) -> &std::path::Path {
            self.0.path()
        }
    }

    fn git_fixture_command(repo: &std::path::Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn review_flow_test_state(dir: &ReviewFlowTestDir) -> DesktopState {
        use std::sync::{Arc, Mutex};

        let desktop_revision = Arc::new(AtomicU64::new(0));
        let meta_cache = Arc::new(Mutex::new(HashMap::new()));
        let pr_cache = Arc::new(Mutex::new(HashMap::new()));
        let gh_user = Arc::new(Mutex::new(None));
        let pr_cache_fetched_at = Arc::new(Mutex::new(HashMap::new()));
        let inbox = Arc::new(Mutex::new(crate::inbox::InboxState::default()));
        let app_owner = Arc::new(crate::app_owner::AppOwner::new(
            Arc::clone(&meta_cache),
            Arc::clone(&pr_cache),
            Arc::clone(&pr_cache_fetched_at),
            Arc::clone(&gh_user),
            Arc::clone(&inbox),
        ));
        let gh_status_cache = Arc::new(Mutex::new(HashMap::new()));
        let watch_status = Arc::new(Mutex::new(crate::snapshot::WatchStatusSnapshot::default()));
        let desktop = DesktopState {
            ports: er_engine::ports::Ports::cli(),
            service: Arc::new(crate::service::ServiceState::with_settings_path(
                er_engine::ports::Ports::for_test(),
                dir.path().join("settings").join("config.toml"),
            )),
            view: Default::default(),
            pr_cache,
            pr_cache_fetched_at,
            branch_base_remote_oid: Arc::new(Mutex::new(HashMap::new())),
            app_owner,
            meta_cache,
            gh_user,
            terminals: Arc::new(Mutex::new(HashMap::new())),
            pending_ai_replies: Default::default(),
            ui_annotations: Default::default(),
            ui_annotations_io: Arc::default(),
            gh_status_cache,
            loading: Arc::new(Mutex::new(crate::snapshot::LoadingFlags::default())),
            pr_open_cache: Arc::new(Mutex::new(HashMap::new())),
            pr_open_prefetch_in_flight: Arc::new(Mutex::new(HashMap::new())),
            branch_preload_in_flight: Arc::new(Mutex::new(HashSet::new())),
            pr_ref_fetch_in_flight: Arc::new(Mutex::new(HashSet::new())),
            remote_pr_open_cache: Arc::new(Mutex::new(HashMap::new())),
            remote_pr_open_in_flight: Arc::new(Mutex::new(HashSet::new())),
            gh_status_in_flight: Arc::new(Mutex::new(HashSet::new())),
            gh_status_fetched: Arc::default(),
            desktop_revision,
            watch_status,
            inbox,
            tauri_app_handle: Arc::new(Mutex::new(None)),
            auto_triage_in_flight: Arc::new(Mutex::new(HashSet::new())),
            line_commits_cache: Arc::default(),
            session: Default::default(),
            live_jobs: Default::default(),
            live_tabs: Default::default(),
            pr_list_clock: Default::default(),
            status_fetch_clock: Default::default(),
            tasks: Default::default(),
        };
        crate::app_owner::startup::read_inbox_prefs_from_settings(&desktop);
        desktop
    }

    /// Both prefetch commands go to the app registry, which warms its caches
    /// through the service's ports.
    #[test]
    fn the_prefetch_commands_route_with_the_feature_on() {
        use er_engine::github::FakeGitHub;
        use std::time::{Duration, Instant};

        let wait_for = |ready: &dyn Fn() -> bool| {
            let deadline = Instant::now() + Duration::from_secs(5);
            while !ready() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            ready()
        };
        let dir = ReviewFlowTestDir::new();
        let Ok(tmp) = tempfile::tempdir() else {
            panic!("temp dir");
        };
        er_engine::storage::with_process_test_storage_root(tmp.path(), || {
            let hint = PrOpenHint {
                base_ref: "main".to_string(),
                head_ref: "feature".to_string(),
                head_oid: "head-1".to_string(),
                updated_at: "2026-11-15T00:00:00Z".to_string(),
                title: "T".to_string(),
                author: "octo".to_string(),
            };
            let github = Arc::new(
                FakeGitHub::default()
                    .with_pr_diff(Ok("diff --git a/y b/y\n".to_string()))
                    .with_pr_commits(Ok(Vec::new()))
                    .with_pr_head_oid_remote(Ok("head-1".to_string()))
                    .with_overview_remote(Ok(Some(er_engine::github::PrOverviewData {
                        number: 7,
                        title: hint.title.clone(),
                        state: "OPEN".to_string(),
                        author: hint.author.clone(),
                        base_branch: hint.base_ref.clone(),
                        head_branch: hint.head_ref.clone(),
                        url: String::new(),
                        body: String::new(),
                        checks: Vec::new(),
                        reviewers: Vec::new(),
                    })))
                    .with_pr_diff_remote(Ok("diff --git a/x b/x\n".to_string()))
                    .with_commits_remote(Ok(Vec::new())),
            );
            let ports = er_engine::ports::Ports {
                git: Arc::new(
                    er_git::FakeGit::default().with_rev_parse_verify(Ok(Some("base".to_string()))),
                ),
                github: github.clone(),
            };
            let mut state = review_flow_test_state(&dir);
            state.service = Arc::new(crate::service::ServiceState::new(ports));
            let project_id = projects::ensure_remote_project("acme/widgets").expect("project");
            let repo_root = projects::load()
                .projects
                .iter()
                .find(|p| p.id == project_id)
                .map(|p| p.root_path.clone())
                .expect("the project is saved");

            prefetch_remote_pr_open_impl("acme".into(), "widgets".into(), 7, &state)
                .expect("remote prefetch");
            prefetch_pr_open_impl(project_id.clone(), 7, hint, &state).expect("local prefetch");

            let registry = state.service.app_registry();
            let key = er_engine::pr_open_entry::PrOpenCacheKey {
                project_id: project_id.clone(),
                repo_root,
                pr_number: 7,
            };
            assert!(
                wait_for(&|| registry
                    .remote_pr_open_entry("acme", "widgets", 7)
                    .is_some()),
                "the remote prefetch did not reach the registry"
            );
            // Wait for the registry's save too, so it lands under this root.
            let saved = || {
                er_engine::pr_open_cache::load_persisted_pr_open_cache()
                    .ok()
                    .flatten()
                    .is_some_and(|map| map.contains_key(&key))
            };
            assert!(
                wait_for(&saved),
                "the local prefetch did not reach the registry"
            );
            assert!(registry.pr_open_entry(&key).is_some());
            let calls = github.calls();
            for sent in ["pr_diff_remote", "overview_remote", "pr_diff", "pr_commits"] {
                assert!(
                    calls
                        .iter()
                        .any(|call| call.split('(').next() == Some(sent)),
                    "{sent} did not go through the port: {calls:?}"
                );
            }
            assert!(state.remote_pr_open_cache.lock().unwrap().is_empty());
            assert!(state.pr_open_cache.lock().unwrap().is_empty());
        });
    }

    /// The open path resolves the project and sends to the actor.
    #[test]
    fn the_service_open_path_opens_through_the_actor() {
        let dir = ReviewFlowTestDir::new();
        let Ok(tmp) = tempfile::tempdir() else {
            panic!("temp dir");
        };
        er_engine::storage::with_process_test_storage_root(tmp.path(), || {
            let diff = "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-a\n+b\n";
            let github = Arc::new(
                er_engine::github::FakeGitHub::default()
                    .with_pr_state(Ok("OPEN".to_string()))
                    .with_pr_diff_remote(Ok(diff.to_string())),
            );
            let github_port: Arc<dyn er_engine::github::GitHub> = github.clone();
            let mut state = review_flow_test_state(&dir);
            state.service = Arc::new(crate::service::ServiceState::new(
                er_engine::ports::Ports::with_github(github_port),
            ));
            let result = service_open_pr_url(
                &state,
                "https://github.com/acme/widgets/pull/7".to_string(),
                None,
            );
            assert!(result.is_ok(), "the service open path failed: {result:?}");
            // The open resolves the target, then loads the new tab's diff and
            // fills its PR section, which no prefetch warmed.
            assert_eq!(
                github.calls(),
                vec![
                    "pr_state(owner=\"acme\", repo=\"widgets\", number=7)",
                    "pr_head_oid_remote(owner=\"acme\", repo=\"widgets\", pr=7)",
                    "pr_diff_remote(owner=\"acme\", repo=\"widgets\", number=7)",
                    "pr_head_oid_remote(owner=\"acme\", repo=\"widgets\", pr=7)",
                    "overview_remote(owner=\"acme\", repo=\"widgets\", number=7)",
                    "commits_remote(owner=\"acme\", repo=\"widgets\", number=7, limit=250)",
                ]
            );
        });
    }

    /// Runs `body` on another thread. A body that blocks trips the deadline
    /// instead of hanging the suite.
    fn on_a_thread<T: Send + 'static>(
        state: &DesktopState,
        what: &str,
        body: impl FnOnce(&DesktopState) -> T + Send + 'static,
    ) -> T {
        use std::time::Duration;

        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let calling_state = state.clone();
        std::thread::spawn(move || {
            let _ = done_tx.send(body(&calling_state));
        });
        let Ok(result) = done_rx.recv_timeout(Duration::from_secs(10)) else {
            panic!("{what} did not return");
        };
        result
    }

    /// An App with no repo, for the follower until it is deleted.
    pub(super) fn app_stub() -> Arc<Mutex<App>> {
        Arc::new(Mutex::new(App::new_empty(
            er_engine::ports::Ports::for_test(),
        )))
    }

    /// The registry's active tab.
    pub(super) fn active_key(state: &DesktopState) -> er_engine::TabKey {
        let active = state.service.app_registry().tabs_view().active;
        active.unwrap_or_else(|| panic!("no active tab"))
    }

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

    fn with_fake_claude<R>(value: &str, f: impl FnOnce() -> R) -> R {
        er_engine::test_support::with_var("ER_FAKE_CLAUDE", value, f)
    }

    #[test]
    fn resolve_review_scope_accepts_pr_diff_mode() {
        let mode = DiffMode::PrDiff;
        assert_eq!(resolve_review_scope_in("branch", mode).unwrap(), "branch");
        assert_eq!(resolve_review_scope_in("current", mode).unwrap(), "branch");
        assert_eq!(resolve_review_scope_in("pr", mode).unwrap(), "branch");
    }

    #[test]
    fn resolve_review_scope_accepts_tour_mode() {
        // Guide (tour) reviews the branch diff it was regrouped from.
        let mode = DiffMode::Tour;
        assert_eq!(resolve_review_scope_in("branch", mode).unwrap(), "branch");
        assert_eq!(resolve_review_scope_in("current", mode).unwrap(), "branch");
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
    fn card_ai_subprocess_honors_fake_sentinel_ok() {
        let inv = CardAiInvocation {
            command: "claude".into(),
            args: vec![],
            work_dir: "/tmp".into(),
            is_claude_compatible: true,
            uses_stream_json: false,
            env: vec![],
            // These go through ER_FAKE_CLAUDE and never reach the spawn.
            timeout: std::time::Duration::from_mins(15),
            slot_cap: 3,
            slot_ceiling: 6,
        };
        let body = with_fake_claude("ok", || {
            run_card_ai_subprocess(&inv, "ctx", "prompt", Some("sonnet"), None)
        });
        assert_eq!(body.as_str(), "mocked ok");
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
            // These go through ER_FAKE_CLAUDE and never reach the spawn.
            timeout: std::time::Duration::from_mins(15),
            slot_cap: 3,
            slot_ceiling: 6,
        };
        let body = with_fake_claude("fail", || {
            run_card_ai_subprocess(&inv, "ctx", "prompt", Some("sonnet"), None)
        });
        assert!(
            body.as_str().starts_with("Pending — invoke via CLI"),
            "expected fallback message, got: {}",
            body.as_str()
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
            // These go through ER_FAKE_CLAUDE and never reach the spawn.
            timeout: std::time::Duration::from_mins(15),
            slot_cap: 3,
            slot_ceiling: 6,
        };
        let body = with_fake_claude("custom-response-text", || {
            run_card_ai_subprocess(&inv, "ctx", "prompt", Some("sonnet"), None)
        });
        assert_eq!(body.as_str(), "custom-response-text");
    }

    fn github_pr_json() -> &'static [u8] {
        br#"{"number":7,"title":"Tidy up","headRefName":"feat/x","baseRefName":"main","headRefOid":"abc123","updatedAt":"2026-01-02T03:04:05Z","state":"OPEN","isDraft":false,"author":{"login":"alice"},"assignees":[{"login":"bob"}],"reviewRequests":[{"login":"carol"}],"reviewDecision":"APPROVED","mergedAt":null,"latestReviews":[{"author":{"login":"dan"},"state":"APPROVED"}]}"#
    }

    #[test]
    fn fetch_single_pr_for_remote_parses_the_handle_output() {
        let github = er_engine::github::FakeGitHub::default()
            .with_pr_view_json(Ok(github_pr_json().to_vec()));

        let pr = fetch_single_pr_for_remote(&github, "o/r", 7).unwrap();

        assert_eq!(pr.number, 7);
        assert_eq!(pr.title, "Tidy up");
        assert_eq!(pr.head_ref, "feat/x");
        assert_eq!(pr.base_ref, "main");
        assert_eq!(pr.head_oid, "abc123");
        assert_eq!(pr.author, "alice");
        assert_eq!(pr.assignees, vec!["bob".to_string()]);
        assert_eq!(pr.reviewers, vec!["carol".to_string()]);
        assert_eq!(pr.review_decision.as_deref(), Some("APPROVED"));
        assert_eq!(
            pr.latest_reviewer_states,
            vec![("dan".to_string(), "APPROVED".to_string())]
        );
        assert_eq!(
            github.calls(),
            vec![format!(
                "pr_view_json(repo_slug=Some({:?}), dir=None, number=7, fields={:?})",
                "o/r",
                "number,title,headRefName,baseRefName,headRefOid,updatedAt,state,isDraft,author,assignees,reviewRequests,reviewDecision,mergedAt,latestReviews"
            )]
        );
    }

    #[test]
    fn fetch_single_pr_for_remote_prefixes_a_gh_error() {
        let github = er_engine::github::FakeGitHub::default().with_pr_view_json(Err(
            er_engine::github::GhError::NotFound {
                message: "nope".into(),
            },
        ));

        let error = fetch_single_pr_for_remote(&github, "o/r", 7).unwrap_err();

        assert_eq!(error, "Failed to fetch PR #7 for o/r: nope");
    }

    #[cfg(unix)]
    fn classified_review_fixture() -> er_engine::github::GhError {
        use std::os::unix::process::ExitStatusExt;
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../er-github/tests/fixtures/gh/api_422_review_string_errors.synthetic");
        let stdout = std::fs::read(dir.join("stdout")).unwrap();
        let stderr = std::fs::read(dir.join("stderr")).unwrap();
        let exit: i32 = std::fs::read_to_string(dir.join("exit"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let output = std::process::Output {
            status: std::process::ExitStatus::from_raw(exit << 8),
            stdout,
            stderr,
        };
        er_engine::github::classify(Ok(output), true, false).unwrap_err()
    }

    #[cfg(unix)]
    #[test]
    fn unprocessable_keeps_githubs_message_for_the_message_checks() {
        let error = classified_review_fixture();
        assert!(matches!(
            error,
            er_engine::github::GhError::Unprocessable { .. }
        ));
        let message = error.to_string();
        assert!(is_one_pending_review_error(&message), "{message}");
        assert!(is_own_pr_approval_error(&message), "{message}");
        assert_eq!(
            gh_review_submit_err(anyhow::Error::from(error)),
            "GitHub does not allow approving your own pull request."
        );
    }

    #[test]
    fn the_422_hint_needs_a_typed_error() {
        let textual = gh_review_submit_err(anyhow::anyhow!("gh: failed on PR #4220 (HTTP 500)"));
        assert!(!textual.contains("Likely causes"), "{textual}");

        let typed = gh_review_submit_err(anyhow::Error::from(
            er_engine::github::GhError::Unprocessable {
                message: "pull_request_review_thread.line must be part of the diff".into(),
            },
        ));
        assert!(typed.contains("Likely causes"), "{typed}");
    }

    #[test]
    fn one_pending_review_refusal_is_detected_from_github_422() {
        let raw = r#"gh: Unprocessable Entity (HTTP 422) ({"message":"Unprocessable Entity","errors":["user_id can only have one pending review per pull request"]})"#;
        assert!(is_one_pending_review_error(raw));
        assert!(!is_one_pending_review_error(
            "gh: Unprocessable Entity (HTTP 422) pull_request_review_thread.line must be part of the diff"
        ));
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

        // No temporary file may survive the rename.
        let leftovers: Vec<String> =
            std::fs::read_dir(std::path::Path::new(&path).parent().unwrap())
                .unwrap()
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| name.contains(".tmp."))
                .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
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

    #[test]
    fn image_data_url_carries_mime_and_exact_bytes() {
        assert_eq!(
            image_data_url("image/png", b"\x89PNG\0"),
            "data:image/png;base64,iVBORwA="
        );
        assert_eq!(
            image_data_url("image/svg+xml", b"<svg/>"),
            "data:image/svg+xml;base64,PHN2Zy8+"
        );
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
        let item_thread = item;
        let (tx, rx) = mpsc::channel();

        std::thread::spawn(move || {
            let mut just_added = Vec::new();
            if let Ok(mut guard) = inbox_thread.lock() {
                if guard.add_item(item_thread.clone()) {
                    just_added.push(item_thread);
                }
            }
            for added in &just_added {
                maybe_send_native_notification(
                    &inbox_thread,
                    &handle_thread,
                    &InboxConfig::default(),
                    added,
                );
            }
            let _ = tx.send(());
        });

        rx.recv_timeout(Duration::from_millis(500))
            .expect("inbox notify path deadlocked (re-entrant lock on ai_review_done)");
        assert_eq!(inbox.lock().unwrap().items.len(), 1);
    }

    #[test]
    fn disabled_notify_kind_is_not_marked_notified() {
        let inbox: InboxHandle = Arc::new(Mutex::new(crate::inbox::InboxState::default()));
        let app_handle: Arc<Mutex<Option<tauri::AppHandle>>> = Arc::new(Mutex::new(None));
        let mut prefs = InboxConfig::default();
        prefs.notify.ai_review_done = false;
        let item = InboxItem {
            id: "inbox-ai-skip".to_string(),
            kind: "ai_review_done".to_string(),
            severity: "success".to_string(),
            title: "AI review completed".to_string(),
            body: "done".to_string(),
            source: "ai".to_string(),
            target: InboxTarget {
                project_id: None,
                repo_root: None,
                remote: None,
                pr_number: None,
                branch: None,
                url: None,
            },
            created_at_ms: 0,
            read_at_ms: None,
            dedupe_key: "ai:skip:done".to_string(),
        };

        maybe_send_native_notification(&inbox, &app_handle, &prefs, &item);
        assert!(
            !inbox.lock().unwrap().notified_item_ids.contains(&item.id),
            "disabled notify kinds must not be marked notified"
        );
    }

    #[test]
    fn failed_native_send_is_not_marked_notified() {
        let inbox: InboxHandle = Arc::new(Mutex::new(crate::inbox::InboxState::default()));
        let app_handle: Arc<Mutex<Option<tauri::AppHandle>>> = Arc::new(Mutex::new(None));
        let item = InboxItem {
            id: "inbox-ai-fail-send".to_string(),
            kind: "ai_review_done".to_string(),
            severity: "success".to_string(),
            title: "AI review completed".to_string(),
            body: "done".to_string(),
            source: "ai".to_string(),
            target: InboxTarget {
                project_id: None,
                repo_root: None,
                remote: None,
                pr_number: None,
                branch: None,
                url: None,
            },
            created_at_ms: 0,
            read_at_ms: None,
            dedupe_key: "ai:fail-send:done".to_string(),
        };

        maybe_send_native_notification(&inbox, &app_handle, &InboxConfig::default(), &item);
        assert!(
            !inbox.lock().unwrap().notified_item_ids.contains(&item.id),
            "a failed native send must not be marked notified"
        );
    }

    #[test]
    fn flush_pending_respects_passed_prefs() {
        let inbox: InboxHandle = Arc::new(Mutex::new(crate::inbox::InboxState::default()));
        let app_handle: Arc<Mutex<Option<tauri::AppHandle>>> = Arc::new(Mutex::new(None));
        let item = InboxItem {
            id: "inbox-flush-skip".to_string(),
            kind: "ai_review_done".to_string(),
            severity: "success".to_string(),
            title: "AI review completed".to_string(),
            body: "done".to_string(),
            source: "ai".to_string(),
            target: InboxTarget {
                project_id: None,
                repo_root: None,
                remote: None,
                pr_number: None,
                branch: None,
                url: None,
            },
            created_at_ms: 0,
            read_at_ms: None,
            dedupe_key: "ai:flush-skip:done".to_string(),
        };
        assert!(inbox.lock().unwrap().add_item(item.clone()));

        let mut prefs = InboxConfig::default();
        prefs.notify.ai_review_done = false;
        flush_pending_native_notifications(&inbox, &app_handle, &prefs);
        assert!(
            !inbox.lock().unwrap().notified_item_ids.contains(&item.id),
            "flush must honor the App prefs it was given, not disk defaults"
        );
    }

    #[test]
    fn diff_syncs_refresh_the_github_status() {
        // The merge box blocks a merge when the synced diff's head and the
        // cached PR head differ; without a refresh it stays blocked after a
        // Sync and tells the user to sync again. The sync takes its key from
        // the tab's content.
        let src = include_str!("resources/diff.rs");
        let (_, from) = src.split_once("fn refresh(").expect("the diff refresh");
        let (body, _) = from.split_once("\n}\n").expect("function end");
        assert!(
            body.contains("refresh_status_after_sync(desktop, synced.status_key);"),
            "a sync must refresh the GitHub status"
        );
    }

    fn cached_pr(number: u64) -> crate::snapshot::PrInfo {
        let mut pr = crate::snapshot::minimal_pr_info(number, "t");
        pr.base_ref = "main".into();
        pr.head_ref = "feat/x".into();
        pr.head_oid = "abc123".into();
        pr
    }

    /// Inbox targets carry the lowercased slug; the cache keeps the project's
    /// spelling. A miss drops the hint and the open takes the slow path.
    #[test]
    fn inbox_pr_hint_matches_mixed_case_cache_key() {
        let mut cache = HashMap::new();
        cache.insert(
            "VilfredSikker/TechProfessor".to_string(),
            vec![cached_pr(7)],
        );
        let hint = inbox_pr_hint_from_cache(&cache, 7, "vilfredsikker/techprofessor")
            .expect("hint from mixed-case cache key");
        assert_eq!(hint.head_oid, "abc123");
        assert_eq!(hint.base_ref, "main");
    }

    #[test]
    fn inbox_pr_hint_ignores_other_repos_with_same_number() {
        let mut cache = HashMap::new();
        cache.insert("other/repo".to_string(), vec![cached_pr(7)]);
        assert!(inbox_pr_hint_from_cache(&cache, 7, "vilfredsikker/techprofessor").is_none());
    }

    fn project(id: &str, root_path: &str, remote: &str) -> projects::ProjectRecord {
        projects::ProjectRecord {
            id: id.to_string(),
            name: id.to_string(),
            root_path: root_path.to_string(),
            remote: Some(remote.to_string()),
            dismissed_prs: Vec::new(),
            tracked_prs: Vec::new(),
            tracked_branches: Vec::new(),
            dismissed_branches: Vec::new(),
            recent_prs: Vec::new(),
            saved_prs: Vec::new(),
            auto_triage_max_diff_kb: 0,
            review_ignore_globs: Vec::new(),
            extra: Default::default(),
        }
    }

    fn pr_target(remote: &str, pr_number: u64) -> InboxTarget {
        InboxTarget {
            project_id: None,
            repo_root: None,
            remote: Some(remote.to_string()),
            pr_number: Some(pr_number),
            branch: Some("feat/x".to_string()),
            url: None,
        }
    }

    /// A remote-only project has no clone for `open_pr_review` to run in, so
    /// its PR must open the way the sidebar opens it, via `open_remote_pr`.
    #[test]
    fn inbox_open_routes_remote_only_project_to_remote_pr() {
        let file = projects::ProjectsFile {
            projects: vec![project("ai-models", "", "ReshapeBiotech/ai-models")],
            active_id: None,
        };
        let resolved = inbox_open_target_in(
            &file,
            &HashMap::new(),
            pr_target("reshapebiotech/ai-models", 3),
        );
        assert!(matches!(
            resolved,
            Some(InboxOpenTarget::RemotePr { ref owner, ref repo, number: 3 })
                if owner == "ReshapeBiotech" && repo == "ai-models"
        ));
    }

    #[test]
    fn inbox_open_routes_local_project_to_pr_review_with_hint() {
        let file = projects::ProjectsFile {
            projects: vec![project("tp", "/repo/tp", "VilfredSikker/TechProfessor")],
            active_id: None,
        };
        let mut cache = HashMap::new();
        cache.insert(
            "VilfredSikker/TechProfessor".to_string(),
            vec![cached_pr(7)],
        );
        let resolved =
            inbox_open_target_in(&file, &cache, pr_target("vilfredsikker/techprofessor", 7));
        assert!(matches!(
            resolved,
            Some(InboxOpenTarget::LocalPr { ref project_id, pr_number: 7, hint: Some(_) })
                if project_id == "tp"
        ));
    }
}

/// The active tab's review as the projection sees it — scoped overlays replace
/// their paths and the validate pass's outcomes are applied — never the raw
/// `review.json`.
pub(crate) fn projected_review_json(
    er_dir: &str,
    branch_diff_hash: &str,
    branch_scope: Option<&str>,
) -> Result<String, String> {
    review_projection(er_dir, branch_diff_hash, branch_scope)?
        .ok_or_else(|| "No review.json found".to_string())
}

/// [`projected_review_json`], with none for a bucket that holds no review.
pub(crate) fn review_projection(
    er_dir: &str,
    branch_diff_hash: &str,
    branch_scope: Option<&str>,
) -> Result<Option<String>, String> {
    let overlay = er_engine::ai::arbiter_overlay_stamp(er_dir);
    let ai = er_engine::ai::load_ai_state_with_arbiter(
        er_dir,
        branch_diff_hash,
        branch_scope,
        overlay.as_deref(),
    );
    let Some(review) = ai.review else {
        return Ok(None);
    };
    // Collapse the duplicate rows the display hides, so the copy matches what
    // the reviewer sees; active, resolved and dropped rows all stay.
    let mut visible = review.clone();
    visible.files = review
        .files
        .iter()
        .map(|(path, file)| {
            let rows = er_engine::ai::project_visible_findings(
                file.findings.iter().map(|finding| (path.as_str(), finding)),
                &ai.arbiter_graded_keys,
            );
            (
                path.clone(),
                er_engine::ai::ErFileReview {
                    risk: file.risk,
                    risk_reason: file.risk_reason.clone(),
                    summary: file.summary.clone(),
                    findings: rows
                        .into_iter()
                        .map(|(_, finding)| finding.clone())
                        .collect(),
                },
            )
        })
        .collect();
    serde_json::to_string_pretty(&visible)
        .map(Some)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod projected_review_tests {
    use super::projected_review_json;

    #[test]
    fn read_review_json_returns_the_projection() {
        let dir = tempfile::tempdir().unwrap();
        er_engine::storage::with_test_storage_root(dir.path(), || {
            let bucket = dir.path().join("bucket");
            er_engine::test_support::write_sidecar_json::<er_engine::ai::ErReview>(
                &bucket,
                serde_json::json!({
                    "version": 1, "diff_hash": "hash", "created_at": "t0",
                    "files": { "src/a.rs": { "risk": "medium", "findings": [
                        { "id": "a1", "severity": "medium", "title": "Old A", "line_start": 1 }
                    ] },
                    "src/b.rs": { "risk": "medium", "findings": [
                        { "id": "b1", "severity": "medium", "title": "Resolved B", "line_start": 2 }
                    ] }
                }
                }),
            );
            er_engine::test_support::write_sidecar_json::<er_engine::ai::ScopedReview>(
                &bucket,
                serde_json::json!({
                    "version": 1, "diff_hash": "hash", "created_at": "t1",
                    "files": { "src/a.rs": { "risk": "medium", "findings": [
                        { "id": "n1", "severity": "medium", "title": "New A", "line_start": 5 }
                    ] } },
                    "scope_paths": ["src/a.rs"],
                    "base_diff_hash": "hash",
                    "base_created_at": "t0",
                    "host_stamped": true
                }),
            );
            let key = er_engine::ai::identity::finding_key("src/b.rs", Some(2), "Resolved B");
            er_engine::test_support::write_sidecar_json::<er_engine::ai::ValidationReview>(
                &bucket,
                serde_json::json!({
                    "version": 1, "diff_hash": "hash", "review_created_at": "t0",
                    "created_at": "t1",
                    "outcomes": [{ "key": key, "file": "src/b.rs", "result": "resolved" }]
                }),
            );

            let mut app = er_engine::app::App::new_for_test(Vec::new());
            app.tab_mut().er_root =
                er_engine::ErRoot::managed(bucket.to_string_lossy().to_string());
            app.tab_mut().branch_diff_hash = "hash".to_string();

            let tab = app.tab();
            let scope = tab.storage_branch_scope();
            let json = projected_review_json(&tab.er_dir(), &tab.branch_diff_hash, scope).unwrap();
            let review: er_engine::ai::ErReview = serde_json::from_str(&json).unwrap();
            let a_titles: Vec<&str> = review.files["src/a.rs"]
                .findings
                .iter()
                .map(|finding| finding.title.as_str())
                .collect();
            assert_eq!(a_titles, ["New A"], "the overlay replaced its path");
            assert_eq!(
                review.files["src/b.rs"].findings[0].confidence,
                er_engine::ai::Confidence::Dropped,
                "the validation was applied"
            );
        });
    }
}

#[cfg(test)]
mod route_tests {
    /// Both finding replies take the service route: the plain reply is the
    /// workspace's write, and an AI-assisted one asks through the flag path's
    /// asks, which never reach the App.
    #[test]
    fn both_finding_replies_take_the_service_route() {
        let source = include_str!("commands.rs");
        let body = source
            .split("fn reply_to_finding_on_service(")
            .nth(1)
            .unwrap_or_else(|| panic!("no flag path for finding replies"));
        let body = body.split("\n}\n").next().unwrap_or(body);
        assert!(body.contains("ai_ask::ask_on_finding("), "{body}");
        assert!(
            body.contains("er_service::Command::ReplyToFinding"),
            "{body}"
        );
        assert!(!body.contains("reply_to_finding_impl"), "{body}");
    }

    /// Every routed annotation command has a service route. The service tests
    /// drive the actor itself; this holds the desktop side to sending each
    /// command instead of falling through to the flag-off body.
    #[test]
    fn every_annotation_command_has_a_service_route() {
        let source = include_str!("commands.rs");
        for command in [
            "Command::ToggleReviewed",
            "Command::MarkReviewed",
            "Command::BulkReviewPillar",
            "Command::AddQuestion",
            "Command::AddNote",
            "Command::AddDocumentThread",
            "Command::ReplyToThread",
            "Command::UpdateThreadMessage",
            "Command::DeleteThread",
            "Command::ResolveThread",
            "Command::PromoteToNote",
            "Command::ReplyToFinding",
            "Command::AddComment",
            "Command::ReplyToGithubThread",
            "Command::PromoteToComment",
            "Command::PromoteFindingToComment",
            "Command::SyncGithubComments",
            "Command::GetPendingReview",
            "Command::AddPendingReviewComment",
            "Command::RemovePendingReviewComment",
            "Command::SubmitPendingReview",
            "Command::PostGithubPrComment",
            "Command::PostGithubLineComment",
            "Command::SubmitGithubPrDecision",
        ] {
            assert!(
                source.contains(&format!("er_service::{command}")),
                "{command} has no service route"
            );
        }
    }

    /// The content commands call their route; the route tests drive the
    /// routes themselves, so this holds each Tauri command to calling one.
    #[test]
    fn every_content_command_calls_its_route() {
        // Built at run time, so this test's own text never matches.
        let source = include_str!("commands.rs");
        let call = |route: &str| format!("content_routes::{route}");
        // Each open command calls the open route in its own body, which ends
        // at the first closing brace in column zero.
        for command in [
            "service_open",
            "service_open_folder",
            "open_local_branch",
            "service_open_project_branch",
            "service_set_active_project",
        ] {
            let body = source
                .split_once(&format!("fn {command}("))
                .and_then(|(_, rest)| rest.split_once("\n}\n"))
                .map(|(body, _)| body);
            let Some(body) = body else {
                panic!("no {command}");
            };
            assert!(
                body.contains(&call("open(")),
                "{command} does not call its route"
            );
        }
        for route in [
            "close(",
            "refresh(&state, false)",
            "sync(&state)",
            "set_mode(",
            "select_file(",
            "select_commit(&state, &sha)",
            "exit_history(&state)",
            "request_file_content(",
        ] {
            let route = call(route);
            assert!(source.contains(&route), "no command calls {route}");
        }
        // The opens that once built an App tab go through the flag-on helpers.
        for (command, helper) in [
            ("set_active_project", "service_set_active_project("),
            ("open_worktree", "service_open_folder("),
            ("open_project_branch", "service_open_project_branch("),
            ("open_pr_branch", "service_open_pr_review("),
            ("open_inbox_item_impl", "service_open_inbox_target("),
        ] {
            let body = source
                .split_once(&format!("fn {command}("))
                .and_then(|(_, rest)| rest.split_once("\n}\n"))
                .map(|(body, _)| body);
            assert!(
                body.is_some_and(|body| body.contains(helper)),
                "{command} does not call {helper}"
            );
        }
        // toggle_reviewed, mark_reviewed and unmark_reviewed.
        let reviewed = source.matches(&call("write_reviewed(")).count();
        assert_eq!(reviewed, 3, "a reviewed command lost its route");
    }
}
