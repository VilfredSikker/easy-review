use serde::{Deserialize, Serialize};

// ── Comment type discriminator ──

/// Distinguishes between personal review questions, local notes, and GitHub PR comments
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommentType {
    /// Personal internal question (stored in questions.json)
    Question,
    /// Local actionable note (stored in notes.json) — private, like a question,
    /// but framed as an instruction to hand off to an agent.
    Note,
    /// GitHub PR comment (stored in github-comments.json)
    GitHubComment,
}

/// Type of navigable hint for unified J/K navigation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HintType {
    Question,
    Note,
    GitHubComment,
    Finding,
}

/// Unified reference to a question, a note, or a GitHub comment.
/// Used by query methods and UI rendering to handle all three uniformly.
///
/// Notes reuse the [`PrivateAnnotation`] struct (their fields are identical); the
/// `Note` variant exists so callers can distinguish a note from a question for
/// icons, colors, and snapshot `kind` tagging.
#[derive(Debug, Clone)]
pub enum CommentRef<'a> {
    Question(&'a PrivateAnnotation),
    Note(&'a PrivateAnnotation),
    GithubComment(&'a GithubComment),
}

impl CommentRef<'_> {
    pub fn id(&self) -> &str {
        match self {
            CommentRef::Question(q) | CommentRef::Note(q) => &q.id,
            CommentRef::GithubComment(c) => &c.id,
        }
    }

    pub const fn comment_type(&self) -> CommentType {
        match self {
            CommentRef::Question(_) => CommentType::Question,
            CommentRef::Note(_) => CommentType::Note,
            CommentRef::GithubComment(_) => CommentType::GitHubComment,
        }
    }

    pub fn text(&self) -> &str {
        match self {
            CommentRef::Question(q) | CommentRef::Note(q) => &q.text,
            CommentRef::GithubComment(c) => &c.comment,
        }
    }

    pub fn author(&self) -> &str {
        match self {
            CommentRef::Question(q) | CommentRef::Note(q) => {
                if q.author.is_empty() {
                    "You"
                } else {
                    &q.author
                }
            }
            CommentRef::GithubComment(c) => {
                if c.author.is_empty() {
                    "You"
                } else {
                    &c.author
                }
            }
        }
    }

    pub fn timestamp(&self) -> &str {
        match self {
            CommentRef::Question(q) | CommentRef::Note(q) => &q.timestamp,
            CommentRef::GithubComment(c) => &c.timestamp,
        }
    }

    /// Whether this comment is a GitHub comment that came from the PR. Local
    /// comments are private AI follow-ups and never appear on GitHub.
    pub fn is_on_github(&self) -> bool {
        match self {
            CommentRef::Question(_) | CommentRef::Note(_) => false,
            CommentRef::GithubComment(c) => c.source == "github",
        }
    }

    pub const fn is_resolved(&self) -> bool {
        match self {
            CommentRef::Question(q) | CommentRef::Note(q) => q.resolved,
            CommentRef::GithubComment(c) => c.resolved,
        }
    }

    pub const fn is_stale(&self) -> bool {
        match self {
            CommentRef::Question(q) | CommentRef::Note(q) => q.stale,
            CommentRef::GithubComment(c) => c.stale || c.outdated,
        }
    }

    pub fn in_reply_to(&self) -> Option<&str> {
        match self {
            CommentRef::Question(q) | CommentRef::Note(q) => q.in_reply_to.as_deref(),
            CommentRef::GithubComment(c) => c.in_reply_to.as_deref(),
        }
    }

    pub fn file(&self) -> &str {
        match self {
            CommentRef::Question(q) | CommentRef::Note(q) => &q.file,
            CommentRef::GithubComment(c) => &c.file,
        }
    }

    pub const fn hunk_index(&self) -> Option<usize> {
        match self {
            CommentRef::Question(q) | CommentRef::Note(q) => q.hunk_index,
            CommentRef::GithubComment(c) => c.hunk_index,
        }
    }

    pub const fn line_start(&self) -> Option<usize> {
        match self {
            CommentRef::Question(q) | CommentRef::Note(q) => q.line_start,
            CommentRef::GithubComment(c) => c.line_start,
        }
    }

    pub const fn line_end(&self) -> Option<usize> {
        match self {
            CommentRef::Question(q) | CommentRef::Note(q) => q.line_end,
            CommentRef::GithubComment(c) => c.line_end,
        }
    }

    pub const fn old_line_start(&self) -> Option<usize> {
        match self {
            CommentRef::Question(q) | CommentRef::Note(q) => q.old_line_start,
            CommentRef::GithubComment(c) => c.old_line_start,
        }
    }

    /// GitHub review side. `LEFT` means `line_start` is an old-side number.
    pub const fn side(&self) -> Option<&str> {
        match self {
            CommentRef::GithubComment(c) => Some(c.side.as_str()),
            CommentRef::Question(q) | CommentRef::Note(q) => Some(q.side.as_str()),
        }
    }

    pub fn anchor_status(&self) -> &str {
        match self {
            CommentRef::Question(q) | CommentRef::Note(q) => &q.anchor_status,
            CommentRef::GithubComment(c) => &c.anchor_status,
        }
    }

    /// Whether this comment can be replied to (top-level comments/questions, not replies themselves)
    pub const fn can_reply(&self) -> bool {
        match self {
            CommentRef::Question(q) | CommentRef::Note(q) => q.in_reply_to.is_none(),
            CommentRef::GithubComment(c) => c.in_reply_to.is_none(),
        }
    }

    /// Whether this comment can be deleted by the user
    pub fn can_delete(&self) -> bool {
        match self {
            CommentRef::Question(_) | CommentRef::Note(_) => true,
            // Local comments are stored with author "You" — ownership is inferred from
            // that display name, not from the GitHub identity.
            CommentRef::GithubComment(c) => c.source != "github" || c.author == "You",
        }
    }
}

// ── .er-questions.json — personal review notes ──

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ErQuestions {
    pub version: u32,
    pub diff_hash: String,
    #[serde(default)]
    pub questions: Vec<PrivateAnnotation>,
}

// ── notes.json — local actionable notes ──
//
// Notes are private (never pushed to GitHub) like questions, but framed as
// instructions to hand off to a coding agent. They reuse the `PrivateAnnotation`
// shape so all comment machinery (anchoring, replies, staleness) works
// unchanged; the separate file + `n-` id prefix keep them distinct from
// questions.

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ErNotes {
    pub version: u32,
    pub diff_hash: String,
    #[serde(default)]
    pub notes: Vec<PrivateAnnotation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PrivateAnnotation {
    pub id: String,
    #[serde(default)]
    pub timestamp: String,
    pub file: String,
    pub hunk_index: Option<usize>,
    pub line_start: Option<usize>,
    /// Inclusive end line when the question spans multiple diff lines (same side as `line_start`).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub line_end: Option<usize>,
    #[serde(default)]
    pub line_content: String,
    pub text: String,
    #[serde(default)]
    pub resolved: bool,
    /// Runtime-only staleness flag (not persisted)
    #[serde(skip)]
    pub stale: bool,
    /// Up to 3 content lines before the target line in the same hunk
    #[serde(default)]
    pub context_before: Vec<String>,
    /// Up to 3 content lines after the target line in the same hunk
    #[serde(default)]
    pub context_after: Vec<String>,
    /// Old-side line number from diff at creation time
    #[serde(default)]
    pub old_line_start: Option<usize>,
    /// "LEFT" for old-side (deleted lines) or "RIGHT" for new-side/unified (default)
    #[serde(default = "default_review_side")]
    pub side: String,
    /// Hunk header string at creation time
    #[serde(default)]
    pub hunk_header: String,
    /// "original" | "relocated" | "lost"
    #[serde(default = "default_anchor_status")]
    // Matched elsewhere against the string values "original"/"relocated"/"lost";
    // any other value falls through unmatched.
    pub anchor_status: String,
    /// Diff hash when this comment was last relocated
    #[serde(default)]
    pub relocated_at_hash: String,
    /// ID of the question this is a reply to (None = top-level question)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub in_reply_to: Option<String>,
    /// Author display name (defaults to "You")
    #[serde(default = "default_author")]
    pub author: String,
    /// ID of the GitHub comment this question was promoted to (if any).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub promoted_to: Option<String>,
    /// When set, this question thread is tied to an AI finding (`prof-*`, etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub finding_ref: Option<String>,
    /// Which AI action wrote this reply (`AI_ACTION_ASK`, `AI_ACTION_ELABORATE`);
    /// `None` for a person's text. A string rather than an enum so a sidecar
    /// written by a newer build with an action this one does not know still
    /// loads instead of failing the whole file.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub ai_action: Option<String>,
}

/// An AI reply that answers the question or note it hangs off.
pub const AI_ACTION_ASK: &str = "ask";
/// An AI reply that rewrites its question or note into a self-contained one.
pub const AI_ACTION_ELABORATE: &str = "elaborate";

impl PrivateAnnotation {
    /// Anchored to a line of the document rather than a hunk: written from the
    /// desktop preview on a line the diff does not show. Relocation searches
    /// hunks, so it would mark these lost on every pass; it leaves them alone.
    pub fn is_document_anchor(&self) -> bool {
        self.hunk_index.is_none() && self.line_start.is_some()
    }

    /// No anchor at all: neither a hunk nor a line.
    pub fn is_file_level(&self) -> bool {
        self.hunk_index.is_none() && self.line_start.is_none() && self.hunk_header.is_empty()
    }
}

// ── .er-github-comments.json — GitHub PR comments ──

#[derive(Debug, Clone, Serialize, Deserialize, Default, schemars::JsonSchema)]
pub struct GitHubSyncState {
    #[serde(default)]
    pub pr_number: Option<u64>,
    #[serde(default)]
    pub owner: String,
    #[serde(default)]
    pub repo: String,
    #[serde(default)]
    pub last_synced: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ErGithubComments {
    pub version: u32,
    pub diff_hash: String,
    #[serde(default)]
    pub github: Option<GitHubSyncState>,
    #[serde(default)]
    pub comments: Vec<GithubComment>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GithubComment {
    pub id: String,
    #[serde(default)]
    pub timestamp: String,
    pub file: String,
    pub hunk_index: Option<usize>,
    pub line_start: Option<usize>,
    pub line_end: Option<usize>,
    #[serde(default)]
    pub line_content: String,
    pub comment: String,
    #[serde(default)]
    pub in_reply_to: Option<String>,
    #[serde(default)]
    pub resolved: bool,
    /// "local" | "github"
    #[serde(default = "default_source")]
    pub source: String,
    /// GitHub comment ID (for sync/dedup)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub github_id: Option<u64>,
    /// Author display name ("You" for local, GitHub login for remote)
    #[serde(default = "default_author")]
    pub author: String,
    /// Persisted GitHub review-thread outdated state.
    #[serde(default)]
    pub outdated: bool,
    /// Runtime-only staleness flag (not persisted)
    #[serde(skip)]
    pub stale: bool,
    /// Up to 3 content lines before the target line in the same hunk
    #[serde(default)]
    pub context_before: Vec<String>,
    /// Up to 3 content lines after the target line in the same hunk
    #[serde(default)]
    pub context_after: Vec<String>,
    /// Old-side line number from diff at creation time
    #[serde(default)]
    pub old_line_start: Option<usize>,
    /// Hunk header string at creation time
    #[serde(default)]
    pub hunk_header: String,
    /// "original" | "relocated" | "lost"
    #[serde(default = "default_anchor_status")]
    pub anchor_status: String,
    /// Diff hash when this comment was last relocated
    #[serde(default)]
    pub relocated_at_hash: String,
    /// Optional reference to an AI finding this comment responds to
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub finding_ref: Option<String>,
    /// "LEFT" for old-side (deleted lines) or "RIGHT" for new-side/unified (default)
    #[serde(default = "default_review_side")]
    pub side: String,
}

fn default_source() -> String {
    "local".to_string()
}

fn default_review_side() -> String {
    "RIGHT".to_string()
}

fn default_anchor_status() -> String {
    "original".to_string()
}

fn default_author() -> String {
    "You".to_string()
}

// ── Artifact kinds ──
//
// These three files stay Bare in 1.0: agents rewrite them in place, so the
// store writes today's payload JSON with no envelope.

impl er_store::Artifact for ErQuestions {
    const KIND: &'static str = "questions";
    const VERSION: u32 = 1;
    const FILE: &'static str = "questions.json";
    type Slot = ();

    fn diff_hash(&self) -> Option<&str> {
        Some(&self.diff_hash)
    }
}

impl er_store::Artifact for ErNotes {
    const KIND: &'static str = "notes";
    const VERSION: u32 = 1;
    const FILE: &'static str = "notes.json";
    type Slot = ();

    fn diff_hash(&self) -> Option<&str> {
        Some(&self.diff_hash)
    }
}

impl er_store::Artifact for ErGithubComments {
    const KIND: &'static str = "github-comments";
    const VERSION: u32 = 1;
    const FILE: &'static str = "github-comments.json";
    type Slot = ();

    fn diff_hash(&self) -> Option<&str> {
        Some(&self.diff_hash)
    }
}

/// Top-level GitHub comment eligible for batch validate / re-anchor.
pub const fn github_comment_eligible_for_batch_validate(c: &GithubComment) -> bool {
    !c.resolved && !c.outdated && c.in_reply_to.is_none() && c.line_start.is_some()
}

/// Count GitHub comments eligible for batch validate.
pub fn count_eligible_github_comments(gc: &ErGithubComments) -> usize {
    gc.comments
        .iter()
        .filter(|c| github_comment_eligible_for_batch_validate(c))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_question() -> PrivateAnnotation {
        PrivateAnnotation {
            id: "q-1".into(),
            timestamp: "2026-01-01T00:00:00Z".into(),
            file: "src/foo.rs".into(),
            hunk_index: Some(0),
            line_start: Some(10),
            line_end: None,
            line_content: "fn foo() {}".into(),
            text: "Why this name?".into(),
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
            finding_ref: None,
            ai_action: None,
        }
    }

    fn sample_github_comment() -> GithubComment {
        GithubComment {
            id: "gh-1".into(),
            timestamp: "2026-01-01T00:00:00Z".into(),
            file: "src/foo.rs".into(),
            hunk_index: Some(0),
            line_start: Some(10),
            line_end: None,
            line_content: "fn foo() {}".into(),
            comment: "This changed upstream".into(),
            in_reply_to: None,
            resolved: false,
            source: "github".into(),
            github_id: Some(1),
            author: "octo".into(),
            outdated: false,
            stale: false,
            context_before: vec![],
            context_after: vec![],
            old_line_start: None,
            hunk_header: String::new(),
            anchor_status: "original".into(),
            relocated_at_hash: String::new(),
            finding_ref: None,
            side: "RIGHT".into(),
        }
    }

    #[test]
    fn ai_action_is_optional_on_disk() {
        let plain = serde_json::to_string(&sample_question()).unwrap();
        assert!(
            !plain.contains("ai_action"),
            "a person's text writes no action"
        );
        let read: PrivateAnnotation = serde_json::from_str(&plain).unwrap();
        assert_eq!(read.ai_action, None);

        let mut reply = sample_question();
        reply.ai_action = Some("a-later-action".into());
        let json = serde_json::to_string(&reply).unwrap();
        let read: PrivateAnnotation = serde_json::from_str(&json).unwrap();
        assert_eq!(read.ai_action.as_deref(), Some("a-later-action"));
    }

    #[test]
    fn review_question_promoted_to_roundtrips_via_serde() {
        let mut q = sample_question();
        q.promoted_to = Some("c-42".into());

        let json = serde_json::to_string(&q).unwrap();
        let back: PrivateAnnotation = serde_json::from_str(&json).unwrap();

        assert_eq!(back.promoted_to.as_deref(), Some("c-42"));
    }

    #[test]
    fn github_comment_outdated_roundtrips_via_serde() {
        let mut c = sample_github_comment();
        c.outdated = true;

        let json = serde_json::to_string(&c).unwrap();
        let back: GithubComment = serde_json::from_str(&json).unwrap();

        assert!(back.outdated);
        assert!(!back.stale, "runtime stale state must not be persisted");
    }

    #[test]
    fn github_comment_outdated_defaults_to_false_when_missing() {
        let json = r#"{
            "id": "gh-1",
            "file": "src/foo.rs",
            "hunk_index": 0,
            "line_start": 10,
            "comment": "This changed upstream"
        }"#;

        let c: GithubComment = serde_json::from_str(json).unwrap();

        assert!(!c.outdated);
    }

    #[test]
    fn github_comments_json_with_a_synced_field_still_loads() {
        // Old sidecars carry `synced`; serde ignores the unknown field.
        let c: GithubComment = serde_json::from_str(
            r#"{"id":"c","file":"a.rs","hunk_index":0,"line_start":1,"comment":"x","synced":true}"#,
        )
        .unwrap();
        assert_eq!(c.id, "c");
    }

    #[test]
    fn comment_ref_github_comment_reports_github_type_and_delete_rules() {
        let mut c = sample_github_comment();
        c.source = "github".into();
        c.author = "octo".into();

        let r = CommentRef::GithubComment(&c);
        assert_eq!(r.comment_type(), CommentType::GitHubComment);
        assert!(r.is_on_github());
        assert!(!r.can_delete());

        let mut local = sample_github_comment();
        local.source = "local".into();
        local.author = "You".into();

        let r = CommentRef::GithubComment(&local);
        assert!(!r.is_on_github());
        assert!(r.can_delete());
    }

    #[test]
    fn github_comment_eligible_for_batch_validate_includes_active_line_comment() {
        assert!(github_comment_eligible_for_batch_validate(
            &sample_github_comment()
        ));
    }

    #[test]
    fn github_comment_eligible_excludes_resolved_outdated_reply_and_file_level() {
        let mut c = sample_github_comment();
        c.resolved = true;
        assert!(!github_comment_eligible_for_batch_validate(&c));

        c = sample_github_comment();
        c.resolved = false;
        c.outdated = true;
        assert!(!github_comment_eligible_for_batch_validate(&c));

        c = sample_github_comment();
        c.outdated = false;
        c.in_reply_to = Some("parent".into());
        assert!(!github_comment_eligible_for_batch_validate(&c));

        c = sample_github_comment();
        c.in_reply_to = None;
        c.line_start = None;
        assert!(!github_comment_eligible_for_batch_validate(&c));
    }

    #[test]
    fn count_eligible_github_comments_counts_roots_only() {
        let gc = ErGithubComments {
            version: 1,
            diff_hash: "h".into(),
            github: None,
            comments: vec![sample_github_comment(), {
                let mut reply = sample_github_comment();
                reply.id = "gh-2".into();
                reply.in_reply_to = Some("gh-1".into());
                reply
            }],
        };
        assert_eq!(count_eligible_github_comments(&gc), 1);
    }

    #[test]
    fn er_notes_roundtrips_via_serde() {
        let notes = ErNotes {
            version: 1,
            diff_hash: "abc".into(),
            notes: vec![{
                let mut n = sample_question();
                n.id = "n-1".into();
                n.text = "Wire this up to the new endpoint".into();
                n
            }],
        };
        let json = serde_json::to_string(&notes).unwrap();
        let back: ErNotes = serde_json::from_str(&json).unwrap();
        assert_eq!(back.notes.len(), 1);
        assert_eq!(back.notes[0].id, "n-1");
    }

    #[test]
    fn comment_ref_note_reports_note_type_but_shares_accessors() {
        let mut n = sample_question();
        n.id = "n-1".into();
        n.text = "Refactor before merge".into();
        let cref = CommentRef::Note(&n);
        assert_eq!(cref.comment_type(), CommentType::Note);
        assert_eq!(cref.id(), "n-1");
        assert_eq!(cref.text(), "Refactor before merge");
        // Notes are local-only, like questions.
        assert!(!cref.is_on_github());
        assert!(cref.can_delete());
    }

    #[test]
    fn review_question_promoted_to_defaults_to_none_when_missing() {
        let json = r#"{
            "id": "q-1",
            "file": "src/foo.rs",
            "hunk_index": 0,
            "line_start": 10,
            "text": "Hi?"
        }"#;
        let q: PrivateAnnotation = serde_json::from_str(json).unwrap();
        assert!(q.promoted_to.is_none());
    }
}
