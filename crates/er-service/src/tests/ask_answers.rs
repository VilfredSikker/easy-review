//! An ask's answers: written to the buckets the ask captured, whatever the
//! workspace shows when they land.

use std::sync::Arc;
use std::time::Duration;

use er_engine::ai::CommentType;
use er_engine::ports::Ports;
use er_engine::TabKey;
use er_git::FakeGit;
use er_github::FakeGitHub;

use crate::actor::WorkspaceDeps;
use crate::protocol::{AnnotationTarget, Command, OpenTarget, Reply, TabContext};
use crate::{spawn, WorkspaceHandle};

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

fn context(bucket: &str, repo_root: &str) -> TabContext {
    TabContext {
        er_dir: bucket.to_string(),
        comments_dir: bucket.to_string(),
        is_remote: false,
        repo_root: repo_root.to_string(),
        base_branch: "main".to_string(),
        scope: "branch".to_string(),
        remote_repo: None,
        pr_number: None,
        pr_head_ref: None,
        local_branch_view: Some("feature".to_string()),
        checkout_root: None,
        branch_diff_hash: String::new(),
        diff_hash: "diff-hash".to_string(),
    }
}

fn actor() -> WorkspaceHandle {
    let git: Arc<dyn er_git::Git> =
        Arc::new(FakeGit::default().with_diff_against_branch(Ok(RAW.to_string())));
    let github: Arc<dyn er_github::GitHub> =
        Arc::new(FakeGitHub::default().with_pr_state(Ok("OPEN".to_string())));
    spawn(WorkspaceDeps::new(Ports { git, github }))
}

fn target() -> AnnotationTarget {
    AnnotationTarget {
        file: "src/b.rs".to_string(),
        hunk_idx: 0,
        line_num: Some(2),
        line_num_end: None,
    }
}

/// Has the actor show a remote PR of another repo, so a write for
/// `/repo-b` names a workspace it does not show.
fn show_another_repo(handle: &WorkspaceHandle) -> anyhow::Result<()> {
    let opened = handle.send_and_wait_with(
        Command::OpenTarget {
            target: OpenTarget::RemotePr {
                owner: "acme".to_string(),
                repo: "widgets".to_string(),
                number: 7,
                repo_root: "/repo-a".to_string(),
            },
            replace: false,
        },
        WAIT,
    )?;
    anyhow::ensure!(matches!(opened, Reply::TargetOpened(_)), "got {opened:?}");
    Ok(())
}

fn write_finding(bucket: &std::path::Path) {
    er_engine::test_support::write_sidecar_json::<er_engine::ai::ErReview>(
        bucket,
        serde_json::json!({
            "version": 1,
            "diff_hash": "diff-hash",
            "files": {
                "src/b.rs": {
                    "risk": "low",
                    "findings": [{
                        "id": "f-1",
                        "severity": "low",
                        "title": "A defect",
                        "hunk_index": 0,
                        "line_start": 2
                    }]
                }
            }
        }),
    );
}

fn in_bucket(
    test: impl FnOnce(&str, &std::path::Path) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let bucket = dir.path().join("bucket");
    std::fs::create_dir_all(&bucket)?;
    let text = bucket.to_string_lossy().into_owned();
    er_engine::storage::with_process_test_storage_root(dir.path(), || test(&text, &bucket))
}

#[test]
fn an_ai_reply_lands_in_the_captured_bucket_while_the_workspace_shows_another_repo(
) -> anyhow::Result<()> {
    in_bucket(|bucket, _| {
        let handle = actor();
        let ctx = context(bucket, "/repo-b");
        // Written before the actor shows a target, which accepts any tab.
        let asked = handle.send_and_wait_with(
            Command::AddQuestion {
                context: ctx.clone(),
                target: target(),
                body: "Why?".to_string(),
                side: None,
                client_id: None,
                reply_to: None,
                finding_ref: None,
            },
            WAIT,
        )?;
        let Reply::AnnotationWritten { id: question } = asked else {
            anyhow::bail!("got {asked:?}");
        };
        show_another_repo(&handle)?;

        let answered = handle.send_and_wait_with(
            Command::SubmitAiReply {
                key: TabKey::next(),
                context: ctx.clone(),
                target: target(),
                body: "Because.".to_string(),
                kind: CommentType::Question,
                reply_to: question.clone(),
                ai_action: Some("ask".to_string()),
            },
            WAIT,
        );
        assert!(
            matches!(answered, Ok(Reply::AnnotationWritten { .. })),
            "got {answered:?}"
        );
        let questions = er_engine::ai::load_ai_state(bucket, &ctx.diff_hash, None)
            .questions
            .map(|q| q.questions)
            .unwrap_or_default();
        let reply = questions
            .iter()
            .find(|q| q.in_reply_to.as_deref() == Some(question.as_str()))
            .ok_or_else(|| anyhow::anyhow!("no reply in {questions:?}"))?;
        assert_eq!(reply.text, "Because.");
        assert_eq!(reply.author, "ai");
        assert_eq!(reply.ai_action.as_deref(), Some("ask"));
        Ok(())
    })
}

#[test]
fn a_finding_answer_lands_in_the_captured_bucket_while_the_workspace_shows_another_repo(
) -> anyhow::Result<()> {
    in_bucket(|bucket, path| {
        write_finding(path);
        let handle = actor();
        show_another_repo(&handle)?;
        let ctx = context(bucket, "/repo-b");
        let answered = handle.send_and_wait_with(
            Command::AppendFindingResponse {
                key: TabKey::next(),
                context: ctx.clone(),
                finding_id: "f-1".to_string(),
                body: "Confirmed.".to_string(),
            },
            WAIT,
        );
        assert!(matches!(answered, Ok(Reply::AiWritten)), "got {answered:?}");
        let review = er_engine::ai::load_ai_state(bucket, &ctx.diff_hash, None).review;
        let responses: Vec<String> = review
            .iter()
            .flat_map(|review| review.files.values())
            .flat_map(|file| file.findings.iter())
            .filter(|finding| finding.id == "f-1")
            .flat_map(|finding| finding.responses.iter().map(|r| r.text.clone()))
            .collect();
        assert_eq!(responses, ["Confirmed."]);
        Ok(())
    })
}

/// An AI-assisted finding reply finds or creates the finding's thread and
/// writes no reply of its own: the ask answers in the thread later.
#[test]
fn an_ai_assisted_finding_reply_creates_the_thread_once_and_writes_no_reply() -> anyhow::Result<()>
{
    in_bucket(|bucket, path| {
        write_finding(path);
        let handle = actor();
        let ask = || Command::ReplyToFinding {
            context: context(bucket, "/repo"),
            finding_id: "f-1".to_string(),
            body: "Is this right?".to_string(),
            ai_assist: true,
            client_id: None,
        };
        let first = handle.send_and_wait_with(ask(), WAIT)?;
        let Reply::FindingReplied { root_id, reply_id } = first else {
            anyhow::bail!("got {first:?}");
        };
        assert_eq!(reply_id, "", "no reply is written");
        let again = handle.send_and_wait_with(ask(), WAIT)?;
        assert_eq!(
            again,
            Reply::FindingReplied {
                root_id: root_id.clone(),
                reply_id: String::new(),
            },
            "the second ask reuses the thread"
        );

        let store = er_engine::store_registry::store();
        let scope = er_store::Scope::dir(bucket)?;
        let er_store::Loaded::Found(stored) =
            er_engine::ai::annotation_store::load_github_comments(&store, &scope)?
        else {
            anyhow::bail!("comments were not written");
        };
        let rows = stored.value.comments;
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0].id, root_id);
        assert_eq!(rows[0].finding_ref.as_deref(), Some("f-1"));
        assert_eq!(rows[0].comment, "AI follow-up requested for this finding.");
        Ok(())
    })
}

/// The tab's context with GitHub comments in their own bucket, as a local PR
/// tab in Branch view has them.
fn split_context(bucket: &str, comments_dir: &str) -> TabContext {
    TabContext {
        comments_dir: comments_dir.to_string(),
        ..context(bucket, "/repo")
    }
}

/// The comments bucket next to `bucket`, under the same storage root.
fn comments_bucket(bucket: &std::path::Path) -> anyhow::Result<String> {
    let dir = bucket
        .parent()
        .ok_or_else(|| anyhow::anyhow!("bucket has no parent"))?
        .join("comments");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.to_string_lossy().into_owned())
}

fn github_rows(dir: &str) -> anyhow::Result<Option<er_engine::ai::ErGithubComments>> {
    let store = er_engine::store_registry::store();
    let scope = er_store::Scope::dir(dir)?;
    Ok(
        match er_engine::ai::annotation_store::load_github_comments(&store, &scope)? {
            er_store::Loaded::Found(stored) => Some(stored.value),
            _ => None,
        },
    )
}

/// The finding's thread root lives in the comments bucket, so a second
/// AI-assisted ask on a tab whose two buckets differ reuses it.
#[test]
fn an_ai_assisted_finding_reply_reuses_the_thread_in_a_separate_comments_bucket(
) -> anyhow::Result<()> {
    in_bucket(|bucket, path| {
        write_finding(path);
        let comments = comments_bucket(path)?;
        let handle = actor();
        let ask = || Command::ReplyToFinding {
            context: split_context(bucket, &comments),
            finding_id: "f-1".to_string(),
            body: "Is this right?".to_string(),
            ai_assist: true,
            client_id: None,
        };
        let first = handle.send_and_wait_with(ask(), WAIT)?;
        let Reply::FindingReplied { root_id, .. } = first else {
            anyhow::bail!("got {first:?}");
        };
        let again = handle.send_and_wait_with(ask(), WAIT)?;
        assert_eq!(
            again,
            Reply::FindingReplied {
                root_id: root_id.clone(),
                reply_id: String::new(),
            },
            "the second ask reuses the thread"
        );
        let rows = github_rows(&comments)?.map_or_else(Vec::new, |c| c.comments);
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0].id, root_id);
        assert!(github_rows(bucket)?.is_none(), "nothing lands in er_dir");
        Ok(())
    })
}

/// A plain finding reply on such a tab also replies under the one root.
#[test]
fn a_finding_reply_reuses_the_thread_in_a_separate_comments_bucket() -> anyhow::Result<()> {
    in_bucket(|bucket, path| {
        write_finding(path);
        let comments = comments_bucket(path)?;
        let handle = actor();
        let reply = |body: &str| Command::ReplyToFinding {
            context: split_context(bucket, &comments),
            finding_id: "f-1".to_string(),
            body: body.to_string(),
            ai_assist: false,
            client_id: None,
        };
        let first = handle.send_and_wait_with(reply("First"), WAIT)?;
        let Reply::FindingReplied { root_id, .. } = first else {
            anyhow::bail!("got {first:?}");
        };
        let second = handle.send_and_wait_with(reply("Second"), WAIT)?;
        let Reply::FindingReplied { root_id: again, .. } = second else {
            anyhow::bail!("got {second:?}");
        };
        assert_eq!(again, root_id, "the second reply reuses the thread");
        let rows = github_rows(&comments)?.map_or_else(Vec::new, |c| c.comments);
        assert_eq!(rows.len(), 3, "one root and two replies: {rows:?}");
        let roots = rows.iter().filter(|row| row.in_reply_to.is_none()).count();
        assert_eq!(roots, 1, "{rows:?}");
        Ok(())
    })
}

/// A retry of a plain finding reply whose root landed and whose reply did
/// not carries the root's id; it writes the reply alone, under its own id.
#[test]
fn a_finding_reply_retried_after_its_root_landed_writes_one_reply() -> anyhow::Result<()> {
    in_bucket(|bucket, path| {
        write_finding(path);
        let handle = actor();
        let reply = || Command::ReplyToFinding {
            context: context(bucket, bucket),
            finding_id: "f-1".to_string(),
            body: "Looks wrong".to_string(),
            ai_assist: false,
            client_id: Some("c-retry".to_string()),
        };
        handle.send_and_wait_with(reply(), WAIT)?;
        // The state a failed reply write leaves: the root alone.
        let mut rows = github_rows(bucket)?.ok_or_else(|| anyhow::anyhow!("no rows"))?;
        rows.comments.retain(|row| row.in_reply_to.is_none());
        er_engine::test_support::write_sidecar(path, &rows);

        let again = handle.send_and_wait_with(reply(), WAIT)?;
        let Reply::FindingReplied { root_id, reply_id } = again else {
            anyhow::bail!("got {again:?}");
        };
        assert_eq!(root_id, "c-retry");
        assert_ne!(reply_id, root_id);
        let rows = github_rows(bucket)?.map_or_else(Vec::new, |c| c.comments);
        let ids: Vec<&str> = rows.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(ids, ["c-retry", reply_id.as_str()], "{rows:?}");
        assert_eq!(rows[1].in_reply_to.as_deref(), Some("c-retry"));

        // A later reply under the same thread keeps the id it was painted.
        let second = Command::ReplyToFinding {
            context: context(bucket, bucket),
            finding_id: "f-1".to_string(),
            body: "And this".to_string(),
            ai_assist: false,
            client_id: Some("c-second".to_string()),
        };
        let landed = handle.send_and_wait_with(second, WAIT)?;
        let Reply::FindingReplied { reply_id, .. } = landed else {
            anyhow::bail!("got {landed:?}");
        };
        assert_eq!(reply_id, "c-second");
        Ok(())
    })
}
