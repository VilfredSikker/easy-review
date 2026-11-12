//! The job model: one provider run with a prompt, a workload and a stdout
//! contract.
//!
//! The engine builds a [`JobSpec`]; the runner owns the process, the slot and
//! the final-text decode. Three run modes exist: a plain blocking run; a
//! controlled run that streams both pipes and stops through the host's handle;
//! and a run that returns as soon as a `stream-json` result arrives (the card
//! reply).

use std::fmt;
use std::io::{BufRead, BufReader, Read};
use std::process::ExitStatus;
use std::sync::mpsc::{SyncSender, TrySendError};
use std::sync::Arc;
use std::time::Duration;

use crate::agent_run::AgentRunHandle;
use crate::agent_slots::{self, Workload};
use crate::agent_timing::AgentRunTimer;
use crate::decode::{decode_final_text, is_final_result_line, OutputProtocol};
use crate::provider::{self, ProviderSpec};

/// What the job is for. Mirrors the engine's `AgentTaskKind` so the two can be
/// mapped one-to-one; `Shell` covers the configured shell commands, which are
/// jobs the runner caps and stops but which are not agent tasks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobKind {
    Review,
    Expert(String),
    Professor,
    Triage,
    Tour { filename: String },
    Questions,
    Summary,
    ValidateReview,
    ValidateComments,
    CardReply,
    ArenaRound,
    Shell(String),
    Other(String),
}

/// What the job must produce. Typed artifacts arrive with the host-commit
/// work; this slice only decodes stdout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputContract {
    Stdout,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobSpec {
    pub kind: JobKind,
    pub provider: ProviderSpec,
    pub prompt: String,
    pub work_dir: String,
    pub workload: Workload,
    pub output: OutputContract,
}

#[derive(Debug)]
pub struct JobOutput {
    /// The provider's final text, decoded under its output protocol.
    pub text: String,
    pub stdout: String,
    pub stderr: String,
    pub status: ExitStatus,
}

#[derive(Debug)]
pub enum JobError {
    Spawn(anyhow::Error),
    TimedOut(Duration),
    /// The run was stopped, either while queued for a slot or while running.
    Cancelled,
}

impl fmt::Display for JobError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn(error) => write!(f, "job failed to spawn: {error}"),
            Self::TimedOut(limit) => write!(f, "job timed out after {limit:?}"),
            Self::Cancelled => write!(f, "job was cancelled"),
        }
    }
}

impl std::error::Error for JobError {}

impl From<provider::ProviderError> for JobError {
    fn from(error: provider::ProviderError) -> Self {
        match error {
            provider::ProviderError::Spawn(error) => Self::Spawn(error),
            provider::ProviderError::TimedOut(limit) => Self::TimedOut(limit),
        }
    }
}

/// One progress event from a running job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobProgress {
    StdoutLine(String),
    StderrLine(String),
}

/// The host-owned pieces of a run: the handle it keeps for a stop control,
/// the progress sink, and where to put the timing marks.
pub struct JobRun<'a> {
    pub handle: Arc<AgentRunHandle>,
    pub progress: Option<SyncSender<JobProgress>>,
    pub timer: Option<&'a mut AgentRunTimer>,
}

/// What a run that may return before exit came to.
#[derive(Debug)]
pub enum JobReply {
    /// The reply, handed back while the child may still be shutting down. The
    /// slot stays held until that child is reaped.
    Reply(String),
    /// The child closed stdout without a usable result and has been reaped, so
    /// the run settles on its exit status.
    Exited(JobOutput),
}

/// How a job that may be retried waits between attempts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Extra attempts after the first.
    pub retries: u32,
    /// The step: attempt `n` waits `backoff * (n + 1)`.
    pub backoff: Duration,
}

impl RetryPolicy {
    /// One attempt, no retry.
    pub const fn none() -> Self {
        Self {
            retries: 0,
            backoff: Duration::ZERO,
        }
    }

    /// The wait after the attempt numbered `attempt` (zero-based).
    pub fn wait_after(self, attempt: u32) -> Duration {
        self.backoff * (attempt.saturating_add(1))
    }
}

/// Runs one job at a time, drawing from the process-wide slot pool.
pub struct JobRunner {
    slot_cap: usize,
    ceiling: usize,
    retry: RetryPolicy,
}

impl JobRunner {
    /// `slot_cap` and `ceiling` are the engine's resolved limits for the runs
    /// this runner will start. They configure each acquisition from the one
    /// global pool; the runner never creates a pool of its own.
    pub fn new(slot_cap: usize, ceiling: usize) -> Self {
        Self {
            slot_cap,
            ceiling,
            retry: RetryPolicy::none(),
        }
    }

    /// A runner that retries a transient failure. The slot is released after
    /// each attempt, so a backoff sleep never holds capacity another run is
    /// waiting for.
    pub fn with_retry(slot_cap: usize, ceiling: usize, retry: RetryPolicy) -> Self {
        Self {
            slot_cap,
            ceiling,
            retry,
        }
    }

    /// Run to completion and decode the final text.
    pub fn run(&self, spec: &JobSpec) -> Result<JobOutput, JobError> {
        // One pool: the same process-wide semaphore every spawn site draws
        // from. A second pool here would break the cap ADR 0021 describes.
        let _slot = agent_slots::acquire_blocking(spec.workload, self.slot_cap, self.ceiling);
        let provider = Self::provider_for(spec);
        let output = provider::run(&provider, &spec.work_dir)?;
        let text = decode_final_text(&output.stdout, provider.protocol);
        Ok(JobOutput {
            text,
            stdout: output.stdout,
            stderr: output.stderr,
            status: output.status,
        })
    }

    /// Run to completion while streaming both pipes as progress, on a handle
    /// the caller owns. The slot wait is cancellable through `run.handle`.
    ///
    /// Each attempt runs on a fresh handle bridged to the caller's cancel
    /// flag. The caller's handle is the stop control; the attempt's handle owns
    /// the finished, timed-out and pid state its watchdog and kill consult, so
    /// a watchdog from an earlier attempt cannot fire into the next one and a
    /// retried attempt still has its own deadline.
    ///
    /// The verdict order is timeout first: a run killed by its deadline also
    /// has its cancel flag set, and reporting that as a user stop would tell
    /// the wrong story.
    pub fn run_controlled(&self, spec: &JobSpec, run: JobRun<'_>) -> Result<JobOutput, JobError> {
        let JobRun {
            handle,
            progress,
            mut timer,
        } = run;
        let mut attempt = 0;
        loop {
            if handle.is_cancelled() {
                return Err(JobError::Cancelled);
            }
            let attempt_handle = AgentRunHandle::new();
            let watch = CancelWatch::start(&handle, &attempt_handle);
            let result = self.run_controlled_once(
                spec,
                JobRun {
                    handle: Arc::clone(&attempt_handle),
                    progress: progress.clone(),
                    timer: timer.as_deref_mut(),
                },
                handle.cancel_flag(),
            );
            drop(watch);
            if attempt >= self.retry.retries || !Self::retryable(&result) {
                return result;
            }
            if !Self::wait_backoff(self.retry, &handle, attempt) {
                return Err(JobError::Cancelled);
            }
            attempt += 1;
        }
    }

    /// One attempt, holding its slot from acquisition to the child's reap.
    fn run_controlled_once(
        &self,
        spec: &JobSpec,
        run: JobRun<'_>,
        abort: &std::sync::atomic::AtomicBool,
    ) -> Result<JobOutput, JobError> {
        let JobRun {
            handle,
            progress,
            mut timer,
        } = run;
        // The caller's flag aborts the wait for a slot directly; the attempt
        // handle only starts mattering once the child exists.
        let _slot = agent_slots::acquire(spec.workload, self.slot_cap, self.ceiling, abort)
            .ok_or(JobError::Cancelled)?;
        if let Some(timer) = timer.as_mut() {
            timer.mark_slot_acquired();
        }

        let provider = Self::provider_for(spec);
        let mut child = provider::spawn_with(Arc::clone(&handle), &provider, &spec.work_dir)?;
        if let Some(timer) = timer.as_mut() {
            timer.mark_spawned();
        }
        let id = child.id();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        handle.register(child);

        let Some(stdout) = stdout else {
            Self::stop_and_reap(&handle, id);
            return Err(JobError::Spawn(anyhow::anyhow!(
                "provider stdout was not piped"
            )));
        };
        let progress_out = progress.clone();
        let handle_out = Arc::clone(&handle);
        let stdout_reader = std::thread::spawn(move || {
            read_progress_lines(stdout, progress_out, &handle_out, true)
        });
        let handle_err = Arc::clone(&handle);
        let stderr_reader = std::thread::spawn(move || {
            stderr.and_then(|pipe| read_progress_lines(pipe, progress, &handle_err, false))
        });

        let Some(child) = handle.take_child(id) else {
            // Nothing is left under the id to wait on; signal whatever the
            // handle still holds so the run cannot outlive this call.
            handle.kill();
            return Err(JobError::Spawn(anyhow::anyhow!(
                "provider: lost its own child"
            )));
        };
        let status = handle
            .wait_for(child)
            .map_err(|error| JobError::Spawn(error.into()))?;
        let stdout = stdout_reader.join().ok().flatten();
        let stderr = stderr_reader.join().ok().flatten();
        if let Some(timer) = timer.as_mut() {
            timer.mark_finished();
        }

        if handle.is_timed_out() {
            return Err(JobError::TimedOut(provider.timeout));
        }
        if handle.is_cancelled() {
            return Err(JobError::Cancelled);
        }
        let stdout = stdout.unwrap_or_default();
        let text = decode_final_text(&stdout, provider.protocol);
        Ok(JobOutput {
            text,
            stdout,
            stderr: stderr.unwrap_or_default(),
            status,
        })
    }

    /// Run until a usable `stream-json` result arrives, or until the child
    /// exits. Each stdout line is offered to `progress`.
    ///
    /// On a reply the slot, the rest of stdout and the stderr drain move to a
    /// reaper thread, so a CLI that answered but has not exited still counts
    /// against the cap. Dropping the pipe instead would kill it on its next
    /// write.
    pub fn run_until_result(
        &self,
        spec: &JobSpec,
        progress: Option<SyncSender<JobProgress>>,
    ) -> Result<JobReply, JobError> {
        let slot = agent_slots::acquire_blocking(spec.workload, self.slot_cap, self.ceiling);
        let provider = Self::provider_for(spec);
        let (run, mut child) = provider::spawn(&provider, &spec.work_dir)?;
        let id = child.id();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        run.register(child);

        let Some(stdout) = stdout else {
            return Err(JobError::Spawn(anyhow::anyhow!(
                "provider stdout was not piped"
            )));
        };
        // Drained alongside stdout: a child blocked on a full stderr pipe never
        // gets as far as its result line.
        let stderr_reader = std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut stderr) = stderr {
                let _ = stderr.read_to_end(&mut buf);
            }
            buf
        });

        let mut reader = BufReader::new(stdout);
        let mut seen = String::new();
        let mut line = Vec::new();
        loop {
            line.clear();
            match reader.read_until(b'\n', &mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            let text = String::from_utf8_lossy(&line).to_string();
            seen.push_str(&text);
            let is_final = provider.protocol == OutputProtocol::ClaudeStreamJson
                && is_final_result_line(&text);
            if !send_progress(progress.as_ref(), JobProgress::StdoutLine(text), &run) {
                return Err(JobError::TimedOut(provider.timeout));
            }
            if is_final {
                let reply = decode_final_text(&seen, provider.protocol);
                drop(progress);
                let run = Arc::clone(&run);
                std::thread::spawn(move || {
                    let _slot = slot;
                    let _ = std::io::copy(&mut reader, &mut std::io::sink());
                    let _ = stderr_reader.join();
                    if let Some(child) = run.take_child(id) {
                        let _ = run.wait_for(child);
                    }
                });
                return Ok(JobReply::Reply(reply));
            }
        }

        let stderr = stderr_reader.join().unwrap_or_default();
        let Some(child) = run.take_child(id) else {
            return Err(JobError::Spawn(anyhow::anyhow!(
                "provider: lost its own child"
            )));
        };
        let status = run
            .wait_for(child)
            .map_err(|error| JobError::Spawn(error.into()))?;
        if run.is_timed_out() {
            return Err(JobError::TimedOut(provider.timeout));
        }
        let stdout = seen;
        let text = decode_final_text(&stdout, provider.protocol);
        Ok(JobReply::Exited(JobOutput {
            text,
            stdout,
            stderr: String::from_utf8_lossy(&stderr).to_string(),
            status,
        }))
    }

    /// Stop a run whose caller is returning early and reap its child, so the
    /// pid cannot outlive the call.
    fn stop_and_reap(handle: &AgentRunHandle, id: u32) {
        handle.kill();
        if let Some(child) = handle.take_child(id) {
            let _ = handle.wait_for(child);
        }
    }

    fn provider_for(spec: &JobSpec) -> ProviderSpec {
        ProviderSpec {
            program: spec.provider.program.clone(),
            args: substituted_args(&spec.provider.args, &spec.prompt),
            env: spec.provider.env.clone(),
            protocol: spec.provider.protocol,
            timeout: spec.provider.timeout,
        }
    }

    /// Whether an attempt is worth repeating. A timeout or a transient exit is
    /// retried; a cancel, a spawn failure or a fatal exit is not.
    fn retryable(result: &Result<JobOutput, JobError>) -> bool {
        match result {
            Err(JobError::TimedOut(_)) => true,
            Err(JobError::Cancelled | JobError::Spawn(_)) => false,
            Ok(output) => !output.status.success() && stderr_looks_transient(&output.stderr),
        }
    }

    /// Wait between attempts in short slices, so a cancel ends the wait at
    /// once instead of after the whole backoff.
    fn wait_backoff(retry: RetryPolicy, handle: &AgentRunHandle, attempt: u32) -> bool {
        let wait = retry.wait_after(attempt);
        let step = Duration::from_millis(25);
        let started = std::time::Instant::now();
        while started.elapsed() < wait {
            if handle.is_cancelled() {
                return false;
            }
            let left = wait.saturating_sub(started.elapsed());
            std::thread::sleep(step.min(left));
        }
        !handle.is_cancelled()
    }
}

/// Stderr markers the arena adapter treated as transient.
fn stderr_looks_transient(stderr: &str) -> bool {
    let text = stderr.to_lowercase();
    ["rate limit", "429", "timed out", "temporar"]
        .iter()
        .any(|marker| text.contains(marker))
}

/// Maps the caller's stop flag onto one attempt's fresh handle.
///
/// The caller kills its own handle; that handle has no child in a retried
/// attempt, so the attempt's handle has to be told separately. Polling keeps
/// the attempt's own finished/pid state authoritative for its watchdog.
struct CancelWatch {
    stop: Arc<std::sync::atomic::AtomicBool>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl CancelWatch {
    fn start(control: &Arc<AgentRunHandle>, attempt: &Arc<AgentRunHandle>) -> Self {
        use std::sync::atomic::{AtomicBool, Ordering};
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        let control = Arc::clone(control);
        let attempt = Arc::clone(attempt);
        let join = std::thread::spawn(move || {
            while !stopped.load(Ordering::SeqCst) {
                if control.is_cancelled() {
                    attempt.kill();
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        Self {
            stop,
            join: Some(join),
        }
    }
}

impl Drop for CancelWatch {
    fn drop(&mut self) {
        use std::sync::atomic::Ordering;
        self.stop.store(true, Ordering::SeqCst);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

/// Offer one event to the progress sink without blocking past the run's
/// deadline. A slow or absent drain turns into the provider timeout rather
/// than a thread stuck on the channel.
fn send_progress(
    progress: Option<&SyncSender<JobProgress>>,
    event: JobProgress,
    run: &AgentRunHandle,
) -> bool {
    let Some(tx) = progress else {
        return true;
    };
    let mut event = event;
    loop {
        match tx.try_send(event) {
            Ok(()) | Err(TrySendError::Disconnected(_)) => return true,
            Err(TrySendError::Full(returned)) => {
                // A stopped run stops reading too: a stalled sink must not
                // keep a cancelled job alive until its deadline.
                if run.is_timed_out() || run.is_cancelled() {
                    return false;
                }
                event = returned;
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }
}

/// Read one pipe line by line, forwarding each line as progress and returning
/// the full text. `None` when the run timed out while the sink was full.
fn read_progress_lines(
    pipe: impl Read,
    progress: Option<SyncSender<JobProgress>>,
    run: &AgentRunHandle,
    stdout: bool,
) -> Option<String> {
    let mut reader = BufReader::new(pipe);
    let mut seen = Vec::new();
    let mut line = Vec::new();
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        seen.extend_from_slice(&line);
        let text = String::from_utf8_lossy(&line).to_string();
        let event = if stdout {
            JobProgress::StdoutLine(text)
        } else {
            JobProgress::StderrLine(text)
        };
        if !send_progress(progress.as_ref(), event, run) {
            return None;
        }
    }
    Some(String::from_utf8_lossy(&seen).to_string())
}

/// Substitute `{prompt}` in the provider's args. Without a placeholder, a
/// non-empty prompt is appended, matching the config's argv builder.
fn substituted_args(args: &[String], prompt: &str) -> Vec<String> {
    let has_placeholder = args.iter().any(|arg| arg.contains("{prompt}"));
    let mut out: Vec<String> = args
        .iter()
        .map(|arg| arg.replace("{prompt}", prompt))
        .collect();
    if !has_placeholder && !prompt.is_empty() {
        out.push(prompt.to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};

    fn prompt_spec(program: &str, args: Vec<String>, dir: &std::path::Path) -> JobSpec {
        JobSpec {
            kind: JobKind::CardReply,
            provider: ProviderSpec {
                program: program.into(),
                args,
                env: Vec::new(),
                protocol: OutputProtocol::Plain,
                timeout: Duration::from_secs(10),
            },
            prompt: String::new(),
            work_dir: dir.to_string_lossy().into_owned(),
            workload: Workload::Background,
            output: OutputContract::Stdout,
        }
    }

    #[test]
    fn prompt_substitution_replaces_a_placeholder_or_appends() {
        let replaced = substituted_args(&["run".into(), "{prompt}".into()], "hello");
        assert_eq!(replaced, ["run", "hello"]);
        let appended = substituted_args(&["run".into()], "hello");
        assert_eq!(appended, ["run", "hello"]);
        let empty = substituted_args(&["run".into()], "");
        assert_eq!(empty, ["run"]);
    }

    /// The runner draws from the process-wide pool: holding that pool through
    /// another `agent_slots` call site must park it.
    #[test]
    fn the_runner_waits_for_a_slot() {
        let _serial = agent_slots::test_serial();
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("ran");
        let runner = JobRunner::new(1, 1);
        let spec = prompt_spec(
            "sh",
            vec![
                "-c".into(),
                format!("printf x >> '{}'; printf done", marker.display()),
            ],
            dir.path(),
        );

        let held = agent_slots::acquire_blocking(Workload::Background, 1, 1);
        let handle = std::thread::spawn(move || runner.run(&spec));

        std::thread::sleep(Duration::from_millis(250));
        assert!(
            !marker.exists(),
            "the job ran while the process-wide pool was held"
        );

        drop(held);
        let out = handle.join().unwrap().expect("job runs");
        assert!(marker.exists(), "the job did not run after the slot freed");
        assert_eq!(out.text, "done");
    }

    /// A retried job releases its slot during the backoff: a second job on a
    /// one-slot pool runs between the first job's attempts.
    #[cfg(unix)]
    #[test]
    fn a_retry_waits_without_holding_the_slot() {
        let _serial = agent_slots::test_serial();
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("order.log");
        let counter = dir.path().join("attempts");
        let retrying = prompt_spec(
            "sh",
            vec![
                "-c".into(),
                format!(
                    "n=$(cat '{counter}' 2>/dev/null || echo 0); n=$((n+1)); echo $n > '{counter}'; \
                     echo a$n >> '{log}'; \
                     if [ $n -eq 1 ]; then echo 'rate limit' >&2; exit 1; fi; echo ok",
                    counter = counter.display(),
                    log = log.display()
                ),
            ],
            dir.path(),
        );
        let other = prompt_spec(
            "sh",
            vec![
                "-c".into(),
                format!("echo b >> '{}'; echo ok", log.display()),
            ],
            dir.path(),
        );
        let runner = JobRunner::with_retry(
            1,
            1,
            RetryPolicy {
                retries: 1,
                backoff: Duration::from_millis(400),
            },
        );
        let other_runner = JobRunner::new(1, 1);

        let handle = std::thread::spawn(move || {
            let mut timer = AgentRunTimer::start();
            runner.run_controlled(
                &retrying,
                JobRun {
                    handle: AgentRunHandle::new(),
                    progress: None,
                    timer: Some(&mut timer),
                },
            )
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !log.exists() {
            assert!(
                std::time::Instant::now() < deadline,
                "first attempt never ran"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        // The first attempt has failed and the first job is backing off; a
        // one-slot pool must be free for the second job now.
        other_runner.run(&other).expect("the second job runs");
        handle.join().unwrap().expect("the retry succeeds");

        let order = std::fs::read_to_string(&log).unwrap();
        let lines: Vec<&str> = order.lines().collect();
        assert_eq!(
            lines,
            ["a1", "b", "a2"],
            "the backoff held the slot: {order}"
        );
    }

    /// A cancel during the backoff ends the retry at once, and no further
    /// attempt runs.
    #[cfg(unix)]
    #[test]
    fn a_cancel_ends_a_retry_backoff_at_once() {
        let _serial = agent_slots::test_serial();
        let dir = tempfile::tempdir().unwrap();
        let counter = dir.path().join("attempts");
        let exited = dir.path().join("attempt-exited");
        let spec = prompt_spec(
            "sh",
            vec![
                "-c".into(),
                format!(
                    "n=$(cat '{counter}' 2>/dev/null || echo 0); n=$((n+1)); echo $n > '{counter}'; \
                     echo 'rate limit' >&2; : > '{exited}'; exit 1",
                    counter = counter.display(),
                    exited = exited.display()
                ),
            ],
            dir.path(),
        );
        let runner = JobRunner::with_retry(
            1,
            1,
            RetryPolicy {
                retries: 3,
                backoff: Duration::from_secs(2),
            },
        );
        let run = AgentRunHandle::new();
        let cancel = Arc::clone(&run);
        let handle = std::thread::spawn(move || {
            let mut timer = AgentRunTimer::start();
            runner.run_controlled(
                &spec,
                JobRun {
                    handle: run,
                    progress: None,
                    timer: Some(&mut timer),
                },
            )
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        // Wait for the first attempt to have exited *on its own*. The marker is
        // its last line, and the settle gives the runner time to reap it and
        // enter the backoff; cancelling earlier would kill the attempt and pass
        // without testing the backoff at all.
        while !exited.exists() {
            assert!(
                std::time::Instant::now() < deadline,
                "first attempt never ran"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(100));

        let cancelling = std::time::Instant::now();
        cancel
            .cancel_flag()
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let result = handle.join().unwrap();
        let elapsed = cancelling.elapsed();

        assert!(matches!(result, Err(JobError::Cancelled)), "{result:?}");
        assert!(
            elapsed < Duration::from_millis(300),
            "the backoff ignored the cancel: {elapsed:?}"
        );
        assert_eq!(
            std::fs::read_to_string(&counter).unwrap().trim(),
            "1",
            "a cancelled backoff ran another attempt"
        );
    }

    /// Each attempt carries its own deadline: a retry that overruns its
    /// timeout is killed by it, not left running.
    #[cfg(unix)]
    #[test]
    fn a_retried_attempt_has_its_own_timeout() {
        let _serial = agent_slots::test_serial();
        let dir = tempfile::tempdir().unwrap();
        let counter = dir.path().join("attempts");
        let mut spec = prompt_spec(
            "sh",
            vec![
                "-c".into(),
                format!(
                    "n=$(cat '{counter}' 2>/dev/null || echo 0); n=$((n+1)); echo $n > '{counter}'; \
                     if [ $n -eq 1 ]; then echo 'rate limit' >&2; exit 1; fi; sleep 5; echo ok",
                    counter = counter.display()
                ),
            ],
            dir.path(),
        );
        spec.provider.timeout = Duration::from_millis(300);
        let runner = JobRunner::with_retry(
            1,
            1,
            RetryPolicy {
                retries: 1,
                backoff: Duration::from_millis(50),
            },
        );

        let started = std::time::Instant::now();
        let result = runner.run_controlled(
            &spec,
            JobRun {
                handle: AgentRunHandle::new(),
                progress: None,
                timer: None,
            },
        );

        assert!(matches!(result, Err(JobError::TimedOut(_))), "{result:?}");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "the retried attempt had no deadline: {:?}",
            started.elapsed()
        );
    }

    /// A cancel during a retried attempt reaches that attempt's process group.
    #[cfg(unix)]
    #[test]
    fn a_cancel_during_a_retried_attempt_kills_it_at_once() {
        let _serial = agent_slots::test_serial();
        let dir = tempfile::tempdir().unwrap();
        let counter = dir.path().join("attempts");
        let pid_file = dir.path().join("pid");
        let mut spec = prompt_spec(
            "sh",
            vec![
                "-c".into(),
                format!(
                    "n=$(cat '{counter}' 2>/dev/null || echo 0); n=$((n+1)); echo $n > '{counter}'; \
                     if [ $n -eq 1 ]; then echo 'rate limit' >&2; exit 1; fi; \
                     echo $$ > '{pid}'; sleep 5; echo ok",
                    counter = counter.display(),
                    pid = pid_file.display()
                ),
            ],
            dir.path(),
        );
        spec.provider.timeout = Duration::from_secs(30);
        let runner = JobRunner::with_retry(
            1,
            1,
            RetryPolicy {
                retries: 1,
                backoff: Duration::from_millis(50),
            },
        );
        let control = AgentRunHandle::new();
        let cancel = Arc::clone(&control);
        let handle = std::thread::spawn(move || {
            runner.run_controlled(
                &spec,
                JobRun {
                    handle: control,
                    progress: None,
                    timer: None,
                },
            )
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !pid_file.exists() {
            assert!(
                std::time::Instant::now() < deadline,
                "the retried attempt never started"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        let pid = std::fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .to_string();

        let cancelling = std::time::Instant::now();
        cancel
            .cancel_flag()
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let result = handle.join().unwrap();
        let elapsed = cancelling.elapsed();

        assert!(matches!(result, Err(JobError::Cancelled)), "{result:?}");
        assert!(
            elapsed < Duration::from_millis(500),
            "the cancel did not reach the retried attempt: {elapsed:?}"
        );
        assert!(
            !pid_alive(&pid),
            "the retried attempt's child survived the cancel"
        );
    }

    /// A retry that succeeds is a success: the timed-out first attempt's verdict
    /// does not follow it forward.
    #[cfg(unix)]
    #[test]
    fn a_retry_that_succeeds_is_not_reported_timed_out() {
        let _serial = agent_slots::test_serial();
        let dir = tempfile::tempdir().unwrap();
        let counter = dir.path().join("attempts");
        let mut spec = prompt_spec(
            "sh",
            vec![
                "-c".into(),
                format!(
                    "n=$(cat '{counter}' 2>/dev/null || echo 0); n=$((n+1)); echo $n > '{counter}'; \
                     if [ $n -eq 1 ]; then sleep 5; exit 0; fi; echo ok",
                    counter = counter.display()
                ),
            ],
            dir.path(),
        );
        spec.provider.timeout = Duration::from_millis(300);
        let runner = JobRunner::with_retry(
            1,
            1,
            RetryPolicy {
                retries: 1,
                backoff: Duration::from_millis(50),
            },
        );

        let started = std::time::Instant::now();
        let result = runner.run_controlled(
            &spec,
            JobRun {
                handle: AgentRunHandle::new(),
                progress: None,
                timer: None,
            },
        );

        let output = result.expect("the retry succeeds");
        assert_eq!(output.text.trim(), "ok");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "the retry did not settle promptly: {:?}",
            started.elapsed()
        );
    }

    /// The point of the early reply: hand it back on the result line, then keep
    /// the slot until the child is reaped. The stderr flood proves the drain.
    #[cfg(unix)]
    #[test]
    fn a_reply_returns_on_the_result_line_and_the_slot_waits_for_exit() {
        let _serial = agent_slots::test_serial();
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pid");
        let script = format!(
            "echo $$ > {pid}; \
             head -c 131072 /dev/zero >&2; \
             echo '{{\"type\":\"assistant\",\"message\":{{\"content\":[{{\"type\":\"text\",\"text\":\"working\"}}]}}}}'; \
             echo '{{\"type\":\"result\",\"subtype\":\"success\",\"result\":\"fast reply\"}}'; \
             sleep 3",
            pid = pid_file.display()
        );
        let mut spec = prompt_spec("sh", vec!["-c".into(), script], dir.path());
        spec.provider.protocol = OutputProtocol::ClaudeStreamJson;
        let runner = JobRunner::new(1, 1);

        let started = std::time::Instant::now();
        let reply = runner.run_until_result(&spec, None).expect("a reply");
        let elapsed = started.elapsed();

        let JobReply::Reply(text) = reply else {
            panic!("expected a reply, got {reply:?}");
        };
        assert_eq!(text, "fast reply");
        assert!(
            elapsed < Duration::from_secs(2),
            "the reply waited for the child to exit: {elapsed:?}"
        );
        let pid = std::fs::read_to_string(&pid_file).expect("child wrote its pid");
        let pid = pid.trim().to_string();
        assert!(pid_alive(&pid), "the child should still be sleeping");
        assert_eq!(
            agent_slots::active_count(),
            1,
            "the slot was released while the child was still running"
        );

        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while agent_slots::active_count() > 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(
            agent_slots::active_count(),
            0,
            "the slot was never released"
        );
        assert!(
            !pid_alive(&pid),
            "the slot was released before the child was reaped"
        );
    }

    /// A child that never prints a `result` line settles on its exit status.
    #[cfg(unix)]
    #[test]
    fn a_child_exiting_without_a_result_line_reports_its_exit() {
        let _serial = agent_slots::test_serial();
        let dir = tempfile::tempdir().unwrap();
        let mut spec = prompt_spec(
            "sh",
            vec![
                "-c".into(),
                "echo '{\"type\":\"assistant\",\"message\":{\"content\":[]}}'; echo boom >&2; exit 3"
                    .into(),
            ],
            dir.path(),
        );
        spec.provider.protocol = OutputProtocol::ClaudeStreamJson;
        let runner = JobRunner::new(1, 1);

        let reply = runner.run_until_result(&spec, None).expect("the exit path");
        let JobReply::Exited(output) = reply else {
            panic!("expected an exit, got {reply:?}");
        };
        assert_eq!(output.status.code(), Some(3));
        assert_eq!(output.stderr.trim(), "boom");
        assert_eq!(agent_slots::active_count(), 0);
    }

    /// The controlled path draws from the same process-wide pool: holding it
    /// through another `agent_slots` call site must park the job.
    #[test]
    fn a_controlled_job_waits_for_a_slot() {
        let _serial = agent_slots::test_serial();
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("ran");
        let runner = JobRunner::new(1, 1);
        let spec = prompt_spec(
            "sh",
            vec![
                "-c".into(),
                format!("printf x >> '{}'; printf done", marker.display()),
            ],
            dir.path(),
        );

        let held = agent_slots::acquire_blocking(Workload::Background, 1, 1);
        let handle = std::thread::spawn(move || {
            runner.run_controlled(
                &spec,
                JobRun {
                    handle: AgentRunHandle::new(),
                    progress: None,
                    timer: None,
                },
            )
        });

        std::thread::sleep(Duration::from_millis(250));
        assert!(
            !marker.exists(),
            "the job ran while the process-wide pool was held"
        );

        drop(held);
        let out = handle.join().unwrap().expect("job runs");
        assert!(marker.exists(), "the job did not run after the slot freed");
        assert_eq!(out.text, "done");
    }

    /// A run past its deadline is a timeout, and the slot it held is freed.
    #[test]
    fn a_run_past_its_deadline_times_out_and_frees_its_slot() {
        let _serial = agent_slots::test_serial();
        let dir = tempfile::tempdir().unwrap();
        let mut spec = prompt_spec("sh", vec!["-c".into(), "sleep 5".into()], dir.path());
        spec.provider.timeout = Duration::from_millis(200);
        let runner = JobRunner::new(1, 1);

        let started = std::time::Instant::now();
        let result = runner.run_controlled(
            &spec,
            JobRun {
                handle: AgentRunHandle::new(),
                progress: None,
                timer: None,
            },
        );
        let elapsed = started.elapsed();
        assert!(
            matches!(result, Err(JobError::TimedOut(limit)) if limit == spec.provider.timeout),
            "{result:?}"
        );
        assert!(
            elapsed < Duration::from_secs(3),
            "the deadline took {elapsed:?}"
        );
        assert_eq!(agent_slots::active_count(), 0, "the slot leaked");
        drop(agent_slots::acquire_blocking(Workload::Background, 1, 1));
    }

    /// A stop while the job is queued ends it promptly instead of parking
    /// until a slot frees.
    #[test]
    fn a_stop_while_waiting_for_a_slot_reports_cancelled() {
        let _serial = agent_slots::test_serial();
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("ran");
        let runner = JobRunner::new(1, 1);
        let spec = prompt_spec(
            "sh",
            vec!["-c".into(), format!("printf x >> '{}'", marker.display())],
            dir.path(),
        );

        let held = agent_slots::acquire_blocking(Workload::Background, 1, 1);
        let run = AgentRunHandle::new();
        let stop = Arc::clone(&run);
        let handle = std::thread::spawn(move || {
            runner.run_controlled(
                &spec,
                JobRun {
                    handle: run,
                    progress: None,
                    timer: None,
                },
            )
        });
        std::thread::sleep(Duration::from_millis(200));
        stop.kill();
        // Free the slot so a runner that ignored the stop runs the job and
        // shows it, instead of parking until the suite gives up.
        drop(held);

        let result = handle.join().expect("the waiter returns");
        assert!(matches!(result, Err(JobError::Cancelled)), "{result:?}");
        assert!(!marker.exists(), "a stopped waiter still ran its job");
    }

    /// A stop after the child spawned is a cancellation, not the exit it
    /// happened to leave behind.
    #[cfg(unix)]
    #[test]
    fn a_stopped_run_reports_cancelled() {
        let _serial = agent_slots::test_serial();
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pid");
        let spec = prompt_spec(
            "sh",
            vec![
                "-c".into(),
                format!("sleep 30 & echo $! > {}; wait", pid_file.display()),
            ],
            dir.path(),
        );
        let runner = JobRunner::new(1, 1);
        let run = AgentRunHandle::new();
        let stop = Arc::clone(&run);
        let handle = std::thread::spawn(move || {
            runner.run_controlled(
                &spec,
                JobRun {
                    handle: run,
                    progress: None,
                    timer: None,
                },
            )
        });

        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !pid_file.exists() {
            assert!(
                std::time::Instant::now() < deadline,
                "the child never started"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        stop.kill();

        let result = handle.join().expect("the run returns");
        assert!(matches!(result, Err(JobError::Cancelled)), "{result:?}");
        let pid = std::fs::read_to_string(&pid_file).unwrap();
        let pid = pid.trim();
        let mut guard = GrandchildGuard(pid.parse().ok());
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while pid_alive(pid) && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!pid_alive(pid), "the child survived its stop");
        guard.disarm();
    }

    /// A stalled progress sink must not keep a cancelled run alive until its
    /// deadline.
    #[test]
    fn a_cancelled_run_stops_a_stalled_progress_sink() {
        let _serial = agent_slots::test_serial();
        let dir = tempfile::tempdir().unwrap();
        let mut spec = prompt_spec(
            "sh",
            vec![
                "-c".into(),
                "i=0; while [ $i -lt 1000 ]; do echo line; i=$((i+1)); done; sleep 5".into(),
            ],
            dir.path(),
        );
        spec.provider.timeout = Duration::from_secs(2);
        let runner = JobRunner::new(1, 1);
        let run = AgentRunHandle::new();
        let stop = Arc::clone(&run);
        // Capacity one and nobody draining: the reader parks on the second
        // line until the run is stopped.
        let (tx, _rx) = std::sync::mpsc::sync_channel(1);
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let handle = std::thread::spawn(move || {
            let result = runner.run_controlled(
                &spec,
                JobRun {
                    handle: run,
                    progress: Some(tx),
                    timer: None,
                },
            );
            let _ = done_tx.send(result);
        });
        std::thread::sleep(Duration::from_millis(300));
        let stopped = std::time::Instant::now();
        stop.kill();
        // Bounded receive, not a join: without the cancel check the reader
        // retries forever once the killed child has been reaped, because the
        // timeout watchdog stands down at that point.
        let result = done_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("a cancelled run with a stalled sink stayed alive");
        assert!(matches!(result, Err(JobError::Cancelled)), "{result:?}");
        assert!(
            stopped.elapsed() < Duration::from_secs(1),
            "the stop waited for the provider timeout: {:?}",
            stopped.elapsed()
        );
        // The run returned, so the worker thread is done. Leave the handle out
        // of the plant's failure path: a join there would hang the suite.
        drop(handle);
    }

    /// Both pipes reach the progress sink while the job runs, and the returned
    /// output holds the full text.
    #[test]
    fn a_controlled_run_streams_stdout_and_stderr() {
        let _serial = agent_slots::test_serial();
        let dir = tempfile::tempdir().unwrap();
        let spec = prompt_spec(
            "sh",
            vec!["-c".into(), "printf 'out\\n'; printf 'err\\n' >&2".into()],
            dir.path(),
        );
        let runner = JobRunner::new(1, 1);
        let (tx, rx) = std::sync::mpsc::sync_channel(16);

        let out = runner
            .run_controlled(
                &spec,
                JobRun {
                    handle: AgentRunHandle::new(),
                    progress: Some(tx),
                    timer: None,
                },
            )
            .expect("the run completes");

        let events: Vec<JobProgress> = rx.try_iter().collect();
        assert!(
            events.contains(&JobProgress::StdoutLine("out\n".into())),
            "{events:?}"
        );
        assert!(
            events.contains(&JobProgress::StderrLine("err\n".into())),
            "{events:?}"
        );
        assert_eq!(out.stdout, "out\n");
        assert_eq!(out.stderr, "err\n");
        assert_eq!(out.text, "out\n");
    }

    /// Kills the child on drop unless disarmed, so a failing assertion leaves
    /// no sleeper behind.
    #[cfg(unix)]
    struct GrandchildGuard(Option<i32>);

    #[cfg(unix)]
    impl GrandchildGuard {
        fn disarm(&mut self) {
            self.0 = None;
        }
    }

    #[cfg(unix)]
    impl Drop for GrandchildGuard {
        fn drop(&mut self) {
            if let Some(pid) = self.0 {
                let _ = Command::new("/bin/kill")
                    .args(["-KILL", &pid.to_string()])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
        }
    }

    #[cfg(unix)]
    fn pid_alive(pid: &str) -> bool {
        Command::new("/bin/kill")
            .args(["-0", pid])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }
}
