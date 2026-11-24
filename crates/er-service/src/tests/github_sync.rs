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

pub(super) const RAW: &str = "\
diff --git a/src/b.rs b/src/b.rs
--- a/src/b.rs
+++ b/src/b.rs
@@ -1,3 +1,3 @@
 b-one
-b-old
+b-new
 b-three
";

pub(super) const WAIT: Duration = Duration::from_secs(5);

/// A remote PR tab, acme/widgets#7, whose comments live in `bucket`.
pub(super) fn pr_tab(bucket: &str) -> TabContext {
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

pub(super) fn acme_7() -> PrRef {
    PrRef {
        owner: "acme".to_string(),
        repo: "widgets".to_string(),
        number: 7,
    }
}

pub(super) fn actor(github: &Arc<FakeGitHub>) -> WorkspaceHandle {
    let git: Arc<dyn er_git::Git> = Arc::new(FakeGit::default().with_is_merge_in_progress(false));
    let github: Arc<dyn er_github::GitHub> = github.clone();
    spawn(WorkspaceDeps::new(Ports { git, github }))
}

/// The method names the fake saw, in order, for the methods in `of`.
pub(super) fn calls_named(github: &FakeGitHub, of: &[&str]) -> Vec<String> {
    github
        .calls()
        .into_iter()
        .filter_map(|call| call.split('(').next().map(str::to_string))
        .filter(|name| of.contains(&name.as_str()))
        .collect()
}

pub(super) fn overview(author: &str) -> er_github::PrOverviewData {
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
            .with_overview_remote(Ok(Some(overview("author"))))
            .with_pending_review(Ok(PendingReview::default())),
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

/// The guard sees only `deps.ports()`, so the bundle read is held and the
/// actor must answer meanwhile: a fetch on the actor thread would block it.
#[test]
fn the_sync_fetch_runs_off_the_actor_thread() -> anyhow::Result<()> {
    let (dir, bucket) = seeded_bucket()?;
    let github = syncing_github()?;
    er_engine::storage::with_process_test_storage_root(dir.path(), || -> anyhow::Result<()> {
        let handle = actor(&github);
        let hold = github.hold_pr_comment_bundle();
        let synced = handle.send(sync(&bucket));
        let deadline = Instant::now() + WAIT;
        let bundle = ["pr_comment_bundle_cached"];
        while calls_named(&github, &bundle).is_empty() && Instant::now() < deadline {
            std::thread::yield_now();
        }
        let context = pr_tab(&bucket);
        let read = handle.send(Command::GetPendingReview { context });
        let answered = read.recv_timeout(WAIT);
        drop(hold);
        assert!(matches!(answered, Ok(Ok(_))), "stalled: {answered:?}");
        let reply = synced.recv_timeout(WAIT)?;
        assert!(
            matches!(reply, Ok(Reply::CommentsSynced { .. })),
            "the sync did not answer: {reply:?}"
        );
        assert_eq!(
            calls_named(&github, &bundle).len(),
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

/// The pulled comments reach the AI section of a loaded tab that reads the
/// synced bucket, so the published content follows a sync without a reload.
#[test]
fn a_sync_patches_the_ai_section_of_the_tab_on_its_bucket() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(&repo)?;
    let github = syncing_github()?;
    let storage = dir.path().join("storage");
    er_engine::storage::with_process_test_storage_root(storage, || -> anyhow::Result<()> {
        let git: Arc<dyn er_git::Git> = Arc::new(
            FakeGit::default()
                .with_is_merge_in_progress(false)
                .with_current_branch(Ok("feature".to_string()))
                .with_diff_shortstat((0, 0, 0))
                .with_diff_raw(Ok(RAW.to_string())),
        );
        let github_port: Arc<dyn er_github::GitHub> = github.clone();
        let handle = spawn(WorkspaceDeps::new(Ports {
            git,
            github: github_port,
        }));
        let key = er_engine::TabKey::next();
        let tab = er_engine::tabs_record::TabDescriptor {
            kind: er_engine::tabs_record::TabKind::Working,
            repo_root: repo.to_string_lossy().into_owned(),
            branch: None,
            pr_owner: None,
            pr_repo: None,
            pr_number: None,
            pr_head_ref: None,
            base_ref: Some("main".to_string()),
            extra: serde_json::Map::new(),
        };
        let mode = None;
        handle.send_and_wait_with(Command::LoadTab { key, tab, mode }, WAIT)?;
        let loaded = handle.view().tab(key).cloned();
        let before = loaded.ok_or_else(|| anyhow::anyhow!("the tab did not load"))?;
        let context = TabContext {
            is_remote: true,
            remote_repo: Some("acme/widgets".to_string()),
            pr_number: Some(7),
            ..before.context.clone()
        };
        let source = SyncSource::Manual { force: false };

        handle.send_and_wait_with(Command::SyncGithubComments { context, source }, WAIT)?;

        let after = handle.view().tab(key).cloned();
        let after = after.ok_or_else(|| anyhow::anyhow!("the sync dropped the tab"))?;
        assert!(
            after.ai.revision > before.ai.revision,
            "the AI section kept"
        );
        let ids: Vec<String> = after
            .ai
            .state
            .github_comments
            .iter()
            .flat_map(|stored| stored.comments.iter().map(|c| c.id.clone()))
            .collect();
        assert!(ids.contains(&"gh-42".to_string()), "{ids:?}");
        assert!(
            Arc::ptr_eq(&before.diff, &after.diff),
            "the sync moved the diff"
        );
        Ok(())
    })
}

/// A pending review holding one draft comment, `PRRC_0`.
pub(super) fn pending() -> PendingReview {
    let draft = PendingReviewComment {
        id: "PRRC_0".to_string(),
        database_id: None,
        path: "src/b.rs".to_string(),
        line: Some(2),
        start_line: None,
        side: "RIGHT".to_string(),
        body: "draft".to_string(),
    };
    PendingReview {
        review_id: Some(9),
        comments: vec![draft],
        ..PendingReview::default()
    }
}

pub(super) fn line_two() -> ReviewLineTarget {
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
            .with_pending_review(Ok(pending()))
            .with_pending_review_add_comment(Ok(()))
            .with_pending_review_remove_comment(Ok(()))
            .with_pending_review_submit(Ok(1)),
    );
    let handle = actor(&github);
    let tab = pr_tab("/unused");
    let expected = Reply::PendingReview {
        pr: acme_7(),
        review: pending(),
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

    let (read, add, remove, submit, drop) = (
        "pending_review",
        "pending_review_add_comment",
        "pending_review_remove_comment",
        "pending_review_submit",
        "invalidate_comments",
    );
    let order = calls_named(&github, &[read, add, remove, submit, drop]);
    assert_eq!(order, [read, add, drop, read, remove, read, submit, drop]);
    let calls = github.calls().join("\n");
    assert!(calls.contains("PRRC_0"), "the comment id was lost: {calls}");
    assert!(
        calls.contains("\"looks good\""),
        "the summary was lost: {calls}"
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
