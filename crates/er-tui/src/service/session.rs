//! The TUI's view file, `session.json`, on the flag path: a tab's view state
//! saved beside the diff it was taken on, and restored only on that diff.
//!
//! The file is the TUI's own: the service never reads it (ADR 0068).
//! It lives in the tab's bucket, where the App path keeps it, so either build
//! picks up where the other left off.

use er_engine::ai::CommentType;
use er_engine::session_store::{self, SessionState};
use er_service::TabContent;
use tui_textarea::TextArea;

use super::tui_view::{Draft, TabViewState};

/// The session saved in the tab's bucket. A remote tab has none.
pub fn load(content: &TabContent) -> Option<SessionState> {
    let scope = scope(content)?;
    session_store::load_session(&er_engine::store_registry::store(), &scope)
}

/// Saves `state` as the tab's session. A remote tab saves nothing.
pub fn save(content: &TabContent, state: &TabViewState) -> anyhow::Result<()> {
    let Some(scope) = scope(content) else {
        return Ok(());
    };
    let session = capture(content, state);
    session_store::save_session(&er_engine::store_registry::store(), &scope, &session)?;
    Ok(())
}

fn scope(content: &TabContent) -> Option<er_store::Scope> {
    if content.context.is_remote {
        return None;
    }
    er_store::Scope::dir(&content.context.er_dir).ok()
}

/// The session for `state` on `content`'s diff.
pub fn capture(content: &TabContent, state: &TabViewState) -> SessionState {
    let draft = &state.draft;
    SessionState {
        diff_hash: content.diff.meta.branch_diff_hash.clone(),
        branch: content.tab.current_branch.clone(),
        selected_file: state.selected_file,
        current_hunk: state.current_hunk,
        current_line: state.current_line,
        diff_scroll: state.diff_scroll,
        h_scroll: state.h_scroll,
        diff_mode: content.tab.mode.git_mode().to_string(),
        filter_expr: state.saved_filter.expr.clone(),
        filter_history: state.saved_filter.history.clone(),
        show_unreviewed_only: state.show_unreviewed_only,
        sort_by_mtime: state.sort_by_mtime,
        comment_draft: draft.textarea.lines().join("\n").trim().to_string(),
        comment_draft_file: draft.file.clone(),
        comment_draft_hunk: draft.hunk,
        comment_draft_line: draft.line,
        comment_draft_type: match draft.kind {
            CommentType::Question => "question",
            CommentType::Note => "note",
            CommentType::GitHubComment => "github",
        }
        .to_string(),
        comment_draft_reply_to: draft.reply_to.clone(),
        comment_draft_finding_ref: draft.finding_ref.clone(),
        comment_draft_edit_id: draft.edit_id.clone(),
        comment_draft_side: draft.side.clone(),
        comment_draft_line_end: draft.line_end,
        comment_draft_client_id: draft.client_id.clone(),
    }
}

/// Puts `session` back on a new tab's `state` when it was saved on this
/// diff; a session from another diff would select a file that moved.
///
/// The position comes back only in the mode it was saved in, since no mode
/// key has a service route yet. The draft comes back whole in any mode: its
/// client id is what lets a retried post find the comment it already made
/// (ADR 0053).
pub fn restore(state: &mut TabViewState, session: SessionState, content: &TabContent) {
    if session.diff_hash != content.diff.meta.branch_diff_hash {
        return;
    }
    state.saved_filter.expr = session.filter_expr;
    state.saved_filter.history = session.filter_history;
    state.show_unreviewed_only = session.show_unreviewed_only;
    state.sort_by_mtime = session.sort_by_mtime;
    if !session.comment_draft.is_empty() {
        state.draft = Draft {
            textarea: TextArea::new(session.comment_draft.lines().map(String::from).collect()),
            file: session.comment_draft_file,
            hunk: session.comment_draft_hunk,
            line: session.comment_draft_line,
            kind: match session.comment_draft_type.as_str() {
                "question" => CommentType::Question,
                "note" => CommentType::Note,
                _ => CommentType::GitHubComment,
            },
            reply_to: session.comment_draft_reply_to,
            finding_ref: session.comment_draft_finding_ref,
            edit_id: session.comment_draft_edit_id,
            side: session.comment_draft_side,
            line_end: session.comment_draft_line_end,
            client_id: session.comment_draft_client_id,
        };
    }
    let saved_mode = match session.diff_mode.as_str() {
        "" => "branch",
        mode => mode,
    };
    let files = &content.diff.files;
    if saved_mode != content.tab.mode.git_mode() || files.is_empty() {
        return;
    }
    state.selected_file = session.selected_file.min(files.len() - 1);
    state.diff_scroll = session.diff_scroll;
    state.h_scroll = session.h_scroll;
    let hunks = files[state.selected_file].hunks.len();
    if hunks > 0 {
        state.current_hunk = session.current_hunk.min(hunks - 1);
    }
    state.current_line = session.current_line;
}
