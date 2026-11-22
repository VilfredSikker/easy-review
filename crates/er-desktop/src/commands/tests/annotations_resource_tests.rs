//! The tab's view-bucket reads and `jobs` on the flag path. A local PR tab
//! in its branch view reads two buckets, the branch's and the PR's, so each
//! test there first checks that the two differ.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use er_engine::tab_identity::DiffMode;
use er_engine::tabs_record::{TabDescriptor, TabKind};
use er_engine::TabKey;
use er_protocol::{Resource, ResourceKey};
use er_service::TabContent;

use super::diff_resource_tests::{fake_git, raw_diff, state_over, with_storage};
use super::ReviewFlowTestDir;
use crate::commands::{content_routes, AppState};
use crate::resources::{self, relay, tick, Watched};
use crate::snapshot::PendingAiReply;

const WAIT: Duration = Duration::from_secs(10);

fn ports() -> er_engine::ports::Ports {
    let git: Arc<dyn er_git::Git> = Arc::new(fake_git().with_diff_against_branch(Ok(raw_diff())));
    let github: Arc<dyn er_engine::github::GitHub> = Arc::new(
        er_engine::github::FakeGitHub::default()
            .with_pr_diff(Ok(raw_diff()))
            .with_pr_commits(Ok(Vec::new())),
    );
    er_engine::ports::Ports { git, github }
}

fn wait_until(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !ready() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn content(state: &AppState, (key, tab): &(TabKey, TabDescriptor)) -> Arc<TabContent> {
    state
        .service
        .content(*key, tab)
        .unwrap_or_else(|| panic!("no content under {key:?}"))
}

/// A local PR tab shown in its branch view and loaded. Its view bucket is
/// the branch's and its GitHub-comment bucket the PR's.
fn branch_view_of_a_pr(dir: &ReviewFlowTestDir) -> (AppState, (TabKey, TabDescriptor)) {
    let state = state_over(dir, ports());
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
    let opened = content_routes::open(&state, tab.clone(), false);
    assert!(opened.is_ok(), "{opened:?}");
    let key = state.service.app_registry().tabs_view().active;
    let key = key.unwrap_or_else(|| panic!("no active tab"));
    let branch =
        content_routes::set_tab_mode(&state.desktop, key, "branch", DiffMode::Branch, None);
    assert!(branch.is_ok(), "{branch:?}");
    let entry = (key, tab);
    wait_until("the branch view to load", || {
        state
            .service
            .content(entry.0, &entry.1)
            .is_some_and(|c| c.tab.mode == DiffMode::Branch)
    });
    let loaded = content(&state, &entry);
    assert_ne!(
        loaded.context.er_dir, loaded.context.comments_dir,
        "the fixture's two buckets are one"
    );
    (state, entry)
}

fn stamp_ahead(path: &Path) {
    let ahead = std::time::SystemTime::now() + Duration::from_secs(5);
    let file = std::fs::OpenOptions::new().write(true).open(path);
    let stamped = file.and_then(|file| file.set_modified(ahead));
    assert!(stamped.is_ok(), "stamp {}: {stamped:?}", path.display());
}

/// A comment an outside writer leaves in the PR bucket reaches the branch
/// view's AI section through the backend tick, with no frontend call.
#[test]
fn an_outside_comment_in_the_pr_bucket_reaches_a_branch_view_through_the_tick() {
    with_storage(|dir| {
        let (state, tab) = branch_view_of_a_pr(dir);
        let loaded = content(&state, &tab);
        let pr_bucket = Path::new(&loaded.context.comments_dir);
        er_engine::test_support::write_sidecar_json::<er_engine::ai::ErGithubComments>(
            pr_bucket,
            serde_json::json!({ "version": 1, "diff_hash": "", "comments": [{
                "id": "c-outside", "file": "src/f0.rs", "hunk_index": 0, "line_start": 1,
                "line_content": "new", "comment": "From outside", "source": "github"
            }] }),
        );
        stamp_ahead(&pr_bucket.join("github-comments.json"));

        tick::run(&state.desktop, &state.app);

        wait_until("the PR bucket's comment", || {
            let now = content(&state, &tab);
            now.ai.revision > loaded.ai.revision
                && now
                    .ai
                    .state
                    .github_comments
                    .as_ref()
                    .is_some_and(|gc| gc.comments.iter().any(|c| c.id == "c-outside"))
        });
    });
}

fn ui_annotation(id: &str) -> er_engine::ai::UiAnnotation {
    serde_json::from_value(serde_json::json!({
        "id": id, "url": "/", "box_x": 0.0, "box_y": 0.0, "box_w": 1.0, "box_h": 1.0,
        "viewport_w": 10, "viewport_h": 10, "text": "here",
        "dom_context": { "tag": "button" }
    }))
    .unwrap_or_else(|e| panic!("annotation: {e}"))
}

/// The browser annotations and the review reads answer the tab's view
/// bucket, never its GitHub-comment bucket.
#[test]
fn the_view_bucket_reads_answer_the_branch_view_and_never_the_pr_bucket() {
    with_storage(|dir| {
        let (state, tab) = branch_view_of_a_pr(dir);
        let loaded = content(&state, &tab);
        let context = &loaded.context;
        let saved = [
            er_engine::ai::save_ui_annotations(&context.er_dir, &[ui_annotation("ui-view")]),
            er_engine::ai::save_ui_annotations(&context.comments_dir, &[ui_annotation("ui-pr")]),
        ];
        assert!(saved.iter().all(Result::is_ok), "{saved:?}");
        let wire = er_service::wire_tab_key(tab.0);

        let fetched = resources::fetch(&state.desktop, ResourceKey::UiAnnotations { tab: wire });
        let rows = match fetched.map(|v| v.value) {
            Ok(Resource::UiAnnotations(rows)) => rows.rows,
            other => panic!("{other:?}"),
        };
        let ids: Vec<&str> = rows.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(ids, ["ui-view"]);
        assert_eq!(
            rows[0].dom_context_json.as_deref(),
            Some(r#"{"tag":"button"}"#)
        );
        let revisions =
            resources::fetch(&state.desktop, ResourceKey::ReviewRevisions { tab: wire });
        let revisions = match revisions.map(|v| v.value) {
            Ok(Resource::ReviewRevisions(revisions)) => revisions.revisions,
            other => panic!("{other:?}"),
        };
        assert!(revisions.is_empty(), "a bucket with no review listed one");
    });
}

/// Every write to the pending replies moves `jobs` and only `jobs`, and
/// the resource lists what is in flight.
#[test]
fn every_pending_reply_write_moves_jobs_and_nothing_else() {
    with_storage(|dir| {
        let state = state_over(dir, ports());
        let desktop = &state.desktop;
        let jobs = || match resources::fetch(desktop, ResourceKey::Jobs).map(|v| v.value) {
            Ok(Resource::Jobs(jobs)) => jobs.ai_replies,
            other => panic!("{other:?}"),
        };
        let mut last = resources::watched(desktop);
        let mut moved = || -> Vec<Watched> {
            let moved = relay::moved(&mut last, resources::watched(desktop));
            moved.into_iter().map(|(watched, _)| watched).collect()
        };
        let reply = PendingAiReply {
            started_at_ms: 1,
            ai_action: Some(er_engine::ai::AI_ACTION_ASK),
        };

        desktop.pending_ai_replies.insert("q-1".into(), reply);
        assert_eq!(moved(), [Watched::Jobs], "an insert");
        let listed: Vec<(String, Option<String>)> = jobs()
            .into_iter()
            .map(|job| (job.target, job.ai_action))
            .collect();
        assert_eq!(listed, [("q-1".to_string(), Some("ask".to_string()))]);

        desktop.pending_ai_replies.remove(["q-1"]);
        assert_eq!(moved(), [Watched::Jobs], "a removal");
        assert!(jobs().is_empty(), "the answered reply is still in flight");
        desktop.pending_ai_replies.remove(["q-1"]);
        assert!(moved().is_empty(), "removing nothing moved a version");
    });
}
