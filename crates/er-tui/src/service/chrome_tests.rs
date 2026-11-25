//! The chrome keys on the flag path (P75.3): the Help hub, the tab close,
//! the two widths and the split diff's focus, each against the App.

use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::parity::assert_frame;
use super::parity::assert_json;
use super::shell::Shell;
use super::tests::{fixture, git, key, loaded, path_of, settle, shell, shell_frame, WAIT};
use super::threads_tests::{active_key, args, both_on, state, step};
use crate::ui::highlight::Highlighter;

fn storage() -> tempfile::TempDir {
    tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"))
}

fn press(shell: &mut Shell, hl: &mut Highlighter, code: KeyCode) {
    step(shell, hl, KeyEvent::new(code, KeyModifiers::NONE));
}

/// `?` opens the Help hub as the App's: the same rows, the cursor skipping
/// headers both ways, and Enter or Esc closing it.
#[test]
fn the_help_hub_draws_and_moves_like_the_app() {
    let _golden = super::parity::guard();
    let flag_repo = fixture(1);
    let storage = storage();
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = both_on(&flag_repo, storage.path());
        let mut hl = Highlighter::new();
        let codes = [
            KeyCode::Char('?'),
            KeyCode::Char('j'),
            KeyCode::Char('j'),
            KeyCode::Char('k'),
            KeyCode::Char('k'),
            KeyCode::Char('k'),
        ];
        for code in codes {
            press(&mut shell, &mut hl, code);
        }
        for _ in 0..12 {
            press(&mut shell, &mut hl, KeyCode::Char('j'));
        }
        for _ in 0..4 {
            press(&mut shell, &mut hl, KeyCode::Char('k'));
        }
        press(&mut shell, &mut hl, KeyCode::Enter);
        assert!(shell.view().overlay.is_none(), "Enter left the hub open");
        press(&mut shell, &mut hl, KeyCode::Char('?'));
        press(&mut shell, &mut hl, KeyCode::Esc);
        assert!(shell.view().overlay.is_none(), "Esc left the hub open");
    });
}

/// `x` closes the active tab as the App's: the tab that moves into its place
/// takes focus and loads, the status line names the closed tab, the closed
/// tab's content leaves its actor, and the last tab stays.
#[test]
fn closing_a_tab_focuses_the_next_and_drops_its_content() {
    let _golden = super::parity::guard();
    let repos = [fixture(1), fixture(2), fixture(3)];
    let storage = storage();
    let paths: Vec<String> = repos.iter().map(path_of).collect();
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = shell(&args(paths.clone()), storage.path());
        settle(&mut shell, loaded);
        let mut hl = Highlighter::new();
        shell.handle_key(key(KeyCode::Char(']')));
        settle(&mut shell, loaded);
        let closed = active_key(&shell);
        let workspace = shell
            .host()
            .registry()
            .tabs_view()
            .tabs
            .iter()
            .find(|entry| entry.key == closed)
            .map(|entry| entry.descriptor.workspace_key())
            .unwrap_or_else(|| panic!("no closed tab"));

        shell.handle_key(key(KeyCode::Char('x')));
        assert_json("notice", &shell.view().notification);
        settle(&mut shell, loaded);
        let tabs = shell.host().registry().tabs_view();
        assert_eq!(tabs.tabs.len(), 2);
        assert_eq!(
            tabs.active,
            tabs.key_at(1),
            "the next tab did not take focus"
        );
        let frame = shell_frame(&mut shell, &mut hl);
        assert_frame("closed", &frame);

        let deadline = Instant::now() + WAIT;
        while shell.host().content(&workspace, closed).is_some() && Instant::now() < deadline {
            shell.tick(Instant::now());
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            shell.host().content(&workspace, closed).is_none(),
            "the closed tab's content stayed on its actor"
        );

        for _ in 0..2 {
            press(&mut shell, &mut hl, KeyCode::Char('x'));
            settle(&mut shell, loaded);
        }
        assert_eq!(shell.host().registry().tabs_view().tabs.len(), 1);
        assert_eq!(
            shell.view().notification.as_deref(),
            Some("Cannot close last tab")
        );
    });
}

/// `<`, `>`, `{` and `}` move the widths as the App's: each held to its
/// range and to a diff at least 20 columns wide at the drawn width.
#[test]
fn the_width_keys_hold_the_apps_limits() {
    let _golden = super::parity::guard();
    let flag_repo = fixture(2);
    let storage = storage();
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = both_on(&flag_repo, storage.path());
        let mut hl = Highlighter::new();
        shell_frame(&mut shell, &mut hl);
        let mut codes = vec![KeyCode::Char('<'); 10];
        codes.extend([KeyCode::Char('>'); 25]);
        codes.extend([KeyCode::Char('<'); 8]);
        codes.push(KeyCode::Char('p'));
        codes.extend([KeyCode::Char('}'); 12]);
        codes.extend([KeyCode::Char('>'); 12]);
        codes.extend([KeyCode::Char('{'); 14]);
        let key = active_key(&shell);
        for code in codes {
            press(&mut shell, &mut hl, code);
            let view = state(&shell, key);
            assert_json(
                &format!("{code:?}"),
                &(view.file_tree_width, view.panel_width),
            );
        }
        assert!(state(&shell, key).panel.is_some(), "no panel to resize");
    });
}

/// With the split diff on, Tab moves the cursor's side, `l`, `h` and Home
/// scroll that side, and a question started on the old side takes its line
/// and `LEFT`, as the App's.
#[test]
fn the_split_diff_focus_and_scroll_follow_the_app() {
    let _golden = super::parity::guard();
    let flag_repo = fixture(1);
    // Three lines added at the top, so the old and new numbers differ on
    // the context lines below them.
    for repo in [&flag_repo] {
        let file = repo.path().join("src/f000.rs");
        let body = std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(&file, format!("let a = 1;\nlet b = 2;\nlet c = 3;\n{body}"))
            .unwrap_or_else(|e| panic!("{e}"));
        git(repo.path(), &["commit", "-q", "-am", "top"]);
    }
    let storage = storage();
    let config = storage.path().join("config.toml");
    std::fs::write(&config, "[display]\nsplit_diff = true\n").unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = both_on(&flag_repo, storage.path());
        let mut hl = Highlighter::new();
        let codes = [
            KeyCode::Char('l'),
            KeyCode::Tab,
            KeyCode::Char('l'),
            KeyCode::Char('l'),
            KeyCode::Char('h'),
            KeyCode::Down,
            KeyCode::Down,
            KeyCode::Down,
            KeyCode::Down,
            KeyCode::Home,
            KeyCode::Char('l'),
        ];
        for code in codes {
            press(&mut shell, &mut hl, code);
        }
        let key = active_key(&shell);
        let view = state(&shell, key);
        assert_json("scroll", &(view.h_scroll_old, view.h_scroll_new));
        press(&mut shell, &mut hl, KeyCode::Char('q'));
        let draft = &state(&shell, key).draft;
        assert_json("draft", &(draft.line, draft.side.as_deref()));
        assert_eq!(draft.side.as_deref(), Some("LEFT"));
        // The cursor is on a context line whose old number is three less
        // than its new one; the old side takes the old number.
        let content = shell
            .active_content()
            .unwrap_or_else(|| panic!("no content"));
        let line = state(&shell, key)
            .current_line
            .and_then(|at| content.diff.files[0].hunks[0].lines.get(at))
            .unwrap_or_else(|| panic!("no cursor line"));
        assert_eq!(line.new_num, line.old_num.map(|old| old + 3), "{line:?}");
        assert_eq!(draft.line, line.old_num);
    });
}

/// Up and Shift+Up move the cursor and the selection's end up, as the
/// App's, after Down and Shift+Down move them down.
#[test]
fn up_and_shift_up_move_like_the_app() {
    let _golden = super::parity::guard();
    let flag_repo = fixture(1);
    let storage = storage();
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = both_on(&flag_repo, storage.path());
        let mut hl = Highlighter::new();
        let none = KeyModifiers::NONE;
        let shift = KeyModifiers::SHIFT;
        let events = [
            with(KeyCode::Down, none),
            with(KeyCode::Down, none),
            with(KeyCode::Down, none),
            with(KeyCode::Down, none),
            with(KeyCode::Up, none),
            with(KeyCode::Down, shift),
            with(KeyCode::Down, shift),
            with(KeyCode::Up, shift),
        ];
        let key = active_key(&shell);
        for event in events {
            step(&mut shell, &mut hl, event);
            let view = state(&shell, key);
            let anchor = view.selection_anchor.as_ref().map(|(_, at)| *at);
            assert_json(&format!("{event:?}"), &(view.current_line, anchor));
        }
    });
}

/// `w` starts and stops the watch, the scroll keys move the diff, and a
/// panel's line keys and Esc act, each as on the App.
#[test]
fn the_watch_scroll_and_panel_keys_draw_like_the_app() {
    let _golden = super::parity::guard();
    let flag_repo = fixture(2);
    let storage = storage();
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = both_on(&flag_repo, storage.path());
        let mut hl = Highlighter::new();
        let codes = [
            KeyCode::Char('w'),
            KeyCode::Char('w'),
            KeyCode::Char('l'),
            KeyCode::Right,
            KeyCode::Char('h'),
            KeyCode::Char('l'),
            KeyCode::Home,
            KeyCode::Char('d'),
            KeyCode::Char('d'),
            KeyCode::Char('u'),
            KeyCode::PageDown,
            KeyCode::PageUp,
            KeyCode::Char('p'),
            KeyCode::Tab,
            KeyCode::Char('k'),
            KeyCode::Char('k'),
            KeyCode::Char('j'),
            KeyCode::Esc,
        ];
        for code in codes {
            press(&mut shell, &mut hl, code);
        }
    });
}

/// A Verify row whose command is not set says where to set it, as the
/// App's disabled row does.
#[test]
fn an_unset_verify_row_says_why_like_the_app() {
    let _golden = super::parity::guard();
    let flag_repo = fixture(1);
    let storage = storage();
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = both_on(&flag_repo, storage.path());
        let mut hl = Highlighter::new();
        for code in [KeyCode::Char('v'), KeyCode::Enter] {
            press(&mut shell, &mut hl, code);
        }
        assert_json("notice", &shell.view().notification);
        let message = shell.view().notification.clone();
        assert!(message.is_some_and(|m| m.contains("[commands]")));
    });
}

fn with(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

/// The composer's new line, its scroll keys and Esc, as on the App.
#[test]
fn the_composer_line_scroll_and_cancel_keys_draw_like_the_app() {
    let _golden = super::parity::guard();
    let flag_repo = fixture(1);
    let storage = storage();
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = both_on(&flag_repo, storage.path());
        let mut hl = Highlighter::new();
        let none = KeyModifiers::NONE;
        let ctrl = KeyModifiers::CONTROL;
        let events = [
            with(KeyCode::Down, none),
            with(KeyCode::Char('q'), none),
            with(KeyCode::Char('a'), none),
            with(KeyCode::Enter, KeyModifiers::SHIFT),
            with(KeyCode::Char('b'), none),
            with(KeyCode::Char('d'), ctrl),
            with(KeyCode::Char('u'), ctrl),
            with(KeyCode::PageDown, none),
            with(KeyCode::PageUp, none),
            with(KeyCode::Esc, none),
        ];
        for event in events {
            step(&mut shell, &mut hl, event);
        }
    });
}

/// The keys on both paths, with no frame compared.
fn blind(shell: &mut Shell, code: KeyCode) {
    shell.handle_key(key(code));
}

/// Opens the Open hub and moves to the row whose label holds `label`, on
/// both paths. The hub's frames are P75.2's to compare.
fn to_open_row(shell: &mut Shell, label: &str) {
    blind(shell, KeyCode::Char('o'));
    for _ in 0..32 {
        let on = match shell.view().overlay.as_ref() {
            Some(super::tui_view::Overlay::Open(hub)) => hub
                .rows
                .get(hub.selected)
                .is_some_and(|row| row.label.contains(label)),
            _ => panic!("no Open hub"),
        };
        if on {
            return;
        }
        blind(shell, KeyCode::Char('j'));
    }
    panic!("no row {label}");
}

/// Each one-line field takes Backspace and Esc as the App's: search,
/// filter, the commit message, the compare base and the remote PR URL.
#[test]
fn one_line_fields_take_backspace_and_esc_like_the_app() {
    let _golden = super::parity::guard();
    let flag_repo = fixture(1);
    let storage = storage();
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = both_on(&flag_repo, storage.path());
        let mut hl = Highlighter::new();
        let typed = [
            KeyCode::Char('a'),
            KeyCode::Char('b'),
            KeyCode::Backspace,
            KeyCode::Esc,
        ];
        for opener in ['/', 'f'] {
            press(&mut shell, &mut hl, KeyCode::Char(opener));
            for code in typed {
                press(&mut shell, &mut hl, code);
            }
        }
        for label in ["Change compare base", "Open remote PR"] {
            to_open_row(&mut shell, label);
            press(&mut shell, &mut hl, KeyCode::Enter);
            for code in typed {
                press(&mut shell, &mut hl, code);
            }
        }
        press(&mut shell, &mut hl, KeyCode::Char('3'));
        settle(&mut shell, loaded);
        press(&mut shell, &mut hl, KeyCode::Char('c'));
        for code in typed {
            press(&mut shell, &mut hl, code);
        }
    });
}

/// Every AI hub row runs, toggles or opens something: none is left with
/// no route, so the hub has nothing that answers unported.
#[test]
fn every_ai_hub_row_runs_or_toggles() {
    let repo = fixture(1);
    let storage = storage();
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = shell(&args(vec![path_of(&repo)]), storage.path());
        settle(&mut shell, loaded);
        shell.handle_key(key(KeyCode::Char('a')));
        let Some(super::tui_view::Overlay::Picker(picker)) = shell.view().overlay.as_ref() else {
            panic!("no AI hub");
        };
        assert!(picker.rows.len() > 20, "the hub lost its rows");
        assert_eq!(picker.inert_rows(), Vec::<&str>::new());
    });
}

/// Opens the Git hub and moves to the row whose label holds `label`, with
/// each move's frame compared.
fn to_git_row(shell: &mut Shell, hl: &mut Highlighter, label: &str) {
    // The goldens were drawn without the review row.
    blind(shell, KeyCode::Char('g'));
    super::tests::without_review_row(shell);
    for _ in 0..16 {
        let on = match shell.view().overlay.as_ref() {
            Some(super::tui_view::Overlay::GitHub(hub)) => hub
                .rows
                .get(hub.selected)
                .is_some_and(|row| row.label.contains(label)),
            _ => panic!("no Git hub"),
        };
        if on {
            return;
        }
        press(shell, hl, KeyCode::Char('j'));
    }
    panic!("no row {label}");
}

/// The Git hub's stage, stage-all, refresh and push rows act as the App's.
#[test]
fn the_git_hub_working_tree_rows_act_like_the_app() {
    let _golden = super::parity::guard();
    let flag_repo = fixture(2);
    for repo in [&flag_repo] {
        for file in ["src/f000.rs", "src/f001.rs"] {
            std::fs::write(repo.path().join(file), "let unstaged = 1;\n")
                .unwrap_or_else(|e| panic!("{e}"));
        }
    }
    let storage = storage();
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = both_on(&flag_repo, storage.path());
        let mut hl = Highlighter::new();
        press(&mut shell, &mut hl, KeyCode::Char('2'));
        let labels = [
            "Push to remote",
            "Stage current file",
            "Refresh diff",
            "Stage all files",
        ];
        for label in labels {
            to_git_row(&mut shell, &mut hl, label);
            press(&mut shell, &mut hl, KeyCode::Enter);
        }
        press(&mut shell, &mut hl, KeyCode::Char('3'));
        to_git_row(&mut shell, &mut hl, "Push to remote");
        press(&mut shell, &mut hl, KeyCode::Enter);
        press(&mut shell, &mut hl, KeyCode::Esc);
    });
}
