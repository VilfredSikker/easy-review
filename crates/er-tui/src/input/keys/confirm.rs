use super::{ConfirmKey, TuiAction, Write};
use crate::screen::view::ConfirmAction;
use crossterm::event::{KeyCode, KeyEvent};

/// A y/n prompt. `y` confirms the write the prompt asked about; `k` runs an
/// agent but keeps what its last run left; `n` and Esc cancel.
pub fn action(key: KeyEvent, asked: ConfirmAction) -> TuiAction {
    match key.code {
        KeyCode::Char('y') => TuiAction::Write(match asked {
            ConfirmAction::DeleteComment => Write::DeleteComment,
            ConfirmAction::DeleteWatchedFile { .. } => Write::DeleteWatchedFile,
            ConfirmAction::Push => Write::Push,
            ConfirmAction::CleanupQuestions { .. } => Write::ClearQuestionsAndNotes,
            ConfirmAction::CleanupReviews { .. } => Write::ClearReviews,
            ConfirmAction::RunAgentReview => Write::ClearReviewAndRun,
            ConfirmAction::RunAgentQuestions => Write::ClearAnswersAndRunQuestions,
            ConfirmAction::RunAgentNotes => Write::ClearRepliesAndRunNotes,
            ConfirmAction::ApprovePR => Write::ApprovePr,
            ConfirmAction::RemoveReviewComment => Write::RemoveReviewComment,
        }),
        KeyCode::Char('k') => match asked {
            ConfirmAction::RunAgentReview => TuiAction::Confirm(ConfirmKey::KeepAndRunReview),
            ConfirmAction::RunAgentQuestions => TuiAction::Confirm(ConfirmKey::KeepAndRunQuestions),
            ConfirmAction::RunAgentNotes => TuiAction::Confirm(ConfirmKey::KeepAndRunNotes),
            _ => TuiAction::Noop,
        },
        KeyCode::Char('n') | KeyCode::Esc => TuiAction::Confirm(ConfirmKey::Cancel),
        _ => TuiAction::Noop,
    }
}
