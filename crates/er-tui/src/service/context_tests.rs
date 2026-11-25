//! P71.3 on the flag path: the context-line keys and the automatic pick.

use std::time::{Duration, Instant};

use crossterm::event::KeyCode;
use er_engine::app::App;

use super::shell::Shell;
use super::startup::Args;
use super::tests::{
    app_on, assert_parity, fixture, key, loaded, path_of, press_app, settle, shell, shell_frame,
    SOURCES,
};
use crate::ui::highlight::Highlighter;

/// One key on both paths, then frame parity.
fn step(app: &mut App, shell: &mut Shell, hl: &mut Highlighter, code: KeyCode) {
    app.clear_notification();
    shell.tick(Instant::now() + Duration::from_secs(60));
    press_app(app, code);
    shell.handle_key(key(code));
    settle(shell, |shell| shell.in_flight() == 0);
    let frame = shell_frame(shell, hl);
    assert_parity(app, &frame, hl, &format!("{code:?}"));
}

/// `keys` on two clones, with the automatic pick at `threshold` on both.
fn parity(threshold: usize, keys: &str) {
    let (app_repo, flag_repo) = (fixture(3), fixture(3));
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut app = app_on(&path_of(&app_repo));
        app.config.display.auto_context_threshold = threshold;
        let args = Args {
            paths: vec![path_of(&flag_repo)],
            ..Args::default()
        };
        let mut shell = shell(&args, storage.path());
        shell.auto_context = Some(threshold);
        settle(&mut shell, loaded);
        let mut hl = Highlighter::new();
        for code in keys.chars() {
            step(&mut app, &mut shell, &mut hl, KeyCode::Char(code));
        }
    });
}

/// `+` and `-` step the App's counts, per file, to full context and back.
#[test]
fn context_keys_draw_like_the_app() {
    let _golden = super::parity::guard();
    parity(0, "-++j+k+++-=---");
}

/// A file a move selects takes the App's automatic count.
#[test]
fn the_automatic_pick_draws_like_the_app() {
    let _golden = super::parity::guard();
    parity(100, "jjk-j");
}

/// The counts come from `er_engine::git`'s rule and the service fetches the
/// file: no flag-on module fetches or measures one itself.
#[test]
fn the_service_fetches_the_context() {
    for (name, text) in SOURCES {
        let flat: String = text.split_whitespace().collect();
        for needle in [
            "refetch_file_with_context",
            "pick_context_for_size",
            "SIZE_LADDER",
            "CONTEXT_STEPS",
            "diff_raw_file",
        ] {
            assert!(!flat.contains(needle), "{name} names {needle}");
        }
    }
}
