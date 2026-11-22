use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One file's hunks, without the threads on them: the page joins those from
/// the tab's annotations by path and line, so a comment never re-sends a
/// file. Immutable under its hash.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct FileHunks {
    /// The hash these hunks hash to, which the next `diff(tab)` names. It
    /// differs from the asked hash when a lazy file was parsed to answer.
    pub hash: String,
    pub hunks: Vec<Hunk>,
}

/// Field names follow the snapshot's hunk, so both reads have one shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Hunk {
    pub header: String,
    pub old_start: u32,
    pub old_count: u32,
    pub new_start: u32,
    pub new_count: u32,
    pub lines: Vec<HunkLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct HunkLine {
    pub old_num: Option<u32>,
    pub new_num: Option<u32>,
    #[ts(type = "\"context\" | \"add\" | \"del\" | \"fold\"")]
    pub kind: String,
    pub text: String,
}
