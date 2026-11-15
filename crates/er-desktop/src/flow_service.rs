//! The review-flow commands on the flag path. Each reads the tab, the App's
//! settings and the view a flow would review here, where they live, by the
//! App's own rules, then sends the command to the workspace actor, which runs
//! the flow and its stages. A run read takes the published view and the store
//! and never waits on the actor (ADR 0067).
//!
//! Only compiled with `v1-service`.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use er_engine::app::App;
use er_engine::review_flow_driver::ReviewFlowHost;
use er_service::{Command, FlowCommand, FlowContext, Reply};

use crate::arena_commands::validate_review_flow_bucket;
use crate::commands::{service_key, snap_from_command, AppState};
use crate::snapshot::{AppSnapshot, ArenaRunSnapshotWire};

/// The live settings, and the view `scope` names when the command reviews or
/// accepts against one.
fn context(app: &App, scope: Option<&str>) -> FlowContext {
    FlowContext {
        config: Arc::new(app.config.clone()),
        effort: app.current_ai_effort.clone(),
        view: match scope {
            Some(scope) => app
                .review_flow_inputs(scope)
                .map_err(|error| error.to_string()),
            None => Err("this command reviews no view".into()),
        },
    }
}

fn key(app: &App) -> String {
    let tab = app.tab();
    service_key(&tab.repo_root, tab.remote_repo.as_deref().unwrap_or(""))
}

/// Builds the command under the App lock, sends it once the lock is released,
/// and answers with the App's snapshot after bumping the desktop revision, as
/// the flag-off commands do.
fn route(
    state: &AppState,
    er_dir: &str,
    check_bucket: bool,
    build: impl FnOnce(&App) -> Result<FlowCommand, String>,
) -> Result<(Reply, AppSnapshot), String> {
    let (key, command) = {
        let app = state.app.lock().map_err(|e| e.to_string())?;
        if check_bucket {
            validate_review_flow_bucket(&app, er_dir)?;
        }
        (key(&app), build(&app)?)
    };
    let reply = state
        .service
        .send(&key, Command::Flow(command))
        .map_err(|error| error.to_string())?;
    state.desktop_revision.fetch_add(1, Ordering::Relaxed);
    let app = state.app.lock().map_err(|e| e.to_string())?;
    Ok((reply, snap_from_command(&app, state)))
}

pub fn start(state: &AppState, scope: String, er_dir: String) -> Result<AppSnapshot, String> {
    route(state, &er_dir, true, |app| {
        let scope = crate::commands::resolve_review_scope(&scope, app.tab())?;
        Ok(FlowCommand::Start {
            context: context(app, Some(&scope)),
            scope,
        })
    })
    .map(|(_, snapshot)| snapshot)
}

pub fn confirm(state: &AppState, er_dir: String) -> Result<AppSnapshot, String> {
    route(state, &er_dir.clone(), true, |app| {
        Ok(FlowCommand::Confirm {
            context: context(app, None),
            er_dir,
        })
    })
    .map(|(_, snapshot)| snapshot)
}

pub fn decline(state: &AppState, er_dir: String) -> Result<AppSnapshot, String> {
    route(state, &er_dir.clone(), true, |_| {
        Ok(FlowCommand::Decline { er_dir })
    })
    .map(|(_, snapshot)| snapshot)
}

/// A re-run reviews the branch view, as the driver's resume does.
pub fn rerun(state: &AppState, er_dir: String) -> Result<AppSnapshot, String> {
    route(state, &er_dir.clone(), true, |app| {
        Ok(FlowCommand::Rerun {
            context: context(app, Some("branch")),
            er_dir,
        })
    })
    .map(|(_, snapshot)| snapshot)
}

pub fn cancel(state: &AppState, er_dir: String) -> Result<AppSnapshot, String> {
    route(state, &er_dir.clone(), true, |_| {
        Ok(FlowCommand::Cancel { er_dir })
    })
    .map(|(_, snapshot)| snapshot)
}

pub fn run_standard(state: &AppState, er_dir: String) -> Result<AppSnapshot, String> {
    route(state, &er_dir.clone(), true, |app| {
        Ok(FlowCommand::RunStandard {
            context: context(app, None),
            er_dir,
        })
    })
    .map(|(_, snapshot)| snapshot)
}

/// Acceptance targets the run's captured bucket, not the active tab; the actor
/// matches that capture against the active view this sends. The App's tabs on
/// the bucket reload what was accepted.
pub fn accept(
    state: &AppState,
    er_dir: String,
    run_id: String,
    finding_ids: Vec<String>,
) -> Result<AppSnapshot, String> {
    let bucket = er_dir.clone();
    route(state, &er_dir.clone(), false, |app| {
        let context = match crate::commands::resolve_review_scope("current", app.tab()) {
            Ok(scope) => context(app, Some(&scope)),
            Err(reason) => FlowContext {
                view: Err(reason),
                ..context(app, None)
            },
        };
        Ok(FlowCommand::AcceptFindings {
            context,
            er_dir,
            run_id,
            finding_ids,
        })
    })?;
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    for tab in app.tabs.iter_mut().filter(|tab| tab.er_dir() == bucket) {
        tab.reload_ai_state();
    }
    Ok(snap_from_command(&app, state))
}

/// A flow run from its captured bucket, checked against the flow and target
/// the actor published.
pub fn get_run(
    state: &AppState,
    er_dir: String,
    run_id: String,
) -> Result<ArenaRunSnapshotWire, String> {
    let (key, config) = {
        let app = state.app.lock().map_err(|e| e.to_string())?;
        (key(&app), app.config.clone())
    };
    er_service::read_flow_run(&state.service.view(&key), &config, &er_dir, &run_id)
        .map_err(|error| error.to_string())
}

/// Recovers every open tab's bucket in its workspace actor at startup. The
/// commands are sent without waiting: recovery marks records and starts
/// nothing, and the published view shows what it found.
pub fn reconcile_on_load(app: &App, service: &crate::service::ServiceState) {
    let mut buckets: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for tab in &app.tabs {
        let key = service_key(&tab.repo_root, tab.remote_repo.as_deref().unwrap_or(""));
        buckets.entry(key).or_default().push(tab.er_dir());
    }
    for (key, buckets) in buckets {
        let _ = service
            .handle_for(&key)
            .send(Command::Reconcile { buckets });
    }
}
