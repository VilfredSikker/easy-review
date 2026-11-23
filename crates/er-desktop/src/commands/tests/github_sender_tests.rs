//! The GitHub senders on the flag path: each reads its tab from the
//! registry and its content, never the App, so each answers with the App
//! locked; the comment tick picks its PR the same way; a reply on a GitHub
//! thread follows the annotation writes' refusal rule.

use std::sync::Arc;

use er_engine::tab_identity::DiffMode;
use er_engine::tabs_record::{TabDescriptor, TabKind};
use er_engine::TabKey;
use er_protocol::{Command, ErErrorKind};
use er_service::AppCommand;

use super::annotation_write_tests::view;
use super::diff_command_tests::send;
use super::diff_resource_tests::{fake_git, raw_diff, state_over, with_storage};
use super::while_the_app_is_locked;
use crate::commands::{content_routes, AppState};
use crate::snapshot::PrInfo;

fn github() -> Arc<er_engine::github::FakeGitHub> {
    Arc::new(
        er_engine::github::FakeGitHub::default()
            .with_pr_diff(Ok(raw_diff()))
            .with_pr_commits(Ok(Vec::new()))
            .with_get_repo_info(Ok(("acme".into(), "widgets".into())))
            .with_post_general(Ok(er_engine::github::CreatedComment {
                id: 501,
                created_at: "2026-01-01T00:00:00Z".into(),
                author: "you".into(),
            })),
    )
}

/// A local PR tab on acme/widgets#7, loaded and active, with the PR cache
/// listing it.
fn with_pr_tab(test: impl FnOnce(&AppState, TabKey, &er_engine::github::FakeGitHub)) {
    with_storage(|dir| {
        let fake = github();
        let git: Arc<dyn er_git::Git> =
            Arc::new(fake_git().with_diff_against_branch(Ok(raw_diff())));
        let gh: Arc<dyn er_engine::github::GitHub> = fake.clone();
        let state = state_over(dir, er_engine::ports::Ports { git, github: gh });
        let root = dir.path().join("repo");
        std::fs::create_dir_all(&root).unwrap_or_else(|e| panic!("repo dir: {e}"));
        let tab = TabDescriptor {
            kind: TabKind::LocalPr,
            pr_owner: Some("acme".into()),
            pr_repo: Some("widgets".into()),
            pr_number: Some(7),
            pr_head_ref: Some("topic".into()),
            ..content_routes::working_tab(root.to_string_lossy().into_owned())
        };
        let opened = content_routes::open(&state, tab, false);
        assert!(opened.is_ok(), "{opened:?}");
        let key = state.service.app_registry().tabs_view().active;
        let key = key.unwrap_or_else(|| panic!("no active tab"));
        let pr: PrInfo = serde_json::from_value(serde_json::json!({
            "number": 7, "title": "", "head_ref": "topic", "state": "OPEN",
            "is_draft": false, "author": "", "head_oid": "abc"
        }))
        .unwrap_or_else(|e| panic!("{e}"));
        state
            .pr_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert("acme/widgets".into(), vec![pr]);
        test(&state, key, &fake);
    });
}

fn posts(github: &er_engine::github::FakeGitHub) -> usize {
    github
        .calls()
        .iter()
        .filter(|call| call.starts_with("post_general"))
        .count()
}

/// The old command's flag path answers while the App is locked: it reads the
/// registry's active tab and its content to pick the workspace.
#[test]
fn a_github_post_answers_with_the_app_locked() {
    with_pr_tab(|state, _, github| {
        let posted = while_the_app_is_locked(state, "the PR comment", |state| {
            crate::commands::service_post_github_pr_comment(state, "ship it")
        });
        assert!(posted.is_ok(), "{posted:?}");
        assert_eq!(posts(github), 1);
    });
}

/// Through `er_send` the post names its tab, which lands with the App
/// locked; a key the registry no longer lists is `NotFound`.
#[test]
fn a_github_post_through_er_send_names_its_tab() {
    with_pr_tab(|state, key, github| {
        let tab = er_service::wire_tab_key(key);
        let body = "ship it".to_string();
        let sent = send(
            state,
            "PR comment",
            Command::PostGithubPrComment { tab, body },
        );
        assert!(sent.is_ok(), "{sent:?}");
        assert_eq!(posts(github), 1);
        let closed = er_protocol::TabKey(tab.0 + 100);
        let body = "again".to_string();
        let gone = send(
            state,
            "PR comment",
            Command::PostGithubPrComment { tab: closed, body },
        );
        assert_eq!(gone.map_err(|e| e.kind).err(), Some(ErErrorKind::NotFound));
        assert_eq!(posts(github), 1);
    });
}

/// A reply on a GitHub thread painted in another mode of the same tab is
/// refused as an annotation write is, and one naming a key the registry no
/// longer lists is `NotFound`; neither reaches the workspace.
#[test]
fn a_github_reply_painted_elsewhere_is_refused() {
    with_pr_tab(|state, key, _| {
        let painted = view(state, key);
        let moved =
            content_routes::set_tab_mode(&state.desktop, key, "branch", DiffMode::Branch, None);
        assert!(moved.is_ok(), "{moved:?}");
        let reply = |tab| Command::ReplyToGithubThread {
            tab,
            view: painted.clone(),
            parent_id: "c-1".into(),
            body: "thanks".into(),
            client_id: Some("c-2".into()),
        };
        let tab = er_service::wire_tab_key(key);
        let refused = send(state, "reply", reply(tab)).err();
        assert_eq!(
            refused.map(|e| (e.kind, e.message)),
            Some((ErErrorKind::Refused, content_routes::MOVED.to_string()))
        );
        let other = state.service.send_app(AppCommand::NewTab {
            tab: content_routes::working_tab("/elsewhere".into()),
        });
        assert!(other.is_ok(), "{other:?}");
        let closed = er_protocol::TabKey(tab.0 + 100);
        let gone = send(state, "reply", reply(closed)).err();
        assert_eq!(gone.map(|e| e.kind), Some(ErErrorKind::NotFound));
    });
}

/// A PR action built for another PR than the one the tab's card shows is
/// refused with the App locked, before any `gh` call.
#[test]
fn a_pr_action_for_another_pr_is_refused_with_the_app_locked() {
    with_pr_tab(|state, key, _| {
        let tab = er_service::wire_tab_key(key);
        let pr = er_protocol::PrTarget {
            owner: "acme".into(),
            repo: "widgets".into(),
            number: 8,
        };
        let action = er_protocol::PrAction::Close;
        let sent = send(
            state,
            "close",
            Command::RunGithubPrAction { tab, pr, action },
        );
        let refused = sent.err().map(|e| (e.kind, e.message));
        let (kind, message) = refused.unwrap_or_else(|| panic!("the close ran"));
        assert_eq!(kind, ErErrorKind::Refused);
        assert!(message.contains("now shows acme/widgets#7"), "{message}");
    });
}

/// With the App locked, one comment tick picks the active tab's PR from the
/// registry, its content and the PR cache, and sends the sync to that tab's
/// workspace.
#[test]
fn the_comment_tick_syncs_the_active_pr_with_the_app_locked() {
    with_pr_tab(|state, _, github| {
        let before = github.calls().len();
        let ticked = while_the_app_is_locked(state, "the comment tick", |state| {
            crate::comment_tick::tick(state, &mut Default::default())
        });
        let (pr, _) = ticked.unwrap_or_else(|| panic!("the tick skipped"));
        assert_eq!(pr, ("acme".into(), "widgets".into(), 7));
        assert!(
            github.calls().len() > before,
            "the sync never reached the workspace's GitHub"
        );
    });
}
