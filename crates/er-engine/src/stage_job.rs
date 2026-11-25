//! One background stage job, from provider choice to committed artifacts.
//!
//! Triage, finder and lens jobs launch through here whichever host runs them,
//! so the launch rules exist once: provider args and effort, the read-only and
//! storage-access policy, the importance guards, staging, and the validate and
//! commit after the run. The host keeps what surrounds a job (its queue, log
//! lines and progress) and names the [`JobSink`] the job runs through.
//!
//! A launch has two halves. [`StageJob::prepare`] resolves and checks the argv
//! on the launching thread, so a refused launch fails before the host accepts
//! the task. [`StageJob::run`] stages the inputs, submits the job and commits
//! its output, on a thread that may block.

use std::sync::mpsc::SyncSender;
use std::sync::Arc;

use anyhow::{Context, Result};
use er_agent::agent_run::AgentRunHandle;
use er_agent::job::{JobError, JobProgress, JobSpec, OutputContract};
use er_agent::provider::ProviderSpec;
use er_store::ArtifactStore;

use crate::ai::ScopedRunStamp;
use crate::config::{AiSelection, CliFamily, ErConfig};
use crate::job_staging::JobStaging;
use crate::jobs::{JobLimits, JobSink, JobSubmission};
use crate::ports::Ports;
use crate::task_target::BackgroundTaskTarget;

/// The opt-in full agent transcript. Not a registered kind, so it is written
/// directly rather than through the store.
pub(crate) const DEBUG_LOG_FILE: &str = "debug-agent.log";

/// Host-owned diagram write target — agent emits JSON; Easy Review persists it.
#[derive(Debug, Clone)]
pub struct HostWriteDiagram {
    pub output_path: std::path::PathBuf,
    pub kind: String,
    pub diff_hash: String,
    pub custom_prompt: Option<String>,
}

/// Give a file-writing kind its staging directory and re-point its prompt
/// there. Kinds whose output is stdout only keep the prompt as it is.
pub fn staging_for(
    kind: &str,
    prompt: String,
    er_dir: &str,
) -> Result<(Option<Arc<JobStaging>>, String)> {
    let staging = match crate::job_commit::job_contract(kind) {
        crate::agent_runtime::ArtifactContract::None => None,
        _ => Some(Arc::new(JobStaging::create()?)),
    };
    let prompt = match &staging {
        Some(staging) => staging.repoint_prompt(&prompt, er_dir),
        None => prompt,
    };
    Ok((staging, prompt))
}

/// One accepted stage task, as the host hands it over.
pub struct StageJobRequest<'a> {
    pub kind: &'a str,
    pub command_name: &'a str,
    pub target: &'a BackgroundTaskTarget,
    /// The checkout a flow captured, separate from the target's identity.
    pub execution_root: Option<String>,
    /// Already re-pointed at `staging` by [`staging_for`].
    pub prompt: String,
    pub prepared_diff: bool,
    pub host_write_diagram: Option<HostWriteDiagram>,
    pub scoped_stamp: Option<ScopedRunStamp>,
    pub staging: Option<Arc<JobStaging>>,
    /// Provider settings: a flow task's frozen copy, otherwise the live one.
    pub config: &'a ErConfig,
    pub selection: &'a AiSelection,
}

/// A stage job whose argv passed every launch check, ready to run.
pub struct StageJob {
    kind: String,
    command_name: Arc<str>,
    er_dir: String,
    repo_root: String,
    execution_root: Option<String>,
    remote_repo: Option<String>,
    managed_local: bool,
    prompt: String,
    prepared_diff: bool,
    host_write_diagram: Option<HostWriteDiagram>,
    scoped_stamp: Option<ScopedRunStamp>,
    staging: Option<Arc<JobStaging>>,
    agent_cmd: String,
    config_args: Vec<String>,
    is_claude_compatible: bool,
    is_stream_json: bool,
    family: CliFamily,
    opencode_env: Option<(String, String)>,
    readonly_run: bool,
    timeout: std::time::Duration,
    rules_key: String,
    store: ArtifactStore,
    ports: Ports,
}

impl StageJob {
    /// Resolve the provider and check the launch policy.
    ///
    /// Also creates the bucket and captures the store, so it touches the disk:
    /// call it on the thread whose storage root the job belongs to, never on
    /// an actor thread.
    pub fn prepare(request: StageJobRequest<'_>, ports: &Ports) -> Result<Self> {
        let StageJobRequest {
            kind,
            command_name,
            target,
            execution_root,
            prompt,
            prepared_diff,
            host_write_diagram,
            scoped_stamp,
            staging,
            config,
            selection,
        } = request;
        // Ranking is refused on remote-only tabs, so the key names the repo
        // here as it does in the prompt.
        let rules_key = crate::storage::rules_key(&*ports.git, &target.repo_root, None);
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
            let family = CliFamily::detect(&cmd);
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

        let effort_override = if kind == "triage" { Some("low") } else { None };
        let effort = crate::config::resolve_effort_for_model(
            &config.ai_hub,
            &config.agent,
            resolved_provider_id.as_deref(),
            resolved_model_id.as_deref(),
            selection.effort.as_deref(),
            effort_override,
        );
        if kind == crate::ai::prompts::IMPORTANCE_TASK_KIND {
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
        let readonly_run = background_run_is_read_only(kind, host_write_diagram.is_some());
        // A staged job gets access to its staging directory; it no longer
        // needs the review bucket. Jobs with no file output keep the bucket
        // path they had.
        let storage_dir = staging
            .as_ref()
            .map_or_else(|| target.er_dir.clone(), |staging| staging.path());
        let storage_access = match family {
            CliFamily::Codex | CliFamily::Cursor if readonly_run => None,
            _ => Some(storage_dir.as_str()),
        };
        crate::config::inject_agent_storage_access(family, &mut config_args, storage_access);
        if readonly_run {
            crate::config::apply_readonly_spawn(family, &mut config_args);
        }
        let opencode_env = if readonly_run {
            crate::config::apply_opencode_readonly_storage_spawn(
                family,
                &mut config_args,
                Some(storage_dir.as_str()),
            )
        } else {
            crate::config::apply_opencode_spawn(
                family,
                &mut config_args,
                Some(storage_dir.as_str()),
            )
        };
        if kind == crate::ai::prompts::IMPORTANCE_TASK_KIND {
            if family == CliFamily::Claude {
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
                (family == CliFamily::Codex && resolved_model_id.is_some())
                    .then_some(effort.as_deref())
                    .flatten(),
            )?;
        }

        std::fs::create_dir_all(&target.er_dir)?;

        Ok(Self {
            kind: kind.to_string(),
            command_name: Arc::from(command_name),
            er_dir: target.er_dir.clone(),
            repo_root: target.repo_root.clone(),
            execution_root,
            remote_repo: target.remote_repo.clone(),
            managed_local: target.managed_local,
            prompt,
            prepared_diff,
            host_write_diagram,
            scoped_stamp,
            staging,
            agent_cmd,
            config_args,
            is_claude_compatible,
            is_stream_json,
            family,
            opencode_env,
            readonly_run,
            timeout: config.ai_hub.effective_agent_timeout(),
            rules_key,
            // Captured here, not on the worker: a test storage root is
            // scoped to the launching thread.
            store: crate::store_registry::store(),
            ports: ports.clone(),
        })
    }

    /// Whether the provider streams JSON, which decides how its stdout lines
    /// read in a log.
    pub fn is_stream_json(&self) -> bool {
        self.is_stream_json
    }

    /// Stage the inputs, run the job through `sink`, then validate and commit
    /// what it wrote. Blocks until the job and its commit finish; the staging
    /// directory is gone by the time this returns.
    #[expect(
        clippy::literal_string_with_formatting_args,
        reason = "{prompt} is a template placeholder substituted via .replace()"
    )]
    pub fn run(
        self,
        sink: &dyn JobSink,
        limits: JobLimits,
        handle: Arc<AgentRunHandle>,
        progress: Option<SyncSender<JobProgress>>,
    ) -> Result<()> {
        let Self {
            kind,
            command_name,
            er_dir,
            repo_root,
            execution_root,
            remote_repo,
            managed_local,
            prompt,
            prepared_diff,
            host_write_diagram,
            scoped_stamp,
            staging,
            agent_cmd,
            config_args,
            is_claude_compatible,
            is_stream_json,
            family,
            opencode_env,
            readonly_run,
            timeout,
            rules_key,
            store,
            ports,
        } = self;
        let is_remote = remote_repo.is_some();
        // For local-branch views (cache-dir er_dir) treat like remote — cwd is
        // the cache dir so .er/* writes land where the loader reads them.
        let is_cache_er_dir = !er_dir.starts_with(&repo_root);
        let debug_path = std::path::Path::new(&er_dir).join(DEBUG_LOG_FILE);

        // The argv still carries `{prompt}`; the runner substitutes the
        // job prompt into it. The slot, the spawn and the stop verdicts
        // are the runner's now.
        let mut agent_args: Vec<String> = config_args;

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
            let allowed = if kind == crate::ai::prompts::IMPORTANCE_TASK_KIND {
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

        if kind == crate::ai::prompts::IMPORTANCE_TASK_KIND && family == CliFamily::Claude {
            validate_importance_launch(family, &agent_args, None, None)?;
        }

        // Captured before `prompt` moves into the spec. The debug
        // transcript shows the argv the provider actually saw.
        let debug_args = debug_agent_log_enabled().then(|| {
            let mut args: Vec<String> = agent_args
                .iter()
                .map(|arg| arg.replace("{prompt}", &prompt))
                .collect();
            if !agent_args.iter().any(|arg| arg.contains("{prompt}")) && !prompt.is_empty() {
                args.push(prompt.clone());
            }
            args
        });

        // managed_local: er_dir is outside repo_root but git must run from repo_root.
        // is_remote / is_cache_er_dir without managed_local: cwd = er_dir (cache-dir or remote).
        // A remote PR whose repo *is* the local checkout runs there instead, so the
        // agent can verify against real code rather than hunting the filesystem.
        let remote_checkout = if execution_root.is_none() && is_remote {
            remote_repo.as_deref().and_then(|slug| {
                ports
                    .github
                    .local_checkout_for_repo(&*ports.git, &repo_root, slug)
            })
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
        let spec = JobSpec {
            kind: background_job_kind(&kind),
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
                timeout,
            },
            prompt,
            work_dir: work_dir.clone(),
            workload: er_agent::agent_slots::Workload::Background,
            output: OutputContract::Stdout,
        };

        // The prompt and `--add-dir` point at staging; the inputs the
        // prompt reads are copied in just before the run. The baseline
        // records which files were absent, so a successful run must
        // have written them.
        let baseline = match staging.as_deref() {
            Some(staging) => {
                staging
                    .stage_inputs(&er_dir)
                    .with_context(|| format!("{command_name}: failed to stage its inputs"))?;
                crate::job_commit::capture_baseline(&kind, scoped_stamp.is_some(), &staging.path())?
            }
            None => None,
        };

        let outcome = sink
            .submit(JobSubmission {
                spec,
                limits,
                handle,
                progress,
                label: "background".to_string(),
            })
            .wait();
        let output = match outcome {
            Ok(output) => output,
            Err(JobError::TimedOut(limit)) => {
                return Err(er_agent::agent_run::timed_out(limit));
            }
            Err(JobError::Cancelled) => {
                return Err(er_agent::agent_run::cancelled());
            }
            Err(JobError::Spawn(error)) => {
                return Err(error).with_context(|| format!("Failed to run review ({agent_cmd})"));
            }
        };
        let stdout_lines: Vec<String> = output.stdout.lines().map(str::to_string).collect();
        let stderr_lines: Vec<String> = output.stderr.lines().map(str::to_string).collect();
        let status = output.status;

        if let Some(debug_args) = &debug_args {
            let debug_content = format!(
                "=== review agent command ===\ncommand: {} {}\nexit code: {}\n\n--- stdout ---\n{}\n\n--- stderr ---\n{}\n",
                agent_cmd,
                debug_args.join(" "),
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
                    "{command_name}: `{agent_cmd}` needs authentication. \
                     Run `{agent_cmd}` in a terminal to sign in, then retry. {stderr_snip}"
                );
            }
            if stderr_snip.is_empty() {
                anyhow::bail!(
                    "{command_name} failed (exit {:?}). See Settings → AI Hub if the \
                     provider CLI is missing or not signed in.",
                    status.code()
                );
            }
            anyhow::bail!("{command_name} failed: {stderr_snip}");
        }
        // Host-owned importance write: the agent printed a rule table
        // and had no Write/Edit. Validate it and commit it as this
        // repo's `risk-rules` artifact.
        if kind == crate::ai::prompts::IMPORTANCE_TASK_KIND {
            // The shared decoder's final text, not the raw stdout: a
            // stream-json run carries the table inside its result event.
            let proposal =
                crate::config::RiskRuleProposal::from_reply(&output.text).ok_or_else(|| {
                    anyhow::anyhow!("{command_name}: the agent printed no importance table")
                })?;
            proposal
                .commit_to_store(&rules_key, &store)
                .with_context(|| {
                    format!("{command_name}: failed to commit the importance table")
                })?;
        }
        // Host-owned diagram write: agent had no Write/Edit; validate
        // only the diagrams/<id>.json payload from stdout, then commit
        // it through the store.
        if let Some(hw) = &host_write_diagram {
            let diagram = crate::ai::validated_diagram_from_stdout(
                &output.stdout,
                is_stream_json,
                &hw.kind,
                &hw.diff_hash,
                hw.custom_prompt.as_deref(),
            )
            .with_context(|| format!("{command_name}: failed to validate its diagram sidecar"))?;
            let id = hw
                .output_path
                .file_stem()
                .and_then(|name| name.to_str())
                .context("diagram output file has no id")?;
            crate::job_commit::commit_diagram(&diagram, id, &er_dir, &store)
                .with_context(|| format!("{command_name}: failed to commit its diagram sidecar"))?;
        }
        // File outputs: validate the staged files, then commit them
        // through the store. A malformed or stale output fails the
        // job here, before anything in the bucket changes.
        if let Some(staging) = staging.as_deref() {
            if scoped_stamp.is_some() {
                crate::job_commit::commit_staged_scoped(
                    command_name.as_ref(),
                    &staging.path(),
                    &er_dir,
                    &store,
                )?;
            } else {
                crate::job_commit::commit_staged(
                    &kind,
                    &staging.path(),
                    &er_dir,
                    baseline.as_ref(),
                    &store,
                )?;
            }
        }
        // A scoped run commits its own overlay; a successful full run
        // clears its lens's old overlay and validation. A failed run
        // never reaches here.
        let scope = er_store::Scope::dir(&er_dir)
            .with_context(|| format!("{command_name}: invalid bucket {er_dir}"))?;
        if let Some(stamp) = &scoped_stamp {
            crate::ai::stamp_scoped_overlay(&store, &scope, &command_name, stamp)
                .with_context(|| format!("{command_name} failed to stamp its scoped overlay"))?;
        } else if command_name.as_ref() == "review"
            || command_name.as_ref() == "professor"
            || command_name.starts_with("expert-")
        {
            crate::ai::clear_full_run_artifacts(&store, &scope, &command_name)
                .with_context(|| format!("{command_name} failed to clear its old artifacts"))?;
        }
        Ok(())
    }
}

/// How a background task kind maps to the runner's job kind. `general` is a
/// review; `importance` and `diagram:<kind>` have no runner variant yet.
pub(crate) fn background_job_kind(kind: &str) -> er_agent::job::JobKind {
    match kind {
        "" | "review" | "general" => er_agent::job::JobKind::Review,
        "triage" => er_agent::job::JobKind::Triage,
        "professor" => er_agent::job::JobKind::Professor,
        "tour" => er_agent::job::JobKind::Tour {
            filename: "tour.json".into(),
        },
        "questions" => er_agent::job::JobKind::Questions,
        "summary" => er_agent::job::JobKind::Summary,
        "validate" => er_agent::job::JobKind::ValidateReview,
        "validate-comments" => er_agent::job::JobKind::ValidateComments,
        other => match other.strip_prefix("expert:") {
            Some(id) => er_agent::job::JobKind::Expert(id.to_string()),
            None => er_agent::job::JobKind::Other(other.to_string()),
        },
    }
}

/// Review agents never need a second checkout: they run inside the repo (or, for a remote PR
/// whose repo is not checked out locally, work from the prepared diff plus `gh`). Cloning has
/// only ever produced stray copies under `/tmp` that later runs then read stale code out of.
/// A speed bump rather than a wall — `gh repo clone` and `cd x && git clone` slip past it.
pub(crate) const CLONE_DENY_RULE: &str = "Bash(git clone*)";
pub(crate) const IMPORTANCE_READ_ONLY_TOOLS: &[&str] = &["Read", "Glob", "Grep"];
pub(crate) const MCP_DENY_RULE: &str = "mcp__*";

pub(crate) const DIAGRAM_READ_ONLY_TOOLS: &[&str] = &[
    "Read",
    "Bash(grep *)",
    "Bash(rg *)",
    "Bash(git grep*)",
    "Bash(git show*)",
    "Bash(git log*)",
];
pub(crate) const WRITABLE_PREPARED_DIFF_TOOLS: &[&str] = &[
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
pub(crate) const WRITABLE_TOOLS: &[&str] = &[
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

pub(crate) fn background_run_is_read_only(task_kind: &str, host_write_diagram: bool) -> bool {
    host_write_diagram || task_kind == crate::ai::prompts::IMPORTANCE_TASK_KIND
}

pub(crate) fn claude_allowed_tools(
    read_only: bool,
    prepared_diff: bool,
) -> &'static [&'static str] {
    if read_only {
        DIAGRAM_READ_ONLY_TOOLS
    } else if prepared_diff {
        WRITABLE_PREPARED_DIFF_TOOLS
    } else {
        WRITABLE_TOOLS
    }
}

/// Check the final configured launch policy before prompt text is substituted.
pub(crate) fn validate_importance_launch(
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
pub(crate) fn validate_importance_user_config(
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

/// Whether a completed agent run writes its full transcript to
/// `debug-agent.log`.
///
/// Off by default. The write formatted the whole of stdout and stderr into a
/// third copy of strings the reader threads already held, on every run, to
/// produce a file nothing reads unless someone is debugging a spawn.
pub(crate) fn debug_agent_log_enabled() -> bool {
    std::env::var("ER_DEBUG").is_ok()
}

/// A failed agent's stderr, trimmed and capped at 280 chars for the error
/// message. Char-safe: agent stderr often includes multi-byte text.
pub(crate) fn agent_stderr_snippet(stderr_lines: &[String]) -> String {
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

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;
    use crate::jobs::{JobResult, JobTicket};
    use std::os::unix::process::ExitStatusExt;
    use std::sync::Mutex;

    const DIFF: &str = "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-a\n+b\n";

    /// Records each spec and plays the agent: it writes `outputs` into the
    /// job's staging directory and answers a clean exit.
    struct RecordingSink {
        specs: Mutex<Vec<JobSpec>>,
        outputs: Vec<(&'static str, String)>,
    }

    impl JobSink for RecordingSink {
        fn submit(&self, job: JobSubmission) -> JobTicket {
            let staging = job
                .spec
                .provider
                .env
                .iter()
                .find(|(key, _)| key == "ER_JOB_STAGING")
                .map(|(_, dir)| std::path::PathBuf::from(dir));
            if let Some(dir) = staging {
                for (name, body) in &self.outputs {
                    std::fs::write(dir.join(name), body).unwrap();
                }
            }
            self.specs.lock().unwrap().push(job.spec);
            let (tx, rx) = std::sync::mpsc::channel::<JobResult>();
            let _ = tx.send(Ok(er_agent::job::JobOutput {
                text: String::new(),
                stdout: String::new(),
                stderr: String::new(),
                status: std::process::ExitStatus::from_raw(0),
            }));
            JobTicket::new(crate::jobs::JobId::next(), rx)
        }
    }

    fn bucket(tmp: &std::path::Path) -> (std::path::PathBuf, BackgroundTaskTarget) {
        let bucket = tmp.join("bucket");
        std::fs::create_dir_all(&bucket).unwrap();
        std::fs::write(bucket.join("diff-tmp"), DIFF).unwrap();
        std::fs::write(
            bucket.join(".diff-tmp.sha256"),
            crate::ai::compute_diff_hash(DIFF),
        )
        .unwrap();
        let target = BackgroundTaskTarget {
            repo_root: tmp.to_string_lossy().to_string(),
            er_dir: bucket.to_string_lossy().to_string(),
            branch_label: "feat".into(),
            base_branch: "main".into(),
            scope: "branch".into(),
            pr_number: None,
            remote_repo: None,
            managed_local: false,
        };
        (bucket, target)
    }

    fn claude_config() -> ErConfig {
        let mut config = ErConfig::default();
        config.ai_hub.providers.clear();
        config.agent.command = "claude".into();
        config.agent.args = vec!["--print".into(), "{prompt}".into()];
        config
    }

    fn limits() -> JobLimits {
        JobLimits {
            slot_cap: 1,
            ceiling: 1,
            retry: er_agent::job::RetryPolicy::none(),
        }
    }

    fn review_outputs(hash: &str) -> Vec<(&'static str, String)> {
        vec![
            (
                "review.json",
                format!(r#"{{"version":1,"diff_hash":"{hash}","files":{{}}}}"#),
            ),
            (
                "order.json",
                format!(r#"{{"version":1,"diff_hash":"{hash}","order":[],"groups":{{}}}}"#),
            ),
            (
                "checklist.json",
                format!(r#"{{"version":1,"diff_hash":"{hash}","items":[]}}"#),
            ),
            ("summary.md", "summary\n".to_string()),
        ]
    }

    /// With no App anywhere, a review job reaches the sink as one spec aimed
    /// at its staging directory, and what it staged is committed to the bucket.
    #[test]
    fn a_stage_job_runs_through_the_sink_and_commits_without_the_app() {
        let tmp = tempfile::tempdir().unwrap();
        let (bucket, target) = bucket(tmp.path());
        let bucket_path = target.er_dir.clone();
        crate::storage::with_test_storage_root(tmp.path(), || {
            let config = claude_config();
            let (staging, prompt) = staging_for(
                "review",
                format!("Write the review into '{bucket_path}'"),
                &bucket_path,
            )
            .unwrap();
            let job = StageJob::prepare(
                StageJobRequest {
                    kind: "review",
                    command_name: "review",
                    target: &target,
                    execution_root: None,
                    prompt,
                    prepared_diff: true,
                    host_write_diagram: None,
                    scoped_stamp: None,
                    staging,
                    config: &config,
                    selection: &AiSelection::default(),
                },
                &Ports::for_test(),
            )
            .unwrap();
            let sink = RecordingSink {
                specs: Mutex::new(Vec::new()),
                outputs: review_outputs(&crate::ai::compute_diff_hash(DIFF)),
            };

            job.run(&sink, limits(), AgentRunHandle::new(), None)
                .unwrap();

            let specs = sink.specs.lock().unwrap();
            assert_eq!(specs.len(), 1, "the job bypassed the sink");
            let spec = &specs[0];
            assert_eq!(spec.kind, er_agent::job::JobKind::Review);
            assert_eq!(spec.work_dir, target.repo_root);
            let staging = spec
                .provider
                .env
                .iter()
                .find_map(|(key, dir)| (key == "ER_JOB_STAGING").then_some(dir.clone()))
                .expect("the spec names no staging");
            assert!(
                spec.provider.args.contains(&format!("--add-dir={staging}")),
                "{:?}",
                spec.provider.args
            );
            assert!(spec.prompt.contains(&staging), "{}", spec.prompt);
            assert!(!spec.prompt.contains(&bucket_path), "{}", spec.prompt);
            for name in ["review.json", "order.json", "checklist.json", "summary.md"] {
                assert!(bucket.join(name).is_file(), "{name} was not committed");
            }
            assert!(
                !std::path::Path::new(&staging).exists(),
                "staging outlived the job"
            );
        });
    }

    /// A host-written diagram runs read-only: Claude gets the read-only tool
    /// list and no write tool, whichever host launched it.
    #[test]
    fn a_host_written_diagram_reaches_the_sink_read_only() {
        let tmp = tempfile::tempdir().unwrap();
        let (_bucket, target) = bucket(tmp.path());
        crate::storage::with_test_storage_root(tmp.path(), || {
            let config = claude_config();
            let job = StageJob::prepare(
                StageJobRequest {
                    kind: "diagram:flow",
                    command_name: "diagram",
                    target: &target,
                    execution_root: None,
                    prompt: "draw".into(),
                    prepared_diff: true,
                    host_write_diagram: Some(HostWriteDiagram {
                        output_path: tmp.path().join("diagrams/d1.json"),
                        kind: "flow".into(),
                        diff_hash: crate::ai::compute_diff_hash(DIFF),
                        custom_prompt: None,
                    }),
                    scoped_stamp: None,
                    staging: None,
                    config: &config,
                    selection: &AiSelection::default(),
                },
                &Ports::for_test(),
            )
            .unwrap();
            let sink = RecordingSink {
                specs: Mutex::new(Vec::new()),
                outputs: Vec::new(),
            };

            // The fake prints no diagram, so the host write fails; the spec
            // the sink saw is what this test reads.
            let _ = job.run(&sink, limits(), AgentRunHandle::new(), None);

            let specs = sink.specs.lock().unwrap();
            let args = &specs.first().expect("nothing was submitted").provider.args;
            let allowed: Vec<&str> = args
                .iter()
                .enumerate()
                .filter(|(_, arg)| *arg == "--allowedTools")
                .filter_map(|(i, _)| args.get(i + 1).map(String::as_str))
                .collect();
            assert_eq!(allowed, DIAGRAM_READ_ONLY_TOOLS, "{args:?}");
        });
    }

    /// The debug transcript is written straight to disk, so its name must not
    /// be one the store owns.
    #[test]
    fn the_debug_transcript_is_not_a_registered_kind() {
        assert!(crate::store_registry::registry()
            .kind_for(std::path::Path::new(DEBUG_LOG_FILE))
            .is_none());
    }
}
