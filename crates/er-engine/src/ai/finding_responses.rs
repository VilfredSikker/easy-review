//! Read/write `Finding.responses` in `review.json` (AI validation replies on findings).

use super::experts::{expert_by_id, ExpertReview};
use super::finding_cleanup::{matches_finding_id, sidecar_update, SidecarWrite};
use super::professor::{ProfessorReview, PROFESSOR_ID_PREFIX};
use super::review::{AiResponse, AiState, ErReview, Finding};
#[cfg(test)]
use std::path::Path;

#[cfg(test)]
fn write_json_atomic<T: serde::Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(value)?;
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)
}

fn iso_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    let days = secs / 86400;
    let remaining = secs % 86400;
    let hours = remaining / 3600;
    let minutes = (remaining % 3600) / 60;
    let seconds = remaining % 60;

    let mut year = 1970i32;
    let mut month = 1u32;
    let mut day = 1u32;
    let mut d = days as i64;
    loop {
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let ydays = if leap { 366 } else { 365 };
        if d < ydays {
            let month_days = [
                31,
                if leap { 29 } else { 28 },
                31,
                30,
                31,
                30,
                31,
                31,
                30,
                31,
                30,
                31,
            ];
            for (i, &md) in month_days.iter().enumerate() {
                if d < i64::from(md) {
                    month = (i + 1) as u32;
                    day = (d + 1) as u32;
                    break;
                }
                d -= i64::from(md);
            }
            break;
        }
        d -= ydays;
        year += 1;
    }
    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

/// Find the first finding matching `finding_id` (replaying the `id_prefix`
/// applied at merge time) and apply `f` to it. Returns whether it was found.
pub(crate) fn apply_to_findings<F>(
    files: &mut [&mut Vec<Finding>],
    finding_id: &str,
    id_prefix: Option<&str>,
    f: &mut Option<F>,
) -> anyhow::Result<bool>
where
    F: FnOnce(&mut Finding) -> anyhow::Result<()>,
{
    for findings in files.iter_mut() {
        if let Some(finding) = findings
            .iter_mut()
            .find(|f| matches_finding_id(&f.id, finding_id, id_prefix))
        {
            let f = f
                .take()
                .ok_or_else(|| anyhow::anyhow!("apply_to_findings called after finding matched"))?;
            f(finding)?;
            return Ok(true);
        }
    }
    Ok(false)
}

/// Locate the finding across all sidecars (`review.json`, `professor.json`,
/// `experts/*.json`) and apply `f` to it in whichever file it lives in.
///
/// Expert and professor findings are merged into the in-memory review with a
/// prefixed id and their producer recorded in `lens`, but they are persisted in
/// their own sidecars — so a validation reply for `sec-1` must be written back
/// to `experts/security.json`, not `review.json` (which may not even exist).
fn with_finding_mut<F>(er_dir: &str, finding_id: &str, f: F) -> anyhow::Result<()>
where
    F: FnOnce(&mut Finding) -> anyhow::Result<()>,
{
    let mut f = Some(f);
    // A scoped overlay row is the displayed row for its lens; write it there
    // before looking at the full-review sidecars.
    if super::scoped_overlay::with_finding_in_overlay(er_dir, finding_id, &mut f)? {
        return Ok(());
    }

    let store = crate::store_registry::store();
    let scope = er_store::Scope::dir(er_dir)?;

    // General findings keep their original id in `review.json` (no prefix). A
    // corrupt/unreadable review.json falls through to the other sidecars rather
    // than aborting — the finding may live in an expert/professor file (mirrors
    // `remove_finding_from_sidecars`).
    let written = sidecar_update::<ErReview, _>(&store, &scope, (), |review| {
        let mut files: Vec<&mut Vec<Finding>> = review
            .files
            .values_mut()
            .map(|fr| &mut fr.findings)
            .collect();
        apply_to_findings(&mut files, finding_id, None, &mut f)
    })?;
    if written == SidecarWrite::Written {
        return Ok(());
    }

    // Professor insights merge with a `prof-` id prefix.
    let written = sidecar_update::<ProfessorReview, _>(&store, &scope, (), |prof| {
        let mut files: Vec<&mut Vec<Finding>> = prof
            .files
            .values_mut()
            .map(|pfr| &mut pfr.findings)
            .collect();
        apply_to_findings(&mut files, finding_id, Some(PROFESSOR_ID_PREFIX), &mut f)
    })?;
    if written == SidecarWrite::Written {
        return Ok(());
    }

    // Each expert merges findings with its own id prefix (`sec-`, `api-`, …).
    // Skip unknown experts: the projection (`build_findings_projection`) never
    // appends their findings to the in-memory review, so they can't be a
    // validation target — and matching with a `None` prefix would collapse to
    // bare-id equality and risk writing to the wrong sidecar.
    if let Ok(slots) = store.list::<ExpertReview>(&scope) {
        for slot in slots {
            let Some(def) = expert_by_id(slot.as_str()) else {
                continue;
            };
            let written = sidecar_update::<ExpertReview, _>(&store, &scope, slot, |expert| {
                let mut files: Vec<&mut Vec<Finding>> = expert
                    .files
                    .values_mut()
                    .map(|efr| &mut efr.findings)
                    .collect();
                apply_to_findings(&mut files, finding_id, Some(def.id_prefix), &mut f)
            })?;
            if written == SidecarWrite::Written {
                return Ok(());
            }
        }
    }

    anyhow::bail!("Finding not found: {finding_id}");
}

/// Append an AI validation reply to a finding. Returns the new response id.
pub fn append_finding_response(
    er_dir: &str,
    finding_id: &str,
    text: &str,
) -> anyhow::Result<String> {
    let id = format!(
        "fr-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    );
    let response_id = id.clone();
    with_finding_mut(er_dir, finding_id, |finding| {
        finding.responses.push(AiResponse {
            id,
            in_reply_to: String::new(),
            timestamp: iso_now(),
            text: text.to_string(),
            new_findings: Vec::new(),
        });
        Ok(())
    })?;
    Ok(response_id)
}

pub fn update_finding_response(
    er_dir: &str,
    finding_id: &str,
    response_id: &str,
    text: &str,
) -> anyhow::Result<()> {
    with_finding_mut(er_dir, finding_id, |finding| {
        let response = finding
            .responses
            .iter_mut()
            .find(|r| r.id == response_id)
            .ok_or_else(|| anyhow::anyhow!("Finding response not found: {response_id}"))?;
        response.text = text.to_string();
        Ok(())
    })
}

pub fn delete_finding_response(
    er_dir: &str,
    finding_id: &str,
    response_id: &str,
) -> anyhow::Result<()> {
    with_finding_mut(er_dir, finding_id, |finding| {
        let before = finding.responses.len();
        finding.responses.retain(|r| r.id != response_id);
        if finding.responses.len() == before {
            anyhow::bail!("Finding response not found: {response_id}");
        }
        Ok(())
    })
}

struct PromoteReply {
    timestamp: String,
    author: String,
    body: String,
    dedupe_key: String,
}

/// Collect validation replies for promote body: finding responses + linked thread replies.
pub fn collect_finding_promote_replies(ai: &AiState, finding_id: &str) -> Vec<(String, String)> {
    let mut entries: Vec<PromoteReply> = Vec::new();

    if let Some(review) = ai.review.as_ref() {
        for fr in review.files.values() {
            if let Some(f) = fr.findings.iter().find(|f| f.id == finding_id) {
                for r in &f.responses {
                    entries.push(PromoteReply {
                        timestamp: r.timestamp.clone(),
                        author: "AI".to_string(),
                        body: r.text.clone(),
                        dedupe_key: format!("response:{}", r.id),
                    });
                }
                break;
            }
        }
    }

    if let Some(qs) = ai.questions.as_ref() {
        let roots: Vec<&str> = qs
            .questions
            .iter()
            .filter(|q| q.finding_ref.as_deref() == Some(finding_id) && q.in_reply_to.is_none())
            .map(|q| q.id.as_str())
            .collect();
        for q in &qs.questions {
            if let Some(parent) = q.in_reply_to.as_deref() {
                if roots.contains(&parent) {
                    let author = if q.author.is_empty() {
                        "You".to_string()
                    } else {
                        q.author.clone()
                    };
                    entries.push(PromoteReply {
                        timestamp: q.timestamp.clone(),
                        author,
                        body: q.text.clone(),
                        dedupe_key: format!("question:{}", q.id),
                    });
                }
            }
        }
    }

    if let Some(ns) = ai.notes.as_ref() {
        let roots: Vec<&str> = ns
            .notes
            .iter()
            .filter(|n| n.finding_ref.as_deref() == Some(finding_id) && n.in_reply_to.is_none())
            .map(|n| n.id.as_str())
            .collect();
        for n in &ns.notes {
            if let Some(parent) = n.in_reply_to.as_deref() {
                if roots.contains(&parent) {
                    let author = if n.author.is_empty() {
                        "You".to_string()
                    } else {
                        n.author.clone()
                    };
                    entries.push(PromoteReply {
                        timestamp: n.timestamp.clone(),
                        author,
                        body: n.text.clone(),
                        dedupe_key: format!("note:{}", n.id),
                    });
                }
            }
        }
    }

    if let Some(gc) = ai.github_comments.as_ref() {
        let roots: Vec<&str> = gc
            .comments
            .iter()
            .filter(|c| c.finding_ref.as_deref() == Some(finding_id) && c.in_reply_to.is_none())
            .map(|c| c.id.as_str())
            .collect();
        for c in &gc.comments {
            if let Some(parent) = c.in_reply_to.as_deref() {
                if roots.contains(&parent) {
                    let author = if c.author.is_empty() {
                        "You".to_string()
                    } else {
                        c.author.clone()
                    };
                    entries.push(PromoteReply {
                        timestamp: c.timestamp.clone(),
                        author,
                        body: c.comment.clone(),
                        dedupe_key: format!("github:{}", c.id),
                    });
                }
            }
        }
    }

    entries.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    let mut seen = std::collections::HashSet::new();
    entries
        .into_iter()
        .filter(|e| seen.insert(e.dedupe_key.clone()))
        .map(|e| (e.author, e.body))
        .collect()
}

pub fn append_promote_replies(mut body: String, replies: &[(String, String)]) -> String {
    for (author, text) in replies {
        let quoted = text
            .lines()
            .map(|l| format!("> {l}"))
            .collect::<Vec<_>>()
            .join("\n");
        body.push_str(&format!("\n\n> **{author}** replied:\n{quoted}"));
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::review::{Confidence, ErFileReview, ErReview, Finding, RiskLevel};
    use std::collections::HashMap;

    #[test]
    fn append_and_update_finding_response() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let er = dir.path();
            let finding = Finding {
                id: "f1".to_string(),
                severity: RiskLevel::Medium,
                lens: String::new(),
                category: "correctness".to_string(),
                raised_by: Vec::new(),
                providers: Vec::new(),
                title: "t".to_string(),
                description: "d".to_string(),
                hunk_index: None,
                line_start: None,
                line_end: None,
                line_content: String::new(),
                stale: false,
                suggestion: String::new(),
                related_files: Vec::new(),
                outside_diff: false,
                confidence: Confidence::default(),
                verification_plan: String::new(),
                evidence: Vec::new(),
                responses: Vec::new(),
                resolved: false,
                resolved_note: String::new(),
                resolved_at: String::new(),
                promoted_to: None,
            };
            let review = ErReview {
                version: 1,
                diff_hash: "h".to_string(),
                created_at: String::new(),
                base_branch: String::new(),
                head_branch: String::new(),
                files: std::iter::once((
                    "a.rs".to_string(),
                    ErFileReview {
                        risk: RiskLevel::Low,
                        risk_reason: String::new(),
                        summary: String::new(),
                        findings: vec![finding],
                    },
                ))
                .collect::<HashMap<_, _>>(),
                file_hashes: HashMap::new(),
            };
            write_json_atomic(&er.join("review.json"), &review).unwrap();

            let id = append_finding_response(er.to_str().unwrap(), "f1", "first").unwrap();
            update_finding_response(er.to_str().unwrap(), "f1", &id, "updated").unwrap();

            let content = std::fs::read_to_string(er.join("review.json")).unwrap();
            let loaded: ErReview = serde_json::from_str(&content).unwrap();
            assert_eq!(
                loaded.files["a.rs"].findings[0].responses[0].text,
                "updated"
            );
        });
    }

    fn bare_finding(id: &str) -> Finding {
        Finding {
            id: id.to_string(),
            severity: RiskLevel::High,
            lens: String::new(),
            category: String::new(),
            raised_by: Vec::new(),
            providers: Vec::new(),
            title: "t".to_string(),
            description: "d".to_string(),
            hunk_index: None,
            line_start: None,
            line_end: None,
            line_content: String::new(),
            stale: false,
            suggestion: String::new(),
            related_files: Vec::new(),
            outside_diff: false,
            confidence: Confidence::default(),
            verification_plan: String::new(),
            evidence: Vec::new(),
            responses: Vec::new(),
            resolved: false,
            resolved_note: String::new(),
            resolved_at: String::new(),
            promoted_to: None,
        }
    }

    // An expert finding lives in `experts/{id}.json`, not `review.json`. The UI
    // works with the merged, prefixed id (`api-1`); the reply must be routed
    // back to the expert sidecar — even when `review.json` does not exist.
    #[test]
    fn append_response_routes_to_expert_sidecar() {
        use crate::ai::experts::{ExpertFileReview, ExpertReview};

        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let er = dir.path();
            std::fs::create_dir_all(er.join("experts")).unwrap();

            let expert = ExpertReview {
                version: 1,
                expert_id: "api".to_string(),
                diff_hash: "h".to_string(),
                diff_scope: String::new(),
                created_at: String::new(),
                summary: String::new(),
                files: std::iter::once((
                    "a.rs".to_string(),
                    ExpertFileReview {
                        findings: vec![bare_finding("1")],
                    },
                ))
                .collect::<HashMap<_, _>>(),
            };
            write_json_atomic(&er.join("experts/api.json"), &expert).unwrap();

            // No review.json at all — must not error with "No such file or directory".
            append_finding_response(er.to_str().unwrap(), "api-1", "validated").unwrap();

            let content = std::fs::read_to_string(er.join("experts/api.json")).unwrap();
            let loaded: ExpertReview = serde_json::from_str(&content).unwrap();
            assert_eq!(
                loaded.files["a.rs"].findings[0].responses[0].text,
                "validated"
            );
        });
    }

    // A corrupt review.json must not be rewritten as an empty review when the
    // reply belongs to an expert sidecar.
    #[test]
    fn replying_to_a_finding_leaves_a_malformed_review_json_untouched() {
        use crate::ai::experts::{ExpertFileReview, ExpertReview};

        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let er = dir.path();
            std::fs::create_dir_all(er.join("experts")).unwrap();
            let corrupt = "{ not valid json";
            std::fs::write(er.join("review.json"), corrupt).unwrap();

            let expert = ExpertReview {
                version: 1,
                expert_id: "api".to_string(),
                diff_hash: "h".to_string(),
                diff_scope: String::new(),
                created_at: String::new(),
                summary: String::new(),
                files: std::iter::once((
                    "a.rs".to_string(),
                    ExpertFileReview {
                        findings: vec![bare_finding("1")],
                    },
                ))
                .collect::<HashMap<_, _>>(),
            };
            write_json_atomic(&er.join("experts/api.json"), &expert).unwrap();

            append_finding_response(er.to_str().unwrap(), "api-1", "validated").unwrap();

            assert_eq!(
                std::fs::read_to_string(er.join("review.json")).unwrap(),
                corrupt,
                "a malformed sidecar is skipped, never rewritten"
            );
            let loaded: ExpertReview = serde_json::from_str(
                &std::fs::read_to_string(er.join("experts/api.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(
                loaded.files["a.rs"].findings[0].responses[0].text,
                "validated"
            );
        });
    }

    // A newer review.json must not be rewritten when the reply belongs to an
    // expert sidecar.
    #[test]
    fn replying_to_a_finding_leaves_a_newer_review_file_untouched() {
        use crate::ai::experts::{ExpertFileReview, ExpertReview};

        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let er = dir.path();
            std::fs::create_dir_all(er.join("experts")).unwrap();
            let newer = r#"{"version":2,"diff_hash":"h","files":{}}"#;
            std::fs::write(er.join("review.json"), newer).unwrap();

            let expert = ExpertReview {
                version: 1,
                expert_id: "api".to_string(),
                diff_hash: "h".to_string(),
                diff_scope: String::new(),
                created_at: String::new(),
                summary: String::new(),
                files: std::iter::once((
                    "a.rs".to_string(),
                    ExpertFileReview {
                        findings: vec![bare_finding("1")],
                    },
                ))
                .collect::<HashMap<_, _>>(),
            };
            write_json_atomic(&er.join("experts/api.json"), &expert).unwrap();

            append_finding_response(er.to_str().unwrap(), "api-1", "validated").unwrap();

            assert_eq!(
                std::fs::read_to_string(er.join("review.json")).unwrap(),
                newer,
                "a newer file is left alone"
            );
            let loaded: ExpertReview = serde_json::from_str(
                &std::fs::read_to_string(er.join("experts/api.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(
                loaded.files["a.rs"].findings[0].responses[0].text,
                "validated"
            );
        });
    }

    // With both sidecars present, a general finding still lands in review.json
    // and an expert finding still lands in the expert sidecar.
    #[test]
    fn routes_general_and_expert_findings_independently() {
        use crate::ai::experts::{ExpertFileReview, ExpertReview};

        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let er = dir.path();
            std::fs::create_dir_all(er.join("experts")).unwrap();

            let review = ErReview {
                version: 1,
                diff_hash: "h".to_string(),
                created_at: String::new(),
                base_branch: String::new(),
                head_branch: String::new(),
                files: std::iter::once((
                    "a.rs".to_string(),
                    ErFileReview {
                        risk: RiskLevel::Low,
                        risk_reason: String::new(),
                        summary: String::new(),
                        findings: vec![bare_finding("f-1")],
                    },
                ))
                .collect::<HashMap<_, _>>(),
                file_hashes: HashMap::new(),
            };
            write_json_atomic(&er.join("review.json"), &review).unwrap();

            let expert = ExpertReview {
                version: 1,
                expert_id: "security".to_string(),
                diff_hash: "h".to_string(),
                diff_scope: String::new(),
                created_at: String::new(),
                summary: String::new(),
                files: std::iter::once((
                    "a.rs".to_string(),
                    ExpertFileReview {
                        findings: vec![bare_finding("2")],
                    },
                ))
                .collect::<HashMap<_, _>>(),
            };
            write_json_atomic(&er.join("experts/security.json"), &expert).unwrap();

            append_finding_response(er.to_str().unwrap(), "f-1", "general-reply").unwrap();
            append_finding_response(er.to_str().unwrap(), "sec-2", "expert-reply").unwrap();

            let r: ErReview =
                serde_json::from_str(&std::fs::read_to_string(er.join("review.json")).unwrap())
                    .unwrap();
            assert_eq!(
                r.files["a.rs"].findings[0].responses[0].text,
                "general-reply"
            );

            let e: ExpertReview = serde_json::from_str(
                &std::fs::read_to_string(er.join("experts/security.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(
                e.files["a.rs"].findings[0].responses[0].text,
                "expert-reply"
            );
        });
    }

    // A corrupt review.json must not block validation of a finding that lives in
    // an expert sidecar — routing falls through to experts/*.json.
    #[test]
    fn corrupt_review_json_still_routes_to_expert() {
        use crate::ai::experts::{ExpertFileReview, ExpertReview};

        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let er = dir.path();
            std::fs::create_dir_all(er.join("experts")).unwrap();
            std::fs::write(er.join("review.json"), "{ not valid json").unwrap();

            let expert = ExpertReview {
                version: 1,
                expert_id: "security".to_string(),
                diff_hash: "h".to_string(),
                diff_scope: String::new(),
                created_at: String::new(),
                summary: String::new(),
                files: std::iter::once((
                    "a.rs".to_string(),
                    ExpertFileReview {
                        findings: vec![bare_finding("3")],
                    },
                ))
                .collect::<HashMap<_, _>>(),
            };
            write_json_atomic(&er.join("experts/security.json"), &expert).unwrap();

            append_finding_response(er.to_str().unwrap(), "sec-3", "validated").unwrap();

            let loaded: ExpertReview = serde_json::from_str(
                &std::fs::read_to_string(er.join("experts/security.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(
                loaded.files["a.rs"].findings[0].responses[0].text,
                "validated"
            );
        });
    }

    // Professor insights merge with a `prof-` id prefix and persist in
    // professor.json — replies must route there.
    #[test]
    fn append_response_routes_to_professor_sidecar() {
        use crate::ai::professor::{ProfessorFileReview, ProfessorReview};

        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let er = dir.path();

            let prof = ProfessorReview {
                version: 1,
                diff_hash: "h".to_string(),
                diff_scope: String::new(),
                created_at: String::new(),
                focus_prompt: String::new(),
                summary: String::new(),
                files: std::iter::once((
                    "a.rs".to_string(),
                    ProfessorFileReview {
                        findings: vec![bare_finding("1")],
                    },
                ))
                .collect::<HashMap<_, _>>(),
            };
            write_json_atomic(&er.join("professor.json"), &prof).unwrap();

            append_finding_response(er.to_str().unwrap(), "prof-1", "validated").unwrap();

            let loaded: ProfessorReview =
                serde_json::from_str(&std::fs::read_to_string(er.join("professor.json")).unwrap())
                    .unwrap();
            assert_eq!(
                loaded.files["a.rs"].findings[0].responses[0].text,
                "validated"
            );
        });
    }

    // An unknown expert (not in the registry) is never merged into the in-memory
    // review, so it must be skipped on write too — a bare-id collision with a
    // general finding must not corrupt the unknown sidecar.
    #[test]
    fn unknown_expert_sidecar_is_skipped() {
        use crate::ai::experts::{ExpertFileReview, ExpertReview};

        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let er = dir.path();
            std::fs::create_dir_all(er.join("experts")).unwrap();

            let review = ErReview {
                version: 1,
                diff_hash: "h".to_string(),
                created_at: String::new(),
                base_branch: String::new(),
                head_branch: String::new(),
                files: std::iter::once((
                    "a.rs".to_string(),
                    ErFileReview {
                        risk: RiskLevel::Low,
                        risk_reason: String::new(),
                        summary: String::new(),
                        findings: vec![bare_finding("1")],
                    },
                ))
                .collect::<HashMap<_, _>>(),
                file_hashes: HashMap::new(),
            };
            write_json_atomic(&er.join("review.json"), &review).unwrap();

            // Unknown expert id with a colliding bare finding id "1".
            let unknown = ExpertReview {
                version: 1,
                expert_id: "made-up".to_string(),
                diff_hash: "h".to_string(),
                diff_scope: String::new(),
                created_at: String::new(),
                summary: String::new(),
                files: std::iter::once((
                    "a.rs".to_string(),
                    ExpertFileReview {
                        findings: vec![bare_finding("1")],
                    },
                ))
                .collect::<HashMap<_, _>>(),
            };
            write_json_atomic(&er.join("experts/made-up.json"), &unknown).unwrap();

            append_finding_response(er.to_str().unwrap(), "1", "general-reply").unwrap();

            // The reply lands in review.json…
            let r: ErReview =
                serde_json::from_str(&std::fs::read_to_string(er.join("review.json")).unwrap())
                    .unwrap();
            assert_eq!(
                r.files["a.rs"].findings[0].responses[0].text,
                "general-reply"
            );

            // …and the unknown sidecar is left untouched (no responses written).
            let u: ExpertReview = serde_json::from_str(
                &std::fs::read_to_string(er.join("experts/made-up.json")).unwrap(),
            )
            .unwrap();
            assert!(u.files["a.rs"].findings[0].responses.is_empty());
        });
    }

    // Two replies on one review.json must both land: the store lock covers the
    // read, so the second write starts from the first one's bytes.
    #[test]
    fn concurrent_appends_to_one_review_keep_both_responses() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_process_test_storage_root(dir.path(), || {
            let er = dir.path();
            let review = ErReview {
                version: 1,
                diff_hash: "h".to_string(),
                created_at: String::new(),
                base_branch: String::new(),
                head_branch: String::new(),
                files: std::iter::once((
                    "a.rs".to_string(),
                    ErFileReview {
                        risk: RiskLevel::Low,
                        risk_reason: String::new(),
                        summary: String::new(),
                        findings: vec![bare_finding("f1"), bare_finding("f2")],
                    },
                ))
                .collect::<HashMap<_, _>>(),
                file_hashes: HashMap::new(),
            };
            write_json_atomic(&er.join("review.json"), &review).unwrap();

            let er_dir = er.to_string_lossy().into_owned();
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
            let spawn = |id: &'static str, text: &'static str| {
                let er_dir = er_dir.clone();
                let barrier = std::sync::Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    append_finding_response(&er_dir, id, text).unwrap();
                })
            };
            let first = spawn("f1", "first");
            let second = spawn("f2", "second");
            barrier.wait();
            first.join().unwrap();
            second.join().unwrap();

            let loaded: ErReview =
                serde_json::from_str(&std::fs::read_to_string(er.join("review.json")).unwrap())
                    .unwrap();
            let findings = &loaded.files["a.rs"].findings;
            assert!(
                findings
                    .iter()
                    .any(|f| f.responses.iter().any(|r| r.text == "first")),
                "the first reply survived"
            );
            assert!(
                findings
                    .iter()
                    .any(|f| f.responses.iter().any(|r| r.text == "second")),
                "the second reply survived"
            );
        });
    }
}
