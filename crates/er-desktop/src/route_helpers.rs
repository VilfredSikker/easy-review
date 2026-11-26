use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
#[cfg(not(target_os = "macos"))]
use tauri_plugin_notification::NotificationExt;

use crate::page_view::{GhUser, PrInfo};
use crate::resources::content_routes;
pub use crate::state::DesktopState;
#[cfg(test)]
use er_engine::card_job::{run_card_ai_subprocess, CardAiInvocation};
use er_engine::config::InboxConfig;
use er_service::app_records::inbox::{
    inbox_item_from_notification, inbox_items_from_pr_transition, is_review_edge_kind, InboxHandle,
    InboxItem, InboxNotification, InboxTarget, NotificationInboxCtx, PrInboxView, PrTransitionCtx,
};
use er_service::app_records::projects::{self, normalize_remote_slug};
use er_service::app_records::AppOwner;

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
pub struct ReviewRevisionSummary {
    pub revision_id: String,
    pub created_at: String,
    pub scope: String,
    pub diff_hash: String,
    pub active: bool,
    pub agents: Vec<String>,
}

pub use er_engine::pr_open_entry::PrOpenFreshness;

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

pub(crate) fn now_ms() -> u64 {
    er_service::app_records::inbox::now_epoch_ms()
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

/// Files finished background tasks in the inbox, each
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
        state.app_owner.save_inbox();
        state.desktop_revision.fetch_add(1, Ordering::Relaxed);
    }
}

/// Kick off an async meta-cache refresh so the *next* poll reflects the
/// mutation that just happened. The refresh runs on a background thread and
/// does NOT block the caller.
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

// ── Navigation ────────────────────────────────────────────────────────────────

pub(crate) fn image_data_url(mime: &str, bytes: &[u8]) -> String {
    use base64::Engine as _;
    format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

// ── Hunk navigation ──────────────────────────────────────────────────────────

// ── Mode ──────────────────────────────────────────────────────────────────────

pub(crate) fn feature_allows_mode_str(
    features: &er_engine::config::FeatureFlags,
    mode: &str,
) -> bool {
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

// ── Reviewed state ────────────────────────────────────────────────────────────

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
    /// Saves the inbox once a sweep has written it.
    pub app_owner: &'a AppOwner,
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
        app_owner,
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
            if now.saturating_sub(last) >= er_service::app_records::inbox::REFRESH_ERROR_TTL_MS {
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
            app_owner.save_inbox();
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
        if now.saturating_sub(last) >= er_service::app_records::inbox::REFRESH_ERROR_TTL_MS {
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
                    .map(|c| {
                        now.saturating_sub(c.fetched_at_ms)
                            >= er_service::app_records::inbox::CI_TTL_MS
                    })
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
                er_service::app_records::inbox::ObservedPrState {
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
                    latest_reviewer_states: er_service::app_records::inbox::merge_reviewer_states(
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
            er_service::app_records::inbox::ObservedCiState {
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
    app_owner.save_inbox();
    if emitted_any {
        crate::profile_log::bump_desktop_revision(desktop_revision, "inbox_items");
    }
    if should_ingest_notifications {
        ingest_github_notifications(handles, &project_by_remote, &skip_review_prs, now, prefs);
    }
}

fn ingest_github_notifications(
    handles: InboxRefreshHandles<'_>,
    project_by_remote: &HashMap<String, projects::ProjectRecord>,
    skip_review_prs: &HashSet<(String, u64)>,
    now: u64,
    prefs: &InboxConfig,
) {
    let InboxRefreshHandles {
        inbox: inbox_handle,
        app_owner,
        desktop_revision,
        app_handle: app_handle_state,
        ..
    } = handles;
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
            } else if er_service::app_records::inbox::should_remember_skipped_author(
                &note,
                skip_review_prs,
            ) {
                inbox.remember_notification(&gh_note.id, &gh_note.updated_at);
                remembered_any = true;
            }
        }
    }
    for item in &just_added {
        maybe_send_native_notification(inbox_handle, app_handle_state, prefs, item);
    }
    if emitted_any || remembered_any {
        app_owner.save_inbox();
    }
    if emitted_any {
        crate::profile_log::bump_desktop_revision(desktop_revision, "inbox_notifications");
    }
}

/// Sidebar triage: queued on the task host, with the
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
    pr_cache: &Arc<Mutex<HashMap<String, Vec<crate::page_view::PrInfo>>>>,
    remote: &str,
    pr_number: u64,
) -> Option<crate::page_view::PrInfo> {
    pr_cache.lock().ok().and_then(|cache| {
        cache
            .get(remote)
            .and_then(|prs| prs.iter().find(|p| p.number == pr_number).cloned())
    })
}

fn open_pr_for_branch(
    pr_cache: &Arc<Mutex<HashMap<String, Vec<crate::page_view::PrInfo>>>>,
    remote: &str,
    branch: &str,
) -> Option<crate::page_view::PrInfo> {
    pr_cache.lock().ok().and_then(|cache| {
        cache.get(remote).and_then(|prs| {
            prs.iter()
                .find(|p| p.state == "OPEN" && !p.is_draft && p.head_ref == branch)
                .cloned()
        })
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
    let pr = pr_from_cache(desktop.app_owner.pr_cache(), &remote, pr_number)
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

    if let Some(pr) = open_pr_for_branch(desktop.app_owner.pr_cache(), &remote, branch) {
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

// ── Filter / search ───────────────────────────────────────────────────────────

// The file-tree filter applies live while the user types (debounced in
// FileTree.svelte). Sync commands run on the main thread, and re-filtering +
// rebuilding the snapshot on a large diff is heavy enough to visibly freeze
// the window per apply — so both commands run via `run_blocking`.

// ── Threads ───────────────────────────────────────────────────────────────────

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

// ── GitHub sync ───────────────────────────────────────────────────────────────

// Sync commands run on the main thread; `refresh_diff` shells out to git and
// rebuilds the snapshot — heavy enough to freeze the window from ⌘K.

/// Latest known PR `head_oid` for `pr_number` from the PR-list cache. This is
/// the exact source the stale-pill compares against (see `build_snapshot`), so
/// callers can align a tab's `last_diff_head_oid` with it to clear the pill.
pub(crate) fn pr_cache_head_oid_for_pr(state: &DesktopState, pr_number: u64) -> Option<String> {
    state.app_owner.pr_cache().lock().ok().and_then(|cache| {
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
pub(crate) fn align_branch_base_probe(state: &DesktopState, repo_root: &str, base_branch: &str) {
    let base_short = base_branch.trim_start_matches("origin/");
    if let Some(oid) = er_engine::github::rev_parse_oid(&*state.ports.git, repo_root, base_branch) {
        let key = format!("{repo_root}\u{0}{base_short}");
        if let Ok(mut cache) = state.branch_base_remote_oid.lock() {
            cache.insert(key, oid);
        }
    }
}

pub(crate) fn refresh_status_after_sync(state: &DesktopState, key: Option<(String, String, u64)>) {
    if let Some((owner, repo, number)) = key {
        state
            .app_owner
            .kick_github_status_refresh(owner, repo, number);
    }
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
    er_service::with_review_hints(raw, unprocessable)
}

// ── Pending review ───────────────────────────────────────────────────────────
//
// The composer's "Start a review" collects line comments in the reviewer's
// GitHub pending review; the Review popover submits them with one decision.
// GitHub holds the draft, so every command here is a `gh` round trip and runs
// off the lock, which is held only to read which PR the tab is on (ADR 0015).

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
    state
        .app_owner
        .kick_github_status_refresh(owner, repo, number);
}

// ── AI integration ───────────────────────────────────────────────────────────

pub(crate) use er_service::runs::resolve_review_scope_in;

// ⌘K / AI Hub: may shell out for the review diff and writes artifacts, then
// rebuilds a snapshot — must not run on the main thread. Keep admission,
// prepared-artifact writes, and enqueue under the App lock so an Advanced
// producer cannot race a flow lease between its check and its write.

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

// ── AI provider / model selection ───────────────────────────────────────────

// ── Promote question to GitHub comment ──────────────────────────────────────

// ── Finding promotions sidecar ───────────────────────────────────────────────
// Map of `finding_id -> comment_id`. Stored alongside `.er/review.json` so
// `er` does not have to write into AI-owned files.

// ── Ask AI ──────────────────────────────────────────────────────────────────

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

/// The model a card ask runs: the one picked, else the agent's.
pub(crate) fn card_model(picked: Option<String>, agent_model: String) -> Option<String> {
    picked.or_else(|| (!agent_model.trim().is_empty()).then_some(agent_model))
}

/// Where a card ask's progress lines go: the card UI has no log panel, so
/// they go to the debug log, drained on their own thread while the job
/// blocks.
pub(crate) fn card_progress() -> er_engine::card_job::ProgressSink {
    Arc::new(|line: &str| {
        log::debug!("card ai: {line}");
    })
}

// ── PR URL open ──────────────────────────────────────────────────────────────

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

/// The desktop side of the service's open path. App-scoped records are
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
    // The PR becomes a registry tab whose content the actor loads.
    content_routes::open(state, tab, replace.unwrap_or(false))
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
    // so it carries no local root.
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

// ── Worktree picker (stub — no dialog dep) ──────────────────────────────────

/// Opens a Working tab on a picked folder, registering it as a project.
pub(crate) fn service_open_folder(state: &DesktopState, path: String) -> Result<(), String> {
    let _ = state
        .app_owner
        .auto_register(&*state.ports.github, &*state.ports.git, &path);
    content_routes::open(state, content_routes::working_tab(path), false)
}

// ── Project commands ─────────────────────────────────────────────────────────

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

/// The app registry warms its own cache through the ports.
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

/// Starts a PR-list fetch through the sidebar's owner, of every project
/// remote or of `remote` alone, and runs the inbox over the result before
/// the fetch ends. A fetch already running absorbs it. `prefs` reads the
/// inbox prefs once the fetch is done.
pub(crate) fn kick_pr_list_refresh(
    desktop: &DesktopState,
    remote: Option<String>,
    prefs: impl FnOnce() -> InboxConfig + Send + 'static,
) -> Result<(), String> {
    let reason = if remote.is_some() {
        "pr_cache_refresh_project_manual"
    } else {
        "pr_cache_refresh_manual"
    };
    let github = Arc::clone(&desktop.ports.github);
    let owner = Arc::clone(&desktop.app_owner);
    let desktop = desktop.clone();
    owner.kick_pr_list_refresh(github, remote, move |failed| {
        let prefs = prefs();
        let handles = InboxRefreshHandles {
            pr_cache: desktop.app_owner.pr_cache(),
            gh_user: desktop.app_owner.gh_user(),
            inbox: &desktop.inbox,
            app_owner: &desktop.app_owner,
            desktop_revision: &desktop.desktop_revision,
            app_handle: &desktop.tauri_app_handle,
        };
        for remote in failed {
            process_inbox_after_pr_refresh(handles, &prefs, Some(remote));
        }
        process_inbox_after_pr_refresh(handles, &prefs, None);
        crate::profile_log::bump_desktop_revision(&desktop.desktop_revision, reason);
    });
    Ok(())
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

/// Build a `PrOpenHint` for an inbox PR open from the cached PR list. Returns
/// `None` if the PR is not cached or a required field is missing; the open
/// then falls back to the no-hint (async-miss) path.
///
/// The cache is keyed by each project's `remote` as written in projects.json
/// (`VilfredSikker/easy-review`), while inbox targets carry the lowercased
/// slug, so the key has to be matched after normalizing both sides.
fn inbox_pr_hint_from_cache(
    cache: &HashMap<String, Vec<crate::page_view::PrInfo>>,
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
    let guard = state.app_owner.pr_cache().lock().ok();
    inbox_open_target_in(&file, guard.as_deref().unwrap_or(&empty), target)
}

fn inbox_open_target_in(
    file: &projects::ProjectsFile,
    cache: &HashMap<String, Vec<crate::page_view::PrInfo>>,
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
        let (owner, repo) = er_service::app_records::projects::remote_owner_repo(remote)?;
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

/// What the open-cache prefetches take.
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

/// An inbox open through the same helpers as a sidebar click.
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

    desktop
        .app_owner
        .add_tracked_branch(project_id, name)
        .map_err(|e| e.to_string())
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

    desktop
        .app_owner
        .remove_tracked_branch(project_id, name)
        .map_err(|e| e.to_string())
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
    state
        .app_owner
        .delete_project(project_id)
        .map_err(|e| e.to_string())?;
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

pub(crate) fn service_open_project_branch(
    state: &DesktopState,
    project_id: String,
    branch: String,
    replace: bool,
) -> Result<(), String> {
    let tab = content_routes::branch_tab(&project_id, branch)?;
    content_routes::open(state, tab, replace)?;
    state.app_owner.set_active_project(&project_id);
    Ok(())
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
        if let Ok(cache) = state.app_owner.pr_cache().lock() {
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
        .app_owner
        .pr_cache()
        .lock()
        .ok()
        .and_then(|g| g.get(remote).cloned())
        .unwrap_or_default();

    let me: Option<String> = state
        .app_owner
        .gh_user()
        .lock()
        .ok()
        .and_then(|v| v.clone());

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

/// Opens a Working tab on the project's root through the registry, as the
/// other opens do.
pub(crate) fn service_set_active_project(state: &DesktopState, id: &str) -> Result<(), String> {
    let root = projects::load()
        .projects
        .iter()
        .find(|p| p.id == id)
        .map(|p| p.root_path.clone())
        .ok_or_else(|| format!("Project not found: {id}"))?;
    content_routes::open(state, content_routes::working_tab(root), false)?;
    state.app_owner.set_active_project(id);
    Ok(())
}

// ── Review checklist ────────────────────────────────────────────────────────

// ── Findings: dismiss / promote / reply (v1 stubs) ──────────────────────────

// ── Commit composer ──────────────────────────────────────────────────────────

// ── History: select commit ──────────────────────────────────────────────────

// ── Tab management ──────────────────────────────────────────────────────────

/// A Working tab on the active tab's repo, read from the registry's view.
pub(crate) fn service_new_tab(state: &DesktopState) -> Result<(), String> {
    let view = state.service.app_registry().tabs_view();
    let root = new_tab_root(&view, project_root_for_remote)
        .ok_or_else(|| "Failed to open new tab: no open tab names a repo".to_string())?;
    let tab = content_routes::working_tab(root);
    let command = er_service::AppCommand::NewTab { tab };
    state
        .service
        .send_app(command)
        .map(drop)
        .map_err(|e| e.to_string())
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

// ── UI annotations (browser view) ───────────────────────────────────────────

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

/// Read a saved screenshot back as a `data:image/png;base64,<...>` URL so the
/// frontend can render it without configuring the Tauri asset protocol.
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

#[cfg(test)]
mod tests {
    use super::*;
    use er_engine::ai::{load_ui_annotations, save_ui_annotations, UiAnnotation};
    use er_engine::tab_identity::DiffMode;

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
        let pr_cache = Arc::new(Mutex::new(HashMap::new()));
        let gh_user = Arc::new(Mutex::new(None));
        let inbox = Arc::new(Mutex::new(
            er_service::app_records::inbox::InboxState::default(),
        ));
        let app_owner = Arc::new(er_service::app_records::AppOwner::new(
            Default::default(),
            Arc::clone(&pr_cache),
            Default::default(),
            Arc::clone(&gh_user),
            Arc::clone(&inbox),
        ));
        let watch_status = Arc::new(Mutex::new(crate::page_view::WatchStatusSnapshot::default()));
        let desktop = DesktopState {
            ports: er_engine::ports::Ports::cli(),
            service: Arc::new(crate::service::ServiceState::with_settings_path(
                er_engine::ports::Ports::for_test(),
                dir.path().join("settings").join("config.toml"),
            )),
            view: Default::default(),
            branch_base_remote_oid: Arc::new(Mutex::new(HashMap::new())),
            app_owner,
            terminals: Arc::new(Mutex::new(HashMap::new())),
            pending_ai_replies: Default::default(),
            ui_annotations: Default::default(),
            ui_annotations_io: Arc::default(),
            desktop_revision,
            watch_status,
            inbox,
            tauri_app_handle: Arc::new(Mutex::new(None)),
            auto_triage_in_flight: Arc::new(Mutex::new(HashSet::new())),
            line_commits_cache: Arc::default(),
            session: Default::default(),
            live_jobs: Default::default(),
            live_tabs: Default::default(),
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
            let project_id = state
                .app_owner
                .ensure_remote_project("acme/widgets")
                .expect("project");
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

        let inbox: InboxHandle = Arc::new(Mutex::new(
            er_service::app_records::inbox::InboxState::default(),
        ));
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
        let inbox: InboxHandle = Arc::new(Mutex::new(
            er_service::app_records::inbox::InboxState::default(),
        ));
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
        let inbox: InboxHandle = Arc::new(Mutex::new(
            er_service::app_records::inbox::InboxState::default(),
        ));
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
        let inbox: InboxHandle = Arc::new(Mutex::new(
            er_service::app_records::inbox::InboxState::default(),
        ));
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

    fn cached_pr(number: u64) -> crate::page_view::PrInfo {
        let mut pr = crate::page_view::minimal_pr_info(number, "t");
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
    use super::review_projection;

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

            let er_dir = bucket.to_string_lossy().to_string();
            let json = review_projection(&er_dir, "hash", None).unwrap().unwrap();
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
