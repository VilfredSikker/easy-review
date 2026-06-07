//! Triage reviewer — fast branch scan in `.er/triage.json` (routing verdict, not findings).

use std::path::Path;

pub const TRIAGE_ID: &str = "triage";
pub const TRIAGE_LABEL: &str = "Triage";
pub const TRIAGE_SKILL: &str = "er-triage";

/// Recommended next review step from triage.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TriageVerdictPrimary {
    #[default]
    General,
    Expert,
    Arena,
    Professor,
    Skip,
}

/// `.er/triage.json` — first impression + routing verdict + priority files.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TriageReview {
    pub version: u32,
    pub diff_hash: String,
    #[serde(default)]
    pub diff_scope: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub first_impression: String,
    #[serde(default)]
    pub diff_stats: TriageDiffStats,
    #[serde(default)]
    pub verdict: TriageVerdict,
    #[serde(default)]
    pub priority_files: Vec<TriagePriorityFile>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct TriageDiffStats {
    #[serde(default)]
    pub files_changed: u32,
    #[serde(default)]
    pub approx_risk: String,
    #[serde(default)]
    pub domains: Vec<String>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct TriageVerdict {
    #[serde(default)]
    pub primary: TriageVerdictPrimary,
    #[serde(default)]
    pub experts: Vec<String>,
    #[serde(default)]
    pub rationale: String,
    #[serde(default)]
    pub confidence: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TriagePriorityFile {
    pub path: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub risk: String,
}

const MAX_SIDECAR_BYTES: u64 = 10_000_000;

fn read_triage_sidecar(path: &Path) -> Option<TriageReview> {
    let metadata = std::fs::metadata(path).ok()?;
    if metadata.len() > MAX_SIDECAR_BYTES {
        return None;
    }
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

pub fn load_triage_review(er_dir: &str) -> Option<TriageReview> {
    read_triage_sidecar(&Path::new(er_dir).join("triage.json"))
}

pub fn triage_is_fresh(triage: &TriageReview, current_diff_hash: &str) -> bool {
    triage.diff_hash == current_diff_hash
}

/// Background task kind for triage runs.
pub fn triage_task_kind() -> String {
    TRIAGE_ID.to_string()
}

pub fn verdict_primary_str(v: &TriageVerdictPrimary) -> &'static str {
    match v {
        TriageVerdictPrimary::General => "general",
        TriageVerdictPrimary::Expert => "expert",
        TriageVerdictPrimary::Arena => "arena",
        TriageVerdictPrimary::Professor => "professor",
        TriageVerdictPrimary::Skip => "skip",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_sample_triage_json() {
        let json = r#"{
            "version": 1,
            "diff_hash": "abc123",
            "diff_scope": "branch",
            "created_at": "2026-01-01T00:00:00Z",
            "first_impression": "Auth refactor touches session handling.",
            "diff_stats": {
                "files_changed": 12,
                "approx_risk": "medium",
                "domains": ["auth", "api"]
            },
            "verdict": {
                "primary": "expert",
                "experts": ["security"],
                "rationale": "New trust boundary in middleware",
                "confidence": "high"
            },
            "priority_files": [
                { "path": "src/auth.rs", "reason": "Session validation", "risk": "high" }
            ]
        }"#;
        let triage: TriageReview = serde_json::from_str(json).unwrap();
        assert_eq!(triage.verdict.primary, TriageVerdictPrimary::Expert);
        assert_eq!(triage.verdict.experts, vec!["security"]);
        assert_eq!(triage.priority_files.len(), 1);
        assert!(triage_is_fresh(&triage, "abc123"));
        assert!(!triage_is_fresh(&triage, "other"));
    }
}
