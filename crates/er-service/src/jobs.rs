//! Agent jobs on the workspace actor.
//!
//! [`ActorJobSink`] turns a submission into `Command::StartJob`. The actor
//! keeps each running job's stop handle, runs the job on a fresh effect thread
//! through the engine's runner, and publishes its progress lines and its
//! finish as events. `CancelJob` kills the handle the runner watches. A
//! run-level arena cancel reaches the same handles: each one is registered
//! with the run's cancel bridge before it is submitted.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use std::thread;

use er_agent::agent_run::AgentRunHandle;
use er_engine::arena::ArenaRunControls;
use er_engine::jobs::{JobId, JobResult, JobSink, JobSubmission, JobTicket};

use crate::actor::WorkspaceDeps;
use crate::guard;
use crate::handle::{ActorMsg, ReplySender, WorkspaceError};
use crate::protocol::{Command, JobOutcome, Reply};
use crate::workspace::LiveArenaRun;

/// Lines a job may get ahead of its forwarder before the runner drops some.
const PROGRESS_BUFFER: usize = 256;

/// The flag path's job sink: every submission becomes a `StartJob` on the
/// actor, so the actor sees and can stop every job its workspace runs.
pub(crate) struct ActorJobSink {
    tx: Sender<ActorMsg>,
}

impl ActorJobSink {
    pub(crate) fn new(tx: Sender<ActorMsg>) -> Self {
        Self { tx }
    }
}

impl JobSink for ActorJobSink {
    fn submit(&self, submission: JobSubmission) -> JobTicket {
        // The submitter waits on the ticket, and the actor is what answers
        // it: a wait on the actor thread would never return.
        guard::assert_off_actor();
        let job = JobId::next();
        let (result, answer) = mpsc::channel();
        let (reply, _) = mpsc::channel();
        let _ = self.tx.send(ActorMsg::Command {
            command: Command::StartJob {
                job,
                submission,
                result,
            },
            reply,
        });
        JobTicket::new(job, answer)
    }
}

/// The actor's running jobs and its live arena runs.
pub(crate) struct JobHost {
    running: HashMap<JobId, Arc<AgentRunHandle>>,
    arena: ArenaRunControls,
    /// Set while an arena wake-up is queued and not yet published.
    pending: Arc<AtomicBool>,
}

impl JobHost {
    /// The arena runs submit to the deps' sink when a test injects one, and to
    /// this actor otherwise. Their progress wakes the actor, which publishes.
    pub(crate) fn new(deps: &WorkspaceDeps, tx: &Sender<ActorMsg>) -> Self {
        let sink = deps
            .jobs()
            .unwrap_or_else(|| Arc::new(ActorJobSink::new(tx.clone())));
        let arena = ArenaRunControls::with_jobs(sink);
        let notify = tx.clone();
        let pending = Arc::new(AtomicBool::new(false));
        let raised = Arc::clone(&pending);
        // Progress ticks arrive in bursts; one wake-up per publish is enough,
        // since the actor reads every run's latest state when it publishes.
        arena.set_notify(Arc::new(move || {
            if !raised.swap(true, Ordering::AcqRel) {
                let _ = notify.send(ActorMsg::ArenaChanged);
            }
        }));
        Self {
            running: HashMap::new(),
            arena,
            pending,
        }
    }

    pub(crate) fn arena(&self) -> &ArenaRunControls {
        &self.arena
    }

    /// The live arena section of the view. Reads the controls' in-memory
    /// state only, so it runs on the actor thread. Clears the pending
    /// wake-up first, so a tick that lands while this reads raises another.
    pub(crate) fn live_arena_runs(&self) -> Vec<LiveArenaRun> {
        self.pending.store(false, Ordering::Release);
        let mut runs: Vec<LiveArenaRun> = self
            .arena
            .all()
            .into_iter()
            .map(|control| LiveArenaRun {
                run_id: control.run_id.clone(),
                er_dir: control.er_dir.clone(),
                status: control.status(),
                progress: control.progress_state(),
            })
            .collect();
        runs.sort_by(|left, right| left.run_id.cmp(&right.run_id));
        runs
    }

    pub(crate) fn start(
        &mut self,
        tx: &Sender<ActorMsg>,
        job: JobId,
        submission: JobSubmission,
        result: Sender<JobResult>,
        reply: ReplySender,
    ) {
        self.running.insert(job, Arc::clone(&submission.handle));
        spawn_job(tx, job, submission, result);
        let _ = reply.send(Ok(Reply::JobStarted { job }));
    }

    /// Kills the job's stop handle. The kill signals the process group, so it
    /// runs off the actor like any other subprocess work.
    pub(crate) fn cancel(&self, job: JobId) -> Option<impl FnOnce() + Send + 'static> {
        let handle = Arc::clone(self.running.get(&job)?);
        Some(move || handle.kill())
    }

    pub(crate) fn finished(&mut self, job: JobId) {
        self.running.remove(&job);
    }
}

/// Test support: the thread each job's effect ran on, by name. The actor
/// thread is named and effect threads are not.
#[cfg(test)]
pub(crate) static SPAWNED_ON: std::sync::Mutex<Vec<(JobId, Option<String>)>> =
    std::sync::Mutex::new(Vec::new());

/// Runs one job on a fresh effect thread: forwards each output line to the
/// actor and to the submitter's own progress sink, then answers the ticket and
/// reports the finish after the last line.
fn spawn_job(
    tx: &Sender<ActorMsg>,
    job: JobId,
    mut submission: JobSubmission,
    result: Sender<JobResult>,
) {
    let effect_tx = tx.clone();
    // Unnamed on purpose: the storage guard lets only unnamed worker threads
    // use a process-wide test root, and production spawns unnamed threads too.
    let spawned = thread::Builder::new().spawn(move || {
        #[cfg(test)]
        if let Ok(mut spawned) = SPAWNED_ON.lock() {
            spawned.push((job, thread::current().name().map(str::to_owned)));
        }
        guard::assert_off_actor();
        let (lines, received) = mpsc::sync_channel(PROGRESS_BUFFER);
        let upstream = submission.progress.replace(lines);
        let forward = effect_tx.clone();
        let forwarder = thread::spawn(move || {
            for line in received {
                if let Some(upstream) = &upstream {
                    let _ = upstream.try_send(line.clone());
                }
                let _ = forward.send(ActorMsg::JobProgress { job, line });
            }
        });
        let output = er_engine::jobs::run_job(submission);
        let _ = forwarder.join();
        let outcome = JobOutcome::of(&output);
        let _ = result.send(output);
        let _ = effect_tx.send(ActorMsg::JobFinished { job, outcome });
    });
    if spawned.is_err() {
        let _ = tx.send(ActorMsg::JobFinished {
            job,
            outcome: JobOutcome::Failed {
                message: "the job thread could not start".into(),
            },
        });
    }
}

/// Answers a cancel for a job the actor is not running.
pub(crate) fn refuse_unknown(job: JobId, reply: ReplySender) {
    let _ = reply.send(Err(WorkspaceError::Failed(format!("{job} is not running"))));
}
