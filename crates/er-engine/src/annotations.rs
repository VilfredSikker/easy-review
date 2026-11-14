//! Annotation use cases shared by the App and the service.
//!
//! A question, a note and a local GitHub comment are the same write with a
//! different store and id prefix. The pieces here are feature-free: the App
//! wraps them with its composer state, and the service calls them from an
//! effect with a diff it fetched off-actor. The id a front end painted arrives
//! as `client_id` and is stored verbatim, so the confirming snapshot finds the
//! row it painted.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use er_store::{ArtifactStore, Scope};

use crate::ai::{self, CommentType};
use crate::git::DiffFile;

/// Anchor data captured at annotation creation time for later relocation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineAnchor {
    pub line_start: Option<usize>,
    pub line_content: String,
    pub context_before: Vec<String>,
    pub context_after: Vec<String>,
    pub old_line_start: Option<usize>,
    pub hunk_header: String,
}

/// Where an annotation written without the composer's input mode is placed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentTarget {
    pub file: String,
    pub hunk_idx: usize,
    pub line_num: Option<usize>,
    /// Inclusive end line for a range annotation; ignored unless past `line_num`.
    pub line_num_end: Option<usize>,
}

/// Who a submit is written as. An AI reply carries the action that wrote it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReplyAttribution {
    pub author: Option<String>,
    pub ai_action: Option<String>,
}

/// The painted id when it matches the kind's prefix, else a fresh one.
///
/// The prefix guard keeps a stale client id from a different kind out of a
/// store: an `n-` id sent with a question would otherwise create a question
/// whose replies and deletes resolve against the notes file.
pub fn take_comment_id(client_id: Option<String>, prefix: &str) -> String {
    match client_id {
        Some(id) if !id.is_empty() && id.starts_with(prefix) => id,
        _ => ai::annotation_store::new_annotation_id(prefix),
    }
}

/// Inclusive end line for a multi-line anchor; `None` when single-line or invalid.
pub const fn normalize_line_end(
    line_start: Option<usize>,
    line_end: Option<usize>,
) -> Option<usize> {
    match (line_start, line_end) {
        (Some(start), Some(end)) if end > start => Some(end),
        _ => None,
    }
}

/// The anchor for a line inside `file`, with three lines of context each way.
///
/// The caller finds the file (and decides which list to look in); this only
/// reads the hunk and line out of it.
pub fn line_anchor_at(
    file: Option<&DiffFile>,
    hunk_idx: usize,
    line: Option<usize>,
    side: Option<&str>,
) -> LineAnchor {
    let Some(df) = file else {
        return LineAnchor::default();
    };
    let Some(hunk) = df.hunks.get(hunk_idx) else {
        return LineAnchor::default();
    };
    let Some(ln) = line else {
        return LineAnchor {
            line_start: None,
            line_content: hunk.header.clone(),
            context_before: Vec::new(),
            context_after: Vec::new(),
            old_line_start: None,
            hunk_header: hunk.header.clone(),
        };
    };

    let target_idx = match side {
        Some("LEFT") => hunk
            .lines
            .iter()
            .position(|l| l.old_num == Some(ln))
            .or_else(|| hunk.lines.iter().position(|l| l.new_num == Some(ln))),
        Some("RIGHT") => hunk
            .lines
            .iter()
            .position(|l| l.new_num == Some(ln))
            .or_else(|| hunk.lines.iter().position(|l| l.old_num == Some(ln))),
        _ => hunk
            .lines
            .iter()
            .position(|l| l.new_num == Some(ln))
            .or_else(|| hunk.lines.iter().position(|l| l.old_num == Some(ln))),
    };
    let (line_content, old_line_start) = match target_idx {
        Some(idx) => {
            let dl = &hunk.lines[idx];
            (dl.content.clone(), dl.old_num)
        }
        None => (String::new(), None),
    };
    let context_before = match target_idx {
        Some(idx) => {
            let start = idx.saturating_sub(3);
            hunk.lines[start..idx]
                .iter()
                .map(|l| l.content.clone())
                .collect()
        }
        None => Vec::new(),
    };
    let context_after = match target_idx {
        Some(idx) => {
            let end = (idx + 4).min(hunk.lines.len());
            hunk.lines[(idx + 1)..end]
                .iter()
                .map(|l| l.content.clone())
                .collect()
        }
        None => Vec::new(),
    };

    LineAnchor {
        line_start: Some(ln),
        line_content,
        context_before,
        context_after,
        old_line_start,
        hunk_header: hunk.header.clone(),
    }
}

/// Anchor on 1-based `line` of a whole document, with the same three lines of
/// context a hunk anchor keeps. `None` when the line is past the end.
pub fn document_line_anchor(source: &str, line: usize) -> Option<LineAnchor> {
    let lines: Vec<&str> = source.lines().collect();
    let idx = line.checked_sub(1).filter(|&i| i < lines.len())?;
    let owned = |range: &[&str]| range.iter().map(|l| (*l).to_string()).collect();
    Some(LineAnchor {
        line_start: Some(line),
        line_content: lines[idx].to_string(),
        context_before: owned(&lines[idx.saturating_sub(3)..idx]),
        context_after: owned(&lines[idx + 1..(idx + 4).min(lines.len())]),
        old_line_start: None,
        hunk_header: String::new(),
    })
}

/// Everything one annotation write needs, with the diff data already resolved.
pub struct AnnotationInput<'a> {
    pub kind: CommentType,
    pub file: &'a str,
    pub hunk_idx: usize,
    pub line_num: Option<usize>,
    pub line_num_end: Option<usize>,
    pub text: &'a str,
    pub side: Option<&'a str>,
    pub reply_to: Option<String>,
    pub finding_ref: Option<String>,
    pub client_id: Option<String>,
    pub attribution: ReplyAttribution,
    pub branch_diff_hash: &'a str,
    pub diff_hash: &'a str,
    /// The target file's parsed diff, when the caller has it.
    pub file_data: Option<&'a DiffFile>,
    pub document_anchor: Option<LineAnchor>,
    /// The parent thread's document anchor, when this is a reply to one.
    pub parent_document_anchor: Option<LineAnchor>,
    pub er_dir: &'a str,
    /// The PR bucket for GitHub comments; unused for questions and notes.
    pub comments_dir: &'a str,
}

/// The stored payload after one write, for the caller to adopt in memory.
pub enum WrittenPayload {
    Questions(ai::ErQuestions),
    Notes(ai::ErNotes),
    Comments(ai::ErGithubComments),
}

pub struct WrittenAnnotation {
    pub id: String,
    pub path: PathBuf,
    pub was_unreadable: bool,
    pub is_reply: bool,
    pub payload: WrittenPayload,
}

/// Resolve the anchor a question or note submit stores: an explicit document
/// anchor wins, then the parent thread's, then the diff line.
fn resolve_anchor(input: &AnnotationInput<'_>) -> (Option<usize>, LineAnchor) {
    let explicit = input
        .document_anchor
        .clone()
        .or_else(|| input.parent_document_anchor.clone());
    match explicit {
        Some(anchor) => (None, anchor),
        None => (
            Some(input.hunk_idx),
            line_anchor_at(input.file_data, input.hunk_idx, input.line_num, input.side),
        ),
    }
}

/// Write one annotation to its store and return the payload the caller adopts.
pub fn write_annotation(
    store: &ArtifactStore,
    input: &AnnotationInput<'_>,
) -> Result<WrittenAnnotation> {
    let id = take_comment_id(
        input.client_id.clone(),
        match input.kind {
            CommentType::Question => "q-",
            CommentType::Note => "n-",
            CommentType::GitHubComment => "c-",
        },
    );
    let timestamp = crate::sync::chrono_now();
    let side = input.side.unwrap_or("RIGHT").to_string();
    let author = input
        .attribution
        .author
        .clone()
        .unwrap_or_else(|| "You".to_string());
    let is_reply = input.reply_to.is_some();

    match input.kind {
        CommentType::Question | CommentType::Note => {
            let (hunk_index, anchor) = resolve_anchor(input);
            let annotation = ai::PrivateAnnotation {
                id: id.clone(),
                timestamp,
                file: input.file.to_string(),
                hunk_index,
                line_start: anchor.line_start,
                line_end: normalize_line_end(anchor.line_start, input.line_num_end),
                line_content: anchor.line_content,
                text: input.text.to_string(),
                resolved: false,
                stale: false,
                context_before: anchor.context_before,
                context_after: anchor.context_after,
                old_line_start: anchor.old_line_start,
                side,
                hunk_header: anchor.hunk_header,
                anchor_status: "original".to_string(),
                relocated_at_hash: input.diff_hash.to_string(),
                in_reply_to: input.reply_to.clone(),
                author,
                promoted_to: None,
                finding_ref: input.finding_ref.clone(),
                ai_action: input.attribution.ai_action.clone(),
            };
            let scope = Scope::dir(input.er_dir)?;
            match input.kind {
                CommentType::Question => {
                    let path = ai::annotation_store::path_of::<ai::ErQuestions>(store, &scope)?;
                    let updated =
                        ai::annotation_store::update_questions(store, &scope, |current| {
                            let mut questions = current.unwrap_or_else(|| ai::ErQuestions {
                                version: 1,
                                diff_hash: input.branch_diff_hash.to_string(),
                                questions: Vec::new(),
                            });
                            questions.diff_hash = input.branch_diff_hash.to_string();
                            questions.questions.push(annotation);
                            Ok::<_, std::convert::Infallible>(questions)
                        })
                        .map_err(|error| anyhow!("{error}"))?;
                    Ok(WrittenAnnotation {
                        id,
                        path,
                        was_unreadable: updated.was_unreadable,
                        is_reply,
                        payload: WrittenPayload::Questions(updated.value),
                    })
                }
                _ => {
                    let path = ai::annotation_store::path_of::<ai::ErNotes>(store, &scope)?;
                    let updated = ai::annotation_store::update_notes(store, &scope, |current| {
                        let mut notes = current.unwrap_or_else(|| ai::ErNotes {
                            version: 1,
                            diff_hash: input.branch_diff_hash.to_string(),
                            notes: Vec::new(),
                        });
                        notes.diff_hash = input.branch_diff_hash.to_string();
                        notes.notes.push(annotation);
                        Ok::<_, std::convert::Infallible>(notes)
                    })
                    .map_err(|error| anyhow!("{error}"))?;
                    Ok(WrittenAnnotation {
                        id,
                        path,
                        was_unreadable: updated.was_unreadable,
                        is_reply,
                        payload: WrittenPayload::Notes(updated.value),
                    })
                }
            }
        }
        CommentType::GitHubComment => {
            // A GitHub comment must sit on a diff line; a document anchor is
            // private-only, so this path always uses the line anchor.
            let anchor =
                line_anchor_at(input.file_data, input.hunk_idx, input.line_num, input.side);
            let comment = ai::GithubComment {
                id: id.clone(),
                timestamp,
                file: input.file.to_string(),
                hunk_index: Some(input.hunk_idx),
                line_start: anchor.line_start,
                line_end: normalize_line_end(anchor.line_start, input.line_num_end),
                line_content: anchor.line_content,
                comment: input.text.to_string(),
                in_reply_to: input.reply_to.clone(),
                resolved: false,
                source: "local".to_string(),
                github_id: None,
                author,
                outdated: false,
                stale: false,
                context_before: anchor.context_before,
                context_after: anchor.context_after,
                old_line_start: anchor.old_line_start,
                hunk_header: anchor.hunk_header,
                anchor_status: "original".to_string(),
                relocated_at_hash: input.diff_hash.to_string(),
                finding_ref: input.finding_ref.clone(),
                side,
            };
            let scope = Scope::dir(input.comments_dir)?;
            let path = ai::annotation_store::path_of::<ai::ErGithubComments>(store, &scope)?;
            let updated = ai::annotation_store::update_github_comments(store, &scope, |current| {
                let mut gc = current.unwrap_or_else(|| ai::ErGithubComments {
                    version: 1,
                    diff_hash: input.branch_diff_hash.to_string(),
                    github: None,
                    comments: Vec::new(),
                });
                gc.diff_hash = input.branch_diff_hash.to_string();
                gc.comments.push(comment);
                Ok::<_, std::convert::Infallible>(gc)
            })
            .map_err(|error| anyhow!("{error}"))?;
            Ok(WrittenAnnotation {
                id,
                path,
                was_unreadable: updated.was_unreadable,
                is_reply,
                payload: WrittenPayload::Comments(updated.value),
            })
        }
    }
}

/// Apply one reviewed mark to the map: insert with the file's current hash, or
/// remove. Returns whether the map changed.
pub fn apply_reviewed(
    reviewed: &mut HashMap<String, String>,
    path: &str,
    hash: String,
    reviewed_state: bool,
) -> bool {
    if reviewed_state {
        let changed = reviewed.get(path) != Some(&hash);
        reviewed.insert(path.to_string(), hash);
        changed
    } else {
        reviewed.remove(path).is_some()
    }
}
