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
            CommentRef::GitHubComment(c) => c.stale,
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

    /// Reference to an AI finding this comment responds to
    #[allow(dead_code)]
    pub fn finding_ref(&self) -> Option<&str> {
        match self {
            CommentRef::Question(_) => None,
            CommentRef::GitHubComment(c) => c.finding_ref.as_deref(),
            CommentRef::Legacy(_) => None,
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

    #[allow(dead_code)]
    pub fn github_id(&self) -> Option<u64> {
        match self {
            CommentRef::Question(_) => None,
            CommentRef::GitHubComment(c) => c.github_id,
            CommentRef::Legacy(c) => c.github_id,
        }
    }

    #[allow(dead_code)]
    pub fn source(&self) -> &str {
        match self {
            CommentRef::Question(_) => "local",
            CommentRef::GitHubComment(c) => &c.source,
            CommentRef::Legacy(c) => &c.source,
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
    /// jj change_id when this comment was created (jj stack mode only)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub change_id: Option<String>,
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
    /// jj change_id when this comment was created (jj stack mode only)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub change_id: Option<String>,
}

fn default_source() -> String {
    "local".to_string()
}

fn default_anchor_status() -> String {
    "original".to_string()
}

fn default_author() -> String {
    "You".to_string()
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
