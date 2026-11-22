//! The tab's view-bucket reads and `jobs` on the flag path. A local PR tab
//! in its branch view reads two buckets, the branch's and the PR's, so each
//! test there first checks that the two differ.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use er_engine::tab_identity::DiffMode;
use er_engine::tabs_record::{TabDescriptor, TabKind};
use er_engine::TabKey;
use er_protocol::{Resource, ResourceKey, ReviewJson};
use er_service::TabContent;

use super::diff_resource_tests::{fake_git, listed, raw_diff, state_over, with_storage};
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

/// An arena accept writes `accepted-findings.json` and the run's `run.json`
/// and answers no patch, so the tick alone brings the accepted rows to the
/// AI section.
#[test]
fn an_arena_accept_reaches_the_ai_section_through_the_tick() {
    with_storage(|dir| {
        let (state, tab) = branch_view_of_a_pr(dir);
        let loaded = content(&state, &tab);
        let bucket = Path::new(&loaded.context.er_dir);
        er_engine::test_support::write_sidecar_json::<
            er_engine::ai::accepted_findings::AcceptedFindings,
        >(
            bucket,
            serde_json::json!({ "version": 1, "findings": [{
                "file": "src/f0.rs", "run_finding_id": "kept", "run_id": "run-1",
                "run_diff_hash": "", "accepted_at": "t0", "providers": [],
                "finding": { "id": "arena-run-1-kept", "severity": "medium", "title": "Accepted" }
            }] }),
        );
        stamp_ahead(&bucket.join("accepted-findings.json"));
        let run = bucket.join("arena").join("run-1");
        let wrote =
            std::fs::create_dir_all(&run).and_then(|()| std::fs::write(run.join("run.json"), "{}"));
        assert!(wrote.is_ok(), "run.json: {wrote:?}");

        tick::run(&state.desktop, &state.app);

        wait_until("the accept to reload the AI section", || {
            content(&state, &tab).ai.revision > loaded.ai.revision
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

/// A loaded working tab on `repo`, over `raw`.
fn working(dir: &ReviewFlowTestDir, raw: String) -> (AppState, (TabKey, TabDescriptor)) {
    let git: Arc<dyn er_git::Git> = Arc::new(fake_git().with_diff_raw(Ok(raw)));
    let github: Arc<dyn er_engine::github::GitHub> =
        Arc::new(er_engine::github::FakeGitHub::default());
    let state = state_over(dir, er_engine::ports::Ports { git, github });
    let root = dir.path().join("repo");
    std::fs::create_dir_all(&root).unwrap_or_else(|e| panic!("repo dir: {e}"));
    let tab = content_routes::working_tab(root.to_string_lossy().into_owned());
    let opened = content_routes::open(&state, tab.clone(), false);
    assert!(opened.is_ok(), "{opened:?}");
    let key = state.service.app_registry().tabs_view().active;
    let entry = (key.unwrap_or_else(|| panic!("no active tab")), tab);
    wait_until("the tab to load", || {
        state.service.content(entry.0, &entry.1).is_some()
    });
    (state, entry)
}

fn annotations_version(state: &AppState, (key, _): &(TabKey, TabDescriptor)) -> u64 {
    let tab = er_service::wire_tab_key(*key);
    match resources::version(&state.desktop, ResourceKey::Annotations { tab }) {
        Ok(version) => version.0,
        Err(e) => panic!("{e:?}"),
    }
}

/// One service question moves `annotations(tab)` once: the tick after it
/// reads the workspace's own write as nothing new.
#[test]
fn a_service_question_moves_the_annotations_once_across_the_next_tick() {
    with_storage(|dir| {
        let (state, tab) = working(dir, raw_diff());
        let before = annotations_version(&state, &tab);
        let context = content(&state, &tab).context.clone();
        let target = er_service::AnnotationTarget {
            file: "src/f0.rs".to_string(),
            hunk_idx: 0,
            line_num: Some(1),
            line_num_end: None,
        };
        let question = er_service::Command::AddQuestion {
            context,
            target,
            body: "why?".to_string(),
            side: Some("RIGHT".to_string()),
            client_id: Some("q-own".to_string()),
            reply_to: None,
            finding_ref: None,
        };
        let asked = state
            .service
            .send(&crate::service::actor_key(&tab.1), question);
        assert!(asked.is_ok(), "{asked:?}");
        let written = annotations_version(&state, &tab);
        assert!(written > before, "the question moved no version");

        tick::run(&state.desktop, &state.app);
        std::thread::sleep(Duration::from_millis(500));

        assert_eq!(
            annotations_version(&state, &tab),
            written,
            "the tick read the write back"
        );
    });
}

/// A question an outside writer leaves in the view's bucket moves
/// `annotations(tab)` through the tick, with no frontend call.
#[test]
fn an_outside_question_moves_the_annotations_through_the_tick() {
    with_storage(|dir| {
        let (state, tab) = working(dir, raw_diff());
        let before = annotations_version(&state, &tab);
        let bucket = content(&state, &tab).context.er_dir.clone();
        er_engine::test_support::write_sidecar_json::<er_engine::ai::ErQuestions>(
            Path::new(&bucket),
            serde_json::json!({ "version": 1, "diff_hash": "", "questions": [{
                "id": "q-outside", "file": "src/f0.rs", "hunk_index": 0, "line_start": 1,
                "line_content": "", "text": "Asked from outside"
            }] }),
        );
        stamp_ahead(&Path::new(&bucket).join("questions.json"));

        tick::run(&state.desktop, &state.app);

        wait_until("the outside question", || {
            annotations_version(&state, &tab) > before
        });
    });
}

/// A parse publishes the tab's diff and moves no `annotations(tab)`.
#[test]
fn a_lazy_parse_moves_no_annotations_version() {
    use std::fmt::Write as _;

    with_storage(|dir| {
        let filler = "x".repeat(120);
        let mut raw = String::new();
        for n in 0..600 {
            let _ = write!(
                raw,
                "diff --git a/src/f{n}.rs b/src/f{n}.rs\n--- a/src/f{n}.rs\n+++ b/src/f{n}.rs\n\
                 @@ -1,2 +1,2 @@\n-old {filler}\n+new {filler}\n {filler}\n"
            );
        }
        let (state, tab) = working(dir, raw);
        let loaded = content(&state, &tab);
        let stub = loaded.diff.files.iter().rev().find(|f| f.hunks.is_empty());
        let stub = stub
            .map(|f| f.path.clone())
            .unwrap_or_else(|| panic!("no stub"));
        let mut last = resources::watched(&state.desktop);

        let parse = er_service::Command::ParseFiles {
            key: tab.0,
            paths: vec![stub],
            expand: false,
        };
        let parsed = state
            .service
            .send(&crate::service::actor_key(&tab.1), parse);

        assert!(parsed.is_ok(), "{parsed:?}");
        let moved = relay::moved(&mut last, resources::watched(&state.desktop));
        let moved: Vec<Watched> = moved.into_iter().map(|(watched, _)| watched).collect();
        assert!(
            moved.contains(&Watched::Diff(tab.0)),
            "the parse published nothing: {moved:?}"
        );
        assert!(!moved.contains(&Watched::Annotations(tab.0)), "{moved:?}");
    });
}

/// A desktop annotation write that lands while `ui_annotations(tab)` reads
/// its rows leaves the answer under the older version, so the relay pushes
/// the tab again and the next fetch brings the write.
#[test]
fn a_ui_annotation_write_during_the_read_is_pushed_again() {
    with_storage(|dir| {
        let (state, tab) = branch_view_of_a_pr(dir);
        let desktop = &state.desktop;
        let wire = er_service::wire_tab_key(tab.0);

        let read = resources::annotations::ui_annotations_read(desktop, wire, |er_dir| {
            let rows = er_engine::ai::load_ui_annotations(er_dir);
            let saved = er_engine::ai::save_ui_annotations(er_dir, &[ui_annotation("ui-during")]);
            assert!(saved.is_ok(), "{saved:?}");
            desktop.ui_annotations.bump();
            rows
        });
        let (answered, _) = read.unwrap_or_else(|e| panic!("{e:?}"));
        let now = resources::annotations::ui_annotations_version(desktop, tab.0, &tab.1);
        assert!(
            answered < now,
            "the read answered version {answered} and the write left {now}"
        );

        let fetched = resources::fetch(desktop, ResourceKey::UiAnnotations { tab: wire });
        let fetched = fetched.unwrap_or_else(|e| panic!("{e:?}"));
        let ids: Vec<String> = match fetched.value {
            Resource::UiAnnotations(rows) => rows.rows.into_iter().map(|row| row.id).collect(),
            other => panic!("{other:?}"),
        };
        assert_eq!(
            (fetched.version.0, ids),
            (now, vec!["ui-during".to_string()])
        );
    });
}

/// `review_json` answers none for a listed tab nothing loaded and for a
/// bucket with no review, so a batched fetch that holds it still answers.
#[test]
fn review_json_answers_none_for_a_tab_nothing_loaded_and_a_bucket_with_no_review() {
    with_storage(|dir| {
        let none = Resource::ReviewJson(ReviewJson::default());

        let unloaded = state_over(dir, ports());
        let (key, fresh) = listed(&unloaded, dir, "fresh");
        let wire = er_service::wire_tab_key(key);
        let fetched = resources::fetch(&unloaded.desktop, ResourceKey::ReviewJson { tab: wire });
        let fetched = fetched.unwrap_or_else(|e| panic!("an unloaded tab: {e:?}"));
        assert_eq!((fetched.version.0, &fetched.value), (0, &none));
        wait_until("the load the fetch posted", || {
            unloaded.service.content(key, &fresh).is_some()
        });

        let (state, tab) = branch_view_of_a_pr(dir);
        let wire = er_service::wire_tab_key(tab.0);
        let batch = resources::fetch_all(
            &state.desktop,
            vec![
                ResourceKey::ReviewJson { tab: wire },
                ResourceKey::UiAnnotations { tab: wire },
            ],
        );
        let batch = batch.unwrap_or_else(|e| panic!("a bucket with no review: {e:?}"));
        assert_eq!(batch[0].value, none);
    });
}
