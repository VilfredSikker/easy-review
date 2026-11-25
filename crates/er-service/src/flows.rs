//! Review flows on the workspace actor.
//!
//! The stage sequence is the engine's [`ReviewFlows`] driver, the same one the
//! App runs; this module is its host on the flag path. Every driver call does
//! disk work and may reach a port, so none runs on the actor thread: each flow
//! command, each advance and the load-time recovery run as effects and take
//! the flows' lock there. The actor reads the flows only from the projection
//! each call sends back, which it publishes as the view's flow section.
//!
//! A stage task runs [`StageJob`] on its own worker thread through the
//! actor's job sink, so its job takes an agent slot and its progress reaches
//! subscribers as `JobProgress`. When the job and its commit finish, the
//! worker wakes the actor, which advances the flows. Arena stages run on the
//! actor's own run controls, so their live progress is published too.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;

use anyhow::{anyhow, bail, Context, Result};
use er_engine::ai::review_flow::{FlowTaskStatus, ReviewFlow};
use er_engine::ai::WorkflowHolder;
use er_engine::arena::{
    self, ArenaRunControl, ArenaRunControls, ArenaRunInputs, ArenaStartParams, RunStatus,
    SeededStartParams,
};
use er_engine::config::{AiSelection, ErConfig};
use er_engine::jobs::{JobLimits, JobSink};
use er_engine::ports::Ports;
use er_engine::review_flow_driver::{
    flow_from_record, holder_of, rerun_plan, ReviewFlowHost, ReviewFlowInputs, ReviewFlowLease,
    ReviewFlows, StageCompletion,
};
use er_engine::stage_job::{staging_for, StageJob, StageJobRequest};
use er_engine::task_target::BackgroundTaskTarget;

use crate::actor::WorkspaceDeps;
use crate::handle::{ActorMsg, WorkspaceError};
use crate::protocol::{AcceptanceDiff, FlowCommand, FlowContext, Reply};
use crate::workspace::LiveReviewFlow;

#[cfg(test)]
#[path = "flow_driver_tests/mod.rs"]
mod driver_tests;

/// What one flow call changed, sent to the actor to publish.
pub(crate) struct FlowsUpdate {
    pub(crate) flows: Vec<LiveReviewFlow>,
    /// Panel runs whose `panel-run.json` appeared since the last update, as
    /// `(bucket, run id)`.
    pub(crate) panel_runs: Vec<(String, String)>,
}

/// The actor's handle on its flows.
#[derive(Clone)]
pub(crate) struct FlowRunner {
    shared: Arc<Mutex<Flows>>,
    /// Set while an advance is queued and has not yet started.
    advance_pending: Arc<AtomicBool>,
    tx: Sender<ActorMsg>,
}

struct Flows {
    flows: ReviewFlows,
    host: ServiceFlowHost,
}

impl FlowRunner {
    pub(crate) fn new(
        deps: &WorkspaceDeps,
        sink: Arc<dyn JobSink>,
        arena: ArenaRunControls,
        tx: &Sender<ActorMsg>,
    ) -> Self {
        let host = ServiceFlowHost {
            deps: deps.clone(),
            sink,
            arena,
            tx: tx.clone(),
            config: Arc::new(ErConfig::default()),
            effort: None,
            view: Err("no review view was sent".into()),
            acceptance: None,
            leases: HashMap::new(),
            next_lease: 1,
            tasks: HashMap::new(),
            finished: Arc::new(Mutex::new(HashMap::new())),
            panel_runs_seen: HashSet::new(),
            task_host: deps.tasks().clone(),
        };
        Self {
            shared: Arc::new(Mutex::new(Flows {
                flows: ReviewFlows::default(),
                host,
            })),
            advance_pending: Arc::new(AtomicBool::new(false)),
            tx: tx.clone(),
        }
    }

    /// A driver panic must not strand the leases the flows hold, so a
    /// poisoned lock still hands out the flows it guards.
    fn lock(&self) -> MutexGuard<'_, Flows> {
        self.shared.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Runs one flow command. Called on an effect thread.
    pub(crate) fn run(&self, command: FlowCommand) -> Result<Reply, WorkspaceError> {
        let mut guard = self.lock();
        let Flows { flows, host } = &mut *guard;
        let result = match command {
            FlowCommand::Start { context, scope } => {
                host.take_context(context);
                flows
                    .start_review_flow(host, &scope)
                    .map(|()| host.started_bucket())
            }
            FlowCommand::Rerun { context, er_dir } => {
                host.take_context(context);
                flows
                    .rerun_review_flow(host, &er_dir)
                    .map(|()| Reply::FlowChanged { er_dir })
            }
            FlowCommand::Confirm { context, er_dir } => {
                host.take_context(context);
                flows
                    .confirm_review_flow(host, &er_dir)
                    .map(|()| Reply::FlowChanged { er_dir })
            }
            FlowCommand::RunStandard { context, er_dir } => {
                host.take_context(context);
                flows
                    .run_review_flow_standard(host, &er_dir)
                    .map(|()| Reply::FlowChanged { er_dir })
            }
            FlowCommand::Decline { er_dir } => flows
                .decline_review_flow(host, &er_dir)
                .map(|()| Reply::FlowChanged { er_dir }),
            FlowCommand::Cancel { er_dir } => {
                if flows.cancel_review_flow(host, &er_dir) {
                    Ok(Reply::FlowChanged { er_dir })
                } else {
                    Err(anyhow!("There is no running review flow to cancel"))
                }
            }
            FlowCommand::AcceptFindings {
                er_dir,
                run_id,
                finding_ids,
                current,
            } => {
                host.acceptance = current;
                let accepted = flows
                    .accept_review_flow_findings(host, &er_dir, &run_id, finding_ids)
                    .map(|accepted| Reply::FlowAccepted { accepted });
                host.acceptance = None;
                accepted
            }
        };
        self.send_update(flows, host);
        result.map_err(|error| WorkspaceError::Failed(format!("{error:#}")))
    }

    /// Queues one advance of every flow. Ticks that land while one is queued
    /// fold into it; a tick during a running advance queues the next.
    pub(crate) fn request_advance(&self) {
        if self.advance_pending.swap(true, Ordering::AcqRel) {
            return;
        }
        let runner = self.clone();
        // Unnamed on purpose: the storage guard lets only unnamed worker
        // threads use a process-wide test root.
        let _ = thread::Builder::new().spawn(move || {
            runner.advance_pending.store(false, Ordering::Release);
            let mut guard = runner.lock();
            let Flows { flows, host } = &mut *guard;
            flows.advance_review_flows(host);
            runner.send_update(flows, host);
        });
    }

    /// Load-time recovery of the named buckets. Stale arena runs and a dead
    /// owner's running stages are marked, and each record that can be re-run
    /// is shown as an interrupted flow with the plan a re-run would follow.
    /// Nothing is started: resuming is an explicit `Rerun`.
    pub(crate) fn reconcile(&self, buckets: &[String]) -> Reply {
        let mut guard = self.lock();
        let Flows { flows, host } = &mut *guard;
        let mut resumable = Vec::new();
        for bucket in buckets {
            let _ = arena::reconcile_stale_runs_excluding(Path::new(bucket), |run_id| {
                host.arena.contains(run_id)
            });
            if host.leases.contains_key(bucket) {
                continue;
            }
            let Some(record) = er_engine::workflow_recovery::reconcile_workflow_record(bucket)
            else {
                continue;
            };
            if record.resumable() {
                resumable.push(bucket.clone());
                flows.insert(bucket.clone(), flow_from_record(&record));
            }
        }
        self.send_update(flows, host);
        Reply::Reconciled { resumable }
    }

    fn send_update(&self, flows: &ReviewFlows, host: &mut ServiceFlowHost) {
        let update = FlowsUpdate {
            flows: project(flows),
            panel_runs: host.new_panel_runs(flows),
        };
        let _ = self.tx.send(ActorMsg::FlowsChanged(update));
    }
}

/// Every flow as the view shows it, by bucket. A record shown after a load
/// carries the plan a re-run would follow, read from the prepared diff the
/// bucket holds; no diff is fetched for it.
fn project(flows: &ReviewFlows) -> Vec<LiveReviewFlow> {
    let mut live: Vec<LiveReviewFlow> = flows
        .buckets()
        .filter_map(|er_dir| {
            let flow = flows.review_flow_for(er_dir)?.clone();
            let rerun_stages = resume_plan(er_dir, &flow);
            Some(LiveReviewFlow {
                er_dir: er_dir.to_owned(),
                draining: flows.review_flow_is_draining(er_dir),
                run_targets: flows.run_targets_in(er_dir),
                flow,
                rerun_stages,
            })
        })
        .collect();
    live.sort_by(|left, right| left.er_dir.cmp(&right.er_dir));
    live
}

fn resume_plan(er_dir: &str, flow: &ReviewFlow) -> Vec<String> {
    use er_engine::ai::review_flow::FlowState;
    if !matches!(flow.state, FlowState::Interrupted { .. }) {
        return Vec::new();
    }
    let store = er_engine::store_registry::store();
    let Ok(scope) = er_store::Scope::dir(er_dir) else {
        return Vec::new();
    };
    let Ok(Some(record)) = er_engine::ai::workflow_record::load(&store, &scope) else {
        return Vec::new();
    };
    let current = er_engine::ai::prepared_diff::diff_tmp_hash(er_dir).unwrap_or_default();
    rerun_plan(&record, &current)
        .map(|plan| plan.stage_ids().into_iter().map(str::to_owned).collect())
        .unwrap_or_default()
}

/// The completion error the driver keeps a cancelled row on.
const CANCELLED_BEFORE_LAUNCH: &str = "cancelled before launch";

/// A stage task the host launched and has not handed back.
struct StageTask {
    kind: String,
    target: BackgroundTaskTarget,
    run: Arc<er_agent::agent_run::AgentRunHandle>,
    /// Stopped while it still waited for its agent slot.
    cancelled: bool,
}

/// The flag path's [`ReviewFlowHost`]. It holds the settings and the view the
/// latest command carried, the bucket leases, the stage tasks it launched and
/// the actor's arena runs.
struct ServiceFlowHost {
    deps: WorkspaceDeps,
    sink: Arc<dyn JobSink>,
    arena: ArenaRunControls,
    tx: Sender<ActorMsg>,
    config: Arc<ErConfig>,
    effort: Option<String>,
    view: Result<ReviewFlowInputs, String>,
    /// Set for the length of one acceptance.
    acceptance: Option<AcceptanceDiff>,
    leases: HashMap<String, er_store::LeaseGuard>,
    next_lease: u64,
    tasks: HashMap<String, StageTask>,
    /// Filled by each stage's worker when its job and commit are done.
    finished: Arc<Mutex<HashMap<String, StageCompletion>>>,
    panel_runs_seen: HashSet<(String, String)>,
    /// The app's task host, whose tasks write into the same buckets.
    task_host: crate::tasks::TaskHost,
}

impl ServiceFlowHost {
    fn take_context(&mut self, context: FlowContext) {
        self.config = context.config;
        self.effort = context.effort;
        self.view = context.view;
    }

    /// A start reviews the view it carried, so that view names its bucket.
    fn started_bucket(&self) -> Reply {
        let er_dir = self
            .view
            .as_ref()
            .map(|view| view.target.er_dir.clone())
            .unwrap_or_default();
        Reply::FlowChanged { er_dir }
    }

    fn finished(&self) -> MutexGuard<'_, HashMap<String, StageCompletion>> {
        self.finished.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn is_running(&self, id: &str) -> bool {
        self.tasks.contains_key(id) && !self.finished().contains_key(id)
    }

    /// A stage of this host still writes into the bucket.
    fn has_producers(&self, er_dir: &str) -> bool {
        self.tasks
            .iter()
            .any(|(id, task)| task.target.er_dir == er_dir && self.is_running(id))
    }

    fn lease_is_valid(&self, lease: &ReviewFlowLease) -> bool {
        self.leases
            .get(lease.bucket())
            .is_some_and(|guard| guard.holder() == lease.holder())
    }

    fn check_arena_lease(&self, er_dir: &str, lease: &ReviewFlowLease) -> Result<()> {
        if er_dir != lease.bucket() || !self.lease_is_valid(lease) {
            bail!("Review flow lease does not own this arena bucket");
        }
        Ok(())
    }

    /// A flow's frozen settings with today's concurrency limits.
    fn with_live_limits(&self, mut captured: ErConfig) -> ErConfig {
        let live = &self.config.ai_hub;
        captured.ai_hub.max_concurrent_reviews = live.max_concurrent_reviews;
        captured.ai_hub.max_concurrent_arena_reviews = live.max_concurrent_arena_reviews;
        captured.ai_hub.max_concurrent_agents = live.max_concurrent_agents;
        captured
    }

    /// Each flow's panel run whose record has appeared since the last look.
    fn new_panel_runs(&mut self, flows: &ReviewFlows) -> Vec<(String, String)> {
        let store = er_engine::store_registry::store();
        let mut fresh = Vec::new();
        for er_dir in flows.buckets() {
            let Some(run_id) = flows
                .review_flow_for(er_dir)
                .and_then(|flow| flow.arena_run_id.clone())
            else {
                continue;
            };
            let key = (er_dir.to_owned(), run_id);
            if self.panel_runs_seen.contains(&key) {
                continue;
            }
            let Ok(scope) = er_store::Scope::dir(er_dir) else {
                continue;
            };
            if er_engine::ai::panel_run::load(&store, &scope)
                .is_some_and(|record| record.run_id == key.1)
            {
                self.panel_runs_seen.insert(key.clone());
                fresh.push(key);
            }
        }
        fresh
    }
}

/// Numbers stage tasks across every actor, so an id never repeats.
static NEXT_TASK: AtomicU64 = AtomicU64::new(1);

impl ReviewFlowHost for ServiceFlowHost {
    fn config(&self) -> &ErConfig {
        &self.config
    }

    fn current_ai_effort(&self) -> Option<&str> {
        self.effort.as_deref()
    }

    fn ports(&self) -> &Ports {
        self.deps.ports()
    }

    // The flag-path desktop shows flows from the published view; it has no
    // toast for these yet.
    fn notify(&mut self, _message: &str) {}

    fn notify_long(&mut self, _message: &str) {}

    fn review_flow_inputs(&self, _scope: &str) -> Result<ReviewFlowInputs> {
        self.view.clone().map_err(|reason| anyhow!(reason))
    }

    /// The fresh diff the acceptance carried, when the desktop fetched it
    /// for this target.
    fn current_review_diff(&self, target: &BackgroundTaskTarget) -> Result<String> {
        match &self.acceptance {
            Some(current) if current.target == *target => {
                current.diff.clone().map_err(|reason| anyhow!(reason))
            }
            _ => bail!("The captured review target is no longer open; start a new Review"),
        }
    }

    fn recover_arena_bucket(&self, er_dir: &str) -> Result<()> {
        arena::reconcile_stale_runs_excluding(Path::new(er_dir), |run_id| {
            self.arena.is_owned(run_id)
        })
    }

    fn acquire_review_flow_lease(&mut self, er_dir: &str) -> Result<ReviewFlowLease> {
        arena::check_bucket_admission(er_dir)?;
        if self.has_producers(er_dir) {
            bail!("Review work is already queued or running in this review bucket");
        }
        let sequence = self.next_lease;
        self.next_lease = sequence.wrapping_add(1).max(1);
        let scope = er_store::Scope::dir(er_dir)?;
        let guard = er_engine::store_registry::store()
            .lease(
                &scope,
                er_store::DEFAULT_LOCK_TIMEOUT,
                &format!("flow-{sequence}"),
            )
            .with_context(|| format!("take the review flow lease for {er_dir}"))?;
        let lease = ReviewFlowLease::new(er_dir.to_owned(), sequence, guard.holder().clone());
        self.leases.insert(er_dir.to_owned(), guard);
        Ok(lease)
    }

    fn release_review_flow_lease(&mut self, lease: &ReviewFlowLease) -> Result<()> {
        if !self.lease_is_valid(lease) {
            bail!("Review flow lease is stale");
        }
        if self.has_producers(lease.bucket()) {
            bail!("Review flow still has queued or running work in this bucket");
        }
        self.leases.remove(lease.bucket());
        Ok(())
    }

    fn workflow_holder(&self, er_dir: &str) -> Option<WorkflowHolder> {
        self.leases.get(er_dir).map(holder_of)
    }

    /// Prepares the stage's argv here, so a refused launch fails before the
    /// task has an id, then runs it on a worker that blocks on the job.
    fn enqueue_with_selection(
        &mut self,
        kind: String,
        command_name: &str,
        target: BackgroundTaskTarget,
        execution_root: String,
        prompt: String,
        config: ErConfig,
        selection: AiSelection,
    ) -> Result<String> {
        if target.repo_root.is_empty() && target.remote_repo.is_none() {
            bail!("Open a repository or PR first — nothing to review yet");
        }
        if self
            .tasks
            .iter()
            .any(|(id, task)| task.kind == kind && task.target == target && self.is_running(id))
        {
            bail!(
                "{command_name} already running for {}",
                target.display_label()
            );
        }
        let (staging, prompt) = staging_for(&kind, prompt, &target.er_dir)?;
        let job = StageJob::prepare(
            StageJobRequest {
                kind: &kind,
                command_name,
                target: &target,
                execution_root: Some(execution_root),
                prompt,
                prepared_diff: true,
                host_write_diagram: None,
                scoped_stamp: None,
                staging,
                config: &config,
                selection: &selection,
            },
            self.deps.ports(),
        )?;
        let limits = JobLimits {
            slot_cap: self.config.ai_hub.effective_max_concurrent_reviews(),
            ceiling: self.config.ai_hub.effective_max_concurrent_agents(),
            retry: er_agent::job::RetryPolicy::none(),
        };
        let id = format!("flow-task-{}", NEXT_TASK.fetch_add(1, Ordering::Relaxed));
        let sink = Arc::clone(&self.sink);
        let finished = Arc::clone(&self.finished);
        let tx = self.tx.clone();
        let completion = StageCompletion {
            target: target.clone(),
            kind: kind.clone(),
            done: false,
            error: None,
        };
        let task_id = id.clone();
        let run = er_agent::agent_run::AgentRunHandle::new();
        let handle = Arc::clone(&run);
        // Unnamed on purpose, like every effect thread.
        thread::Builder::new()
            .spawn(move || {
                let result = job.run(&*sink, limits, handle, None);
                let completion = StageCompletion {
                    done: result.is_ok(),
                    error: result.err().map(|error| error.to_string()),
                    ..completion
                };
                finished
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(task_id, completion);
                let _ = tx.send(ActorMsg::FlowTick);
            })
            .context("start the stage worker")?;
        self.tasks.insert(
            id.clone(),
            StageTask {
                kind,
                target,
                run,
                cancelled: false,
            },
        );
        Ok(id)
    }

    /// Every task this host launches belongs to a flow.
    fn register_review_flow_task(&mut self, _id: String) {}

    fn take_review_flow_task_completion(&mut self, id: &str) -> Option<StageCompletion> {
        let mut completion = self.finished().remove(id)?;
        if self.tasks.remove(id).is_some_and(|task| task.cancelled) {
            completion.done = false;
            completion.error = Some(CANCELLED_BEFORE_LAUNCH.into());
        }
        Some(completion)
    }

    /// A launched task waits for its agent slot inside the runner, so the
    /// host only ever sees it running.
    fn stage_status(&self, id: &str) -> Option<FlowTaskStatus> {
        self.is_running(id).then_some(FlowTaskStatus::Running)
    }

    /// A stage still waiting for its agent slot is stopped there and never
    /// launches. A launched stage is left to drain on its own.
    fn cancel_queued_background_task(&mut self, id: &str) -> bool {
        if !self.is_running(id) {
            return false;
        }
        let Some(task) = self.tasks.get_mut(id) else {
            return false;
        };
        if !task.run.cancel_before_launch() {
            return false;
        }
        task.cancelled = true;
        true
    }

    fn arena_start_seeded_for(
        &mut self,
        repo_root: String,
        config: ErConfig,
        params: SeededStartParams,
        lease: &ReviewFlowLease,
    ) -> Result<Option<String>> {
        self.check_arena_lease(&params.er_dir, lease)?;
        let config = self.with_live_limits(config);
        arena::start_seeded_run(&self.arena, config, repo_root, params, None)
    }

    fn arena_start_for(
        &mut self,
        inputs: ArenaRunInputs,
        config: ErConfig,
        params: ArenaStartParams,
        lease: &ReviewFlowLease,
    ) -> Result<String> {
        self.check_arena_lease(&inputs.er_dir, lease)?;
        let config = self.with_live_limits(config);
        arena::start_review_flow_run(&self.arena, config, inputs, params)
    }

    fn arena_cancel(&mut self, run_id: &str) -> Result<()> {
        let Some(control) = self.arena.get(run_id) else {
            bail!("no active arena run {run_id}");
        };
        control.cancel();
        let paths = arena::ArenaPaths::for_run(Path::new(&control.er_dir), run_id);
        if let Ok(mut run) = arena::load_run(&paths) {
            run.status = RunStatus::Cancelled;
            run.completed_at = Some(er_engine::sync::chrono_now());
            let _ = arena::save_run(&paths, &run);
        }
        Ok(())
    }

    fn arena_control(&self, run_id: &str) -> Option<Arc<ArenaRunControl>> {
        self.arena.get(run_id)
    }

    fn arena_is_active(&self, run_id: &str) -> bool {
        self.arena.is_active(run_id)
    }
}

/// One flow run's snapshot for a reader: checked against the flow and the
/// target the view published, then read from the store with the live status.
/// Reads the published view and the store only (ADR 0067).
pub fn read_flow_run(
    view: &crate::workspace::WorkspaceView,
    config: &ErConfig,
    er_dir: &str,
    run_id: &str,
) -> Result<arena::ArenaRunSnapshot, WorkspaceError> {
    let failed = |error: anyhow::Error| WorkspaceError::Failed(format!("{error:#}"));
    let live = view.review_flow(er_dir);
    let target = live.and_then(|live| {
        live.run_targets
            .iter()
            .find(|(id, _)| id == run_id)
            .map(|(_, target)| target)
    });
    er_engine::review_flow_driver::flow_run_results(
        live.map(|live| &live.flow),
        target,
        er_dir,
        run_id,
    )
    .map_err(failed)?;
    let paths = arena::ArenaPaths::for_run(Path::new(er_dir), run_id);
    let mut run = arena::load_run(&paths).map_err(failed)?;
    if let Some(live) = view.arena_run(run_id) {
        run.status = live.status.clone();
    }
    Ok(arena::build_snapshot_with_config(config, run))
}
