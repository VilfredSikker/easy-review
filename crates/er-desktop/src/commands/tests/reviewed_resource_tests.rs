//! `reviewed(tab)` and the keyed reviewed writes on `er_send`: a write
//! names the tab the page painted and is refused once the registry's active
//! tab is another (decision A); a bulk write also names the view it was
//! painted on and is refused once the tab shows another view.

use er_engine::TabKey;
use er_protocol::{Ack, Command, ErErrorKind, PaintedView, Resource, ResourceKey, TabReviewed};

use super::diff_command_tests::{send, with_tab};
use super::diff_resource_tests::{fake_git, fake_ports, state_over, with_storage};
use crate::commands::{content_routes, AppState};

fn reviewed(state: &AppState, key: TabKey) -> (u64, TabReviewed) {
    let key = ResourceKey::Reviewed {
        tab: er_service::wire_tab_key(key),
    };
    let fetched = crate::resources::fetch(&state.desktop, key).unwrap_or_else(|e| panic!("{e:?}"));
    match fetched.value {
        Resource::Reviewed(reviewed) => (fetched.version.0, reviewed),
        other => panic!("not the marks: {other:?}"),
    }
}

fn marks(state: &AppState, key: TabKey) -> Vec<String> {
    reviewed(state, key).1.paths
}

/// The three single-file writes on `tab`, each setting `src/f0.rs`'s mark.
fn writes(tab: TabKey) -> [(&'static str, Command); 3] {
    let tab = er_service::wire_tab_key(tab);
    let path = || "src/f0.rs".to_string();
    [
        ("toggle", Command::ToggleReviewed { tab, path: path() }),
        (
            "mark",
            Command::MarkReviewed {
                tab,
                path: path(),
                reviewed: true,
            },
        ),
        (
            "unmark",
            Command::MarkReviewed {
                tab,
                path: path(),
                reviewed: false,
            },
        ),
    ]
}

/// The registry has moved to B while the page still shows A: each reviewed
/// write the page keys A is refused, and B keeps its marks.
#[test]
fn a_reviewed_write_keyed_to_the_tab_the_registry_left_is_refused() {
    with_storage(|dir| {
        let state = state_over(dir, fake_ports());
        let open = |name: &str| {
            let root = dir.path().join(name);
            std::fs::create_dir_all(&root).unwrap_or_else(|e| panic!("{name}: {e}"));
            let tab = content_routes::working_tab(root.to_string_lossy().into_owned());
            let opened = content_routes::open(&state, tab, false);
            assert!(opened.is_ok(), "{opened:?}");
            state
                .service
                .app_registry()
                .tabs_view()
                .active
                .expect("the opened tab")
        };
        let painted = open("a");
        let active = open("b");
        for (name, command) in writes(painted) {
            // B holds the mark the write would flip, so an unmark that lands
            // shows as much as a mark.
            let marked = name == "unmark";
            let setup = &writes(active)[if marked { 1 } else { 2 }].1;
            let set = send(&state, "setup", setup.clone());
            assert!(set.is_ok(), "{name}: {set:?}");
            let before = marks(&state, active);

            let refused = send(&state, name, command);

            let kind = refused.as_ref().err().map(|e| e.kind);
            assert_eq!(kind, Some(ErErrorKind::Refused), "{name}: {refused:?}");
            assert_eq!(
                marks(&state, active),
                before,
                "{name} landed on the active tab"
            );
            assert!(
                marks(&state, painted).is_empty(),
                "{name} landed on the painted tab"
            );
        }
        let (name, command) = writes(active)[1].clone();
        let written = send(&state, name, command);
        assert!(written.is_ok(), "the active tab's own write: {written:?}");
    });
}

/// A write keyed to the active tab lands, and its `Ack` names the marks
/// and the diff at the versions a fetch then answers.
#[test]
fn a_reviewed_write_moves_the_marks_and_the_counts() {
    with_tab(|state, (key, _)| {
        let (before, empty) = reviewed(state, key);
        assert_eq!((empty.reviewed_count, empty.total_count), (0, 3));
        let [(_, toggle), ..] = writes(key);

        let ack = send(state, "toggle", toggle).unwrap_or_else(|e| panic!("{e:?}"));

        let (version, marked) = reviewed(state, key);
        assert!(version > before, "the marks stayed at {before}");
        assert_eq!(marked.paths, ["src/f0.rs"]);
        assert_eq!((marked.reviewed_count, marked.total_count), (1, 3));
        let wire = er_service::wire_tab_key(key);
        let (diff_version, _) = super::diff_command_tests::diff(state, key);
        assert_eq!(
            named(&ack),
            [
                (ResourceKey::Reviewed { tab: wire }, version),
                (ResourceKey::Diff { tab: wire }, diff_version)
            ]
        );
    });
}

fn named(ack: &Ack) -> Vec<(ResourceKey, u64)> {
    ack.changed
        .iter()
        .map(|changed| (changed.key.clone(), changed.version.0))
        .collect()
}

/// A tour whose one pillar holds `src/f0.rs`, in the tab's review bucket.
fn seed_tour(state: &AppState, key: TabKey, tab: &er_engine::tabs_record::TabDescriptor) {
    let content = state.service.content(key, tab).expect("the loaded tab");
    er_engine::test_support::write_sidecar_json::<er_engine::ai::ErTour>(
        std::path::Path::new(&content.context.er_dir),
        serde_json::json!({
            "version": 1,
            "diff_hash": "",
            "pillars": [{"id": "p-1", "title": "One", "files": [{"path": "src/f0.rs"}]}]
        }),
    );
}

fn bulk(key: TabKey, view: PaintedView) -> Command {
    Command::BulkReviewPillar {
        tab: er_service::wire_tab_key(key),
        view,
        pillar_id: "p-1".into(),
        reviewed: true,
    }
}

/// A bulk review painted while the tab showed Branch is refused once it
/// shows Staged, and nothing is marked; back on Branch, the view
/// `reviewed(tab)` names is the painted one again, and the same write lands.
#[test]
fn a_bulk_review_painted_in_another_mode_of_the_tab_is_refused() {
    with_tab(|state, (key, tab)| {
        seed_tour(state, key, &tab);
        let painted = reviewed(state, key).1.view;
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

        let refused = send(state, "stale bulk", bulk(key, painted.clone()));

        let kind = refused.as_ref().err().map(|e| e.kind);
        assert_eq!(kind, Some(ErErrorKind::Refused), "{refused:?}");
        assert!(marks(state, key).is_empty(), "the stale bulk review landed");
        switch("branch");
        assert_eq!(reviewed(state, key).1.view, painted);
        let written = send(state, "fresh bulk", bulk(key, painted));
        assert!(written.is_ok(), "{written:?}");
        assert_eq!(marks(state, key), ["src/f0.rs"]);
    });
}

/// The view `reviewed(tab)` names is the one the comparison accepts, on
/// every kind of tab: a local PR saved without a branch shows `pr/N`, and
/// a remote PR's content roots where its tab row does not.
#[test]
fn a_bulk_review_on_the_view_the_marks_name_is_accepted_on_every_kind() {
    super::tab_key_tests::with_storage(|dir| {
        for (kind, tab) in super::tab_key_tests::kinds(dir) {
            let state = super::tab_key_tests::state(dir);
            let opened = content_routes::open(&state, tab, false);
            assert!(opened.is_ok(), "{kind}: {opened:?}");
            let key = state
                .service
                .app_registry()
                .tabs_view()
                .active
                .expect("the tab");
            let view = reviewed(&state, key).1.view;

            let written = send(&state, "bulk", bulk(key, view));

            assert!(written.is_ok(), "{kind}: {written:?}");
        }
    });
}

/// A9's mark reviewed on the flag path, beside the flag-off baseline in
/// `latency_tests.rs`: `er_send` `mark_reviewed` to its `Ack`, then
/// `er_fetch` of what it names, serialised, over a 5,000-line diff.
/// Alternate runs mark and clear, so each is a write.
#[test]
#[ignore = "latency harness: run with --ignored --nocapture and paste the numbers in the PR; never a CI assertion"]
fn the_mark_reviewed_reports_its_flag_on_median() {
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
        let git: Arc<dyn er_git::Git> = Arc::new(fake_git().with_diff_raw(Ok(raw)));
        let github: Arc<dyn er_engine::github::GitHub> =
            Arc::new(er_engine::github::FakeGitHub::default());
        let state = state_over(dir, er_engine::ports::Ports { git, github });
        let root = dir.path().join("repo");
        std::fs::create_dir_all(&root).unwrap_or_else(|e| panic!("repo: {e}"));
        let tab = content_routes::working_tab(root.to_string_lossy().into_owned());
        let opened = content_routes::open(&state, tab, false);
        assert!(opened.is_ok(), "{opened:?}");
        let key = state
            .service
            .app_registry()
            .tabs_view()
            .active
            .expect("the tab");
        let effects = crate::resources::send::Effects {
            notify: &|_, _| {},
            follow_selection: &|_| {},
            check_update: || panic!("no update check in this test"),
            webviews: &crate::browser_webview::NoWebviews,
            pick_folder: &|| None,
        };
        // All the `Ack` names, then only `reviewed(tab)`, which the checkmark reads.
        for (name, all) in [("ack keys", true), ("reviewed(tab) only", false)] {
            let mut samples = Vec::new();
            let mut bytes = 0;
            for run in 0..50 {
                let started = Instant::now();
                let tab = er_service::wire_tab_key(key);
                let path = "src/f0.rs".to_string();
                let command = Command::MarkReviewed {
                    tab,
                    path,
                    reviewed: run % 2 == 0,
                };
                let applied = crate::resources::send::apply(&state.desktop, command, &effects);
                let ack = applied.unwrap_or_else(|e| panic!("{e:?}")).ack;
                let keys = ack.changed.into_iter().map(|c| c.key);
                let keys = keys.filter(|k| all || matches!(k, ResourceKey::Reviewed { .. }));
                let fetched = crate::resources::fetch_all(&state.desktop, keys.collect())
                    .unwrap_or_else(|e| panic!("{e:?}"));
                bytes = serde_json::to_vec(&fetched).map(|v| v.len()).unwrap_or(0);
                if run >= 5 {
                    samples.push(started.elapsed());
                }
            }
            samples.sort_unstable();
            let median = samples[samples.len() / 2];
            println!("mark reviewed (flag on, {name}): median {median:?}, {bytes} bytes answered");
        }
    });
}
