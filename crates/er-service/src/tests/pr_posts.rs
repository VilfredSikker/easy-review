//! The PR-level posts through the actor, against a scripted GitHub.

use std::sync::Arc;

use er_engine::github_post::PENDING_REVIEW_BLOCKS_COMMENT;
use er_github::{CreatedComment, FakeGitHub, GhError, PendingReview};

use super::github_sync::{
    acme_7, actor, calls_named, line_two, overview, pending, pr_tab, RAW, WAIT,
};
use crate::handle::WorkspaceError;
use crate::protocol::{Command, Event, Reply, ReviewEvent};
use crate::OWN_PR_APPROVAL_REFUSED;

fn created(id: u64) -> CreatedComment {
    CreatedComment {
        id,
        created_at: "2026-01-01T00:00:00Z".to_string(),
        author: "you".to_string(),
    }
}

fn decision(event: ReviewEvent) -> Command {
    Command::SubmitGithubPrDecision {
        context: pr_tab("/unused"),
        event,
        summary: " fine ".to_string(),
    }
}

#[test]
fn the_pr_level_posts_round_trip() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let bucket = dir.path().join("bucket");
    std::fs::create_dir_all(&bucket)?;
    let bucket = bucket.to_string_lossy().into_owned();
    let github = Arc::new(
        FakeGitHub::default()
            .with_pr_diff_remote(Ok(RAW.to_string()))
            .with_post_general(Ok(created(501)))
            .with_pending_review(Ok(PendingReview::default()))
            .with_pr_head_oid(Ok("abc123".to_string()))
            .with_post_line_comment(Ok(created(502)))
            .with_submit_review_remote(Ok(())),
    );
    er_engine::storage::with_process_test_storage_root(dir.path(), || -> anyhow::Result<()> {
        let handle = actor(&github);
        let events = handle.subscribe();
        let posted = Reply::CommentPosted { pr: acme_7() };
        let general = Command::PostGithubPrComment {
            context: pr_tab(&bucket),
            body: "ship it".to_string(),
            client_id: None,
        };
        assert_eq!(handle.send_and_wait_with(general, WAIT)?, posted);
        let line = Command::PostGithubLineComment {
            context: pr_tab(&bucket),
            target: line_two(),
            body: "here".to_string(),
        };
        assert_eq!(handle.send_and_wait_with(line, WAIT)?, posted);
        assert_eq!(
            handle.send_and_wait_with(decision(ReviewEvent::Comment), WAIT)?,
            Reply::ReviewSubmitted { pr: acme_7() }
        );

        let (general, check, line, submit) = (
            "post_general",
            "pending_review",
            "post_line_comment",
            "submit_review_remote",
        );
        let order = calls_named(&github, &[general, check, line, submit]);
        assert_eq!(order, [general, check, line, submit]);
        let announced = events
            .try_iter()
            .filter(|event| matches!(event, Event::CommentPosted { .. }))
            .count();
        assert_eq!(announced, 3);

        // The line comment is recorded where it anchors; the PR-wide one is not.
        let store = er_engine::store_registry::store();
        let scope = er_store::Scope::dir(&bucket)?;
        let er_store::Loaded::Found(stored) =
            er_engine::ai::annotation_store::load_github_comments(&store, &scope)?
        else {
            anyhow::bail!("the line comment was not recorded");
        };
        let ids: Vec<&str> = stored
            .value
            .comments
            .iter()
            .map(|c| c.id.as_str())
            .collect();
        assert_eq!(ids, ["gh-502"]);
        assert_eq!(stored.value.comments[0].line_content, "b-new");
        Ok(())
    })
}

/// GitHub would attach a standalone line comment to the viewer's draft, so
/// the post is refused before anything is sent.
#[test]
fn a_line_comment_is_refused_while_a_review_is_pending() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let github = Arc::new(
        FakeGitHub::default()
            .with_pr_diff_remote(Ok(RAW.to_string()))
            .with_pending_review(Ok(pending()))
            .with_pr_head_oid(Ok("abc123".to_string()))
            .with_post_line_comment(Ok(created(503))),
    );
    er_engine::storage::with_process_test_storage_root(dir.path(), || -> anyhow::Result<()> {
        let handle = actor(&github);
        let line = Command::PostGithubLineComment {
            context: pr_tab(&dir.path().to_string_lossy()),
            target: line_two(),
            body: "here".to_string(),
        };
        let reply = handle.send_and_wait_with(line, WAIT);
        let refused = reply
            .as_ref()
            .is_err_and(|error| error.to_string().contains(PENDING_REVIEW_BLOCKS_COMMENT));
        assert!(refused, "the line comment was not refused: {reply:?}");
        assert!(
            calls_named(&github, &["post_line_comment"]).is_empty(),
            "the refused comment was posted: {:?}",
            github.calls()
        );
        Ok(())
    })
}

#[test]
fn approving_your_own_pr_decision_is_refused() {
    let github = Arc::new(
        FakeGitHub::default()
            .with_viewer_login(Ok("you".to_string()))
            .with_overview_remote(Ok(Some(overview("You"))))
            .with_submit_review_remote(Ok(())),
    );
    let reply = actor(&github).send_and_wait_with(decision(ReviewEvent::Approve), WAIT);
    let refused = WorkspaceError::Failed(OWN_PR_APPROVAL_REFUSED.to_string());
    assert_eq!(reply, Err(refused));
    assert!(calls_named(&github, &["submit_review_remote"]).is_empty());
}

#[test]
fn a_decision_error_crosses_the_service_typed() {
    let refused = GhError::Unprocessable {
        message: "Validation Failed".to_string(),
    };
    let github = Arc::new(FakeGitHub::default().with_submit_review_remote(Err(refused.clone())));
    let reply = actor(&github).send_and_wait_with(decision(ReviewEvent::RequestChanges), WAIT);
    assert_eq!(reply, Err(WorkspaceError::Github(refused)));
}

/// A bucket holding a posted comment and its two posted replies on
/// acme/widgets#7.
fn posted_thread() -> anyhow::Result<(tempfile::TempDir, String)> {
    let dir = tempfile::tempdir()?;
    let bucket = dir.path().join("bucket");
    std::fs::create_dir_all(&bucket)?;
    let row = |id: &str, github_id: u64, parent: Option<&str>| {
        serde_json::json!({
            "id": id, "timestamp": "2026-01-01T00:00:00Z", "file": "src/b.rs",
            "hunk_index": 0, "line_start": 1, "line_content": "b-one",
            "comment": "posted", "author": "You", "source": "github",
            "github_id": github_id, "in_reply_to": parent, "side": "RIGHT"
        })
    };
    er_engine::test_support::write_sidecar_json::<er_engine::ai::ErGithubComments>(
        &bucket,
        serde_json::json!({
            "version": 1, "diff_hash": "",
            "github": {"pr_number": 7, "owner": "acme", "repo": "widgets"},
            "comments": [
                row("gh-11", 11, None),
                row("gh-12", 12, Some("gh-11")),
                row("gh-13", 13, Some("gh-11"))
            ]
        }),
    );
    Ok((dir, bucket.to_string_lossy().into_owned()))
}

fn stored_ids(bucket: &str) -> anyhow::Result<Vec<String>> {
    let store = er_engine::store_registry::store();
    let scope = er_store::Scope::dir(bucket)?;
    let er_store::Loaded::Found(stored) =
        er_engine::ai::annotation_store::load_github_comments(&store, &scope)?
    else {
        anyhow::bail!("the comments file is gone");
    };
    Ok(stored.value.comments.into_iter().map(|c| c.id).collect())
}

/// A posted delete reaches GitHub for the replies, then the root, before
/// the rows go. A refusal keeps every row, drops the cache once anything
/// went, and says how far it got and what to do next.
#[test]
fn a_posted_delete_removes_its_rows_only_once_github_has() -> anyhow::Result<()> {
    let no = || {
        Err(GhError::Forbidden {
            message: "not yours".to_string(),
        })
    };
    let cases = [
        (
            vec![no()],
            Some("Deleted 0 of 2 replies on GitHub; GitHub refused the next one"),
        ),
        (
            vec![Ok(()), no()],
            Some("Deleted 1 of 2 replies on GitHub; GitHub refused the next one"),
        ),
        (
            vec![Ok(()), Ok(()), no()],
            Some("Deleted 2 of 2 replies on GitHub; GitHub refused the comment itself"),
        ),
        (vec![Ok(()), Ok(()), Ok(())], None),
    ];
    for (answers, said) in cases {
        let (dir, bucket) = posted_thread()?;
        let tried = answers.len();
        let github = Arc::new(FakeGitHub::default().with_delete_comment_answers(answers));
        er_engine::storage::with_process_test_storage_root(dir.path(), || {
            let handle = actor(&github);
            let delete = Command::DeleteGithubComment {
                context: pr_tab(&bucket),
                id: "gh-11".to_string(),
            };
            let reply = handle.send_and_wait_with(delete, WAIT);
            let left = stored_ids(&bucket)?;
            let calls = github.calls();
            let deletes: Vec<&String> = calls
                .iter()
                .filter(|call| call.starts_with("delete_comment("))
                .collect();
            assert_eq!(deletes.len(), tried, "{calls:?}");
            assert!(
                deletes[0].contains("comment_id=12"),
                "the root went first: {deletes:?}"
            );
            let dropped = calls
                .iter()
                .any(|call| call.starts_with("invalidate_comments"));
            assert_eq!(dropped, tried > 1, "{calls:?}");
            match said {
                None => assert!(left.is_empty() && reply.is_ok(), "{left:?} {reply:?}"),
                Some(prefix) => {
                    assert_eq!(
                        left,
                        ["gh-11", "gh-12", "gh-13"],
                        "a refused delete lost rows"
                    );
                    let message = reply.err().map(|e| e.to_string()).unwrap_or_default();
                    assert!(message.starts_with(prefix), "{message}");
                    let next = if tried == 3 {
                        "retry"
                    } else {
                        "Delete that reply on GitHub"
                    };
                    assert!(message.contains(next), "{message}");
                }
            }
            Ok::<_, anyhow::Error>(())
        })?;
    }
    Ok(())
}

fn stored_text(bucket: &str, id: &str) -> anyhow::Result<String> {
    let store = er_engine::store_registry::store();
    let scope = er_store::Scope::dir(bucket)?;
    let er_store::Loaded::Found(stored) =
        er_engine::ai::annotation_store::load_github_comments(&store, &scope)?
    else {
        anyhow::bail!("the comments file is gone");
    };
    let row = stored.value.comments.into_iter().find(|c| c.id == id);
    Ok(row.map(|row| row.comment).unwrap_or_default())
}

fn edits(github: &FakeGitHub) -> usize {
    let calls = github.calls();
    calls
        .iter()
        .filter(|call| call.starts_with("update_review_comment("))
        .count()
}

/// Gives `gh-12` another author, as a pull would record someone else's reply.
fn authored(bucket: &str, author: &str) {
    let dir = std::path::Path::new(bucket);
    let mut stored = er_engine::test_support::read_sidecar::<er_engine::ai::ErGithubComments>(dir);
    stored.comments[1].author = author.to_string();
    let value = serde_json::to_value(&stored).unwrap_or_else(|e| panic!("{e}"));
    er_engine::test_support::write_sidecar_json::<er_engine::ai::ErGithubComments>(dir, value);
}

/// An edit of a posted comment reaches GitHub before its row changes, and
/// drops the pull's cached list, which still holds the old body; a refused
/// edit keeps the row and says so in `update_comment_text`'s words. Another
/// author's row and an AI row are refused before GitHub is asked.
#[test]
fn a_posted_edit_writes_its_row_only_once_github_has() -> anyhow::Result<()> {
    let refused = GhError::Forbidden {
        message: "not yours".to_string(),
    };
    let cases = [
        ("You", Ok(()), None),
        ("You", Err(refused), Some("Failed to update comment: ")),
        ("reviewer", Ok(()), Some("Can only edit your own messages")),
        ("ai", Ok(()), Some("Cannot edit AI-generated text")),
    ];
    for (author, answer, said) in cases {
        let (dir, bucket) = posted_thread()?;
        let github = Arc::new(FakeGitHub::default().with_update_review_comment(answer));
        er_engine::storage::with_process_test_storage_root(dir.path(), || {
            authored(&bucket, author);
            let edit = Command::UpdateGithubComment {
                context: pr_tab(&bucket),
                id: "gh-12".to_string(),
                body: " edited ".to_string(),
            };
            let reply = actor(&github).send_and_wait_with(edit, WAIT);
            let calls = github.calls();
            let asked = author == "You";
            assert_eq!(edits(&github), usize::from(asked), "{author}: {calls:?}");
            if asked {
                let edit = calls
                    .iter()
                    .find(|c| c.starts_with("update_review_comment("));
                let edit = edit.map(String::as_str).unwrap_or_default();
                assert!(edit.contains("comment_id=12, body=\"edited\""), "{edit}");
            }
            let text = stored_text(&bucket, "gh-12")?;
            match said {
                None => {
                    assert!(reply.is_ok(), "{reply:?}");
                    assert_eq!(text, "edited");
                    let dropped = calls.iter().any(|c| c.starts_with("invalidate_comments"));
                    assert!(dropped, "the cached list keeps the old body: {calls:?}");
                }
                Some(prefix) => {
                    assert_eq!(text, "posted", "{author}: a refused edit changed the row");
                    let message = reply.err().map(|e| e.to_string()).unwrap_or_default();
                    assert!(message.starts_with(prefix), "{author}: {message}");
                }
            }
            Ok::<_, anyhow::Error>(())
        })?;
    }
    Ok(())
}

/// Every id but a question's or a note's is edited through
/// `UpdateGithubComment`, whose effect reads the stored row: a comment never
/// posted stays local, and a posted one no loaded content knows of still
/// goes to GitHub first.
#[test]
fn an_edit_is_routed_by_its_id_and_the_stored_row() -> anyhow::Result<()> {
    let (dir, bucket) = posted_thread()?;
    let github = Arc::new(FakeGitHub::default().with_update_review_comment(Ok(())));
    for id in ["q-1", "n-1"] {
        let local = crate::edit_message(pr_tab(&bucket), id.into(), "x".into());
        assert!(matches!(local, Command::UpdateThreadMessage { .. }), "{id}");
    }
    er_engine::storage::with_process_test_storage_root(dir.path(), || {
        // A comment never posted, in the same PR's bucket: edited locally.
        let path = std::path::Path::new(&bucket);
        let mut stored =
            er_engine::test_support::read_sidecar::<er_engine::ai::ErGithubComments>(path);
        let mut local = stored.comments[0].clone();
        (local.id, local.github_id) = ("c-9".to_string(), None);
        stored.comments.push(local);
        let value = serde_json::to_value(&stored)?;
        er_engine::test_support::write_sidecar_json::<er_engine::ai::ErGithubComments>(path, value);
        let edit = crate::edit_message(pr_tab(&bucket), "c-9".into(), "kept here".into());
        let reply = actor(&github).send_and_wait_with(edit, WAIT);
        assert!(reply.is_ok(), "{reply:?}");
        assert!(
            github.calls().is_empty(),
            "an unposted edit reached GitHub: {:?}",
            github.calls()
        );
        assert_eq!(stored_text(&bucket, "c-9")?, "kept here");

        let edit = crate::edit_message(pr_tab(&bucket), "gh-11".into(), "edited".into());
        let reply = actor(&github).send_and_wait_with(edit, WAIT);
        assert!(reply.is_ok(), "{reply:?}");
        assert_eq!(edits(&github), 1, "{:?}", github.calls());
        assert_eq!(stored_text(&bucket, "gh-11")?, "edited");
        Ok::<_, anyhow::Error>(())
    })?;
    Ok(())
}
