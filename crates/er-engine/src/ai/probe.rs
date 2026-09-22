//! Probe pass: Hub-written Questions that should fail if the change is wrong.
//!
//! Probes stay Questions (ADR 0007). The host writes `questions.json` from
//! stdout so a run cannot emit a questionnaire, and so the prompt's untrusted
//! diff never gets a Write tool (ADR 0022). Stamps land on the Question.
//! Findings are not opened, not rewritten (ADR 0009). Promote is not this path.

use super::comments::{ErQuestions, ProbeStamp, ReviewQuestion};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

/// Hard cap. A handful, not a questionnaire.
pub const PROBE_CAP: usize = 5;

pub const PROBE_JSON_BEGIN: &str = "---PROBE_JSON---";
pub const PROBE_JSON_END: &str = "---END_PROBE_JSON---";

pub const PROBE_TASK_KIND: &str = "probes";
pub const PROBE_ANSWER_TASK_KIND: &str = "probe-answers";

static PROBE_SEQ: AtomicU64 = AtomicU64::new(1);

/// One probe the Hub wants written as a Question.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbeDraft {
    pub file: String,
    #[serde(default)]
    pub hunk_index: Option<usize>,
    #[serde(default)]
    pub line_start: Option<usize>,
    #[serde(default)]
    pub line_content: String,
    pub text: String,
}

/// One Hub answer to a probe the person picked.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbeAnswerDraft {
    pub id: String,
    pub stamp: ProbeStamp,
    #[serde(default)]
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProbeWritePayload {
    #[serde(default)]
    probes: Vec<ProbeDraft>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProbeAnswerPayload {
    #[serde(default)]
    answers: Vec<RawProbeAnswer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RawProbeAnswer {
    pub id: String,
    #[serde(default)]
    pub stamp: Option<String>,
    #[serde(default)]
    pub text: String,
}

/// Keep at most [`PROBE_CAP`] non-empty drafts. Extra rows are dropped.
pub fn cap_probe_drafts(drafts: Vec<ProbeDraft>) -> Vec<ProbeDraft> {
    drafts
        .into_iter()
        .filter(|d| !d.text.trim().is_empty())
        .take(PROBE_CAP)
        .collect()
}

/// Files stay closed unless a probe failed or came back empty.
pub fn files_stay_closed(questions: &[ReviewQuestion]) -> bool {
    !questions.iter().any(claim_opens_diff)
}

/// Probes whose stamp opens the diff at that claim.
pub fn claims_to_open(questions: &[ReviewQuestion]) -> Vec<&ReviewQuestion> {
    questions.iter().filter(|q| claim_opens_diff(q)).collect()
}

pub fn is_probe_question(q: &ReviewQuestion) -> bool {
    q.probe && q.in_reply_to.is_none()
}

fn claim_opens_diff(q: &ReviewQuestion) -> bool {
    is_probe_question(q) && matches!(q.probe_stamp, Some(ProbeStamp::Fail | ProbeStamp::Empty))
}

pub fn parse_probe_stamp(raw: &str) -> ProbeStamp {
    match raw.trim().to_ascii_lowercase().as_str() {
        "pass" => ProbeStamp::Pass,
        "fail" => ProbeStamp::Fail,
        "empty" => ProbeStamp::Empty,
        _ => ProbeStamp::Empty,
    }
}

/// Seed drafts from changed paths. Cap is applied by [`cap_probe_drafts`].
pub fn drafts_from_paths<I, S>(paths: I) -> Vec<ProbeDraft>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    paths
        .into_iter()
        .map(|p| {
            let file = p.as_ref().to_string();
            ProbeDraft {
                text: format!(
                    "This change in `{file}` is wrong in a way the diff would show. The probe fails if that is true."
                ),
                file,
                hunk_index: Some(0),
                line_start: Some(1),
                line_content: String::new(),
            }
        })
        .collect()
}

/// Host-write probes from agent stdout. Never touches `review.json`.
pub fn persist_probes_from_agent_stdout(
    stdout: &str,
    stream_json: bool,
    er_dir: &Path,
    diff_hash: &str,
    mode: ProbeHostMode,
) -> Result<()> {
    let text = if stream_json {
        extract_agent_stdout_text(stdout)
    } else {
        stdout.to_string()
    };
    match mode {
        ProbeHostMode::Write => {
            let payload = parse_write_payload(&text)?;
            persist_probe_write(er_dir, diff_hash, payload.probes)?;
        }
        ProbeHostMode::Answer { ids } => {
            let payload = parse_answer_payload(&text)?;
            let answers: Vec<ProbeAnswerDraft> = payload
                .answers
                .into_iter()
                .map(|a| ProbeAnswerDraft {
                    stamp: a
                        .stamp
                        .as_deref()
                        .map(parse_probe_stamp)
                        .unwrap_or(ProbeStamp::Empty),
                    id: a.id,
                    text: a.text,
                })
                .collect();
            persist_probe_answers(er_dir, diff_hash, &ids, answers)?;
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub enum ProbeHostMode {
    Write,
    Answer { ids: Vec<String> },
}

/// Replace previous Hub probes with `drafts`, capped. Person-written Questions stay.
pub fn persist_probe_write(
    er_dir: &Path,
    diff_hash: &str,
    drafts: Vec<ProbeDraft>,
) -> Result<Vec<String>> {
    let drafts = cap_probe_drafts(drafts);
    let mut questions = load_or_new_questions(er_dir, diff_hash)?;
    drop_existing_probes(&mut questions.questions);
    questions.diff_hash = diff_hash.to_string();
    let mut ids = Vec::new();
    let now = crate::sync::chrono_now();
    for draft in drafts {
        let id = mint_probe_id();
        ids.push(id.clone());
        questions
            .questions
            .push(question_from_draft(id, &draft, &now));
    }
    write_questions_atomic(er_dir, &questions)?;
    Ok(ids)
}

/// Stamp selected probes. Unknown ids are ignored. No new probes. No Findings.
pub fn persist_probe_answers(
    er_dir: &Path,
    diff_hash: &str,
    selected_ids: &[String],
    answers: Vec<ProbeAnswerDraft>,
) -> Result<()> {
    let allowed: HashSet<&str> = selected_ids.iter().map(String::as_str).collect();
    let mut questions = load_or_new_questions(er_dir, diff_hash)?;
    questions.diff_hash = diff_hash.to_string();
    let now = crate::sync::chrono_now();
    for answer in answers {
        if !allowed.contains(answer.id.as_str()) {
            continue;
        }
        let Some(parent_idx) = questions
            .questions
            .iter()
            .position(|q| is_probe_question(q) && q.id == answer.id)
        else {
            continue;
        };
        let parent = &questions.questions[parent_idx];
        let reply = ReviewQuestion {
            id: mint_answer_id(),
            timestamp: now.clone(),
            file: parent.file.clone(),
            hunk_index: parent.hunk_index,
            line_start: parent.line_start,
            line_end: parent.line_end,
            line_content: parent.line_content.clone(),
            text: answer.text.clone(),
            resolved: false,
            stale: false,
            context_before: parent.context_before.clone(),
            context_after: parent.context_after.clone(),
            old_line_start: parent.old_line_start,
            side: parent.side.clone(),
            hunk_header: parent.hunk_header.clone(),
            anchor_status: parent.anchor_status.clone(),
            relocated_at_hash: parent.relocated_at_hash.clone(),
            in_reply_to: Some(parent.id.clone()),
            author: "Hub".into(),
            promoted_to: None,
            finding_ref: parent.finding_ref.clone(),
            probe: false,
            probe_stamp: None,
        };
        let parent = &mut questions.questions[parent_idx];
        parent.probe_stamp = Some(answer.stamp);
        // Pass can hide with hide-resolved. Fail and empty stay visible so
        // the claim is still on the diff when files open.
        parent.resolved = matches!(answer.stamp, ProbeStamp::Pass);
        questions.questions.push(reply);
    }
    write_questions_atomic(er_dir, &questions)?;
    Ok(())
}

fn question_from_draft(id: String, draft: &ProbeDraft, now: &str) -> ReviewQuestion {
    ReviewQuestion {
        id,
        timestamp: now.to_string(),
        file: draft.file.clone(),
        hunk_index: draft.hunk_index,
        line_start: draft.line_start,
        line_end: None,
        line_content: draft.line_content.clone(),
        text: draft.text.clone(),
        resolved: false,
        stale: false,
        context_before: Vec::new(),
        context_after: Vec::new(),
        old_line_start: None,
        side: "RIGHT".into(),
        hunk_header: String::new(),
        anchor_status: "original".into(),
        relocated_at_hash: String::new(),
        in_reply_to: None,
        author: "Hub".into(),
        promoted_to: None,
        finding_ref: None,
        probe: true,
        probe_stamp: None,
    }
}

fn drop_existing_probes(questions: &mut Vec<ReviewQuestion>) {
    let probe_ids: HashSet<String> = questions
        .iter()
        .filter(|q| is_probe_question(q))
        .map(|q| q.id.clone())
        .collect();
    questions.retain(|q| {
        if probe_ids.contains(&q.id) {
            return false;
        }
        match &q.in_reply_to {
            Some(parent) if probe_ids.contains(parent) => false,
            _ => true,
        }
    });
}

fn load_or_new_questions(er_dir: &Path, diff_hash: &str) -> Result<ErQuestions> {
    let path = er_dir.join("questions.json");
    match std::fs::read_to_string(&path) {
        Ok(content) => {
            serde_json::from_str(&content).with_context(|| format!("parse {}", path.display()))
        }
        Err(_) => Ok(ErQuestions {
            version: 1,
            diff_hash: diff_hash.to_string(),
            questions: Vec::new(),
        }),
    }
}

fn write_questions_atomic(er_dir: &Path, questions: &ErQuestions) -> Result<()> {
    std::fs::create_dir_all(er_dir).with_context(|| format!("mkdir {}", er_dir.display()))?;
    let path = er_dir.join("questions.json");
    let tmp = er_dir.join("questions.json.tmp");
    let body = serde_json::to_string_pretty(questions).context("serialize questions")?;
    std::fs::write(&tmp, body).with_context(|| format!("write {}", tmp.display()))?;
    std::fs::rename(&tmp, &path).with_context(|| format!("rename onto {}", path.display()))?;
    Ok(())
}

fn mint_probe_id() -> String {
    mint_id("q")
}

fn mint_answer_id() -> String {
    mint_id("a")
}

fn mint_id(prefix: &str) -> String {
    let seq = PROBE_SEQ.fetch_add(1, Ordering::Relaxed);
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("{prefix}-{ms}-{seq}")
}

fn parse_write_payload(text: &str) -> Result<ProbeWritePayload> {
    let json = extract_probe_json_payload(text)
        .ok_or_else(|| anyhow::anyhow!("agent did not emit a probe JSON payload"))?;
    serde_json::from_str(&json).context("parse probe JSON from agent output")
}

fn parse_answer_payload(text: &str) -> Result<ProbeAnswerPayload> {
    let json = extract_probe_json_payload(text)
        .ok_or_else(|| anyhow::anyhow!("agent did not emit a probe JSON payload"))?;
    serde_json::from_str(&json).context("parse probe-answer JSON from agent output")
}

fn extract_probe_json_payload(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if let Some(start) = trimmed.find(PROBE_JSON_BEGIN) {
        let after = &trimmed[start + PROBE_JSON_BEGIN.len()..];
        let end = after.find(PROBE_JSON_END).unwrap_or(after.len());
        return Some(after[..end].trim().to_string());
    }
    if trimmed.starts_with('{') {
        return Some(trimmed.to_string());
    }
    None
}

fn extract_agent_stdout_text(stdout: &str) -> String {
    let mut last_result: Option<String> = None;
    let mut assistant_text: Vec<String> = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if v.get("type").and_then(|t| t.as_str()) == Some("result") {
            if let Some(r) = v.get("result").and_then(|r| r.as_str()) {
                last_result = Some(r.to_string());
            }
        }
        if v.get("type").and_then(|t| t.as_str()) == Some("assistant") {
            if let Some(content) = v
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_array())
            {
                for item in content {
                    if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                        if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                            if !text.trim().is_empty() {
                                assistant_text.push(text.trim().to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    last_result
        .filter(|s| !s.trim().is_empty())
        .or_else(|| (!assistant_text.is_empty()).then(|| assistant_text.join("\n\n")))
        .unwrap_or_else(|| stdout.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(probe: bool, stamp: Option<ProbeStamp>, id: &str) -> ReviewQuestion {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "file": "src/a.rs",
            "text": "claim",
            "probe": probe,
            "probe_stamp": stamp,
        }))
        .unwrap()
    }

    #[test]
    fn cap_drops_empty_and_extras() {
        let mut drafts = Vec::new();
        for i in 0..8 {
            drafts.push(ProbeDraft {
                file: format!("f{i}.rs"),
                hunk_index: Some(0),
                line_start: Some(1),
                line_content: String::new(),
                text: if i == 1 {
                    "  ".into()
                } else {
                    format!("probe {i}")
                },
            });
        }
        let capped = cap_probe_drafts(drafts);
        assert_eq!(capped.len(), PROBE_CAP);
        assert_eq!(capped[0].text, "probe 0");
        assert_eq!(capped[1].text, "probe 2");
        assert_eq!(capped.last().unwrap().text, "probe 5");
    }

    #[test]
    fn files_stay_closed_until_fail_or_empty() {
        let none = vec![q(true, None, "q-1"), q(true, Some(ProbeStamp::Pass), "q-2")];
        assert!(files_stay_closed(&none));
        assert!(claims_to_open(&none).is_empty());

        let fail = vec![q(true, Some(ProbeStamp::Fail), "q-3")];
        assert!(!files_stay_closed(&fail));
        assert_eq!(claims_to_open(&fail)[0].id, "q-3");

        let empty = vec![q(true, Some(ProbeStamp::Empty), "q-4")];
        assert!(!files_stay_closed(&empty));
    }

    #[test]
    fn person_questions_do_not_open_files() {
        let qs = vec![q(false, Some(ProbeStamp::Fail), "q-person")];
        assert!(files_stay_closed(&qs));
        assert!(claims_to_open(&qs).is_empty());
    }

    #[test]
    fn persist_write_caps_and_skips_findings() {
        let dir = tempfile::tempdir().unwrap();
        let review_path = dir.path().join("review.json");
        std::fs::write(
            &review_path,
            r#"{"version":1,"diff_hash":"abc","files":{}}"#,
        )
        .unwrap();
        let before = std::fs::read_to_string(&review_path).unwrap();

        let mut drafts = Vec::new();
        for i in 0..7 {
            drafts.push(ProbeDraft {
                file: format!("f{i}.rs"),
                hunk_index: Some(0),
                line_start: Some(i + 1),
                line_content: String::new(),
                text: format!("Should fail if f{i} is wrong"),
            });
        }
        let ids = persist_probe_write(dir.path(), "abc", drafts).unwrap();
        assert_eq!(ids.len(), PROBE_CAP);

        let loaded: ErQuestions = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("questions.json")).unwrap(),
        )
        .unwrap();
        let probes: Vec<_> = loaded
            .questions
            .iter()
            .filter(|q| is_probe_question(q))
            .collect();
        assert_eq!(probes.len(), PROBE_CAP);
        assert!(probes.iter().all(|q| q.probe && q.promoted_to.is_none()));
        assert!(probes.iter().all(|q| q.author == "Hub"));
        assert_eq!(std::fs::read_to_string(&review_path).unwrap(), before);
    }

    #[test]
    fn persist_write_keeps_person_questions() {
        let dir = tempfile::tempdir().unwrap();
        let person: ErQuestions = serde_json::from_value(serde_json::json!({
            "version": 1,
            "diff_hash": "h",
            "questions": [{
                "id": "q-person",
                "file": "src/a.rs",
                "text": "Why this name?",
                "probe": false
            }]
        }))
        .unwrap();
        std::fs::write(
            dir.path().join("questions.json"),
            serde_json::to_string_pretty(&person).unwrap(),
        )
        .unwrap();
        persist_probe_write(
            dir.path(),
            "h",
            vec![ProbeDraft {
                file: "src/b.rs".into(),
                hunk_index: Some(0),
                line_start: Some(2),
                line_content: String::new(),
                text: "Hub probe".into(),
            }],
        )
        .unwrap();
        let loaded: ErQuestions = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("questions.json")).unwrap(),
        )
        .unwrap();
        assert!(loaded
            .questions
            .iter()
            .any(|q| q.id == "q-person" && !q.probe));
        assert_eq!(
            loaded
                .questions
                .iter()
                .filter(|q| is_probe_question(q))
                .count(),
            1
        );
    }

    #[test]
    fn persist_answers_stamps_only_selected_and_skips_findings() {
        let dir = tempfile::tempdir().unwrap();
        let review_path = dir.path().join("review.json");
        std::fs::write(&review_path, "{\"version\":1,\"diff_hash\":\"h\"}").unwrap();
        let before = std::fs::read_to_string(&review_path).unwrap();

        let ids = persist_probe_write(
            dir.path(),
            "h",
            vec![
                ProbeDraft {
                    file: "a.rs".into(),
                    hunk_index: Some(0),
                    line_start: Some(1),
                    line_content: String::new(),
                    text: "probe a".into(),
                },
                ProbeDraft {
                    file: "b.rs".into(),
                    hunk_index: Some(0),
                    line_start: Some(2),
                    line_content: String::new(),
                    text: "probe b".into(),
                },
            ],
        )
        .unwrap();
        persist_probe_answers(
            dir.path(),
            "h",
            &[ids[0].clone()],
            vec![
                ProbeAnswerDraft {
                    id: ids[0].clone(),
                    stamp: ProbeStamp::Fail,
                    text: "broken here".into(),
                },
                ProbeAnswerDraft {
                    id: ids[1].clone(),
                    stamp: ProbeStamp::Pass,
                    text: "should not stamp, not selected".into(),
                },
            ],
        )
        .unwrap();

        let loaded: ErQuestions = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("questions.json")).unwrap(),
        )
        .unwrap();
        let a = loaded.questions.iter().find(|q| q.id == ids[0]).unwrap();
        let b = loaded.questions.iter().find(|q| q.id == ids[1]).unwrap();
        assert_eq!(a.probe_stamp, Some(ProbeStamp::Fail));
        assert!(!a.resolved);
        assert!(a.promoted_to.is_none());
        assert_eq!(b.probe_stamp, None);
        assert!(!b.resolved);
        assert_eq!(
            loaded
                .questions
                .iter()
                .filter(|q| q.in_reply_to.as_deref() == Some(ids[0].as_str()))
                .count(),
            1
        );
        assert_eq!(std::fs::read_to_string(&review_path).unwrap(), before);
        assert!(!files_stay_closed(&loaded.questions));
        assert_eq!(claims_to_open(&loaded.questions)[0].file, "a.rs");

        persist_probe_answers(
            dir.path(),
            "h",
            &[ids[1].clone()],
            vec![ProbeAnswerDraft {
                id: ids[1].clone(),
                stamp: ProbeStamp::Pass,
                text: "holds".into(),
            }],
        )
        .unwrap();
        let loaded: ErQuestions = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("questions.json")).unwrap(),
        )
        .unwrap();
        let b = loaded.questions.iter().find(|q| q.id == ids[1]).unwrap();
        assert_eq!(b.probe_stamp, Some(ProbeStamp::Pass));
        assert!(b.resolved);
        assert!(!files_stay_closed(&loaded.questions));
    }

    #[test]
    fn pass_only_keeps_files_closed() {
        let dir = tempfile::tempdir().unwrap();
        let ids = persist_probe_write(
            dir.path(),
            "h",
            vec![ProbeDraft {
                file: "a.rs".into(),
                hunk_index: Some(0),
                line_start: Some(1),
                line_content: String::new(),
                text: "probe a".into(),
            }],
        )
        .unwrap();
        persist_probe_answers(
            dir.path(),
            "h",
            &ids,
            vec![ProbeAnswerDraft {
                id: ids[0].clone(),
                stamp: ProbeStamp::Pass,
                text: "holds".into(),
            }],
        )
        .unwrap();
        let loaded: ErQuestions = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("questions.json")).unwrap(),
        )
        .unwrap();
        assert!(files_stay_closed(&loaded.questions));
        assert!(claims_to_open(&loaded.questions).is_empty());
    }

    #[test]
    fn stdout_write_is_capped() {
        let dir = tempfile::tempdir().unwrap();
        let mut probes = Vec::new();
        for i in 0..9 {
            probes.push(serde_json::json!({
                "file": format!("f{i}.rs"),
                "text": format!("p{i}"),
                "line_start": 1
            }));
        }
        let stdout = format!(
            "{PROBE_JSON_BEGIN}\n{}\n{PROBE_JSON_END}",
            serde_json::json!({ "probes": probes })
        );
        persist_probes_from_agent_stdout(&stdout, false, dir.path(), "h", ProbeHostMode::Write)
            .unwrap();
        let loaded: ErQuestions = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("questions.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            loaded
                .questions
                .iter()
                .filter(|q| is_probe_question(q))
                .count(),
            PROBE_CAP
        );
    }

    #[test]
    fn missing_stdout_payload_fails() {
        let dir = tempfile::tempdir().unwrap();
        let err = persist_probes_from_agent_stdout(
            "hub said nothing useful",
            false,
            dir.path(),
            "h",
            ProbeHostMode::Write,
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("did not emit a probe JSON payload"),
            "{err}"
        );
        assert!(!dir.path().join("questions.json").exists());
    }

    #[test]
    fn empty_probes_json_persists_none() {
        let dir = tempfile::tempdir().unwrap();
        let stdout = format!(
            "{PROBE_JSON_BEGIN}\n{}\n{PROBE_JSON_END}",
            serde_json::json!({ "probes": [] })
        );
        persist_probes_from_agent_stdout(&stdout, false, dir.path(), "h", ProbeHostMode::Write)
            .unwrap();
        let loaded: ErQuestions = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("questions.json")).unwrap(),
        )
        .unwrap();
        assert!(loaded.questions.iter().all(|q| !is_probe_question(q)));
    }

    #[test]
    fn missing_stamp_is_empty() {
        assert_eq!(parse_probe_stamp(""), ProbeStamp::Empty);
        assert_eq!(parse_probe_stamp("PASS"), ProbeStamp::Pass);
        assert_eq!(parse_probe_stamp("nope"), ProbeStamp::Empty);
    }
}
