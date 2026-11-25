//! The one stage sequencer for Quick, Standard and Deep review flows.
//!
//! The driver pins one filtered diff, one bucket lease, and exact background
//! task IDs for the life of a flow. UI history is deliberately not used as
//! orchestration state.
//!
//! Every host runs its flows through this driver, so the App and the
//! workspace service cannot drift apart on stage order. A host supplies what
//! only it can: the view's inputs, the bucket lease, the stage launches and
//! the arena runs ([`ReviewFlowHost`]).

mod deep;
mod host;
mod output;
mod progress;
mod record;
mod results;
mod standard;
mod view;

pub use host::{ReviewFlowHost, ReviewFlowInputs, ReviewFlowLease, StageCompletion};
pub use record::{flow_from_record, holder_of, rerun_plan, task_from_stage};
pub use results::{flow_run_results, ReviewFlowResultFinding, ReviewFlowResults};
pub use view::{
    cached_diff_matches_scope, filtered_review_diff, review_flow_inputs, review_md_source,
    shows_target, ReviewedTab,
};

use crate::ai::flow_providers::{
    assign_lenses_to_models, resolve_review_models, FlowLensAssignment, FlowProvider,
    ResolvedReviewModels,
};
use crate::ai::review_flow::{plan_for_triage, FlowDepth, FlowEvent, FlowState, ReviewFlow};
use crate::ai::review_flow::{FlowTaskStage, FlowTaskStatus};
use crate::ai::triage::TriageReview;
use crate::arena::{
    load_run, ArenaPaths, ArenaScope, ReviewerRef, RunStatus, SeededStartParams, Verdict,
};
use crate::config::{AiSelection, ErConfig};
use crate::task_target::BackgroundTaskTarget;
use anyhow::{bail, Context, Result};
use output::{archive_flow_output, validate_flow_output, FlowOutputBackup, FlowProducer};
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Triage,
    Finder,
}

#[derive(Debug, Clone)]
struct OwnedTask {
    stage: Stage,
    lens: Option<String>,
    producer: FlowProducer,
    archive_key: String,
}

/// Captured execution inputs. This is private driver state because a later tab
/// switch must not retarget accepted work.
pub struct ReviewFlowContext {
    lease: ReviewFlowLease,
    target: BackgroundTaskTarget,
    scope: String,
    arena_scope: ArenaScope,
    /// Checkout used by providers, distinct from `target.repo_root` identity.
    repo_root: String,
    /// Prompt note naming the ref that holds the reviewed code, set when
    /// `repo_root` is a checkout of another branch. Every stage appends it.
    reviewed_ref_note: Option<String>,
    branch_ref: String,
    base_branch: String,
    raw_diff: String,
    config: ErConfig,
    model_pools: ResolvedReviewModels,
    assignments: Vec<FlowLensAssignment>,
    arbiter: Option<ReviewerRef>,
    owned_tasks: HashMap<String, OwnedTask>,
    enqueued_lenses: HashSet<String>,
    successful_lenses: HashSet<String>,
    successful_finder_providers: Vec<String>,
    successful_arbiter_provider: Option<String>,
    backups: HashMap<String, FlowOutputBackup>,
    next_archive: u64,
    arena_run_id: Option<String>,
    arena_progress_round: u32,
    professor_follow_up_suggested: bool,
    draining: bool,
}

/// Every review flow one host runs, keyed by the exact review bucket path:
/// the visible projection, its captured execution context, and the original
/// target of each flow-owned arena run.
#[derive(Default)]
pub struct ReviewFlows {
    pub(crate) flows: HashMap<String, ReviewFlow>,
    pub(crate) contexts: HashMap<String, ReviewFlowContext>,
    revision: u64,
    /// Acceptance must resolve its bucket and view identity from this
    /// capture, never the active tab.
    run_targets: HashMap<(String, String), BackgroundTaskTarget>,
}

impl ReviewFlows {
    /// Start a flow against the host's view. `scope` must identify that
    /// exact view; branch, staged and unstaged diffs are supported.
    pub fn start_review_flow(&mut self, host: &mut dyn ReviewFlowHost, scope: &str) -> Result<()> {
        self.start_review_flow_inner(host, scope, None)
    }

    /// Re-run an interrupted workflow by its [`rerun_plan`]: resume its
    /// unfinished stages, or start a new flow.
    pub fn rerun_review_flow(&mut self, host: &mut dyn ReviewFlowHost, er_dir: &str) -> Result<()> {
        let store = crate::store_registry::store();
        let scope = er_store::Scope::dir(er_dir)?;
        let record = crate::ai::workflow_record::load(&store, &scope)?
            .ok_or_else(|| anyhow::anyhow!("no workflow record exists for this bucket"))?;
        if !record.resumable() {
            bail!("this review has no stage that can be resumed");
        }
        let current = crate::ai::prepared_diff::diff_tmp_hash(er_dir).unwrap_or_default();
        match rerun_plan(&record, &current) {
            Some(plan) => self.start_review_flow_inner(host, "branch", Some((&record, &plan))),
            None => self.start_review_flow(host, "branch"),
        }
    }

    fn start_review_flow_inner(
        &mut self,
        host: &mut dyn ReviewFlowHost,
        scope: &str,
        resume: Option<(
            &crate::ai::WorkflowRecord,
            &crate::ai::workflow_record::ResumePlan,
        )>,
    ) -> Result<()> {
        let ReviewFlowInputs {
            target,
            arena_scope,
            repo_root,
            reviewed_ref_note,
            branch_ref,
            base_branch,
            raw_diff,
            owned_rules,
            review_md_source,
        } = host.review_flow_inputs(scope)?;

        let model_pools = resolve_review_models(&host.config().ai_hub)?;
        let triage_provider = model_pools
            .fast
            .first()
            .cloned()
            .context("no eligible Fast review model is configured")?;
        let mut config = host.config().clone();
        config.ai_hub.default_effort = crate::config::resolve_effort(
            &config.ai_hub,
            &config.agent,
            host.current_ai_effort(),
            None,
        );
        let triage_effort = crate::config::resolve_effort_for_model(
            &config.ai_hub,
            &config.agent,
            Some(&triage_provider.provider_id),
            Some(&triage_provider.model_id),
            config.ai_hub.default_effort.as_deref(),
            Some("low"),
        );
        let mut triage_config = config.clone();
        triage_config.ai_hub.default_effort = triage_effort.clone();
        let triage_selection = AiSelection {
            provider_id: Some(triage_provider.provider_id.clone()),
            model_id: Some(triage_provider.model_id.clone()),
            effort: triage_effort,
        };

        // Acquire before touching any shared prepared-diff or output files.
        // App mutation is serialized by the caller's App lock.
        host.recover_arena_bucket(&target.er_dir)?;
        let lease = host.acquire_review_flow_lease(&target.er_dir)?;
        let review_md = crate::ai::review_md::collect(host.ports(), &review_md_source, &raw_diff);
        let diff_hash = match crate::ai::prepared_diff::ensure_review_inputs(
            &target.er_dir,
            &raw_diff,
            owned_rules.as_rules(),
            review_md.as_deref(),
        ) {
            Ok(hash) => hash,
            Err(error) => {
                let _ = host.release_review_flow_lease(&lease);
                bail!("failed to prepare review inputs: {error}");
            }
        };

        if let Some((record, plan)) = resume {
            if record.diff_hash != diff_hash {
                let _ = host.release_review_flow_lease(&lease);
                bail!("the prepared diff changed; start a new review flow");
            }
            let depth = match record.kind {
                Some(crate::ai::WorkflowKind::Quick) => FlowDepth::Quick,
                Some(crate::ai::WorkflowKind::Standard) => FlowDepth::Standard,
                _ => {
                    let _ = host.release_review_flow_lease(&lease);
                    bail!("this workflow cannot be resumed");
                }
            };
            let rerun_lenses = plan.lenses();
            // Every finder row the record holds, kept or not. A kept finder
            // already wrote its output, which verification reads; an
            // unfinished one queues again.
            let record_lenses: Vec<String> = record
                .stages
                .iter()
                .filter(|stage| stage.stage == crate::ai::StageKind::Finding)
                .filter_map(|stage| stage.id.strip_prefix("finding:").map(str::to_string))
                .collect();
            let verifying_rerun = plan
                .rerun
                .iter()
                .any(|stage| stage.id == "verifying:arbiter");
            if rerun_lenses.is_empty() && !verifying_rerun {
                let _ = host.release_review_flow_lease(&lease);
                bail!("this workflow has no stage that can be resumed");
            }
            if rerun_lenses.is_empty() && record_lenses.is_empty() {
                let _ = host.release_review_flow_lease(&lease);
                bail!("this workflow has no finished finders to verify");
            }
            let mut flow = record::flow_from_record(record);
            // The projection is terminal; the resumed plan starts at finding.
            flow.state = FlowState::Triaging;
            // The plan names every finder the record holds, so the kept ones
            // can be marked successful below and verification sees them.
            flow.apply(FlowEvent::TriageSucceeded {
                plan: Some((depth, record_lenses.clone())),
            });
            for task in &mut flow.tasks {
                task.status = if plan.kept.iter().any(|stage| stage.id == task.id) {
                    FlowTaskStatus::Done
                } else {
                    FlowTaskStatus::Waiting
                };
            }
            let mut context = ReviewFlowContext {
                lease,
                target: target.clone(),
                scope: scope.to_owned(),
                arena_scope,
                repo_root,
                reviewed_ref_note,
                branch_ref,
                base_branch,
                raw_diff,
                config,
                model_pools,
                assignments: Vec::new(),
                arbiter: None,
                owned_tasks: HashMap::new(),
                enqueued_lenses: HashSet::new(),
                successful_lenses: HashSet::new(),
                successful_finder_providers: Vec::new(),
                successful_arbiter_provider: None,
                backups: HashMap::new(),
                next_archive: 2,
                arena_run_id: None,
                arena_progress_round: 0,
                professor_follow_up_suggested: false,
                draining: false,
            };
            // A kept finder counts as successful and never queues again. Its
            // provider still comes from `plan_finders`, so the seeded run can
            // map the lens it verifies to the model that produced it.
            for lens in &record_lenses {
                if !rerun_lenses.contains(lens) {
                    context.enqueued_lenses.insert(lens.clone());
                    context.successful_lenses.insert(lens.clone());
                }
            }
            if let Err(error) = Self::plan_finders(&mut context, depth, &record_lenses) {
                let _ = host.release_review_flow_lease(&context.lease);
                return Err(error);
            }
            // Mark the kept finders successful in the flow, not only in the
            // context: verification starts from the flow's finder set.
            for lens in &record_lenses {
                if rerun_lenses.contains(lens) {
                    continue;
                }
                flow.apply(FlowEvent::FinderSucceeded { lens: lens.clone() });
                record_successful_finder_provider(&mut context, lens);
            }
            flow.successful_finder_providers = context.successful_finder_providers.clone();
            flow.providers_used = flow.successful_finder_providers.clone();
            let key = target.er_dir.clone();
            if let Err(error) = Self::enqueue_finders(host, &mut context, &mut flow) {
                let _ = host.release_review_flow_lease(&context.lease);
                return Err(error);
            }
            self.flows.insert(key.clone(), flow);
            self.contexts.insert(key.clone(), context);
            if let Some(flow) = self.flows.get(&key) {
                Self::sync_workflow_record(&key, flow);
            }
            self.bump_review_flow_revision();
            return Ok(());
        }

        // The record exists before the first stage starts, so a crash at any
        // point leaves its owner and plan on disk.
        let Some(holder) = host.workflow_holder(&target.er_dir) else {
            let _ = host.release_review_flow_lease(&lease);
            bail!("review flow lease is missing its holder");
        };
        if let Err(error) =
            Self::create_workflow_record(&target.er_dir, None, &diff_hash, &[], None, holder)
        {
            let _ = host.release_review_flow_lease(&lease);
            return Err(error).context("write the workflow record");
        }

        let producer = FlowProducer::Triage;
        let archive_key = make_archive_key(lease.sequence(), 1);
        let backup = match archive_flow_output(Path::new(&target.er_dir), &archive_key, &producer) {
            Ok(backup) => backup,
            Err(error) => {
                let _ = host.release_review_flow_lease(&lease);
                return Err(error).context("archive prior triage output");
            }
        };
        let prompt = crate::ai::prompts::with_reviewed_ref_note(
            crate::ai::prompts::build_triage_review_prompt_prepared_diff(
                scope,
                &target.er_dir,
                &diff_hash,
            ),
            reviewed_ref_note.as_deref(),
        );
        let id = match host.enqueue_with_selection(
            crate::ai::triage_task_kind(),
            "triage",
            target.clone(),
            repo_root.clone(),
            prompt,
            triage_config,
            triage_selection,
        ) {
            Ok(id) => id,
            Err(error) => {
                let _ = backup.restore();
                let _ = host.release_review_flow_lease(&lease);
                return Err(error).context("enqueue review triage");
            }
        };
        host.register_review_flow_task(id.clone());
        let mut flow = ReviewFlow::new(diff_hash);
        let mut triage_progress = progress::task_progress(
            &config,
            "triage".into(),
            FlowTaskStage::Triage,
            None,
            "Triage".into(),
            "Choose review depth and lenses".into(),
            &triage_provider,
        );
        triage_progress.background_task_id = Some(id.clone());
        triage_progress.status =
            progress::background_status(host, &id).unwrap_or(FlowTaskStatus::Queued);
        flow.tasks.push(triage_progress);
        let mut owned_tasks = HashMap::new();
        owned_tasks.insert(
            id,
            OwnedTask {
                stage: Stage::Triage,
                lens: None,
                producer,
                archive_key: archive_key.clone(),
            },
        );
        let key = target.er_dir.clone();
        let mut backups = HashMap::new();
        backups.insert(archive_key, backup);
        let start_notice = flow_notification_message(&flow, &target, false);
        self.flows.insert(key.clone(), flow);
        self.contexts.insert(
            key.clone(),
            ReviewFlowContext {
                lease,
                target,
                scope: scope.to_owned(),
                arena_scope,
                repo_root,
                reviewed_ref_note,
                branch_ref,
                base_branch,
                raw_diff,
                config,
                model_pools,
                assignments: Vec::new(),
                arbiter: None,
                owned_tasks,
                enqueued_lenses: HashSet::new(),
                successful_lenses: HashSet::new(),
                successful_finder_providers: Vec::new(),
                successful_arbiter_provider: None,
                backups,
                next_archive: 2,
                arena_run_id: None,
                arena_progress_round: 0,
                professor_follow_up_suggested: false,
                draining: false,
            },
        );
        if let Some(flow) = self.flows.get(&key) {
            Self::sync_workflow_record(&key, flow);
        }
        host.notify(&start_notice);
        self.bump_review_flow_revision();
        Ok(())
    }

    /// Advance every flow using exact registered task completions and captured
    /// arena run IDs. Called by the background poll hook after queue dispatch.
    pub fn advance_review_flows(&mut self, host: &mut dyn ReviewFlowHost) {
        let keys: Vec<String> = self.contexts.keys().cloned().collect();
        for key in keys {
            self.advance_review_flow(host, &key);
        }
    }

    pub fn advance_review_flow(&mut self, host: &mut dyn ReviewFlowHost, key: &str) {
        let Some(mut context) = self.contexts.remove(key) else {
            return;
        };
        let Some(mut flow) = self.flows.remove(key) else {
            self.contexts.insert(key.to_owned(), context);
            return;
        };
        let mut changed = progress::refresh_background_tasks(host, &mut flow);

        let task_ids: Vec<String> = context.owned_tasks.keys().cloned().collect();
        for id in task_ids {
            let Some(task) = host.take_review_flow_task_completion(&id) else {
                continue;
            };
            let Some(owned) = context.owned_tasks.remove(&id) else {
                continue;
            };
            let expected_kind = match owned.stage {
                Stage::Triage => crate::ai::triage_task_kind(),
                Stage::Finder => owned.lens.as_deref().map_or_else(
                    || "review".to_owned(),
                    |lens| {
                        if lens == "general" {
                            "review".to_owned()
                        } else {
                            crate::ai::expert_task_kind(lens)
                        }
                    },
                ),
            };
            let task_ok = task.target == context.target
                && task.kind == expected_kind
                && task.done
                && task.error.is_none();
            if !task_ok {
                let reason = task
                    .error
                    .clone()
                    .unwrap_or_else(|| "background task failed".into());
                let cancelled_before_launch = reason == "cancelled before launch"
                    && flow.tasks.iter().any(|progress| {
                        progress.background_task_id.as_deref() == Some(id.as_str())
                            && progress.status == FlowTaskStatus::Cancelled
                    });
                progress::set_background_result(
                    &mut flow,
                    &id,
                    if cancelled_before_launch {
                        FlowTaskStatus::Cancelled
                    } else {
                        FlowTaskStatus::Failed
                    },
                    Some(reason.clone()),
                );
                Self::restore_backup_for(&mut context, &owned.archive_key);
                match owned.stage {
                    Stage::Triage => host.apply_flow_event_notifying(
                        &mut flow,
                        &context.target,
                        key,
                        context.draining,
                        FlowEvent::TriageFailed { reason },
                    ),
                    Stage::Finder => host.apply_flow_event_notifying(
                        &mut flow,
                        &context.target,
                        key,
                        context.draining,
                        FlowEvent::FinderFailed {
                            lens: owned.lens.unwrap_or_default(),
                            reason,
                        },
                    ),
                }
                changed = true;
                continue;
            }

            if let Err(error) = validate_flow_output(
                Path::new(&context.target.er_dir),
                &owned.producer,
                &flow.diff_hash,
            ) {
                Self::restore_backup_for(&mut context, &owned.archive_key);
                let reason = error.to_string();
                progress::set_background_result(
                    &mut flow,
                    &id,
                    FlowTaskStatus::Failed,
                    Some(reason.clone()),
                );
                match owned.stage {
                    Stage::Triage => host.apply_flow_event_notifying(
                        &mut flow,
                        &context.target,
                        key,
                        context.draining,
                        FlowEvent::TriageFailed { reason },
                    ),
                    Stage::Finder => host.apply_flow_event_notifying(
                        &mut flow,
                        &context.target,
                        key,
                        context.draining,
                        FlowEvent::FinderFailed {
                            lens: owned.lens.unwrap_or_default(),
                            reason,
                        },
                    ),
                }
                changed = true;
                continue;
            }

            match owned.stage {
                Stage::Triage => {
                    let triage = match read_triage(&context.target.er_dir) {
                        Ok(triage) => triage,
                        Err(error) => {
                            Self::restore_backup_for(&mut context, &owned.archive_key);
                            progress::set_background_result(
                                &mut flow,
                                &id,
                                FlowTaskStatus::Failed,
                                Some(error.to_string()),
                            );
                            host.apply_flow_event_notifying(
                                &mut flow,
                                &context.target,
                                key,
                                context.draining,
                                FlowEvent::TriageFailed {
                                    reason: error.to_string(),
                                },
                            );
                            changed = true;
                            continue;
                        }
                    };
                    progress::set_background_result(&mut flow, &id, FlowTaskStatus::Done, None);
                    context.professor_follow_up_suggested = triage.verdict.primary
                        == crate::ai::triage::TriageVerdictPrimary::Professor;
                    flow.professor_follow_up_suggested = context.professor_follow_up_suggested;
                    let plan = plan_for_triage(&triage);
                    if let Some((depth, lenses)) = &plan {
                        if let Err(error) =
                            Self::prepare_triage_plan(&mut context, &mut flow, *depth, lenses)
                        {
                            host.apply_flow_event_notifying(
                                &mut flow,
                                &context.target,
                                key,
                                context.draining,
                                FlowEvent::TriageFailed {
                                    reason: error.to_string(),
                                },
                            );
                            changed = true;
                            continue;
                        }
                    }
                    host.apply_flow_event_notifying(
                        &mut flow,
                        &context.target,
                        key,
                        context.draining,
                        FlowEvent::TriageSucceeded { plan },
                    );
                    if flow.state == FlowState::Finding {
                        if let Err(error) = Self::enqueue_finders(host, &mut context, &mut flow) {
                            host.apply_flow_event_notifying(
                                &mut flow,
                                &context.target,
                                key,
                                context.draining,
                                FlowEvent::DriverFailed {
                                    reason: error.to_string(),
                                },
                            );
                        }
                    }
                }
                Stage::Finder => {
                    if let Some(lens) = owned.lens {
                        progress::set_background_result(&mut flow, &id, FlowTaskStatus::Done, None);
                        context.successful_lenses.insert(lens.clone());
                        record_successful_finder_provider(&mut context, &lens);
                        host.apply_flow_event_notifying(
                            &mut flow,
                            &context.target,
                            key,
                            context.draining,
                            FlowEvent::FinderSucceeded { lens },
                        );
                        flow.successful_finder_providers =
                            context.successful_finder_providers.clone();
                        flow.providers_used = flow.successful_finder_providers.clone();
                    }
                }
            }
            changed = true;
        }

        if flow.state == FlowState::Finding
            && flow.pending_finders.is_empty()
            && !context.successful_lenses.is_empty()
            && context.arena_run_id.is_none()
        {
            match self.start_seeded_verification(host, &mut context, &mut flow) {
                Ok(()) => changed = true,
                Err(error) => {
                    host.apply_flow_event_notifying(
                        &mut flow,
                        &context.target,
                        key,
                        context.draining,
                        FlowEvent::DriverFailed {
                            reason: error.to_string(),
                        },
                    );
                    changed = true;
                }
            }
        }

        if let Some(run_id) = context.arena_run_id.clone() {
            let control = host.arena_control(&run_id);
            if let Some(control) = control.as_ref() {
                changed |= progress::refresh_arena_tasks(&mut context, &mut flow, control);
            }
            let live = host.arena_is_active(&run_id);
            if flow.state == FlowState::Verifying {
                // The control's status can become Complete before the
                // supervisor returns and saves its final record. Only the
                // persisted terminal record after it returns is authoritative.
                if !live {
                    let disk_run = load_run(&ArenaPaths::for_run(
                        Path::new(&context.target.er_dir),
                        &run_id,
                    ))
                    .ok();
                    // The control's final progress is still in memory; apply it
                    // before closing the task rows.
                    if let Some(control) = control.as_ref() {
                        progress::refresh_arena_tasks(&mut context, &mut flow, control);
                    }
                    match disk_run {
                        Some(run) if run.status == RunStatus::Complete => {
                            progress::finish_arena_tasks(&context, &mut flow, &run);
                            let (kept, dropped, awaiting_accept) =
                                record_verified_run(&mut context, &mut flow, &run);
                            host.apply_flow_event_notifying(
                                &mut flow,
                                &context.target,
                                key,
                                context.draining,
                                FlowEvent::VerifyDone {
                                    kept,
                                    dropped,
                                    awaiting_accept,
                                },
                            );
                            changed = true;
                        }
                        Some(run) if run.status == RunStatus::Failed => {
                            progress::finish_arena_tasks(&context, &mut flow, &run);
                            host.apply_flow_event_notifying(
                                &mut flow,
                                &context.target,
                                key,
                                context.draining,
                                FlowEvent::VerifyFailed {
                                    reason: "arena verification failed".into(),
                                },
                            );
                            changed = true;
                        }
                        Some(run) if run.status == RunStatus::Cancelled => {
                            progress::finish_arena_tasks(&context, &mut flow, &run);
                            host.apply_flow_event_notifying(
                                &mut flow,
                                &context.target,
                                key,
                                context.draining,
                                FlowEvent::Cancel,
                            );
                            changed = true;
                        }
                        Some(_) => {
                            host.apply_flow_event_notifying(
                                &mut flow,
                                &context.target,
                                key,
                                context.draining,
                                FlowEvent::VerifyFailed {
                                    reason: "arena supervisor drained before a terminal run was persisted".into(),
                                },
                            );
                            changed = true;
                        }
                        None => {
                            host.apply_flow_event_notifying(
                                &mut flow,
                                &context.target,
                                key,
                                context.draining,
                                FlowEvent::VerifyFailed {
                                    reason: "completed arena run could not be read after supervisor drain".into(),
                                },
                            );
                            changed = true;
                        }
                    }
                }
            } else if flow.state == FlowState::Cancelled && !live {
                if let Ok(run) = load_run(&ArenaPaths::for_run(
                    Path::new(&context.target.er_dir),
                    &run_id,
                )) {
                    if let Some(control) = control.as_ref() {
                        changed |= progress::refresh_arena_tasks(&mut context, &mut flow, control);
                    }
                    changed |= progress::finish_arena_tasks(&context, &mut flow, &run);
                }
            }
        }

        match flow.state.clone() {
            FlowState::Failed { reason } => {
                changed |= progress::finish_unstarted_tasks(
                    &mut flow,
                    FlowTaskStatus::Skipped,
                    Some(&reason),
                );
            }
            FlowState::Cancelled => {
                changed |= progress::cancel_waiting_tasks(&mut flow);
            }
            FlowState::Done { .. } | FlowState::Skipped => {
                changed |=
                    progress::finish_unstarted_tasks(&mut flow, FlowTaskStatus::Skipped, None);
            }
            _ => {}
        }
        self.flows.insert(key.to_owned(), flow);
        if changed {
            self.bump_review_flow_revision();
        }
        if let Some(flow) = self.flows.get(key) {
            Self::sync_workflow_record(key, flow);
        }
        let terminal = !self.flows.get(key).is_some_and(ReviewFlow::is_running);
        if terminal {
            let outcome = match self.flows.get(key).map(|flow| &flow.state) {
                Some(FlowState::Done { .. } | FlowState::Skipped) => {
                    Some(crate::ai::workflow_record::WorkflowOutcome::Complete)
                }
                Some(FlowState::Failed { .. }) => {
                    Some(crate::ai::workflow_record::WorkflowOutcome::Failed)
                }
                Some(FlowState::Cancelled) => {
                    Some(crate::ai::workflow_record::WorkflowOutcome::Cancelled)
                }
                _ => None,
            };
            if let Some(outcome) = outcome {
                Self::finish_workflow_record(key, outcome);
            }
        }
        let has_producers = Self::flow_context_has_producers(host, &context);
        let was_draining = context.draining;
        context.draining = terminal && has_producers;
        if context.draining && !was_draining {
            let notice = self
                .flows
                .get(key)
                .map(|flow| flow_notification_message(flow, &context.target, true));
            if let Some(notice) = notice {
                host.notify(&notice);
            }
        }
        if terminal && !has_producers {
            let lease = context.lease.clone();
            match host.release_review_flow_lease(&lease) {
                Ok(()) => {
                    self.bump_review_flow_revision();
                    if was_draining {
                        host.notify(&format!(
                            "Review flow for {} has drained.",
                            context.target.branch_label
                        ));
                    }
                    // Keep terminal flow projection and captured run target for
                    // later generation-checked acceptance.
                }
                Err(error) => {
                    host.notify_long(&format!("Review flow cleanup is still pending: {error}"));
                    context.draining = true;
                    self.contexts.insert(key.to_owned(), context);
                }
            }
        } else {
            self.contexts.insert(key.to_owned(), context);
        }
    }

    fn plan_finders(
        context: &mut ReviewFlowContext,
        depth: FlowDepth,
        lenses: &[String],
    ) -> Result<()> {
        let finder_pool = match depth {
            FlowDepth::Quick => &context.model_pools.fast,
            FlowDepth::Standard => &context.model_pools.standard,
            FlowDepth::Deep => &context.model_pools.advanced,
        };
        context.assignments = assign_lenses_to_models(depth, lenses, finder_pool)?;
        let assigned_providers: Vec<String> = context
            .assignments
            .iter()
            .flat_map(|a| a.providers.iter().map(|p| p.provider_id.clone()))
            .collect();
        context.arbiter = Some(crate::arena::cross_model_arbiter(
            &context.config.ai_hub,
            &context.model_pools.advanced,
            &assigned_providers,
        )?);
        Ok(())
    }

    fn prepare_triage_plan(
        context: &mut ReviewFlowContext,
        flow: &mut ReviewFlow,
        depth: FlowDepth,
        lenses: &[String],
    ) -> Result<()> {
        Self::plan_finders(context, depth, lenses)?;
        if depth == FlowDepth::Deep {
            Self::prepare_deep(context, flow)?;
        }
        progress::append_planned_tasks(context, flow, depth);
        Ok(())
    }

    fn enqueue_finders(
        host: &mut dyn ReviewFlowHost,
        context: &mut ReviewFlowContext,
        flow: &mut ReviewFlow,
    ) -> Result<()> {
        for assignment in context.assignments.clone() {
            if context.enqueued_lenses.contains(&assignment.lens) {
                continue;
            }
            let Some(provider) = assignment.providers.first() else {
                flow.apply(FlowEvent::FinderFailed {
                    lens: assignment.lens.clone(),
                    reason: "no provider assigned".into(),
                });
                continue;
            };
            let producer = if assignment.lens == "general" {
                FlowProducer::General
            } else {
                FlowProducer::Expert(assignment.lens.clone())
            };
            let archive_key = make_archive_key(context.lease.sequence(), context.next_archive);
            context.next_archive = context.next_archive.wrapping_add(1).max(2);
            let backup =
                archive_flow_output(Path::new(&context.target.er_dir), &archive_key, &producer)?;
            let prompt = if assignment.lens == "general" {
                crate::ai::prompts::build_review_prompt_prepared_diff(
                    &context.scope,
                    &context.target.er_dir,
                    &context.base_branch,
                    &context.branch_ref,
                    &flow.diff_hash,
                    None,
                )
            } else {
                crate::ai::prompts::build_expert_review_prompt_prepared_diff(
                    &context.scope,
                    &context.target.er_dir,
                    &assignment.lens,
                    &flow.diff_hash,
                    None,
                )
            };
            let prompt = crate::ai::prompts::with_reviewed_ref_note(
                prompt,
                context.reviewed_ref_note.as_deref(),
            );
            let selection = selection_for_provider(context, provider);
            let mut launch_config = context.config.clone();
            launch_config.ai_hub.default_effort = selection.effort.clone();
            let (kind, command) = if assignment.lens == "general" {
                ("review".to_owned(), "review".to_owned())
            } else {
                (
                    crate::ai::expert_task_kind(&assignment.lens),
                    format!("expert-{}", assignment.lens),
                )
            };
            match host.enqueue_with_selection(
                kind,
                &command,
                context.target.clone(),
                context.repo_root.clone(),
                prompt,
                launch_config,
                selection,
            ) {
                Ok(id) => {
                    let progress_id = format!("finding:{}", assignment.lens);
                    if let Some(task) = flow.tasks.iter_mut().find(|task| task.id == progress_id) {
                        task.background_task_id = Some(id.clone());
                        task.status = progress::background_status(host, &id)
                            .unwrap_or(FlowTaskStatus::Queued);
                    }
                    host.register_review_flow_task(id.clone());
                    context.owned_tasks.insert(
                        id,
                        OwnedTask {
                            stage: Stage::Finder,
                            lens: Some(assignment.lens.clone()),
                            producer,
                            archive_key: archive_key.clone(),
                        },
                    );
                    context.backups.insert(archive_key, backup);
                    context.enqueued_lenses.insert(assignment.lens);
                }
                Err(error) => {
                    let _ = backup.restore();
                    progress::set_task_status(
                        flow,
                        &format!("finding:{}", assignment.lens),
                        FlowTaskStatus::Failed,
                        Some(error.to_string()),
                    );
                    flow.apply(FlowEvent::FinderFailed {
                        lens: assignment.lens,
                        reason: error.to_string(),
                    });
                }
            }
        }
        Ok(())
    }

    fn start_seeded_verification(
        &mut self,
        host: &mut dyn ReviewFlowHost,
        context: &mut ReviewFlowContext,
        flow: &mut ReviewFlow,
    ) -> Result<()> {
        let arbiter = context
            .arbiter
            .clone()
            .context("review-flow arbiter was not captured")?;
        let successful: Vec<String> = context.successful_lenses.iter().cloned().collect();
        let mut lens_providers = std::collections::BTreeMap::new();
        for lens in &successful {
            let assignment = context
                .assignments
                .iter()
                .find(|assignment| assignment.lens == *lens)
                .with_context(|| {
                    format!("successful lens `{lens}` has no captured provider assignment")
                })?;
            let provider = assignment
                .providers
                .first()
                .with_context(|| format!("successful lens `{lens}` has no captured provider"))?;
            lens_providers.insert(
                lens.clone(),
                ReviewerRef {
                    provider_id: provider.provider_id.clone(),
                    model_id: provider.model_id.clone(),
                    agent_kind: Some(lens.clone()),
                },
            );
        }
        let params = SeededStartParams {
            er_dir: context.target.er_dir.clone(),
            branch_ref: context.branch_ref.clone(),
            base_branch: context.base_branch.clone(),
            scope: context.arena_scope,
            diff_hash: flow.diff_hash.clone(),
            review_hash: String::new(),
            raw_diff: context.raw_diff.clone(),
            arbiter: Some(arbiter),
            include_general: successful.iter().any(|lens| lens == "general"),
            source_lenses: Some(successful),
            lens_providers,
            reviewed_ref_note: context.reviewed_ref_note.clone(),
        };
        match host.arena_start_seeded_for(
            context.repo_root.clone(),
            context.config.clone(),
            params,
            &context.lease,
        )? {
            Some(run_id) => {
                flow.arena_run_id = Some(run_id.clone());
                context.arena_run_id = Some(run_id.clone());
                context.arena_progress_round = 0;
                self.remember_review_flow_run_target(&context.target, &run_id);
                host.apply_flow_event_notifying(
                    flow,
                    &context.target,
                    &context.target.er_dir,
                    context.draining,
                    FlowEvent::VerifyStarted,
                );
            }
            None => {
                progress::set_arbiter_skipped(flow);
                host.apply_flow_event_notifying(
                    flow,
                    &context.target,
                    &context.target.er_dir,
                    context.draining,
                    FlowEvent::NoCandidates,
                );
            }
        }
        Ok(())
    }

    fn flow_context_has_producers(host: &dyn ReviewFlowHost, context: &ReviewFlowContext) -> bool {
        !context.owned_tasks.is_empty()
            || context
                .arena_run_id
                .as_ref()
                .is_some_and(|id| host.arena_is_active(id))
    }

    fn restore_backup_for(context: &mut ReviewFlowContext, archive_key: &str) {
        // Backups remain recovery material even when the producer wrote a
        // parseable but stale generation. Restore only follows output helper
        // rules and never overwrites a newer sidecar.
        if let Some(backup) = context.backups.get(archive_key) {
            let _ = backup.restore();
        }
    }

    /// Cancel orchestration. Running finder processes drain naturally; queued
    /// owned tasks are retired with their exact completion records.
    pub fn cancel_review_flow(&mut self, host: &mut dyn ReviewFlowHost, er_dir: &str) -> bool {
        let Some(flow) = self.flows.get(er_dir) else {
            return false;
        };
        if !flow.is_running() {
            return false;
        }
        let (ids, run_id) = self.contexts.get(er_dir).map_or_else(
            || (Vec::new(), None),
            |context| {
                (
                    context.owned_tasks.keys().cloned().collect(),
                    context.arena_run_id.clone(),
                )
            },
        );
        let target = self
            .contexts
            .get(er_dir)
            .map(|context| context.target.clone());
        if let Some(context) = self.contexts.get_mut(er_dir) {
            context.draining = true;
        }
        let notice = if let Some(flow) = self.flows.get_mut(er_dir) {
            let previous = flow.state.clone();
            flow.apply(FlowEvent::Cancel);
            (flow.state != previous).then(|| {
                target.as_ref().map_or_else(
                    || "Review flow was cancelled.".to_owned(),
                    |target| flow_notification_message(flow, target, true),
                )
            })
        } else {
            None
        };
        if let Some(notice) = notice {
            host.notify(&notice);
        }
        for id in ids {
            if host.cancel_queued_background_task(&id) {
                if let Some(flow) = self.flows.get_mut(er_dir) {
                    progress::set_background_result(
                        flow,
                        &id,
                        FlowTaskStatus::Cancelled,
                        Some("cancelled before launch".into()),
                    );
                }
            }
        }
        if let Some(flow) = self.flows.get_mut(er_dir) {
            progress::cancel_waiting_tasks(flow);
        }
        if let Some(run_id) = run_id {
            let _ = host.arena_cancel(&run_id);
        }
        if let Some(flow) = self.flows.get(er_dir) {
            Self::sync_workflow_record(er_dir, flow);
        }
        self.bump_review_flow_revision();
        true
    }

    pub fn review_flow_for(&self, er_dir: &str) -> Option<&ReviewFlow> {
        self.flows.get(er_dir)
    }

    /// Revision for host snapshot invalidation. It changes when a flow's
    /// visible projection changes, not on every poll.
    pub fn review_flow_revision(&self) -> u64 {
        self.revision
    }

    /// True while terminal flow state still has a process or arena supervisor
    /// draining from the bucket.
    pub fn review_flow_is_draining(&self, er_dir: &str) -> bool {
        self.contexts
            .get(er_dir)
            .is_some_and(|context| context.draining)
            && self
                .flows
                .get(er_dir)
                .is_some_and(|flow| !flow.is_running())
    }

    pub fn review_flow_run_target(
        &self,
        er_dir: &str,
        run_id: &str,
    ) -> Option<&BackgroundTaskTarget> {
        self.run_targets
            .get(&(er_dir.to_owned(), run_id.to_owned()))
    }

    /// Every bucket with a flow, for a host that publishes them all.
    pub fn buckets(&self) -> impl Iterator<Item = &str> {
        self.flows.keys().map(String::as_str)
    }

    /// The captured target of each flow-owned run in `er_dir`, by run id.
    pub fn run_targets_in(&self, er_dir: &str) -> Vec<(String, BackgroundTaskTarget)> {
        self.run_targets
            .iter()
            .filter(|((bucket, _), _)| bucket == er_dir)
            .map(|((_, run_id), target)| (run_id.clone(), target.clone()))
            .collect()
    }

    pub fn remember_review_flow_run_target(&mut self, target: &BackgroundTaskTarget, run_id: &str) {
        self.run_targets
            .insert((target.er_dir.clone(), run_id.to_owned()), target.clone());
    }

    /// Professor triage keeps its ordinary Quick review and exposes this
    /// follow-up suggestion to a later snapshot/UI layer.
    pub fn review_flow_professor_follow_up_suggested(&self, er_dir: &str) -> bool {
        self.flows
            .get(er_dir)
            .is_some_and(|flow| flow.professor_follow_up_suggested)
    }

    pub(crate) fn bump_review_flow_revision(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// The flow for one bucket, the same answer as `review_flow_for`.
    pub fn get(&self, er_dir: &str) -> Option<&ReviewFlow> {
        self.flows.get(er_dir)
    }

    /// Show a flow that no driver call started, such as the projection of a
    /// recovered workflow record. It owns no context, so nothing advances it.
    pub fn insert(&mut self, er_dir: String, flow: ReviewFlow) {
        self.flows.insert(er_dir, flow);
    }
}

fn make_archive_key(lease_sequence: u64, stage_sequence: u64) -> String {
    format!(
        "{}-{}-{}",
        std::process::id(),
        lease_sequence,
        stage_sequence
    )
}

fn record_successful_finder_provider(context: &mut ReviewFlowContext, lens: &str) {
    let provider_id = context
        .assignments
        .iter()
        .find(|assignment| assignment.lens == lens)
        .and_then(|assignment| assignment.providers.first())
        .map(|provider| provider.provider_id.clone());
    if let Some(provider_id) = provider_id {
        context.successful_finder_providers.push(provider_id);
        context.successful_finder_providers.sort();
        context.successful_finder_providers.dedup();
    }
}

fn record_verified_run(
    context: &mut ReviewFlowContext,
    flow: &mut ReviewFlow,
    run: &crate::arena::ArenaRun,
) -> (usize, usize, usize) {
    let kept = run
        .findings
        .iter()
        .filter(|finding| matches!(finding.verdict, Verdict::Kept | Verdict::Escalated))
        .count();
    let dropped = run
        .findings
        .iter()
        .filter(|finding| finding.verdict == Verdict::Dropped)
        .count();
    let deep_run = flow.depth == Some(FlowDepth::Deep);
    let awaiting_accept = if deep_run {
        context.successful_finder_providers = run
            .reviewers
            .iter()
            .filter(|reviewer| reviewer.status == crate::arena::ReviewerRunStatus::Ok)
            .map(|reviewer| reviewer.provider_id.clone())
            .collect();
        context.successful_finder_providers.sort();
        context.successful_finder_providers.dedup();
        kept
    } else {
        0
    };
    context.successful_arbiter_provider = Some(run.config.arbiter.provider_id.clone());
    flow.successful_finder_providers = context.successful_finder_providers.clone();
    flow.successful_arbiter_provider = context.successful_arbiter_provider.clone();
    flow.providers_used = flow.successful_finder_providers.clone();
    flow.providers_used
        .extend(flow.successful_arbiter_provider.iter().cloned());
    flow.providers_used.sort();
    flow.providers_used.dedup();
    if deep_run {
        (0, 0, awaiting_accept)
    } else {
        (kept, dropped, 0)
    }
}

fn flow_notification_message(
    flow: &ReviewFlow,
    target: &BackgroundTaskTarget,
    draining: bool,
) -> String {
    let subject = if target.branch_label.is_empty() {
        format!("Review flow ({})", target.scope)
    } else {
        format!("Review flow for {} ({})", target.branch_label, target.scope)
    };
    match &flow.state {
        FlowState::Triaging => format!("{subject} started triage."),
        FlowState::Skipped => format!("{subject} was skipped by triage."),
        FlowState::AwaitingConfirmation => format!(
            "{subject} is ready for Deep review confirmation. Estimated cost ${:.2}; latency {} seconds.",
            flow.estimated_cost.unwrap_or_default(),
            flow.estimated_latency_seconds.unwrap_or_default()
        ),
        FlowState::Finding => {
            let depth = match flow.depth {
                Some(FlowDepth::Quick) => "Quick",
                Some(FlowDepth::Standard) => "Standard",
                Some(FlowDepth::Deep) => "Deep",
                None => "planned",
            };
            let lens_label = if flow.lenses.len() == 1 {
                "lens"
            } else {
                "lenses"
            };
            format!(
                "{subject} is running {depth} finders across {} {lens_label}.",
                flow.lenses.len()
            )
        }
        FlowState::Verifying => format!("{subject} is verifying finder results."),
        FlowState::Done {
            kept,
            dropped,
            awaiting_accept,
        } if flow.arena_run_id.is_none() && flow.successful_arbiter_provider.is_none() => {
            let providers = if flow.successful_finder_providers.is_empty() {
                String::new()
            } else {
                format!(
                    " Finder providers: {}.",
                    flow.successful_finder_providers.join(", ")
                )
            };
            let _ = (kept, dropped, awaiting_accept);
            format!("{subject} finished with no candidates to verify.{providers}")
        }
        FlowState::Done {
            kept,
            dropped,
            awaiting_accept,
        } => {
            let outcome = if *awaiting_accept > 0 {
                format!("{awaiting_accept} findings await acceptance")
            } else {
                format!("kept {kept}, dropped {dropped}")
            };
            let providers = if flow.providers_used.is_empty() {
                String::new()
            } else {
                format!(" Providers: {}.", flow.providers_used.join(", "))
            };
            let cross_provider_note =
                if flow.successful_arbiter_provider.is_some() && flow.providers_used.len() < 2 {
                    " The arbiter used one provider, so no cross-provider check ran."
                } else {
                    ""
                };
            format!("{subject} finished; {outcome}.{providers}{cross_provider_note}")
        }
        FlowState::Failed { reason } => format!("{subject} failed: {reason}"),
        FlowState::Cancelled if draining => {
            format!("{subject} was cancelled; background work is draining.")
        }
        FlowState::Cancelled => format!("{subject} was cancelled."),
        FlowState::Interrupted { unfinished } => format!(
            "{subject} was interrupted; {unfinished} unfinished stage(s) can be re-run."
        ),
    }
}

fn selection_for_provider(context: &ReviewFlowContext, provider: &FlowProvider) -> AiSelection {
    let effort = crate::config::resolve_effort_for_model(
        &context.config.ai_hub,
        &context.config.agent,
        Some(&provider.provider_id),
        Some(&provider.model_id),
        context.config.ai_hub.default_effort.as_deref(),
        None,
    );
    AiSelection {
        provider_id: Some(provider.provider_id.clone()),
        model_id: Some(provider.model_id.clone()),
        effort,
    }
}

fn read_triage(er_dir: &str) -> Result<TriageReview> {
    let store = crate::store_registry::store();
    let scope = er_store::Scope::dir(er_dir)?;
    let key = er_store::ArtifactKey::<TriageReview>::new(scope, ())?;
    let path = Path::new(er_dir).join("triage.json");
    match store
        .read(&key)
        .with_context(|| format!("read {}", path.display()))?
    {
        er_store::Loaded::Found(stored) => Ok(stored.value),
        er_store::Loaded::Absent => bail!("parse triage output: triage.json is missing"),
        er_store::Loaded::Unreadable(reason) => bail!("parse triage output: {reason:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        AiHubConfig, AiModelConfig, AiProviderConfig, ReviewModelGroups, ReviewModelRef,
    };

    struct TestDir(tempfile::TempDir);
    impl TestDir {
        fn new(_label: &str) -> Self {
            Self(tempfile::tempdir().unwrap())
        }

        fn path(&self) -> &std::path::Path {
            self.0.path()
        }
    }

    #[test]
    fn read_triage_reads_through_the_store() {
        let dir = TestDir::new("triage-store");
        crate::storage::with_test_storage_root(dir.path(), || {
            let store = crate::store_registry::store();
            let scope = er_store::Scope::dir(dir.path()).unwrap();
            let key = er_store::ArtifactKey::<TriageReview>::new(scope, ()).unwrap();
            let triage: TriageReview =
                serde_json::from_value(serde_json::json!({"version": 1, "diff_hash": "hash"}))
                    .unwrap();
            store.write(&key, &triage).unwrap();
            assert_eq!(
                read_triage(dir.path().to_str().unwrap()).unwrap().diff_hash,
                "hash"
            );

            // A version this build does not understand is unreadable, not a
            // parse the raw text would have accepted. The envelope carries the
            // kind's version, so a newer file is written directly.
            std::fs::write(
                store.path(&key),
                r#"{"kind":"triage","version":2,"diff_hash":null,"producer":"host","written_at":"2026-11-13T00:00:00Z","payload":{"version":2,"diff_hash":"hash"}}"#,
            )
            .unwrap();
            assert!(read_triage(dir.path().to_str().unwrap()).is_err());

            store.delete::<TriageReview>(&key).unwrap();
            assert!(read_triage(dir.path().to_str().unwrap()).is_err());
        });
    }

    /// The captured context a started flow plans from, holding the
    /// bucket's lease as a host would.
    fn context_for(bucket: &str, config: ErConfig) -> (ReviewFlowContext, er_store::LeaseGuard) {
        let scope = er_store::Scope::dir(bucket).unwrap();
        let guard = crate::store_registry::store()
            .lease(&scope, er_store::DEFAULT_LOCK_TIMEOUT, "flow-1")
            .unwrap();
        let target = BackgroundTaskTarget {
            repo_root: "/repo".into(),
            er_dir: bucket.into(),
            branch_label: "feature".into(),
            base_branch: "main".into(),
            scope: "branch".into(),
            pr_number: None,
            remote_repo: None,
            managed_local: true,
        };
        let context = ReviewFlowContext {
            lease: ReviewFlowLease::new(bucket.into(), 1, guard.holder().clone()),
            target,
            scope: "branch".into(),
            arena_scope: ArenaScope::Branch,
            repo_root: "/repo".into(),
            reviewed_ref_note: None,
            branch_ref: "feature".into(),
            base_branch: "main".into(),
            raw_diff: String::new(),
            model_pools: resolve_review_models(&config.ai_hub).unwrap(),
            config,
            assignments: Vec::new(),
            arbiter: None,
            owned_tasks: HashMap::new(),
            enqueued_lenses: HashSet::new(),
            successful_lenses: HashSet::new(),
            successful_finder_providers: Vec::new(),
            successful_arbiter_provider: None,
            backups: HashMap::new(),
            next_archive: 1,
            arena_run_id: None,
            arena_progress_round: 0,
            professor_follow_up_suggested: false,
            draining: false,
        };
        (context, guard)
    }

    /// Each depth plans its finders from its own model tier, and the
    /// arbiter comes from the advanced pool, on a provider the finders did
    /// not use when the pool has one. The captured-model half runs on the
    /// service host (er-service `flow_driver_tests::views`).
    #[test]
    fn flow_depths_use_their_model_tiers_and_advanced_arbiter_pool() {
        let dir = TestDir::new("model-tiers");
        crate::storage::with_test_storage_root(dir.path(), || {
            let providers = [
                ("fast", "fast-model", 0.5),
                ("standard", "standard-model", 1.0),
                ("advanced-a", "advanced-a-model", 8.0),
                ("advanced-b", "advanced-b-model", 3.0),
            ];
            let selection = |provider_id: &str, model_id: &str| ReviewModelRef {
                provider_id: provider_id.into(),
                model_id: model_id.into(),
            };
            let config = ErConfig {
                ai_hub: AiHubConfig {
                    providers: providers
                        .iter()
                        .map(|(provider_id, model_id, cost)| {
                            let provider = AiProviderConfig {
                                command: "/bin/true".into(),
                                models: vec![AiModelConfig {
                                    id: (*model_id).into(),
                                    cost_per_1k_in: Some(*cost),
                                    cost_per_1k_out: Some(0.0),
                                    ..Default::default()
                                }],
                                ..Default::default()
                            };
                            ((*provider_id).into(), provider)
                        })
                        .collect(),
                    default_provider: Some("fast".into()),
                    flow_providers: providers
                        .iter()
                        .map(|(provider_id, _, _)| (*provider_id).into())
                        .collect(),
                    review_models: ReviewModelGroups {
                        fast: vec![selection("fast", "fast-model")],
                        standard: vec![selection("standard", "standard-model")],
                        advanced: vec![
                            selection("advanced-a", "advanced-a-model"),
                            selection("advanced-b", "advanced-b-model"),
                        ],
                    },
                    ..Default::default()
                },
                ..Default::default()
            };
            let bucket = dir.path().to_string_lossy().into_owned();
            let (mut context, _lease) = context_for(&bucket, config);
            assert_eq!(context.model_pools.fast[0].model_id, "fast-model");
            assert_eq!(
                selection_for_provider(&context, &context.model_pools.fast[0])
                    .model_id
                    .as_deref(),
                Some("fast-model"),
                "triage uses the first resolved Fast model"
            );

            let lenses = vec!["general".to_owned()];
            ReviewFlows::plan_finders(&mut context, FlowDepth::Quick, &lenses).unwrap();
            assert_eq!(context.assignments[0].providers[0].model_id, "fast-model");
            assert_eq!(context.arbiter.as_ref().unwrap().provider_id, "advanced-a");
            assert_eq!(
                context.arbiter.as_ref().unwrap().model_id,
                "advanced-a-model"
            );

            ReviewFlows::plan_finders(&mut context, FlowDepth::Standard, &lenses).unwrap();
            assert_eq!(
                context.assignments[0].providers[0].model_id,
                "standard-model"
            );
            assert_eq!(context.arbiter.as_ref().unwrap().provider_id, "advanced-a");

            ReviewFlows::plan_finders(&mut context, FlowDepth::Deep, &lenses).unwrap();
            assert_eq!(
                context.assignments[0]
                    .providers
                    .iter()
                    .map(|provider| provider.model_id.as_str())
                    .collect::<Vec<_>>(),
                ["advanced-a-model", "advanced-b-model"]
            );
            assert_eq!(context.arbiter.as_ref().unwrap().provider_id, "advanced-b");
            assert_eq!(
                context.arbiter.as_ref().unwrap().model_id,
                "advanced-b-model"
            );
        });
    }

    #[test]
    fn deep_completion_notice_reports_pending_acceptance_and_provider_truth() {
        let target = BackgroundTaskTarget {
            repo_root: "/repo".into(),
            er_dir: "/repo/.er".into(),
            branch_label: "main".into(),
            base_branch: "main".into(),
            scope: "branch".into(),
            pr_number: None,
            remote_repo: None,
            managed_local: true,
        };
        let mut flow = ReviewFlow::new("hash");
        flow.depth = Some(FlowDepth::Deep);
        flow.arena_run_id = Some("run-1".into());
        flow.successful_arbiter_provider = Some("arbiter".into());
        flow.successful_finder_providers = vec!["finder".into()];
        flow.providers_used = vec!["arbiter".into(), "finder".into()];
        flow.state = FlowState::Done {
            kept: 0,
            dropped: 0,
            awaiting_accept: 3,
        };
        let message = flow_notification_message(&flow, &target, false);
        assert!(message.contains("3 findings await acceptance"));
        assert!(message.contains("Providers: arbiter, finder"));
        assert!(!message.contains("no cross-provider check"));

        flow.providers_used = vec!["arbiter".into()];
        let message = flow_notification_message(&flow, &target, false);
        assert!(message.contains("no cross-provider check ran"));

        flow.state = FlowState::Failed {
            reason: "verification failed".into(),
        };
        let message = flow_notification_message(&flow, &target, false);
        assert!(message.contains("failed: verification failed"));
        assert!(!message.contains("Providers:"));
    }
}
