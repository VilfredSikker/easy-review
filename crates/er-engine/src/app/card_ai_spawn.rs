//! Subprocess invocation for desktop card-level AI (Ask AI / Validate with AI).

use crate::config::{agent_command_uses_stream_json, inject_provider_effort, ErConfig};
use std::process::Command;

/// Resolved agent command + args for a card AI subprocess.
pub struct CardAiInvocation {
    pub command: String,
    pub args: Vec<String>,
    pub work_dir: String,
    pub is_claude_compatible: bool,
    pub uses_stream_json: bool,
    /// Extra process environment (OpenCode read-only permissions).
    pub env: Vec<(String, String)>,
}

/// Resolve provider/command/args from config (mirrors background review selection).
pub fn plan_card_ai_invocation(
    config: &ErConfig,
    provider_id: Option<&str>,
    model_id: Option<&str>,
    runtime_effort: Option<&str>,
    work_dir: String,
) -> CardAiInvocation {
    let (command, mut args, is_claude, resolved_provider_id, resolved_model_id, family) =
        if let Some(pid) = config.ai_hub.resolve_provider_id(provider_id) {
            if let Some(provider) = config.ai_hub.providers.get(&pid) {
                let mut args = provider.args.clone();
                let family = provider.cli_family();
                let resolved_model_id = config.ai_hub.resolve_model_id(&pid, model_id);
                if let Some(mid) = &resolved_model_id {
                    if let Some(model) = provider.models.iter().find(|m| m.id == *mid) {
                        crate::config::extend_provider_model_args(family, &mut args, &model.args);
                    }
                }
                let is_claude = crate::config::agent_command_is_claude(&provider.command);
                (
                    provider.command.clone(),
                    args,
                    is_claude,
                    Some(pid.to_string()),
                    resolved_model_id,
                    family,
                )
            } else {
                fallback_agent(config)
            }
        } else {
            fallback_agent(config)
        };

    // Hub and legacy `[agent]` paths both need `--auto` for headless OpenCode,
    // paired with a read-only permission object so asks cannot mutate the tree.
    let mut env = Vec::new();
    if let Some(pair) = crate::config::apply_opencode_readonly_spawn(family, &mut args) {
        env.push(pair);
    }

    let uses_stream_json =
        agent_command_uses_stream_json(&command) && args.iter().any(|a| a == "stream-json");

    if is_claude {
        inject_read_only_tools(&mut args);
    }
    let effort = crate::config::resolve_effort_for_model(
        &config.ai_hub,
        &config.agent,
        resolved_provider_id.as_deref(),
        resolved_model_id.as_deref(),
        runtime_effort,
        None,
    );
    inject_provider_effort(
        family,
        &mut args,
        resolved_model_id.as_deref(),
        effort.as_deref(),
    );
    // Card AI returns text on stdout; the host persists replies. No managed
    // storage --add-dir is required (and would be overly broad for Codex).

    CardAiInvocation {
        command,
        args,
        work_dir,
        is_claude_compatible: is_claude,
        uses_stream_json,
        env,
    }
}

fn fallback_agent(
    config: &ErConfig,
) -> (
    String,
    Vec<String>,
    bool,
    Option<String>,
    Option<String>,
    crate::config::CliFamily,
) {
    let cmd = config.agent.command.clone();
    let is_claude = crate::config::agent_command_is_claude(&cmd);
    let family = crate::config::CliFamily::detect(&cmd);
    (
        cmd,
        config.agent.args.clone(),
        is_claude,
        None,
        (!config.agent.model.is_empty()).then(|| config.agent.model.clone()),
        family,
    )
}

fn inject_read_only_tools(args: &mut Vec<String>) {
    const TOOLS: &[&str] = &[
        "Read",
        "Bash(grep *)",
        "Bash(rg *)",
        "Bash(git grep*)",
        "Bash(git show*)",
        "Bash(git log*)",
    ];
    for rule in TOOLS.iter().rev() {
        args.insert(0, rule.to_string());
        args.insert(0, "--allowedTools".to_string());
    }
}

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

/// Run card AI subprocess; honors `ER_FAKE_CLAUDE` for tests.
pub fn run_card_ai_subprocess(
    inv: &CardAiInvocation,
    system: &str,
    user: &str,
    model_override: Option<&str>,
) -> String {
    if let Ok(fake) = std::env::var("ER_FAKE_CLAUDE") {
        return match fake.as_str() {
            "fail" => "Pending — invoke via CLI (error: ER_FAKE_CLAUDE=fail)".to_string(),
            "ok" => "mocked ok".to_string(),
            other if !other.is_empty() => other.to_string(),
            _ => "mocked ok".to_string(),
        };
    }

    let mut args = build_card_ai_argv(inv, system, user);
    if let Some(model) = model_override.filter(|m| !m.trim().is_empty()) {
        if inv.is_claude_compatible && !args.iter().any(|a| a == "--model") {
            args.push("--model".to_string());
            args.push(model.to_string());
        }
    }

    let result = {
        let mut cmd = Command::new(&inv.command);
        cmd.args(&args).current_dir(&inv.work_dir);
        for (key, value) in &inv.env {
            cmd.env(key, value);
        }
        cmd.output()
    };

    match result {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let mut s = extract_reply_from_stdout(&stdout, inv.uses_stream_json);
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
        Ok(out) => {
            let err = String::from_utf8_lossy(&out.stderr);
            format!(
                "Pending — invoke via CLI ({} exited {}: {})",
                inv.command,
                out.status.code().unwrap_or(-1),
                err.trim()
            )
        }
        Err(e) => format!(
            "Pending — invoke via CLI (failed to spawn {}: {e})",
            inv.command
        ),
    }
}

fn extract_reply_from_stdout(stdout: &str, uses_stream_json: bool) -> String {
    if !uses_stream_json {
        return stdout.to_string();
    }

    let mut last_result: Option<String> = None;
    let mut assistant_text: Vec<String> = Vec::new();

    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
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
        };
        let args = build_card_ai_argv(&inv, "system context", "how does this work?");
        assert!(args
            .windows(2)
            .any(|pair| { pair[0] == "--append-system-prompt" && pair[1] == "system context" }));
        assert!(args.iter().any(|a| a == "how does this work?"));
        assert!(!args.iter().any(|a| a.contains("User request:")));
    }

    #[test]
    fn extract_stream_json_result_field() {
        let stdout = concat!(
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"working"}]}}"#,
            "\n",
            r#"{"type":"result","subtype":"success","result":"**Verdict**: Confirmed"}"#,
        );
        let inv = CardAiInvocation {
            command: "claude".into(),
            args: vec![],
            work_dir: "/tmp".into(),
            is_claude_compatible: true,
            uses_stream_json: true,
            env: vec![],
        };
        let reply = extract_reply_from_stdout(stdout, inv.uses_stream_json);
        assert_eq!(reply, "**Verdict**: Confirmed");
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

    // ── run_card_ai_subprocess ────────────────────────────────────────────
    //
    // The subprocess is a plain `sh` script, so these exercise the real spawn
    // path (argv assembly, exit-status classification, output shaping) without
    // an agent CLI or a network call.

    // Serialize env-var-touching tests to avoid races on parallel runners.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn with_fake_claude<R>(value: Option<&str>, f: impl FnOnce() -> R) -> R {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev = std::env::var("ER_FAKE_CLAUDE").ok();
        match value {
            Some(v) => std::env::set_var("ER_FAKE_CLAUDE", v),
            None => std::env::remove_var("ER_FAKE_CLAUDE"),
        }
        let out = f();
        match prev {
            Some(v) => std::env::set_var("ER_FAKE_CLAUDE", v),
            None => std::env::remove_var("ER_FAKE_CLAUDE"),
        }
        out
    }

    fn sh_invocation(script: &str) -> CardAiInvocation {
        CardAiInvocation {
            command: "sh".into(),
            args: vec!["-c".into(), script.into()],
            work_dir: std::env::temp_dir().to_string_lossy().into_owned(),
            is_claude_compatible: false,
            uses_stream_json: false,
            env: vec![],
        }
    }

    #[test]
    fn run_card_ai_subprocess_maps_each_fake_claude_value_to_a_canned_reply() {
        let inv = sh_invocation("echo real-subprocess-ran");
        assert_eq!(
            with_fake_claude(Some("fail"), || run_card_ai_subprocess(
                &inv, "s", "u", None
            )),
            "Pending — invoke via CLI (error: ER_FAKE_CLAUDE=fail)"
        );
        assert_eq!(
            with_fake_claude(Some("ok"), || run_card_ai_subprocess(&inv, "s", "u", None)),
            "mocked ok"
        );
        assert_eq!(
            with_fake_claude(Some("canned body"), || run_card_ai_subprocess(
                &inv, "s", "u", None
            )),
            "canned body",
            "any other value is returned verbatim as the reply"
        );
        assert_eq!(
            with_fake_claude(Some(""), || run_card_ai_subprocess(&inv, "s", "u", None)),
            "mocked ok",
            "an empty value still short-circuits the subprocess"
        );
    }

    #[test]
    fn run_card_ai_subprocess_returns_trimmed_stdout_on_success() {
        let inv = sh_invocation("echo '  mocked-reply  '");
        let reply = with_fake_claude(None, || run_card_ai_subprocess(&inv, "sys", "usr", None));
        assert_eq!(reply, "mocked-reply");
    }

    #[test]
    fn run_card_ai_subprocess_reports_an_empty_response_as_pending() {
        let inv = sh_invocation("true");
        let reply = with_fake_claude(None, || run_card_ai_subprocess(&inv, "sys", "usr", None));
        assert_eq!(reply, "Pending — invoke via CLI (empty response)");
    }

    #[test]
    fn run_card_ai_subprocess_reports_the_exit_code_and_stderr_on_failure() {
        let inv = sh_invocation("echo boom >&2; exit 3");
        let reply = with_fake_claude(None, || run_card_ai_subprocess(&inv, "sys", "usr", None));
        assert_eq!(reply, "Pending — invoke via CLI (sh exited 3: boom)");
    }

    #[test]
    fn run_card_ai_subprocess_reports_a_spawn_failure_with_the_command_name() {
        let mut inv = sh_invocation("true");
        inv.command = "er-no-such-agent-binary".into();
        let reply = with_fake_claude(None, || run_card_ai_subprocess(&inv, "sys", "usr", None));
        assert!(
            reply.starts_with("Pending — invoke via CLI (failed to spawn er-no-such-agent-binary:"),
            "unspawnable agent is reported, not panicked on: {reply}"
        );
    }

    #[test]
    fn run_card_ai_subprocess_truncates_replies_over_8_kib() {
        // 200 × 100 characters of output, no whitespace, so the trim after the
        // truncation cannot hide an off-by-one in the cap.
        let inv = sh_invocation(
            "i=0; while [ $i -lt 200 ]; do printf '%s' \
             0123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789; \
             i=$((i+1)); done",
        );
        let reply = with_fake_claude(None, || run_card_ai_subprocess(&inv, "sys", "usr", None));
        assert_eq!(reply.len(), 8 * 1024, "reply capped at 8 KiB");
    }

    #[test]
    fn run_card_ai_subprocess_extracts_the_stream_json_result_when_the_agent_streams() {
        let mut inv = sh_invocation(
            "echo '{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"working\"}]}}'; \
             echo '{\"type\":\"result\",\"result\":\"Verdict: Confirmed\"}'",
        );
        inv.uses_stream_json = true;
        let reply = with_fake_claude(None, || run_card_ai_subprocess(&inv, "sys", "usr", None));
        assert_eq!(
            reply, "Verdict: Confirmed",
            "the final result event wins over streamed assistant text"
        );

        // The same stdout is passed through untouched when the invocation is
        // not a stream-json one.
        inv.uses_stream_json = false;
        let raw = with_fake_claude(None, || run_card_ai_subprocess(&inv, "sys", "usr", None));
        assert!(raw.contains("\"type\":\"assistant\""), "raw stdout: {raw}");
    }

    #[test]
    fn run_card_ai_subprocess_appends_the_model_override_only_for_claude_agents() {
        // `sh -c '<script>' sh …` puts the assembled argv in "$@".
        let echo_argv = |claude: bool| {
            let mut inv = sh_invocation("echo \"$@\"");
            inv.args.push("sh".into());
            inv.is_claude_compatible = claude;
            inv
        };

        let reply = with_fake_claude(None, || {
            run_card_ai_subprocess(&echo_argv(true), "sys", "usr", Some("opus-x"))
        });
        assert!(
            reply.contains("--model opus-x"),
            "claude-compatible agents get the override appended: {reply}"
        );

        let reply = with_fake_claude(None, || {
            run_card_ai_subprocess(&echo_argv(false), "sys", "usr", Some("opus-x"))
        });
        assert!(
            !reply.contains("--model"),
            "non-claude agents carry their model in their own args: {reply}"
        );

        let reply = with_fake_claude(None, || {
            run_card_ai_subprocess(&echo_argv(true), "sys", "usr", Some("   "))
        });
        assert!(
            !reply.contains("--model"),
            "a blank override is ignored: {reply}"
        );

        let mut preset = echo_argv(true);
        preset.args.push("--model".into());
        preset.args.push("sonnet-y".into());
        let reply = with_fake_claude(None, || {
            run_card_ai_subprocess(&preset, "sys", "usr", Some("opus-x"))
        });
        assert!(
            reply.contains("--model sonnet-y") && !reply.contains("opus-x"),
            "an explicit --model in the invocation is not overridden: {reply}"
        );
    }
}
