use super::adapter::{is_cancelled_error, resolve_provider_command, run_provider_json};
use super::merge::findings_from_round1;
use super::model::*;
use super::registry::{ArenaRegistry, ArenaRunHandle, new_run_id};
use super::storage::{
    append_progress_event, load_run, save_diff_patch, save_round_output, save_run, ArenaPaths,
    ProgressEvent,
};
use super::voting::{apply_round3_verdicts, record_round3_ballots, severity_from_round2};
use crate::ai::compute_diff_hash;
use crate::ai::prompts::{
    build_arena_round1_prompt, build_arena_round2_prompt, build_arena_round3_prompt,
};
use crate::config::ErConfig;
use crate::git::filter_raw_diff_by_paths;
use crate::git::git_diff_raw;
use anyhow::{Context, Result};
use serde_json::json;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

pub const DEFAULT_COST_LIMIT_USD: f32 = 25.0;
pub const MIN_QUORUM: usize = 2;

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
    /// Requested round count (1–3); defaults to [`ARENA_ROUNDS_V1`].
    pub rounds: Option<u8>,
    pub confirm: bool,
}

pub fn scope_git_mode(scope: ArenaScope) -> &'static str {
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
    hub: &crate::config::AiHubConfig,
) -> f32 {
    let rounds = effective_arena_rounds(rounds) as f64;
    let reviewer_count = reviewers.len().max(1) as f64;
    let tokens_in = (diff_bytes as f64 * reviewer_count * rounds * 1.2) as f32;
    let mut rate_sum = 0.0f32;
    let mut n = 0u32;
    for rf in reviewers {
        if let Some(p) = hub.providers.get(&rf.provider_id) {
            if let Some(m) = p.models.iter().find(|m| m.id == rf.model_id) {
                rate_sum += m.cost_per_1k_in.unwrap_or(0.015) + m.cost_per_1k_out.unwrap_or(0.075);
                n += 1;
            }
        }
    }
    let rate = if n > 0 {
        rate_sum / n as f32
    } else {
        0.02
    };
    (tokens_in / 1000.0) * rate
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
    params: ArenaStartParams,
) -> Result<String> {
    if params.reviewers.len() < MIN_QUORUM {
        anyhow::bail!("arena requires at least {MIN_QUORUM} reviewers");
    }

    let scope_mode = scope_git_mode(params.scope);
    let mut raw_diff = git_diff_raw(scope_mode, &base_branch, &repo_root, None)?;
    if let Some(ref paths) = params.files {
        if !paths.is_empty() {
            raw_diff = filter_raw_diff_by_paths(&raw_diff, paths);
        }
    }
    let rounds = effective_arena_rounds(params.rounds);
    let est = estimate_cost_usd(
        raw_diff.len(),
        &params.reviewers,
        Some(rounds),
        &config.ai_hub,
    );
    if est > DEFAULT_COST_LIMIT_USD && !params.confirm {
        anyhow::bail!(
            "estimated cost ${est:.2} exceeds limit ${DEFAULT_COST_LIMIT_USD:.2}; pass confirm=true"
        );
    }

    let run_id = new_run_id();
    let paths = ArenaPaths::for_run(Path::new(&er_dir), &run_id);
    paths.ensure_dirs()?;
    save_diff_patch(&paths, &raw_diff)?;

    let diff_hash = compute_diff_hash(&raw_diff);
    let reviewers = resolve_reviewers(&config, &params.reviewers)?;
    let arbiter_ref = pick_arbiter(&params.reviewers, &reviewers);

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
            reviewers: params.reviewers,
            rounds,
            arbiter: arbiter_ref,
            auto_accept_threshold: 0.75,
            scope: params.scope,
            files: params.files,
        },
        reviewers: reviewers.clone(),
        findings: vec![],
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
                eprintln!("[arena] run {} failed: {e:#}", run_id_thread);
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
        registry_thread.take(&run_id_thread);
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

fn emit(registry: &ArenaRegistry, paths: &ArenaPaths, event: &ProgressEvent) {
    let _ = append_progress_event(paths, event);
    registry.notify_progress();
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

    macro_rules! bail_cancelled {
        () => {
            run.status = RunStatus::Cancelled;
            run.completed_at = Some(crate::app::chrono_now());
            save_run(paths, &run)?;
            *status.lock().unwrap() = RunStatus::Cancelled;
            emit(registry, paths, &ProgressEvent::RunComplete { run_id: run_id.clone() });
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

    let mut round1_ok: Vec<(String, super::schema::Round1Output)> = Vec::new();
    for reviewer in &reviewers {
        cancelled!();
        emit(
            registry,
            paths,
            &ProgressEvent::ReviewerThinking {
                reviewer_id: reviewer.id.clone(),
                round: 1,
            },
        );
        let cmd = resolve_provider_command(&config.ai_hub, &reviewer.provider_id, &reviewer.model_id)?;
        let prompt = build_arena_round1_prompt(patch_path, &reviewer.name);
        match run_provider_json(&cmd, &prompt, repo_root, &cancel, &children) {
            Ok(v) => match super::schema::validate_round1_output(&v) {
                Ok(out) => {
                    let _ = save_round_output(paths, 1, &reviewer.id, &v);
                    round1_ok.push((reviewer.id.clone(), out));
                    emit(
                        registry,
                        paths,
                        &ProgressEvent::ReviewerDone {
                            reviewer_id: reviewer.id.clone(),
                            round: 1,
                            findings_count: round1_ok.last().map(|(_, o)| o.findings.len()).unwrap_or(0),
                        },
                    );
                }
                Err(e) => {
                    mark_reviewer_failed(&mut run, &reviewer.id, &e.to_string());
                }
            },
            Err(e) => {
                if is_cancelled_error(&e) {
                    bail_cancelled!();
                }
                mark_reviewer_failed(&mut run, &reviewer.id, &e.to_string());
            }
        }
        save_run(paths, &run)?;
    }

    if survivors(&run) < MIN_QUORUM {
        anyhow::bail!("insufficient reviewers after round 1");
    }

    run.findings = findings_from_round1(&round1_ok);

    if total_rounds < 2 {
        run.status = RunStatus::Complete;
        run.completed_at = Some(crate::app::chrono_now());
        *status.lock().unwrap() = RunStatus::Complete;
        save_run(paths, &run)?;
        emit(
            registry,
            paths,
            &ProgressEvent::RunComplete { run_id: run_id.clone() },
        );
        return Ok(());
    }

    // Round 2
    cancelled!();
    *status.lock().unwrap() = RunStatus::Running { round: 2 };
    run.status = RunStatus::Running { round: 2 };
    save_run(paths, &run)?;
    emit(
        registry,
        paths,
        &ProgressEvent::RoundStarted {
            round: 2,
            total_rounds,
        },
    );

    let findings_json = serde_json::to_string(&run.findings)?;
    let mut round2: Vec<(String, super::schema::Round2Output)> = Vec::new();
    let active: Vec<Reviewer> = active_reviewers(&run, &reviewers)
        .into_iter()
        .cloned()
        .collect();
    for reviewer in &active {
        cancelled!();
        emit(
            registry,
            paths,
            &ProgressEvent::ReviewerThinking {
                reviewer_id: reviewer.id.clone(),
                round: 2,
            },
        );
        let cmd = resolve_provider_command(&config.ai_hub, &reviewer.provider_id, &reviewer.model_id)?;
        let prompt = build_arena_round2_prompt(patch_path, &reviewer.id, &findings_json);
        match run_provider_json(&cmd, &prompt, repo_root, &cancel, &children) {
            Ok(v) => match super::schema::validate_round2_output(&v) {
                Ok(out) => {
                    let count = out.ballots.len();
                    let _ = save_round_output(paths, 2, &reviewer.id, &v);
                    round2.push((reviewer.id.clone(), out));
                    emit(
                        registry,
                        paths,
                        &ProgressEvent::ReviewerDone {
                            reviewer_id: reviewer.id.clone(),
                            round: 2,
                            findings_count: count,
                        },
                    );
                }
                Err(e) => mark_reviewer_failed(&mut run, &reviewer.id, &e.to_string()),
            },
            Err(e) => {
                if is_cancelled_error(&e) {
                    bail_cancelled!();
                }
                mark_reviewer_failed(&mut run, &reviewer.id, &e.to_string())
            }
        }
    }
    severity_from_round2(&mut run.findings, &round2);
    save_run(paths, &run)?;

    if total_rounds < 3 {
        run.status = RunStatus::Complete;
        run.completed_at = Some(crate::app::chrono_now());
        *status.lock().unwrap() = RunStatus::Complete;
        save_run(paths, &run)?;
        emit(
            registry,
            paths,
            &ProgressEvent::RunComplete { run_id: run_id.clone() },
        );
        return Ok(());
    }

    // Round 3
    cancelled!();
    *status.lock().unwrap() = RunStatus::Running { round: 3 };
    run.status = RunStatus::Running { round: 3 };
    save_run(paths, &run)?;
    emit(
        registry,
        paths,
        &ProgressEvent::RoundStarted {
            round: 3,
            total_rounds,
        },
    );

    let arbiter = pick_arbiter_reviewer(&run, &reviewers);
    let arbiter_id = arbiter.id.clone();
    let arbiter_provider = arbiter.provider_id.clone();
    let arbiter_model = arbiter.model_id.clone();
    let summary = json!({ "findings": run.findings });
    let prompt = build_arena_round3_prompt(&summary.to_string());
    let cmd = resolve_provider_command(&config.ai_hub, &arbiter_provider, &arbiter_model)?;
    emit(
        registry,
        paths,
        &ProgressEvent::ReviewerThinking {
            reviewer_id: arbiter_id.clone(),
            round: 3,
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
    let _ = save_round_output(paths, 3, "arbiter", &v);
    apply_round3_verdicts(
        &mut run.findings,
        &r3,
        run.config.auto_accept_threshold,
    );
    record_round3_ballots(&mut run.findings, &r3, &arbiter_id);

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
    emit(
        registry,
        paths,
        &ProgressEvent::RunComplete { run_id: run_id.clone() },
    );
    Ok(())
}

fn mark_reviewer_failed(run: &mut ArenaRun, id: &str, reason: &str) {
    if let Some(r) = run.reviewers.iter_mut().find(|r| r.id == id) {
        r.status = ReviewerRunStatus::Failed {
            reason: reason.to_string(),
        };
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

fn pick_arbiter_reviewer<'a>(run: &'a ArenaRun, all: &'a [Reviewer]) -> &'a Reviewer {
    let active = active_reviewers(run, all);
    let arb = &run.config.arbiter;
    active
        .into_iter()
        .find(|r| r.provider_id == arb.provider_id && r.model_id == arb.model_id)
        .or_else(|| active_reviewers(run, all).into_iter().next())
        .unwrap_or(&all[0])
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
        let id = format!("{}-{}", rf.provider_id, rf.model_id);
        out.push(Reviewer {
            id: id.clone(),
            name: model
                .label
                .clone()
                .unwrap_or_else(|| model.id.clone()),
            kind: ReviewerKind::Model,
            provider_id: rf.provider_id.clone(),
            model_id: rf.model_id.clone(),
            system_prompt: String::new(),
            color: reviewer_color(i),
            icon: "cube".into(),
            tagline: provider.display_name(&rf.provider_id),
            cost_per_1k_in: model.cost_per_1k_in.unwrap_or(0.015),
            cost_per_1k_out: model.cost_per_1k_out.unwrap_or(0.075),
            avg_latency_ms: model.avg_latency_ms.unwrap_or(12_000),
            status: ReviewerRunStatus::Ok,
        });
    }
    Ok(out)
}

fn pick_arbiter(refs: &[ReviewerRef], _resolved: &[Reviewer]) -> ReviewerRef {
    refs.iter()
        .max_by_key(|r| r.model_id.len())
        .cloned()
        .unwrap_or_else(|| refs[0].clone())
}

fn reviewer_color(i: usize) -> String {
    const COLORS: &[&str] = &["#ff7a2b", "#ff6b6b", "#7f87ff", "#4ec9a4", "#ffc457", "#5fd970"];
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
                    args: vec![],
                    cost_per_1k_in: Some(0.001),
                    cost_per_1k_out: Some(0.001),
                    avg_latency_ms: None,
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
                    args: vec![],
                    cost_per_1k_in: Some(0.1),
                    cost_per_1k_out: Some(0.1),
                    avg_latency_ms: None,
                }],
                ..Default::default()
            },
        );
        let cheap = vec![ReviewerRef {
            provider_id: "cheap".into(),
            model_id: "m1".into(),
        }];
        let dear = vec![ReviewerRef {
            provider_id: "dear".into(),
            model_id: "m2".into(),
        }];
        let low = estimate_cost_usd(10_000, &cheap, Some(3), &hub);
        let high = estimate_cost_usd(10_000, &dear, Some(3), &hub);
        assert!(high > low * 5.0);
    }

    #[test]
    fn pick_arbiter_skips_failed_reviewer() {
        use crate::arena::model::{ArenaConfig, ArenaRun, ArenaScope, ReviewerRunStatus};
        let refs = vec![
            ReviewerRef {
                provider_id: "a".into(),
                model_id: "m1".into(),
            },
            ReviewerRef {
                provider_id: "b".into(),
                model_id: "m2".into(),
            },
        ];
        let reviewers = vec![
            Reviewer {
                id: "a-m1".into(),
                name: "A".into(),
                kind: ReviewerKind::Model,
                provider_id: "a".into(),
                model_id: "m1".into(),
                system_prompt: String::new(),
                color: String::new(),
                icon: String::new(),
                tagline: String::new(),
                cost_per_1k_in: 0.0,
                cost_per_1k_out: 0.0,
                avg_latency_ms: 0,
                status: ReviewerRunStatus::Failed {
                    reason: "x".into(),
                },
            },
            Reviewer {
                id: "b-m2".into(),
                name: "B".into(),
                kind: ReviewerKind::Model,
                provider_id: "b".into(),
                model_id: "m2".into(),
                system_prompt: String::new(),
                color: String::new(),
                icon: String::new(),
                tagline: String::new(),
                cost_per_1k_in: 0.0,
                cost_per_1k_out: 0.0,
                avg_latency_ms: 0,
                status: ReviewerRunStatus::Ok,
            },
        ];
        let run = ArenaRun {
            id: "t".into(),
            title: None,
            branch_ref: "main".into(),
            base_branch: "main".into(),
            scope: ArenaScope::Branch,
            diff_hash: String::new(),
            created_at: String::new(),
            completed_at: None,
            status: RunStatus::Running { round: 3 },
            config: ArenaConfig {
                reviewers: refs.clone(),
                rounds: 3,
                arbiter: refs[0].clone(),
                auto_accept_threshold: 0.75,
                scope: ArenaScope::Branch,
                files: None,
            },
            reviewers: reviewers.clone(),
            findings: vec![],
            cost_estimate: CostEstimate {
                tokens_in: 0,
                tokens_out: 0,
                usd: 0.0,
            },
        };
        let picked = pick_arbiter_reviewer(&run, &reviewers);
        assert_eq!(picked.id, "b-m2");
    }
}
