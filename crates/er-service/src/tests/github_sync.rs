//! Comment sync and the pending review through the actor, against a scripted
//! GitHub. No test here runs `gh`: an unscripted call answers with an error.

use std::sync::Arc;
use std::time::{Duration, Instant};

use er_engine::ports::Ports;
use er_git::FakeGit;
use er_github::{FakeGitHub, GhError, PendingReview, PendingReviewComment};

use crate::actor::WorkspaceDeps;
use crate::handle::WorkspaceError;
use crate::protocol::{
    Command, Event, PrRef, Reply, ReviewEvent, ReviewLineTarget, SyncSource, TabContext,
};
use crate::{spawn, WorkspaceHandle, OWN_PR_APPROVAL_REFUSED};

const RAW: &str = "\
diff --git a/src/b.rs b/src/b.rs
--- a/src/b.rs
+++ b/src/b.rs
@@ -1,3 +1,3 @@
 b-one
-b-old
+b-new
 b-three
";

const WAIT: Duration = Duration::from_secs(5);

/// A remote PR tab, acme/widgets#7, whose comments live in `bucket`.
fn pr_tab(bucket: &str) -> TabContext {
    TabContext {
        er_dir: bucket.to_string(),
        comments_dir: bucket.to_string(),
        is_remote: true,
        repo_root: "/repo".to_string(),
        base_branch: "main".to_string(),
        scope: "branch".to_string(),
        remote_repo: Some("acme/widgets".to_string()),
        pr_number: Some(7),
        pr_head_ref: None,
        local_branch_view: None,
        checkout_root: None,
        branch_diff_hash: String::new(),
        diff_hash: String::new(),
    }
}

fn acme_7() -> PrRef {
    PrRef {
        owner: "acme".to_string(),
        repo: "widgets".to_string(),
        number: 7,
    }
}

fn actor(github: &Arc<FakeGitHub>) -> WorkspaceHandle {
    let git: Arc<dyn er_git::Git> = Arc::new(FakeGit::default());
    let github: Arc<dyn er_github::GitHub> = github.clone();
    spawn(WorkspaceDeps::new(Ports { git, github }))
}

/// The method names the fake saw, in order, for the methods in `of`.
fn calls_named(github: &FakeGitHub, of: &[&str]) -> Vec<String> {
    github
        .calls()
        .into_iter()
        .filter_map(|call| call.split('(').next().map(str::to_string))
        .filter(|name| of.contains(&name.as_str()))
        .collect()
}

fn overview(author: &str) -> er_github::PrOverviewData {
    er_github::PrOverviewData {
        number: 7,
        title: "T".to_string(),
        body: String::new(),
        state: "OPEN".to_string(),
        author: author.to_string(),
        url: String::new(),
        base_branch: "main".to_string(),
        head_branch: "feature".to_string(),
        checks: Vec::new(),
        reviewers: Vec::new(),
    }
}

/// One review comment on `src/b.rs` line 2, as GitHub lists it.
fn listed_comment() -> anyhow::Result<er_github::GitHubComment> {
    Ok(serde_json::from_value(serde_json::json!({
        "id": 42, "body": "from github", "path": "src/b.rs", "line": 2,
        "original_line": 2, "side": "RIGHT", "user": { "login": "reviewer" },
        "created_at": "2026-01-01T00:00:00Z", "updated_at": "2026-01-01T00:00:00Z",
        "diff_hunk": null,
    }))?)
}

/// A fake that answers a manual, unforced sync of acme/widgets#7.
fn syncing_github() -> anyhow::Result<Arc<FakeGitHub>> {
    let bundle = er_github::PrCommentBundle {
        comments: vec![listed_comment()?],
        threads: std::collections::HashMap::new(),
    };
    Ok(Arc::new(
        FakeGitHub::default()
            .with_pr_diff_remote(Ok(RAW.to_string()))
            .with_pr_comment_bundle_cached(Ok(Arc::new(bundle)))
            .with_overview_remote(Ok(Some(overview("author")))),
    ))
}

/// A temp root with one bucket holding a private local comment.
fn seeded_bucket() -> anyhow::Result<(tempfile::TempDir, String)> {
    let dir = tempfile::tempdir()?;
    let bucket = dir.path().join("bucket");
    std::fs::create_dir_all(&bucket)?;
    er_engine::test_support::write_sidecar_json::<er_engine::ai::ErGithubComments>(
        &bucket,
        serde_json::json!({
            "version": 1,
            "diff_hash": "",
            "comments": [{
                "id": "c-local-1",
                "timestamp": "2026-01-01T00:00:00Z",
                "file": "src/b.rs",
                "hunk_index": 0,
                "line_start": 1,
                "line_content": "b-one",
                "comment": "kept locally",
                "author": "you",
                "source": "local",
                "side": "RIGHT"
            }]
        }),
    );
    Ok((dir, bucket.to_string_lossy().into_owned()))
}

fn sync(bucket: &str) -> Command {
    Command::SyncGithubComments {
        context: pr_tab(bucket),
        source: SyncSource::Manual { force: false },
    }
}

fn comments_path(bucket: &str) -> anyhow::Result<String> {
    let store = er_engine::store_registry::store();
    let scope = er_store::Scope::dir(bucket)?;
    Ok(
        er_engine::ai::annotation_store::path_of::<er_engine::ai::ErGithubComments>(
            &store, &scope,
        )?
        .to_string_lossy()
        .into_owned(),
    )
}

/// A GitHub read on the actor thread trips the guard and kills the actor,
/// so a sync that answers at all fetched on an effect thread.
#[test]
fn the_sync_fetch_runs_off_the_actor_thread() -> anyhow::Result<()> {
    let (dir, bucket) = seeded_bucket()?;
    let github = syncing_github()?;
    er_engine::storage::with_process_test_storage_root(dir.path(), || -> anyhow::Result<()> {
        let handle = actor(&github);
        let reply = handle.send_and_wait_with(sync(&bucket), WAIT);
        assert!(
            matches!(reply, Ok(Reply::CommentsSynced { .. })),
            "the sync did not answer: {reply:?}"
        );
        assert_eq!(
            calls_named(&github, &["pr_comment_bundle_cached"]).len(),
            1,
            "{:?}",
            github.calls()
        );
        Ok(())
    })
}

/// The effect only reads; the actor thread writes the pulled comments, once.
#[test]
fn the_sync_apply_is_the_only_writer() -> anyhow::Result<()> {
    let (dir, bucket) = seeded_bucket()?;
    let github = syncing_github()?;
    er_engine::storage::with_process_test_storage_root(dir.path(), || -> anyhow::Result<()> {
        let handle = actor(&github);
        handle.send_and_wait_with(sync(&bucket), WAIT)?;
        let writers = er_engine::sync::write_recorder::writers_of(&comments_path(&bucket)?);
        assert_eq!(writers, vec![Some("er-workspace-actor".to_string())]);
        Ok(())
    })
}

#[test]
fn a_fake_backed_sync_merges_github_comments_into_the_sidecar() -> anyhow::Result<()> {
    let (dir, bucket) = seeded_bucket()?;
    let github = syncing_github()?;
    er_engine::storage::with_process_test_storage_root(dir.path(), || -> anyhow::Result<()> {
        let handle = actor(&github);
        let reply = handle.send_and_wait_with(sync(&bucket), WAIT)?;
        assert_eq!(
            reply,
            Reply::CommentsSynced {
                github_count: 1,
                local_count: 1
            }
        );

        let store = er_engine::store_registry::store();
        let scope = er_store::Scope::dir(&bucket)?;
        let er_store::Loaded::Found(stored) =
            er_engine::ai::annotation_store::load_github_comments(&store, &scope)?
        else {
            anyhow::bail!("the sync wrote no sidecar");
        };
        let comments = &stored.value.comments;
        assert!(
            comments.iter().any(|c| c.id == "c-local-1"),
            "the local row was dropped: {comments:?}"
        );
        let pulled = comments
            .iter()
            .find(|c| c.id == "gh-42")
            .ok_or_else(|| anyhow::anyhow!("the GitHub row is missing: {comments:?}"))?;
        assert_eq!(pulled.comment, "from github");
        assert_eq!(pulled.line_content, "b-new", "anchored to the fetched diff");
        let sync_state = stored.value.github.as_ref();
        assert_eq!(sync_state.map(|s| s.owner.as_str()), Some("acme"));
        Ok(())
    })
}

#[test]
fn a_sync_publishes_an_event_the_relay_hears() -> anyhow::Result<()> {
    let (dir, bucket) = seeded_bucket()?;
    let github = syncing_github()?;
    er_engine::storage::with_process_test_storage_root(dir.path(), || -> anyhow::Result<()> {
        let handle = actor(&github);
        let events = handle.subscribe();
        let before = handle.view().revision;
        let _replies = handle.send(sync(&bucket));

        let deadline = Instant::now() + WAIT;
        let mut synced = None;
        while synced.is_none() {
            let left = deadline.saturating_duration_since(Instant::now());
            match events.recv_timeout(left)? {
                Event::CommentsSynced {
                    context,
                    github_count,
                    local_count,
                } => synced = Some((context, github_count, local_count)),
                Event::CommandFailed { message } => anyhow::bail!("the sync failed: {message}"),
                _ => {}
            }
        }
        assert_eq!(synced, Some((pr_tab(&bucket), 1, 1)));
        assert!(handle.view().revision > before, "the revision did not rise");
        Ok(())
    })
}

#[test]
fn a_command_for_another_repo_is_refused_before_the_sync_runs() -> anyhow::Result<()> {
    let (dir, bucket) = seeded_bucket()?;
    let github = Arc::new(
        FakeGitHub::default()
            .with_pr_state(Ok("OPEN".to_string()))
            .with_pr_diff_remote(Ok(RAW.to_string())),
    );
    er_engine::storage::with_process_test_storage_root(dir.path(), || -> anyhow::Result<()> {
        let handle = actor(&github);
        handle.send_and_wait_with(
            Command::OpenTarget {
                target: crate::protocol::OpenTarget::RemotePr {
                    owner: "acme".to_string(),
                    repo: "widgets".to_string(),
                    number: 7,
                    repo_root: "/other".to_string(),
                },
                replace: false,
            },
            WAIT,
        )?;
        let reply = handle.send_and_wait_with(sync(&bucket), WAIT);
        assert!(matches!(reply, Err(WorkspaceError::Failed(_))), "{reply:?}");
        assert!(
            calls_named(&github, &["pr_comment_bundle_cached", "pr_diff_remote"]).is_empty(),
            "the stale sync ran: {:?}",
            github.calls()
        );
        Ok(())
    })
}

fn pending(comments: usize) -> PendingReview {
    PendingReview {
        review_id: Some(9),
        review_node_id: Some("PRR_9".to_string()),
        comments: (0..comments)
            .map(|index| PendingReviewComment {
                id: format!("PRRC_{index}"),
                database_id: None,
                path: "src/b.rs".to_string(),
                line: Some(2),
                start_line: None,
                side: "RIGHT".to_string(),
                body: "draft".to_string(),
            })
            .collect(),
        viewer: Some("you".to_string()),
        review_request: None,
    }
}

fn line_two() -> ReviewLineTarget {
    ReviewLineTarget {
        file: "src/b.rs".to_string(),
        line_num: 2,
        line_num_end: None,
        side: Some("RIGHT".to_string()),
    }
}

#[test]
fn a_fake_backed_pending_review_post_round_trips() -> anyhow::Result<()> {
    let github = Arc::new(
        FakeGitHub::default()
            .with_pending_review(Ok(pending(1)))
            .with_pending_review_add_comment(Ok(()))
            .with_pending_review_remove_comment(Ok(()))
            .with_pending_review_submit(Ok(1)),
    );
    let handle = actor(&github);
    let tab = pr_tab("/unused");
    let expected = Reply::PendingReview {
        pr: acme_7(),
        review: pending(1),
    };

    let read = Command::GetPendingReview {
        context: tab.clone(),
    };
    assert_eq!(handle.send_and_wait_with(read, WAIT)?, expected);
    let add = Command::AddPendingReviewComment {
        context: tab.clone(),
        target: line_two(),
        body: "  draft  ".to_string(),
    };
    assert_eq!(handle.send_and_wait_with(add, WAIT)?, expected);
    let remove = Command::RemovePendingReviewComment {
        context: tab.clone(),
        comment_node_id: "PRRC_0".to_string(),
    };
    assert_eq!(handle.send_and_wait_with(remove, WAIT)?, expected);
    let submit = Command::SubmitPendingReview {
        context: tab,
        event: ReviewEvent::Comment,
        summary: "  looks good ".to_string(),
    };
    assert_eq!(
        handle.send_and_wait_with(submit, WAIT)?,
        Reply::ReviewSubmitted { pr: acme_7() }
    );

    let order = calls_named(
        &github,
        &[
            "pending_review",
            "pending_review_add_comment",
            "pending_review_remove_comment",
            "pending_review_submit",
        ],
    );
    assert_eq!(
        order,
        [
            "pending_review",
            "pending_review_add_comment",
            "pending_review",
            "pending_review_remove_comment",
            "pending_review",
            "pending_review_submit",
        ]
    );
    let calls = github.calls();
    assert!(
        calls.iter().any(|call| call.contains("PRRC_0")),
        "the named comment was not removed: {calls:?}"
    );
    assert!(
        calls
            .iter()
            .any(|call| call.starts_with("pending_review_submit(")
                && call.contains("COMMENT")
                && call.contains("\"looks good\"")),
        "the submit lost its event or summary: {calls:?}"
    );
    Ok(())
}

#[test]
fn approving_your_own_pr_is_refused_before_the_submit() -> anyhow::Result<()> {
    let github = Arc::new(
        FakeGitHub::default()
            .with_viewer_login(Ok("you".to_string()))
            .with_overview_remote(Ok(Some(overview("You"))))
            .with_pending_review_submit(Ok(0)),
    );
    let handle = actor(&github);
    let reply = handle.send_and_wait_with(
        Command::SubmitPendingReview {
            context: pr_tab("/unused"),
            event: ReviewEvent::Approve,
            summary: String::new(),
        },
        WAIT,
    );
    assert_eq!(
        reply,
        Err(WorkspaceError::Failed(OWN_PR_APPROVAL_REFUSED.to_string()))
    );
    assert!(
        calls_named(&github, &["pending_review_submit"]).is_empty(),
        "the own-PR approval was sent: {:?}",
        github.calls()
    );
    Ok(())
}

/// ADR 0055 on the flag path: the caller sees GitHub's classification, not
/// a sentence it would have to read.
#[test]
fn a_github_error_crosses_the_service_typed() -> anyhow::Result<()> {
    let refused = GhError::Unprocessable {
        message: "Validation Failed".to_string(),
    };
    let github = Arc::new(FakeGitHub::default().with_pending_review_submit(Err(refused.clone())));
    let handle = actor(&github);
    let reply = handle.send_and_wait_with(
        Command::SubmitPendingReview {
            context: pr_tab("/unused"),
            event: ReviewEvent::RequestChanges,
            summary: "please fix".to_string(),
        },
        WAIT,
    );
    assert_eq!(reply, Err(WorkspaceError::Github(refused)));
    Ok(())
}
