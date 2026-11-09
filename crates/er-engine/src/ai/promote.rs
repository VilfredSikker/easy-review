//! The one conversion from a private annotation to something a GitHub write
//! accepts.
//!
//! `PromotedComment` has private fields, so only the functions here build one.
//! There is no `From`/`Into` between `PrivateAnnotation` and the GitHub write
//! types; the compile-fail doctests below pin that.

use crate::ai::annotation_store;
use crate::ai::Finding;

/// An owned comment the GitHub post path can carry.
///
/// Only `ai::promote` builds one; its fields are private.
///
/// ```compile_fail
/// use er_engine::ai::promote::PromotedComment;
/// let _ = PromotedComment {
///     path: String::new(),
///     hunk_index: None,
///     line_start: 1,
///     line_end: None,
///     side: String::new(),
///     body: String::new(),
/// };
/// ```
///
/// ```compile_fail
/// use er_engine::ai::PrivateAnnotation;
/// use er_engine::app::NewGitHubComment;
/// let question: PrivateAnnotation = todo!();
/// let _: NewGitHubComment = question.into();
/// ```
///
/// ```compile_fail
/// use er_engine::ai::PrivateAnnotation;
/// use er_github::NewLineComment;
/// let question: PrivateAnnotation = todo!();
/// let _ = NewLineComment::from(&question);
/// ```
///
/// ```compile_fail
/// use er_engine::ai::PrivateAnnotation;
/// use er_engine::app::NewGitHubComment;
/// let question: PrivateAnnotation = todo!();
/// let _ = NewGitHubComment::from(&question);
/// ```
///
/// ```compile_fail
/// use er_engine::ai::PrivateAnnotation;
/// use er_github::NewLineComment;
/// let question: PrivateAnnotation = todo!();
/// let _ = NewLineComment::from(question);
/// ```
///
/// ```compile_fail
/// use er_engine::ai::{GithubComment, PrivateAnnotation};
/// let comment: GithubComment = todo!();
/// let _: PrivateAnnotation = comment.into();
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromotedComment {
    path: String,
    hunk_index: Option<usize>,
    line_start: usize,
    line_end: Option<usize>,
    side: String,
    body: String,
}

impl PromotedComment {
    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn hunk_index(&self) -> Option<usize> {
        self.hunk_index
    }

    pub fn line_start(&self) -> usize {
        self.line_start
    }

    pub fn line_end(&self) -> Option<usize> {
        self.line_end
    }

    pub fn side(&self) -> &str {
        &self.side
    }

    pub fn body(&self) -> &str {
        &self.body
    }
}

/// The anchor a promoted note gets from the current diff.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PromotionAnchor {
    pub line_content: String,
    pub context_before: Vec<String>,
    pub context_after: Vec<String>,
    pub old_line_start: Option<usize>,
    pub hunk_header: String,
}

/// Why a private annotation cannot become a GitHub comment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromoteError {
    /// Anchored to a document line outside the diff (ADR 0041).
    DocumentAnchor,
    /// No diff line to anchor a GitHub comment to.
    NoLine,
}

impl std::fmt::Display for PromoteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DocumentAnchor => {
                f.write_str("This line is not in the diff, so it cannot become a GitHub comment")
            }
            Self::NoLine => f.write_str(
                "This item has no line in the diff, so it cannot become a GitHub comment",
            ),
        }
    }
}

impl std::error::Error for PromoteError {}

/// Turn a private annotation into the comment the GitHub post path carries.
///
/// The body is the annotation's text plus every reply quoted under its author.
/// A document-anchored source is refused (ADR 0041); a source with no diff line
/// is refused instead of being posted at line 1.
pub fn promote_to_github(
    source: &crate::ai::PrivateAnnotation,
    thread: &[crate::ai::PrivateAnnotation],
    body: Option<String>,
) -> Result<PromotedComment, PromoteError> {
    if source.is_document_anchor() {
        return Err(PromoteError::DocumentAnchor);
    }
    let line_start = source.line_start.ok_or(PromoteError::NoLine)?;
    let replies = replies_of(source, thread);
    let body = body.unwrap_or_else(|| build_promoted_body(&source.text, &replies));
    Ok(PromotedComment {
        path: source.file.clone(),
        hunk_index: source.hunk_index,
        line_start,
        line_end: None,
        side: normal_side(&source.side),
        body,
    })
}

/// Turn a question into a note, keeping a document anchor.
///
/// The caller supplies the anchor recomputed from the current diff (or the
/// source's own fields for a document anchor), the file (the source's, or the
/// selected diff file when it is empty) and the current diff hash.
pub fn promote_to_note(
    source: &crate::ai::PrivateAnnotation,
    thread: &[crate::ai::PrivateAnnotation],
    body: Option<String>,
    file: String,
    anchor: PromotionAnchor,
    diff_hash: String,
) -> crate::ai::PrivateAnnotation {
    let replies = replies_of(source, thread);
    let text = body.unwrap_or_else(|| build_promoted_body(&source.text, &replies));
    let hunk_index = if source.is_document_anchor() {
        None
    } else {
        source.hunk_index
    };
    crate::ai::PrivateAnnotation {
        id: annotation_store::new_annotation_id("n-"),
        timestamp: crate::sync::chrono_now(),
        file,
        hunk_index,
        line_start: source.line_start,
        line_end: None,
        line_content: anchor.line_content,
        text,
        resolved: false,
        stale: false,
        context_before: anchor.context_before,
        context_after: anchor.context_after,
        old_line_start: anchor.old_line_start,
        side: normal_side(&source.side),
        hunk_header: anchor.hunk_header,
        anchor_status: "original".to_string(),
        relocated_at_hash: diff_hash,
        in_reply_to: None,
        author: "You".to_string(),
        promoted_to: None,
        finding_ref: None,
        ai_action: None,
    }
}

/// Turn an AI finding into the comment the GitHub post path carries.
pub fn promote_finding(
    file: &str,
    finding: &Finding,
    replies: &[(String, String)],
    body: Option<String>,
) -> Result<PromotedComment, PromoteError> {
    let line_start = finding.line_start.ok_or(PromoteError::NoLine)?;
    let default = if finding.description.is_empty() {
        finding.title.clone()
    } else {
        format!("{}\n\n{}", finding.title, finding.description)
    };
    let default = crate::ai::append_promote_replies(default, replies);
    let body = body.unwrap_or(default);
    Ok(PromotedComment {
        path: file.to_string(),
        hunk_index: finding.hunk_index,
        line_start,
        line_end: None,
        side: "RIGHT".to_string(),
        body,
    })
}

/// Compose a markdown body: the root text, then each reply quoted under its
/// author.
fn build_promoted_body(root_text: &str, replies: &[(String, String)]) -> String {
    let mut out = root_text.trim().to_string();
    for (author, body) in replies {
        out.push_str("\n\n");
        let quoted: String = body
            .lines()
            .map(|line| format!("> {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        out.push_str(&format!("> **{author}** replied:\n{quoted}"));
    }
    out
}

fn replies_of(
    source: &crate::ai::PrivateAnnotation,
    thread: &[crate::ai::PrivateAnnotation],
) -> Vec<(String, String)> {
    thread
        .iter()
        .filter(|reply| reply.in_reply_to.as_deref() == Some(source.id.as_str()))
        .map(|reply| (reply.author.clone(), reply.text.clone()))
        .collect()
}

fn normal_side(side: &str) -> String {
    if side.is_empty() {
        "RIGHT".to_string()
    } else {
        side.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{Finding, PrivateAnnotation};

    fn annotation(id: &str) -> PrivateAnnotation {
        PrivateAnnotation {
            id: id.to_string(),
            timestamp: String::new(),
            file: "a.rs".to_string(),
            hunk_index: Some(0),
            line_start: Some(4),
            line_end: None,
            line_content: "line".to_string(),
            text: "text".to_string(),
            resolved: false,
            stale: false,
            context_before: Vec::new(),
            context_after: Vec::new(),
            old_line_start: None,
            side: "RIGHT".to_string(),
            hunk_header: String::new(),
            anchor_status: "original".to_string(),
            relocated_at_hash: String::new(),
            in_reply_to: None,
            author: "You".to_string(),
            promoted_to: None,
            finding_ref: None,
            ai_action: None,
        }
    }

    fn document_annotation() -> PrivateAnnotation {
        let mut annotation = annotation("q-1");
        annotation.hunk_index = None;
        annotation.line_start = Some(6);
        annotation.line_content = "delta".to_string();
        annotation.context_before =
            vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()];
        annotation
    }

    fn finding() -> Finding {
        serde_json::from_value(serde_json::json!({
            "id": "f-1",
            "severity": "info",
            "title": "Null deref",
            "description": "desc",
            "hunk_index": 1,
            "line_start": 4
        }))
        .unwrap()
    }

    #[test]
    fn promoted_body_quotes_each_reply_with_its_author() {
        let source = annotation("q-1");
        let mut reply = annotation("q-2");
        reply.in_reply_to = Some("q-1".to_string());
        reply.author = "AI".to_string();
        reply.text = "reply".to_string();
        let promoted = promote_to_github(&source, &[source.clone(), reply], None).unwrap();
        assert!(promoted.body().contains("> **AI** replied:\n> reply"));
    }

    #[test]
    fn promoting_a_document_anchor_to_github_is_refused() {
        let source = document_annotation();
        assert_eq!(
            promote_to_github(&source, std::slice::from_ref(&source), None).unwrap_err(),
            PromoteError::DocumentAnchor
        );
    }

    #[test]
    fn promoting_a_question_without_a_line_is_refused() {
        let mut source = annotation("q-1");
        source.line_start = None;
        assert_eq!(
            promote_to_github(&source, std::slice::from_ref(&source), None).unwrap_err(),
            PromoteError::NoLine
        );
    }

    #[test]
    fn promoting_a_finding_without_a_line_is_refused() {
        let mut finding = finding();
        finding.line_start = None;
        assert_eq!(
            promote_finding("a.rs", &finding, &[], None).unwrap_err(),
            PromoteError::NoLine
        );
    }

    #[test]
    fn promoting_to_a_note_keeps_the_document_anchor() {
        let source = document_annotation();
        let anchor = PromotionAnchor {
            line_content: source.line_content.clone(),
            context_before: source.context_before.clone(),
            context_after: source.context_after.clone(),
            old_line_start: source.old_line_start,
            hunk_header: source.hunk_header.clone(),
        };
        let note = promote_to_note(
            &source,
            std::slice::from_ref(&source),
            Some("Rename delta".to_string()),
            "README.md".to_string(),
            anchor,
            "hash".to_string(),
        );
        assert_eq!(note.hunk_index, None);
        assert_eq!(note.line_start, Some(6));
        assert_eq!(note.line_content, "delta");
        assert_eq!(note.context_before, vec!["alpha", "beta", "gamma"]);
        assert_eq!(note.text, "Rename delta");
        assert_eq!(note.side, "RIGHT");
        assert_eq!(note.anchor_status, "original");
        assert_eq!(note.relocated_at_hash, "hash");
    }

    #[test]
    fn promote_to_note_generates_an_n_id() {
        let source = annotation("q-1");
        let note = promote_to_note(
            &source,
            std::slice::from_ref(&source),
            None,
            "a.rs".to_string(),
            PromotionAnchor::default(),
            "hash".to_string(),
        );
        assert!(note.id.starts_with("n-"));
    }

    #[test]
    fn promote_finding_builds_a_comment_for_the_finding_line() {
        let promoted = promote_finding("a.rs", &finding(), &[], None).unwrap();
        assert_eq!(promoted.path(), "a.rs");
        assert_eq!(promoted.line_start(), 4);
        assert_eq!(promoted.hunk_index(), Some(1));
        assert_eq!(promoted.side(), "RIGHT");
        assert!(promoted.body().contains("Null deref"));
    }

    #[test]
    fn promote_finding_appends_its_replies() {
        let replies = vec![("AI".to_string(), "reply".to_string())];
        let promoted = promote_finding("a.rs", &finding(), &replies, None).unwrap();
        assert!(promoted.body().contains("> **AI** replied:\n> reply"));
    }
}
