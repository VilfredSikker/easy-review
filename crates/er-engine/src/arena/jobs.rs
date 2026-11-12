//! Arena reviewer and arbiter calls as jobs on the shared runner.
//!
//! The arena builds a [`JobSpec`] and hands it to `er_agent::JobRunner`; the
//! runner owns the slot, the process group, the deadline, the retry and the
//! final-text decode. What stays here is arena-specific: the prompts' JSON
//! extraction, the cancel bridge that maps the run's flag onto each job's
//! handle, and the test hook that answers a round from a fixture directory.

use crate::agent_runtime::provider_spec;
use crate::agent_slots::Workload;
use crate::agent_timing::AgentRunTimer;
use crate::config::ErConfig;
use anyhow::{Context, Result};
use er_agent::agent_run::AgentRunHandle;
use er_agent::job::{JobError, JobKind, JobRun, JobRunner, JobSpec, OutputContract, RetryPolicy};
use er_agent::provider::ProviderSpec;
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// The retry the arena has always used: two extra attempts, a 500 ms step.
const RETRIES: u32 = 2;
const RETRY_STEP: Duration = Duration::from_millis(500);

/// How often the cancel bridge checks the run flag.
const CANCEL_POLL: Duration = Duration::from_millis(20);

/// Why one reviewer's job did not produce a value.
pub(super) enum JobFailure {
    Cancelled,
    Failed(String),
}

/// Build the job one reviewer or the arbiter runs.
pub(crate) fn arena_job(
    config: &ErConfig,
    provider_id: &str,
    model_id: &str,
    effort: Option<&str>,
    storage_dir: &str,
    prompt: &str,
    work_dir: &str,
) -> Result<JobSpec> {
    let command = provider_spec(
        &config.ai_hub,
        provider_id,
        model_id,
        effort,
        Some(storage_dir),
    )?;
    Ok(JobSpec {
        kind: JobKind::ArenaRound,
        provider: ProviderSpec {
            program: command.program,
            args: command.args,
            env: command.env,
            protocol: command.protocol,
            timeout: command.timeout,
        },
        prompt: prompt.to_string(),
        work_dir: work_dir.to_string(),
        workload: Workload::Arena,
        output: OutputContract::Stdout,
    })
}

/// A runner that retries a transient reviewer failure the way the arena always
/// has. The runner releases the slot between attempts.
pub(super) fn arena_runner(config: &ErConfig) -> JobRunner {
    JobRunner::with_retry(
        config.ai_hub.effective_max_concurrent_arena_reviews(),
        config.ai_hub.effective_max_concurrent_agents(),
        RetryPolicy {
            retries: RETRIES,
            backoff: RETRY_STEP,
        },
    )
}

/// Run one arena job, honouring the run's cancel flag and returning the JSON
/// the round's schema validates.
pub(super) fn run_arena_job(
    runner: &JobRunner,
    spec: &JobSpec,
    bridge: &CancelBridge,
    label: &str,
) -> std::result::Result<Value, JobFailure> {
    if let Some(dir) = crate::env_var::var("ER_FAKE_ARENA_DIR") {
        return fake_arena_json_from_dir(&dir)
            .map_err(|error| JobFailure::Failed(error.to_string()));
    }
    let handle = AgentRunHandle::new();
    bridge.register(&handle);
    let mut timer = AgentRunTimer::start();
    let output = runner.run_controlled(
        spec,
        JobRun {
            handle,
            progress: None,
            timer: Some(&mut timer),
        },
    );
    timer.emit(
        label,
        &spec.provider.program,
        output.as_ref().is_ok_and(|output| output.status.success()),
    );
    match output {
        Ok(output) if !output.status.success() => Err(JobFailure::Failed(format!(
            "{} failed (code {:?}): {}",
            spec.provider.program,
            output.status.code(),
            truncate(&output.stderr, 400)
        ))),
        Ok(output) => extract_json_from_text(&output.text)
            .map_err(|error| JobFailure::Failed(error.to_string())),
        Err(JobError::Cancelled) => Err(JobFailure::Cancelled),
        Err(error) => Err(JobFailure::Failed(error.to_string())),
    }
}

/// Kills every job registered with it when the run's cancel flag turns on.
#[derive(Clone)]
pub(super) struct CancelBridge(Arc<BridgeInner>);

struct BridgeInner {
    handles: Arc<std::sync::Mutex<Vec<Arc<AgentRunHandle>>>>,
    stop: Arc<AtomicBool>,
    join: std::sync::Mutex<Option<thread::JoinHandle<()>>>,
}

impl CancelBridge {
    pub(super) fn start(cancel: Arc<AtomicBool>) -> Self {
        let handles: Arc<std::sync::Mutex<Vec<Arc<AgentRunHandle>>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let watched = Arc::clone(&handles);
        let stopped = Arc::clone(&stop);
        let join = thread::spawn(move || {
            while !stopped.load(Ordering::SeqCst) {
                if cancel.load(Ordering::SeqCst) {
                    let handles = watched.lock().unwrap_or_else(|e| e.into_inner());
                    for handle in handles.iter() {
                        handle.kill();
                    }
                }
                thread::sleep(CANCEL_POLL);
            }
        });
        Self(Arc::new(BridgeInner {
            handles,
            stop,
            join: std::sync::Mutex::new(Some(join)),
        }))
    }

    pub(super) fn register(&self, handle: &Arc<AgentRunHandle>) {
        self.0
            .handles
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(Arc::clone(handle));
    }
}

impl Drop for BridgeInner {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(join) = self.join.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = join.join();
        }
    }
}

/// How many fake provider responses this process has served.
///
/// The seeded path's whole economic argument is that it costs *one* arbiter call
/// however many experts contributed, and that is only assertable by counting.
static FAKE_ARENA_CALLS: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Test hook: read `round1.json`, `round2.json`, or `round3.json` from a directory (in order).
pub fn fake_arena_json_from_dir(dir: &str) -> Result<Value> {
    let n = FAKE_ARENA_CALLS.fetch_add(1, Ordering::SeqCst) + 1;
    let path = std::path::Path::new(dir).join(format!("round{}.json", n.min(3)));
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("read fake arena fixture {}", path.display()))?;
    serde_json::from_str(&text).context("parse fake arena json")
}

/// Provider responses served so far. A test records this before and after a run
/// and asserts the delta — see `seeded_arbiter.rs`.
pub fn fake_arena_call_count() -> u8 {
    FAKE_ARENA_CALLS.load(Ordering::SeqCst)
}

/// Pull the JSON object a reviewer's final text carries.
///
/// The runner has already decoded the provider's protocol; what remains is the
/// shapes agents print: a bare object, a fenced block, or a line after chat.
fn extract_json_from_text(text: &str) -> Result<Value> {
    let trimmed = text.trim();
    if let Ok(v) = serde_json::from_str(trimmed) {
        return Ok(v);
    }
    if let Some(block) = extract_fenced_json(trimmed) {
        return serde_json::from_str(&block).context("parse fenced json");
    }
    for line in trimmed.lines().rev() {
        let t = line.trim();
        if t.starts_with('{') {
            if let Ok(v) = serde_json::from_str(t) {
                return Ok(v);
            }
        }
    }
    anyhow::bail!("no JSON object in provider output");
}

fn extract_fenced_json(s: &str) -> Option<String> {
    let (_, rest) = s.split_once("```json")?;
    let (body, _) = rest.split_once("```")?;
    Some(body.trim().to_string())
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    // Back off to a char boundary so a multi-byte char is never split.
    let end = (0..=max)
        .rev()
        .find(|&i| s.is_char_boundary(i))
        .unwrap_or(0);
    format!("{}…", s.get(..end).unwrap_or_default())
}

/// Serializes tests that read or write `ER_FAKE_ARENA_DIR`.
#[cfg(test)]
pub(crate) static ARENA_TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    fn frozen_calls(count: u8) {
        FAKE_ARENA_CALLS.store(count, Ordering::SeqCst);
    }

    #[test]
    fn extract_json_from_a_fenced_block() {
        let v = extract_json_from_text(
            "Here is my verdict:\n```json\n{\"verdicts\":[{\"finding_id\":\"abc\"}]}\n```",
        )
        .unwrap();
        assert_eq!(v["verdicts"][0]["finding_id"], "abc");
    }

    #[test]
    fn extract_json_from_a_trailing_line() {
        let v = extract_json_from_text("thinking...\n{\"findings\":[]}\n").unwrap();
        assert!(v.get("findings").is_some());
    }

    #[test]
    fn a_reply_without_json_is_an_error() {
        assert!(extract_json_from_text("no object here").is_err());
    }

    #[test]
    fn truncate_stops_at_a_char_boundary() {
        assert_eq!(truncate("aéé", 2), "a…");
        assert_eq!(truncate("aéé", 3), "aé…");
        assert_eq!(truncate("aéé", 5), "aéé");
    }

    #[test]
    fn fake_arena_dir_round_robin() {
        let guard = ARENA_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        frozen_calls(0);
        let dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/arena/fake"
        );
        let value = crate::test_support::with_var("ER_FAKE_ARENA_DIR", dir, || {
            fake_arena_json_from_dir(dir).unwrap()
        });
        assert!(value.get("findings").is_some());
        drop(guard);
    }
}
