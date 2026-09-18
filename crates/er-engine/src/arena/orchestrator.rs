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
use crate::agent_slots::Workload;
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
    /// Highest round count the engine will actually run. The launcher offers
    /// this as its maximum, so the number shown is the number that runs —
    /// `effective_arena_rounds` silently clamps anything higher, and a picker
    /// that offers more than this reports rounds nobody will execute.
    pub max_rounds: u8,
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
        max_rounds: ARENA_ROUNDS_V1,
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

/// Where a seeded run reads from and what it covers.
pub struct SeededStartParams {
    /// The active view bucket's sidecar directory — expert files in, run out.
    pub er_dir: String,
    pub branch_ref: String,
    pub base_branch: String,
    pub scope: ArenaScope,
    pub diff_hash: String,
    /// The loaded review's hash, so an expert set from the same generation as a
    /// stale review still counts. Empty when no review is loaded.
    pub review_hash: String,
    /// The branch diff, saved so the arbiter can read the hunks its findings
    /// point at. Empty means it grades on the findings alone.
    pub raw_diff: String,
    /// Overrides the hub default arbiter when the caller picked one.
    pub arbiter: Option<ReviewerRef>,
}

/// Rule on the deduped expert findings, without running a debate.
///
/// The cheap path: triage picks the lenses, the experts run, and this grades
/// what they produced. Spawned like `start_arena_run` so the run shows up in the
/// arena list, honours cancellation, and reports through the same progress
/// events — but the only model call is the arbiter's, however many experts ran.
///
/// Returns `None` when there are no expert findings to validate, so a caller
/// offers the action only when it would do something.
pub fn start_seeded_run(
    registry: Arc<ArenaRegistry>,
    config: ErConfig,
    repo_root: String,
    params: SeededStartParams,
) -> Result<Option<String>> {
    let findings = super::seeded::dedupe_expert_findings(
        &params.er_dir,
        &params.diff_hash,
        &params.review_hash,
    );
    if findings.is_empty() {
        return Ok(None);
    }

    let arbiter = match params.arbiter.clone() {
        Some(arbiter) => arbiter,
        None => default_arbiter_from_hub(&config.ai_hub)
            .context("no arbiter model available in ai_hub")?,
    };

    let run_id = new_run_id();
    let paths = ArenaPaths::for_run(Path::new(&params.er_dir), &run_id);
    let run = super::seeded::build_seeded_run(
        &config,
        super::seeded::SeededRunParams {
            id: run_id.clone(),
            diff_hash: params.diff_hash.clone(),
            base_branch: params.base_branch.clone(),
            branch_ref: params.branch_ref.clone(),
            scope: params.scope,
            arbiter,
        },
        findings,
    )?;
    if !params.raw_diff.trim().is_empty() {
        save_diff_patch(&paths, &params.raw_diff)?;
    }
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
    let er_dir = params.er_dir.clone();
    let diff_hash = params.diff_hash.clone();

    let join = thread::spawn(move || {
        let ctx = ArbiterCtx {
            registry: &registry_thread,
            config: &config,
            repo_root: &repo_root,
            paths: &paths_clone,
            cancel: &cancel,
            children: &children,
            status: &status,
        };
        let mut run = match load_run(&paths_clone) {
            Ok(run) => run,
            Err(e) => {
                crate::dev_log::arena_line(format!("seeded {run_id_thread}: load failed: {e:#}"));
                registry_thread.release_run(&run_id_thread);
                return;
            }
        };
        let effort = run.config.effort.clone();
        let outcome = run_arbiter(&ctx, &mut run, effort.as_deref());
        match outcome {
            Ok(ArbiterOutcome::Cancelled) => {}
            Ok(ArbiterOutcome::Judged) => {
                run.status = RunStatus::Complete;
                run.completed_at = Some(crate::app::chrono_now());
                *status.lock().unwrap() = RunStatus::Complete;
                let _ = save_run(&paths_clone, &run);
                // The review's copy of the verdicts. The run record above is the
                // arena's; this is the overlay the review loads, so `review.json`
                // keeps its three writers (ADR 0001).
                let arbiter = arbiter_review_from_run(&run, &run_id_thread, &diff_hash);
                if let Err(e) = crate::ai::write_arbiter_review(&er_dir, &arbiter) {
                    crate::dev_log::arena_line(format!("seeded {run_id_thread}: {e:#}"));
                }
                emit(
                    &registry_thread,
                    &paths_clone,
                    &ProgressEvent::RunComplete {
                        run_id: run_id_thread.clone(),
                    },
                );
            }
            Err(e) => {
                crate::dev_log::arena_line(format!("seeded {run_id_thread} failed: {e:#}"));
                *status.lock().unwrap() = RunStatus::Failed;
                run.status = RunStatus::Failed;
                run.completed_at = Some(crate::app::chrono_now());
                let _ = save_run(&paths_clone, &run);
            }
        }
        registry_thread.release_run(&run_id_thread);
        registry_thread.notify_progress();
    });

    registry.insert(
        run_id.clone(),
        ArenaRunHandle {
            cancel: cancel_handle,
            children: children_handle,
            status: status_handle,
            join: Some(join),
        },
    );
    Ok(Some(run_id))
}

/// The review-facing view of a finished run's verdicts.
///
/// Only findings the arbiter actually ruled on become verdicts: a `Pending`
/// finding was never judged, and writing it as one would claim a grade that
/// does not exist.
fn arbiter_review_from_run(
    run: &ArenaRun,
    run_id: &str,
    diff_hash: &str,
) -> crate::ai::ArbiterReview {
    let verdicts = run
        .findings
        .iter()
        .filter_map(|f| {
            let verdict = match &f.verdict {
                Verdict::Kept => crate::ai::ArbiterRuling::Kept,
                Verdict::Escalated => crate::ai::ArbiterRuling::Escalated,
                Verdict::Merged { .. } => crate::ai::ArbiterRuling::Merged,
                Verdict::Dropped => crate::ai::ArbiterRuling::Dropped,
                Verdict::Pending => return None,
            };
            Some(crate::ai::ArbiterVerdict {
                id: f.id.clone(),
                file: f.file.clone(),
                verdict,
                confidence: Some(crate::ai::Confidence::from_score(f.confidence)),
                merged_into: match &f.verdict {
                    Verdict::Merged { into } => Some(into.clone()),
                    _ => None,
                },
                rationale: f.rationale.clone(),
                raised_by: f.raised_by.clone(),
            })
        })
        .collect();

    crate::ai::ArbiterReview {
        version: 1,
        diff_hash: diff_hash.to_string(),
        created_at: crate::app::chrono_now(),
        run_id: run_id.to_string(),
        verdicts,
    }
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
    // One thread per reviewer parked the surplus inside `acquire`, so thread
    // count tracked the reviewer count rather than the cap. A bounded pool of
    // workers consuming a queue keeps the thread count at the cap and lets the
    // surplus wait as data rather than as parked stacks.
    let arena_cap = config.ai_hub.effective_max_concurrent_arena_reviews();
    let worker_count = reviewers.len().min(arena_cap.max(1));
    let queue = Arc::new(Mutex::new(
        reviewers
            .iter()
            .cloned()
            .collect::<std::collections::VecDeque<_>>(),
    ));
    for _ in 0..worker_count {
        let queue = Arc::clone(&queue);
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
        // `emit` needs the registry, which cannot move into a 'static thread;
        // its notify half can, and the file half comes from `paths`.
        let notify_progress = Arc::clone(&registry.notify);
        handles.push(thread::spawn(move || loop {
            if cancel.load(Ordering::SeqCst) {
                cancelled.store(true, Ordering::SeqCst);
                return;
            }
            // Taken one at a time: a worker holds a slot for the reviewer it
            // is running and picks up the next when that finishes. Running out
            // of queue is how a worker retires.
            let Some(reviewer) = queue.lock().unwrap_or_else(|e| e.into_inner()).pop_front() else {
                return;
            };
            // Wait for an arena slot so several runs (or runs with many
            // reviewers) can't spawn unbounded agent processes at once. The
            // arena's own cap, charged against the shared ceiling, so a
            // background review and an arena round do not take each
            // other's slot.
            let arena_cap = config.ai_hub.effective_max_concurrent_arena_reviews();
            let ceiling = config.ai_hub.effective_max_concurrent_agents();
            let Some(_slot) =
                crate::agent_slots::acquire(Workload::Arena, arena_cap, ceiling, &cancel)
            else {
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
                    // `continue`, not `return`: this worker still has the rest
                    // of the queue to get through. Returning here would retire
                    // the worker and silently drop every reviewer behind it.
                    continue;
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
                        // Announced here rather than after the join, so the UI
                        // tracks the slowest reviewer instead of the batch —
                        // emitting after the join would land every reviewer's
                        // "done" in one burst once the last one finished.
                        let _ = append_progress_event(
                            &paths,
                            &ProgressEvent::ReviewerDone {
                                reviewer_id: reviewer.id.clone(),
                                round: 1,
                                findings_count: out.findings.len(),
                            },
                        );
                        notify_progress();
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
    // threads.
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
    // Same bounded pool as round 1: threads at the cap, surplus as data.
    let arena_cap = config.ai_hub.effective_max_concurrent_arena_reviews();
    let worker_count = reviewers.len().min(arena_cap.max(1));
    let queue = Arc::new(Mutex::new(
        reviewers
            .iter()
            .cloned()
            .collect::<std::collections::VecDeque<_>>(),
    ));
    for _ in 0..worker_count {
        let queue = Arc::clone(&queue);
        let config = Arc::clone(&config);
        let effort = effort.clone();
        let repo_root = repo_root.clone();
        let patch_path = patch_path.clone();
        let findings_json = Arc::clone(&findings_json);
        let notify_progress = Arc::clone(&registry.notify);
        let paths = paths.clone();
        let storage_dir = storage_dir.clone();
        let cancel = Arc::clone(&cancel);
        let children = Arc::clone(&children);
        let ok = Arc::clone(&ok);
        let failed = Arc::clone(&failed);
        let cancelled = Arc::clone(&cancelled);
        handles.push(thread::spawn(move || loop {
            if cancel.load(Ordering::SeqCst) {
                cancelled.store(true, Ordering::SeqCst);
                return;
            }
            let Some(reviewer) = queue.lock().unwrap_or_else(|e| e.into_inner()).pop_front() else {
                return;
            };
            let arena_cap = config.ai_hub.effective_max_concurrent_arena_reviews();
            let ceiling = config.ai_hub.effective_max_concurrent_agents();
            let Some(_slot) =
                crate::agent_slots::acquire(Workload::Arena, arena_cap, ceiling, &cancel)
            else {
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
                    // `continue`, not `return` — see round 1: returning here
                    // would retire the worker and drop the rest of the queue.
                    continue;
                }
            };
            let prompt =
                build_arena_round2_prompt(&patch_path, &reviewer.id, round, findings_json.as_str());
            match run_provider_json(&cmd, &prompt, &repo_root, &cancel, &children) {
                Ok(v) => match super::schema::validate_round2_output(&v) {
                    Ok(out) => {
                        let _ = save_round_output(&paths, round, &reviewer.id, &v);
                        // Announced as it completes — see round 1.
                        let _ = append_progress_event(
                            &paths,
                            &ProgressEvent::ReviewerDone {
                                reviewer_id: reviewer.id.clone(),
                                round,
                                findings_count: out.ballots.len(),
                            },
                        );
                        notify_progress();
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

    let round1_started = std::time::Instant::now();
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
    crate::agent_timing::emit(
        "arena_round",
        &[
            ("round", "1".to_string()),
            ("reviewers", reviewers.len().to_string()),
            (
                "elapsed_ms",
                round1_started.elapsed().as_millis().to_string(),
            ),
        ],
    );
    if round1.cancelled {
        bail_cancelled!();
    }
    for (id, reason) in round1.failed {
        mark_reviewer_failed(&mut run, &id, &reason);
    }
    // `ReviewerDone` is emitted by each worker as it finishes, so nothing is
    // announced here — doing it again would double the events.
    let round1_ok = round1.ok;
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
        let round_started = std::time::Instant::now();
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
        crate::agent_timing::emit(
            "arena_round",
            &[
                ("round", round.to_string()),
                ("reviewers", active.len().to_string()),
                (
                    "elapsed_ms",
                    round_started.elapsed().as_millis().to_string(),
                ),
            ],
        );
        if cross_out.cancelled {
            bail_cancelled!();
        }
        for (id, reason) in cross_out.failed {
            mark_reviewer_failed(&mut run, &id, &reason);
        }
        // `ReviewerDone` is emitted by each worker as it finishes — see round 1.
        let cross_ok = cross_out.ok;
        severity_from_cross_check(&mut run.findings, &cross_ok, round);
        save_run(paths, &run)?;
    }

    // Arbiter phase (after all reviewer cross-check rounds)
    let ctx = ArbiterCtx {
        registry,
        config,
        repo_root,
        paths,
        cancel: &cancel,
        children: &children,
        status: &status,
    };
    if matches!(
        run_arbiter(&ctx, &mut run, run_effort.as_deref())?,
        ArbiterOutcome::Cancelled
    ) {
        return Ok(());
    }

    run.status = RunStatus::Complete;
    run.completed_at = Some(crate::app::chrono_now());
    *status.lock().unwrap() = RunStatus::Complete;
    save_run(paths, &run)?;
    emit(registry, paths, &ProgressEvent::RunComplete { run_id });
    Ok(())
}

/// The supervisor's ambient scope, bundled so the arbiter call can be shared by
/// the normal path and the seeded one without a ten-argument signature.
struct ArbiterCtx<'a> {
    registry: &'a ArenaRegistry,
    config: &'a ErConfig,
    repo_root: &'a str,
    paths: &'a ArenaPaths,
    cancel: &'a AtomicBool,
    children: &'a Arc<Mutex<Vec<Child>>>,
    status: &'a Mutex<RunStatus>,
}

/// Whether the arbiter pass ruled, or the run was cancelled under it.
///
/// Cancellation is an outcome rather than an error: the call site marks the run
/// cancelled and returns `Ok(())`, and both paths need that.
enum ArbiterOutcome {
    Judged,
    Cancelled,
}

/// One arbiter call over `run.findings`, applying its verdicts in place.
///
/// Shared by the normal path (after every cross-check round) and the seeded path
/// (straight from the expert dedupe). The caller owns what follows — marking the
/// run complete, or returning early on cancellation.
fn run_arbiter(
    ctx: &ArbiterCtx<'_>,
    run: &mut ArenaRun,
    run_effort: Option<&str>,
) -> Result<ArbiterOutcome> {
    let ArbiterCtx {
        registry,
        config,
        repo_root,
        paths,
        cancel,
        children,
        status,
    } = *ctx;
    let round = run.config.rounds;
    let run_id = run.id.clone();

    if cancel.load(Ordering::SeqCst) || registry.is_cancelled(&run_id) {
        return cancel_run(ctx, run);
    }

    let arbiter_ref = &run.config.arbiter;
    let arbiter_label = arbiter_display_label(&config.ai_hub, arbiter_ref);
    emit(
        registry,
        paths,
        &ProgressEvent::ArbiterStarted { arbiter_label },
    );
    *status.lock().unwrap() = RunStatus::Running { round };
    run.status = RunStatus::Running { round };
    save_run(paths, run)?;

    let summary = json!({ "findings": run.findings });
    // The hunks the findings point at, so a drop is a judgement about code
    // rather than about a claim. Missing patch file degrades to no excerpt
    // rather than failing the run — the arbiter grades on the findings alone,
    // as it did before this existed.
    let anchored = std::fs::read_to_string(paths.diff_patch())
        .map(|raw| {
            let targets: Vec<(String, Option<usize>)> = run
                .findings
                .iter()
                .map(|f| (f.file.clone(), f.line))
                .collect();
            crate::ai::prepared_diff::hunks_for_findings(&raw, &targets)
        })
        .unwrap_or_default();
    let prompt = build_arena_round3_prompt(&summary.to_string(), &anchored);
    let cmd = resolve_provider_command(
        &config.ai_hub,
        &arbiter_ref.provider_id,
        &arbiter_ref.model_id,
        run_effort,
        Some(paths.root.to_string_lossy().as_ref()),
    )?;
    emit(
        registry,
        paths,
        &ProgressEvent::ReviewerThinking {
            reviewer_id: ARBITER_REVIEWER_ID.to_string(),
            round,
        },
    );
    // The arbiter is an agent process like any reviewer, so it waits for a
    // global slot too. Without this an arena could fork one more provider CLI
    // than the cap allows — worst at the moment the cap is saturated — and N
    // seeded runs would start N arbiters at once, since the seeded path has no
    // reviewer loop to hold a slot on its behalf.
    //
    // Reviewers have released theirs by now (their round joined), so this
    // normally does not wait. It can, if another arena run holds the slots.
    let arena_cap = config.ai_hub.effective_max_concurrent_arena_reviews();
    let ceiling = config.ai_hub.effective_max_concurrent_agents();
    let Some(_arbiter_slot) =
        crate::agent_slots::acquire(Workload::Arena, arena_cap, ceiling, cancel)
    else {
        return cancel_run(ctx, run);
    };
    let arbiter_started = std::time::Instant::now();
    let v = match run_provider_json(&cmd, &prompt, repo_root, cancel, children) {
        Ok(v) => v,
        Err(e) if is_cancelled_error(&e) => return cancel_run(ctx, run),
        Err(e) => return Err(e),
    };
    // The arbiter cannot be overlapped: its wall time adds straight onto the
    // run and no cap change shortens it.
    crate::agent_timing::emit(
        "arena_round",
        &[
            ("round", "arbiter".to_string()),
            ("reviewers", "1".to_string()),
            (
                "elapsed_ms",
                arbiter_started.elapsed().as_millis().to_string(),
            ),
        ],
    );
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
    Ok(ArbiterOutcome::Judged)
}

/// Mark the run cancelled and announce it, preserving what `bail_cancelled!`
/// did at the call site before this was extracted.
fn cancel_run(ctx: &ArbiterCtx<'_>, run: &mut ArenaRun) -> Result<ArbiterOutcome> {
    run.status = RunStatus::Cancelled;
    run.completed_at = Some(crate::app::chrono_now());
    save_run(ctx.paths, run)?;
    *ctx.status.lock().unwrap() = RunStatus::Cancelled;
    emit(
        ctx.registry,
        ctx.paths,
        &ProgressEvent::RunComplete {
            run_id: run.id.clone(),
        },
    );
    Ok(ArbiterOutcome::Cancelled)
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

/// Build the `Reviewer` records for a set of refs, each marked `Ok`.
///
/// Shared with the seeded path, which has no round 1 to run: without these the
/// survivor count and the process matrix would both read empty.
pub(super) fn resolve_reviewers(config: &ErConfig, refs: &[ReviewerRef]) -> Result<Vec<Reviewer>> {
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
}
