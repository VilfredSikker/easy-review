//! The agent launcher for a tab command: the argv, tool allow-list, effort
//! and staging the tab commands share. Always compiled, so the App and a host
//! with no App launch a tab command the same way.

use crate::agent_log::{log_job_progress, AgentLogEntry, AgentLogSource};
use crate::job_commit::commit_error;
use crate::stage_job::{debug_agent_log_enabled, CLONE_DENY_RULE, DEBUG_LOG_FILE};
use anyhow::{Context, Result};
use er_agent::job::{JobError, JobProgress, JobRun, JobRunner, JobSpec, OutputContract};
use er_agent::provider::ProviderSpec;

/// Lines buffered ahead of the log drain. A stalled drain trips the provider
/// timeout rather than blocking the job.
pub(crate) const BACKGROUND_PROGRESS_CAPACITY: usize = 256;

/// Say which user edits beat the agent in a by-id merge.
///
/// A same-id conflict leaves the agent's rewrite looking successful, so this
/// log line is the only sign that part of it was not applied.
fn log_dropped_edits(
    log_tx: &std::sync::mpsc::Sender<AgentLogEntry>,
    name: &str,
    dropped: &[String],
) {
    if dropped.is_empty() {
        return;
    }
    let _ = log_tx.send(AgentLogEntry {
        timestamp: std::time::Instant::now(),
        command_name: std::sync::Arc::from(name),
        source: AgentLogSource::Status,
        text: format!(
            "{name}: kept your edits; the agent's changes to {} were dropped",
            dropped.join(", ")
        ),
    });
}

/// Read a registered text sidecar through the store. The raw text is what a
/// caller copies or posts, so it is not parsed.
pub fn stored_text(er_dir: &str, file: &str) -> Result<er_store::Loaded<String>> {
    let store = crate::store_registry::store();
    let scope = er_store::Scope::dir(er_dir)?;
    let file = er_store::RelPath::new(file)?;
    Ok(store.read_text(&scope, &file)?)
}

/// Push the summary file to the PR body. An unreadable summary is skipped, same
/// as an empty one.
fn push_summary_to_pr(github: &dyn er_github::GitHub, er_dir: &str, repo_root: &str) -> Result<()> {
    let summary = match stored_text(er_dir, "summary.md") {
        Ok(er_store::Loaded::Found(stored)) => stored.value,
        _ => String::new(),
    };
    if summary.trim().is_empty() {
        return Ok(());
    }
    github
        .edit_body(repo_root, &summary)
        .map_err(|error| anyhow::anyhow!("Failed to update PR body: {error}"))
}

/// Test seam: the job kinds the tab-local spawn paths submitted, in order.
#[cfg(any(test, feature = "test-support"))]
static SUBMITTED_TAB_JOB_KINDS: std::sync::Mutex<Vec<er_agent::job::JobKind>> =
    std::sync::Mutex::new(Vec::new());

/// Take the job kinds the tab-local spawn paths submitted since the last call.
#[cfg(any(test, feature = "test-support"))]
pub fn take_submitted_tab_job_kinds() -> Vec<er_agent::job::JobKind> {
    SUBMITTED_TAB_JOB_KINDS
        .lock()
        .map(|mut kinds| std::mem::take(&mut *kinds))
        .unwrap_or_default()
}

pub(crate) fn record_submitted_tab_job(kind: &er_agent::job::JobKind) {
    #[cfg(any(test, feature = "test-support"))]
    {
        if let Ok(mut kinds) = SUBMITTED_TAB_JOB_KINDS.lock() {
            kinds.push(kind.clone());
        }
    }
    #[cfg(not(any(test, feature = "test-support")))]
    {
        let _ = kind;
    }
}

/// What `launch_agent_prompt` runs: a tab command's name and prompt,
/// the tab's roots, the config and pick it runs with, and where its log
/// lines go.
pub struct AgentPromptRequest<'a> {
    pub name: &'a str,
    pub prompt: &'a str,
    pub repo_root: String,
    pub er_dir: String,
    pub is_remote: bool,
    pub remote_repo: Option<String>,
    pub config: &'a crate::config::ErConfig,
    pub selection: crate::config::AiSelection,
    pub ports: &'a crate::ports::Ports,
    pub log_tx: std::sync::mpsc::Sender<AgentLogEntry>,
}

/// A launched tab command: its stop handle and the receiver its outcome
/// arrives on.
pub struct AgentPromptRun {
    pub run: std::sync::Arc<er_agent::agent_run::AgentRunHandle>,
    pub result: std::sync::mpsc::Receiver<Result<()>>,
}

/// Launches the agent for the tab command `request.name` on its own
/// thread, with the argv, tool allow-list, effort and staging the tab
/// commands share, and commits its staged output when it succeeds.
/// Answers the run's stop handle and the receiver its outcome arrives on.
/// The App's `spawn_agent_prompt` calls it with its tab's inputs; a host
/// with no App calls it with a tab's published content.
pub fn launch_agent_prompt(request: AgentPromptRequest<'_>) -> Result<AgentPromptRun> {
    let AgentPromptRequest {
        name,
        prompt,
        repo_root,
        er_dir: er_dir_path,
        is_remote,
        remote_repo,
        config,
        selection,
        ports,
        log_tx,
    } = request;

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
    let effort_override = if name == "triage" { Some("low") } else { None };
    let effort = crate::config::resolve_effort_for_model(
        &config.ai_hub,
        &config.agent,
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
    // File-producing tab jobs write to staging; the host commits from it.
    let contract = crate::job_commit::tab_job_contract(name)?;
    let staging = match contract {
        crate::agent_runtime::ArtifactContract::None => None,
        _ => Some(std::sync::Arc::new(
            crate::job_staging::JobStaging::create()?
        )),
    };
    let storage_dir = staging
        .as_ref()
        .map_or_else(|| er_dir_path.clone(), |staging| staging.path());
    crate::config::inject_agent_storage_access(
        family,
        &mut config_args,
        Some(storage_dir.as_str()),
    );
    let opencode_env =
        crate::config::apply_opencode_spawn(family, &mut config_args, Some(storage_dir.as_str()));

    // Ensure .er/ directory exists
    std::fs::create_dir_all(std::path::Path::new(&er_dir_path))?;

    let name_owned: std::sync::Arc<str> = std::sync::Arc::from(name);
    let prompt_owned = match &staging {
        Some(staging) => staging.repoint_prompt(prompt, &er_dir_path),
        None => prompt.to_string(),
    };
    let push_to_pr = name == "summary" && config.summary.push_to_pr;
    let git_handle = ports.git.clone();
    let github_handle = ports.github.clone();

    // Send status log entry before spawning
    let _ = log_tx.send(AgentLogEntry {
        timestamp: std::time::Instant::now(),
        command_name: std::sync::Arc::from(name),
        source: AgentLogSource::Status,
        text: format!("{} started", name),
    });

    let artifact_store = crate::store_registry::store();
    let agent_timeout = config.ai_hub.effective_agent_timeout();
    let slot_cap = config.ai_hub.effective_max_concurrent_reviews();
    let ceiling = config.ai_hub.effective_max_concurrent_agents();
    let run = er_agent::agent_run::AgentRunHandle::new();
    let run_for_tab = std::sync::Arc::clone(&run);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut timer = er_agent::agent_timing::AgentRunTimer::start();
        let result = (|| -> Result<()> {
            let debug_path = std::path::Path::new(&er_dir_path).join(DEBUG_LOG_FILE);

            // The argv still carries `{prompt}`; the runner substitutes the
            // job prompt into it and waits for a slot, so `queue_ms` reads
            // the real queue wait.
            let mut agent_args: Vec<String> = config_args.clone();

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

            // Captured before the args move into the spec. The debug
            // transcript shows the argv the provider actually saw.
            let debug_args = debug_agent_log_enabled().then(|| {
                let mut args: Vec<String> = agent_args
                    .iter()
                    .map(|arg| arg.replace("{prompt}", &prompt_owned))
                    .collect();
                if !agent_args.iter().any(|arg| arg.contains("{prompt}"))
                    && !prompt_owned.is_empty()
                {
                    args.push(prompt_owned.clone());
                }
                args
            });

            // Remote mode has no repo around it, which is what sends the agent hunting
            // the filesystem for a copy of the code. When the local checkout *is* the
            // PR's repo, run there so verification reads hit real files; otherwise fall
            // back to the artifact dir. Output dirs are absolute either way.
            let remote_checkout = if is_remote {
                remote_repo.as_deref().and_then(|slug| {
                    github_handle.local_checkout_for_repo(&*git_handle, &repo_root, slug)
                })
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
            let spec = JobSpec {
                kind: crate::agent_runtime::AgentTaskKind::from_command_name(name_owned.as_ref())
                    .job_kind(),
                provider: ProviderSpec {
                    program: agent_cmd.clone(),
                    args: agent_args,
                    env: opencode_env
                        .iter()
                        .cloned()
                        .chain(
                            staging
                                .as_ref()
                                .map(|staging| ("ER_JOB_STAGING".to_string(), staging.path())),
                        )
                        .collect(),
                    protocol: if is_stream_json {
                        crate::agent_runtime::OutputProtocol::ClaudeStreamJson
                    } else {
                        crate::agent_runtime::OutputProtocol::Plain
                    },
                    timeout: agent_timeout,
                },
                prompt: prompt_owned,
                work_dir: work_dir.clone(),
                workload: er_agent::agent_slots::Workload::Background,
                output: OutputContract::Stdout,
            };
            record_submitted_tab_job(&spec.kind);

            // Stage the inputs the prompt reads, and record which output
            // files were absent so the commit can require them.
            let baseline = match staging.as_deref() {
                Some(staging) => {
                    staging
                        .stage_inputs(&er_dir_path)
                        .with_context(|| format!("{name_owned}: failed to stage its inputs"))?;
                    staging
                        .stage_update_inputs(
                            &er_dir_path,
                            crate::job_commit::update_inputs(contract.clone()),
                        )
                        .with_context(|| {
                            format!("{name_owned}: failed to stage its update inputs")
                        })?;
                    crate::job_commit::capture_contract(contract.clone(), &staging.path())?
                }
                None => None,
            };

            // Log lines are drained off the job thread, so a slow UI
            // cannot hold the runner's pipe readers.
            let (progress_tx, progress_rx) =
                std::sync::mpsc::sync_channel::<JobProgress>(BACKGROUND_PROGRESS_CAPACITY);
            let log_tx_drain = log_tx.clone();
            let name_out = name_owned.clone();
            let name_err = name_owned.clone();
            let drain = std::thread::spawn(move || {
                while let Ok(event) = progress_rx.recv() {
                    log_job_progress(event, is_stream_json, &name_out, &name_err, &log_tx_drain);
                }
            });

            let runner = JobRunner::new(slot_cap, ceiling);
            let outcome = runner.run_controlled(
                &spec,
                JobRun {
                    handle: std::sync::Arc::clone(&run),
                    progress: Some(progress_tx),
                    timer: Some(&mut timer),
                },
            );
            let _ = drain.join();
            let output = match outcome {
                Ok(output) => output,
                Err(JobError::TimedOut(limit)) => {
                    return Err(er_agent::agent_run::timed_out(limit));
                }
                Err(JobError::Cancelled) => {
                    return Err(er_agent::agent_run::cancelled());
                }
                Err(JobError::Spawn(error)) => {
                    return Err(error)
                        .with_context(|| format!("Failed to run {} ({})", name_owned, agent_cmd));
                }
            };
            let stdout_lines: Vec<String> = output.stdout.lines().map(str::to_string).collect();
            let stderr_lines: Vec<String> = output.stderr.lines().map(str::to_string).collect();
            let status = output.status;

            // Full transcript with accumulated stdout + stderr, opt-in.
            if let Some(debug_args) = &debug_args {
                let debug_content = format!(
                    "=== {} agent command ===\ncommand: {} {}\nexit code: {}\n\n--- stdout ---\n{}\n\n--- stderr ---\n{}\n",
                    name_owned,
                    agent_cmd,
                    debug_args.join(" "),
                    status.code().map_or_else(|| "signal".to_string(), |c| c.to_string()),
                    stdout_lines.join("\n"),
                    stderr_lines.join("\n"),
                );
                let _ = std::fs::write(&debug_path, &debug_content);
            }

            if !status.success() {
                // Only point at the transcript when there is one to read.
                let detail = if debug_agent_log_enabled() {
                    format!(" (full transcript in {er_dir_path}/{})", DEBUG_LOG_FILE)
                } else {
                    " (set ER_DEBUG=1 for the full transcript)".to_string()
                };
                anyhow::bail!("{} failed{detail}", name_owned);
            }

            // Commit the staged output through the store before anything
            // reads the bucket's copy.
            if let Some(staging) = staging.as_deref() {
                if contract != crate::agent_runtime::ArtifactContract::None {
                    let report = crate::job_commit::commit_contract(
                        contract,
                        &staging.path(),
                        &er_dir_path,
                        baseline.as_ref(),
                        &artifact_store,
                    )
                    .map_err(|error| commit_error(&name_owned, error))?;
                    // A same-id conflict keeps the user's copy; say so,
                    // since the agent's rewrite looked successful.
                    log_dropped_edits(&log_tx, &name_owned, &report.dropped);
                }
            }

            // The summary is a built-in job; a configured push still runs
            // once it succeeds.
            if push_to_pr {
                push_summary_to_pr(&*github_handle, &er_dir_path, &repo_root)?;
            }

            Ok(())
        })();
        let ok = result.is_ok();
        // Completion is the caller's signal that staging is gone, so remove
        // it before the message: the receiver may check the directory the
        // moment the status changes.
        drop(staging);
        let _ = tx.send(result);
        timer.emit("tab_command", &name_owned, ok);
    });

    Ok(AgentPromptRun {
        run: run_for_tab,
        result: rx,
    })
}
