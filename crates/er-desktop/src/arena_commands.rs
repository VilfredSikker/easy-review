//! Tauri commands for AI Review Arena.

use crate::commands::AppState;
use crate::snapshot::{ArenaRunSnapshotWire, ArenaRunSummaryWire};
use er_engine::app::App;
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
    return on_active(&state, |desktop, tab| {
        crate::arena_service::estimate(desktop, tab, &estimate_plan(req))
    });
}

#[tauri::command]
pub fn arena_estimate_batch(
    req: ArenaBatchEstimateRequest,
    state: State<AppState>,
) -> Result<ArenaDiffPreview, String> {
    return on_active(&state, |desktop, tab| {
        crate::arena_service::estimate_batch(desktop, tab, &estimate_batch_params(req))
    });
}

#[tauri::command]
pub async fn arena_start(
    req: ArenaStartRequest,
    state: State<'_, AppState>,
) -> Result<String, String> {
    return {
        let state = state.inner().clone();
        crate::commands::run_blocking(move || {
            on_active(&state, |desktop, tab| {
                crate::arena_service::start(desktop, tab, start_params(req))
            })
        })
        .await
    };
}

/// The flag path checks the painted bucket against the named tab's content
/// in `flow_service`.
#[cfg(test)]
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
    return crate::commands::run_blocking(move || {
        flow_on_active(&state, er_dir, crate::flow_service::Control::Start(scope))
    })
    .await;
}

#[tauri::command]
pub async fn confirm_review_flow(
    er_dir: String,
    state: State<'_, AppState>,
) -> Result<crate::snapshot::AppSnapshot, String> {
    let state = state.inner().clone();
    return crate::commands::run_blocking(move || {
        flow_on_active(&state, er_dir, crate::flow_service::Control::Confirm)
    })
    .await;
}

#[tauri::command]
pub async fn decline_review_flow(
    er_dir: String,
    state: State<'_, AppState>,
) -> Result<crate::snapshot::AppSnapshot, String> {
    let state = state.inner().clone();
    return crate::commands::run_blocking(move || {
        flow_on_active(&state, er_dir, crate::flow_service::Control::Decline)
    })
    .await;
}

#[tauri::command]
pub async fn rerun_review_flow(
    er_dir: String,
    state: State<'_, AppState>,
) -> Result<crate::snapshot::AppSnapshot, String> {
    let state = state.inner().clone();
    return crate::commands::run_blocking(move || {
        flow_on_active(&state, er_dir, crate::flow_service::Control::Rerun)
    })
    .await;
}

#[tauri::command]
pub async fn cancel_review_flow(
    er_dir: String,
    state: State<'_, AppState>,
) -> Result<crate::snapshot::AppSnapshot, String> {
    let state = state.inner().clone();
    return crate::commands::run_blocking(move || {
        flow_on_active(&state, er_dir, crate::flow_service::Control::Cancel)
    })
    .await;
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
    return crate::commands::run_blocking(move || {
        let bucket = er_dir.clone();
        crate::flow_service::entry(&state.desktop, None)
            .and_then(|active| {
                crate::flow_service::accept(&state.desktop, &active, er_dir, run_id, finding_ids)
            })
            .map_err(|error| error.message)?;
        // The App's tabs on the bucket show what was accepted until P62.2
        // retires them; the service's content reloads on its own tick.
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        for tab in app.tabs.iter_mut().filter(|tab| tab.er_dir() == bucket) {
            tab.reload_ai_state();
        }
        Ok(crate::commands::snap_from_command(&app, &state))
    })
    .await;
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
    return crate::commands::run_blocking(move || {
        crate::flow_service::entry(&state.desktop, None)
            .and_then(|active| {
                crate::flow_service::get_run(&state.desktop, &active, &er_dir, &run_id)
            })
            .map_err(|error| error.message)
    })
    .await;
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
    return {
        let state = state.inner().clone();
        crate::commands::run_blocking(move || {
            on_active(&state, |desktop, tab| {
                crate::arena_service::validate_findings(desktop, tab)
            })
        })
        .await
    };
}

#[tauri::command]
pub async fn arena_start_batch(
    req: ArenaBatchStartRequest,
    state: State<'_, AppState>,
) -> Result<Vec<String>, String> {
    return {
        let state = state.inner().clone();
        crate::commands::run_blocking(move || {
            on_active(&state, |desktop, tab| {
                crate::arena_service::start_batch(desktop, tab, start_batch_params(req))
            })
        })
        .await
    };
}

#[tauri::command]
pub fn arena_accept_findings(
    req: ArenaAcceptRequest,
    state: State<AppState>,
) -> Result<usize, String> {
    return on_active(&state, |desktop, tab| {
        crate::arena_service::accept_findings(desktop, tab, req.run_id, req.finding_ids)
    });
}

#[tauri::command]
pub fn arena_progress(
    run_id: String,
    state: State<AppState>,
) -> Result<ArenaProgressState, String> {
    return on_active(&state, |desktop, tab| {
        Ok(crate::arena_service::progress(desktop, tab, &run_id))
    });
}

#[tauri::command]
pub fn arena_get(run_id: String, state: State<AppState>) -> Result<ArenaRunSnapshotWire, String> {
    return on_active(&state, |desktop, tab| {
        crate::arena_service::get(desktop, tab, &run_id)
    });
}

#[tauri::command]
pub fn arena_list(state: State<AppState>) -> Result<Vec<ArenaRunSummaryWire>, String> {
    return on_active(&state, |desktop, tab| {
        crate::arena_service::list(desktop, tab)
    });
}

#[tauri::command]
pub fn arena_delete(run_id: String, state: State<AppState>) -> Result<(), String> {
    return on_active(&state, |desktop, tab| {
        crate::arena_service::delete(desktop, tab, run_id)
    });
}

#[tauri::command]
pub fn arena_cancel(run_id: String, state: State<AppState>) -> Result<(), String> {
    return on_active(&state, |desktop, tab| {
        crate::arena_service::cancel(desktop, tab, run_id)
    });
}

#[tauri::command]
pub fn arena_override(
    req: ArenaOverrideRequest,
    state: State<AppState>,
) -> Result<ArenaRunSnapshotWire, String> {
    return on_active(&state, |desktop, tab| {
        let run_id = req.run_id.clone();
        let verdict = parse_verdict(&req.verdict);
        crate::arena_service::override_finding(
            desktop,
            tab,
            req.run_id,
            req.finding_id,
            verdict,
            req.note,
        )?;
        crate::arena_service::get(desktop, tab, &run_id)
    });
}

/// Runs a flag-on flow control on the registry's active tab with no App
/// lock, then answers the App's snapshot, as the flag-off command does.
pub(crate) fn flow_on_active(
    state: &AppState,
    er_dir: String,
    control: crate::flow_service::Control,
) -> Result<crate::snapshot::AppSnapshot, String> {
    on_active(state, |desktop, tab| {
        crate::flow_service::run(desktop, tab, er_dir, control)
    })?;
    let app = state.app.lock().map_err(|e| e.to_string())?;
    Ok(crate::commands::snap_from_command(&app, state))
}

/// Runs a flag-on arena command on the registry's active tab, the tab the
/// page shows, with no App lock.
fn on_active<T>(
    desktop: &crate::state::DesktopState,
    run: impl FnOnce(
        &crate::state::DesktopState,
        &crate::arena_service::ArenaTab,
    ) -> Result<T, er_protocol::ErError>,
) -> Result<T, String> {
    crate::arena_service::tab(desktop, None)
        .and_then(|tab| run(desktop, &tab))
        .map_err(|error| error.message)
}

fn estimate_plan(req: ArenaEstimateRequest) -> crate::arena_service::Plan {
    crate::arena_service::Plan {
        scope: parse_scope(&req.scope),
        files: req.files,
        reviewers: wire_reviewers(&req.reviewers),
        rounds: req.rounds,
        arbiter: req.arbiter.as_ref().map(wire_reviewer),
    }
}

fn estimate_batch_params(
    req: ArenaBatchEstimateRequest,
) -> er_engine::arena::ArenaBatchStartParams {
    er_engine::arena::ArenaBatchStartParams {
        scope: parse_scope(&req.scope),
        files: req.files,
        rounds: req.rounds,
        arbiter: req.arbiter.as_ref().map(wire_reviewer),
        confirm: false,
        groups: wire_groups(&req.groups),
        effort: None,
    }
}

fn start_params(req: ArenaStartRequest) -> er_engine::arena::ArenaStartParams {
    er_engine::arena::ArenaStartParams {
        title: req.title,
        reviewers: wire_reviewers(&req.reviewers),
        scope: parse_scope(&req.scope),
        files: req.files,
        rounds: req.rounds,
        arbiter: req.arbiter.as_ref().map(wire_reviewer),
        confirm: req.confirm.unwrap_or(false),
        agent_kind: req.agent_kind,
        effort: req.effort,
    }
}

fn start_batch_params(req: ArenaBatchStartRequest) -> er_engine::arena::ArenaBatchStartParams {
    er_engine::arena::ArenaBatchStartParams {
        scope: parse_scope(&req.scope),
        files: req.files,
        rounds: req.rounds,
        arbiter: req.arbiter.as_ref().map(wire_reviewer),
        confirm: req.confirm.unwrap_or(false),
        groups: wire_groups(&req.groups),
        effort: req.effort,
    }
}

/// Tells the App's arena recovery which runs the actors own. Without it a tab
/// switch marks an actor's running run stale, since the App's registry never
/// holds it.
pub fn attach_live_runs(app: &App, service: Arc<crate::service::ServiceState>) {
    app.set_arena_live_elsewhere(Arc::new(move |run_id| service.arena_run_live(run_id)));
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

#[cfg(test)]
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
            // An estimate reads the diff off the actor, as every read does.
            "arena::estimate(",
            "arena::estimate_batch(",
            // Reads take the published view and the store, never a round trip.
            "arena::read_progress(",
            "arena::read_run(",
            "arena::read_runs(",
        ] {
            assert!(service.contains(sent), "{sent} is never used");
        }
        // The App's recovery must know the actors' runs, or a tab switch marks
        // them stale.
        assert!(include_str!("main.rs").contains("arena_commands::attach_live_runs("));
    }

    /// Every review-flow command needs a flag-path route, or the flag path
    /// silently runs the flow in the App the actor never sees.
    #[test]
    fn the_flow_commands_route_with_the_feature_on() {
        let source = include_str!("arena_commands.rs");
        for (command, route) in [
            ("start_review_flow", "flow_service::Control::Start("),
            ("confirm_review_flow", "flow_service::Control::Confirm"),
            ("decline_review_flow", "flow_service::Control::Decline"),
            ("rerun_review_flow", "flow_service::Control::Rerun"),
            ("cancel_review_flow", "flow_service::Control::Cancel"),
            ("flow_accept_findings", "flow_service::accept("),
            ("flow_get_run", "flow_service::get_run("),
        ] {
            assert!(
                body(source, command).contains(route),
                "{command} does not route through the service"
            );
        }
        assert!(body(source, "flow_on_active").contains("flow_service::run("));
        assert!(
            body(include_str!("commands.rs"), "run_standard_review_flow")
                .contains("flow_service::Control::RunStandard"),
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
            "er_service::current_review_diff(",
            // A run read takes the published view and the store.
            "er_service::read_flow_run(",
        ] {
            assert!(service.contains(sent), "{sent} is never used");
        }
        // A run is read and accepted in the actor that runs its flow, which
        // the active tab names only while it shows that flow's project.
        let accept = body(service, "accept");
        assert!(accept.contains("flow_key(desktop, &er_dir, fallback)"));
        assert!(
            !accept.contains("tab_key(&tab.entry)"),
            "accept reads the named tab's actor"
        );
        assert!(body(service, "get_run").contains("flow_key(desktop, er_dir, fallback)"));
        // A load recovers the flows in the actors, not only in the App.
        assert!(include_str!("main.rs").contains("flow_service::reconcile_on_load("));
    }
}
