//! The background agent tasks with the flag on: review lenses, tours,
//! diagrams, importance ranking and sidebar triage, queued and run here with
//! no App. `jobs` lists each task's row, and `task_log` reads its output
//! lines, so a line moves no version.
//!
//! The host is the desktop's: sidebar triage runs on a PR no tab shows, so
//! no workspace actor holds it, and the cap and the queue are app-wide, as
//! the App's are. A task launches through `StageJob` on the one slot pool
//! every agent shares, as the App's and the service's flows do. Its outputs
//! reach the tabs' content through the tick's `ReloadSidecars`, since the
//! agent's writes move the sidecars' stamps.
//!
//! Only compiled with `v1-service`.

use std::collections::VecDeque;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use er_engine::ai::ScopedRunStamp;
use er_engine::app::{AgentLogEntry, AgentLogSource, AgentPromptRun};
use er_engine::config::{AiSelection, ErConfig};
use er_engine::job_staging::JobStaging;
use er_engine::jobs::{AgentRunHandle, JobProgress};
use er_engine::stage_job::{staging_for, HostWriteDiagram, StageJob, StageJobRequest};
use er_engine::task_target::BackgroundTaskTarget;
use er_protocol::{ErError, ErErrorKind, TaskJob, TaskLogLine};

use crate::commands::now_ms;
use crate::state::{DesktopState, WriteClock};

/// How long a finished task stays listed, so its pill can show the outcome.
const FINISHED_WINDOW_MS: u64 = 8_000;
/// Output lines one task keeps.
const LOG_LINES: usize = 500;
/// Lines a job may get ahead of its log drain before the runner drops some.
const PROGRESS_CAPACITY: usize = 256;

/// The desktop's background tasks. A clone shares them.
#[derive(Clone, Default)]
pub struct TaskHost {
    tasks: Arc<Mutex<Vec<Task>>>,
    /// Moves on every row change and never for an output line.
    pub clock: WriteClock,
}

struct Task {
    row: TaskJob,
    log: VecDeque<TaskLogLine>,
    /// Set while it runs, for a stop.
    run: Option<Arc<AgentRunHandle>>,
    /// Set while it waits for a slot.
    queued: Option<Queued>,
    target: BackgroundTaskTarget,
    /// A tab command the agent launcher started (validate, a note sent to
    /// the agent), which tells its own outcome and stays out of the inbox.
    agent: bool,
}

struct Queued {
    launch: Launch,
    staging: Option<Arc<JobStaging>>,
}

/// What one task runs, captured when it is asked for, so a later change of
/// pick or config cannot retarget it.
pub struct Launch {
    pub kind: String,
    pub command_name: String,
    pub target: BackgroundTaskTarget,
    pub prompt: String,
    pub prepared_diff: bool,
    pub host_write_diagram: Option<HostWriteDiagram>,
    pub scoped_stamp: Option<ScopedRunStamp>,
    pub config: Arc<ErConfig>,
    pub selection: AiSelection,
    /// The caller tells the person once for a run of several tasks, so the
    /// host sends no started or queued notice of its own while it accepts
    /// this one; a start later, from the queue, is told as any other.
    pub quiet: bool,
}

impl TaskHost {
    /// A host whose row changes move `clock`.
    pub fn new(clock: WriteClock) -> Self {
        Self {
            tasks: Arc::default(),
            clock,
        }
    }

    fn lock(&self) -> MutexGuard<'_, Vec<Task>> {
        self.tasks.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Every listed task's row, oldest first.
    pub fn rows(&self) -> Vec<TaskJob> {
        self.lock().iter().map(|task| task.row.clone()).collect()
    }

    /// The output lines of a listed task, oldest first; none once it left
    /// the list.
    pub fn log(&self, id: &str) -> Vec<TaskLogLine> {
        self.lock()
            .iter()
            .find(|task| task.row.id == id)
            .map(|task| task.log.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Removes a task that has not started. Answers whether one was.
    pub fn cancel_queued(&self, id: &str) -> bool {
        let removed = {
            let mut tasks = self.lock();
            let before = tasks.len();
            tasks.retain(|task| task.row.id != id || task.queued.is_none());
            tasks.len() != before
        };
        if removed {
            self.clock.bump();
        }
        removed
    }

    /// The stop handle of a running task. The worker records the outcome
    /// once the kill ends the run.
    pub fn running(&self, id: &str) -> Option<Arc<AgentRunHandle>> {
        self.lock()
            .iter()
            .find(|task| task.row.id == id)
            .and_then(|task| task.run.clone())
    }

    fn running_count(&self) -> usize {
        self.lock().iter().filter(|task| task.run.is_some()).count()
    }
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
    let run = desktop
        .tasks
        .running(id)
        .ok_or_else(|| refused("No running review with that id"))?;
    run.kill();
    Ok(())
}

pub(crate) fn refused(message: impl Into<String>) -> ErError {
    ErError {
        kind: ErErrorKind::Refused,
        message: message.into(),
    }
}

/// The settings view's cap on running review agents.
fn cap(desktop: &DesktopState) -> usize {
    crate::arena_service::config(desktop)
        .ai_hub
        .effective_max_concurrent_reviews()
}

/// Accepts `launch` and starts it when the cap allows; otherwise it waits in
/// the queue. Answers the task's id. The same kind on the same target is
/// refused while one is queued or running.
pub fn enqueue(desktop: &DesktopState, launch: Launch) -> Result<String, String> {
    let host = &desktop.tasks;
    let target = launch.target.clone();
    if target.repo_root.is_empty() && target.remote_repo.is_none() {
        return Err("Open a repository or PR first — nothing to review yet".to_string());
    }
    let (staging, prompt) =
        staging_for(&launch.kind, launch.prompt, &target.er_dir).map_err(|e| e.to_string())?;
    let launch = Launch { prompt, ..launch };
    let id = task_id(&launch.kind, &target);
    let row = TaskJob {
        id: id.clone(),
        label: er_engine::app::kind_label(&launch.kind),
        kind: launch.kind.clone(),
        target_label: target.display_label(),
        scope: target.scope.clone(),
        repo_root: target.repo_root.clone(),
        branch_label: target.branch_label.clone(),
        pr_number: target.pr_number,
        remote_repo: target.remote_repo.clone(),
        er_dir: target.er_dir.clone(),
        status: "queued".to_string(),
        error: None,
        started_at_ms: now_ms(),
        finished_at_ms: None,
    };
    let command_name = launch.command_name.clone();
    let quiet = launch.quiet;
    {
        // One lock over the busy check and the push, so two clicks at once
        // queue one task.
        let mut tasks = host.lock();
        let busy = tasks.iter().any(|task| {
            task.row.kind == launch.kind
                && task.target == target
                && (task.run.is_some() || task.queued.is_some())
        });
        if busy {
            return Err(format!(
                "{} already running for {}",
                launch.command_name,
                target.display_label()
            ));
        }
        tasks.push(Task {
            row,
            log: VecDeque::new(),
            run: None,
            queued: Some(Queued { launch, staging }),
            target: target.clone(),
            agent: false,
        });
    }
    host.clock.bump();
    dispatch(desktop);
    let position = {
        let tasks = host.lock();
        let waiting: Vec<&Task> = tasks.iter().filter(|task| task.queued.is_some()).collect();
        waiting.iter().position(|task| task.row.id == id)
    };
    if let Some(position) = position {
        if quiet {
            hear_from_queue(host, &id);
            return Ok(id);
        }
        let label = target.display_label();
        crate::resources::notice::tell(
            desktop,
            format!("{command_name} queued (#{}, {label})", position + 1),
            false,
        );
    }
    Ok(id)
}

/// Lists a tab command that `launch` starts through the agent launcher
/// (`App::launch_agent_prompt`), and records its lines and its outcome. It
/// takes no queue slot: the runner waits for an agent slot itself, as the
/// App's tab commands do. The same command on the same bucket is refused
/// while one runs. Answers the task's id.
pub fn hold_agent(
    desktop: &DesktopState,
    target: BackgroundTaskTarget,
    name: &str,
    launch: impl FnOnce(Sender<AgentLogEntry>) -> anyhow::Result<AgentPromptRun>,
) -> Result<String, String> {
    let host = &desktop.tasks;
    // One lock over the busy check, the launch and the push, so two sends at
    // once start one agent. The launch only spawns: the runner waits for its
    // slot on its own thread.
    let mut tasks = host.lock();
    let busy = tasks.iter().any(|task| {
        task.row.kind == name && task.target.er_dir == target.er_dir && task.run.is_some()
    });
    if busy {
        return Err(format!(
            "{name} already running for {}",
            target.display_label()
        ));
    }
    let (lines_tx, lines) = std::sync::mpsc::channel();
    let started = launch(lines_tx).map_err(|e| e.to_string())?;
    let id = task_id(name, &target);
    let row = TaskJob {
        id: id.clone(),
        label: er_engine::app::kind_label(name),
        kind: name.to_string(),
        target_label: target.display_label(),
        scope: target.scope.clone(),
        repo_root: target.repo_root.clone(),
        branch_label: target.branch_label.clone(),
        pr_number: target.pr_number,
        remote_repo: target.remote_repo.clone(),
        er_dir: target.er_dir.clone(),
        status: "running".to_string(),
        error: None,
        started_at_ms: now_ms(),
        finished_at_ms: None,
    };
    tasks.push(Task {
        row,
        log: VecDeque::new(),
        run: Some(started.run),
        queued: None,
        target,
        agent: true,
    });
    drop(tasks);
    host.clock.bump();
    let (desktop, task) = (desktop.clone(), id.clone());
    // Unnamed, like every thread that reaches the storage root. The lines
    // end when the launcher's worker drops its sender, after it answered.
    std::thread::Builder::new()
        .spawn(move || {
            for entry in lines {
                append(&desktop.tasks, &task, agent_line(entry));
            }
            let result = started
                .result
                .recv()
                .unwrap_or_else(|_| Err(anyhow::anyhow!("the agent thread crashed")));
            finish(&desktop, &task, result.map_err(|e| e.to_string()));
        })
        .map_err(|e| format!("start the task worker: {e}"))?;
    Ok(id)
}

/// Lets the queued task `id` tell the person when it starts.
fn hear_from_queue(host: &TaskHost, id: &str) {
    let mut tasks = host.lock();
    if let Some(queued) = tasks
        .iter_mut()
        .find(|task| task.row.id == id)
        .and_then(|task| task.queued.as_mut())
    {
        queued.launch.quiet = false;
    }
}

/// Starts queued tasks, oldest first, while fewer than the cap run here and
/// the slot pool has a free slot.
pub fn dispatch(desktop: &DesktopState) {
    let cap = cap(desktop);
    loop {
        if desktop.tasks.running_count() >= cap || er_engine::jobs::active_agent_slots() >= cap {
            return;
        }
        let next = {
            let mut tasks = desktop.tasks.lock();
            let Some(task) = tasks.iter_mut().find(|task| task.queued.is_some()) else {
                return;
            };
            let queued = task.queued.take();
            let run = AgentRunHandle::new();
            task.run = Some(Arc::clone(&run));
            task.row.status = "running".to_string();
            task.row.started_at_ms = now_ms();
            queued.map(|queued| (task.row.id.clone(), queued, run))
        };
        let Some((id, queued, run)) = next else {
            return;
        };
        desktop.tasks.clock.bump();
        if let Err(error) = launch(desktop, &id, queued, run) {
            let label = desktop
                .tasks
                .lock()
                .iter()
                .find(|task| task.row.id == id)
                .map(|task| task.row.target_label.clone())
                .unwrap_or_default();
            finish(
                desktop,
                &id,
                Err(format!("failed to start ({label}): {error}")),
            );
        }
    }
}

/// Prepares the job's argv here, so a refused launch fails at once, then
/// runs it on a worker that records the outcome.
fn launch(
    desktop: &DesktopState,
    id: &str,
    queued: Queued,
    run: Arc<AgentRunHandle>,
) -> Result<(), String> {
    let Queued { launch, staging } = queued;
    let job = StageJob::prepare(
        StageJobRequest {
            kind: &launch.kind,
            command_name: &launch.command_name,
            target: &launch.target,
            execution_root: None,
            prompt: launch.prompt,
            prepared_diff: launch.prepared_diff,
            host_write_diagram: launch.host_write_diagram,
            scoped_stamp: launch.scoped_stamp,
            staging,
            config: &launch.config,
            selection: &launch.selection,
        },
        &desktop.ports,
    )
    .map_err(|e| e.to_string())?;
    let limits = er_engine::jobs::JobLimits {
        slot_cap: launch.config.ai_hub.effective_max_concurrent_reviews(),
        ceiling: launch.config.ai_hub.effective_max_concurrent_agents(),
        retry: er_engine::jobs::RetryPolicy::none(),
    };
    let name = launch.command_name;
    let started = format!("{name} started ({})", launch.target.display_label());
    append(&desktop.tasks, id, status_line(&name, started.clone()));
    let is_stream_json = job.is_stream_json();
    let (progress, lines) = std::sync::mpsc::sync_channel::<JobProgress>(PROGRESS_CAPACITY);
    let (host, log_id, log_name) = (desktop.tasks.clone(), id.to_string(), name.clone());
    // The drain is its own thread, so a slow reader cannot hold the
    // runner's pipe readers.
    let drain = std::thread::spawn(move || {
        for event in lines {
            if let Some(line) = log_line(&log_name, event, is_stream_json) {
                append(&host, &log_id, line);
            }
        }
    });
    let (desktop_for_worker, id) = (desktop.clone(), id.to_string());
    // Unnamed, like every thread that reaches the storage root.
    std::thread::Builder::new()
        .spawn(move || {
            let result = job.run(&er_engine::jobs::LocalJobSink, limits, run, Some(progress));
            let _ = drain.join();
            finish(&desktop_for_worker, &id, result.map_err(|e| e.to_string()));
            dispatch(&desktop_for_worker);
        })
        .map_err(|e| format!("start the task worker: {e}"))?;
    if !launch.quiet {
        crate::resources::notice::tell(desktop, started, false);
    }
    Ok(())
}

/// Records a task's outcome, tells the person once and files it in the
/// inbox.
pub(crate) fn finish(desktop: &DesktopState, id: &str, result: Result<(), String>) {
    let finished = {
        let mut tasks = desktop.tasks.lock();
        let Some(task) = tasks.iter_mut().find(|task| task.row.id == id) else {
            return;
        };
        let label = task.row.target_label.clone();
        let what = if task.agent {
            task.row.kind.clone()
        } else {
            "review".to_string()
        };
        let (status, error, message) = match result {
            Ok(()) => ("done", None, format!("{what} completed ({label})")),
            Err(error) => (
                "failed",
                Some(error.clone()),
                format!("{what} failed ({label}): {error}"),
            ),
        };
        task.row.status = status.to_string();
        task.row.error = error;
        task.row.finished_at_ms = Some(now_ms());
        task.run = None;
        push_line(&mut task.log, status_line(&what, message.clone()));
        (task.row.clone(), message, task.agent, task.target.clone())
    };
    desktop.tasks.clock.bump();
    let (row, message, agent, target) = finished;
    if row.kind == er_engine::ai::prompts::IMPORTANCE_TASK_KIND && row.status == "done" {
        reload_risk_rules(desktop, &target);
        crate::resources::notice::tell(
            desktop,
            "Risk rules committed — see the task log for its report".to_string(),
            true,
        );
    }
    crate::resources::notice::tell(desktop, message, true);
    if !agent {
        crate::commands::file_tasks_in_inbox(desktop, &[row]);
    }
}

/// Has the settings view read the repo's rules again after an importance run
/// committed them, so `risk(key)` moves and the open tabs filter with the new
/// rules at once, as the App's `sync_repo_rules_to_tabs` hands them on. A load
/// replaces the paths the view resolves, so it sends a loaded tab's paths on
/// that repo, the active tab's first.
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
}

/// Drops the tasks finished longer ago than the window and starts what the
/// queue can. The tick runs it.
pub fn poll(desktop: &DesktopState) {
    let cutoff = now_ms().saturating_sub(FINISHED_WINDOW_MS);
    let retired = {
        let mut tasks = desktop.tasks.lock();
        let before = tasks.len();
        tasks.retain(|task| task.row.finished_at_ms.is_none_or(|at| at >= cutoff));
        tasks.len() != before
    };
    if retired {
        desktop.tasks.clock.bump();
    }
    dispatch(desktop);
}

fn append(host: &TaskHost, id: &str, line: TaskLogLine) {
    if let Some(task) = host.lock().iter_mut().find(|task| task.row.id == id) {
        push_line(&mut task.log, line);
    }
}

fn push_line(log: &mut VecDeque<TaskLogLine>, line: TaskLogLine) {
    log.push_back(line);
    while log.len() > LOG_LINES {
        log.pop_front();
    }
}

fn status_line(name: &str, text: String) -> TaskLogLine {
    TaskLogLine {
        command_name: name.to_string(),
        source: "status".to_string(),
        text,
    }
}

/// One runner line as the log shows it: a stream-json event parsed, other
/// stdout trimmed, stderr as it came; none when nothing is left.
fn log_line(name: &str, event: JobProgress, is_stream_json: bool) -> Option<TaskLogLine> {
    let (source, text) = match event {
        JobProgress::StdoutLine(line) => (
            "stdout",
            er_engine::app::stdout_log_text(line.trim_end_matches(['\r', '\n']), is_stream_json)?,
        ),
        JobProgress::StderrLine(line) => {
            ("stderr", line.trim_end_matches(['\r', '\n']).to_string())
        }
    };
    Some(TaskLogLine {
        command_name: name.to_string(),
        source: source.to_string(),
        text,
    })
}

/// A launcher log entry as a task's line.
fn agent_line(entry: AgentLogEntry) -> TaskLogLine {
    let source = match entry.source {
        AgentLogSource::Stdout => "stdout",
        AgentLogSource::Stderr => "stderr",
        AgentLogSource::Status => "status",
    };
    TaskLogLine {
        command_name: entry.command_name.to_string(),
        source: source.to_string(),
        text: entry.text,
    }
}

fn task_id(kind: &str, target: &BackgroundTaskTarget) -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{kind}:{}:{}:{sequence}", target.key(), std::process::id())
}
