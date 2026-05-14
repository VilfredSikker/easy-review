use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
use tauri::State;

use er_engine::ai::CommentType;
use er_engine::app::{App, DiffMode, InputMode};
use er_engine::highlight::Highlighter;

use crate::projects;
use crate::snapshot::{
    build_snapshot, AppSnapshot, CheckSummary, GhCommentSummary, GhReviewSummary, GhStatusCache,
    GhUser, GithubStatusSnapshot, LoadingState, MetaCache, PendingAiReplies, PrInfo,
};

const DEFAULT_ASK_AI_PROMPT: &str = "Elaborate on this and answer any question directly.";

#[derive(Debug, Clone, serde::Serialize)]
pub struct PollResponse {
    pub revision: u64,
    pub snapshot: AppSnapshot,
}

pub struct AppState {
    pub app: Arc<Mutex<App>>,
    pub highlighter: Mutex<Highlighter>,
    pub pr_cache: Arc<Mutex<HashMap<String, Vec<PrInfo>>>>,
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
}

macro_rules! snap {
    ($state:expr) => {{
        let app = $state.app.lock().map_err(|e| e.to_string())?;
        let mut hl = $state.highlighter.lock().map_err(|e| e.to_string())?;
        Ok(build_snapshot(
            &app,
            &mut hl,
            Some(&$state.pr_cache),
            Some(&$state.meta_cache),
            Some(&$state.gh_user),
            Some(&$state.pending_ai_replies),
            Some(&$state.gh_status_cache),
            Some(&$state.loading),
        ))
    }};
}

/// Build a snapshot using the lock guards directly (when callers already hold them).
fn snap_from(app: &App, hl: &mut Highlighter, state: &AppState) -> AppSnapshot {
    build_snapshot(
        app,
        hl,
        Some(&state.pr_cache),
        Some(&state.meta_cache),
        Some(&state.gh_user),
        Some(&state.pending_ai_replies),
        Some(&state.gh_status_cache),
        Some(&state.loading),
    )
}

/// Spawn a background fetch of the GitHub status for the given (owner, repo, number).
/// Returns immediately. The cache is updated on success; failures are logged.
/// Deduplicates: if a fetch for the same key is already in-flight, this is a no-op.
pub fn kick_github_status_refresh(
    cache: GhStatusCache,
    in_flight: Arc<Mutex<HashSet<(String, String, u64)>>>,
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
    let in_flight_clone = Arc::clone(&in_flight);
    std::thread::spawn(move || {
        let snap = fetch_github_status(&owner, &repo, number);
        if let Some(snap) = snap {
            if let Ok(mut g) = cache.lock() {
                g.insert((owner, repo, number), snap);
            }
        }
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

/// Fetch all GitHub status data for a PR. Runs the 4 gh calls in parallel.
/// Returns None when the PR overview fetch fails (e.g. no network, gh not authed).
/// Comments/reviews/checks failures are non-fatal — the snapshot still populates.
pub fn fetch_github_status(owner: &str, repo: &str, number: u64) -> Option<GithubStatusSnapshot> {
    let t = std::time::Instant::now();
    // Run 4 independent gh calls concurrently — cuts wall time from ~3.5s to ~1s.
    let (overview_res, checks, comments, reviews) = std::thread::scope(|s| {
        let o = s.spawn(|| er_engine::github::gh_pr_overview_remote_full(owner, repo, number));
        let c = s.spawn(|| er_engine::github::gh_pr_checks_remote(owner, repo, number).unwrap_or_default());
        let cm = s.spawn(|| er_engine::github::gh_pr_comments_overview(owner, repo, number).unwrap_or_default());
        let r = s.spawn(|| er_engine::github::gh_pr_reviews(owner, repo, number).unwrap_or_default());
        (
            o.join().ok(),
            c.join().unwrap_or_default(),
            cm.join().unwrap_or_default(),
            r.join().unwrap_or_default(),
        )
    });
    let overview = overview_res?.ok()?;
    log::info!("gh_status fetch {owner}/{repo}#{number} in {}ms", t.elapsed().as_millis());

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
    std::thread::spawn(move || {
        crate::snapshot::refresh_meta_cache(&root, &cache);
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
    app.tab_mut().toggle_compacted().map_err(|e| e.to_string())?;
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
            let hash = tab.current_per_file_hashes.get(&path).cloned().unwrap_or_default();
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
pub fn open_in_editor(state: State<AppState>) -> Result<(), String> {
    let app = state.app.lock().map_err(|e| e.to_string())?;
    app.tab().open_in_editor().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_url_in_browser(url: String) -> Result<(), String> {
    let result = if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(&url).spawn()
    } else if cfg!(target_os = "linux") {
        std::process::Command::new("xdg-open").arg(&url).spawn()
    } else {
        std::process::Command::new("cmd").args(["/c", "start", &url]).spawn()
    };
    result.map(|_| ()).map_err(|e| e.to_string())
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
    app.submit_comment_text(file, hunk_idx, line_num, text, CommentType::GitHubComment, None)
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
    app.submit_comment_text(file, hunk_idx, line_num, text, CommentType::Question, None)
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
            let q = tab.ai.questions.as_ref()
                .and_then(|qs| qs.questions.iter().find(|q| q.id == parent_id))
                .map(|q| (q.file.clone(), q.hunk_index.unwrap_or(0), q.line_start, CommentType::Question));
            q.ok_or_else(|| "Question not found".to_string())?
        } else {
            let c = tab.ai.github_comments.as_ref()
                .and_then(|gc| gc.comments.iter().find(|c| c.id == parent_id))
                .map(|c| (c.file.clone(), c.hunk_index.unwrap_or(0), c.line_start, CommentType::GitHubComment));
            c.ok_or_else(|| "Comment not found".to_string())?
        }
    };
    app.submit_comment_text(file, hunk_idx, line_num, text, comment_type, Some(parent_id))
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
    file_anchors
        .get(file)
        .is_some_and(|ranges| ranges.iter().any(|(start, end)| line >= *start && line < *end))
}

// ── GitHub sync ───────────────────────────────────────────────────────────────

#[tauri::command]
pub fn refresh_diff(state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    app.tab_mut().refresh_diff().map_err(|e| e.to_string())?;
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
        if let Some(snap) = fetch_github_status(&owner, &repo, number) {
            if let Ok(mut g) = state.gh_status_cache.lock() {
                g.insert((owner, repo, number), snap);
            }
        }
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
    app.push_all_comments_to_github().map_err(|e| e.to_string())?;
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
        _ => return Err(format!("Invalid review mode: {mode}. Use COMMENT, APPROVE, or REQUEST_CHANGES.")),
    };

    let (owner, repo_name, pr_number, is_remote, repo_root, comments_path, file_anchors) = {
        let app = state.app.lock().map_err(|e| e.to_string())?;
        let tab = app.tab();
        let is_remote = tab.is_remote();
        let repo_root = tab.repo_root.clone();
        let comments_path = tab.github_comments_path();
        let mut file_anchors: std::collections::HashMap<String, Vec<(usize, usize)>> =
            std::collections::HashMap::new();
        for f in &tab.files {
            let mut ranges = Vec::new();
            for h in &f.hunks {
                let start = h.new_start;
                let end = h.new_start + h.new_count.max(1);
                ranges.push((start, end));
            }
            file_anchors.insert(f.path.clone(), ranges);
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
        )
    };

    // Collect pending line comments into the batch format.
    let gc: ErGitHubComments = std::fs::read_to_string(&comments_path)
        .ok()
        .and_then(|s| serde_json::from_str::<ErGitHubComments>(&s).ok())
        .unwrap_or(ErGitHubComments { version: 1, diff_hash: String::new(), github: None, comments: vec![] });

    // Reject early if any unsynced local comment has no line anchor — those can
    // never be part of a GitHub review batch and would silently get marked synced
    // without actually being sent.
    let unsubmittable_count = gc
        .comments
        .iter()
        .filter(|c| c.source == "local" && !c.synced && !c.file.is_empty() && c.line_start.is_none())
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
                body: c.comment.clone(),
                side: c.side.clone(),
            })
        })
        .collect();

    // Partition into valid (anchor in current diff) and stale.
    let mut invalid_anchors: Vec<(String, usize, String)> = Vec::new();
    let mut batch_entries: Vec<BatchEntry> = Vec::new();
    for e in candidates {
        if is_anchor_in_current_diff(&file_anchors, &e.file, e.line) {
            batch_entries.push(e);
        } else {
            invalid_anchors.push((e.id, e.line, e.file));
        }
    }

    let batch: Vec<er_engine::github::ReviewBatchEntry> = batch_entries
        .iter()
        .map(|e| er_engine::github::ReviewBatchEntry {
            file: e.file.clone(),
            line: e.line,
            body: e.body.clone(),
            side: e.side.clone(),
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

// ── AI integration ───────────────────────────────────────────────────────────

#[tauri::command]
pub fn run_ai_review(
    scope: String,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;

    let repo_root = app.tab().repo_root.clone();
    let prompt = format!("/er-review {}", scope);
    std::process::Command::new("claude")
        .arg(&prompt)
        .current_dir(&repo_root)
        .spawn()
        .map_err(|_| {
            "Claude CLI not found; install with: npm i -g @anthropic-ai/claude-code".to_string()
        })?;

    Ok(snap_from(&app, &mut hl, &*state))
}

#[tauri::command]
pub fn set_ai_model(
    model: String,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;

    let repo_root = app.tab().repo_root.clone();
    let mut cfg = er_engine::config::load_config(&repo_root);
    cfg.agent.model = model;
    er_engine::config::save_config(&cfg).map_err(|e| e.to_string())?;

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
    app.submit_comment_text(file, hunk_idx, line_start, text, CommentType::GitHubComment, None)
        .map_err(|e| e.to_string())?;

    // 4. Find the new comment id (anything not in the pre-existing set).
    let new_id: Option<String> = {
        let tab = app.tab();
        tab.ai
            .github_comments
            .as_ref()
            .and_then(|gc| gc.comments.iter().find(|c| !existing_ids.contains(&c.id)).map(|c| c.id.clone()))
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

pub(crate) fn load_finding_promotions(
    er_dir: &str,
) -> std::collections::HashMap<String, String> {
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
                for r in qs.questions.iter().filter(|r| r.in_reply_to.as_deref() == Some(thread_id.as_str())) {
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
                for r in gc.comments.iter().filter(|r| r.in_reply_to.as_deref() == Some(thread_id.as_str())) {
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

    let user_prompt = if prompt.trim().is_empty() {
        DEFAULT_ASK_AI_PROMPT.to_string()
    } else {
        prompt
    };
    let app_arc = Arc::clone(&state.app);
    let pending_arc = Arc::clone(&state.pending_ai_replies);
    let meta_cache = state.meta_cache.clone();
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
                "ai".to_string(),
            );
        }
        if let Ok(mut p) = pending_arc.lock() {
            p.remove(&thread_id_for_thread);
        }
        crate::snapshot::refresh_meta_cache(&repo_root_for_thread, &meta_cache);
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
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    let pr_ref = er_engine::github::parse_github_pr_url(&url)
        .ok_or_else(|| format!("Not a valid GitHub PR URL: {url}"))?;
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

    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;

    let new_tab = er_engine::app::TabState::new(path_str.clone())
        .map_err(|e| format!("Failed to open {path_str}: {e}"))?;
    app.open_tab(new_tab);
    app.tab_mut()
        .refresh_diff()
        .map_err(|e| format!("Failed to refresh {path_str}: {e}"))?;
    let _ = projects::auto_register(&path_str);
    kick_meta_refresh(&*state, app.tab().repo_root.clone());
    Ok(snap_from(&app, &mut hl, &*state))
}

// ── Project commands ─────────────────────────────────────────────────────────

/// Inner helper: build the tab for `project_id`/`name` and place it on the
/// app via append (default) or replace (Cmd-click / middle-click). Returns
/// nothing — caller refreshes meta and builds the snapshot.
pub(crate) fn do_open_local_branch(
    app: &mut App,
    project_id: &str,
    name: String,
    replace: bool,
) -> Result<(), String> {
    let file = projects::load();
    let proj = file
        .projects
        .iter()
        .find(|p| p.id == project_id)
        .ok_or_else(|| format!("Project not found: {project_id}"))?
        .clone();
    let new_tab = er_engine::app::TabState::new_local_branch(proj.root_path.clone(), name)
        .map_err(|e| e.to_string())?;
    place_tab(app, new_tab, replace);
    Ok(())
}

#[tauri::command]
pub fn open_local_branch(
    project_id: String,
    name: String,
    replace: Option<bool>,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let t = std::time::Instant::now();
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    do_open_local_branch(&mut app, &project_id, name, replace.unwrap_or(false))?;
    log::info!("open_local_branch: branch tab built in {}ms", t.elapsed().as_millis());
    kick_meta_refresh(&*state, app.tab().repo_root.clone());
    kick_active_gh_status(&app, &*state);
    Ok(snap_from(&app, &mut hl, &*state))
}

/// Inner helper: fetch the PR head ref and build a read-only local PR review tab.
/// Never runs `gh pr checkout` or mutates the working tree.
pub(crate) fn do_open_local_pr(
    app: &mut App,
    project_id: &str,
    pr_number: u64,
    replace: bool,
) -> Result<(), String> {
    let file = projects::load();
    let proj = file
        .projects
        .iter()
        .find(|p| p.id == project_id)
        .ok_or_else(|| format!("Project not found: {project_id}"))?
        .clone();
    let new_tab =
        er_engine::app::TabState::new_local_pr(proj.root_path.clone(), pr_number)
            .map_err(|e| e.to_string())?;
    place_tab(app, new_tab, replace);
    Ok(())
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
    let t = std::time::Instant::now();
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    do_open_local_pr(&mut app, &project_id, pr_number, replace.unwrap_or(false))
        .map_err(|e| {
            log::error!("open_pr_review: pr=#{pr_number} project_id={project_id} err={e}");
            e
        })?;
    kick_meta_refresh(&*state, app.tab().repo_root.clone());
    kick_active_gh_status(&app, &*state);
    log::info!(
        "open_pr_review: pr=#{pr_number} opened in {}ms",
        t.elapsed().as_millis()
    );
    Ok(snap_from(&app, &mut hl, &*state))
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
        let loading = Arc::clone(&state.loading);
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to build tokio runtime");
            rt.block_on(async move {
                crate::pr_cache::refresh_pr_cache(&cache).await;
            });
            if let Ok(mut f) = loading.lock() {
                f.pr_list = false;
            }
        });
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
pub fn list_available_branches(
    project_id: String,
) -> Result<Vec<String>, String> {
    let file = projects::load();
    let proj = file
        .projects
        .iter()
        .find(|p| p.id == project_id)
        .ok_or_else(|| format!("Project not found: {project_id}"))?;

    let out = std::process::Command::new("git")
        .args([
            "for-each-ref",
            "--format=%(refname:short)",
            "refs/heads/",
        ])
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
    let new_tab = er_engine::app::TabState::new_local_branch(proj.root_path.clone(), branch)
        .map_err(|e| e.to_string())?;

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
pub fn dismiss_finding(
    finding_id: String,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
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

    app.submit_comment_text(file, hunk_idx, line_start, text, CommentType::GitHubComment, None)
        .map_err(|e| e.to_string())?;

    let new_id: Option<String> = {
        let tab = app.tab();
        tab.ai
            .github_comments
            .as_ref()
            .and_then(|gc| gc.comments.iter().find(|c| !existing_ids.contains(&c.id)).map(|c| c.id.clone()))
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
    let target = {
        let tab = app.tab();
        let mut result: Option<(String, usize, Option<usize>)> = None;
        if let Some(review) = tab.ai.review.as_ref() {
            'outer: for (path, file) in review.files.iter() {
                for f in file.findings.iter() {
                    if f.id == finding_id {
                        result = Some((path.clone(), f.hunk_index.unwrap_or(0), f.line_start));
                        break 'outer;
                    }
                }
            }
        }
        result
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
        )
        .map_err(|e| e.to_string())?;
        let root_id = app
            .tab()
            .ai
            .github_comments
            .as_ref()
            .and_then(|gc| gc.comments.last().map(|c| c.id.clone()))
            .ok_or_else(|| "Failed to create finding comment thread".to_string())?;
        drop(hl);
        drop(app);
        return ask_ai(root_id, prompt, state);
    } else {
        app.submit_comment_text(file, hunk_idx, line_start, body, CommentType::GitHubComment, None)
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
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("Failed to create {dir}: {e}"))?;
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
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("Failed to create {dir}: {e}"))?;
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
    // TODO: trigger the engine's commit-load path. For v1 just move the index
    // when the sha matches a loaded commit.
    {
        let tab = app.tab_mut();
        if let Some(history) = tab.history.as_mut() {
            if let Some(pos) = history.commits.iter().position(|c| c.hash == sha) {
                history.selected_commit = pos;
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
    let bytes: Vec<u8> = input
        .bytes()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
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
fn save_screenshot_bytes(
    comments_dir: &str,
    id: &str,
    bytes: &[u8],
) -> std::io::Result<String> {
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
    const A: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
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
pub fn delete_ui_annotation(
    id: String,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
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
    // Check if .er/ AI files changed — cheap mtime check, reloads AI state if yes
    app.tab_mut().check_ai_files_changed();
    let snapshot = snap_from(&app, &mut hl, &*state);
    if std::env::var("ER_DESKTOP_PROFILE_POLL").as_deref() == Ok("1") {
        eprintln!(
            "er-desktop poll_ms={} files={} threads={}",
            t0.elapsed().as_millis(),
            snapshot.files.len(),
            snapshot.ai.threads.len()
        );
    }
    Ok(PollResponse {
        revision: compute_poll_revision(&app),
        snapshot,
    })
}

fn compute_poll_revision(app: &App) -> u64 {
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
    tab.ai.questions.as_ref().map(|q| q.questions.len()).unwrap_or(0).hash(&mut h);
    tab.ai.github_comments.as_ref().map(|g| g.comments.len()).unwrap_or(0).hash(&mut h);
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

#[cfg(test)]
mod tests {
    use super::*;
    use er_engine::ai::{
        load_ui_annotations, save_ui_annotations, ErGitHubComments, ErQuestions, GitHubReviewComment,
        ReviewQuestion, UiAnnotation,
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
                stale: false,
                context_before: vec![],
                context_after: vec![],
                old_line_start: None,
                hunk_header: String::new(),
                anchor_status: "original".to_string(),
                relocated_at_hash: String::new(),
                finding_ref: None,
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
        let png_bytes: Vec<u8> = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0xDE, 0xAD, 0xBE, 0xEF];

        let path = save_screenshot_bytes(&dir, "ui-test-42", &png_bytes).unwrap();
        assert!(path.ends_with("ui-test-42.png"), "path should be the id.png: {path}");
        let read_back = std::fs::read(&path).unwrap();
        assert_eq!(read_back, png_bytes, "saved bytes must match input");

        // Ensure tmp file was cleaned up by the rename.
        let tmp_path = format!("{path}.tmp");
        assert!(!std::path::Path::new(&tmp_path).exists(), "tmp file must be gone after rename");
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
        write_pkg(
            tmp.path(),
            r#"{ "scripts": { "dev": "vite" } }"#,
        );
        let got = detect_dev_url(tmp.path().to_string_lossy().to_string()).unwrap();
        assert_eq!(got.as_deref(), Some("http://localhost:5173"));
    }

    #[test]
    fn detect_dev_url_next() {
        let tmp = tempfile::tempdir().unwrap();
        write_pkg(
            tmp.path(),
            r#"{ "scripts": { "dev": "next dev" } }"#,
        );
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

    fn make_gh_comment(id: &str, file: &str, line_start: Option<usize>, synced: bool) -> GitHubReviewComment {
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
            stale: false,
            context_before: vec![],
            context_after: vec![],
            old_line_start: None,
            hunk_header: String::new(),
            anchor_status: "original".to_string(),
            relocated_at_hash: String::new(),
            finding_ref: None,
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
                make_gh_comment("c-2", "src/lib.rs", None, false),      // NO anchor, unsynced — the problem
                make_gh_comment("c-3", "src/foo.rs", None, true),       // no anchor but already synced — OK
            ],
        };

        let unsubmittable_count = gc
            .comments
            .iter()
            .filter(|c| c.source == "local" && !c.synced && !c.file.is_empty() && c.line_start.is_none())
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
            .filter(|c| c.source == "local" && !c.synced && !c.file.is_empty() && c.line_start.is_none())
            .count();

        assert_eq!(unsubmittable_count, 0, "no unsubmittable comments when all have line anchors");
    }
}
