//! P71.3 on the flag path: the context-line keys and the automatic pick.

use std::time::{Duration, Instant};

use crossterm::event::KeyCode;

use super::parity::assert_frame;
use super::shell::Shell;
use super::startup::Args;
use super::tests::{fixture, key, loaded, path_of, service_sources, settle, shell, shell_frame};
use crate::ui::highlight::Highlighter;

/// One key, then the frame against its golden.
fn step(shell: &mut Shell, hl: &mut Highlighter, code: KeyCode) {
    shell.tick(Instant::now() + Duration::from_secs(60));
    shell.handle_key(key(code));
    settle(shell, |shell| shell.in_flight() == 0);
    let frame = shell_frame(shell, hl);
    assert_frame(&format!("{code:?}"), &frame);
}

/// `keys`, with the automatic pick at `threshold`.
fn parity(threshold: usize, keys: &str) {
    let flag_repo = fixture(3);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let args = Args {
            paths: vec![path_of(&flag_repo)],
            ..Args::default()
        };
        let mut shell = shell(&args, storage.path());
        shell.auto_context = Some(threshold);
        settle(&mut shell, loaded);
        let mut hl = Highlighter::new();
        for code in keys.chars() {
            step(&mut shell, &mut hl, KeyCode::Char(code));
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
    for (name, text) in &service_sources() {
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
