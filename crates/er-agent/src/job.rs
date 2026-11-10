//! The job model: one provider run with a prompt, a workload and a stdout
//! contract.
//!
//! The engine builds a [`JobSpec`]; the runner owns the process, the slot and
//! the final-text decode. Two run modes exist: a blocking run that waits for
//! exit, and a run that returns as soon as a `stream-json` result arrives (the
//! card reply). The queue and a cancel handle arrive with the background worker
//! port.

use std::fmt;
use std::io::{BufRead, BufReader, Read};
use std::process::ExitStatus;
use std::sync::mpsc::{SyncSender, TrySendError};
use std::sync::Arc;
use std::time::Duration;

use crate::agent_run::AgentRunHandle;
use crate::agent_slots::{self, Workload};
use crate::decode::{decode_final_text, is_final_result_line, OutputProtocol};
use crate::provider::{self, ProviderSpec};

/// What the job is for. Mirrors the engine's `AgentTaskKind` so the two can be
/// mapped one-to-one.
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
}

impl fmt::Display for JobError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn(error) => write!(f, "job failed to spawn: {error}"),
            Self::TimedOut(limit) => write!(f, "job timed out after {limit:?}"),
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

/// Runs one job at a time, drawing from the process-wide slot pool.
pub struct JobRunner {
    slot_cap: usize,
    ceiling: usize,
}

impl JobRunner {
    /// `slot_cap` and `ceiling` are the engine's resolved limits for the runs
    /// this runner will start. They configure each acquisition from the one
    /// global pool; the runner never creates a pool of its own.
    pub fn new(slot_cap: usize, ceiling: usize) -> Self {
        Self { slot_cap, ceiling }
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
            if !send_progress(progress.as_ref(), &text, &run) {
                return Err(JobError::TimedOut(provider.timeout));
            }
            if provider.protocol == OutputProtocol::ClaudeStreamJson && is_final_result_line(&text)
            {
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

    fn provider_for(spec: &JobSpec) -> ProviderSpec {
        ProviderSpec {
            program: spec.provider.program.clone(),
            args: substituted_args(&spec.provider.args, &spec.prompt),
            env: spec.provider.env.clone(),
            protocol: spec.provider.protocol,
            timeout: spec.provider.timeout,
        }
    }
}

/// Offer one line to the progress sink without blocking past the run's
/// deadline. A slow or absent drain turns into the provider timeout rather
/// than a thread stuck on the channel.
fn send_progress(
    progress: Option<&SyncSender<JobProgress>>,
    line: &str,
    run: &AgentRunHandle,
) -> bool {
    let Some(tx) = progress else {
        return true;
    };
    let mut event = JobProgress::StdoutLine(line.to_string());
    loop {
        match tx.try_send(event) {
            Ok(()) | Err(TrySendError::Disconnected(_)) => return true,
            Err(TrySendError::Full(returned)) => {
                if run.is_timed_out() {
                    return false;
                }
                event = returned;
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }
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
