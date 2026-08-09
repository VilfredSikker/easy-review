//! Headless read/write of PR review feedback: questions, notes, and AI findings.
//!
//! Used by `er-mcp` so client agents can inspect and respond to review artifacts
//! stored in the managed PR bucket (`prs/pr-<N>/`).

use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::ai::{
    append_finding_response, compute_diff_hash, load_ai_state, ErNotes, ErQuestions, Finding,
    ReviewQuestion, RiskLevel,
};
use crate::github::owner_repo_storage_slug;
use crate::storage::resolve_managed_root_for_pr_bucket;
use crate::sync::chrono_now;

/// Slim finding row for MCP / API consumers (merged review + experts + professor).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrFindingItem {
    pub id: String,
    pub file: String,
    pub severity: String,
    pub category: String,
    pub title: String,
    pub description: String,
    pub suggestion: String,
    pub hunk_index: Option<usize>,
    pub line_start: Option<usize>,
    pub line_end: Option<usize>,
    pub confidence: String,
    pub resolved: bool,
    pub outside_diff: bool,
    pub responses: Vec<crate::ai::AiResponse>,
    /// `review`, `professor`, or `expert:<id>`.
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrReviewFeedback {
    pub owner: String,
    pub repo: String,
    pub number: u64,
    pub bucket_path: String,
    pub diff_hash: String,
    pub questions: Vec<ReviewQuestion>,
    pub notes: Vec<ReviewQuestion>,
    pub findings: Vec<PrFindingItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrFeedbackReply {
    pub id: String,
    pub kind: String,
    pub parent_id: Option<String>,
}

fn resolve_pr_er_dir(owner: &str, repo: &str, pr: u64) -> Result<String> {
    let slug = owner_repo_storage_slug(owner, repo);
    let er_dir = resolve_managed_root_for_pr_bucket(&slug, pr).er_dir();
    if er_dir.is_empty() {
        bail!("failed to resolve managed PR storage for {owner}/{repo}#{pr}");
    }
    Ok(er_dir)
}

fn pr_bucket_path(er_dir: &str) -> String {
    er_dir.to_string()
}

fn pr_diff_hash(er_dir: &str) -> String {
    let diff_tmp = Path::new(er_dir).join("diff-tmp");
    if let Ok(content) = std::fs::read_to_string(&diff_tmp) {
        if !content.trim().is_empty() {
            return compute_diff_hash(&content);
        }
    }
    for name in ["review.json", "questions.json", "notes.json"] {
        let path = Path::new(er_dir).join(name);
        if let Ok(content) = std::fs::read_to_string(&path) {
            if name == "review.json" {
                if let Ok(review) = serde_json::from_str::<crate::ai::ErReview>(&content) {
                    if !review.diff_hash.is_empty() {
                        return review.diff_hash;
                    }
                }
            } else if name == "questions.json" {
                if let Ok(qs) = serde_json::from_str::<ErQuestions>(&content) {
                    if !qs.diff_hash.is_empty() {
                        return qs.diff_hash;
                    }
                }
            } else if let Ok(ns) = serde_json::from_str::<ErNotes>(&content) {
                if !ns.diff_hash.is_empty() {
                    return ns.diff_hash;
                }
            }
        }
    }
    String::new()
}

fn severity_label(level: RiskLevel) -> &'static str {
    match level {
        RiskLevel::High => "high",
        RiskLevel::Medium => "medium",
        RiskLevel::Low => "low",
        RiskLevel::Info => "info",
    }
}

fn finding_source(category: &str) -> String {
    if category.starts_with("professor:") {
        "professor".to_string()
    } else if let Some(rest) = category.strip_prefix("expert:") {
        format!("expert:{rest}")
    } else {
        "review".to_string()
    }
}

fn collect_findings(review: &crate::ai::ErReview) -> Vec<PrFindingItem> {
    let mut out = Vec::new();
    for (file, fr) in &review.files {
        for f in &fr.findings {
            out.push(finding_to_item(file, f));
        }
    }
    out.sort_by(|a, b| {
        a.file
            .cmp(&b.file)
            .then_with(|| a.line_start.cmp(&b.line_start))
            .then_with(|| a.id.cmp(&b.id))
    });
    out
}

fn finding_to_item(file: &str, f: &Finding) -> PrFindingItem {
    PrFindingItem {
        id: f.id.clone(),
        file: file.to_string(),
        severity: severity_label(f.severity).to_string(),
        category: f.category.clone(),
        title: f.title.clone(),
        description: f.description.clone(),
        suggestion: f.suggestion.clone(),
        hunk_index: f.hunk_index,
        line_start: f.line_start,
        line_end: f.line_end,
        confidence: format!("{:?}", f.confidence).to_ascii_lowercase(),
        resolved: f.resolved,
        outside_diff: f.outside_diff,
        responses: f.responses.clone(),
        source: finding_source(&f.category),
    }
}

fn write_json_atomic<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(value)?;
    std::fs::write(&tmp, json).with_context(|| format!("write {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("rename {}", path.display()))?;
    Ok(())
}

fn load_questions(er_dir: &str) -> Result<ErQuestions> {
    let path = Path::new(er_dir).join("questions.json");
    match std::fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content)
            .with_context(|| format!("parse corrupt questions.json at {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ErQuestions {
            version: 1,
            diff_hash: String::new(),
            questions: Vec::new(),
        }),
        Err(e) => Err(e.into()),
    }
}

fn load_notes(er_dir: &str) -> Result<ErNotes> {
    let path = Path::new(er_dir).join("notes.json");
    match std::fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content)
            .with_context(|| format!("parse corrupt notes.json at {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ErNotes {
            version: 1,
            diff_hash: String::new(),
            notes: Vec::new(),
        }),
        Err(e) => Err(e.into()),
    }
}

fn next_thread_id(prefix: &str) -> String {
    format!(
        "{prefix}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    )
}

fn validate_parent_thread<'a>(
    items: &'a [ReviewQuestion],
    parent_id: &str,
    kind: &str,
) -> Result<&'a ReviewQuestion> {
    let parent = items
        .iter()
        .find(|q| q.id == parent_id)
        .with_context(|| format!("{kind} not found: {parent_id}"))?;
    if parent.in_reply_to.is_some() {
        bail!("reply to the top-level {kind} id, not a reply (flat threads)");
    }
    Ok(parent)
}

fn append_question_reply(
    er_dir: &str,
    parent: &ReviewQuestion,
    text: &str,
    author: &str,
) -> Result<String> {
    let mut questions = load_questions(er_dir)?;
    let diff_hash = pr_diff_hash(er_dir);
    if questions.diff_hash.is_empty() {
        questions.diff_hash = diff_hash.clone();
    } else if !diff_hash.is_empty() && questions.diff_hash != diff_hash {
        questions.diff_hash = diff_hash;
    }

    let id = next_thread_id("q");
    questions.questions.push(ReviewQuestion {
        id: id.clone(),
        timestamp: chrono_now(),
        file: parent.file.clone(),
        hunk_index: parent.hunk_index,
        line_start: parent.line_start,
        line_end: parent.line_end,
        line_content: parent.line_content.clone(),
        text: text.to_string(),
        resolved: false,
        stale: false,
        context_before: parent.context_before.clone(),
        context_after: parent.context_after.clone(),
        old_line_start: parent.old_line_start,
        hunk_header: parent.hunk_header.clone(),
        anchor_status: parent.anchor_status.clone(),
        relocated_at_hash: parent.relocated_at_hash.clone(),
        in_reply_to: Some(parent.id.clone()),
        author: author.to_string(),
        promoted_to: None,
        finding_ref: parent.finding_ref.clone(),
    });

    std::fs::create_dir_all(er_dir)?;
    write_json_atomic(&Path::new(er_dir).join("questions.json"), &questions)?;
    Ok(id)
}

fn append_note_reply(
    er_dir: &str,
    parent: &ReviewQuestion,
    text: &str,
    author: &str,
) -> Result<String> {
    let mut notes = load_notes(er_dir)?;
    let diff_hash = pr_diff_hash(er_dir);
    if notes.diff_hash.is_empty() {
        notes.diff_hash = diff_hash.clone();
    } else if !diff_hash.is_empty() && notes.diff_hash != diff_hash {
        notes.diff_hash = diff_hash;
    }

    let id = next_thread_id("n");
    notes.notes.push(ReviewQuestion {
        id: id.clone(),
        timestamp: chrono_now(),
        file: parent.file.clone(),
        hunk_index: parent.hunk_index,
        line_start: parent.line_start,
        line_end: parent.line_end,
        line_content: parent.line_content.clone(),
        text: text.to_string(),
        resolved: false,
        stale: false,
        context_before: parent.context_before.clone(),
        context_after: parent.context_after.clone(),
        old_line_start: parent.old_line_start,
        hunk_header: parent.hunk_header.clone(),
        anchor_status: parent.anchor_status.clone(),
        relocated_at_hash: parent.relocated_at_hash.clone(),
        in_reply_to: Some(parent.id.clone()),
        author: author.to_string(),
        promoted_to: None,
        finding_ref: parent.finding_ref.clone(),
    });

    std::fs::create_dir_all(er_dir)?;
    write_json_atomic(&Path::new(er_dir).join("notes.json"), &notes)?;
    Ok(id)
}

/// Read questions, notes, and merged AI findings from the managed PR bucket.
pub fn get_pr_review_feedback(
    owner: &str,
    repo: &str,
    number: u64,
    include_resolved: bool,
) -> Result<PrReviewFeedback> {
    let er_dir = resolve_pr_er_dir(owner, repo, number)?;
    let diff_hash = pr_diff_hash(&er_dir);
    let ai = load_ai_state(&er_dir, &diff_hash, None);

    let questions = ai
        .questions
        .map(|qs| qs.questions)
        .unwrap_or_default()
        .into_iter()
        .filter(|q| include_resolved || !q.resolved)
        .collect();

    let notes = ai
        .notes
        .map(|ns| ns.notes)
        .unwrap_or_default()
        .into_iter()
        .filter(|n| include_resolved || !n.resolved)
        .collect();

    let findings = ai
        .review
        .as_ref()
        .map(collect_findings)
        .unwrap_or_default()
        .into_iter()
        .filter(|f| include_resolved || !f.resolved)
        .collect();

    Ok(PrReviewFeedback {
        owner: owner.to_string(),
        repo: repo.to_string(),
        number,
        bucket_path: pr_bucket_path(&er_dir),
        diff_hash,
        questions,
        notes,
        findings,
    })
}

/// Reply to a top-level question thread in `questions.json`.
pub fn reply_to_pr_question(
    owner: &str,
    repo: &str,
    number: u64,
    question_id: &str,
    text: &str,
    author: Option<&str>,
) -> Result<PrFeedbackReply> {
    if text.trim().is_empty() {
        bail!("reply text must be non-empty");
    }
    let er_dir = resolve_pr_er_dir(owner, repo, number)?;
    let questions = load_questions(&er_dir)?;
    let parent = validate_parent_thread(&questions.questions, question_id, "question")?;
    let author = author.unwrap_or("agent");
    let id = append_question_reply(&er_dir, parent, text, author)?;
    Ok(PrFeedbackReply {
        id,
        kind: "question".to_string(),
        parent_id: Some(question_id.to_string()),
    })
}

/// Reply to a top-level note thread in `notes.json`.
pub fn reply_to_pr_note(
    owner: &str,
    repo: &str,
    number: u64,
    note_id: &str,
    text: &str,
    author: Option<&str>,
) -> Result<PrFeedbackReply> {
    if text.trim().is_empty() {
        bail!("reply text must be non-empty");
    }
    let er_dir = resolve_pr_er_dir(owner, repo, number)?;
    let notes = load_notes(&er_dir)?;
    let parent = validate_parent_thread(&notes.notes, note_id, "note")?;
    let author = author.unwrap_or("agent");
    let id = append_note_reply(&er_dir, parent, text, author)?;
    Ok(PrFeedbackReply {
        id,
        kind: "note".to_string(),
        parent_id: Some(note_id.to_string()),
    })
}

/// Append a validation / follow-up reply on an AI finding (`review.json` or expert/professor sidecars).
pub fn reply_to_pr_finding(
    owner: &str,
    repo: &str,
    number: u64,
    finding_id: &str,
    text: &str,
) -> Result<PrFeedbackReply> {
    if text.trim().is_empty() {
        bail!("reply text must be non-empty");
    }
    let er_dir = resolve_pr_er_dir(owner, repo, number)?;
    let id = append_finding_response(&er_dir, finding_id, text)?;
    Ok(PrFeedbackReply {
        id,
        kind: "finding".to_string(),
        parent_id: Some(finding_id.to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{Confidence, ErFileReview, ErReview, Finding};
    use std::collections::HashMap;

    fn with_storage_root<F: FnOnce()>(f: F) {
        let _guard = crate::storage::STORAGE_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("ER_STORAGE_ROOT", dir.path());
        f();
        std::env::remove_var("ER_STORAGE_ROOT");
    }

    fn sample_question(id: &str) -> ReviewQuestion {
        ReviewQuestion {
            id: id.to_string(),
            timestamp: "2026-01-01T00:00:00Z".into(),
            file: "src/a.rs".into(),
            hunk_index: Some(0),
            line_start: Some(10),
            line_end: None,
            line_content: "fn a() {}".into(),
            text: "Why?".into(),
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
            finding_ref: None,
        }
    }

    #[test]
    fn get_and_reply_roundtrip() {
        with_storage_root(|| {
            let er_dir = resolve_pr_er_dir("acme", "widgets", 5).unwrap();
            std::fs::create_dir_all(&er_dir).unwrap();
            let qs = ErQuestions {
                version: 1,
                diff_hash: "hash".into(),
                questions: vec![sample_question("q-root")],
            };
            write_json_atomic(&Path::new(&er_dir).join("questions.json"), &qs).unwrap();

            let feedback = get_pr_review_feedback("acme", "widgets", 5, true).unwrap();
            assert_eq!(feedback.questions.len(), 1);
            assert!(feedback.findings.is_empty());

            let reply =
                reply_to_pr_question("acme", "widgets", 5, "q-root", "Because.", None).unwrap();
            assert_eq!(reply.kind, "question");

            let feedback = get_pr_review_feedback("acme", "widgets", 5, true).unwrap();
            assert_eq!(feedback.questions.len(), 2);
            assert_eq!(feedback.questions[1].in_reply_to.as_deref(), Some("q-root"));
            assert_eq!(feedback.questions[1].author, "agent");
        });
    }

    #[test]
    fn finding_reply_persists() {
        with_storage_root(|| {
            let er_dir = resolve_pr_er_dir("acme", "widgets", 6).unwrap();
            std::fs::create_dir_all(&er_dir).unwrap();
            let finding = Finding {
                id: "f1".into(),
                severity: RiskLevel::High,
                category: "general".into(),
                title: "Bug".into(),
                description: "desc".into(),
                hunk_index: None,
                line_start: None,
                line_end: None,
                suggestion: String::new(),
                related_files: vec![],
                outside_diff: false,
                confidence: Confidence::default(),
                verification_plan: String::new(),
                evidence: vec![],
                responses: vec![],
                resolved: false,
                resolved_note: String::new(),
                resolved_at: String::new(),
                promoted_to: None,
            };
            let review = ErReview {
                version: 1,
                diff_hash: "h".into(),
                created_at: String::new(),
                base_branch: String::new(),
                head_branch: String::new(),
                files: [(
                    "a.rs".into(),
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
            write_json_atomic(&Path::new(&er_dir).join("review.json"), &review).unwrap();

            reply_to_pr_finding("acme", "widgets", 6, "f1", "Confirmed.").unwrap();
            let feedback = get_pr_review_feedback("acme", "widgets", 6, true).unwrap();
            assert_eq!(feedback.findings.len(), 1);
            assert_eq!(feedback.findings[0].responses[0].text, "Confirmed.");
        });
    }
}
