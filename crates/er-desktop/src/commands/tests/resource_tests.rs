//! The flag-on resource host: the relay, the `tabs` read path and the backend
//! tick, each over a real service and the App fixture.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use er_engine::tabs_record::TabDescriptor;
use er_engine::TabKey;
use er_protocol::{ResourceChanged, ResourceKey, ResourceVersion};
use er_service::{AppCommand, Command, Reply, TabContent};

use super::{review_flow_test_state, ReviewFlowTestDir};
use crate::commands::{content_routes, AppState};
use crate::resources::relay::{self, Moved};
use crate::resources::{self, tick, Watched};
use crate::service::{actor_key, ServiceState};

const WAIT: Duration = Duration::from_secs(5);

fn raw_diff() -> String {
    use std::fmt::Write as _;

    let mut raw = String::new();
    for n in 0..3 {
        let _ = write!(
            raw,
            "diff --git a/src/f{n}.rs b/src/f{n}.rs\n--- a/src/f{n}.rs\n+++ b/src/f{n}.rs\n\
             @@ -1,2 +1,2 @@\n-old\n+new\n ctx\n"
        );
    }
    raw
}

fn ports() -> er_engine::ports::Ports {
    let git: Arc<dyn er_git::Git> = Arc::new(
        er_git::FakeGit::default()
            .with_current_branch(Ok("feature".to_string()))
            .with_detect_base_branch(Ok("main".to_string()))
            .with_diff_shortstat((0, 0, 0))
            .with_diff_raw(Ok(raw_diff()))
            .with_is_merge_in_progress(false)
            .with_remote_url(Ok("https://github.com/acme/widgets.git".to_string())),
    );
    let github: Arc<dyn er_engine::github::GitHub> =
        Arc::new(er_engine::github::FakeGitHub::default());
    er_engine::ports::Ports { git, github }
}

/// A working tab on each of `repos`, each on its own actor and loaded, the
/// last one active. The service wakes `wake` as the desktop's does.
fn fixture(
    dir: &ReviewFlowTestDir,
    wake: Option<Sender<()>>,
    repos: &[&str],
) -> (AppState, Vec<(TabKey, TabDescriptor)>) {
    let mut state = review_flow_test_state(dir);
    let settings = dir.path().join("settings").join("config.toml");
    let service = ServiceState::with_settings_path(ports(), settings);
    state.service = Arc::new(match wake {
        Some(wake) => service.with_resource_wake(wake),
        None => service,
    });
    for repo in repos {
        let root = dir.path().join(repo);
        std::fs::create_dir_all(&root).unwrap_or_else(|e| panic!("repo dir: {e}"));
        let tab = content_routes::working_tab(root.to_string_lossy().into_owned());
        let opened = content_routes::open(&state, tab, false);
        assert!(opened.is_ok(), "{opened:?}");
    }
    let view = state.service.app_registry().tabs_view();
    let tabs: Vec<_> = view
        .tabs
        .iter()
        .map(|entry| (entry.key, entry.descriptor.clone()))
        .collect();
    for tab in &tabs {
        wait_until("the tab to load", || {
            state.service.content(tab.0, &tab.1).is_some()
        });
    }
    (state, tabs)
}

fn with_storage(test: impl FnOnce(&ReviewFlowTestDir)) {
    let dir = ReviewFlowTestDir::new();
    let storage = dir.path().join("storage");
    er_engine::storage::with_process_test_storage_root(storage, || test(&dir));
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

/// The relay over `state`, started once the fixture's own publishes have
/// settled, so each burst it hands on comes from the test's write.
fn relay(state: &AppState, wakes: Receiver<()>) -> Receiver<Moved> {
    std::thread::sleep(Duration::from_millis(200));
    while wakes.try_recv().is_ok() {}
    let (tx, bursts) = mpsc::channel();
    relay::spawn(state.desktop.clone(), wakes, move |moved| {
        let _ = tx.send(moved);
    });
    bursts
}

/// An outside write to a tab's sidecars, stamped ahead of the clock so the
/// reload sees it however coarse the file system's mtime is.
fn write_review(er_dir: &str, diff_hash: &str) {
    let dir = std::path::Path::new(er_dir);
    er_engine::test_support::write_sidecar_json::<er_engine::ai::ErReview>(
        dir,
        serde_json::json!({
            "version": 1,
            "diff_hash": diff_hash,
            "files": { "src/f0.rs": { "risk": "low", "findings": [{
                "id": "f-1", "severity": "low", "title": "A defect",
                "description": "desc", "hunk_index": 0, "line_start": 1
            }] } }
        }),
    );
    stamp_ahead(&dir.join("review.json"));
}

fn stamp_ahead(path: &std::path::Path) {
    let ahead = std::time::SystemTime::now() + Duration::from_secs(5);
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    file.set_modified(ahead)
        .unwrap_or_else(|e| panic!("stamp {}: {e}", path.display()));
}

/// A command the App started that has already finished, so the next
/// `check_commands` takes it.
fn finished_command(state: &AppState) {
    let (tx, rx) = mpsc::channel();
    let _ = tx.send(Ok(()));
    let mut app = state.app.lock().unwrap_or_else(|e| e.into_inner());
    app.tab_mut()
        .command_rx
        .insert("tick-probe".to_string(), rx);
}

fn command_pending(state: &AppState) -> bool {
    let app = state.app.lock().unwrap_or_else(|e| e.into_inner());
    app.tab().command_rx.contains_key("tick-probe")
}

/// A reviewed write moves that tab's reviewed section and nothing else, so
/// the relay hands on that one key: no other section of either tab, and no
/// `tabs` change.
#[test]
fn the_relay_hands_on_only_the_sections_that_moved() {
    with_storage(|dir| {
        let (wake, wakes) = mpsc::channel();
        let (state, tabs) = fixture(dir, Some(wake), &["one", "two"]);
        let bursts = relay(&state, wakes);
        let active = tabs[1].0;

        let written = content_routes::write_reviewed(&state, |context| Command::MarkReviewed {
            context,
            path: "src/f0.rs".to_string(),
            reviewed: true,
        });
        assert!(written.is_ok(), "{written:?}");

        let moved = bursts.recv_timeout(WAIT).expect("no burst after the write");
        let reviewed = content(&state, &tabs[1]).reviewed.revision;
        // The tab's diff lists each file's reviewed mark, so it moves too.
        let diff = resources::diff::published_version(&state.desktop, active, &tabs[1].1);
        let entry = er_service::TabEntry {
            key: active,
            descriptor: tabs[1].1.clone(),
        };
        let marks = resources::reviewed::published_version(&state.desktop, &entry);
        assert_eq!(
            moved,
            vec![
                (Watched::Reviewed(active), reviewed),
                (Watched::TabDiff(active), diff),
                (Watched::TabReviewed(active), marks)
            ]
        );
        let wire: Vec<ResourceKey> = relay::wire(&moved).into_iter().map(|c| c.key).collect();
        let tab = er_service::wire_tab_key(active);
        let expected = [ResourceKey::Diff { tab }, ResourceKey::Reviewed { tab }];
        assert_eq!(wire, expected, "{moved:?}");
    });
}

/// A tab command reaches the wire as one `tabs` change at the registry's new
/// revision.
#[test]
fn a_tab_change_reaches_the_wire_as_one_tabs_change() {
    with_storage(|dir| {
        let (wake, wakes) = mpsc::channel();
        let (state, tabs) = fixture(dir, Some(wake), &["one", "two"]);
        let bursts = relay(&state, wakes);

        let selected = state
            .service
            .send_app(AppCommand::SelectTab { key: tabs[0].0 });
        assert!(selected.is_ok(), "{selected:?}");

        let moved = bursts
            .recv_timeout(WAIT)
            .expect("no burst after the tab change");
        let revision = state.service.app_registry().tabs_view().revision;
        assert_eq!(
            relay::wire(&moved),
            vec![ResourceChanged {
                key: ResourceKey::Tabs,
                version: ResourceVersion(revision),
            }]
        );
    });
}

/// `tabs` is read from the registry's published list. No read command exists
/// on the registry, so a scan of the read is what can fail: its command path
/// applies tab commands inline on the caller's thread.
#[test]
fn the_tabs_read_comes_from_the_published_list() {
    let source = include_str!("../../resources/host.rs");
    let body = source
        .split_once("fn tabs(")
        .and_then(|(_, rest)| rest.split_once("\n}\n"))
        .map(|(body, _)| body)
        .unwrap_or_else(|| panic!("no tabs read in the host"));
    assert!(body.contains("tabs_view()"), "{body}");
    for command_path in ["send_app", ".send("] {
        assert!(
            !body.contains(command_path),
            "the read calls {command_path}"
        );
    }

    with_storage(|dir| {
        let (state, tabs) = fixture(dir, None, &["one", "two"]);
        let fetched = resources::fetch(&state.desktop, ResourceKey::Tabs);
        let view = state.service.app_registry().tabs_view();
        let Ok(fetched) = fetched else {
            panic!("tabs answered {fetched:?}");
        };
        assert_eq!(fetched.version, ResourceVersion(view.revision));
        assert_eq!(
            fetched.value,
            er_protocol::Resource::Tabs(er_service::tabs_resource(&view))
        );
        assert_eq!(view.tabs.len(), tabs.len());
    });
}

/// Every module of the host takes `DesktopState`, so none reaches the App.
/// The tick is the one that may: it is handed the App beside the state.
#[test]
fn no_resource_module_names_the_app() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/resources");
    let entries = std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    let mut scanned = 0;
    for entry in entries {
        let path = entry.unwrap_or_else(|e| panic!("{e}")).path();
        if path.file_name().is_some_and(|name| name == "tick.rs") {
            continue;
        }
        let source = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{e}"));
        for name in ["AppState", "er_engine::app"] {
            assert!(!source.contains(name), "{} names {name}", path.display());
        }
        scanned += 1;
    }
    assert!(scanned >= 2, "scanned {scanned} modules");
}

/// An outside write to the loaded tabs' bucket reaches each tab's content
/// through one tick, the inactive tab's too, with no frontend call.
#[test]
fn the_tick_reloads_every_loaded_tab() {
    with_storage(|dir| {
        let (state, tabs) = fixture(dir, None, &["one", "two"]);
        let before: Vec<Arc<TabContent>> = tabs.iter().map(|tab| content(&state, tab)).collect();
        assert_eq!(before[0].context.er_dir, before[1].context.er_dir);
        let context = &before[0].context;
        write_review(&context.er_dir, &context.branch_diff_hash);

        tick::run(&state.desktop, &state.app);

        for (tab, loaded) in tabs.iter().zip(&before) {
            wait_until("each loaded tab's reload", || {
                content(&state, tab).ai.revision > loaded.ai.revision
            });
        }
    });
}

/// With the flag on, an outside question reaches the export after one tick.
/// The export renders the App's tab until it renders from the content, and
/// the tick's sidecar check is the only path into that tab.
#[test]
fn an_outside_question_reaches_the_flag_on_export_after_one_tick() {
    with_storage(|dir| {
        let state = review_flow_test_state(dir);
        let (er_dir, diff_hash) = {
            let mut app = state.app.lock().unwrap_or_else(|e| e.into_inner());
            app.tab_mut().check_ai_files_changed();
            (app.tab().er_dir(), app.tab().diff_hash.clone())
        };
        let dir = std::path::Path::new(&er_dir);
        er_engine::test_support::write_sidecar_json::<er_engine::ai::ErQuestions>(
            dir,
            serde_json::json!({
                "version": 1,
                "diff_hash": diff_hash,
                "questions": [{
                    "id": "q-outside", "timestamp": "", "file": "src/lib.rs",
                    "hunk_index": 0, "line_start": 1, "line_content": "",
                    "text": "Asked from outside the app"
                }]
            }),
        );
        stamp_ahead(&dir.join("questions.json"));

        tick::run(&state.desktop, &state.app);

        let app = state.app.lock().unwrap_or_else(|e| e.into_inner());
        let export =
            crate::export::render_markdown(app.tab(), &crate::export::ExportOpts::default());
        assert!(export.contains("Asked from outside the app"), "{export}");
    });
}

/// With the flag on the poll runs none of the tick's duties: a finished
/// command stays for the tick, and the sidecars are not read again, so a
/// reload after the poll still finds the outside write new.
#[test]
fn the_flag_on_poll_leaves_the_duties_to_the_tick() {
    with_storage(|dir| {
        let (state, tabs) = fixture(dir, None, &["one"]);
        let active = &tabs[0];
        let loaded = content(&state, active);
        finished_command(&state);
        write_review(&loaded.context.er_dir, &loaded.context.branch_diff_hash);

        let polled = crate::commands::poll_impl(&state);
        assert!(polled.is_ok(), "{:?}", polled.err());

        assert!(command_pending(&state), "the poll checked the commands");
        let reload = state.service.send(
            &actor_key(&active.1),
            Command::ReloadSidecars { key: active.0 },
        );
        assert!(
            matches!(reload, Ok(Reply::SidecarsReloaded { reloaded: true, .. })),
            "the poll reloaded the sidecars: {reload:?}"
        );
    });
}

/// A tick that finishes a command bumps the desktop revision, so the page
/// polls for it; one that only drains agent log lines does not, or a running
/// agent would wake the tick and the poll every debounce.
#[test]
fn the_tick_bumps_for_a_finished_command_and_not_for_log_lines() {
    with_storage(|dir| {
        let state = review_flow_test_state(dir);
        let revision = || {
            state
                .desktop_revision
                .load(std::sync::atomic::Ordering::Relaxed)
        };
        {
            let app = state.app.lock().unwrap_or_else(|e| e.into_inner());
            let line = er_engine::app::AgentLogEntry {
                timestamp: Instant::now(),
                command_name: Arc::from("lens"),
                source: er_engine::app::AgentLogSource::Stdout,
                text: "working".to_string(),
            };
            assert!(app.tab().log_tx.send(line).is_ok());
        }
        let before = revision();

        tick::run(&state.desktop, &state.app);

        let drained = {
            let app = state.app.lock().unwrap_or_else(|e| e.into_inner());
            app.tab().agent_log.len()
        };
        assert_eq!(drained, 1, "the tick did not drain the line");
        assert_eq!(revision(), before, "a log line bumped the revision");

        finished_command(&state);
        tick::run(&state.desktop, &state.app);

        assert!(!command_pending(&state));
        assert!(revision() > before, "a finished command did not bump");
    });
}

/// A revision bump runs the tick at once: a finished command is drained
/// long before the interval.
#[test]
fn a_wake_runs_the_tick_before_the_interval() {
    with_storage(|dir| {
        let state = review_flow_test_state(dir);
        let wake = tick::spawn(
            state.desktop.clone(),
            Arc::clone(&state.app),
            Duration::from_secs(600),
        );
        finished_command(&state);

        wake.wake();

        wait_until("the tick to drain the command", || !command_pending(&state));
    });
}

/// A desktop revision bump runs the tick through the watcher the app runs,
/// the one that emits `er://revision`: a finished command is drained long
/// before the interval.
#[test]
fn a_revision_bump_runs_the_tick_through_the_watcher() {
    with_storage(|dir| {
        let state = review_flow_test_state(dir);
        let stop = Arc::new(AtomicBool::new(false));
        let (emitted, emits) = mpsc::channel();
        let follower = {
            let desktop = state.desktop.clone();
            let app = Arc::clone(&state.app);
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                let emit = move |revision: u64, _delta: u64| {
                    let _ = emitted.send(revision);
                };
                tick::follow_revisions(desktop, app, Duration::from_secs(600), &stop, &emit);
            })
        };
        // The watcher's first read emits, and wakes the tick, with no bump;
        // the command goes in once that tick has passed.
        let first = emits.recv_timeout(WAIT).expect("no first emit");
        std::thread::sleep(Duration::from_millis(200));
        finished_command(&state);

        let bumped =
            crate::profile_log::bump_desktop_revision(&state.desktop_revision, "test_bump");
        assert!(bumped > first, "{bumped} after {first}");

        wait_until("the tick to drain the command", || !command_pending(&state));
        stop.store(true, Ordering::Relaxed);
        assert!(follower.join().is_ok(), "the watcher panicked");
    });
}

/// The reload's cost per loaded tab when nothing moved: the actor's round
/// trip and the stamp read. Run with `--ignored --nocapture`.
#[test]
#[ignore = "cost harness: run with --ignored --nocapture and paste the numbers in the PR; never a CI assertion"]
fn the_tick_reload_reports_its_cost() {
    const RUNS: usize = 50;
    with_storage(|dir| {
        let repos: Vec<String> = (0..10).map(|n| format!("repo{n}")).collect();
        let names: Vec<&str> = repos.iter().map(String::as_str).collect();
        let (state, tabs) = fixture(dir, None, &names);
        let mut samples: Vec<Duration> = (0..RUNS)
            .map(|_| {
                let started = Instant::now();
                for (key, tab) in &tabs {
                    let reload = state
                        .service
                        .send(&actor_key(tab), Command::ReloadSidecars { key: *key });
                    assert!(reload.is_ok(), "{reload:?}");
                }
                started.elapsed()
            })
            .collect();
        samples.sort_unstable();
        let median = samples[RUNS / 2];
        let started = Instant::now();
        let posted = tick::reload_loaded_tabs(&state.desktop);
        println!(
            "reload of {} loaded tabs: median {median:?} ({:?} per tab) over {RUNS} runs; \
             posting {posted} from the tick took {:?}",
            tabs.len(),
            median / tabs.len() as u32,
            started.elapsed(),
        );
    });
}
