//! The diff commands on `er_send`: each moves its tab's `diff(tab)` and
//! answers that version, with the App lock held; a filter is the desktop's
//! own view state, never the App tab's.

use er_engine::tab_identity::DiffMode;
use er_engine::tabs_record::TabDescriptor;
use er_engine::TabKey;
use er_protocol::{Ack, Command, ErError, Resource, ResourceKey, TabDiff};

use super::diff_resource_tests::{fake_ports, state_over, with_storage};
use super::while_the_app_is_locked;
use crate::commands::{content_routes, AppState};
use crate::resources::send::{apply, Effects};

fn with_tab(test: impl FnOnce(&AppState, (TabKey, TabDescriptor))) {
    with_storage(|dir| {
        let state = state_over(dir, fake_ports());
        let root = dir.path().join("repo");
        std::fs::create_dir_all(&root).unwrap_or_else(|e| panic!("repo dir: {e}"));
        let tab = content_routes::working_tab(root.to_string_lossy().into_owned());
        let opened = content_routes::open(&state, tab.clone(), false);
        assert!(opened.is_ok(), "{opened:?}");
        let key = state.service.app_registry().tabs_view().active;
        test(&state, (key.unwrap_or_else(|| panic!("no active tab")), tab));
    });
}

fn send(state: &AppState, name: &'static str, command: Command) -> Result<Ack, ErError> {
    while_the_app_is_locked(state, name, move |state| {
        let effects = Effects {
            notify: &|_, _| {},
            follow_selection: &|_| {},
            check_update: || panic!("no update check in this test"),
            webviews: &crate::browser_webview::NoWebviews,
            pick_folder: &|| None,
        };
        apply(&state.desktop, command, &effects).map(|applied| applied.ack)
    })
}

fn diff(state: &AppState, key: TabKey) -> (u64, TabDiff) {
    let key = ResourceKey::Diff {
        tab: er_service::wire_tab_key(key),
    };
    let fetched = crate::resources::fetch(&state.desktop, key).unwrap_or_else(|e| panic!("{e:?}"));
    match fetched.value {
        Resource::Diff(diff) => (fetched.version.0, *diff),
        other => panic!("not a diff: {other:?}"),
    }
}

/// The `Ack` names the tab's diff at the version a fetch then answers, above
/// the one before the command.
fn assert_moved(state: &AppState, key: TabKey, before: u64, ack: &Ack) -> TabDiff {
    let (version, diff) = diff(state, key);
    assert!(version > before, "the diff stayed at {before}");
    let named: Vec<u64> = ack
        .changed
        .iter()
        .map(|changed| changed.version.0)
        .collect();
    assert_eq!(named, [version], "the ack names another version");
    diff
}

#[test]
fn each_diff_command_moves_its_tab_diff_with_the_app_lock_held() {
    with_tab(|state, (key, tab)| {
        let wire = er_service::wire_tab_key(key);
        let commands = [
            (
                "set_diff_mode",
                Command::SetDiffMode {
                    tab: wire,
                    mode: "staged".into(),
                    pr_number: None,
                },
            ),
            (
                "set_filter",
                Command::SetFilter {
                    tab: wire,
                    query: "src/f1.rs".into(),
                },
            ),
            (
                "refresh_diff",
                Command::RefreshDiff {
                    tab: wire,
                    sync: false,
                },
            ),
            (
                "sync",
                Command::RefreshDiff {
                    tab: wire,
                    sync: true,
                },
            ),
            (
                "change_base",
                Command::ChangeBase {
                    tab: wire,
                    base: "dev".into(),
                },
            ),
        ];
        for (name, command) in commands {
            let before = diff(state, key).0;
            let ack = send(state, name, command).unwrap_or_else(|e| panic!("{name}: {e:?}"));
            assert_moved(state, key, before, &ack);
        }
        let content = state
            .service
            .content(key, &tab)
            .unwrap_or_else(|| panic!("no content"));
        assert_eq!(content.tab.mode, DiffMode::Staged);
        assert_eq!(content.tab.base_branch, "dev");
        let saved = state.service.app_registry().tabs_view();
        let base_ref = saved
            .tabs
            .iter()
            .find(|entry| entry.key == key)
            .map(|entry| entry.descriptor.base_ref.clone());
        assert_eq!(
            base_ref,
            Some(Some("dev".to_string())),
            "the base was not saved"
        );
    });
}

#[test]
fn a_filter_lists_only_the_files_it_keeps_and_joins_the_recent_filters() {
    with_tab(|state, (key, _)| {
        let wire = er_service::wire_tab_key(key);
        let query = "src/f1.rs".to_string();
        let filter = Command::SetFilter { tab: wire, query };
        send(state, "set_filter", filter).unwrap_or_else(|e| panic!("{e:?}"));
        let (_, filtered) = diff(state, key);
        let paths: Vec<&str> = filtered.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["src/f1.rs"]);
        assert_eq!(filtered.total_count, 3, "the total counts the whole diff");
        assert_eq!(filtered.filter.as_deref(), Some("src/f1.rs"));
        let recent = filtered
            .filter_suggestions
            .iter()
            .any(|s| s.kind == "history" && s.expr == "src/f1.rs");
        assert!(recent, "the filter is not among the recent ones");

        let query = String::new();
        let clear = Command::SetFilter { tab: wire, query };
        send(state, "clear_filter", clear).unwrap_or_else(|e| panic!("{e:?}"));
        let (_, cleared) = diff(state, key);
        assert_eq!((cleared.files.len(), cleared.filter), (3, None));
    });
}

/// The filter is the desktop's own state: the module that answers and writes
/// it names neither the App nor its tab.
#[test]
fn the_flag_on_filter_never_reads_the_app_tab() {
    let source = include_str!("../../resources/diff.rs");
    for app in [
        "state.app",
        ".app.lock",
        "TabState",
        "filter_input",
        "apply_filter_expr",
        ".tab()",
    ] {
        assert!(!source.contains(app), "resources/diff.rs reads {app}");
    }
}
