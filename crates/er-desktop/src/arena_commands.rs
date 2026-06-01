//! Tauri commands for AI Review Arena (S8).

use crate::snapshot::{ArenaRunSnapshotWire, ArenaRunSummaryWire};
use crate::commands::AppState;
use er_engine::arena::{
    estimate_batch_cost_usd, AgentGroupStart, ArenaBatchStartParams, ArenaDiffPreview,
    ArenaProgressState, ArenaScope, ArenaStartParams, ReviewerRef, Verdict,
};
use er_engine::app::App;
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

fn wire_reviewers(req: &[ReviewerRefWire]) -> Vec<ReviewerRef> {
    req.iter()
        .map(|r| ReviewerRef {
            provider_id: r.provider_id.clone(),
            model_id: r.model_id.clone(),
            agent_kind: r.agent_kind.clone(),
        })
        .collect()
}

fn wire_groups(groups: &[AgentGroupWire]) -> Vec<AgentGroupStart> {
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

fn parse_scope(s: &str) -> ArenaScope {
    match s {
        "unstaged" => ArenaScope::Unstaged,
        "staged" => ArenaScope::Staged,
        _ => ArenaScope::Branch,
    }
}

fn parse_verdict(s: &str) -> Verdict {
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

pub fn wire_snapshot(snap: er_engine::arena::ArenaRunSnapshot) -> ArenaRunSnapshotWire {
    snap
}

#[tauri::command]
pub fn arena_estimate(
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
    app.arena_preview(scope, req.files.as_deref(), &reviewers, req.rounds, arbiter.as_ref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn arena_estimate_batch(
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
    })
}

#[tauri::command]
pub fn arena_start(req: ArenaStartRequest, state: State<AppState>) -> Result<String, String> {
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
}

#[tauri::command]
pub fn arena_start_batch(
    req: ArenaBatchStartRequest,
    state: State<AppState>,
) -> Result<Vec<String>, String> {
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
}

#[tauri::command]
pub fn arena_accept_findings(
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
pub fn arena_progress(run_id: String, state: State<AppState>) -> Result<ArenaProgressState, String> {
    let app = state.app.lock().map_err(|e| e.to_string())?;
    app.arena_progress(&run_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn arena_get(run_id: String, state: State<AppState>) -> Result<ArenaRunSnapshotWire, String> {
    let snap = {
        let app = state.app.lock().map_err(|e| e.to_string())?;
        app.arena_get_snapshot(&run_id).map_err(|e| e.to_string())?
    };
    Ok(wire_snapshot(snap))
}

#[tauri::command]
pub fn arena_list(state: State<AppState>) -> Result<Vec<ArenaRunSummaryWire>, String> {
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
    {
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        app.arena_delete(&run_id).map_err(|e| e.to_string())?;
    }
    state.desktop_revision.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
pub fn arena_cancel(run_id: String, state: State<AppState>) -> Result<(), String> {
    {
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        app.arena_cancel(&run_id).map_err(|e| e.to_string())?;
    }
    state.desktop_revision.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
pub fn arena_override(req: ArenaOverrideRequest, state: State<AppState>) -> Result<ArenaRunSnapshotWire, String> {
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
        app.arena_get_snapshot(&req.run_id).map_err(|e| e.to_string())?
    };
    state.desktop_revision.fetch_add(1, Ordering::Relaxed);
    Ok(wire_snapshot(snap))
}

#[tauri::command]
pub fn dev_log_filter() -> Option<Vec<String>> {
    crate::dev_log::filter_groups()
}

/// Re-wire arena registry notify to desktop revision (call once at startup).
pub fn attach_arena_notify(app: &mut App, desktop_revision: Arc<std::sync::atomic::AtomicU64>) {
    app.arena_registry = App::init_arena_registry(Arc::new(move || {
        desktop_revision.fetch_add(1, Ordering::Relaxed);
    }));
    app.reconcile_arena_runs();
}
