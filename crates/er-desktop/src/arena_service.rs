//! The arena commands on the flag path. Each reads the tab and the App's
//! settings here, where they live, and sends the command to the workspace
//! actor, which runs the arena and its jobs. Reads never wait on the actor
//! (ADR 0067): progress and live status come from its published view, and run
//! records from the store.
//!
//! Only compiled with `v1-service`.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use er_engine::app::App;
use er_engine::arena::{
    ArenaBatchStartParams, ArenaDiffPreview, ArenaProgressState, ArenaStartParams,
};
use er_service::{ArenaCommand, ArenaContext, Command, Reply, WorkspaceView};

use crate::arena_commands::{
    parse_scope, parse_verdict, wire_groups, wire_reviewer, wire_reviewers, ArenaAcceptRequest,
    ArenaBatchEstimateRequest, ArenaBatchStartRequest, ArenaEstimateRequest, ArenaOverrideRequest,
    ArenaStartRequest,
};
use crate::commands::{service_key, tab_context, AppState};
use crate::snapshot::{ArenaRunSnapshotWire, ArenaRunSummaryWire};

fn arena_context(app: &App) -> ArenaContext {
    ArenaContext {
        tab: tab_context(app),
        branch_ref: app.arena_branch_ref(),
        config: Arc::new(app.config.clone()),
        buckets: app.tabs.iter().map(|tab| tab.er_dir()).collect(),
        review_hash: app
            .tab()
            .ai
            .review
            .as_ref()
            .map(|review| review.diff_hash.clone())
            .unwrap_or_default(),
    }
}

/// The run's effort, resolved against the App's current pick the way the
/// flag-off start does.
fn resolve_effort(app: &App, requested: Option<&str>) -> Option<String> {
    er_engine::config::resolve_effort(
        &app.config.ai_hub,
        &app.config.agent,
        app.current_ai_effort.as_deref(),
        requested,
    )
}

/// The actor key and the arena context, read under the App lock. `prepare`
/// sees the App too, for the inputs only it holds.
fn resolve<T>(
    state: &AppState,
    prepare: impl FnOnce(&App, ArenaContext) -> T,
) -> Result<(String, T), String> {
    let app = state.app.lock().map_err(|error| error.to_string())?;
    let arena = arena_context(&app);
    let key = service_key(
        &arena.tab.repo_root,
        arena.tab.remote_repo.as_deref().unwrap_or(""),
    );
    Ok((key, prepare(&app, arena)))
}

/// Sends one arena command and waits for the actor's bounded reply.
fn ask(
    state: &AppState,
    build: impl FnOnce(&App, ArenaContext) -> ArenaCommand,
) -> Result<Reply, String> {
    let (key, command) = resolve(state, build)?;
    state
        .service
        .send(&key, Command::Arena(command))
        .map_err(|error| error.to_string())
}

/// Sends one arena command that changes a run and bumps the desktop revision,
/// as the flag-off commands do, so the front end re-reads.
fn send(
    state: &AppState,
    build: impl FnOnce(&App, ArenaContext) -> ArenaCommand,
) -> Result<Reply, String> {
    let reply = ask(state, build)?;
    state.desktop_revision.fetch_add(1, Ordering::Relaxed);
    Ok(reply)
}

/// The actor's published view and the arena context, for a read. The App
/// lock is released before the read touches the store.
fn read_context(state: &AppState) -> Result<(Arc<WorkspaceView>, ArenaContext), String> {
    let (key, arena) = resolve(state, |_, arena| arena)?;
    Ok((state.service.view(&key), arena))
}

fn unexpected(answer: impl std::fmt::Debug) -> String {
    format!("unexpected answer from the workspace: {answer:?}")
}

fn estimated(reply: Reply) -> Result<ArenaDiffPreview, String> {
    match reply {
        Reply::ArenaEstimate(preview) => Ok(preview),
        other => Err(unexpected(other)),
    }
}

pub fn estimate(state: &AppState, req: ArenaEstimateRequest) -> Result<ArenaDiffPreview, String> {
    estimated(ask(state, |_, arena| ArenaCommand::Estimate {
        arena,
        scope: parse_scope(&req.scope),
        files: req.files,
        reviewers: wire_reviewers(&req.reviewers),
        rounds: req.rounds,
        arbiter: req.arbiter.as_ref().map(wire_reviewer),
    })?)
}

pub fn estimate_batch(
    state: &AppState,
    req: ArenaBatchEstimateRequest,
) -> Result<ArenaDiffPreview, String> {
    let params = ArenaBatchStartParams {
        scope: parse_scope(&req.scope),
        files: req.files,
        rounds: req.rounds,
        arbiter: req.arbiter.as_ref().map(wire_reviewer),
        confirm: false,
        groups: wire_groups(&req.groups),
        effort: None,
    };
    estimated(ask(state, |_, arena| ArenaCommand::EstimateBatch {
        arena,
        params,
    })?)
}

pub fn start(state: &AppState, req: ArenaStartRequest) -> Result<String, String> {
    let reply = send(state, |app, arena| ArenaCommand::StartRun {
        arena,
        params: ArenaStartParams {
            title: req.title,
            reviewers: wire_reviewers(&req.reviewers),
            scope: parse_scope(&req.scope),
            files: req.files,
            rounds: req.rounds,
            arbiter: req.arbiter.as_ref().map(wire_reviewer),
            confirm: req.confirm.unwrap_or(false),
            agent_kind: req.agent_kind,
            effort: resolve_effort(app, req.effort.as_deref()),
        },
    })?;
    match reply {
        Reply::ArenaStarted { mut run_ids } if run_ids.len() == 1 => Ok(run_ids.remove(0)),
        other => Err(unexpected(other)),
    }
}

pub fn start_batch(state: &AppState, req: ArenaBatchStartRequest) -> Result<Vec<String>, String> {
    let reply = send(state, |app, arena| ArenaCommand::StartBatch {
        arena,
        params: ArenaBatchStartParams {
            scope: parse_scope(&req.scope),
            files: req.files,
            rounds: req.rounds,
            arbiter: req.arbiter.as_ref().map(wire_reviewer),
            confirm: req.confirm.unwrap_or(false),
            groups: wire_groups(&req.groups),
            effort: resolve_effort(app, req.effort.as_deref()),
        },
    })?;
    match reply {
        Reply::ArenaStarted { run_ids } => Ok(run_ids),
        other => Err(unexpected(other)),
    }
}

pub fn validate_findings(state: &AppState) -> Result<Option<String>, String> {
    match send(state, |_, arena| ArenaCommand::ValidateFindings { arena })? {
        Reply::ArenaValidating { run_id } => Ok(run_id),
        other => Err(unexpected(other)),
    }
}

pub fn accept_findings(state: &AppState, req: ArenaAcceptRequest) -> Result<usize, String> {
    let reply = send(state, |_, arena| ArenaCommand::AcceptFindings {
        arena,
        run_id: req.run_id,
        finding_ids: req.finding_ids,
    })?;
    match reply {
        Reply::ArenaAccepted { accepted } => Ok(accepted),
        other => Err(unexpected(other)),
    }
}

pub fn progress(state: &AppState, run_id: String) -> Result<ArenaProgressState, String> {
    let (view, _) = read_context(state)?;
    Ok(er_service::arena::read_progress(&view, &run_id))
}

pub fn get(state: &AppState, run_id: String) -> Result<ArenaRunSnapshotWire, String> {
    let (view, arena) = read_context(state)?;
    er_service::arena::read_run(&view, &arena, &run_id).map_err(|error| error.to_string())
}

pub fn list(state: &AppState) -> Result<Vec<ArenaRunSummaryWire>, String> {
    let (_, arena) = read_context(state)?;
    er_service::arena::read_runs(&arena).map_err(|error| error.to_string())
}

pub fn delete(state: &AppState, run_id: String) -> Result<(), String> {
    send(state, |_, arena| ArenaCommand::Delete { arena, run_id }).map(|_| ())
}

pub fn cancel(state: &AppState, run_id: String) -> Result<(), String> {
    send(state, |_, _| ArenaCommand::Cancel { run_id }).map(|_| ())
}

/// Records the verdict, then reads the run back for the overlay, as the
/// flag-off command answers with the updated snapshot.
pub fn override_finding(
    state: &AppState,
    req: ArenaOverrideRequest,
) -> Result<ArenaRunSnapshotWire, String> {
    let run_id = req.run_id.clone();
    send(state, |_, arena| ArenaCommand::Override {
        arena,
        run_id: req.run_id,
        finding_id: req.finding_id,
        verdict: parse_verdict(&req.verdict),
        note: req.note,
    })?;
    get(state, run_id)
}

/// Tells the App's arena recovery which runs the actors own. Without it a tab
/// switch marks an actor's running run stale, since the App's registry never
/// holds it.
pub fn attach(app: &App, service: Arc<crate::service::ServiceState>) {
    app.set_arena_live_elsewhere(Arc::new(move |run_id| service.arena_run_live(run_id)));
}
