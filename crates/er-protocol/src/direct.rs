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
