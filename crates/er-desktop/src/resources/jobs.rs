//! `jobs`: the work in flight the page shows while it runs. Its parts are
//! the ask-AI replies being written, which `AiReplyJobs` holds and counts;
//! the arena runs and review flows the started workspaces hold, which their
//! published views carry; and the background tasks and the sidebar triage
//! requests, which the task host holds and counts; and whether a PR-list
//! refresh runs, which its clock counts. A task's output lines
//! stay out: `task_log` reads them, so a line moves no version.

use er_protocol::{AiReplyJob, ArenaRunJob, Jobs, Resource, ReviewFlowJob};

use crate::state::DesktopState;

/// `jobs` at the count of its writes. The live parts are read once, so the
/// version names what the answer carries; a reply written during the read
/// is pushed again under a newer version.
pub fn jobs(desktop: &DesktopState) -> (u64, Resource) {
    let arena_runs = super::arena::live_runs(desktop);
    let review_flows = super::flows::live_flows(desktop);
    let version = counted(desktop, &arena_runs, &review_flows);
    let mut ai_replies: Vec<AiReplyJob> = desktop
        .pending_ai_replies
        .replies()
        .lock()
        .map(|replies| {
            replies
                .iter()
                .map(|(target, reply)| AiReplyJob {
                    target: target.clone(),
                    ai_action: reply.ai_action.map(str::to_string),
                    started_at_ms: reply.started_at_ms,
                })
                .collect()
        })
        .unwrap_or_default();
    ai_replies.sort_by(|a, b| a.target.cmp(&b.target));
    let mut triage: Vec<String> = desktop
        .auto_triage_in_flight
        .lock()
        .map(|queue| queue.iter().cloned().collect())
        .unwrap_or_default();
    triage.sort();
    let jobs = Jobs {
        ai_replies,
        arena_runs,
        review_flows,
        tasks: desktop.tasks.rows(),
        triage,
        pr_list_refresh: desktop
            .pr_list_running
            .load(std::sync::atomic::Ordering::Acquire),
    };
    (version, Resource::Jobs(jobs))
}

/// The version `jobs` answers at now. A live part that moved since the last
/// read moves it, so a progress tick is pushed and a diff publish is not.
pub fn version(desktop: &DesktopState) -> u64 {
    counted(
        desktop,
        &super::arena::live_runs(desktop),
        &super::flows::live_flows(desktop),
    )
}

/// The replies' writes plus the times the live runs or flows differed from
/// the last ones seen. Every read of the version counts here, so a fetch and
/// the relay never disagree.
fn counted(desktop: &DesktopState, arena_runs: &[ArenaRunJob], flows: &[ReviewFlowJob]) -> u64 {
    desktop.live_jobs.observe(arena_runs);
    desktop.pending_ai_replies.clock.revision()
        + desktop.live_jobs.observe_flows(flows)
        + desktop.tasks.clock.revision()
        + desktop.pr_list_clock.revision()
}
