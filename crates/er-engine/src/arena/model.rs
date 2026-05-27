use crate::ai::{EvidenceItem, RiskLevel};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Diff scope for an arena run (subset of app `DiffMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArenaScope {
    Branch,
    Unstaged,
    Staged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunStatus {
    Queued,
    Running {
        round: u8,
    },
    Complete,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReviewerKind {
    Model,
    Agent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReviewerRunStatus {
    Ok,
    Failed {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Kept,
    Escalated,
    Merged {
        into: String,
    },
    Dropped,
    Pending,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Vote {
    Propose,
    Keep,
    Drop,
    Merge,
    Escalate,
    Lower,
    Abstain,
    Flag,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewerRef {
    pub provider_id: String,
    pub model_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArenaConfig {
    pub reviewers: Vec<ReviewerRef>,
    pub rounds: u8,
    pub arbiter: ReviewerRef,
    pub auto_accept_threshold: f32,
    pub scope: ArenaScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reviewer {
    pub id: String,
    pub name: String,
    pub kind: ReviewerKind,
    pub provider_id: String,
    pub model_id: String,
    pub system_prompt: String,
    pub color: String,
    pub icon: String,
    pub tagline: String,
    pub cost_per_1k_in: f32,
    pub cost_per_1k_out: f32,
    pub avg_latency_ms: u32,
    pub status: ReviewerRunStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HumanOverride {
    pub verdict: Verdict,
    pub note: String,
    pub at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ballot {
    pub reviewer: String,
    pub vote: Vote,
    #[serde(default)]
    pub note: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_target: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundLog {
    pub n: u8,
    pub log: Vec<Ballot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArenaFinding {
    pub id: String,
    pub file: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    pub title: String,
    pub body: String,
    pub severity_by_round: BTreeMap<u8, RiskLevel>,
    pub raised_by: Vec<String>,
    pub verdict: Verdict,
    pub confidence: f32,
    pub rationale: String,
    pub rounds: Vec<RoundLog>,
    #[serde(default)]
    pub merge_candidates: Vec<String>,
    #[serde(default)]
    pub merged_children: Vec<ArenaFinding>,
    #[serde(default)]
    pub evidence: Vec<EvidenceItem>,
    #[serde(default, rename = "override", skip_serializing_if = "Option::is_none")]
    pub override_: Option<HumanOverride>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostEstimate {
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub usd: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArenaRun {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub branch_ref: String,
    pub base_branch: String,
    pub scope: ArenaScope,
    pub diff_hash: String,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    pub status: RunStatus,
    pub config: ArenaConfig,
    pub reviewers: Vec<Reviewer>,
    pub findings: Vec<ArenaFinding>,
    pub cost_estimate: CostEstimate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArenaRunSummary {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub branch_ref: String,
    pub status: RunStatus,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    pub reviewer_count: usize,
    pub finding_count: usize,
}

impl ArenaRun {
    pub fn summary(&self) -> ArenaRunSummary {
        ArenaRunSummary {
            id: self.id.clone(),
            title: self.title.clone(),
            branch_ref: self.branch_ref.clone(),
            status: self.status.clone(),
            created_at: self.created_at.clone(),
            completed_at: self.completed_at.clone(),
            reviewer_count: self.reviewers.len(),
            finding_count: self.findings.len(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatrixRow {
    pub finding_id: String,
    pub latest_vote: BTreeMap<String, Vote>,
    pub verdict: Verdict,
    pub confidence: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FunnelStage {
    Proposed,
    CrossChecked,
    Resolved,
    Final,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunnelCounts {
    pub proposed: usize,
    pub cross_checked: usize,
    pub resolved: usize,
    #[serde(rename = "final")]
    pub final_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunnelStages {
    pub counts: FunnelCounts,
    pub exited_at: BTreeMap<String, FunnelStage>,
}
