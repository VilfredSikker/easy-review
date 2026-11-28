//! The resource host: the relay, the `tabs` read path and the backend
//! tick, each over a real service.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use er_engine::tabs_record::TabDescriptor;
use er_engine::TabKey;
use er_protocol::{ResourceChanged, ResourceKey};
use er_service::{AppCommand, Command, TabContent};

use super::{review_flow_test_state, ReviewFlowTestDir};
use crate::resources::content_routes;
use crate::resources::relay::{self, Moved};
use crate::resources::{self, tick, Watched};
use crate::route_helpers::DesktopState;
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
) -> (DesktopState, Vec<(TabKey, TabDescriptor)>) {
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

fn content(state: &DesktopState, (key, tab): &(TabKey, TabDescriptor)) -> Arc<TabContent> {
    state
        .service
        .content(*key, tab)
        .unwrap_or_else(|| panic!("no content under {key:?}"))
}

/// The relay over `state`, started once the fixture's own publishes have
/// settled, so each burst it hands on comes from the test's write.
fn relay(state: &DesktopState, wakes: Receiver<()>) -> Receiver<Moved> {
    std::thread::sleep(Duration::from_millis(200));
    while wakes.try_recv().is_ok() {}
    let (tx, bursts) = mpsc::channel();
    relay::spawn(state.clone(), wakes, move |moved| {
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

        let written =
            content_routes::write_keyed(&state, active, None, |context| Command::MarkReviewed {
                context,
                path: "src/f0.rs".to_string(),
                reviewed: true,
            });
        assert!(written.is_ok(), "{written:?}");

        let moved = bursts.recv_timeout(WAIT).expect("no burst after the write");
        let reviewed = content(&state, &tabs[1]).reviewed.revision;
        // The tab's diff lists each file's reviewed mark, and its tour counts
        // the reviewed files of each pillar, so both move too.
        let diff = resources::diff::published_version(&state, active, &tabs[1].1);
        let annotations = er_service::annotations_version(&content(&state, &tabs[1]));
        let entry = er_service::TabEntry {
            key: active,
            descriptor: tabs[1].1.clone(),
        };
        let marks = resources::reviewed::published_version(&state, &entry);
        assert_eq!(
            moved,
            vec![
                (Watched::Reviewed(active), reviewed),
                (Watched::TabDiff(active), diff),
                (Watched::Annotations(active), annotations),
                (Watched::TabReviewed(active), marks)
            ]
        );
        let wire: Vec<ResourceKey> = relay::wire(&moved).into_iter().map(|c| c.key).collect();
        let tab = er_service::wire_tab_key(active);
        let expected = [
            ResourceKey::Diff { tab },
            ResourceKey::Annotations { tab },
            ResourceKey::Reviewed { tab },
        ];
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
        let version = resources::version(&state, ResourceKey::Tabs);
        let version = version.unwrap_or_else(|e| panic!("tabs version: {e:?}"));
        assert_eq!(
            relay::wire(&moved),
            vec![ResourceChanged {
                key: ResourceKey::Tabs,
                version,
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
        let fetched = resources::fetch(&state, ResourceKey::Tabs);
        let view = state.service.app_registry().tabs_view();
        let Ok(fetched) = fetched else {
            panic!("tabs answered {fetched:?}");
        };
        assert!(fetched.version.0 >= view.revision, "{fetched:?}");
        let er_protocol::Resource::Tabs(fetched) = fetched.value else {
            panic!("tabs answered {:?}", fetched.value);
        };
        let listed = |rows: &[er_protocol::TabRow]| -> Vec<_> {
            rows.iter()
                .map(|row| (row.key, row.kind, row.repo_root.clone()))
                .collect()
        };
        let plain = er_service::tabs_resource(&view);
        assert_eq!(listed(&fetched.tabs), listed(&plain.tabs));
        assert_eq!(fetched.active, plain.active);
        assert_eq!(view.tabs.len(), tabs.len());
    });
}

/// The `tabs` row of `key`, fetched.
fn tab_row(state: &DesktopState, key: TabKey) -> er_protocol::TabRow {
    let fetched = resources::fetch(state, ResourceKey::Tabs);
    let Ok(er_protocol::Versioned {
        value: er_protocol::Resource::Tabs(tabs),
        ..
    }) = fetched
    else {
        panic!("tabs answered {fetched:?}");
    };
    let wire = er_service::wire_tab_key(key);
    tabs.tabs
        .into_iter()
        .find(|row| row.key == wire)
        .unwrap_or_else(|| panic!("no row for {key:?}"))
}

/// A loaded Working tab's row names its checkout's branch, as the App's tab
/// row does, where its descriptor alone names the folder.
#[test]
fn a_working_tab_row_names_its_checkouts_branch() {
    with_storage(|dir| {
        let (state, tabs) = fixture(dir, None, &["one"]);
        let row = tab_row(&state, tabs[0].0);
        assert_eq!(row.label, "feature");
        assert_eq!(row.live_branch.as_deref(), Some("feature"));
    });
}

/// A loaded tab's row carries its diff's branch diff hash, which the strip's
/// new-changes dot compares.
#[test]
fn a_tab_row_carries_its_diffs_change_token() {
    with_storage(|dir| {
        let (state, tabs) = fixture(dir, None, &["one"]);
        let hash = content(&state, &tabs[0]).diff.meta.branch_diff_hash.clone();
        assert!(!hash.is_empty(), "the fixture's diff has no hash");
        assert_eq!(tab_row(&state, tabs[0].0).change_token, hash);
    });
}

/// A local PR tab saved with no branch, loaded, over a clone whose `origin`
/// names acme/widgets.
fn local_pr_without_a_branch(dir: &ReviewFlowTestDir) -> (DesktopState, (TabKey, TabDescriptor)) {
    let github = er_engine::github::FakeGitHub::default()
        .with_pr_diff(Ok(raw_diff()))
        .with_pr_commits(Ok(Vec::new()))
        .with_get_repo_info(Ok(("acme".into(), "widgets".into())));
    let ports = er_engine::ports::Ports {
        github: Arc::new(github),
        ..ports()
    };
    let mut state = review_flow_test_state(dir);
    let settings = dir.path().join("settings").join("config.toml");
    state.service = Arc::new(ServiceState::with_settings_path(ports.clone(), settings));
    state.ports = ports;
    let root = dir.path().join("pr");
    std::fs::create_dir_all(&root).unwrap_or_else(|e| panic!("repo dir: {e}"));
    let tab = TabDescriptor {
        kind: er_engine::tabs_record::TabKind::LocalPr,
        pr_number: Some(7),
        pr_head_ref: Some("refs/er/pr/7/head".into()),
        ..content_routes::working_tab(root.to_string_lossy().into_owned())
    };
    let opened = content_routes::open(&state, tab.clone(), false);
    assert!(opened.is_ok(), "{opened:?}");
    let key = state.service.app_registry().tabs_view().active;
    let entry = (key.unwrap_or_else(|| panic!("no active tab")), tab);
    wait_until("the PR to load", || {
        state.service.content(entry.0, &entry.1).is_some()
    });
    (state, entry)
}

/// A local PR saved with no branch is named by the head view its content
/// resolves, as the App names it, where its descriptor alone names the folder.
#[test]
fn a_local_pr_row_without_a_branch_names_its_head_view() {
    with_storage(|dir| {
        let (state, (key, _)) = local_pr_without_a_branch(dir);
        let row = tab_row(&state, key);
        assert_eq!(row.branch, None);
        assert_eq!(row.label, "pr/7");
    });
}

/// A local PR's row carries the `owner/repo` its clone's `origin` names,
/// which its stale mark and comment pull key on.
#[test]
fn a_local_pr_row_carries_its_clones_owner_and_repo() {
    with_storage(|dir| {
        let (state, (key, _)) = local_pr_without_a_branch(dir);
        assert_eq!(tab_row(&state, key).remote.as_deref(), Some("acme/widgets"));
    });
}

/// A load that changes what a row takes from its content moves `tabs` past
/// the registry's revision with no tab command, so the page refetches it.
#[test]
fn a_loaded_rows_values_move_tabs_past_the_registry() {
    with_storage(|dir| {
        let (state, _) = fixture(dir, None, &["one"]);
        let revision = state.service.app_registry().tabs_view().revision;
        let version = resources::version(&state, ResourceKey::Tabs);
        let version = version.unwrap_or_else(|e| panic!("tabs version: {e:?}"));
        assert!(version.0 > revision, "{version:?} at revision {revision}");
    });
}

/// Every module of the host takes `DesktopState`, so none reaches the App,
/// the tick included.
#[test]
fn no_resource_module_names_the_app() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/resources");
    let entries = std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    let mut scanned = 0;
    for entry in entries {
        let path = entry.unwrap_or_else(|e| panic!("{e}")).path();
        let source = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{e}"));
        for name in ["AppState", "er_engine::app"] {
            assert!(!source.contains(name), "{} names {name}", path.display());
        }
        scanned += 1;
    }
    assert!(scanned >= 2, "scanned {scanned} modules");
}

/// The asks, runs and task host outside `resources/` name nothing of the
/// App either, which er-engine no longer has. They reach the launcher and log
/// types through the engine's modules. The asks are planned in er-service's
/// `card_asks`.
#[test]
fn no_flag_on_host_module_names_the_engine_app() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for file in [
        "../er-service/src/card_asks/run.rs",
        "../er-service/src/card_asks/context.rs",
        "src/runs.rs",
        "src/task_hooks.rs",
    ] {
        let path = manifest.join(file);
        let source =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(
            !source.contains("er_engine::app"),
            "{file} names er_engine::app"
        );
    }
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

        tick::run(&state);

        for (tab, loaded) in tabs.iter().zip(&before) {
            wait_until("each loaded tab's reload", || {
                content(&state, tab).ai.revision > loaded.ai.revision
            });
        }
    });
}

/// An outside question reaches the export after one tick:
/// the export renders the tab's content, which the tick's `ReloadSidecars`
/// reads again.
#[test]
fn an_outside_question_reaches_the_flag_on_export_after_one_tick() {
    with_storage(|dir| {
        let (state, tabs) = fixture(dir, None, &["one"]);
        let loaded = content(&state, &tabs[0]);
        let (er_dir, diff_hash) = (&loaded.context.er_dir, &loaded.context.diff_hash);
        let dir = std::path::Path::new(er_dir);
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

        tick::run(&state);

        let wire = er_service::wire_tab_key(tabs[0].0);
        let options = er_protocol::ExportOptions::default();
        let export = || crate::direct::export::render_tab(&state, wire, &options);
        wait_until("the question in the export", || {
            export().is_ok_and(|body| body.contains("Asked from outside the app"))
        });
    });
}

/// The tick's duties publish through their own resources, so a tick that
/// brings an outside write into a tab's content bumps no desktop revision,
/// and the tick never wakes itself.
#[test]
fn a_tick_bumps_no_desktop_revision() {
    with_storage(|dir| {
        let (state, tabs) = fixture(dir, None, &["one"]);
        let loaded = content(&state, &tabs[0]);
        let revision = || state.desktop_revision.load(Ordering::Relaxed);
        write_review(&loaded.context.er_dir, &loaded.context.branch_diff_hash);
        let before = revision();

        tick::run(&state);

        wait_until("the tab's reload", || {
            content(&state, &tabs[0]).ai.revision > loaded.ai.revision
        });
        assert_eq!(revision(), before, "the tick bumped the desktop revision");
    });
}

/// A wake runs the tick at once: an outside write reaches the content long
/// before the interval.
#[test]
fn a_wake_runs_the_tick_before_the_interval() {
    with_storage(|dir| {
        let (state, tabs) = fixture(dir, None, &["one"]);
        let wake = tick::spawn(state.clone(), Duration::from_secs(600));
        let loaded = content(&state, &tabs[0]);
        write_review(&loaded.context.er_dir, &loaded.context.branch_diff_hash);

        wake.wake();

        wait_until("the tick to reload the tab", || {
            content(&state, &tabs[0]).ai.revision > loaded.ai.revision
        });
    });
}

/// A desktop revision bump runs the tick through the watcher the app runs:
/// an outside write reaches the content long before the interval.
#[test]
fn a_revision_bump_runs_the_tick_through_the_watcher() {
    with_storage(|dir| {
        let (state, tabs) = fixture(dir, None, &["one"]);
        let stop = Arc::new(AtomicBool::new(false));
        let (emitted, emits) = mpsc::channel();
        let follower = {
            let desktop = state.clone();
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                let emit = move |revision: u64, _delta: u64| {
                    let _ = emitted.send(revision);
                };
                tick::follow_revisions(desktop, Duration::from_secs(600), &stop, &emit);
            })
        };
        // The watcher's first read emits, and wakes the tick, with no bump;
        // the write goes in once that tick has passed.
        let first = emits.recv_timeout(WAIT).expect("no first emit");
        std::thread::sleep(Duration::from_millis(200));
        let loaded = content(&state, &tabs[0]);
        write_review(&loaded.context.er_dir, &loaded.context.branch_diff_hash);

        let bumped =
            crate::profile_log::bump_desktop_revision(&state.desktop_revision, "test_bump");
        assert!(bumped > first, "{bumped} after {first}");

        wait_until("the tick to reload the tab", || {
            content(&state, &tabs[0]).ai.revision > loaded.ai.revision
        });
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
        let posted = tick::reload_loaded_tabs(&state);
        println!(
            "reload of {} loaded tabs: median {median:?} ({:?} per tab) over {RUNS} runs; \
             posting {posted} from the tick took {:?}",
            tabs.len(),
            median / tabs.len() as u32,
            started.elapsed(),
        );
    });
}
