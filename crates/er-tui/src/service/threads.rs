//! The flag-on threads: J and K over the published AI state's hints, and the
//! composer, whose creates, replies and edits go to the actor of the tab they
//! were written on, as the delete does. The threads, their order and their
//! counts come from the engine's `AiState` methods, as on the App path;
//! nothing here orders them.

use er_engine::ai::{annotation_store, CommentRef, CommentType, HintType};
use er_engine::git::{DiffFile, DiffLine};
use er_service::{AnnotationTarget, Command, TabContent};
use tui_textarea::TextArea;

use super::tui_view::{is_stub, Mode, TabViewState};
use crate::input::keys::CommentAction;

/// The diff file the composer and the hints act on: none while a watched
/// file is selected.
fn selected<'c>(state: &TabViewState, content: &'c TabContent) -> Option<&'c DiffFile> {
    content
        .diff
        .files
        .get(state.selected_file)
        .filter(|_| state.watched.selected.is_none())
        .map(|file| &**file)
}

/// Opens the composer on the cursor's line of the selected file, as the
/// App's `start_comment` does. A draft whose write is in flight stays.
/// Answers whether it opened.
pub fn start(state: &mut TabViewState, content: &TabContent, kind: CommentType) -> bool {
    let Some(file) = selected(state, content).filter(|_| state.sent.is_none()) else {
        return false;
    };
    let line = cursor_line(state, file);
    let draft = &mut state.draft;
    draft.textarea = TextArea::default();
    draft.file.clone_from(&file.path);
    draft.hunk = state.current_hunk;
    draft.line = line.and_then(|line| line.new_num.or(line.old_num));
    draft.reply_to = None;
    draft.finding_ref = None;
    // A paused or emptied edit must not turn this new draft into its edit.
    draft.edit_id = None;
    draft.kind = kind;
    draft.review = false;
    draft.client_id = None;
    let deleted = line.is_some_and(|line| line.new_num.is_none());
    draft.side = Some(if deleted { "LEFT" } else { "RIGHT" }.to_string());
    true
}

/// What a thread key on the focused comment asks of the loop.
pub enum Focus {
    /// No comment the key acts on: nothing happens, as on the App.
    Nothing,
    /// The composer opened on a reply or an edit.
    Opened,
    /// A delete waits on the y/n prompt.
    AskDelete,
    /// The key's write is another entry's.
    Unported,
    /// The key does not act here, and says why.
    Refused(&'static str),
}

/// The focused question, note or comment, unless a write of the draft is in
/// flight. A comment posted on GitHub takes the same keys: its writes go to
/// GitHub first.
fn focused<'c>(state: &TabViewState, content: &'c TabContent) -> Result<CommentRef<'c>, Focus> {
    let id = state.focused_comment_id.as_deref().ok_or(Focus::Nothing)?;
    let comment = content.ai.state.find_comment(id).ok_or(Focus::Nothing)?;
    if state.sent.is_some() {
        return Err(Focus::Nothing);
    }
    Ok(comment)
}

/// `r`: a reply draft under the focused top-level comment, placed and typed
/// as its parent, as the App's `start_reply_comment` does.
pub fn start_reply(state: &mut TabViewState, content: &TabContent) -> Focus {
    let comment = match focused(state, content) {
        Ok(comment) if comment.can_reply() => comment,
        Ok(_) => return Focus::Nothing,
        Err(focus) => return focus,
    };
    let draft = &mut state.draft;
    draft.textarea = TextArea::default();
    draft.file = comment.file().to_string();
    draft.hunk = comment.hunk_index().unwrap_or(0);
    draft.line = comment.line_start();
    draft.reply_to = Some(comment.id().to_string());
    draft.finding_ref = None;
    draft.kind = comment.comment_type();
    draft.review = false;
    draft.edit_id = None;
    draft.client_id = None;
    Focus::Opened
}

/// `e`: an edit draft of your own focused top-level comment, prefilled with
/// its text, as the App's `start_edit_comment` does. Anything else opens the
/// editor, which P75 ports.
pub fn start_edit(state: &mut TabViewState, content: &TabContent) -> Focus {
    if state.sent.is_some() {
        return Focus::Nothing;
    }
    let comment = match focused(state, content) {
        Ok(comment) if comment.author() == "You" && comment.can_reply() => comment,
        Err(Focus::Nothing) | Ok(_) => return Focus::Unported,
        Err(focus) => return focus,
    };
    let Some(file) = selected(state, content) else {
        return Focus::Nothing;
    };
    let line = cursor_line(state, file);
    let draft = &mut state.draft;
    draft.textarea = TextArea::new(vec![comment.text().to_string()]);
    draft.file.clone_from(&file.path);
    draft.hunk = state.current_hunk;
    draft.line = line.and_then(|line| line.new_num.or(line.old_num));
    draft.reply_to = None;
    draft.kind = comment.comment_type();
    draft.review = false;
    draft.edit_id = Some(comment.id().to_string());
    draft.client_id = None;
    Focus::Opened
}

/// `x`: the focused comment waits on the y/n prompt, if it may be deleted.
pub fn ask_delete(state: &mut TabViewState, content: &TabContent) -> Focus {
    match focused(state, content) {
        Ok(comment) if comment.can_delete() => {
            state.deleting_comment = Some(comment.id().to_string());
            Focus::AskDelete
        }
        Ok(_) => Focus::Nothing,
        Err(focus) => focus,
    }
}

/// The diff line under the cursor of the selected file.
fn cursor_line<'f>(state: &TabViewState, file: &'f DiffFile) -> Option<&'f DiffLine> {
    file.hunks
        .get(state.current_hunk)
        .zip(state.current_line)
        .and_then(|(hunk, line)| hunk.lines.get(line))
}

/// What a composer key asks of the loop.
pub enum Composed {
    Stay,
    Send(Box<Command>),
    /// A GitHub post, whose reply the GitHub writes take.
    Posted(Box<Command>),
}

/// Runs a composer key on the active tab's draft. Submit builds the create
/// and keeps the text until its reply lands; while it is in flight the
/// draft takes no edit.
pub fn key(
    state: &mut TabViewState,
    mode: &mut Mode,
    action: CommentAction,
    content: &TabContent,
) -> Composed {
    let sending = state.sent.is_some();
    let draft = &mut state.draft;
    match action {
        CommentAction::NewLine if !sending => draft.textarea.insert_newline(),
        CommentAction::Type(key) if !sending => {
            draft.textarea.input(key);
        }
        CommentAction::ToggleType if !sending && draft.can_toggle_kind() => {
            // On this path a GitHub comment on a line may instead join the
            // pending review (D3), the fourth kind.
            (draft.kind, draft.review) = match (draft.kind, draft.review) {
                (CommentType::Question, _) => (CommentType::Note, false),
                (CommentType::Note, _) => (CommentType::GitHubComment, false),
                (CommentType::GitHubComment, false) if draft.line.is_some() => {
                    (CommentType::GitHubComment, true)
                }
                (CommentType::GitHubComment, _) => (CommentType::Question, false),
            };
            // The painted id names its kind, so a new kind paints a new one.
            draft.client_id = None;
        }
        CommentAction::Cancel => {
            draft.textarea = TextArea::default();
            draft.edit_id = None;
            draft.client_id = None;
            *mode = Mode::Normal;
        }
        CommentAction::Pause => *mode = Mode::Normal,
        CommentAction::ScrollDown(rows) => {
            state.scroll(selected(state, content), i32::from(rows));
        }
        CommentAction::ScrollUp(rows) => {
            state.scroll(selected(state, content), -i32::from(rows));
        }
        CommentAction::Submit if !sending => return submit(state, mode, content),
        _ => {}
    }
    Composed::Stay
}

fn submit(state: &mut TabViewState, mode: &mut Mode, content: &TabContent) -> Composed {
    let draft = &mut state.draft;
    let text = draft.textarea.lines().join("\n");
    if text.is_empty() {
        *mode = Mode::Normal;
        return Composed::Stay;
    }
    // A whitespace-only body goes too, and the service refuses it, so the
    // composer keeps it (ruling 5 on #653).
    let body = text.trim().to_string();
    let context = content.context.clone();
    if let Some(id) = draft.edit_id.clone() {
        state.sent = Some(format!("Comment updated: {}", truncate(&body, 40)));
        let edit = er_service::edit_message(context, id, body);
        return Composed::Send(Box::new(edit));
    }
    // A finding reply writes a thread on the finding: a root, then the reply
    // (ruling 5 on #653). A GitHub comment, or a reply under a posted one,
    // goes to GitHub.
    if let Some(finding_id) = draft.finding_ref.clone() {
        let client_id = draft
            .client_id
            .get_or_insert_with(|| annotation_store::new_annotation_id("c-"))
            .clone();
        state.sent = Some(format!("Comment added: {}", truncate(&body, 40)));
        return Composed::Send(Box::new(Command::ReplyToFinding {
            context,
            finding_id,
            body,
            ai_assist: false,
            client_id: Some(client_id),
        }));
    }
    if draft.kind == CommentType::GitHubComment {
        if let Some(command) = super::github::submit(state, content, body.clone()) {
            return Composed::Posted(Box::new(command));
        }
    }
    let draft = &mut state.draft;
    let prefix = match draft.kind {
        CommentType::Question => "q-",
        CommentType::Note => "n-",
        CommentType::GitHubComment => "c-",
    };
    let client_id = Some(
        draft
            .client_id
            .get_or_insert_with(|| annotation_store::new_annotation_id(prefix))
            .clone(),
    );
    let label = match (&draft.reply_to, draft.kind) {
        (Some(_), _) => "Reply",
        (None, CommentType::Note) => "Note",
        (None, _) => "Question",
    };
    state.sent = Some(format!("{label} added: {}", truncate(&body, 40)));
    if let Some(parent_id) = draft.reply_to.clone() {
        return Composed::Send(Box::new(Command::ReplyToThread {
            context,
            parent_id,
            body,
            client_id,
        }));
    }
    let target = AnnotationTarget {
        file: draft.file.clone(),
        hunk_idx: draft.hunk,
        line_num: draft.line,
        line_num_end: draft.line_end,
    };
    let side = draft.side.clone();
    let kind = draft.kind;
    let (reply_to, finding_ref) = (None, None);
    Composed::Send(Box::new(if kind == CommentType::Note {
        Command::AddNote {
            context,
            target,
            body,
            side,
            client_id,
            reply_to,
            finding_ref,
        }
    } else {
        Command::AddQuestion {
            context,
            target,
            body,
            side,
            client_id,
            reply_to,
            finding_ref,
        }
    }))
}

/// The write in flight landed: the draft clears. Answers what the status
/// line says, as the App's write does.
pub fn landed(state: &mut TabViewState) -> Option<String> {
    let notice = state.sent.take()?;
    let draft = &mut state.draft;
    draft.textarea = TextArea::default();
    draft.edit_id = None;
    draft.client_id = None;
    draft.side = None;
    Some(notice)
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let kept: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{kept}…")
}

/// Moves the focus to the next or previous question, note or comment, in
/// the order the AI state gives, wrapping, and selects its file and hunk as
/// the App's `jump_hint` does. Answers the file it newly selected.
pub fn jump_hint(state: &mut TabViewState, content: &TabContent, forward: bool) -> Option<usize> {
    let hints: Vec<_> = content
        .ai
        .state
        .all_hints_ordered()
        .into_iter()
        .filter(|hint| hint.4 != HintType::Finding)
        .collect();
    let last = hints.len().checked_sub(1)?;
    let files = &content.diff.files;
    let current_path = files.get(state.selected_file).map(|file| &file.path);
    let at = state
        .focused_comment_id
        .as_ref()
        .and_then(|id| hints.iter().position(|hint| &hint.3 == id))
        .or_else(|| {
            let path = current_path?;
            if forward {
                hints.iter().position(|hint| &hint.0 == path)
            } else {
                hints.iter().rposition(|hint| &hint.0 == path)
            }
        });
    let next = match (at, forward) {
        (Some(at), true) if at < last => at + 1,
        (Some(_) | None, true) => 0,
        (Some(at), false) if at > 0 => at - 1,
        (Some(_) | None, false) => last,
    };
    let (path, hunk, _, id, _) = &hints[next];
    state.focused_comment_id = Some(id.clone());
    state.focused_finding_id = None;
    let mut newly = None;
    if current_path != Some(path) {
        if let Some(idx) = files.iter().position(|file| &file.path == path) {
            state.selected_file = idx;
            state.current_hunk = hunk.unwrap_or(0);
            state.current_line = None;
            state.diff_scroll = 0;
            state.h_scroll = 0;
            newly = Some(idx);
        }
    } else if let Some(hunk) = hunk {
        state.current_hunk = *hunk;
        state.current_line = None;
    }
    let file = files.get(state.selected_file).map(|file| &**file);
    if file.is_some_and(is_stub) {
        state.scroll_on_parse = true;
    } else {
        state.scroll_to_cursor(file);
    }
    newly
}

/// A hint's file that was a stub has parsed: the diff scrolls to its hunk.
pub fn scroll_if_parsed(state: &mut TabViewState, content: &TabContent) {
    let file = content.diff.files.get(state.selected_file);
    if state.scroll_on_parse && file.is_some_and(|file| !is_stub(file)) {
        state.scroll_on_parse = false;
        state.scroll_to_cursor(file.map(|file| &**file));
    }
}
