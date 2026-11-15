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
#[cfg(feature = "ui")]
mod tests {
    use super::*;
    use crate::ai::review_flow::{FlowTaskProgress, FlowTaskStage, FlowTaskStatus, ReviewFlow};
    use crate::app::App;

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

    /// The test tab's default bucket is a fixed `/tmp` path, which the
    /// sandboxed test run cannot write; point it at the test root instead.
    fn test_app(bucket: &std::path::Path) -> App {
        let mut app = App::new_for_test(Vec::new());
        app.tab_mut().er_root =
            crate::paths::ErRoot::managed(bucket.to_string_lossy().into_owned());
        app
    }

    #[test]
    fn sync_mirrors_the_plan_and_finish_records_the_outcome() {
        let root = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(root.path(), || {
            let mut app = test_app(&root.path().join("bucket"));
            let er_dir = app.tab().er_dir();
            let lease = app.acquire_review_flow_lease(&er_dir).unwrap();
            let holder = app.workflow_leases[&er_dir].holder();
            let holder = WorkflowHolder {
                pid: holder.pid,
                label: holder.label.clone(),
            };
            ReviewFlows::create_workflow_record(&er_dir, None, "hash", &[], None, holder).unwrap();

            let mut flow = flow_with(vec![task(
                "triage",
                FlowTaskStage::Triage,
                FlowTaskStatus::Running,
            )]);
            ReviewFlows::sync_workflow_record(&er_dir, &flow);
            let store = crate::store_registry::store();
            let scope = Scope::dir(&er_dir).unwrap();
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
            app.release_review_flow_lease(&lease).unwrap();
        });
    }

    #[test]
    fn load_marks_a_dead_owners_running_stage_interrupted_and_spawns_nothing() {
        let root = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(root.path(), || {
            let mut app = test_app(&root.path().join("bucket"));
            let er_dir = app.tab().er_dir();
            let lease = app.acquire_review_flow_lease(&er_dir).unwrap();
            let holder = app.workflow_leases[&er_dir].holder();
            let holder = WorkflowHolder {
                pid: holder.pid,
                label: holder.label.clone(),
            };
            let flow = flow_with(vec![
                task("triage", FlowTaskStage::Triage, FlowTaskStatus::Done),
                task(
                    "finding:security",
                    FlowTaskStage::Finding,
                    FlowTaskStatus::Running,
                ),
            ]);
            ReviewFlows::create_workflow_record(&er_dir, None, "hash", &flow.tasks, None, holder)
                .unwrap();
            // No live lease: the owner is gone.
            app.release_review_flow_lease(&lease).unwrap();

            app.reconcile_workflow_records();

            let store = crate::store_registry::store();
            let scope = Scope::dir(&er_dir).unwrap();
            let record = workflow_record::load(&store, &scope).unwrap().unwrap();
            assert_eq!(
                record.stage("finding:security").unwrap().status,
                StageStatus::Interrupted
            );
            assert_eq!(record.outcome, Some(WorkflowOutcome::Interrupted));
            assert!(app.pending_background_tasks.is_empty());
            assert!(app.background_tasks.is_empty());
            // The record projects into the flow view so the user can re-run it.
            assert!(matches!(
                app.review_flow_for(&er_dir).map(|flow| &flow.state),
                Some(crate::ai::review_flow::FlowState::Interrupted { .. })
            ));
        });
    }

    /// A user cancel whose finders were still draining when the process died
    /// is not an invitation to finish the run.
    #[test]
    fn a_user_cancel_is_not_offered_on_load() {
        let root = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(root.path(), || {
            let mut app = test_app(&root.path().join("bucket"));
            let er_dir = app.tab().er_dir();
            let lease = app.acquire_review_flow_lease(&er_dir).unwrap();
            let holder = app.workflow_leases[&er_dir].holder();
            let holder = WorkflowHolder {
                pid: holder.pid,
                label: holder.label.clone(),
            };
            let flow = flow_with(vec![
                task("triage", FlowTaskStage::Triage, FlowTaskStatus::Done),
                task(
                    "finding:security",
                    FlowTaskStage::Finding,
                    FlowTaskStatus::Running,
                ),
            ]);
            ReviewFlows::create_workflow_record(
                &er_dir,
                Some(WorkflowKind::Standard),
                "hash",
                &flow.tasks,
                None,
                holder,
            )
            .unwrap();
            // The terminal poll records the cancel while the finder drains;
            // the process then dies before that finder finishes.
            ReviewFlows::finish_workflow_record(&er_dir, WorkflowOutcome::Cancelled);
            app.release_review_flow_lease(&lease).unwrap();

            app.reconcile_workflow_records();

            let scope = Scope::dir(&er_dir).unwrap();
            let record = workflow_record::load(&crate::store_registry::store(), &scope)
                .unwrap()
                .unwrap();
            assert_eq!(record.outcome, Some(WorkflowOutcome::Cancelled));
            assert_eq!(
                record.stage("finding:security").unwrap().status,
                StageStatus::Interrupted
            );
            assert!(
                app.review_flow_for(&er_dir).is_none(),
                "a cancelled run must not be offered for a re-run"
            );
            assert!(app.pending_background_tasks.is_empty());
        });
    }

    /// A Quick or Standard record whose only unfinished stage is triage has
    /// nothing the resume can continue; the load must not offer `r`.
    #[test]
    fn a_triage_only_record_is_not_offered_on_load() {
        let root = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(root.path(), || {
            let mut app = test_app(&root.path().join("bucket"));
            let er_dir = app.tab().er_dir();
            let scope = Scope::dir(&er_dir).unwrap();
            let tasks = [task(
                "triage",
                FlowTaskStage::Triage,
                FlowTaskStatus::Running,
            )];
            let record = WorkflowRecord::new(
                Some(WorkflowKind::Standard),
                "hash",
                "t0".to_string(),
                WorkflowHolder {
                    pid: 1,
                    label: "flow-1".to_string(),
                },
                &tasks,
                None,
            );
            workflow_record::save(&crate::store_registry::store(), &scope, &record).unwrap();

            app.reconcile_workflow_records();

            let stored = workflow_record::load(&crate::store_registry::store(), &scope)
                .unwrap()
                .unwrap();
            assert_eq!(
                stored.stage("triage").unwrap().status,
                StageStatus::Interrupted
            );
            assert!(!stored.resumable(), "a triage-only plan cannot resume");
            assert!(
                app.review_flow_for(&er_dir).is_none(),
                "an unresumable record must not be offered"
            );
        });
    }

    /// A user cancel writes `cancelled` while its finders are still draining,
    /// and a failed run wrote `failed` before it stopped. Either outcome is
    /// the truth about what happened; reconcile may interrupt the stage but
    /// must not turn the run into an interrupted one the load offers to finish.
    #[test]
    fn reconcile_preserves_a_user_cancel_and_a_failure() {
        let root = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(root.path(), || {
            for (outcome, offered) in [
                (WorkflowOutcome::Cancelled, false),
                (WorkflowOutcome::Failed, true),
            ] {
                let bucket = root.path().join(format!("bucket-{outcome:?}"));
                let mut app = test_app(&bucket);
                let er_dir = app.tab().er_dir();
                let lease = app.acquire_review_flow_lease(&er_dir).unwrap();
                let holder = app.workflow_leases[&er_dir].holder();
                let holder = WorkflowHolder {
                    pid: holder.pid,
                    label: holder.label.clone(),
                };
                let flow = flow_with(vec![
                    task("triage", FlowTaskStage::Triage, FlowTaskStatus::Done),
                    task(
                        "finding:security",
                        FlowTaskStage::Finding,
                        FlowTaskStatus::Running,
                    ),
                ]);
                ReviewFlows::create_workflow_record(
                    &er_dir,
                    Some(WorkflowKind::Standard),
                    "hash",
                    &flow.tasks,
                    None,
                    holder,
                )
                .unwrap();
                ReviewFlows::finish_workflow_record(&er_dir, outcome);
                // The owner is gone: the finder is the stage that drained past
                // the terminal write.
                app.release_review_flow_lease(&lease).unwrap();

                app.reconcile_workflow_records();

                let scope = Scope::dir(&er_dir).unwrap();
                let record = workflow_record::load(&crate::store_registry::store(), &scope)
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    record.stage("finding:security").unwrap().status,
                    StageStatus::Interrupted,
                    "{outcome:?}"
                );
                assert_eq!(record.outcome, Some(outcome), "the outcome is kept");
                assert_eq!(
                    record.offers_rerun(),
                    offered,
                    "{outcome:?}: only a run worth finishing is offered"
                );
            }
        });
    }

    #[test]
    fn load_leaves_a_record_whose_lease_is_held_alone() {
        let root = tempfile::tempdir().unwrap();
        crate::storage::with_process_test_storage_root(root.path(), || {
            let mut app = test_app(&root.path().join("bucket"));
            let er_dir = app.tab().er_dir();
            let scope = Scope::dir(&er_dir).unwrap();
            let holder = crate::store_registry::store()
                .lease(&scope, std::time::Duration::from_secs(5), "other")
                .unwrap();
            let record = WorkflowRecord::new(
                Some(WorkflowKind::Standard),
                "hash",
                "t0".to_string(),
                WorkflowHolder {
                    pid: 1,
                    label: "flow-1".to_string(),
                },
                &[task(
                    "finding:security",
                    FlowTaskStage::Finding,
                    FlowTaskStatus::Running,
                )],
                None,
            );
            workflow_record::save(&crate::store_registry::store(), &scope, &record).unwrap();

            app.reconcile_workflow_records();

            let stored = workflow_record::load(&crate::store_registry::store(), &scope)
                .unwrap()
                .unwrap();
            assert_eq!(
                stored.stage("finding:security").unwrap().status,
                StageStatus::Running
            );
            assert_eq!(stored.outcome, None);
            // A live owner's record is not offered for re-run.
            assert!(app.review_flow_for(&er_dir).is_none());
            drop(holder);
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
