//! The App's side of review-flow results: the driver reads and accepts them,
//! and the App supplies the open view's diff and reloads the tabs after.

use super::super::background::BackgroundTaskTarget;
use super::super::{App, DiffMode};
use anyhow::{Context, Result};

pub use crate::review_flow_driver::ReviewFlowResults;

impl App {
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
    pub(super) fn current_review_diff(&self, target: &BackgroundTaskTarget) -> Result<String> {
        let tab = self
            .tabs
            .iter()
            .find(|tab| tab_matches_target(tab, target))
            .context("The captured review target is no longer open; start a new Review")?;
        let raw_diff = tab
            .fetch_tab_raw_diff(&target.scope)
            .context("fetch the current diff for review acceptance")?;
        let globs = crate::projects_pins::review_ignore_globs_for_repo(
            &target.repo_root,
            target.remote_repo.as_deref(),
        );
        Ok(crate::git::filter_raw_diff_exclude_globs(&raw_diff, &globs))
    }
}

fn tab_matches_target(tab: &crate::app::TabState, target: &BackgroundTaskTarget) -> bool {
    let branch_ref = tab
        .local_branch_view
        .as_deref()
        .unwrap_or(tab.current_branch.as_str());
    let scope_matches = match target.scope.as_str() {
        "branch" => matches!(
            tab.mode,
            DiffMode::Branch | DiffMode::PrDiff | DiffMode::Tour
        ),
        "staged" => tab.mode == DiffMode::Staged,
        "unstaged" => tab.mode == DiffMode::Unstaged,
        _ => false,
    };
    tab.repo_root == target.repo_root
        && tab.er_dir() == target.er_dir
        && branch_ref == target.branch_label
        && tab.base_branch == target.base_branch
        && tab.pr_number == target.pr_number
        && tab.remote_repo == target.remote_repo
        && tab.is_remote() == (target.remote_repo.is_some() && tab.local_branch_view.is_none())
        && scope_matches
}
