//! `jobs`: the work in flight the page shows while it runs. Its first part is
//! the ask-AI replies being written, which `AiReplyJobs` holds and counts;
//! the arena, flow and AI runs join it later.

use er_protocol::{AiReplyJob, Jobs, Resource};

use crate::state::DesktopState;

/// `jobs` at the count of its writes. The count is read first, so a write
/// landing during the read is pushed again under a newer version.
pub fn jobs(desktop: &DesktopState) -> (u64, Resource) {
    let version = version(desktop);
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
    (version, Resource::Jobs(Jobs { ai_replies }))
}

pub fn version(desktop: &DesktopState) -> u64 {
    desktop.pending_ai_replies.clock.revision()
}
