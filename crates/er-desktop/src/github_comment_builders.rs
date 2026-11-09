//! The GitHub write intents the desktop commands construct.
//!
//! They live here so `er::comment_boundary` can allow this one file instead of
//! all of `commands.rs`. The `#[tauri::command]` functions stay in
//! `commands.rs`.

use er_engine::app::NewGitHubComment;
use er_engine::github::{NewLineComment, ReviewBatchEntry};

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

/// One line of a pending review, validated against the API's shape.
pub fn review_batch_entry(
    file: &str,
    line_num: usize,
    line_num_end: Option<usize>,
    side: Option<&str>,
    body: &str,
) -> Result<ReviewBatchEntry, String> {
    if file.is_empty() {
        return Err("A review comment needs a file and line".to_string());
    }
    let side = match side {
        None | Some("RIGHT") => "RIGHT",
        Some("LEFT") => "LEFT",
        Some(other) => return Err(format!("Invalid diff side: {other}")),
    };
    let end = line_num_end
        .filter(|end| *end > line_num)
        .unwrap_or(line_num);
    Ok(ReviewBatchEntry {
        file: file.to_string(),
        line: end,
        start_line: (end > line_num).then_some(line_num),
        body: body.to_string(),
        side: side.to_string(),
    })
}

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
