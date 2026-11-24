//! The App's host for the review-flow driver.
//!
//! The stage sequence lives in `crate::review_flow_driver`, shared with the
//! workspace service. The App supplies the active view's inputs, launches
//! stages through its background queue, holds the bucket lease and owns the
//! arena runs. Its public review-flow calls keep their names and lend the
//! driver the App as its host for one call.

mod deep;
mod standard;

/// The driver reads and accepts flow results; the App's callers name them here.
pub mod results {
    pub use crate::review_flow_driver::ReviewFlowResults;
}

use super::background::BackgroundTaskTarget;
use super::{App, CommandStatus};
use crate::ai::review_flow::{FlowTaskStatus, ReviewFlow};
use crate::ai::WorkflowHolder;
use crate::arena::{ArenaRunControl, ArenaRunInputs, ArenaStartParams, SeededStartParams};
use crate::config::{AiSelection, ErConfig};
use crate::ports::Ports;
use crate::review_flow_driver::{
    flow_from_record, holder_of, ReviewFlowHost, ReviewFlowInputs, ReviewFlowLease, ReviewFlows,
    StageCompletion,
};
use anyhow::{Context, Result};
use results::ReviewFlowResults;
use std::sync::Arc;

impl App {
    /// Lend the driver this App as its host. The flows are out of `self`
    /// for the call, so no host method may read them; none does.
    ///
    /// A panic in the driver puts the flows back before it unwinds further:
    /// they name the leases in `workflow_leases`, and nothing else would
    /// release those.
    fn with_review_flows<T>(
        &mut self,
        drive: impl FnOnce(&mut ReviewFlows, &mut dyn ReviewFlowHost) -> T,
    ) -> T {
        let mut flows = std::mem::take(&mut self.review_flows);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            drive(&mut flows, &mut *self)
        }));
        self.review_flows = flows;
        result.unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    }

    /// Start a flow against the active local view. `scope` must identify that
    /// exact view; branch, staged and unstaged diffs are supported.
    pub fn start_review_flow(&mut self, scope: &str) -> Result<()> {
        self.with_review_flows(|flows, host| flows.start_review_flow(host, scope))
    }

    /// Re-run an interrupted workflow from its record.
    pub fn rerun_review_flow(&mut self, er_dir: &str) -> Result<()> {
        self.with_review_flows(|flows, host| flows.rerun_review_flow(host, er_dir))
    }

    /// Start the already-approved Deep plan against its captured bucket.
    pub fn confirm_review_flow(&mut self, er_dir: &str) -> Result<()> {
        self.with_review_flows(|flows, host| flows.confirm_review_flow(host, er_dir))
    }

    /// Decline only a pending Deep confirmation.
    pub fn decline_review_flow(&mut self, er_dir: &str) -> Result<()> {
        self.with_review_flows(|flows, host| flows.decline_review_flow(host, er_dir))
    }

    /// Run the lenses triage already chose as ordinary Standard finders.
    pub fn run_review_flow_standard(&mut self, er_dir: &str) -> Result<()> {
        self.with_review_flows(|flows, host| flows.run_review_flow_standard(host, er_dir))
    }

    /// Cancel orchestration. Running finder processes drain naturally; queued
    /// owned tasks are retired with their exact completion records.
    pub fn cancel_review_flow(&mut self, er_dir: &str) -> bool {
        self.with_review_flows(|flows, host| flows.cancel_review_flow(host, er_dir))
    }

    /// Advance every flow using exact registered task completions and captured
    /// arena run IDs. Called by the background poll hook after queue dispatch.
    pub(crate) fn advance_review_flows(&mut self) {
        self.with_review_flows(|flows, host| flows.advance_review_flows(host));
    }

    #[cfg(test)]
    pub(crate) fn advance_review_flow(&mut self, key: &str) {
        self.with_review_flows(|flows, host| flows.advance_review_flow(host, key));
    }

    pub fn review_flow_for(&self, er_dir: &str) -> Option<&ReviewFlow> {
        self.review_flows.review_flow_for(er_dir)
    }

    /// Revision for host snapshot invalidation. It changes when a flow's
    /// visible projection changes, not on every poll.
    pub fn review_flow_revision(&self) -> u64 {
        self.review_flows.review_flow_revision()
    }

    /// True while terminal flow state still has a process or arena supervisor
    /// draining from the bucket.
    pub fn review_flow_is_draining(&self, er_dir: &str) -> bool {
        self.review_flows.review_flow_is_draining(er_dir)
    }

    pub(super) fn review_flow_run_target(
        &self,
        er_dir: &str,
        run_id: &str,
    ) -> Option<&BackgroundTaskTarget> {
        self.review_flows.review_flow_run_target(er_dir, run_id)
    }

    /// Professor triage keeps its ordinary Quick review and exposes this
    /// follow-up suggestion to a later snapshot/UI layer.
    pub fn review_flow_professor_follow_up_suggested(&self, er_dir: &str) -> bool {
        self.review_flows
            .review_flow_professor_follow_up_suggested(er_dir)
    }

    /// The holder of the lease this process holds for `er_dir`, if any.
    pub(super) fn workflow_holder(&self, er_dir: &str) -> Option<WorkflowHolder> {
        self.workflow_leases.get(er_dir).map(holder_of)
    }

    /// On load, mark a dead owner's running stages `Interrupted` and project
    /// the record into the flow view so the user can re-run it. The lease probe
    /// does not wait, and nothing is enqueued or spawned here.
    pub fn reconcile_workflow_records(&mut self) {
        let mut projections = Vec::new();
        for tab in &self.tabs {
            let er_dir = tab.er_dir();
            if self.workflow_leases.contains_key(&er_dir) {
                continue;
            }
            let Some(record) = crate::workflow_recovery::reconcile_workflow_record(&er_dir) else {
                continue;
            };
            if record.resumable() {
                projections.push((er_dir, flow_from_record(&record)));
            }
        }
        for (er_dir, flow) in projections {
            self.review_flows.insert(er_dir, flow);
        }
    }

    /// Load a flow's surviving results without importing or changing any files.
    /// Both the bucket and run ID are required; this never follows the active tab.
    pub fn review_flow_results(&self, er_dir: &str, run_id: &str) -> Result<ReviewFlowResults> {
        self.review_flows.review_flow_results(er_dir, run_id)
    }

    /// Explicitly import only selected, still-eligible findings from a flow run.
    /// Admission, captured-view checks, and a fresh diff hash guard the write.
    pub fn accept_review_flow_findings(
        &mut self,
        er_dir: &str,
        run_id: &str,
        finding_ids: Vec<String>,
    ) -> Result<usize> {
        let accepted = self.with_review_flows(|flows, host| {
            flows.accept_review_flow_findings(host, er_dir, run_id, finding_ids)
        })?;
        for tab in self.tabs.iter_mut().filter(|tab| tab.er_dir() == er_dir) {
            tab.reload_ai_state();
        }
        Ok(accepted)
    }

    /// The filtered diff of the open tab that shows `target`, which acceptance
    /// checks against the run's diff hash.
    fn current_review_diff(&self, target: &BackgroundTaskTarget) -> Result<String> {
        let tab = self
            .tabs
            .iter()
            .find(|tab| {
                crate::review_flow_driver::shows_target(&tab.identity(), &tab.er_dir(), target)
            })
            .context("The captured review target is no longer open; start a new Review")?;
        let raw_diff = tab
            .fetch_tab_raw_diff(&target.scope)
            .context("fetch the current diff for review acceptance")?;
        Ok(crate::review_flow_driver::filtered_review_diff(
            &raw_diff,
            &target.repo_root,
            target.remote_repo.as_deref(),
        ))
    }
}

/// The App's tab, read by the rules every host shares.
impl crate::review_flow_driver::ReviewedTab for crate::app::TabState {
    fn identity(&self) -> crate::tab_identity::TabIdentity<'_> {
        crate::app::TabState::identity(self)
    }

    fn er_dir(&self) -> String {
        crate::app::TabState::er_dir(self)
    }

    fn raw_diff(&self, scope: &str) -> Result<String> {
        self.raw_diff_for_review(scope)
    }

    fn owned_rules(&self) -> crate::ai::change_facts::OwnedRepoRules {
        self.owned_repo_rules()
    }

    fn review_md_source(&self) -> crate::ai::review_md::ReviewMdSource {
        crate::app::TabState::review_md_source(self)
    }
}

/// Stages queue as App background tasks, so they share the review cap, the
/// task list and the log panel with every other review action.
impl ReviewFlowHost for App {
    fn config(&self) -> &ErConfig {
        &self.config
    }

    fn current_ai_effort(&self) -> Option<&str> {
        self.current_ai_effort.as_deref()
    }

    fn ports(&self) -> &Ports {
        &self.ports
    }

    fn notify(&mut self, message: &str) {
        App::notify(self, message);
    }

    fn notify_long(&mut self, message: &str) {
        App::notify_long(self, message);
    }

    fn review_flow_inputs(&self, scope: &str) -> Result<ReviewFlowInputs> {
        crate::review_flow_driver::review_flow_inputs(self.tab(), scope)
    }

    fn current_review_diff(&self, target: &BackgroundTaskTarget) -> Result<String> {
        App::current_review_diff(self, target)
    }

    fn recover_arena_bucket(&self, er_dir: &str) -> Result<()> {
        App::recover_arena_bucket(self, er_dir)
    }

    fn acquire_review_flow_lease(&mut self, er_dir: &str) -> Result<ReviewFlowLease> {
        App::acquire_review_flow_lease(self, er_dir)
    }

    fn release_review_flow_lease(&mut self, lease: &ReviewFlowLease) -> Result<()> {
        App::release_review_flow_lease(self, lease)
    }

    fn workflow_holder(&self, er_dir: &str) -> Option<WorkflowHolder> {
        App::workflow_holder(self, er_dir)
    }

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
            None,
        );
        self.pending_ai_selection_override = previous;
        result
    }

    fn register_review_flow_task(&mut self, id: String) {
        App::register_review_flow_task(self, id);
    }

    fn take_review_flow_task_completion(&mut self, id: &str) -> Option<StageCompletion> {
        App::take_review_flow_task_completion(self, id).map(|task| StageCompletion {
            done: task.status == CommandStatus::Done,
            target: task.target,
            kind: task.kind,
            error: task.error,
        })
    }

    fn stage_status(&self, id: &str) -> Option<FlowTaskStatus> {
        if self
            .pending_background_tasks
            .iter()
            .any(|task| task.task.id == id)
        {
            return Some(FlowTaskStatus::Queued);
        }
        if self.background_tasks.contains_key(id)
            || self
                .recent_background_tasks
                .iter()
                .any(|task| task.id == id)
        {
            return Some(FlowTaskStatus::Running);
        }
        None
    }

    fn cancel_queued_background_task(&mut self, id: &str) -> bool {
        App::cancel_queued_background_task(self, id)
    }

    fn arena_start_seeded_for(
        &mut self,
        repo_root: String,
        config: ErConfig,
        params: SeededStartParams,
        lease: &ReviewFlowLease,
    ) -> Result<Option<String>> {
        App::arena_start_seeded_for(self, repo_root, config, params, lease)
    }

    fn arena_start_for(
        &mut self,
        inputs: ArenaRunInputs,
        config: ErConfig,
        params: ArenaStartParams,
        lease: &ReviewFlowLease,
    ) -> Result<String> {
        App::arena_start_for(self, inputs, config, params, lease)
    }

    fn arena_cancel(&mut self, run_id: &str) -> Result<()> {
        App::arena_cancel(self, run_id)
    }

    fn arena_control(&self, run_id: &str) -> Option<Arc<ArenaRunControl>> {
        self.arena_runs.get(run_id)
    }

    fn arena_is_active(&self, run_id: &str) -> bool {
        self.arena_runs.is_active(run_id)
    }
}

#[cfg(test)]
mod tests {
    use super::App;
    use crate::ai::review_flow::ReviewFlow;

    #[test]
    fn a_panicking_driver_call_keeps_the_flows() {
        let mut app = App::new_for_test(Vec::new());
        app.review_flows
            .insert("bucket".to_owned(), ReviewFlow::new("hash"));

        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            app.with_review_flows(|_, _| panic!("driver panicked"));
        }));

        assert!(unwound.is_err(), "the panic must still reach the caller");
        assert!(app.review_flow_for("bucket").is_some());
    }
}
