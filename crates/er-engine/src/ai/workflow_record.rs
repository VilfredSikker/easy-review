//! `workflow.json`: the durable record of one bucket's review flow or run.
//!
//! The record is host-written and replaces the in-memory flow lease as the
//! authority on what is running in a bucket and what a crashed run left
//! unfinished. It stores each stage's status, the artifact the stage wrote and
//! the diff generation it ran against; the store lease names the live owner.
//! See ADR 0060.

use super::review_flow::{FlowTaskProgress, FlowTaskStage, FlowTaskStatus};
use er_store::{
    Artifact, ArtifactKey, ArtifactStore, LeaseGuard, Loaded, Mode, Scope, StoreError, UpdateError,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// What kind of workflow the record describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowKind {
    Quick,
    Standard,
    Deep,
    /// A run started from the arena launcher rather than a review flow.
    Arena,
}

/// One stage's durable status. `Running` covers queued work as well: the
/// engine's in-memory task rows distinguish waiting from queued, but a crashed
/// bucket only needs to know the stage never finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StageStatus {
    Pending,
    Running,
    Complete,
    Failed,
    Interrupted,
}

/// The stage family, matching the flow's task stages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StageKind {
    Triage,
    Finding,
    Verifying,
    Arena,
}

/// One durable stage row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct StageRecord {
    /// The flow's stable task id (`triage`, `finding:<lens>`,
    /// `reviewer:<id>:round:<n>`, `verifying:arbiter`), so the record and the
    /// in-memory progress rows cannot drift apart.
    pub id: String,
    pub stage: StageKind,
    pub label: String,
    pub status: StageStatus,
    /// The artifact the stage wrote, such as `triage`, `review`,
    /// `expert:<lens>` or `arena:<run_id>`.
    #[serde(default)]
    pub output: String,
    /// The diff generation the stage ran against.
    pub diff_hash: String,
}

/// The process that owns a running record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WorkflowHolder {
    pub pid: u32,
    /// Unique within the process, so a token from a released flow never
    /// matches a later one.
    pub label: String,
}

/// How the workflow ended. `Cancelled` still leaves unfinished stages
/// `Interrupted`; the outcome records that the user asked to stop, which
/// suppresses the load-time re-run offer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowOutcome {
    Complete,
    Failed,
    Cancelled,
    Interrupted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WorkflowRecord {
    pub version: u32,
    /// None until triage chooses a depth; the arena launcher sets it at once.
    #[serde(default)]
    pub kind: Option<WorkflowKind>,
    /// The diff generation the whole workflow is bound to.
    pub diff_hash: String,
    #[serde(default)]
    pub started_at: String,
    #[serde(default)]
    pub outcome: Option<WorkflowOutcome>,
    #[serde(default)]
    pub holder: Option<WorkflowHolder>,
    #[serde(default)]
    pub stages: Vec<StageRecord>,
}

impl Artifact for WorkflowRecord {
    const KIND: &'static str = "workflow";
    const VERSION: u32 = 1;
    const MODE: Mode = Mode::Enveloped;
    const FILE: &'static str = "workflow.json";
    type Slot = ();

    fn diff_hash(&self) -> Option<&str> {
        Some(&self.diff_hash)
    }
}

impl WorkflowRecord {
    /// A new record whose stages mirror the flow's planned task rows.
    pub fn new(
        kind: Option<WorkflowKind>,
        diff_hash: &str,
        started_at: String,
        holder: WorkflowHolder,
        tasks: &[FlowTaskProgress],
        arena_run_id: Option<&str>,
    ) -> Self {
        Self {
            version: Self::VERSION,
            kind,
            diff_hash: diff_hash.to_string(),
            started_at,
            outcome: None,
            holder: Some(holder),
            stages: stages_from_tasks(tasks, arena_run_id, diff_hash),
        }
    }

    pub fn stage(&self, id: &str) -> Option<&StageRecord> {
        self.stages.iter().find(|stage| stage.id == id)
    }

    /// Whether any stage was still running when the record was last written.
    pub fn has_running_stage(&self) -> bool {
        self.stages
            .iter()
            .any(|stage| stage.status == StageStatus::Running)
    }

    /// Whether a load should offer to re-run unfinished stages.
    pub fn offers_rerun(&self) -> bool {
        self.outcome != Some(WorkflowOutcome::Cancelled)
            && self
                .stages
                .iter()
                .any(|stage| matches!(stage.status, StageStatus::Interrupted | StageStatus::Failed))
    }
}

/// Map the flow's task rows onto durable stage rows. `Skipped` rows never ran,
/// so they are pending again for a resume.
pub fn stages_from_tasks(
    tasks: &[FlowTaskProgress],
    arena_run_id: Option<&str>,
    diff_hash: &str,
) -> Vec<StageRecord> {
    tasks
        .iter()
        .map(|task| StageRecord {
            id: task.id.clone(),
            stage: stage_kind(task.stage),
            label: task.label.clone(),
            status: stage_status(task.status),
            output: stage_output(task, arena_run_id),
            diff_hash: diff_hash.to_string(),
        })
        .collect()
}

fn stage_kind(stage: FlowTaskStage) -> StageKind {
    match stage {
        FlowTaskStage::Triage => StageKind::Triage,
        FlowTaskStage::Finding => StageKind::Finding,
        FlowTaskStage::Verifying => StageKind::Verifying,
    }
}

fn stage_status(status: FlowTaskStatus) -> StageStatus {
    match status {
        FlowTaskStatus::Waiting | FlowTaskStatus::Skipped => StageStatus::Pending,
        FlowTaskStatus::Queued | FlowTaskStatus::Running => StageStatus::Running,
        FlowTaskStatus::Done => StageStatus::Complete,
        FlowTaskStatus::Failed => StageStatus::Failed,
        FlowTaskStatus::Cancelled => StageStatus::Interrupted,
    }
}

fn stage_output(task: &FlowTaskProgress, arena_run_id: Option<&str>) -> String {
    match task.stage {
        FlowTaskStage::Triage => "triage".to_string(),
        FlowTaskStage::Finding => match arena_run_id {
            Some(run_id) => format!("arena:{run_id}"),
            None => match task.lens.as_deref() {
                Some(lens) if lens == super::review::GENERAL_LENS => "review".to_string(),
                Some(lens) => format!("expert:{lens}"),
                None => String::new(),
            },
        },
        FlowTaskStage::Verifying => match arena_run_id {
            Some(run_id) => format!("arena:{run_id}"),
            None => "arbiter".to_string(),
        },
    }
}

/// Read the record. `None` when the bucket has none.
///
/// A record this build cannot read is an error: the file is host-written, so
/// an unreadable one is damage, not an agent's partial output.
pub fn load(store: &ArtifactStore, scope: &Scope) -> anyhow::Result<Option<WorkflowRecord>> {
    let key = ArtifactKey::<WorkflowRecord>::new(scope.clone(), ())?;
    match store.read(&key)? {
        Loaded::Found(stored) => Ok(Some(stored.value)),
        Loaded::Absent => Ok(None),
        Loaded::Unreadable(reason) => Err(anyhow::anyhow!(
            "{} is unreadable: {reason:?}",
            store.path(&key).display()
        )),
    }
}

/// Write a new record, replacing the bucket's previous one.
pub fn save(store: &ArtifactStore, scope: &Scope, record: &WorkflowRecord) -> anyhow::Result<()> {
    let key = ArtifactKey::<WorkflowRecord>::new(scope.clone(), ())?;
    store.write_durable(&key, record)?;
    Ok(())
}

/// One read-modify-write of the record under the store lock. `false` when the
/// bucket has no record.
pub fn update(
    store: &ArtifactStore,
    scope: &Scope,
    f: impl FnOnce(&mut WorkflowRecord),
) -> anyhow::Result<bool> {
    let key = ArtifactKey::<WorkflowRecord>::new(scope.clone(), ())?;
    let path = store.path(&key);
    let outcome = store.update_durable(
        &key,
        er_store::DEFAULT_LOCK_TIMEOUT,
        |current| -> Result<WorkflowRecord, ()> {
            let Some(mut record) = current else {
                return Err(());
            };
            f(&mut record);
            Ok(record)
        },
    );
    match outcome {
        Ok(_) => Ok(true),
        Err(UpdateError::Apply(())) if store.exists(&key) => {
            // The file exists but the update read it as unreadable; report why
            // instead of treating it as absent.
            load(store, scope)?;
            Ok(false)
        }
        Err(UpdateError::Apply(())) => Ok(false),
        Err(UpdateError::Store(StoreError::NewerVersion { found, .. })) => Err(anyhow::anyhow!(
            "{} is unreadable: newer version {found}",
            path.display()
        )),
        Err(UpdateError::Store(error)) => Err(error.into()),
    }
}

/// Try to take the bucket's workflow lease without waiting.
pub fn try_lease(
    store: &ArtifactStore,
    scope: &Scope,
    label: &str,
) -> anyhow::Result<Option<LeaseGuard>> {
    match store.lease(scope, Duration::ZERO, label) {
        Ok(guard) => Ok(Some(guard)),
        Err(StoreError::LeaseHeld { .. } | StoreError::AlreadyLocked { .. }) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// A standalone arena action's lease and record identity. Every run it starts
/// shares the lease; the guard releases when the last supervisor drops it.
#[derive(Clone)]
pub struct ArenaWorkflow {
    pub scope: Scope,
    pub diff_hash: String,
    pub started_at: String,
    pub holder: WorkflowHolder,
    pub guard: std::sync::Arc<LeaseGuard>,
}

fn arena_stage(run_id: &str, diff_hash: &str) -> StageRecord {
    StageRecord {
        id: format!("arena:{run_id}"),
        stage: StageKind::Arena,
        label: format!("Arena run {run_id}"),
        status: StageStatus::Running,
        output: format!("arena:{run_id}"),
        diff_hash: diff_hash.to_string(),
    }
}

/// Append one run's stage to the bucket's arena record, replacing a record
/// from a different diff generation.
pub fn begin_arena_run(
    store: &ArtifactStore,
    workflow: &ArenaWorkflow,
    run_id: &str,
) -> anyhow::Result<()> {
    let key = ArtifactKey::<WorkflowRecord>::new(workflow.scope.clone(), ())?;
    let fresh = WorkflowRecord {
        version: WorkflowRecord::VERSION,
        kind: Some(WorkflowKind::Arena),
        diff_hash: workflow.diff_hash.clone(),
        started_at: workflow.started_at.clone(),
        outcome: None,
        holder: Some(workflow.holder.clone()),
        stages: vec![arena_stage(run_id, &workflow.diff_hash)],
    };
    match store.update_durable(
        &key,
        er_store::DEFAULT_LOCK_TIMEOUT,
        |current| -> Result<WorkflowRecord, ()> {
            let Some(mut record) = current else {
                return Ok(fresh);
            };
            if record.diff_hash != workflow.diff_hash {
                return Ok(fresh);
            }
            record.holder = Some(workflow.holder.clone());
            record.outcome = None;
            let stage = arena_stage(run_id, &workflow.diff_hash);
            match record.stages.iter_mut().find(|row| row.id == stage.id) {
                Some(row) => *row = stage,
                None => record.stages.push(stage),
            }
            Ok(record)
        },
    ) {
        Ok(_) => Ok(()),
        Err(er_store::UpdateError::Apply(())) => Err(anyhow::anyhow!(
            "{} is unreadable",
            store.path(&key).display()
        )),
        Err(er_store::UpdateError::Store(error)) => Err(error.into()),
    }
}

/// Close one run's stage; when every stage is terminal, set the record's
/// outcome and clear the holder.
pub fn end_arena_run(
    store: &ArtifactStore,
    workflow: &ArenaWorkflow,
    run_id: &str,
    outcome: WorkflowOutcome,
) -> anyhow::Result<()> {
    update(store, &workflow.scope, |record| {
        if record.diff_hash != workflow.diff_hash {
            return;
        }
        let id = format!("arena:{run_id}");
        let Some(stage) = record.stages.iter_mut().find(|row| row.id == id) else {
            return;
        };
        stage.status = match outcome {
            WorkflowOutcome::Complete => StageStatus::Complete,
            WorkflowOutcome::Failed => StageStatus::Failed,
            WorkflowOutcome::Cancelled | WorkflowOutcome::Interrupted => StageStatus::Interrupted,
        };
        let all_done = record.stages.iter().all(|row| {
            matches!(
                row.status,
                StageStatus::Complete | StageStatus::Failed | StageStatus::Interrupted
            )
        });
        if all_done {
            record.outcome = Some(
                if record
                    .stages
                    .iter()
                    .any(|row| row.status == StageStatus::Failed)
                {
                    WorkflowOutcome::Failed
                } else if record
                    .stages
                    .iter()
                    .any(|row| row.status == StageStatus::Interrupted)
                {
                    WorkflowOutcome::Cancelled
                } else {
                    WorkflowOutcome::Complete
                },
            );
            record.holder = None;
        }
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::review_flow::{FlowTaskProgress, FlowTaskStatus};

    fn task(
        id: &str,
        stage: FlowTaskStage,
        lens: Option<&str>,
        status: FlowTaskStatus,
    ) -> FlowTaskProgress {
        FlowTaskProgress {
            id: id.to_string(),
            stage,
            lens: lens.map(str::to_string),
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

    fn record() -> WorkflowRecord {
        let tasks = vec![
            task("triage", FlowTaskStage::Triage, None, FlowTaskStatus::Done),
            task(
                "finding:security",
                FlowTaskStage::Finding,
                Some("security"),
                FlowTaskStatus::Running,
            ),
            task(
                "verifying:arbiter",
                FlowTaskStage::Verifying,
                None,
                FlowTaskStatus::Waiting,
            ),
        ];
        WorkflowRecord::new(
            Some(WorkflowKind::Standard),
            "hash",
            "t0".to_string(),
            WorkflowHolder {
                pid: 1,
                label: "flow-1".to_string(),
            },
            &tasks,
            None,
        )
    }

    fn dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn stage_rows_mirror_the_flow_plan() {
        let record = record();
        assert_eq!(record.stages.len(), 3);
        let triage = record.stage("triage").unwrap();
        assert_eq!(triage.status, StageStatus::Complete);
        assert_eq!(triage.output, "triage");
        let finding = record.stage("finding:security").unwrap();
        assert_eq!(finding.stage, StageKind::Finding);
        assert_eq!(finding.status, StageStatus::Running);
        assert_eq!(finding.output, "expert:security");
        let arbiter = record.stage("verifying:arbiter").unwrap();
        assert_eq!(arbiter.status, StageStatus::Pending);
        assert_eq!(arbiter.output, "arbiter");
    }

    #[test]
    fn arena_rows_name_the_run_and_status_maps_every_task_state() {
        let tasks = vec![
            task(
                "reviewer:expert:security::p-m:round:1",
                FlowTaskStage::Finding,
                Some("security"),
                FlowTaskStatus::Queued,
            ),
            task(
                "verifying:arbiter",
                FlowTaskStage::Verifying,
                None,
                FlowTaskStatus::Skipped,
            ),
        ];
        let rows = stages_from_tasks(&tasks, Some("run-1"), "hash");
        assert_eq!(rows[0].status, StageStatus::Running);
        assert_eq!(rows[0].output, "arena:run-1");
        assert_eq!(rows[1].status, StageStatus::Pending);
        assert_eq!(rows[1].output, "arena:run-1");

        // Every in-memory status maps to exactly one durable status.
        let mapped = [
            (FlowTaskStatus::Waiting, StageStatus::Pending),
            (FlowTaskStatus::Queued, StageStatus::Running),
            (FlowTaskStatus::Running, StageStatus::Running),
            (FlowTaskStatus::Done, StageStatus::Complete),
            (FlowTaskStatus::Failed, StageStatus::Failed),
            (FlowTaskStatus::Cancelled, StageStatus::Interrupted),
            (FlowTaskStatus::Skipped, StageStatus::Pending),
        ];
        for (flow, stage) in mapped {
            assert_eq!(stage_status(flow), stage, "{flow:?}");
        }
    }

    #[test]
    fn record_round_trips_through_the_store() {
        let dir = dir();
        crate::storage::with_test_storage_root(dir.path(), || {
            let store = crate::store_registry::store();
            let scope = Scope::dir(dir.path().to_str().unwrap()).unwrap();
            assert!(load(&store, &scope).unwrap().is_none());
            let record = record();
            save(&store, &scope, &record).unwrap();
            assert_eq!(load(&store, &scope).unwrap(), Some(record.clone()));

            assert!(update(&store, &scope, |value| {
                value.stages[1].status = StageStatus::Complete;
            })
            .unwrap());
            let stored = load(&store, &scope).unwrap().unwrap();
            assert_eq!(
                stored.stage("finding:security").unwrap().status,
                StageStatus::Complete
            );
        });
    }

    #[test]
    fn a_newer_record_is_refused_and_left_alone() {
        let dir = dir();
        crate::storage::with_test_storage_root(dir.path(), || {
            let store = crate::store_registry::store();
            let scope = Scope::dir(dir.path().to_str().unwrap()).unwrap();
            let newer: &[u8] = br#"{"version":99,"kind":"quick","diff_hash":"h","stages":[]}"#;
            std::fs::write(dir.path().join("workflow.json"), newer).unwrap();
            let error = load(&store, &scope).unwrap_err();
            assert!(error.to_string().contains("unreadable"), "{error}");
            let error = update(&store, &scope, |_| {}).unwrap_err();
            assert!(error.to_string().contains("unreadable"), "{error}");
            assert_eq!(
                std::fs::read(dir.path().join("workflow.json")).unwrap(),
                newer
            );
        });
    }

    #[test]
    fn a_held_lease_is_not_acquired_without_waiting() {
        use std::sync::{Arc, Barrier};

        let dir = dir();
        crate::storage::with_process_test_storage_root(dir.path(), || {
            let scope = Scope::dir(dir.path().to_str().unwrap()).unwrap();
            let barrier = Arc::new(Barrier::new(2));
            let held = {
                let barrier = Arc::clone(&barrier);
                let scope = scope.clone();
                std::thread::spawn(move || {
                    let store = crate::store_registry::store();
                    let guard = store
                        .lease(&scope, Duration::from_secs(5), "holder")
                        .unwrap();
                    barrier.wait();
                    barrier.wait();
                    drop(guard);
                })
            };
            barrier.wait();
            let store = crate::store_registry::store();
            assert!(
                try_lease(&store, &scope, "probe").unwrap().is_none(),
                "a held lease reports as unavailable"
            );
            barrier.wait();
            held.join().unwrap();
            assert!(try_lease(&store, &scope, "probe").unwrap().is_some());
        });
    }
}
