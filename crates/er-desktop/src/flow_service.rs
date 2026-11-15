//! The review-flow commands on the flag path. Each reads the tab, the App's
//! settings and the view a flow would review here, where they live, by the
//! App's own rules, then sends the command to the workspace actor, which runs
//! the flow and its stages. A run read takes the published view and the store
//! and never waits on the actor (ADR 0067).
//!
//! Only compiled with `v1-service`.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use er_engine::app::{App, TabState};
use er_engine::review_flow_driver::ReviewFlowHost;
use er_service::{AcceptanceDiff, Command, FlowCommand, FlowContext, Reply};

use crate::arena_commands::validate_review_flow_bucket;
use crate::commands::{service_key, snap_from_command, AppState};
use crate::service::ServiceState;
use crate::snapshot::{AppSnapshot, ArenaRunSnapshotWire};

/// The live settings, and the view `scope` names when the command reviews one.
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

fn tab_key(tab: &TabState) -> String {
    service_key(&tab.repo_root, tab.remote_repo.as_deref().unwrap_or(""))
}

fn key(app: &App) -> String {
    tab_key(app.tab())
}

/// The actor that runs the flow on `er_dir`: the started actor whose view
/// holds it, else the one for the open tab on the bucket. A run is read and
/// accepted after the user may have switched tabs, and tabs of different
/// projects map to different actors, so the active tab can name the wrong one.
/// With neither, the active tab's actor answers that there is no such flow.
fn flow_key(service: &ServiceState, app: &App, er_dir: &str) -> String {
    service
        .flow_key(er_dir)
        .or_else(|| {
            app.tabs
                .iter()
                .find(|tab| tab.er_dir() == er_dir)
                .map(tab_key)
        })
        .unwrap_or_else(|| key(app))
}

/// Builds the command under the App lock, sends it once the lock is released,
/// and answers with the App's snapshot after bumping the desktop revision, as
/// the flag-off commands do. A command that checks the bucket controls the
/// active tab's flow; one that does not finds the actor by the bucket.
fn route(
    state: &AppState,
    er_dir: &str,
    check_bucket: bool,
    build: impl FnOnce(&App, &str) -> Result<FlowCommand, String>,
) -> Result<(Reply, AppSnapshot), String> {
    let (key, command) = {
        let app = state.app.lock().map_err(|e| e.to_string())?;
        let key = if check_bucket {
            validate_review_flow_bucket(&app, er_dir)?;
            key(&app)
        } else {
            flow_key(&state.service, &app, er_dir)
        };
        let command = build(&app, &key)?;
        (key, command)
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
    route(state, &er_dir, true, |app, _| {
        let scope = crate::commands::resolve_review_scope(&scope, app.tab())?;
        Ok(FlowCommand::Start {
            context: context(app, Some(&scope)),
            scope,
        })
    })
    .map(|(_, snapshot)| snapshot)
}

pub fn confirm(state: &AppState, er_dir: String) -> Result<AppSnapshot, String> {
    route(state, &er_dir.clone(), true, |app, _| {
        Ok(FlowCommand::Confirm {
            context: context(app, None),
            er_dir,
        })
    })
    .map(|(_, snapshot)| snapshot)
}

pub fn decline(state: &AppState, er_dir: String) -> Result<AppSnapshot, String> {
    route(state, &er_dir.clone(), true, |_, _| {
        Ok(FlowCommand::Decline { er_dir })
    })
    .map(|(_, snapshot)| snapshot)
}

/// A re-run reviews the branch view, as the driver's resume does.
pub fn rerun(state: &AppState, er_dir: String) -> Result<AppSnapshot, String> {
    route(state, &er_dir.clone(), true, |app, _| {
        Ok(FlowCommand::Rerun {
            context: context(app, Some("branch")),
            er_dir,
        })
    })
    .map(|(_, snapshot)| snapshot)
}

pub fn cancel(state: &AppState, er_dir: String) -> Result<AppSnapshot, String> {
    route(state, &er_dir.clone(), true, |_, _| {
        Ok(FlowCommand::Cancel { er_dir })
    })
    .map(|(_, snapshot)| snapshot)
}

pub fn run_standard(state: &AppState, er_dir: String) -> Result<AppSnapshot, String> {
    route(state, &er_dir.clone(), true, |app, _| {
        Ok(FlowCommand::RunStandard {
            context: context(app, None),
            er_dir,
        })
    })
    .map(|(_, snapshot)| snapshot)
}

/// Acceptance targets the bucket the run captured, in the actor that runs its
/// flow, whichever tab is active. The actor checks the run against the diff
/// the open tab that shows the run's target has now, which the App fetches
/// fresh here by its own rules, as the flag-off path does. The App's tabs on
/// the bucket then reload what was accepted.
pub fn accept(
    state: &AppState,
    er_dir: String,
    run_id: String,
    finding_ids: Vec<String>,
) -> Result<AppSnapshot, String> {
    let bucket = er_dir.clone();
    route(state, &er_dir.clone(), false, |app, key| {
        let view = state.service.view(key);
        let current = view
            .review_flow(&er_dir)
            .and_then(|live| live.run_targets.iter().find(|(id, _)| *id == run_id))
            .map(|(_, target)| AcceptanceDiff {
                target: target.clone(),
                diff: <App as ReviewFlowHost>::current_review_diff(app, target)
                    .map_err(|error| format!("{error:#}")),
            });
        Ok(FlowCommand::AcceptFindings {
            er_dir,
            run_id,
            finding_ids,
            current,
        })
    })?;
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    for tab in app.tabs.iter_mut().filter(|tab| tab.er_dir() == bucket) {
        tab.reload_ai_state();
    }
    Ok(snap_from_command(&app, state))
}

/// A flow run from its captured bucket, checked against the flow and target
/// the actor that runs the flow published, whichever tab is active.
pub fn get_run(
    state: &AppState,
    er_dir: String,
    run_id: String,
) -> Result<ArenaRunSnapshotWire, String> {
    let (key, config) = {
        let app = state.app.lock().map_err(|e| e.to_string())?;
        (flow_key(&state.service, &app, &er_dir), app.config.clone())
    };
    er_service::read_flow_run(&state.service.view(&key), &config, &er_dir, &run_id)
        .map_err(|error| error.to_string())
}

/// Recovers every open tab's bucket in its workspace actor at startup. The
/// commands are sent without waiting: recovery marks records and starts
/// nothing, and the published view shows what it found.
pub fn reconcile_on_load(app: &App, service: &ServiceState) {
    let mut buckets: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for tab in &app.tabs {
        buckets.entry(tab_key(tab)).or_default().push(tab.er_dir());
    }
    for (key, buckets) in buckets {
        let _ = service
            .handle_for(&key)
            .send(Command::Reconcile { buckets });
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::{Duration, Instant};

    use er_engine::app::{App, TabState};
    use er_engine::paths::ErRoot;
    use er_engine::ports::Ports;
    use er_service::Command;

    use super::flow_key;
    use crate::service::ServiceState;

    const DEADLINE: Duration = Duration::from_secs(10);

    /// A Deep flow's record whose finding stage was running when its owner
    /// died, so a load shows it interrupted.
    fn write_record(bucket: &Path) {
        std::fs::create_dir_all(bucket).unwrap();
        let record = serde_json::json!({
            "version": 1, "diff_hash": "hash", "started_at": "t0",
            "holder": { "pid": 1, "label": "flow-1" },
            "kind": "deep",
            "stages": [
                { "id": "triage", "stage": "triage", "label": "Triage",
                  "status": "complete", "diff_hash": "hash" },
                { "id": "finding:security", "stage": "finding", "label": "Security",
                  "status": "running", "diff_hash": "hash" }
            ]
        });
        er_engine::test_support::write_sidecar_json::<er_engine::ai::workflow_record::WorkflowRecord>(
            bucket, record,
        );
        std::fs::write(bucket.join(".diff-tmp.sha256"), "hash").unwrap();
    }

    fn tab(repo_root: &str, bucket: &str) -> TabState {
        let mut tab = TabState::new_for_test(Vec::new());
        tab.repo_root = repo_root.to_string();
        tab.er_root = ErRoot::managed(bucket);
        tab
    }

    /// A run is read and accepted in the actor that runs its flow after the
    /// user switched to a tab of another project, whose actor has no such
    /// flow; and still once the flow's own tab is closed.
    #[test]
    fn a_flow_command_finds_the_flow_actor_after_a_tab_switch() {
        let dir = tempfile::tempdir().unwrap();
        let bucket = |name: &str| dir.path().join(name).to_string_lossy().into_owned();
        let (flow_bucket, other_bucket) = (bucket("flow-bucket"), bucket("other-bucket"));
        er_engine::storage::with_process_test_storage_root(dir.path(), || {
            let mut app = App::new_for_test(Vec::new());
            app.tabs = vec![tab("/a", &flow_bucket), tab("/b", &other_bucket)];
            app.active_tab = 1;
            let service = ServiceState::new(Ports::for_test());

            // No actor has published the flow yet: the open tab on the bucket
            // names the actor.
            assert_eq!(flow_key(&service, &app, &flow_bucket), "/a");

            write_record(Path::new(&flow_bucket));
            let flow_actor = service.handle_for("/a");
            let events = flow_actor.subscribe();
            service
                .send(
                    "/a",
                    Command::Reconcile {
                        buckets: vec![flow_bucket.clone()],
                    },
                )
                .unwrap();
            let deadline = Instant::now() + DEADLINE;
            while flow_actor.view().review_flow(&flow_bucket).is_none() {
                let left = deadline.saturating_duration_since(Instant::now());
                if let Err(error) = events.recv_timeout(left) {
                    panic!("the recovered flow is published within {DEADLINE:?}: {error}");
                }
            }
            // The other project's actor is up and holds no flow on the bucket.
            assert!(service.view("/b").review_flow(&flow_bucket).is_none());

            // With the flow's tab closed, the published view still names it.
            app.tabs.remove(0);
            app.active_tab = 0;
            assert_eq!(flow_key(&service, &app, &flow_bucket), "/a");
        });
    }
}
