use crate::config::AiHubConfig;
use anyhow::{Context, Result};
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const MAX_RETRIES: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorClass {
    Transient,
    RateLimit,
    Fatal,
}

pub struct ProviderCommand {
    pub command: String,
    pub args: Vec<String>,
    pub stream_json: bool,
}

pub fn resolve_provider_command(
    hub: &AiHubConfig,
    provider_id: &str,
    model_id: &str,
) -> Result<ProviderCommand> {
    let provider = hub
        .providers
        .get(provider_id)
        .with_context(|| format!("unknown provider: {provider_id}"))?;
    let mut args = provider.args.clone();
    if let Some(model) = provider.models.iter().find(|m| m.id == model_id) {
        args.extend(model.args.clone());
    }
    Ok(ProviderCommand {
        command: provider.command.clone(),
        args,
        stream_json: provider.uses_stream_json_log(),
    })
}

pub struct SpawnResult {
    pub stdout: String,
    pub stderr: String,
}

/// Run provider CLI with `{prompt}` substitution; returns parsed JSON value from stdout.
pub fn run_provider_json(
    cmd: &ProviderCommand,
    prompt: &str,
    work_dir: &str,
    cancel: &AtomicBool,
    children: &Arc<Mutex<Vec<Child>>>,
) -> Result<Value> {
    let mut last_err = None;
    for attempt in 0..=MAX_RETRIES {
        if cancel.load(Ordering::SeqCst) {
            anyhow::bail!("cancelled");
        }
        match run_once(cmd, prompt, work_dir, cancel, children) {
            Ok(v) => return Ok(v),
            Err(e) => {
                let class = classify_error(&e);
                last_err = Some(e);
                if class == ErrorClass::Fatal || attempt == MAX_RETRIES {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(500 * (attempt as u64 + 1)));
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("provider failed")))
}

fn run_once(
    cmd: &ProviderCommand,
    prompt: &str,
    work_dir: &str,
    cancel: &AtomicBool,
    children: &Arc<Mutex<Vec<Child>>>,
) -> Result<Value> {
    let agent_args: Vec<String> = cmd
        .args
        .iter()
        .map(|a| a.replace("{prompt}", prompt))
        .collect();

    let mut child = Command::new(&cmd.command)
        .args(&agent_args)
        .current_dir(work_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("spawn {}", cmd.command))?;

    let child_id = child.id();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    if let Ok(mut kids) = children.lock() {
        kids.push(child);
    }

    let stdout_text = read_stdout(stdout);
    let stderr_text = read_stderr(stderr);

    let status = {
        let mut kids = children
            .lock()
            .map_err(|_| anyhow::anyhow!("children lock poisoned"))?;
        let slot = kids
            .iter_mut()
            .find(|c| c.id() == child_id)
            .with_context(|| "child handle missing")?;
        slot.wait()
            .with_context(|| format!("wait {}", cmd.command))?
    };

    if let Ok(mut kids) = children.lock() {
        kids.retain(|c| c.id() != child_id);
    }

    if cancel.load(Ordering::SeqCst) {
        anyhow::bail!("cancelled");
    }

    if !status.success() {
        anyhow::bail!(
            "{} failed (code {:?}): {}",
            cmd.command,
            status.code(),
            truncate(&stderr_text, 400)
        );
    }

    extract_json_from_stdout(&stdout_text, cmd.stream_json)
}

fn read_stdout(pipe: Option<std::process::ChildStdout>) -> String {
    pipe.map(read_lines).unwrap_or_default()
}

fn read_stderr(pipe: Option<std::process::ChildStderr>) -> String {
    pipe.map(read_lines).unwrap_or_default()
}

fn read_lines<R: std::io::Read>(pipe: R) -> String {
    let reader = BufReader::new(pipe);
    reader
        .lines()
        .map_while(Result::ok)
        .collect::<Vec<_>>()
        .join("\n")
}

fn extract_json_from_stdout(stdout: &str, stream_json: bool) -> Result<Value> {
    if stream_json {
        if let Some(v) = last_json_object_from_stream(stdout) {
            return Ok(v);
        }
    }
    if let Ok(v) = serde_json::from_str(stdout.trim()) {
        return Ok(v);
    }
    if let Some(block) = extract_fenced_json(stdout) {
        return serde_json::from_str(&block).context("parse fenced json");
    }
    for line in stdout.lines().rev() {
        let t = line.trim();
        if t.starts_with('{') {
            if let Ok(v) = serde_json::from_str(t) {
                return Ok(v);
            }
        }
    }
    anyhow::bail!("no JSON object in provider output");
}

fn last_json_object_from_stream(stdout: &str) -> Option<Value> {
    let mut last: Option<Value> = None;
    for line in stdout.lines() {
        let t = line.trim();
        if !t.starts_with('{') {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(t) {
            if v.is_object() {
                last = Some(v);
            }
        }
    }
    last
}

fn extract_fenced_json(s: &str) -> Option<String> {
    let start = s.find("```json")?;
    let rest = &s[start + 7..];
    let end = rest.find("```")?;
    Some(rest[..end].trim().to_string())
}

pub fn classify_error(err: &anyhow::Error) -> ErrorClass {
    let msg = err.to_string().to_lowercase();
    if msg.contains("rate limit") || msg.contains("429") {
        ErrorClass::RateLimit
    } else if msg.contains("cancelled") {
        ErrorClass::Fatal
    } else if msg.contains("timed out") || msg.contains("temporar") {
        ErrorClass::Transient
    } else {
        ErrorClass::Fatal
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}
