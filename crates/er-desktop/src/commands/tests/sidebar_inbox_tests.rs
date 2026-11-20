//! The flag-on inbox: `inbox` holds the snapshot's items, unread count and
//! last refresh, built on the server with the settings actor's prefs; the
//! inbox commands go through `er_send` to the sidebar's owner; and the PR
//! sweep and the setup hook read their prefs with the App locked.

use std::sync::mpsc;
use std::time::Duration;

use er_protocol::{Command, ConfigFieldValue, ErErrorKind, Inbox, Resource, ResourceKey};

use super::{review_flow_test_state, snap_from, ReviewFlowTestDir};
use crate::app_owner::startup;
use crate::commands::AppState;
use crate::inbox::{InboxItem, InboxTarget};
use crate::resources;

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
fn inbox_fixture(dir: &ReviewFlowTestDir) -> AppState {
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

fn fetch_inbox(state: &AppState) -> (u64, Inbox) {
    let fetched = resources::fetch(&state.desktop, ResourceKey::Inbox).unwrap();
    let Resource::Inbox(inbox) = fetched.value else {
        panic!("{:?}", fetched.value);
    };
    (fetched.version.0, inbox)
}

fn apply(state: &AppState, command: Command) -> Result<er_protocol::Ack, er_protocol::ErError> {
    resources::send::apply(
        &state.desktop,
        command,
        &resources::send::Effects {
            notify: &|_, _| {},
            follow_selection: &|_| {},
            check_update: || panic!("no update check in this test"),
            webviews: &crate::browser_webview::NoWebviews,
            pick_folder: &|| None,
        },
    )
    .map(|applied| applied.ack)
}

fn send(state: &AppState, command: Command) -> er_protocol::Ack {
    apply(state, command).unwrap()
}

/// Hides CI failures in the settings actor's config, and leaves the App's
/// config showing them.
fn hide_ci_in_settings(state: &AppState) {
    send(
        state,
        Command::ApplyConfigPatch {
            key: "inbox.show.ci_failed".into(),
            value: ConfigFieldValue::Bool(false),
        },
    );
    assert!(state.app.lock().unwrap().config.inbox.show.ci_failed);
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

        let saved = crate::inbox::load_inbox_state();
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
            let running = state.loading.lock().unwrap().pr_list;
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

/// `inbox` lists what the snapshot lists, in its order, with the same fields
/// and categories, and counts unread the same way: a kind the person hid is
/// left out of the items and of the count. Each side reads its own prefs,
/// so both hide CI failures here.
#[test]
fn the_inbox_matches_the_snapshot() {
    with_storage(|dir| {
        let state = inbox_fixture(dir);
        hide_ci_in_settings(&state);
        state.app.lock().unwrap().config.inbox.show.ci_failed = false;
        state.app_owner.publish();

        let (_, inbox) = fetch_inbox(&state);
        let snap = snap_from(&state.app.lock().unwrap(), &state);
        assert_eq!(inbox.unread_count, 2);
        assert_eq!(
            usize::try_from(inbox.unread_count).unwrap(),
            snap.inbox_unread_count
        );
        assert_eq!(inbox.last_refresh_ms, snap.inbox_last_refresh_ms);
        assert_eq!(inbox.items.len(), snap.inbox_items.len());
        for (wire, shown) in inbox.items.iter().zip(&snap.inbox_items) {
            let fields = |w: &er_protocol::InboxItem| {
                (
                    w.id.clone(),
                    w.kind.clone(),
                    w.category.clone(),
                    w.severity.clone(),
                    w.title.clone(),
                    w.body.clone(),
                    w.source.clone(),
                    w.created_at_ms,
                    w.read_at_ms,
                    w.dedupe_key.clone(),
                )
            };
            let snapshot_fields = (
                shown.id.clone(),
                shown.kind.clone(),
                shown.category.clone(),
                shown.severity.clone(),
                shown.title.clone(),
                shown.body.clone(),
                shown.source.clone(),
                shown.created_at_ms,
                shown.read_at_ms,
                shown.dedupe_key.clone(),
            );
            assert_eq!(fields(wire), snapshot_fields);
            let target = &wire.target;
            let target = serde_json::json!({
                "project_id": target.project_id,
                "repo_root": target.repo_root,
                "remote": target.remote,
                "pr_number": target.pr_number,
                "branch": target.branch,
                "url": target.url,
            });
            assert_eq!(target, shown.target, "{}", wire.id);
        }
        let ids: Vec<&str> = inbox.items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, ["merged", "review", "ai"]);
    });
}

/// Runs `read` on another thread while this one holds the App's lock, and
/// answers what it returned, or none when it waited on the App.
fn with_the_app_locked<T: Send + 'static>(
    state: &AppState,
    read: impl FnOnce(&AppState) -> T + Send + 'static,
) -> Option<T> {
    let _held = state.app.lock().unwrap();
    let (tx, rx) = mpsc::channel();
    let worker = state.clone();
    std::thread::spawn(move || {
        let _ = tx.send(read(&worker));
    });
    rx.recv_timeout(Duration::from_secs(5)).ok()
}

/// The PR sweep's inbox prefs come from the settings actor's view, so a held
/// App lock does not hold up the sweep, and the App's config is not what it
/// reads.
#[test]
fn the_sweep_reads_the_inbox_prefs_with_the_app_locked() {
    with_storage(|dir| {
        let state = review_flow_test_state(dir);
        hide_ci_in_settings(&state);
        let prefs = with_the_app_locked(&state, startup::inbox_prefs)
            .expect("the sweep's prefs waited on the App");
        assert!(!prefs.show.ci_failed);
    });
}

/// The setup hook flushes the pending native notifications with prefs from
/// the settings actor, so it runs while the App is locked.
#[test]
fn the_setup_flush_reads_its_prefs_with_the_app_locked() {
    with_storage(|dir| {
        let state = review_flow_test_state(dir);
        let flushed = with_the_app_locked(&state, startup::flush_pending_notifications);
        assert!(flushed.is_some(), "the setup flush waited on the App");
    });
}
