//! A named `[commands]` entry run as a shell job in the repo root. Always
//! compiled, so the App and a host with no App run one the same way.

use crate::agent_log::{log_job_progress, AgentLogEntry, AgentLogSource};
use crate::agent_prompt::{record_submitted_tab_job, AgentPromptRun, BACKGROUND_PROGRESS_CAPACITY};
use crate::ai::prompts::sanitize_for_shell;
use anyhow::{Context, Result};
use er_agent::job::{JobError, JobProgress, JobRun, JobRunner, JobSpec, OutputContract};
use er_agent::provider::ProviderSpec;

/// What a named command runs on.
pub struct ShellCommandRequest<'a> {
    pub name: &'a str,
    /// The command as the config gives it, with its placeholders.
    pub shell_cmd: &'a str,
    pub repo_root: String,
    pub base_branch: &'a str,
    pub branch: &'a str,
    pub er_dir: String,
    pub config: &'a crate::config::ErConfig,
    pub log_tx: std::sync::mpsc::Sender<AgentLogEntry>,
}

/// `shell_cmd` with `{base}`, `{branch}`, `{repo}` and `{output}` replaced,
/// each quoted for the shell. `{output}` is the bucket's `summary.md`.
#[expect(
    clippy::literal_string_with_formatting_args,
    reason = "{base}/{branch}/{repo}/{output} are template placeholders, not format args"
)]
pub fn substitute(shell_cmd: &str, base: &str, branch: &str, repo: &str, er_dir: &str) -> String {
    shell_cmd
        .replace("{base}", &sanitize_for_shell(base))
        .replace("{branch}", &sanitize_for_shell(branch))
        .replace("{repo}", &sanitize_for_shell(repo))
        .replace(
            "{output}",
            &sanitize_for_shell(&format!("{er_dir}/summary.md")),
        )
}

/// Runs the command through `sh -c` in the repo root on its own thread, as a
/// job that holds an agent slot and a stop control, since it may launch a
/// provider CLI. Answers the run's stop handle and the receiver its outcome
/// arrives on.
pub fn launch_shell_command(request: ShellCommandRequest<'_>) -> Result<AgentPromptRun> {
    let ShellCommandRequest {
        name,
        shell_cmd,
        repo_root,
        base_branch,
        branch,
        er_dir,
        config,
        log_tx,
    } = request;
    let cmd = substitute(shell_cmd, base_branch, branch, &repo_root, &er_dir);
    std::fs::create_dir_all(&er_dir).with_context(|| format!("create {er_dir}"))?;

    let name_owned: std::sync::Arc<str> = std::sync::Arc::from(name);
    let _ = log_tx.send(AgentLogEntry {
        timestamp: std::time::Instant::now(),
        command_name: std::sync::Arc::clone(&name_owned),
        source: AgentLogSource::Status,
        text: format!("{name} started"),
    });

    let agent_timeout = config.ai_hub.effective_agent_timeout();
    let slot_cap = config.ai_hub.effective_max_concurrent_reviews();
    let ceiling = config.ai_hub.effective_max_concurrent_agents();
    let run = er_agent::agent_run::AgentRunHandle::new();
    let run_for_caller = std::sync::Arc::clone(&run);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut timer = er_agent::agent_timing::AgentRunTimer::start();
        let result = (|| -> Result<()> {
            let spec = JobSpec {
                kind: er_agent::job::JobKind::Shell(name_owned.to_string()),
                provider: ProviderSpec {
                    program: "sh".to_string(),
                    args: vec!["-c".to_string(), cmd],
                    env: Vec::new(),
                    protocol: crate::agent_runtime::OutputProtocol::Plain,
                    timeout: agent_timeout,
                },
                prompt: String::new(),
                work_dir: repo_root,
                workload: er_agent::agent_slots::Workload::Background,
                output: OutputContract::Stdout,
            };
            record_submitted_tab_job(&spec.kind);

            // Log lines are drained off the job thread, so a slow reader
            // cannot hold the runner's pipe readers.
            let (progress_tx, progress_rx) =
                std::sync::mpsc::sync_channel::<JobProgress>(BACKGROUND_PROGRESS_CAPACITY);
            let name_out = name_owned.clone();
            let name_err = name_owned.clone();
            let drain = std::thread::spawn(move || {
                while let Ok(event) = progress_rx.recv() {
                    log_job_progress(event, false, &name_out, &name_err, &log_tx);
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
                    return Err(error).with_context(|| format!("Failed to run {name_owned}"));
                }
            };

            if !output.status.success() {
                anyhow::bail!("{} failed: {}", name_owned, output.stderr.trim());
            }

            Ok(())
        })();
        let ok = result.is_ok();
        let _ = tx.send(result);
        timer.emit("shell_command", &name_owned, ok);
    });
    Ok(AgentPromptRun {
        run: run_for_caller,
        result: rx,
    })
}
