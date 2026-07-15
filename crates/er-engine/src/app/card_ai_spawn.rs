//! Subprocess invocation for desktop card-level AI (Ask AI / Validate with AI).

use crate::config::{
    agent_command_uses_stream_json, inject_agent_storage_access, inject_provider_effort, ErConfig,
};
use std::process::Command;

/// Resolved agent command + args for a card AI subprocess.
pub struct CardAiInvocation {
    pub command: String,
    pub args: Vec<String>,
    pub work_dir: String,
    pub is_claude_compatible: bool,
    pub uses_stream_json: bool,
}

/// Resolve provider/command/args from config (mirrors background review selection).
pub fn plan_card_ai_invocation(
    config: &ErConfig,
    provider_id: Option<&str>,
    model_id: Option<&str>,
    runtime_effort: Option<&str>,
    work_dir: String,
) -> CardAiInvocation {
    let (command, mut args, is_claude, resolved_provider_id, resolved_model_id) = if let Some(pid) =
        config.ai_hub.resolve_provider_id(provider_id)
    {
        if let Some(provider) = config.ai_hub.providers.get(&pid) {
            let mut args = provider.args.clone();
            let resolved_model_id = config.ai_hub.resolve_model_id(&pid, model_id);
            if let Some(mid) = &resolved_model_id {
                if let Some(model) = provider.models.iter().find(|m| m.id == *mid) {
                    args.extend(model.args.clone());
                }
            }
            let is_claude = provider.command.ends_with("claude") || provider.command == "claude";
            (
                provider.command.clone(),
                args,
                is_claude,
                Some(pid.to_string()),
                resolved_model_id,
            )
        } else {
            fallback_agent(config)
        }
    } else {
        fallback_agent(config)
    };

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
        &command,
        &mut args,
        resolved_model_id.as_deref(),
        effort.as_deref(),
    );
    inject_agent_storage_access(&command, &mut args);

    CardAiInvocation {
        command,
        args,
        work_dir,
        is_claude_compatible: is_claude,
        uses_stream_json,
    }
}

fn fallback_agent(
    config: &ErConfig,
) -> (String, Vec<String>, bool, Option<String>, Option<String>) {
    let cmd = config.agent.command.clone();
    let is_claude = cmd.ends_with("claude") || cmd == "claude";
    (
        cmd,
        config.agent.args.clone(),
        is_claude,
        None,
        (!config.agent.model.is_empty()).then(|| config.agent.model.clone()),
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
    let combined_prompt = if inv.is_claude_compatible {
        user.to_string()
    } else if system.is_empty() {
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

    let result = Command::new(&inv.command)
        .args(&args)
        .current_dir(&inv.work_dir)
        .output();

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
        };
        let args = build_card_ai_argv(&inv, "system context", "how does this work?");
        assert!(args.windows(2).any(|pair| {
            pair[0] == "--append-system-prompt" && pair[1] == "system context"
        }));
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
        assert!(inv.args.windows(2).any(|pair| {
            pair[0] == "--add-dir"
                && pair[1] == crate::storage::storage_root().to_string_lossy().as_ref()
        }));
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
        assert!(inv.args.windows(2).any(|pair| {
            pair[0] == "--add-dir"
                && pair[1] == crate::storage::storage_root().to_string_lossy().as_ref()
        }));
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
