use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ArenaRunJob;

/// The work in flight that the page shows while it runs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Jobs {
    /// The ask-AI replies being written, by thread id, or `finding:<id>`.
    pub ai_replies: Vec<AiReplyJob>,
    /// The arena runs every started workspace is running, by run id.
    pub arena_runs: Vec<ArenaRunJob>,
    /// The review flows every started workspace holds, by bucket.
    pub review_flows: Vec<ReviewFlowJob>,
}

/// One bucket's review flow, as the page's `ReviewFlowSnapshot` describes
/// it, in JSON text: its stage tasks are engine types the protocol does not
/// carry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ReviewFlowJob {
    /// The bucket the flow reviews into.
    pub er_dir: String,
    pub json: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AiReplyJob {
    pub target: String,
    /// `ask` or `elaborate`; none for a GitHub comment.
    pub ai_action: Option<String>,
    #[ts(type = "number")]
    pub started_at_ms: u64,
}
