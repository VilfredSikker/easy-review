//! Host wiring between the review flow and its durable workflow record.

use super::ReviewFlows;
use crate::ai::review_flow::{FlowDepth, ReviewFlow};
use crate::ai::workflow_record::{
    self, ResumePlan, StageStatus, WorkflowHolder, WorkflowKind, WorkflowOutcome, WorkflowRecord,
};
use er_store::Scope;

pub fn holder_of(guard: &er_store::LeaseGuard) -> WorkflowHolder {
    let holder = guard.holder();
    WorkflowHolder {
        pid: holder.pid,
        label: holder.label.clone(),
    }
}

fn kind_for(flow: &ReviewFlow) -> Option<WorkflowKind> {
    flow.depth.map(|depth| match depth {
        FlowDepth::Quick => WorkflowKind::Quick,
        FlowDepth::Standard => WorkflowKind::Standard,
        FlowDepth::Deep => WorkflowKind::Deep,
    })
}

impl ReviewFlows {
    /// Write the bucket's record before any stage starts, so a crash at any
    /// point leaves a record naming the owner.
    pub(super) fn create_workflow_record(
        er_dir: &str,
        kind: Option<WorkflowKind>,
        diff_hash: &str,
        tasks: &[crate::ai::review_flow::FlowTaskProgress],
        arena_run_id: Option<&str>,
        holder: WorkflowHolder,
    ) -> anyhow::Result<()> {
        let store = crate::store_registry::store();
        let scope = Scope::dir(er_dir)?;
        let record = WorkflowRecord::new(
            kind,
            diff_hash,
            crate::sync::chrono_now(),
            holder,
            tasks,
            arena_run_id,
        );
        workflow_record::save(&store, &scope, &record)
    }

    /// Mirror the flow's plan into the record. Writes only when a row or the
    /// depth changed, so the driver's poll stays cheap.
    pub(super) fn sync_workflow_record(er_dir: &str, flow: &ReviewFlow) {
        let store = crate::store_registry::store();
        let Ok(scope) = Scope::dir(er_dir) else {
            return;
        };
        let Ok(Some(record)) = workflow_record::load(&store, &scope) else {
            return;
        };
        let stages = workflow_record::stages_from_tasks(
            &flow.tasks,
            flow.arena_run_id.as_deref(),
            &flow.diff_hash,
        );
        let kind = kind_for(flow);
        if record.stages == stages && record.kind == kind {
            return;
        }
        let _ = workflow_record::update(&store, &scope, |record| {
            record.kind = kind;
            record.stages = stages;
        });
    }

    /// Close the record. The flow's terminal cleanup has already mapped its
    /// unfinished rows to `Interrupted` or `Failed`.
    pub(super) fn finish_workflow_record(er_dir: &str, outcome: WorkflowOutcome) {
        let store = crate::store_registry::store();
        let Ok(scope) = Scope::dir(er_dir) else {
            return;
        };
        let _ = workflow_record::update(&store, &scope, |record| {
            record.outcome = Some(outcome);
            record.holder = None;
        });
    }
}

/// What a re-run of `record` does against the prepared diff at
/// `current_diff_hash`: the stages it resumes, or `None` when it starts a new
/// flow. Only a Quick or Standard record at the same diff generation resumes;
/// anything else re-confirms Deep and re-runs the panel rather than trusting
/// a partial one.
pub fn rerun_plan(record: &WorkflowRecord, current_diff_hash: &str) -> Option<ResumePlan> {
    let plan = record.plan_resume(current_diff_hash);
    let resumes = matches!(
        record.kind,
        Some(WorkflowKind::Quick | WorkflowKind::Standard)
    );
    (resumes && !plan.restart).then_some(plan)
}

pub fn flow_from_record(record: &WorkflowRecord) -> ReviewFlow {
    let mut flow = ReviewFlow::new(record.diff_hash.clone());
    flow.depth = match record.kind {
        Some(WorkflowKind::Quick) => Some(FlowDepth::Quick),
        Some(WorkflowKind::Standard) => Some(FlowDepth::Standard),
        Some(WorkflowKind::Deep) => Some(FlowDepth::Deep),
        _ => None,
    };
    flow.lenses = record
        .stages
        .iter()
        .filter_map(|stage| stage.id.strip_prefix("finding:").map(str::to_string))
        .collect();
    flow.tasks = record.stages.iter().map(task_from_stage).collect();
    let unfinished = record
        .stages
        .iter()
        .filter(|stage| stage.status != StageStatus::Complete)
        .count();
    flow.state = crate::ai::review_flow::FlowState::Interrupted { unfinished };
    flow
}

pub fn task_from_stage(stage: &crate::ai::StageRecord) -> crate::ai::review_flow::FlowTaskProgress {
    use crate::ai::review_flow::{FlowTaskProgress, FlowTaskStage, FlowTaskStatus};
    use crate::ai::StageKind;
    FlowTaskProgress {
        id: stage.id.clone(),
        stage: match stage.stage {
            StageKind::Triage => FlowTaskStage::Triage,
            StageKind::Finding | StageKind::Panel => FlowTaskStage::Finding,
            StageKind::Verifying | StageKind::Arbiter => FlowTaskStage::Verifying,
        },
        lens: stage.id.strip_prefix("finding:").map(str::to_string),
        label: stage.label.clone(),
        purpose: String::new(),
        provider_id: String::new(),
        provider_label: String::new(),
        model_id: String::new(),
        model_label: String::new(),
        status: match stage.status {
            StageStatus::Pending => FlowTaskStatus::Waiting,
            StageStatus::Running => FlowTaskStatus::Running,
            StageStatus::Complete => FlowTaskStatus::Done,
            StageStatus::Failed => FlowTaskStatus::Failed,
            StageStatus::Interrupted => FlowTaskStatus::Cancelled,
        },
        error: None,
        background_task_id: None,
        round: None,
        total_rounds: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::review_flow::{FlowTaskProgress, FlowTaskStage, FlowTaskStatus, ReviewFlow};

    fn task(id: &str, stage: FlowTaskStage, status: FlowTaskStatus) -> FlowTaskProgress {
        FlowTaskProgress {
            id: id.to_string(),
            stage,
            lens: None,
            label: id.to_string(),
            purpose: String::new(),
            provider_id: "p".to_string(),
            provider_label: "P".to_string(),
            model_id: "m".to_string(),
            model_label: "M".to_string(),
            status,
            error: None,
            background_task_id: None,
            round: None,
            total_rounds: None,
        }
    }

    fn flow_with(tasks: Vec<FlowTaskProgress>) -> ReviewFlow {
        let mut flow = ReviewFlow::new("hash");
        flow.tasks = tasks;
        flow
    }

    #[test]
    fn sync_mirrors_the_plan_and_finish_records_the_outcome() {
        let root = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(root.path(), || {
            let er_dir = root.path().join("bucket").to_string_lossy().into_owned();
            let store = crate::store_registry::store();
            let scope = Scope::dir(&er_dir).unwrap();
            let guard = store
                .lease(&scope, er_store::DEFAULT_LOCK_TIMEOUT, "flow-1")
                .unwrap();
            let holder = holder_of(&guard);
            ReviewFlows::create_workflow_record(&er_dir, None, "hash", &[], None, holder).unwrap();

            let mut flow = flow_with(vec![task(
                "triage",
                FlowTaskStage::Triage,
                FlowTaskStatus::Running,
            )]);
            ReviewFlows::sync_workflow_record(&er_dir, &flow);
            let record = workflow_record::load(&store, &scope).unwrap().unwrap();
            assert_eq!(record.stage("triage").unwrap().status, StageStatus::Running);
            assert_eq!(record.kind, None);

            flow.depth = Some(FlowDepth::Standard);
            ReviewFlows::sync_workflow_record(&er_dir, &flow);
            ReviewFlows::finish_workflow_record(&er_dir, WorkflowOutcome::Complete);
            let record = workflow_record::load(&store, &scope).unwrap().unwrap();
            assert_eq!(record.kind, Some(WorkflowKind::Standard));
            assert_eq!(record.outcome, Some(WorkflowOutcome::Complete));
            assert!(record.holder.is_none());
            drop(guard);
        });
    }

    #[test]
    fn a_cancelled_record_offers_no_rerun() {
        let record = WorkflowRecord::new(
            Some(WorkflowKind::Deep),
            "hash",
            "t0".to_string(),
            WorkflowHolder {
                pid: 1,
                label: "flow-1".to_string(),
            },
            &[task(
                "verifying:arbiter",
                FlowTaskStage::Verifying,
                FlowTaskStatus::Cancelled,
            )],
            None,
        );
        let mut record = record;
        assert_eq!(record.stages[0].status, StageStatus::Interrupted);
        record.outcome = Some(WorkflowOutcome::Cancelled);
        assert!(!record.offers_rerun());
        // An interrupted run without a user cancel is offered again.
        record.outcome = Some(WorkflowOutcome::Interrupted);
        assert!(record.offers_rerun());
        assert!(!record.has_running_stage());
    }
}
