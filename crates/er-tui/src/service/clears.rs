//! The flag-on clears (P72.3). `z`, `Z` and the AI hub's two Cleanup rows
//! ask first, with the App's counts; `y` sends the clear to the tab's actor,
//! whose effect holds the bucket's admission while it deletes. Where the
//! App reports a clear that never ran, a refusal here says why.

use er_service::{Command, ReviewArtifact, TabContent};

use super::tui_view::Ask;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clear {
    /// `z`: the questions and notes, with their backups.
    QuestionsAndNotes,
    /// `Z`: every review, with the professor and expert results.
    Reviews,
}

/// The prompt a clear opens. The keys open none on a remote tab, as on the
/// App; the hub rows skip that check there too.
pub fn ask(clear: Clear, content: &TabContent, from_key: bool) -> Option<Ask> {
    if from_key && content.tab.identity().is_remote() {
        return None;
    }
    let ai = &content.ai.state;
    Some(match clear {
        Clear::QuestionsAndNotes => Ask::CleanupQuestions {
            count: ai.local_draft_count(),
        },
        Clear::Reviews => Ask::CleanupReviews {
            count: ai.review.as_ref().map_or(0, |review| review.files.len()),
        },
    })
}

/// The command a confirmed prompt sends. `Z` sends the service's review
/// delete, which also takes the professor and expert results.
pub fn command(clear: Clear, content: &TabContent) -> Command {
    let context = content.context.clone();
    match clear {
        Clear::QuestionsAndNotes => Command::ClearQuestionsAndNotes { context },
        Clear::Reviews => Command::DeleteReviewArtifact {
            context,
            artifact: ReviewArtifact::Review,
        },
    }
}

/// The App's notice once the clear lands.
pub const fn landed(clear: Clear) -> &'static str {
    match clear {
        Clear::QuestionsAndNotes => "Questions & notes cleared",
        Clear::Reviews => "Review cleared",
    }
}

/// What a refused clear says: a review flow holds the bucket, or the bucket
/// is busy. The App's review clear words its refusal the same way.
pub fn refused(clear: Clear, error: &str) -> String {
    match clear {
        Clear::QuestionsAndNotes => format!("Questions & notes clear skipped: {error}"),
        Clear::Reviews => format!("Review clear skipped: {error}"),
    }
}
