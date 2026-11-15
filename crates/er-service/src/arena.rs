//! The arena use cases, run as actor effects over the actor's live runs.
//!
//! Each effect fetches the diff it needs through the ports and calls the same
//! engine functions the App does. The runs' jobs go to the actor's sink, so
//! every reviewer and arbiter the flag path starts is a job the actor holds.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use er_engine::ai::workflow_record::ArenaWorkflow;
use er_engine::ai::WorkflowHolder;
use er_engine::arena::{
    self, ArenaProgressState, ArenaRunControls, ArenaRunInputs, ArenaRunKind, ArenaRunSnapshot,
    ArenaRunSummary, ArenaScope, RunStatus, SeededStartParams,
};
use er_store::Scope;

use crate::actor::WorkspaceDeps;
use crate::handle::WorkspaceError;
use crate::protocol::{ArenaCommand, ArenaContext, Reply};
use crate::workspace::WorkspaceView;

fn failed(context: &str, error: impl std::fmt::Display) -> WorkspaceError {
    WorkspaceError::Failed(format!("{context}: {error:#}"))
}

/// The arena's diff: the tab's diff in the run's scope, narrowed to `files`.
fn raw_diff(
    deps: &WorkspaceDeps,
    arena: &ArenaContext,
    scope: ArenaScope,
    files: Option<&[String]>,
) -> Result<String, WorkspaceError> {
    let mut tab = arena.tab.clone();
    tab.scope = arena::scope_git_mode(scope).to_string();
    let raw = crate::annotations::fetch_raw(deps, &tab)?;
    Ok(match files {
        Some(paths) if !paths.is_empty() => er_engine::git::filter_raw_diff_by_paths(&raw, paths),
        _ => raw,
    })
}

/// The bucket lease a standalone arena action holds until its last run
/// drains. Admission is probed first, so a flow that owns the bucket refuses
/// the start instead of making it wait.
fn standalone_workflow(er_dir: &str, raw_diff: &str) -> Result<ArenaWorkflow, WorkspaceError> {
    static SEQUENCE: AtomicU64 = AtomicU64::new(1);
    arena::check_bucket_admission(er_dir).map_err(|error| failed("admit the run", error))?;
    let scope = Scope::dir(er_dir).map_err(|error| failed("open the bucket", error))?;
    let label = format!("arena-{}", SEQUENCE.fetch_add(1, Ordering::Relaxed));
    let guard = er_engine::store_registry::store()
        .lease(&scope, er_store::DEFAULT_LOCK_TIMEOUT, &label)
        .map_err(|error| failed("take the arena lease", error))?;
    let holder = guard.holder();
    Ok(ArenaWorkflow {
        scope,
        diff_hash: er_engine::ai::compute_diff_hash(raw_diff),
        started_at: er_engine::sync::chrono_now(),
        holder: WorkflowHolder {
            pid: holder.pid,
            label: holder.label.clone(),
        },
        guard: Arc::new(guard),
    })
}

fn inputs(arena: &ArenaContext, raw_diff: String) -> ArenaRunInputs {
    ArenaRunInputs {
        repo_root: arena.tab.repo_root.clone(),
        er_dir: arena.tab.er_dir.clone(),
        branch_ref: arena.branch_ref.clone(),
        base_branch: arena.tab.base_branch.clone(),
        raw_diff,
        reviewed_ref_note: None,
    }
}

fn bucket_for(
    controls: &ArenaRunControls,
    arena: &ArenaContext,
    run_id: &str,
) -> std::path::PathBuf {
    arena::bucket_for_run(controls, &arena.buckets, &arena.tab.er_dir, run_id)
}

/// The flag path has no review-flow host yet, so a run is flow-owned when its
/// record says so.
fn is_flow_run(run: &er_engine::arena::ArenaRun) -> bool {
    run.config.run_kind == ArenaRunKind::ReviewFlow
}

/// One run's snapshot for a reader: the record on disk with the live status
/// the view published. Reads the store and the published view only, so the
/// caller never waits on the actor (ADR 0067).
pub fn read_run(
    view: &WorkspaceView,
    arena: &ArenaContext,
    run_id: &str,
) -> Result<ArenaRunSnapshot, WorkspaceError> {
    let live = view.arena_run(run_id);
    let er_dir = arena::bucket_for_run_in(
        live.map(|run| run.er_dir.as_str()),
        &arena.buckets,
        &arena.tab.er_dir,
        run_id,
    );
    let status = live.map(|run| run.status.clone());
    arena::run_snapshot_with_status(&arena.config, status, &er_dir, run_id, is_flow_run)
        .map_err(|error| failed("read the run", error))
}

/// The active bucket's run summaries on the tab's branch, newest first.
pub fn read_runs(arena: &ArenaContext) -> Result<Vec<ArenaRunSummary>, WorkspaceError> {
    arena::list_summaries(Path::new(&arena.tab.er_dir), Some(&arena.branch_ref))
        .map_err(|error| failed("list the runs", error))
}

/// A live run's progress from the published view; empty for a run this actor
/// is not running, as the flag-off read answers.
pub fn read_progress(view: &WorkspaceView, run_id: &str) -> ArenaProgressState {
    view.arena_run(run_id)
        .map(|run| run.progress.clone())
        .unwrap_or_default()
}

/// Runs one arena command. Called on an effect thread.
pub(crate) fn run(
    deps: &WorkspaceDeps,
    controls: &ArenaRunControls,
    command: ArenaCommand,
) -> Result<Reply, WorkspaceError> {
    match command {
        ArenaCommand::Estimate {
            arena,
            scope,
            files,
            reviewers,
            rounds,
            arbiter,
        } => {
            let raw = raw_diff(deps, &arena, scope, files.as_deref())?;
            arena::build_arena_diff_preview(
                &arena.config,
                &raw,
                &reviewers,
                rounds,
                arbiter.as_ref(),
            )
            .map(Reply::ArenaEstimate)
            .map_err(|error| failed("estimate the run", error))
        }
        ArenaCommand::EstimateBatch { arena, params } => {
            let raw = raw_diff(deps, &arena, params.scope, params.files.as_deref())?;
            let hub = &arena.config.ai_hub;
            let models: Vec<_> = params
                .groups
                .iter()
                .flat_map(|group| group.models.iter())
                .cloned()
                .collect();
            Ok(Reply::ArenaEstimate(arena::ArenaDiffPreview {
                diff_bytes: raw.len(),
                cost_usd: arena::estimate_batch_cost_usd(raw.len(), &params, hub),
                latency_sec: arena::estimate_latency_sec(&models, params.rounds, hub),
                cost_limit_usd: arena::DEFAULT_COST_LIMIT_USD,
                max_rounds: arena::ARENA_ROUNDS_V1,
            }))
        }
        ArenaCommand::StartRun { arena, params } => {
            arena::check_bucket_admission(&arena.tab.er_dir)
                .map_err(|error| failed("admit the run", error))?;
            let raw = raw_diff(deps, &arena, params.scope, params.files.as_deref())?;
            let workflow = standalone_workflow(&arena.tab.er_dir, &raw)?;
            let config = (*arena.config).clone();
            let run_id = arena::start_arena_run(
                controls,
                config,
                inputs(&arena, raw),
                params,
                Some(workflow),
            )
            .map_err(|error| failed("start the arena run", error))?;
            Ok(Reply::ArenaStarted {
                run_ids: vec![run_id],
            })
        }
        ArenaCommand::StartBatch { arena, params } => {
            arena::check_bucket_admission(&arena.tab.er_dir)
                .map_err(|error| failed("admit the runs", error))?;
            let raw = raw_diff(deps, &arena, params.scope, params.files.as_deref())?;
            let workflow = standalone_workflow(&arena.tab.er_dir, &raw)?;
            let config = (*arena.config).clone();
            let run_ids = arena::start_arena_batch(
                controls,
                config,
                inputs(&arena, raw),
                params,
                Some(workflow),
            )
            .map_err(|error| failed("start the arena runs", error))?;
            Ok(Reply::ArenaStarted { run_ids })
        }
        ArenaCommand::ValidateFindings { arena } => validate_findings(deps, controls, &arena),
        ArenaCommand::AcceptFindings {
            arena,
            run_id,
            finding_ids,
        } => {
            let er_dir = bucket_for(controls, &arena, &run_id);
            let accepted = arena::accept_findings(&er_dir, &run_id, finding_ids, is_flow_run)
                .map_err(|error| failed("accept the findings", error))?;
            Ok(Reply::ArenaAccepted { accepted })
        }
        ArenaCommand::Delete { arena, run_id } => {
            if let Some(control) = controls.get(&run_id) {
                if !control.is_finished() {
                    return Err(WorkspaceError::Failed(format!(
                        "cannot delete active arena run {run_id}; cancel it first"
                    )));
                }
                controls.remove(&run_id);
            }
            let er_dir = bucket_for(controls, &arena, &run_id);
            arena::check_bucket_admission(&er_dir.to_string_lossy())
                .map_err(|error| failed("admit the delete", error))?;
            arena::delete_run_dir(&er_dir, &run_id)
                .map_err(|error| failed("delete the run", error))?;
            Ok(Reply::ArenaChanged { run_id })
        }
        ArenaCommand::Cancel { run_id } => {
            let Some(control) = controls.get(&run_id) else {
                return Err(WorkspaceError::Failed(format!(
                    "no active arena run {run_id}"
                )));
            };
            control.cancel();
            let paths = arena::ArenaPaths::for_run(Path::new(&control.er_dir), &run_id);
            if let Ok(mut run) = arena::load_run(&paths) {
                run.status = RunStatus::Cancelled;
                run.completed_at = Some(er_engine::sync::chrono_now());
                let _ = arena::save_run(&paths, &run);
            }
            Ok(Reply::ArenaChanged { run_id })
        }
        ArenaCommand::Override {
            arena,
            run_id,
            finding_id,
            verdict,
            note,
        } => {
            let er_dir = bucket_for(controls, &arena, &run_id);
            arena::override_finding(&er_dir, &run_id, &finding_id, verdict, note)
                .map_err(|error| failed("override the finding", error))?;
            controls.notify_progress();
            Ok(Reply::ArenaChanged { run_id })
        }
    }
}

/// Dedupe the tab's expert findings and rule on them with one arbiter job.
fn validate_findings(
    deps: &WorkspaceDeps,
    controls: &ArenaRunControls,
    arena: &ArenaContext,
) -> Result<Reply, WorkspaceError> {
    let er_dir = arena.tab.er_dir.clone();
    arena::check_bucket_admission(&er_dir).map_err(|error| failed("admit the run", error))?;
    let raw = raw_diff(deps, arena, ArenaScope::Branch, None)?;
    // The hash the review's sidecars carry, which a verdict must match; see
    // the App's validation for why it is read rather than recomputed.
    let diff_hash = er_engine::ai::prepared_diff::diff_tmp_hash(&er_dir)
        .unwrap_or_else(|| er_engine::ai::compute_diff_hash(&raw));
    let workflow = standalone_workflow(&er_dir, &raw)?;
    let params = SeededStartParams {
        er_dir,
        branch_ref: arena.branch_ref.clone(),
        base_branch: arena.tab.base_branch.clone(),
        scope: ArenaScope::Branch,
        diff_hash,
        review_hash: arena.review_hash.clone(),
        raw_diff: raw,
        arbiter: None,
        include_general: false,
        source_lenses: None,
        lens_providers: std::collections::BTreeMap::new(),
        reviewed_ref_note: None,
    };
    let run_id = arena::start_seeded_run(
        controls,
        (*arena.config).clone(),
        arena.tab.repo_root.clone(),
        params,
        Some(workflow),
    )
    .map_err(|error| failed("start the validation", error))?;
    Ok(Reply::ArenaValidating { run_id })
}

/// Load-time recovery: marks stale arena runs and a dead owner's running
/// workflow stages in each bucket. Never submits a job. Returns the buckets
/// whose workflow can be re-run.
pub(crate) fn reconcile(controls: &ArenaRunControls, buckets: &[String]) -> Reply {
    let mut resumable = Vec::new();
    for bucket in buckets {
        let _ = arena::reconcile_stale_runs_excluding(Path::new(bucket), |run_id| {
            controls.contains(run_id)
        });
        if er_engine::workflow_recovery::reconcile_workflow_record(bucket)
            .is_some_and(|record| record.resumable())
        {
            resumable.push(bucket.clone());
        }
    }
    Reply::Reconciled { resumable }
}
