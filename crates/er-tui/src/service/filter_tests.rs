//! The flag-on filter and search (P70.2): the keys, the history overlay,
//! the filtered count, the repo's rule tables, `--filter` against a saved
//! session, and the snap off a hidden watched file, each as the App does.

use std::time::{Duration, Instant};

use crossterm::event::KeyCode;

use super::parity::assert_frame;
use super::parity::assert_json;
use super::shell::Shell;
use super::startup::Args;
use super::tests::{
    fixture, key, loaded, path_of, rows, save_session_of, service_sources, settle, shell,
    shell_frame,
};
use super::tui_view::TabViewState;
use crate::ui::highlight::Highlighter;

/// Presses `code`, the last step's message gone first, and checks the frame
/// against its golden once the replies have landed.
fn step(shell: &mut Shell, hl: &mut Highlighter, code: KeyCode) {
    shell.tick(Instant::now() + Duration::from_secs(60));
    shell.handle_key(key(code));
    settle(shell, |shell| shell.in_flight() == 0);
    let frame = shell_frame(shell, hl);
    assert_frame(&format!("{code:?}"), &frame);
}

fn steps(shell: &mut Shell, hl: &mut Highlighter, codes: &[KeyCode]) {
    for &code in codes {
        step(shell, hl, code);
    }
}

fn typed(text: &str) -> Vec<KeyCode> {
    text.chars().map(KeyCode::Char).collect()
}

fn state(shell: &Shell) -> &TabViewState {
    let key = shell.host().registry().tabs_view().active;
    key.and_then(|key| shell.view().tab(key))
        .unwrap_or_else(|| panic!("no view state"))
}

fn start(path: &str, storage: &std::path::Path, filter: Option<&str>) -> Shell {
    let args = Args {
        paths: vec![path.to_string()],
        filter: filter.map(String::from),
        ..Args::default()
    };
    let mut shell = shell(&args, storage);
    settle(&mut shell, loaded);
    shell
}

/// A shell on a three-file fixture.
fn both(storage: &std::path::Path, run: impl FnOnce(&mut Shell, &mut Highlighter)) {
    let flag_repo = fixture(3);
    er_engine::storage::with_process_test_storage_root(storage, || {
        let mut shell = start(&path_of(&flag_repo), storage, None);
        run(&mut shell, &mut Highlighter::new());
    });
}

/// Typing a filter, picking a preset from the history overlay, opening it
/// again with both filters listed, and clearing draw as the App draws, and
/// leave the same filter and history.
#[test]
fn filter_keys_and_history_match_the_app() {
    let _golden = super::parity::guard();
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    both(storage.path(), |shell, hl| {
        let mut codes = vec![KeyCode::Char('f')];
        codes.extend(typed("-f000"));
        codes.extend([KeyCode::Enter, KeyCode::Char('F'), KeyCode::Char('j')]);
        codes.extend([KeyCode::Enter, KeyCode::Char('F'), KeyCode::Char('j')]);
        steps(shell, hl, &codes);
        assert_json("history", &state(shell).filter_history);
        steps(shell, hl, &[KeyCode::Esc, KeyCode::Esc]);
        assert_json("expr", &state(shell).filter_expr);
        assert_json("history", &state(shell).filter_history);
    });
}

/// A search narrows the list as it is typed, Enter snaps to a file it
/// keeps, and Esc drops it.
#[test]
fn search_keys_match_the_app() {
    let _golden = super::parity::guard();
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    both(storage.path(), |shell, hl| {
        let codes = [
            KeyCode::Char('/'),
            KeyCode::Char('2'),
            KeyCode::Enter,
            KeyCode::Esc,
            KeyCode::Char('/'),
            KeyCode::Char('1'),
            KeyCode::Esc,
        ];
        steps(shell, hl, &codes);
    });
}

/// A filter that hides a reviewed file draws the count among the files it
/// keeps beside the count over every file.
#[test]
fn the_filtered_count_draws_like_the_app() {
    let _golden = super::parity::guard();
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    both(storage.path(), |shell, hl| {
        let mut codes = vec![KeyCode::Char(' '), KeyCode::Char('f')];
        codes.extend(typed("-f000"));
        codes.push(KeyCode::Enter);
        steps(shell, hl, &codes);
        let top = rows(&shell_frame(shell, hl))[1].clone();
        assert!(top.contains("0/2 · 1/3 reviewed"), "{top}");
    });
}

/// A `kind:` filter reads the repo's `[file_kinds]`.
#[test]
fn a_kind_filter_reads_the_repo_tables() {
    let _golden = super::parity::guard();
    let flag_repo = fixture(3);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let flag_path = path_of(&flag_repo);
    let git = er_engine::ports::Ports::cli().git;
    let slug = er_engine::storage::rules_key(&*git, &flag_path, None);
    let config = format!("[file_kinds.\"{slug}\"]\n\"src/f001.rs\" = \"test\"\n");
    std::fs::write(storage.path().join("config.toml"), config).unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = start(&flag_path, storage.path(), None);
        let mut hl = Highlighter::new();
        let mut codes = vec![KeyCode::Char('f')];
        codes.extend(typed("-kind:test"));
        codes.push(KeyCode::Enter);
        steps(&mut shell, &mut hl, &codes);
        let listed = rows(&shell_frame(&mut shell, &mut hl)).join("\n");
        assert!(!listed.contains("f001.rs"), "{listed}");
    });
}

/// `--filter` applies before the session: with no session it holds, and a
/// saved filter from the same diff replaces it and its history, as on the App.
#[test]
fn a_start_filter_yields_to_a_saved_one_like_the_app() {
    let _golden = super::parity::guard();
    for saved in [false, true] {
        let flag_repo = fixture(3);
        let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let flag_path = path_of(&flag_repo);
        er_engine::storage::with_process_test_storage_root(storage.path(), || {
            if saved {
                let first = start(&flag_path, storage.path(), None);
                save_session_of(&first, |session| {
                    session.filter_expr = "f002".into();
                    session.filter_history = vec!["f002".into(), "old".into()];
                });
                drop(first);
            }
            let mut shell = start(&flag_path, storage.path(), Some("f001"));
            let mut hl = Highlighter::new();
            assert_frame("start", &shell_frame(&mut shell, &mut hl));
            let restored = if saved { "f002" } else { "f001" };
            assert_eq!(state(&shell).filter_expr, restored);
            assert_json("expr", &state(&shell).filter_expr);
            assert_json("history", &state(&shell).filter_history);
        });
    }
}

/// A search that hides the selected watched file selects the first diff
/// file it keeps, as the App's snap does.
#[test]
fn a_search_that_hides_a_watched_file_snaps_like_the_app() {
    let _golden = super::parity::guard();
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let repo = super::watched_tests::watched_repo(storage.path());
    let path = path_of(&repo);
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = super::watched_tests::start(&path, storage.path());
        let mut codes = vec![KeyCode::Char('k'), KeyCode::Char('/')];
        codes.extend(typed("f000"));
        for code in codes {
            shell.handle_key(key(code));
        }
        assert!(state(&shell).watched.selected.is_some(), "k left the diff");
        let mut hl = Highlighter::new();
        step(&mut shell, &mut hl, KeyCode::Enter);
        assert_eq!(state(&shell).watched.selected, None);
    });
}

/// The filter runs on the view state: no flag-on module calls the App
/// tab's filter or snap, or reads its rules.
#[test]
fn the_filter_runs_on_the_view_state() {
    let banned = [
        "apply_filter_expr",
        "filter_rules",
        "snap_to_visible",
        ".tab().",
        "tab_mut().",
    ];
    for (name, text) in &service_sources() {
        let flat: String = text.split_whitespace().collect();
        for needle in banned {
            assert!(!flat.contains(needle), "{name} names {needle}");
        }
    }
}
