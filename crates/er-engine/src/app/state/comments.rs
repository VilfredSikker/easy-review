use super::*;

/// Review agents never need a second checkout: they run inside the repo (or, for a remote PR
/// whose repo is not checked out locally, work from the prepared diff plus `gh`). Cloning has
/// only ever produced stray copies under `/tmp` that later runs then read stale code out of.
/// A speed bump rather than a wall — `gh repo clone` and `cd x && git clone` slip past it.
const CLONE_DENY_RULE: &str = "Bash(git clone*)";
const IMPORTANCE_READ_ONLY_TOOLS: &[&str] = &["Read", "Glob", "Grep"];
const MCP_DENY_RULE: &str = "mcp__*";

const DIAGRAM_READ_ONLY_TOOLS: &[&str] = &[
    "Read",
    "Bash(grep *)",
    "Bash(rg *)",
    "Bash(git grep*)",
    "Bash(git show*)",
    "Bash(git log*)",
];
const WRITABLE_PREPARED_DIFF_TOOLS: &[&str] = &[
    "Read",
    "Write",
    "Edit",
    "Bash(grep *)",
    "Bash(rg *)",
    "Bash(git grep*)",
    "Bash(git show*)",
    "Bash(git fetch origin pull/*)",
    "Bash(cp .er/*)",
    "Bash(shasum*)",
    "Bash(sha256sum*)",
    "Bash(mkdir*)",
    "Bash(awk*)",
];
const WRITABLE_TOOLS: &[&str] = &[
    "Read",
    "Write",
    "Edit",
    "Bash(gh pr *)",
    "Bash(cp .er/*)",
    "Bash(git diff*)",
    "Bash(git grep*)",
    "Bash(git show*)",
    "Bash(git fetch origin pull/*)",
    "Bash(shasum*)",
    "Bash(sha256sum*)",
    "Bash(mkdir*)",
];

fn background_run_is_read_only(task_kind: &str, host_write_diagram: bool) -> bool {
    host_write_diagram || task_kind == crate::ai::prompts::IMPORTANCE_TASK_KIND
}

fn claude_allowed_tools(read_only: bool, prepared_diff: bool) -> &'static [&'static str] {
    if read_only {
        DIAGRAM_READ_ONLY_TOOLS
    } else if prepared_diff {
        WRITABLE_PREPARED_DIFF_TOOLS
    } else {
        WRITABLE_TOOLS
    }
}

/// Check the final configured launch policy before prompt text is substituted.
fn validate_importance_launch(
    family: crate::config::CliFamily,
    args: &[String],
    opencode_env: Option<&(String, String)>,
    trusted_codex_effort: Option<&str>,
) -> Result<()> {
    use crate::config::CliFamily;
    let has = |flag: &str| {
        args.iter()
            .any(|arg| arg == flag || arg.starts_with(&format!("{flag}=")))
    };
    let has_split = |flag: &str| {
        args.iter()
            .enumerate()
            .any(|(i, arg)| arg == flag && args.get(i + 1).is_some())
    };
    match family {
        CliFamily::Other => anyhow::bail!(
            "Importance is unavailable for custom CLI families because their read-only policy is unknown"
        ),
        CliFamily::Codex => {
            let sandbox: Vec<_> = args
                .iter()
                .enumerate()
                .filter_map(|(i, arg)| {
                    if arg == "--sandbox" {
                        args.get(i + 1).map(String::as_str)
                    } else {
                        arg.strip_prefix("--sandbox=")
                    }
                })
                .collect();
            if sandbox.as_slice() != ["read-only"]
                || args.iter().any(|arg| arg.starts_with("-s"))
                || has("--full-auto")
                || has("--yolo")
                || has("--dangerously-bypass-approvals-and-sandbox")
                || has("--add-dir")
            {
                anyhow::bail!(
                    "Importance requires one Codex --sandbox read-only option and no write grants or bypass flags"
                );
            }
            if args
                .iter()
                .filter(|a| a.as_str() == "--ignore-user-config")
                .count()
                != 1
            {
                anyhow::bail!("Importance requires the engine's Codex --ignore-user-config option");
            }
            let codex_config: Vec<_> = args
                .iter()
                .enumerate()
                .filter(|(_, arg)| arg.as_str() == "-c")
                .collect();
            if let Some(level) = trusted_codex_effort {
                if codex_config.len() != 1
                    || args.get(codex_config[0].0 + 1).map(String::as_str)
                        != Some(format!("model_reasoning_effort={level}").as_str())
                {
                    anyhow::bail!(
                        "Importance requires the engine's configured Codex effort override"
                    );
                }
            } else if !codex_config.is_empty() {
                anyhow::bail!("Importance rejects Codex config overrides");
            }
            if has("--config")
                || has_split("--add-dir")
                || args.iter().any(|a| a.starts_with("--add-dir="))
            {
                anyhow::bail!("Importance rejects Codex config overrides and --add-dir grants");
            }
        }
        CliFamily::Cursor => {
            let modes: Vec<_> = args
                .iter()
                .enumerate()
                .filter_map(|(i, arg)| {
                    if arg == "--mode" {
                        args.get(i + 1).map(String::as_str)
                    } else {
                        arg.strip_prefix("--mode=")
                    }
                })
                .collect();
            if modes.as_slice() != ["plan"]
                || has("--force")
                || has("--yolo")
                || has("--add-dir")
                || has_split("--add-dir")
                || has("--config")
                || has("--settings")
            {
                anyhow::bail!(
                    "Importance requires one Cursor --mode plan option and no force, yolo, or add-dir options"
                );
            }
        }
        CliFamily::Claude => {
            let values = |flag: &str| -> Vec<&str> {
                args.iter().enumerate().filter_map(|(i, arg)| {
                    if arg == flag {
                        args.get(i + 1).map(String::as_str)
                    } else {
                        arg.strip_prefix(&format!("{flag}="))
                    }
                }).collect()
            };
            let tools = values("--tools");
            // Autoapproval rules are additive to inherited settings. Only the
            // actual tool set prevents inherited Write/Edit/Bash grants from
            // reopening writes. MCP tools require their own deny rule.
            if tools.len() != 1 || tools[0] != IMPORTANCE_READ_ONLY_TOOLS.join(",")
                || args.iter().filter(|arg| *arg == "--tools" || arg.starts_with("--tools=")).count() != 1
                || !values("--disallowedTools").contains(&MCP_DENY_RULE)
            {
                anyhow::bail!("Importance requires the engine's Claude read-only tool ceiling and MCP denial");
            }
            for flag in ["--allowedTools", "--allowed-tools"] {
                if values(flag).iter().any(|tool| !IMPORTANCE_READ_ONLY_TOOLS.contains(tool)) {
                    anyhow::bail!("Importance rejects Claude tool autoapproval outside its read-only ceiling");
                }
            }
            for flag in ["--disallowedTools", "--disallowed-tools"] {
                if values(flag).iter().any(|rule| ![MCP_DENY_RULE, CLONE_DENY_RULE].contains(rule)) {
                    anyhow::bail!("Importance rejects unexpected Claude tool-denial overrides");
                }
            }
            let overrides = [
                "--permission-mode",
                "--dangerously-skip-permissions",
                "--allow-dangerously-skip-permissions",
                "--settings",
                "--setting-sources",
                "--plugin-dir",
            ];
            if overrides.iter().any(|flag| has(flag) || has_split(flag)) {
                anyhow::bail!("Importance rejects Claude permission and tool-list overrides");
            }
        }
        CliFamily::OpenCode => {
            if has("--permission") || has("--config") || has("-c") || has("--agent") {
                anyhow::bail!("Importance rejects OpenCode permission/config overrides");
            }
            let Some((key, value)) = opencode_env else {
                anyhow::bail!(
                    "Importance requires the engine-generated OpenCode read-only permission"
                );
            };
            let parsed: serde_json::Value = serde_json::from_str(value)
                .context("Importance received an invalid OpenCode permission policy")?;
            if key != "OPENCODE_PERMISSION"
                || parsed.get("edit").and_then(serde_json::Value::as_str) != Some("deny")
                || parsed
                    .pointer("/bash/*")
                    .and_then(serde_json::Value::as_str)
                    != Some("deny")
            {
                anyhow::bail!(
                    "Importance requires the engine-generated OpenCode read-only permission"
                );
            }
        }
    }
    Ok(())
}

/// Reject permission and config overrides before trusted model flags are injected.
fn validate_importance_user_config(
    family: crate::config::CliFamily,
    args: &[String],
) -> Result<()> {
    use crate::config::CliFamily;
    let has = |flag: &str| {
        args.iter()
            .any(|arg| arg == flag || arg.starts_with(&format!("{flag}=")))
    };
    match family {
        CliFamily::Other => anyhow::bail!(
            "Importance is unavailable for custom CLI families because their read-only policy is unknown"
        ),
        CliFamily::Codex
            if args
                .iter()
                .any(|arg| arg.starts_with("-c") || arg.starts_with("-s"))
                || has("--config") =>
        {
            anyhow::bail!("Importance rejects configured Codex -c/-s and config overrides");
        }
        CliFamily::Codex => {
            let sandboxes: Vec<_> = args
                .iter()
                .enumerate()
                .filter_map(|(i, arg)| {
                    if arg == "--sandbox" {
                        args.get(i + 1).map(String::as_str)
                    } else {
                        arg.strip_prefix("--sandbox=")
                    }
                })
                .collect();
            if sandboxes.len() > 1 || sandboxes.iter().any(|value| *value != "read-only") {
                anyhow::bail!("Importance rejects writable or duplicate Codex sandbox options");
            }
        }
        CliFamily::Cursor
            if ["--force", "--yolo", "--add-dir", "--config", "--settings"]
                .iter()
                .any(|flag| has(flag)) =>
        {
            anyhow::bail!("Importance rejects Cursor write and config overrides");
        }
        CliFamily::Claude
            if [
                "--permission-mode",
                "--dangerously-skip-permissions",
                "--allow-dangerously-skip-permissions",
                "--allowedTools",
                "--allowed-tools",
                "--tools",
                "--disallowedTools",
                "--disallowed-tools",
                "--settings",
                "--setting-sources",
                "--plugin-dir",
            ]
            .iter()
            .any(|flag| has(flag)) =>
        {
            anyhow::bail!("Importance rejects Claude permission and tool-list overrides");
        }
        CliFamily::OpenCode
            if ["--permission", "--config", "-c", "--agent"]
                .iter()
                .any(|flag| has(flag)) =>
        {
            anyhow::bail!("Importance rejects OpenCode permission/config overrides");
        }
        _ => {}
    }
    Ok(())
}

/// Wakes a host poll after the worker sends its result, including an
/// unexpected worker panic that disconnects the result channel.
struct NotifyBackgroundProgressOnDrop {
    result_tx: Option<std::sync::mpsc::Sender<Result<()>>>,
    notify: Option<crate::arena::ArenaNotify>,
}

impl NotifyBackgroundProgressOnDrop {
    fn send_result(&mut self, result: Result<()>) {
        if let Some(result_tx) = self.result_tx.take() {
            let _ = result_tx.send(result);
        }
        self.notify_host();
    }

    fn notify_host(&mut self) {
        if let Some(notify) = self.notify.take() {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| notify()));
        }
    }
}

impl Drop for NotifyBackgroundProgressOnDrop {
    fn drop(&mut self) {
        // On panic the receiver must already see Disconnected before the wake.
        drop(self.result_tx.take());
        self.notify_host();
    }
}

#[cfg(test)]
mod background_allowlist_tests {
    use super::*;

    #[test]
    fn importance_is_read_only() {
        assert!(background_run_is_read_only(
            crate::ai::prompts::IMPORTANCE_TASK_KIND,
            false
        ));
        assert!(!background_run_is_read_only("review", false));
    }

    #[test]
    fn host_written_diagrams_are_read_only() {
        assert!(background_run_is_read_only("diagram", true));
    }

    #[test]
    fn readonly_branches_share_the_diagram_allowlist() {
        assert_eq!(claude_allowed_tools(true, false), DIAGRAM_READ_ONLY_TOOLS);
        assert_eq!(claude_allowed_tools(true, true), DIAGRAM_READ_ONLY_TOOLS);
    }

    #[test]
    fn prepared_diff_writable_list_is_unchanged() {
        assert_eq!(
            claude_allowed_tools(false, true),
            &[
                "Read",
                "Write",
                "Edit",
                "Bash(grep *)",
                "Bash(rg *)",
                "Bash(git grep*)",
                "Bash(git show*)",
                "Bash(git fetch origin pull/*)",
                "Bash(cp .er/*)",
                "Bash(shasum*)",
                "Bash(sha256sum*)",
                "Bash(mkdir*)",
                "Bash(awk*)",
            ]
        );
    }

    #[test]
    fn ordinary_writable_list_is_unchanged() {
        assert_eq!(
            claude_allowed_tools(false, false),
            &[
                "Read",
                "Write",
                "Edit",
                "Bash(gh pr *)",
                "Bash(cp .er/*)",
                "Bash(git diff*)",
                "Bash(git grep*)",
                "Bash(git show*)",
                "Bash(git fetch origin pull/*)",
                "Bash(shasum*)",
                "Bash(sha256sum*)",
                "Bash(mkdir*)",
            ]
        );
    }
}

#[cfg(test)]
mod importance_launch_policy {
    use super::*;

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn codex_requires_exactly_one_read_only_sandbox_in_split_or_equals_form() {
        for args in [
            strings(&["exec", "--sandbox", "read-only", "--ignore-user-config"]),
            strings(&["exec", "--sandbox=read-only", "--ignore-user-config"]),
        ] {
            validate_importance_launch(crate::config::CliFamily::Codex, &args, None, None).unwrap();
        }
        for args in [
            strings(&[
                "exec",
                "--sandbox=read-only",
                "--sandbox",
                "workspace-write",
                "--ignore-user-config",
            ]),
            strings(&[
                "exec",
                "--sandbox",
                "workspace-write",
                "--ignore-user-config",
            ]),
            strings(&[
                "exec",
                "--sandbox=read-only",
                "--ignore-user-config",
                "--add-dir=/tmp",
            ]),
        ] {
            assert!(
                validate_importance_launch(crate::config::CliFamily::Codex, &args, None, None)
                    .is_err()
            );
        }
    }

    #[test]
    fn cursor_requires_one_plan_mode_and_rejects_write_options() {
        for args in [strings(&["--mode", "plan"]), strings(&["--mode=plan"])] {
            validate_importance_launch(crate::config::CliFamily::Cursor, &args, None, None)
                .unwrap();
        }
        assert!(validate_importance_launch(
            crate::config::CliFamily::Cursor,
            &strings(&["--mode=plan", "--mode", "ask"]),
            None,
            None
        )
        .is_err());
        assert!(validate_importance_launch(
            crate::config::CliFamily::Cursor,
            &strings(&["--mode=plan", "--force"]),
            None,
            None
        )
        .is_err());
    }

    #[test]
    fn importance_claude_requires_actual_read_tool_ceiling() {
        for args in [
            strings(&[
                "--print",
                "--tools",
                "Read,Glob,Grep",
                "--disallowedTools",
                "mcp__*",
            ]),
            strings(&[
                "--print",
                "--tools=Read,Glob,Grep",
                "--disallowedTools=mcp__*",
            ]),
        ] {
            validate_importance_launch(crate::config::CliFamily::Claude, &args, None, None)
                .unwrap();
        }
        for args in [
            strings(&["--print", "{prompt}"]),
            strings(&[
                "--allowedTools",
                "Read",
                "--disallowedTools",
                "Bash(git clone*)",
            ]),
            strings(&[
                "--tools",
                "Read,Glob,Grep,Write",
                "--disallowedTools=mcp__*",
            ]),
            strings(&["--tools", "Read,Glob,Grep"]),
            strings(&["--tools=Read,Glob,Grep,Bash", "--disallowedTools=mcp__*"]),
            strings(&[
                "--tools",
                "Read,Glob,Grep",
                "--tools=default",
                "--disallowedTools=mcp__*",
            ]),
            strings(&[
                "--tools",
                "Read,Glob,Grep",
                "--tools",
                "--disallowedTools=mcp__*",
            ]),
            strings(&[
                "--tools",
                "Read,Glob,Grep",
                "--permission-mode=bypassPermissions",
                "--disallowedTools=mcp__*",
            ]),
            strings(&[
                "--tools",
                "Read,Glob,Grep",
                "--disallowedTools=mcp__*",
                "--allowedTools=Write",
            ]),
        ] {
            assert!(
                validate_importance_launch(crate::config::CliFamily::Claude, &args, None, None,)
                    .is_err(),
                "{args:?}"
            );
        }
    }

    #[test]
    fn importance_claude_rejects_configured_tool_ceiling_overrides() {
        for flag in [
            "--tools",
            "--allowedTools",
            "--allowed-tools",
            "--disallowedTools",
            "--disallowed-tools",
            "--permission-mode",
            "--settings",
            "--setting-sources",
            "--plugin-dir",
        ] {
            for args in [
                strings(&[flag, "Write"]),
                strings(&[&format!("{flag}=Write")]),
            ] {
                assert!(
                    validate_importance_user_config(crate::config::CliFamily::Claude, &args)
                        .is_err(),
                    "{args:?}"
                );
            }
        }
    }

    #[test]
    fn claude_rejects_permission_overrides_before_canonical_allowlist_insertion() {
        assert!(validate_importance_launch(
            crate::config::CliFamily::Claude,
            &strings(&["--permission-mode", "bypassPermissions"]),
            None,
            None
        )
        .is_err());
        assert!(validate_importance_launch(
            crate::config::CliFamily::Claude,
            &strings(&["--allowedTools=Write"]),
            None,
            None
        )
        .is_err());
        validate_importance_user_config(
            crate::config::CliFamily::Claude,
            &strings(&["--print", "{prompt}"]),
        )
        .unwrap();
    }

    #[test]
    fn opencode_requires_engine_permission_environment_and_rejects_overrides() {
        let mut args = strings(&["run", "--auto"]);
        let env = crate::config::apply_opencode_readonly_storage_spawn(
            crate::config::CliFamily::OpenCode,
            &mut args,
            Some("/managed/review"),
        )
        .unwrap();
        validate_importance_launch(crate::config::CliFamily::OpenCode, &args, Some(&env), None)
            .unwrap();
        assert!(validate_importance_launch(
            crate::config::CliFamily::OpenCode,
            &strings(&["run", "--permission", "allow"]),
            Some(&env),
            None
        )
        .is_err());
        assert!(
            validate_importance_launch(crate::config::CliFamily::OpenCode, &args, None, None)
                .is_err()
        );
    }

    #[test]
    fn unknown_cli_family_is_rejected() {
        assert!(
            validate_importance_launch(crate::config::CliFamily::Other, &[], None, None).is_err()
        );
    }
}

/// Whether a completed agent run writes its full transcript to
/// `debug-agent.log`.
///
/// Off by default. The write formatted the whole of stdout and stderr into a
/// third copy of strings the reader threads already held, on every run, to
/// produce a file nothing reads unless someone is debugging a spawn.
fn debug_agent_log_enabled() -> bool {
    std::env::var("ER_DEBUG").is_ok()
}

/// Delete GitHub comments in order on a background thread, discarding results.
fn spawn_github_comment_deletes(gh: ai::GitHubSyncState, ids: Vec<u64>, repo_root: String) {
    std::thread::spawn(move || {
        for id in ids {
            let _ = crate::github::gh_pr_delete_comment(&gh.owner, &gh.repo, id, &repo_root);
        }
    });
}

/// What one agent stdout line adds to the agent log: the parsed stream-json
/// event, or the trimmed raw line for other formats. `None` when that is empty.
fn stdout_log_text(line: &str, is_stream_json: bool) -> Option<String> {
    let display = if is_stream_json {
        parse_stream_json_line(line)
    } else {
        Some(line.trim().to_string())
    };
    display.filter(|text| !text.is_empty())
}

/// Cap a command failure at 80 chars for the status bar (safe for multi-byte UTF-8).
fn status_bar_failure(msg: String) -> String {
    if msg.len() <= 80 {
        return msg;
    }
    let boundary = msg.char_indices().nth(80).map_or(msg.len(), |(i, _)| i);
    format!("{}…", msg.get(..boundary).unwrap_or_default())
}

/// A failed agent's stderr, trimmed and capped at 280 chars for the error
/// message. Char-safe: agent stderr often includes multi-byte text.
fn agent_stderr_snippet(stderr_lines: &[String]) -> String {
    const MAX_CHARS: usize = 280;
    let lines: Vec<_> = stderr_lines
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect();
    let is_error = |line: &&str| {
        line.trim_start()
            .get(..6)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("error:"))
    };
    let is_auth_error = |line: &&str| {
        let lower = line.to_ascii_lowercase();
        [
            "authentication",
            "not logged in",
            "not authenticated",
            "unauthorized",
        ]
        .iter()
        .any(|marker| lower.contains(marker))
    };
    let start = lines
        .iter()
        .rposition(is_error)
        .or_else(|| lines.iter().rposition(is_auth_error));
    let (snippet, keep_start) = if let Some(start) = start {
        (lines[start..].join("\n"), true)
    } else {
        (lines.join("\n"), false)
    };
    let chars: Vec<_> = snippet.chars().collect();
    if chars.len() <= MAX_CHARS {
        return snippet;
    }
    if keep_start {
        chars.iter().take(MAX_CHARS - 1).collect::<String>() + "…"
    } else {
        "…".to_string()
            + &chars
                .iter()
                .skip(chars.len() - (MAX_CHARS - 1))
                .collect::<String>()
    }
}

fn mint_comment_id(prefix: &str) -> String {
    let seq = COMMENT_SEQ.fetch_add(1, Ordering::Relaxed);
    format!(
        "{prefix}{}-{seq}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    )
}

/// Anchor on 1-based `line` of a whole document, with the same three lines of
/// context a hunk anchor keeps. `None` when the line is past the end.
fn document_line_anchor(source: &str, line: usize) -> Option<LineAnchor> {
    let lines: Vec<&str> = source.lines().collect();
    let idx = line.checked_sub(1).filter(|&i| i < lines.len())?;
    let owned = |range: &[&str]| range.iter().map(|l| (*l).to_string()).collect();
    Some(LineAnchor {
        line_start: Some(line),
        line_content: lines[idx].to_string(),
        context_before: owned(&lines[idx.saturating_sub(3)..idx]),
        context_after: owned(&lines[idx + 1..(idx + 4).min(lines.len())]),
        old_line_start: None,
        hunk_header: String::new(),
    })
}

fn take_comment_id(tab: &mut TabState, prefix: &str) -> String {
    let usable = tab
        .comment_id_override
        .as_deref()
        .is_some_and(|id| !id.is_empty() && id.starts_with(prefix));
    if usable {
        return tab
            .comment_id_override
            .take()
            .unwrap_or_else(|| mint_comment_id(prefix));
    }
    mint_comment_id(prefix)
}

/// Who a desktop submit is written as. Both fields are set on every desktop
/// submit, so an override never carries over into the next one.
#[derive(Default)]
struct ReplyAttribution {
    author: Option<String>,
    ai_action: Option<String>,
}

impl App {
    // ── Comment System ──

    /// Enter comment mode for the current file + hunk (and optionally line)
    pub fn start_comment(&mut self, comment_type: CommentType) {
        let split_active = self.split_diff_active(&self.config);
        let split_focus = self.tab().split_focus;
        let tab = self.tab_mut();
        let file_path = match tab.selected_diff_file() {
            Some(f) => f.path.clone(),
            None => return,
        };
        tab.comment_textarea = TextArea::default();
        tab.comment_file = file_path;
        tab.comment_hunk = tab.current_hunk;
        tab.comment_line_num = if split_active {
            tab.current_line_number_for_split(split_focus)
        } else {
            tab.current_line_number()
        };
        tab.comment_reply_to = None;
        tab.comment_finding_ref = None;
        tab.comment_type = comment_type;
        let side = tab.comment_side_for_cursor(split_active);
        tab.comment_side = Some(side);
        self.input_mode = InputMode::Comment;
    }

    /// Start typing a general PR comment (not attached to any file/line)
    pub fn start_general_comment(&mut self) {
        let tab = self.tab_mut();
        tab.comment_textarea = TextArea::default();
        tab.comment_file = String::new();
        tab.comment_hunk = 0;
        tab.comment_line_num = None;
        tab.comment_line_end = None;
        tab.comment_reply_to = None;
        tab.comment_finding_ref = None;
        tab.comment_type = CommentType::GitHubComment;
        tab.comment_edit_id = None;
        self.input_mode = InputMode::Comment;
    }

    /// Start editing an existing comment — opens comment input pre-filled with its text
    pub fn start_edit_comment(&mut self, comment_id: &str) {
        let tab = self.tab();
        // Find the comment text and type
        let (text, comment_type) = if comment_id.starts_with("q-") {
            if let Some(qs) = &tab.ai.questions {
                if let Some(q) = qs.questions.iter().find(|q| q.id == comment_id) {
                    (q.text.clone(), CommentType::Question)
                } else {
                    return;
                }
            } else {
                return;
            }
        } else if comment_id.starts_with("n-") {
            if let Some(ns) = &tab.ai.notes {
                if let Some(n) = ns.notes.iter().find(|n| n.id == comment_id) {
                    (n.text.clone(), CommentType::Note)
                } else {
                    return;
                }
            } else {
                return;
            }
        } else if let Some(gc) = &tab.ai.github_comments {
            if let Some(c) = gc.comments.iter().find(|c| c.id == comment_id) {
                (c.comment.clone(), CommentType::GitHubComment)
            } else {
                return;
            }
        } else {
            return;
        };

        let tab = self.tab_mut();
        let file_path = match tab.selected_diff_file() {
            Some(f) => f.path.clone(),
            None => return,
        };
        tab.comment_textarea = TextArea::new(vec![text]);
        tab.comment_file = file_path;
        tab.comment_hunk = tab.current_hunk;
        tab.comment_line_num = tab.current_line_number();
        tab.comment_reply_to = None;
        tab.comment_type = comment_type;
        tab.comment_edit_id = Some(comment_id.to_string());
        self.input_mode = InputMode::Comment;
    }

    /// Start replying to a comment or question — creates a threaded reply
    pub fn start_reply_comment(&mut self, comment_id: &str) {
        let tab = self.tab();
        // Determine type from ID prefix and find the parent comment's location
        let (file, hunk_index, line_start, comment_type) = if comment_id.starts_with("q-") {
            if let Some(qs) = &tab.ai.questions {
                if let Some(q) = qs.questions.iter().find(|q| q.id == comment_id) {
                    (
                        q.file.clone(),
                        q.hunk_index.unwrap_or(0),
                        q.line_start,
                        CommentType::Question,
                    )
                } else {
                    return;
                }
            } else {
                return;
            }
        } else if comment_id.starts_with("n-") {
            if let Some(ns) = &tab.ai.notes {
                if let Some(n) = ns.notes.iter().find(|n| n.id == comment_id) {
                    (
                        n.file.clone(),
                        n.hunk_index.unwrap_or(0),
                        n.line_start,
                        CommentType::Note,
                    )
                } else {
                    return;
                }
            } else {
                return;
            }
        } else if let Some(gc) = &tab.ai.github_comments {
            if let Some(c) = gc.comments.iter().find(|c| c.id == comment_id) {
                (
                    c.file.clone(),
                    c.hunk_index.unwrap_or(0),
                    c.line_start,
                    CommentType::GitHubComment,
                )
            } else {
                return;
            }
        } else {
            return;
        };

        let tab = self.tab_mut();
        tab.comment_textarea = TextArea::default();
        tab.comment_file = file;
        tab.comment_hunk = hunk_index;
        tab.comment_line_num = line_start;
        tab.comment_reply_to = Some(comment_id.to_string());
        tab.comment_finding_ref = None;
        tab.comment_type = comment_type;
        tab.comment_edit_id = None;
        self.input_mode = InputMode::Comment;
    }

    /// Start replying to an AI finding — creates a GitHubComment referencing the finding
    pub fn start_reply_finding(&mut self, finding_id: &str) {
        let tab = self.tab();
        // Find the finding's file and location
        let (file, hunk_index, line_start) = if let Some(review) = &tab.ai.review {
            let mut found = None;
            for (file_path, file_review) in &review.files {
                for finding in &file_review.findings {
                    if finding.id == finding_id {
                        found = Some((
                            file_path.clone(),
                            finding.hunk_index.unwrap_or(0),
                            finding.line_start,
                        ));
                        break;
                    }
                }
                if found.is_some() {
                    break;
                }
            }
            match found {
                Some(f) => f,
                None => {
                    self.notify("Finding not found — review may be stale");
                    return;
                }
            }
        } else {
            self.notify("No AI review loaded — cannot reply to finding");
            return;
        };

        let tab = self.tab_mut();
        tab.comment_textarea = TextArea::default();
        tab.comment_file = file;
        tab.comment_hunk = hunk_index;
        tab.comment_line_num = line_start;
        tab.comment_reply_to = None;
        tab.comment_finding_ref = Some(finding_id.to_string());
        tab.comment_type = CommentType::GitHubComment;
        tab.comment_edit_id = None;
        self.input_mode = InputMode::Comment;
    }

    /// Submit the current comment/question to the appropriate file
    pub fn submit_comment(&mut self) -> Result<()> {
        let tab = self.tab();
        let text = tab.comment_text();
        if text.is_empty() {
            self.tab_mut().comment_id_override = None;
            self.input_mode = InputMode::Normal;
            return Ok(());
        }

        // If editing an existing comment, update it in-place
        if let Some(edit_id) = tab.comment_edit_id.clone() {
            return self.update_comment(edit_id, text);
        }

        let comment_type = tab.comment_type;
        match comment_type {
            CommentType::Question => self.submit_question(text),
            CommentType::Note => self.submit_note(text),
            CommentType::GitHubComment => self.submit_github_comment(text),
        }
    }

    /// Submit a personal review question to .er-questions.json
    fn submit_question(&mut self, text: String) -> Result<()> {
        let tab = self.tab();
        let er_dir = tab.er_dir();
        let repo_root = tab.repo_root.clone();
        let mut diff_hash = tab.branch_diff_hash.clone();
        let base_branch = tab.base_branch.clone();
        let file_path = tab.comment_file.clone();
        let hunk_index = tab.comment_hunk;
        let comment_line_num = tab.comment_line_num;
        let comment_line_end = tab.comment_line_end;
        let reply_to = tab.comment_reply_to.clone();
        let pr_head_ref_owned = tab.pr_head_ref.clone();

        // Compute branch_diff_hash on-demand when not yet set (e.g., non-Branch mode with no AI data).
        // Without this, questions would always be marked stale because the hash would be empty.
        // Skip in remote mode — git_diff_raw requires a local git repo.
        if diff_hash.is_empty() && !self.tab().is_remote() {
            if let Ok(br) = git::git_diff_raw(
                "branch",
                &base_branch,
                &repo_root,
                pr_head_ref_owned.as_deref(),
            ) {
                diff_hash = ai::compute_diff_hash(&br);
                self.tab_mut().branch_diff_hash = diff_hash.clone();
            }
        }

        let (hunk_index, anchor) = self.take_comment_anchor(hunk_index, comment_line_num);

        // Load or create questions.json
        let questions_path = format!("{}/questions.json", er_dir);
        let mut questions: ai::ErQuestions = match std::fs::read_to_string(&questions_path) {
            Ok(content) => match serde_json::from_str(&content) {
                Ok(qs) => qs,
                Err(_) => {
                    self.notify("Warning: .er/questions.json is invalid JSON — starting fresh");
                    ai::ErQuestions {
                        version: 1,
                        diff_hash: diff_hash.clone(),
                        questions: Vec::new(),
                    }
                }
            },
            Err(_) => ai::ErQuestions {
                version: 1,
                diff_hash: diff_hash.clone(),
                questions: Vec::new(),
            },
        };

        // If diff hash changed, update it but preserve existing questions
        // (the relocation system handles comment drift)
        if questions.diff_hash != diff_hash {
            questions.diff_hash = diff_hash;
        }

        let id = take_comment_id(self.tab_mut(), "q-");

        let is_reply = reply_to.is_some();
        let finding_ref = self.tab().comment_finding_ref.clone();
        let author = self
            .tab_mut()
            .comment_author_override
            .take()
            .unwrap_or_else(|| "You".to_string());
        let side = self
            .tab_mut()
            .comment_side
            .take()
            .unwrap_or_else(|| "RIGHT".to_string());
        questions.questions.push(ai::ReviewQuestion {
            id,
            timestamp: chrono_now(),
            file: file_path,
            hunk_index,
            line_start: anchor.line_start,
            line_end: Self::normalize_line_end(anchor.line_start, comment_line_end),
            line_content: anchor.line_content,
            text: text.clone(),
            resolved: false,
            stale: false,
            context_before: anchor.context_before,
            context_after: anchor.context_after,
            old_line_start: anchor.old_line_start,
            side,
            hunk_header: anchor.hunk_header,
            anchor_status: "original".to_string(),
            relocated_at_hash: self.tab().diff_hash.clone(),
            in_reply_to: reply_to,
            author,
            promoted_to: None,
            finding_ref,
            ai_action: self.tab_mut().comment_ai_action_override.take(),
        });

        // Write atomically
        std::fs::create_dir_all(&er_dir)?;
        let json = serde_json::to_string_pretty(&questions)?;
        let tmp_path = format!("{}.tmp", questions_path);
        std::fs::write(&tmp_path, json)?;
        std::fs::rename(&tmp_path, &questions_path)?;

        self.tab_mut().ai.questions = Some(questions);
        self.tab_mut().ai.rebuild_comment_index();
        self.tab_mut().mark_sidecar_written(&questions_path);
        self.tab_mut().comment_textarea = TextArea::default();
        self.input_mode = InputMode::Normal;
        let label = if is_reply { "Reply" } else { "Question" };
        self.notify(&format!("{} added: {}", label, truncate(&text, 40)));
        Ok(())
    }

    /// Submit a local note to notes.json. Mirrors `submit_question` but writes to
    /// the separate notes sidecar and uses an `n-` id prefix.
    fn submit_note(&mut self, text: String) -> Result<()> {
        let tab = self.tab();
        let er_dir = tab.er_dir();
        let repo_root = tab.repo_root.clone();
        let mut diff_hash = tab.branch_diff_hash.clone();
        let base_branch = tab.base_branch.clone();
        let file_path = tab.comment_file.clone();
        let hunk_index = tab.comment_hunk;
        let comment_line_num = tab.comment_line_num;
        let comment_line_end = tab.comment_line_end;
        let reply_to = tab.comment_reply_to.clone();
        let pr_head_ref_owned = tab.pr_head_ref.clone();

        if diff_hash.is_empty() && !self.tab().is_remote() {
            if let Ok(br) = git::git_diff_raw(
                "branch",
                &base_branch,
                &repo_root,
                pr_head_ref_owned.as_deref(),
            ) {
                diff_hash = ai::compute_diff_hash(&br);
                self.tab_mut().branch_diff_hash = diff_hash.clone();
            }
        }

        let (hunk_index, anchor) = self.take_comment_anchor(hunk_index, comment_line_num);

        // Load or create notes.json
        let notes_path = format!("{}/notes.json", er_dir);
        let mut notes: ai::ErNotes = match std::fs::read_to_string(&notes_path) {
            Ok(content) => match serde_json::from_str(&content) {
                Ok(ns) => ns,
                Err(_) => {
                    self.notify("Warning: .er/notes.json is invalid JSON — starting fresh");
                    ai::ErNotes {
                        version: 1,
                        diff_hash: diff_hash.clone(),
                        notes: Vec::new(),
                    }
                }
            },
            Err(_) => ai::ErNotes {
                version: 1,
                diff_hash: diff_hash.clone(),
                notes: Vec::new(),
            },
        };

        if notes.diff_hash != diff_hash {
            notes.diff_hash = diff_hash;
        }

        let id = take_comment_id(self.tab_mut(), "n-");

        let is_reply = reply_to.is_some();
        let finding_ref = self.tab().comment_finding_ref.clone();
        let author = self
            .tab_mut()
            .comment_author_override
            .take()
            .unwrap_or_else(|| "You".to_string());
        let side = self
            .tab_mut()
            .comment_side
            .take()
            .unwrap_or_else(|| "RIGHT".to_string());
        notes.notes.push(ai::ReviewQuestion {
            id,
            timestamp: chrono_now(),
            file: file_path,
            hunk_index,
            line_start: anchor.line_start,
            line_end: Self::normalize_line_end(anchor.line_start, comment_line_end),
            line_content: anchor.line_content,
            text: text.clone(),
            resolved: false,
            stale: false,
            context_before: anchor.context_before,
            context_after: anchor.context_after,
            old_line_start: anchor.old_line_start,
            side,
            hunk_header: anchor.hunk_header,
            anchor_status: "original".to_string(),
            relocated_at_hash: self.tab().diff_hash.clone(),
            in_reply_to: reply_to,
            author,
            promoted_to: None,
            finding_ref,
            ai_action: self.tab_mut().comment_ai_action_override.take(),
        });

        // Write atomically
        std::fs::create_dir_all(&er_dir)?;
        let json = serde_json::to_string_pretty(&notes)?;
        let tmp_path = format!("{}.tmp", notes_path);
        std::fs::write(&tmp_path, json)?;
        std::fs::rename(&tmp_path, &notes_path)?;

        self.tab_mut().ai.notes = Some(notes);
        self.tab_mut().ai.rebuild_comment_index();
        self.tab_mut().mark_sidecar_written(&notes_path);
        self.tab_mut().comment_textarea = TextArea::default();
        self.input_mode = InputMode::Normal;
        let label = if is_reply { "Reply" } else { "Note" };
        self.notify(&format!("{} added: {}", label, truncate(&text, 40)));
        Ok(())
    }

    /// Submit a GitHub PR comment to .er/github-comments.json
    fn submit_github_comment(&mut self, text: String) -> Result<()> {
        let tab = self.tab();
        let diff_hash = tab.branch_diff_hash.clone();
        let file_path = tab.comment_file.clone();
        let hunk_index = tab.comment_hunk;
        let reply_to = tab.comment_reply_to.clone();
        let finding_ref = tab.comment_finding_ref.clone();
        let comment_line_num = tab.comment_line_num;
        let comment_line_end = tab.comment_line_end;

        let anchor = self.get_line_anchor(hunk_index, comment_line_num);

        // Load or create github-comments.json (uses cache dir in remote mode)
        let comments_path = self.tab().github_comments_path();
        let mut gh_comments: ai::ErGitHubComments = match std::fs::read_to_string(&comments_path) {
            Ok(content) => match serde_json::from_str(&content) {
                Ok(gc) => gc,
                Err(_) => {
                    self.notify(
                        "Warning: .er/github-comments.json is invalid JSON — starting fresh",
                    );
                    ai::ErGitHubComments {
                        version: 1,
                        diff_hash: diff_hash.clone(),
                        github: None,
                        comments: Vec::new(),
                    }
                }
            },
            Err(_) => ai::ErGitHubComments {
                version: 1,
                diff_hash: diff_hash.clone(),
                github: None,
                comments: Vec::new(),
            },
        };

        // If diff hash changed, update it but preserve existing comments
        // (the relocation system handles comment drift)
        if gh_comments.diff_hash != diff_hash {
            gh_comments.diff_hash = diff_hash;
        }

        let id = take_comment_id(self.tab_mut(), "c-");

        let is_reply = reply_to.is_some();
        let author = self
            .tab_mut()
            .comment_author_override
            .take()
            .unwrap_or_else(|| "You".to_string());
        let side = self
            .tab_mut()
            .comment_side
            .take()
            .unwrap_or_else(|| "RIGHT".to_string());
        gh_comments.comments.push(ai::GitHubReviewComment {
            id,
            timestamp: chrono_now(),
            file: file_path,
            hunk_index: Some(hunk_index),
            line_start: anchor.line_start,
            line_end: Self::normalize_line_end(anchor.line_start, comment_line_end),
            line_content: anchor.line_content,
            comment: text.clone(),
            in_reply_to: reply_to,
            resolved: false,
            source: "local".to_string(),
            github_id: None,
            author,
            synced: false,
            outdated: false,
            stale: false,
            context_before: anchor.context_before,
            context_after: anchor.context_after,
            old_line_start: anchor.old_line_start,
            hunk_header: anchor.hunk_header,
            anchor_status: "original".to_string(),
            relocated_at_hash: self.tab().diff_hash.clone(),
            finding_ref,
            side,
        });

        // Write atomically (github-comments.json is PR-scoped — shared PR bucket)
        let comments_dir = self.tab().github_comments_dir();
        std::fs::create_dir_all(&comments_dir)?;
        let json = serde_json::to_string_pretty(&gh_comments)?;
        let tmp_path = format!("{}.tmp", comments_path);
        std::fs::write(&tmp_path, json)?;
        std::fs::rename(&tmp_path, &comments_path)?;

        // Keep the in-memory copy. `reload_ai_state()` re-reads every sidecar
        // (review/experts/tour/…) and is what made each local inline comment
        // feel like a GitHub round-trip. The comment is local and unpushed.
        self.tab_mut().ai.github_comments = Some(gh_comments);
        self.tab_mut().ai.rebuild_comment_index();
        self.tab_mut().mark_sidecar_written(&comments_path);
        self.tab_mut().comment_textarea = TextArea::default();
        self.input_mode = InputMode::Normal;
        let label = if is_reply { "Reply" } else { "Comment" };
        self.notify(&format!("{} added: {}", label, truncate(&text, 40)));
        Ok(())
    }

    /// Anchor for the question or note being submitted, with the hunk it is
    /// stored under. A document anchor set by `submit_document_comment` wins
    /// and stores no hunk; a reply to a document-anchored thread inherits its
    /// parent's line, since the diff holds no line to re-anchor it on.
    fn take_comment_anchor(
        &mut self,
        hunk_index: usize,
        comment_line_num: Option<usize>,
    ) -> (Option<usize>, LineAnchor) {
        let explicit = self.tab_mut().comment_document_anchor.take();
        let anchor = explicit.or_else(|| {
            let parent = self.tab().comment_reply_to.as_deref()?;
            self.document_anchor_of(parent)
        });
        match anchor {
            Some(anchor) => (None, anchor),
            None => (
                Some(hunk_index),
                self.get_line_anchor(hunk_index, comment_line_num),
            ),
        }
    }

    /// The stored line of a question or note anchored on the document rather
    /// than a hunk, or `None` for any other id. Replies and promotions copy it
    /// so they stay on that line.
    pub fn document_anchor_of(&self, id: &str) -> Option<LineAnchor> {
        let ai = &self.tab().ai;
        ai.questions
            .iter()
            .flat_map(|qs| &qs.questions)
            .chain(ai.notes.iter().flat_map(|ns| &ns.notes))
            .find(|q| q.id == id && q.is_document_anchor())
            .map(|q| LineAnchor {
                line_start: q.line_start,
                line_content: q.line_content.clone(),
                context_before: q.context_before.clone(),
                context_after: q.context_after.clone(),
                old_line_start: None,
                hunk_header: String::new(),
            })
    }

    /// Anchor a question or note on a line of the document itself, which the
    /// preview can show even when no hunk holds it. `source` is the file's
    /// full new-side text; the caller reads it, since that can be a network
    /// fetch and must not happen under the app lock. GitHub only takes
    /// comments on diff lines, so a document anchor is private-only.
    pub fn submit_document_comment(
        &mut self,
        target: CommentTarget,
        text: String,
        comment_type: CommentType,
        source: &str,
    ) -> Result<()> {
        if comment_type == CommentType::GitHubComment {
            anyhow::bail!("A GitHub comment must sit on a line in the diff");
        }
        let line = target.line_num.context("A document comment needs a line")?;
        let anchor = document_line_anchor(source, line)
            .with_context(|| format!("Line {line} is past the end of {}", target.file))?;
        self.tab_mut().comment_document_anchor = Some(anchor);
        let result = self.submit_comment_text_inner(
            target,
            text,
            comment_type,
            None,
            None,
            ReplyAttribution::default(),
        );
        // An empty body returns before the anchor is taken; never let it leak
        // into the next submit.
        self.tab_mut().comment_document_anchor = None;
        result
    }

    /// Richer anchor data captured when placing a comment
    pub(crate) fn get_line_anchor(
        &self,
        hunk_index: usize,
        comment_line_num: Option<usize>,
    ) -> LineAnchor {
        let tab = self.tab();
        let diff_file = if tab.comment_file.is_empty() {
            tab.selected_diff_file()
        } else if tab.mode == DiffMode::History {
            tab.history.as_ref().and_then(|history| {
                history
                    .commit_files
                    .iter()
                    .find(|f| f.path == tab.comment_file)
            })
        } else {
            tab.files.iter().find(|f| f.path == tab.comment_file)
        };

        if let Some(df) = diff_file {
            if let Some(hunk) = df.hunks.get(hunk_index) {
                if let Some(ln) = comment_line_num {
                    // Find the target line index within the hunk
                    let target_idx = match tab.comment_side.as_deref() {
                        Some("LEFT") => hunk
                            .lines
                            .iter()
                            .position(|l| l.old_num == Some(ln))
                            .or_else(|| hunk.lines.iter().position(|l| l.new_num == Some(ln))),
                        Some("RIGHT") => hunk
                            .lines
                            .iter()
                            .position(|l| l.new_num == Some(ln))
                            .or_else(|| hunk.lines.iter().position(|l| l.old_num == Some(ln))),
                        _ => hunk
                            .lines
                            .iter()
                            .position(|l| l.new_num == Some(ln))
                            .or_else(|| hunk.lines.iter().position(|l| l.old_num == Some(ln))),
                    };
                    let (line_content, old_line_start) = if let Some(idx) = target_idx {
                        let dl = &hunk.lines[idx];
                        (dl.content.clone(), dl.old_num)
                    } else {
                        (String::new(), None)
                    };

                    // Collect up to 3 content lines before the target (same hunk)
                    let context_before = if let Some(idx) = target_idx {
                        let start = idx.saturating_sub(3);
                        hunk.lines[start..idx]
                            .iter()
                            .map(|l| l.content.clone())
                            .collect()
                    } else {
                        Vec::new()
                    };

                    // Collect up to 3 content lines after the target (same hunk)
                    let context_after = if let Some(idx) = target_idx {
                        let end = (idx + 4).min(hunk.lines.len());
                        hunk.lines[(idx + 1)..end]
                            .iter()
                            .map(|l| l.content.clone())
                            .collect()
                    } else {
                        Vec::new()
                    };

                    LineAnchor {
                        line_start: Some(ln),
                        line_content,
                        context_before,
                        context_after,
                        old_line_start,
                        hunk_header: hunk.header.clone(),
                    }
                } else {
                    // Hunk-level comment
                    LineAnchor {
                        line_start: None,
                        line_content: hunk.header.clone(),
                        context_before: Vec::new(),
                        context_after: Vec::new(),
                        old_line_start: None,
                        hunk_header: hunk.header.clone(),
                    }
                }
            } else {
                LineAnchor::default()
            }
        } else {
            LineAnchor::default()
        }
    }

    /// Inclusive end line for a multi-line anchor; `None` when single-line or invalid.
    pub(crate) const fn normalize_line_end(
        line_start: Option<usize>,
        line_end: Option<usize>,
    ) -> Option<usize> {
        match (line_start, line_end) {
            (Some(start), Some(end)) if end > start => Some(end),
            _ => None,
        }
    }

    /// Submit a comment or question without going through InputMode flow.
    /// Used by the desktop app where there is no TextArea widget.
    pub fn submit_comment_text(
        &mut self,
        target: CommentTarget,
        text: String,
        comment_type: CommentType,
        reply_to: Option<String>,
        finding_ref: Option<String>,
    ) -> Result<()> {
        self.submit_comment_text_inner(
            target,
            text,
            comment_type,
            reply_to,
            finding_ref,
            ReplyAttribution::default(),
        )
    }

    /// Submit an AI-written reply: `author` is "ai" rather than "You", and a
    /// question or note reply also records which AI action wrote it, so a front
    /// end can label it. Used by the desktop card AI flows.
    pub fn submit_ai_reply(
        &mut self,
        target: CommentTarget,
        text: String,
        comment_type: CommentType,
        reply_to: Option<String>,
        ai_action: Option<&str>,
    ) -> Result<()> {
        self.submit_comment_text_inner(
            target,
            text,
            comment_type,
            reply_to,
            None,
            ReplyAttribution {
                author: Some("ai".to_string()),
                ai_action: ai_action.map(str::to_string),
            },
        )
    }

    fn submit_comment_text_inner(
        &mut self,
        target: CommentTarget,
        text: String,
        comment_type: CommentType,
        reply_to: Option<String>,
        finding_ref: Option<String>,
        attribution: ReplyAttribution,
    ) -> Result<()> {
        let CommentTarget {
            file,
            hunk_idx,
            line_num,
            line_num_end,
        } = target;
        {
            let tab = self.tab_mut();
            tab.comment_file = file;
            tab.comment_hunk = hunk_idx;
            tab.comment_line_num = line_num;
            tab.comment_line_end = Self::normalize_line_end(line_num, line_num_end);
            tab.comment_reply_to = reply_to;
            tab.comment_finding_ref = finding_ref;
            tab.comment_type = comment_type;
            tab.comment_edit_id = None;
            tab.comment_textarea = TextArea::new(vec![text]);
            tab.comment_author_override = attribution.author;
            tab.comment_ai_action_override = attribution.ai_action;
        }
        self.input_mode = InputMode::Comment;
        self.submit_comment()
    }

    /// Delete a comment or question by ID, bypassing the confirmation dialog.
    /// Used by the desktop app.
    pub fn delete_comment_direct(&mut self, comment_id: &str) -> Result<()> {
        self.confirm_delete_comment(comment_id)
    }

    /// Update comment/question body in `.er/` sidecars. Syncs to GitHub when `github_id` is set.
    pub fn update_comment_text(&mut self, comment_id: &str, new_text: &str) -> Result<()> {
        let er_dir = self.tab().er_dir();
        let repo_root = self.tab().repo_root.clone();

        if comment_id.starts_with("q-") {
            let path = format!("{}/questions.json", er_dir);
            let content =
                std::fs::read_to_string(&path).with_context(|| format!("Failed to read {path}"))?;
            let mut qs: ai::ErQuestions =
                serde_json::from_str(&content).context("Failed to parse questions.json")?;
            let q = qs
                .questions
                .iter_mut()
                .find(|q| q.id == comment_id)
                .context("Question not found")?;
            if q.author == "ai" {
                anyhow::bail!("Cannot edit AI-generated text");
            }
            q.text = new_text.to_string();
            let json = serde_json::to_string_pretty(&qs)?;
            let tmp = format!("{path}.tmp");
            std::fs::write(&tmp, json)?;
            std::fs::rename(&tmp, &path)?;
            self.adopt_questions(qs, &path);
        } else if comment_id.starts_with("n-") {
            let path = format!("{}/notes.json", er_dir);
            let content =
                std::fs::read_to_string(&path).with_context(|| format!("Failed to read {path}"))?;
            let mut ns: ai::ErNotes =
                serde_json::from_str(&content).context("Failed to parse notes.json")?;
            let n = ns
                .notes
                .iter_mut()
                .find(|n| n.id == comment_id)
                .context("Note not found")?;
            if n.author == "ai" {
                anyhow::bail!("Cannot edit AI-generated text");
            }
            n.text = new_text.to_string();
            let json = serde_json::to_string_pretty(&ns)?;
            let tmp = format!("{path}.tmp");
            std::fs::write(&tmp, json)?;
            std::fs::rename(&tmp, &path)?;
            self.adopt_notes(ns, &path);
        } else {
            let path = self.tab().github_comments_path();
            let content =
                std::fs::read_to_string(&path).with_context(|| format!("Failed to read {path}"))?;
            let mut gc: ai::ErGitHubComments =
                serde_json::from_str(&content).context("Failed to parse github-comments.json")?;
            let (github_id, gh_meta, author) = {
                let c = gc
                    .comments
                    .iter()
                    .find(|c| c.id == comment_id)
                    .context("Comment not found")?;
                (c.github_id, gc.github.clone(), c.author.clone())
            };
            if author == "ai" {
                anyhow::bail!("Cannot edit AI-generated text");
            }
            if let (Some(gh_id), Some(gh)) = (github_id, gh_meta.as_ref()) {
                crate::github::gh_pr_update_review_comment(
                    &gh.owner, &gh.repo, gh_id, new_text, &repo_root,
                )?;
            }
            if let Some(c) = gc.comments.iter_mut().find(|c| c.id == comment_id) {
                c.comment = new_text.to_string();
            }
            let json = serde_json::to_string_pretty(&gc)?;
            let tmp = format!("{path}.tmp");
            std::fs::write(&tmp, json)?;
            std::fs::rename(&tmp, &path)?;
            self.adopt_github_comments(gc, &path);
        }

        Ok(())
    }

    /// Cancel comment input
    pub fn cancel_comment(&mut self) {
        self.tab_mut().comment_textarea = TextArea::default();
        self.tab_mut().comment_edit_id = None;
        self.input_mode = InputMode::Normal;
    }

    /// Check if there is a non-empty comment draft that is paused (not actively being edited)
    pub fn has_comment_draft(&self) -> bool {
        let lines = self.tab().comment_textarea.lines();
        let has_text = lines.len() > 1 || !lines[0].is_empty();
        has_text && self.input_mode != InputMode::Comment
    }

    /// Pause comment editing — return to normal mode but keep the draft
    pub fn pause_comment(&mut self) {
        self.input_mode = InputMode::Normal;
    }

    /// Resume editing a paused comment draft
    pub fn resume_comment(&mut self) {
        if self.has_comment_draft() {
            self.input_mode = InputMode::Comment;
        }
    }

    /// Whether the user may switch question ↔ GitHub comment while composing.
    pub fn can_toggle_comment_type(&self) -> bool {
        let tab = self.tab();
        tab.comment_reply_to.is_none()
            && tab.comment_edit_id.is_none()
            && tab.comment_finding_ref.is_none()
            && !tab.comment_file.is_empty()
    }

    /// Flip question ↔ GitHub comment for a new file-anchored draft.
    pub fn toggle_comment_type(&mut self) {
        if !self.can_toggle_comment_type() {
            return;
        }
        let tab = self.tab_mut();
        // Cycle through all three local draft kinds: question → note → comment.
        tab.comment_type = match tab.comment_type {
            CommentType::Question => CommentType::Note,
            CommentType::Note => CommentType::GitHubComment,
            CommentType::GitHubComment => CommentType::Question,
        };
    }

    /// Update an existing comment in-place: new text, re-anchored to current position
    fn update_comment(&mut self, comment_id: String, new_text: String) -> Result<()> {
        let tab = self.tab();
        let er_dir = tab.er_dir();
        let hunk_index = tab.comment_hunk;
        let comment_line_num = tab.comment_line_num;

        let anchor = self.get_line_anchor(hunk_index, comment_line_num);
        let diff_hash = self.tab().diff_hash.clone();

        if comment_id.starts_with("q-") {
            let path = format!("{}/questions.json", er_dir);
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(mut qs) = serde_json::from_str::<ai::ErQuestions>(&content) {
                    if let Some(q) = qs.questions.iter_mut().find(|q| q.id == comment_id) {
                        q.text = new_text.clone();
                        q.line_start = anchor.line_start;
                        q.line_content = anchor.line_content.clone();
                        q.context_before = anchor.context_before.clone();
                        q.context_after = anchor.context_after.clone();
                        q.old_line_start = anchor.old_line_start;
                        q.hunk_header = anchor.hunk_header;
                        q.hunk_index = Some(hunk_index);
                        q.anchor_status = "original".to_string();
                        q.relocated_at_hash = diff_hash;
                        q.stale = false;
                    }
                    let json = serde_json::to_string_pretty(&qs)?;
                    let tmp = format!("{}.tmp", path);
                    std::fs::write(&tmp, json)?;
                    std::fs::rename(&tmp, &path)?;
                }
            }
        } else if comment_id.starts_with("n-") {
            let path = format!("{}/notes.json", er_dir);
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(mut ns) = serde_json::from_str::<ai::ErNotes>(&content) {
                    if let Some(n) = ns.notes.iter_mut().find(|n| n.id == comment_id) {
                        n.text = new_text.clone();
                        n.line_start = anchor.line_start;
                        n.line_content = anchor.line_content.clone();
                        n.context_before = anchor.context_before.clone();
                        n.context_after = anchor.context_after.clone();
                        n.old_line_start = anchor.old_line_start;
                        n.hunk_header = anchor.hunk_header;
                        n.hunk_index = Some(hunk_index);
                        n.anchor_status = "original".to_string();
                        n.relocated_at_hash = diff_hash;
                        n.stale = false;
                    }
                    let json = serde_json::to_string_pretty(&ns)?;
                    let tmp = format!("{}.tmp", path);
                    std::fs::write(&tmp, json)?;
                    std::fs::rename(&tmp, &path)?;
                }
            }
        } else {
            let path = self.tab().github_comments_path();
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(mut gc) = serde_json::from_str::<ai::ErGitHubComments>(&content) {
                    if let Some(c) = gc.comments.iter_mut().find(|c| c.id == comment_id) {
                        c.comment = new_text.clone();
                        c.line_start = anchor.line_start;
                        c.line_content = anchor.line_content.clone();
                        c.context_before = anchor.context_before.clone();
                        c.context_after = anchor.context_after.clone();
                        c.old_line_start = anchor.old_line_start;
                        c.hunk_header = anchor.hunk_header;
                        c.hunk_index = Some(hunk_index);
                        c.anchor_status = "original".to_string();
                        c.relocated_at_hash = diff_hash;
                        c.stale = false;
                    }
                    let json = serde_json::to_string_pretty(&gc)?;
                    let tmp = format!("{}.tmp", path);
                    std::fs::write(&tmp, json)?;
                    std::fs::rename(&tmp, &path)?;
                }
            }
        }

        self.tab_mut().comment_textarea = TextArea::default();
        self.tab_mut().comment_edit_id = None;
        self.input_mode = InputMode::Normal;
        self.tab_mut().reload_ai_state();
        self.notify(&format!("Comment updated: {}", truncate(&new_text, 40)));
        Ok(())
    }

    /// Jump to the next comment across all files.
    pub fn next_comment(&mut self) {
        self.jump_comment(true, false);
    }

    /// Jump to the previous comment across all files.
    pub fn prev_comment(&mut self) {
        self.jump_comment(false, false);
    }

    /// Core jump logic: navigate forward/backward through comments or questions across all files.
    /// Uses focused_comment_id for exact position tracking instead of file+hunk guessing.
    fn jump_comment(&mut self, forward: bool, questions_only: bool) {
        let tab = self.tab_mut();
        let all = if questions_only {
            // Convert 3-tuple to 4-tuple for uniform handling
            tab.ai
                .all_questions_ordered()
                .into_iter()
                .map(|(f, h, id)| (f, h, None::<usize>, id))
                .collect::<Vec<_>>()
        } else {
            tab.ai.all_comments_ordered()
        };

        if all.is_empty() {
            return;
        }

        // Find current position by exact ID match first, then fallback to file position
        let current_pos = tab
            .focused_comment_id
            .as_ref()
            .and_then(|fid| all.iter().position(|(_, _, _, id)| id == fid))
            .or_else(|| {
                let current_file = tab.files.get(tab.selected_file).map(|f| &f.path);
                current_file.and_then(|cf| {
                    if forward {
                        all.iter().position(|(f, _, _, _)| f == cf)
                    } else {
                        all.iter().rposition(|(f, _, _, _)| f == cf)
                    }
                })
            });

        let next_idx = match current_pos {
            Some(pos) => {
                if forward {
                    if pos + 1 < all.len() {
                        pos + 1
                    } else {
                        0
                    }
                } else if pos > 0 {
                    pos - 1
                } else {
                    all.len() - 1
                }
            }
            None => {
                if forward {
                    0
                } else {
                    all.len() - 1
                }
            }
        };

        let (ref file, hunk_index, _, ref comment_id) = all[next_idx];

        tab.focused_comment_id = Some(comment_id.clone());
        tab.focused_finding_id = None;

        let needs_file_change = tab
            .files
            .get(tab.selected_file)
            .is_none_or(|f| f.path != *file);

        if needs_file_change {
            if let Some(idx) = tab.files.iter().position(|f| f.path == *file) {
                tab.selected_file = idx;
                tab.current_hunk = hunk_index.unwrap_or(0);
                tab.current_line = None;
                tab.selection_anchor = None;
                tab.diff_scroll = 0;
                tab.h_scroll = 0;
                tab.ensure_file_parsed();
                tab.rebuild_hunk_offsets();
            }
        } else if let Some(hi) = hunk_index {
            tab.current_hunk = hi;
            tab.current_line = None;
        }

        tab.scroll_to_current_hunk();
    }

    /// Jump forward to the next AI finding.
    pub fn next_finding(&mut self) {
        self.jump_finding(true);
    }

    /// Jump backward to the previous AI finding.
    pub fn prev_finding(&mut self) {
        self.jump_finding(false);
    }

    /// Navigate to the next/prev finding within the current file's panel list.
    /// Uses the same sort order as the FileDetail panel renderer.
    pub fn navigate_panel_finding(&mut self, forward: bool) {
        use crate::ai::RiskLevel;
        let tab = self.tab_mut();
        let path = match tab.files.get(tab.selected_file) {
            Some(f) => f.path.clone(),
            None => return,
        };
        let fr = match tab.ai.file_review(&path) {
            Some(fr) if !fr.findings.is_empty() => fr,
            _ => return,
        };
        let sev_ord = |r: &RiskLevel| match r {
            RiskLevel::High => 0,
            RiskLevel::Medium => 1,
            RiskLevel::Low => 2,
            RiskLevel::Info => 3,
        };
        let conf_ord = |c: &crate::ai::Confidence| match c {
            crate::ai::Confidence::Confirmed => 0,
            crate::ai::Confidence::Tentative => 1,
            crate::ai::Confidence::Informational => 2,
            crate::ai::Confidence::Dropped => 3,
        };
        let mut sorted: Vec<&crate::ai::Finding> = fr.findings.iter().collect();
        sorted.sort_by(|a, b| {
            conf_ord(&a.confidence)
                .cmp(&conf_ord(&b.confidence))
                .then_with(|| a.hunk_index.cmp(&b.hunk_index))
                .then_with(|| a.line_start.cmp(&b.line_start))
                .then_with(|| sev_ord(&a.severity).cmp(&sev_ord(&b.severity)))
        });

        let current = tab
            .focused_finding_id
            .as_ref()
            .and_then(|id| sorted.iter().position(|f| &f.id == id));

        let next_idx = match current {
            Some(i) => {
                if forward {
                    (i + 1) % sorted.len()
                } else {
                    i.checked_sub(1).unwrap_or(sorted.len() - 1)
                }
            }
            None => {
                if forward {
                    0
                } else {
                    sorted.len() - 1
                }
            }
        };
        tab.focused_finding_id = Some(sorted[next_idx].id.clone());
    }

    /// Jump the diff view to the inline location of the currently focused finding.
    /// Scoped to the current file — does not change selected_file.
    /// Unfocuses the panel. Notifies if the finding is outside the diff.
    pub fn jump_to_focused_finding(&mut self) {
        let fid = match self.tab().focused_finding_id.clone() {
            Some(id) => id,
            None => return,
        };
        let path = match self.tab().files.get(self.tab().selected_file) {
            Some(f) => f.path.clone(),
            None => return,
        };
        let (hunk_index, line_start) = {
            let tab = self.tab();
            let fr = match tab.ai.file_review(&path) {
                Some(fr) => fr,
                None => return,
            };
            match fr.findings.iter().find(|f| f.id == fid) {
                Some(f) => (f.hunk_index, f.line_start),
                None => return,
            }
        };

        // Validate the hunk and line exist in the parsed diff.
        let hunk_ok = hunk_index.is_none_or(|h| {
            self.tab()
                .files
                .get(self.tab().selected_file)
                .is_some_and(|f| h < f.hunks.len())
        });
        let line_ok = line_start.is_none_or(|ls| {
            self.tab()
                .files
                .get(self.tab().selected_file)
                .is_some_and(|f| {
                    f.hunks
                        .iter()
                        .any(|hunk| hunk.lines.iter().any(|l| l.new_num == Some(ls)))
                })
        });

        if !hunk_ok || !line_ok {
            let ln = line_start
                .map(|l| l.to_string())
                .unwrap_or_else(|| "?".into());
            self.tab_mut().panel_focus = false;
            self.notify(&format!(
                "Line {} is outside the diff — open in editor to view",
                ln
            ));
            return;
        }

        let tab = self.tab_mut();
        if let Some(hi) = hunk_index {
            tab.current_hunk = hi;
            tab.current_line = None;
        }
        let hi = hunk_index.unwrap_or(0);
        if let Some(hunk) = tab
            .files
            .get(tab.selected_file)
            .and_then(|f| f.hunks.get(hi))
        {
            tab.current_line = if let Some(ls) = line_start {
                hunk.lines.iter().position(|l| l.new_num == Some(ls))
            } else {
                Some(hunk.lines.len().saturating_sub(1))
            };
        }
        tab.panel_focus = false;
        tab.scroll_to_current_hunk();
    }

    /// Core jump logic: navigate forward/backward through AI findings across all files.
    /// Uses focused_finding_id for exact position tracking.
    fn jump_finding(&mut self, forward: bool) {
        let tab = self.tab_mut();
        let file_paths: std::collections::HashSet<&str> =
            tab.files.iter().map(|f| f.path.as_str()).collect();
        let all: Vec<_> = tab
            .ai
            .all_findings_ordered()
            .into_iter()
            .filter(|(file, _, _, _)| file_paths.contains(file.as_str()))
            .collect();

        if all.is_empty() {
            return;
        }

        // Find current position by exact ID match first, then fallback to file position
        let current_pos = tab
            .focused_finding_id
            .as_ref()
            .and_then(|fid| all.iter().position(|(_, _, _, id)| id == fid))
            .filter(|&pos| {
                // Ignore stale focused ID if user moved to a different file
                tab.files
                    .get(tab.selected_file)
                    .is_some_and(|f| f.path == all[pos].0)
            })
            .or_else(|| {
                let current_file = tab.files.get(tab.selected_file).map(|f| f.path.as_str());
                let current_hunk = tab.current_hunk;
                current_file.and_then(|cf| {
                    if forward {
                        // Find first finding at or after current position
                        all.iter().position(|(f, hi, _, _)| {
                            f.as_str() > cf || (f == cf && hi.unwrap_or(0) >= current_hunk)
                        })
                    } else {
                        // Find last finding at or before current position
                        all.iter().rposition(|(f, hi, _, _)| {
                            f.as_str() < cf || (f == cf && hi.unwrap_or(0) <= current_hunk)
                        })
                    }
                })
            });

        let next_idx = match current_pos {
            Some(pos) => {
                if forward {
                    if pos + 1 < all.len() {
                        pos + 1
                    } else {
                        0
                    }
                } else if pos > 0 {
                    pos - 1
                } else {
                    all.len() - 1
                }
            }
            None => {
                if forward {
                    0
                } else {
                    all.len() - 1
                }
            }
        };

        let (ref file, hunk_index, line_start, ref finding_id) = all[next_idx];

        tab.focused_finding_id = Some(finding_id.clone());
        tab.focused_comment_id = None;

        let needs_file_change = tab
            .files
            .get(tab.selected_file)
            .is_none_or(|f| f.path != *file);

        if needs_file_change {
            if let Some(idx) = tab.files.iter().position(|f| f.path == *file) {
                tab.selected_file = idx;
                tab.current_hunk = hunk_index.unwrap_or(0);
                tab.current_line = None;
                tab.selection_anchor = None;
                tab.diff_scroll = 0;
                tab.h_scroll = 0;
                tab.ensure_file_parsed();
                tab.rebuild_hunk_offsets();
            }
        } else if let Some(hi) = hunk_index {
            tab.current_hunk = hi;
            tab.current_line = None;
        }

        // Compute current_line from finding's line_start for precise scroll positioning
        let hi = hunk_index.unwrap_or(0);
        if let Some(diff_file) = tab.files.get(tab.selected_file) {
            if let Some(hunk) = diff_file.hunks.get(hi) {
                if let Some(ls) = line_start {
                    // Line-level finding: scroll to the specific line within the hunk
                    if let Some(line_idx) = hunk.lines.iter().position(|l| l.new_num == Some(ls)) {
                        tab.current_line = Some(line_idx);
                    }
                } else {
                    // Hunk-level finding: renders at end of hunk, scroll near the end
                    tab.current_line = Some(hunk.lines.len().saturating_sub(1));
                }
            }
        }

        tab.scroll_to_current_hunk();
    }

    /// Jump to the next comment/question (Shift+J). Excludes findings.
    pub fn next_hint(&mut self) {
        self.jump_hint(true);
    }

    /// Jump to the previous comment/question (Shift+K). Excludes findings.
    pub fn prev_hint(&mut self) {
        self.jump_hint(false);
    }

    /// Navigation across comments and questions only (excludes findings).
    fn jump_hint(&mut self, forward: bool) {
        use crate::ai::HintType;

        let tab = self.tab_mut();
        let all: Vec<_> = tab
            .ai
            .all_hints_ordered()
            .into_iter()
            .filter(|(_, _, _, _, ht)| *ht != HintType::Finding)
            .collect();

        if all.is_empty() {
            return;
        }

        // Find current position by matching the currently focused ID
        let current_id = tab
            .focused_comment_id
            .as_ref()
            .or(tab.focused_finding_id.as_ref());
        let current_pos = current_id
            .and_then(|fid| all.iter().position(|(_, _, _, id, _)| id == fid))
            .or_else(|| {
                let current_file = tab.files.get(tab.selected_file).map(|f| &f.path);
                current_file.and_then(|cf| {
                    if forward {
                        all.iter().position(|(f, _, _, _, _)| f == cf)
                    } else {
                        all.iter().rposition(|(f, _, _, _, _)| f == cf)
                    }
                })
            });

        let next_idx = match current_pos {
            Some(pos) => {
                if forward {
                    if pos + 1 < all.len() {
                        pos + 1
                    } else {
                        0
                    }
                } else if pos > 0 {
                    pos - 1
                } else {
                    all.len() - 1
                }
            }
            None => {
                if forward {
                    0
                } else {
                    all.len() - 1
                }
            }
        };

        let (ref file, hunk_index, _, ref id, hint_type) = all[next_idx];

        // Set the appropriate focus ID based on hint type
        match hint_type {
            HintType::Question | HintType::Note | HintType::GitHubComment => {
                tab.focused_comment_id = Some(id.clone());
                tab.focused_finding_id = None;
            }
            HintType::Finding => {
                tab.focused_finding_id = Some(id.clone());
                tab.focused_comment_id = None;
            }
        }

        let needs_file_change = tab
            .files
            .get(tab.selected_file)
            .is_none_or(|f| f.path != *file);

        if needs_file_change {
            if let Some(idx) = tab.files.iter().position(|f| f.path == *file) {
                tab.selected_file = idx;
                tab.current_hunk = hunk_index.unwrap_or(0);
                tab.current_line = None;
                tab.selection_anchor = None;
                tab.diff_scroll = 0;
                tab.h_scroll = 0;
                tab.ensure_file_parsed();
                tab.rebuild_hunk_offsets();
            }
        } else if let Some(hi) = hunk_index {
            tab.current_hunk = hi;
            tab.current_line = None;
        }

        tab.scroll_to_current_hunk();
    }

    /// Keep the in-memory questions in step with a sidecar we just rewrote.
    ///
    /// Adopting the written list avoids `reload_ai_state()`, which re-reads
    /// every sidecar (review.json, experts, tour, …) to learn about a change
    /// this process just made. That is what keeps `submit_github_comment` fast;
    /// delete/edit/resolve follow the same path.
    fn adopt_questions(&mut self, qs: ai::ErQuestions, path: &str) {
        let tab = self.tab_mut();
        tab.ai.questions = Some(qs);
        tab.ai.rebuild_comment_index();
        tab.mark_sidecar_written(path);
    }

    fn adopt_notes(&mut self, ns: ai::ErNotes, path: &str) {
        let tab = self.tab_mut();
        tab.ai.notes = Some(ns);
        tab.ai.rebuild_comment_index();
        tab.mark_sidecar_written(path);
    }

    fn adopt_github_comments(&mut self, gc: ai::ErGitHubComments, path: &str) {
        let tab = self.tab_mut();
        tab.ai.github_comments = Some(gc);
        tab.ai.rebuild_comment_index();
        tab.mark_sidecar_written(path);
    }

    /// Execute comment deletion after confirmation
    pub fn confirm_delete_comment(&mut self, comment_id: &str) -> Result<()> {
        let er_dir = self.tab().er_dir();
        let repo_root = self.tab().repo_root.clone();

        // Determine which file this comment lives in (by id prefix)
        if comment_id.starts_with("q-") {
            // Delete from questions.json
            let path = format!("{}/questions.json", er_dir);
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(mut qs) = serde_json::from_str::<ai::ErQuestions>(&content) {
                    qs.questions.retain(|q| {
                        q.id != comment_id && q.in_reply_to.as_deref() != Some(comment_id)
                    });
                    let json = serde_json::to_string_pretty(&qs)?;
                    let tmp_path = format!("{}.tmp", path);
                    std::fs::write(&tmp_path, &json)?;
                    std::fs::rename(&tmp_path, &path)?;
                    self.adopt_questions(qs, &path);
                }
            }
        } else if comment_id.starts_with("n-") {
            // Delete from notes.json (cascade replies)
            let path = format!("{}/notes.json", er_dir);
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(mut ns) = serde_json::from_str::<ai::ErNotes>(&content) {
                    ns.notes.retain(|n| {
                        n.id != comment_id && n.in_reply_to.as_deref() != Some(comment_id)
                    });
                    let json = serde_json::to_string_pretty(&ns)?;
                    let tmp_path = format!("{}.tmp", path);
                    std::fs::write(&tmp_path, &json)?;
                    std::fs::rename(&tmp_path, &path)?;
                    self.adopt_notes(ns, &path);
                }
            }
        } else {
            // Delete from github-comments.json (uses cache dir in remote mode)
            let path = self.tab().github_comments_path();
            if let Some(mut gc) = std::fs::read_to_string(&path)
                .ok()
                .and_then(|content| serde_json::from_str::<ai::ErGitHubComments>(&content).ok())
            {
                // Check if the comment has a github_id for API deletion
                let github_id = gc
                    .comments
                    .iter()
                    .find(|c| c.id == comment_id)
                    .and_then(|c| c.github_id);

                let reply_github_ids: Vec<u64> = gc
                    .comments
                    .iter()
                    .filter(|c| {
                        c.in_reply_to.as_deref() == Some(comment_id) && c.github_id.is_some()
                    })
                    .filter_map(|c| c.github_id)
                    .collect();

                // Remove comment and cascade replies
                gc.comments
                    .retain(|c| c.id != comment_id && c.in_reply_to.as_deref() != Some(comment_id));

                let json = serde_json::to_string_pretty(&gc)?;
                let tmp_path = format!("{}.tmp", path);
                std::fs::write(&tmp_path, &json)?;
                std::fs::rename(&tmp_path, &path)?;

                // Delete from GitHub off this thread. The results were already
                // discarded (`let _ =`), and running `gh` inline held the
                // desktop's app lock for the whole network round trip, so every
                // poll and click queued behind a comment deletion.
                if let (Some(gh_id), Some(gh)) = (github_id, gc.github.clone()) {
                    let ids = std::iter::once(gh_id).chain(reply_github_ids).collect();
                    spawn_github_comment_deletes(gh, ids, repo_root);
                }

                self.adopt_github_comments(gc, &path);
            }
        }

        self.input_mode = InputMode::Normal;
        self.notify("Comment deleted");
        Ok(())
    }

    /// Cancel the confirm dialog
    pub fn cancel_confirm(&mut self) {
        self.input_mode = InputMode::Normal;
        self.clear_ai_selection_override();
    }

    // ── Hunk Comment (Shift-C) ──

    // ── Commit ──

    /// Start commit input (only in Staged mode)
    pub fn start_commit(&mut self) {
        self.tab_mut().commit_input.clear();
        self.input_mode = InputMode::Commit;
    }

    /// Run git commit with the typed message
    pub fn submit_commit(&mut self) -> Result<()> {
        let message = self.tab().commit_input.trim().to_string();
        if message.is_empty() {
            self.input_mode = InputMode::Normal;
            return Ok(());
        }
        let repo_root = self.tab().repo_root.clone();
        git::git_commit(&repo_root, &message)?;
        self.tab_mut().commit_input.clear();
        self.input_mode = InputMode::Normal;
        self.tab_mut().committed_unpushed = true;
        let _ = self.tab_mut().refresh_diff();
        self.notify("Committed! Ctrl+P to push");
        Ok(())
    }

    /// Cancel commit input
    pub fn cancel_commit(&mut self) {
        self.tab_mut().commit_input.clear();
        self.input_mode = InputMode::Normal;
    }

    // ── AiReview Navigation ──

    /// Jump from AiSummary panel to the selected file in FileDetail mode
    pub fn review_jump_to_file(&mut self) {
        let file_path = {
            let tab = self.tab();
            match tab.review_focus {
                ReviewFocus::Files => tab.ai.review_file_at(tab.review_cursor),
                ReviewFocus::Checklist => tab.ai.checklist_file_at(tab.review_cursor),
            }
        };

        if let Some(path) = file_path {
            let file_idx = self.tab().files.iter().position(|f| f.path == path);
            if let Some(idx) = file_idx {
                // Collect first anchored finding before taking mutable borrow
                let first_finding = self
                    .tab()
                    .ai
                    .file_review(&path)
                    .and_then(|fr| {
                        fr.findings
                            .iter()
                            .filter_map(|f| Some((f.hunk_index?, f)))
                            .min_by_key(|(hunk, f)| (*hunk, f.line_start))
                    })
                    .map(|(hunk, f)| (hunk, f.id.clone()));

                let tab = self.tab_mut();
                tab.selected_file = idx;
                tab.current_hunk = first_finding.as_ref().map(|(hi, _)| *hi).unwrap_or(0);
                tab.focused_finding_id = first_finding.map(|(_, id)| id);
                tab.current_line = None;
                tab.diff_scroll = 0;
                tab.h_scroll = 0;
                tab.ensure_file_parsed();
                tab.rebuild_hunk_offsets();
                tab.scroll_to_current_hunk();
                if tab.panel.is_none() {
                    tab.panel = Some(PanelContent::FileDetail);
                }
                self.notify(&format!("Jumped to: {}", path));
            } else {
                self.notify(&format!("File not in diff: {}", path));
            }
        } else {
            self.notify("No file associated with this item");
        }
    }

    /// Toggle the checklist item under the review cursor.
    ///
    /// The focus guard and the cursor are TUI navigation state, so they stay
    /// here; the write itself is addressed by index, which is what the desktop
    /// can send.
    pub fn review_toggle_checklist(&mut self) -> Result<()> {
        if self.tab().review_focus != ReviewFocus::Checklist {
            return Ok(());
        }
        let cursor = self.tab().review_cursor;

        match self.toggle_checklist_item_at(cursor)? {
            Some(true) => self.notify("✓ Item checked"),
            Some(false) => self.notify("○ Item unchecked"),
            None => {}
        }
        Ok(())
    }

    /// Toggle the checklist item at `index`, persist it, and report its new
    /// state. `None` means the index names no item, so nothing was written or
    /// toggled — the checklist may not exist yet, or the generator may have
    /// rewritten it since the caller last read it.
    ///
    /// Addressed by index rather than by cursor or focus: the checklist is one
    /// JSON document per view bucket, and a write path only one front end used
    /// would let the two disagree on disk.
    pub fn toggle_checklist_item_at(&mut self, index: usize) -> Result<Option<bool>> {
        let tab = self.tab_mut();
        if tab
            .ai
            .checklist
            .as_ref()
            .is_none_or(|c| c.items.get(index).is_none())
        {
            return Ok(None);
        }
        tab.ai.toggle_checklist_item(index);

        // Persist atomically via temp file + rename, so a reader never sees a
        // half-written document.
        if let Some(checklist) = tab.ai.checklist.as_ref() {
            let checklist_path = format!("{}/checklist.json", tab.er_dir());
            let tmp_path = format!("{}.tmp", checklist_path);
            let json = serde_json::to_string_pretty(checklist)?;
            std::fs::write(&tmp_path, json)?;
            std::fs::rename(&tmp_path, &checklist_path)?;
            tab.mark_sidecar_written(&checklist_path);
        }

        Ok(tab
            .ai
            .checklist
            .as_ref()
            .and_then(|c| c.items.get(index))
            .map(|i| i.checked))
    }

    // ── Clipboard ──

    /// Copy the current hunk to the system clipboard
    pub fn copy_review_json(&mut self) -> Result<()> {
        let er_dir = self.tab().er_dir();
        let path = std::path::Path::new(&er_dir).join("review.json");
        if !path.exists() {
            self.notify("No review.json found");
            return Ok(());
        }
        let content = std::fs::read_to_string(&path).context("Failed to read review.json")?;
        let bytes = content.len();
        Self::copy_to_clipboard(&content)?;
        self.notify(&format!("Copied review.json ({} bytes)", bytes));
        Ok(())
    }

    /// Copy the current questions.json to the system clipboard
    pub fn copy_questions_json(&mut self) -> Result<()> {
        let er_dir = self.tab().er_dir();
        let path = std::path::Path::new(&er_dir).join("questions.json");
        if !path.exists() {
            self.notify("No questions.json found");
            return Ok(());
        }
        let content = std::fs::read_to_string(&path).context("Failed to read questions.json")?;
        let bytes = content.len();
        Self::copy_to_clipboard(&content)?;
        self.notify(&format!("Copied questions.json ({} bytes)", bytes));
        Ok(())
    }

    pub fn yank_hunk(&mut self) -> Result<()> {
        let si = self.tab().selected_file;
        let hi = self.tab().current_hunk;

        if si >= self.tab().files.len() {
            self.notify("No file selected");
            return Ok(());
        }
        if hi >= self.tab().files[si].hunks.len() {
            self.notify("No hunk selected");
            return Ok(());
        }

        let text = self.tab().files[si].hunks[hi].to_text();
        Self::copy_to_clipboard(&text)?;
        self.notify("Hunk copied to clipboard");
        Ok(())
    }

    /// Copy all hunks for the selected file in unified diff format
    pub fn copy_full_file(&mut self) -> Result<()> {
        let tab = self.tab();
        if let Some(file) = tab.selected_diff_file() {
            let mut text = format!("--- a/{}\n+++ b/{}\n", file.path, file.path);
            for hunk in &file.hunks {
                text.push_str(&hunk.to_text());
                text.push('\n');
            }
            let count = file.hunks.len();
            Self::copy_to_clipboard(&text)?;
            self.notify(&format!("Copied full file diff ({} hunks)", count));
        } else {
            self.notify("No file selected");
        }
        Ok(())
    }

    /// Copy the selected file's path to clipboard
    pub fn copy_file_path(&mut self) -> Result<()> {
        let tab = self.tab();
        if let Some(file) = tab.selected_diff_file() {
            let path = file.path.clone();
            Self::copy_to_clipboard(&path)?;
            self.notify(&format!("Copied: {}", path));
        } else {
            self.notify("No file selected");
        }
        Ok(())
    }

    /// Copy the current line's content to clipboard (requires line-level navigation)
    pub fn copy_line(&mut self) -> Result<()> {
        let tab = self.tab();
        if let Some(file) = tab.selected_diff_file() {
            if let Some(line_idx) = tab.current_line {
                if let Some(hunk) = file.hunks.get(tab.current_hunk) {
                    if let Some(line) = hunk.lines.get(line_idx) {
                        let content = line.content.clone();
                        Self::copy_to_clipboard(&content)?;
                        self.notify("Line copied to clipboard");
                        return Ok(());
                    }
                }
            }
            self.notify("No line selected — use arrow keys to enter line navigation");
        } else {
            self.notify("No file selected");
        }
        Ok(())
    }

    /// Copy rich context to clipboard for pasting into an agent terminal.
    ///
    /// What gets copied depends on navigation state:
    /// - Selection active (shift+arrow): selected lines only
    /// - Line-level nav (arrow keys): current line only
    /// - Hunk-level nav (n/N keys): full hunk
    pub fn copy_context(&mut self) -> Result<()> {
        let tab = self.tab();
        let file = match tab.selected_diff_file() {
            Some(f) => f,
            None => {
                self.notify("No file selected");
                return Ok(());
            }
        };
        let hunk = match file.hunks.get(tab.current_hunk) {
            Some(h) => h,
            None => {
                self.notify("No hunk selected");
                return Ok(());
            }
        };

        let mut text = String::new();

        // Header
        text.push_str(&format!("File: {}\n", file.path));
        text.push_str(&format!(
            "Branch: {} (vs {})\n",
            tab.current_branch, tab.base_branch
        ));

        // Determine what to copy based on navigation state
        let (lines_to_copy, line_label) = if let Some(range) = tab.selected_range() {
            // Shift+arrow selection: copy selected lines
            let selected: Vec<_> = hunk
                .lines
                .iter()
                .enumerate()
                .filter(|(i, _)| range.contains(i))
                .map(|(_, l)| l)
                .collect();
            let start = selected.first().and_then(|l| l.new_num).unwrap_or(0);
            let end = selected.last().and_then(|l| l.new_num).unwrap_or(0);
            let label = if start == end {
                format!("Line {}", start)
            } else {
                format!("Lines {}-{}", start, end)
            };
            (selected, label)
        } else if let Some(line_idx) = tab.current_line {
            // Line-level navigation: copy current line only
            if let Some(line) = hunk.lines.get(line_idx) {
                let ln = line.new_num.unwrap_or(0);
                (vec![line], format!("Line {}", ln))
            } else {
                let all: Vec<_> = hunk.lines.iter().collect();
                (all, format!("Hunk #{}", tab.current_hunk + 1))
            }
        } else {
            // Hunk-level navigation: copy full hunk
            let all: Vec<_> = hunk.lines.iter().collect();
            (all, format!("Hunk #{}", tab.current_hunk + 1))
        };

        text.push_str(&format!("{}:\n\n", line_label));

        // Hunk header
        text.push_str(&format!(" {}\n", hunk.header));

        // Diff lines
        for line in &lines_to_copy {
            let prefix = match line.line_type {
                crate::git::LineType::Add => "+",
                crate::git::LineType::Delete => "-",
                crate::git::LineType::Context => " ",
                crate::git::LineType::Fold(_) => continue,
            };
            text.push_str(&format!("{}{}\n", prefix, line.content));
        }

        // AI finding if present
        let findings =
            tab.ai
                .findings_for_hunk(&file.path, tab.current_hunk, file.hunks.len(), &tab.layers);
        if let Some(finding) = findings.first() {
            text.push_str(&format!(
                "\nFinding: [{:?}] {}\n",
                finding.severity, finding.title
            ));
            if !finding.suggestion.is_empty() {
                text.push_str(&format!("Suggestion: {}\n", finding.suggestion));
            }
        }

        let line_count = lines_to_copy.len();
        let scope = if tab.selected_range().is_some() {
            "selection"
        } else if tab.current_line.is_some() {
            "line"
        } else {
            "hunk"
        };
        Self::copy_to_clipboard(&text)?;
        self.notify(&format!("Copied {} ({} lines)", scope, line_count));
        Ok(())
    }

    pub(super) fn copy_to_clipboard(text: &str) -> Result<()> {
        let (cmd, args): (&str, Vec<&str>) = if cfg!(target_os = "macos") {
            ("pbcopy", vec![])
        } else if cfg!(target_os = "windows") {
            ("clip", vec![])
        } else {
            // Linux — try xclip, fall back to xsel
            if std::process::Command::new("which")
                .arg("xclip")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
            {
                ("xclip", vec!["-selection", "clipboard"])
            } else {
                ("xsel", vec!["--clipboard", "--input"])
            }
        };

        let mut child = std::process::Command::new(cmd)
            .args(&args)
            .stdin(std::process::Stdio::piped())
            .spawn()
            .context("Failed to open clipboard command")?;

        if let Some(ref mut stdin) = child.stdin {
            stdin.write_all(text.as_bytes())?;
        }

        child.wait().context("Clipboard command failed")?;
        Ok(())
    }

    // ── Notifications ──

    pub fn notify(&mut self, msg: &str) {
        self.set_notification(msg, false);
    }

    /// Like [`Self::notify`], flagged so a UI can leave it up longer.
    pub fn notify_long(&mut self, msg: &str) {
        self.set_notification(msg, true);
    }

    /// Store `msg` as the current notification, stamping it with a fresh seq.
    ///
    /// `seq` comes from a process-wide counter rather than from what's already
    /// stored, so a repeat of the same text still counts as a new message and
    /// clearing the field does not rewind the numbering.
    fn set_notification(&mut self, msg: &str, long: bool) {
        self.notification = Some(Notification {
            message: msg.to_string(),
            seq: NOTIFICATION_SEQ.fetch_add(1, Ordering::Relaxed) + 1,
            long,
        });
    }

    /// Drop the current message.
    ///
    /// The TUI calls this when its dwell timer expires. The desktop never does:
    /// its snapshots only arrive when the revision counter moves, so it dedupes
    /// on [`Notification::seq`] and lets the message sit in the snapshot.
    pub fn clear_notification(&mut self) {
        self.notification = None;
    }

    // ── Background Commands ──

    /// Build a human-readable summary after an agent command completes.
    /// Reads the output files to report what was produced.
    fn agent_completion_summary_for(tab: &TabState, name: &str) -> String {
        let er_dir = std::path::PathBuf::from(tab.er_dir());

        match name {
            "review" => {
                let review_path = er_dir.join("review.json");
                if let Ok(content) = std::fs::read_to_string(&review_path) {
                    if let Ok(review) = serde_json::from_str::<ai::ErReview>(&content) {
                        let file_count = review.files.len();
                        let finding_count: usize =
                            review.files.values().map(|f| f.findings.len()).sum();
                        format!(
                            "Review done — {} file{}, {} finding{}",
                            file_count,
                            if file_count == 1 { "" } else { "s" },
                            finding_count,
                            if finding_count == 1 { "" } else { "s" },
                        )
                    } else {
                        "Review done — review.json written but could not be parsed".into()
                    }
                } else {
                    "Review done — but no review.json found (agent may lack permissions)".into()
                }
            }
            "questions" => {
                let questions_path = er_dir.join("questions.json");
                if let Ok(content) = std::fs::read_to_string(&questions_path) {
                    if let Ok(qs) = serde_json::from_str::<ai::ErQuestions>(&content) {
                        let answered = qs
                            .questions
                            .iter()
                            .filter(|q| q.in_reply_to.is_some())
                            .count();
                        let total = qs
                            .questions
                            .iter()
                            .filter(|q| q.in_reply_to.is_none())
                            .count();
                        format!("Questions done — {} of {} answered", answered, total)
                    } else {
                        "Questions done — questions.json written but could not be parsed".into()
                    }
                } else {
                    "Questions done — but no questions.json found".into()
                }
            }
            _ => format!("{} done", name),
        }
    }

    fn agent_command_writes_ai_artifacts(name: &str) -> bool {
        matches!(
            name,
            "summary"
                | "review"
                | "questions"
                | "triage"
                | "professor"
                | "validate"
                | "validate-comments"
        ) || name.starts_with("expert-")
    }

    /// Spawn a shell command in the background under the given name.
    /// The command string is run via `sh -c` in the repo root.
    /// Placeholders {base}, {branch}, {repo}, {output} are substituted.
    #[expect(
        clippy::literal_string_with_formatting_args,
        reason = "{base}/{branch}/{repo}/{output} are template placeholders, not format args"
    )]
    pub fn spawn_command(&mut self, name: &str, shell_cmd: &str) -> Result<()> {
        if self.tab().command_status.get(name) == Some(&CommandStatus::Running) {
            self.notify(&format!("{} already running", name));
            return Ok(());
        }

        let tab = self.tab();
        let repo_root = tab.repo_root.clone();
        let base = tab.base_branch.clone();
        let branch = tab.current_branch.clone();
        let er_dir = tab.er_dir();
        let output_path = format!("{}/summary.md", er_dir);

        // Substitute placeholders — sanitize values for safe shell interpolation
        let cmd = shell_cmd
            .replace("{base}", &crate::ai::prompts::sanitize_for_shell(&base))
            .replace("{branch}", &crate::ai::prompts::sanitize_for_shell(&branch))
            .replace(
                "{repo}",
                &crate::ai::prompts::sanitize_for_shell(&repo_root),
            )
            .replace(
                "{output}",
                &crate::ai::prompts::sanitize_for_shell(&output_path),
            );

        std::fs::create_dir_all(&er_dir)?;

        let push_to_pr = name == "summary" && self.config.summary.push_to_pr;
        let name_owned: std::sync::Arc<str> = std::sync::Arc::from(name);

        // Send status log entry before spawning
        let _ = self.tab().log_tx.send(AgentLogEntry {
            timestamp: std::time::Instant::now(),
            command_name: std::sync::Arc::from(name),
            source: AgentLogSource::Status,
            text: format!("{} started", name),
        });

        let log_tx = self.tab().log_tx.clone();
        let agent_timeout = self.config.ai_hub.effective_agent_timeout();
        let slot_cap = self.config.ai_hub.effective_max_concurrent_reviews();
        let ceiling = self.config.ai_hub.effective_max_concurrent_agents();
        let run = crate::agent_run::AgentRunHandle::new();
        let run_for_tab = std::sync::Arc::clone(&run);
        let run_name = name.to_string();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut timer = crate::agent_timing::AgentRunTimer::start();
            let result = (|| -> Result<()> {
                // Sixth spawn path: a configured shell command. The summary
                // agent runs through here. It waits for a slot like every
                // other path -- a cap that covers some spawn sites is not a
                // cap, and this one launches provider CLIs just like the rest.
                let Some(_slot) = crate::agent_slots::acquire(
                    crate::agent_slots::Workload::Background,
                    slot_cap,
                    ceiling,
                    run.cancel_flag(),
                ) else {
                    return Err(crate::agent_run::cancelled());
                };
                timer.mark_slot_acquired();
                let mut cmd_builder = std::process::Command::new("sh");
                cmd_builder
                    .args(["-c", &cmd])
                    .current_dir(&repo_root)
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped());
                let mut child = run
                    .spawn(&mut cmd_builder)
                    .with_context(|| format!("Failed to run {}", name_owned))?;
                let child_id = child.id();
                timer.mark_spawned();

                let stdout = child.stdout.take();
                let stderr = child.stderr.take();
                run.register(child);
                run.arm_timeout(agent_timeout);

                let log_tx_out = log_tx.clone();
                let cmd_name_out = name_owned.clone();
                let stdout_handle = std::thread::spawn(move || {
                    use std::io::BufRead;
                    let pipe_lines = stdout.into_iter().flat_map(|pipe| {
                        std::io::BufReader::new(pipe).lines().map_while(Result::ok)
                    });
                    for line in pipe_lines {
                        let _ = log_tx_out.send(AgentLogEntry {
                            timestamp: std::time::Instant::now(),
                            command_name: cmd_name_out.clone(),
                            source: AgentLogSource::Stdout,
                            text: line,
                        });
                    }
                });

                let log_tx_err = log_tx.clone();
                let cmd_name_err = name_owned.clone();
                let mut stderr_lines: Vec<String> = Vec::new();
                let stderr_handle = std::thread::spawn(move || -> Vec<String> {
                    use std::io::BufRead;
                    let pipe_lines = stderr.into_iter().flat_map(|pipe| {
                        std::io::BufReader::new(pipe).lines().map_while(Result::ok)
                    });
                    for line in pipe_lines {
                        let _ = log_tx_err.send(AgentLogEntry {
                            timestamp: std::time::Instant::now(),
                            command_name: cmd_name_err.clone(),
                            source: AgentLogSource::Stderr,
                            text: line.clone(),
                        });
                        stderr_lines.push(line);
                    }
                    stderr_lines
                });

                let child = run
                    .take_child(child_id)
                    .with_context(|| format!("Lost the handle for {}", name_owned))?;
                let status = run
                    .wait_for(child)
                    .with_context(|| format!("Failed to wait for {}", name_owned))?;
                let _ = stdout_handle.join();
                let accumulated_stderr = stderr_handle.join().unwrap_or_default();
                timer.mark_finished();

                // Timeout before cancel: a run killed by its deadline has both
                // flags set, and "you stopped this" would be the wrong story.
                if run.is_timed_out() {
                    return Err(crate::agent_run::timed_out(agent_timeout));
                }
                if run.is_cancelled() {
                    return Err(crate::agent_run::cancelled());
                }

                if !status.success() {
                    let stderr_text = accumulated_stderr.join("\n");
                    anyhow::bail!("{} failed: {}", name_owned, stderr_text.trim());
                }

                // Summary-specific: optionally push to PR body
                if push_to_pr {
                    let summary_path = std::path::Path::new(&er_dir).join("summary.md");
                    // An unreadable summary is skipped, same as an empty one.
                    let summary = std::fs::read_to_string(&summary_path).unwrap_or_default();
                    if !summary.trim().is_empty() {
                        crate::github::gh_pr_edit_body(&repo_root, &summary)?;
                    }
                }

                Ok(())
            })();
            let ok = result.is_ok();
            let _ = tx.send(result);
            timer.emit("shell_command", &name_owned, ok);
        });

        self.tab_mut().command_rx.insert(name.to_string(), rx);
        self.tab_mut()
            .command_status
            .insert(name.to_string(), CommandStatus::Running);
        self.tab_mut().command_runs.insert(run_name, run_for_tab);
        self.notify(&format!("{} started...", name));
        Ok(())
    }

    /// Drain all pending agent log entries from the channel into `agent_log`.
    /// Called each tick. Auto-scrolls the AgentLog panel when new entries arrive.
    pub fn drain_agent_log(&mut self) {
        /// Most entries one tab may take in a single tick.
        ///
        /// A chatty agent can out-produce the loop, and draining without a
        /// bound means one tick does unbounded work — the frame stretches, the
        /// backlog grows, and the next tick is worse. Whatever is left stays in
        /// the channel for the next tick.
        const MAX_PER_TICK: usize = 500;

        for (i, tab) in self.tabs.iter_mut().enumerate() {
            let mut received = false;
            let mut taken = 0usize;
            while taken < MAX_PER_TICK {
                let Ok(entry) = tab.log_rx.try_recv() else {
                    break;
                };
                tab.agent_log.push_back(entry);
                received = true;
                taken += 1;
                if tab.agent_log.len() > 5000 {
                    tab.agent_log.pop_front();
                }
            }
            if received && i == self.active_tab && tab.agent_log_auto_scroll {
                if let Some(panel) = tab.panel {
                    if panel == crate::ai::PanelContent::AgentLog {
                        tab.panel_scroll =
                            tab.agent_log.len().saturating_sub(1).min(u16::MAX as usize) as u16;
                    }
                }
            }
        }
    }

    /// Poll all running commands for completion (called from event loop).
    pub fn check_commands(&mut self) {
        // Collect completions per tab to avoid borrow conflicts with notify_long
        let mut notifications: Vec<String> = Vec::new();

        for tab in self.tabs.iter_mut() {
            let names: Vec<String> = tab.command_rx.keys().cloned().collect();
            for name in names {
                let result = if let Some(rx) = tab.command_rx.get(&name) {
                    match rx.try_recv() {
                        Ok(ok_or_err) => Some(ok_or_err),
                        Err(std::sync::mpsc::TryRecvError::Empty) => None,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            Some(Err(anyhow::anyhow!("{} thread crashed", name)))
                        }
                    }
                } else {
                    None
                };

                let Some(result) = result else {
                    continue;
                };
                tab.command_rx.remove(&name);
                match result {
                    Ok(()) => {
                        tab.command_status.insert(name.clone(), CommandStatus::Done);
                        let _ = tab.log_tx.send(AgentLogEntry {
                            timestamp: std::time::Instant::now(),
                            command_name: std::sync::Arc::from(name.as_str()),
                            source: AgentLogSource::Status,
                            text: format!("{} completed", name),
                        });
                        // Force AI reload for commands that write sidecar artifacts.
                        // reload_ai_state() resets last_ai_check itself.
                        if Self::agent_command_writes_ai_artifacts(&name) {
                            tab.reload_ai_state();
                        }
                        let msg = Self::agent_completion_summary_for(tab, &name);
                        notifications.push(msg);
                    }
                    Err(e) => {
                        let msg = format!("{}", e);
                        tab.command_status
                            .insert(name.clone(), CommandStatus::Failed(msg.clone()));
                        let _ = tab.log_tx.send(AgentLogEntry {
                            timestamp: std::time::Instant::now(),
                            command_name: std::sync::Arc::from(name.as_str()),
                            source: AgentLogSource::Status,
                            text: format!("{} failed: {}", name, msg),
                        });
                        let short = status_bar_failure(msg);
                        notifications.push(format!("{} failed: {}", name, short));
                    }
                }
            }
        }

        // Apply notifications after the tab iteration loop (avoids borrow conflict)
        for msg in notifications {
            self.notify_long(&msg);
        }
    }
    /// Spawn the configured agent command with a pre-built prompt.
    ///
    /// Uses `agent.command` from config (default: "claude") with `-p` flag
    /// for non-interactive agentic execution. The agent is expected to read
    /// the diff and write `.er/` files directly.
    pub fn spawn_agent_prompt(&mut self, name: &str, prompt: &str) -> Result<()> {
        if self.tab().command_status.get(name) == Some(&CommandStatus::Running) {
            self.notify(&format!("{} already running", name));
            return Ok(());
        }

        let repo_root = self.tab().repo_root.clone();
        let er_dir_path = self.tab().er_dir();
        let is_remote = self.tab().is_remote();
        let remote_repo = self.tab().remote_repo.clone();
        let selection = if let Some(selection) = self.pending_ai_selection_override.clone() {
            selection
        } else {
            self.sync_ai_selection();
            crate::config::AiSelection {
                provider_id: self.current_ai_provider.clone(),
                model_id: self.current_ai_model.clone(),
                effort: self.current_ai_effort.clone(),
            }
        };

        let (
            agent_cmd,
            mut config_args,
            is_claude_compatible,
            is_codex,
            is_stream_json,
            resolved_provider_id,
            resolved_model_id,
            family,
        ) = if let Some(provider_id) = self
            .config
            .ai_hub
            .resolve_provider_id(selection.provider_id.as_deref())
        {
            let provider = self
                .config
                .ai_hub
                .providers
                .get(&provider_id)
                .ok_or_else(|| anyhow::anyhow!("Unknown AI provider: {}", provider_id))?;
            let mut args = provider.args.clone();
            let resolved_model_id = self
                .config
                .ai_hub
                .resolve_model_id(&provider_id, selection.model_id.as_deref());
            let family = provider.cli_family();
            if let Some(model_id) = &resolved_model_id {
                if let Some(model) = provider.models.iter().find(|m| m.id == *model_id) {
                    crate::config::extend_provider_model_args(family, &mut args, &model.args);
                }
            }
            let is_claude = crate::config::agent_command_is_claude(&provider.command);
            let is_codex = crate::config::agent_command_is_codex(&provider.command);
            (
                provider.command.clone(),
                args,
                is_claude,
                is_codex,
                provider.uses_stream_json_log(),
                Some(provider_id),
                resolved_model_id,
                family,
            )
        } else {
            let cmd = self.config.agent.command.clone();
            let is_claude = crate::config::agent_command_is_claude(&cmd);
            let is_codex = crate::config::agent_command_is_codex(&cmd);
            let family = crate::config::CliFamily::detect(&cmd);
            (
                cmd.clone(),
                self.config.agent.args.clone(),
                is_claude,
                is_codex,
                crate::config::agent_command_uses_stream_json(&cmd),
                None,
                (!self.config.agent.model.is_empty()).then(|| self.config.agent.model.clone()),
                family,
            )
        };
        let effort_override = if name == "triage" { Some("low") } else { None };
        let effort = crate::config::resolve_effort_for_model(
            &self.config.ai_hub,
            &self.config.agent,
            resolved_provider_id.as_deref(),
            resolved_model_id.as_deref(),
            selection.effort.as_deref(),
            effort_override,
        );
        crate::config::inject_provider_effort(
            family,
            &mut config_args,
            resolved_model_id.as_deref(),
            effort.as_deref(),
        );
        if is_codex {
            crate::config::inject_codex_ignore_user_config(&mut config_args);
        }
        crate::config::inject_claude_startup_trim(family, &mut config_args);
        crate::config::inject_agent_storage_access(
            family,
            &mut config_args,
            Some(er_dir_path.as_str()),
        );
        let opencode_env = crate::config::apply_opencode_spawn(
            family,
            &mut config_args,
            Some(er_dir_path.as_str()),
        );

        // Ensure .er/ directory exists
        std::fs::create_dir_all(&er_dir_path)?;

        let name_owned: std::sync::Arc<str> = std::sync::Arc::from(name);
        let prompt_owned = prompt.to_string();

        // Send status log entry before spawning
        let _ = self.tab().log_tx.send(AgentLogEntry {
            timestamp: std::time::Instant::now(),
            command_name: std::sync::Arc::from(name),
            source: AgentLogSource::Status,
            text: format!("{} started", name),
        });

        let log_tx = self.tab().log_tx.clone();
        let agent_timeout = self.config.ai_hub.effective_agent_timeout();
        let slot_cap = self.config.ai_hub.effective_max_concurrent_reviews();
        let ceiling = self.config.ai_hub.effective_max_concurrent_agents();
        let run = crate::agent_run::AgentRunHandle::new();
        let run_for_tab = std::sync::Arc::clone(&run);
        let run_name = name.to_string();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut timer = crate::agent_timing::AgentRunTimer::start();
            let result = (|| -> Result<()> {
                // Waits for a slot like the background path. Admitting this
                // immediately would make `queue_ms` read ~0 even with the cap
                // saturated -- a measurement of the absent gate rather than of
                // a queue.
                let Some(_slot) = crate::agent_slots::acquire(
                    crate::agent_slots::Workload::Background,
                    slot_cap,
                    ceiling,
                    run.cancel_flag(),
                ) else {
                    return Err(crate::agent_run::cancelled());
                };
                timer.mark_slot_acquired();
                let debug_path = std::path::Path::new(&er_dir_path).join("debug-agent.log");

                let mut agent_args: Vec<String> = config_args
                    .iter()
                    .map(|a| a.replace("{prompt}", &prompt_owned))
                    .collect();

                // Auto-inject --output-format stream-json for claude commands so the
                // agent log panel can show real-time tool calls and progress. This is
                // injected here (not in config defaults) so user configs that override
                // agent.args still get streaming without manual changes.
                if is_stream_json {
                    if !agent_args.iter().any(|a| a == "--output-format") {
                        agent_args.push("--output-format".to_string());
                        agent_args.push("stream-json".to_string());
                    }
                    // --verbose is required when combining --print with stream-json (Claude only)
                    let has_print = agent_args.iter().any(|a| a == "--print");
                    let has_stream = agent_args.iter().any(|a| a == "stream-json");
                    let has_verbose = agent_args.iter().any(|a| a == "--verbose");
                    if is_claude_compatible && has_print && has_stream && !has_verbose {
                        agent_args.push("--verbose".to_string());
                    }
                }

                // Grant the agent targeted tool permissions without blanket
                // --dangerously-skip-permissions. The prompt is fully controlled by er.
                if is_claude_compatible {
                    let allowed: &[&str] = &[
                        "Read",
                        "Write",
                        "Edit",
                        "Bash(gh pr *)",
                        "Bash(cp *)",
                        "Bash(grep *)",
                        "Bash(rg *)",
                        "Bash(git grep*)",
                        "Bash(git diff*)",
                        "Bash(git show*)",
                        // Narrow on purpose: a bare `git fetch*` would also permit
                        // `git fetch origin +refs/heads/*:refs/heads/*`, which can
                        // clobber the user's local branches.
                        "Bash(git fetch origin pull/*)",
                        "Bash(shasum*)",
                        "Bash(sha256sum*)",
                        "Bash(mkdir*)",
                        "Bash(awk*)",
                    ];
                    for rule in allowed.iter().rev() {
                        agent_args.insert(0, rule.to_string());
                        agent_args.insert(0, "--allowedTools".to_string());
                    }
                    agent_args.insert(0, CLONE_DENY_RULE.to_string());
                    agent_args.insert(0, "--disallowedTools".to_string());
                }

                // Remote mode has no repo around it, which is what sends the agent hunting
                // the filesystem for a copy of the code. When the local checkout *is* the
                // PR's repo, run there so verification reads hit real files; otherwise fall
                // back to the artifact dir. Output dirs are absolute either way.
                let remote_checkout = if is_remote {
                    remote_repo
                        .as_deref()
                        .and_then(|slug| crate::github::local_checkout_for_repo(&repo_root, slug))
                } else {
                    None
                };
                let work_dir = if let Some(checkout) = &remote_checkout {
                    checkout
                } else if is_remote {
                    &er_dir_path
                } else {
                    &repo_root
                };
                let mut cmd = std::process::Command::new(&agent_cmd);
                cmd.args(&agent_args)
                    .current_dir(work_dir)
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped());
                if let Some((key, value)) = &opencode_env {
                    cmd.env(key, value);
                }
                let mut child = run
                    .spawn(&mut cmd)
                    .with_context(|| format!("Failed to run {} ({})", name_owned, agent_cmd))?;
                let child_id = child.id();
                timer.mark_spawned();

                let stdout = child.stdout.take();
                let stderr = child.stderr.take();
                run.register(child);
                run.arm_timeout(agent_timeout);

                // Accumulate stdout for debug log while also streaming to agent log.
                // When output is stream-json (default for claude), parse events into
                // human-readable log entries. Falls back to raw lines for other formats.
                let log_tx_out = log_tx.clone();
                let cmd_name_out = name_owned.clone();
                let stdout_handle = std::thread::spawn(move || -> Vec<String> {
                    let mut lines: Vec<String> = Vec::new();
                    use std::io::BufRead;
                    let pipe_lines = stdout.into_iter().flat_map(|pipe| {
                        std::io::BufReader::new(pipe).lines().map_while(Result::ok)
                    });
                    // Lines that parse to None (noise like empty results) are kept
                    // in `lines` but not logged.
                    let log_texts = pipe_lines
                        .inspect(|line| lines.push(line.clone()))
                        .filter_map(|line| stdout_log_text(&line, is_stream_json));
                    for text in log_texts {
                        let _ = log_tx_out.send(AgentLogEntry {
                            timestamp: std::time::Instant::now(),
                            command_name: cmd_name_out.clone(),
                            source: AgentLogSource::Stdout,
                            text,
                        });
                    }
                    lines
                });

                // Accumulate stderr for debug log while also streaming to agent log
                let log_tx_err = log_tx.clone();
                let cmd_name_err = name_owned.clone();
                let stderr_handle = std::thread::spawn(move || -> Vec<String> {
                    let mut lines: Vec<String> = Vec::new();
                    use std::io::BufRead;
                    let pipe_lines = stderr.into_iter().flat_map(|pipe| {
                        std::io::BufReader::new(pipe).lines().map_while(Result::ok)
                    });
                    for line in pipe_lines {
                        let _ = log_tx_err.send(AgentLogEntry {
                            timestamp: std::time::Instant::now(),
                            command_name: cmd_name_err.clone(),
                            source: AgentLogSource::Stderr,
                            text: line.clone(),
                        });
                        lines.push(line);
                    }
                    lines
                });

                let child = run.take_child(child_id).with_context(|| {
                    format!("Lost the handle for {} ({})", name_owned, agent_cmd)
                })?;
                let status = run.wait_for(child).with_context(|| {
                    format!("Failed to wait for {} ({})", name_owned, agent_cmd)
                })?;
                let stdout_lines = stdout_handle.join().unwrap_or_default();
                let stderr_lines = stderr_handle.join().unwrap_or_default();
                timer.mark_finished();

                // Timeout before cancel: a run killed by its deadline has both
                // flags set, and "you stopped this" would be the wrong story.
                if run.is_timed_out() {
                    return Err(crate::agent_run::timed_out(agent_timeout));
                }
                if run.is_cancelled() {
                    return Err(crate::agent_run::cancelled());
                }

                // Full transcript with accumulated stdout + stderr, opt-in.
                if debug_agent_log_enabled() {
                    let debug_content = format!(
                        "=== {} agent command ===\ncommand: {} {}\nexit code: {}\n\n--- stdout ---\n{}\n\n--- stderr ---\n{}\n",
                        name_owned,
                        agent_cmd,
                        agent_args.join(" "),
                        status.code().map_or_else(|| "signal".to_string(), |c| c.to_string()),
                        stdout_lines.join("\n"),
                        stderr_lines.join("\n"),
                    );
                    let _ = std::fs::write(&debug_path, &debug_content);
                }

                if !status.success() {
                    // Only point at the transcript when there is one to read.
                    let detail = if debug_agent_log_enabled() {
                        format!(" (full transcript in {er_dir_path}/debug-agent.log)")
                    } else {
                        " (set ER_DEBUG=1 for the full transcript)".to_string()
                    };
                    anyhow::bail!("{} failed{detail}", name_owned);
                }

                Ok(())
            })();
            let ok = result.is_ok();
            let _ = tx.send(result);
            timer.emit("tab_command", &name_owned, ok);
        });

        self.tab_mut().command_rx.insert(name.to_string(), rx);
        self.tab_mut()
            .command_status
            .insert(name.to_string(), CommandStatus::Running);
        self.tab_mut().command_runs.insert(run_name, run_for_tab);
        self.notify(&format!("{} started...", name));
        Ok(())
    }

    /// Spawn an app-level background general review (`kind` = `review`).
    pub fn spawn_background_review(
        &mut self,
        target: super::background::BackgroundTaskTarget,
        prompt: String,
        prepared_diff: bool,
    ) -> Result<()> {
        self.spawn_background_agent_task(
            "review".to_string(),
            "review",
            target,
            prompt,
            prepared_diff,
            None,
        )
    }

    /// Spawn a specialized expert review (`kind` = `expert:{id}`). General and
    /// multiple experts can run concurrently on the same target.
    pub fn spawn_background_expert_review(
        &mut self,
        expert_id: &str,
        target: super::background::BackgroundTaskTarget,
        prompt: String,
        prepared_diff: bool,
    ) -> Result<()> {
        let def = crate::ai::expert_by_id(expert_id)
            .ok_or_else(|| anyhow::anyhow!("unknown expert: {expert_id}"))?;
        self.spawn_background_agent_task(
            crate::ai::expert_task_kind(expert_id),
            &format!("expert-{}", def.id),
            target,
            prompt,
            prepared_diff,
            None,
        )
    }

    /// Spawn guided-tour generation (`kind` = `tour`). Writes `tour.json` only.
    pub fn spawn_background_tour(
        &mut self,
        target: super::background::BackgroundTaskTarget,
        prompt: String,
        prepared_diff: bool,
    ) -> Result<()> {
        self.spawn_background_agent_task(
            "tour".to_string(),
            "tour",
            target,
            prompt,
            prepared_diff,
            None,
        )
    }

    /// Spawn the agent that proposes this repo's importance rules
    /// (`kind` = `importance`).
    ///
    /// The prompt carries the repo, not a diff, and the agent has no write path:
    /// it prints a rule table, and the worker validates and merges that into the
    /// global config (`RiskRuleProposal::merge_into_global_config`). A table
    /// nobody can read back before it lands is worse than no table.
    pub fn spawn_background_importance(&mut self) -> Result<()> {
        let scope = "branch".to_string();
        // Ranking reads the tree. A remote-only tab has none: its `repo_root`
        // is the process's working directory, so the agent would rank an
        // unrelated checkout and file the rules under the PR's repo.
        if self.tab().is_remote() {
            anyhow::bail!("Ranking importance needs a local clone of this repo");
        }
        let (repo_root, branch_label, base_branch, er_dir, pr_number, remote_repo, is_remote) = {
            let tab = self.tab();
            (
                tab.repo_root.clone(),
                tab.local_branch_view
                    .clone()
                    .unwrap_or_else(|| tab.current_branch.clone()),
                tab.base_branch.clone(),
                tab.er_dir(),
                tab.pr_number,
                tab.remote_repo.clone(),
                tab.remote_repo.is_some(),
            )
        };
        if repo_root.is_empty() {
            anyhow::bail!("Open a repository first — there is nothing to rank");
        }

        // The tab's key, so the table lands where its filter and header read.
        let repo = self.tab().rules_key();
        let prompt = crate::ai::prompts::build_importance_prompt(&repo, &repo_root);
        let target = super::background::BackgroundTaskTarget {
            repo_root,
            er_dir,
            branch_label,
            base_branch,
            scope,
            pr_number,
            remote_repo,
            managed_local: !is_remote,
        };

        self.spawn_background_agent_task(
            crate::ai::prompts::IMPORTANCE_TASK_KIND.to_string(),
            "importance",
            target,
            prompt,
            false,
            None,
        )
    }

    /// Generate a guided tour for the **active tab** and spawn it as an
    /// app-level background task (`kind` = `tour`).
    ///
    /// Mirrors the desktop `generate_tour` command so the TUI and desktop
    /// produce identical artifacts: captures the active view's diff (the PR
    /// head-vs-base diff in PrDiff mode), writes the prepared-diff artifacts
    /// into the active view's tour bucket (`tour_bucket_er_dir`), builds the
    /// tour prompt, and enqueues the task. Completion is surfaced by the
    /// app-level task poll (`poll_background_tasks`), which both reloads the
    /// sidecars and lets the mtime poll surface the Guide tab.
    pub fn spawn_tour_for_active_tab(&mut self) -> Result<()> {
        let scope = "branch".to_string();
        let (repo_root, branch_label, base_branch, er_dir, pr_number, remote_repo, is_remote) = {
            let tab = self.tab();
            let branch_label = tab
                .local_branch_view
                .clone()
                .unwrap_or_else(|| tab.current_branch.clone());
            let er_dir = tab.tour_bucket_er_dir().unwrap_or_else(|| tab.er_dir());
            (
                tab.repo_root.clone(),
                branch_label,
                tab.base_branch.clone(),
                er_dir,
                tab.pr_number,
                tab.remote_repo.clone(),
                tab.remote_repo.is_some(),
            )
        };

        // The TUI calls this method directly. Check the exact tour destination
        // before creating the directory or preparing diff artifacts.
        self.check_review_bucket_admission(&er_dir)?;
        std::fs::create_dir_all(&er_dir)?;

        let raw = self.tab().raw_diff_for_review(&scope)?;
        if raw.trim().is_empty() {
            anyhow::bail!("Nothing to tour");
        }
        let diff_hash = crate::ai::prepared_diff::ensure_diff_artifacts(&er_dir, &raw)
            .map_err(|e| anyhow::anyhow!("failed to prepare tour diff: {e}"))?;

        let is_pr = self.tab().tour_context_is_pr();
        let scope_label = if is_pr { "PR diff" } else { "branch diff" };
        let prompt = crate::ai::prompts::build_tour_prompt_prepared_diff(
            scope_label,
            &er_dir,
            "tour.json",
            &diff_hash,
        );
        let target = super::background::BackgroundTaskTarget {
            repo_root,
            er_dir,
            branch_label,
            base_branch,
            scope,
            pr_number,
            remote_repo,
            managed_local: !is_remote,
        };
        self.spawn_background_tour(target, prompt, true)
    }

    /// Spawn diagram generation (`kind` = `diagram:<diagram-kind>`). The agent
    /// runs read-only and emits JSON; the harness atomically writes
    /// `diagrams/<file>.json` only (prompt-injection write confinement).
    pub fn spawn_background_diagram(
        &mut self,
        diagram_kind: &str,
        target: super::background::BackgroundTaskTarget,
        prompt: String,
        prepared_diff: bool,
        host_write: super::background::HostWriteDiagram,
    ) -> Result<()> {
        self.spawn_background_agent_task(
            crate::ai::diagram_task_kind(diagram_kind),
            "diagram",
            target,
            prompt,
            prepared_diff,
            Some(host_write),
        )
    }

    /// Spawn the Professor learning agent (`kind` = `professor`).
    pub fn spawn_background_professor_review(
        &mut self,
        target: super::background::BackgroundTaskTarget,
        prompt: String,
        prepared_diff: bool,
    ) -> Result<()> {
        self.spawn_background_agent_task(
            crate::ai::professor_task_kind(),
            "professor",
            target,
            prompt,
            prepared_diff,
            None,
        )
    }

    /// Spawn triage scan (`kind` = `triage`) using the shared default model at low effort.
    pub fn spawn_background_triage_review(
        &mut self,
        target: super::background::BackgroundTaskTarget,
        prompt: String,
        prepared_diff: bool,
    ) -> Result<()> {
        self.spawn_background_agent_task(
            crate::ai::triage_task_kind(),
            "triage",
            target,
            prompt,
            prepared_diff,
            None,
        )
    }

    /// Number of background agent tasks currently running a subprocess.
    pub(crate) fn running_background_task_count(&self) -> usize {
        self.background_tasks
            .values()
            .filter(|h| matches!(h.task.status, CommandStatus::Running))
            .count()
    }

    /// Mark an accepted background task as belonging to a review flow.
    /// Call this immediately after enqueue returns its ID.
    pub(crate) fn register_review_flow_task(&mut self, id: String) {
        self.owned_flow_task_ids.insert(id);
    }

    /// Consume one terminal task record. Ownership is released at the same
    /// time, so later polling cannot retain or redeliver it.
    pub(crate) fn take_review_flow_task_completion(
        &mut self,
        id: &str,
    ) -> Option<super::background::BackgroundTask> {
        let completion = self.flow_task_completions.remove(id);
        if completion.is_some() {
            self.owned_flow_task_ids.remove(id);
        }
        completion
    }

    fn record_flow_task_completion(&mut self, task: &super::background::BackgroundTask) {
        if self.owned_flow_task_ids.contains(&task.id) {
            self.flow_task_completions
                .insert(task.id.clone(), task.clone());
        }
    }

    /// App-level background agent task (review or expert). When the number
    /// of running tasks has reached `ai_hub.max_concurrent_reviews`, the
    /// task is queued and launched later by `poll_background_tasks`.
    fn spawn_background_agent_task(
        &mut self,
        kind: String,
        command_name: &str,
        target: super::background::BackgroundTaskTarget,
        prompt: String,
        prepared_diff: bool,
        host_write_diagram: Option<super::background::HostWriteDiagram>,
    ) -> Result<()> {
        self.check_review_bucket_admission(&target.er_dir)?;
        self.enqueue_background_agent_task(
            kind,
            command_name,
            target,
            prompt,
            prepared_diff,
            host_write_diagram,
            None,
        )
        .map(|_| ())
    }

    /// Accept a background task and return the exact ID assigned before it is
    /// queued or launched. Flow callers register the ID before App polling can
    /// observe a terminal result.
    #[expect(
        clippy::too_many_arguments,
        reason = "preserve ordinary action admission with the captured launch configuration"
    )]
    pub(super) fn enqueue_background_agent_task(
        &mut self,
        kind: String,
        command_name: &str,
        target: super::background::BackgroundTaskTarget,
        prompt: String,
        prepared_diff: bool,
        host_write_diagram: Option<super::background::HostWriteDiagram>,
        launch_config: Option<crate::config::ErConfig>,
    ) -> Result<String> {
        self.enqueue_background_agent_task_in_checkout(
            kind,
            command_name,
            target,
            prompt,
            prepared_diff,
            host_write_diagram,
            launch_config,
            None,
        )
    }

    /// Capture a flow's execution checkout without changing its target key or
    /// the ordinary actions' checkout lookup policy.
    #[expect(
        clippy::too_many_arguments,
        reason = "flow launches carry a captured checkout separately from stable target identity"
    )]
    pub(super) fn enqueue_background_agent_task_in_checkout(
        &mut self,
        kind: String,
        command_name: &str,
        target: super::background::BackgroundTaskTarget,
        prompt: String,
        prepared_diff: bool,
        host_write_diagram: Option<super::background::HostWriteDiagram>,
        launch_config: Option<crate::config::ErConfig>,
        execution_root: Option<String>,
    ) -> Result<String> {
        use super::background::{BackgroundTask, PendingBackgroundTask};

        if target.repo_root.is_empty() && target.remote_repo.is_none() {
            anyhow::bail!("Open a repository or PR first — nothing to review yet");
        }

        let already_active = self.background_tasks.values().any(|h| {
            h.task.kind == kind
                && h.task.target == target
                && matches!(h.task.status, CommandStatus::Running)
        }) || self
            .pending_background_tasks
            .iter()
            .any(|p| p.task.kind == kind && p.task.target == target);
        if already_active {
            anyhow::bail!(
                "{command_name} already running for {}",
                target.display_label()
            );
        }

        let mut task = BackgroundTask::new(kind, target);
        task.execution_root = execution_root;
        let pending =
            PendingBackgroundTask {
                task,
                command_name: command_name.to_string(),
                prompt,
                prepared_diff,
                host_write_diagram,
                // Snapshot at enqueue so a mid-queue palette change cannot retarget
                // an already-queued job.
                ai_selection: Some(self.pending_ai_selection_override.clone().unwrap_or_else(
                    || {
                        self.sync_ai_selection();
                        crate::config::AiSelection {
                            provider_id: self.current_ai_provider.clone(),
                            model_id: self.current_ai_model.clone(),
                            effort: self.current_ai_effort.clone(),
                        }
                    },
                )),
                launch_config,
            };
        let task_id = pending.task.id.clone();

        let cap = self.config.ai_hub.effective_max_concurrent_reviews();
        if self.running_background_task_count() >= cap {
            let label = pending.task.target.display_label();
            self.pending_background_tasks.push_back(pending);
            let pos = self.pending_background_tasks.len();
            self.notify(&format!("{command_name} queued (#{pos}, {label})"));
            return Ok(task_id);
        }

        self.launch_background_agent_task(pending)
    }

    /// Actually spawn the agent subprocess for an accepted task. Split from
    /// `spawn_background_agent_task` so the dispatch loop can launch queued
    /// tasks when capacity frees up.
    #[expect(
        clippy::literal_string_with_formatting_args,
        reason = "{prompt} is a template placeholder substituted via .replace()"
    )]
    fn launch_background_agent_task(
        &mut self,
        pending: super::background::PendingBackgroundTask,
    ) -> Result<String> {
        use super::background::BackgroundTaskHandle;

        let super::background::PendingBackgroundTask {
            mut task,
            command_name,
            prompt,
            prepared_diff,
            host_write_diagram,
            ai_selection,
            launch_config,
        } = pending;
        let command_name = command_name.as_str();
        let target = task.target.clone();
        // Captured before the task moves into its handle: the worker recognises
        // its own kind to know whose reply it is parsing, and which repo that
        // reply's table belongs to.
        let task_kind = task.kind.clone();
        // Ranking is refused on remote-only tabs, so the clone names the repo
        // here as it does in the prompt (`TabState::rules_key`).
        let repo_for_worker = crate::storage::rules_key(&target.repo_root, None);
        // The task may have waited in the queue; report runtime from launch.
        // The id (assigned at enqueue) stays stable so UI pills don't jump.
        task.started_at_ms = super::background::unix_now_ms();

        let selection = ai_selection.unwrap_or_else(|| {
            self.sync_ai_selection();
            crate::config::AiSelection {
                provider_id: self.current_ai_provider.clone(),
                model_id: self.current_ai_model.clone(),
                effort: self.current_ai_effort.clone(),
            }
        });
        let config = launch_config.as_ref().unwrap_or(&self.config);

        let (
            agent_cmd,
            mut config_args,
            is_claude_compatible,
            is_codex,
            is_stream_json,
            resolved_provider_id,
            resolved_model_id,
            family,
        ) = if let Some(provider_id) = config
            .ai_hub
            .resolve_provider_id(selection.provider_id.as_deref())
        {
            let provider = config
                .ai_hub
                .providers
                .get(&provider_id)
                .ok_or_else(|| anyhow::anyhow!("Unknown AI provider: {}", provider_id))?;
            let mut args = provider.args.clone();
            let resolved_model_id = config
                .ai_hub
                .resolve_model_id(&provider_id, selection.model_id.as_deref());
            let family = provider.cli_family();
            if let Some(model_id) = &resolved_model_id {
                if let Some(model) = provider.models.iter().find(|m| m.id == *model_id) {
                    crate::config::extend_provider_model_args(family, &mut args, &model.args);
                }
            }
            let is_claude = crate::config::agent_command_is_claude(&provider.command);
            let is_codex = crate::config::agent_command_is_codex(&provider.command);
            (
                provider.command.clone(),
                args,
                is_claude,
                is_codex,
                provider.uses_stream_json_log(),
                Some(provider_id),
                resolved_model_id,
                family,
            )
        } else {
            let cmd = config.agent.command.clone();
            let is_claude = crate::config::agent_command_is_claude(&cmd);
            let is_codex = crate::config::agent_command_is_codex(&cmd);
            let family = crate::config::CliFamily::detect(&cmd);
            (
                cmd.clone(),
                config.agent.args.clone(),
                is_claude,
                is_codex,
                crate::config::agent_command_uses_stream_json(&cmd),
                None,
                (!config.agent.model.is_empty()).then(|| config.agent.model.clone()),
                family,
            )
        };

        let effort_override = if task.kind == "triage" {
            Some("low")
        } else {
            None
        };
        let effort = crate::config::resolve_effort_for_model(
            &config.ai_hub,
            &config.agent,
            resolved_provider_id.as_deref(),
            resolved_model_id.as_deref(),
            selection.effort.as_deref(),
            effort_override,
        );
        if task_kind == crate::ai::prompts::IMPORTANCE_TASK_KIND {
            validate_importance_user_config(family, &config_args)?;
        }
        crate::config::inject_provider_effort(
            family,
            &mut config_args,
            resolved_model_id.as_deref(),
            effort.as_deref(),
        );
        if is_codex {
            crate::config::inject_codex_ignore_user_config(&mut config_args);
        }
        crate::config::inject_claude_startup_trim(family, &mut config_args);
        // Diagrams and importance leave persistence to the host. Importance's
        // Claude tool ceiling and OpenCode's permission env deny edits while
        // retaining bucket access for reading diff-tmp. Codex and
        // Cursor have no tool list, so for them the only write control is the
        // sandbox, and `--add-dir` is documented as adding a *writable* directory
        // — so a read-only run withholds it and narrows the sandbox instead.
        let readonly_run = background_run_is_read_only(&task_kind, host_write_diagram.is_some());
        let storage_access = match family {
            crate::config::CliFamily::Codex | crate::config::CliFamily::Cursor if readonly_run => {
                None
            }
            _ => Some(target.er_dir.as_str()),
        };
        crate::config::inject_agent_storage_access(family, &mut config_args, storage_access);
        if readonly_run {
            crate::config::apply_readonly_spawn(family, &mut config_args);
        }
        let opencode_env = if readonly_run {
            crate::config::apply_opencode_readonly_storage_spawn(
                family,
                &mut config_args,
                Some(target.er_dir.as_str()),
            )
        } else {
            crate::config::apply_opencode_spawn(
                family,
                &mut config_args,
                Some(target.er_dir.as_str()),
            )
        };
        if task_kind == crate::ai::prompts::IMPORTANCE_TASK_KIND {
            if family == crate::config::CliFamily::Claude {
                config_args.splice(
                    0..0,
                    [
                        "--tools".to_string(),
                        IMPORTANCE_READ_ONLY_TOOLS.join(","),
                        "--disallowedTools".to_string(),
                        MCP_DENY_RULE.to_string(),
                    ],
                );
            }
            validate_importance_launch(
                family,
                &config_args,
                opencode_env.as_ref(),
                (family == crate::config::CliFamily::Codex && resolved_model_id.is_some())
                    .then_some(effort.as_deref())
                    .flatten(),
            )?;
        }

        std::fs::create_dir_all(&target.er_dir)?;

        let task_id = task.id.clone();
        let er_dir = target.er_dir.clone();
        let repo_root = target.repo_root.clone();
        let execution_root = task.execution_root.clone();
        let remote_repo = target.remote_repo.clone();
        let is_remote = target.remote_repo.is_some();
        let managed_local = target.managed_local;
        // For local-branch views (cache-dir er_dir) treat like remote — cwd is
        // the cache dir so .er/* writes land where the loader reads them.
        let is_cache_er_dir = !er_dir.starts_with(&repo_root);

        let (log_tx, log_rx) = std::sync::mpsc::channel::<AgentLogEntry>();
        let (result_tx, result_rx) = std::sync::mpsc::channel::<Result<()>>();

        let _ = log_tx.send(AgentLogEntry {
            timestamp: std::time::Instant::now(),
            command_name: std::sync::Arc::from(command_name),
            source: AgentLogSource::Status,
            text: format!("{command_name} started ({})", target.display_label()),
        });

        let log_tx_thread = log_tx;
        let command_name_stdout: std::sync::Arc<str> = std::sync::Arc::from(command_name);
        let command_name_stderr: std::sync::Arc<str> = std::sync::Arc::from(command_name);
        let command_name_fail: std::sync::Arc<str> = std::sync::Arc::from(command_name);
        let command_name_emit: std::sync::Arc<str> = std::sync::Arc::from(command_name);
        let slot_cap = self.config.ai_hub.effective_max_concurrent_reviews();
        let ceiling = self.config.ai_hub.effective_max_concurrent_agents();
        let agent_timeout = config.ai_hub.effective_agent_timeout();
        // The worker owns one clone; the App keeps the other so a stop control
        // can reach the process. Built here, outside the thread, so the handle
        // exists before anything can try to stop it.
        let run = crate::agent_run::AgentRunHandle::new();
        let run_for_task = std::sync::Arc::clone(&run);
        let progress_notify = self.background_progress_notify.clone();
        std::thread::spawn(move || {
            let mut progress_signal = NotifyBackgroundProgressOnDrop {
                result_tx: Some(result_tx),
                notify: progress_notify,
            };
            let mut timer = crate::agent_timing::AgentRunTimer::start();
            let result = (|| -> Result<()> {
                // Hard process-wide cap shared with arena reviewers. The
                // App-level queue already bounds how many of these workers
                // exist, so this only waits while arena rounds hold slots.
                let _slot = crate::agent_slots::acquire_blocking(
                    crate::agent_slots::Workload::Background,
                    slot_cap,
                    ceiling,
                );
                timer.mark_slot_acquired();
                let debug_path = std::path::Path::new(&er_dir).join("debug-agent.log");

                let mut agent_args: Vec<String> = config_args
                    .iter()
                    .map(|a| a.replace("{prompt}", &prompt))
                    .collect();

                if is_stream_json {
                    if !agent_args.iter().any(|a| a == "--output-format") {
                        agent_args.push("--output-format".to_string());
                        agent_args.push("stream-json".to_string());
                    }
                    let has_print = agent_args.iter().any(|a| a == "--print");
                    let has_stream = agent_args.iter().any(|a| a == "stream-json");
                    let has_verbose = agent_args.iter().any(|a| a == "--verbose");
                    if is_claude_compatible && has_print && has_stream && !has_verbose {
                        agent_args.push("--verbose".to_string());
                    }
                }

                if is_claude_compatible {
                    let allowed = if task_kind == crate::ai::prompts::IMPORTANCE_TASK_KIND {
                        IMPORTANCE_READ_ONLY_TOOLS
                    } else {
                        claude_allowed_tools(readonly_run, prepared_diff)
                    };
                    for rule in allowed.iter().rev() {
                        agent_args.insert(0, rule.to_string());
                        agent_args.insert(0, "--allowedTools".to_string());
                    }
                    agent_args.insert(0, CLONE_DENY_RULE.to_string());
                    agent_args.insert(0, "--disallowedTools".to_string());
                }

                if task_kind == crate::ai::prompts::IMPORTANCE_TASK_KIND
                    && family == crate::config::CliFamily::Claude
                {
                    validate_importance_launch(family, &agent_args, None, None)?;
                }

                // managed_local: er_dir is outside repo_root but git must run from repo_root.
                // is_remote / is_cache_er_dir without managed_local: cwd = er_dir (cache-dir or remote).
                // A remote PR whose repo *is* the local checkout runs there instead, so the
                // agent can verify against real code rather than hunting the filesystem.
                let remote_checkout = if execution_root.is_none() && is_remote {
                    remote_repo
                        .as_deref()
                        .and_then(|slug| crate::github::local_checkout_for_repo(&repo_root, slug))
                } else {
                    None
                };
                let work_dir = if let Some(checkout) = &execution_root {
                    checkout
                } else if let Some(checkout) = &remote_checkout {
                    checkout
                } else if managed_local || (!is_remote && !is_cache_er_dir) {
                    &repo_root
                } else {
                    &er_dir
                };
                let mut cmd = std::process::Command::new(&agent_cmd);
                cmd.args(&agent_args)
                    .current_dir(work_dir)
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped());
                if let Some((key, value)) = &opencode_env {
                    cmd.env(key, value);
                }
                let mut child = run
                    .spawn(&mut cmd)
                    .with_context(|| format!("Failed to run review ({})", agent_cmd))?;
                let child_id = child.id();
                timer.mark_spawned();

                let stdout = child.stdout.take();
                let stderr = child.stderr.take();
                // Hand ownership over only once the pipes are off it, so a
                // timeout firing here still reaches the process.
                run.register(child);
                run.arm_timeout(agent_timeout);

                let log_tx_out = log_tx_thread.clone();
                let stdout_handle = std::thread::spawn(move || -> Vec<String> {
                    let mut lines: Vec<String> = Vec::new();
                    use std::io::BufRead;
                    let pipe_lines = stdout.into_iter().flat_map(|pipe| {
                        std::io::BufReader::new(pipe).lines().map_while(Result::ok)
                    });
                    // Lines that parse to None (noise like empty results) are kept
                    // in `lines` but not logged.
                    let log_texts = pipe_lines
                        .inspect(|line| lines.push(line.clone()))
                        .filter_map(|line| stdout_log_text(&line, is_stream_json));
                    for text in log_texts {
                        let _ = log_tx_out.send(AgentLogEntry {
                            timestamp: std::time::Instant::now(),
                            command_name: command_name_stdout.clone(),
                            source: AgentLogSource::Stdout,
                            text,
                        });
                    }
                    lines
                });

                let log_tx_err = log_tx_thread.clone();
                let stderr_handle = std::thread::spawn(move || -> Vec<String> {
                    let mut lines: Vec<String> = Vec::new();
                    use std::io::BufRead;
                    let pipe_lines = stderr.into_iter().flat_map(|pipe| {
                        std::io::BufReader::new(pipe).lines().map_while(Result::ok)
                    });
                    for line in pipe_lines {
                        let _ = log_tx_err.send(AgentLogEntry {
                            timestamp: std::time::Instant::now(),
                            command_name: command_name_stderr.clone(),
                            source: AgentLogSource::Stderr,
                            text: line.clone(),
                        });
                        lines.push(line);
                    }
                    lines
                });

                let child = run
                    .take_child(child_id)
                    .with_context(|| format!("Lost the handle for review ({agent_cmd})"))?;
                let status = run
                    .wait_for(child)
                    .with_context(|| format!("Failed to wait for review ({})", agent_cmd))?;
                let stdout_lines = stdout_handle.join().unwrap_or_default();
                let stderr_lines = stderr_handle.join().unwrap_or_default();
                timer.mark_finished();

                // The verdict, read once the child is reaped and the pipes are
                // drained. Timeout first: a run killed by its deadline also
                // has `cancel` set, and "you stopped this" would be the wrong
                // story to tell about it.
                if run.is_timed_out() {
                    return Err(crate::agent_run::timed_out(agent_timeout));
                }
                if run.is_cancelled() {
                    return Err(crate::agent_run::cancelled());
                }

                if debug_agent_log_enabled() {
                    let debug_content = format!(
                        "=== review agent command ===\ncommand: {} {}\nexit code: {}\n\n--- stdout ---\n{}\n\n--- stderr ---\n{}\n",
                        agent_cmd,
                        agent_args.join(" "),
                        status
                            .code()
                            .map_or_else(|| "signal".to_string(), |c| c.to_string()),
                        stdout_lines.join("\n"),
                        stderr_lines.join("\n"),
                    );
                    let _ = std::fs::write(&debug_path, &debug_content);
                }

                if !status.success() {
                    let stderr_snip = agent_stderr_snippet(&stderr_lines);
                    if stderr_snip.to_lowercase().contains("not logged in")
                        || stderr_snip.to_lowercase().contains("authentication")
                        || stderr_snip.to_lowercase().contains("unauthorized")
                        || stderr_snip.to_lowercase().contains("not authenticated")
                    {
                        anyhow::bail!(
                            "{command_name_fail}: `{agent_cmd}` needs authentication. \
                             Run `{agent_cmd}` in a terminal to sign in, then retry. {stderr_snip}"
                        );
                    }
                    if stderr_snip.is_empty() {
                        anyhow::bail!(
                            "{command_name_fail} failed (exit {:?}). See Settings → AI Hub if the \
                             provider CLI is missing or not signed in.",
                            status.code()
                        );
                    }
                    anyhow::bail!("{command_name_fail} failed: {stderr_snip}");
                }
                // Host-owned importance write: the agent printed a rule table
                // and had no Write/Edit. Validate it and merge it into the
                // global config, replacing only this repo's table.
                if task_kind == crate::ai::prompts::IMPORTANCE_TASK_KIND {
                    let reply = stdout_lines.join("\n");
                    let proposal =
                        crate::config::RiskRuleProposal::from_reply(&reply).ok_or_else(|| {
                            anyhow::anyhow!(
                                "{command_name_fail}: the agent printed no importance table"
                            )
                        })?;
                    crate::config::RiskRuleProposal::merge_into_global_config(
                        &repo_for_worker,
                        proposal,
                    )
                    .with_context(|| {
                        format!("{command_name_fail}: failed to merge the importance table")
                    })?;
                }
                // Host-owned diagram write: agent had no Write/Edit; persist
                // only the validated diagrams/<id>.json from stdout.
                if let Some(hw) = &host_write_diagram {
                    crate::ai::persist_diagram_from_agent_stdout(
                        &stdout_lines.join("\n"),
                        is_stream_json,
                        &hw.kind,
                        &hw.diff_hash,
                        hw.custom_prompt.as_deref(),
                        &hw.output_path,
                    )
                    .with_context(|| {
                        format!("{command_name_fail}: failed to persist diagram sidecar")
                    })?;
                }
                // Selected-file reviews overwrite sidecars with a subset —
                // merge back into the pre-scoped snapshot when present.
                if let Err(e) = crate::ai::apply_scoped_sidecar_merge(
                    std::path::Path::new(&er_dir),
                    &command_name_fail,
                ) {
                    anyhow::bail!(
                        "{command_name_fail} wrote artifacts but failed to merge with previous review: {e}"
                    );
                }
                Ok(())
            })();
            let ok = result.is_ok();
            progress_signal.send_result(result);
            timer.emit("background", &command_name_emit, ok);
        });

        self.background_tasks.insert(
            task_id.clone(),
            BackgroundTaskHandle {
                task,
                run: run_for_task,
                result_rx,
                log_rx,
                recent_log: std::collections::VecDeque::new(),
            },
        );

        if super::background::debug_bg_enabled() {
            eprintln!(
                "[bg] inserted task id={} target={} map_size={}",
                task_id,
                target.display_label(),
                self.background_tasks.len()
            );
        }

        self.notify(&format!(
            "{command_name} started ({})",
            target.display_label()
        ));
        Ok(task_id)
    }

    /// Drain log channels and check for completion across all app-level
    /// background tasks. Should be called once per tick from the event loop
    /// (or per `poll` Tauri call on the desktop).
    pub fn poll_background_tasks(&mut self) {
        // 1. Drain log channels — push entries into matching tabs' agent_log
        //    so existing UI keeps working when viewing a matching tab.
        let task_ids: Vec<String> = self.background_tasks.keys().cloned().collect();
        for id in &task_ids {
            // Pull log entries without holding a long borrow.
            let mut drained: Vec<AgentLogEntry> = Vec::new();
            if let Some(handle) = self.background_tasks.get(id) {
                while let Ok(entry) = handle.log_rx.try_recv() {
                    drained.push(entry);
                }
            }
            if drained.is_empty() {
                continue;
            }
            let target = match self.background_tasks.get(id) {
                Some(h) => h.task.target.clone(),
                None => continue,
            };
            for tab in self.tabs.iter_mut() {
                if !tab.matches_target(&target) {
                    continue;
                }
                for entry in &drained {
                    tab.agent_log.push_back(entry.clone());
                    if tab.agent_log.len() > 5000 {
                        tab.agent_log.pop_front();
                    }
                }
            }
            // Also push into the per-handle ring buffer for app-wide log access.
            if let Some(handle) = self.background_tasks.get_mut(id) {
                for entry in &drained {
                    handle.recent_log.push_back(entry.clone());
                    if handle.recent_log.len() > 500 {
                        handle.recent_log.pop_front();
                    }
                }
            }
        }

        // 2. Check for completion.
        let mut completed: Vec<(String, Result<()>)> = Vec::new();
        for id in &task_ids {
            if let Some(handle) = self.background_tasks.get(id) {
                if !matches!(handle.task.status, CommandStatus::Running) {
                    continue;
                }
                match handle.result_rx.try_recv() {
                    Ok(res) => completed.push((id.clone(), res)),
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        completed.push((id.clone(), Err(anyhow::anyhow!("review thread crashed"))));
                    }
                }
            }
        }

        for (id, result) in completed {
            let now = super::background::unix_now_ms();
            let Some(handle) = self.background_tasks.get_mut(&id) else {
                continue;
            };
            let target = handle.task.target.clone();
            let label = target.display_label();
            let (status, error, status_msg) = match result {
                Ok(()) => (
                    CommandStatus::Done,
                    None,
                    format!("review completed ({})", label),
                ),
                Err(e) => {
                    let msg = e.to_string();
                    (
                        CommandStatus::Failed(msg.clone()),
                        Some(msg.clone()),
                        format!("review failed ({}): {}", label, msg),
                    )
                }
            };
            handle.task.status = status.clone();
            handle.task.finished_at_ms = Some(now);
            handle.task.error = error.clone();
            let wrote_importance = handle.task.kind == crate::ai::prompts::IMPORTANCE_TASK_KIND;
            let completion_task = handle.task.clone();
            self.record_flow_task_completion(&completion_task);

            // Force reload only on matching tabs. No `last_ai_check = None`
            // reset here: the agent's freshly written sidecars have
            // newer mtimes than the previous check, so `check_ai_files_changed`
            // fires the reload naturally — while a tab whose poll already
            // loaded the final files skips the redundant full re-read.
            for tab in self.tabs.iter_mut() {
                if tab.matches_target(&target) {
                    tab.push_synthetic_log("review", status_msg.clone(), AgentLogSource::Status);
                }
            }
            // Mirror completion status into handle's recent_log.
            if let Some(handle) = self.background_tasks.get_mut(&id) {
                handle.recent_log.push_back(AgentLogEntry {
                    timestamp: std::time::Instant::now(),
                    command_name: std::sync::Arc::from("review"),
                    source: AgentLogSource::Status,
                    text: status_msg.clone(),
                });
                if handle.recent_log.len() > 500 {
                    handle.recent_log.pop_front();
                }
            }

            // The risk-rules worker writes the config *file*; the app's copy is
            // now behind it, so re-read it and re-hand the rules to the tabs
            // that filter with them.
            if wrote_importance && matches!(status, CommandStatus::Done) {
                // Only the tables this task wrote: `[risk]`, and the `[importance]`
                // table it retires. A wholesale reload is what
                // silently reverted in-flight settings before — see the note on
                // `sync_config_from_active_tab`, and `docs/adr/0005`.
                let written = crate::config::load_global_config();
                self.config.risk = written.risk;
                self.config.importance = written.importance;
                self.sync_repo_rules_to_tabs();
                // The agent's own one-line distribution — the share each tier
                // covers — is what tells a reader whether the table is
                // over-broad, and it is in this task's log. Point at it rather
                // than repeating it here, where it would be a number without
                // the reasoning beside it.
                self.notify_long("Risk rules written — see the task log for its report");
            }

            self.notify_long(&status_msg);
        }

        // 3. Retire finished tasks past the 8s display window into
        //    `recent_background_tasks` (snapshot-only).
        let cutoff = super::background::unix_now_ms().saturating_sub(8_000);
        let mut to_remove: Vec<String> = Vec::new();
        for (id, handle) in self.background_tasks.iter() {
            if let Some(finished_at) = handle.task.finished_at_ms {
                if finished_at < cutoff {
                    to_remove.push(id.clone());
                }
            }
        }
        for id in to_remove {
            if let Some(handle) = self.background_tasks.remove(&id) {
                // Keep around in finished form only briefly; bounded.
                self.recent_background_tasks.push(handle.task);
                if self.recent_background_tasks.len() > 32 {
                    self.recent_background_tasks.remove(0);
                }
            }
        }

        // 4. Launch queued tasks while there's capacity.
        self.dispatch_pending_background_tasks();

        // Flow advancement must observe the terminal records from this poll,
        // including queued launch failures, before UI history is consulted.
        self.advance_review_flows();
        self.refresh_review_flow_progress();
    }

    /// Pop queued review tasks and launch them while the running count is
    /// below the configured cap. Launch failures are surfaced as failed
    /// tasks so the UI shows them like any other review failure.
    fn dispatch_pending_background_tasks(&mut self) {
        let cap = self.config.ai_hub.effective_max_concurrent_reviews();
        while !self.pending_background_tasks.is_empty() && self.can_dispatch_task(cap) {
            let Some(pending) = self.pending_background_tasks.pop_front() else {
                break;
            };
            let label = pending.task.target.display_label();
            let command_name = pending.command_name.clone();
            let mut failed_task = pending.task.clone();
            if let Err(e) = self.launch_background_agent_task(pending) {
                let msg = format!("{command_name} failed to start ({label}): {e}");
                failed_task.status = CommandStatus::Failed(e.to_string());
                failed_task.error = Some(e.to_string());
                failed_task.finished_at_ms = Some(super::background::unix_now_ms());
                self.record_flow_task_completion(&failed_task);
                self.recent_background_tasks.push(failed_task);
                if self.recent_background_tasks.len() > 32 {
                    self.recent_background_tasks.remove(0);
                }
                self.notify_long(&msg);
            }
        }
    }

    /// Whether a queued review can start now.
    ///
    /// Two questions, and they are not the same one. The App's own count is
    /// its queueing policy, and it is what the desktop's queued pills show —
    /// deterministic, because the count moves the moment a task dispatches.
    /// The slot pool is the real limit, and it is shared with arena reviewers,
    /// card AI and tab commands.
    ///
    /// Checking only the first put work in flight that could not start: with
    /// an arena holding every slot, a dispatched review showed as `Running`
    /// while parked inside `acquire`, indistinguishable from running. Checking
    /// only the second would race — the slot is taken on another thread, so a
    /// second task could dispatch in the window before the first acquires.
    fn can_dispatch_task(&self, cap: usize) -> bool {
        self.running_background_task_count() < cap && crate::agent_slots::active_count() < cap
    }

    /// Remove a queued (not yet started) review task. Returns true when a
    /// matching task was found and removed.
    pub fn cancel_queued_background_task(&mut self, id: &str) -> bool {
        let mut cancelled = None;
        self.pending_background_tasks.retain(|pending| {
            if pending.task.id == id {
                cancelled = Some(pending.task.clone());
                false
            } else {
                true
            }
        });
        let Some(mut task) = cancelled else {
            return false;
        };
        if self.owned_flow_task_ids.contains(id) {
            let reason = "cancelled before launch".to_owned();
            task.status = CommandStatus::Failed(reason.clone());
            task.error = Some(reason);
            task.finished_at_ms = Some(super::background::unix_now_ms());
            self.record_flow_task_completion(&task);
        }
        self.notify("review removed from queue");
        true
    }

    /// The running process for a background task, if it has one.
    ///
    /// Returns the handle rather than stopping the run, because stopping it
    /// forks a process and `AGENTS.md` forbids that under the app mutex. A
    /// caller holding the lock clones this, releases, and *then* calls
    /// [`crate::agent_run::AgentRunHandle::kill`]. A caller holding no lock
    /// may kill straight away.
    ///
    /// Signalling is all a stop does. The worker owns the verdict and reaches
    /// it after its `wait` returns — which the kill causes — so a stop must
    /// not also write a result, or a stopped run and a finished one would
    /// race to describe the same task.
    pub fn running_background_review(
        &self,
        id: &str,
    ) -> Option<std::sync::Arc<crate::agent_run::AgentRunHandle>> {
        self.background_tasks
            .get(id)
            .map(|h| std::sync::Arc::clone(&h.run))
    }

    /// The running process for a tab-level command (the tab-command and
    /// configured-shell paths), if it has one.
    ///
    /// Returns the handle for the same reason as
    /// [`Self::running_background_review`]: kill forks, so the caller releases
    /// the lock first.
    pub fn running_command(
        &self,
        name: &str,
    ) -> Option<std::sync::Arc<crate::agent_run::AgentRunHandle>> {
        self.tab().command_runs.get(name).map(std::sync::Arc::clone)
    }

    /// Every running tab-level command, by name.
    ///
    /// Collected into owned handles so the caller can release the borrow —
    /// and, in the desktop, the lock — before signalling any of them.
    pub fn running_commands(
        &self,
    ) -> Vec<(String, std::sync::Arc<crate::agent_run::AgentRunHandle>)> {
        self.tab()
            .command_runs
            .iter()
            .map(|(name, run)| (name.clone(), std::sync::Arc::clone(run)))
            .collect()
    }

    /// Snapshot of in-flight + recently finished background tasks. Includes
    /// Running tasks and Done/Failed within the last 8 seconds.
    pub fn background_task_snapshots(&self) -> Vec<super::background::BackgroundTaskSnapshot> {
        // Only log when there's actually something interesting to report —
        // otherwise this function fires many times per second from every
        // build_snapshot call and floods stderr.
        let debug_bg = super::background::debug_bg_enabled()
            && (!self.background_tasks.is_empty() || !self.recent_background_tasks.is_empty());
        let cutoff = super::background::unix_now_ms().saturating_sub(8_000);
        let mut out: Vec<super::background::BackgroundTaskSnapshot> = Vec::new();
        if debug_bg {
            eprintln!(
                "[bg] snapshots called map_size={} recent_size={} cutoff={}",
                self.background_tasks.len(),
                self.recent_background_tasks.len(),
                cutoff
            );
            for (id, handle) in self.background_tasks.iter() {
                eprintln!(
                    "[bg]   handle id={} status={:?} finished_at_ms={:?}",
                    id, handle.task.status, handle.task.finished_at_ms
                );
            }
        }
        for handle in self.background_tasks.values() {
            let include = match handle.task.status {
                CommandStatus::Running => true,
                _ => handle
                    .task
                    .finished_at_ms
                    .map(|t| t >= cutoff)
                    .unwrap_or(false),
            };
            if include {
                let mut snap = super::background::BackgroundTaskSnapshot::from_task(&handle.task);
                snap.recent_log = handle
                    .recent_log
                    .iter()
                    .rev()
                    .take(40)
                    .rev()
                    .cloned()
                    .collect();
                out.push(snap);
            }
        }
        for task in &self.recent_background_tasks {
            if task.finished_at_ms.map(|t| t >= cutoff).unwrap_or(false) {
                out.push(super::background::BackgroundTaskSnapshot::from_task(task));
            }
        }
        for pending in &self.pending_background_tasks {
            let mut snap = super::background::BackgroundTaskSnapshot::from_task(&pending.task);
            snap.status = "queued".to_string();
            out.push(snap);
        }
        out.sort_by_key(|t| t.started_at_ms);
        if debug_bg {
            eprintln!(
                "[bg] snapshots returning len={} statuses=[{}]",
                out.len(),
                out.iter()
                    .map(|t| format!("{}={}", t.id, t.status))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        out
    }

    /// Background tasks whose target matches the given tab. Used to merge
    /// app-level task entries into the active tab's `agent_commands`
    /// snapshot list.
    pub fn background_tasks_for_tab(
        &self,
        tab: &TabState,
    ) -> Vec<super::background::BackgroundTaskSnapshot> {
        let cutoff = super::background::unix_now_ms().saturating_sub(8_000);
        let mut out = Vec::new();
        for handle in self.background_tasks.values() {
            if !tab.matches_target(&handle.task.target) {
                continue;
            }
            let include = match handle.task.status {
                CommandStatus::Running => true,
                _ => handle
                    .task
                    .finished_at_ms
                    .map(|t| t >= cutoff)
                    .unwrap_or(false),
            };
            if include {
                out.push(super::background::BackgroundTaskSnapshot::from_task(
                    &handle.task,
                ));
            }
        }
        for pending in &self.pending_background_tasks {
            if !tab.matches_target(&pending.task.target) {
                continue;
            }
            let mut snap = super::background::BackgroundTaskSnapshot::from_task(&pending.task);
            snap.status = "queued".to_string();
            out.push(snap);
        }
        out
    }

    /// Return a tail of log entries for a specific background task by ID.
    /// Returns an empty Vec if the task is not found (may have been reaped).
    pub fn background_task_log_tail(&self, task_id: &str) -> Vec<AgentLogEntry> {
        self.background_tasks
            .get(task_id)
            .map(|h| h.recent_log.iter().cloned().collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod background_queue_tests {
    use crate::app::{App, BackgroundTaskTarget};
    use crate::config::{AiModelConfig, AiProviderConfig, AiSelection, ErConfig};

    fn target(tmp: &std::path::Path, branch: &str) -> BackgroundTaskTarget {
        BackgroundTaskTarget {
            repo_root: tmp.to_string_lossy().to_string(),
            er_dir: tmp.join(".er").to_string_lossy().to_string(),
            branch_label: branch.to_string(),
            base_branch: "main".to_string(),
            scope: "branch".to_string(),
            pr_number: None,
            remote_repo: None,
            managed_local: false,
        }
    }

    /// Build an App over a throwaway git repo with a slow no-op agent so
    /// spawned "reviews" run long enough to observe queue state. Returns
    /// None when git isn't available (test then silently skips, matching
    /// the pattern in background.rs).
    /// Serializes the tests below, which share the process-wide slot pool.
    ///
    /// `agent_slots` is one pool for the whole process, and dispatch now asks
    /// it whether a slot is free rather than counting only this App's own
    /// tasks. So a slot held by one test changes another's dispatch decision,
    /// and these run in parallel by default. Holding this for the duration
    /// keeps each test's view of the pool its own.
    fn serial() -> crate::agent_slots::TestSerialGuard {
        crate::agent_slots::test_serial()
    }

    fn test_app(cap: usize) -> Option<(App, std::path::PathBuf)> {
        let tmp =
            std::env::temp_dir().join(format!("er-bg-queue-test-{}-{}", std::process::id(), cap));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).ok()?;
        // `git init` alone leaves HEAD unborn, so `rev-parse --abbrev-ref HEAD`
        // yields nothing and `App::new_with_args` fails with "Failed to
        // determine current branch". That made this helper return None for
        // every caller, and every caller skips silently on None — so five
        // tests in this module had never run at all.
        for args in [
            vec!["init", "-b", "main"],
            vec!["config", "user.email", "t@example.com"],
            vec!["config", "user.name", "t"],
            vec!["config", "commit.gpgsign", "false"],
        ] {
            std::process::Command::new("git")
                .args(&args)
                .current_dir(&tmp)
                .output()
                .ok()?;
        }
        std::fs::write(tmp.join("seed.txt"), "seed\n").ok()?;
        for args in [vec!["add", "seed.txt"], vec!["commit", "-qm", "seed"]] {
            std::process::Command::new("git")
                .args(&args)
                .current_dir(&tmp)
                .output()
                .ok()?;
        }
        let mut app = App::new_with_args(&[tmp.to_string_lossy().to_string()]).ok()?;
        app.config.ai_hub.max_concurrent_reviews = cap;
        app.config.agent.command = "sleep".to_string();
        app.config.agent.args = vec!["5".to_string()];
        Some((app, tmp))
    }

    #[test]
    fn excess_reviews_queue_and_dedup() {
        let _serial = serial();
        let Some((mut app, tmp)) = test_app(1) else {
            return;
        };

        app.spawn_background_triage_review(target(&tmp, "feat-a"), "p".into(), true)
            .unwrap();
        app.set_ai_selection_override(AiSelection {
            provider_id: Some("codex".into()),
            model_id: Some("gpt-5.3-codex-spark".into()),
            effort: None,
        });
        app.spawn_background_triage_review(target(&tmp, "feat-b"), "p".into(), true)
            .unwrap();
        app.clear_ai_selection_override();

        assert_eq!(app.running_background_task_count(), 1, "cap of 1 enforced");
        assert_eq!(app.pending_background_tasks.len(), 1, "second task queued");
        assert!(app.pending_background_tasks[0].launch_config.is_none());
        assert_eq!(
            app.pending_background_tasks[0]
                .ai_selection
                .as_ref()
                .and_then(|selection| selection.model_id.as_deref()),
            Some("gpt-5.3-codex-spark")
        );

        // Same kind+target as the queued task is rejected as a duplicate.
        let dup = app.spawn_background_triage_review(target(&tmp, "feat-b"), "p".into(), true);
        assert!(dup.is_err(), "duplicate of queued task rejected");

        // Queued task is visible in snapshots as "queued".
        let snaps = app.background_task_snapshots();
        assert!(
            snaps.iter().any(|s| s.status == "queued"),
            "queued status surfaced: {:?}",
            snaps.iter().map(|s| s.status.clone()).collect::<Vec<_>>()
        );

        // Polling while the first task still runs must not launch the queued one.
        app.poll_background_tasks();
        assert_eq!(app.running_background_task_count(), 1);
        assert_eq!(app.pending_background_tasks.len(), 1);

        // Queued tasks can be cancelled by id.
        let queued_id = app.pending_background_tasks[0].task.id.clone();
        assert!(app.cancel_queued_background_task(&queued_id));
        assert!(app.pending_background_tasks.is_empty());
        assert!(!app.cancel_queued_background_task(&queued_id));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn a_saturated_slot_pool_stops_the_app_dispatching() {
        // The App's own running count is zero here, so the old condition would
        // have dispatched — and the task would have shown as "Running" while
        // parked inside `acquire`, which is the state this exists to prevent.
        //
        // Hold every slot to stand in for an arena run doing the same. The
        // pool is process-wide, so this must take all `cap` rather than assume
        // it starts empty: once held, nobody else can be in it.
        let _serial = serial();
        let Some((app, tmp)) = test_app(1) else {
            return;
        };
        let cap = app.config.ai_hub.effective_max_concurrent_reviews();
        let ceiling = app.config.ai_hub.effective_max_concurrent_agents();

        assert_eq!(app.running_background_task_count(), 0, "nothing dispatched");
        let held: Vec<_> = (0..cap)
            .map(|_| {
                crate::agent_slots::acquire_blocking(
                    crate::agent_slots::Workload::Background,
                    cap,
                    ceiling,
                )
            })
            .collect();
        assert!(
            !app.can_dispatch_task(cap),
            "a full pool must stop dispatch even with nothing running here"
        );

        drop(held);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn queued_task_snapshots_current_selection_without_override() {
        let _serial = serial();
        let Some((mut app, tmp)) = test_app(1) else {
            return;
        };
        app.config.ai_hub.providers.insert(
            "codex".into(),
            AiProviderConfig {
                models: vec![AiModelConfig {
                    id: "gpt-5.6-luna".into(),
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        app.current_ai_provider = Some("codex".into());
        app.current_ai_model = Some("gpt-5.6-luna".into());
        app.current_ai_effort = Some("high".into());

        app.spawn_background_triage_review(target(&tmp, "feat-a"), "p".into(), true)
            .unwrap();
        app.current_ai_model = Some("other".into());
        app.spawn_background_triage_review(target(&tmp, "feat-b"), "p".into(), true)
            .unwrap();

        let queued = &app.pending_background_tasks[0];
        assert_eq!(
            queued
                .ai_selection
                .as_ref()
                .and_then(|s| s.model_id.as_deref()),
            Some("gpt-5.6-luna"),
            "queued task keeps the selection from enqueue time"
        );
        // Effort is normalised against the model when the selection syncs, so
        // a literal here would be asserting that normalisation rather than
        // what this test is about. The claim is that the queued task holds
        // what was resolved at enqueue, not what the palette says later.
        let enqueued_effort = queued.ai_selection.as_ref().and_then(|s| s.effort.clone());
        app.current_ai_effort = Some("low".into());
        assert_eq!(
            app.pending_background_tasks[0]
                .ai_selection
                .as_ref()
                .and_then(|s| s.effort.clone()),
            enqueued_effort,
            "a later effort change must not retarget a queued task"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn dispatch_launches_queued_when_slot_frees() {
        let _serial = serial();
        let Some((mut app, tmp)) = test_app(2) else {
            return;
        };
        // A fast-exiting agent, installed in the hub rather than via
        // `config.agent`: the background path resolves its command from the
        // hub providers, and `App::new_with_args` loads the *user's* global
        // config, so leaving this to `config.agent` ran a real provider and
        // took 24 s. Completion frees a slot on the next poll.
        let fake = tmp.join("fake-agent");
        std::fs::write(&fake, "#!/bin/sh\nexit 0\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&fake).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&fake, perms).unwrap();
        }
        app.config.ai_hub.providers.clear();
        app.config.ai_hub.default_provider = Some("fake".to_string());
        app.config.ai_hub.providers.insert(
            "fake".to_string(),
            AiProviderConfig {
                command: fake.to_string_lossy().to_string(),
                args: vec!["{prompt}".to_string()],
                models: vec![AiModelConfig {
                    id: "m".to_string(),
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        app.config.ai_hub.default_model = Some("m".to_string());
        app.current_ai_provider = Some("fake".to_string());
        app.current_ai_model = Some("m".to_string());

        for branch in ["a", "b", "c"] {
            app.spawn_background_triage_review(target(&tmp, branch), "p".into(), true)
                .unwrap();
        }
        assert_eq!(app.running_background_task_count(), 2);
        assert_eq!(app.pending_background_tasks.len(), 1);

        // Wait for the fast agents to exit, then poll: completions are
        // detected and the queued task is dispatched.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            app.poll_background_tasks();
            if app.pending_background_tasks.is_empty() {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "queued task was never dispatched: running={} slots_in_use={}",
                app.running_background_task_count(),
                crate::agent_slots::active_count()
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    #[cfg(unix)]
    fn background_codex_review_ignores_user_config() {
        use std::os::unix::fs::PermissionsExt;
        // The transcript this test inspects is opt-in, so turn it on. Left set
        // for the rest of the binary rather than removed: clear it mid-run and
        // a concurrently finishing spawn could skip its own write.
        std::env::set_var("ER_DEBUG", "1");

        let _serial = serial();
        let Some((mut app, tmp)) = test_app(1) else {
            return;
        };
        let fake_bin = tmp.join("codex");
        std::fs::write(&fake_bin, "#!/bin/sh\nexit 0\n").unwrap();
        let mut permissions = std::fs::metadata(&fake_bin).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&fake_bin, permissions).unwrap();

        app.config.ai_hub.providers.clear();
        app.config.ai_hub.default_provider = Some("codex".to_string());
        app.config.ai_hub.default_model = Some("gpt-5.5".to_string());
        app.config.ai_hub.providers.insert(
            "codex".to_string(),
            AiProviderConfig {
                command: fake_bin.to_string_lossy().to_string(),
                args: vec!["exec".to_string(), "{prompt}".to_string()],
                models: vec![AiModelConfig {
                    id: "gpt-5.5".to_string(),
                    args: vec!["--model".to_string(), "gpt-5.5".to_string()],
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        app.current_ai_provider = Some("codex".to_string());
        app.current_ai_model = Some("gpt-5.5".to_string());

        app.spawn_background_triage_review(target(&tmp, "feat-codex"), "p".into(), true)
            .unwrap();

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            app.poll_background_tasks();
            let done = app
                .background_tasks
                .values()
                .any(|handle| matches!(handle.task.status, crate::app::CommandStatus::Done));
            if done {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "fake Codex task did not finish"
            );
            std::thread::sleep(std::time::Duration::from_millis(25));
        }

        let debug_log = std::fs::read_to_string(tmp.join(".er/debug-agent.log")).unwrap();
        // Assert the flag rather than its position: other flags (e.g. `--add-dir`)
        // may be injected ahead of it, and the claim -- Codex is told not to
        // read the user's config -- holds regardless of order.
        let command_line = debug_log
            .lines()
            .find(|l| l.starts_with("command: "))
            .unwrap_or("");
        assert!(
            command_line.contains("exec") && command_line.contains("--ignore-user-config"),
            "debug log should show an isolated Codex invocation:\n{debug_log}"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn background_task_ids_are_unique_for_same_kind_and_target() {
        let target = target(std::path::Path::new("/repo"), "main");
        let first = super::super::background::BackgroundTask::new("triage".into(), target.clone());
        let second = super::super::background::BackgroundTask::new("triage".into(), target);
        assert_ne!(first.id, second.id);
        assert!(first.id.contains(&std::process::id().to_string()));
    }

    #[test]
    fn registered_completion_is_exact_one_shot_and_survives_history_retirement() {
        let _serial = serial();
        let Some((mut app, tmp)) = test_app(2) else {
            return;
        };
        app.config.ai_hub = Default::default();
        app.config.agent.command = "sh".into();
        app.config.agent.args = vec!["-c".into(), "sleep 0.05".into()];
        let owned_target = target(&tmp, "owned");
        let unowned_target = target(&tmp, "unowned");
        let owned_id = app
            .enqueue_background_agent_task(
                "triage".into(),
                "triage",
                owned_target.clone(),
                "p".into(),
                true,
                None,
                None,
            )
            .unwrap();
        app.register_review_flow_task(owned_id.clone());
        let unowned_id = app
            .enqueue_background_agent_task(
                "triage".into(),
                "triage",
                unowned_target,
                "p".into(),
                true,
                None,
                None,
            )
            .unwrap();

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !app.flow_task_completions.contains_key(&owned_id) {
            app.poll_background_tasks();
            assert!(
                std::time::Instant::now() < deadline,
                "owned task did not complete"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(!app.flow_task_completions.contains_key(&unowned_id));
        let handle = app.background_tasks.get_mut(&owned_id).unwrap();
        handle.task.finished_at_ms =
            Some(super::super::background::unix_now_ms().saturating_sub(9_000));
        app.poll_background_tasks();
        assert!(
            !app.background_tasks.contains_key(&owned_id),
            "UI task history retired"
        );
        assert!(
            app.flow_task_completions.contains_key(&owned_id),
            "flow completion is independent of UI history"
        );

        let completion = app.take_review_flow_task_completion(&owned_id).unwrap();
        assert_eq!(completion.id, owned_id);
        assert_eq!(completion.target, owned_target);
        assert!(matches!(completion.status, crate::app::CommandStatus::Done));
        assert!(app
            .take_review_flow_task_completion(&completion.id)
            .is_none());
        assert!(!app.owned_flow_task_ids.contains(&completion.id));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn background_progress_callback_wakes_after_success_and_failure_results() {
        let _serial = serial();
        let Some((mut app, tmp)) = test_app(2) else {
            return;
        };
        app.config.ai_hub = Default::default();
        app.config.ai_hub.max_concurrent_reviews = 2;
        app.config.agent.command = "sh".into();
        let wake_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let callback_count = std::sync::Arc::clone(&wake_count);
        app.set_background_progress_notify(Some(std::sync::Arc::new(move || {
            callback_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        })));

        app.config.agent.args = vec!["-c".into(), "exit 0".into()];
        app.spawn_background_review(target(&tmp, "callback-success"), "p".into(), false)
            .unwrap();
        app.config.agent.args = vec!["-c".into(), "exit 1".into()];
        app.spawn_background_review(target(&tmp, "callback-failure"), "p".into(), false)
            .unwrap();

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while wake_count.load(std::sync::atomic::Ordering::SeqCst) < 2 {
            app.poll_background_tasks();
            assert!(
                std::time::Instant::now() < deadline,
                "worker did not wake the host"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        app.poll_background_tasks();
        assert_eq!(wake_count.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert!(app
            .background_tasks
            .values()
            .any(|task| matches!(task.task.status, crate::app::CommandStatus::Done)));
        assert!(app
            .background_tasks
            .values()
            .any(|task| matches!(task.task.status, crate::app::CommandStatus::Failed(_))));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn worker_panic_drops_result_sender_before_progress_callback() {
        let (result_tx, result_rx) = std::sync::mpsc::channel::<anyhow::Result<()>>();
        let result_rx = std::sync::Mutex::new(result_rx);
        let saw_terminal_channel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let callback_observed = std::sync::Arc::clone(&saw_terminal_channel);
        let notify: crate::arena::ArenaNotify = std::sync::Arc::new(move || {
            let disconnected = matches!(
                result_rx.lock().unwrap().try_recv(),
                Err(std::sync::mpsc::TryRecvError::Disconnected)
            );
            callback_observed.store(disconnected, std::sync::atomic::Ordering::SeqCst);
        });
        let worker = std::thread::spawn(move || {
            let _progress_signal = super::NotifyBackgroundProgressOnDrop {
                result_tx: Some(result_tx),
                notify: Some(notify),
            };
            panic!("simulated worker-body panic");
        });

        assert!(worker.join().is_err());
        assert!(
            saw_terminal_channel.load(std::sync::atomic::Ordering::SeqCst),
            "callback must observe a sent result or disconnected sender"
        );
    }

    #[test]
    fn queued_launch_failure_is_delivered_for_registered_id() {
        let _serial = serial();
        let Some((mut app, tmp)) = test_app(1) else {
            return;
        };
        app.config.ai_hub = Default::default();
        app.config.ai_hub.max_concurrent_reviews = 1;
        let blocker_target = target(&tmp, "blocker");
        let mut blocker_task =
            super::super::background::BackgroundTask::new("review".into(), blocker_target);
        blocker_task.status = crate::app::CommandStatus::Running;
        let blocker_id = blocker_task.id.clone();
        let (_result_tx, result_rx) = std::sync::mpsc::channel();
        let (_log_tx, log_rx) = std::sync::mpsc::channel();
        app.background_tasks.insert(
            blocker_id.clone(),
            super::super::background::BackgroundTaskHandle {
                task: blocker_task,
                run: crate::agent_run::AgentRunHandle::new(),
                result_rx,
                log_rx,
                recent_log: Default::default(),
            },
        );

        let obstructed_path = tmp.join("not-a-directory");
        std::fs::write(&obstructed_path, "file").unwrap();
        let mut failed_target = target(&tmp, "queued-failure");
        failed_target.er_dir = obstructed_path
            .join("bucket")
            .to_string_lossy()
            .into_owned();
        let id = app
            .enqueue_background_agent_task(
                "triage".into(),
                "triage",
                failed_target,
                "p".into(),
                true,
                None,
                Some(ErConfig::default()),
            )
            .unwrap();
        app.register_review_flow_task(id.clone());
        // A different live cap can release queued work; captured launch
        // configuration must not freeze this resource policy.
        app.config.ai_hub.max_concurrent_reviews = 16;
        app.background_tasks
            .get_mut(&blocker_id)
            .unwrap()
            .task
            .status = crate::app::CommandStatus::Done;
        app.dispatch_pending_background_tasks();
        assert!(app.flow_task_completions.contains_key(&id));
        let completion = app.take_review_flow_task_completion(&id).unwrap();
        assert_eq!(completion.id, id);
        assert!(matches!(
            completion.status,
            crate::app::CommandStatus::Failed(_)
        ));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn public_spawn_wrapper_refuses_a_leased_review_bucket() {
        let _serial = serial();
        let Some((mut app, tmp)) = test_app(1) else {
            return;
        };
        let target = target(&tmp, "leased");
        let lease = app.acquire_review_flow_lease(&target.er_dir).unwrap();

        let error = app
            .spawn_background_review(target.clone(), "p".into(), true)
            .unwrap_err();
        assert!(error.to_string().contains("review flow is already using"));
        assert!(app.pending_background_tasks.is_empty());
        assert!(app.background_tasks.is_empty());

        app.release_review_flow_lease(&lease).unwrap();
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn cancelling_queued_flow_task_delivers_failure_and_cleans_ownership() {
        let _serial = serial();
        let Some((mut app, tmp)) = test_app(1) else {
            return;
        };
        app.config.ai_hub.max_concurrent_reviews = 1;
        let blocker_task =
            super::super::background::BackgroundTask::new("review".into(), target(&tmp, "blocker"));
        let blocker_id = blocker_task.id.clone();
        let (_result_tx, result_rx) = std::sync::mpsc::channel();
        let (_log_tx, log_rx) = std::sync::mpsc::channel();
        app.background_tasks.insert(
            blocker_id,
            super::super::background::BackgroundTaskHandle {
                task: blocker_task,
                run: crate::agent_run::AgentRunHandle::new(),
                result_rx,
                log_rx,
                recent_log: Default::default(),
            },
        );

        let flow_id = app
            .enqueue_background_agent_task(
                "triage".into(),
                "triage",
                target(&tmp, "flow-queued"),
                "p".into(),
                true,
                None,
                None,
            )
            .unwrap();
        app.register_review_flow_task(flow_id.clone());
        let legacy_id = app
            .enqueue_background_agent_task(
                "triage".into(),
                "triage",
                target(&tmp, "legacy-queued"),
                "p".into(),
                true,
                None,
                None,
            )
            .unwrap();

        assert!(app.cancel_queued_background_task(&legacy_id));
        assert!(!app.flow_task_completions.contains_key(&legacy_id));
        assert!(app.cancel_queued_background_task(&flow_id));
        let completion = app.take_review_flow_task_completion(&flow_id).unwrap();
        assert_eq!(completion.id, flow_id);
        assert_eq!(completion.error.as_deref(), Some("cancelled before launch"));
        assert!(matches!(
            completion.status,
            crate::app::CommandStatus::Failed(ref reason) if reason == "cancelled before launch"
        ));
        assert!(!app.owned_flow_task_ids.contains(&completion.id));
        assert!(app
            .take_review_flow_task_completion(&completion.id)
            .is_none());
        assert!(app.pending_background_tasks.is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    #[cfg(unix)]
    fn queued_flow_task_launches_from_frozen_provider_config() {
        use std::os::unix::fs::PermissionsExt;

        let _serial = serial();
        let Some((mut app, tmp)) = test_app(1) else {
            return;
        };
        app.config.ai_hub = Default::default();
        app.config.ai_hub.max_concurrent_reviews = 1;
        app.config.agent.command = "sleep".into();
        app.config.agent.args = vec!["30".into()];
        app.spawn_background_triage_review(target(&tmp, "blocker"), "p".into(), true)
            .unwrap();

        let argv_path = tmp.join("frozen-argv.txt");
        let frozen_command = tmp.join("frozen-agent");
        std::fs::write(
            &frozen_command,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$0\" \"$@\" > '{}'\nexit 0\n",
                argv_path.display()
            ),
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&frozen_command).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&frozen_command, permissions).unwrap();

        let mut frozen = ErConfig::default();
        frozen.ai_hub.default_provider = Some("frozen".into());
        frozen.ai_hub.default_model = Some("frozen-model".into());
        frozen.ai_hub.default_effort = Some("high".into());
        frozen.ai_hub.agent_timeout_secs = 3;
        frozen.ai_hub.providers.insert(
            "frozen".into(),
            AiProviderConfig {
                command: frozen_command.to_string_lossy().into_owned(),
                family: Some("claude".into()),
                args: vec!["{prompt}".into()],
                models: vec![AiModelConfig {
                    id: "frozen-model".into(),
                    args: vec!["--model".into(), "frozen-model".into()],
                    effort_levels: vec!["low".into(), "high".into()],
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        let id = app
            .enqueue_background_agent_task(
                "general".into(),
                "review",
                target(&tmp, "frozen-flow"),
                "prompt".into(),
                true,
                None,
                Some(frozen.clone()),
            )
            .unwrap();
        app.register_review_flow_task(id.clone());
        assert_eq!(
            app.pending_background_tasks[0]
                .launch_config
                .as_ref()
                .unwrap()
                .ai_hub
                .agent_timeout_secs,
            3
        );

        app.config.agent.command = tmp
            .join("live-config-must-not-run")
            .to_string_lossy()
            .into_owned();
        app.config.ai_hub = Default::default();
        app.config.ai_hub.default_provider = Some("live".into());
        app.config.ai_hub.providers.insert(
            "live".into(),
            AiProviderConfig {
                command: tmp
                    .join("live-config-must-not-run")
                    .to_string_lossy()
                    .into_owned(),
                ..Default::default()
            },
        );
        app.background_tasks
            .values()
            .find(|h| h.task.target.branch_label == "blocker")
            .unwrap()
            .run
            .kill();

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !app.flow_task_completions.contains_key(&id) {
            app.poll_background_tasks();
            assert!(
                std::time::Instant::now() < deadline,
                "frozen provider did not complete"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let argv = std::fs::read_to_string(&argv_path).unwrap();
        assert!(
            argv.contains("frozen-model"),
            "model args must come from captured config: {argv}"
        );
        assert!(
            argv.contains("--effort\nhigh"),
            "effort must come from captured config: {argv}"
        );
        assert!(!argv.contains("live-config-must-not-run"));
        assert!(matches!(
            app.take_review_flow_task_completion(&id).unwrap().status,
            crate::app::CommandStatus::Done
        ));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// One spawn case for the startup-trim tests: a fake CLI whose basename
    /// sets the family, the provider args it is configured with, and the file
    /// it writes its argv to, one argument per line.
    #[cfg(unix)]
    struct ArgvCase {
        provider_id: String,
        argv_file: std::path::PathBuf,
    }

    /// Make `provider_id` a provider whose command is a script named `cli`
    /// that records the argv it was given. The rename into place means a
    /// reader never sees a half-written file.
    #[cfg(unix)]
    fn install_argv_recorder(
        app: &mut App,
        tmp: &std::path::Path,
        provider_id: &str,
        cli: &str,
        args: &[&str],
    ) -> ArgvCase {
        use std::os::unix::fs::PermissionsExt;
        let dir = tmp.join(format!("bin-{provider_id}"));
        std::fs::create_dir_all(&dir).unwrap();
        let argv_file = tmp.join(format!("argv-{provider_id}.txt"));
        let bin = dir.join(cli);
        let out = argv_file.display();
        std::fs::write(
            &bin,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"{out}.tmp\" && mv \"{out}.tmp\" \"{out}\"\n"
            ),
        )
        .unwrap();
        let mut perms = std::fs::metadata(&bin).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&bin, perms).unwrap();
        app.config.ai_hub.providers.insert(
            provider_id.to_string(),
            AiProviderConfig {
                command: bin.to_string_lossy().to_string(),
                args: args.iter().map(ToString::to_string).collect(),
                models: vec![AiModelConfig {
                    id: "m".to_string(),
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        ArgvCase {
            provider_id: provider_id.to_string(),
            argv_file,
        }
    }

    #[cfg(unix)]
    fn select(app: &mut App, case: &ArgvCase) {
        app.set_ai_selection_override(AiSelection {
            provider_id: Some(case.provider_id.clone()),
            model_id: Some("m".to_string()),
            effort: None,
        });
    }

    /// The three startup-trim cases every Claude argv builder must satisfy:
    /// both flags added for Claude, a flag the user already set not repeated,
    /// and nothing added for Codex.
    #[cfg(unix)]
    fn startup_trim_cases(app: &mut App, tmp: &std::path::Path) -> [ArgvCase; 3] {
        app.config.ai_hub.providers.clear();
        [
            install_argv_recorder(app, tmp, "claude", "claude", &["--print", "-p", "{prompt}"]),
            install_argv_recorder(
                app,
                tmp,
                "claude-strict",
                "claude",
                &["--strict-mcp-config", "--print", "-p", "{prompt}"],
            ),
            install_argv_recorder(app, tmp, "codex", "codex", &["exec", "{prompt}"]),
        ]
    }

    /// Wait for each case's argv file, polling `poll` meanwhile, and return
    /// how often each case saw `--strict-mcp-config` and
    /// `--disable-slash-commands`.
    #[cfg(unix)]
    fn startup_flag_counts(cases: &[ArgvCase], mut poll: impl FnMut()) -> Vec<(usize, usize)> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !cases.iter().all(|c| c.argv_file.exists()) {
            assert!(
                std::time::Instant::now() < deadline,
                "a fake agent never recorded its argv"
            );
            poll();
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        cases
            .iter()
            .map(|case| {
                let argv = std::fs::read_to_string(&case.argv_file).unwrap();
                let count = |flag: &str| argv.lines().filter(|l| *l == flag).count();
                (
                    count("--strict-mcp-config"),
                    count("--disable-slash-commands"),
                )
            })
            .collect()
    }

    #[test]
    #[cfg(unix)]
    fn foreground_agent_prompt_trims_claude_startup_only() {
        let _serial = serial();
        // `spawn_agent_prompt` writes into the tab's managed bucket, so keep
        // that bucket out of the user's real storage.
        let _storage = crate::storage::STORAGE_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let storage = tempfile::TempDir::new().unwrap();
        std::env::set_var("ER_STORAGE_ROOT", storage.path());
        let Some((mut app, tmp)) = test_app(3) else {
            return;
        };
        let cases = startup_trim_cases(&mut app, &tmp);

        for case in &cases {
            select(&mut app, case);
            app.spawn_agent_prompt(&format!("trim-{}", case.provider_id), "p")
                .unwrap();
        }
        app.clear_ai_selection_override();

        assert_eq!(
            startup_flag_counts(&cases, || {}),
            vec![(1, 1), (1, 1), (0, 0)],
            "per case (claude, claude with the flag preset, codex): \
             (--strict-mcp-config, --disable-slash-commands) counts"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    #[cfg(unix)]
    fn background_agent_task_trims_claude_startup_only() {
        let _serial = serial();
        let Some((mut app, tmp)) = test_app(3) else {
            return;
        };
        let cases = startup_trim_cases(&mut app, &tmp);

        for case in &cases {
            select(&mut app, case);
            app.spawn_background_triage_review(target(&tmp, &case.provider_id), "p".into(), true)
                .unwrap();
        }
        app.clear_ai_selection_override();

        assert_eq!(
            startup_flag_counts(&cases, || app.poll_background_tasks()),
            vec![(1, 1), (1, 1), (0, 0)],
            "per case (claude, claude with the flag preset, codex): \
             (--strict-mcp-config, --disable-slash-commands) counts"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    #[cfg(unix)]
    fn importance_launch_uses_read_only_builtin_arguments_and_environment() {
        use std::os::unix::fs::PermissionsExt;

        let _serial = serial();
        let Some((mut app, tmp)) = test_app(1) else {
            return;
        };
        app.config.ai_hub.providers.clear();
        let args_path = tmp.join("importance-argv");
        let env_path = tmp.join("importance-env");

        for (family, base_args) in [
            ("claude", vec!["--print", "{prompt}"]),
            ("codex", vec!["exec", "{prompt}"]),
            ("cursor", vec!["{prompt}"]),
            ("opencode", vec!["run", "{prompt}"]),
        ] {
            let command = tmp.join(family);
            let script = format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nprintf '%s' \"${{OPENCODE_PERMISSION-}}\" > '{}'\nexit 23\n",
                args_path.display(),
                env_path.display()
            );
            std::fs::write(&command, script).unwrap();
            let mut permissions = std::fs::metadata(&command).unwrap().permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&command, permissions).unwrap();

            let model = AiModelConfig {
                id: "test-model".into(),
                effort_levels: if family == "codex" {
                    vec!["high".into()]
                } else {
                    vec![]
                },
                ..Default::default()
            };
            app.config.ai_hub.providers.insert(
                family.into(),
                AiProviderConfig {
                    command: command.to_string_lossy().into_owned(),
                    family: Some(family.into()),
                    args: base_args.iter().map(|arg| (*arg).to_string()).collect(),
                    models: vec![model],
                    ..Default::default()
                },
            );
            app.current_ai_provider = Some(family.into());
            app.current_ai_model = Some("test-model".into());
            app.current_ai_effort = (family == "codex").then(|| "high".into());

            let old_ids: std::collections::HashSet<_> =
                app.background_tasks.keys().cloned().collect();
            app.spawn_background_agent_task(
                crate::ai::prompts::IMPORTANCE_TASK_KIND.to_string(),
                "importance",
                target(&tmp, "importance-test"),
                "fake importance prompt".into(),
                false,
                None,
            )
            .unwrap();
            let new_id = app
                .background_tasks
                .keys()
                .find(|id| !old_ids.contains(*id))
                .cloned()
                .expect("importance task was launched");
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            loop {
                app.poll_background_tasks();
                if app.background_tasks.get(&new_id).is_some_and(|handle| {
                    matches!(handle.task.status, crate::app::CommandStatus::Failed(_))
                }) {
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "fake {family} did not finish"
                );
                std::thread::sleep(std::time::Duration::from_millis(25));
            }

            let argv = std::fs::read_to_string(&args_path).unwrap();
            match family {
                "claude" => {
                    // Inherited allow rules for Write/Edit/Bash cannot restore tools
                    // removed from the actual tool set by --tools.
                    assert!(argv.contains("--tools\nRead,Glob,Grep\n"), "{argv}");
                    assert_eq!(argv.lines().filter(|arg| *arg == "--tools").count(), 1);
                    assert!(argv.contains("--disallowedTools\nmcp__*\n"), "{argv}");
                    assert!(argv.contains("--allowedTools\nRead\n"), "{argv}");
                    assert!(!argv.contains("--allowedTools\nWrite\n"), "{argv}");
                    assert!(
                        argv.contains("--disallowedTools\nBash(git clone*)"),
                        "{argv}"
                    );
                }
                "codex" => {
                    assert!(argv.contains("--sandbox\nread-only"), "{argv}");
                    assert!(argv.contains("-c\nmodel_reasoning_effort=high"), "{argv}");
                    assert!(!argv.contains("--add-dir"), "{argv}");
                }
                "cursor" => {
                    assert!(argv.contains("--mode\nplan"), "{argv}");
                    assert!(!argv.contains("--add-dir"), "{argv}");
                }
                "opencode" => {
                    assert!(argv.contains("--auto"), "{argv}");
                    assert!(!std::fs::read_to_string(&env_path).unwrap().is_empty());
                    let env: serde_json::Value =
                        serde_json::from_str(&std::fs::read_to_string(&env_path).unwrap()).unwrap();
                    assert_eq!(env["edit"], "deny");
                    assert_eq!(env["bash"]["*"], "deny");
                }
                _ => unreachable!(),
            }
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    #[cfg(unix)]
    fn importance_rejects_configured_permission_conflicts_before_launch() {
        use std::os::unix::fs::PermissionsExt;

        let _serial = serial();
        let Some((mut app, tmp)) = test_app(1) else {
            return;
        };
        app.config.ai_hub.providers.clear();
        for (family, bad_args) in [
            (
                "codex",
                vec!["exec", "-csandbox_mode=workspace-write", "{prompt}"],
            ),
            ("codex", vec!["exec", "-sunsafe", "{prompt}"]),
            (
                "codex",
                vec!["exec", "--sandbox=workspace-write", "{prompt}"],
            ),
            (
                "claude",
                vec!["--permission-mode", "bypassPermissions", "{prompt}"],
            ),
            ("claude", vec!["--tools", "default", "{prompt}"]),
            ("claude", vec!["--tools=Write,Bash", "{prompt}"]),
            ("cursor", vec!["--mode=ask", "{prompt}"]),
            ("opencode", vec!["run", "--permission", "allow", "{prompt}"]),
        ] {
            let command = tmp.join(family);
            std::fs::write(&command, "#!/bin/sh\nexit 23\n").unwrap();
            let mut permissions = std::fs::metadata(&command).unwrap().permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&command, permissions).unwrap();
            app.config.ai_hub.providers.insert(
                family.into(),
                AiProviderConfig {
                    command: command.to_string_lossy().into_owned(),
                    family: Some(family.into()),
                    args: bad_args.iter().map(|arg| (*arg).to_string()).collect(),
                    models: vec![AiModelConfig {
                        id: "test-model".into(),
                        effort_levels: if family == "codex" {
                            vec!["high".into()]
                        } else {
                            vec![]
                        },
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            );
            app.current_ai_provider = Some(family.into());
            app.current_ai_model = Some("test-model".into());
            app.current_ai_effort = Some("high".into());
            assert!(app
                .spawn_background_agent_task(
                    crate::ai::prompts::IMPORTANCE_TASK_KIND.to_string(),
                    "importance",
                    target(&tmp, family),
                    "fake importance prompt".into(),
                    false,
                    None,
                )
                .is_err());
        }
        assert!(!tmp.join(".er/debug-agent.log").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn agent_command_artifact_reload_classification() {
        for name in [
            "summary",
            "review",
            "questions",
            "triage",
            "professor",
            "expert-security",
            "validate",
            "validate-comments",
        ] {
            assert!(
                App::agent_command_writes_ai_artifacts(name),
                "{name} should trigger AI sidecar reload"
            );
        }

        for name in ["test", "lint", "fmt", "build"] {
            assert!(
                !App::agent_command_writes_ai_artifacts(name),
                "{name} should not trigger AI sidecar reload"
            );
        }
    }

    /// Edit and delete keep the in-memory thread list in step with the sidecar
    /// they just wrote, without `reload_ai_state()`. A reload would re-read
    /// every sidecar (review.json, experts, tour, summary…) to learn about a
    /// change this process made — that round trip was the cost of every
    /// delete/edit click. `summary.md` is the canary: only a full reload
    /// would pick it up.
    #[test]
    fn thread_edit_and_delete_update_memory_without_reloading_every_sidecar() {
        let _guard = crate::storage::STORAGE_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::TempDir::new().unwrap();
        std::env::set_var("ER_STORAGE_ROOT", tmp.path());

        let mut app = App::new_for_test(vec![]);
        {
            let tab = app.tab_mut();
            tab.repo_root = "/home/user/my-project".to_string();
            tab.current_branch = "main".to_string();
            tab.apply_managed_root();
        }
        let er_dir = app.tab().er_dir();
        std::fs::create_dir_all(&er_dir).unwrap();
        let q_path = format!("{er_dir}/questions.json");
        std::fs::write(
            &q_path,
            r#"{"version":1,"diff_hash":"h","questions":[
                {"id":"q-1","file":"x","hunk_index":0,"line_start":1,"line_content":"c","text":"pending","resolved":false},
                {"id":"q-2","file":"x","hunk_index":0,"line_start":2,"line_content":"c","text":"other","resolved":false}
            ]}"#,
        )
        .unwrap();
        app.tab_mut().reload_ai_state();
        assert_eq!(
            app.tab().ai.questions.as_ref().map(|q| q.questions.len()),
            Some(2)
        );

        // Written after the reload: visible only to a full `reload_ai_state()`.
        std::fs::write(format!("{er_dir}/summary.md"), "# summary").unwrap();

        app.update_comment_text("q-1", "edited").unwrap();
        let edited = app
            .tab()
            .ai
            .questions
            .as_ref()
            .and_then(|qs| qs.questions.iter().find(|q| q.id == "q-1"))
            .map(|q| q.text.clone());
        assert_eq!(edited.as_deref(), Some("edited"), "edit visible in memory");
        assert!(
            app.tab().ai.summary.is_none(),
            "edit must not re-read unrelated sidecars"
        );

        app.confirm_delete_comment("q-1").unwrap();
        let ids: Vec<String> = app
            .tab()
            .ai
            .questions
            .as_ref()
            .map(|qs| qs.questions.iter().map(|q| q.id.clone()).collect())
            .unwrap_or_default();
        assert_eq!(ids, vec!["q-2".to_string()], "delete visible in memory");
        assert!(
            app.tab().ai.summary.is_none(),
            "delete must not re-read unrelated sidecars"
        );
        let on_disk = std::fs::read_to_string(&q_path).unwrap();
        assert!(!on_disk.contains("\"q-1\"") && on_disk.contains("\"q-2\""));

        std::env::remove_var("ER_STORAGE_ROOT");
    }
}

/// The checklist toggle is the one write both front ends share: the TUI drives
/// it from a cursor, the desktop from an index in the snapshot. These pin the
/// split — the shared half persists and reports state with no navigation
/// precondition, the TUI half keeps the focus guard.
#[cfg(test)]
mod checklist_toggle_tests {
    use crate::ai::{ChecklistItem, ErChecklist, ReviewFocus};
    use crate::app::App;
    use crate::paths::ErRoot;

    /// An app whose view bucket is a throwaway directory: the toggle writes,
    /// so the test needs a real er_dir it owns.
    fn app_with_checklist(tmp: &tempfile::TempDir) -> App {
        let mut app = App::new_for_test(vec![]);
        let tab = app.tab_mut();
        tab.er_root = ErRoot::RepoLocal(tmp.path().to_string_lossy().to_string());
        let er_dir = std::path::PathBuf::from(tab.er_dir());
        std::fs::create_dir_all(&er_dir).unwrap();
        std::fs::write(
            er_dir.join("checklist.json"),
            r#"{"version":1,"diff_hash":"h","items":[
                {"id":"c-1","text":"Schema migration adds a NOT NULL column","category":"schema","checked":false,"related_findings":["f-1"],"related_files":["db/m.sql"]},
                {"id":"c-2","text":"Retry path fails the test suite it should not","category":"tests","checked":true,"related_findings":[],"related_files":[]}
            ]}"#,
        )
        .unwrap();
        tab.reload_ai_state();
        app
    }

    fn item(app: &App, index: usize) -> ChecklistItem {
        app.tab().ai.checklist.as_ref().unwrap().items[index].clone()
    }

    fn on_disk(tmp: &tempfile::TempDir) -> ErChecklist {
        let raw = std::fs::read_to_string(tmp.path().join(".er/checklist.json")).unwrap();
        serde_json::from_str(&raw).unwrap()
    }

    #[test]
    fn toggling_by_index_persists_and_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = app_with_checklist(&tmp);

        assert_eq!(app.toggle_checklist_item_at(0).unwrap(), Some(true));
        assert!(item(&app, 0).checked, "in-memory state follows the toggle");
        assert!(
            item(&app, 1).checked,
            "the neighbouring item keeps the state the fixture gave it"
        );
        assert!(
            on_disk(&tmp).items[0].checked,
            "the toggle was persisted, so the other front end reads it on its next poll"
        );

        // Toggling the same index back must land on false on both sides — the
        // state the desktop reads in its next snapshot comes from this file.
        assert_eq!(app.toggle_checklist_item_at(0).unwrap(), Some(false));
        assert!(!on_disk(&tmp).items[0].checked);
        assert!(!item(&app, 0).checked);
    }

    #[test]
    fn an_index_past_the_end_toggles_nothing_and_writes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = app_with_checklist(&tmp);
        let before = std::fs::read_to_string(tmp.path().join(".er/checklist.json")).unwrap();

        assert_eq!(app.toggle_checklist_item_at(9).unwrap(), None);
        assert!(!item(&app, 0).checked && item(&app, 1).checked);
        assert_eq!(
            std::fs::read_to_string(tmp.path().join(".er/checklist.json")).unwrap(),
            before
        );
    }

    #[test]
    fn toggling_with_no_checklist_reports_none_and_creates_no_file() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = App::new_for_test(vec![]);
        app.tab_mut().er_root = ErRoot::RepoLocal(tmp.path().to_string_lossy().to_string());
        std::fs::create_dir_all(tmp.path().join(".er")).unwrap();

        assert_eq!(app.toggle_checklist_item_at(0).unwrap(), None);
        assert!(!tmp.path().join(".er/checklist.json").exists());
    }

    /// Regression guard on the split: the shared function has no navigation
    /// precondition, so the guard has to survive in the wrapper.
    #[test]
    fn the_tui_wrapper_still_requires_checklist_focus() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = app_with_checklist(&tmp);
        app.tab_mut().review_cursor = 0;

        app.tab_mut().review_focus = ReviewFocus::Files;
        app.review_toggle_checklist().unwrap();
        assert!(
            !item(&app, 0).checked,
            "a toggle while the file column has focus must not reach the checklist"
        );
        assert!(!on_disk(&tmp).items[0].checked);

        app.tab_mut().review_focus = ReviewFocus::Checklist;
        app.review_toggle_checklist().unwrap();
        assert!(item(&app, 0).checked, "focused, it toggles the cursor item");
        assert!(on_disk(&tmp).items[0].checked);
    }
}

#[cfg(test)]
mod agent_stderr_snippet_tests {
    use super::agent_stderr_snippet;

    #[test]
    fn terminal_model_error_replaces_codex_startup_banner() {
        let stderr = vec![
            "OpenAI Codex (v0.1.0)".to_string(),
            "Starting interactive agent session".to_string(),
            "ERROR: model 'gpt-5.3' is not supported for this account".to_string(),
        ];

        let snippet = agent_stderr_snippet(&stderr);
        assert!(snippet.starts_with("ERROR: model 'gpt-5.3'"));
        assert!(!snippet.contains("Codex"));
    }

    #[test]
    fn cursor_authentication_error_keeps_login_instructions() {
        let stderr = vec![
            "Cursor Agent CLI starting".to_string(),
            "Authentication required".to_string(),
            "Run `cursor-agent login` to authenticate.".to_string(),
            "Then retry the review.".to_string(),
        ];

        let snippet = agent_stderr_snippet(&stderr);
        assert!(snippet.starts_with("Authentication required"));
        assert!(snippet.contains("cursor-agent login"));
        assert!(snippet.contains("retry the review"));
    }

    #[test]
    fn stderr_without_error_marker_uses_the_tail() {
        let stderr = vec![
            "startup banner".to_string(),
            "initializing agent".to_string(),
            "provider connection closed".to_string(),
        ];

        let snippet = agent_stderr_snippet(&stderr);
        assert!(snippet.starts_with("startup banner"));
        assert!(snippet.ends_with("provider connection closed"));
    }

    #[test]
    fn empty_and_unicode_stderr_are_bounded_by_characters() {
        assert_eq!(agent_stderr_snippet(&[]), "");

        let stderr = vec![format!("ERROR: {}", "é界".repeat(200))];
        let snippet = agent_stderr_snippet(&stderr);
        assert!(snippet.starts_with("ERROR:"));
        assert!(snippet.ends_with('…'));
        assert_eq!(snippet.chars().count(), 280);
    }

    #[test]
    fn long_fallback_keeps_the_unicode_tail_within_the_cap() {
        let stderr = vec![format!("banner {} final detail", "界".repeat(300))];
        let snippet = agent_stderr_snippet(&stderr);
        assert!(snippet.starts_with('…'));
        assert!(snippet.ends_with("final detail"));
        assert_eq!(snippet.chars().count(), 280);
    }
}
