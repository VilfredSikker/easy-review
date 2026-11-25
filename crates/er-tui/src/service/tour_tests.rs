//! The flag-on tour (P72.2): the Tour key, the pillars and files from the
//! service's tour builder, the tour's moves, and Space and `b` through the
//! reviewed writes, beside the App path.

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use crossterm::event::KeyCode;
use er_engine::ai::ErTour;
use er_engine::tab_identity::DiffMode;
use er_engine::test_support::write_sidecar_json;

use super::parity::assert_json;
use super::shell::Shell;
use super::tests::{
    fixture, git, key, loaded, path_of, rows, service_sources, settle, shell, shell_frame,
};
use super::threads_tests::{active_key, args, both_on, er_dir, settle_replies, state, step};
use crate::ui::highlight::Highlighter;

/// A foundation pillar with `f000` and its related test `f001`, a second
/// pillar naming `f002` and `f000` again, and `f003` named by none.
fn write_tour(dir: &str, hash: &str) {
    let dir = Path::new(dir);
    std::fs::create_dir_all(dir).unwrap_or_else(|e| panic!("{e}"));
    let core = serde_json::json!({
        "id": "p-core", "title": "Core", "foundation": true, "importance": 80,
        "description": "Starts at [f000](src/f000.rs) and its test.",
        "files": [{"path": "src/f000.rs", "related": [{"path": "src/f001.rs", "kind": "test"}]}]
    });
    let edge = serde_json::json!({
        "id": "p-edge", "title": "Edge", "importance": 40, "description": "The edges.",
        "files": [{"path": "src/f002.rs"}, {"path": "src/f000.rs"}]
    });
    write_sidecar_json::<ErTour>(
        dir,
        serde_json::json!({"version": 1, "diff_hash": hash, "pillars": [core, edge]}),
    );
}

/// Seeds the tour in the bucket, and waits for the reload tick.
fn seed_tour(shell: &mut Shell) {
    let before = shell
        .active_content()
        .unwrap_or_else(|| panic!("no content"));
    write_tour(&before.context.er_dir, &before.context.branch_diff_hash);
    wait_for_tour(shell);
}

/// Ticks the reload until the AI section shows a tour.
fn wait_for_tour(shell: &mut Shell) {
    let shown = |shell: &Shell| {
        shell
            .active_content()
            .is_some_and(|now| now.ai.state.tour.is_some())
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut ahead = 3600;
    while !shown(shell) && Instant::now() < deadline {
        ahead += 1;
        shell.tick(Instant::now() + Duration::from_secs(ahead));
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(shown(shell), "the seeded tour never reached the AI section");
    super::tests::let_the_reload_notice_go(shell);
}

/// A shell with the tour seeded, on a line of the diff and then in Tour
/// mode, the frames checked.
fn in_tour(flag_repo: &tempfile::TempDir, storage: &Path, hl: &mut Highlighter) -> Shell {
    let mut shell = both_on(flag_repo, storage);
    seed_tour(&mut shell);
    // A line cursor the Tour drops, so leaving it shows where it went.
    for code in [KeyCode::Down, KeyCode::Down, KeyCode::Char('5')] {
        step(&mut shell, hl, key(code));
    }
    let content = shell
        .active_content()
        .unwrap_or_else(|| panic!("no content"));
    assert_eq!(content.tab.mode, DiffMode::Tour);
    shell
}

fn marks(dir: &str) -> HashMap<String, String> {
    let store = er_engine::store_registry::store();
    let scope = er_store::Scope::dir(dir).unwrap_or_else(|e| panic!("{e}"));
    er_engine::session_store::load_reviewed(&store, &scope).unwrap_or_else(|e| panic!("{e}"))
}

fn assert_same_marks(shell: &Shell, expected: &[&str]) {
    let flag_marks = marks(&er_dir(shell));
    assert_json("marks", &flag_marks);
    let mut paths: Vec<&str> = flag_marks.keys().map(String::as_str).collect();
    paths.sort_unstable();
    assert_eq!(paths, expected);
}

/// The Tour key shows the builder's pillars, the related test nested and
/// the unnamed file under "Other changes", and the pillar, file, line and
/// scroll keys move as the App's do.
#[test]
fn the_tour_draws_and_moves_like_the_app() {
    let _golden = super::parity::guard();
    let flag_repo = fixture(4);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut hl = Highlighter::new();
        let mut shell = in_tour(&flag_repo, storage.path(), &mut hl);
        let text = rows(&shell_frame(&mut shell, &mut hl)).join("\n");
        assert!(text.contains("Other changes"), "{text}");
        assert!(text.contains("↳"), "{text}");
        let codes = [
            KeyCode::Char('k'),
            KeyCode::Char('n'),
            KeyCode::Down,
            KeyCode::Down,
            KeyCode::Char('d'),
            KeyCode::Char('j'),
            KeyCode::Char('k'),
            KeyCode::Char('k'),
            KeyCode::Char('N'),
            KeyCode::Up,
            KeyCode::Char('u'),
            KeyCode::Char('l'),
            KeyCode::Char('1'),
        ];
        for code in codes {
            step(&mut shell, &mut hl, key(code));
        }
    });
}

/// The tour's horizontal scroll keys and its page keys draw as the App's.
#[test]
fn the_tour_scroll_keys_draw_like_the_app() {
    let _golden = super::parity::guard();
    let flag_repo = fixture(4);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut hl = Highlighter::new();
        let mut shell = in_tour(&flag_repo, storage.path(), &mut hl);
        let codes = [
            KeyCode::Char('l'),
            KeyCode::Right,
            KeyCode::Char('h'),
            KeyCode::Left,
            KeyCode::Char('l'),
            KeyCode::Home,
            KeyCode::PageDown,
            KeyCode::PageUp,
        ];
        for code in codes {
            step(&mut shell, &mut hl, key(code));
        }
    });
}

/// Space toggles the tour's selected file and `n` moves on, as the App's:
/// the marks saved match.
#[test]
fn the_tour_toggle_marks_like_the_app() {
    let _golden = super::parity::guard();
    let flag_repo = fixture(4);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut hl = Highlighter::new();
        let mut shell = in_tour(&flag_repo, storage.path(), &mut hl);
        for code in [' ', ' ', 'n', ' '] {
            step(&mut shell, &mut hl, key(KeyCode::Char(code)));
        }
        assert_same_marks(&shell, &["src/f001.rs"]);
    });
}

/// `b` reviews every file of the pillar, its related test included, as the
/// App's does, and says so.
#[test]
fn a_pillar_review_marks_like_the_app() {
    let _golden = super::parity::guard();
    let flag_repo = fixture(4);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut hl = Highlighter::new();
        let mut shell = in_tour(&flag_repo, storage.path(), &mut hl);
        step(&mut shell, &mut hl, key(KeyCode::Char('b')));
        assert_same_marks(&shell, &["src/f000.rs", "src/f001.rs"]);
        assert_eq!(
            shell.view().notification.as_deref(),
            Some("Reviewed all files in pillar")
        );
    });
}

/// Twenty frames with nothing new build the tour's rows no more.
#[test]
fn frames_with_nothing_new_build_the_tour_once() {
    let _golden = super::parity::guard();
    let flag_repo = fixture(2);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut hl = Highlighter::new();
        let mut shell = in_tour(&flag_repo, storage.path(), &mut hl);
        let tab = active_key(&shell);
        let before = state(&shell, tab).tour.builds;
        assert!(before >= 1, "the tour was never built");
        for _ in 0..20 {
            let frame = shell_frame(&mut shell, &mut hl);
            assert!(rows(&frame).join("\n").contains("TOUR"));
        }
        assert_eq!(state(&shell, tab).tour.builds, before);
    });
}

/// No flag-on module orders or assigns the pillars itself, or rebuilds the
/// App's tour: the rows come from the service's tour builder.
#[test]
fn flag_on_modules_build_no_tour_of_their_own() {
    for (name, text) in &service_sources() {
        for needle in ["pillar_ownership", "ordered_pillars", "rebuild_tour_state"] {
            assert!(!text.contains(needle), "{name} names {needle}");
        }
    }
}

/// A refresh whose diff moves the files under the tour puts the place on the
/// pillar's first file, as the App's rebuild does. The load, the diff
/// revision and the ranges each start it again. With `P1` naming `f000`
/// and `f001`, and `P2` naming `f002` and `f003`, the diff going from
/// `[f000, f002, f003]` to `[f000, f001, f003]` keeps the row count and the
/// pillars, and the selected row would name a file of the other pillar.
#[test]
fn a_new_diff_puts_the_tour_on_its_pillars_first_file() {
    let repo = fixture(4);
    let root = repo.path();
    git(root, &["checkout", "main", "--", "src/f001.rs"]);
    git(root, &["commit", "-qam", "keep f001"]);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = shell(&args(vec![path_of(&repo)]), storage.path());
        settle(&mut shell, loaded);
        let pillar = |id: &str, a: &str, b: &str| serde_json::json!({"id": id, "title": id, "files": [{"path": a}, {"path": b}]});
        let pillars = [
            pillar("P1", "src/f000.rs", "src/f001.rs"),
            pillar("P2", "src/f002.rs", "src/f003.rs"),
        ];
        write_sidecar_json::<ErTour>(
            Path::new(&er_dir(&shell)),
            serde_json::json!({"version": 1, "diff_hash": "", "pillars": pillars}),
        );
        wait_for_tour(&mut shell);
        for code in ['5', 'n'] {
            shell.handle_key(key(KeyCode::Char(code)));
            settle_replies(&mut shell);
        }
        let tab = active_key(&shell);
        let selected = |shell: &mut Shell| {
            shell_frame(shell, &mut Highlighter::new());
            let content = shell
                .active_content()
                .unwrap_or_else(|| panic!("no content"));
            let tour = &state(shell, tab).tour;
            let view = super::tour::view(tour, &content).unwrap_or_else(|| panic!("no tour"));
            let path = view.files.get(view.selected_file).map(|f| f.path.clone());
            let range = view.pillar_file_ranges.get(view.selected_pillar).copied();
            (view.selected_pillar, view.selected_file, path, range)
        };
        assert_eq!(
            selected(&mut shell),
            (1, 1, Some("src/f002.rs".to_string()), Some((1, 3)))
        );

        git(root, &["checkout", "main", "--", "src/f002.rs"]);
        std::fs::write(root.join("src/f001.rs"), "let moved = 1;\n")
            .unwrap_or_else(|e| panic!("{e}"));
        git(root, &["commit", "-qam", "move"]);
        shell.handle_key(key(KeyCode::Char('R')));
        settle_replies(&mut shell);
        assert_eq!(
            selected(&mut shell),
            (1, 2, Some("src/f003.rs".to_string()), Some((2, 3)))
        );
    });
}
