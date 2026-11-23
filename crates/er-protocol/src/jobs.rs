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
    /// The background agent tasks queued, running or just finished, oldest
    /// first. Their output lines are read per task through `task_log`.
    pub tasks: Vec<TaskJob>,
    /// The sidebar triage requests still being prepared, before their task
    /// is queued, by remote and PR or branch.
    pub triage: Vec<String>,
    /// A refresh of the sidebar's PR lists is running.
    pub pr_list_refresh: bool,
}

/// One background agent task, as the running pill and the task panel list
/// it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TaskJob {
    pub id: String,
    /// `review`, `expert:<id>`, `professor`, `triage`, `tour`,
    /// `diagram:<kind>`, `importance`, or `command:<name>` for a tab
    /// command.
    pub kind: String,
    /// What the agent does.
    pub label: String,
    /// Where it does it: the branch or `owner/repo#n`.
    pub target_label: String,
    pub scope: String,
    pub repo_root: String,
    pub branch_label: String,
    #[ts(type = "number | null")]
    pub pr_number: Option<u64>,
    pub remote_repo: Option<String>,
    /// The bucket it writes to.
    pub er_dir: String,
    /// `queued`, `running`, `done` or `failed`.
    pub status: String,
    pub error: Option<String>,
    #[ts(type = "number")]
    pub started_at_ms: u64,
    #[ts(type = "number | null")]
    pub finished_at_ms: Option<u64>,
}

/// A task's output lines, oldest first.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TaskLog {
    pub lines: Vec<TaskLogLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TaskLogLine {
    pub command_name: String,
    /// `stdout`, `stderr` or `status`.
    pub source: String,
    pub text: String,
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
