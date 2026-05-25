//! Specialized expert reviewers — registry, sidecar types, merge into general review.

use super::review::{ErFileReview, ErReview, Finding, RiskLevel};
use std::collections::HashMap;
use std::path::Path;

/// Caps for findings in prompts (general vs expert).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FindingCaps {
    pub per_file: u32,
    pub total: u32,
    pub is_expert: bool,
}

impl FindingCaps {
    pub fn general() -> Self {
        Self {
            per_file: 4,
            total: 15,
            is_expert: false,
        }
    }

    pub fn expert() -> Self {
        Self {
            per_file: 2,
            total: 10,
            is_expert: true,
        }
    }
}

/// One registered expert reviewer (v1 hardcoded list).
#[derive(Debug, Clone, Copy)]
pub struct ExpertDef {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub skill_name: &'static str,
    /// Short prefix for finding ids (`sec-1`, `pat-2`).
    pub id_prefix: &'static str,
}

pub const EXPERTS: &[ExpertDef] = &[
    ExpertDef {
        id: "security",
        label: "Security",
        description: "AuthZ/authN, injection, secrets, unsafe defaults",
        skill_name: "er-review-security",
        id_prefix: "sec",
    },
    ExpertDef {
        id: "performance",
        label: "Performance",
        description: "Hot paths, allocations, blocking I/O, unnecessary work",
        skill_name: "er-review-performance",
        id_prefix: "perf",
    },
    ExpertDef {
        id: "reliability",
        label: "Reliability",
        description: "Error handling, retries, timeouts, resource cleanup",
        skill_name: "er-review-reliability",
        id_prefix: "rel",
    },
    ExpertDef {
        id: "testing",
        label: "Testing",
        description: "Assertion quality, missing negative cases",
        skill_name: "er-review-testing",
        id_prefix: "tst",
    },
    ExpertDef {
        id: "api",
        label: "API / contracts",
        description: "Breaking changes, public surface, semver impact",
        skill_name: "er-review-api",
        id_prefix: "api",
    },
    ExpertDef {
        id: "patterns",
        label: "Patterns",
        description: "Consistency with existing code in the same module/package",
        skill_name: "er-review-patterns",
        id_prefix: "pat",
    },
    ExpertDef {
        id: "simplifying",
        label: "Simplifying",
        description: "Hard-to-read complexity — simplify or document with comments",
        skill_name: "er-review-simplifying",
        id_prefix: "simp",
    },
    ExpertDef {
        id: "mentorship",
        label: "Mentorship",
        description: "Exemplary patterns and quality worth fostering on the team",
        skill_name: "er-review-mentorship",
        id_prefix: "ment",
    },
];

pub fn expert_by_id(id: &str) -> Option<&'static ExpertDef> {
    EXPERTS.iter().find(|e| e.id == id)
}

pub fn expert_label_for_category(category: &str) -> Option<&'static str> {
    expert_by_id(category).map(|e| e.label)
}

/// Display label for the agent that produced a finding (pill in UI).
pub fn agent_label_for_category(category: &str) -> &'static str {
    if category == super::professor::PROFESSOR_ID {
        return super::professor::PROFESSOR_LABEL;
    }
    if let Some(def) = expert_by_id(category) {
        return def.label;
    }
    "General"
}

/// One selectable reviewer in the AI Hub / file-review picker.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ReviewerInfo {
    pub kind: String,
    pub label: String,
    pub description: String,
}

pub fn list_ai_reviewers() -> Vec<ReviewerInfo> {
    let mut out = vec![ReviewerInfo {
        kind: "general".to_string(),
        label: "General".to_string(),
        description: "Risk, order, checklist, and summary".to_string(),
    }];
    for e in EXPERTS {
        out.push(ReviewerInfo {
            kind: format!("expert:{}", e.id),
            label: e.label.to_string(),
            description: e.description.to_string(),
        });
    }
    out.push(ReviewerInfo {
        kind: super::professor::PROFESSOR_ID.to_string(),
        label: super::professor::PROFESSOR_LABEL.to_string(),
        description: "Learn the implementation — key mechanisms in this diff".to_string(),
    });
    out
}

/// Parse `reviewer_kind` from UI/API.
pub fn parse_reviewer_kind(kind: &str) -> Option<ReviewerKind> {
    match kind {
        "general" => Some(ReviewerKind::General),
        "professor" => Some(ReviewerKind::Professor),
        s if s.starts_with("expert:") => {
            let id = s.strip_prefix("expert:")?;
            expert_by_id(id).map(|_| ReviewerKind::Expert(id.to_string()))
        }
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewerKind {
    General,
    Expert(String),
    Professor,
}

/// Desktop / Tauri list payload.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ExpertInfo {
    pub id: String,
    pub label: String,
    pub description: String,
}

pub fn list_expert_info() -> Vec<ExpertInfo> {
    EXPERTS
        .iter()
        .map(|e| ExpertInfo {
            id: e.id.to_string(),
            label: e.label.to_string(),
            description: e.description.to_string(),
        })
        .collect()
}

/// `.er/experts/{id}.json` — findings only (no order/checklist/summary).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExpertReview {
    pub version: u32,
    pub expert_id: String,
    pub diff_hash: String,
    #[serde(default)]
    pub diff_scope: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub files: HashMap<String, ExpertFileReview>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExpertFileReview {
    #[serde(default)]
    pub findings: Vec<Finding>,
}

const MAX_SIDECAR_BYTES: u64 = 10_000_000;

fn read_expert_sidecar(path: &Path) -> Option<ExpertReview> {
    let metadata = std::fs::metadata(path).ok()?;
    if metadata.len() > MAX_SIDECAR_BYTES {
        return None;
    }
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Load all `.er/experts/*.json` sidecars.
pub fn load_expert_reviews(er_dir: &str) -> Vec<ExpertReview> {
    let experts_dir = Path::new(er_dir).join("experts");
    let Ok(entries) = std::fs::read_dir(&experts_dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if let Some(review) = read_expert_sidecar(&path) {
            out.push(review);
        }
    }
    out
}

fn prefix_finding_id(prefix: &str, id: &str) -> String {
    if id.starts_with(&format!("{prefix}-")) {
        id.to_string()
    } else {
        format!("{prefix}-{id}")
    }
}

/// Merge expert findings into `review` at load time (skip stale expert files).
pub fn merge_experts_into_review(
    review: &mut ErReview,
    experts: &[ExpertReview],
    current_diff_hash: &str,
) {
    for expert in experts {
        if expert.diff_hash != current_diff_hash {
            continue;
        }
        let Some(def) = expert_by_id(&expert.expert_id) else {
            continue;
        };
        for (path, efr) in &expert.files {
            let entry = review.files.entry(path.clone()).or_insert_with(|| ErFileReview {
                risk: RiskLevel::Info,
                risk_reason: String::new(),
                summary: String::new(),
                findings: Vec::new(),
            });
            for mut finding in efr.findings.clone() {
                finding.id = prefix_finding_id(def.id_prefix, &finding.id);
                finding.category = def.id.to_string();
                entry.findings.push(finding);
            }
        }
    }
}

/// Build a minimal `ErReview` shell when only expert sidecars exist.
pub fn synthesize_review_from_experts(
    experts: &[ExpertReview],
    current_diff_hash: &str,
) -> Option<ErReview> {
    let fresh: Vec<&ExpertReview> = experts
        .iter()
        .filter(|e| e.diff_hash == current_diff_hash)
        .collect();
    if fresh.is_empty() {
        return None;
    }
    let first = fresh[0];
    let mut review = ErReview {
        version: 1,
        diff_hash: current_diff_hash.to_string(),
        created_at: first.created_at.clone(),
        base_branch: String::new(),
        head_branch: String::new(),
        files: HashMap::new(),
        file_hashes: HashMap::new(),
    };
    merge_experts_into_review(&mut review, experts, current_diff_hash);
    if review.files.is_empty() {
        return None;
    }
    Some(review)
}

/// Background task kind label, e.g. `expert:security`.
pub fn expert_task_kind(expert_id: &str) -> String {
    format!("expert:{expert_id}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::review::{Confidence, RiskLevel};

    fn sample_finding(id: &str) -> Finding {
        Finding {
            id: id.to_string(),
            severity: RiskLevel::Medium,
            category: "security".to_string(),
            title: "Test".to_string(),
            description: String::new(),
            hunk_index: Some(0),
            line_start: Some(1),
            line_end: None,
            suggestion: String::new(),
            related_files: vec![],
            outside_diff: false,
            confidence: Confidence::Confirmed,
            verification_plan: String::new(),
            evidence: vec![],
            responses: vec![],
            resolved: false,
            resolved_note: String::new(),
            resolved_at: String::new(),
            promoted_to: None,
        }
    }

    #[test]
    fn merge_prefixes_ids_and_skips_stale() {
        let hash = "abc123";
        let mut review = ErReview {
            version: 1,
            diff_hash: hash.to_string(),
            created_at: String::new(),
            base_branch: String::new(),
            head_branch: String::new(),
            files: HashMap::new(),
            file_hashes: HashMap::new(),
        };
        let experts = vec![
            ExpertReview {
                version: 1,
                expert_id: "security".to_string(),
                diff_hash: hash.to_string(),
                diff_scope: "branch".to_string(),
                created_at: String::new(),
                files: HashMap::from([(
                    "src/a.rs".to_string(),
                    ExpertFileReview {
                        findings: vec![sample_finding("1")],
                    },
                )]),
            },
            ExpertReview {
                version: 1,
                expert_id: "patterns".to_string(),
                diff_hash: "stale".to_string(),
                diff_scope: String::new(),
                created_at: String::new(),
                files: HashMap::from([(
                    "src/b.rs".to_string(),
                    ExpertFileReview {
                        findings: vec![sample_finding("9")],
                    },
                )]),
            },
        ];
        merge_experts_into_review(&mut review, &experts, hash);
        assert_eq!(review.files.len(), 1);
        let f = &review.files["src/a.rs"];
        assert_eq!(f.findings.len(), 1);
        assert_eq!(f.findings[0].id, "sec-1");
        assert_eq!(f.findings[0].category, "security");
    }

    #[test]
    fn agent_label_maps_general_expert_professor() {
        assert_eq!(agent_label_for_category("security"), "Security");
        assert_eq!(agent_label_for_category("professor"), "Professor");
        assert_eq!(agent_label_for_category("logic"), "General");
    }

    #[test]
    fn parse_reviewer_kind_accepts_all_kinds() {
        assert_eq!(parse_reviewer_kind("general"), Some(ReviewerKind::General));
        assert_eq!(
            parse_reviewer_kind("expert:api"),
            Some(ReviewerKind::Expert("api".to_string()))
        );
        assert_eq!(parse_reviewer_kind("professor"), Some(ReviewerKind::Professor));
        assert!(parse_reviewer_kind("expert:unknown").is_none());
    }

    #[test]
    fn list_ai_reviewers_includes_general_and_professor() {
        let list = list_ai_reviewers();
        assert!(list.iter().any(|r| r.kind == "general"));
        assert!(list.iter().any(|r| r.kind == "professor"));
        assert_eq!(list.len(), EXPERTS.len() + 2);
    }

    #[test]
    fn merge_adds_to_existing_file_entry() {
        let hash = "h";
        let mut review = ErReview {
            version: 1,
            diff_hash: hash.to_string(),
            created_at: String::new(),
            base_branch: String::new(),
            head_branch: String::new(),
            files: HashMap::from([(
                "x.rs".to_string(),
                ErFileReview {
                    risk: RiskLevel::Low,
                    risk_reason: String::new(),
                    summary: "s".to_string(),
                    findings: vec![sample_finding("f-1")],
                },
            )]),
            file_hashes: HashMap::new(),
        };
        let experts = vec![ExpertReview {
            version: 1,
            expert_id: "api".to_string(),
            diff_hash: hash.to_string(),
            diff_scope: String::new(),
            created_at: String::new(),
            files: HashMap::from([(
                "x.rs".to_string(),
                ExpertFileReview {
                    findings: vec![sample_finding("2")],
                },
            )]),
        }];
        merge_experts_into_review(&mut review, &experts, hash);
        assert_eq!(review.files["x.rs"].findings.len(), 2);
        assert!(review.files["x.rs"].findings.iter().any(|f| f.id == "api-2"));
    }
}
