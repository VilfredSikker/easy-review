use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One reviewer the launcher picked: a provider's model, or an agent run on it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
pub struct ArenaReviewer {
    pub provider_id: String,
    pub model_id: String,
    pub agent_kind: Option<String>,
}

/// One agent's run in a batch: the models it runs on.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
pub struct ArenaAgentGroup {
    pub agent_kind: String,
    pub models: Vec<ArenaReviewer>,
    pub title: Option<String>,
}

/// What an arena run reviews and with whom. The launcher prices a plan and
/// then starts it. With `groups` empty it is one run of `reviewers`;
/// otherwise one run per group.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
pub struct ArenaPlan {
    /// `branch`, `staged` or `unstaged`.
    pub scope: String,
    pub files: Option<Vec<String>>,
    pub rounds: Option<u8>,
    pub arbiter: Option<ArenaReviewer>,
    pub reviewers: Vec<ArenaReviewer>,
    pub groups: Vec<ArenaAgentGroup>,
}

/// A plan's price before it starts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct ArenaEstimate {
    #[ts(type = "number")]
    pub diff_bytes: u64,
    pub cost_usd: f32,
    pub latency_sec: u32,
    pub cost_limit_usd: f32,
    /// The most rounds the engine runs, which the launcher offers as its
    /// maximum.
    pub max_rounds: u8,
}

/// Where a run is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum ArenaRunStatus {
    Queued,
    Running { round: u8 },
    Complete,
    Cancelled,
    Failed,
}

/// What a live run's reviewers are doing in its current round.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ArenaRunProgress {
    pub round: u8,
    pub total_rounds: u8,
    /// Empty during the reviewer rounds; `arbiter` while the arbiter rules.
    pub phase: String,
    pub thinking: Vec<String>,
    pub done: Vec<String>,
}

/// A run a workspace is running, as `jobs` lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ArenaRunJob {
    pub run_id: String,
    /// The bucket the run writes to.
    pub er_dir: String,
    pub status: ArenaRunStatus,
    pub progress: ArenaRunProgress,
}

/// One run's line in the arena's run list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ArenaRunRow {
    pub id: String,
    pub title: Option<String>,
    pub branch_ref: String,
    pub status: ArenaRunStatus,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub reviewer_count: u32,
    pub finding_count: u32,
}

/// A tab's bucket's runs on the tab's branch, newest first.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ArenaRuns {
    pub runs: Vec<ArenaRunRow>,
}

/// One run as the arena page draws it: its record, matrix, funnel, arbiter
/// and panel, as JSON text in the shape the page's arena types describe.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ArenaRun {
    pub json: String,
}
