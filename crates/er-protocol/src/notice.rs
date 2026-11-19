use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Something the backend tells the person once, such as a soft validation
/// message from a settings write. It has no resource to refetch, so it travels
/// as an event of its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Notice {
    /// Rises with each notice in a backend process, so a listener attached
    /// twice shows a notice once.
    // JSON over IPC delivers a number, never a BigInt; the count stays below 2^53.
    #[ts(type = "number")]
    pub seq: u64,
    pub level: NoticeLevel,
    /// Text for the person, never parsed.
    pub message: String,
    /// Worth leaving up longer than the page's default.
    pub long: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum NoticeLevel {
    Info,
    Success,
    Error,
}
