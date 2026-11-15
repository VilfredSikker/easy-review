//! What a host supplies to the review-flow driver.
//!
//! The driver decides which stage runs next; the host owns everything that
//! differs between the App and the workspace service: where the view's inputs
//! come from, how a stage task is queued and reported, who holds the bucket
//! lease, and where arena runs live. Every method is a plain call with plain
//! data, so a host can answer from its own state without the driver knowing
//! its shape.

use super::flow_notification_message;
use crate::ai::change_facts::OwnedRepoRules;
use crate::ai::review_flow::{FlowEvent, FlowTaskStatus, ReviewFlow};
use crate::ai::review_md::ReviewMdSource;
use crate::ai::WorkflowHolder;
use crate::arena::{
    ArenaRunControl, ArenaRunInputs, ArenaScope, ArenaStartParams, SeededStartParams,
};
use crate::config::{AiSelection, ErConfig};
use crate::ports::Ports;
use crate::task_target::BackgroundTaskTarget;
use anyhow::Result;
use std::sync::Arc;

/// Capability naming the one workflow that owns a bucket. The OS lease behind
/// it is the authority across processes; the holder is its identity, so a
/// token from a released flow never matches a later one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewFlowLease {
    bucket: String,
    sequence: u64,
    holder: er_store::HolderInfo,
}

impl ReviewFlowLease {
    /// Only a host that has just taken the OS lease builds one.
    pub fn new(bucket: String, sequence: u64, holder: er_store::HolderInfo) -> Self {
        Self {
            bucket,
            sequence,
            holder,
        }
    }

    pub fn bucket(&self) -> &str {
        &self.bucket
    }

    /// Unique per flow within the process, for archive keys.
    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    pub fn holder(&self) -> &er_store::HolderInfo {
        &self.holder
    }
}

/// The view a flow reviews, captured once when it starts. A later view switch
/// in the host must not retarget accepted work.
#[derive(Debug, Clone)]
pub struct ReviewFlowInputs {
    pub target: BackgroundTaskTarget,
    pub arena_scope: ArenaScope,
    /// Checkout used by providers, distinct from `target.repo_root` identity.
    pub repo_root: String,
    /// Prompt note naming the ref that holds the reviewed code, set when
    /// `repo_root` is a checkout of another branch.
    pub reviewed_ref_note: Option<String>,
    pub branch_ref: String,
    pub base_branch: String,
    /// The diff with the project's review-ignore globs already removed.
    pub raw_diff: String,
    pub owned_rules: OwnedRepoRules,
    pub review_md_source: ReviewMdSource,
}

/// A stage task's terminal record, consumed once.
pub struct StageCompletion {
    pub target: BackgroundTaskTarget,
    pub kind: String,
    /// True only when the task finished cleanly.
    pub done: bool,
    pub error: Option<String>,
}

/// The host side of a review flow. The driver calls these in the same order
/// whichever host runs it.
pub trait ReviewFlowHost {
    /// Live settings. Confirming Deep and switching to Standard recheck the
    /// model groups against these, not against the flow's captured copy.
    fn config(&self) -> &ErConfig;
    fn current_ai_effort(&self) -> Option<&str>;
    fn ports(&self) -> &Ports;
    fn notify(&mut self, message: &str);
    fn notify_long(&mut self, message: &str);

    /// The view to review for `scope`, or why it cannot be reviewed.
    fn review_flow_inputs(&self, scope: &str) -> Result<ReviewFlowInputs>;
    /// The filtered diff the open view of `target` shows now, or why no open
    /// view shows it. Acceptance checks a run against it.
    fn current_review_diff(&self, target: &BackgroundTaskTarget) -> Result<String>;

    /// Mark stale arena runs in the bucket before a flow takes it.
    fn recover_arena_bucket(&self, er_dir: &str) -> Result<()>;
    fn acquire_review_flow_lease(&mut self, er_dir: &str) -> Result<ReviewFlowLease>;
    fn release_review_flow_lease(&mut self, lease: &ReviewFlowLease) -> Result<()>;
    fn workflow_holder(&self, er_dir: &str) -> Option<WorkflowHolder>;

    /// Queue one triage or finder task and answer its id before anything can
    /// observe its result.
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
    ) -> Result<String>;
    /// Claim a queued task for the flow, so its completion is kept for
    /// [`Self::take_review_flow_task_completion`].
    fn register_review_flow_task(&mut self, id: String);
    fn take_review_flow_task_completion(&mut self, id: &str) -> Option<StageCompletion>;
    /// Queued or running, while the task is still the host's.
    fn stage_status(&self, id: &str) -> Option<FlowTaskStatus>;
    /// Retire a task that has not launched; false once it is running.
    fn cancel_queued_background_task(&mut self, id: &str) -> bool;

    fn arena_start_seeded_for(
        &mut self,
        repo_root: String,
        config: ErConfig,
        params: SeededStartParams,
        lease: &ReviewFlowLease,
    ) -> Result<Option<String>>;
    fn arena_start_for(
        &mut self,
        inputs: ArenaRunInputs,
        config: ErConfig,
        params: ArenaStartParams,
        lease: &ReviewFlowLease,
    ) -> Result<String>;
    fn arena_cancel(&mut self, run_id: &str) -> Result<()>;
    fn arena_control(&self, run_id: &str) -> Option<Arc<ArenaRunControl>>;
    fn arena_is_active(&self, run_id: &str) -> bool;
}

impl dyn ReviewFlowHost + '_ {
    /// Apply one event and tell the user when it moved the flow.
    pub(super) fn apply_flow_event_notifying(
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
}
