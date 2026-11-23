//! The tab commands and the opens through `er_send`: each
//! answers, names `tabs` at the registry's new
//! revision, and leaves the registry as the strip asked. The browser commands
//! write the registry's active tab, and a close prunes the closed tab's view.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use er_engine::TabKey;
use er_protocol::{
    Ack, BrowserLayout, Command, ErErrorKind, PrOpenHint, Resource, ResourceChanged, ResourceKey,
    TabKind,
};
use er_service::wire_tab_key;

use super::{on_a_thread, review_flow_test_state, ReviewFlowTestDir};
use crate::browser_webview::TabWebviews;
use crate::resources::content_routes;
use crate::resources::send::{apply, Effects};
use crate::route_helpers::DesktopState;
use crate::service::ServiceState;
use crate::view_state::ViewStateHandle;

const REMOTE: &str = "acme/widgets";
const WAIT: Duration = Duration::from_secs(5);

fn diff() -> String {
    "diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1 +1 @@\n-a\n+b\n"
        .to_string()
}

fn ports() -> er_engine::ports::Ports {
    let git: Arc<dyn er_git::Git> = Arc::new(
        er_git::FakeGit::default()
            .with_current_branch(Ok("feature".to_string()))
            .with_detect_base_branch(Ok("main".to_string()))
            .with_diff_shortstat((0, 0, 0))
            .with_diff_raw(Ok(diff()))
            .with_diff_against_branch(Ok(diff()))
            .with_diff_checkout_against_base(Ok(diff()))
            .with_is_merge_in_progress(false)
            .with_remote_url(Ok(format!("https://github.com/{REMOTE}.git"))),
    );
    let github: Arc<dyn er_engine::github::GitHub> = Arc::new(
        er_engine::github::FakeGitHub::default()
            .with_pr_state(Ok("OPEN".to_string()))
            .with_pr_diff_remote(Ok(diff()))
            .with_pr_diff(Ok(diff()))
            .with_pr_head_oid_remote(Ok("head-1".to_string()))
            .with_overview_remote(Ok(Some(remote_overview())))
            .with_commits_remote(Ok(Vec::new())),
    );
    er_engine::ports::Ports { git, github }
}

/// What GitHub answers for a PR with no checkout, so a remote warm-up fills
/// the registry's remote open cache.
fn remote_overview() -> er_engine::github::PrOverviewData {
    er_engine::github::PrOverviewData {
        number: 7,
        title: "T".to_string(),
        body: String::new(),
        state: "OPEN".to_string(),
        author: "octo".to_string(),
        url: String::new(),
        base_branch: "main".to_string(),
        head_branch: "feature".to_string(),
        checks: Vec::new(),
        reviewers: Vec::new(),
    }
}

pub(super) fn state(dir: &ReviewFlowTestDir) -> DesktopState {
    let mut state = review_flow_test_state(dir);
    let settings = dir.path().join("settings").join("config.toml");
    state.service = Arc::new(ServiceState::with_settings_path(ports(), settings));
    state
}

fn with_storage(test: impl FnOnce(&ReviewFlowTestDir)) {
    let dir = ReviewFlowTestDir::new();
    let storage = dir.path().join("storage");
    er_engine::storage::with_process_test_storage_root(storage, || test(&dir));
}

pub(super) fn dir_in(dir: &ReviewFlowTestDir, name: &str) -> String {
    let path = dir.path().join(name);
    std::fs::create_dir_all(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
    path.to_string_lossy().into_owned()
}

/// A project on `root` that tracks the remote.
pub(super) fn register_project(root: &str) {
    let record = crate::projects::ProjectRecord {
        id: "widgets".to_string(),
        name: "widgets".to_string(),
        root_path: root.to_string(),
        remote: Some(REMOTE.to_string()),
        ..Default::default()
    };
    let saved = er_engine::projects_pins::update(|file| {
        file.projects.push(record);
        Ok(true)
    });
    assert!(saved.is_ok(), "{saved:?}");
    crate::projects::invalidate_load_cache();
}

/// The webview steps a browser command asked for, by tab key and position.
#[derive(Default)]
struct Webviews {
    steps: Mutex<Vec<(&'static str, TabKey, usize)>>,
}

impl TabWebviews for Webviews {
    fn show(&self, _: &ViewStateHandle, key: TabKey, tab_idx: usize) -> Result<(), String> {
        self.steps
            .lock()
            .map_err(|e| e.to_string())?
            .push(("show", key, tab_idx));
        Ok(())
    }

    fn sync_chrome(
        &self,
        _: &ViewStateHandle,
        key: TabKey,
        tab_idx: usize,
        _: bool,
    ) -> Result<(), String> {
        self.steps
            .lock()
            .map_err(|e| e.to_string())?
            .push(("sync", key, tab_idx));
        Ok(())
    }
}

/// Sends `command` through `er_send`'s body on a thread.
pub(super) fn send_on_a_thread(
    state: &DesktopState,
    command: Command,
    picked: Option<String>,
) -> Result<Ack, er_protocol::ErError> {
    let what = format!("{command:?}");
    on_a_thread(state, &what, move |state| {
        let pick = move || picked.clone();
        let effects = Effects {
            notify: &|_, _| {},
            check_update: || panic!("no update check in this test"),
            webviews: &crate::browser_webview::NoWebviews,
            pick_folder: &pick,
        };
        apply(state, command, &effects).map(|applied| applied.ack)
    })
}

/// Sends `command` and checks its `Ack` names `tabs` at the new revision.
fn send_tab_command(state: &DesktopState, command: Command) -> er_service::TabsView {
    let what = format!("{command:?}");
    let ack = send_on_a_thread(state, command, None).unwrap_or_else(|e| panic!("{what}: {e:?}"));
    let view = state.service.app_registry().tabs_view();
    let version = crate::resources::version(state, ResourceKey::Tabs);
    let version = version.unwrap_or_else(|e| panic!("{what}: {e:?}"));
    assert!(version.0 >= view.revision, "{what}: {version:?}");
    assert_eq!(
        ack.changed,
        vec![ResourceChanged {
            key: ResourceKey::Tabs,
            version,
        }],
        "{what}"
    );
    (*view).clone()
}

fn wire(key: TabKey) -> er_protocol::TabKey {
    wire_tab_key(key)
}

/// The kinds and labels the registry lists, in strip order.
fn strip(state: &DesktopState) -> Vec<(TabKind, String)> {
    let view = state.service.app_registry().tabs_view();
    er_service::tabs_resource(&view)
        .tabs
        .into_iter()
        .map(|row| (row.kind, row.label))
        .collect()
}

/// Every tab command and open answers through `er_send`, and the registry
/// holds what the strip asked for.
#[test]
fn every_tab_command_and_open_answers_through_er_send() {
    with_storage(|dir| {
        let state = state(dir);
        let one = dir_in(dir, "widgets");
        let two = dir_in(dir, "gadgets");
        register_project(&one);

        let project = || "widgets".to_string();
        send_tab_command(
            &state,
            Command::SetActiveProject {
                project_id: project(),
            },
        );
        let picked = send_on_a_thread(&state, Command::OpenWorktree, Some(two.clone()));
        assert!(picked.is_ok(), "open_worktree: {picked:?}");
        let cancelled = send_on_a_thread(&state, Command::OpenWorktree, None);
        assert_eq!(cancelled.map(|ack| ack.changed), Ok(Vec::new()));
        send_tab_command(&state, Command::NewTab);
        assert_eq!(
            strip(&state),
            vec![
                (TabKind::Working, "widgets".to_string()),
                (TabKind::Working, "gadgets".to_string()),
                (TabKind::Working, "gadgets".to_string()),
            ]
        );

        let first = state.service.app_registry().tabs_view().tabs[0].key;
        let view = send_tab_command(&state, Command::SelectTab { key: wire(first) });
        assert_eq!(view.active, Some(first));
        let view = send_tab_command(
            &state,
            Command::ReorderTabs {
                key: wire(first),
                to_idx: 2,
            },
        );
        assert_eq!(view.tabs[2].key, first);
        assert_eq!(view.active, Some(first));
        let closed = view.tabs[0].key;
        let view = send_tab_command(&state, Command::CloseTab { key: wire(closed) });
        assert!(view.tabs.iter().all(|tab| tab.key != closed));
        let gone = send_on_a_thread(&state, Command::SelectTab { key: wire(closed) }, None);
        assert_eq!(gone.map_err(|e| e.kind), Err(ErErrorKind::NotFound));

        let opens = [
            Command::OpenLocalBranch {
                project_id: project(),
                name: "feature".into(),
                replace: false,
            },
            Command::OpenProjectBranch {
                project_id: project(),
                branch: "topic".into(),
                replace: false,
            },
            Command::OpenRemotePr {
                owner: "acme".into(),
                repo: "gadgets".into(),
                number: 7,
                replace: false,
            },
            Command::OpenPrUrl {
                url: "https://github.com/acme/tools/pull/8".into(),
                replace: false,
            },
            Command::OpenPrReview {
                project_id: project(),
                pr_number: 9,
                replace: false,
            },
        ];
        for open in opens {
            let what = format!("{open:?}");
            let before = state.service.app_registry().tabs_view().tabs.len();
            let view = send_tab_command(&state, open);
            assert_eq!(view.tabs.len(), before + 1, "{what}");
        }
        let rows = er_service::tabs_resource(&state.service.app_registry().tabs_view()).tabs;
        let tail: Vec<_> = rows[rows.len() - 5..]
            .iter()
            .map(|row| (row.kind, row.label.clone()))
            .collect();
        assert_eq!(
            tail,
            vec![
                (TabKind::LocalBranch, "feature".to_string()),
                (TabKind::LocalBranch, "topic".to_string()),
                (TabKind::RemotePr, "gadgets#7".to_string()),
                (TabKind::RemotePr, "tools#8".to_string()),
                (TabKind::LocalPr, "widgets#9".to_string()),
            ]
        );

        // The prefetches answer at once and move nothing.
        for prefetch in [
            Command::PrefetchPrOpen {
                project_id: project(),
                pr_number: 9,
                hint: PrOpenHint {
                    base_ref: "main".into(),
                    head_ref: "topic".into(),
                    head_oid: "abc".into(),
                    updated_at: "2026-11-19T00:00:00Z".into(),
                },
            },
            Command::PrefetchRemotePrOpen {
                owner: "acme".into(),
                repo: "gadgets".into(),
                number: 7,
            },
        ] {
            let what = format!("{prefetch:?}");
            let ack = send_on_a_thread(&state, prefetch, None);
            assert_eq!(ack.map(|ack| ack.changed), Ok(Vec::new()), "{what}");
        }
    });
}

/// Two working tabs, the second active in the registry.
fn two_tabs(state: &DesktopState, dir: &ReviewFlowTestDir) -> (TabKey, TabKey) {
    for name in ["one", "two"] {
        let tab = content_routes::working_tab(dir_in(dir, name));
        let opened = on_a_thread(state, "open", move |state| {
            content_routes::open(state, tab, false)
        });
        assert!(opened.is_ok(), "{opened:?}");
    }
    let view = state.service.app_registry().tabs_view();
    let keys = (view.tabs[0].key, view.tabs[1].key);
    assert_eq!(view.active, Some(keys.1));
    keys
}

fn browser_of(state: &DesktopState, key: TabKey) -> crate::view_state::TabBrowserState {
    state
        .view
        .lock()
        .map(|view| view.browser(key))
        .unwrap_or_default()
}

/// The browser commands write the registry's active tab, drive its webview
/// at its position, and answer its `view` version.
#[test]
fn the_browser_commands_write_the_registry_active_tab() {
    with_storage(|dir| {
        let state = state(dir);
        let (first, active) = two_tabs(&state, dir);
        let webviews = Arc::new(Webviews::default());

        let sent = Arc::clone(&webviews);
        let ack = on_a_thread(&state, "update_tab_browser", move |state| {
            let effects = Effects {
                notify: &|_, _| {},
                check_update: || panic!("no update check in this test"),
                webviews: &*sent,
                pick_folder: &|| None,
            };
            let command = Command::UpdateTabBrowser {
                layout: Some(BrowserLayout::Split),
                url: Some("http://localhost:5173/".into()),
                navigate: true,
                annotate: None,
                tooltips: Some(true),
                split_ratio: None,
            };
            apply(state, command, &effects).map(|applied| applied.ack)
        });
        let ack = ack.unwrap_or_else(|e| panic!("update_tab_browser: {e:?}"));
        let written = browser_of(&state, active);
        assert_eq!(written.url, "http://localhost:5173/");
        assert_eq!(written.layout, crate::view_state::BrowserLayout::Split);
        assert!(written.show_tooltips);
        assert_eq!(browser_of(&state, first), Default::default());

        let fetched = crate::resources::fetch(&state, ResourceKey::View { tab: wire(active) })
            .unwrap_or_else(|e| panic!("view fetch: {e:?}"));
        assert_eq!(
            ack.changed,
            vec![ResourceChanged {
                key: ResourceKey::View { tab: wire(active) },
                version: fetched.version,
            }]
        );
        match fetched.value {
            Resource::View(view) => {
                assert_eq!(view.browser.layout, BrowserLayout::Split);
                assert_eq!(view.browser.url, "http://localhost:5173/");
            }
            other => panic!("not a view: {other:?}"),
        }

        let steps = webviews.steps.lock().map(|s| s.clone()).unwrap_or_default();
        assert_eq!(steps, vec![("show", active, 1)]);
    });
}

/// `cycle_tab_browser_layout` through `er_send` moves the registry's active
/// tab one layout on, shows its webview, and answers its
/// `view` version.
#[test]
fn cycle_tab_browser_layout_through_er_send_moves_the_registry_active_tab() {
    with_storage(|dir| {
        let state = state(dir);
        let (first, active) = two_tabs(&state, dir);
        let webviews = Arc::new(Webviews::default());

        let sent = Arc::clone(&webviews);
        let ack = on_a_thread(&state, "cycle_tab_browser_layout", move |state| {
            let effects = Effects {
                notify: &|_, _| {},
                check_update: || panic!("no update check in this test"),
                webviews: &*sent,
                pick_folder: &|| None,
            };
            let command = Command::CycleTabBrowserLayout;
            apply(state, command, &effects).map(|applied| applied.ack)
        });
        let ack = ack.unwrap_or_else(|e| panic!("cycle_tab_browser_layout: {e:?}"));
        let layout = browser_of(&state, active).layout;
        assert_eq!(layout, crate::view_state::BrowserLayout::Split);
        assert_eq!(browser_of(&state, first), Default::default());

        let fetched = crate::resources::fetch(&state, ResourceKey::View { tab: wire(active) })
            .unwrap_or_else(|e| panic!("view fetch: {e:?}"));
        assert_eq!(
            ack.changed,
            vec![ResourceChanged {
                key: ResourceKey::View { tab: wire(active) },
                version: fetched.version,
            }]
        );
        let steps = webviews.steps.lock().map(|s| s.clone()).unwrap_or_default();
        assert_eq!(steps, vec![("show", active, 1)]);
    });
}

/// The bodies the tab arm calls take `DesktopState`, which still reaches the
/// App through `tauri_app_handle`. That handle is `None` in tests, so the
/// thread-run tests cannot see a lock taken through it; this scan can. It
/// reads each named body; the helpers those bodies call go unscanned.
#[test]
fn no_body_the_tab_arm_calls_reaches_the_app() {
    let commands = include_str!("../../route_helpers.rs");
    let routes = include_str!("../../resources/content_routes.rs");
    let bodies = [
        (commands, "fn service_open("),
        (commands, "fn service_open_pr_url("),
        (commands, "fn service_open_remote_pr("),
        (commands, "fn service_open_pr_review("),
        (commands, "fn service_open_folder("),
        (commands, "fn service_open_project_branch("),
        (commands, "fn service_set_active_project("),
        (commands, "fn service_new_tab("),
        (commands, "pub(crate) fn prefetch_pr_open_impl("),
        (commands, "pub(crate) fn prefetch_remote_pr_open_impl("),
        (routes, "fn open("),
        (routes, "fn app_command("),
        (routes, "fn close_key("),
        (routes, "fn drop_content("),
    ];
    let forbidden = ["tauri_app_handle", ".app.lock", "er_engine::app"];
    for (source, signature) in bodies {
        let body = source
            .split_once(signature)
            .and_then(|(_, rest)| rest.split_once("\n}\n"))
            .map(|(body, _)| body)
            .unwrap_or_else(|| panic!("no {signature}"));
        assert!(
            body.contains("DesktopState"),
            "{signature} does not take DesktopState"
        );
        for name in forbidden {
            assert!(!body.contains(name), "{signature} names {name}");
        }
    }
}

/// A close's `TabsChanged` drops the closed tab's browser state over the
/// registry's list, and keeps the other tab's.
#[test]
fn a_close_prunes_the_closed_tabs_view() {
    with_storage(|dir| {
        let state = state(dir);
        let (first, closed) = two_tabs(&state, dir);
        for key in [first, closed] {
            if let Ok(mut view) = state.view.lock() {
                view.browser_mut(key).url = "http://localhost:5173/".into();
            }
        }
        crate::tabs::follow_service(&state.service, Arc::clone(&state.view));

        let pruned = on_a_thread(&state, "close_tab", move |state| {
            let effects = Effects {
                notify: &|_, _| {},
                check_update: || panic!("no update check in this test"),
                webviews: &crate::browser_webview::NoWebviews,
                pick_folder: &|| None,
            };
            let closed_tab = Command::CloseTab { key: wire(closed) };
            let ack = apply(state, closed_tab, &effects).map(|applied| applied.ack);
            assert!(ack.is_ok(), "{ack:?}");
            let deadline = Instant::now() + WAIT;
            while !browser_of(state, closed).url.is_empty() {
                if Instant::now() > deadline {
                    return false;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            true
        });

        assert!(pruned, "the closed tab's browser state stayed");
        assert_eq!(browser_of(&state, first).url, "http://localhost:5173/");
    });
}

/// The open-target median on the resource path: `er_send` `new_tab` to its
/// `Ack`, then `er_fetch` of `tabs` and its serialisation. Compare with the
/// flag-off baseline in `latency_tests.rs`.
#[test]
#[ignore = "latency harness: run with --ignored --nocapture and paste the numbers in the PR; never a CI assertion"]
fn the_open_target_reports_its_median() {
    const RUNS: usize = 50;
    const WARMUP: usize = 5;
    with_storage(|dir| {
        let state = state(dir);
        let (first, _) = two_tabs(&state, dir);
        let effects = Effects {
            notify: &|_, _| {},
            check_update: || panic!("no update check in this test"),
            webviews: &crate::browser_webview::NoWebviews,
            pick_folder: &|| None,
        };
        let mut samples = Vec::with_capacity(RUNS);
        let mut bytes = 0;
        for run in 0..RUNS {
            let started = Instant::now();
            let ack = apply(&state, Command::NewTab, &effects)
                .unwrap_or_else(|e| panic!("new_tab: {e:?}"))
                .ack;
            let keys: Vec<ResourceKey> = ack.changed.into_iter().map(|c| c.key).collect();
            let fetched: Vec<_> = keys
                .into_iter()
                .map(|key| crate::resources::fetch(&state, key))
                .collect::<Result<_, _>>()
                .unwrap_or_else(|e| panic!("fetch: {e:?}"));
            bytes = serde_json::to_vec(&fetched).map(|v| v.len()).unwrap_or(0);
            if run >= WARMUP {
                samples.push(started.elapsed());
            }
            let opened = state.service.app_registry().tabs_view().active;
            if let Some(opened) = opened.filter(|key| *key != first) {
                let close = Command::CloseTab { key: wire(opened) };
                let _ = apply(&state, close, &effects);
            }
        }
        samples.sort_unstable();
        let median = samples[samples.len() / 2];
        println!("open target: median {median:?}, answer {bytes} bytes");
    });
}
