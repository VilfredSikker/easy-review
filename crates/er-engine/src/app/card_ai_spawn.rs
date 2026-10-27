//! Subprocess invocation for desktop card-level AI (Ask AI / Validate with AI).

use crate::config::{agent_command_uses_stream_json, ErConfig};
use std::io::{BufRead, BufReader, Read};
use std::process::{Child, Command};
use std::sync::Arc;

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

    // Spawned through the run handle rather than `cmd.output()`, so card AI
    // gets the same deadline as the other spawn paths. It was the one path
    // that never created a child at all, which is why it could not be stopped
    // and could not time out.
    let run = crate::agent_run::AgentRunHandle::new();

    // Waits for a slot, like every other spawn path. This is the path a user
    // waits on directly, so it is the one where queueing is felt -- but a cap
    // that exempts the interactive path is not a cap, and the caller runs
    // under `run_blocking` so the UI stays responsive while it waits.
    let Some(slot) = crate::agent_slots::acquire(
        crate::agent_slots::Workload::Background,
        inv.slot_cap,
        inv.slot_ceiling,
        run.cancel_flag(),
    ) else {
        return "Pending — invoke via CLI (stopped while waiting for a slot)".to_string();
    };

    spawn_and_settle(inv, args, &run, slot)
}

/// Spawn the agent holding `slot`, and turn what it produced into the reply.
///
/// Split from [`run_card_ai_subprocess`] so a test can hand in a slot from its
/// own pool: the process-wide one is shared with every other test running.
fn spawn_and_settle(
    inv: &CardAiInvocation,
    mut args: Vec<String>,
    run: &Arc<crate::agent_run::AgentRunHandle>,
    slot: crate::agent_slots::AgentSlotGuard<'static>,
) -> String {
    let stream_json = if inv.is_claude_compatible {
        ensure_claude_stream_json(&mut args)
    } else {
        inv.uses_stream_json
    };

    let result = {
        let mut cmd = Command::new(&inv.command);
        cmd.args(&args)
            .current_dir(&inv.work_dir)
            // `cmd.output()` set these itself; spawning by hand means asking.
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        for (key, value) in &inv.env {
            cmd.env(key, value);
        }
        match run.spawn(&mut cmd) {
            // The Claude CLI goes on running for seconds after it prints its
            // answer, and the user is waiting on this one.
            Ok(child) if inv.is_claude_compatible && stream_json => {
                match read_until_result(run, child, inv.timeout, slot) {
                    StreamOutcome::Reply(reply) => return finish_reply(reply),
                    StreamOutcome::Exited(out) => out.map_err(anyhow::Error::from),
                }
            }
            Ok(child) => {
                let id = child.id();
                run.register(child);
                run.arm_timeout(inv.timeout);
                match run.take_child(id) {
                    Some(child) => run.wait_for_output(child).map_err(anyhow::Error::from),
                    // Unreachable: `register` just put it there under this id.
                    // Reported as a spawn failure rather than panicking, since
                    // this path's contract is to hand back a string.
                    None => Err(anyhow::anyhow!("card AI: lost its own child")),
                }
            }
            Err(e) => Err(e),
        }
    };

    match result {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            finish_reply(extract_reply_from_stdout(&stdout, stream_json))
        }
        Ok(out) => {
            // Reported through this path's usual string rather than the engine
            // error types: card AI hands back display text, and a caller that
            // has to tell a timeout from a crash here would need the signature
            // changed for it.
            if run.is_timed_out() {
                return format!(
                    "Pending — invoke via CLI ({})",
                    crate::agent_run::timed_out(inv.timeout)
                );
            }
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

/// What reading a stream-json child up to its `result` event came to.
enum StreamOutcome {
    /// The reply, handed back while the child may still be shutting down.
    Reply(String),
    /// The child closed stdout without a usable `result` and has been reaped,
    /// so the run settles on its exit status exactly as the wait path does.
    Exited(std::io::Result<std::process::Output>),
}

/// An error result is not an answer: that run settles on its exit status, so
/// the user sees the same message the wait path would give.
fn is_final_result(line: &str) -> bool {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
        return false;
    };
    v.get("type").and_then(|t| t.as_str()) == Some("result")
        && v.get("is_error").and_then(|e| e.as_bool()) != Some(true)
        && v.get("result")
            .and_then(|r| r.as_str())
            .is_some_and(|r| !r.trim().is_empty())
}

/// Read stdout until the `result` event and hand the reply back then, leaving
/// the child to be reaped on a thread of its own.
fn read_until_result(
    run: &Arc<crate::agent_run::AgentRunHandle>,
    mut child: Child,
    timeout: std::time::Duration,
    slot: crate::agent_slots::AgentSlotGuard<'static>,
) -> StreamOutcome {
    let id = child.id();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    run.register(child);
    run.arm_timeout(timeout);

    let Some(stdout) = stdout else {
        // Unreachable: the command was built with a piped stdout.
        return StreamOutcome::Exited(match run.take_child(id) {
            Some(child) => run.wait_for_output(child),
            None => Err(std::io::Error::other("card AI: lost its own child")),
        });
    };
    // Drained alongside stdout: a child blocked on a full stderr pipe never
    // gets as far as its `result` line.
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
        let text = String::from_utf8_lossy(&line);
        seen.push_str(&text);
        if is_final_result(&text) {
            let reply = extract_reply_from_stdout(&seen, true);
            let run = Arc::clone(run);
            std::thread::spawn(move || {
                // The slot goes with the reap, so the ADR 0021 cap still
                // counts a CLI that has answered but not yet exited.
                let _slot = slot;
                // Dropping the pipe instead would kill the CLI on its next write.
                let _ = std::io::copy(&mut reader, &mut std::io::sink());
                let _ = stderr_reader.join();
                if let Some(child) = run.take_child(id) {
                    let _ = run.wait_for(child);
                }
            });
            return StreamOutcome::Reply(reply);
        }
    }

    let stderr = stderr_reader.join().unwrap_or_default();
    StreamOutcome::Exited(match run.take_child(id) {
        Some(child) => run.wait_for(child).map(|status| std::process::Output {
            status,
            stdout: seen.into_bytes(),
            stderr,
        }),
        None => Err(std::io::Error::other("card AI: lost its own child")),
    })
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
            assistant_text.extend(crate::agent_runtime::assistant_text_blocks(&v));
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
            // Irrelevant to argv construction; these tests never spawn.
            timeout: std::time::Duration::from_secs(900),
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
            timeout: std::time::Duration::from_secs(900),
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
            // Irrelevant to argv construction; these tests never spawn.
            timeout: std::time::Duration::from_secs(900),
            slot_cap: 3,
            slot_ceiling: 6,
        };
        let reply = extract_reply_from_stdout(stdout, inv.uses_stream_json);
        assert_eq!(reply, "**Verdict**: Confirmed");
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
            // Long enough never to fire here; short enough that a regression
            // fails the suite instead of hanging it.
            timeout: std::time::Duration::from_secs(30),
            slot_cap: 1,
            slot_ceiling: 1,
        }
    }

    #[cfg(unix)]
    fn pid_alive(pid: &str) -> bool {
        Command::new("/bin/kill")
            .args(["-0", pid])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    /// The point of the change: the reply comes back on the `result` line,
    /// while the CLI is still running, and the slot stays held until the
    /// child is reaped.
    ///
    /// The child also writes more stderr than a pipe buffers before its
    /// answer, so it only reaches the `result` line if stderr is drained.
    #[cfg(unix)]
    #[test]
    fn the_reply_returns_on_the_result_line_and_the_slot_waits_for_exit() {
        static POOL: crate::agent_slots::SlotPool = crate::agent_slots::SlotPool::new();
        let dir = std::env::temp_dir().join(format!("er-cardai-early-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let pid_file = dir.join("pid");
        let _ = std::fs::remove_file(&pid_file);

        let script = format!(
            "echo $$ > {pid}; \
             head -c 131072 /dev/zero >&2; \
             echo '{{\"type\":\"assistant\",\"message\":{{\"content\":[{{\"type\":\"text\",\"text\":\"working\"}}]}}}}'; \
             echo '{{\"type\":\"result\",\"subtype\":\"success\",\"result\":\"fast reply\"}}'; \
             sleep 3",
            pid = pid_file.display()
        );
        let inv = fake_claude(&script);
        let run = crate::agent_run::AgentRunHandle::new();
        let slot = POOL.acquire_blocking(crate::agent_slots::Workload::Background, 1, 1);

        let started = std::time::Instant::now();
        let reply = spawn_and_settle(&inv, inv.args.clone(), &run, slot);
        let elapsed = started.elapsed();

        assert_eq!(reply, "fast reply");
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "the reply waited for the child to exit: {elapsed:?}"
        );
        let pid = std::fs::read_to_string(&pid_file).expect("child wrote its pid");
        let pid = pid.trim();
        assert!(pid_alive(pid), "the child should still be sleeping");
        assert_eq!(
            POOL.active_count(),
            1,
            "the slot was released while the child was still running"
        );

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while POOL.active_count() > 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert_eq!(POOL.active_count(), 0, "the slot was never released");
        assert!(
            !pid_alive(pid),
            "the slot was released before the child was reaped"
        );
        assert!(run.is_finished(), "the reaper must mark the run finished");
    }

    /// A child that never prints a `result` line settles on its exit status,
    /// with the same message the wait-for-exit path gives.
    #[cfg(unix)]
    #[test]
    fn a_child_exiting_without_a_result_line_reports_its_exit() {
        static POOL: crate::agent_slots::SlotPool = crate::agent_slots::SlotPool::new();
        let inv = fake_claude(
            "echo '{\"type\":\"assistant\",\"message\":{\"content\":[]}}'; echo boom >&2; exit 3",
        );
        let run = crate::agent_run::AgentRunHandle::new();
        let slot = POOL.acquire_blocking(crate::agent_slots::Workload::Background, 1, 1);

        let reply = spawn_and_settle(&inv, inv.args.clone(), &run, slot);

        assert_eq!(reply, "Pending — invoke via CLI (sh exited 3: boom)");
        assert_eq!(POOL.active_count(), 0);
        assert!(run.is_finished());
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
