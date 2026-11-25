//! The background agent tasks: review lenses, tours, diagrams, importance
//! ranking, sidebar triage and the tab commands, queued and run here with no
//! App. A front end lists each task's row, and reads its output lines per
//! task or per bucket, so a line moves no row clock.
//!
//! The host is app-wide: sidebar triage runs on a PR no tab shows, so no
//! workspace actor holds it, and the cap and the queue are app-wide, as the
//! App's are. A task launches through `StageJob` on the one slot pool every
//! agent shares, as the App's and the actors' flows do. Its outputs reach the
//! tabs' content through `ReloadSidecars`, since the agent's writes move the
//! sidecars' stamps. What only one front end does when a task moves (its
//! notices, its inbox) comes in through [`TaskHooks`] (ADR 0074).

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use er_engine::agent_log::{AgentLogEntry, AgentLogSource};
use er_engine::agent_prompt::AgentPromptRun;
use er_engine::ai::ScopedRunStamp;
use er_engine::config::{AiSelection, ErConfig};
use er_engine::job_staging::JobStaging;
use er_engine::jobs::{AgentRunHandle, JobProgress};
use er_engine::ports::Ports;
use er_engine::stage_job::{staging_for, HostWriteDiagram, StageJob, StageJobRequest};
use er_engine::task_target::BackgroundTaskTarget;
use er_protocol::{ErError, ErErrorKind, TaskJob, TaskLogLine};

/// How long a finished task stays listed, so its pill can show the outcome.
const FINISHED_WINDOW_MS: u64 = 8_000;
/// Output lines one task keeps.
const LOG_LINES: usize = 500;
/// Output lines one bucket keeps across its tasks, as a TUI tab's log does.
pub const BUCKET_LOG_LINES: usize = 5_000;
/// Lines a job may get ahead of its log drain before the runner drops some.
const PROGRESS_CAPACITY: usize = 256;
/// A tour's task kind. A tour reads its own prepared diff and writes only
/// `tour.json`, which no review-flow stage reads or writes, so it is the one
/// task that runs beside a flow.
pub const TOUR_TASK_KIND: &str = "tour";

/// The app's background tasks. A clone shares them.
#[derive(Clone, Default)]
pub struct TaskHost {
    tasks: Arc<Mutex<Vec<Task>>>,
    /// Each bucket's lines, kept after its tasks leave the list.
    logs: Arc<Mutex<HashMap<String, VecDeque<AgentLogEntry>>>>,
    /// Moves on every row change and never for an output line.
    pub clock: TaskClock,
}

/// A count of the host's row changes, and who to wake on each.
#[derive(Clone, Default)]
pub struct TaskClock {
    revision: Arc<AtomicU64>,
    wake: Option<Arc<dyn Fn(u64) + Send + Sync>>,
    /// Called on every row change; one that answers `false` is dropped.
    watchers: Arc<Mutex<Vec<Watcher>>>,
}

type Watcher = Box<dyn Fn(u64) -> bool + Send + Sync>;

impl TaskClock {
    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }

    pub fn bump(&self) {
        let revision = self.revision.fetch_add(1, Ordering::AcqRel) + 1;
        if let Some(wake) = &self.wake {
            wake(revision);
        }
        lock(&self.watchers).retain(|watch| watch(revision));
    }
}

/// What a front end does when a task needs its settings or moves. The host
/// takes them per call, so it holds no front end's state.
#[derive(Clone)]
pub struct TaskHooks {
    pub ports: Ports,
    /// The settings view's config, which the cap is read from.
    pub config: Arc<dyn Fn() -> Arc<ErConfig> + Send + Sync>,
    /// Tells the person; `true` for a message that should stay up longer.
    pub notify: Arc<dyn Fn(String, bool) + Send + Sync>,
    /// Runs after a task's outcome is recorded and told.
    pub finished: Arc<dyn Fn(Finished<'_>) + Send + Sync>,
}

/// A task that just finished.
pub struct Finished<'a> {
    pub row: &'a TaskJob,
    pub target: &'a BackgroundTaskTarget,
    /// A tab command the agent launcher ran, which tells its own outcome.
    pub agent: bool,
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
    /// A host that calls `wake` with the new revision on every row change.
    pub fn new(wake: impl Fn(u64) + Send + Sync + 'static) -> Self {
        Self {
            clock: TaskClock {
                revision: Arc::default(),
                wake: Some(Arc::new(wake)),
                watchers: Arc::default(),
            },
            ..Self::default()
        }
    }

    fn lock(&self) -> MutexGuard<'_, Vec<Task>> {
        self.tasks.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Calls `watch` with the new revision on every row change until it
    /// answers `false`. Every
    /// caller bumps the clock after it lets the tasks go, so a watcher never
    /// runs under the tasks lock.
    pub fn watch(&self, watch: impl Fn(u64) -> bool + Send + Sync + 'static) {
        lock(&self.clock.watchers).push(Box::new(watch));
    }

    /// Whether a task that writes into `er_dir` is queued or running there.
    /// A review flow counts these as its bucket's producers, as the App did.
    pub fn writes_into(&self, er_dir: &str) -> bool {
        self.lock().iter().any(|task| {
            task.target.er_dir == er_dir
                && task.row.kind != TOUR_TASK_KIND
                && (task.run.is_some() || task.queued.is_some())
        })
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

    /// Stops a running task. The worker records the outcome once the kill
    /// ends the run. Answers whether one ran.
    pub fn stop(&self, id: &str) -> bool {
        let run = self
            .lock()
            .iter()
            .find(|task| task.row.id == id)
            .and_then(|task| task.run.clone());
        run.inspect(|run| run.kill()).is_some()
    }

    fn running_count(&self) -> usize {
        self.lock().iter().filter(|task| task.run.is_some()).count()
    }

    /// The last [`BUCKET_LOG_LINES`] output lines of every task that ran on
    /// `er_dir`, oldest first, whether or not its row is still listed.
    pub fn bucket_log(&self, er_dir: &str) -> Vec<AgentLogEntry> {
        lock(&self.logs)
            .get(er_dir)
            .map(|log| log.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Forgets `er_dir`'s log, as a closed tab's goes.
    pub fn drop_bucket_log(&self, er_dir: &str) {
        lock(&self.logs).remove(er_dir);
    }
}

/// Has the settings view read the repo's rules again after an importance run
/// committed them, so `risk(key)` moves, and refreshes every loaded tab on
/// the repo through `refresh`, so its diff reads the new table at once, as
/// the App's `sync_repo_rules_to_tabs` handed them on. A load replaces the
/// paths the view resolves, so it sends a loaded tab's paths on that repo,
/// the active tab's first. It waits on the settings actor, so it runs on the
/// task's worker, never on a front end's loop. A failed load is answered
/// after the refreshes.
pub fn reload_risk_rules(
    tabs: &crate::TabsView,
    target: &BackgroundTaskTarget,
    settings: &crate::SettingsHandle,
    content: impl Fn(&crate::TabEntry) -> Option<Arc<crate::TabContent>>,
    refresh: impl Fn(&crate::TabEntry),
) -> Result<(), crate::WorkspaceError> {
    let mut on_repo: Vec<_> = tabs
        .tabs
        .iter()
        .filter(|entry| entry.descriptor.repo_root == target.repo_root)
        .collect();
    on_repo.sort_by_key(|entry| Some(entry.key) != tabs.active);
    let paths = on_repo
        .iter()
        .find_map(|entry| content(entry))
        .map(|content| content.diff.files.iter().map(|f| f.path.clone()).collect())
        .unwrap_or_default();
    let load = crate::SettingsCommand::LoadRiskRules {
        repo_root: target.repo_root.clone(),
        remote_repo: target.remote_repo.clone(),
        paths,
    };
    let loaded = settings.send_and_wait(load);
    for entry in on_repo {
        refresh(entry);
    }
    loaded.map(drop)
}

/// Whether `row` is an importance run that committed its table.
pub fn committed_importance(row: &TaskJob) -> bool {
    row.kind == er_engine::ai::prompts::IMPORTANCE_TASK_KIND && row.status == "done"
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

pub fn refused(message: impl Into<String>) -> ErError {
    ErError {
        kind: ErErrorKind::Refused,
        message: message.into(),
    }
}

/// The settings view's cap on running review agents.
fn cap(hooks: &TaskHooks) -> usize {
    (hooks.config)().ai_hub.effective_max_concurrent_reviews()
}

/// Accepts `launch` and starts it when the cap allows; otherwise it waits in
/// the queue. Answers the task's id. The same kind on the same target is
/// refused while one is queued or running.
pub fn enqueue(host: &TaskHost, hooks: &TaskHooks, launch: Launch) -> Result<String, String> {
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
        label: er_engine::task_target::kind_label(&launch.kind),
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
        // One lock over the busy check, the admission and the push, so two
        // clicks at once queue one task, and a flow that takes the bucket
        // either sees this task or refuses it here.
        let mut tasks = host.lock();
        admit(&launch.kind, &target.er_dir)?;
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
    dispatch(host, hooks);
    // Under one lock with the position, so no dispatch can start a quiet
    // task between our reading that it waits and our letting it tell its start.
    let position = {
        let mut tasks = host.lock();
        let position = tasks
            .iter()
            .filter(|task| task.queued.is_some())
            .position(|task| task.row.id == id);
        if position.is_some() && quiet {
            hear_from_queue(&mut tasks, &id);
        }
        position
    };
    if let Some(position) = position {
        if quiet {
            return Ok(id);
        }
        let label = target.display_label();
        (hooks.notify)(
            format!("{command_name} queued (#{}, {label})", position + 1),
            false,
        );
    }
    Ok(id)
}

/// Lists a tab command that `launch` starts through the agent launcher
/// (`er_engine::agent_prompt`), and records its lines and its outcome. It
/// takes no queue slot: the runner waits for an agent slot itself, as the
/// App's tab commands do. The same command on the same bucket is refused
/// while one runs. Answers the task's id.
pub fn hold_agent(
    host: &TaskHost,
    hooks: &TaskHooks,
    target: BackgroundTaskTarget,
    name: &str,
    launch: impl FnOnce(Sender<AgentLogEntry>) -> anyhow::Result<AgentPromptRun>,
) -> Result<String, String> {
    hold(host, hooks, target, name, true, launch)
}

/// [`hold_agent`] past the bucket's admission, so a test can put a host
/// task in a bucket a flow already holds.
#[cfg(test)]
pub(crate) fn hold_agent_unadmitted(
    host: &TaskHost,
    hooks: &TaskHooks,
    target: BackgroundTaskTarget,
    name: &str,
    launch: impl FnOnce(Sender<AgentLogEntry>) -> anyhow::Result<AgentPromptRun>,
) -> Result<String, String> {
    hold(host, hooks, target, name, false, launch)
}

fn hold(
    host: &TaskHost,
    hooks: &TaskHooks,
    target: BackgroundTaskTarget,
    name: &str,
    admitted: bool,
    launch: impl FnOnce(Sender<AgentLogEntry>) -> anyhow::Result<AgentPromptRun>,
) -> Result<String, String> {
    // One lock over the busy check, the launch and the push, so two sends at
    // once start one agent. The launch only spawns: the runner waits for its
    // slot on its own thread.
    let mut tasks = host.lock();
    if admitted {
        admit(name, &target.er_dir)?;
    }
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
        label: er_engine::task_target::kind_label(name),
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
    let (host, hooks, task) = (host.clone(), hooks.clone(), id.clone());
    // Unnamed, like every thread that reaches the storage root. The lines
    // end when the launcher's worker drops its sender, after it answered.
    std::thread::Builder::new()
        .spawn(move || {
            for entry in lines {
                append(&host, &task, agent_line(entry));
            }
            let result = started
                .result
                .recv()
                .unwrap_or_else(|_| Err(anyhow::anyhow!("the agent thread crashed")));
            finish(&host, &hooks, &task, result.map_err(|e| e.to_string()));
        })
        .map_err(|e| format!("start the task worker: {e}"))?;
    Ok(id)
}

/// Refuses a task that writes into a bucket a review flow holds. Called
/// under the tasks lock: a flow takes its lease before it reads the tasks,
/// so either it sees this task or this probe sees its lease. The probe never
/// waits, so holding the tasks lock across it cannot deadlock with a flow.
fn admit(kind: &str, er_dir: &str) -> Result<(), String> {
    if kind == TOUR_TASK_KIND {
        return Ok(());
    }
    er_engine::arena::check_bucket_admission(er_dir).map_err(|e| e.to_string())
}

/// Lets the queued task `id` tell the person when it starts.
fn hear_from_queue(tasks: &mut [Task], id: &str) {
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
pub fn dispatch(host: &TaskHost, hooks: &TaskHooks) {
    let cap = cap(hooks);
    loop {
        if host.running_count() >= cap || er_engine::jobs::active_agent_slots() >= cap {
            return;
        }
        let next = {
            let mut tasks = host.lock();
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
        host.clock.bump();
        if let Err(error) = launch(host, hooks, &id, queued, run) {
            let label = host
                .lock()
                .iter()
                .find(|task| task.row.id == id)
                .map(|task| task.row.target_label.clone())
                .unwrap_or_default();
            finish(
                host,
                hooks,
                &id,
                Err(format!("failed to start ({label}): {error}")),
            );
        }
    }
}

/// Prepares the job's argv here, so a refused launch fails at once, then
/// runs it on a worker that records the outcome.
fn launch(
    host: &TaskHost,
    hooks: &TaskHooks,
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
        &hooks.ports,
    )
    .map_err(|e| e.to_string())?;
    let limits = er_engine::jobs::JobLimits {
        slot_cap: launch.config.ai_hub.effective_max_concurrent_reviews(),
        ceiling: launch.config.ai_hub.effective_max_concurrent_agents(),
        retry: er_engine::jobs::RetryPolicy::none(),
    };
    let name = launch.command_name;
    let started = format!("{name} started ({})", launch.target.display_label());
    append(host, id, status_line(&name, started.clone()));
    let is_stream_json = job.is_stream_json();
    let (progress, lines) = std::sync::mpsc::sync_channel::<JobProgress>(PROGRESS_CAPACITY);
    let (log_host, log_id, log_name) = (host.clone(), id.to_string(), name.clone());
    // The drain is its own thread, so a slow reader cannot hold the
    // runner's pipe readers.
    let drain = std::thread::spawn(move || {
        for event in lines {
            if let Some(line) = log_line(&log_name, event, is_stream_json) {
                append(&log_host, &log_id, line);
            }
        }
    });
    // Told before the job runs, so a quick failure is told after it.
    if !launch.quiet {
        (hooks.notify)(started, false);
    }
    let (worker_host, worker_hooks, id) = (host.clone(), hooks.clone(), id.to_string());
    // Unnamed, like every thread that reaches the storage root.
    std::thread::Builder::new()
        .spawn(move || {
            let result = job.run(&er_engine::jobs::LocalJobSink, limits, run, Some(progress));
            let _ = drain.join();
            finish(
                &worker_host,
                &worker_hooks,
                &id,
                result.map_err(|e| e.to_string()),
            );
            dispatch(&worker_host, &worker_hooks);
        })
        .map_err(|e| format!("start the task worker: {e}"))?;
    Ok(())
}

/// Records a task's outcome, tells the person once and hands it to the
/// front end's `finished` hook.
pub fn finish(host: &TaskHost, hooks: &TaskHooks, id: &str, result: Result<(), String>) {
    let finished = {
        let mut tasks = host.lock();
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
        let line = status_line(&what, message.clone());
        push_line(&mut task.log, line.clone());
        host.keep_line(&task.target.er_dir, line);
        (task.row.clone(), message, task.agent, task.target.clone())
    };
    host.clock.bump();
    let (row, message, agent, target) = finished;
    (hooks.notify)(message, true);
    (hooks.finished)(Finished {
        row: &row,
        target: &target,
        agent,
    });
}

/// Drops the tasks finished longer ago than the window and starts what the
/// queue can. The front end's tick runs it.
pub fn poll(host: &TaskHost, hooks: &TaskHooks) {
    retire(host, now_ms().saturating_sub(FINISHED_WINDOW_MS));
    dispatch(host, hooks);
}

/// Drops the tasks that finished before `cutoff`. Their lines stay in their
/// bucket's log.
pub(crate) fn retire(host: &TaskHost, cutoff: u64) {
    let retired = {
        let mut tasks = host.lock();
        let before = tasks.len();
        tasks.retain(|task| task.row.finished_at_ms.is_none_or(|at| at >= cutoff));
        tasks.len() != before
    };
    if retired {
        host.clock.bump();
    }
}

fn append(host: &TaskHost, id: &str, line: TaskLogLine) {
    let er_dir = {
        let mut tasks = host.lock();
        let Some(task) = tasks.iter_mut().find(|task| task.row.id == id) else {
            return;
        };
        push_line(&mut task.log, line.clone());
        task.target.er_dir.clone()
    };
    host.keep_line(&er_dir, line);
}

impl TaskHost {
    /// Adds `line` to `er_dir`'s log, which outlives the task's row.
    fn keep_line(&self, er_dir: &str, line: TaskLogLine) {
        let source = match line.source.as_str() {
            "stdout" => AgentLogSource::Stdout,
            "stderr" => AgentLogSource::Stderr,
            _ => AgentLogSource::Status,
        };
        let mut logs = lock(&self.logs);
        let log = logs.entry(er_dir.to_string()).or_default();
        log.push_back(AgentLogEntry {
            timestamp: std::time::Instant::now(),
            command_name: Arc::from(line.command_name),
            source,
            text: line.text,
        });
        while log.len() > BUCKET_LOG_LINES {
            log.pop_front();
        }
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
            er_engine::agent_log::stdout_log_text(
                line.trim_end_matches(['\r', '\n']),
                is_stream_json,
            )?,
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

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

fn task_id(kind: &str, target: &BackgroundTaskTarget) -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{kind}:{}:{}:{sequence}", target.key(), std::process::id())
}
