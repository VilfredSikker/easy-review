//! App-owned orchestration for Quick and Standard review flows.
//!
//! The driver pins one filtered diff, one bucket lease, and exact background
//! task IDs for the life of a flow. UI history is deliberately not used as
//! orchestration state.

mod deep;
mod progress;
pub mod results;

use super::background::BackgroundTaskTarget;
use super::review_flow_admission::ReviewFlowLease;
use super::review_flow_output::{
    archive_flow_output, validate_flow_output, FlowOutputBackup, FlowProducer,
};
use super::{App, DiffMode};
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
use anyhow::{bail, Context, Result};
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

/// Captured execution inputs. This is private App state because a later tab
/// switch must not retarget accepted work.
pub(super) struct ReviewFlowContext {
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
    arena_progress_offset: u64,
    arena_progress_round: u32,
    professor_follow_up_suggested: bool,
    draining: bool,
}

impl App {
    /// Start a flow against the active local view. `scope` must identify that
    /// exact view; branch, staged and unstaged diffs are supported.
    pub fn start_review_flow(&mut self, scope: &str) -> Result<()> {
        let (
            target,
            arena_scope,
            repo_root,
            reviewed_ref_note,
            branch_ref,
            base_branch,
            raw_diff,
            owned_rules,
            review_md_source,
        ) = {
            let tab = self.tab();
            let arena_scope = match (scope, tab.mode) {
                ("branch", DiffMode::Branch | DiffMode::PrDiff | DiffMode::Tour) => {
                    ArenaScope::Branch
                }
                ("unstaged", DiffMode::Unstaged) => ArenaScope::Unstaged,
                ("staged", DiffMode::Staged) => ArenaScope::Staged,
                _ => bail!("Review requires the active branch, staged, or unstaged diff"),
            };
            if tab.repo_root.trim().is_empty() || tab.is_remote() {
                bail!("Review requires a local repository view");
            }
            let branch_ref = tab
                .local_branch_view
                .clone()
                .unwrap_or_else(|| tab.current_branch.clone());
            // A PR or branch view whose head has no worktree runs in the main
            // checkout, like the other review actions, and the prompts name the
            // ref that holds the reviewed code. Staged and unstaged diffs only
            // exist in a checkout of the viewed branch.
            let (execution_root, reviewed_ref_note) = match tab.local_checkout_root() {
                Some(root) => (root.to_owned(), None),
                None if arena_scope == ArenaScope::Branch => (
                    tab.repo_root.clone(),
                    Some(crate::ai::prompts::reviewed_ref_section(
                        tab.pr_head_ref.as_deref().unwrap_or(&branch_ref),
                        tab.pr_number,
                    )),
                ),
                None => bail!(
                    "Review of staged or unstaged changes requires a checkout of the viewed branch"
                ),
            };
            let raw = tab.raw_diff_for_review(scope)?;
            let globs = crate::projects_pins::review_ignore_globs_for_repo(
                &tab.repo_root,
                tab.remote_repo.as_deref(),
            );
            let filtered = crate::git::filter_raw_diff_exclude_globs(&raw, &globs);
            if filtered.trim().is_empty() {
                bail!("Nothing to review after ignored files are removed");
            }
            let target = BackgroundTaskTarget {
                repo_root: tab.repo_root.clone(),
                er_dir: tab.er_dir(),
                branch_label: branch_ref.clone(),
                base_branch: tab.base_branch.clone(),
                scope: scope.to_owned(),
                pr_number: tab.pr_number,
                remote_repo: tab.remote_repo.clone(),
                managed_local: tab.remote_repo.is_none(),
            };
            (
                target,
                arena_scope,
                execution_root,
                reviewed_ref_note,
                branch_ref,
                tab.base_branch.clone(),
                filtered,
                tab.owned_repo_rules(),
                tab.review_md_source(),
            )
        };

        let model_pools = resolve_review_models(&self.config.ai_hub)?;
        let triage_provider = model_pools
            .fast
            .first()
            .cloned()
            .context("no eligible Fast review model is configured")?;
        let mut config = self.config.clone();
        config.ai_hub.default_effort = crate::config::resolve_effort(
            &config.ai_hub,
            &config.agent,
            self.current_ai_effort.as_deref(),
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
        self.recover_arena_bucket(&target.er_dir)?;
        let lease = self.acquire_review_flow_lease(&target.er_dir)?;
        let review_md = crate::ai::review_md::collect(&review_md_source, &raw_diff);
        let diff_hash = match crate::ai::prepared_diff::ensure_review_inputs(
            &target.er_dir,
            &raw_diff,
            owned_rules.as_rules(),
            review_md.as_deref(),
        ) {
            Ok(hash) => hash,
            Err(error) => {
                let _ = self.release_review_flow_lease(&lease);
                bail!("failed to prepare review inputs: {error}");
            }
        };

        let producer = FlowProducer::Triage;
        let archive_key = make_archive_key(lease.sequence(), 1);
        let backup = match archive_flow_output(Path::new(&target.er_dir), &archive_key, &producer) {
            Ok(backup) => backup,
            Err(error) => {
                let _ = self.release_review_flow_lease(&lease);
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
        let id = match self.enqueue_with_selection(
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
                let _ = self.release_review_flow_lease(&lease);
                return Err(error).context("enqueue review triage");
            }
        };
        self.register_review_flow_task(id.clone());
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
            progress::background_status(self, &id).unwrap_or(FlowTaskStatus::Queued);
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
        self.review_flows.insert(key.clone(), flow);
        self.review_flow_contexts.insert(
            key,
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
                arena_progress_offset: 0,
                arena_progress_round: 0,
                professor_follow_up_suggested: false,
                draining: false,
            },
        );
        self.notify(&start_notice);
        self.bump_review_flow_revision();
        Ok(())
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "captured execution checkout is separate from stable target identity"
    )]
    fn enqueue_with_selection(
        &mut self,
        kind: String,
        command_name: &str,
        target: BackgroundTaskTarget,
        execution_root: String,
        prompt: String,
        config: ErConfig,
        selection: AiSelection,
    ) -> Result<String> {
        let previous = self.pending_ai_selection_override.clone();
        self.pending_ai_selection_override = Some(selection);
        let result = self.enqueue_background_agent_task_in_checkout(
            kind,
            command_name,
            target,
            prompt,
            true,
            None,
            Some(config),
            Some(execution_root),
        );
        self.pending_ai_selection_override = previous;
        result
    }

    /// Advance every flow using exact registered task completions and captured
    /// arena run IDs. Called by the background poll hook after queue dispatch.
    pub(super) fn advance_review_flows(&mut self) {
        let keys: Vec<String> = self.review_flow_contexts.keys().cloned().collect();
        for key in keys {
            self.advance_review_flow(&key);
        }
    }

    fn apply_flow_event_notifying(
        &mut self,
        flow: &mut ReviewFlow,
        target: &BackgroundTaskTarget,
        _bucket: &str,
        draining: bool,
        event: FlowEvent,
    ) {
        let previous = flow.state.clone();
        flow.apply(event);
        if flow.state != previous {
            let message = flow_notification_message(flow, target, draining);
            self.notify(&message);
        }
    }

    fn advance_review_flow(&mut self, key: &str) {
        let Some(mut context) = self.review_flow_contexts.remove(key) else {
            return;
        };
        let Some(mut flow) = self.review_flows.remove(key) else {
            self.review_flow_contexts.insert(key.to_owned(), context);
            return;
        };
        let mut changed = progress::refresh_background_tasks(self, &mut flow);

        let task_ids: Vec<String> = context.owned_tasks.keys().cloned().collect();
        for id in task_ids {
            let Some(task) = self.take_review_flow_task_completion(&id) else {
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
                && task.status == super::CommandStatus::Done
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
                    Stage::Triage => self.apply_flow_event_notifying(
                        &mut flow,
                        &context.target,
                        key,
                        context.draining,
                        FlowEvent::TriageFailed { reason },
                    ),
                    Stage::Finder => self.apply_flow_event_notifying(
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
                    Stage::Triage => self.apply_flow_event_notifying(
                        &mut flow,
                        &context.target,
                        key,
                        context.draining,
                        FlowEvent::TriageFailed { reason },
                    ),
                    Stage::Finder => self.apply_flow_event_notifying(
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
                            self.apply_flow_event_notifying(
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
                            self.apply_flow_event_notifying(
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
                    self.apply_flow_event_notifying(
                        &mut flow,
                        &context.target,
                        key,
                        context.draining,
                        FlowEvent::TriageSucceeded { plan },
                    );
                    if flow.state == FlowState::Finding {
                        if let Err(error) = self.enqueue_finders(&mut context, &mut flow) {
                            self.apply_flow_event_notifying(
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
                        self.apply_flow_event_notifying(
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
            match self.start_seeded_verification(&mut context, &mut flow) {
                Ok(()) => changed = true,
                Err(error) => {
                    self.apply_flow_event_notifying(
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
            changed |= progress::refresh_arena_tasks(&mut context, &mut flow, &run_id);
            if flow.state == FlowState::Verifying {
                // The registry status can become Complete before the supervisor
                // saves its final record and releases its registry entry. Only
                // the persisted terminal record after release is authoritative.
                if !self.arena_registry.is_active(&run_id) {
                    let disk_run = load_run(&ArenaPaths::for_run(
                        Path::new(&context.target.er_dir),
                        &run_id,
                    ))
                    .ok();
                    // Progress can advance between the first read and supervisor
                    // release. Consume its final events before closing task rows.
                    progress::refresh_arena_tasks(&mut context, &mut flow, &run_id);
                    match disk_run {
                        Some(run) if run.status == RunStatus::Complete => {
                            progress::finish_arena_tasks(&context, &mut flow, &run);
                            let (kept, dropped, awaiting_accept) =
                                record_verified_run(&mut context, &mut flow, &run);
                            self.apply_flow_event_notifying(
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
                            self.apply_flow_event_notifying(
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
                            self.apply_flow_event_notifying(
                                &mut flow,
                                &context.target,
                                key,
                                context.draining,
                                FlowEvent::Cancel,
                            );
                            changed = true;
                        }
                        Some(_) => {
                            self.apply_flow_event_notifying(
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
                            self.apply_flow_event_notifying(
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
            } else if flow.state == FlowState::Cancelled && !self.arena_registry.is_active(&run_id)
            {
                if let Ok(run) = load_run(&ArenaPaths::for_run(
                    Path::new(&context.target.er_dir),
                    &run_id,
                )) {
                    changed |= progress::refresh_arena_tasks(&mut context, &mut flow, &run_id);
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
        self.review_flows.insert(key.to_owned(), flow);
        if changed {
            self.bump_review_flow_revision();
        }
        let terminal = !self
            .review_flows
            .get(key)
            .is_some_and(ReviewFlow::is_running);
        let has_producers = self.flow_context_has_producers(&context);
        let was_draining = context.draining;
        context.draining = terminal && has_producers;
        if context.draining && !was_draining {
            let notice = self
                .review_flows
                .get(key)
                .map(|flow| flow_notification_message(flow, &context.target, true));
            if let Some(notice) = notice {
                self.notify(&notice);
            }
        }
        if terminal && !has_producers {
            let lease = context.lease.clone();
            match self.release_review_flow_lease(&lease) {
                Ok(()) => {
                    self.bump_review_flow_revision();
                    if was_draining {
                        self.notify(&format!(
                            "Review flow for {} has drained.",
                            context.target.branch_label
                        ));
                    }
                    // Keep terminal flow projection and captured run target for
                    // later generation-checked acceptance.
                }
                Err(error) => {
                    self.notify_long(&format!("Review flow cleanup is still pending: {error}"));
                    context.draining = true;
                    self.review_flow_contexts.insert(key.to_owned(), context);
                }
            }
        } else {
            self.review_flow_contexts.insert(key.to_owned(), context);
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
        &mut self,
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
                )
            } else {
                crate::ai::prompts::build_expert_review_prompt_prepared_diff(
                    &context.scope,
                    &context.target.er_dir,
                    &assignment.lens,
                    &flow.diff_hash,
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
            match self.enqueue_with_selection(
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
                        task.status = progress::background_status(self, &id)
                            .unwrap_or(FlowTaskStatus::Queued);
                    }
                    self.register_review_flow_task(id.clone());
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
        match self.arena_start_seeded_for(
            context.repo_root.clone(),
            context.config.clone(),
            params,
            &context.lease,
        )? {
            Some(run_id) => {
                flow.arena_run_id = Some(run_id.clone());
                context.arena_run_id = Some(run_id.clone());
                context.arena_progress_offset = 0;
                context.arena_progress_round = 0;
                self.remember_review_flow_run_target(&context.target, &run_id);
                self.apply_flow_event_notifying(
                    flow,
                    &context.target,
                    &context.target.er_dir,
                    context.draining,
                    FlowEvent::VerifyStarted,
                );
            }
            None => {
                progress::set_arbiter_skipped(flow);
                self.apply_flow_event_notifying(
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

    fn flow_context_has_producers(&self, context: &ReviewFlowContext) -> bool {
        !context.owned_tasks.is_empty()
            || context
                .arena_run_id
                .as_ref()
                .is_some_and(|id| self.arena_registry.is_active(id))
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
    pub fn cancel_review_flow(&mut self, er_dir: &str) -> bool {
        let Some(flow) = self.review_flows.get(er_dir) else {
            return false;
        };
        if !flow.is_running() {
            return false;
        }
        let (ids, run_id) = self.review_flow_contexts.get(er_dir).map_or_else(
            || (Vec::new(), None),
            |context| {
                (
                    context.owned_tasks.keys().cloned().collect(),
                    context.arena_run_id.clone(),
                )
            },
        );
        let target = self
            .review_flow_contexts
            .get(er_dir)
            .map(|context| context.target.clone());
        if let Some(context) = self.review_flow_contexts.get_mut(er_dir) {
            context.draining = true;
        }
        let notice = if let Some(flow) = self.review_flows.get_mut(er_dir) {
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
            self.notify(&notice);
        }
        for id in ids {
            if self.cancel_queued_background_task(&id) {
                if let Some(flow) = self.review_flows.get_mut(er_dir) {
                    progress::set_background_result(
                        flow,
                        &id,
                        FlowTaskStatus::Cancelled,
                        Some("cancelled before launch".into()),
                    );
                }
            }
        }
        if let Some(flow) = self.review_flows.get_mut(er_dir) {
            progress::cancel_waiting_tasks(flow);
        }
        if let Some(run_id) = run_id {
            let _ = self.arena_cancel(&run_id);
        }
        self.bump_review_flow_revision();
        true
    }

    pub fn review_flow_for(&self, er_dir: &str) -> Option<&ReviewFlow> {
        self.review_flows.get(er_dir)
    }

    /// Revision for host snapshot invalidation. It changes when a flow's
    /// visible projection changes, not on every poll.
    pub fn review_flow_revision(&self) -> u64 {
        self.review_flow_revision
    }

    /// True while terminal flow state still has a process or arena supervisor
    /// draining from the bucket.
    pub fn review_flow_is_draining(&self, er_dir: &str) -> bool {
        self.review_flow_contexts
            .get(er_dir)
            .is_some_and(|context| context.draining)
            && self
                .review_flows
                .get(er_dir)
                .is_some_and(|flow| !flow.is_running())
    }

    pub(super) fn review_flow_run_target(
        &self,
        er_dir: &str,
        run_id: &str,
    ) -> Option<&BackgroundTaskTarget> {
        self.review_flow_run_targets
            .get(&(er_dir.to_owned(), run_id.to_owned()))
    }

    pub(super) fn remember_review_flow_run_target(
        &mut self,
        target: &BackgroundTaskTarget,
        run_id: &str,
    ) {
        self.review_flow_run_targets
            .insert((target.er_dir.clone(), run_id.to_owned()), target.clone());
    }

    /// Professor triage keeps its ordinary Quick review and exposes this
    /// follow-up suggestion to a later snapshot/UI layer.
    pub fn review_flow_professor_follow_up_suggested(&self, er_dir: &str) -> bool {
        self.review_flows
            .get(er_dir)
            .is_some_and(|flow| flow.professor_follow_up_suggested)
    }

    fn bump_review_flow_revision(&mut self) {
        self.review_flow_revision = self.review_flow_revision.wrapping_add(1);
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
    let path = Path::new(er_dir).join("triage.json");
    let contents =
        crate::ai::read_sidecar(&path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&contents).context("parse triage output")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::background::BackgroundTask;
    use crate::config::{
        AiHubConfig, AiModelConfig, AiProviderConfig, ReviewModelGroups, ReviewModelRef,
    };
    use crate::paths::ErRoot;
    use std::path::PathBuf;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    struct TestDir(PathBuf);
    impl TestDir {
        fn new(label: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir()
                .join(format!("er-flow-{label}-{}-{nonce}", std::process::id()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn fake_app(dir: &TestDir, triage_primary: &str, experts: &[&str]) -> App {
        fake_app_with_script(
            dir,
            triage_primary,
            experts,
            "{\"reasons\":[]}",
            "hash=$(cat '__BUCKET__/.diff-tmp.sha256'); printf '{\"version\":1,\"diff_hash\":\"%s\",\"files\":{}}' \"$hash\" > '__BUCKET__/review.json'",
        )
    }

    fn fake_app_with_script(
        dir: &TestDir,
        triage_primary: &str,
        experts: &[&str],
        sensitive: &str,
        review_behavior: &str,
    ) -> App {
        fake_app_with_triage_hash(
            dir,
            triage_primary,
            experts,
            sensitive,
            review_behavior,
            "$hash",
        )
    }

    fn fake_app_with_triage_hash(
        dir: &TestDir,
        triage_primary: &str,
        experts: &[&str],
        sensitive: &str,
        review_behavior: &str,
        triage_hash_expr: &str,
    ) -> App {
        let repo = dir.0.join("repo");
        let bucket = dir.0.join("bucket");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir_all(&bucket).unwrap();
        let expert_json = serde_json::to_string(experts).unwrap();
        let script = "case \"$1\" in\n\
             *triage.json*) hash=$(cat '__BUCKET__/.diff-tmp.sha256'); triage_hash=__TRIAGE_HASH__; printf '{\"version\":1,\"diff_hash\":\"%s\",\"verdict\":{\"primary\":\"__PRIMARY__\",\"experts\":__EXPERTS__},\"sensitive\":__SENSITIVE__}' \"$triage_hash\" > '__BUCKET__/triage.json' ;;\n\
             *experts/security.json*) hash=$(cat '__BUCKET__/.diff-tmp.sha256'); mkdir -p '__BUCKET__/experts'; printf '{\"version\":1,\"expert_id\":\"security\",\"diff_hash\":\"%s\",\"files\":{}}' \"$hash\" > '__BUCKET__/experts/security.json' ;;\n\
             *review.json*) __REVIEW_BEHAVIOR__ ;;\n\
             *) echo \"unexpected review-flow prompt\" >&2; exit 2 ;;\n\
             esac"
            .replace("__BUCKET__", &bucket.to_string_lossy())
            .replace("__PRIMARY__", triage_primary)
            .replace("__EXPERTS__", &expert_json)
            .replace("__SENSITIVE__", sensitive)
            .replace("__TRIAGE_HASH__", triage_hash_expr)
            .replace("__REVIEW_BEHAVIOR__", review_behavior)
            // Review behaviors are inserted after the first path replacement.
            .replace("__BUCKET__", &bucket.to_string_lossy());
        let provider = AiProviderConfig {
            command: "/bin/sh".into(),
            args: vec!["-c".into(), script, "fake-flow".into(), "{prompt}".into()],
            models: vec![AiModelConfig {
                id: "fake-model".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut app = App::new_for_test(Vec::new());
        app.config.ai_hub = AiHubConfig {
            default_provider: Some("fake".into()),
            providers: [("fake".into(), provider)].into(),
            review_models: one_provider_review_models("fake", "fake-model"),
            max_concurrent_reviews: 16,
            max_concurrent_agents: 16,
            ..Default::default()
        };
        let tab = app.tab_mut();
        tab.repo_root = repo.to_string_lossy().into_owned();
        tab.er_root = ErRoot::Managed {
            agent_dir: bucket.to_string_lossy().into_owned(),
            session_dir: bucket.to_string_lossy().into_owned(),
        };
        tab.set_raw_diff_for_test(
            "diff --git a/src/lib.rs b/src/lib.rs\nindex 1111111..2222222 100644\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-old\n+new\n",
        );
        app
    }

    fn one_provider_review_models(provider_id: &str, model_id: &str) -> ReviewModelGroups {
        let model = ReviewModelRef {
            provider_id: provider_id.into(),
            model_id: model_id.into(),
        };
        ReviewModelGroups {
            fast: vec![model.clone()],
            standard: vec![model.clone()],
            advanced: vec![model],
        }
    }

    #[test]
    fn linked_checkout_review_flow_keeps_execution_separate_from_target_identity() {
        let _serial = crate::agent_slots::test_serial();
        for remote in [None, Some("example/linked-checkout")] {
            for deep in [false, true] {
                let dir = TestDir::new("linked-checkout");
                let mut app = fake_app(&dir, if deep { "arena" } else { "expert" }, &["security"]);
                let repo = dir.0.join("repo");
                let checkout = dir.0.join("feature");
                let git = |root: &Path, args: &[&str]| {
                    let output = std::process::Command::new("git")
                        .args(args)
                        .current_dir(root)
                        .output()
                        .unwrap();
                    assert!(
                        output.status.success(),
                        "{}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                };
                git(&repo, &["init", "-b", "main"]);
                git(&repo, &["config", "user.name", "Test"]);
                git(&repo, &["config", "user.email", "test@example.invalid"]);
                std::fs::create_dir_all(repo.join("src")).unwrap();
                std::fs::write(repo.join("src/lib.rs"), "main checkout\n").unwrap();
                git(&repo, &["add", "."]);
                git(
                    &repo,
                    &["-c", "commit.gpgsign=false", "commit", "-qm", "seed"],
                );
                git(
                    &repo,
                    &[
                        "worktree",
                        "add",
                        "-b",
                        "feature",
                        checkout.to_str().unwrap(),
                    ],
                );
                std::fs::write(checkout.join("src/lib.rs"), "feature checkout\n").unwrap();
                let bucket = app.tab().er_dir();
                let log = dir.0.join("execution.log");
                let provider = app.config.ai_hub.providers.get_mut("fake").unwrap();
                // Every actual subprocess records the source it can read. The
                // arena response satisfies all three round schemas.
                let script = format!(
                    r#"
record() {{ printf '%s|%s\n%s\n' "$1" "$(pwd -P)" "$(cat src/lib.rs)" >> '{log}'; }}
case "$1" in
  *triage.json*) record triage; hash=$(cat '{bucket}/.diff-tmp.sha256'); printf '{{"version":1,"diff_hash":"%s","verdict":{{"primary":"{primary}","experts":["security"]}}}}' "$hash" > '{bucket}/triage.json' ;;
  *experts/security.json*) record expert; hash=$(cat '{bucket}/.diff-tmp.sha256'); mkdir -p '{bucket}/experts'; printf '{{"version":1,"expert_id":"security","diff_hash":"%s","files":{{}}}}' "$hash" > '{bucket}/experts/security.json' ;;
  *review.json*) record general; hash=$(cat '{bucket}/.diff-tmp.sha256'); printf '{{"version":1,"diff_hash":"%s","files":{{"src/lib.rs":{{"risk":"high","findings":[{{"id":"general:1","severity":"high","title":"Check the feature source"}}]}}}}}}' "$hash" > '{bucket}/review.json' ;;
  *) record arena; printf '{{"findings":[],"ballots":[],"verdicts":[]}}' ;;
esac
"#,
                    log = log.display(),
                    primary = if deep { "arena" } else { "expert" }
                );
                provider.args[1] = script;
                if deep {
                    let second = provider.clone();
                    app.config
                        .ai_hub
                        .providers
                        .insert("fake-second".into(), second);
                    app.config
                        .ai_hub
                        .review_models
                        .advanced
                        .push(ReviewModelRef {
                            provider_id: "fake-second".into(),
                            model_id: "fake-model".into(),
                        });
                }
                app.config.ai_hub.max_concurrent_reviews = 1;
                let tab = app.tab_mut();
                tab.local_branch_view = Some("feature".into());
                tab.local_branch_checkout_root = Some(checkout.to_string_lossy().into_owned());
                tab.remote_repo = remote.map(str::to_owned);
                let target = BackgroundTaskTarget {
                    repo_root: repo.to_string_lossy().into_owned(),
                    er_dir: "blocker".into(),
                    branch_label: "blocker".into(),
                    base_branch: "main".into(),
                    scope: "branch".into(),
                    pr_number: None,
                    remote_repo: None,
                    managed_local: true,
                };
                let blocker = BackgroundTask::new("review".into(), target);
                let blocker_id = blocker.id.clone();
                let (_result_tx, result_rx) = std::sync::mpsc::channel();
                let (_log_tx, log_rx) = std::sync::mpsc::channel();
                app.background_tasks.insert(
                    blocker_id.clone(),
                    super::super::background::BackgroundTaskHandle {
                        task: blocker,
                        run: crate::agent_run::AgentRunHandle::new(),
                        result_rx,
                        log_rx,
                        recent_log: Default::default(),
                    },
                );
                app.start_review_flow("branch").unwrap();
                let context = &app.review_flow_contexts[&bucket];
                assert_eq!(context.target.repo_root, repo.to_string_lossy());
                assert_eq!(context.target.er_dir, bucket);
                assert_eq!(context.repo_root, checkout.to_string_lossy());
                assert_eq!(app.pending_background_tasks.len(), 1);
                assert_eq!(
                    app.pending_background_tasks[0]
                        .task
                        .execution_root
                        .as_deref(),
                    checkout.to_str()
                );
                assert_eq!(
                    app.pending_background_tasks[0].task.target.repo_root,
                    repo.to_string_lossy()
                );
                app.tabs
                    .push(crate::app::TabState::new_for_test(Vec::new()));
                app.active_tab = 1;
                app.background_tasks.remove(&blocker_id);
                if deep {
                    let deadline = Instant::now() + Duration::from_secs(10);
                    while app.review_flow_for(&bucket).unwrap().state
                        != FlowState::AwaitingConfirmation
                    {
                        app.poll_background_tasks();
                        assert!(Instant::now() < deadline, "Deep did not reach confirmation");
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    app.confirm_review_flow(&bucket).unwrap();
                }
                run_until_terminal(&mut app, &bucket);
                assert!(
                    matches!(
                        app.review_flow_for(&bucket).unwrap().state,
                        FlowState::Done { .. }
                    ),
                    "{:?}",
                    app.review_flow_for(&bucket).unwrap().state
                );
                let observed = std::fs::read_to_string(&log).unwrap();
                let execution = std::fs::canonicalize(&checkout).unwrap();
                for stage in if deep {
                    vec!["triage", "arena"]
                } else {
                    vec!["triage", "general", "expert", "arena"]
                } {
                    assert!(
                        observed.contains(&format!(
                            "{stage}|{}\nfeature checkout\n",
                            execution.display()
                        )),
                        "missing {stage} at feature checkout\n{observed}"
                    );
                }
                assert!(!observed.contains("main checkout"), "{observed}");
            }
        }
    }

    #[test]
    fn pr_view_without_checkout_reviews_from_the_head_ref() {
        let _serial = crate::agent_slots::test_serial();
        for deep in [false, true] {
            let dir = TestDir::new("pr-no-checkout");
            let primary = if deep { "arena" } else { "expert" };
            let mut app = fake_app(&dir, primary, &["security"]);
            let repo = dir.0.join("repo");
            let bucket = app.tab().er_dir();
            let log = dir.0.join("execution.log");
            // Each subprocess records where it ran and whether its prompt names
            // the PR head ref and how to fetch it, since the working tree holds
            // another branch. The general finding gives verification work.
            let script = format!(
                r#"
record() {{ case "$2" in *refs/er/pr/7/head*pull/7/head*|*pull/7/head*refs/er/pr/7/head*) r=ref ;; *) r=noref ;; esac; printf '%s|%s|%s\n' "$1" "$(pwd -P)" "$r" >> '{log}'; }}
case "$1" in
  *triage.json*) record triage "$1"; hash=$(cat '{bucket}/.diff-tmp.sha256'); printf '{{"version":1,"diff_hash":"%s","verdict":{{"primary":"{primary}","experts":["security"]}}}}' "$hash" > '{bucket}/triage.json' ;;
  *experts/security.json*) record expert "$1"; hash=$(cat '{bucket}/.diff-tmp.sha256'); mkdir -p '{bucket}/experts'; printf '{{"version":1,"expert_id":"security","diff_hash":"%s","files":{{}}}}' "$hash" > '{bucket}/experts/security.json' ;;
  *review.json*) record general "$1"; hash=$(cat '{bucket}/.diff-tmp.sha256'); printf '{{"version":1,"diff_hash":"%s","files":{{"src/lib.rs":{{"risk":"high","findings":[{{"id":"general:1","severity":"high","title":"Check the PR source"}}]}}}}}}' "$hash" > '{bucket}/review.json' ;;
  *) record arena "$1"; printf '{{"findings":[],"ballots":[],"verdicts":[]}}' ;;
esac
"#,
                log = log.display(),
            );
            let provider = app.config.ai_hub.providers.get_mut("fake").unwrap();
            provider.args[1] = script;
            if deep {
                let second = provider.clone();
                app.config
                    .ai_hub
                    .providers
                    .insert("fake-second".into(), second);
                app.config
                    .ai_hub
                    .review_models
                    .advanced
                    .push(ReviewModelRef {
                        provider_id: "fake-second".into(),
                        model_id: "fake-model".into(),
                    });
            }
            let tab = app.tab_mut();
            tab.remote_repo = Some("example/pr-no-checkout".into());
            tab.pr_number = Some(7);
            tab.local_branch_view = Some("feature/pr-head".into());
            tab.current_branch = "feature/pr-head".into();
            tab.pr_head_ref = Some("refs/er/pr/7/head".into());
            assert!(tab.local_checkout_root().is_none());

            app.start_review_flow("branch").unwrap();
            assert_eq!(
                app.review_flow_contexts[&bucket].repo_root,
                repo.to_string_lossy()
            );
            if deep {
                let deadline = Instant::now() + Duration::from_secs(10);
                while app.review_flow_for(&bucket).unwrap().state != FlowState::AwaitingConfirmation
                {
                    app.poll_background_tasks();
                    assert!(Instant::now() < deadline, "Deep did not reach confirmation");
                    std::thread::sleep(Duration::from_millis(10));
                }
                app.confirm_review_flow(&bucket).unwrap();
            }
            run_until_terminal(&mut app, &bucket);

            let observed = std::fs::read_to_string(&log).unwrap();
            let execution = std::fs::canonicalize(&repo).unwrap();
            let stages = if deep {
                vec!["triage", "arena"]
            } else {
                vec!["triage", "general", "expert", "arena"]
            };
            for stage in stages {
                assert!(
                    observed.contains(&format!("{stage}|{}|ref\n", execution.display())),
                    "{stage} did not run in the repo with the head ref (deep: {deep})\n{observed}"
                );
            }
            assert!(!observed.contains("|noref"), "deep: {deep}\n{observed}");
        }
    }

    #[test]
    fn view_without_checkout_still_rejects_working_tree_scopes() {
        let dir = TestDir::new("pr-no-checkout-unstaged");
        let mut app = fake_app(&dir, "general", &[]);
        let tab = app.tab_mut();
        tab.mode = DiffMode::Unstaged;
        tab.local_branch_view = Some("feature/pr-head".into());
        let error = app.start_review_flow("unstaged").unwrap_err().to_string();
        assert!(error.contains("checkout"), "{error}");
    }

    #[test]
    fn flow_depths_use_their_model_tiers_and_advanced_arbiter_pool() {
        let _serial = crate::agent_slots::test_serial();
        let dir = TestDir::new("model-tiers");
        let mut app = fake_app(&dir, "general", &[]);
        let provider = app.config.ai_hub.providers["fake"].clone();
        let providers = [
            ("fast", "fast-model", 0.5),
            ("standard", "standard-model", 1.0),
            ("advanced-a", "advanced-a-model", 8.0),
            ("advanced-b", "advanced-b-model", 3.0),
        ];
        app.config.ai_hub.providers = providers
            .iter()
            .map(|(provider_id, model_id, cost)| {
                let mut provider = provider.clone();
                provider.models[0].id = (*model_id).into();
                provider.models[0].cost_per_1k_in = Some(*cost);
                provider.models[0].cost_per_1k_out = Some(0.0);
                ((*provider_id).into(), provider)
            })
            .collect();
        let selection = |provider_id: &str, model_id: &str| ReviewModelRef {
            provider_id: provider_id.into(),
            model_id: model_id.into(),
        };
        app.config.ai_hub.default_provider = Some("fast".into());
        app.config.ai_hub.flow_providers = providers
            .iter()
            .map(|(provider_id, _, _)| (*provider_id).into())
            .collect();
        app.config.ai_hub.review_models = ReviewModelGroups {
            fast: vec![selection("fast", "fast-model")],
            standard: vec![selection("standard", "standard-model")],
            advanced: vec![
                selection("advanced-a", "advanced-a-model"),
                selection("advanced-b", "advanced-b-model"),
            ],
        };

        app.start_review_flow("branch").unwrap();
        let bucket = app.tab().er_dir();
        let captured_task = app.review_flow_for(&bucket).unwrap().tasks[0].clone();
        assert!(matches!(
            captured_task.status,
            FlowTaskStatus::Queued | FlowTaskStatus::Running
        ));
        assert_eq!(captured_task.provider_id, "fast");
        assert_eq!(captured_task.model_id, "fast-model");
        app.config.ai_hub.providers.get_mut("fast").unwrap().models[0].label =
            Some("Edited Fast Label".into());
        app.config.ai_hub.review_models.fast = vec![selection("standard", "standard-model")];
        assert_eq!(
            app.review_flow_for(&bucket).unwrap().tasks[0].model_label,
            captured_task.model_label,
            "task metadata remains tied to the captured model after settings change"
        );
        let context = app.review_flow_contexts.get_mut(&bucket).unwrap();
        assert_eq!(context.model_pools.fast[0].model_id, "fast-model");
        assert_eq!(
            selection_for_provider(context, &context.model_pools.fast[0])
                .model_id
                .as_deref(),
            Some("fast-model"),
            "triage uses the first resolved Fast model"
        );

        let lenses = vec!["general".to_owned()];
        App::plan_finders(context, FlowDepth::Quick, &lenses).unwrap();
        assert_eq!(context.assignments[0].providers[0].model_id, "fast-model");
        assert_eq!(context.arbiter.as_ref().unwrap().provider_id, "advanced-a");
        assert_eq!(
            context.arbiter.as_ref().unwrap().model_id,
            "advanced-a-model"
        );

        App::plan_finders(context, FlowDepth::Standard, &lenses).unwrap();
        assert_eq!(
            context.assignments[0].providers[0].model_id,
            "standard-model"
        );
        assert_eq!(context.arbiter.as_ref().unwrap().provider_id, "advanced-a");

        App::plan_finders(context, FlowDepth::Deep, &lenses).unwrap();
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
    }

    fn run_until_terminal(app: &mut App, bucket: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            app.poll_background_tasks();
            app.advance_review_flows();
            if app
                .review_flows
                .get(bucket)
                .is_some_and(|flow| !flow.is_running())
            {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "review flow did not reach a terminal state"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn fake_cli_runs_triage_and_quick_general_before_empty_seeded_completion() {
        let _serial = crate::agent_slots::test_serial();
        let dir = TestDir::new("quick");
        let mut app = fake_app(&dir, "general", &[]);
        let bucket = app.tab().er_dir();
        let before_config = app.config.clone();
        app.start_review_flow("branch").unwrap();
        assert_eq!(
            app.config.ai_hub.default_effort,
            before_config.ai_hub.default_effort
        );
        run_until_terminal(&mut app, &bucket);
        let flow = app.review_flow_for(&bucket).unwrap();
        assert!(
            matches!(
                flow.state,
                FlowState::Done {
                    kept: 0,
                    dropped: 0,
                    awaiting_accept: 0
                }
            ),
            "state={:?}, failed_finders={:?}",
            flow.state,
            flow.failed_finders
        );
        assert_eq!(flow.lenses, ["general"]);
        assert_eq!(flow.successful_finder_providers, ["fake"]);
        assert_eq!(flow.successful_arbiter_provider, None);
        assert_eq!(flow.providers_used, ["fake"]);
        assert_eq!(flow.tasks[0].id, "triage");
        assert_eq!(flow.tasks[0].status, FlowTaskStatus::Done);
        assert_eq!(flow.tasks[1].id, "finding:general");
        assert_eq!(flow.tasks[1].status, FlowTaskStatus::Done);
        assert_eq!(flow.tasks[2].id, "verifying:arbiter");
        assert_eq!(flow.tasks[2].status, FlowTaskStatus::Skipped);
        assert!(Path::new(&bucket).join("flow-backups").is_dir());
        assert!(!app.review_flow_is_draining(&bucket));
        assert!(app.review_flow_revision() > 1);
    }

    #[test]
    fn professor_route_keeps_follow_up_suggestion_after_completion() {
        let _serial = crate::agent_slots::test_serial();
        let dir = TestDir::new("professor");
        let mut app = fake_app(&dir, "professor", &[]);
        let bucket = app.tab().er_dir();
        app.start_review_flow("branch").unwrap();
        run_until_terminal(&mut app, &bucket);
        let flow = app.review_flow_for(&bucket).unwrap();
        assert!(flow.professor_follow_up_suggested);
        assert_eq!(
            flow.state,
            FlowState::Done {
                kept: 0,
                dropped: 0,
                awaiting_accept: 0,
            },
            "failed_finders={:?}",
            flow.failed_finders
        );
    }

    #[test]
    fn successful_finder_output_replaces_same_hash_archived_output_only_after_validation() {
        let _serial = crate::agent_slots::test_serial();
        let dir = TestDir::new("restore-old-output");
        let no_review = "exit 0";
        let mut app = fake_app_with_script(&dir, "general", &[], "{\"reasons\":[]}", no_review);
        let bucket = app.tab().er_dir();
        let raw_diff = app.tab().raw_diff_for_review("branch").unwrap();
        let rules = app.tab().owned_repo_rules();
        let hash =
            crate::ai::prepared_diff::ensure_review_inputs(&bucket, &raw_diff, rules.as_rules(), None)
                .unwrap();
        let old = format!(
            "{{\"version\":1,\"diff_hash\":\"{hash}\",\"files\":{{\"old.rs\":{{\"risk\":\"low\"}}}}}}"
        );
        std::fs::write(Path::new(&bucket).join("review.json"), &old).unwrap();

        app.start_review_flow("branch").unwrap();
        run_until_terminal(&mut app, &bucket);
        assert!(matches!(
            app.review_flow_for(&bucket).unwrap().state,
            FlowState::Failed { .. }
        ));
        assert_eq!(
            std::fs::read_to_string(Path::new(&bucket).join("review.json")).unwrap(),
            old
        );
    }

    #[test]
    fn stale_triage_hash_fails_closed() {
        let _serial = crate::agent_slots::test_serial();
        let dir = TestDir::new("stale-triage");
        let mut app = fake_app_with_triage_hash(
            &dir,
            "general",
            &[],
            "{\"reasons\":[]}",
            "exit 0",
            "stale-diff-hash",
        );
        let bucket = app.tab().er_dir();
        app.start_review_flow("branch").unwrap();
        run_until_terminal(&mut app, &bucket);
        assert!(matches!(
            app.review_flow_for(&bucket).unwrap().state,
            FlowState::Failed { .. }
        ));
        assert_eq!(
            app.review_flow_for(&bucket).unwrap().tasks[0].status,
            FlowTaskStatus::Failed
        );
        assert!(!Path::new(&bucket).join("review.json").exists());
    }

    #[test]
    fn sensitive_skip_routes_to_security_only_quick_finder() {
        let _serial = crate::agent_slots::test_serial();
        let dir = TestDir::new("security-only");
        let mut app = fake_app_with_script(
            &dir,
            "skip",
            &[],
            "{\"reasons\":[{\"kind\":\"secret\",\"evidence\":\"credential literal\"}]}",
            "hash=$(cat '__BUCKET__/.diff-tmp.sha256'); printf '{\"version\":1,\"diff_hash\":\"%s\",\"files\":{}}' \"$hash\" > '__BUCKET__/review.json'",
        );
        let bucket = app.tab().er_dir();
        app.start_review_flow("branch").unwrap();
        run_until_terminal(&mut app, &bucket);
        let flow = app.review_flow_for(&bucket).unwrap();
        assert_eq!(flow.depth, Some(FlowDepth::Quick));
        assert_eq!(flow.lenses, ["security"]);
        assert_eq!(flow.successful_finder_providers, ["fake"]);
        assert!(
            matches!(flow.state, FlowState::Done { .. }),
            "state={:?}, failed_finders={:?}",
            flow.state,
            flow.failed_finders
        );
    }

    #[test]
    fn quick_general_and_security_both_finish_before_verification() {
        let _serial = crate::agent_slots::test_serial();
        let dir = TestDir::new("quick-security");
        let mut app = fake_app_with_script(
            &dir,
            "general",
            &[],
            "{\"reasons\":[{\"kind\":\"secret\",\"evidence\":\"credential literal\"}]}",
            "sleep 1; hash=$(cat '__BUCKET__/.diff-tmp.sha256'); printf '{\"version\":1,\"diff_hash\":\"%s\",\"files\":{}}' \"$hash\" > '__BUCKET__/review.json'",
        );
        let bucket = app.tab().er_dir();
        app.start_review_flow("branch").unwrap();
        run_until_terminal(&mut app, &bucket);
        let flow = app.review_flow_for(&bucket).unwrap();
        assert_eq!(flow.lenses, ["general", "security"]);
        assert!(flow.pending_finders.is_empty());
        assert_eq!(flow.successful_finder_providers, ["fake"]);
        assert!(
            matches!(flow.state, FlowState::Done { .. }),
            "state={:?}, failed_finders={:?}",
            flow.state,
            flow.failed_finders
        );
        assert!(Path::new(&bucket).join("review.json").exists());
        assert!(Path::new(&bucket).join("experts/security.json").exists());
        assert_eq!(flow.arena_run_id, None);
    }

    #[test]
    fn partial_finder_failure_ignores_unplanned_same_hash_expert_output() {
        let _serial = crate::agent_slots::test_serial();
        let dir = TestDir::new("partial-finder");
        let mut app =
            fake_app_with_script(&dir, "expert", &["security"], "{\"reasons\":[]}", "exit 7");
        let bucket = app.tab().er_dir();
        app.start_review_flow("branch").unwrap();
        let hash = app.review_flow_for(&bucket).unwrap().diff_hash.clone();
        let experts_dir = Path::new(&bucket).join("experts");
        std::fs::create_dir_all(&experts_dir).unwrap();
        std::fs::write(
            experts_dir.join("architecture.json"),
            format!(
                "{{\"version\":1,\"expert_id\":\"architecture\",\"diff_hash\":\"{hash}\",\"files\":{{}}}}"
            ),
        )
        .unwrap();
        run_until_terminal(&mut app, &bucket);
        let flow = app.review_flow_for(&bucket).unwrap();
        assert!(flow
            .failed_finders
            .iter()
            .any(|(lens, _)| lens == "general"));
        assert_eq!(flow.successful_finder_providers, ["fake"]);
        assert_eq!(flow.providers_used, ["fake"]);
        assert!(flow.arena_run_id.is_none());
    }

    #[test]
    fn running_cancellation_keeps_lease_until_writer_drains() {
        let _serial = crate::agent_slots::test_serial();
        let dir = TestDir::new("cancel-running");
        let mut app = fake_app_with_script(
            &dir,
            "general",
            &[],
            "{\"reasons\":[]}",
            "sleep 1; hash=$(cat '__BUCKET__/.diff-tmp.sha256'); printf '{\"version\":1,\"diff_hash\":\"%s\",\"files\":{}}' \"$hash\" > '__BUCKET__/review.json'",
        );
        let bucket = app.tab().er_dir();
        app.start_review_flow("branch").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.review_flow_for(&bucket).unwrap().state != FlowState::Finding {
            app.poll_background_tasks();
            assert!(Instant::now() < deadline, "finder did not start");
            std::thread::sleep(Duration::from_millis(10));
        }
        app.cancel_review_flow(&bucket);
        assert!(app.review_flow_is_draining(&bucket));
        assert!(app.start_review_flow("branch").is_err());

        while app.review_flow_is_draining(&bucket) {
            app.poll_background_tasks();
            assert!(Instant::now() < deadline, "cancelled writer did not drain");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(app.check_review_bucket_admission(&bucket).is_ok());
    }

    #[test]
    fn verifying_waits_for_drained_supervisor_and_final_persisted_run() {
        let _serial = crate::agent_slots::test_serial();
        let dir = TestDir::new("wait-final-save");
        let mut app = fake_app(&dir, "general", &[]);
        let bucket = app.tab().er_dir();
        let tab = app.tab();
        let repo_root = tab.repo_root.clone();
        let branch_ref = tab.current_branch.clone();
        let base_branch = tab.base_branch.clone();
        let target = BackgroundTaskTarget {
            repo_root: repo_root.clone(),
            er_dir: bucket.clone(),
            branch_label: branch_ref.clone(),
            base_branch: base_branch.clone(),
            scope: "branch".into(),
            pr_number: None,
            remote_repo: None,
            managed_local: true,
        };
        let raw_diff = tab.raw_diff_for_review("branch").unwrap();
        let lease = app.acquire_review_flow_lease(&bucket).unwrap();
        let model_pools = resolve_review_models(&app.config.ai_hub).unwrap();
        let flow_config = app.config.clone();
        let run_id = "flow-drain-order".to_owned();
        let diff_hash = "pinned-flow-hash".to_owned();
        let mut flow = ReviewFlow::new(diff_hash.clone());
        flow.apply(FlowEvent::TriageSucceeded {
            plan: Some((FlowDepth::Quick, vec!["general".into()])),
        });
        flow.apply(FlowEvent::FinderSucceeded {
            lens: "general".into(),
        });
        flow.apply(FlowEvent::VerifyStarted);
        flow.arena_run_id = Some(run_id.clone());
        flow.successful_finder_providers = vec!["finder-provider".into()];
        let context = ReviewFlowContext {
            lease,
            target,
            scope: "branch".into(),
            arena_scope: ArenaScope::Branch,
            repo_root,
            reviewed_ref_note: None,
            branch_ref,
            base_branch,
            raw_diff,
            config: flow_config,
            model_pools,
            assignments: Vec::new(),
            arbiter: Some(ReviewerRef {
                provider_id: "arbiter-provider".into(),
                model_id: "arbiter-model".into(),
                agent_kind: None,
            }),
            owned_tasks: HashMap::new(),
            enqueued_lenses: HashSet::new(),
            successful_lenses: HashSet::from(["general".into()]),
            successful_finder_providers: vec!["finder-provider".into()],
            successful_arbiter_provider: None,
            backups: HashMap::new(),
            next_archive: 1,
            arena_run_id: Some(run_id.clone()),
            arena_progress_offset: 0,
            arena_progress_round: 0,
            professor_follow_up_suggested: false,
            draining: false,
        };
        app.review_flows.insert(bucket.clone(), flow);
        app.review_flow_contexts.insert(bucket.clone(), context);

        let paths = ArenaPaths::for_run(Path::new(&bucket), &run_id);
        let mut run: crate::arena::ArenaRun = serde_json::from_value(serde_json::json!({
            "id": run_id,
            "branch_ref": "main",
            "base_branch": "main",
            "scope": "branch",
            "diff_hash": diff_hash,
            "created_at": "now",
            "status": {"running": {"round": 1}},
            "config": {
                "reviewers": [],
                "rounds": 2,
                "arbiter": {"provider_id": "arbiter-provider", "model_id": "arbiter-model"},
                "auto_accept_threshold": 0.75,
                "scope": "branch",
                "run_kind": "seeded"
            },
            "reviewers": [],
            "findings": [],
            "cost_estimate": {"tokens_in": 0, "tokens_out": 0, "usd": 0.0},
            "accepted_finding_ids": []
        }))
        .unwrap();
        crate::arena::save_run(&paths, &run).unwrap();
        app.arena_registry
            .insert_active_for_test(&run_id, RunStatus::Complete);

        app.advance_review_flow(&bucket);
        assert_eq!(
            app.review_flow_for(&bucket).unwrap().state,
            FlowState::Verifying,
            "live Complete plus persisted Running must remain Verifying until drain"
        );
        assert!(app.check_review_bucket_admission(&bucket).is_err());

        run.status = RunStatus::Complete;
        crate::arena::save_run(&paths, &run).unwrap();
        let _drained_handle = app.arena_registry.take(&run_id).unwrap();
        app.advance_review_flow(&bucket);
        assert!(matches!(
            app.review_flow_for(&bucket).unwrap().state,
            FlowState::Done { .. }
        ));
        let flow = app.review_flow_for(&bucket).unwrap();
        assert_eq!(flow.providers_used, ["arbiter-provider", "finder-provider"]);
        assert!(app.check_review_bucket_admission(&bucket).is_ok());
    }

    #[test]
    fn fake_cli_runs_both_standard_producer_outputs_before_seeded_completion() {
        let _serial = crate::agent_slots::test_serial();
        let dir = TestDir::new("standard");
        let mut app = fake_app(&dir, "expert", &["security"]);
        let bucket = app.tab().er_dir();
        app.start_review_flow("branch").unwrap();
        run_until_terminal(&mut app, &bucket);
        let flow = app.review_flow_for(&bucket).unwrap();
        assert!(
            matches!(
                flow.state,
                FlowState::Done {
                    kept: 0,
                    dropped: 0,
                    awaiting_accept: 0
                }
            ),
            "state={:?}, failed_finders={:?}",
            flow.state,
            flow.failed_finders
        );
        assert_eq!(flow.depth, Some(FlowDepth::Standard));
        assert!(Path::new(&bucket).join("review.json").is_file());
        assert!(Path::new(&bucket).join("experts/security.json").is_file());
        assert_eq!(flow.arena_run_id, None);
    }

    #[test]
    fn fake_cli_skip_does_not_launch_finder() {
        let _serial = crate::agent_slots::test_serial();
        let dir = TestDir::new("skip");
        let mut app = fake_app(&dir, "skip", &[]);
        let bucket = app.tab().er_dir();
        app.start_review_flow("branch").unwrap();
        run_until_terminal(&mut app, &bucket);
        assert_eq!(
            app.review_flow_for(&bucket).unwrap().state,
            FlowState::Skipped
        );
        assert!(!Path::new(&bucket).join("review.json").exists());
    }

    #[test]
    fn recovery_incomplete_historical_manifests_allow_public_review_start() {
        for malformed in [false, true] {
            let dir = TestDir::new("incomplete-history");
            let mut app = fake_app(&dir, "general", &[]);
            app.config.ai_hub.max_concurrent_reviews = 1;
            let blocker_target = BackgroundTaskTarget {
                repo_root: app.tab().repo_root.clone(),
                er_dir: dir.0.join("blocker").to_string_lossy().into_owned(),
                branch_label: "blocker".into(),
                base_branch: "main".into(),
                scope: "branch".into(),
                pr_number: None,
                remote_repo: None,
                managed_local: true,
            };
            let blocker = BackgroundTask::new("review".into(), blocker_target);
            let (_result_tx, result_rx) = std::sync::mpsc::channel();
            let (_log_tx, log_rx) = std::sync::mpsc::channel();
            app.background_tasks.insert(
                blocker.id.clone(),
                super::super::background::BackgroundTaskHandle {
                    task: blocker,
                    run: crate::agent_run::AgentRunHandle::new(),
                    result_rx,
                    log_rx,
                    recent_log: Default::default(),
                },
            );
            let bucket = app.tab().er_dir();
            let paths = crate::arena::ArenaPaths::for_run(Path::new(&bucket), "orphan");
            paths.ensure_dirs().unwrap();
            std::fs::write(paths.diff_patch(), "recoverable input").unwrap();
            if malformed {
                std::fs::write(paths.run_json(), "{broken").unwrap();
            }
            app.start_review_flow("branch").unwrap();
            assert_eq!(app.pending_background_tasks.len(), 1);
            assert!(app.check_review_bucket_admission(&bucket).is_err());
            assert_eq!(
                std::fs::read_to_string(paths.diff_patch()).unwrap(),
                "recoverable input"
            );
            assert!(app.cancel_review_flow(&bucket));
        }
    }

    #[test]
    fn provider_admission_fails_before_preparing_or_leasing_bucket() {
        let dir = TestDir::new("no-provider");
        let mut app = fake_app(&dir, "general", &[]);
        app.config.ai_hub.providers.clear();
        let bucket = app.tab().er_dir();
        assert!(app.start_review_flow("branch").is_err());
        assert!(!Path::new(&bucket).join("diff-tmp").exists());
        assert!(app.check_review_bucket_admission(&bucket).is_ok());
    }

    #[test]
    fn queued_cancellation_delivers_exact_completion_and_releases_lease() {
        let dir = TestDir::new("cancel-queued");
        let mut app = fake_app(&dir, "general", &[]);
        app.config.ai_hub.max_concurrent_reviews = 1;
        let blocker_er_dir = dir.0.join("blocker").to_string_lossy().into_owned();
        let blocker_target = BackgroundTaskTarget {
            repo_root: app.tab().repo_root.clone(),
            er_dir: blocker_er_dir,
            branch_label: "blocker".into(),
            base_branch: "main".into(),
            scope: "branch".into(),
            pr_number: None,
            remote_repo: None,
            managed_local: true,
        };
        let blocker = BackgroundTask::new("review".into(), blocker_target);
        let blocker_id = blocker.id.clone();
        let (_result_tx, result_rx) = std::sync::mpsc::channel();
        let (_log_tx, log_rx) = std::sync::mpsc::channel();
        app.background_tasks.insert(
            blocker_id.clone(),
            super::super::background::BackgroundTaskHandle {
                task: blocker,
                run: crate::agent_run::AgentRunHandle::new(),
                result_rx,
                log_rx,
                recent_log: Default::default(),
            },
        );
        let bucket = app.tab().er_dir();
        app.start_review_flow("branch").unwrap();
        let flow_task_id = app.review_flow_contexts[&bucket]
            .owned_tasks
            .keys()
            .next()
            .unwrap()
            .clone();
        assert!(app
            .pending_background_tasks
            .iter()
            .any(|task| task.task.id == flow_task_id));

        assert!(app.cancel_review_flow(&bucket));
        app.poll_background_tasks();
        assert_eq!(
            app.review_flow_for(&bucket).unwrap().state,
            FlowState::Cancelled
        );
        assert_eq!(
            app.review_flow_for(&bucket).unwrap().tasks[0].status,
            FlowTaskStatus::Cancelled
        );
        assert!(app
            .take_review_flow_task_completion(&flow_task_id)
            .is_none());
        assert!(!app.owned_flow_task_ids.contains(&flow_task_id));
        assert!(app.check_review_bucket_admission(&bucket).is_ok());

        app.background_tasks.remove(&blocker_id);
    }

    #[test]
    fn cancelled_queued_finder_stays_cancelled_after_completion_is_drained() {
        let _serial = crate::agent_slots::test_serial();
        let dir = TestDir::new("cancel-queued-finder");
        let mut app = fake_app(&dir, "general", &[]);
        app.config.ai_hub.max_concurrent_reviews = 1;
        app.config.ai_hub.providers.get_mut("fake").unwrap().args[1].insert_str(0, "sleep 0.3; ");

        let blocker_target = BackgroundTaskTarget {
            repo_root: app.tab().repo_root.clone(),
            er_dir: dir.0.join("blocker").to_string_lossy().into_owned(),
            branch_label: "blocker".into(),
            base_branch: "main".into(),
            scope: "branch".into(),
            pr_number: None,
            remote_repo: None,
            managed_local: true,
        };
        let blocker = BackgroundTask::new("review".into(), blocker_target);
        let blocker_id = blocker.id.clone();
        let blocker_handle = || {
            let (result_tx, result_rx) = std::sync::mpsc::channel();
            let (log_tx, log_rx) = std::sync::mpsc::channel();
            (
                super::super::background::BackgroundTaskHandle {
                    task: blocker.clone(),
                    run: crate::agent_run::AgentRunHandle::new(),
                    result_rx,
                    log_rx,
                    recent_log: Default::default(),
                },
                result_tx,
                log_tx,
            )
        };
        let (handle, _initial_result_tx, _initial_log_tx) = blocker_handle();
        app.background_tasks.insert(blocker_id.clone(), handle);
        let bucket = app.tab().er_dir();
        app.start_review_flow("branch").unwrap();
        assert_eq!(
            app.review_flow_for(&bucket).unwrap().tasks[0].status,
            FlowTaskStatus::Queued
        );

        app.background_tasks.remove(&blocker_id);
        app.poll_background_tasks();
        let (handle, _blocker_result_tx, _blocker_log_tx) = blocker_handle();
        app.background_tasks.insert(blocker_id.clone(), handle);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            app.poll_background_tasks();
            if app
                .review_flow_for(&bucket)
                .unwrap()
                .tasks
                .iter()
                .any(|task| task.id == "finding:general" && task.status == FlowTaskStatus::Queued)
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "finder did not queue behind blocker"
            );
            std::thread::sleep(Duration::from_millis(10));
        }

        let finder_id = app.review_flow_contexts[&bucket]
            .owned_tasks
            .iter()
            .find_map(|(id, task)| (task.stage == Stage::Finder).then(|| id.clone()))
            .unwrap();
        assert!(app.cancel_review_flow(&bucket));
        app.poll_background_tasks();
        let flow = app.review_flow_for(&bucket).unwrap();
        assert_eq!(flow.state, FlowState::Cancelled);
        assert_eq!(flow.tasks[0].status, FlowTaskStatus::Done);
        assert_eq!(
            flow.tasks
                .iter()
                .find(|task| task.id == "finding:general")
                .unwrap()
                .status,
            FlowTaskStatus::Cancelled
        );
        assert!(!app.owned_flow_task_ids.contains(&finder_id));
        app.background_tasks.remove(&blocker_id);
    }

    #[test]
    fn notification_is_emitted_on_transition_but_not_duplicate_event() {
        let dir = TestDir::new("flow-notification");
        let mut app = fake_app(&dir, "general", &[]);
        let target = BackgroundTaskTarget {
            repo_root: app.tab().repo_root.clone(),
            er_dir: app.tab().er_dir(),
            branch_label: "main".into(),
            base_branch: "main".into(),
            scope: "branch".into(),
            pr_number: None,
            remote_repo: None,
            managed_local: true,
        };
        let mut flow = ReviewFlow::new("hash");
        let plan = Some((FlowDepth::Quick, vec!["general".into()]));
        app.apply_flow_event_notifying(
            &mut flow,
            &target,
            &target.er_dir,
            false,
            FlowEvent::TriageSucceeded { plan: plan.clone() },
        );
        let notification = app.notification.clone().unwrap();
        assert!(notification.message.contains("Quick finders across 1 lens"));

        app.apply_flow_event_notifying(
            &mut flow,
            &target,
            &target.er_dir,
            false,
            FlowEvent::TriageSucceeded { plan },
        );
        assert_eq!(app.notification.as_ref().unwrap().seq, notification.seq);
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
