//! Load-time recovery of a bucket's workflow record: a dead owner's running
//! stage is marked interrupted and offered for a re-run, and nothing starts.
//! Moved from the driver's `record.rs`, where they ran on the App.

use er_engine::ai::review_flow::{FlowState, FlowTaskProgress, FlowTaskStage, FlowTaskStatus};
use er_engine::ai::workflow_record;
use er_engine::ai::{StageStatus, WorkflowHolder, WorkflowKind, WorkflowOutcome, WorkflowRecord};
use er_engine::ports::Ports;
use er_engine::review_flow_driver::ReviewFlowHost;
use er_store::Scope;

use super::Harness;
use crate::protocol::Reply;

fn harness() -> Harness {
    Harness::new(Ports::cli())
}

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

/// Triage done and the security finder running when the owner died.
fn running_finder() -> Vec<FlowTaskProgress> {
    vec![
        task("triage", FlowTaskStage::Triage, FlowTaskStatus::Done),
        task(
            "finding:security",
            FlowTaskStage::Finding,
            FlowTaskStatus::Running,
        ),
    ]
}

/// Writes the record the driver writes at start, under the holder named, and
/// closes it with `outcome` when one is given, as the terminal poll does.
fn write_record(
    er_dir: &str,
    kind: Option<WorkflowKind>,
    tasks: &[FlowTaskProgress],
    holder: WorkflowHolder,
    outcome: Option<WorkflowOutcome>,
) {
    let store = er_engine::store_registry::store();
    let scope = Scope::dir(er_dir).unwrap();
    let mut record = WorkflowRecord::new(kind, "hash", "t0".to_string(), holder, tasks, None);
    if outcome.is_some() {
        record.outcome = outcome;
        record.holder = None;
    }
    workflow_record::save(&store, &scope, &record).unwrap();
}

fn stored(er_dir: &str) -> WorkflowRecord {
    let scope = Scope::dir(er_dir).unwrap();
    workflow_record::load(&er_engine::store_registry::store(), &scope)
        .unwrap()
        .unwrap()
}

/// Takes the bucket's lease through the host, writes the record under it,
/// then lets the lease go: the owner is gone.
fn record_of_a_dead_owner(
    harness: &Harness,
    er_dir: &str,
    kind: Option<WorkflowKind>,
    outcome: Option<WorkflowOutcome>,
) {
    let mut flows = harness.flows();
    let host = &mut flows.host;
    let lease = host.acquire_review_flow_lease(er_dir).unwrap();
    let holder = host.workflow_holder(er_dir).unwrap();
    write_record(er_dir, kind, &running_finder(), holder, outcome);
    host.release_review_flow_lease(&lease).unwrap();
}

#[test]
fn load_marks_a_dead_owners_running_stage_interrupted_and_spawns_nothing() {
    let root = tempfile::tempdir().unwrap();
    let er_dir = root.path().join("bucket").to_string_lossy().into_owned();
    er_engine::storage::with_test_storage_root(root.path(), || {
        let harness = harness();
        record_of_a_dead_owner(&harness, &er_dir, None, None);

        let reply = harness.reconcile(std::slice::from_ref(&er_dir));

        assert_eq!(
            reply,
            Reply::Reconciled {
                resumable: vec![er_dir.clone()]
            }
        );
        let record = stored(&er_dir);
        assert_eq!(
            record.stage("finding:security").unwrap().status,
            StageStatus::Interrupted
        );
        assert_eq!(record.outcome, Some(WorkflowOutcome::Interrupted));
        let flows = harness.flows();
        assert!(flows.host.tasks.is_empty(), "a load launches no stage");
        // The record projects into the flow view so the user can re-run it.
        assert!(matches!(
            flows.flows.review_flow_for(&er_dir).map(|flow| &flow.state),
            Some(FlowState::Interrupted { .. })
        ));
    });
}

/// A user cancel whose finders were still draining when the process died
/// is not an invitation to finish the run.
#[test]
fn a_user_cancel_is_not_offered_on_load() {
    let root = tempfile::tempdir().unwrap();
    let er_dir = root.path().join("bucket").to_string_lossy().into_owned();
    er_engine::storage::with_test_storage_root(root.path(), || {
        let harness = harness();
        record_of_a_dead_owner(
            &harness,
            &er_dir,
            Some(WorkflowKind::Standard),
            Some(WorkflowOutcome::Cancelled),
        );

        harness.reconcile(std::slice::from_ref(&er_dir));

        let record = stored(&er_dir);
        assert_eq!(record.outcome, Some(WorkflowOutcome::Cancelled));
        assert_eq!(
            record.stage("finding:security").unwrap().status,
            StageStatus::Interrupted
        );
        let flows = harness.flows();
        assert!(
            flows.flows.review_flow_for(&er_dir).is_none(),
            "a cancelled run must not be offered for a re-run"
        );
        assert!(flows.host.tasks.is_empty());
    });
}

/// A Quick or Standard record whose only unfinished stage is triage has
/// nothing the resume can continue; the load must not offer `r`.
#[test]
fn a_triage_only_record_is_not_offered_on_load() {
    let root = tempfile::tempdir().unwrap();
    let er_dir = root.path().join("bucket").to_string_lossy().into_owned();
    er_engine::storage::with_test_storage_root(root.path(), || {
        let harness = harness();
        let holder = WorkflowHolder {
            pid: 1,
            label: "flow-1".to_string(),
        };
        let tasks = [task(
            "triage",
            FlowTaskStage::Triage,
            FlowTaskStatus::Running,
        )];
        write_record(&er_dir, Some(WorkflowKind::Standard), &tasks, holder, None);

        harness.reconcile(std::slice::from_ref(&er_dir));

        let record = stored(&er_dir);
        assert_eq!(
            record.stage("triage").unwrap().status,
            StageStatus::Interrupted
        );
        assert!(!record.resumable(), "a triage-only plan cannot resume");
        assert!(
            harness.flows().flows.review_flow_for(&er_dir).is_none(),
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
    er_engine::storage::with_test_storage_root(root.path(), || {
        for (outcome, offered) in [
            (WorkflowOutcome::Cancelled, false),
            (WorkflowOutcome::Failed, true),
        ] {
            let er_dir = root
                .path()
                .join(format!("bucket-{outcome:?}"))
                .to_string_lossy()
                .into_owned();
            let harness = harness();
            record_of_a_dead_owner(
                &harness,
                &er_dir,
                Some(WorkflowKind::Standard),
                Some(outcome),
            );

            harness.reconcile(std::slice::from_ref(&er_dir));

            let record = stored(&er_dir);
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
    let er_dir = root.path().join("bucket").to_string_lossy().into_owned();
    er_engine::storage::with_process_test_storage_root(root.path(), || {
        let harness = harness();
        let scope = Scope::dir(&er_dir).unwrap();
        let holder = er_engine::store_registry::store()
            .lease(&scope, std::time::Duration::from_secs(5), "other")
            .unwrap();
        let tasks = [task(
            "finding:security",
            FlowTaskStage::Finding,
            FlowTaskStatus::Running,
        )];
        let owner = WorkflowHolder {
            pid: 1,
            label: "flow-1".to_string(),
        };
        write_record(&er_dir, Some(WorkflowKind::Standard), &tasks, owner, None);

        harness.reconcile(std::slice::from_ref(&er_dir));

        let record = stored(&er_dir);
        assert_eq!(
            record.stage("finding:security").unwrap().status,
            StageStatus::Running
        );
        assert_eq!(record.outcome, None);
        // A live owner's record is not offered for re-run.
        assert!(harness.flows().flows.review_flow_for(&er_dir).is_none());
        drop(holder);
    });
}
