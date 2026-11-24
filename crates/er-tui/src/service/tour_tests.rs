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

use super::shell::Shell;
use super::tests::{fixture, key, rows, shell_frame, SOURCES};
use super::threads_tests::{active_key, both_on, er_dir, state, step};
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

/// Seeds the tour in both buckets; the App reads it at once, and the flag
/// path on its reload tick.
fn seed_tour(app: &mut er_engine::app::App, shell: &mut Shell) {
    let hash = app.tab().branch_diff_hash.clone();
    write_tour(&app.tab().er_dir(), &hash);
    app.tab_mut().reload_ai_state();
    let before = shell
        .active_content()
        .unwrap_or_else(|| panic!("no content"));
    write_tour(&before.context.er_dir, &before.context.branch_diff_hash);
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
}

/// Both paths on their own fixture with the tour seeded, in Tour mode, the
/// frames compared.
fn in_tour(
    app_repo: &tempfile::TempDir,
    flag_repo: &tempfile::TempDir,
    storage: &Path,
    hl: &mut Highlighter,
) -> (er_engine::app::App, Shell) {
    let (mut app, mut shell) = both_on(app_repo, flag_repo, storage);
    seed_tour(&mut app, &mut shell);
    step(&mut app, &mut shell, hl, key(KeyCode::Char('5')));
    assert_eq!(app.tab().mode, DiffMode::Tour);
    let content = shell
        .active_content()
        .unwrap_or_else(|| panic!("no content"));
    assert_eq!(content.tab.mode, DiffMode::Tour);
    (app, shell)
}

fn marks(dir: &str) -> HashMap<String, String> {
    let store = er_engine::store_registry::store();
    let scope = er_store::Scope::dir(dir).unwrap_or_else(|e| panic!("{e}"));
    er_engine::session_store::load_reviewed(&store, &scope).unwrap_or_else(|e| panic!("{e}"))
}

fn assert_same_marks(app: &er_engine::app::App, shell: &Shell, expected: &[&str]) {
    let (app_marks, flag_marks) = (marks(&app.tab().er_dir()), marks(&er_dir(shell)));
    assert_eq!(flag_marks, app_marks);
    let mut paths: Vec<&str> = flag_marks.keys().map(String::as_str).collect();
    paths.sort_unstable();
    assert_eq!(paths, expected);
}

/// The Tour key shows the builder's pillars, the related test nested and
/// the unnamed file under "Other changes", and the pillar, file, line and
/// scroll keys move as the App's do.
#[test]
fn the_tour_draws_and_moves_like_the_app() {
    let (app_repo, flag_repo) = (fixture(4), fixture(4));
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut hl = Highlighter::new();
        let (mut app, mut shell) = in_tour(&app_repo, &flag_repo, storage.path(), &mut hl);
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
            step(&mut app, &mut shell, &mut hl, key(code));
        }
    });
}

/// Space toggles the tour's selected file and `n` moves on, as the App's:
/// the marks saved match.
#[test]
fn the_tour_toggle_marks_like_the_app() {
    let (app_repo, flag_repo) = (fixture(4), fixture(4));
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut hl = Highlighter::new();
        let (mut app, mut shell) = in_tour(&app_repo, &flag_repo, storage.path(), &mut hl);
        for code in [' ', ' ', 'n', ' '] {
            step(&mut app, &mut shell, &mut hl, key(KeyCode::Char(code)));
        }
        assert_same_marks(&app, &shell, &["src/f001.rs"]);
    });
}

/// `b` reviews every file of the pillar, its related test included, as the
/// App's does, and says so.
#[test]
fn a_pillar_review_marks_like_the_app() {
    let (app_repo, flag_repo) = (fixture(4), fixture(4));
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut hl = Highlighter::new();
        let (mut app, mut shell) = in_tour(&app_repo, &flag_repo, storage.path(), &mut hl);
        step(&mut app, &mut shell, &mut hl, key(KeyCode::Char('b')));
        assert_same_marks(&app, &shell, &["src/f000.rs", "src/f001.rs"]);
        assert_eq!(
            shell.view().notification.as_deref(),
            Some("Reviewed all files in pillar")
        );
    });
}

/// Twenty frames with nothing new build the tour's rows no more.
#[test]
fn frames_with_nothing_new_build_the_tour_once() {
    let (app_repo, flag_repo) = (fixture(2), fixture(2));
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut hl = Highlighter::new();
        let (_app, mut shell) = in_tour(&app_repo, &flag_repo, storage.path(), &mut hl);
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
    for (name, text) in SOURCES {
        for needle in ["pillar_ownership", "ordered_pillars", "rebuild_tour_state"] {
            assert!(!text.contains(needle), "{name} names {needle}");
        }
    }
}
