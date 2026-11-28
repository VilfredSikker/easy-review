use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::Serialize;

pub use er_service::app_records::sidebar::{
    minimal_pr_info, recently_merged_prs, BranchInfo, CheckSummary, GhCommentSummary,
    GhReviewSummary, GhReviewerSummary, GhStatusCache, GhUser, GithubStatusSnapshot, MetaCache,
    PrInfo, ProjectMeta, RepoMergeSnapshot, WorktreeSnapshot,
};

// ── Wire types ──────────────────────────────────────────────────────────────

/// Active-branch watcher status. Set by the desktop background thread that
/// watches the working tree of the currently active local-branch tab.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct WatchStatusSnapshot {
    pub active: bool,
    pub branch: Option<String>,
    pub root_path: Option<String>,
}

pub type WatchStatusState = Arc<Mutex<WatchStatusSnapshot>>;

#[derive(Debug, Clone, Serialize)]
pub struct ReviewFlowSnapshot {
    pub state: String,
    pub tasks: Vec<er_engine::ai::review_flow::FlowTaskProgress>,
    pub depth: Option<String>,
    pub diff_hash: String,
    pub lenses: Vec<String>,
    pub pending_finders: Vec<String>,
    pub failed_finders: Vec<FailedReviewFlowFinderSnapshot>,
    pub arena_run_id: Option<String>,
    /// Providers with successful finder output for this flow.
    pub successful_finder_providers: Vec<String>,
    /// Provider that successfully completed arbitration, if any.
    pub successful_arbiter_provider: Option<String>,
    /// Sorted unique providers that actually completed finder or arbiter work.
    pub providers_used: Vec<String>,
    pub estimated_cost: Option<f64>,
    pub estimated_latency_seconds: Option<f64>,
    pub kept: usize,
    pub dropped: usize,
    pub awaiting_accept: usize,
    pub reason: Option<String>,
    pub draining: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct FailedReviewFlowFinderSnapshot {
    pub lens: String,
    pub reason: String,
}

impl ReviewFlowSnapshot {
    pub(crate) fn from_flow(flow: &er_engine::ai::review_flow::ReviewFlow, draining: bool) -> Self {
        use er_engine::ai::review_flow::{FlowDepth, FlowState};

        let (state, kept, dropped, awaiting_accept, reason) = match &flow.state {
            FlowState::Triaging => ("triaging", 0, 0, 0, None),
            FlowState::Skipped => ("skipped", 0, 0, 0, None),
            FlowState::AwaitingConfirmation => ("awaiting_confirmation", 0, 0, 0, None),
            FlowState::Finding => ("finding", 0, 0, 0, None),
            FlowState::Verifying => ("verifying", 0, 0, 0, None),
            FlowState::Done {
                kept,
                dropped,
                awaiting_accept,
            } => ("done", *kept, *dropped, *awaiting_accept, None),
            FlowState::Failed { reason } => ("failed", 0, 0, 0, Some(reason.clone())),
            FlowState::Cancelled => ("cancelled", 0, 0, 0, None),
            FlowState::Interrupted { unfinished } => (
                "interrupted",
                0,
                0,
                0,
                Some(format!("{unfinished} unfinished stage(s) can be re-run")),
            ),
        };

        Self {
            state: state.to_string(),
            tasks: flow.tasks.clone(),
            depth: flow.depth.map(|depth| {
                match depth {
                    FlowDepth::Quick => "quick",
                    FlowDepth::Standard => "standard",
                    FlowDepth::Deep => "deep",
                }
                .to_string()
            }),
            diff_hash: flow.diff_hash.clone(),
            lenses: flow.lenses.clone(),
            pending_finders: flow.pending_finders.clone(),
            failed_finders: flow
                .failed_finders
                .iter()
                .map(|(lens, reason)| FailedReviewFlowFinderSnapshot {
                    lens: lens.clone(),
                    reason: reason.clone(),
                })
                .collect(),
            arena_run_id: flow.arena_run_id.clone(),
            successful_finder_providers: flow.successful_finder_providers.clone(),
            successful_arbiter_provider: flow.successful_arbiter_provider.clone(),
            providers_used: flow.providers_used.clone(),
            estimated_cost: flow.estimated_cost,
            estimated_latency_seconds: flow.estimated_latency_seconds,
            kept,
            dropped,
            awaiting_accept,
            reason,
            draining,
        }
    }
}

/// [`resolve_github_status_key`] over a tab's identity, which the desktop
/// reads from the tab's published content.
pub fn resolve_github_status_key_for(
    tab: &er_engine::tab_identity::TabIdentity<'_>,
    pr_cache: &HashMap<String, Vec<PrInfo>>,
) -> Option<(String, String, u64)> {
    // 1. Remote PR: slug + number directly.
    if let (Some(slug), Some(number)) = (tab.remote_repo, tab.pr_number) {
        return slug
            .split_once('/')
            .map(|(o, r)| (o.to_string(), r.to_string(), number));
    }

    let branch = tab.local_branch_view.unwrap_or(tab.current_branch);

    // 2. Local PR tab: trust the tab's own pr_number; only resolve the slug.
    //    When the tab knows its own remote (local PR tabs resolve it from the
    //    repo's git remote), restrict the slug search to that repo — otherwise
    //    a PR number that exists in several repos (e.g. #73 in both easy-review
    //    and design-system) matches an arbitrary repo's PR.
    if let Some(number) = tab.pr_number {
        let matches = |slug: &str, prs: &[PrInfo]| {
            let in_repo = tab
                .remote_repo
                .map(|r| r.eq_ignore_ascii_case(slug))
                .unwrap_or(true);
            in_repo && prs.iter().any(|p| p.number == number)
        };
        return pr_cache
            .iter()
            .find(|(slug, prs)| matches(slug, prs))
            .or_else(|| {
                pr_cache
                    .iter()
                    .filter(|(slug, _)| {
                        tab.remote_repo
                            .map(|r| r.eq_ignore_ascii_case(slug))
                            .unwrap_or(true)
                    })
                    .find(|(_, prs)| prs.iter().any(|p| p.head_ref == branch))
            })
            .and_then(|(slug, _)| {
                slug.split_once('/')
                    .map(|(o, r)| (o.to_string(), r.to_string(), number))
            });
    }

    // 3. Plain branch / working tab: match by head_ref, preferring an OPEN PR.
    //    When the tab knows its own remote (branch tabs resolve it from the
    //    repo's git remote), restrict the match to that repo — otherwise a
    //    branch name that exists in several repos matches an arbitrary PR in
    //    the wrong one.
    pr_cache
        .iter()
        .filter(|(slug, _)| {
            tab.remote_repo
                .map(|r| r.eq_ignore_ascii_case(slug))
                .unwrap_or(true)
        })
        .find_map(|(slug, prs)| {
            prs.iter()
                .filter(|p| p.head_ref == branch)
                .min_by_key(|p| if p.state == "OPEN" { 0 } else { 1 })
                .and_then(|p| {
                    slug.split_once('/')
                        .map(|(o, r)| (o.to_string(), r.to_string(), p.number))
                })
        })
}

/// The reviewer list for a PR. The author is left out: their reviews are
/// replies on their own PR, not reviews of it.
pub fn github_reviewers(
    overview: &er_engine::github::PrOverviewFull,
    reviews: &[er_engine::github::PrReview],
) -> Vec<GhReviewerSummary> {
    er_engine::github::pr_reviewers(&overview.author, &overview.review_requests, reviews)
        .into_iter()
        .map(|r| GhReviewerSummary {
            reviewer: r.reviewer,
            is_team: r.is_team,
            requested: r.requested,
            state: r.state,
        })
        .collect()
}

// ── Helpers ──────────────────────────────────────────────────────────────────

// ── Builder ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use er_service::app_records::sidebar::RECENTLY_MERGED_LIMIT;

    #[test]
    fn review_flow_snapshot_keeps_captured_task_identity_and_wire_status() {
        use er_engine::ai::review_flow::{
            FlowState, FlowTaskProgress, FlowTaskStage, FlowTaskStatus, ReviewFlow,
        };

        let mut flow = ReviewFlow::new("captured-hash");
        flow.state = FlowState::Failed {
            reason: "provider failed".into(),
        };
        flow.tasks.push(FlowTaskProgress {
            id: "finder:security".into(),
            stage: FlowTaskStage::Finding,
            lens: Some("security".into()),
            label: "Security review".into(),
            purpose: "Check permissions and sensitive data".into(),
            provider_id: "captured-provider".into(),
            provider_label: "Captured provider".into(),
            model_id: "captured-model".into(),
            model_label: "Captured model".into(),
            status: FlowTaskStatus::Failed,
            error: Some("provider failed".into()),
            background_task_id: Some("exact-background-task".into()),
            round: None,
            total_rounds: None,
        });
        let snapshot = ReviewFlowSnapshot::from_flow(&flow, false);
        let wire = serde_json::to_value(snapshot).unwrap();
        assert_eq!(wire["tasks"][0]["provider_id"], "captured-provider");
        assert_eq!(wire["tasks"][0]["model_id"], "captured-model");
        assert_eq!(wire["tasks"][0]["stage"], "finding");
        assert_eq!(wire["tasks"][0]["status"], "failed");
        assert_eq!(wire["tasks"][0]["error"], "provider failed");
        assert_eq!(
            wire["tasks"][0]["background_task_id"],
            "exact-background-task"
        );
    }

    fn pr_with(number: u64, head_ref: &str, state: &str) -> PrInfo {
        let mut p = minimal_pr_info(number, "t");
        p.head_ref = head_ref.to_string();
        p.state = state.to_string();
        p
    }

    #[test]
    fn recently_merged_keeps_more_than_the_first_page_newest_first() {
        let mut prs: Vec<PrInfo> = (1..=30)
            .map(|n| {
                let mut p = pr_with(n, "b", "MERGED");
                p.merged_at = Some(format!("2026-09-{n:02}T00:00:00Z"));
                p
            })
            .collect();
        prs.push(pr_with(99, "open", "OPEN"));
        prs.push(pr_with(98, "closed", "CLOSED"));

        let merged = recently_merged_prs(prs);
        let numbers: Vec<u64> = merged.iter().map(|p| p.number).collect();
        assert_eq!(numbers.len(), RECENTLY_MERGED_LIMIT);
        assert!(numbers.len() > 5, "Show more needs more than one page");
        assert_eq!(numbers[0], 30);
        assert!(numbers.windows(2).all(|w| w[0] > w[1]));
    }

    #[test]
    fn check_summary_carries_timing_and_duration() {
        let run = er_engine::github::CheckRun {
            name: "ci".into(),
            status: "COMPLETED".into(),
            conclusion: "SUCCESS".into(),
            url: None,
            started_at: Some("2026-10-30T10:00:00Z".into()),
            completed_at: Some("2026-10-30T10:01:52Z".into()),
            description: None,
        };
        let summary = CheckSummary::from(run);
        assert_eq!(summary.duration_secs, Some(112));
        assert_eq!(summary.started_at.as_deref(), Some("2026-10-30T10:00:00Z"));
    }
}
