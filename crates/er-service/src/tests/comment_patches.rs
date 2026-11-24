//! A comment post or a promote patches the AI section of the loaded tab on
//! its bucket, so the published content follows the write without a reload.

use std::sync::Arc;

use er_engine::ports::Ports;
use er_engine::tabs_record::{TabDescriptor, TabKind};
use er_engine::TabKey;
use er_git::FakeGit;
use er_github::{CreatedComment, FakeGitHub};

use super::github_sync::{RAW, WAIT};
use crate::actor::WorkspaceDeps;
use crate::protocol::{AnnotationTarget, Command, TabContext};
use crate::{spawn, TabContent, WorkspaceHandle};

fn created(id: u64) -> CreatedComment {
    CreatedComment {
        id,
        created_at: "2026-01-01T00:00:00Z".to_string(),
        author: "you".to_string(),
    }
}

fn line_two() -> AnnotationTarget {
    AnnotationTarget {
        file: "src/b.rs".to_string(),
        hunk_idx: 0,
        line_num: Some(2),
        line_num_end: None,
    }
}

/// Runs `test` on an actor over `github` with one plain tab loaded on a repo
/// under a temp storage root.
fn with_loaded_tab(
    github: FakeGitHub,
    test: impl FnOnce(&WorkspaceHandle, TabKey) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(&repo)?;
    let storage = dir.path().join("storage");
    er_engine::storage::with_process_test_storage_root(storage, || -> anyhow::Result<()> {
        let git: Arc<dyn er_git::Git> = Arc::new(
            FakeGit::default()
                .with_is_merge_in_progress(false)
                .with_current_branch(Ok("feature".to_string()))
                .with_diff_shortstat((0, 0, 0))
                .with_diff_raw(Ok(RAW.to_string())),
        );
        let github: Arc<dyn er_github::GitHub> = Arc::new(github);
        let handle = spawn(WorkspaceDeps::new(Ports { git, github }));
        let key = TabKey::next();
        let tab = TabDescriptor {
            kind: TabKind::Working,
            repo_root: repo.to_string_lossy().into_owned(),
            branch: None,
            pr_owner: None,
            pr_repo: None,
            pr_number: None,
            pr_head_ref: None,
            base_ref: Some("main".to_string()),
            extra: serde_json::Map::new(),
        };
        handle.send_and_wait_with(
            Command::LoadTab {
                key,
                tab,
                mode: None,
            },
            WAIT,
        )?;
        test(&handle, key)
    })
}

fn content(handle: &WorkspaceHandle, key: TabKey) -> anyhow::Result<Arc<TabContent>> {
    handle
        .view()
        .tab(key)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("no content under {key:?}"))
}

/// The tab's context, posting to acme/widgets#7.
fn on_pr(content: &TabContent) -> TabContext {
    TabContext {
        is_remote: true,
        remote_repo: Some("acme/widgets".to_string()),
        pr_number: Some(7),
        ..content.context.clone()
    }
}

fn comment_ids(content: &TabContent) -> Vec<String> {
    let comments = content.ai.state.github_comments.iter();
    comments
        .flat_map(|stored| stored.comments.iter().map(|c| c.id.clone()))
        .collect()
}

fn question_ids(content: &TabContent) -> Vec<String> {
    let questions = content.ai.state.questions.iter();
    questions
        .flat_map(|stored| stored.questions.iter().map(|q| q.id.clone()))
        .collect()
}

#[test]
fn a_posted_comment_and_reply_reach_the_ai_section_of_the_tab() -> anyhow::Result<()> {
    let github = FakeGitHub::default()
        .with_pr_diff_remote(Ok(RAW.to_string()))
        .with_pr_head_oid(Ok("abc123".to_string()))
        .with_post_line_comment(Ok(created(4242)))
        .with_post_reply(Ok(created(500)));
    with_loaded_tab(github, |handle, key| {
        let before = content(handle, key)?;
        let comment = Command::AddComment {
            context: on_pr(&before),
            target: line_two(),
            body: "look here".to_string(),
            side: Some("RIGHT".to_string()),
            client_id: Some("c-patch-1".to_string()),
        };
        handle.send_and_wait_with(comment, WAIT)?;

        let commented = content(handle, key)?;
        assert!(
            commented.ai.revision > before.ai.revision,
            "the AI section kept"
        );
        let ids = comment_ids(&commented);
        assert_eq!(ids, ["gh-4242"], "the comment did not reach the tab");
        assert!(
            Arc::ptr_eq(&before.diff, &commented.diff),
            "the post moved the diff"
        );

        let reply = Command::ReplyToGithubThread {
            context: on_pr(&before),
            parent_id: "gh-4242".to_string(),
            body: "agreed".to_string(),
            client_id: Some("c-patch-2".to_string()),
        };
        handle.send_and_wait_with(reply, WAIT)?;

        let replied = content(handle, key)?;
        let ids = comment_ids(&replied);
        assert!(ids.contains(&"gh-500".to_string()), "{ids:?}");
        Ok(())
    })
}

#[test]
fn a_promote_moves_the_thread_from_questions_to_comments_in_the_tab() -> anyhow::Result<()> {
    let github = FakeGitHub::default()
        .with_pr_diff_remote(Ok(RAW.to_string()))
        .with_pr_head_oid(Ok("abc123".to_string()))
        .with_post_line_comment(Ok(created(77)));
    with_loaded_tab(github, |handle, key| {
        let loaded = content(handle, key)?;
        let question = Command::AddQuestion {
            context: loaded.context.clone(),
            target: line_two(),
            body: "promote me".to_string(),
            side: None,
            client_id: Some("q-patch-1".to_string()),
            reply_to: None,
            finding_ref: None,
        };
        handle.send_and_wait_with(question, WAIT)?;
        let asked = content(handle, key)?;
        assert_eq!(question_ids(&asked), ["q-patch-1"]);

        let promote = Command::PromoteToComment {
            context: on_pr(&loaded),
            id: "q-patch-1".to_string(),
            body: None,
        };
        handle.send_and_wait_with(promote, WAIT)?;

        let promoted = content(handle, key)?;
        let questions = question_ids(&promoted);
        assert!(
            questions.is_empty(),
            "the source thread stayed: {questions:?}"
        );
        assert_eq!(comment_ids(&promoted), ["gh-77"]);
        Ok(())
    })
}

/// The promoted finding leaves the review in the tab, not only on disk.
#[test]
fn a_promoted_finding_leaves_the_review_in_the_tab() -> anyhow::Result<()> {
    let github = FakeGitHub::default()
        .with_pr_diff_remote(Ok(RAW.to_string()))
        .with_pr_head_oid(Ok("abc123".to_string()))
        .with_post_line_comment(Ok(created(88)));
    with_loaded_tab(github, |handle, key| {
        let loaded = content(handle, key)?;
        er_engine::test_support::write_sidecar_json::<er_engine::ai::ErReview>(
            std::path::Path::new(&loaded.context.er_dir),
            serde_json::json!({
                "version": 1,
                "diff_hash": loaded.context.branch_diff_hash,
                "files": { "src/b.rs": { "risk": "low", "findings": [{
                    "id": "f-1", "severity": "low", "title": "A defect",
                    "description": "desc", "hunk_index": 0, "line_start": 2
                }] } }
            }),
        );
        handle.send_and_wait_with(Command::ReloadSidecars { key }, WAIT)?;
        assert_eq!(content(handle, key)?.ai.state.total_findings(), 1);

        let promote = Command::PromoteFindingToComment {
            context: on_pr(&loaded),
            finding_id: "f-1".to_string(),
            body: None,
        };
        handle.send_and_wait_with(promote, WAIT)?;

        let promoted = content(handle, key)?;
        assert_eq!(promoted.ai.state.total_findings(), 0, "the finding stayed");
        assert_eq!(comment_ids(&promoted), ["gh-88"]);
        Ok(())
    })
}
