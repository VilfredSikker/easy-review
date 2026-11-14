//! Agent jobs as submissions to a sink.
//!
//! A caller that launches an agent builds a [`JobSpec`] and submits it here
//! instead of driving a `JobRunner` itself, so the host decides where the job
//! runs. [`LocalJobSink`] runs it in this process for the App path; the
//! workspace service's sink hands it to the actor, which runs it on an effect
//! thread and publishes its progress. Every sink ends in [`run_job`], so each
//! job draws from the one agent-slot pool (ADR 0021) whichever host runs it.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender};
use std::sync::Arc;

use er_agent::agent_run::AgentRunHandle;
use er_agent::agent_timing::AgentRunTimer;
use er_agent::job::{JobError, JobOutput, JobProgress, JobRun, JobRunner, JobSpec, RetryPolicy};

/// Names one submitted job for its whole life: its progress, its cancel and
/// its finish all carry it. Unique within the process.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JobId(pub u64);

impl JobId {
    /// A fresh id. Sinks call this so `submit` can answer without a round trip.
    pub fn next() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

impl fmt::Display for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "job-{}", self.0)
    }
}

/// The runner a job needs: its slot cap, the agent ceiling and its retry.
/// Plain data, so a submission can cross a channel and the receiving side
/// builds the same runner the submitter would have.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JobLimits {
    pub slot_cap: usize,
    pub ceiling: usize,
    pub retry: RetryPolicy,
}

impl JobLimits {
    pub fn runner(&self) -> JobRunner {
        JobRunner::with_retry(self.slot_cap, self.ceiling, self.retry)
    }
}

/// One job, with everything the host owns about it.
#[derive(Clone, Debug)]
pub struct JobSubmission {
    pub spec: JobSpec,
    pub limits: JobLimits,
    /// The stop control. Killing it ends the job's process group; the runner
    /// watches its flag while queued for a slot and while running.
    pub handle: Arc<AgentRunHandle>,
    pub progress: Option<SyncSender<JobProgress>>,
    /// The timing line's label.
    pub label: String,
}

pub type JobResult = Result<JobOutput, JobError>;

/// What `submit` hands back: the job's id and where its result arrives.
pub struct JobTicket {
    pub id: JobId,
    result: Receiver<JobResult>,
}

impl JobTicket {
    pub fn new(id: JobId, result: Receiver<JobResult>) -> Self {
        Self { id, result }
    }

    /// Blocks until the job finishes. A sink that dropped the job without an
    /// answer reads as a spawn failure, so a caller is never stranded.
    pub fn wait(self) -> JobResult {
        self.result.recv().unwrap_or_else(|_| {
            Err(JobError::Spawn(anyhow::anyhow!(
                "{} ended without a result",
                self.id
            )))
        })
    }
}

/// Where agent jobs go.
pub trait JobSink: Send + Sync {
    /// Submit one job. A sink may run it before returning; the caller learns
    /// the outcome from the ticket either way.
    fn submit(&self, job: JobSubmission) -> JobTicket;
}

/// Runs one job on the calling thread through the shared runner, recording its
/// timing line. Every sink ends here.
pub fn run_job(job: JobSubmission) -> JobResult {
    let JobSubmission {
        spec,
        limits,
        handle,
        progress,
        label,
    } = job;
    let mut timer = AgentRunTimer::start();
    let output = limits.runner().run_controlled(
        &spec,
        JobRun {
            handle,
            progress,
            timer: Some(&mut timer),
        },
    );
    timer.emit(
        &label,
        &spec.provider.program,
        output.as_ref().is_ok_and(|output| output.status.success()),
    );
    output
}

/// The App's sink: runs the job on the submitting thread. Arena callers
/// already submit from their own worker threads, so the flag-off path keeps
/// the threads, the thread-scoped test overrides and the timing it had.
#[derive(Clone, Copy, Debug, Default)]
pub struct LocalJobSink;

impl JobSink for LocalJobSink {
    fn submit(&self, job: JobSubmission) -> JobTicket {
        let id = JobId::next();
        let (tx, rx) = std::sync::mpsc::channel();
        let _ = tx.send(run_job(job));
        JobTicket::new(id, rx)
    }
}
