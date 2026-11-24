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
    if er_service::tasks::committed_importance(row) {
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

/// Reloads the repo's rules and refreshes its loaded tabs, through
/// er-service's step, as the TUI's host does.
fn reload_risk_rules(desktop: &DesktopState, target: &BackgroundTaskTarget) {
    let service = &desktop.service;
    let reloaded = er_service::tasks::reload_risk_rules(
        &service.app_registry().tabs_view(),
        target,
        &service.settings(),
        |entry| service.content(entry.key, &entry.descriptor),
        |entry| {
            crate::watcher::refresh(service, entry.key, &entry.descriptor);
        },
    );
    if let Err(error) = reloaded {
        log::warn!("reload risk rules after importance: {error}");
    }
}
