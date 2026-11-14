use serde::{Deserialize, Serialize};

use crate::artifact::Artifact;

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
// Agents still author these three files, but they cannot write them into the
// store directly: a payload without an envelope reads as unreadable. The agent
// writes its output into the job's staging directory, and the host commits that
// output through the store, which adds the envelope, so the store only ever
// sees its own shape.

impl Artifact for ErQuestions {
    const KIND: &'static str = "questions";
    const VERSION: u32 = 1;
    const FILE: &'static str = "questions.json";
    type Slot = ();

    fn diff_hash(&self) -> Option<&str> {
        Some(&self.diff_hash)
    }
}

impl Artifact for ErNotes {
    const KIND: &'static str = "notes";
    const VERSION: u32 = 1;
    const FILE: &'static str = "notes.json";
    type Slot = ();

    fn diff_hash(&self) -> Option<&str> {
        Some(&self.diff_hash)
    }
}

impl Artifact for ErGithubComments {
    const KIND: &'static str = "github-comments";
    const VERSION: u32 = 1;
    const FILE: &'static str = "github-comments.json";
    type Slot = ();

    fn diff_hash(&self) -> Option<&str> {
        Some(&self.diff_hash)
    }
}
