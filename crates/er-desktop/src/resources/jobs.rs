//! `jobs`: the work in flight the page shows while it runs. Its parts are
//! the ask-AI replies being written, which `AiReplyJobs` holds and counts,
//! and the arena runs the started workspaces are running, which their
//! published views hold; the flow and AI runs join it later.

use er_protocol::{AiReplyJob, ArenaRunJob, Jobs, Resource};

use crate::state::DesktopState;

/// `jobs` at the count of its writes. The live runs are read once, so the
/// version names what the answer carries; a reply written during the read
/// is pushed again under a newer version.
pub fn jobs(desktop: &DesktopState) -> (u64, Resource) {
    let arena_runs = super::arena::live_runs(desktop);
    let version =
        desktop.pending_ai_replies.clock.revision() + desktop.live_jobs.observe(&arena_runs);
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
    let jobs = Jobs {
        ai_replies,
        arena_runs,
    };
    (version, Resource::Jobs(jobs))
}

/// The version `jobs` answers at now. A live part that moved since the last
/// read moves it, so a progress tick is pushed and a diff publish is not.
pub fn version(desktop: &DesktopState) -> u64 {
    let arena_runs: Vec<ArenaRunJob> = super::arena::live_runs(desktop);
    desktop.pending_ai_replies.clock.revision() + desktop.live_jobs.observe(&arena_runs)
}
