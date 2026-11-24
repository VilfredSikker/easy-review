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

/// A bucket holding a posted comment and its posted reply on acme/widgets#7.
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
            "comments": [row("gh-11", 11, None), row("gh-12", 12, Some("gh-11"))]
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

/// A posted delete reaches GitHub for the root and its reply before the
/// rows go, and a refused one keeps both rows.
#[test]
fn a_posted_delete_removes_its_rows_only_once_github_has() -> anyhow::Result<()> {
    for refused in [true, false] {
        let (dir, bucket) = posted_thread()?;
        let answer = if refused {
            Err(GhError::Forbidden {
                message: "not yours".to_string(),
            })
        } else {
            Ok(())
        };
        let github = Arc::new(FakeGitHub::default().with_delete_comment(answer));
        er_engine::storage::with_process_test_storage_root(dir.path(), || {
            let handle = actor(&github);
            let delete = Command::DeleteGithubComment {
                context: pr_tab(&bucket),
                id: "gh-11".to_string(),
            };
            let reply = handle.send_and_wait_with(delete, WAIT);
            assert_eq!(reply.is_err(), refused, "{reply:?}");
            let left = stored_ids(&bucket)?;
            let calls = calls_named(&github, &["delete_comment"]).len();
            if refused {
                assert_eq!(left, ["gh-11", "gh-12"], "a refused delete lost rows");
                assert_eq!(calls, 1);
            } else {
                assert!(left.is_empty(), "{left:?}");
                assert_eq!(calls, 2, "{:?}", github.calls());
            }
            Ok::<_, anyhow::Error>(())
        })?;
    }
    Ok(())
}
