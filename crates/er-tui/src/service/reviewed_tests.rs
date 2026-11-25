//! The flag-on reviewed keys (P70.1): a mark is written by the actor to the
//! tab the key was pressed on, the count comes from the reviewed builder
//! once per version, and mark, jump, unreviewed-only and expand draw and
//! write as the App does.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crossterm::event::KeyCode;

use super::parity::assert_frame;
use super::parity::assert_json;
use super::shell::Shell;
use super::startup::Args;
use super::tests::{
    fixture, git, key, loaded, path_of, service_sources, settle, shell, shell_frame,
};
use crate::ui::highlight::Highlighter;

fn marks_in(er_dir: &str) -> HashMap<String, String> {
    let store = er_engine::store_registry::store();
    let scope = er_store::Scope::dir(er_dir).unwrap_or_else(|e| panic!("{e}"));
    er_engine::session_store::load_reviewed(&store, &scope).unwrap_or_else(|e| panic!("{e}"))
}

fn er_dir(shell: &Shell) -> String {
    let content = shell
        .active_content()
        .unwrap_or_else(|| panic!("no content"));
    content.context.er_dir.clone()
}

/// Ticks until every command the keys sent has answered.
fn settle_replies(shell: &mut Shell) {
    settle(shell, |shell| shell.in_flight() == 0);
}

/// Presses `code` on both paths, the last step's message gone first, and
/// compares the frames once the flag path's replies have landed.
fn step(shell: &mut Shell, hl: &mut Highlighter, code: KeyCode) {
    shell.tick(Instant::now() + Duration::from_secs(60));
    shell.handle_key(key(code));
    settle_replies(shell);
    let frame = shell_frame(shell, hl);
    assert_frame(&format!("{code:?}"), &frame);
}

/// A shell on the fixture, its first tab loaded.
fn both_on(flag_repo: &tempfile::TempDir, config_dir: &std::path::Path) -> Shell {
    let args = Args {
        paths: vec![path_of(flag_repo)],
        ..Args::default()
    };
    let mut shell = shell(&args, config_dir);
    settle(&mut shell, loaded);
    shell
}

/// A mark pressed after a tab switch lands on the tab it was pressed on.
#[test]
fn a_mark_lands_on_the_tab_it_was_pressed_on() {
    let (first, second) = (fixture(1), fixture(1));
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let args = Args {
            paths: vec![path_of(&first), path_of(&second)],
            ..Args::default()
        };
        let mut shell = shell(&args, storage.path());
        settle(&mut shell, loaded);
        let mut hl = Highlighter::new();
        shell_frame(&mut shell, &mut hl);
        let first_dir = er_dir(&shell);
        shell.handle_key(key(KeyCode::Char(']')));
        settle(&mut shell, loaded);
        shell_frame(&mut shell, &mut hl);
        let second_dir = er_dir(&shell);
        shell.handle_key(key(KeyCode::Char(' ')));
        settle_replies(&mut shell);
        assert!(
            marks_in(&second_dir).contains_key("src/f000.rs"),
            "the pressed tab has no mark: {:?}",
            shell.view().notification
        );
        assert!(
            marks_in(&first_dir).is_empty(),
            "the other tab got the mark"
        );
    });
}

/// No flag-on module builds the reviewed resource but the cache, and none
/// counts reviewed files from the marks itself.
#[test]
fn the_reviewed_count_comes_from_the_builder() {
    for (name, text) in &service_sources() {
        let flat: String = text.split_whitespace().collect();
        if name != "reviewed.rs" {
            assert!(
                !flat.contains("reviewed_resource("),
                "{name} builds the count"
            );
        }
        for statement in flat.split(';') {
            let counts = [".count()", ".len()", "+=1"]
                .iter()
                .any(|word| statement.contains(word));
            let reads_marks = statement.contains("contains_key(") || statement.contains("marks");
            assert!(
                !(counts && reads_marks),
                "{name} counts the marks: {statement}"
            );
        }
    }
}

/// Frames with nothing new reuse the count; a toggle builds it once more.
#[test]
fn the_builder_runs_once_per_reviewed_version() {
    let repo = fixture(3);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let args = Args {
            paths: vec![path_of(&repo)],
            ..Args::default()
        };
        let mut shell = shell(&args, storage.path());
        settle(&mut shell, loaded);
        let mut hl = Highlighter::new();
        let mut draw_twenty = |shell: &mut Shell| {
            for _ in 0..20 {
                shell.tick(Instant::now());
                shell_frame(shell, &mut hl);
            }
        };
        draw_twenty(&mut shell);
        assert_eq!(shell.reviewed_builds(), 1, "a frame rebuilt the count");
        shell.handle_key(key(KeyCode::Char(' ')));
        settle_replies(&mut shell);
        draw_twenty(&mut shell);
        assert_eq!(shell.reviewed_builds(), 2, "the toggle did not build once");
    });
}

/// Mark, unmark and the jump write the same marks as the App and draw the
/// same frames, the reviewed count included.
#[test]
fn marks_and_the_jump_match_the_app() {
    let _golden = super::parity::guard();
    let flag_repo = fixture(3);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = both_on(&flag_repo, storage.path());
        let mut hl = Highlighter::new();
        let space = KeyCode::Char(' ');
        let steps = [
            space,
            KeyCode::Char('k'),
            space,
            KeyCode::Char('j'),
            space,
            KeyCode::Char('U'),
            space,
        ];
        for code in steps {
            step(&mut shell, &mut hl, code);
        }
        assert_json("marks", &marks_in(&er_dir(&shell)));
    });
}

/// A mark under unreviewed-only moves the selection where the App's does.
#[test]
fn a_mark_under_unreviewed_only_advances_like_the_app() {
    let _golden = super::parity::guard();
    let flag_repo = fixture(3);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = both_on(&flag_repo, storage.path());
        let mut hl = Highlighter::new();
        for code in [KeyCode::Char('!'), KeyCode::Char('k'), KeyCode::Char(' ')] {
            step(&mut shell, &mut hl, code);
        }
    });
}

/// A fixture whose diff holds a lock file the default patterns compact.
fn with_lock() -> tempfile::TempDir {
    let repo = fixture(1);
    let lock = "dep = 1\n".repeat(30);
    std::fs::write(repo.path().join("Cargo.lock"), lock).unwrap_or_else(|e| panic!("{e}"));
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-q", "-m", "lock"]);
    repo
}

/// Enter on a compacted file expands it through the service.
#[test]
fn enter_expands_a_compacted_file_like_the_app() {
    let _golden = super::parity::guard();
    let flag_repo = with_lock();
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = both_on(&flag_repo, storage.path());
        let compacted = |shell: &Shell| {
            shell
                .active_content()
                .is_some_and(|content| content.diff.files[0].compacted)
        };
        assert!(compacted(&shell), "the lock file loaded expanded");
        let mut hl = Highlighter::new();
        step(&mut shell, &mut hl, KeyCode::Enter);
        assert!(!compacted(&shell), "Enter left the file compacted");
    });
}
