//! Invocation planning and the job call for desktop card-level AI (Ask AI /
//! Validate with AI).

use crate::config::{agent_command_uses_stream_json, ErConfig};
use er_agent::agent_slots::Workload;
use er_agent::decode::OutputProtocol;
use er_agent::job::{JobError, JobKind, JobProgress, JobReply, JobRunner, JobSpec, OutputContract};
use er_agent::provider::ProviderSpec;
use std::sync::Arc;

/// A card AI reply: the text the user sees in the thread, or the "Pending"
/// message that explains why there is none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardReply(String);

impl CardReply {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

/// Receives one progress line at a time, on the drain thread.
pub type ProgressSink = Arc<dyn Fn(&str) + Send + Sync>;

/// Lines buffered ahead of the drain. A stalled sink fills this and then trips
/// the provider timeout rather than blocking the job forever.
const PROGRESS_CAPACITY: usize = 32;

/// Resolved agent command + args for a card AI subprocess.
pub struct CardAiInvocation {
    pub command: String,
    pub args: Vec<String>,
    pub work_dir: String,
    pub is_claude_compatible: bool,
    pub uses_stream_json: bool,
    /// Extra process environment (OpenCode read-only permissions).
    pub env: Vec<(String, String)>,
    /// Wall-clock limit for this run, resolved from config when the invocation
    /// is planned. Carried on the invocation because the spawn is the only
    /// place that needs it, and it must not be re-resolved there against a
    /// config the caller may since have changed.
    pub timeout: std::time::Duration,
    /// Process-wide concurrency cap this run waits on before spawning,
    /// resolved from config alongside the deadline.
    pub slot_cap: usize,
    /// Shared ceiling across both workloads, resolved alongside `slot_cap`.
    pub slot_ceiling: usize,
}

/// Resolve provider/command/args from config (mirrors background review selection).
pub fn plan_card_ai_invocation(
    config: &ErConfig,
    provider_id: Option<&str>,
    model_id: Option<&str>,
    runtime_effort: Option<&str>,
    work_dir: String,
) -> CardAiInvocation {
    // One resolution path. `agent_runtime` was written to be exactly this and
    // then nothing called it, which left 1339 lines dead and this another copy
    // of the same rules.
    //
    // The delegation is pinned by two tests in `agent_runtime`:
    // `what_the_runtime_resolves_versus_what_card_ai_resolves` compares argv
    // across every catalog provider and model, and its `..._with_no_hub_provider`
    // sibling covers the legacy `[agent]` fallback -- the branch a delegation
    // could quietly change, and the one a happy-path check would miss.
    let resolved = crate::agent_runtime::resolve_invocation(
        config,
        crate::agent_runtime::AgentInvocationRequest {
            selection: crate::agent_runtime::AgentSelection::Runtime {
                provider_id,
                model_id,
            },
            task: &crate::agent_runtime::AgentTaskKind::CardReply,
            effort: runtime_effort,
            effort_override: None,
            work_dir: work_dir.clone(),
            access: crate::agent_runtime::AgentAccessProfile::ReadOnly,
            live_logs: false,
        },
    );

    let (command, args, work_dir, family, output_protocol, env) = match resolved {
        Ok(inv) => (
            inv.command,
            inv.args,
            inv.work_dir,
            inv.family,
            inv.output_protocol,
            inv.env,
        ),
        // Unreachable in practice — `resolve_invocation` falls back internally
        // rather than failing — but this path's contract is a display string,
        // so a resolution failure has to produce a shape rather than a panic.
        Err(_) => (
            config.agent.command.clone(),
            config.agent.args.clone(),
            work_dir,
            crate::config::CliFamily::detect(&config.agent.command),
            crate::agent_runtime::OutputProtocol::Plain,
            Vec::new(),
        ),
    };

    // Kept as the old derivation rather than read off `output_protocol`: the
    // protocol does not carry the command-family guard, so a non-CLI command
    // with a `stream-json` argument would flip this field under the mapping.
    let uses_stream_json = agent_command_uses_stream_json(&command)
        && output_protocol == crate::agent_runtime::OutputProtocol::ClaudeStreamJson;

    CardAiInvocation {
        command,
        is_claude_compatible: family == crate::config::CliFamily::Claude,
        uses_stream_json,
        args,
        work_dir,
        env,
        timeout: config.ai_hub.effective_agent_timeout(),
        slot_cap: config.ai_hub.effective_max_concurrent_reviews(),
        slot_ceiling: config.ai_hub.effective_max_concurrent_agents(),
    }
}

// `fallback_agent` and `inject_read_only_tools` lived here and are gone:
// `agent_runtime::resolve_invocation` owns both the `[agent]` fallback and the
// read-only tool list, and the delegation above is pinned against this file's
// former behaviour by the differential tests in that module.

/// Build argv: Claude uses `--append-system-prompt`; other CLIs (e.g. Codex) fold
/// system context into the `{prompt}` placeholder or trailing prompt arg.
pub fn build_card_ai_argv(inv: &CardAiInvocation, system: &str, user: &str) -> Vec<String> {
    let mut args = inv.args.clone();
    let has_placeholder = args.iter().any(|a| a.contains("{prompt}"));
    let combined_prompt = if inv.is_claude_compatible || system.is_empty() {
        user.to_string()
    } else {
        format!("{system}\n\nUser request:\n{user}")
    };

    for a in args.iter_mut() {
        if a.contains("{prompt}") {
            *a = a.replace("{prompt}", &combined_prompt);
        }
    }

    if inv.is_claude_compatible {
        if args.iter().any(|a| a == "--append-system-prompt") {
            if let Some(i) = args.iter().position(|a| a == "--append-system-prompt") {
                if i + 1 < args.len() {
                    args[i + 1] = system.to_string();
                } else {
                    args.push(system.to_string());
                }
            }
        } else {
            args.push("--append-system-prompt".to_string());
            args.push(system.to_string());
            if !has_placeholder {
                args.push(user.to_string());
            }
        }
    } else if !has_placeholder {
        args.push(combined_prompt);
    }

    args
}

/// The fake sentinel, from this thread's test override or the environment.
fn fake_claude() -> Option<String> {
    #[cfg(feature = "test-support")]
    {
        crate::test_support::var("ER_FAKE_CLAUDE")
    }
    #[cfg(not(feature = "test-support"))]
    {
        std::env::var("ER_FAKE_CLAUDE").ok()
    }
}

/// Run card AI as a job; honors `ER_FAKE_CLAUDE` for tests.
///
/// The reply returns on the `result` line while the CLI may still be running.
/// Progress is drained on its own thread, so a slow UI sink cannot stop the
/// job: a full buffer trips the provider timeout instead.
pub fn run_card_ai_subprocess(
    inv: &CardAiInvocation,
    system: &str,
    user: &str,
    model_override: Option<&str>,
    progress: Option<ProgressSink>,
) -> CardReply {
    if let Some(fake) = fake_claude() {
        return CardReply(match fake.as_str() {
            "fail" => "Pending — invoke via CLI (error: ER_FAKE_CLAUDE=fail)".to_string(),
            "ok" => "mocked ok".to_string(),
            other if !other.is_empty() => other.to_string(),
            _ => "mocked ok".to_string(),
        });
    }

    let mut args = build_card_ai_argv(inv, system, user);
    if let Some(model) = model_override.filter(|m| !m.trim().is_empty()) {
        if inv.is_claude_compatible && !args.iter().any(|a| a == "--model") {
            args.push("--model".to_string());
            args.push(model.to_string());
        }
    }
    let stream_json = if inv.is_claude_compatible {
        ensure_claude_stream_json(&mut args)
    } else {
        inv.uses_stream_json
    };
    let protocol = if stream_json {
        OutputProtocol::ClaudeStreamJson
    } else {
        OutputProtocol::Plain
    };

    let spec = JobSpec {
        kind: JobKind::CardReply,
        provider: ProviderSpec {
            program: inv.command.clone(),
            args,
            env: inv.env.clone(),
            protocol,
            timeout: inv.timeout,
        },
        // `build_card_ai_argv` already carries the system/user prompt.
        prompt: String::new(),
        work_dir: inv.work_dir.clone(),
        // The card path has always drawn from the background workload.
        workload: Workload::Background,
        output: OutputContract::Stdout,
    };
    let runner = JobRunner::new(inv.slot_cap, inv.slot_ceiling);
    let (tx, drain) = match progress {
        Some(sink) => {
            let (tx, rx) = std::sync::mpsc::sync_channel(PROGRESS_CAPACITY);
            let drain = std::thread::spawn(move || {
                while let Ok(event) = rx.recv() {
                    if let JobProgress::StdoutLine(line) = event {
                        sink(&line);
                    }
                }
            });
            (Some(tx), Some(drain))
        }
        None => (None, None),
    };

    let result = runner.run_until_result(&spec, tx);
    if let Some(drain) = drain {
        let _ = drain.join();
    }

    match result {
        Ok(JobReply::Reply(text)) => CardReply(finish_reply(text)),
        Ok(JobReply::Exited(out)) if out.status.success() => CardReply(finish_reply(out.text)),
        Ok(JobReply::Exited(out)) => CardReply(format!(
            "Pending — invoke via CLI ({} exited {}: {})",
            inv.command,
            out.status.code().unwrap_or(-1),
            out.stderr.trim()
        )),
        Err(JobError::TimedOut(limit)) => CardReply(format!(
            "Pending — invoke via CLI ({})",
            crate::agent_run::timed_out(limit)
        )),
        Err(JobError::Cancelled) => CardReply(format!(
            "Pending — invoke via CLI ({})",
            crate::agent_run::cancelled()
        )),
        Err(JobError::Spawn(error)) => CardReply(format!(
            "Pending — invoke via CLI (failed to spawn {}: {error})",
            inv.command
        )),
    }
}

fn finish_reply(mut s: String) -> String {
    const MAX: usize = 8 * 1024;
    if s.len() > MAX {
        s.truncate(MAX);
    }
    let trimmed = s.trim().to_string();
    if trimmed.is_empty() {
        "Pending — invoke via CLI (empty response)".to_string()
    } else {
        trimmed
    }
}

/// Put a Claude run on `stream-json`, the one output format that marks the end
/// of the answer before the process exits. Returns whether the run ends up on
/// it: a format the user configured is left alone, and that run waits for exit
/// as before.
fn ensure_claude_stream_json(args: &mut Vec<String>) -> bool {
    let format = args.iter().enumerate().find_map(|(i, arg)| {
        if arg == "--output-format" {
            Some(args.get(i + 1).map_or("", String::as_str))
        } else {
            arg.strip_prefix("--output-format=")
        }
    });
    match format {
        None => {
            args.push("--output-format".to_string());
            args.push("stream-json".to_string());
        }
        Some("stream-json") => {}
        Some(_) => return false,
    }
    // `--print` refuses stream-json without it.
    if !args.iter().any(|arg| arg == "--verbose") {
        args.push("--verbose".to_string());
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_card_ai_argv_codex_combines_system_without_claude_flag() {
        let inv = CardAiInvocation {
            command: "codex".into(),
            args: vec!["exec".into(), "{prompt}".into()],
            work_dir: "/repo".into(),
            is_claude_compatible: false,
            uses_stream_json: false,
            env: vec![],
            // Irrelevant to argv construction; these tests never spawn.
            timeout: std::time::Duration::from_mins(15),
            slot_cap: 3,
            slot_ceiling: 6,
        };
        let args = build_card_ai_argv(&inv, "system context", "how does this work?");
        assert!(!args.iter().any(|a| a == "--append-system-prompt"));
        assert!(args.iter().any(|a| {
            a.contains("system context") && a.contains("User request:\nhow does this work?")
        }));
    }

    #[test]
    fn build_card_ai_argv_claude_uses_append_system_prompt() {
        let inv = CardAiInvocation {
            command: "claude".into(),
            args: vec!["--print".into(), "-p".into(), "{prompt}".into()],
            work_dir: "/repo".into(),
            is_claude_compatible: true,
            uses_stream_json: false,
            env: vec![],
            // Irrelevant to argv construction; these tests never spawn.
            timeout: std::time::Duration::from_mins(15),
            slot_cap: 3,
            slot_ceiling: 6,
        };
        let args = build_card_ai_argv(&inv, "system context", "how does this work?");
        assert!(args
            .windows(2)
            .any(|pair| { pair[0] == "--append-system-prompt" && pair[1] == "system context" }));
        assert!(args.iter().any(|a| a == "how does this work?"));
        assert!(!args.iter().any(|a| a.contains("User request:")));
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn claude_card_ai_is_put_on_stream_json_once() {
        let mut plain = args(&["--print", "-p", "q"]);
        assert!(ensure_claude_stream_json(&mut plain));
        assert_eq!(
            plain,
            args(&[
                "--print",
                "-p",
                "q",
                "--output-format",
                "stream-json",
                "--verbose"
            ])
        );

        // The legacy default already asks for stream-json; only `--verbose`
        // is missing, and neither flag may appear twice.
        let mut configured = args(&["--print", "--output-format", "stream-json", "--verbose"]);
        assert!(ensure_claude_stream_json(&mut configured));
        assert_eq!(
            configured,
            args(&["--print", "--output-format", "stream-json", "--verbose"])
        );

        let mut joined = args(&["--output-format=stream-json"]);
        assert!(ensure_claude_stream_json(&mut joined));
        assert_eq!(joined, args(&["--output-format=stream-json", "--verbose"]));

        // A format the user picked is theirs; that run waits for exit.
        let mut text = args(&["--print", "--output-format", "text"]);
        assert!(!ensure_claude_stream_json(&mut text));
        assert_eq!(text, args(&["--print", "--output-format", "text"]));
    }

    /// A Claude-family invocation that runs `sh -c <script>` in place of the
    /// CLI. The flags card AI appends land in the script's positional args.
    #[cfg(unix)]
    fn fake_claude(script: &str) -> CardAiInvocation {
        CardAiInvocation {
            command: "sh".into(),
            args: vec!["-c".into(), script.into()],
            work_dir: std::env::temp_dir().display().to_string(),
            is_claude_compatible: true,
            uses_stream_json: false,
            env: vec![],
            // Short enough that the drain test's planted run times out
            // quickly; the green run finishes well inside it.
            timeout: std::time::Duration::from_secs(2),
            slot_cap: 1,
            slot_ceiling: 1,
        }
    }

    /// A stalled sink must not stall the job: the bounded channel fills and the
    /// provider timeout ends the run. The green path drains 200 lines and
    /// returns on the result line, well inside the 2s deadline.
    #[cfg(unix)]
    #[test]
    fn the_card_reply_drains_progress_while_the_child_runs() {
        let _serial = er_agent::agent_slots::test_serial();
        let script = r#"i=0; while [ $i -lt 200 ]; do echo '{"type":"assistant","message":{"content":[{"type":"text","text":"tick"}}]}}'; i=$((i+1)); done; echo '{"type":"result","result":"fast reply"}'"#;
        let inv = fake_claude(script);
        let lines = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let sink_lines = Arc::clone(&lines);
        let sink: ProgressSink = Arc::new(move |_line| {
            sink_lines.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        });

        let started = std::time::Instant::now();
        let reply = run_card_ai_subprocess(&inv, "", "", None, Some(sink));
        let elapsed = started.elapsed();

        assert_eq!(reply.as_str(), "fast reply");
        let seen = lines.load(std::sync::atomic::Ordering::Relaxed);
        assert!(seen > 100, "the sink saw {seen} lines");
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "the drain stalled: {elapsed:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_nonzero_exit_keeps_the_pending_message() {
        let _serial = er_agent::agent_slots::test_serial();
        let inv = fake_claude("echo boom >&2; exit 3");
        let reply = run_card_ai_subprocess(&inv, "", "", None, None);
        assert_eq!(
            reply.as_str(),
            "Pending — invoke via CLI (sh exited 3: boom)"
        );
    }

    #[test]
    fn plan_injects_read_tools_for_claude() {
        let mut config = ErConfig::default();
        config.agent.command = "claude".into();
        config.agent.args = vec!["--print".into(), "-p".into(), "{prompt}".into()];
        let inv = plan_card_ai_invocation(&config, None, None, None, "/repo".into());
        assert!(inv.args.iter().any(|a| a == "Read"));
        assert!(inv.args.iter().any(|a| a.contains("grep")));
        assert!(!inv.args.iter().any(|a| a.contains("--add-dir")));
    }

    #[test]
    fn plan_ensures_opencode_auto_for_legacy_agent_fallback() {
        let mut config = ErConfig::default();
        config.agent.command = "opencode".into();
        config.agent.args = vec!["run".into(), "{prompt}".into()];
        let inv = plan_card_ai_invocation(&config, None, None, None, "/repo".into());
        assert_eq!(inv.command, "opencode");
        assert!(inv.args.iter().any(|a| a == "--auto"));
        let prompt_idx = inv
            .args
            .iter()
            .position(|a| a.contains("{prompt}"))
            .unwrap();
        let auto_idx = inv.args.iter().position(|a| a == "--auto").unwrap();
        assert!(auto_idx < prompt_idx);
        assert_eq!(inv.env.len(), 1);
        assert_eq!(inv.env[0].0, "OPENCODE_PERMISSION");
        let parsed: serde_json::Value = serde_json::from_str(&inv.env[0].1).unwrap();
        assert_eq!(parsed["edit"], "deny");
        assert_eq!(parsed["external_directory"]["*"], "deny");
    }

    #[test]
    fn plan_ensures_opencode_auto_for_hub_provider() {
        let mut config = ErConfig::default();
        crate::config::supplement_ai_hub(&mut config.ai_hub);
        let inv = plan_card_ai_invocation(
            &config,
            Some("opencode"),
            Some("default"),
            None,
            "/repo".into(),
        );
        assert!(inv.args.iter().any(|a| a == "--auto"));
        assert!(!inv.args.iter().any(|a| a.contains("--add-dir")));
        assert_eq!(inv.env.len(), 1);
        let parsed: serde_json::Value = serde_json::from_str(&inv.env[0].1).unwrap();
        assert_eq!(parsed["bash"]["*"], "deny");
        assert_eq!(parsed["bash"]["grep *"], "allow");
    }

    #[test]
    fn plan_injects_reasoning_effort_for_codex() {
        let mut config = ErConfig::default();
        config.ai_hub.default_effort = Some("high".into());
        config.ai_hub.providers.insert(
            "codex".into(),
            crate::config::AiProviderConfig {
                command: "codex".into(),
                models: vec![crate::config::AiModelConfig {
                    id: "gpt-5.6-sol".into(),
                    args: vec!["--model".into(), "gpt-5.6-sol".into()],
                    effort_levels: vec!["low", "medium", "high", "xhigh", "max"]
                        .into_iter()
                        .map(String::from)
                        .collect(),
                    ..Default::default()
                }],
                ..Default::default()
            },
        );

        let inv = plan_card_ai_invocation(
            &config,
            Some("codex"),
            Some("gpt-5.6-sol"),
            None,
            "/repo".into(),
        );
        assert!(inv
            .args
            .windows(2)
            .any(|pair| { pair[0] == "-c" && pair[1] == "model_reasoning_effort=high" }));
        assert!(!inv.args.iter().any(|a| a.contains("--add-dir")));
    }

    #[test]
    fn the_planned_invocation_carries_the_configured_deadline() {
        // The spawn reads the limit off the invocation, so a plan that dropped
        // it would leave card AI as the one path with no deadline -- exactly
        // the state this changed.
        let mut config = ErConfig::default();
        let inv = plan_card_ai_invocation(&config, None, None, None, "/repo".into());
        assert_eq!(
            inv.timeout,
            std::time::Duration::from_secs(crate::config::DEFAULT_AGENT_TIMEOUT_SECS)
        );

        config.ai_hub.agent_timeout_secs = 120;
        let inv = plan_card_ai_invocation(&config, None, None, None, "/repo".into());
        assert_eq!(inv.timeout, std::time::Duration::from_secs(120));
    }

    #[test]
    fn plan_card_ai_uses_the_shared_default_model() {
        let mut config = ErConfig::default();
        config.ai_hub.default_provider = Some("codex".into());
        config.ai_hub.default_model = Some("gpt-5.6-luna".into());
        config.ai_hub.providers.insert(
            "codex".into(),
            crate::config::AiProviderConfig {
                models: vec![
                    crate::config::AiModelConfig {
                        id: "gpt-5.6-luna".into(),
                        args: vec!["--model".into(), "gpt-5.6-luna".into()],
                        ..Default::default()
                    },
                    crate::config::AiModelConfig {
                        id: "gpt-5.3-codex-spark".into(),
                        args: vec!["--model".into(), "gpt-5.3-codex-spark".into()],
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
        );

        let inv = plan_card_ai_invocation(&config, Some("codex"), None, None, "/repo".into());
        assert!(inv
            .args
            .windows(2)
            .any(|pair| pair[0] == "--model" && pair[1] == "gpt-5.6-luna"));
        assert!(!inv
            .args
            .windows(2)
            .any(|pair| pair[0] == "--model" && pair[1] == "gpt-5.3-codex-spark"));
    }
}
