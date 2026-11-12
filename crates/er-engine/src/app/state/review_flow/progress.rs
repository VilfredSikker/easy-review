//! Retained review-flow task metadata and incremental arena progress updates.

use super::{App, ReviewFlowContext};
use crate::ai::flow_providers::FlowProvider;
use crate::ai::review_flow::{
    FlowDepth, FlowTaskProgress, FlowTaskStage, FlowTaskStatus, ReviewFlow,
};
use crate::arena::ArenaRunControl;
use crate::arena::{ArenaRun, ReviewerRunStatus, RunStatus, ARBITER_REVIEWER_ID};
use crate::config::{AiHubConfig, ErConfig};

fn provider_label(hub: &AiHubConfig, provider_id: &str) -> String {
    hub.providers
        .get(provider_id)
        .map(|provider| provider.display_name(provider_id))
        .unwrap_or_else(|| provider_id.to_owned())
}

fn finding_purpose(lens: &str) -> String {
    crate::ai::experts::expert_by_id(lens)
        .map(|expert| expert.description.to_owned())
        .unwrap_or_else(|| "Find correctness, security and reliability defects".to_owned())
}

pub(super) fn task_progress(
    config: &ErConfig,
    id: String,
    stage: FlowTaskStage,
    lens: Option<String>,
    label: String,
    purpose: String,
    model: &FlowProvider,
) -> FlowTaskProgress {
    let provider_label = provider_label(&config.ai_hub, &model.provider_id);
    let model_label = config
        .ai_hub
        .providers
        .get(&model.provider_id)
        .and_then(|provider| {
            provider
                .models
                .iter()
                .find(|item| item.id == model.model_id)
        })
        .and_then(|model| model.label.clone())
        .unwrap_or_else(|| model.model_id.clone());
    FlowTaskProgress {
        id,
        stage,
        lens,
        label,
        purpose,
        provider_id: model.provider_id.clone(),
        provider_label,
        model_id: model.model_id.clone(),
        model_label,
        status: FlowTaskStatus::Waiting,
        error: None,
        background_task_id: None,
        round: None,
        total_rounds: None,
    }
}

pub(super) fn append_planned_tasks(
    context: &ReviewFlowContext,
    flow: &mut ReviewFlow,
    depth: FlowDepth,
) {
    let hub = &context.config.ai_hub;
    if depth == FlowDepth::Deep {
        let rounds = u32::from(crate::arena::effective_arena_rounds(None));
        for assignment in &context.assignments {
            let lens_label = crate::ai::agent_label_for_id(&assignment.lens);
            let agent_kind = if assignment.lens == "general" {
                "general".to_owned()
            } else {
                format!("expert:{}", assignment.lens)
            };
            for model in &assignment.providers {
                let reviewer_id = format!("{agent_kind}::{}-{}", model.provider_id, model.model_id);
                for round in 1..=rounds {
                    let stage = if round == 1 {
                        FlowTaskStage::Finding
                    } else {
                        FlowTaskStage::Verifying
                    };
                    let purpose = if round == 1 {
                        finding_purpose(&assignment.lens)
                    } else {
                        "Cross-check the other reviews against the code".to_owned()
                    };
                    let mut task = task_progress(
                        &context.config,
                        format!("reviewer:{reviewer_id}:round:{round}"),
                        stage,
                        Some(assignment.lens.clone()),
                        format!("{lens_label} · {}", provider_label(hub, &model.provider_id)),
                        purpose,
                        model,
                    );
                    task.round = Some(round);
                    task.total_rounds = Some(rounds);
                    flow.tasks.push(task);
                }
            }
        }
    } else {
        for assignment in &context.assignments {
            let Some(model) = assignment.providers.first() else {
                continue;
            };
            let lens_label = crate::ai::agent_label_for_id(&assignment.lens);
            flow.tasks.push(task_progress(
                &context.config,
                format!("finding:{}", assignment.lens),
                FlowTaskStage::Finding,
                Some(assignment.lens.clone()),
                format!("{lens_label} · {}", provider_label(hub, &model.provider_id)),
                finding_purpose(&assignment.lens),
                model,
            ));
        }
    }

    if let Some(arbiter) = context.arbiter.as_ref() {
        let model = FlowProvider {
            provider_id: arbiter.provider_id.clone(),
            model_id: arbiter.model_id.clone(),
        };
        flow.tasks.push(task_progress(
            &context.config,
            "verifying:arbiter".into(),
            FlowTaskStage::Verifying,
            None,
            format!("Arbiter · {}", provider_label(hub, &model.provider_id)),
            "Check evidence, reject false positives and rank findings".into(),
            &model,
        ));
    }
}

pub(super) fn background_status(app: &App, task_id: &str) -> Option<FlowTaskStatus> {
    if app
        .pending_background_tasks
        .iter()
        .any(|task| task.task.id == task_id)
    {
        return Some(FlowTaskStatus::Queued);
    }
    if app.background_tasks.contains_key(task_id)
        || app
            .recent_background_tasks
            .iter()
            .any(|task| task.id == task_id)
    {
        return Some(FlowTaskStatus::Running);
    }
    None
}

pub(super) fn refresh_background_tasks(app: &App, flow: &mut ReviewFlow) -> bool {
    let mut changed = false;
    for task in &mut flow.tasks {
        let Some(background_id) = task.background_task_id.as_deref() else {
            continue;
        };
        if !matches!(
            task.status,
            FlowTaskStatus::Waiting | FlowTaskStatus::Queued | FlowTaskStatus::Running
        ) {
            continue;
        }
        let Some(status) = background_status(app, background_id) else {
            continue;
        };
        if status != task.status {
            task.status = status;
            changed = true;
        }
    }
    changed
}

pub(super) fn set_background_result(
    flow: &mut ReviewFlow,
    background_id: &str,
    status: FlowTaskStatus,
    error: Option<String>,
) {
    if let Some(task) = flow
        .tasks
        .iter_mut()
        .find(|task| task.background_task_id.as_deref() == Some(background_id))
    {
        task.status = status;
        task.error = error;
    }
}

pub(super) fn cancel_waiting_tasks(flow: &mut ReviewFlow) -> bool {
    let mut changed = false;
    for task in &mut flow.tasks {
        if task.status == FlowTaskStatus::Waiting {
            task.status = FlowTaskStatus::Cancelled;
            changed = true;
        }
    }
    changed
}

pub(super) fn fail_waiting_tasks(flow: &mut ReviewFlow, error: &str) -> bool {
    let mut changed = false;
    for task in &mut flow.tasks {
        if task.status == FlowTaskStatus::Waiting {
            task.status = FlowTaskStatus::Skipped;
            task.error = Some(error.to_owned());
            changed = true;
        }
    }
    changed
}

pub(super) fn finish_unstarted_tasks(
    flow: &mut ReviewFlow,
    status: FlowTaskStatus,
    error: Option<&str>,
) -> bool {
    let mut changed = false;
    for task in &mut flow.tasks {
        if task.status == FlowTaskStatus::Waiting {
            task.status = status;
            task.error = error.map(str::to_owned);
            changed = true;
        }
    }
    changed
}

pub(super) fn set_arbiter_skipped(flow: &mut ReviewFlow) -> bool {
    if let Some(task) = flow
        .tasks
        .iter_mut()
        .find(|task| task.id == "verifying:arbiter")
    {
        if task.status == FlowTaskStatus::Waiting {
            task.status = FlowTaskStatus::Skipped;
            return true;
        }
    }
    false
}

/// Apply the run's live progress to the flow's task rows.
///
/// The control's state is a snapshot, so this re-applies it every poll; every
/// step is idempotent, and a round start is the only transition that clears
/// the previous round's sets.
pub(super) fn refresh_arena_tasks(
    context: &mut ReviewFlowContext,
    flow: &mut ReviewFlow,
    control: &ArenaRunControl,
) -> bool {
    let progress = control.progress();
    let mut changed = false;
    if progress.round != 0 && u32::from(progress.round) != context.arena_progress_round {
        context.arena_progress_round = u32::from(progress.round);
        changed |= queue_round(
            flow,
            u32::from(progress.round),
            u32::from(progress.total_rounds),
        );
    }
    if progress.phase == "arbiter" {
        changed |= mark_arbiter_queued(flow);
    }
    for reviewer_id in &progress.thinking {
        if reviewer_id == ARBITER_REVIEWER_ID {
            changed |= mark_arbiter_running(flow);
        } else {
            let id = reviewer_task_id(reviewer_id, context.arena_progress_round);
            changed |= set_task_status(flow, &id, FlowTaskStatus::Running, None);
        }
    }
    for (round, reviewers) in &progress.done_by_round {
        for reviewer_id in reviewers {
            if reviewer_id == ARBITER_REVIEWER_ID {
                changed |= set_task_status(flow, "verifying:arbiter", FlowTaskStatus::Done, None);
            } else {
                let id = reviewer_task_id(reviewer_id, u32::from(*round));
                changed |= set_task_status(flow, &id, FlowTaskStatus::Done, None);
            }
        }
    }
    for (round, failures) in &progress.failed_by_round {
        for (reviewer_id, reason) in failures {
            if reviewer_id == ARBITER_REVIEWER_ID {
                changed |= set_task_status(
                    flow,
                    "verifying:arbiter",
                    FlowTaskStatus::Failed,
                    Some(reason.clone()),
                );
            } else {
                let id = reviewer_task_id(reviewer_id, u32::from(*round));
                changed |= set_task_status(flow, &id, FlowTaskStatus::Failed, Some(reason.clone()));
            }
        }
    }
    changed
}

fn queue_round(flow: &mut ReviewFlow, round: u32, total_rounds: u32) -> bool {
    let previously_failed = flow
        .tasks
        .iter()
        .filter(|task| {
            task.status == FlowTaskStatus::Failed
                && task.round.is_some_and(|failed_round| failed_round < round)
        })
        .filter_map(reviewer_plan_key)
        .collect::<std::collections::HashSet<_>>();
    let mut changed = false;
    for task in flow
        .tasks
        .iter_mut()
        .filter(|task| task.round == Some(round))
    {
        task.total_rounds = Some(total_rounds);
        if reviewer_plan_key(task).is_some_and(|key| previously_failed.contains(&key)) {
            task.status = FlowTaskStatus::Skipped;
            task.error = None;
            changed = true;
        } else if task.status == FlowTaskStatus::Waiting {
            task.status = FlowTaskStatus::Queued;
            changed = true;
        }
    }
    changed
}

pub(super) fn finish_arena_tasks(
    context: &ReviewFlowContext,
    flow: &mut ReviewFlow,
    run: &ArenaRun,
) -> bool {
    let mut changed = false;
    let cancelled = matches!(run.status, RunStatus::Cancelled);
    let complete = matches!(run.status, RunStatus::Complete);
    let failed = matches!(run.status, RunStatus::Failed);
    let mut prior_failures = std::collections::HashMap::<String, u32>::new();
    for task in flow
        .tasks
        .iter()
        .filter(|task| task.status == FlowTaskStatus::Failed)
    {
        if let (Some(key), Some(round)) = (reviewer_plan_key(task), task.round) {
            prior_failures
                .entry(key)
                .and_modify(|existing| *existing = (*existing).min(round))
                .or_insert(round);
        }
    }
    for task in &mut flow.tasks {
        if !matches!(
            task.status,
            FlowTaskStatus::Waiting | FlowTaskStatus::Queued | FlowTaskStatus::Running
        ) {
            continue;
        }
        let next = if task.id == "verifying:arbiter" {
            if cancelled {
                FlowTaskStatus::Cancelled
            } else if failed && task.status != FlowTaskStatus::Waiting {
                FlowTaskStatus::Failed
            } else if complete && task.status == FlowTaskStatus::Running {
                FlowTaskStatus::Done
            } else {
                FlowTaskStatus::Skipped
            }
        } else if let Some(round) = task.round {
            if cancelled {
                FlowTaskStatus::Cancelled
            } else if round > context.arena_progress_round
                || reviewer_plan_key(task)
                    .and_then(|key| prior_failures.get(&key).copied())
                    .is_some_and(|failed_round| failed_round < round)
            {
                FlowTaskStatus::Skipped
            } else if let Some(reason) =
                reviewer_failure(run, task).filter(|_| round == context.arena_progress_round)
            {
                task.error = Some(reason);
                FlowTaskStatus::Failed
            } else if failed {
                FlowTaskStatus::Failed
            } else {
                FlowTaskStatus::Skipped
            }
        } else {
            continue;
        };
        if task.status != next {
            task.status = next;
            changed = true;
        }
    }
    changed
}

fn reviewer_failure(run: &ArenaRun, task: &FlowTaskProgress) -> Option<String> {
    let id = reviewer_plan_key(task)?;
    run.reviewers
        .iter()
        .find(|reviewer| reviewer.id == id)
        .and_then(|reviewer| match &reviewer.status {
            ReviewerRunStatus::Failed { reason } => Some(reason.clone()),
            ReviewerRunStatus::Ok => None,
        })
}

fn reviewer_plan_key(task: &FlowTaskProgress) -> Option<String> {
    let lens = task.lens.as_deref()?;
    let agent_kind = if lens == "general" {
        "general".to_owned()
    } else {
        format!("expert:{lens}")
    };
    Some(format!(
        "{agent_kind}::{}-{}",
        task.provider_id, task.model_id
    ))
}

fn reviewer_task_id(reviewer_id: &str, round: u32) -> String {
    format!("reviewer:{reviewer_id}:round:{round}")
}

fn mark_arbiter_queued(flow: &mut ReviewFlow) -> bool {
    set_task_status(flow, "verifying:arbiter", FlowTaskStatus::Queued, None)
}

fn mark_arbiter_running(flow: &mut ReviewFlow) -> bool {
    set_task_status(flow, "verifying:arbiter", FlowTaskStatus::Running, None)
}

pub(super) fn set_task_status(
    flow: &mut ReviewFlow,
    id: &str,
    status: FlowTaskStatus,
    error: Option<String>,
) -> bool {
    let Some(task) = flow.tasks.iter_mut().find(|task| task.id == id) else {
        return false;
    };
    let changed = task.status != status || task.error != error;
    task.status = status;
    task.error = error;
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::flow_providers::ResolvedReviewModels;
    use crate::app::state::background::BackgroundTaskTarget;
    use crate::arena::{ArenaScope, ProgressEvent};
    use std::collections::{HashMap, HashSet};

    #[test]
    fn arbiter_stays_queued_until_the_thinking_event() {
        let mut flow = ReviewFlow::new("hash");
        flow.tasks.push(FlowTaskProgress {
            id: "verifying:arbiter".into(),
            stage: FlowTaskStage::Verifying,
            lens: None,
            label: "Arbiter".into(),
            purpose: "Verify findings".into(),
            provider_id: "advanced".into(),
            provider_label: "Advanced".into(),
            model_id: "advanced-model".into(),
            model_label: "Advanced model".into(),
            status: FlowTaskStatus::Waiting,
            error: None,
            background_task_id: None,
            round: None,
            total_rounds: None,
        });

        mark_arbiter_queued(&mut flow);
        assert_eq!(flow.tasks[0].status, FlowTaskStatus::Queued);
        mark_arbiter_running(&mut flow);
        assert_eq!(flow.tasks[0].status, FlowTaskStatus::Running);
    }

    #[test]
    fn reviewer_rows_follow_the_live_control_state() {
        let tmp = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(tmp.path(), || {
            let bucket = tmp.path().to_string_lossy().into_owned();
            let mut app = App::new_for_test(Vec::new());
            let lease = app.acquire_review_flow_lease(&bucket).unwrap();
            let mut context = ReviewFlowContext {
                lease,
                target: BackgroundTaskTarget {
                    repo_root: "/repo".into(),
                    er_dir: bucket.clone(),
                    branch_label: "main".into(),
                    base_branch: "main".into(),
                    scope: "branch".into(),
                    pr_number: None,
                    remote_repo: None,
                    managed_local: true,
                },
                scope: "branch".into(),
                arena_scope: ArenaScope::Branch,
                repo_root: "/repo".into(),
                reviewed_ref_note: None,
                branch_ref: "main".into(),
                base_branch: "main".into(),
                raw_diff: String::new(),
                config: app.config.clone(),
                model_pools: ResolvedReviewModels {
                    fast: Vec::new(),
                    standard: Vec::new(),
                    advanced: Vec::new(),
                },
                assignments: Vec::new(),
                arbiter: None,
                owned_tasks: HashMap::new(),
                enqueued_lenses: HashSet::new(),
                successful_lenses: HashSet::new(),
                successful_finder_providers: Vec::new(),
                successful_arbiter_provider: None,
                backups: HashMap::new(),
                next_archive: 1,
                arena_run_id: Some("run".into()),
                arena_progress_round: 0,
                professor_follow_up_suggested: false,
                draining: false,
            };
            let controls = crate::arena::ArenaRunControls::default();
            let control = controls.new_control("run", &bucket);
            let reviewer_id = "general::provider-model";
            let mut flow = ReviewFlow::new("hash");
            for round in [1, 2] {
                flow.tasks.push(FlowTaskProgress {
                    id: reviewer_task_id(reviewer_id, round),
                    stage: if round == 1 {
                        FlowTaskStage::Finding
                    } else {
                        FlowTaskStage::Verifying
                    },
                    lens: Some("general".into()),
                    label: "General".into(),
                    purpose: "Review".into(),
                    provider_id: "provider".into(),
                    provider_label: "Provider".into(),
                    model_id: "model".into(),
                    model_label: "Model".into(),
                    status: FlowTaskStatus::Waiting,
                    error: None,
                    background_task_id: None,
                    round: Some(round),
                    total_rounds: Some(2),
                });
            }

            control.emit(&ProgressEvent::RoundStarted {
                round: 1,
                total_rounds: 2,
            });
            assert!(refresh_arena_tasks(&mut context, &mut flow, &control));
            assert_eq!(flow.tasks[0].status, FlowTaskStatus::Queued);
            assert_eq!(flow.tasks[1].status, FlowTaskStatus::Waiting);

            control.emit(&ProgressEvent::ReviewerThinking {
                reviewer_id: reviewer_id.into(),
                round: 1,
            });
            assert!(refresh_arena_tasks(&mut context, &mut flow, &control));
            assert_eq!(flow.tasks[0].status, FlowTaskStatus::Running);

            control.emit(&ProgressEvent::ReviewerDone {
                reviewer_id: reviewer_id.into(),
                round: 1,
                findings_count: 1,
            });
            assert!(refresh_arena_tasks(&mut context, &mut flow, &control));
            assert_eq!(flow.tasks[0].status, FlowTaskStatus::Done);

            control.emit(&ProgressEvent::RoundStarted {
                round: 2,
                total_rounds: 2,
            });
            assert!(refresh_arena_tasks(&mut context, &mut flow, &control));
            assert_eq!(flow.tasks[1].status, FlowTaskStatus::Queued);
            assert_eq!(
                flow.tasks[0].status,
                FlowTaskStatus::Done,
                "a finished round keeps its row state"
            );
        });
    }

    #[test]
    fn failed_reviewer_is_not_requeued_or_failed_in_later_rounds() {
        let tmp = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(tmp.path(), || {
            let bucket = tmp.path().to_string_lossy().into_owned();
            let mut app = App::new_for_test(Vec::new());
            let lease = app.acquire_review_flow_lease(&bucket).unwrap();
            let context = ReviewFlowContext {
                lease,
                target: BackgroundTaskTarget {
                    repo_root: "/repo".into(),
                    er_dir: bucket,
                    branch_label: "main".into(),
                    base_branch: "main".into(),
                    scope: "branch".into(),
                    pr_number: None,
                    remote_repo: None,
                    managed_local: true,
                },
                scope: "branch".into(),
                arena_scope: ArenaScope::Branch,
                repo_root: "/repo".into(),
                reviewed_ref_note: None,
                branch_ref: "main".into(),
                base_branch: "main".into(),
                raw_diff: String::new(),
                config: app.config.clone(),
                model_pools: ResolvedReviewModels {
                    fast: Vec::new(),
                    standard: Vec::new(),
                    advanced: Vec::new(),
                },
                assignments: Vec::new(),
                arbiter: None,
                owned_tasks: HashMap::new(),
                enqueued_lenses: HashSet::new(),
                successful_lenses: HashSet::new(),
                successful_finder_providers: Vec::new(),
                successful_arbiter_provider: None,
                backups: HashMap::new(),
                next_archive: 1,
                arena_run_id: Some("run".into()),
                arena_progress_round: 1,
                professor_follow_up_suggested: false,
                draining: false,
            };
            let reviewer_id = "general::provider-model";
            let mut flow = ReviewFlow::new("hash");
            for round in [1, 2] {
                flow.tasks.push(FlowTaskProgress {
                    id: reviewer_task_id(reviewer_id, round),
                    stage: if round == 1 {
                        FlowTaskStage::Finding
                    } else {
                        FlowTaskStage::Verifying
                    },
                    lens: Some("general".into()),
                    label: "General".into(),
                    purpose: "Review".into(),
                    provider_id: "provider".into(),
                    provider_label: "Provider".into(),
                    model_id: "model".into(),
                    model_label: "Model".into(),
                    status: FlowTaskStatus::Waiting,
                    error: None,
                    background_task_id: None,
                    round: Some(round),
                    total_rounds: Some(3),
                });
            }
            flow.tasks[0].status = FlowTaskStatus::Failed;
            flow.tasks[0].error = Some("provider returned invalid output".into());

            assert!(queue_round(&mut flow, 2, 3));
            assert_eq!(flow.tasks[1].status, FlowTaskStatus::Skipped);
            assert_eq!(flow.tasks[1].error, None);

            let run: ArenaRun = serde_json::from_value(serde_json::json!({
                "id": "run",
                "branch_ref": "main",
                "base_branch": "main",
                "scope": "branch",
                "diff_hash": "hash",
                "created_at": "now",
                "status": "complete",
                "config": {
                    "reviewers": [],
                    "rounds": 3,
                    "arbiter": {"provider_id": "provider", "model_id": "model"},
                    "auto_accept_threshold": 0.75,
                    "scope": "branch",
                    "run_kind": "models"
                },
                "reviewers": [],
                "findings": [],
                "cost_estimate": {"tokens_in": 0, "tokens_out": 0, "usd": 0.0},
                "accepted_finding_ids": []
            }))
            .unwrap();
            assert!(!finish_arena_tasks(&context, &mut flow, &run));
            assert_eq!(flow.tasks[1].status, FlowTaskStatus::Skipped);
        });
    }
}
