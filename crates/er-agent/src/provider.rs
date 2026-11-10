//! What to run: the provider program and its fixed inputs.
//!
//! The engine resolves a provider, model and effort into these fields; the
//! runtime only spawns what it is given. Keeping the type here is what lets
//! `er-agent` stay free of the engine's config, AI and app modules.
//!
//! [`spawn`] and [`run`] take no slot: the caller decides whether a run is
//! charged to the process-wide pool. [`crate::job::JobRunner`] is the
//! slot-holding caller; model discovery deliberately is not.

use std::fmt;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use crate::agent_run::AgentRunHandle;
use crate::decode::OutputProtocol;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSpec {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub protocol: OutputProtocol,
    pub timeout: Duration,
}

#[derive(Debug)]
pub struct ProviderOutput {
    pub stdout: String,
    pub stderr: String,
    pub status: std::process::ExitStatus,
}

#[derive(Debug)]
pub enum ProviderError {
    Spawn(anyhow::Error),
    TimedOut(Duration),
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn(error) => write!(f, "failed to spawn: {error}"),
            Self::TimedOut(limit) => write!(f, "timed out after {limit:?}"),
        }
    }
}

impl std::error::Error for ProviderError {}

/// Spawn the provider and arm its timeout. No slot.
///
/// `stdin` is null: every provider this runtime starts is given its prompt in
/// argv, so nothing reads stdin, and a CLI that decided to ask a question
/// fails instead of blocking past the deadline.
pub fn spawn(
    spec: &ProviderSpec,
    work_dir: &str,
) -> Result<(Arc<AgentRunHandle>, Child), ProviderError> {
    let run = AgentRunHandle::new();
    let child = spawn_with(Arc::clone(&run), spec, work_dir)?;
    Ok((run, child))
}

/// Spawn the provider on a handle the caller already holds, so a host can keep
/// a stop control on the same run. Null stdin, env, process group and timeout
/// arming are the same as [`spawn`].
pub fn spawn_with(
    run: Arc<AgentRunHandle>,
    spec: &ProviderSpec,
    work_dir: &str,
) -> Result<Child, ProviderError> {
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args)
        .current_dir(work_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in &spec.env {
        cmd.env(key, value);
    }
    let child = run.spawn(&mut cmd).map_err(ProviderError::Spawn)?;
    run.arm_timeout(spec.timeout);
    Ok(child)
}

/// Run to completion, draining both pipes concurrently. No slot.
pub fn run(spec: &ProviderSpec, work_dir: &str) -> Result<ProviderOutput, ProviderError> {
    let (run, child) = spawn(spec, work_dir)?;
    let output = run
        .wait_for_output(child)
        .map_err(|error| ProviderError::Spawn(error.into()))?;
    if run.is_timed_out() {
        return Err(ProviderError::TimedOut(spec.timeout));
    }
    Ok(ProviderOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        status: output.status,
    })
}
