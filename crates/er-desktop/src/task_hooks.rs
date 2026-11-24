//! The desktop's side of er-service's task host (ADR 0074): the settings it
//! reads, what it tells the person, and what a finished task files and
//! reloads; and the commands that remove or stop a task.

use std::sync::Arc;

use er_engine::agent_log::AgentLogEntry;
use er_engine::agent_prompt::AgentPromptRun;
use er_engine::task_target::BackgroundTaskTarget;
use er_protocol::ErError;
pub(crate) use er_service::tasks::refused;
use er_service::{Finished, Launch, TaskHooks};

use crate::state::DesktopState;

/// The hooks a task on `desktop`'s host runs with.
pub fn hooks(desktop: &DesktopState) -> TaskHooks {
    let (config, notify, finished) = (desktop.clone(), desktop.clone(), desktop.clone());
    TaskHooks {
        ports: desktop.ports.clone(),
        config: Arc::new(move || crate::arena_service::config(&config)),
        notify: Arc::new(move |message, long| {
            crate::resources::notice::tell(&notify, message, long);
        }),
        finished: Arc::new(move |task| on_finish(&finished, &task)),
    }
}

pub fn enqueue(desktop: &DesktopState, launch: Launch) -> Result<String, String> {
    er_service::tasks::enqueue(&desktop.tasks, &hooks(desktop), launch)
}

pub fn hold_agent(
    desktop: &DesktopState,
    target: BackgroundTaskTarget,
    name: &str,
    launch: impl FnOnce(std::sync::mpsc::Sender<AgentLogEntry>) -> anyhow::Result<AgentPromptRun>,
) -> Result<String, String> {
    er_service::tasks::hold_agent(&desktop.tasks, &hooks(desktop), target, name, launch)
}

#[cfg(test)]
pub(crate) fn finish(desktop: &DesktopState, id: &str, result: Result<(), String>) {
    er_service::tasks::finish(&desktop.tasks, &hooks(desktop), id, result);
}

/// Retires finished tasks and starts what the queue can. The tick runs it.
pub fn poll(desktop: &DesktopState) {
    er_service::tasks::poll(&desktop.tasks, &hooks(desktop));
}

/// Removes a task that has not started, as `CancelQueuedTask` asks.
pub fn cancel_queued(desktop: &DesktopState, id: &str) -> Result<(), ErError> {
    if !desktop.tasks.cancel_queued(id) {
        return Err(refused("Queued review not found (it may have started)"));
    }
    crate::resources::notice::tell(desktop, "review removed from queue".to_string(), false);
    Ok(())
}

/// Stops a running task, as `CancelRunningTask` asks. The kill ends the run,
/// and its worker records the outcome.
pub fn cancel_running(desktop: &DesktopState, id: &str) -> Result<(), ErError> {
    if !desktop.tasks.stop(id) {
        return Err(refused("No running review with that id"));
    }
    Ok(())
}

/// A committed importance run reloads the repo's rules; a queued task is
/// filed in the inbox.
fn on_finish(desktop: &DesktopState, task: &Finished<'_>) {
    let row = task.row;
    if row.kind == er_engine::ai::prompts::IMPORTANCE_TASK_KIND && row.status == "done" {
        reload_risk_rules(desktop, task.target);
        crate::resources::notice::tell(
            desktop,
            "Risk rules committed — see the task log for its report".to_string(),
            true,
        );
    }
    if !task.agent {
        crate::route_helpers::file_tasks_in_inbox(desktop, std::slice::from_ref(row));
    }
}

/// Has the settings view read the repo's rules again after an importance run
/// committed them, so `risk(key)` moves, and refreshes every loaded tab on
/// the repo, so its diff reads the new table and filters with it at once, as
/// the App's `sync_repo_rules_to_tabs` hands them on. A load replaces the
/// paths the view resolves, so it sends a loaded tab's paths on that repo,
/// the active tab's first.
fn reload_risk_rules(desktop: &DesktopState, target: &BackgroundTaskTarget) {
    let tabs = desktop.service.app_registry().tabs_view();
    let active = tabs.active;
    let mut on_repo: Vec<_> = tabs
        .tabs
        .iter()
        .filter(|entry| entry.descriptor.repo_root == target.repo_root)
        .collect();
    on_repo.sort_by_key(|entry| Some(entry.key) != active);
    let paths = on_repo
        .iter()
        .find_map(|entry| desktop.service.content(entry.key, &entry.descriptor))
        .map(|content| content.diff.files.iter().map(|f| f.path.clone()).collect())
        .unwrap_or_default();
    let load = er_service::SettingsCommand::LoadRiskRules {
        repo_root: target.repo_root.clone(),
        remote_repo: target.remote_repo.clone(),
        paths,
    };
    if let Err(error) = desktop.service.settings().send_and_wait(load) {
        log::warn!("reload risk rules after importance: {error}");
    }
    for entry in on_repo {
        crate::watcher::refresh(&desktop.service, entry.key, &entry.descriptor);
    }
}
