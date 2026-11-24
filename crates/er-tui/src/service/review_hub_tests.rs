//! The flag-on review submission (P68.2, D3) against a scripted GitHub: the
//! composer's review kind, the review hub's read, remove, refresh and
//! submit, and the review draft in the session file.

use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use er_github::{FakeGitHub, GhError, PendingReview, PendingReviewComment};
use er_service::{Command, OWN_PR_APPROVAL_REFUSED};

use super::github_tests::{
    calls, git_hub_row, notice, on_pr_7, overview, settle_replies, state, type_text, working,
};
use super::review_hub::ReviewHub;
use super::shell::Shell;
use super::tests::{fixture, key, rows, settle_within, shell_frame, WAIT};
use super::tui_view::{Ask, Mode, Overlay, TabViewState};
use crate::ui::highlight::Highlighter;

const REVIEW_ROW: usize = 7;

fn comment(n: u64, line: u64) -> PendingReviewComment {
    PendingReviewComment {
        id: format!("PRRC_{n}"),
        database_id: Some(100 + n),
        path: "src/f000.rs".into(),
        line: Some(line),
        start_line: None,
        side: "RIGHT".into(),
        body: format!("Point {n}"),
    }
}

fn draft(comments: Vec<PendingReviewComment>) -> PendingReview {
    PendingReview {
        review_id: Some(1),
        comments,
        ..PendingReview::default()
    }
}

fn two() -> FakeGitHub {
    on_pr_7()
        .with_pending_review(Ok(draft(vec![comment(1, 2), comment(2, 4)])))
        .with_pending_review_submit(Ok(2))
}

fn hub(shell: &Shell) -> &ReviewHub {
    let hub = shell.view().review.as_ref();
    hub.unwrap_or_else(|| panic!("the review hub is closed"))
}

fn labels(shell: &Shell) -> Vec<String> {
    hub(shell)
        .rows
        .iter()
        .map(|row| row.label.clone())
        .collect()
}

/// Opens the review hub from the Git hub and waits for its read.
fn open(shell: &mut Shell) {
    git_hub_row(shell, REVIEW_ROW);
    assert!(matches!(shell.view().overlay, Some(Overlay::Review)));
    settle_replies(shell);
    assert!(hub(shell).loaded(), "the read never landed");
}

/// Moves the hub's selection to the row whose label starts with `label`.
fn to_row(shell: &mut Shell, label: &str) {
    for _ in 0..10 {
        shell.handle_key(key(KeyCode::Up));
    }
    for _ in 0..10 {
        let hub = hub(shell);
        if hub.rows[hub.selected].label.starts_with(label) {
            return;
        }
        shell.handle_key(key(KeyCode::Down));
    }
    panic!("no row {label}: {:?}", labels(shell));
}

fn submits(github: &FakeGitHub) -> Vec<String> {
    calls(github, "pending_review_submit")
}

/// The draft's comments are listed from GitHub's read, and the submit
/// sends the review with its decision, once while it is in flight, never a
/// bare decision; the hub then closes.
#[test]
fn a_draft_review_is_submitted_with_its_decision() {
    let repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let github = Arc::new(two());
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = working(&repo, &github, storage.path());
        open(&mut shell);
        let listed = labels(&shell);
        assert_eq!(listed[0], "Pending review: 2 comments");
        assert_eq!(listed[1..3], ["src/f000.rs:2", "src/f000.rs:4"]);
        to_row(&mut shell, "Decision");
        shell.handle_key(key(KeyCode::Enter));
        to_row(&mut shell, "Submit review");
        shell.handle_key(key(KeyCode::Enter));
        shell.handle_key(key(KeyCode::Enter));
        settle_replies(&mut shell);
        let sent = submits(&github);
        assert_eq!(sent.len(), 1, "{:?}", github.calls());
        assert!(sent[0].contains("event=\"APPROVE\""), "{sent:?}");
        assert!(calls(&github, "submit_review").is_empty());
        let read = github
            .calls()
            .iter()
            .position(|c| c.starts_with("pending_review("));
        let submit = github
            .calls()
            .iter()
            .position(|c| c.starts_with("pending_review_submit("));
        assert!(read < submit, "{:?}", github.calls());
        assert!(shell.view().review.is_none() && shell.view().overlay.is_none());
        assert_eq!(notice(&shell), "Review submitted");
    });
}

/// Request changes goes as itself with the summary typed in the hub, and
/// the hub draws the decision and the summary input.
#[test]
fn request_changes_goes_with_its_summary() {
    let repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let github = Arc::new(two());
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = working(&repo, &github, storage.path());
        let mut hl = Highlighter::new();
        open(&mut shell);
        to_row(&mut shell, "Decision");
        shell.handle_key(key(KeyCode::Enter));
        shell.handle_key(key(KeyCode::Enter));
        let frame = rows(&shell_frame(&mut shell, &mut hl)).join("\n");
        assert!(frame.contains("Decision: Request changes"), "{frame}");
        to_row(&mut shell, "Summary");
        shell.handle_key(key(KeyCode::Enter));
        assert_eq!(shell.view().input_mode, Mode::ReviewSummary);
        type_text(&mut shell, "Fix it");
        let frame = rows(&shell_frame(&mut shell, &mut hl)).join("\n");
        assert!(frame.contains(" summary  Fix it"), "{frame}");
        shell.handle_key(key(KeyCode::Enter));
        assert!(matches!(shell.view().overlay, Some(Overlay::Review)));
        to_row(&mut shell, "Submit review");
        shell.handle_key(key(KeyCode::Enter));
        settle_replies(&mut shell);
        let sent = submits(&github);
        assert_eq!(sent.len(), 1, "{:?}", github.calls());
        assert!(
            sent[0].contains("event=\"REQUEST_CHANGES\", body=\"Fix it\""),
            "{sent:?}"
        );
    });
}

/// A remove asks first, names the comment by its node id, and the hub
/// lists what GitHub reads back.
#[test]
fn a_remove_names_the_comment_by_its_node_id() {
    let repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let github = Arc::new(two().with_pending_review_remove_comment(Ok(())));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = working(&repo, &github, storage.path());
        open(&mut shell);
        to_row(&mut shell, "src/f000.rs:4");
        shell.handle_key(key(KeyCode::Enter));
        let asked = Mode::Confirm(Ask::RemoveReviewComment);
        assert_eq!(shell.view().input_mode, asked);
        github.set_pending_review(Ok(draft(vec![comment(1, 2)])));
        shell.handle_key(key(KeyCode::Char('y')));
        settle_replies(&mut shell);
        let removed = calls(&github, "pending_review_remove_comment");
        assert_eq!(
            removed,
            ["pending_review_remove_comment(comment_node_id=\"PRRC_2\")"]
        );
        assert_eq!(
            labels(&shell)[..2],
            ["Pending review: 1 comments", "src/f000.rs:2"]
        );
        assert!(matches!(shell.view().overlay, Some(Overlay::Review)));
    });
}

/// A remove made elsewhere on the same PR, as another tab would make it,
/// reaches the open hub through `PendingReviewChanged`.
#[test]
fn the_hub_reads_again_when_the_review_changes() {
    let repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let github = Arc::new(two().with_pending_review_remove_comment(Ok(())));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = working(&repo, &github, storage.path());
        open(&mut shell);
        github.set_pending_review(Ok(draft(vec![comment(1, 2)])));
        let content = shell.active_content().unwrap_or_else(|| panic!("gone"));
        let remove = Command::RemovePendingReviewComment {
            context: content.context.clone(),
            comment_node_id: "PRRC_2".into(),
        };
        let workspace = content.context.workspace_key();
        let replies = shell.host().post(&workspace, remove);
        let reply = replies.recv_timeout(WAIT);
        assert!(matches!(reply, Ok(Ok(_))), "{reply:?}");
        let one = |shell: &Shell| labels(shell)[0] == "Pending review: 1 comments";
        assert!(settle_within(&mut shell, WAIT, one), "{:?}", labels(&shell));
    });
}

/// Approving your own PR from the hub shows the refusal once, and the hub
/// stays open with its summary.
#[test]
fn a_refused_submit_keeps_the_hub() {
    let repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let github = Arc::new(
        two()
            .with_viewer_login(Ok("me".into()))
            .with_overview_no_checks(Ok(Some(overview("me")))),
    );
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = working(&repo, &github, storage.path());
        open(&mut shell);
        to_row(&mut shell, "Summary");
        shell.handle_key(key(KeyCode::Enter));
        type_text(&mut shell, "LGTM");
        shell.handle_key(key(KeyCode::Enter));
        to_row(&mut shell, "Decision");
        shell.handle_key(key(KeyCode::Enter));
        to_row(&mut shell, "Submit review");
        shell.handle_key(key(KeyCode::Enter));
        settle_replies(&mut shell);
        assert_eq!(notice(&shell), OWN_PR_APPROVAL_REFUSED);
        assert_eq!(shell.settings().times_shown(OWN_PR_APPROVAL_REFUSED), 1);
        assert!(submits(&github).is_empty());
        assert!(matches!(shell.view().overlay, Some(Overlay::Review)));
        assert_eq!(hub(&shell).summary, "LGTM");
    });
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

/// The composer's fourth kind adds the line to the pending review and posts
/// no single comment; a refused add keeps the text.
#[test]
fn the_review_kind_adds_to_the_pending_review() {
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        for refused in [false, true] {
            let repo = fixture(1);
            let answer = if refused {
                Err(GhError::Network {
                    message: "connection reset".into(),
                })
            } else {
                Ok(())
            };
            let github = Arc::new(two().with_pending_review_add_comment(answer));
            let mut shell = working(&repo, &github, storage.path());
            for _ in 0..4 {
                shell.handle_key(key(KeyCode::Down));
            }
            shell.handle_key(key(KeyCode::Char('c')));
            shell.handle_key(ctrl('t'));
            assert!(
                state(&shell).draft.review,
                "Ctrl+T did not reach the review kind"
            );
            type_text(&mut shell, "Join the review");
            shell.handle_key(key(KeyCode::Enter));
            settle_replies(&mut shell);
            let added = calls(&github, "pending_review_add_comment");
            assert_eq!(added.len(), 1, "{:?}", github.calls());
            assert!(added[0].contains("src/f000.rs"), "{added:?}");
            assert!(calls(&github, "post_line_comment").is_empty());
            let text = state(&shell).draft.textarea.lines().join("\n");
            if refused {
                assert_eq!(text, "Join the review");
                assert_eq!(shell.view().input_mode, Mode::Comment);
            } else {
                assert_eq!(text, "");
                assert_eq!(notice(&shell), "Comment added to your pending review");
            }
        }
    });
}

/// A review draft goes to the session file as "review" and comes back as
/// the review kind.
#[test]
fn a_review_draft_survives_the_session() {
    let repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let github = Arc::new(two());
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = working(&repo, &github, storage.path());
        for _ in 0..4 {
            shell.handle_key(key(KeyCode::Down));
        }
        shell.handle_key(key(KeyCode::Char('c')));
        shell.handle_key(ctrl('t'));
        type_text(&mut shell, "Later");
        let content = shell.active_content().unwrap_or_else(|| panic!("gone"));
        let saved = super::session::capture(&content, state(&shell));
        assert_eq!(saved.comment_draft_type, "review");
        let mut restored = TabViewState::default();
        super::session::restore(&mut restored, saved, &content);
        assert!(restored.draft.review);
        assert_eq!(restored.draft.textarea.lines(), ["Later"]);
    });
}
