//! Specialized expert reviewers — registry and sidecar types.

pub use er_core::experts::{
    backfill_finding_lenses, expert_by_id, expert_hash_accepted, expert_id_for_id_prefix,
    expert_task_kind, lens_for_producer_id, parse_reviewer_kind, ExpertDef, ExpertFileReview,
    ExpertReview, ReviewerKind, EXPERTS,
};

#[cfg(test)]
use super::review::{ErReview, Finding, GENERAL_LENS};
#[cfg(test)]
use std::collections::HashMap;

/// Caps for findings in prompts (general vs expert).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FindingCaps {
    pub per_file: u32,
    pub total: u32,
    pub is_expert: bool,
}

impl FindingCaps {
    pub const fn general() -> Self {
        Self {
            per_file: 4,
            total: 15,
            is_expert: false,
        }
    }

    pub const fn expert() -> Self {
        Self {
            per_file: 2,
            total: 10,
            is_expert: true,
        }
    }

    pub const fn professor() -> Self {
        Self {
            per_file: 3,
            total: 12,
            is_expert: true,
        }
    }
}

/// What the `summary` field in an expert sidecar should cover (prompt + skills).
pub fn expert_summary_focus(expert_id: &str) -> &'static str {
    match expert_id {
        "security" => {
            "security posture of this diff: trust boundaries touched, authZ/authN, secrets, injection, and whether findings are blocking"
        }
        "performance" => {
            "performance impact: hot paths, allocations, blocking I/O, and whether the diff adds overhead or removes waste"
        }
        "reliability" => {
            "reliability: error handling, retries, timeouts, resource cleanup, and failure modes introduced or fixed"
        }
        "testing" => {
            "test coverage and quality: what is exercised, missing negative cases, and assertion strength"
        }
        "api" => {
            "API and contract impact: breaking changes, public surface changes, and semver implications"
        }
        "patterns" => {
            "consistency with existing patterns in the codebase — where the diff matches or diverges from established usage"
        }
        "simplifying" => {
            "readability and complexity: what is hard to follow, what should be simplified or documented, and review friction"
        }
        "mentorship" => {
            "exemplary patterns worth fostering (positive-only): what this diff does well and why it is worth emulating"
        }
        _ => "findings from this expert lens in 2–3 short paragraphs",
    }
}

pub fn expert_label_for_id(id: &str) -> Option<&'static str> {
    expert_by_id(id).map(|e| e.label)
}

/// Display label for a producer: an expert id, `professor`, or `triage`. Fed a
/// finding's `lens` and a background task's kind — the two places a producer id
/// is recorded. Never a finding's `category`, which names a kind of defect.
pub fn agent_label_for_id(id: &str) -> &'static str {
    if id == super::triage::TRIAGE_ID {
        return super::triage::TRIAGE_LABEL;
    }
    if id == super::professor::PROFESSOR_ID {
        return super::professor::PROFESSOR_LABEL;
    }
    if let Some(def) = expert_by_id(id) {
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
    let mut out = vec![
        ReviewerInfo {
            kind: super::triage::TRIAGE_ID.to_string(),
            label: super::triage::TRIAGE_LABEL.to_string(),
            description: "Fast branch scan — first impression and review routing".to_string(),
        },
        ReviewerInfo {
            kind: "general".to_string(),
            label: "General".to_string(),
            description: "Risk, order, checklist, and summary".to_string(),
        },
    ];
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

/// Load all `.er/experts/*.json` sidecars, sorted by expert id.
///
/// Slots the store rejects — hidden files and `experts/<id>.prev.json`
/// snapshots — are not expert files and never load.
pub fn load_expert_reviews(er_dir: &str) -> Vec<ExpertReview> {
    if er_dir.is_empty() {
        return Vec::new();
    }
    let store = crate::store_registry::store();
    let Ok(scope) = er_store::Scope::dir(er_dir) else {
        return Vec::new();
    };
    let Ok(slots) = store.list::<ExpertReview>(&scope) else {
        return Vec::new();
    };
    slots
        .into_iter()
        .filter_map(|slot| crate::store_registry::read_found::<ExpertReview>(er_dir, slot))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::review::{Confidence, ErFileReview, RiskLevel};

    /// A finding as an expert sidecar writes it: a defect kind, no lens (the
    /// merge assigns that), no line text.
    fn sample_finding(id: &str) -> Finding {
        Finding {
            id: id.to_string(),
            severity: RiskLevel::Medium,
            lens: String::new(),
            category: "correctness".to_string(),
            raised_by: Vec::new(),
            providers: Vec::new(),
            title: "Test".to_string(),
            description: String::new(),
            hunk_index: Some(0),
            line_start: Some(1),
            line_end: None,
            line_content: String::new(),
            stale: false,
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
    fn expert_files_are_listed_sorted_and_hidden_or_foreign_files_are_skipped() {
        let dir = tempfile::TempDir::new().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let experts = dir.path().join("experts");
            std::fs::create_dir_all(&experts).unwrap();
            for (slot, hash) in [("security", "hash-security"), ("api", "hash-api")] {
                crate::test_support::write_sidecar_json_slot::<ExpertReview>(
                    dir.path(),
                    er_store::ExpertId::new(slot).unwrap(),
                    serde_json::json!({"version":1,"expert_id":"x","diff_hash":hash,"files":{}}),
                );
            }
            for (name, hash) in [
                (".hidden.json", "hash-hidden"),
                ("security.prev.json", "hash-prev"),
                ("notes.txt", "hash-text"),
                (".security.json.tmp", "hash-temp"),
            ] {
                std::fs::write(
                    experts.join(name),
                    format!(r#"{{"version":1,"expert_id":"x","diff_hash":"{hash}","files":{{}}}}"#),
                )
                .unwrap();
            }

            let loaded = load_expert_reviews(dir.path().to_str().unwrap());

            let hashes: Vec<&str> = loaded
                .iter()
                .map(|expert| expert.diff_hash.as_str())
                .collect();
            assert_eq!(
                hashes,
                ["hash-api", "hash-security"],
                "only expert ids load, sorted by slot"
            );
        });
    }

    #[test]
    fn expert_file_written_by_an_agent_loads() {
        let dir = tempfile::TempDir::new().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            crate::test_support::write_sidecar_json_slot::<ExpertReview>(
                dir.path(),
                er_store::ExpertId::new("security").unwrap(),
                serde_json::json!({"version":1,"expert_id":"security","diff_hash":"hash","summary":"lens view","files":{"a.rs":{"findings":[{"id":"1","title":"t","severity":"medium","hunk_index":0}]}}}),
            );

            let loaded = load_expert_reviews(dir.path().to_str().unwrap());

            assert_eq!(loaded.len(), 1);
            assert_eq!(loaded[0].expert_id, "security");
            assert_eq!(loaded[0].files["a.rs"].findings.len(), 1);
        });
    }

    #[test]
    fn backfill_attributes_findings_written_before_lens_existed() {
        let mut review = ErReview {
            version: 1,
            diff_hash: "h".to_string(),
            created_at: String::new(),
            base_branch: String::new(),
            head_branch: String::new(),
            files: HashMap::from([(
                "src/a.rs".to_string(),
                ErFileReview {
                    risk: RiskLevel::Low,
                    risk_reason: String::new(),
                    summary: String::new(),
                    findings: vec![
                        sample_finding("sec-1"),
                        sample_finding("pat-2"),
                        sample_finding("prof-3"),
                        sample_finding("f-4"),
                    ],
                },
            )]),
            file_hashes: HashMap::new(),
        };
        // A finding already attributed by a merge path keeps that attribution
        // even when its id prefix says otherwise.
        review.files.get_mut("src/a.rs").unwrap().findings[0].lens = "api".to_string();

        backfill_finding_lenses(&mut review);

        let lenses: Vec<&str> = review.files["src/a.rs"]
            .findings
            .iter()
            .map(|f| f.lens.as_str())
            .collect();
        assert_eq!(lenses, vec!["api", "patterns", "professor", "general"]);
    }

    #[test]
    fn expert_summary_focus_non_empty_for_all_experts() {
        for e in EXPERTS {
            assert!(!expert_summary_focus(e.id).is_empty());
        }
    }

    /// Producer ids are task kinds; `lens` is the flat finding vocabulary. An
    /// unset kind or `triage` — which routes rather than producing findings —
    /// attributes to the general pass.
    #[test]
    fn lens_for_producer_id_normalizes_task_kinds() {
        assert_eq!(lens_for_producer_id("expert:security"), "security");
        assert_eq!(lens_for_producer_id("expert:simplifying"), "simplifying");
        assert_eq!(lens_for_producer_id("professor"), "professor");
        assert_eq!(lens_for_producer_id("general"), GENERAL_LENS);
        assert_eq!(lens_for_producer_id("triage"), GENERAL_LENS);
        assert_eq!(lens_for_producer_id(""), GENERAL_LENS);
        // An expert task kind with an id no longer in the registry.
        assert_eq!(lens_for_producer_id("expert:retired"), GENERAL_LENS);
    }

    #[test]
    fn agent_label_maps_general_expert_professor() {
        assert_eq!(agent_label_for_id("security"), "Security");
        assert_eq!(agent_label_for_id("professor"), "Professor");
        assert_eq!(agent_label_for_id("logic"), "General");
    }

    #[test]
    fn parse_reviewer_kind_accepts_all_kinds() {
        assert_eq!(parse_reviewer_kind("general"), Some(ReviewerKind::General));
        assert_eq!(
            parse_reviewer_kind("expert:api"),
            Some(ReviewerKind::Expert("api".to_string()))
        );
        assert_eq!(
            parse_reviewer_kind("professor"),
            Some(ReviewerKind::Professor)
        );
        assert_eq!(parse_reviewer_kind("triage"), Some(ReviewerKind::Triage));
        assert!(parse_reviewer_kind("expert:unknown").is_none());
    }

    #[test]
    fn list_ai_reviewers_includes_general_and_professor() {
        let list = list_ai_reviewers();
        assert!(list.iter().any(|r| r.kind == "general"));
        assert!(list.iter().any(|r| r.kind == "professor"));
        assert!(list.first().is_some_and(|r| r.kind == "triage"));
        assert_eq!(list.len(), EXPERTS.len() + 3);
    }
}
