//! Tauri commands for AI Review Arena (S8).

use crate::snapshot::{ArenaRunSnapshotWire, ArenaRunSummaryWire};
use crate::commands::AppState;
use er_engine::arena::{
    ArenaScope, ArenaStartParams, ReviewerRef, Verdict,
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
    pub confirm: Option<bool>,
}

#[derive(Debug, Deserialize, serde::Serialize)]
pub struct ReviewerRefWire {
    pub provider_id: String,
    pub model_id: String,
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
        "merged" => Verdict::Merged { into: String::new() },
        _ => Verdict::Pending,
    }
}

pub fn wire_snapshot(snap: er_engine::arena::ArenaRunSnapshot) -> ArenaRunSnapshotWire {
    snap
}

#[tauri::command]
pub fn arena_start(req: ArenaStartRequest, state: State<AppState>) -> Result<String, String> {
    eprintln!(
        "[er-arena] arena_start: reviewers={} scope={} rounds={:?} confirm={}",
        req.reviewers.len(),
        req.scope,
        req.rounds,
        req.confirm.unwrap_or(false)
    );
    let reviewers: Vec<ReviewerRef> = req
        .reviewers
        .into_iter()
        .map(|r| ReviewerRef {
            provider_id: r.provider_id,
            model_id: r.model_id,
        })
        .collect();
    let scope = parse_scope(&req.scope);
    let params = ArenaStartParams {
        title: req.title,
        reviewers,
        scope,
        files: req.files,
        rounds: req.rounds,
        confirm: req.confirm.unwrap_or(false),
    };
    let run_id = {
        let mut app = state.app.lock().map_err(|e| {
            let msg = e.to_string();
            eprintln!("[er-arena] arena_start: lock app failed: {msg}");
            msg
        })?;
        app.arena_start(params).map_err(|e| {
            let msg = e.to_string();
            eprintln!("[er-arena] arena_start: engine failed: {msg}");
            msg
        })?
    };
    eprintln!("[er-arena] arena_start: ok run_id={run_id}");
    state.desktop_revision.fetch_add(1, Ordering::Relaxed);
    Ok(run_id)
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
        app.arena_list_summaries().map_err(|e| e.to_string())?
    };
    Ok(summaries)
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

/// Re-wire arena registry notify to desktop revision (call once at startup).
pub fn attach_arena_notify(app: &mut App, desktop_revision: Arc<std::sync::atomic::AtomicU64>) {
    app.arena_registry = App::init_arena_registry(Arc::new(move || {
        desktop_revision.fetch_add(1, Ordering::Relaxed);
    }));
    app.reconcile_arena_runs();
}
