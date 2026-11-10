//! The job model: one provider run with a prompt, a workload and a stdout
//! contract.
//!
//! The engine builds a [`JobSpec`]; the runner owns the process, the slot and
//! the final-text decode. This is the first slice: a blocking run with the
//! provider's timeout. The queue and a cancel handle arrive with the background
//! worker port.

use std::fmt;
use std::process::{Command, ExitStatus, Stdio};
use std::time::Duration;

use crate::agent_run::AgentRunHandle;
use crate::agent_slots::{self, Workload};
use crate::decode::decode_final_text;
use crate::provider::ProviderSpec;

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

    pub fn run(&self, spec: &JobSpec) -> Result<JobOutput, JobError> {
        // One pool: the same process-wide semaphore every spawn site draws
        // from. A second pool here would break the cap ADR 0021 describes.
        let _slot = agent_slots::acquire_blocking(spec.workload, self.slot_cap, self.ceiling);

        let args = substituted_args(&spec.provider.args, &spec.prompt);
        let run = AgentRunHandle::new();
        let mut cmd = Command::new(&spec.provider.program);
        cmd.args(&args)
            .current_dir(&spec.work_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in &spec.provider.env {
            cmd.env(key, value);
        }

        let child = run.spawn(&mut cmd).map_err(JobError::Spawn)?;
        run.arm_timeout(spec.provider.timeout);
        let output = run
            .wait_for_output(child)
            .map_err(|error| JobError::Spawn(error.into()))?;

        if run.is_timed_out() {
            return Err(JobError::TimedOut(spec.provider.timeout));
        }

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let text = decode_final_text(&stdout, spec.provider.protocol);
        Ok(JobOutput {
            text,
            stdout,
            stderr,
            status: output.status,
        })
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
        let spec = JobSpec {
            kind: JobKind::CardReply,
            provider: ProviderSpec {
                program: "sh".into(),
                args: vec![
                    "-c".into(),
                    format!("printf x >> '{}'; printf done", marker.display()),
                ],
                env: Vec::new(),
                protocol: crate::decode::OutputProtocol::Plain,
                timeout: Duration::from_secs(10),
            },
            prompt: String::new(),
            work_dir: dir.path().to_string_lossy().into_owned(),
            workload: Workload::Background,
            output: OutputContract::Stdout,
        };

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
}
