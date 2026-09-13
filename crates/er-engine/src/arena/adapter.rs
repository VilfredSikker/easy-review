use crate::config::{
    agent_command_is_codex, inject_agent_storage_access, inject_codex_ignore_user_config,
    inject_provider_effort, AiHubConfig,
};
use anyhow::{Context, Result};
use serde_json::Value;
use std::io::{BufRead, BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

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
    pub env: Vec<(String, String)>,
}

pub fn resolve_provider_command(
    hub: &AiHubConfig,
    provider_id: &str,
    model_id: &str,
    effort: Option<&str>,
    storage_dir: Option<&str>,
) -> Result<ProviderCommand> {
    let provider = hub
        .providers
        .get(provider_id)
        .with_context(|| format!("unknown provider: {provider_id}"))?;
    let mut args = provider.args.clone();
    let family = provider.cli_family();
    if let Some(model) = provider.models.iter().find(|m| m.id == model_id) {
        crate::config::extend_provider_model_args(family, &mut args, &model.args);
    }
    let effort = crate::config::normalize_effort(hub, Some(provider_id), Some(model_id), effort);
    inject_provider_effort(family, &mut args, Some(model_id), effort.as_deref());
    if agent_command_is_codex(&provider.command) {
        inject_codex_ignore_user_config(&mut args);
    }
    inject_agent_storage_access(family, &mut args, storage_dir);
    let mut env = Vec::new();
    if let Some(pair) = crate::config::apply_opencode_spawn(family, &mut args, storage_dir) {
        env.push(pair);
    }
    Ok(ProviderCommand {
        command: provider.command.clone(),
        args,
        stream_json: provider.uses_stream_json_log(),
        env,
    })
}

/// Run provider CLI with `{prompt}` substitution; returns parsed JSON value from stdout.
pub fn run_provider_json(
    cmd: &ProviderCommand,
    prompt: &str,
    work_dir: &str,
    cancel: &AtomicBool,
    children: &Arc<Mutex<Vec<Child>>>,
) -> Result<Value> {
    if let Ok(dir) = std::env::var("ER_FAKE_ARENA_DIR") {
        return fake_arena_json_from_dir(&dir);
    }

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
                thread::sleep(std::time::Duration::from_millis(500 * (attempt as u64 + 1)));
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("provider failed")))
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

#[allow(clippy::literal_string_with_formatting_args)] // {prompt} is a deliberate template placeholder, substituted via .replace()
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

    let mut child = Command::new(&cmd.command);
    child
        .args(&agent_args)
        .current_dir(work_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in &cmd.env {
        child.env(key, value);
    }
    let mut child = child
        .spawn()
        .with_context(|| format!("spawn {}", cmd.command))?;

    let child_id = child.id();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    if let Ok(mut kids) = children.lock() {
        kids.push(child);
    }

    let (stdout_text, stderr_text) = read_pipes_concurrent(stdout, stderr);

    let status = {
        let mut kids = children
            .lock()
            .map_err(|_| anyhow::anyhow!("children lock poisoned"))?;
        // Take ownership of the child so the guard drops before the blocking
        // wait() — holding the mutex across a long-running agent would block
        // unrelated spawns that push to the same map.
        let idx = kids
            .iter()
            .position(|c| c.id() == child_id)
            .with_context(|| "child handle missing")?;
        let mut child = kids.remove(idx);
        drop(kids);
        child
            .wait()
            .with_context(|| format!("wait {}", cmd.command))?
    };

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

fn read_pipes_concurrent(
    stdout: Option<std::process::ChildStdout>,
    stderr: Option<std::process::ChildStderr>,
) -> (String, String) {
    let out_handle = stdout.map(|pipe| thread::spawn(move || read_lines(pipe)));
    let err_handle = stderr.map(|pipe| thread::spawn(move || read_lines(pipe)));
    let stdout_text = out_handle
        .map(|h| h.join().unwrap_or_default())
        .unwrap_or_default();
    let stderr_text = err_handle
        .map(|h| h.join().unwrap_or_default())
        .unwrap_or_default();
    (stdout_text, stderr_text)
}

fn read_lines<R: Read>(pipe: R) -> String {
    let reader = BufReader::new(pipe);
    reader
        .lines()
        .map_while(Result::ok)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Pull the model's final text from Claude/Cursor `stream-json` NDJSON logs.
fn extract_agent_stdout_text(stdout: &str) -> String {
    let mut last_result: Option<String> = None;
    let mut assistant_text: Vec<String> = Vec::new();

    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if v.get("type").and_then(|t| t.as_str()) == Some("result") {
            if let Some(r) = v.get("result").and_then(|r| r.as_str()) {
                last_result = Some(r.to_string());
            }
        }
        if v.get("type").and_then(|t| t.as_str()) == Some("assistant") {
            if let Some(content) = v
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_array())
            {
                for item in content {
                    if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                        if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                            let t = text.trim();
                            if !t.is_empty() {
                                assistant_text.push(t.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    if let Some(r) = last_result.filter(|s| !s.trim().is_empty()) {
        return r;
    }
    if !assistant_text.is_empty() {
        return assistant_text.join("\n\n");
    }
    stdout.to_string()
}

fn extract_json_from_stdout(stdout: &str, stream_json: bool) -> Result<Value> {
    let text = if stream_json {
        extract_agent_stdout_text(stdout)
    } else {
        stdout.to_string()
    };
    extract_json_from_text(&text)
}

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

pub fn is_cancelled_error(err: &anyhow::Error) -> bool {
    err.to_string().to_lowercase().contains("cancelled")
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shared mutex for tests that read or write `ER_FAKE_ARENA_DIR`.
    ///
    /// It is process-global, so a test that inherits another test's fixture
    /// silently gets someone else's JSON — and a test asserting a *real* spawn
    /// fails outright, because the fake branch returns before the command runs.
    /// Hold it for the duration of any test that touches the variable, with
    /// `.lock().unwrap_or_else(|e| e.into_inner())` so one panic does not poison
    /// the rest.
    static ARENA_TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn extract_json_from_stream_json_result_event() {
        let stdout = concat!(
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"thinking"}]}}"#,
            "\n",
            r#"{"type":"result","subtype":"success","result":"{\"findings\":[{\"file\":\"a.rs\",\"title\":\"t\",\"body\":\"b\",\"severity\":\"low\"}]}"}"#,
        );
        let v = extract_json_from_stdout(stdout, true).unwrap();
        let findings = v.get("findings").and_then(|f| f.as_array()).unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn extract_json_from_plain_stdout() {
        let stdout = r#"{"findings":[]}"#;
        let v = extract_json_from_stdout(stdout, false).unwrap();
        assert!(v.get("findings").is_some());
    }

    #[test]
    fn codex_provider_command_ignores_user_config() {
        const DIR: &str = "/managed/repos/demo/branches/main/view-buckets/branch";
        let mut hub = AiHubConfig::default();
        hub.providers.insert(
            "codex".to_string(),
            crate::config::AiProviderConfig {
                command: "codex".to_string(),
                args: vec!["exec".to_string(), "{prompt}".to_string()],
                models: vec![crate::config::AiModelConfig {
                    id: "gpt-5.5".to_string(),
                    args: vec!["--model".to_string(), "gpt-5.5".to_string()],
                    ..Default::default()
                }],
                ..Default::default()
            },
        );

        let cmd = resolve_provider_command(&hub, "codex", "gpt-5.5", None, Some(DIR)).unwrap();

        assert_eq!(cmd.args[0], "exec");
        assert_eq!(
            cmd.args
                .iter()
                .filter(|arg| arg.as_str() == "--ignore-user-config")
                .count(),
            1
        );
        assert!(cmd
            .args
            .windows(2)
            .any(|pair| pair[0] == "--add-dir" && pair[1] == DIR));
    }

    #[test]
    fn codex_provider_command_skips_storage_without_dir() {
        let mut hub = AiHubConfig::default();
        hub.providers.insert(
            "codex".to_string(),
            crate::config::AiProviderConfig {
                command: "codex".to_string(),
                args: vec!["exec".to_string(), "{prompt}".to_string()],
                models: vec![crate::config::AiModelConfig {
                    id: "gpt-5.5".to_string(),
                    args: vec!["--model".to_string(), "gpt-5.5".to_string()],
                    ..Default::default()
                }],
                ..Default::default()
            },
        );

        let cmd = resolve_provider_command(&hub, "codex", "gpt-5.5", None, None).unwrap();
        assert!(!cmd.args.iter().any(|arg| arg.contains("--add-dir")));
    }

    #[test]
    fn opencode_provider_command_orders_flags_and_sets_permission_env() {
        const DIR: &str = "/managed/repos/demo/branches/main/view-buckets/branch";
        let mut hub = AiHubConfig::default();
        hub.providers.insert(
            "opencode".to_string(),
            crate::config::AiProviderConfig {
                command: "opencode".to_string(),
                args: vec!["run".to_string(), "{prompt}".to_string()],
                models: vec![crate::config::AiModelConfig {
                    id: "claude-sonnet-4-5".to_string(),
                    args: vec![
                        "--model".to_string(),
                        "anthropic/claude-sonnet-4-5".to_string(),
                    ],
                    effort_levels: vec!["high".into(), "max".into()],
                    ..Default::default()
                }],
                ..Default::default()
            },
        );

        let cmd = resolve_provider_command(
            &hub,
            "opencode",
            "claude-sonnet-4-5",
            Some("high"),
            Some(DIR),
        )
        .unwrap();

        let prompt_idx = cmd
            .args
            .iter()
            .position(|a| a.contains("{prompt}"))
            .unwrap();
        assert!(cmd.args.iter().any(|a| a == "--auto"));
        assert!(cmd
            .args
            .windows(2)
            .any(|pair| pair[0] == "--model" && pair[1] == "anthropic/claude-sonnet-4-5"));
        assert!(cmd
            .args
            .windows(2)
            .any(|pair| pair[0] == "--variant" && pair[1] == "high"));
        let model_idx = cmd
            .args
            .windows(2)
            .position(|pair| pair[0] == "--model")
            .unwrap();
        let variant_idx = cmd
            .args
            .windows(2)
            .position(|pair| pair[0] == "--variant")
            .unwrap();
        assert!(model_idx < prompt_idx);
        assert!(variant_idx < prompt_idx);

        assert_eq!(cmd.env.len(), 1);
        assert_eq!(cmd.env[0].0, "OPENCODE_PERMISSION");
        let parsed: serde_json::Value = serde_json::from_str(&cmd.env[0].1).unwrap();
        assert!(parsed.get("permission").is_none());
        assert_eq!(parsed["external_directory"]["*"], "deny");
        assert_eq!(parsed["external_directory"][format!("{DIR}/**")], "allow");
    }

    #[test]
    fn opencode_provider_command_skips_permission_env_without_dir() {
        let mut hub = AiHubConfig::default();
        hub.providers.insert(
            "opencode".to_string(),
            crate::config::AiProviderConfig {
                command: "opencode".to_string(),
                args: vec![
                    "run".to_string(),
                    "--auto".to_string(),
                    "{prompt}".to_string(),
                ],
                models: vec![crate::config::AiModelConfig {
                    id: "default".to_string(),
                    ..Default::default()
                }],
                ..Default::default()
            },
        );

        let cmd = resolve_provider_command(&hub, "opencode", "default", None, None).unwrap();
        assert!(cmd.env.is_empty());
        assert!(cmd.args.iter().any(|a| a == "--auto"));
    }

    /// The real spawn path, with no `ER_FAKE_ARENA_DIR` to short-circuit it.
    ///
    /// The fake-provider harness returns before the command is ever built, so
    /// everything below `run_provider_json`'s first branch — spawning, reading
    /// both pipes, the exit-status check, and pulling a JSON object out of the
    /// output — is unexercised by every other test. Those are exactly the parts
    /// that break against a real CLI, so they are worth a stub that behaves like
    /// one rather than a model call.
    #[test]
    fn provider_output_is_parsed_from_a_real_process() {
        let _guard = ARENA_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("arbiter.sh");
        std::fs::write(
            &script,
            "#!/bin/sh\n\
             # Chatty preamble on stderr, a fenced JSON block on stdout: what a\n\
             # real agent prints when it narrates before answering.\n\
             echo 'thinking about it' >&2\n\
             printf '%s\\n' 'Here is my verdict:' '```json' \\\n\
               '{\"verdicts\":[{\"finding_id\":\"abc\",\"verdict\":\"kept\",\"confidence\":0.8}]}' \\\n\
               '```'\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let v = run_provider_json(
            &ProviderCommand {
                command: script.display().to_string(),
                // The prompt placeholder is substituted before spawning, so a
                // command that ignores it still proves the substitution runs.
                args: vec!["--prompt".into(), "{prompt}".into()],
                stream_json: false,
                env: vec![("ER_TEST_MARKER".into(), "1".into())],
            },
            "review this diff",
            dir.path().to_str().unwrap(),
            &AtomicBool::new(false),
            &Arc::new(Mutex::new(Vec::new())),
        )
        .expect("a real process's output parses");

        assert_eq!(
            v["verdicts"][0]["verdict"], "kept",
            "the JSON came out of the fenced block"
        );
    }

    /// A nonzero exit is an error with the stderr attached, not a silent empty
    /// result — a failed arbiter must not read as "no verdicts".
    #[test]
    fn a_failing_provider_reports_its_stderr() {
        let _guard = ARENA_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let err = run_provider_json(
            &ProviderCommand {
                command: "sh".into(),
                args: vec!["-c".into(), "echo 'quota exceeded' >&2; exit 3".into()],
                stream_json: false,
                env: vec![],
            },
            "",
            ".",
            &AtomicBool::new(false),
            &Arc::new(Mutex::new(Vec::new())),
        )
        .expect_err("a nonzero exit is an error");

        let text = format!("{err:#}");
        assert!(text.contains("quota exceeded"), "stderr is carried: {text}");
    }

    #[test]
    fn fake_arena_dir_round_robin() {
        let _guard = ARENA_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/arena/fake"
        );
        std::env::set_var("ER_FAKE_ARENA_DIR", dir);
        let v1 = run_provider_json(
            &ProviderCommand {
                command: "true".into(),
                args: vec![],
                stream_json: false,
                env: vec![],
            },
            "",
            ".",
            &AtomicBool::new(false),
            &Arc::new(Mutex::new(Vec::new())),
        )
        .unwrap();
        assert!(v1.get("findings").is_some());
        std::env::remove_var("ER_FAKE_ARENA_DIR");
    }
}
