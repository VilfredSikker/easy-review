use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The work in flight that the page shows while it runs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Jobs {
    /// The ask-AI replies being written, by thread id, or `finding:<id>`.
    pub ai_replies: Vec<AiReplyJob>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AiReplyJob {
    pub target: String,
    /// `ask` or `elaborate`; none for a GitHub comment.
    pub ai_action: Option<String>,
    #[ts(type = "number")]
    pub started_at_ms: u64,
}
