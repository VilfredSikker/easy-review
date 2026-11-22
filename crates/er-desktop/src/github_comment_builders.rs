//! The GitHub write intents the desktop commands construct.
//!
//! They live here so `er::comment_boundary` can allow this one file instead of
//! all of `commands.rs`. The `#[tauri::command]` functions stay in
//! `commands.rs`.

use er_engine::github::{NewLineComment, ReviewBatchEntry};
use er_engine::github_post_session::NewGitHubComment;

/// The intent for a new line comment, posted through the post-and-record flow.
pub fn line_intent(
    file: String,
    hunk_idx: usize,
    line_start: Option<usize>,
    line_end: Option<usize>,
    side: Option<String>,
    body: String,
) -> NewGitHubComment {
    NewGitHubComment::Line {
        file,
        hunk_idx,
        line_start,
        line_end,
        side,
        body,
    }
}

/// The intent for a reply to a comment already on GitHub.
pub fn reply_intent(parent_id: String, body: String) -> NewGitHubComment {
    NewGitHubComment::Reply { parent_id, body }
}

/// The pending-review line builder lives beside its type, so the service
/// builds the same entry.
pub use er_engine::github::review_batch_entry;

/// The borrowed intent for a single line comment posted outside a review.
pub fn line_comment(entry: &ReviewBatchEntry) -> NewLineComment<'_> {
    NewLineComment {
        path: &entry.file,
        line_start: entry.start_line.unwrap_or(entry.line),
        line_end: Some(entry.line),
        body: &entry.body,
        side: &entry.side,
    }
}
