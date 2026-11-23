//! The annotation writes on `er_send`: each names the tab the page painted
//! and the view `annotations(tab)` named, and lands on that tab whichever
//! one is active, with the App locked. A key the registry no longer lists is
//! `NotFound`; a tab that shows another view since refuses the write.

use std::path::Path;

use er_engine::tab_identity::DiffMode;
use er_engine::tabs_record::TabDescriptor;
use er_engine::TabKey;
use er_protocol::{
    Annotations, Command, ErErrorKind, LineTarget, PaintedView, Resource, ResourceKey,
};
use er_service::AppCommand;

use super::diff_command_tests::{send, with_tab};
use super::diff_resource_tests::{fake_ports, state_over, with_storage};
use super::watcher_tests::{canonical, git};
use crate::commands::{content_routes, DesktopState};
use crate::service::actor_key;
use crate::snapshot::PendingAiReply;

pub(super) fn annotations(state: &DesktopState, key: TabKey) -> (u64, Annotations) {
    let tab = er_service::wire_tab_key(key);
    let fetched = crate::resources::fetch(state, ResourceKey::Annotations { tab })
        .unwrap_or_else(|e| panic!("{e:?}"));
    match fetched.value {
        Resource::Annotations(annotations) => (fetched.version.0, *annotations),
        other => panic!("not the annotations: {other:?}"),
    }
}

pub(super) fn view(state: &DesktopState, key: TabKey) -> PaintedView {
    annotations(state, key).1.view
}

fn thread_ids(state: &DesktopState, key: TabKey) -> Vec<String> {
    let threads = annotations(state, key).1.ai.threads;
    threads.into_iter().map(|thread| thread.id).collect()
}

fn question(key: TabKey, view: PaintedView, client_id: &str) -> Command {
    Command::AddQuestion {
        tab: er_service::wire_tab_key(key),
        view,
        target: LineTarget {
            file: "src/f0.rs".into(),
            hunk_idx: 0,
            line_num: Some(1),
            line_num_end: None,
        },
        body: "Why this line?".into(),
        side: None,
        client_id: Some(client_id.into()),
    }
}

/// Opens a Working tab on a directory of its own and answers its key; the
/// tab it opens is the active one.
fn open(state: &DesktopState, dir: &Path, name: &str) -> TabKey {
    let root = dir.join(name);
    std::fs::create_dir_all(&root).unwrap_or_else(|e| panic!("{name}: {e}"));
    let tab = content_routes::working_tab(root.to_string_lossy().into_owned());
    let opened = content_routes::open(state, tab, false);
    assert!(opened.is_ok(), "{opened:?}");
    let active = state.service.app_registry().tabs_view().active;
    active.unwrap_or_else(|| panic!("no active tab"))
}

/// The confirmed row is the one the page painted: the question takes the
/// `client_id`, the `Ack` names it as created, and `annotations(tab)` holds
/// it at the version the `Ack` names.
#[test]
fn a_question_is_confirmed_on_the_row_the_page_painted() {
    with_tab(|state, (key, _)| {
        let (before, _) = annotations(state, key);

        let ack = send(
            state,
            "add question",
            question(key, view(state, key), "q-1-1"),
        )
        .unwrap_or_else(|e| panic!("{e:?}"));

        let (version, _) = annotations(state, key);
        assert!(version > before, "annotations(tab) stayed at {before}");
        assert_eq!(ack.created.as_deref(), Some("q-1-1"));
        let named: Vec<_> = ack
            .changed
            .iter()
            .map(|c| (c.key.clone(), c.version.0))
            .collect();
        let tab = er_service::wire_tab_key(key);
        assert_eq!(named, [(ResourceKey::Annotations { tab }, version)]);
        assert_eq!(thread_ids(state, key), ["q-1-1"]);
    });
}

/// The registry moved to B while the page still shows A: a write keyed to A
/// with A's view lands on A, and B gets nothing.
#[test]
fn a_write_lands_on_the_tab_it_names_while_another_is_active() {
    with_storage(|dir| {
        let state = state_over(dir, fake_ports());
        let painted = open(&state, dir.path(), "a");
        let painted_view = view(&state, painted);
        let active = open(&state, dir.path(), "b");
        assert_ne!(painted, active);

        let written = send(
            &state,
            "add question",
            question(painted, painted_view, "q-1-2"),
        );

        assert!(written.is_ok(), "{written:?}");
        assert_eq!(thread_ids(&state, painted), ["q-1-2"]);
        assert!(
            thread_ids(&state, active).is_empty(),
            "the write landed on the active tab"
        );
    });
}

/// A write keyed to a tab the registry no longer lists answers `NotFound`,
/// and the tab that is open gets nothing.
#[test]
fn a_write_for_a_closed_tab_is_not_found() {
    with_storage(|dir| {
        let state = state_over(dir, fake_ports());
        let closed = open(&state, dir.path(), "a");
        let closed_view = view(&state, closed);
        let open_key = open(&state, dir.path(), "b");
        let gone = content_routes::close_key(&state, closed);
        assert!(gone.is_ok(), "{gone:?}");

        let refused = send(
            &state,
            "add question",
            question(closed, closed_view, "q-1-3"),
        );

        let kind = refused.as_ref().err().map(|e| e.kind);
        assert_eq!(kind, Some(ErErrorKind::NotFound), "{refused:?}");
        assert!(
            thread_ids(&state, open_key).is_empty(),
            "the write landed on the open tab"
        );
    });
}

/// A question painted while the tab showed Branch is refused once it shows
/// Staged, and nothing is written; back on Branch the same write lands.
#[test]
fn a_write_painted_in_another_mode_of_the_tab_is_refused() {
    with_tab(|state, (key, _)| {
        let painted = view(state, key);
        assert_eq!(painted.mode, "branch");
        let switch = |mode: &str| {
            let command = Command::SetDiffMode {
                tab: er_service::wire_tab_key(key),
                mode: mode.into(),
                pr_number: None,
            };
            let switched = send(state, "set_diff_mode", command);
            assert!(switched.is_ok(), "{switched:?}");
        };
        switch("staged");

        let refused = send(
            state,
            "stale question",
            question(key, painted.clone(), "q-1-4"),
        );

        let kind = refused.as_ref().err().map(|e| e.kind);
        assert_eq!(kind, Some(ErErrorKind::Refused), "{refused:?}");
        switch("branch");
        assert!(
            thread_ids(state, key).is_empty(),
            "the stale question landed"
        );
        let written = send(state, "fresh question", question(key, painted, "q-1-4"));
        assert!(written.is_ok(), "{written:?}");
        assert_eq!(thread_ids(state, key), ["q-1-4"]);
    });
}

/// A thread with an ask-AI reply still being written: deleting it with the
/// flag on drops the reply from `jobs` and names `jobs` in the `Ack`.
#[test]
fn deleting_a_thread_drops_its_pending_ai_reply_from_jobs() {
    with_tab(|state, (key, _)| {
        let painted = view(state, key);
        let added = send(
            state,
            "add question",
            question(key, painted.clone(), "q-1-5"),
        );
        assert!(added.is_ok(), "{added:?}");
        let reply = PendingAiReply {
            started_at_ms: 1,
            ai_action: Some(er_engine::ai::AI_ACTION_ASK),
        };
        state.pending_ai_replies.insert("q-1-5".into(), reply);
        let jobs = || match crate::resources::fetch(state, ResourceKey::Jobs) {
            Ok(fetched) => match fetched.value {
                Resource::Jobs(jobs) => (fetched.version.0, jobs.ai_replies),
                other => panic!("{other:?}"),
            },
            Err(e) => panic!("{e:?}"),
        };
        let (before, listed) = jobs();
        assert_eq!(listed.len(), 1);
        let delete = Command::DeleteThread {
            tab: er_service::wire_tab_key(key),
            view: painted,
            id: "q-1-5".into(),
        };

        let ack = send(state, "delete thread", delete).unwrap_or_else(|e| panic!("{e:?}"));

        let (after, listed) = jobs();
        assert!(listed.is_empty(), "the thinking reply stayed: {listed:?}");
        assert!(after > before, "jobs stayed at {before}");
        assert!(
            ack.changed
                .iter()
                .any(|c| c.key == ResourceKey::Jobs && c.version.0 == after),
            "{ack:?}"
        );
        assert!(thread_ids(state, key).is_empty(), "the thread stayed");
    });
}

/// A repo whose README has an unstaged edit, open in its Unstaged view.
fn readme_tab(state: &DesktopState, root: &Path) -> (TabKey, TabDescriptor) {
    std::fs::create_dir_all(root).unwrap_or_else(|e| panic!("repo dir: {e}"));
    git(root, &["init", "-q", "-b", "main"]);
    std::fs::write(root.join("README.md"), "# one\n").unwrap_or_else(|e| panic!("{e}"));
    git(root, &["add", "README.md"]);
    git(root, &["commit", "-q", "-m", "a"]);
    std::fs::write(root.join("README.md"), "# two\n").unwrap_or_else(|e| panic!("{e}"));
    let tab = content_routes::working_tab(canonical(root).to_string_lossy().into_owned());
    let added = state
        .service
        .send_app(AppCommand::NewTab { tab: tab.clone() });
    assert!(added.is_ok(), "{added:?}");
    let key = state.service.app_registry().tabs_view().active;
    let key = key.unwrap_or_else(|| panic!("no tab"));
    let load = er_service::Command::LoadTab {
        key,
        tab: tab.clone(),
        mode: Some(DiffMode::Unstaged),
    };
    assert!(state.service.send(&actor_key(&tab), load).is_ok());
    (key, tab)
}

/// README's preview key in the tab's content as it is now.
fn readme_key(state: &DesktopState, key: TabKey) -> String {
    let tab = er_service::wire_tab_key(key);
    let context = match crate::resources::fetch(state, ResourceKey::Diff { tab }) {
        Ok(fetched) => match fetched.value {
            Resource::Diff(diff) => diff.preview_context_key,
            other => panic!("not a diff: {other:?}"),
        },
        Err(e) => panic!("{e:?}"),
    };
    let path = "README.md".to_string();
    let preview = ResourceKey::FilePreview { tab, path, context };
    match crate::resources::fetch(state, preview).map(|f| f.value) {
        Ok(Resource::FilePreview(preview)) => preview.preview_key,
        other => panic!("not a preview: {other:?}"),
    }
}

fn readme_note(key: TabKey, preview_key: String) -> Command {
    Command::AddDocumentThread {
        tab: er_service::wire_tab_key(key),
        path: "README.md".into(),
        thread_kind: "note".into(),
        line: 1,
        line_end: None,
        body: "Why this title?".into(),
        preview_key,
    }
}

/// A document thread is checked against the preview of the tab it names, in
/// that tab's content: a key picked in text that has since changed is
/// refused, and the current key is written to that tab while another is
/// active, with the App locked.
#[test]
fn a_document_thread_is_checked_against_its_own_tabs_preview() {
    with_storage(|dir| {
        let state = state_over(dir, er_engine::ports::Ports::cli());
        let root = dir.path().join("repo");
        let (key, tab) = readme_tab(&state, &root);
        let picked = readme_key(&state, key);
        let other_root = dir.path().join("other");
        std::fs::create_dir_all(&other_root).unwrap_or_else(|e| panic!("other dir: {e}"));
        git(&other_root, &["init", "-q", "-b", "main"]);
        git(&other_root, &["commit", "-q", "--allow-empty", "-m", "a"]);
        let other = open(&state, &canonical(dir.path()), "other");
        assert_ne!(key, other);
        std::fs::write(root.join("README.md"), "# three\n").unwrap_or_else(|e| panic!("{e}"));
        assert!(content_routes::refresh_tab(&state, key, false).is_ok());
        let notes = || {
            let content = state.service.content(key, &tab);
            let notes = content.and_then(|c| c.ai.state.notes.as_ref().map(|n| n.notes.len()));
            notes.unwrap_or(0)
        };

        let refused = send(&state, "stale document note", readme_note(key, picked));

        let kind = refused.as_ref().err().map(|e| e.kind);
        assert_eq!(kind, Some(ErErrorKind::Refused), "{refused:?}");
        assert_eq!(notes(), 0, "a note picked in other text was written");
        let current = readme_key(&state, key);
        let written = send(&state, "document note", readme_note(key, current));
        assert!(written.is_ok(), "{written:?}");
        assert_eq!(notes(), 1, "the note was not written to its tab");
    });
}

/// A9's add question on the flag path, beside the flag-off baseline in
/// `latency_tests.rs`: `er_send` `add_question` to its `Ack`, then
/// `er_fetch` of what it names, serialised, over a 5,000-line diff.
#[test]
#[ignore = "latency harness: run with --ignored --nocapture and paste the numbers in the PR; never a CI assertion"]
fn the_add_question_reports_its_flag_on_median() {
    use std::fmt::Write as _;
    use std::sync::Arc;
    use std::time::Instant;

    let mut raw = String::new();
    for file in 0..250 {
        let _ = write!(raw, "diff --git a/src/f{file}.rs b/src/f{file}.rs\n--- a/src/f{file}.rs\n+++ b/src/f{file}.rs\n@@ -1,10 +1,10 @@\n");
        for line in 0..10 {
            let _ = write!(raw, "-old {file} {line}\n+new {file} {line}\n");
        }
    }
    with_storage(|dir| {
        let git: Arc<dyn er_git::Git> =
            Arc::new(super::diff_resource_tests::fake_git().with_diff_raw(Ok(raw)));
        let github: Arc<dyn er_engine::github::GitHub> =
            Arc::new(er_engine::github::FakeGitHub::default());
        let state = state_over(dir, er_engine::ports::Ports { git, github });
        let key = open(&state, dir.path(), "repo");
        let painted = view(&state, key);
        let effects = crate::resources::send::Effects {
            notify: &|_, _| {},
            check_update: || panic!("no update check in this test"),
            webviews: &crate::browser_webview::NoWebviews,
            pick_folder: &|| None,
        };
        let mut samples = Vec::new();
        let mut bytes = 0;
        for run in 0..50 {
            let started = Instant::now();
            let command = question(key, painted.clone(), &format!("q-latency-{run}"));
            let applied = crate::resources::send::apply(&state, command, &effects);
            let ack = applied.unwrap_or_else(|e| panic!("{e:?}")).ack;
            let keys = ack.changed.into_iter().map(|c| c.key).collect();
            let fetched =
                crate::resources::fetch_all(&state, keys).unwrap_or_else(|e| panic!("{e:?}"));
            bytes = serde_json::to_vec(&fetched).map(|v| v.len()).unwrap_or(0);
            if run >= 5 {
                samples.push(started.elapsed());
            }
        }
        samples.sort_unstable();
        let median = samples[samples.len() / 2];
        println!("add question (flag on): median {median:?}, {bytes} bytes answered");
    });
}
