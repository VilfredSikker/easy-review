//! Tauri commands for AI Review Arena.

use crate::commands::AppState;
use crate::snapshot::{ArenaRunSnapshotWire, ArenaRunSummaryWire};
use er_engine::app::App;
#[cfg(not(feature = "v1-service"))]
use er_engine::arena::{estimate_batch_cost_usd, ArenaBatchStartParams, ArenaStartParams};
use er_engine::arena::{
    AgentGroupStart, ArenaDiffPreview, ArenaProgressState, ArenaScope, ReviewerRef, Verdict,
};
use serde::Deserialize;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::State;

#[derive(Debug, Deserialize)]
pub struct ArenaStartRequest {
    pub title: Option<String>,
    pub reviewers: Vec<ReviewerRefWire>,
    pub scope: String,
    pub files: Option<Vec<String>>,
    pub rounds: Option<u8>,
    pub arbiter: Option<ReviewerRefWire>,
    pub confirm: Option<bool>,
    pub agent_kind: Option<String>,
    pub effort: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AgentGroupWire {
    pub agent_kind: String,
    pub models: Vec<ReviewerRefWire>,
    pub title: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ArenaBatchStartRequest {
    pub scope: String,
    pub files: Option<Vec<String>>,
    pub rounds: Option<u8>,
    pub arbiter: Option<ReviewerRefWire>,
    pub confirm: Option<bool>,
    pub groups: Vec<AgentGroupWire>,
    pub effort: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ArenaAcceptRequest {
    pub run_id: String,
    pub finding_ids: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, serde::Serialize)]
pub struct ReviewerRefWire {
    pub provider_id: String,
    pub model_id: String,
    #[serde(default)]
    pub agent_kind: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ArenaEstimateRequest {
    pub reviewers: Vec<ReviewerRefWire>,
    pub scope: String,
    pub files: Option<Vec<String>>,
    pub rounds: Option<u8>,
    pub arbiter: Option<ReviewerRefWire>,
}

#[derive(Debug, Deserialize)]
pub struct ArenaBatchEstimateRequest {
    pub scope: String,
    pub files: Option<Vec<String>>,
    pub rounds: Option<u8>,
    pub arbiter: Option<ReviewerRefWire>,
    pub groups: Vec<AgentGroupWire>,
}

pub(crate) fn wire_reviewer(r: &ReviewerRefWire) -> ReviewerRef {
    ReviewerRef {
        provider_id: r.provider_id.clone(),
        model_id: r.model_id.clone(),
        agent_kind: r.agent_kind.clone(),
    }
}

pub(crate) fn wire_reviewers(req: &[ReviewerRefWire]) -> Vec<ReviewerRef> {
    req.iter().map(wire_reviewer).collect()
}

pub(crate) fn wire_groups(groups: &[AgentGroupWire]) -> Vec<AgentGroupStart> {
    groups
        .iter()
        .map(|g| AgentGroupStart {
            agent_kind: g.agent_kind.clone(),
            models: wire_reviewers(&g.models),
            title: g.title.clone(),
        })
        .collect()
}

#[derive(Debug, Deserialize)]
pub struct ArenaOverrideRequest {
    pub run_id: String,
    pub finding_id: String,
    pub verdict: String,
    pub note: String,
}

pub(crate) fn parse_scope(s: &str) -> ArenaScope {
    match s {
        "unstaged" => ArenaScope::Unstaged,
        "staged" => ArenaScope::Staged,
        _ => ArenaScope::Branch,
    }
}

pub(crate) fn parse_verdict(s: &str) -> Verdict {
    match s.to_ascii_lowercase().as_str() {
        "kept" => Verdict::Kept,
        "escalated" => Verdict::Escalated,
        "dropped" => Verdict::Dropped,
        "merged" => Verdict::Merged {
            into: String::new(),
        },
        _ => Verdict::Pending,
    }
}

pub const fn wire_snapshot(snap: er_engine::arena::ArenaRunSnapshot) -> ArenaRunSnapshotWire {
    snap
}

#[tauri::command]
pub fn arena_estimate(
    req: ArenaEstimateRequest,
    state: State<AppState>,
) -> Result<ArenaDiffPreview, String> {
    #[cfg(feature = "v1-service")]
    return crate::arena_service::estimate(&state, req);
    #[cfg(not(feature = "v1-service"))]
    arena_estimate_local(req, state)
}

#[cfg(not(feature = "v1-service"))]
fn arena_estimate_local(
    req: ArenaEstimateRequest,
    state: State<AppState>,
) -> Result<ArenaDiffPreview, String> {
    let reviewers = wire_reviewers(&req.reviewers);
    let scope = parse_scope(&req.scope);
    let app = state.app.lock().map_err(|e| e.to_string())?;
    let arbiter = req.arbiter.as_ref().map(|a| ReviewerRef {
        provider_id: a.provider_id.clone(),
        model_id: a.model_id.clone(),
        agent_kind: a.agent_kind.clone(),
    });
    app.arena_preview(
        scope,
        req.files.as_deref(),
        &reviewers,
        req.rounds,
        arbiter.as_ref(),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn arena_estimate_batch(
    req: ArenaBatchEstimateRequest,
    state: State<AppState>,
) -> Result<ArenaDiffPreview, String> {
    #[cfg(feature = "v1-service")]
    return crate::arena_service::estimate_batch(&state, req);
    #[cfg(not(feature = "v1-service"))]
    arena_estimate_batch_local(req, state)
}

#[cfg(not(feature = "v1-service"))]
fn arena_estimate_batch_local(
    req: ArenaBatchEstimateRequest,
    state: State<AppState>,
) -> Result<ArenaDiffPreview, String> {
    let scope = parse_scope(&req.scope);
    let app = state.app.lock().map_err(|e| e.to_string())?;
    let tab = app.tab();
    let raw_diff = tab
        .raw_diff_for_arena(scope, req.files.as_deref())
        .map_err(|e| e.to_string())?;
    let batch = ArenaBatchStartParams {
        scope,
        files: req.files,
        rounds: req.rounds,
        arbiter: req.arbiter.map(|a| ReviewerRef {
            provider_id: a.provider_id,
            model_id: a.model_id,
            agent_kind: a.agent_kind,
        }),
        confirm: false,
        groups: wire_groups(&req.groups),
        effort: None,
    };
    let cost_usd = estimate_batch_cost_usd(raw_diff.len(), &batch, &app.config.ai_hub);
    Ok(ArenaDiffPreview {
        diff_bytes: raw_diff.len(),
        cost_usd,
        latency_sec: er_engine::arena::estimate_latency_sec(
            &batch
                .groups
                .iter()
                .flat_map(|g| g.models.iter())
                .cloned()
                .collect::<Vec<_>>(),
            req.rounds,
            &app.config.ai_hub,
        ),
        cost_limit_usd: er_engine::arena::DEFAULT_COST_LIMIT_USD,
        max_rounds: er_engine::arena::ARENA_ROUNDS_V1,
    })
}

#[tauri::command]
pub async fn arena_start(
    req: ArenaStartRequest,
    state: State<'_, AppState>,
) -> Result<String, String> {
    #[cfg(feature = "v1-service")]
    return {
        let state = state.inner().clone();
        crate::commands::run_blocking(move || crate::arena_service::start(&state, req)).await
    };
    #[cfg(not(feature = "v1-service"))]
    arena_start_local(req, state).await
}

#[cfg(not(feature = "v1-service"))]
async fn arena_start_local(
    req: ArenaStartRequest,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let state = state.inner().clone();
    // Off the main thread: starting a run does git and file IO under the App
    // lock, and a sync command doing that blocks the one thread that also
    // pumps the window.
    crate::commands::run_blocking(move || {
        crate::dev_log::arena_line(format!(
            "arena_start: reviewers={} scope={} rounds={:?} confirm={}",
            req.reviewers.len(),
            req.scope,
            req.rounds,
            req.confirm.unwrap_or(false)
        ));
        let reviewers = wire_reviewers(&req.reviewers);
        let scope = parse_scope(&req.scope);
        let arbiter = req.arbiter.map(|a| ReviewerRef {
            provider_id: a.provider_id,
            model_id: a.model_id,
            agent_kind: a.agent_kind,
        });
        let params = ArenaStartParams {
            title: req.title,
            reviewers,
            scope,
            files: req.files,
            rounds: req.rounds,
            arbiter,
            confirm: req.confirm.unwrap_or(false),
            agent_kind: req.agent_kind,
            effort: req.effort,
        };
        let run_id = {
            let mut app = state.app.lock().map_err(|e| {
                let msg = e.to_string();
                crate::dev_log::arena_line(format!("arena_start: lock app failed: {msg}"));
                msg
            })?;
            app.arena_start(params).map_err(|e| {
                let msg = e.to_string();
                crate::dev_log::arena_line(format!("arena_start: engine failed: {msg}"));
                msg
            })?
        };
        crate::dev_log::arena_line(format!("arena_start: ok run_id={run_id}"));
        state.desktop_revision.fetch_add(1, Ordering::Relaxed);
        Ok(run_id)
    })
    .await
}

pub(crate) fn validate_review_flow_bucket(app: &App, expected_er_dir: &str) -> Result<(), String> {
    let active_er_dir = app.tab().er_dir();
    if active_er_dir != expected_er_dir {
        return Err(
            "The active review bucket changed. Refresh before controlling this review.".into(),
        );
    }
    Ok(())
}

#[tauri::command]
pub async fn start_review_flow(
    scope: String,
    er_dir: String,
    state: State<'_, AppState>,
) -> Result<crate::snapshot::AppSnapshot, String> {
    let state = state.inner().clone();
    #[cfg(feature = "v1-service")]
    return crate::commands::run_blocking(move || {
        crate::flow_service::start(&state, scope, er_dir)
    })
    .await;
    #[cfg(not(feature = "v1-service"))]
    crate::commands::run_blocking(move || {
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        validate_review_flow_bucket(&app, &er_dir)?;
        let scope = crate::commands::resolve_review_scope(&scope, app.tab())?;
        app.start_review_flow(&scope).map_err(|e| e.to_string())?;
        state.desktop_revision.fetch_add(1, Ordering::Relaxed);
        Ok(crate::commands::snap_from_command(&app, &state))
    })
    .await
}

#[tauri::command]
pub async fn confirm_review_flow(
    er_dir: String,
    state: State<'_, AppState>,
) -> Result<crate::snapshot::AppSnapshot, String> {
    let state = state.inner().clone();
    #[cfg(feature = "v1-service")]
    return crate::commands::run_blocking(move || crate::flow_service::confirm(&state, er_dir))
        .await;
    #[cfg(not(feature = "v1-service"))]
    crate::commands::run_blocking(move || {
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        validate_review_flow_bucket(&app, &er_dir)?;
        app.confirm_review_flow(&er_dir)
            .map_err(|e| e.to_string())?;
        state.desktop_revision.fetch_add(1, Ordering::Relaxed);
        Ok(crate::commands::snap_from_command(&app, &state))
    })
    .await
}

#[tauri::command]
pub async fn decline_review_flow(
    er_dir: String,
    state: State<'_, AppState>,
) -> Result<crate::snapshot::AppSnapshot, String> {
    let state = state.inner().clone();
    #[cfg(feature = "v1-service")]
    return crate::commands::run_blocking(move || crate::flow_service::decline(&state, er_dir))
        .await;
    #[cfg(not(feature = "v1-service"))]
    crate::commands::run_blocking(move || {
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        validate_review_flow_bucket(&app, &er_dir)?;
        app.decline_review_flow(&er_dir)
            .map_err(|e| e.to_string())?;
        state.desktop_revision.fetch_add(1, Ordering::Relaxed);
        Ok(crate::commands::snap_from_command(&app, &state))
    })
    .await
}

#[tauri::command]
pub async fn rerun_review_flow(
    er_dir: String,
    state: State<'_, AppState>,
) -> Result<crate::snapshot::AppSnapshot, String> {
    let state = state.inner().clone();
    #[cfg(feature = "v1-service")]
    return crate::commands::run_blocking(move || crate::flow_service::rerun(&state, er_dir)).await;
    #[cfg(not(feature = "v1-service"))]
    crate::commands::run_blocking(move || {
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        validate_review_flow_bucket(&app, &er_dir)?;
        app.rerun_review_flow(&er_dir).map_err(|e| e.to_string())?;
        state.desktop_revision.fetch_add(1, Ordering::Relaxed);
        Ok(crate::commands::snap_from_command(&app, &state))
    })
    .await
}

#[tauri::command]
pub async fn cancel_review_flow(
    er_dir: String,
    state: State<'_, AppState>,
) -> Result<crate::snapshot::AppSnapshot, String> {
    let state = state.inner().clone();
    #[cfg(feature = "v1-service")]
    return crate::commands::run_blocking(move || crate::flow_service::cancel(&state, er_dir))
        .await;
    #[cfg(not(feature = "v1-service"))]
    crate::commands::run_blocking(move || {
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        validate_review_flow_bucket(&app, &er_dir)?;
        if !app.cancel_review_flow(&er_dir) {
            return Err("There is no running review flow to cancel".into());
        }
        state.desktop_revision.fetch_add(1, Ordering::Relaxed);
        Ok(crate::commands::snap_from_command(&app, &state))
    })
    .await
}

/// Accept selected findings from the arena run captured by a review flow.
/// This intentionally targets the captured bucket rather than the active tab,
/// since the user may have switched tabs before accepting the result.
#[tauri::command]
pub async fn flow_accept_findings(
    er_dir: String,
    run_id: String,
    finding_ids: Vec<String>,
    state: State<'_, AppState>,
) -> Result<crate::snapshot::AppSnapshot, String> {
    let state = state.inner().clone();
    #[cfg(feature = "v1-service")]
    return crate::commands::run_blocking(move || {
        crate::flow_service::accept(&state, er_dir, run_id, finding_ids)
    })
    .await;
    #[cfg(not(feature = "v1-service"))]
    crate::commands::run_blocking(move || {
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        app.accept_review_flow_findings(&er_dir, &run_id, finding_ids)
            .map_err(|e| e.to_string())?;
        state.desktop_revision.fetch_add(1, Ordering::Relaxed);
        Ok(crate::commands::snap_from_command(&app, &state))
    })
    .await
}

/// Fetch a review-flow arena run from its captured bucket. Unlike `arena_get`,
/// this never resolves the run id through the active tab's arena index.
#[tauri::command]
pub async fn flow_get_run(
    er_dir: String,
    run_id: String,
    state: State<'_, AppState>,
) -> Result<ArenaRunSnapshotWire, String> {
    let state = state.inner().clone();
    #[cfg(feature = "v1-service")]
    return crate::commands::run_blocking(move || {
        crate::flow_service::get_run(&state, er_dir, run_id)
    })
    .await;
    #[cfg(not(feature = "v1-service"))]
    crate::commands::run_blocking(move || {
        let app = state.app.lock().map_err(|e| e.to_string())?;
        // Validate both the flow's captured bucket and run id before reading
        // from the explicit path below.
        app.review_flow_results(&er_dir, &run_id)
            .map_err(|e| e.to_string())?;
        let paths = er_engine::arena::ArenaPaths::for_run(std::path::Path::new(&er_dir), &run_id);
        let mut run = er_engine::arena::load_run(&paths).map_err(|e| e.to_string())?;
        if let Some(control) = app.arena_runs.get(&run_id) {
            run.status = control.status();
        }
        Ok(er_engine::arena::build_snapshot_with_config(
            &app.config,
            run,
        ))
    })
    .await
}

/// Validate the current tab's expert findings: dedupe them and rule on the
/// result with one arbiter call.
///
/// Returns the run id, or `None` when there are no expert findings to validate
/// — the UI offers the action only when it would do something. Wrapped in
/// `run_blocking` because it takes the `App` lock and shells out to git on the
/// main thread otherwise.
#[tauri::command]
pub async fn arena_validate_findings(state: State<'_, AppState>) -> Result<Option<String>, String> {
    #[cfg(feature = "v1-service")]
    return {
        let state = state.inner().clone();
        crate::commands::run_blocking(move || crate::arena_service::validate_findings(&state)).await
    };
    #[cfg(not(feature = "v1-service"))]
    arena_validate_findings_local(state).await
}

#[cfg(not(feature = "v1-service"))]
async fn arena_validate_findings_local(
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    let app = state.app.clone();
    let revision = state.desktop_revision.clone();
    crate::commands::run_blocking(move || {
        let run_id = {
            let mut app = app.lock().map_err(|e| e.to_string())?;
            app.arena_start_seeded().map_err(|e| e.to_string())?
        };
        crate::dev_log::arena_line(format!("arena_validate_findings: {run_id:?}"));
        revision.fetch_add(1, Ordering::Relaxed);
        Ok(run_id)
    })
    .await
}

#[tauri::command]
pub async fn arena_start_batch(
    req: ArenaBatchStartRequest,
    state: State<'_, AppState>,
) -> Result<Vec<String>, String> {
    #[cfg(feature = "v1-service")]
    return {
        let state = state.inner().clone();
        crate::commands::run_blocking(move || crate::arena_service::start_batch(&state, req)).await
    };
    #[cfg(not(feature = "v1-service"))]
    arena_start_batch_local(req, state).await
}

#[cfg(not(feature = "v1-service"))]
async fn arena_start_batch_local(
    req: ArenaBatchStartRequest,
    state: State<'_, AppState>,
) -> Result<Vec<String>, String> {
    let state = state.inner().clone();
    // Off the main thread, for the same reason as `arena_start`.
    crate::commands::run_blocking(move || {
        let batch = ArenaBatchStartParams {
            scope: parse_scope(&req.scope),
            files: req.files,
            rounds: req.rounds,
            arbiter: req.arbiter.map(|a| ReviewerRef {
                provider_id: a.provider_id,
                model_id: a.model_id,
                agent_kind: a.agent_kind,
            }),
            confirm: req.confirm.unwrap_or(false),
            groups: wire_groups(&req.groups),
            effort: req.effort,
        };
        let run_ids = {
            let mut app = state.app.lock().map_err(|e| e.to_string())?;
            app.arena_start_batch(batch).map_err(|e| e.to_string())?
        };
        state.desktop_revision.fetch_add(1, Ordering::Relaxed);
        Ok(run_ids)
    })
    .await
}

#[tauri::command]
pub fn arena_accept_findings(
    req: ArenaAcceptRequest,
    state: State<AppState>,
) -> Result<usize, String> {
    #[cfg(feature = "v1-service")]
    return crate::arena_service::accept_findings(&state, req);
    #[cfg(not(feature = "v1-service"))]
    arena_accept_findings_local(req, state)
}

#[cfg(not(feature = "v1-service"))]
fn arena_accept_findings_local(
    req: ArenaAcceptRequest,
    state: State<AppState>,
) -> Result<usize, String> {
    let n = {
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        app.arena_accept_findings(&req.run_id, req.finding_ids)
            .map_err(|e| e.to_string())?
    };
    state.desktop_revision.fetch_add(1, Ordering::Relaxed);
    Ok(n)
}

#[tauri::command]
pub fn arena_progress(
    run_id: String,
    state: State<AppState>,
) -> Result<ArenaProgressState, String> {
    #[cfg(feature = "v1-service")]
    return crate::arena_service::progress(&state, run_id);
    #[cfg(not(feature = "v1-service"))]
    arena_progress_local(run_id, state)
}

#[cfg(not(feature = "v1-service"))]
fn arena_progress_local(
    run_id: String,
    state: State<AppState>,
) -> Result<ArenaProgressState, String> {
    let app = state.app.lock().map_err(|e| e.to_string())?;
    app.arena_progress(&run_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn arena_get(run_id: String, state: State<AppState>) -> Result<ArenaRunSnapshotWire, String> {
    #[cfg(feature = "v1-service")]
    return crate::arena_service::get(&state, run_id);
    #[cfg(not(feature = "v1-service"))]
    arena_get_local(run_id, state)
}

#[cfg(not(feature = "v1-service"))]
fn arena_get_local(run_id: String, state: State<AppState>) -> Result<ArenaRunSnapshotWire, String> {
    let snap = {
        let app = state.app.lock().map_err(|e| e.to_string())?;
        app.arena_get_snapshot(&run_id).map_err(|e| e.to_string())?
    };
    Ok(wire_snapshot(snap))
}

#[tauri::command]
pub fn arena_list(state: State<AppState>) -> Result<Vec<ArenaRunSummaryWire>, String> {
    #[cfg(feature = "v1-service")]
    return crate::arena_service::list(&state);
    #[cfg(not(feature = "v1-service"))]
    arena_list_local(state)
}

#[cfg(not(feature = "v1-service"))]
fn arena_list_local(state: State<AppState>) -> Result<Vec<ArenaRunSummaryWire>, String> {
    let summaries = {
        let app = state.app.lock().map_err(|e| e.to_string())?;
        let branch = app.arena_branch_ref();
        app.arena_list_summaries(Some(&branch))
            .map_err(|e| e.to_string())?
    };
    Ok(summaries)
}

#[tauri::command]
pub fn arena_delete(run_id: String, state: State<AppState>) -> Result<(), String> {
    #[cfg(feature = "v1-service")]
    return crate::arena_service::delete(&state, run_id);
    #[cfg(not(feature = "v1-service"))]
    arena_delete_local(run_id, state)
}

#[cfg(not(feature = "v1-service"))]
fn arena_delete_local(run_id: String, state: State<AppState>) -> Result<(), String> {
    {
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        app.arena_delete(&run_id).map_err(|e| e.to_string())?;
    }
    state.desktop_revision.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
pub fn arena_cancel(run_id: String, state: State<AppState>) -> Result<(), String> {
    #[cfg(feature = "v1-service")]
    return crate::arena_service::cancel(&state, run_id);
    #[cfg(not(feature = "v1-service"))]
    arena_cancel_local(run_id, state)
}

#[cfg(not(feature = "v1-service"))]
fn arena_cancel_local(run_id: String, state: State<AppState>) -> Result<(), String> {
    {
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        app.arena_cancel(&run_id).map_err(|e| e.to_string())?;
    }
    state.desktop_revision.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
pub fn arena_override(
    req: ArenaOverrideRequest,
    state: State<AppState>,
) -> Result<ArenaRunSnapshotWire, String> {
    #[cfg(feature = "v1-service")]
    return crate::arena_service::override_finding(&state, req);
    #[cfg(not(feature = "v1-service"))]
    arena_override_local(req, state)
}

#[cfg(not(feature = "v1-service"))]
fn arena_override_local(
    req: ArenaOverrideRequest,
    state: State<AppState>,
) -> Result<ArenaRunSnapshotWire, String> {
    let snap = {
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        let _finding = app
            .arena_override_finding(
                &req.run_id,
                &req.finding_id,
                parse_verdict(&req.verdict),
                req.note,
            )
            .map_err(|e| e.to_string())?;
        app.arena_get_snapshot(&req.run_id)
            .map_err(|e| e.to_string())?
    };
    state.desktop_revision.fetch_add(1, Ordering::Relaxed);
    Ok(wire_snapshot(snap))
}

/// Re-wire the arena's live-progress notify to desktop revision (once at startup).
pub fn attach_arena_notify(app: &mut App, desktop_revision: Arc<std::sync::atomic::AtomicU64>) {
    let arena_revision = Arc::clone(&desktop_revision);
    app.set_arena_notify(Arc::new(move || {
        arena_revision.fetch_add(1, Ordering::Relaxed);
    }));
    app.set_background_progress_notify(Some(Arc::new(move || {
        desktop_revision.fetch_add(1, Ordering::Relaxed);
    })));
    app.reconcile_arena_runs();
    app.reconcile_workflow_records();
}

#[cfg(test)]
mod review_flow_command_tests {
    use super::*;

    #[test]
    fn review_flow_controls_reject_delayed_click_for_another_bucket() {
        let app = App::new_for_test(Vec::new());
        assert!(validate_review_flow_bucket(&app, "/stale/managed/bucket").is_err());
        let current_bucket = app.tab().er_dir();
        assert!(validate_review_flow_bucket(&app, &current_bucket).is_ok());
    }
}

#[cfg(all(test, feature = "v1-service"))]
mod route_tests {
    /// The text of `fn name(` in `source`, up to its closing brace.
    fn body<'a>(source: &'a str, name: &str) -> &'a str {
        let start = source
            .find(&format!("fn {name}("))
            .unwrap_or_else(|| panic!("{name} is missing"));
        let rest = source.get(start..).unwrap_or_default();
        rest.split("\n}\n").next().unwrap_or(rest)
    }

    /// Each routed arena command sends through the service on the flag path.
    /// The service tests drive the actor; this holds each desktop command to
    /// its route instead of the flag-off body.
    #[test]
    fn the_arena_commands_route_with_the_feature_on() {
        let source = include_str!("arena_commands.rs");
        for (command, route) in [
            ("arena_estimate", "arena_service::estimate("),
            ("arena_estimate_batch", "arena_service::estimate_batch("),
            ("arena_start", "arena_service::start("),
            (
                "arena_validate_findings",
                "arena_service::validate_findings(",
            ),
            ("arena_start_batch", "arena_service::start_batch("),
            ("arena_accept_findings", "arena_service::accept_findings("),
            ("arena_progress", "arena_service::progress("),
            ("arena_get", "arena_service::get("),
            ("arena_list", "arena_service::list("),
            ("arena_delete", "arena_service::delete("),
            ("arena_cancel", "arena_service::cancel("),
            ("arena_override", "arena_service::override_finding("),
        ] {
            assert!(
                body(source, command).contains(route),
                "{command} does not route through the service"
            );
        }
        // A process-wide filter with no workspace state: deliberately unrouted.
        let dev_log = include_str!("direct/dev_log.rs");
        assert!(!body(dev_log, "dev_log_filter").contains("arena_service"));

        let service = include_str!("arena_service.rs");
        for sent in [
            "ArenaCommand::StartRun",
            "ArenaCommand::StartBatch",
            "ArenaCommand::ValidateFindings",
            "ArenaCommand::AcceptFindings",
            "ArenaCommand::Delete",
            "ArenaCommand::Cancel",
            "ArenaCommand::Override",
            "ArenaCommand::Estimate {",
            "ArenaCommand::EstimateBatch",
            // Reads take the published view and the store, never a round trip.
            "arena::read_progress(",
            "arena::read_run(",
            "arena::read_runs(",
        ] {
            assert!(service.contains(sent), "{sent} is never used");
        }
        // The App's recovery must know the actors' runs, or a tab switch marks
        // them stale.
        assert!(include_str!("main.rs").contains("arena_service::attach("));
    }

    /// Every review-flow command needs a flag-path route, or the flag path
    /// silently runs the flow in the App the actor never sees.
    #[test]
    fn the_flow_commands_route_with_the_feature_on() {
        let source = include_str!("arena_commands.rs");
        for (command, route) in [
            ("start_review_flow", "flow_service::start("),
            ("confirm_review_flow", "flow_service::confirm("),
            ("decline_review_flow", "flow_service::decline("),
            ("rerun_review_flow", "flow_service::rerun("),
            ("cancel_review_flow", "flow_service::cancel("),
            ("flow_accept_findings", "flow_service::accept("),
            ("flow_get_run", "flow_service::get_run("),
        ] {
            assert!(
                body(source, command).contains(route),
                "{command} does not route through the service"
            );
        }
        assert!(
            body(include_str!("commands.rs"), "run_standard_review_flow")
                .contains("flow_service::run_standard("),
            "run_standard_review_flow does not route through the service"
        );

        let service = include_str!("flow_service.rs");
        for sent in [
            "FlowCommand::Start {",
            "FlowCommand::Confirm {",
            "FlowCommand::Decline {",
            "FlowCommand::Rerun {",
            "FlowCommand::Cancel {",
            "FlowCommand::RunStandard {",
            "FlowCommand::AcceptFindings {",
            // Acceptance hashes a fresh diff of the tab that shows the run's
            // target, never the active view's cached one.
            "<App as ReviewFlowHost>::current_review_diff(app, target)",
            // A run read takes the published view and the store.
            "er_service::read_flow_run(",
        ] {
            assert!(service.contains(sent), "{sent} is never used");
        }
        // A run is read and accepted in the actor that runs its flow, which
        // the active tab names only while it shows that flow's project.
        assert!(body(service, "route").contains("flow_key(&state.service, &app, er_dir)"));
        let accept = body(service, "accept");
        assert!(accept.contains("route(state, &er_dir.clone(), false,"));
        assert!(
            !accept.contains("key(app)"),
            "accept reads the active tab's actor"
        );
        let get_run = body(service, "get_run");
        assert!(get_run.contains("flow_key(&state.service, &app, &er_dir)"));
        assert!(
            !get_run.contains("key(&app)"),
            "get_run reads the active tab's actor"
        );
        // A load recovers the flows in the actors, not only in the App.
        assert!(include_str!("main.rs").contains("flow_service::reconcile_on_load("));
    }
}
