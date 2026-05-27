use serde::{Deserialize, Serialize};

// ── Comment type discriminator ──

/// Distinguishes between personal review questions and GitHub PR comments
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CommentType {
    /// Personal internal question (stored in .er-questions.json)
    Question,
    /// GitHub PR comment (stored in .er-github-comments.json)
    GitHubComment,
}

/// Type of navigable hint for unified J/K navigation
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HintType {
    Question,
    GitHubComment,
    Finding,
}

/// Unified reference to either a question, GitHub comment, or legacy comment.
/// Used by query methods and UI rendering to handle both types uniformly.
#[derive(Debug, Clone)]
pub enum CommentRef<'a> {
    Question(&'a ReviewQuestion),
    GitHubComment(&'a GitHubReviewComment),
    Legacy(&'a FeedbackComment),
}

impl<'a> CommentRef<'a> {
    pub fn id(&self) -> &str {
        match self {
            CommentRef::Question(q) => &q.id,
            CommentRef::GitHubComment(c) => &c.id,
            CommentRef::Legacy(c) => &c.id,
        }
    }

    pub fn comment_type(&self) -> CommentType {
        match self {
            CommentRef::Question(_) => CommentType::Question,
            CommentRef::GitHubComment(_) | CommentRef::Legacy(_) => CommentType::GitHubComment,
        }
    }

    pub fn text(&self) -> &str {
        match self {
            CommentRef::Question(q) => &q.text,
            CommentRef::GitHubComment(c) => &c.comment,
            CommentRef::Legacy(c) => &c.comment,
        }
    }

    pub fn author(&self) -> &str {
        match self {
            CommentRef::Question(q) => {
                if q.author.is_empty() {
                    "You"
                } else {
                    &q.author
                }
            }
            CommentRef::GitHubComment(c) => {
                if c.author.is_empty() {
                    "You"
                } else {
                    &c.author
                }
            }
            CommentRef::Legacy(c) => {
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
            CommentRef::Question(q) => &q.timestamp,
            CommentRef::GitHubComment(c) => &c.timestamp,
            CommentRef::Legacy(c) => &c.timestamp,
        }
    }

    pub fn is_synced(&self) -> bool {
        match self {
            CommentRef::Question(_) => false,
            CommentRef::GitHubComment(c) => c.synced,
            CommentRef::Legacy(c) => c.synced,
        }
    }

    pub fn is_resolved(&self) -> bool {
        match self {
            CommentRef::Question(q) => q.resolved,
            CommentRef::GitHubComment(c) => c.resolved,
            CommentRef::Legacy(c) => c.resolved,
        }
    }

    pub fn is_stale(&self) -> bool {
        match self {
            CommentRef::Question(q) => q.stale,
            CommentRef::GitHubComment(c) => c.stale || c.outdated,
            CommentRef::Legacy(_) => false,
        }
    }

    pub fn in_reply_to(&self) -> Option<&str> {
        match self {
            CommentRef::Question(q) => q.in_reply_to.as_deref(),
            CommentRef::GitHubComment(c) => c.in_reply_to.as_deref(),
            CommentRef::Legacy(c) => c.in_reply_to.as_deref(),
        }
    }

    #[allow(dead_code)]
    pub fn file(&self) -> &str {
        match self {
            CommentRef::Question(q) => &q.file,
            CommentRef::GitHubComment(c) => &c.file,
            CommentRef::Legacy(c) => &c.file,
        }
    }

    pub fn hunk_index(&self) -> Option<usize> {
        match self {
            CommentRef::Question(q) => q.hunk_index,
            CommentRef::GitHubComment(c) => c.hunk_index,
            CommentRef::Legacy(c) => c.hunk_index,
        }
    }

    pub fn line_start(&self) -> Option<usize> {
        match self {
            CommentRef::Question(q) => q.line_start,
            CommentRef::GitHubComment(c) => c.line_start,
            CommentRef::Legacy(c) => c.line_start,
        }
    }

    pub fn line_end(&self) -> Option<usize> {
        match self {
            CommentRef::Question(q) => q.line_end,
            CommentRef::GitHubComment(c) => c.line_end,
            CommentRef::Legacy(c) => c.line_end,
        }
    }

    pub fn old_line_start(&self) -> Option<usize> {
        match self {
            CommentRef::Question(q) => q.old_line_start,
            CommentRef::GitHubComment(c) => c.old_line_start,
            CommentRef::Legacy(_) => None,
        }
    }

    #[allow(dead_code)]
    pub fn anchor_status(&self) -> &str {
        match self {
            CommentRef::Question(q) => &q.anchor_status,
            CommentRef::GitHubComment(c) => &c.anchor_status,
            CommentRef::Legacy(_) => "original",
        }
    }

    /// Whether this comment can be replied to (top-level comments/questions, not replies themselves)
    pub fn can_reply(&self) -> bool {
        match self {
            CommentRef::Question(q) => q.in_reply_to.is_none(),
            CommentRef::GitHubComment(c) => c.in_reply_to.is_none(),
            CommentRef::Legacy(c) => c.in_reply_to.is_none(),
        }
    }

    /// Whether this comment can be deleted by the user
    #[allow(dead_code)]
    pub fn can_delete(&self) -> bool {
        match self {
            CommentRef::Question(_) => true,
            // TODO(risk:medium): Authorship check compares `c.author` (a display name from
            // the JSON file) to the literal string "You". A GitHub comment whose author
            // happens to be named "You", or a crafted sidecar that sets `author = "You"`,
            // would allow the UI to offer deletion of a remote comment it does not own.
            CommentRef::GitHubComment(c) => c.source != "github" || c.author == "You",
            CommentRef::Legacy(c) => c.source != "github" || c.author == "You",
        }
    }

}

// ── .er-questions.json — personal review notes ──

// TODO(risk:medium): No upper bound on `questions` vec. If an external tool writes
// thousands of questions, every hunk render scans the full list (O(n) per hunk per frame).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErQuestions {
    pub version: u32,
    pub diff_hash: String,
    #[serde(default)]
    pub questions: Vec<ReviewQuestion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewQuestion {
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
    /// Hunk header string at creation time
    #[serde(default)]
    pub hunk_header: String,
    /// "original" | "relocated" | "lost"
    #[serde(default = "default_anchor_status")]
    // TODO(risk:minor): `anchor_status` is a free-form String from untrusted JSON, but code
    // elsewhere pattern-matches on the string values "original"/"relocated"/"lost". An
    // unexpected value silently falls through without warning.
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
}

// ── .er-github-comments.json — GitHub PR comments ──

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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

// TODO(risk:medium): No upper bound on `comments` vec. Repos with active review threads
// can accumulate hundreds of entries; scanning all of them on every hunk render is O(n).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErGitHubComments {
    pub version: u32,
    pub diff_hash: String,
    #[serde(default)]
    pub github: Option<GitHubSyncState>,
    #[serde(default)]
    pub comments: Vec<GitHubReviewComment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubReviewComment {
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
    /// Whether this comment was pushed to GitHub
    #[serde(default)]
    pub synced: bool,
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

// ── .er/ui-annotations.json — browser-view annotations ──

/// A point/region annotation captured from the embedded browser view.
/// Stored at `<comments_dir>/ui-annotations.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UiAnnotation {
    pub id: String,
    /// Path portion of the page URL (no query/hash), e.g. `/dashboard`.
    pub url: String,
    /// CSS selector (best-effort tag + nth-child path). `None` for cross-origin.
    #[serde(default)]
    pub selector: Option<String>,
    pub box_x: f64,
    pub box_y: f64,
    pub box_w: f64,
    pub box_h: f64,
    pub viewport_w: u32,
    pub viewport_h: u32,
    pub text: String,
    #[serde(default)]
    pub timestamp: String,
    #[serde(default = "default_author")]
    pub author: String,
    #[serde(default)]
    pub screenshot_path: Option<String>,
    /// Persisted: set when re-anchoring fails or the new box deviates too much.
    #[serde(default)]
    pub stale: bool,
    /// Short description of the annotated element (tag + label/text), e.g. "button: Submit".
    #[serde(default)]
    pub element_context: Option<String>,
    /// Structured DOM context captured from the resolved element at annotation time.
    /// This is intended for agent communication, not as a stable re-anchor.
    #[serde(default)]
    pub dom_context: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ErUiAnnotations {
    #[serde(default = "default_ui_version")]
    pub version: u32,
    #[serde(default)]
    pub annotations: Vec<UiAnnotation>,
}

fn default_ui_version() -> u32 {
    1
}

/// Load annotations from `<comments_dir>/ui-annotations.json`. Missing file → empty.
pub fn load_ui_annotations(comments_dir: &str) -> Vec<UiAnnotation> {
    let path = format!("{comments_dir}/ui-annotations.json");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    serde_json::from_str::<ErUiAnnotations>(&content)
        .map(|f| f.annotations)
        .unwrap_or_default()
}

/// Atomically write annotations to `<comments_dir>/ui-annotations.json`.
pub fn save_ui_annotations(
    comments_dir: &str,
    annotations: &[UiAnnotation],
) -> std::io::Result<()> {
    std::fs::create_dir_all(comments_dir)?;
    let path = format!("{comments_dir}/ui-annotations.json");
    let tmp = format!("{path}.tmp");
    let file = ErUiAnnotations {
        version: 1,
        annotations: annotations.to_vec(),
    };
    let json = serde_json::to_string_pretty(&file).map_err(std::io::Error::other)?;
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, &path)
}

// ── Legacy .er-feedback.json (for migration) ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErFeedback {
    pub version: u32,
    pub diff_hash: String,
    #[serde(default)]
    pub github: Option<GitHubSyncState>,
    #[serde(default)]
    pub comments: Vec<FeedbackComment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackComment {
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
    /// Whether this comment was pushed to GitHub
    #[serde(default)]
    pub synced: bool,
}

/// Top-level GitHub comment eligible for batch validate / re-anchor.
pub fn github_comment_eligible_for_batch_validate(c: &GitHubReviewComment) -> bool {
    !c.resolved
        && !c.outdated
        && c.in_reply_to.is_none()
        && c.line_start.is_some()
}

/// Count GitHub comments eligible for batch validate.
pub fn count_eligible_github_comments(gc: &ErGitHubComments) -> usize {
    gc.comments
        .iter()
        .filter(|c| github_comment_eligible_for_batch_validate(c))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_question() -> ReviewQuestion {
        ReviewQuestion {
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
            hunk_header: String::new(),
            anchor_status: "original".into(),
            relocated_at_hash: String::new(),
            in_reply_to: None,
            author: "You".into(),
            promoted_to: None,
            finding_ref: None,
        }
    }

    fn sample_github_comment() -> GitHubReviewComment {
        GitHubReviewComment {
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
            synced: true,
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
    fn review_question_promoted_to_roundtrips_via_serde() {
        let mut q = sample_question();
        q.promoted_to = Some("c-42".into());

        let json = serde_json::to_string(&q).unwrap();
        let back: ReviewQuestion = serde_json::from_str(&json).unwrap();

        assert_eq!(back.promoted_to.as_deref(), Some("c-42"));
    }

    #[test]
    fn github_comment_outdated_roundtrips_via_serde() {
        let mut c = sample_github_comment();
        c.outdated = true;

        let json = serde_json::to_string(&c).unwrap();
        let back: GitHubReviewComment = serde_json::from_str(&json).unwrap();

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

        let c: GitHubReviewComment = serde_json::from_str(json).unwrap();

        assert!(!c.outdated);
    }

    #[test]
    fn ui_annotation_roundtrips_via_serde() {
        let ann = UiAnnotation {
            id: "ann-1".into(),
            url: "/dashboard".into(),
            selector: Some("button.primary:nth-child(2)".into()),
            box_x: 12.5,
            box_y: 30.0,
            box_w: 100.0,
            box_h: 28.0,
            viewport_w: 1280,
            viewport_h: 800,
            text: "This button looks off".into(),
            timestamp: "2026-05-13T10:00:00Z".into(),
            author: "You".into(),
            screenshot_path: None,
            stale: false,
            element_context: Some("button: Save".into()),
            dom_context: Some(serde_json::json!({
                "tag": "button",
                "text": "Save",
                "role": "button"
            })),
        };
        let json = serde_json::to_string(&ann).unwrap();
        let back: UiAnnotation = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ann);
    }

    #[test]
    fn ui_annotation_file_load_missing_returns_empty() {
        let tmp = std::env::temp_dir().join("er-ui-ann-test-missing");
        let _ = std::fs::remove_dir_all(&tmp);
        let dir = tmp.to_string_lossy().to_string();
        let v = load_ui_annotations(&dir);
        assert!(v.is_empty());
    }

    #[test]
    fn ui_annotation_save_then_load_roundtrip() {
        let tmp = std::env::temp_dir().join(format!(
            "er-ui-ann-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let dir = tmp.to_string_lossy().to_string();
        let anns = vec![UiAnnotation {
            id: "a".into(),
            url: "/x".into(),
            selector: None,
            box_x: 0.0,
            box_y: 0.0,
            box_w: 10.0,
            box_h: 10.0,
            viewport_w: 800,
            viewport_h: 600,
            text: "hi".into(),
            timestamp: "t".into(),
            author: "You".into(),
            screenshot_path: None,
            stale: false,
            element_context: None,
            dom_context: None,
        }];
        save_ui_annotations(&dir, &anns).unwrap();
        let back = load_ui_annotations(&dir);
        assert_eq!(back, anns);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn github_comment_eligible_for_batch_validate_includes_active_line_comment() {
        assert!(github_comment_eligible_for_batch_validate(&sample_github_comment()));
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
        let gc = ErGitHubComments {
            version: 1,
            diff_hash: "h".into(),
            github: None,
            comments: vec![
                sample_github_comment(),
                {
                    let mut reply = sample_github_comment();
                    reply.id = "gh-2".into();
                    reply.in_reply_to = Some("gh-1".into());
                    reply
                },
            ],
        };
        assert_eq!(count_eligible_github_comments(&gc), 1);
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
        let q: ReviewQuestion = serde_json::from_str(json).unwrap();
        assert!(q.promoted_to.is_none());
    }
}
