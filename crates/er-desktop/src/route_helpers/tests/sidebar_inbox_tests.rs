//! The inbox: `inbox` holds the items, unread count and last refresh, built
//! on the server with the settings actor's prefs; the inbox commands go
//! through `er_send` to the sidebar's owner; and the PR sweep and the setup
//! hook read their prefs from the settings actor.

use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use er_protocol::{Command, ConfigFieldValue, ErErrorKind, Inbox, Resource, ResourceKey};

use super::on_a_thread;
use super::tab_resource_tests::{dir_in, register_project, send_on_a_thread, state as tab_state};
use super::{review_flow_test_state, ReviewFlowTestDir};
use crate::app_owner::startup;
use crate::resources;
use crate::resources::content_routes;
use crate::route_helpers::DesktopState;
use er_service::app_records::inbox::{InboxItem, InboxTarget};
use er_service::app_records::AppOwner;

fn with_storage(test: impl FnOnce(&ReviewFlowTestDir)) {
    let dir = ReviewFlowTestDir::new();
    let storage = dir.path().join("storage");
    er_engine::storage::with_process_test_storage_root(storage, || test(&dir));
}

fn item(id: &str, kind: &str, created_at_ms: u64, read_at_ms: Option<u64>) -> InboxItem {
    InboxItem {
        id: id.into(),
        kind: kind.into(),
        severity: "info".into(),
        title: format!("title {id}"),
        body: format!("body {id}"),
        source: "github".into(),
        target: InboxTarget {
            project_id: Some("p".into()),
            repo_root: Some("/src/p".into()),
            remote: Some("acme/p".into()),
            pr_number: Some(created_at_ms),
            branch: None,
            url: Some(format!("https://github.com/acme/p/pull/{created_at_ms}")),
        },
        created_at_ms,
        read_at_ms,
        dedupe_key: format!("dedupe-{id}"),
    }
}

/// Three unread items and one read, newest first, last swept at 42.
fn inbox_fixture(dir: &ReviewFlowTestDir) -> DesktopState {
    let state = review_flow_test_state(dir);
    {
        let mut inbox = state.inbox.lock().unwrap();
        inbox.items = vec![
            item("merged", "pr_merged", 4, None),
            item("ci", "ci_failed", 3, None),
            item("review", "review_requested", 2, Some(20)),
            item("ai", "ai_review_done", 1, None),
        ];
        inbox.last_refresh_ms = 42;
    }
    // The inbox's kind filter is the settings actor's, which the app starts
    // at launch; a publish never starts it.
    state.service.settings();
    state.app_owner.publish();
    state
}

fn fetch_inbox(state: &DesktopState) -> (u64, Inbox) {
    let fetched = resources::fetch(state, ResourceKey::Inbox).unwrap();
    let Resource::Inbox(inbox) = fetched.value else {
        panic!("{:?}", fetched.value);
    };
    (fetched.version.0, inbox)
}

fn apply(state: &DesktopState, command: Command) -> Result<er_protocol::Ack, er_protocol::ErError> {
    resources::send::apply(
        state,
        command,
        &resources::send::Effects {
            notify: &|_, _| {},
            check_update: || panic!("no update check in this test"),
            webviews: &crate::browser_webview::NoWebviews,
            pick_folder: &|| None,
        },
    )
    .map(|applied| applied.ack)
}

fn send(state: &DesktopState, command: Command) -> er_protocol::Ack {
    apply(state, command).unwrap()
}

/// Hides CI failures in the settings actor's config.
fn hide_ci_in_settings(state: &DesktopState) {
    send(
        state,
        Command::ApplyConfigPatch {
            key: "inbox.show.ci_failed".into(),
            value: ConfigFieldValue::Bool(false),
        },
    );
}

/// Polls `done` for up to five seconds, and answers whether it came true.
fn wait_for(mut done: impl FnMut() -> bool) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

fn unread(inbox: &Inbox) -> Vec<&str> {
    let unread = inbox.items.iter().filter(|item| item.read_at_ms.is_none());
    unread.map(|item| item.id.as_str()).collect()
}

/// A mark-read through `er_send` answers an `Ack` naming `inbox` at the
/// version that holds the read, with no tick between, and saves the inbox.
#[test]
fn a_mark_read_moves_the_inbox_in_its_ack() {
    with_storage(|dir| {
        let state = inbox_fixture(dir);
        let (before, inbox) = fetch_inbox(&state);
        assert_eq!(inbox.unread_count, 3);

        let ack = send(&state, Command::MarkInboxItemRead { id: "ci".into() });
        let moved = ack
            .changed
            .iter()
            .find(|changed| changed.key == ResourceKey::Inbox)
            .unwrap_or_else(|| panic!("the mark-read did not move inbox: {ack:?}"));
        let (after, inbox) = fetch_inbox(&state);
        assert!(after > before);
        assert_eq!(moved.version.0, after);
        assert_eq!(inbox.unread_count, 2);
        assert_eq!(unread(&inbox), ["merged", "ai"]);

        let saved = er_service::app_records::inbox::load_inbox_state();
        let ci = saved.items.iter().find(|item| item.id == "ci").unwrap();
        assert!(ci.read_at_ms.is_some(), "the read was not saved");
    });
}

/// Every inbox command reaches the owner and answers there: the reads and
/// clears land in `inbox`, the refresh runs the PR sweep, which stamps the
/// inbox's last refresh, and the test notification is refused in a process
/// that cannot show one.
#[test]
fn every_inbox_command_reaches_the_sidebar_owner() {
    with_storage(|dir| {
        let mut state = inbox_fixture(dir);
        state.ports = er_engine::ports::Ports {
            git: std::sync::Arc::new(er_git::FakeGit::default()),
            github: std::sync::Arc::new(er_engine::github::FakeGitHub::default()),
        };

        let ids = vec!["merged".to_string(), "ai".to_string()];
        send(&state, Command::MarkInboxItemsRead { ids: ids.clone() });
        assert_eq!(unread(&fetch_inbox(&state).1), ["ci"]);

        // Only read items leave; `ci` is unread and stays.
        let ids = vec!["merged".to_string(), "ci".to_string()];
        send(&state, Command::ClearInboxItems { ids });
        let ids: Vec<String> = fetch_inbox(&state)
            .1
            .items
            .into_iter()
            .map(|i| i.id)
            .collect();
        assert_eq!(ids, ["ci", "review", "ai"]);

        send(&state, Command::MarkAllInboxRead);
        assert_eq!(fetch_inbox(&state).1.unread_count, 0);

        send(&state, Command::ClearReadInboxItems);
        assert!(fetch_inbox(&state).1.items.is_empty());

        assert_eq!(state.inbox.lock().unwrap().last_refresh_ms, 42);
        send(&state, Command::RefreshNotifications);
        let swept = wait_for(|| {
            let running = state
                .pr_list_running
                .load(std::sync::atomic::Ordering::Acquire);
            !running && state.inbox.lock().unwrap().last_refresh_ms != 42
        });
        assert!(swept, "the refresh did not run the PR sweep");

        let refused = apply(&state, Command::TestNativeNotification).unwrap_err();
        assert_eq!(refused.kind, ErErrorKind::Refused, "{refused:?}");
        assert!(
            refused.message.contains("native notification"),
            "{refused:?}"
        );
    });
}

/// A kind the person hid is left out of `inbox`'s items and of its unread
/// count, and the items keep their order.
#[test]
fn the_inbox_leaves_out_hidden_kinds() {
    with_storage(|dir| {
        let state = inbox_fixture(dir);
        hide_ci_in_settings(&state);
        state.app_owner.publish();

        let (_, inbox) = fetch_inbox(&state);
        assert_eq!(inbox.unread_count, 2);
        let ids: Vec<&str> = inbox.items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, ["merged", "review", "ai"]);
    });
}

/// Runs `read` on another thread, and answers what it returned, or none when
/// it did not return in time.
fn off_thread<T: Send + 'static>(
    state: &DesktopState,
    read: impl FnOnce(&DesktopState) -> T + Send + 'static,
) -> Option<T> {
    let (tx, rx) = mpsc::channel();
    let worker = state.clone();
    std::thread::spawn(move || {
        let _ = tx.send(read(&worker));
    });
    rx.recv_timeout(Duration::from_secs(5)).ok()
}

/// The PR sweep's inbox prefs come from the settings actor's view.
#[test]
fn the_sweep_reads_the_inbox_prefs_off_thread() {
    with_storage(|dir| {
        let state = review_flow_test_state(dir);
        hide_ci_in_settings(&state);
        let prefs =
            off_thread(&state, startup::inbox_prefs).expect("the sweep's prefs never returned");
        assert!(!prefs.show.ci_failed);
    });
}

/// The setup hook flushes the pending native notifications with prefs from
/// the settings actor.
#[test]
fn the_setup_flush_reads_its_prefs_off_thread() {
    with_storage(|dir| {
        let state = review_flow_test_state(dir);
        let flushed = off_thread(&state, startup::flush_pending_notifications);
        assert!(flushed.is_some(), "the setup flush never returned");
    });
}

/// An item that names a branch of the `widgets` project `register_project`
/// records on `root`.
fn branch_item(id: &str, root: &str) -> InboxItem {
    let mut item = item(id, "ci_failed", 9, None);
    item.target = InboxTarget {
        project_id: Some("widgets".into()),
        repo_root: Some(root.into()),
        remote: Some("acme/widgets".into()),
        pr_number: None,
        branch: Some("feature".into()),
        url: None,
    };
    item
}

/// A project known only by its remote, as the sidebar adds one for a PR
/// opened by URL.
fn register_remote_only(id: &str, remote: &str) {
    let record = er_service::app_records::projects::ProjectRecord {
        id: id.to_string(),
        name: id.to_string(),
        root_path: String::new(),
        remote: Some(remote.to_string()),
        ..Default::default()
    };
    let saved = er_engine::projects_pins::update(|file| {
        file.projects.push(record);
        Ok(true)
    });
    assert!(saved.is_ok(), "{saved:?}");
    er_service::app_records::projects::invalidate_load_cache();
}

/// `tab_resource_tests`' state, which opens through fake ports, with the
/// sidebar's owner reading its inbox prefs from that state's settings actor.
fn open_state(dir: &ReviewFlowTestDir) -> DesktopState {
    let mut state = tab_state(dir);
    state.app_owner = Arc::new(AppOwner::new(
        Default::default(),
        Arc::clone(&state.pr_cache),
        Default::default(),
        Arc::clone(&state.gh_user),
        Arc::clone(&state.inbox),
    ));
    startup::read_inbox_prefs_from_settings(&state);
    state
}

/// An inbox open through `er_send` marks
/// the item read and opens its target in a new tab beside the active one, and
/// its `Ack` names `inbox` and `tabs`, with the new tab as created. A target
/// no project holds is refused after the read lands. A prefetch answers at
/// once and moves nothing, and the remote item's warms the registry's remote
/// open cache. An open with `replace` takes the active tab's place.
#[test]
fn an_inbox_open_reads_the_item_and_opens_its_target_off_thread() {
    with_storage(|dir| {
        let state = open_state(dir);
        let root = dir_in(dir, "widgets");
        register_project(&root);
        register_remote_only("gadgets", "acme/gadgets");
        let mut remote = item("remote", "ci_failed", 7, Some(1));
        remote.target.project_id = Some("gadgets".into());
        remote.target.repo_root = None;
        remote.target.remote = Some("acme/gadgets".into());
        state.inbox.lock().unwrap().items = vec![
            branch_item("feature", &root),
            item("lost", "ci_failed", 3, None),
            remote,
        ];
        state.service.settings();
        state.app_owner.publish();
        // A tab is active first, so an open that replaces it adds no tab.
        let working = content_routes::working_tab(dir_in(dir, "working"));
        let opened = on_a_thread(&state, "open", move |state| {
            content_routes::open(state, working, false)
        });
        assert!(opened.is_ok(), "{opened:?}");
        let registry = state.service.app_registry();
        assert!(registry.tabs_view().active.is_some(), "no active tab");
        let before = registry.tabs_view().tabs.len();

        let open = Command::OpenInboxItem {
            id: "feature".into(),
            replace: false,
        };
        let ack = send_on_a_thread(&state, open, None).unwrap_or_else(|e| panic!("open: {e:?}"));
        let view = state.service.app_registry().tabs_view();
        assert_eq!(view.tabs.len(), before + 1, "the open added no tab");
        let tabs = ack.changed.iter().find(|c| c.key == ResourceKey::Tabs);
        let version = crate::resources::version(&state, ResourceKey::Tabs);
        assert_eq!(
            tabs.map(|c| c.version),
            version.ok(),
            "the open's Ack does not name tabs: {ack:?}"
        );
        let active = view
            .active
            .map(|key| er_service::wire_tab_key(key).0.to_string());
        assert_eq!(ack.created, active);
        let (version, inbox) = fetch_inbox(&state);
        let moved = ack.changed.iter().find(|c| c.key == ResourceKey::Inbox);
        assert_eq!(
            moved.map(|c| c.version.0),
            Some(version),
            "the open's Ack does not name inbox: {ack:?}"
        );
        assert_eq!(unread(&inbox), ["lost"]);

        let lost = Command::OpenInboxItem {
            id: "lost".into(),
            replace: true,
        };
        let refused = send_on_a_thread(&state, lost, None).unwrap_err();
        assert_eq!(refused.kind, ErErrorKind::Refused, "{refused:?}");
        assert!(refused.message.contains("Could not open"), "{refused:?}");
        assert!(
            unread(&fetch_inbox(&state).1).is_empty(),
            "the read did not land"
        );

        // `remote` names a project with no checkout, so its prefetch reaches
        // the remote PR warm-up; the others warm nothing.
        assert!(registry
            .remote_pr_open_entry("acme", "gadgets", 7)
            .is_none());
        for id in ["remote", "feature", "lost", "gone"] {
            let prefetch = Command::PrefetchInboxItem { id: id.into() };
            let ack = send_on_a_thread(&state, prefetch, None);
            assert_eq!(ack.map(|ack| ack.changed), Ok(Vec::new()), "{id}");
        }
        let warmed = wait_for(|| {
            registry
                .remote_pr_open_entry("acme", "gadgets", 7)
                .is_some()
        });
        assert!(warmed, "the prefetch did not warm the remote open cache");

        let before = registry.tabs_view().tabs.len();
        let replace = Command::OpenInboxItem {
            id: "remote".into(),
            replace: true,
        };
        let ack =
            send_on_a_thread(&state, replace, None).unwrap_or_else(|e| panic!("replace: {e:?}"));
        let view = registry.tabs_view();
        assert_eq!(view.tabs.len(), before, "the open with replace added a tab");
        let active = view
            .active
            .map(|key| er_service::wire_tab_key(key).0.to_string());
        assert_eq!(ack.created, active);
        let rows = er_service::tabs_resource(&view).tabs;
        let remote = rows.iter().any(|row| row.label == "gadgets#7");
        assert!(
            remote,
            "the open with replace opened no remote tab: {rows:?}"
        );
    });
}

/// The mark-read median on the resource path: `er_send`
/// `mark_inbox_item_read` to its `Ack`, then `er_fetch` of what it names
/// and the serialisation. The inbox holds one item; an untimed reset marks
/// it unread before each run, so the version moves every time.
#[test]
#[ignore = "latency harness: run with --ignored --nocapture and paste the numbers in the PR; never a CI assertion"]
fn the_mark_read_reports_its_median() {
    const RUNS: usize = 50;
    const WARMUP: usize = 5;
    with_storage(|dir| {
        let state = inbox_fixture(dir);
        state.inbox.lock().unwrap().items = vec![item("one", "ci_failed", 1, None)];
        let mut samples = Vec::with_capacity(RUNS);
        let mut bytes = 0;
        for run in 0..RUNS {
            state.inbox.lock().unwrap().items[0].read_at_ms = None;
            state.app_owner.publish();
            let started = Instant::now();
            let ack = send(&state, Command::MarkInboxItemRead { id: "one".into() });
            assert!(!ack.changed.is_empty(), "run {run} moved nothing");
            let fetched: Vec<_> = ack
                .changed
                .into_iter()
                .map(|changed| resources::fetch(&state, changed.key))
                .collect::<Result<_, _>>()
                .unwrap_or_else(|e| panic!("fetch: {e:?}"));
            bytes = serde_json::to_vec(&fetched).map(|v| v.len()).unwrap_or(0);
            if run >= WARMUP {
                samples.push(started.elapsed());
            }
        }
        samples.sort_unstable();
        let median = samples[samples.len() / 2];
        println!("mark read: median {median:?}, answer {bytes} bytes");
    });
}
