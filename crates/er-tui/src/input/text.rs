use super::{CommentAction, Edit};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// The one-line fields (search, filter, commit message, remote URL and base
/// branch) take characters and Backspace; Enter submits and Esc cancels.
pub fn edit(key: KeyEvent) -> Option<Edit> {
    match key.code {
        KeyCode::Enter => Some(Edit::Submit),
        KeyCode::Esc => Some(Edit::Cancel),
        KeyCode::Char(c) => Some(Edit::Insert(c)),
        KeyCode::Backspace => Some(Edit::Backspace),
        _ => None,
    }
}

pub fn comment(key: KeyEvent) -> CommentAction {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => CommentAction::NewLine,
        KeyCode::Enter => CommentAction::Submit,
        KeyCode::Esc => CommentAction::Cancel,
        KeyCode::Tab | KeyCode::BackTab => CommentAction::Pause,
        KeyCode::Char('t') if ctrl => CommentAction::ToggleType,
        KeyCode::Char('d') if ctrl => CommentAction::ScrollDown(10),
        KeyCode::Char('u') if ctrl => CommentAction::ScrollUp(10),
        KeyCode::PageDown => CommentAction::ScrollDown(20),
        KeyCode::PageUp => CommentAction::ScrollUp(20),
        _ => CommentAction::Type(key),
    }
}
