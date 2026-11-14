//! The professor reviewer's sidecar type and merge.

use crate::artifact::Artifact;
use crate::review::{ErFileReview, ErReview, Finding, RiskLevel};
use std::collections::HashMap;

pub const PROFESSOR_ID: &str = "professor";
pub const PROFESSOR_LABEL: &str = "Professor";
pub const PROFESSOR_ID_PREFIX: &str = "prof";

/// `.er/professor.json` — teaching insights anchored to hunks.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ProfessorReview {
    pub version: u32,
    pub diff_hash: String,
    #[serde(default)]
    pub diff_scope: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub focus_prompt: String,
    /// 2–3 short markdown paragraphs on what the diff implements (teaching tone).
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub files: HashMap<String, ProfessorFileReview>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ProfessorFileReview {
    #[serde(default, deserialize_with = "super::review::lenient_findings")]
    pub findings: Vec<Finding>,
}

impl Artifact for ProfessorReview {
    const KIND: &'static str = "professor";
    const VERSION: u32 = 1;
    const FILE: &'static str = "professor.json";
    type Slot = ();

    fn diff_hash(&self) -> Option<&str> {
        Some(&self.diff_hash)
    }
}

fn prefix_finding_id(id: &str) -> String {
    if id.starts_with(&format!("{PROFESSOR_ID_PREFIX}-")) {
        id.to_string()
    } else {
        format!("{PROFESSOR_ID_PREFIX}-{id}")
    }
}

/// Merge professor insights into `review` (skip stale).
pub fn merge_professor_into_review(
    review: &mut ErReview,
    professor: &ProfessorReview,
    current_diff_hash: &str,
) {
    if professor.diff_hash != current_diff_hash {
        return;
    }
    for (path, pfr) in &professor.files {
        let entry = review
            .files
            .entry(path.clone())
            .or_insert_with(|| ErFileReview {
                risk: RiskLevel::Info,
                risk_reason: String::new(),
                summary: String::new(),
                findings: Vec::new(),
            });
        for mut finding in pfr.findings.clone() {
            finding.id = prefix_finding_id(&finding.id);
            finding.lens = PROFESSOR_ID.to_string();
            if finding.raised_by.is_empty() {
                finding.raised_by = vec![PROFESSOR_ID.to_string()];
            }
            entry.findings.push(finding);
        }
    }
}

/// Background task kind for professor runs.
pub fn professor_task_kind() -> String {
    PROFESSOR_ID.to_string()
}
