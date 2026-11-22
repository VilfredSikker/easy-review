use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One tab's annotations, built on the server from its AI section: the AI
/// review card, the threads, the tour and what the other view's bucket holds.
/// Field names follow the snapshot's `ai`, `tour` and `other_bucket`, so
/// both reads have one shape. The in-flight ask-AI replies are in `jobs`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
pub struct Annotations {
    pub ai: AiAnnotations,
    pub tour: Tour,
    pub other_bucket: Option<OtherBucket>,
}

/// The AI section as the cards read it, counts and thread order included.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
pub struct AiAnnotations {
    pub fresh: bool,
    pub stale_reason: Option<String>,
    pub summary_markdown: Option<String>,
    pub agent_summaries: BTreeMap<String, String>,
    #[ts(type = "number")]
    pub high: u64,
    #[ts(type = "number")]
    pub med: u64,
    #[ts(type = "number")]
    pub low: u64,
    #[ts(type = "number")]
    pub local_comment_count: u64,
    #[ts(type = "number")]
    pub github_comment_count: u64,
    #[ts(type = "number")]
    pub comments: u64,
    #[ts(type = "number")]
    pub questions: u64,
    #[ts(type = "number")]
    pub notes: u64,
    /// Root threads: questions, then notes, then comments, each in file order.
    pub threads: Vec<Thread>,
    pub findings: Vec<Finding>,
    #[ts(type = "number")]
    pub arbiter_dropped: u64,
    #[ts(type = "number")]
    pub arbiter_merged: u64,
    #[ts(type = "number")]
    pub arbiter_unmatched: u64,
    #[ts(type = "number")]
    pub arbiter_regraded: u64,
    pub file_risks: Vec<FileRisk>,
    pub resolved_findings: Vec<Finding>,
    pub dropped_findings: Vec<Finding>,
    pub min_trust_default: String,
    pub review_created_at: Option<String>,
    pub review_diff_hash: Option<String>,
    pub has_review_json: bool,
    #[ts(type = "number")]
    pub eligible_comment_count: u64,
    pub triage: Option<Triage>,
    pub checklist: Option<Checklist>,
    pub diagrams: Vec<Diagram>,
    pub diagram_presets: Vec<DiagramPreset>,
}

/// A root thread with its replies, oldest first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct Thread {
    pub id: String,
    /// `comment`, `question` or `note`.
    pub kind: String,
    pub file: String,
    #[ts(type = "number")]
    pub line: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub line_end: Option<u64>,
    /// `LEFT` or `RIGHT`.
    pub side: String,
    /// `local` or `github`.
    pub source: String,
    pub stale: bool,
    pub resolved: bool,
    pub root: ThreadMessage,
    pub replies: Vec<ThreadMessage>,
    pub promoted_to: Option<String>,
    #[ts(type = "number | null")]
    pub github_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub hunk_index: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub old_line: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct ThreadMessage {
    pub id: String,
    pub author: String,
    /// `you`, `human` or `ai`.
    pub kind: String,
    pub timestamp: String,
    pub body_markdown: String,
    pub origin: Option<String>,
    pub source: Option<String>,
    pub editable: Option<bool>,
    pub deletable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub ai_action: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct Finding {
    pub id: String,
    pub file: String,
    #[ts(type = "number | null")]
    pub line: Option<u64>,
    #[ts(type = "number | null")]
    pub hunk_index: Option<u64>,
    /// `high`, `med` or `low`.
    pub severity: String,
    pub confidence: String,
    pub arbiter_verdict: Option<String>,
    pub arbiter_rationale: Option<String>,
    pub lens_category: String,
    pub resolved: bool,
    pub expert_label: Option<String>,
    pub agent_label: String,
    pub raised_by: Vec<String>,
    pub providers: Vec<String>,
    pub security_stage: bool,
    pub title: String,
    pub message_markdown: String,
    pub promoted_to: Option<String>,
    pub thread_id: Option<String>,
    pub responses: Vec<FindingResponse>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct FindingResponse {
    pub id: String,
    pub author: String,
    pub kind: String,
    pub timestamp: String,
    pub body_markdown: String,
    pub origin: String,
    pub editable: bool,
    pub deletable: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct FileRisk {
    pub path: String,
    pub risk: String,
    pub risk_reason: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct Triage {
    pub fresh: bool,
    pub first_impression: String,
    pub verdict_primary: String,
    pub experts: Vec<String>,
    pub rationale: String,
    pub confidence: String,
    pub priority_files: Vec<TriagePriorityFile>,
    pub files_changed: u32,
    pub approx_risk: String,
    pub domains: Vec<String>,
    pub reach: String,
    pub reach_reason: String,
    pub touch_points: Vec<String>,
    pub guard: Option<TriageGuard>,
    pub sensitive: Vec<TriageSensitiveReason>,
    pub created_at: String,
    pub diff_hash: String,
    /// The review's regrading of triage's call, once a review of this diff
    /// exists.
    pub review_update: Option<TriageReviewUpdate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TriagePriorityFile {
    pub path: String,
    pub reason: String,
    pub risk: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TriageGuard {
    pub kind: String,
    pub name: String,
    pub evidence: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TriageSensitiveReason {
    pub kind: String,
    pub evidence: String,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TriageReviewUpdate {
    /// `high`, `med` or `low`.
    pub approx_risk: String,
    /// `fix`, `check` or `done`.
    pub next: String,
    #[ts(type = "number")]
    pub high: u64,
    #[ts(type = "number")]
    pub med: u64,
    #[ts(type = "number")]
    pub low: u64,
    pub reviewed_by: Vec<String>,
    pub priority_files: Vec<TriagePriorityFile>,
    pub created_at: String,
}

/// The review checklist, in file order: an item's index is its toggle address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Checklist {
    pub items: Vec<ChecklistItem>,
    pub fresh: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ChecklistItem {
    pub id: String,
    pub text: String,
    pub category: String,
    pub checked: bool,
    pub related_findings: Vec<String>,
    pub related_files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Diagram {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub prompt: String,
    pub mermaid: String,
    pub fresh: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DiagramPreset {
    pub kind: String,
    pub label: String,
    pub description: String,
}

/// The guided tour for the view the tab shows.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Tour {
    pub available: bool,
    pub fresh: bool,
    /// `pr` or `branch`, set even with no tour.
    pub scope: String,
    pub title: String,
    pub overview_markdown: String,
    pub pillars: Vec<TourPillar>,
    pub created_at: String,
    pub diff_hash: String,
}

/// One pillar with its review progress, which counts the reviewed marks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TourPillar {
    pub id: String,
    pub title: String,
    pub description_markdown: String,
    pub summary_markdown: String,
    pub importance: u32,
    pub foundation: bool,
    pub files: Vec<TourFile>,
    #[ts(type = "number")]
    pub reviewed_count: u64,
    #[ts(type = "number")]
    pub total_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TourFile {
    pub path: String,
    pub reason: String,
    pub finding_ids: Vec<String>,
    pub related: Vec<TourRelatedFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TourRelatedFile {
    pub path: String,
    pub kind: String,
    pub reason: String,
}

/// What the other view of a local PR tab holds.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct OtherBucket {
    /// `pr` or `branch`.
    pub view: String,
    pub has_triage: bool,
    pub has_review: bool,
    #[ts(type = "number")]
    pub finding_count: u64,
    #[ts(type = "number")]
    pub pillar_count: u64,
    pub tour_shared: bool,
    pub same_diff: bool,
}

/// The browser annotations of a tab's view bucket.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
pub struct UiAnnotations {
    pub rows: Vec<UiAnnotation>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct UiAnnotation {
    pub id: String,
    pub url: String,
    pub selector: Option<String>,
    pub box_x: f64,
    pub box_y: f64,
    pub box_w: f64,
    pub box_h: f64,
    pub viewport_w: u32,
    pub viewport_h: u32,
    pub text: String,
    pub timestamp: String,
    pub author: String,
    pub screenshot_path: Option<String>,
    pub stale: bool,
    pub element_context: Option<String>,
    /// The DOM context as JSON text, which the page parses.
    pub dom_context_json: Option<String>,
}

/// The tab's review projected as the reviewer sees it, as pretty JSON.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ReviewJson {
    pub json: String,
}

/// The review revisions a tab's bucket holds: at most the current one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ReviewRevisions {
    pub revisions: Vec<ReviewRevision>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ReviewRevision {
    pub revision_id: String,
    pub created_at: String,
    pub scope: String,
    pub diff_hash: String,
    pub active: bool,
    pub agents: Vec<String>,
}

/// A saved annotation screenshot as a `data:` URL.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AnnotationScreenshot {
    pub data_url: String,
}
