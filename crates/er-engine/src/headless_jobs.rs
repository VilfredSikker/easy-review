//! Headless AI review jobs for MCP / non-UI callers.
//!
//! Fetches a remote PR diff, writes it into the shared managed PR bucket
//! (`…/repos/<owner-repo>/prs/pr-<N>/`), and runs the same prepared-diff agent
//! prompts Desktop uses — so TUI/Desktop see the sidecars on next load.

use crate::agent_runtime::{
    build_argv, resolve_invocation, AgentAccessProfile, AgentInvocationRequest, AgentPrompt,
    AgentSelection, AgentTaskKind, ArtifactBaseline, ArtifactContract,
};
use crate::ai::prompts::{
    build_review_prompt_prepared_diff, build_tour_prompt_prepared_diff,
    build_triage_review_prompt_prepared_diff,
};
use crate::config::{inject_codex_ignore_user_config, load_global_config, ErConfig};
use crate::github::{gh_pr_diff_remote, gh_pr_metadata_remote, owner_repo_storage_slug};
use crate::storage::resolve_managed_root_for_pr_bucket;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

static JOB_SEQ: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeadlessJobKind {
    Triage,
    Review,
    Tour,
}

impl HeadlessJobKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Triage => "triage",
            Self::Review => "review",
            Self::Tour => "tour",
        }
    }

    fn task_kind(self) -> AgentTaskKind {
        match self {
            Self::Triage => AgentTaskKind::Triage,
            Self::Review => AgentTaskKind::Review,
            Self::Tour => AgentTaskKind::Tour {
                filename: "tour.json".into(),
            },
        }
    }

    fn artifact_contract(self) -> ArtifactContract {
        self.task_kind().artifact_contract()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeadlessJobStatus {
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeadlessJobInfo {
    pub id: String,
    pub kind: HeadlessJobKind,
    pub status: HeadlessJobStatus,
    pub owner: String,
    pub repo: String,
    pub pr: u64,
    pub er_dir: String,
    pub started_at_ms: u64,
    pub finished_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct HeadlessJobRequest {
    pub kind: HeadlessJobKind,
    pub owner: String,
    pub repo: String,
    pub pr: u64,
    pub base_ref: Option<String>,
    pub head_ref: Option<String>,
    pub ignore_globs: Vec<String>,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    /// Prepare storage + register the job, but do not spawn an agent (tests).
    pub dry_run: bool,
}

struct JobRecord {
    info: HeadlessJobInfo,
}

struct Registry {
    jobs: HashMap<String, JobRecord>,
}

fn registry() -> &'static Mutex<Registry> {
    use std::sync::OnceLock;
    static REG: OnceLock<Mutex<Registry>> = OnceLock::new();
    REG.get_or_init(|| {
        Mutex::new(Registry {
            jobs: HashMap::new(),
        })
    })
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn expected_artifacts(kind: HeadlessJobKind, er_dir: &str) -> Vec<String> {
    match kind {
        HeadlessJobKind::Triage => vec![format!("{er_dir}/triage.json")],
        HeadlessJobKind::Review => vec![
            format!("{er_dir}/review.json"),
            format!("{er_dir}/order.json"),
            format!("{er_dir}/checklist.json"),
            format!("{er_dir}/summary.md"),
        ],
        HeadlessJobKind::Tour => vec![format!("{er_dir}/tour.json")],
    }
}

/// Resolve managed PR bucket and write `diff-tmp` for a remote PR.
pub fn prepare_pr_diff_tmp(
    owner: &str,
    repo: &str,
    pr: u64,
    ignore_globs: &[String],
) -> Result<(String, String)> {
    let mut raw = gh_pr_diff_remote(owner, repo, pr)?;
    if !ignore_globs.is_empty() {
        raw = crate::git::filter_raw_diff_exclude_globs(&raw, ignore_globs);
    }
    if raw.trim().is_empty() {
        bail!("PR #{pr} has an empty diff after filters");
    }

    let slug = owner_repo_storage_slug(owner, repo);
    let er_dir = resolve_managed_root_for_pr_bucket(&slug, pr).er_dir();
    if er_dir.is_empty() {
        bail!("failed to resolve managed PR storage for {owner}/{repo}#{pr}");
    }
    std::fs::create_dir_all(&er_dir).with_context(|| format!("mkdir {er_dir}"))?;
    let diff_path = format!("{er_dir}/diff-tmp");
    std::fs::write(&diff_path, &raw).with_context(|| format!("write {diff_path}"))?;
    Ok((er_dir, diff_path))
}

fn build_prompt(kind: HeadlessJobKind, er_dir: &str, base: &str, head: &str) -> String {
    match kind {
        HeadlessJobKind::Triage => build_triage_review_prompt_prepared_diff("branch", er_dir),
        HeadlessJobKind::Review => build_review_prompt_prepared_diff("branch", er_dir, base, head),
        HeadlessJobKind::Tour => build_tour_prompt_prepared_diff("PR diff", er_dir, "tour.json"),
    }
}

fn has_active_duplicate(
    reg: &Registry,
    kind: HeadlessJobKind,
    owner: &str,
    repo: &str,
    pr: u64,
) -> bool {
    reg.jobs.values().any(|j| {
        j.info.kind == kind
            && j.info.owner == owner
            && j.info.repo == repo
            && j.info.pr == pr
            && matches!(
                j.info.status,
                HeadlessJobStatus::Queued | HeadlessJobStatus::Running
            )
    })
}

fn write_job_status_file(er_dir: &str, info: &HeadlessJobInfo) {
    let path = Path::new(er_dir).join("headless-job.json");
    if let Ok(json) = serde_json::to_string_pretty(info) {
        let _ = std::fs::write(path, json);
    }
}

fn update_job(id: &str, f: impl FnOnce(&mut HeadlessJobInfo)) {
    if let Ok(mut reg) = registry().lock() {
        if let Some(rec) = reg.jobs.get_mut(id) {
            f(&mut rec.info);
            let info = rec.info.clone();
            write_job_status_file(&info.er_dir, &info);
        }
    }
}

pub fn job_status(id: &str) -> Option<HeadlessJobInfo> {
    registry().lock().ok()?.jobs.get(id).map(|r| r.info.clone())
}

pub fn list_jobs() -> Vec<HeadlessJobInfo> {
    let Ok(reg) = registry().lock() else {
        return Vec::new();
    };
    let mut out: Vec<_> = reg.jobs.values().map(|r| r.info.clone()).collect();
    out.sort_by_key(|b| std::cmp::Reverse(b.started_at_ms));
    out
}

/// Cancel a queued job. Running jobs cannot be interrupted yet.
pub fn cancel_job(id: &str) -> Result<HeadlessJobInfo> {
    let mut reg = registry().lock().map_err(|e| anyhow::anyhow!("{e}"))?;
    let rec = reg
        .jobs
        .get_mut(id)
        .ok_or_else(|| anyhow::anyhow!("unknown job id: {id}"))?;
    match rec.info.status {
        HeadlessJobStatus::Queued => {
            rec.info.status = HeadlessJobStatus::Cancelled;
            rec.info.finished_at_ms = Some(now_ms());
            let info = rec.info.clone();
            write_job_status_file(&info.er_dir, &info);
            Ok(info)
        }
        HeadlessJobStatus::Running => {
            bail!("job {id} is already running; cancel mid-run is not supported yet")
        }
        other => bail!("job {id} is {:?}, nothing to cancel", other),
    }
}

/// Prepare the PR bucket, enqueue, and spawn triage / review / tour.
pub fn start_job(req: HeadlessJobRequest) -> Result<HeadlessJobInfo> {
    {
        let reg = registry().lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        if has_active_duplicate(&reg, req.kind, &req.owner, &req.repo, req.pr) {
            bail!(
                "{} already running/queued for {}/{}#{}",
                req.kind.as_str(),
                req.owner,
                req.repo,
                req.pr
            );
        }
    }

    let (meta_base, meta_head) = gh_pr_metadata_remote(&req.owner, &req.repo, req.pr)
        .unwrap_or_else(|_| ("main".into(), format!("pr-{}", req.pr)));
    let base = req
        .base_ref
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or(meta_base);
    let head = req
        .head_ref
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or(meta_head);

    let (er_dir, _) = prepare_pr_diff_tmp(&req.owner, &req.repo, req.pr, &req.ignore_globs)?;
    let artifacts = expected_artifacts(req.kind, &er_dir);
    let id = format!("hj-{}", JOB_SEQ.fetch_add(1, Ordering::Relaxed));
    let info = HeadlessJobInfo {
        id: id.clone(),
        kind: req.kind,
        status: if req.dry_run {
            HeadlessJobStatus::Done
        } else {
            HeadlessJobStatus::Queued
        },
        owner: req.owner.clone(),
        repo: req.repo.clone(),
        pr: req.pr,
        er_dir: er_dir.clone(),
        started_at_ms: now_ms(),
        finished_at_ms: if req.dry_run { Some(now_ms()) } else { None },
        error: None,
        artifacts,
    };

    {
        let mut reg = registry().lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        reg.jobs
            .insert(id.clone(), JobRecord { info: info.clone() });
    }
    write_job_status_file(&er_dir, &info);

    if req.dry_run {
        return Ok(info);
    }

    let config = Arc::new(load_global_config());
    let prompt = build_prompt(req.kind, &er_dir, &base, &head);
    let info_for_thread = info.clone();
    let provider_id = req.provider_id;
    let model_id = req.model_id;

    std::thread::spawn(move || {
        update_job(&info_for_thread.id, |j| {
            j.status = HeadlessJobStatus::Running;
        });
        let result = run_agent_job(
            &config,
            &info_for_thread,
            &prompt,
            provider_id.as_deref(),
            model_id.as_deref(),
        );
        update_job(&info_for_thread.id, |j| {
            j.finished_at_ms = Some(now_ms());
            match result {
                Ok(()) => {
                    j.status = HeadlessJobStatus::Done;
                    j.error = None;
                }
                Err(e) => {
                    j.status = HeadlessJobStatus::Failed;
                    j.error = Some(e.to_string());
                }
            }
        });
    });

    job_status(&id).ok_or_else(|| anyhow::anyhow!("job vanished"))
}

fn run_agent_job(
    config: &ErConfig,
    info: &HeadlessJobInfo,
    prompt: &str,
    provider_id: Option<&str>,
    model_id: Option<&str>,
) -> Result<()> {
    let task_kind = info.kind.task_kind();
    let effort_override = if matches!(info.kind, HeadlessJobKind::Triage) {
        Some("low")
    } else {
        None
    };

    let mut invocation = resolve_invocation(
        config,
        AgentInvocationRequest {
            selection: AgentSelection::Runtime {
                provider_id,
                model_id,
            },
            task: &task_kind,
            effort: None,
            effort_override,
            work_dir: info.er_dir.clone(),
            access: AgentAccessProfile::PreparedArtifacts {
                output_dir: info.er_dir.clone(),
            },
            live_logs: true,
        },
    )?;

    if crate::config::agent_command_is_codex(&invocation.command) {
        inject_codex_ignore_user_config(&mut invocation.args);
    }

    let argv = build_argv(
        &invocation,
        AgentPrompt {
            system: None,
            user: prompt,
        },
    );

    let baseline = ArtifactBaseline::capture(info.kind.artifact_contract(), &info.er_dir)?;
    let slot_cap = config.ai_hub.effective_max_concurrent_reviews();
    let _slot = crate::agent_slots::acquire_blocking(slot_cap);

    let debug_path = Path::new(&info.er_dir).join("debug-agent.log");
    let mut child = std::process::Command::new(&invocation.command)
        .args(&argv)
        .current_dir(&invocation.work_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to spawn agent ({})", invocation.command))?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_handle = std::thread::spawn(move || {
        let mut lines = Vec::new();
        if let Some(pipe) = stdout {
            use std::io::BufRead;
            for line in std::io::BufReader::new(pipe).lines().map_while(Result::ok) {
                lines.push(line);
            }
        }
        lines
    });
    let stderr_handle = std::thread::spawn(move || {
        let mut lines = Vec::new();
        if let Some(pipe) = stderr {
            use std::io::BufRead;
            for line in std::io::BufReader::new(pipe).lines().map_while(Result::ok) {
                lines.push(line);
            }
        }
        lines
    });

    let status = child.wait().context("wait for agent")?;
    let stdout_lines = stdout_handle.join().unwrap_or_default();
    let stderr_lines = stderr_handle.join().unwrap_or_default();
    let debug_content = format!(
        "=== headless {} ===\ncommand: {} {}\nexit: {}\n\n--- stdout ---\n{}\n\n--- stderr ---\n{}\n",
        info.kind.as_str(),
        invocation.command,
        argv.join(" "),
        status
            .code()
            .map_or_else(|| "signal".into(), |c| c.to_string()),
        stdout_lines.join("\n"),
        stderr_lines.join("\n"),
    );
    let _ = std::fs::write(&debug_path, debug_content);

    if !status.success() {
        bail!(
            "{} failed (see {}/debug-agent.log)",
            info.kind.as_str(),
            info.er_dir
        );
    }

    if let Err(e) = baseline.validate(&info.er_dir) {
        let primary = match info.kind {
            HeadlessJobKind::Triage => format!("{}/triage.json", info.er_dir),
            HeadlessJobKind::Review => format!("{}/review.json", info.er_dir),
            HeadlessJobKind::Tour => format!("{}/tour.json", info.er_dir),
        };
        if !Path::new(&primary).exists() {
            return Err(e).context("agent exited 0 but artifacts missing");
        }
    }
    Ok(())
}

pub fn start_triage(owner: &str, repo: &str, pr: u64) -> Result<HeadlessJobInfo> {
    start_job(HeadlessJobRequest {
        kind: HeadlessJobKind::Triage,
        owner: owner.into(),
        repo: repo.into(),
        pr,
        base_ref: None,
        head_ref: None,
        ignore_globs: vec![],
        provider_id: None,
        model_id: None,
        dry_run: false,
    })
}

pub fn start_review(owner: &str, repo: &str, pr: u64) -> Result<HeadlessJobInfo> {
    start_job(HeadlessJobRequest {
        kind: HeadlessJobKind::Review,
        owner: owner.into(),
        repo: repo.into(),
        pr,
        base_ref: None,
        head_ref: None,
        ignore_globs: vec![],
        provider_id: None,
        model_id: None,
        dry_run: false,
    })
}

pub fn start_tour(owner: &str, repo: &str, pr: u64) -> Result<HeadlessJobInfo> {
    start_job(HeadlessJobRequest {
        kind: HeadlessJobKind::Tour,
        owner: owner.into(),
        repo: repo.into(),
        pr,
        base_ref: None,
        head_ref: None,
        ignore_globs: vec![],
        provider_id: None,
        model_id: None,
        dry_run: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_labels_and_artifacts() {
        let er = "/tmp/er-pr-1";
        assert_eq!(HeadlessJobKind::Tour.as_str(), "tour");
        assert!(expected_artifacts(HeadlessJobKind::Tour, er)[0].ends_with("tour.json"));
        assert!(expected_artifacts(HeadlessJobKind::Triage, er)[0].ends_with("triage.json"));
        assert!(expected_artifacts(HeadlessJobKind::Review, er)
            .iter()
            .any(|p| p.ends_with("review.json")));
    }

    #[test]
    fn prompt_builders_mention_outputs() {
        let er = "/tmp/er-pr-1";
        let triage = build_prompt(HeadlessJobKind::Triage, er, "main", "feat");
        assert!(triage.contains("triage.json"));
        let review = build_prompt(HeadlessJobKind::Review, er, "main", "feat");
        assert!(review.contains(er) || review.contains("review"));
        let tour = build_prompt(HeadlessJobKind::Tour, er, "main", "feat");
        assert!(tour.contains("tour.json"));
        assert!(tour.contains("diff-tmp"));
    }
}
