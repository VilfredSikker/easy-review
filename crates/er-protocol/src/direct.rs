use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// What an open-in-editor command did with the file it was asked to open.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct OpenSource {
    pub kind: OpenSourceKind,
    /// The path or URL opened, or the reason nothing was.
    pub target: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum OpenSourceKind {
    OpenedLocal,
    OpenedUrl,
    /// No checkout holds the file; the target says why.
    NeedsCheckout,
}

/// Which commit introduced or removed each changed line of one file.
/// Asked per file, never pushed: blame is a git call per file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LineCommits {
    pub path: String,
    /// The file row's `cache_key` it was computed for, so a reply for hunks
    /// the page no longer shows can be dropped.
    pub cache_key: String,
    /// Why this view cannot be attributed; none when it was.
    pub unavailable: Option<String>,
    /// Full shas, oldest first.
    pub commits: Vec<String>,
    /// New-side lines, by the commit that introduced them.
    pub added: Vec<BlameSpan>,
    /// Old-side lines, by the commit that removed them.
    pub deleted: Vec<BlameSpan>,
    /// False when the old side could not be blamed at all (a new file).
    pub deleted_attributed: bool,
}

/// A run of consecutive lines on one side of a diff, all from one commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct BlameSpan {
    /// 1-based line number on that side.
    pub start: u32,
    pub len: u32,
    /// Index into `LineCommits.commits`.
    pub commit: u32,
}

/// What the review export includes. Every field may be left out: a category
/// toggle left out is on, apart from `include_checklist` and
/// `only_unresolved`, which are off. An `include_*_ids` list keeps only the
/// items it names; an `exclude_*_ids` list drops the items it names, which
/// lets the page leave out an item without listing the ones it never sees.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(optional_fields)]
pub struct ExportOptions {
    pub include_comments: Option<bool>,
    pub include_questions: Option<bool>,
    pub include_notes: Option<bool>,
    pub include_findings: Option<bool>,
    pub only_unresolved: Option<bool>,
    pub include_annotations: Option<bool>,
    pub include_checklist: Option<bool>,
    pub include_comment_ids: Option<Vec<String>>,
    pub include_question_ids: Option<Vec<String>>,
    pub include_note_ids: Option<Vec<String>>,
    pub include_finding_ids: Option<Vec<String>>,
    pub exclude_comment_ids: Option<Vec<String>>,
    pub exclude_question_ids: Option<Vec<String>>,
    pub exclude_note_ids: Option<Vec<String>>,
    pub exclude_finding_ids: Option<Vec<String>>,
}
