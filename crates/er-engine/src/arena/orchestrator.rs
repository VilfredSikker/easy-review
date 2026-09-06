#![allow(clippy::too_many_arguments)]

use super::adapter::{is_cancelled_error, resolve_provider_command, run_provider_json};
use super::agents::agent_meta;
use super::merge::findings_from_round1;
use super::model::*;
use super::registry::{new_run_id, ArenaRegistry, ArenaRunHandle};
use super::storage::{
    append_progress_event, load_run, save_arbiter_output, save_diff_patch, save_round_output,
    save_run, ArenaPaths, ProgressEvent,
};
use super::voting::{apply_round3_verdicts, record_arbiter_ballots, severity_from_cross_check};
use crate::ai::compute_diff_hash;
use crate::ai::prompts::{
    build_arena_round1_prompt_agent, build_arena_round2_prompt, build_arena_round3_prompt,
};
use crate::config::ErConfig;
use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::json;
use std::path::Path;
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

pub const DEFAULT_COST_LIMIT_USD: f32 = 25.0;
pub const MIN_QUORUM: usize = 2;

/// Minimum successful reviewers required after round 1 (1 for solo runs, 2 for arena).
pub fn min_survivors_required(reviewer_count: usize) -> usize {
    reviewer_count.clamp(1, MIN_QUORUM)
}

/// Effective round count for v1 (1–3).
pub fn effective_arena_rounds(requested: Option<u8>) -> u8 {
    requested
        .unwrap_or(ARENA_ROUNDS_V1)
        .clamp(1, ARENA_ROUNDS_V1)
}
pub const ARENA_ROUNDS_V1: u8 = 3;

#[derive(Debug, Clone)]
pub struct ArenaStartParams {
    pub title: Option<String>,
    pub reviewers: Vec<ReviewerRef>,
    pub scope: ArenaScope,
    pub files: Option<Vec<String>>,
    /// Requested reviewer round count (1–3); defaults to [`ARENA_ROUNDS_V1`].
    pub rounds: Option<u8>,
    /// Final arbiter model; defaults to most expensive model in ai_hub.
    pub arbiter: Option<ReviewerRef>,
    pub confirm: bool,
    /// When set, all reviewers use this agent lens (`general`, `professor`, `expert:security`, …).
    pub agent_kind: Option<String>,
    /// Per-run effort override (resolved with global default at start).
    pub effort: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AgentGroupStart {
    pub agent_kind: String,
    pub models: Vec<ReviewerRef>,
    pub title: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ArenaBatchStartParams {
    pub scope: ArenaScope,
    pub files: Option<Vec<String>>,
    pub rounds: Option<u8>,
    pub arbiter: Option<ReviewerRef>,
    pub confirm: bool,
    pub groups: Vec<AgentGroupStart>,
    pub effort: Option<String>,
}

pub const ARBITER_REVIEWER_ID: &str = "arbiter";

pub fn default_arbiter_from_hub(hub: &crate::config::AiHubConfig) -> Option<ReviewerRef> {
    let mut best: Option<(f32, ReviewerRef)> = None;
    for (provider_id, provider) in &hub.providers {
        for model in &provider.models {
            let cost =
                model.cost_per_1k_in.unwrap_or(0.015) + model.cost_per_1k_out.unwrap_or(0.075);
            let rf = ReviewerRef {
                provider_id: provider_id.clone(),
                model_id: model.id.clone(),
                agent_kind: None,
            };
            if best.as_ref().is_none_or(|(c, _)| cost > *c) {
                best = Some((cost, rf));
            }
        }
    }
    best.map(|(_, r)| r)
}

fn model_cost_rate(hub: &crate::config::AiHubConfig, rf: &ReviewerRef) -> f32 {
    hub.providers
        .get(&rf.provider_id)
        .and_then(|p| p.models.iter().find(|m| m.id == rf.model_id))
        .map(|m| m.cost_per_1k_in.unwrap_or(0.015) + m.cost_per_1k_out.unwrap_or(0.075))
        .unwrap_or(0.09)
}

pub fn resolve_arbiter(
    params: &ArenaStartParams,
    hub: &crate::config::AiHubConfig,
) -> Result<ReviewerRef> {
    if let Some(ref a) = params.arbiter {
        return Ok(a.clone());
    }
    default_arbiter_from_hub(hub)
        .or_else(|| params.reviewers.first().cloned())
        .context("no arbiter model available in ai_hub")
}

pub fn arbiter_display_label(hub: &crate::config::AiHubConfig, arbiter: &ReviewerRef) -> String {
    hub.providers
        .get(&arbiter.provider_id)
        .and_then(|p| p.models.iter().find(|m| m.id == arbiter.model_id))
        .and_then(|m| m.label.clone())
        .unwrap_or_else(|| arbiter.model_id.clone())
}

pub const fn scope_git_mode(scope: ArenaScope) -> &'static str {
    match scope {
        ArenaScope::Branch => "branch",
        ArenaScope::Unstaged => "unstaged",
        ArenaScope::Staged => "staged",
    }
}

pub fn estimate_cost_usd(
    diff_bytes: usize,
    reviewers: &[ReviewerRef],
    rounds: Option<u8>,
    arbiter: Option<&ReviewerRef>,
    hub: &crate::config::AiHubConfig,
) -> f32 {
    let rounds_n = effective_arena_rounds(rounds);
    let rounds = rounds_n as f64;
    let reviewer_count = reviewers.len().max(1) as f64;
    let tokens_in = (diff_bytes as f64 * reviewer_count * rounds * 1.2) as f32;
    let mut rate_sum = 0.0f32;
    let mut n = 0u32;
    for rf in reviewers {
        rate_sum += model_cost_rate(hub, rf);
        n += 1;
    }
    let rate = if n > 0 { rate_sum / n as f32 } else { 0.02 };
    let mut cost = (tokens_in / 1000.0) * rate;
    if rounds_n >= 2 {
        let arb = arbiter.cloned().or_else(|| default_arbiter_from_hub(hub));
        if let Some(arb) = arb {
            let arb_rate = model_cost_rate(hub, &arb);
            let arb_tokens = (diff_bytes as f64 * 0.45 * 1.2) as f32;
            cost = (arb_tokens / 1000.0).mul_add(arb_rate, cost);
        }
    }
    cost
}

/// Diff size + cost/latency preview for launcher UI (same diff path as [`start_arena_run`]).
#[derive(Debug, Clone, Serialize)]
pub struct ArenaDiffPreview {
    pub diff_bytes: usize,
    pub cost_usd: f32,
    pub latency_sec: u32,
    pub cost_limit_usd: f32,
}

pub fn estimate_latency_sec(
    reviewers: &[ReviewerRef],
    rounds: Option<u8>,
    hub: &crate::config::AiHubConfig,
) -> u32 {
    let rounds = effective_arena_rounds(rounds);
    let mut max_latency = 0u32;
    for rf in reviewers {
        if let Some(p) = hub.providers.get(&rf.provider_id) {
            if let Some(m) = p.models.iter().find(|m| m.id == rf.model_id) {
                max_latency = max_latency.max(m.avg_latency_ms.unwrap_or(12_000));
            }
        }
    }
    if max_latency == 0 {
        max_latency = 12_000;
    }
    let sec = ((max_latency as f64) * (rounds as f64) * 0.85 / 1000.0).round() as u32;
    sec.max(5)
}

/// Cost/latency preview from an already-resolved raw diff (see [`TabState::raw_diff_for_arena`]).
pub fn build_arena_diff_preview(
    config: &ErConfig,
    raw_diff: &str,
    reviewers: &[ReviewerRef],
    rounds: Option<u8>,
    arbiter: Option<&ReviewerRef>,
) -> Result<ArenaDiffPreview> {
    let rounds_eff = effective_arena_rounds(rounds);
    let cost_usd = estimate_cost_usd(
        raw_diff.len(),
        reviewers,
        Some(rounds_eff),
        arbiter,
        &config.ai_hub,
    );
    let latency_sec = estimate_latency_sec(reviewers, Some(rounds_eff), &config.ai_hub);
    Ok(ArenaDiffPreview {
        diff_bytes: raw_diff.len(),
        cost_usd,
        latency_sec,
        cost_limit_usd: DEFAULT_COST_LIMIT_USD,
    })
}

pub fn reconcile_stale_runs(er_dir: &Path) -> Result<()> {
    for run_id in super::storage::list_run_ids(er_dir)? {
        let paths = ArenaPaths::for_run(er_dir, &run_id);
        if !paths.run_json().is_file() {
            continue;
        }
        let mut run = load_run(&paths)?;
        if matches!(run.status, RunStatus::Running { .. } | RunStatus::Queued) {
            run.status = RunStatus::Failed;
            save_run(&paths, &run)?;
        }
    }
    Ok(())
}

pub fn start_arena_run(
    registry: Arc<ArenaRegistry>,
    config: ErConfig,
    repo_root: String,
    er_dir: String,
    branch_ref: String,
    base_branch: String,
    raw_diff: String,
    params: ArenaStartParams,
) -> Result<String> {
    crate::dev_log::arena_line(format!(
        "start_arena_run repo={repo_root} branch={branch_ref} base={base_branch} diff_bytes={}",
        raw_diff.len()
    ));
    if params.reviewers.is_empty() {
        anyhow::bail!("arena requires at least one reviewer");
    }
    let mut rounds = effective_arena_rounds(params.rounds);
    if params.reviewers.len() == 1 {
        rounds = 1;
    } else if params.reviewers.len() < MIN_QUORUM {
        anyhow::bail!("arena requires at least {MIN_QUORUM} reviewers");
    } else if rounds > 1 && params.reviewers.len() < MIN_QUORUM {
        anyhow::bail!("arena requires at least {MIN_QUORUM} reviewers for {rounds} rounds");
    }

    if raw_diff.trim().is_empty() {
        let scope_label = scope_git_mode(params.scope);
        anyhow::bail!(
            "no diff for arena scope \"{scope_label}\" (base {base_branch}, branch {branch_ref}). \
             Use Branch or Selected files on this tab, or open a tab with changes."
        );
    }
    let arbiter_ref = resolve_arbiter(&params, &config.ai_hub)?;
    let run_effort = crate::config::resolve_effort(
        &config.ai_hub,
        &config.agent,
        None,
        params.effort.as_deref(),
    );
    let est = estimate_cost_usd(
        raw_diff.len(),
        &params.reviewers,
        Some(rounds),
        Some(&arbiter_ref),
        &config.ai_hub,
    );
    crate::dev_log::arena_line(format!(
        "diff_bytes={} rounds={rounds} est_usd={est:.2}",
        raw_diff.len()
    ));
    if est > DEFAULT_COST_LIMIT_USD && !params.confirm {
        crate::dev_log::arena_line(format!(
            "start blocked: cost ${est:.2} > limit (confirm=false)"
        ));
        anyhow::bail!(
            "estimated cost ${est:.2} exceeds limit ${DEFAULT_COST_LIMIT_USD:.2}; pass confirm=true"
        );
    }

    let run_id = new_run_id();
    crate::dev_log::arena_line(format!("run_id={run_id} spawning supervisor thread"));
    let paths = ArenaPaths::for_run(Path::new(&er_dir), &run_id);
    paths.ensure_dirs()?;
    save_diff_patch(&paths, &raw_diff)?;

    let diff_hash = compute_diff_hash(&raw_diff);
    let reviewers = resolve_reviewers(&config, &params.reviewers)?;

    let run = ArenaRun {
        id: run_id.clone(),
        title: params.title,
        branch_ref,
        base_branch,
        scope: params.scope,
        diff_hash,
        created_at: crate::app::chrono_now(),
        completed_at: None,
        status: RunStatus::Queued,
        config: ArenaConfig {
            reviewers: params.reviewers.clone(),
            rounds,
            arbiter: arbiter_ref,
            auto_accept_threshold: 0.75,
            scope: params.scope,
            files: params.files.clone(),
            run_kind: if params.agent_kind.is_some() {
                ArenaRunKind::Agent
            } else {
                ArenaRunKind::Models
            },
            agent_kind: params.agent_kind,
            effort: run_effort,
        },
        reviewers: reviewers.clone(),
        findings: vec![],
        accepted_finding_ids: vec![],
        cost_estimate: CostEstimate {
            tokens_in: raw_diff.len() as u64,
            tokens_out: 0,
            usd: est,
        },
    };
    save_run(&paths, &run)?;

    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_handle = Arc::clone(&cancel);
    let children = Arc::new(Mutex::new(Vec::new()));
    let children_handle = Arc::clone(&children);
    let status = Arc::new(Mutex::new(RunStatus::Running { round: 1 }));
    let status_handle = Arc::clone(&status);
    let registry_thread = Arc::clone(&registry);
    let run_id_thread = run_id.clone();
    let paths_clone = paths.clone();
    let patch_path = paths.diff_patch().display().to_string();

    let join = thread::spawn(move || {
        let result = run_supervisor(
            &registry_thread,
            &config,
            &repo_root,
            &paths_clone,
            &patch_path,
            run_id_thread.clone(),
            reviewers,
            cancel.clone(),
            children.clone(),
            status.clone(),
        );
        if let Err(e) = result {
            if is_cancelled_error(&e) {
                if let Ok(mut st) = status.lock() {
                    *st = RunStatus::Cancelled;
                }
                if let Ok(mut run) = load_run(&paths_clone) {
                    run.status = RunStatus::Cancelled;
                    run.completed_at = Some(crate::app::chrono_now());
                    let _ = save_run(&paths_clone, &run);
                }
            } else {
                crate::dev_log::arena_line(format!("run {} failed: {e:#}", run_id_thread));
                if let Ok(mut st) = status.lock() {
                    *st = RunStatus::Failed;
                }
                if let Ok(mut run) = load_run(&paths_clone) {
                    run.status = RunStatus::Failed;
                    run.completed_at = Some(crate::app::chrono_now());
                    let _ = save_run(&paths_clone, &run);
                }
            }
        }
        registry_thread.release_run(&run_id_thread);
        registry_thread.notify_progress();
    });

    let handle = ArenaRunHandle {
        cancel: cancel_handle,
        children: children_handle,
        status: status_handle,
        join: Some(join),
    };
    registry.insert(run_id.clone(), handle);

    Ok(run_id)
}

/// Start one arena/single run per agent group (parallel supervisors).
pub fn start_arena_batch(
    registry: Arc<ArenaRegistry>,
    config: ErConfig,
    repo_root: String,
    er_dir: String,
    branch_ref: String,
    base_branch: String,
    raw_diff: String,
    batch: ArenaBatchStartParams,
) -> Result<Vec<String>> {
    let mut run_ids = Vec::new();
    let mut total_est = 0.0f32;
    for group in &batch.groups {
        if group.models.is_empty() {
            continue;
        }
        let reviewers: Vec<ReviewerRef> = group
            .models
            .iter()
            .map(|m| ReviewerRef {
                provider_id: m.provider_id.clone(),
                model_id: m.model_id.clone(),
                agent_kind: Some(group.agent_kind.clone()),
            })
            .collect();
        let rounds = if reviewers.len() == 1 {
            Some(1u8)
        } else {
            batch.rounds
        };
        let arbiter = batch
            .arbiter
            .clone()
            .or_else(|| default_arbiter_from_hub(&config.ai_hub));
        total_est += estimate_cost_usd(
            raw_diff.len(),
            &reviewers,
            rounds,
            arbiter.as_ref(),
            &config.ai_hub,
        );
        if total_est > DEFAULT_COST_LIMIT_USD && !batch.confirm {
            anyhow::bail!(
                "estimated batch cost ${total_est:.2} exceeds limit ${DEFAULT_COST_LIMIT_USD:.2}; pass confirm=true"
            );
        }
        let title = group
            .title
            .clone()
            .or_else(|| agent_meta(&group.agent_kind).map(|a| format!("{} review", a.label)));
        let params = ArenaStartParams {
            title,
            reviewers,
            scope: batch.scope,
            files: batch.files.clone(),
            rounds,
            arbiter,
            confirm: true,
            agent_kind: Some(group.agent_kind.clone()),
            effort: batch.effort.clone(),
        };
        let id = start_arena_run(
            Arc::clone(&registry),
            config.clone(),
            repo_root.clone(),
            er_dir.clone(),
            branch_ref.clone(),
            base_branch.clone(),
            raw_diff.clone(),
            params,
        )?;
        run_ids.push(id);
    }
    if run_ids.is_empty() {
        anyhow::bail!("batch requires at least one agent group with models");
    }
    Ok(run_ids)
}

pub fn estimate_batch_cost_usd(
    diff_bytes: usize,
    batch: &ArenaBatchStartParams,
    hub: &crate::config::AiHubConfig,
) -> f32 {
    let mut total = 0.0f32;
    for group in &batch.groups {
        if group.models.is_empty() {
            continue;
        }
        let reviewers: Vec<ReviewerRef> = group
            .models
            .iter()
            .map(|m| ReviewerRef {
                provider_id: m.provider_id.clone(),
                model_id: m.model_id.clone(),
                agent_kind: Some(group.agent_kind.clone()),
            })
            .collect();
        let rounds = if reviewers.len() == 1 {
            Some(1u8)
        } else {
            batch.rounds
        };
        let default_arb = default_arbiter_from_hub(hub);
        let arb = batch.arbiter.as_ref().or(default_arb.as_ref());
        total += estimate_cost_usd(diff_bytes, &reviewers, rounds, arb, hub);
    }
    total
}

fn emit(registry: &ArenaRegistry, paths: &ArenaPaths, event: &ProgressEvent) {
    let _ = append_progress_event(paths, event);
    registry.notify_progress();
}

struct Round1ParallelOutcome {
    ok: Vec<(String, super::schema::Round1Output)>,
    failed: Vec<(String, String)>,
    cancelled: bool,
}

fn run_round1_parallel(
    registry: &ArenaRegistry,
    config: &ErConfig,
    repo_root: &str,
    paths: &ArenaPaths,
    patch_path: &str,
    reviewers: &[Reviewer],
    effort: Option<&str>,
    cancel: &Arc<AtomicBool>,
    children: &Arc<Mutex<Vec<Child>>>,
) -> Result<Round1ParallelOutcome> {
    for reviewer in reviewers {
        emit(
            registry,
            paths,
            &ProgressEvent::ReviewerThinking {
                reviewer_id: reviewer.id.clone(),
                round: 1,
            },
        );
    }

    let config = Arc::new(config.clone());
    let effort = effort.map(|s| s.to_string());
    let repo_root = repo_root.to_string();
    let patch_path = patch_path.to_string();
    let cancel = Arc::clone(cancel);
    let children = Arc::clone(children);
    let ok: Arc<Mutex<Vec<(String, super::schema::Round1Output)>>> =
        Arc::new(Mutex::new(Vec::new()));
    let failed: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let cancelled: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));

    let paths = paths.clone();
    let storage_dir = paths.root.to_string_lossy().into_owned();
    let mut handles = Vec::new();
    for reviewer in reviewers {
        let reviewer = reviewer.clone();
        let config = Arc::clone(&config);
        let effort = effort.clone();
        let repo_root = repo_root.clone();
        let patch_path = patch_path.clone();
        let paths = paths.clone();
        let storage_dir = storage_dir.clone();
        let cancel = Arc::clone(&cancel);
        let children = Arc::clone(&children);
        let ok = Arc::clone(&ok);
        let failed = Arc::clone(&failed);
        let cancelled = Arc::clone(&cancelled);
        handles.push(thread::spawn(move || {
            if cancel.load(Ordering::SeqCst) {
                cancelled.store(true, Ordering::SeqCst);
                return;
            }
            // Wait for a global agent slot so several runs (or runs with many
            // reviewers) can't spawn unbounded agent processes at once.
            let cap = config.ai_hub.effective_max_concurrent_reviews();
            let Some(_slot) = crate::agent_slots::acquire(cap, &cancel) else {
                cancelled.store(true, Ordering::SeqCst);
                return;
            };
            let cmd = match resolve_provider_command(
                &config.ai_hub,
                &reviewer.provider_id,
                &reviewer.model_id,
                effort.as_deref(),
                Some(storage_dir.as_str()),
            ) {
                Ok(c) => c,
                Err(e) => {
                    failed
                        .lock()
                        .unwrap()
                        .push((reviewer.id.clone(), e.to_string()));
                    return;
                }
            };
            let prompt = build_arena_round1_prompt_agent(
                &patch_path,
                &reviewer.name,
                reviewer.agent_kind.as_deref(),
            );
            match run_provider_json(&cmd, &prompt, &repo_root, &cancel, &children) {
                Ok(v) => match super::schema::validate_round1_output(&v) {
                    Ok(out) => {
                        let _ = save_round_output(&paths, 1, &reviewer.id, &v);
                        ok.lock().unwrap().push((reviewer.id.clone(), out));
                    }
                    Err(e) => {
                        failed
                            .lock()
                            .unwrap()
                            .push((reviewer.id.clone(), e.to_string()));
                    }
                },
                Err(e) => {
                    if is_cancelled_error(&e) {
                        cancelled.store(true, Ordering::SeqCst);
                    } else {
                        failed
                            .lock()
                            .unwrap()
                            .push((reviewer.id.clone(), e.to_string()));
                    }
                }
            }
        }));
    }

    for handle in handles {
        if handle.join().is_err() {
            anyhow::bail!("round 1 reviewer thread panicked");
        }
    }

    Ok(Round1ParallelOutcome {
        ok: Arc::try_unwrap(ok)
            .map_err(|_| anyhow::anyhow!("round1 ok lock"))?
            .into_inner()
            .unwrap(),
        failed: Arc::try_unwrap(failed)
            .map_err(|_| anyhow::anyhow!("round1 failed lock"))?
            .into_inner()
            .unwrap(),
        cancelled: cancelled.load(Ordering::SeqCst),
    })
}

struct Round2ParallelOutcome {
    ok: Vec<(String, super::schema::Round2Output)>,
    failed: Vec<(String, String)>,
    cancelled: bool,
}

fn run_round2_parallel(
    registry: &ArenaRegistry,
    config: &ErConfig,
    repo_root: &str,
    paths: &ArenaPaths,
    patch_path: &str,
    round: u8,
    findings_json: &str,
    reviewers: &[Reviewer],
    effort: Option<&str>,
    cancel: &Arc<AtomicBool>,
    children: &Arc<Mutex<Vec<Child>>>,
) -> Result<Round2ParallelOutcome> {
    for reviewer in reviewers {
        emit(
            registry,
            paths,
            &ProgressEvent::ReviewerThinking {
                reviewer_id: reviewer.id.clone(),
                round,
            },
        );
    }

    let config = Arc::new(config.clone());
    let effort = effort.map(|s| s.to_string());
    let repo_root = repo_root.to_string();
    let patch_path = patch_path.to_string();
    // One copy of the round findings payload, shared across all reviewer
    // threads (previously cloned per reviewer — O4).
    let findings_json = Arc::new(findings_json.to_string());
    let cancel = Arc::clone(cancel);
    let children = Arc::clone(children);
    let ok: Arc<Mutex<Vec<(String, super::schema::Round2Output)>>> =
        Arc::new(Mutex::new(Vec::new()));
    let failed: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let cancelled: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));

    let paths = paths.clone();
    let storage_dir = paths.root.to_string_lossy().into_owned();
    let mut handles = Vec::new();
    for reviewer in reviewers {
        let reviewer = reviewer.clone();
        let config = Arc::clone(&config);
        let effort = effort.clone();
        let repo_root = repo_root.clone();
        let patch_path = patch_path.clone();
        let findings_json = Arc::clone(&findings_json);
        let paths = paths.clone();
        let storage_dir = storage_dir.clone();
        let cancel = Arc::clone(&cancel);
        let children = Arc::clone(&children);
        let ok = Arc::clone(&ok);
        let failed = Arc::clone(&failed);
        let cancelled = Arc::clone(&cancelled);
        handles.push(thread::spawn(move || {
            if cancel.load(Ordering::SeqCst) {
                cancelled.store(true, Ordering::SeqCst);
                return;
            }
            let cap = config.ai_hub.effective_max_concurrent_reviews();
            let Some(_slot) = crate::agent_slots::acquire(cap, &cancel) else {
                cancelled.store(true, Ordering::SeqCst);
                return;
            };
            let cmd = match resolve_provider_command(
                &config.ai_hub,
                &reviewer.provider_id,
                &reviewer.model_id,
                effort.as_deref(),
                Some(storage_dir.as_str()),
            ) {
                Ok(c) => c,
                Err(e) => {
                    failed
                        .lock()
                        .unwrap()
                        .push((reviewer.id.clone(), e.to_string()));
                    return;
                }
            };
            let prompt =
                build_arena_round2_prompt(&patch_path, &reviewer.id, round, findings_json.as_str());
            match run_provider_json(&cmd, &prompt, &repo_root, &cancel, &children) {
                Ok(v) => match super::schema::validate_round2_output(&v) {
                    Ok(out) => {
                        let _ = save_round_output(&paths, round, &reviewer.id, &v);
                        ok.lock().unwrap().push((reviewer.id.clone(), out));
                    }
                    Err(e) => {
                        failed
                            .lock()
                            .unwrap()
                            .push((reviewer.id.clone(), e.to_string()));
                    }
                },
                Err(e) => {
                    if is_cancelled_error(&e) {
                        cancelled.store(true, Ordering::SeqCst);
                    } else {
                        failed
                            .lock()
                            .unwrap()
                            .push((reviewer.id.clone(), e.to_string()));
                    }
                }
            }
        }));
    }

    for handle in handles {
        if handle.join().is_err() {
            anyhow::bail!("round {round} reviewer thread panicked");
        }
    }

    Ok(Round2ParallelOutcome {
        ok: Arc::try_unwrap(ok)
            .map_err(|_| anyhow::anyhow!("round2 ok lock"))?
            .into_inner()
            .unwrap(),
        failed: Arc::try_unwrap(failed)
            .map_err(|_| anyhow::anyhow!("round2 failed lock"))?
            .into_inner()
            .unwrap(),
        cancelled: cancelled.load(Ordering::SeqCst),
    })
}

fn run_supervisor(
    registry: &ArenaRegistry,
    config: &ErConfig,
    repo_root: &str,
    paths: &ArenaPaths,
    patch_path: &str,
    run_id: String,
    reviewers: Vec<Reviewer>,
    cancel: Arc<AtomicBool>,
    children: Arc<Mutex<Vec<std::process::Child>>>,
    status: Arc<Mutex<RunStatus>>,
) -> Result<()> {
    let mut run = load_run(paths)?;
    let total_rounds = run.config.rounds;
    let run_effort = run.config.effort.clone();

    macro_rules! bail_cancelled {
        () => {
            run.status = RunStatus::Cancelled;
            run.completed_at = Some(crate::app::chrono_now());
            save_run(paths, &run)?;
            *status.lock().unwrap() = RunStatus::Cancelled;
            emit(
                registry,
                paths,
                &ProgressEvent::RunComplete {
                    run_id: run_id.clone(),
                },
            );
            return Ok(());
        };
    }

    macro_rules! cancelled {
        () => {
            if cancel.load(Ordering::SeqCst) || registry.is_cancelled(&run_id) {
                bail_cancelled!();
            }
        };
    }

    // Round 1
    cancelled!();
    *status.lock().unwrap() = RunStatus::Running { round: 1 };
    run.status = RunStatus::Running { round: 1 };
    save_run(paths, &run)?;
    emit(
        registry,
        paths,
        &ProgressEvent::RoundStarted {
            round: 1,
            total_rounds,
        },
    );

    cancelled!();
    let round1 = run_round1_parallel(
        registry,
        config,
        repo_root,
        paths,
        patch_path,
        &reviewers,
        run_effort.as_deref(),
        &cancel,
        &children,
    )?;
    if round1.cancelled {
        bail_cancelled!();
    }
    for (id, reason) in round1.failed {
        mark_reviewer_failed(&mut run, &id, &reason);
    }
    let round1_ok = round1.ok;
    for (reviewer_id, out) in &round1_ok {
        emit(
            registry,
            paths,
            &ProgressEvent::ReviewerDone {
                reviewer_id: reviewer_id.clone(),
                round: 1,
                findings_count: out.findings.len(),
            },
        );
    }
    save_run(paths, &run)?;

    let min_survivors = min_survivors_required(run.reviewers.len());
    if survivors(&run) < min_survivors {
        let reasons: Vec<String> = run
            .reviewers
            .iter()
            .filter_map(|r| {
                if let ReviewerRunStatus::Failed { reason } = &r.status {
                    Some(format!("{}: {reason}", r.name))
                } else {
                    None
                }
            })
            .collect();
        let detail = if reasons.is_empty() {
            "all reviewers failed".to_string()
        } else {
            reasons.join("; ")
        };
        anyhow::bail!(
            "insufficient reviewers after round 1 ({}/{} ok, need {min_survivors}): {detail}",
            survivors(&run),
            run.reviewers.len()
        );
    }

    run.findings = findings_from_round1(&round1_ok);

    if total_rounds < 2 {
        finalize_single_round_verdicts(&mut run.findings);
        run.status = RunStatus::Complete;
        run.completed_at = Some(crate::app::chrono_now());
        *status.lock().unwrap() = RunStatus::Complete;
        save_run(paths, &run)?;
        emit(registry, paths, &ProgressEvent::RunComplete { run_id });
        return Ok(());
    }

    for round in 2..=total_rounds {
        cancelled!();
        *status.lock().unwrap() = RunStatus::Running { round };
        run.status = RunStatus::Running { round };
        save_run(paths, &run)?;
        emit(
            registry,
            paths,
            &ProgressEvent::RoundStarted {
                round,
                total_rounds,
            },
        );

        let findings_json = serde_json::to_string(&run.findings)?;
        let active: Vec<Reviewer> = active_reviewers(&run, &reviewers)
            .into_iter()
            .cloned()
            .collect();
        cancelled!();
        let cross_out = run_round2_parallel(
            registry,
            config,
            repo_root,
            paths,
            patch_path,
            round,
            &findings_json,
            &active,
            run_effort.as_deref(),
            &cancel,
            &children,
        )?;
        if cross_out.cancelled {
            bail_cancelled!();
        }
        for (id, reason) in cross_out.failed {
            mark_reviewer_failed(&mut run, &id, &reason);
        }
        let cross_ok = cross_out.ok;
        for (reviewer_id, out) in &cross_ok {
            emit(
                registry,
                paths,
                &ProgressEvent::ReviewerDone {
                    reviewer_id: reviewer_id.clone(),
                    round,
                    findings_count: out.ballots.len(),
                },
            );
        }
        severity_from_cross_check(&mut run.findings, &cross_ok, round);
        save_run(paths, &run)?;
    }

    // Arbiter phase (after all reviewer cross-check rounds)
    cancelled!();
    let arbiter_ref = &run.config.arbiter;
    let arbiter_label = arbiter_display_label(&config.ai_hub, arbiter_ref);
    emit(
        registry,
        paths,
        &ProgressEvent::ArbiterStarted { arbiter_label },
    );
    *status.lock().unwrap() = RunStatus::Running {
        round: total_rounds,
    };
    run.status = RunStatus::Running {
        round: total_rounds,
    };
    save_run(paths, &run)?;

    let summary = json!({ "findings": run.findings });
    let prompt = build_arena_round3_prompt(&summary.to_string());
    let cmd = resolve_provider_command(
        &config.ai_hub,
        &arbiter_ref.provider_id,
        &arbiter_ref.model_id,
        run_effort.as_deref(),
        Some(paths.root.to_string_lossy().as_ref()),
    )?;
    emit(
        registry,
        paths,
        &ProgressEvent::ReviewerThinking {
            reviewer_id: ARBITER_REVIEWER_ID.to_string(),
            round: total_rounds,
        },
    );
    let v = match run_provider_json(&cmd, &prompt, repo_root, &cancel, &children) {
        Ok(v) => v,
        Err(e) if is_cancelled_error(&e) => {
            bail_cancelled!();
        }
        Err(e) => return Err(e),
    };
    let r3 = super::schema::validate_round3_output(&v)?;
    let _ = save_arbiter_output(paths, &v);
    apply_round3_verdicts(&mut run.findings, &r3, run.config.auto_accept_threshold);
    record_arbiter_ballots(&mut run.findings, &r3, ARBITER_REVIEWER_ID);

    for f in &run.findings {
        let verdict_str = match &f.verdict {
            Verdict::Kept => "kept",
            Verdict::Escalated => "escalated",
            Verdict::Dropped => "dropped",
            Verdict::Merged { .. } => "merged",
            Verdict::Pending => "pending",
        };
        emit(
            registry,
            paths,
            &ProgressEvent::FindingVerdict {
                finding_id: f.id.clone(),
                verdict: verdict_str.to_string(),
                confidence: f.confidence,
            },
        );
    }

    run.status = RunStatus::Complete;
    run.completed_at = Some(crate::app::chrono_now());
    *status.lock().unwrap() = RunStatus::Complete;
    save_run(paths, &run)?;
    emit(registry, paths, &ProgressEvent::RunComplete { run_id });
    Ok(())
}

fn mark_reviewer_failed(run: &mut ArenaRun, id: &str, reason: &str) {
    if let Some(r) = run.reviewers.iter_mut().find(|r| r.id == id) {
        r.status = ReviewerRunStatus::Failed {
            reason: reason.to_string(),
        };
    }
}

/// Single-round runs skip arbiter; mark proposed findings as kept so Review import works.
fn finalize_single_round_verdicts(findings: &mut [ArenaFinding]) {
    for f in findings {
        if matches!(f.verdict, Verdict::Pending) {
            f.verdict = Verdict::Kept;
        }
        if f.confidence <= 0.0 {
            f.confidence = 0.75;
        }
    }
}

fn survivors(run: &ArenaRun) -> usize {
    run.reviewers
        .iter()
        .filter(|r| matches!(r.status, ReviewerRunStatus::Ok))
        .count()
}

fn active_reviewers<'a>(run: &'a ArenaRun, all: &'a [Reviewer]) -> Vec<&'a Reviewer> {
    all.iter()
        .filter(|r| {
            run.reviewers
                .iter()
                .find(|x| x.id == r.id)
                .map(|x| matches!(x.status, ReviewerRunStatus::Ok))
                .unwrap_or(false)
        })
        .collect()
}

fn resolve_reviewers(config: &ErConfig, refs: &[ReviewerRef]) -> Result<Vec<Reviewer>> {
    let mut out = Vec::new();
    for (i, rf) in refs.iter().enumerate() {
        let provider = config
            .ai_hub
            .providers
            .get(&rf.provider_id)
            .with_context(|| format!("unknown provider {}", rf.provider_id))?;
        let model = provider
            .models
            .iter()
            .find(|m| m.id == rf.model_id)
            .with_context(|| format!("unknown model {}", rf.model_id))?;
        let agent_kind = rf.agent_kind.clone();
        let (kind, name, color, icon, tagline) = if let Some(ref ak) = agent_kind {
            let meta = agent_meta(ak).with_context(|| format!("unknown agent_kind {ak}"))?;
            let display = format!(
                "{} · {}",
                meta.label,
                model.label.as_deref().unwrap_or(&model.id)
            );
            (
                ReviewerKind::Agent,
                display,
                meta.color,
                meta.icon,
                meta.description,
            )
        } else {
            (
                ReviewerKind::Model,
                model.label.clone().unwrap_or_else(|| model.id.clone()),
                reviewer_color(i),
                "cube".to_string(),
                provider.display_name(&rf.provider_id),
            )
        };
        let id = if let Some(ref ak) = agent_kind {
            format!("{ak}::{}-{}", rf.provider_id, rf.model_id)
        } else {
            format!("{}-{}", rf.provider_id, rf.model_id)
        };
        out.push(Reviewer {
            id,
            name,
            kind,
            provider_id: rf.provider_id.clone(),
            model_id: rf.model_id.clone(),
            system_prompt: String::new(),
            color,
            icon,
            tagline,
            cost_per_1k_in: model.cost_per_1k_in.unwrap_or(0.015),
            cost_per_1k_out: model.cost_per_1k_out.unwrap_or(0.075),
            avg_latency_ms: model.avg_latency_ms.unwrap_or(12_000),
            status: ReviewerRunStatus::Ok,
            agent_kind,
        });
    }
    Ok(out)
}

fn reviewer_color(i: usize) -> String {
    const COLORS: &[&str] = &[
        "#ff7a2b", "#ff6b6b", "#7f87ff", "#4ec9a4", "#ffc457", "#5fd970",
    ];
    COLORS[i % COLORS.len()].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::ReviewerRef;
    use crate::config::{AiHubConfig, AiModelConfig, AiProviderConfig};

    #[test]
    fn effective_arena_rounds_clamps() {
        assert_eq!(effective_arena_rounds(None), 3);
        assert_eq!(effective_arena_rounds(Some(0)), 1);
        assert_eq!(effective_arena_rounds(Some(2)), 2);
        assert_eq!(effective_arena_rounds(Some(9)), 3);
    }

    #[test]
    fn min_survivors_required_scales_with_reviewer_count() {
        assert_eq!(min_survivors_required(1), 1);
        assert_eq!(min_survivors_required(2), 2);
        assert_eq!(min_survivors_required(6), 2);
    }

    #[test]
    fn estimate_latency_uses_max_model() {
        let mut hub = AiHubConfig::default();
        hub.providers.insert(
            "p".into(),
            AiProviderConfig {
                command: "true".into(),
                args: vec![],
                models: vec![
                    AiModelConfig {
                        id: "fast".into(),
                        label: None,
                        description: None,
                        args: vec![],
                        cost_per_1k_in: None,
                        cost_per_1k_out: None,
                        avg_latency_ms: Some(5_000),
                        effort_levels: vec![],
                        discovered: false,
                    },
                    AiModelConfig {
                        id: "slow".into(),
                        label: None,
                        description: None,
                        args: vec![],
                        cost_per_1k_in: None,
                        cost_per_1k_out: None,
                        avg_latency_ms: Some(20_000),
                        effort_levels: vec![],
                        discovered: false,
                    },
                ],
                ..Default::default()
            },
        );
        let refs = vec![
            ReviewerRef {
                provider_id: "p".into(),
                model_id: "fast".into(),
                agent_kind: None,
            },
            ReviewerRef {
                provider_id: "p".into(),
                model_id: "slow".into(),
                agent_kind: None,
            },
        ];
        let sec = estimate_latency_sec(&refs, Some(3), &hub);
        assert!(sec >= 50, "expected slow model to dominate, got {sec}");
    }

    #[test]
    fn estimate_cost_uses_selected_models() {
        let mut hub = AiHubConfig::default();
        hub.providers.insert(
            "cheap".into(),
            AiProviderConfig {
                command: "true".into(),
                args: vec![],
                models: vec![AiModelConfig {
                    id: "m1".into(),
                    label: None,
                    description: None,
                    args: vec![],
                    cost_per_1k_in: Some(0.001),
                    cost_per_1k_out: Some(0.001),
                    avg_latency_ms: None,
                    effort_levels: vec![],
                    discovered: false,
                }],
                ..Default::default()
            },
        );
        hub.providers.insert(
            "dear".into(),
            AiProviderConfig {
                command: "true".into(),
                args: vec![],
                models: vec![AiModelConfig {
                    id: "m2".into(),
                    label: None,
                    description: None,
                    args: vec![],
                    cost_per_1k_in: Some(0.1),
                    cost_per_1k_out: Some(0.1),
                    avg_latency_ms: None,
                    effort_levels: vec![],
                    discovered: false,
                }],
                ..Default::default()
            },
        );
        let cheap = vec![ReviewerRef {
            provider_id: "cheap".into(),
            model_id: "m1".into(),
            agent_kind: None,
        }];
        let dear = vec![ReviewerRef {
            provider_id: "dear".into(),
            model_id: "m2".into(),
            agent_kind: None,
        }];
        let low = estimate_cost_usd(10_000, &cheap, Some(3), None, &hub);
        let high = estimate_cost_usd(10_000, &dear, Some(3), None, &hub);
        assert!(high > low * 5.0);
    }

    #[test]
    fn default_arbiter_picks_most_expensive_model() {
        let mut hub = AiHubConfig::default();
        hub.providers.insert(
            "p".into(),
            AiProviderConfig {
                command: "true".into(),
                args: vec![],
                models: vec![
                    AiModelConfig {
                        id: "cheap".into(),
                        label: None,
                        description: None,
                        args: vec![],
                        cost_per_1k_in: Some(0.001),
                        cost_per_1k_out: Some(0.001),
                        avg_latency_ms: None,
                        effort_levels: vec![],
                        discovered: false,
                    },
                    AiModelConfig {
                        id: "dear".into(),
                        label: None,
                        description: None,
                        args: vec![],
                        cost_per_1k_in: Some(0.2),
                        cost_per_1k_out: Some(0.2),
                        avg_latency_ms: None,
                        effort_levels: vec![],
                        discovered: false,
                    },
                ],
                ..Default::default()
            },
        );
        let arb = default_arbiter_from_hub(&hub).expect("arbiter");
        assert_eq!(arb.model_id, "dear");
    }

    /// Two models on one expensive provider, so a batch group is guaranteed to blow the
    /// $25 limit long before any supervisor thread could be spawned.
    fn pricey_hub() -> AiHubConfig {
        let model = |id: &str| AiModelConfig {
            id: id.into(),
            label: None,
            description: None,
            args: vec![],
            cost_per_1k_in: Some(0.2),
            cost_per_1k_out: Some(0.2),
            avg_latency_ms: None,
            effort_levels: vec![],
            discovered: false,
        };
        let mut hub = AiHubConfig::default();
        hub.providers.insert(
            "p".into(),
            AiProviderConfig {
                command: "true".into(),
                args: vec![],
                models: vec![model("m1"), model("m2")],
                ..Default::default()
            },
        );
        hub
    }

    fn pricey_models() -> Vec<ReviewerRef> {
        vec![
            ReviewerRef {
                provider_id: "p".into(),
                model_id: "m1".into(),
                agent_kind: None,
            },
            ReviewerRef {
                provider_id: "p".into(),
                model_id: "m2".into(),
                agent_kind: None,
            },
        ]
    }

    fn batch_of(groups: Vec<AgentGroupStart>) -> ArenaBatchStartParams {
        ArenaBatchStartParams {
            scope: ArenaScope::Branch,
            files: None,
            rounds: Some(3),
            arbiter: None,
            confirm: false,
            groups,
            effort: None,
        }
    }

    /// Runs `start_arena_batch` on paths that are never touched unless a run actually
    /// starts — every assertion below is on a bail path, so no provider is ever spawned.
    fn run_batch(
        config: &ErConfig,
        raw_diff: &str,
        batch: ArenaBatchStartParams,
    ) -> Result<Vec<String>> {
        let dir = tempfile::tempdir().unwrap();
        let notify: crate::arena::ArenaNotify = Arc::new(|| {});
        let registry = Arc::new(ArenaRegistry::new(notify));
        start_arena_batch(
            registry,
            config.clone(),
            dir.path().display().to_string(),
            dir.path().join(".er").display().to_string(),
            "feature/x".into(),
            "main".into(),
            raw_diff.to_string(),
            batch,
        )
    }

    #[test]
    fn start_arena_batch_rejects_a_batch_with_no_groups() {
        let err = run_batch(
            &ErConfig::default(),
            "diff --git a/a b/a\n",
            batch_of(vec![]),
        )
        .expect_err("an empty batch must not report success");
        assert!(
            err.to_string().contains("at least one agent group"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn start_arena_batch_skips_groups_without_models_and_keeps_iterating() {
        let config = ErConfig {
            ai_hub: pricey_hub(),
            ..Default::default()
        };
        let batch = batch_of(vec![
            AgentGroupStart {
                agent_kind: "expert:security".into(),
                models: vec![],
                title: None,
            },
            AgentGroupStart {
                agent_kind: "general".into(),
                models: pricey_models(),
                title: None,
            },
        ]);
        let err = run_batch(&config, &"x".repeat(200_000), batch)
            .expect_err("the second group is over the cost limit");
        // Reaching the cost check proves the models-less group was skipped rather than
        // ending the loop — otherwise this would be the "no agent group" error.
        assert!(
            err.to_string().contains("exceeds limit"),
            "expected the cost bail from the second group, got: {err}"
        );

        // And a models-less group contributes no run at all: dropping the `continue`
        // would hand an empty reviewer list to start_arena_run, whose own guard reports
        // "arena requires at least one reviewer" instead. A tiny diff keeps the cost
        // check from masking which bail fires.
        let only_empty = batch_of(vec![AgentGroupStart {
            agent_kind: "expert:security".into(),
            models: vec![],
            title: None,
        }]);
        let err = run_batch(&config, "diff --git a/a b/a\n", only_empty)
            .expect_err("a batch of models-less groups starts nothing");
        assert!(
            err.to_string().contains("at least one agent group"),
            "a skipped group must not reach start_arena_run, got: {err}"
        );
    }

    #[test]
    fn start_arena_batch_bails_on_cost_before_starting_any_run() {
        let config = ErConfig {
            ai_hub: pricey_hub(),
            ..Default::default()
        };
        let raw_diff = "x".repeat(200_000);
        let batch = batch_of(vec![AgentGroupStart {
            agent_kind: "general".into(),
            models: pricey_models(),
            title: None,
        }]);
        let expected = estimate_batch_cost_usd(raw_diff.len(), &batch, &config.ai_hub);
        assert!(
            expected > DEFAULT_COST_LIMIT_USD,
            "fixture must exceed the limit, estimated ${expected:.2}"
        );

        let err = run_batch(&config, &raw_diff, batch)
            .expect_err("an unconfirmed over-limit batch must not start");
        let msg = err.to_string();
        assert!(
            msg.contains(&format!("${expected:.2}")),
            "the bail must quote the same estimate as estimate_batch_cost_usd: {msg}"
        );
        assert!(
            msg.contains(&format!("${DEFAULT_COST_LIMIT_USD:.2}")),
            "the bail must name the limit: {msg}"
        );
        assert!(
            msg.contains("confirm=true"),
            "the bail must tell the caller how to override: {msg}"
        );
    }

    // ---- live-run fixtures ---------------------------------------------------------
    //
    // A provider is nothing but "a command whose stdout holds JSON", so a provider
    // whose command is `/bin/echo` and whose single argument *is* the JSON drives the
    // real `resolve_provider_command` → `run_once` → `extract_json_from_stdout` path
    // with no network, no agent CLI, and no fake injected anywhere. The schema structs
    // ignore unknown fields, so one `{findings, ballots, verdicts}` payload satisfies
    // the round-1, round-2 and round-3 validators in turn.

    use crate::ai::RiskLevel;

    /// Echoes its payload and exits 0.
    const ECHO: &str = "/bin/echo";
    /// Never spawns — `Command::spawn` fails with a Fatal (non-retried) error.
    const MISSING_CMD: &str = "/nonexistent/er-arena-missing-provider";

    fn model_named(id: &str) -> AiModelConfig {
        AiModelConfig {
            id: id.into(),
            ..Default::default()
        }
    }

    fn provider_of(
        command: &str,
        args: Vec<String>,
        models: Vec<AiModelConfig>,
    ) -> AiProviderConfig {
        AiProviderConfig {
            command: command.into(),
            args,
            models,
            ..Default::default()
        }
    }

    fn hub_of(entries: Vec<(&str, AiProviderConfig)>) -> AiHubConfig {
        let mut hub = AiHubConfig::default();
        for (id, provider) in entries {
            hub.providers.insert(id.to_string(), provider);
        }
        hub
    }

    fn config_with(hub: AiHubConfig) -> ErConfig {
        ErConfig {
            ai_hub: hub,
            ..Default::default()
        }
    }

    fn rf(provider_id: &str, model_id: &str) -> ReviewerRef {
        ReviewerRef {
            provider_id: provider_id.into(),
            model_id: model_id.into(),
            agent_kind: None,
        }
    }

    fn notify_registry() -> Arc<ArenaRegistry> {
        let notify: crate::arena::ArenaNotify = Arc::new(|| {});
        Arc::new(ArenaRegistry::new(notify))
    }

    /// The finding every fixture payload proposes, keyed the way `findings_from_round1`
    /// keys it — so the ballots and verdicts below can name it.
    fn fixture_finding_id() -> String {
        crate::arena::finding_id("src/a.rs", "", "Leaky buffer")
    }

    /// One payload that validates as round-1 findings, round-2 ballots and round-3
    /// verdicts. Round 1 proposes `low`; the cross check escalates it.
    fn combined_payload(finding_id: &str) -> String {
        json!({
            "findings": [{
                "file": "src/a.rs",
                "line": 7,
                "title": "Leaky buffer",
                "body": "grows without bound",
                "severity": "low"
            }],
            "ballots": [{
                "finding_id": finding_id,
                "vote": "escalate",
                "note": "still reachable after the fix"
            }],
            "verdicts": [{
                "finding_id": finding_id,
                "verdict": "kept",
                "confidence": 0.9,
                "rationale": "reproduced on main"
            }]
        })
        .to_string()
    }

    /// Provider `p` with reviewer models `m1`/`m2` and arbiter model `arb`.
    fn echo_hub(payload: &str) -> AiHubConfig {
        hub_of(vec![(
            "p",
            provider_of(
                ECHO,
                vec![payload.to_string()],
                vec![
                    model_named("m1"),
                    model_named("m2"),
                    AiModelConfig {
                        id: "arb".into(),
                        label: Some("Arbiter Prime".into()),
                        ..Default::default()
                    },
                ],
            ),
        )])
    }

    fn params_for(reviewers: Vec<ReviewerRef>, rounds: Option<u8>) -> ArenaStartParams {
        ArenaStartParams {
            title: Some("arena fixture".into()),
            reviewers,
            scope: ArenaScope::Branch,
            files: None,
            rounds,
            arbiter: Some(rf("p", "arb")),
            confirm: false,
            agent_kind: None,
            effort: None,
        }
    }

    /// Start a run against a throwaway repo root + `.er` dir. The returned `TempDir`
    /// must outlive the supervisor thread.
    fn try_start(
        config: &ErConfig,
        raw_diff: &str,
        params: ArenaStartParams,
    ) -> (tempfile::TempDir, Arc<ArenaRegistry>, Result<String>) {
        let dir = tempfile::tempdir().unwrap();
        let registry = notify_registry();
        let result = start_arena_run(
            Arc::clone(&registry),
            config.clone(),
            dir.path().display().to_string(),
            dir.path().join(".er").display().to_string(),
            "feature/x".into(),
            "main".into(),
            raw_diff.to_string(),
            params,
        );
        (dir, registry, result)
    }

    /// Block until the supervisor thread releases the run, then read what it persisted.
    fn wait_for_run(registry: &ArenaRegistry, run_id: &str, paths: &ArenaPaths) -> ArenaRun {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while registry.is_active(run_id) {
            assert!(
                std::time::Instant::now() < deadline,
                "arena run {run_id} never finished"
            );
            thread::sleep(std::time::Duration::from_millis(5));
        }
        load_run(paths).expect("run.json after the supervisor finished")
    }

    fn progress_events(paths: &ArenaPaths) -> Vec<ProgressEvent> {
        let text = std::fs::read_to_string(paths.progress_jsonl()).unwrap_or_default();
        text.lines()
            .filter_map(|line| serde_json::from_str::<ProgressEvent>(line).ok())
            .collect()
    }

    fn rounds_started(events: &[ProgressEvent]) -> Vec<u8> {
        events
            .iter()
            .filter_map(|e| match e {
                ProgressEvent::RoundStarted { round, .. } => Some(*round),
                _ => None,
            })
            .collect()
    }

    fn thinking(events: &[ProgressEvent]) -> Vec<(String, u8)> {
        let mut out: Vec<(String, u8)> = events
            .iter()
            .filter_map(|e| match e {
                ProgressEvent::ReviewerThinking { reviewer_id, round } => {
                    Some((reviewer_id.clone(), *round))
                }
                _ => None,
            })
            .collect();
        out.sort();
        out
    }

    fn arbiter_labels(events: &[ProgressEvent]) -> Vec<String> {
        events
            .iter()
            .filter_map(|e| match e {
                ProgressEvent::ArbiterStarted { arbiter_label } => Some(arbiter_label.clone()),
                _ => None,
            })
            .collect()
    }

    fn run_completed(events: &[ProgressEvent]) -> bool {
        events
            .iter()
            .any(|e| matches!(e, ProgressEvent::RunComplete { .. }))
    }

    fn ballots_at(finding: &ArenaFinding, n: u8) -> Vec<Ballot> {
        finding
            .rounds
            .iter()
            .filter(|r| r.n == n)
            .flat_map(|r| r.log.clone())
            .collect()
    }

    fn failure_reason(run: &ArenaRun, id: &str) -> String {
        let reviewer = run
            .reviewers
            .iter()
            .find(|r| r.id == id)
            .unwrap_or_else(|| panic!("no reviewer {id} in run.json"));
        match &reviewer.status {
            ReviewerRunStatus::Failed { reason } => reason.clone(),
            ReviewerRunStatus::Ok => panic!("reviewer {id} was not marked failed"),
        }
    }

    // ---- start_arena_run -----------------------------------------------------------

    #[test]
    fn start_arena_run_rejects_an_empty_reviewer_list() {
        let config = config_with(echo_hub("{}"));
        let (_dir, _registry, result) =
            try_start(&config, "diff --git a/a b/a\n", params_for(vec![], Some(3)));
        let err = result.expect_err("a run with nobody to review it must not start");
        assert!(
            err.to_string().contains("at least one reviewer"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn start_arena_run_rejects_an_empty_diff_and_names_the_scope_and_branches() {
        let config = config_with(echo_hub("{}"));
        let mut params = params_for(vec![rf("p", "m1")], Some(3));
        params.scope = ArenaScope::Unstaged;
        // Whitespace only — the guard trims before deciding there is nothing to review.
        let (_dir, _registry, result) = try_start(&config, "  \n\t\n", params);

        let err = result.expect_err("an empty diff must not start a run");
        let msg = err.to_string();
        assert!(
            msg.contains("no diff for arena scope \"unstaged\""),
            "the bail must name the scope that came up empty: {msg}"
        );
        assert!(
            msg.contains("base main") && msg.contains("branch feature/x"),
            "the bail must name the diff endpoints so the user can tell why: {msg}"
        );
    }

    #[test]
    fn start_arena_run_blocks_an_over_limit_estimate_until_confirm_is_passed() {
        let pricey = |id: &str| AiModelConfig {
            id: id.into(),
            cost_per_1k_in: Some(0.2),
            cost_per_1k_out: Some(0.2),
            ..Default::default()
        };
        // The command never spawns, so the confirmed half below fails fast instead of
        // waiting on an agent — the cost gate is all this test is about.
        let config = config_with(hub_of(vec![(
            "p",
            provider_of(
                MISSING_CMD,
                vec!["{prompt}".into()],
                vec![pricey("m1"), pricey("arb")],
            ),
        )]));
        let raw_diff = "x".repeat(200_000);
        let reviewers = vec![rf("p", "m1")];
        // A solo run is forced to one round, so the estimate must be quoted for one.
        let est = estimate_cost_usd(
            raw_diff.len(),
            &reviewers,
            Some(1),
            Some(&rf("p", "arb")),
            &config.ai_hub,
        );
        assert!(
            est > DEFAULT_COST_LIMIT_USD,
            "fixture must exceed the limit, estimated ${est:.2}"
        );

        let (_dir, _registry, blocked) =
            try_start(&config, &raw_diff, params_for(reviewers.clone(), Some(3)));
        let msg = blocked
            .expect_err("an unconfirmed over-limit run must not start")
            .to_string();
        assert!(
            msg.contains(&format!("${est:.2}")) && msg.contains("confirm=true"),
            "the bail must quote the estimate and the override: {msg}"
        );

        // confirm=true makes the limit advisory: the run starts, and the estimate it
        // was warned about is what gets persisted on the run.
        let mut confirmed = params_for(reviewers, Some(3));
        confirmed.confirm = true;
        let (dir, registry, started) = try_start(&config, &raw_diff, confirmed);
        let run_id = started.expect("a confirmed over-limit run must start");
        let paths = ArenaPaths::for_run(&dir.path().join(".er"), &run_id);
        let run = wait_for_run(&registry, &run_id, &paths);

        assert!(
            run.cost_estimate.usd > DEFAULT_COST_LIMIT_USD,
            "the persisted estimate must be the one the gate objected to, got ${:.2}",
            run.cost_estimate.usd
        );
        // ...and an unspawnable provider ends the run as Failed, not Complete.
        assert_eq!(run.status, RunStatus::Failed);
        assert!(run.completed_at.is_some());
        assert!(
            failure_reason(&run, "p-m1").contains("spawn"),
            "the reviewer failure must carry the spawn error"
        );
    }

    #[test]
    fn start_arena_run_forces_a_solo_reviewer_to_a_single_round_with_no_arbiter() {
        let fid = fixture_finding_id();
        let config = config_with(echo_hub(&combined_payload(&fid)));
        let raw_diff = "diff --git a/src/a.rs b/src/a.rs\n+let leak = alloc();\n";
        // Three rounds are requested, but one reviewer has nobody to cross-check with.
        let mut params = params_for(vec![rf("p", "m1")], Some(3));
        params.agent_kind = Some("general".into());

        let (dir, registry, started) = try_start(&config, raw_diff, params);
        let run_id = started.expect("start_arena_run");
        let paths = ArenaPaths::for_run(&dir.path().join(".er"), &run_id);
        let run = wait_for_run(&registry, &run_id, &paths);

        assert_eq!(run.config.rounds, 1, "a solo run cannot cross-check");
        assert_eq!(run.config.run_kind, ArenaRunKind::Agent);
        assert_eq!(run.config.agent_kind.as_deref(), Some("general"));
        assert_eq!(run.title.as_deref(), Some("arena fixture"));
        assert_eq!(run.branch_ref, "feature/x");
        assert_eq!(run.base_branch, "main");
        assert_eq!(run.diff_hash, compute_diff_hash(raw_diff));
        assert_eq!(run.cost_estimate.tokens_in, raw_diff.len() as u64);
        assert_eq!(run.reviewers.len(), 1);
        assert_eq!(run.reviewers[0].id, "p-m1");
        assert_eq!(run.status, RunStatus::Complete);
        assert!(run.completed_at.is_some());
        assert!(paths.diff_patch().is_file());

        // The round-1 proposal survives, and single-round runs stand in for the
        // arbiter by keeping every pending finding — otherwise Review import
        // would receive a finding nobody ever adjudicated.
        assert_eq!(run.findings.len(), 1);
        let finding = &run.findings[0];
        assert_eq!(finding.id, fid);
        assert_eq!(finding.file, "src/a.rs");
        assert_eq!(finding.line, Some(7));
        assert_eq!(finding.verdict, Verdict::Kept);
        assert_eq!(finding.severity_by_round.get(&1), Some(&RiskLevel::Low));
        assert!(
            finding.severity_by_round.get(&2).is_none(),
            "no cross-check round ran, so there is no round-2 severity"
        );
        assert!(
            (finding.confidence - 0.5).abs() < 1e-6,
            "the round-1 default confidence must survive, got {}",
            finding.confidence
        );

        assert!(paths.round_reviewer_json(1, "p-m1").is_file());
        assert!(!paths.round_reviewer_json(2, "p-m1").exists());
        assert!(
            !paths.arbiter_output_json().exists(),
            "a single-round run must not invoke the arbiter"
        );

        let events = progress_events(&paths);
        assert_eq!(rounds_started(&events), vec![1]);
        assert!(arbiter_labels(&events).is_empty());
        assert!(run_completed(&events));
    }

    #[test]
    fn start_arena_run_cross_checks_every_round_then_lets_the_arbiter_rule() {
        let fid = fixture_finding_id();
        let config = config_with(echo_hub(&combined_payload(&fid)));
        let raw_diff = "diff --git a/src/a.rs b/src/a.rs\n+let leak = alloc();\n";

        let (dir, registry, started) = try_start(
            &config,
            raw_diff,
            params_for(vec![rf("p", "m1"), rf("p", "m2")], Some(3)),
        );
        let run_id = started.expect("start_arena_run");
        let paths = ArenaPaths::for_run(&dir.path().join(".er"), &run_id);
        let run = wait_for_run(&registry, &run_id, &paths);

        assert_eq!(run.status, RunStatus::Complete);
        assert_eq!(run.config.rounds, 3);
        assert_eq!(run.config.run_kind, ArenaRunKind::Models);

        // Both reviewers proposed the same finding, so it collapses to one entry
        // credited to both.
        assert_eq!(run.findings.len(), 1);
        let finding = &run.findings[0];
        let mut raised_by = finding.raised_by.clone();
        raised_by.sort();
        assert_eq!(raised_by, vec!["p-m1".to_string(), "p-m2".to_string()]);

        // Round 1 proposed `low`; every cross-check round voted escalate.
        assert_eq!(finding.severity_by_round.get(&1), Some(&RiskLevel::Low));
        assert_eq!(finding.severity_by_round.get(&2), Some(&RiskLevel::High));
        assert_eq!(finding.severity_by_round.get(&3), Some(&RiskLevel::High));

        // One log per round plus the arbiter's own pseudo-round.
        assert_eq!(
            finding.rounds.iter().map(|r| r.n).collect::<Vec<_>>(),
            vec![1, 2, 3, ARENA_ARBITER_ROUND]
        );
        let round2 = ballots_at(finding, 2);
        let mut voters = round2
            .iter()
            .map(|b| b.reviewer.clone())
            .collect::<Vec<_>>();
        voters.sort();
        assert_eq!(voters, vec!["p-m1".to_string(), "p-m2".to_string()]);
        assert!(round2.iter().all(|b| b.vote == Vote::Escalate));

        let arbiter_ballots = ballots_at(finding, ARENA_ARBITER_ROUND);
        assert_eq!(arbiter_ballots.len(), 1);
        assert_eq!(arbiter_ballots[0].reviewer, ARBITER_REVIEWER_ID);
        assert_eq!(arbiter_ballots[0].vote, Vote::Keep);

        assert_eq!(finding.verdict, Verdict::Kept);
        assert_eq!(finding.rationale, "reproduced on main");
        assert!(
            (finding.confidence - 0.9).abs() < 1e-6,
            "the arbiter's confidence must be copied verbatim, got {}",
            finding.confidence
        );

        assert!(paths.round_reviewer_json(1, "p-m1").is_file());
        assert!(paths.round_reviewer_json(2, "p-m2").is_file());
        assert!(paths.round_reviewer_json(3, "p-m1").is_file());
        assert!(paths.arbiter_output_json().is_file());

        let events = progress_events(&paths);
        assert_eq!(rounds_started(&events), vec![1, 2, 3]);
        assert_eq!(
            arbiter_labels(&events),
            vec!["Arbiter Prime".to_string()],
            "the arbiter badge must use the configured model label, not its id"
        );
        assert!(events.iter().any(|e| matches!(
            e,
            ProgressEvent::FindingVerdict { verdict, .. } if verdict == "kept"
        )));
        assert!(run_completed(&events));
    }

    // ---- run_supervisor ------------------------------------------------------------

    fn test_reviewer(id: &str, provider_id: &str, model_id: &str) -> Reviewer {
        Reviewer {
            id: id.into(),
            name: id.into(),
            kind: ReviewerKind::Model,
            provider_id: provider_id.into(),
            model_id: model_id.into(),
            system_prompt: String::new(),
            color: "#ff7a2b".into(),
            icon: "cube".into(),
            tagline: String::new(),
            cost_per_1k_in: 0.0,
            cost_per_1k_out: 0.0,
            avg_latency_ms: 1,
            status: ReviewerRunStatus::Ok,
            agent_kind: None,
        }
    }

    /// Persist the `run.json` a supervisor expects to find. `run.reviewers` must carry
    /// the same ids as the `reviewers` argument — `survivors` and `mark_reviewer_failed`
    /// both look reviewers up there, not in the argument.
    fn seed_run(paths: &ArenaPaths, run_id: &str, reviewers: &[Reviewer], rounds: u8) {
        let run = ArenaRun {
            id: run_id.to_string(),
            title: None,
            branch_ref: "feature/x".into(),
            base_branch: "main".into(),
            scope: ArenaScope::Branch,
            diff_hash: "hash".into(),
            created_at: "2026-05-27T00:00:00Z".into(),
            completed_at: None,
            status: RunStatus::Queued,
            config: ArenaConfig {
                reviewers: reviewers
                    .iter()
                    .map(|r| rf(&r.provider_id, &r.model_id))
                    .collect(),
                rounds,
                arbiter: rf("p", "arb"),
                auto_accept_threshold: 0.75,
                scope: ArenaScope::Branch,
                files: None,
                run_kind: ArenaRunKind::Models,
                agent_kind: None,
                effort: None,
            },
            reviewers: reviewers.to_vec(),
            findings: vec![],
            accepted_finding_ids: vec![],
            cost_estimate: CostEstimate {
                tokens_in: 0,
                tokens_out: 0,
                usd: 0.0,
            },
        };
        save_run(paths, &run).unwrap();
    }

    struct SupervisorCall {
        dir: tempfile::TempDir,
        registry: Arc<ArenaRegistry>,
        paths: ArenaPaths,
        status: Arc<Mutex<RunStatus>>,
    }

    impl SupervisorCall {
        fn new(run_id: &str, reviewers: &[Reviewer], rounds: u8, register: bool) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let paths = ArenaPaths::for_run(&dir.path().join(".er"), run_id);
            seed_run(&paths, run_id, reviewers, rounds);
            let registry = notify_registry();
            if register {
                registry.insert_active_for_test(run_id, RunStatus::Queued);
            }
            Self {
                dir,
                registry,
                paths,
                status: Arc::new(Mutex::new(RunStatus::Queued)),
            }
        }

        fn run(
            &self,
            config: &ErConfig,
            run_id: &str,
            reviewers: Vec<Reviewer>,
            cancel: bool,
        ) -> Result<()> {
            run_supervisor(
                &self.registry,
                config,
                self.dir.path().to_str().unwrap(),
                &self.paths,
                "diff.patch",
                run_id.to_string(),
                reviewers,
                Arc::new(AtomicBool::new(cancel)),
                Arc::new(Mutex::new(Vec::new())),
                Arc::clone(&self.status),
            )
        }
    }

    #[test]
    fn run_supervisor_records_a_cancelled_run_without_starting_round_one() {
        // "ghost" is absent from the default (empty) hub, so had round 1 begun the
        // reviewer would have been marked Failed instead of staying Ok.
        let reviewers = vec![test_reviewer("rev-1", "ghost", "m1")];
        let call = SupervisorCall::new("run-cancel", &reviewers, 3, true);

        call.run(&ErConfig::default(), "run-cancel", reviewers, true)
            .expect("a cancelled run is a normal outcome, not an error");

        let run = load_run(&call.paths).unwrap();
        assert_eq!(run.status, RunStatus::Cancelled);
        assert!(run.completed_at.is_some());
        assert_eq!(*call.status.lock().unwrap(), RunStatus::Cancelled);
        assert!(matches!(run.reviewers[0].status, ReviewerRunStatus::Ok));

        let events = progress_events(&call.paths);
        assert!(
            rounds_started(&events).is_empty(),
            "no round may start after cancellation"
        );
        assert!(
            run_completed(&events),
            "the UI still needs a RunComplete to stop showing the run as live"
        );
    }

    #[test]
    fn run_supervisor_treats_a_run_missing_from_the_registry_as_cancelled() {
        // `ArenaRegistry::is_cancelled` fails safe: a supervisor whose handle is gone
        // (killed, or dropped by the desktop) must stop rather than keep spending.
        let reviewers = vec![test_reviewer("rev-1", "ghost", "m1")];
        let call = SupervisorCall::new("run-orphan", &reviewers, 3, false);

        call.run(&ErConfig::default(), "run-orphan", reviewers, false)
            .expect("an orphaned run stops cleanly");

        assert_eq!(load_run(&call.paths).unwrap().status, RunStatus::Cancelled);
        assert!(rounds_started(&progress_events(&call.paths)).is_empty());
    }

    #[test]
    fn run_supervisor_bails_when_too_few_reviewers_survive_round_one() {
        let reviewers = vec![
            test_reviewer("rev-1", "ghost", "m1"),
            test_reviewer("rev-2", "ghost", "m2"),
        ];
        let call = SupervisorCall::new("run-quorum", &reviewers, 3, true);

        let err = call
            .run(&ErConfig::default(), "run-quorum", reviewers, false)
            .expect_err("a run with no surviving reviewer must not proceed to round 2");
        let msg = err.to_string();
        assert!(
            msg.contains("insufficient reviewers after round 1 (0/2 ok, need 2)"),
            "the bail must quote the quorum it missed: {msg}"
        );
        // Every failure is named, so the user learns *why* the arena collapsed.
        assert!(
            msg.contains("rev-1: unknown provider: ghost")
                && msg.contains("rev-2: unknown provider: ghost"),
            "the bail must list each reviewer's reason: {msg}"
        );

        // The failures are persisted before the bail — the desktop reads them off run.json.
        let run = load_run(&call.paths).unwrap();
        assert!(failure_reason(&run, "rev-1").contains("unknown provider: ghost"));
        assert!(failure_reason(&run, "rev-2").contains("unknown provider: ghost"));
        assert!(
            !paths_has_round_output(&call.paths, 1),
            "no reviewer produced output, so no round-1 file may exist"
        );
    }

    fn paths_has_round_output(paths: &ArenaPaths, round: u8) -> bool {
        std::fs::read_dir(paths.round_dir(round))
            .map(|entries| entries.flatten().count() > 0)
            .unwrap_or(false)
    }

    #[test]
    fn run_supervisor_reports_all_reviewers_failed_when_the_run_has_no_reviewers() {
        // A run whose reviewer list is empty produces no per-reviewer reasons, so the
        // bail has to supply its own — otherwise the message ends in a bare colon.
        let call = SupervisorCall::new("run-empty", &[], 3, true);

        let err = call
            .run(&ErConfig::default(), "run-empty", vec![], false)
            .expect_err("a reviewerless run cannot satisfy the quorum of 1");
        let msg = err.to_string();
        assert!(
            msg.contains("insufficient reviewers after round 1 (0/0 ok, need 1)"),
            "unexpected error: {msg}"
        );
        assert!(
            msg.contains("all reviewers failed"),
            "the empty-reason fallback must fill in: {msg}"
        );
    }

    #[test]
    fn run_supervisor_fails_the_run_when_the_arbiter_cannot_be_spawned() {
        let fid = fixture_finding_id();
        let mut hub = echo_hub(&combined_payload(&fid));
        // The arbiter model lives on a provider whose command does not exist.
        hub.providers.insert(
            "broken".into(),
            provider_of(
                MISSING_CMD,
                vec!["{prompt}".into()],
                vec![model_named("arb")],
            ),
        );
        let config = config_with(hub);

        let reviewers = vec![
            test_reviewer("p-m1", "p", "m1"),
            test_reviewer("p-m2", "p", "m2"),
        ];
        let call = SupervisorCall::new("run-arbiter", &reviewers, 2, true);
        // Point the persisted run at the broken arbiter (seed_run defaults to `p`).
        let mut seeded = load_run(&call.paths).unwrap();
        seeded.config.arbiter = rf("broken", "arb");
        save_run(&call.paths, &seeded).unwrap();

        let err = call
            .run(&config, "run-arbiter", reviewers, false)
            .expect_err("an arbiter that cannot run must fail the run");
        assert!(
            err.to_string().contains("spawn"),
            "the arbiter's spawn error must surface: {err}"
        );

        // Round 1 and the cross-check round did land, but nothing was adjudicated:
        // the run must not be left looking finished.
        let run = load_run(&call.paths).unwrap();
        assert_eq!(run.status, RunStatus::Running { round: 2 });
        assert_eq!(run.findings.len(), 1);
        assert_eq!(run.findings[0].verdict, Verdict::Pending);
        assert_eq!(
            run.findings[0].severity_by_round.get(&2),
            Some(&RiskLevel::High)
        );
        assert!(!call.paths.arbiter_output_json().exists());
    }

    // ---- run_round1_parallel / run_round2_parallel ----------------------------------

    struct RoundHarness {
        dir: tempfile::TempDir,
        registry: Arc<ArenaRegistry>,
        paths: ArenaPaths,
    }

    impl RoundHarness {
        fn new(run_id: &str) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let paths = ArenaPaths::for_run(&dir.path().join(".er"), run_id);
            paths.ensure_dirs().unwrap();
            Self {
                dir,
                registry: notify_registry(),
                paths,
            }
        }

        fn round1(
            &self,
            config: &ErConfig,
            reviewers: &[Reviewer],
            cancel: bool,
        ) -> Round1ParallelOutcome {
            run_round1_parallel(
                &self.registry,
                config,
                self.dir.path().to_str().unwrap(),
                &self.paths,
                "diff.patch",
                reviewers,
                None,
                &Arc::new(AtomicBool::new(cancel)),
                &Arc::new(Mutex::new(Vec::new())),
            )
            .expect("no reviewer thread may panic")
        }

        fn round2(
            &self,
            config: &ErConfig,
            round: u8,
            reviewers: &[Reviewer],
            cancel: bool,
        ) -> Round2ParallelOutcome {
            run_round2_parallel(
                &self.registry,
                config,
                self.dir.path().to_str().unwrap(),
                &self.paths,
                "diff.patch",
                round,
                "[]",
                reviewers,
                None,
                &Arc::new(AtomicBool::new(cancel)),
                &Arc::new(Mutex::new(Vec::new())),
            )
            .expect("no reviewer thread may panic")
        }
    }

    fn sorted_pairs(mut pairs: Vec<(String, String)>) -> Vec<(String, String)> {
        pairs.sort();
        pairs
    }

    #[test]
    fn run_round1_parallel_reports_cancelled_without_running_a_reviewer() {
        let config = config_with(echo_hub(&combined_payload(&fixture_finding_id())));
        let harness = RoundHarness::new("run-r1-cancel");
        let reviewers = vec![test_reviewer("p-m1", "p", "m1")];

        let outcome = harness.round1(&config, &reviewers, true);

        assert!(outcome.cancelled);
        assert!(outcome.ok.is_empty());
        assert!(
            outcome.failed.is_empty(),
            "cancellation is not a reviewer failure"
        );
        assert!(!harness.paths.round_reviewer_json(1, "p-m1").exists());
        // The thinking badge is emitted before the cancel check, so the UI shows the
        // reviewer being torn down rather than never appearing at all.
        assert_eq!(
            thinking(&progress_events(&harness.paths)),
            vec![("p-m1".to_string(), 1u8)]
        );
    }

    #[test]
    fn run_round1_parallel_keeps_valid_output_and_isolates_each_kind_of_failure() {
        let mut hub = echo_hub(&combined_payload(&fixture_finding_id()));
        // A reviewer whose JSON parses but violates the round-1 contract.
        hub.providers.insert(
            "bad".into(),
            provider_of(
                ECHO,
                vec![json!({
                    "findings": [{ "file": "src/a.rs", "title": "", "body": "b", "severity": "high" }]
                })
                .to_string()],
                vec![model_named("m1")],
            ),
        );
        let config = config_with(hub);

        let harness = RoundHarness::new("run-r1-mixed");
        let reviewers = vec![
            test_reviewer("p-m1", "p", "m1"),
            test_reviewer("bad-m1", "bad", "m1"),
            test_reviewer("ghost-m1", "ghost", "m1"),
        ];

        let outcome = harness.round1(&config, &reviewers, false);

        assert!(!outcome.cancelled);
        assert_eq!(
            outcome.ok.len(),
            1,
            "only one reviewer produced valid output"
        );
        assert_eq!(outcome.ok[0].0, "p-m1");
        assert_eq!(outcome.ok[0].1.findings.len(), 1);
        assert_eq!(outcome.ok[0].1.findings[0].title, "Leaky buffer");

        let failed = sorted_pairs(outcome.failed.clone());
        assert_eq!(
            failed.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>(),
            vec!["bad-m1".to_string(), "ghost-m1".to_string()]
        );
        assert!(
            failed[0].1.contains("title and body are required"),
            "a schema violation must be reported as that reviewer's reason: {}",
            failed[0].1
        );
        assert!(
            failed[1].1.contains("unknown provider: ghost"),
            "an unresolvable provider must be reported as that reviewer's reason: {}",
            failed[1].1
        );

        // Only validated output is durable — a rejected reviewer leaves no artifact
        // for the next round to read back.
        assert!(harness.paths.round_reviewer_json(1, "p-m1").is_file());
        assert!(!harness.paths.round_reviewer_json(1, "bad-m1").exists());
        assert!(!harness.paths.round_reviewer_json(1, "ghost-m1").exists());

        assert_eq!(
            thinking(&progress_events(&harness.paths)),
            vec![
                ("bad-m1".to_string(), 1u8),
                ("ghost-m1".to_string(), 1u8),
                ("p-m1".to_string(), 1u8),
            ]
        );
    }

    #[test]
    fn run_round1_parallel_treats_a_cancelled_provider_as_a_cancelled_round() {
        // A provider that reports cancellation (the agent CLI was killed) must abort
        // the whole round rather than be recorded as one reviewer's failure — the
        // supervisor turns `cancelled` into Cancelled and `failed` into Failed.
        let config = config_with(hub_of(vec![(
            "p",
            provider_of(
                "/bin/sh",
                vec!["-c".into(), "echo cancelled >&2; exit 1".into()],
                vec![model_named("m1")],
            ),
        )]));
        let harness = RoundHarness::new("run-r1-provider-cancel");
        let reviewers = vec![test_reviewer("p-m1", "p", "m1")];

        let outcome = harness.round1(&config, &reviewers, false);

        assert!(outcome.cancelled);
        assert!(
            outcome.failed.is_empty(),
            "a cancellation must not be logged as a reviewer failure"
        );
        assert!(outcome.ok.is_empty());
    }

    #[test]
    fn run_round2_parallel_files_ballots_under_the_round_they_were_cast_in() {
        let fid = fixture_finding_id();
        let mut hub = echo_hub(&combined_payload(&fid));
        // A ballot whose note is missing — the grounding requirement.
        hub.providers.insert(
            "bad".into(),
            provider_of(
                ECHO,
                vec![json!({
                    "ballots": [{ "finding_id": "f1", "vote": "keep", "note": "" }]
                })
                .to_string()],
                vec![model_named("m1")],
            ),
        );
        let config = config_with(hub);

        let harness = RoundHarness::new("run-r2-mixed");
        let reviewers = vec![
            test_reviewer("p-m1", "p", "m1"),
            test_reviewer("bad-m1", "bad", "m1"),
            test_reviewer("ghost-m1", "ghost", "m1"),
        ];

        // Round 4, not 2: the round is a parameter and has to reach both the progress
        // event and the output path.
        let outcome = harness.round2(&config, 4, &reviewers, false);

        assert!(!outcome.cancelled);
        assert_eq!(outcome.ok.len(), 1);
        assert_eq!(outcome.ok[0].0, "p-m1");
        assert_eq!(outcome.ok[0].1.ballots.len(), 1);
        assert_eq!(outcome.ok[0].1.ballots[0].vote, "escalate");
        assert_eq!(outcome.ok[0].1.ballots[0].finding_id, fid);

        let failed = sorted_pairs(outcome.failed.clone());
        assert_eq!(
            failed.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>(),
            vec!["bad-m1".to_string(), "ghost-m1".to_string()]
        );
        assert!(
            failed[0].1.contains("note is required"),
            "an ungrounded ballot must be rejected: {}",
            failed[0].1
        );
        assert!(failed[1].1.contains("unknown provider: ghost"));

        assert!(harness.paths.round_reviewer_json(4, "p-m1").is_file());
        assert!(
            !harness.paths.round_reviewer_json(2, "p-m1").exists(),
            "the output must land under the round it was cast in, not a hardcoded 2"
        );
        assert_eq!(
            thinking(&progress_events(&harness.paths)),
            vec![
                ("bad-m1".to_string(), 4u8),
                ("ghost-m1".to_string(), 4u8),
                ("p-m1".to_string(), 4u8),
            ]
        );
    }

    #[test]
    fn run_round2_parallel_reports_cancelled_without_running_a_reviewer() {
        let config = config_with(echo_hub(&combined_payload(&fixture_finding_id())));
        let harness = RoundHarness::new("run-r2-cancel");
        let reviewers = vec![test_reviewer("p-m1", "p", "m1")];

        let outcome = harness.round2(&config, 2, &reviewers, true);

        assert!(outcome.cancelled);
        assert!(outcome.ok.is_empty());
        assert!(outcome.failed.is_empty());
        assert!(!harness.paths.round_reviewer_json(2, "p-m1").exists());
    }
}
