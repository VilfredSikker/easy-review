//! The flag-on GitHub writes (P68.1): the GitHub composer and the PR-wide
//! comment, the pull, and the approval. Each goes to the actor of the tab it
//! was written on; a refusal shows the service's message, and a refused post
//! keeps its draft and its client id, so a resend finds the comment an
//! unknown outcome may have made (ADR 0053).

use er_engine::ai::{annotation_store, CommentRef, CommentType};
use er_engine::github_post::NO_PR_FOR_COMMENT;
use er_service::{
    AnnotationTarget, Command, ReviewEvent, SyncSource, TabContent, EARLIER_VERSION_POSTED,
};
use tui_textarea::TextArea;

use super::threads;
use super::tui_view::{PrKnown, TabViewState};

/// `c`: the composer on the cursor's line as a GitHub comment, refused on a
/// tab known to have no PR, as the App's held state refuses it. Answers
/// whether it opened.
pub fn start_line(
    state: &mut TabViewState,
    content: &TabContent,
    known: PrKnown,
) -> Result<bool, String> {
    if known == PrKnown::Absent {
        return Err(NO_PR_FOR_COMMENT.to_string());
    }
    Ok(threads::start(state, content, CommentType::GitHubComment))
}

/// The Git hub's "Comment on PR": a draft on no file, which posts PR-wide,
/// as the App's `start_general_comment` opens it.
pub fn start_general(state: &mut TabViewState, known: PrKnown) -> Result<bool, String> {
    if known == PrKnown::Absent {
        return Err(NO_PR_FOR_COMMENT.to_string());
    }
    if state.sent.is_some() {
        return Ok(false);
    }
    let draft = &mut state.draft;
    draft.textarea = TextArea::default();
    draft.file.clear();
    draft.hunk = 0;
    draft.line = None;
    draft.line_end = None;
    draft.reply_to = None;
    draft.finding_ref = None;
    draft.kind = CommentType::GitHubComment;
    draft.edit_id = None;
    draft.client_id = None;
    Ok(true)
}

/// The GitHub composer's post: a reply under a posted comment, a PR-wide
/// comment for a draft on no file, else a line comment. The client id is
/// painted on the first send and kept until one lands. Answers `None` for a
/// reply under a row that is not on GitHub, which is a thread write.
pub fn submit(state: &mut TabViewState, content: &TabContent, body: String) -> Option<Command> {
    let draft = &mut state.draft;
    let context = content.context.clone();
    let posted_parent = draft.reply_to.as_deref().map(|id| {
        matches!(
            content.ai.state.find_comment(id),
            Some(CommentRef::GithubComment(row)) if row.github_id.is_some()
        )
    });
    if posted_parent == Some(false) {
        return None;
    }
    let client_id = Some(
        draft
            .client_id
            .get_or_insert_with(|| annotation_store::new_annotation_id("c-"))
            .clone(),
    );
    let command = if let Some(parent_id) = draft.reply_to.clone() {
        Command::ReplyToGithubThread {
            context,
            parent_id,
            body: body.clone(),
            client_id,
        }
    } else if draft.file.is_empty() {
        Command::PostGithubPrComment {
            context,
            body: body.clone(),
            client_id,
        }
    } else {
        Command::AddComment {
            context,
            target: AnnotationTarget {
                file: draft.file.clone(),
                hunk_idx: draft.hunk,
                line_num: draft.line,
                line_num_end: draft.line_end,
            },
            body: body.clone(),
            side: draft.side.clone(),
            client_id,
        }
    };
    state.sent = Some(body);
    Some(command)
}

/// The post in flight landed: the draft clears. Answers the App's notice.
pub fn landed(state: &mut TabViewState) -> Option<&'static str> {
    state.sent.take()?;
    state.draft.textarea = TextArea::default();
    state.draft.client_id = None;
    Some("Comment posted to GitHub")
}

/// The post in flight was refused: the draft keeps its text. When the retry
/// adopted an earlier post with another body, the client id goes, so the
/// next send posts this text as a new comment, as the App's does.
pub fn refused(state: &mut TabViewState, message: &str) {
    state.sent = None;
    if message == EARLIER_VERSION_POSTED {
        state.draft.client_id = None;
    }
}

/// `G` and "Pull GitHub comments": a pull that always reads GitHub, as the
/// App's manual pull does.
pub fn sync(content: &TabContent) -> Command {
    Command::SyncGithubComments {
        context: content.context.clone(),
        source: SyncSource::Manual { force: true },
    }
}

/// What a landed pull says. The App's notice also says the PR status was
/// refreshed; the PR section's refresh is P73's.
pub fn synced(github_count: usize, local_count: usize) -> String {
    format!("GitHub sync: {github_count} from GitHub, {local_count} local kept")
}

/// What a refused pull teaches about the tab's PR: only the no-PR refusal
/// marks it absent.
pub fn known_after_refusal(message: &str) -> Option<PrKnown> {
    (message == NO_PR_FOR_COMMENT).then_some(PrKnown::Absent)
}

/// The approval the y/n prompt confirmed.
pub fn approve(content: &TabContent) -> Command {
    Command::SubmitGithubPrDecision {
        context: content.context.clone(),
        event: ReviewEvent::Approve,
        summary: String::new(),
    }
}
