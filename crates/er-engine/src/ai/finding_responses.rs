//! Read/write `Finding.responses` in `review.json` (AI validation replies on findings).

use super::experts::{expert_by_id, load_expert_reviews, ExpertReview};
use super::finding_cleanup::matches_finding_id;
use super::professor::{load_professor_review, PROFESSOR_ID_PREFIX};
use super::review::{AiResponse, AiState, ErReview, Finding};
use std::path::Path;

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
                if d < md as i64 {
                    month = (i + 1) as u32;
                    day = (d + 1) as u32;
                    break;
                }
                d -= md as i64;
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
fn apply_to_findings<F>(
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
                .expect("apply_to_findings called after finding matched");
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
/// prefixed id and a rewritten `category`, but they are persisted in their own
/// sidecars — so a validation reply for `sec-1` must be written back to
/// `experts/security.json`, not `review.json` (which may not even exist).
fn with_finding_mut<F>(er_dir: &str, finding_id: &str, f: F) -> anyhow::Result<()>
where
    F: FnOnce(&mut Finding) -> anyhow::Result<()>,
{
    let er = Path::new(er_dir);
    let mut f = Some(f);

    // General findings keep their original id in `review.json` (no prefix).
    let review_path = er.join("review.json");
    if review_path.is_file() {
        let content = std::fs::read_to_string(&review_path)
            .map_err(|e| anyhow::anyhow!("Failed to read review.json: {e}"))?;
        let mut review: ErReview = serde_json::from_str(&content)
            .map_err(|e| anyhow::anyhow!("Failed to parse review.json: {e}"))?;
        let mut files: Vec<&mut Vec<Finding>> = review
            .files
            .values_mut()
            .map(|fr| &mut fr.findings)
            .collect();
        if apply_to_findings(&mut files, finding_id, None, &mut f)? {
            write_json_atomic(&review_path, &review)?;
            return Ok(());
        }
    }

    // Professor insights merge with a `prof-` id prefix.
    let prof_path = er.join("professor.json");
    if let Some(mut prof) = load_professor_review(er_dir) {
        let mut files: Vec<&mut Vec<Finding>> = prof
            .files
            .values_mut()
            .map(|pfr| &mut pfr.findings)
            .collect();
        if apply_to_findings(&mut files, finding_id, Some(PROFESSOR_ID_PREFIX), &mut f)? {
            write_json_atomic(&prof_path, &prof)?;
            return Ok(());
        }
    }

    // Each expert merges findings with its own id prefix (`sec-`, `api-`, …).
    for expert in load_expert_reviews(er_dir) {
        let prefix = expert_by_id(&expert.expert_id).map(|d| d.id_prefix);
        let path = er
            .join("experts")
            .join(format!("{}.json", expert.expert_id));
        let mut review: ExpertReview = expert;
        let mut files: Vec<&mut Vec<Finding>> = review
            .files
            .values_mut()
            .map(|efr| &mut efr.findings)
            .collect();
        if apply_to_findings(&mut files, finding_id, prefix, &mut f)? {
            write_json_atomic(&path, &review)?;
            return Ok(());
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
        let er = dir.path();
        let finding = Finding {
            id: "f1".to_string(),
            severity: RiskLevel::Medium,
            category: "general".to_string(),
            title: "t".to_string(),
            description: "d".to_string(),
            hunk_index: None,
            line_start: None,
            line_end: None,
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
            files: [(
                "a.rs".to_string(),
                ErFileReview {
                    risk: RiskLevel::Low,
                    risk_reason: String::new(),
                    summary: String::new(),
                    findings: vec![finding],
                },
            )]
            .into_iter()
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
    }

    fn bare_finding(id: &str) -> Finding {
        Finding {
            id: id.to_string(),
            severity: RiskLevel::High,
            category: String::new(),
            title: "t".to_string(),
            description: "d".to_string(),
            hunk_index: None,
            line_start: None,
            line_end: None,
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
        let er = dir.path();
        std::fs::create_dir_all(er.join("experts")).unwrap();

        let expert = ExpertReview {
            version: 1,
            expert_id: "api".to_string(),
            diff_hash: "h".to_string(),
            diff_scope: String::new(),
            created_at: String::new(),
            summary: String::new(),
            files: [(
                "a.rs".to_string(),
                ExpertFileReview {
                    findings: vec![bare_finding("1")],
                },
            )]
            .into_iter()
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
    }

    // With both sidecars present, a general finding still lands in review.json
    // and an expert finding still lands in the expert sidecar.
    #[test]
    fn routes_general_and_expert_findings_independently() {
        use crate::ai::experts::{ExpertFileReview, ExpertReview};

        let dir = tempfile::tempdir().unwrap();
        let er = dir.path();
        std::fs::create_dir_all(er.join("experts")).unwrap();

        let review = ErReview {
            version: 1,
            diff_hash: "h".to_string(),
            created_at: String::new(),
            base_branch: String::new(),
            head_branch: String::new(),
            files: [(
                "a.rs".to_string(),
                ErFileReview {
                    risk: RiskLevel::Low,
                    risk_reason: String::new(),
                    summary: String::new(),
                    findings: vec![bare_finding("f-1")],
                },
            )]
            .into_iter()
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
            files: [(
                "a.rs".to_string(),
                ExpertFileReview {
                    findings: vec![bare_finding("2")],
                },
            )]
            .into_iter()
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
    }
}
