//! Load-time recovery of a bucket's workflow record, shared by the App and the
//! workspace service. It marks what a dead owner left running; it never
//! enqueues or spawns anything, so loading a workspace cannot start an agent.

use crate::ai::workflow_record::{self, StageStatus, WorkflowOutcome, WorkflowRecord};
use er_store::Scope;

/// Marks a dead owner's running stages `Interrupted` and returns the record,
/// so the caller can offer a re-run when it is resumable. `None` when the
/// bucket has no readable record, or a live owner still holds its lease. The
/// lease probe does not wait.
pub fn reconcile_workflow_record(er_dir: &str) -> Option<WorkflowRecord> {
    let store = crate::store_registry::store();
    let scope = Scope::dir(er_dir).ok()?;
    let mut record = workflow_record::load(&store, &scope).ok()??;
    if record.has_running_stage() {
        let guard = workflow_record::try_lease(&store, &scope, "reconcile").ok()??;
        for stage in &mut record.stages {
            if stage.status == StageStatus::Running {
                stage.status = StageStatus::Interrupted;
            }
        }
        // A cancel writes `cancelled` while its finders are still draining,
        // and a failure wrote `failed` before it stopped. Marking the stage
        // interrupted must not rewrite either into an interrupted run the
        // load then offers to finish.
        record.outcome.get_or_insert(WorkflowOutcome::Interrupted);
        record.holder = None;
        let stored = record.clone();
        let _ = workflow_record::update(&store, &scope, |record| {
            *record = stored;
        });
        drop(guard);
    }
    Some(record)
}
