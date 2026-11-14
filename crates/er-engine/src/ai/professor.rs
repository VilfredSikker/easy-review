//! Professor learning agent — `.er/professor.json` sidecar and merge into review.

pub use er_core::professor::{
    merge_professor_into_review, professor_task_kind, ProfessorFileReview, ProfessorReview,
    PROFESSOR_ID, PROFESSOR_ID_PREFIX, PROFESSOR_LABEL,
};

/// What the `summary` field in professor.json should cover (prompt + skills).
pub const PROFESSOR_SUMMARY_FOCUS: &str = "what this diff implements for a skilled developer: purpose, architecture, data flow, and 1–2 non-obvious mechanisms (teaching tone, not a bug list)";

pub fn load_professor_review(er_dir: &str) -> Option<ProfessorReview> {
    crate::store_registry::read_found::<ProfessorReview>(er_dir, ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::review::{Confidence, ErReview, Finding, RiskLevel};
    use std::collections::HashMap;

    fn sample_insight(id: &str) -> Finding {
        Finding {
            id: id.to_string(),
            severity: RiskLevel::Info,
            lens: String::new(),
            category: String::new(),
            raised_by: Vec::new(),
            providers: Vec::new(),
            title: "State machine".to_string(),
            description: "Explains transition".to_string(),
            hunk_index: Some(0),
            line_start: Some(10),
            line_end: None,
            line_content: String::new(),
            stale: false,
            suggestion: String::new(),
            related_files: vec![],
            outside_diff: false,
            confidence: Confidence::Informational,
            verification_plan: String::new(),
            evidence: vec![],
            responses: vec![],
            resolved: false,
            resolved_note: String::new(),
            resolved_at: String::new(),
            promoted_to: None,
        }
    }

    /// The professor prompt does not ask for `category`, so a sidecar omits it.
    /// That must still load.
    #[test]
    fn sidecar_without_a_category_deserializes() {
        let json = r#"{
            "version": 1,
            "diff_hash": "h",
            "files": {
                "src/lib.rs": {
                    "findings": [{ "id": "prof-1", "severity": "info", "title": "State machine" }]
                }
            }
        }"#;
        let review: ProfessorReview = serde_json::from_str(json).unwrap();
        let f = &review.files["src/lib.rs"].findings[0];
        assert_eq!(f.category, "");
        assert_eq!(f.lens, "");
    }

    #[test]
    fn merge_prefixes_and_skips_stale() {
        let hash = "abc";
        let mut review = ErReview {
            version: 1,
            diff_hash: hash.to_string(),
            created_at: String::new(),
            base_branch: String::new(),
            head_branch: String::new(),
            files: HashMap::new(),
            file_hashes: HashMap::new(),
        };
        let prof = ProfessorReview {
            version: 1,
            diff_hash: hash.to_string(),
            diff_scope: "branch".to_string(),
            created_at: String::new(),
            focus_prompt: String::new(),
            summary: String::new(),
            files: HashMap::from([(
                "src/lib.rs".to_string(),
                ProfessorFileReview {
                    findings: vec![sample_insight("1")],
                },
            )]),
        };
        merge_professor_into_review(&mut review, &prof, hash);
        assert_eq!(review.files["src/lib.rs"].findings[0].id, "prof-1");
        assert_eq!(review.files["src/lib.rs"].findings[0].lens, "professor");
        // Teaching insights carry no defect kind, and `category` must never
        // double as the producer name.
        assert_eq!(review.files["src/lib.rs"].findings[0].category, "");

        let stale = ProfessorReview {
            diff_hash: "old".to_string(),
            ..prof
        };
        review.files.clear();
        merge_professor_into_review(&mut review, &stale, hash);
        assert!(review.files.is_empty());
    }

    #[test]
    fn professor_file_written_by_an_agent_loads() {
        let dir = tempfile::TempDir::new().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            crate::test_support::write_sidecar_json::<ProfessorReview>(
                dir.path(),
                serde_json::json!({"version":1,"diff_hash":"hash","summary":"what the diff implements","files":{"a.rs":{"findings":[]}}}),
            );

            let professor =
                load_professor_review(dir.path().to_str().unwrap()).expect("the agent's professor");

            assert_eq!(professor.summary, "what the diff implements");
            assert!(professor.files.contains_key("a.rs"));
        });
    }

    #[test]
    fn a_professor_file_for_another_version_reads_as_absent() {
        let dir = tempfile::TempDir::new().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            std::fs::write(
                dir.path().join("professor.json"),
                r#"{"version":2,"diff_hash":"hash","summary":"from the future"}"#,
            )
            .unwrap();

            assert!(load_professor_review(dir.path().to_str().unwrap()).is_none());
        });
    }
}
