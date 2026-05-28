//! Lightweight preemptive PR triage sidecar (`.er/triage.json`).

use serde::{Deserialize, Serialize};
use std::path::Path;

use super::loader::read_sidecar;

/// Overall triage verdict — whether a human or full AI review is warranted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TriageVerdict {
    #[default]
    Skip,
    Review,
    DeepReview,
}

impl TriageVerdict {
    pub fn as_str(&self) -> &'static str {
        match self {
            TriageVerdict::Skip => "skip",
            TriageVerdict::Review => "review",
            TriageVerdict::DeepReview => "deep_review",
        }
    }

    pub fn display_label(&self) -> &'static str {
        match self {
            TriageVerdict::Skip => "Skip — low risk",
            TriageVerdict::Review => "Review recommended",
            TriageVerdict::DeepReview => "Deep review recommended",
        }
    }
}

/// What kind of follow-up AI review to run, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RecommendedReview {
    #[default]
    None,
    General,
    Expert,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriageSmell {
    pub severity: String,
    pub category: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErTriage {
    pub version: u32,
    pub diff_hash: String,
    #[serde(default)]
    pub head_oid: String,
    #[serde(default)]
    pub created_at: String,
    pub verdict: TriageVerdict,
    #[serde(default)]
    pub confidence: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub smells: Vec<TriageSmell>,
    #[serde(default)]
    pub recommended_experts: Vec<String>,
    #[serde(default)]
    pub recommended_review: RecommendedReview,
}

impl ErTriage {
    pub fn path_in(er_dir: &str) -> String {
        format!("{er_dir}/triage.json")
    }
}

/// Load triage sidecar if present.
pub fn load_triage(er_dir: &str) -> Option<ErTriage> {
    let path = Path::new(er_dir).join("triage.json");
    let content = read_sidecar(&path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Format a concise inbox body from triage results.
pub fn format_triage_inbox_body(triage: &ErTriage, pr_title: &str) -> String {
    let mut lines = vec![
        format!("Verdict: {}", triage.verdict.display_label()),
        triage.summary.clone(),
    ];
    if !triage.smells.is_empty() {
        lines.push(String::new());
        lines.push("Top smells:".to_string());
        for smell in triage.smells.iter().take(3) {
            lines.push(format!("• [{}] {}", smell.severity, smell.text));
        }
    }
    if !triage.recommended_experts.is_empty() {
        lines.push(format!(
            "Suggested experts: {}",
            triage.recommended_experts.join(", ")
        ));
    }
    if pr_title.is_empty() {
        lines.join("\n")
    } else {
        format!("{}\n\n{}", pr_title, lines.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_triage_inbox_body_includes_verdict_and_smells() {
        let triage = ErTriage {
            version: 1,
            diff_hash: "abc".to_string(),
            head_oid: "deadbeef".to_string(),
            created_at: String::new(),
            verdict: TriageVerdict::DeepReview,
            confidence: "high".to_string(),
            summary: "Large auth change.".to_string(),
            smells: vec![TriageSmell {
                severity: "high".to_string(),
                category: "security".to_string(),
                text: "Missing auth check".to_string(),
            }],
            recommended_experts: vec!["security".to_string()],
            recommended_review: RecommendedReview::Expert,
        };
        let body = format_triage_inbox_body(&triage, "PR #42");
        assert!(body.contains("Deep review recommended"));
        assert!(body.contains("Missing auth check"));
        assert!(body.contains("security"));
    }
}
