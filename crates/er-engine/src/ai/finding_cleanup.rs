//! Persisted removal of findings and linked validation threads.

use super::comments::{ErGitHubComments, ErNotes, ErQuestions};
use super::experts::{expert_by_id, load_expert_reviews, ExpertReview};
use super::professor::{load_professor_review, PROFESSOR_ID_PREFIX};
use super::review::AiState;
use super::review::ErReview;
use std::path::Path;

/// Root thread id (question or github comment) linked to a finding via `finding_ref`.
pub fn find_finding_thread_root(ai: &AiState, finding_id: &str) -> Option<String> {
    if let Some(qs) = ai.questions.as_ref() {
        if let Some(q) = qs
            .questions
            .iter()
            .find(|q| q.finding_ref.as_deref() == Some(finding_id) && q.in_reply_to.is_none())
        {
            return Some(q.id.clone());
        }
    }
    if let Some(ns) = ai.notes.as_ref() {
        if let Some(n) = ns
            .notes
            .iter()
            .find(|n| n.finding_ref.as_deref() == Some(finding_id) && n.in_reply_to.is_none())
        {
            return Some(n.id.clone());
        }
    }
    if let Some(gc) = ai.github_comments.as_ref() {
        if let Some(c) = gc
            .comments
            .iter()
            .find(|c| c.finding_ref.as_deref() == Some(finding_id) && c.in_reply_to.is_none())
        {
            return Some(c.id.clone());
        }
    }
    None
}

fn matches_finding_id(stored_id: &str, target_id: &str, id_prefix: Option<&str>) -> bool {
    if stored_id == target_id {
        return true;
    }
    let Some(prefix) = id_prefix else {
        return false;
    };
    let prefixed = format!("{prefix}-{stored_id}");
    prefixed == target_id
        || stored_id == target_id.strip_prefix(&format!("{prefix}-")).unwrap_or("")
}

fn retain_findings(
    findings: &mut Vec<super::review::Finding>,
    target_id: &str,
    id_prefix: Option<&str>,
) -> bool {
    let before = findings.len();
    findings.retain(|f| !matches_finding_id(&f.id, target_id, id_prefix));
    findings.len() < before
}

fn write_json_atomic<T: serde::Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(value)?;
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)
}

/// Remove a finding from `review.json`, `professor.json`, and `experts/*.json`. Returns true if any file changed.
pub fn remove_finding_from_sidecars(er_dir: &str, finding_id: &str) -> std::io::Result<bool> {
    let er = Path::new(er_dir);
    let mut changed = false;

    let review_path = er.join("review.json");
    if review_path.is_file() {
        if let Ok(content) = std::fs::read_to_string(&review_path) {
            if let Ok(mut review) = serde_json::from_str::<ErReview>(&content) {
                let mut file_changed = false;
                for fr in review.files.values_mut() {
                    if retain_findings(&mut fr.findings, finding_id, None) {
                        file_changed = true;
                    }
                }
                if file_changed {
                    write_json_atomic(&review_path, &review)?;
                    changed = true;
                }
            }
        }
    }

    let prof_path = er.join("professor.json");
    if prof_path.is_file() {
        if let Some(mut prof) = load_professor_review(er_dir) {
            let mut file_changed = false;
            for pfr in prof.files.values_mut() {
                if retain_findings(&mut pfr.findings, finding_id, Some(PROFESSOR_ID_PREFIX)) {
                    file_changed = true;
                }
            }
            if file_changed {
                write_json_atomic(&prof_path, &prof)?;
                changed = true;
            }
        }
    }

    for expert in load_expert_reviews(er_dir) {
        let prefix = expert_by_id(&expert.expert_id).map(|d| d.id_prefix);
        let path = er
            .join("experts")
            .join(format!("{}.json", expert.expert_id));
        if !path.is_file() {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(mut er) = serde_json::from_str::<ExpertReview>(&content) {
                let mut file_changed = false;
                for efr in er.files.values_mut() {
                    if retain_findings(&mut efr.findings, finding_id, prefix) {
                        file_changed = true;
                    }
                }
                if file_changed {
                    write_json_atomic(&path, &er)?;
                    changed = true;
                }
            }
        }
    }

    Ok(changed)
}

/// Delete question/github threads created for finding validation (`finding_ref`).
pub fn delete_threads_linked_to_finding(er_dir: &str, finding_id: &str) -> std::io::Result<bool> {
    let er = Path::new(er_dir);
    let mut changed = false;

    let q_path = er.join("questions.json");
    if q_path.is_file() {
        if let Ok(content) = std::fs::read_to_string(&q_path) {
            if let Ok(mut qs) = serde_json::from_str::<ErQuestions>(&content) {
                let roots: Vec<String> = qs
                    .questions
                    .iter()
                    .filter(|q| {
                        q.finding_ref.as_deref() == Some(finding_id) && q.in_reply_to.is_none()
                    })
                    .map(|q| q.id.clone())
                    .collect();
                if !roots.is_empty() {
                    let root_set: std::collections::HashSet<&str> =
                        roots.iter().map(|s| s.as_str()).collect();
                    qs.questions.retain(|q| {
                        if root_set.contains(q.id.as_str()) {
                            return false;
                        }
                        if let Some(parent) = q.in_reply_to.as_deref() {
                            if root_set.contains(parent) {
                                return false;
                            }
                        }
                        q.finding_ref.as_deref() != Some(finding_id)
                    });
                    write_json_atomic(&q_path, &qs)?;
                    changed = true;
                }
            }
        }
    }

    let notes_path = er.join("notes.json");
    if notes_path.is_file() {
        if let Ok(content) = std::fs::read_to_string(&notes_path) {
            if let Ok(mut ns) = serde_json::from_str::<ErNotes>(&content) {
                let roots: Vec<String> = ns
                    .notes
                    .iter()
                    .filter(|n| {
                        n.finding_ref.as_deref() == Some(finding_id) && n.in_reply_to.is_none()
                    })
                    .map(|n| n.id.clone())
                    .collect();
                if !roots.is_empty() {
                    let root_set: std::collections::HashSet<&str> =
                        roots.iter().map(|s| s.as_str()).collect();
                    ns.notes.retain(|n| {
                        if root_set.contains(n.id.as_str()) {
                            return false;
                        }
                        if let Some(parent) = n.in_reply_to.as_deref() {
                            if root_set.contains(parent) {
                                return false;
                            }
                        }
                        n.finding_ref.as_deref() != Some(finding_id)
                    });
                    write_json_atomic(&notes_path, &ns)?;
                    changed = true;
                }
            }
        }
    }

    let gc_path = er.join("github-comments.json");
    if gc_path.is_file() {
        if let Ok(content) = std::fs::read_to_string(&gc_path) {
            if let Ok(mut gc) = serde_json::from_str::<ErGitHubComments>(&content) {
                let roots: Vec<String> = gc
                    .comments
                    .iter()
                    .filter(|c| {
                        c.finding_ref.as_deref() == Some(finding_id) && c.in_reply_to.is_none()
                    })
                    .map(|c| c.id.clone())
                    .collect();
                if !roots.is_empty() {
                    let root_set: std::collections::HashSet<&str> =
                        roots.iter().map(|s| s.as_str()).collect();
                    gc.comments.retain(|c| {
                        if root_set.contains(c.id.as_str()) {
                            return false;
                        }
                        if let Some(parent) = c.in_reply_to.as_deref() {
                            if root_set.contains(parent) {
                                return false;
                            }
                        }
                        c.finding_ref.as_deref() != Some(finding_id)
                    });
                    write_json_atomic(&gc_path, &gc)?;
                    changed = true;
                }
            }
        }
    }

    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::review::{Confidence, Finding, RiskLevel};
    use tempfile::tempdir;

    fn sample_finding(id: &str) -> Finding {
        Finding {
            id: id.to_string(),
            severity: RiskLevel::Medium,
            category: "security".to_string(),
            title: "t".to_string(),
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
    fn find_finding_thread_root_prefers_question_over_github() {
        use super::super::comments::{
            ErGitHubComments, ErQuestions, GitHubReviewComment, ReviewQuestion,
        };
        use super::super::review::AiState;

        let mut ai = AiState::default();
        ai.questions = Some(ErQuestions {
            version: 1,
            diff_hash: "h".into(),
            questions: vec![ReviewQuestion {
                id: "q-root".into(),
                timestamp: String::new(),
                file: "a.rs".into(),
                hunk_index: Some(0),
                line_start: Some(1),
                line_end: None,
                line_content: String::new(),
                text: "stub".into(),
                resolved: false,
                stale: false,
                context_before: vec![],
                context_after: vec![],
                old_line_start: None,
                hunk_header: String::new(),
                anchor_status: "original".into(),
                relocated_at_hash: String::new(),
                in_reply_to: None,
                author: "You".into(),
                promoted_to: None,
                finding_ref: Some("f-1".into()),
            }],
        });
        ai.github_comments = Some(ErGitHubComments {
            version: 1,
            diff_hash: "h".into(),
            github: None,
            comments: vec![GitHubReviewComment {
                id: "c-root".into(),
                timestamp: String::new(),
                file: "a.rs".into(),
                hunk_index: Some(0),
                line_start: Some(1),
                line_end: None,
                line_content: String::new(),
                comment: "stub".into(),
                in_reply_to: None,
                resolved: false,
                source: "local".into(),
                github_id: None,
                author: "You".into(),
                synced: false,
                outdated: false,
                stale: false,
                context_before: vec![],
                context_after: vec![],
                old_line_start: None,
                hunk_header: String::new(),
                anchor_status: "original".into(),
                relocated_at_hash: String::new(),
                finding_ref: Some("f-1".into()),
                side: "RIGHT".into(),
            }],
        });
        assert_eq!(
            find_finding_thread_root(&ai, "f-1").as_deref(),
            Some("q-root")
        );
        ai.questions = None;
        assert_eq!(
            find_finding_thread_root(&ai, "f-1").as_deref(),
            Some("c-root")
        );
    }

    #[test]
    fn remove_finding_from_review_and_expert_sidecars() {
        let dir = tempdir().unwrap();
        let er = dir.path().to_str().unwrap();
        std::fs::create_dir_all(format!("{er}/experts")).unwrap();

        let review = ErReview {
            version: 1,
            diff_hash: "h".into(),
            created_at: String::new(),
            base_branch: String::new(),
            head_branch: String::new(),
            files: [(
                "a.rs".to_string(),
                crate::ai::review::ErFileReview {
                    risk: RiskLevel::Info,
                    risk_reason: String::new(),
                    summary: String::new(),
                    findings: vec![sample_finding("f-2")],
                },
            )]
            .into_iter()
            .collect(),
            file_hashes: Default::default(),
        };
        write_json_atomic(&Path::new(er).join("review.json"), &review).unwrap();

        let expert = ExpertReview {
            version: 1,
            expert_id: "security".into(),
            diff_hash: "h".into(),
            diff_scope: String::new(),
            created_at: String::new(),
            summary: String::new(),
            files: [(
                "a.rs".into(),
                crate::ai::experts::ExpertFileReview {
                    findings: vec![sample_finding("f-1")],
                },
            )]
            .into_iter()
            .collect(),
        };
        write_json_atomic(&Path::new(er).join("experts/security.json"), &expert).unwrap();

        assert!(remove_finding_from_sidecars(er, "sec-f-1").unwrap());

        let review2: ErReview =
            serde_json::from_str(&std::fs::read_to_string(format!("{er}/review.json")).unwrap())
                .unwrap();
        assert_eq!(review2.files["a.rs"].findings.len(), 1);
        assert_eq!(review2.files["a.rs"].findings[0].id, "f-2");

        let expert2: ExpertReview = serde_json::from_str(
            &std::fs::read_to_string(format!("{er}/experts/security.json")).unwrap(),
        )
        .unwrap();
        assert!(expert2.files["a.rs"].findings.is_empty());
    }
}
