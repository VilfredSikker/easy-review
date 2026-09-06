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

/// True when `stored_id` (as written in a sidecar) refers to the same finding
/// as `target_id` (the merged/prefixed id the UI works with). Findings merged
/// from experts/professor get an `id_prefix` (`sec-`, `prof-`, …) added at load
/// time; this replays that mapping so a write can find the original record.
pub(crate) fn matches_finding_id(
    stored_id: &str,
    target_id: &str,
    id_prefix: Option<&str>,
) -> bool {
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
                side: "RIGHT".to_string(),
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
            files: std::iter::once((
                "a.rs".to_string(),
                crate::ai::review::ErFileReview {
                    risk: RiskLevel::Info,
                    risk_reason: String::new(),
                    summary: String::new(),
                    findings: vec![sample_finding("f-2")],
                },
            ))
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
            files: std::iter::once((
                "a.rs".into(),
                crate::ai::experts::ExpertFileReview {
                    findings: vec![sample_finding("f-1")],
                },
            ))
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

    fn thread_entry(
        id: &str,
        in_reply_to: Option<&str>,
        finding_ref: Option<&str>,
    ) -> crate::ai::comments::ReviewQuestion {
        crate::ai::comments::ReviewQuestion {
            id: id.to_string(),
            timestamp: String::new(),
            file: "a.rs".into(),
            hunk_index: Some(0),
            line_start: Some(1),
            line_end: None,
            line_content: String::new(),
            text: format!("body of {id}"),
            resolved: false,
            stale: false,
            context_before: vec![],
            context_after: vec![],
            old_line_start: None,
            side: "RIGHT".to_string(),
            hunk_header: String::new(),
            anchor_status: "original".into(),
            relocated_at_hash: String::new(),
            in_reply_to: in_reply_to.map(|s| s.to_string()),
            author: "You".into(),
            promoted_to: None,
            finding_ref: finding_ref.map(|s| s.to_string()),
        }
    }

    fn gh_entry(
        id: &str,
        in_reply_to: Option<&str>,
        finding_ref: Option<&str>,
    ) -> crate::ai::comments::GitHubReviewComment {
        crate::ai::comments::GitHubReviewComment {
            id: id.to_string(),
            timestamp: String::new(),
            file: "a.rs".into(),
            hunk_index: Some(0),
            line_start: Some(1),
            line_end: None,
            line_content: String::new(),
            comment: format!("body of {id}"),
            in_reply_to: in_reply_to.map(|s| s.to_string()),
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
            finding_ref: finding_ref.map(|s| s.to_string()),
            side: "RIGHT".into(),
        }
    }

    fn question_ids(er: &str) -> Vec<String> {
        let qs: ErQuestions =
            serde_json::from_str(&std::fs::read_to_string(format!("{er}/questions.json")).unwrap())
                .unwrap();
        qs.questions.into_iter().map(|q| q.id).collect()
    }

    fn note_ids(er: &str) -> Vec<String> {
        let ns: ErNotes =
            serde_json::from_str(&std::fs::read_to_string(format!("{er}/notes.json")).unwrap())
                .unwrap();
        ns.notes.into_iter().map(|n| n.id).collect()
    }

    fn github_ids(er: &str) -> Vec<String> {
        let gc: ErGitHubComments = serde_json::from_str(
            &std::fs::read_to_string(format!("{er}/github-comments.json")).unwrap(),
        )
        .unwrap();
        gc.comments.into_iter().map(|c| c.id).collect()
    }

    /// Writes one linked thread (root + reply), one orphan reply that still
    /// carries the `finding_ref`, and one unrelated entry into each of the three
    /// thread sidecars.
    fn write_linked_threads(er: &str, finding_id: &str) {
        let questions = ErQuestions {
            version: 1,
            diff_hash: "h".into(),
            questions: vec![
                thread_entry("q-root", None, Some(finding_id)),
                thread_entry("q-reply", Some("q-root"), None),
                thread_entry("q-orphan", Some("q-deleted-earlier"), Some(finding_id)),
                thread_entry("q-keep", None, None),
            ],
        };
        write_json_atomic(&Path::new(er).join("questions.json"), &questions).unwrap();

        let notes = ErNotes {
            version: 1,
            diff_hash: "h".into(),
            notes: vec![
                thread_entry("n-root", None, Some(finding_id)),
                thread_entry("n-reply", Some("n-root"), None),
                thread_entry("n-keep", None, Some("some-other-finding")),
            ],
        };
        write_json_atomic(&Path::new(er).join("notes.json"), &notes).unwrap();

        let gh = ErGitHubComments {
            version: 1,
            diff_hash: "h".into(),
            github: None,
            comments: vec![
                gh_entry("c-root", None, Some(finding_id)),
                gh_entry("c-reply", Some("c-root"), None),
                gh_entry("c-keep", None, None),
            ],
        };
        write_json_atomic(&Path::new(er).join("github-comments.json"), &gh).unwrap();
    }

    #[test]
    fn delete_threads_removes_root_reply_and_orphan_from_every_sidecar() {
        let dir = tempdir().unwrap();
        let er = dir.path().to_str().unwrap();
        write_linked_threads(er, "sec-1");

        assert!(delete_threads_linked_to_finding(er, "sec-1").unwrap());

        // Root, its reply, and the orphan that still points at the finding all go;
        // the unrelated entry survives.
        assert_eq!(question_ids(er), vec!["q-keep".to_string()]);
        // A note whose finding_ref names a *different* finding is untouched.
        assert_eq!(note_ids(er), vec!["n-keep".to_string()]);
        assert_eq!(github_ids(er), vec!["c-keep".to_string()]);
    }

    #[test]
    fn delete_threads_is_a_noop_when_no_thread_references_the_finding() {
        let dir = tempdir().unwrap();
        let er = dir.path().to_str().unwrap();
        write_linked_threads(er, "sec-1");

        assert!(!delete_threads_linked_to_finding(er, "sec-999").unwrap());

        assert_eq!(
            question_ids(er),
            vec!["q-root", "q-reply", "q-orphan", "q-keep"]
        );
        assert_eq!(note_ids(er), vec!["n-root", "n-reply", "n-keep"]);
        assert_eq!(github_ids(er), vec!["c-root", "c-reply", "c-keep"]);
    }

    #[test]
    fn delete_threads_reports_false_when_no_sidecars_exist() {
        let dir = tempdir().unwrap();
        let er = dir.path().to_str().unwrap();
        assert!(!delete_threads_linked_to_finding(er, "sec-1").unwrap());
    }

    // A corrupt questions.json must not stop the github/notes threads for the
    // same finding from being cleaned up.
    #[test]
    fn delete_threads_skips_unparseable_sidecar_but_still_cleans_the_others() {
        let dir = tempdir().unwrap();
        let er = dir.path().to_str().unwrap();
        write_linked_threads(er, "sec-1");
        std::fs::write(Path::new(er).join("questions.json"), "{ not valid json").unwrap();

        assert!(delete_threads_linked_to_finding(er, "sec-1").unwrap());

        assert_eq!(note_ids(er), vec!["n-keep".to_string()]);
        assert_eq!(github_ids(er), vec!["c-keep".to_string()]);
        // The corrupt file was left exactly as it was, not rewritten.
        assert_eq!(
            std::fs::read_to_string(Path::new(er).join("questions.json")).unwrap(),
            "{ not valid json"
        );
    }

    #[test]
    fn remove_finding_from_sidecars_strips_professor_insight_by_prefixed_id() {
        use crate::ai::professor::{ProfessorFileReview, ProfessorReview};

        let dir = tempdir().unwrap();
        let er = dir.path().to_str().unwrap();

        let prof = ProfessorReview {
            version: 1,
            diff_hash: "h".into(),
            diff_scope: String::new(),
            created_at: String::new(),
            focus_prompt: String::new(),
            summary: String::new(),
            files: std::iter::once((
                "a.rs".to_string(),
                ProfessorFileReview {
                    findings: vec![sample_finding("1"), sample_finding("2")],
                },
            ))
            .collect(),
        };
        write_json_atomic(&Path::new(er).join("professor.json"), &prof).unwrap();

        assert!(remove_finding_from_sidecars(er, "prof-1").unwrap());

        let loaded: ProfessorReview =
            serde_json::from_str(&std::fs::read_to_string(format!("{er}/professor.json")).unwrap())
                .unwrap();
        let ids: Vec<&str> = loaded.files["a.rs"]
            .findings
            .iter()
            .map(|f| f.id.as_str())
            .collect();
        assert_eq!(ids, vec!["2"]);
    }

    #[test]
    fn remove_finding_from_sidecars_returns_false_and_rewrites_nothing_when_id_is_unknown() {
        let dir = tempdir().unwrap();
        let er = dir.path().to_str().unwrap();

        let review = ErReview {
            version: 1,
            diff_hash: "h".into(),
            created_at: String::new(),
            base_branch: String::new(),
            head_branch: String::new(),
            files: std::iter::once((
                "a.rs".to_string(),
                crate::ai::review::ErFileReview {
                    risk: RiskLevel::Info,
                    risk_reason: String::new(),
                    summary: String::new(),
                    findings: vec![sample_finding("f-2")],
                },
            ))
            .collect(),
            file_hashes: Default::default(),
        };
        let review_path = Path::new(er).join("review.json");
        write_json_atomic(&review_path, &review).unwrap();
        let before = std::fs::read_to_string(&review_path).unwrap();

        assert!(!remove_finding_from_sidecars(er, "f-nope").unwrap());
        assert_eq!(std::fs::read_to_string(&review_path).unwrap(), before);
    }

    // The expert loop rebuilds the sidecar path from `expert_id`, so a sidecar
    // saved under a different filename is skipped rather than rewritten at the
    // canonical path (which would resurrect a file the user renamed away).
    #[test]
    fn remove_finding_skips_expert_sidecar_whose_filename_does_not_match_expert_id() {
        let dir = tempdir().unwrap();
        let er = dir.path().to_str().unwrap();
        std::fs::create_dir_all(format!("{er}/experts")).unwrap();

        let expert = ExpertReview {
            version: 1,
            expert_id: "security".into(),
            diff_hash: "h".into(),
            diff_scope: String::new(),
            created_at: String::new(),
            summary: String::new(),
            files: std::iter::once((
                "a.rs".into(),
                crate::ai::experts::ExpertFileReview {
                    findings: vec![sample_finding("1")],
                },
            ))
            .collect(),
        };
        let renamed = Path::new(er).join("experts/security.backup.json");
        write_json_atomic(&renamed, &expert).unwrap();

        assert!(!remove_finding_from_sidecars(er, "sec-1").unwrap());

        // Nothing was written at the canonical path…
        assert!(!Path::new(er).join("experts/security.json").exists());
        // …and the renamed sidecar still holds its finding.
        let loaded: ExpertReview =
            serde_json::from_str(&std::fs::read_to_string(&renamed).unwrap()).unwrap();
        assert_eq!(loaded.files["a.rs"].findings.len(), 1);
    }
}
