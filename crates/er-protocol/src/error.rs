use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Why a command or a fetch failed, in a form the frontend branches on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ErError {
    pub kind: ErErrorKind,
    /// Text for the person, never parsed.
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ErErrorKind {
    /// The key or the target is not there.
    NotFound,
    /// The tab changed under the command, so it was not applied.
    Moved,
    /// The content the request named was replaced; refetch what holds it.
    Stale,
    /// The backend declined the command in its current state.
    Refused,
    Timeout,
    /// The backend part that owns the request is not running.
    Disconnected,
    /// A GitHub call failed; the message carries its class.
    Github,
    /// The request itself is malformed.
    Invalid,
    /// This build does not serve the request.
    Unsupported,
    Internal,
}
