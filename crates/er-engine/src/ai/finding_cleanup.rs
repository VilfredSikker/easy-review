//! Persisted removal and resolution of findings, and removal of linked validation threads.

use super::annotation_store;
use super::experts::{expert_by_id, ExpertReview};
use super::professor::{ProfessorReview, PROFESSOR_ID_PREFIX};
use super::review::AiState;
use super::review::{ErReview, Finding};
use crate::store_registry;
use anyhow::Context;
use std::collections::HashSet;
#[cfg(test)]
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

fn retain_findings(findings: &mut Vec<Finding>, target_id: &str, id_prefix: Option<&str>) -> bool {
    let before = findings.len();
    findings.retain(|f| !matches_finding_id(&f.id, target_id, id_prefix));
    findings.len() < before
}

#[cfg(test)]
fn write_json_atomic<T: serde::Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(value)?;
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)
}

/// What an update closure decided about one sidecar.
enum SidecarStop {
    /// The file is absent or unreadable; leave it alone.
    Missing,
    /// The file was read but the edit changed nothing.
    NotHere,
}

/// The result of one update-under-lock pass over a sidecar.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SidecarWrite {
    /// The closure returned a changed value and the store wrote it.
    Written,
    /// Nothing was written: absent, unreadable, newer, or no change.
    Skipped,
}

/// Read, apply and write one sidecar under a single lock hold.
///
/// `apply` reports whether it changed the value; a `false` or an unreadable file
/// leaves the bytes untouched.
pub(crate) fn sidecar_update<T, A>(
    store: &er_store::ArtifactStore,
    scope: &er_store::Scope,
    slot: T::Slot,
    apply: A,
) -> anyhow::Result<SidecarWrite>
where
    T: er_store::Artifact,
    A: FnOnce(&mut T) -> anyhow::Result<bool>,
{
    let key = er_store::ArtifactKey::<T>::new(scope.clone(), slot)?;
    let mut failure = None;
    let outcome = store.update(&key, er_store::DEFAULT_LOCK_TIMEOUT, |current| {
        let Some(mut value) = current else {
            return Err(SidecarStop::Missing);
        };
        match apply(&mut value) {
            Ok(true) => Ok(value),
            Ok(false) => Err(SidecarStop::NotHere),
            Err(error) => {
                failure = Some(error);
                Err(SidecarStop::NotHere)
            }
        }
    });
    match outcome {
        Ok(_) => Ok(SidecarWrite::Written),
        Err(er_store::UpdateError::Apply(SidecarStop::NotHere)) => match failure {
            Some(error) => Err(error),
            None => Ok(SidecarWrite::Skipped),
        },
        Err(er_store::UpdateError::Apply(SidecarStop::Missing)) => Ok(SidecarWrite::Skipped),
        Err(er_store::UpdateError::Store(er_store::StoreError::NewerVersion { .. })) => {
            Ok(SidecarWrite::Skipped)
        }
        Err(er_store::UpdateError::Store(error)) => Err(error.into()),
    }
}

/// Apply `edit` to the findings of every sidecar that can own a finding —
/// `review.json`, `professor.json` and `experts/*.json` — writing back each file
/// the edit reports as changed. `edit` gets the id prefix the loader adds to that
/// sidecar's ids, so it can match the merged id the UI holds.
///
/// Each file is read and written under one lock hold, and a file that cannot be
/// read is skipped, never overwritten.
fn edit_finding_sidecars<F>(er_dir: &str, mut edit: F) -> std::io::Result<bool>
where
    F: FnMut(&mut Vec<Finding>, Option<&str>) -> bool,
{
    let store = store_registry::store();
    let scope = er_store::Scope::dir(er_dir).map_err(io_error)?;
    let mut changed = false;

    let review = sidecar_update::<ErReview, _>(&store, &scope, (), |review| {
        let mut edited = false;
        for fr in review.files.values_mut() {
            edited |= edit(&mut fr.findings, None);
        }
        Ok(edited)
    })
    .map_err(io_error)?;
    changed |= review == SidecarWrite::Written;

    let professor = sidecar_update::<ProfessorReview, _>(&store, &scope, (), |prof| {
        let mut edited = false;
        for pfr in prof.files.values_mut() {
            edited |= edit(&mut pfr.findings, Some(PROFESSOR_ID_PREFIX));
        }
        Ok(edited)
    })
    .map_err(io_error)?;
    changed |= professor == SidecarWrite::Written;

    if let Ok(slots) = store.list::<ExpertReview>(&scope) {
        for slot in slots {
            let prefix = expert_by_id(slot.as_str()).map(|def| def.id_prefix);
            let written = sidecar_update::<ExpertReview, _>(&store, &scope, slot, |expert| {
                let mut edited = false;
                for efr in expert.files.values_mut() {
                    edited |= edit(&mut efr.findings, prefix);
                }
                Ok(edited)
            })
            .map_err(io_error)?;
            changed |= written == SidecarWrite::Written;
        }
    }

    Ok(changed)
}

/// Remove a finding from `review.json`, `professor.json`, and `experts/*.json`. Returns true if any file changed.
pub fn remove_finding_from_sidecars(er_dir: &str, finding_id: &str) -> std::io::Result<bool> {
    edit_finding_sidecars(er_dir, |findings, prefix| {
        retain_findings(findings, finding_id, prefix)
    })
}

/// Set or clear a person's resolution of a finding in whichever sidecar owns
/// it. Only the status fields move; the claim is left as the AI wrote it
/// (ADR 0047). Clearing also drops any `resolved_note` a legacy or
/// hand-written sidecar carries, because a reopened finding has no resolution
/// left to explain.
/// Returns false when no sidecar holds the finding.
pub fn set_finding_resolved(
    er_dir: &str,
    finding_id: &str,
    resolved: bool,
) -> std::io::Result<bool> {
    let resolved_at = if resolved {
        crate::sync::chrono_now()
    } else {
        String::new()
    };
    let mut found = false;
    edit_finding_sidecars(er_dir, |findings, prefix| {
        let mut changed = false;
        for f in findings
            .iter_mut()
            .filter(|f| matches_finding_id(&f.id, finding_id, prefix))
        {
            found = true;
            if f.resolved == resolved {
                continue;
            }
            f.resolved = resolved;
            f.resolved_at.clone_from(&resolved_at);
            if !resolved {
                f.resolved_note.clear();
            }
            changed = true;
        }
        changed
    })?;
    Ok(found)
}

fn io_error(error: impl std::fmt::Display) -> std::io::Error {
    std::io::Error::other(format!("{error}"))
}

fn remove_unlinked<T>(
    items: &mut Vec<T>,
    id_of: impl Fn(&T) -> &str,
    reply_to_of: impl Fn(&T) -> Option<&str>,
    finding_ref_of: impl Fn(&T) -> Option<&str>,
    finding_id: &str,
) -> bool {
    let roots: Vec<String> = items
        .iter()
        .filter(|item| finding_ref_of(item) == Some(finding_id) && reply_to_of(item).is_none())
        .map(|item| id_of(item).to_string())
        .collect();
    if roots.is_empty() {
        return false;
    }
    let root_set: HashSet<&str> = roots.iter().map(String::as_str).collect();
    items.retain(|item| {
        !root_set.contains(id_of(item))
            && !reply_to_of(item).is_some_and(|parent| root_set.contains(parent))
            && finding_ref_of(item) != Some(finding_id)
    });
    true
}

/// Delete question, note and GitHub-comment threads created for finding
/// validation (`finding_ref`).
///
/// The read only decides whether a write is needed; the delete itself is
/// applied to `current` inside the store's update closure, and an absent or
/// unreadable file is left alone.
pub fn delete_threads_linked_to_finding(
    er_dir: &str,
    github_comments_dir: &str,
    finding_id: &str,
) -> std::io::Result<bool> {
    let store = store_registry::store();
    let scope = er_store::Scope::dir(er_dir).map_err(io_error)?;
    let gh_scope = er_store::Scope::dir(github_comments_dir).map_err(io_error)?;
    let mut changed = false;

    if let er_store::Loaded::Found(stored) =
        annotation_store::load_questions(&store, &scope).map_err(io_error)?
    {
        let has_roots = stored.value.questions.iter().any(|question| {
            question.finding_ref.as_deref() == Some(finding_id) && question.in_reply_to.is_none()
        });
        if has_roots {
            let finding = finding_id.to_string();
            annotation_store::update_questions(&store, &scope, move |current| {
                let mut questions =
                    current.context("the questions file is missing or unreadable")?;
                remove_unlinked(
                    &mut questions.questions,
                    |question| question.id.as_str(),
                    |question| question.in_reply_to.as_deref(),
                    |question| question.finding_ref.as_deref(),
                    &finding,
                );
                Ok::<_, anyhow::Error>(questions)
            })
            .map_err(io_error)?;
            changed = true;
        }
    }

    if let er_store::Loaded::Found(stored) =
        annotation_store::load_notes(&store, &scope).map_err(io_error)?
    {
        let has_roots = stored.value.notes.iter().any(|note| {
            note.finding_ref.as_deref() == Some(finding_id) && note.in_reply_to.is_none()
        });
        if has_roots {
            let finding = finding_id.to_string();
            annotation_store::update_notes(&store, &scope, move |current| {
                let mut notes = current.context("the notes file is missing or unreadable")?;
                remove_unlinked(
                    &mut notes.notes,
                    |note| note.id.as_str(),
                    |note| note.in_reply_to.as_deref(),
                    |note| note.finding_ref.as_deref(),
                    &finding,
                );
                Ok::<_, anyhow::Error>(notes)
            })
            .map_err(io_error)?;
            changed = true;
        }
    }

    if let er_store::Loaded::Found(stored) =
        annotation_store::load_github_comments(&store, &gh_scope).map_err(io_error)?
    {
        let has_roots = stored.value.comments.iter().any(|comment| {
            comment.finding_ref.as_deref() == Some(finding_id) && comment.in_reply_to.is_none()
        });
        if has_roots {
            let finding = finding_id.to_string();
            annotation_store::update_github_comments(&store, &gh_scope, move |current| {
                let mut comments =
                    current.context("the GitHub comments file is missing or unreadable")?;
                remove_unlinked(
                    &mut comments.comments,
                    |comment| comment.id.as_str(),
                    |comment| comment.in_reply_to.as_deref(),
                    |comment| comment.finding_ref.as_deref(),
                    &finding,
                );
                Ok::<_, anyhow::Error>(comments)
            })
            .map_err(io_error)?;
            changed = true;
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
            lens: String::new(),
            category: "correctness".to_string(),
            raised_by: Vec::new(),
            providers: Vec::new(),
            title: "t".to_string(),
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
    fn find_finding_thread_root_prefers_question_over_github() {
        use super::super::comments::{
            ErGithubComments, ErQuestions, GithubComment, PrivateAnnotation,
        };
        use super::super::review::AiState;

        let mut ai = AiState::default();
        ai.questions = Some(ErQuestions {
            version: 1,
            diff_hash: "h".into(),
            questions: vec![PrivateAnnotation {
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
                ai_action: None,
            }],
        });
        ai.github_comments = Some(ErGithubComments {
            version: 1,
            diff_hash: "h".into(),
            github: None,
            comments: vec![GithubComment {
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

    #[test]
    fn removing_a_finding_leaves_a_malformed_professor_file_untouched() {
        let dir = tempdir().unwrap();
        let er = dir.path().to_str().unwrap();
        std::fs::write(format!("{er}/professor.json"), "{ not valid json").unwrap();
        let before = std::fs::read_to_string(format!("{er}/professor.json")).unwrap();
        write_expert_with(er, sample_finding("f-1"));

        assert!(remove_finding_from_sidecars(er, "sec-f-1").unwrap());

        assert_eq!(
            std::fs::read_to_string(format!("{er}/professor.json")).unwrap(),
            before,
            "a malformed sidecar is skipped, never rewritten"
        );
        let expert: ExpertReview = serde_json::from_str(
            &std::fs::read_to_string(format!("{er}/experts/security.json")).unwrap(),
        )
        .unwrap();
        assert!(expert.files["a.rs"].findings.is_empty());
    }

    #[test]
    fn removing_a_finding_leaves_a_newer_review_file_untouched() {
        let dir = tempdir().unwrap();
        let er = dir.path().to_str().unwrap();
        let newer = r#"{"version":2,"diff_hash":"h","files":{}}"#;
        std::fs::write(format!("{er}/review.json"), newer).unwrap();
        write_expert_with(er, sample_finding("f-1"));

        assert!(remove_finding_from_sidecars(er, "sec-f-1").unwrap());

        assert_eq!(
            std::fs::read_to_string(format!("{er}/review.json")).unwrap(),
            newer,
            "a newer file is left alone"
        );
        let expert: ExpertReview = serde_json::from_str(
            &std::fs::read_to_string(format!("{er}/experts/security.json")).unwrap(),
        )
        .unwrap();
        assert!(expert.files["a.rs"].findings.is_empty());
    }

    #[test]
    fn a_held_bucket_lock_keeps_the_finding_file_unchanged() {
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
                    findings: vec![sample_finding("f-1")],
                },
            ))
            .collect(),
            file_hashes: Default::default(),
        };
        write_json_atomic(&Path::new(er).join("review.json"), &review).unwrap();
        let before = std::fs::read_to_string(Path::new(er).join("review.json")).unwrap();

        let store = crate::store_registry::store();
        let scope = er_store::Scope::dir(er).unwrap();
        let guard = store
            .lock_scope(&scope, er_store::DEFAULT_LOCK_TIMEOUT)
            .unwrap();
        let result = set_finding_resolved(er, "f-1", true);
        drop(guard);

        assert!(result.is_err(), "a held lock must block the rewrite");
        assert_eq!(
            std::fs::read_to_string(Path::new(er).join("review.json")).unwrap(),
            before
        );
    }

    fn write_expert_with(er: &str, finding: Finding) {
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
                    findings: vec![finding],
                },
            ))
            .collect(),
        };
        write_json_atomic(&Path::new(er).join("experts/security.json"), &expert).unwrap();
    }

    fn read_expert_finding(er: &str) -> Finding {
        let expert: ExpertReview = serde_json::from_str(
            &std::fs::read_to_string(format!("{er}/experts/security.json")).unwrap(),
        )
        .unwrap();
        expert.files["a.rs"].findings[0].clone()
    }

    #[test]
    fn resolving_a_finding_flips_status_and_keeps_the_claim() {
        let dir = tempdir().unwrap();
        let er = dir.path().to_str().unwrap();
        let mut finding = sample_finding("f-1");
        finding.description = "the AI's claim".into();
        write_expert_with(er, finding);

        assert!(set_finding_resolved(er, "sec-f-1", true).unwrap());
        let resolved = read_expert_finding(er);
        assert!(resolved.resolved);
        assert!(!resolved.resolved_at.is_empty());
        assert_eq!(resolved.title, "t");
        assert_eq!(resolved.description, "the AI's claim");

        assert!(set_finding_resolved(er, "sec-f-1", false).unwrap());
        let reopened = read_expert_finding(er);
        assert!(!reopened.resolved);
        assert!(reopened.resolved_at.is_empty());
    }

    #[test]
    fn unresolving_clears_a_leftover_resolved_note() {
        let dir = tempdir().unwrap();
        let er = dir.path().to_str().unwrap();
        let mut finding = sample_finding("f-1");
        finding.resolved = true;
        finding.resolved_note = "null check added".into();
        finding.resolved_at = "2026-01-01T00:00:00Z".into();
        write_expert_with(er, finding);

        assert!(set_finding_resolved(er, "sec-f-1", false).unwrap());
        let reopened = read_expert_finding(er);
        assert!(!reopened.resolved);
        assert!(reopened.resolved_note.is_empty());
        assert!(reopened.resolved_at.is_empty());
    }

    #[test]
    fn resolving_is_idempotent_and_reports_unknown_ids() {
        let dir = tempdir().unwrap();
        let er = dir.path().to_str().unwrap();
        let mut finding = sample_finding("f-1");
        finding.resolved = true;
        finding.resolved_at = "2026-01-01T00:00:00Z".into();
        write_expert_with(er, finding);

        assert!(set_finding_resolved(er, "sec-f-1", true).unwrap());
        assert_eq!(read_expert_finding(er).resolved_at, "2026-01-01T00:00:00Z");
        assert!(!set_finding_resolved(er, "sec-missing", true).unwrap());
    }
}
