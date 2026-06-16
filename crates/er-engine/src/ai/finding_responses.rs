//! Read/write `Finding.responses` in `review.json` (AI validation replies on findings).

use super::review::{AiResponse, AiState, ErReview};
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

fn with_finding_mut<F>(er_dir: &str, finding_id: &str, f: F) -> anyhow::Result<()>
where
    F: FnOnce(&mut super::review::Finding) -> anyhow::Result<()>,
{
    let review_path = Path::new(er_dir).join("review.json");
    let content = std::fs::read_to_string(&review_path)
        .map_err(|e| anyhow::anyhow!("Failed to read review.json: {e}"))?;
    let mut review: ErReview = serde_json::from_str(&content)
        .map_err(|e| anyhow::anyhow!("Failed to parse review.json: {e}"))?;
    let mut found = false;
    for fr in review.files.values_mut() {
        if let Some(finding) = fr.findings.iter_mut().find(|f| f.id == finding_id) {
            f(finding)?;
            found = true;
            break;
        }
    }
    if !found {
        anyhow::bail!("Finding not found: {finding_id}");
    }
    write_json_atomic(&review_path, &review)?;
    Ok(())
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
}
