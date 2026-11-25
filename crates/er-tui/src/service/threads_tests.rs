//! The flag-on threads (P67.1): the composer's question and note creates go
//! to the actor of the tab they were written on, keep their text until the
//! reply, and carry a painted id; the threads, layers, the Gate, J and K and
//! the panels draw as on the App path, from the published AI state.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use er_engine::ai::{ErNotes, ErQuestions, ErReview, PrivateAnnotation};
use er_engine::ports::Ports;
use er_engine::test_support::{read_sidecar, write_sidecar_json};
use er_engine::TabKey;

use super::loop_tests::{Gate, GatedGit};
use super::shell::Shell;
use super::startup::Args;
use super::tests::{
    app_on, assert_parity, fixture, key, loaded, path_of, plan, press_app, settle, settle_within,
    shell, shell_frame, shell_on, SOURCES,
};
use super::tui_view::{Mode, TabViewState};
use crate::ui::highlight::Highlighter;

pub(super) fn args(paths: Vec<String>) -> Args {
    Args {
        paths,
        ..Args::default()
    }
}

pub(super) fn active_key(shell: &Shell) -> TabKey {
    let tabs = shell.host().registry().tabs_view();
    tabs.active.unwrap_or_else(|| panic!("no active tab"))
}

pub(super) fn state(shell: &Shell, key: TabKey) -> &TabViewState {
    shell
        .view()
        .tab(key)
        .unwrap_or_else(|| panic!("no view state"))
}

pub(super) fn draft_text(shell: &Shell, key: TabKey) -> String {
    state(shell, key).draft.textarea.lines().join("\n")
}

pub(super) fn er_dir(shell: &Shell) -> String {
    let content = shell
        .active_content()
        .unwrap_or_else(|| panic!("no content"));
    content.context.er_dir.clone()
}

pub(super) fn settle_replies(shell: &mut Shell) {
    settle(shell, |shell| shell.in_flight() == 0);
}

pub(super) fn type_text(shell: &mut Shell, text: &str) {
    for c in text.chars() {
        shell.handle_key(key(KeyCode::Char(c)));
    }
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

/// A directory where the questions file goes, which refuses the write even
/// for root.
fn block_questions(dir: &str) -> std::path::PathBuf {
    let blocked = Path::new(dir).join("questions.json");
    std::fs::create_dir_all(&blocked).unwrap_or_else(|e| panic!("{e}"));
    blocked
}

/// A refused create keeps the composer open with its text and says why;
/// sent again, it writes one row under the id painted on the first submit.
#[test]
fn a_refused_create_keeps_the_composer_and_retries_the_same_row() {
    let repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = shell(&args(vec![path_of(&repo)]), storage.path());
        settle(&mut shell, loaded);
        let tab = active_key(&shell);
        let dir = er_dir(&shell);
        let blocked = block_questions(&dir);
        shell.handle_key(key(KeyCode::Char('q')));
        type_text(&mut shell, "Why this?");
        shell.handle_key(key(KeyCode::Enter));
        let painted = state(&shell, tab).draft.client_id.clone();
        settle_replies(&mut shell);
        assert_eq!(
            shell.view().input_mode,
            Mode::Comment,
            "the composer closed"
        );
        assert_eq!(draft_text(&shell, tab), "Why this?");
        assert!(shell.view().notification.is_some(), "the refusal is silent");

        std::fs::remove_dir(&blocked).unwrap_or_else(|e| panic!("{e}"));
        shell.handle_key(key(KeyCode::Enter));
        settle_replies(&mut shell);
        let rows = read_sidecar::<ErQuestions>(Path::new(&dir)).questions;
        let ids: Vec<&str> = rows.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(ids, [painted.as_deref().unwrap_or("none")], "{ids:?}");
        assert!(painted.is_some_and(|id| id.starts_with("q-")));
        assert_eq!(shell.view().input_mode, Mode::Normal);
        assert_eq!(draft_text(&shell, tab), "");
    });
}

/// Ctrl+T after a refused question paints a note's id, so the note lands
/// under the id the draft holds.
#[test]
fn a_new_kind_paints_a_new_id() {
    let repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = shell(&args(vec![path_of(&repo)]), storage.path());
        settle(&mut shell, loaded);
        let tab = active_key(&shell);
        let dir = er_dir(&shell);
        block_questions(&dir);
        shell.handle_key(key(KeyCode::Char('q')));
        type_text(&mut shell, "Note this");
        shell.handle_key(key(KeyCode::Enter));
        settle_replies(&mut shell);
        shell.handle_key(ctrl('t'));
        shell.handle_key(key(KeyCode::Enter));
        let painted = state(&shell, tab).draft.client_id.clone();
        settle_replies(&mut shell);
        let rows = read_sidecar::<ErNotes>(Path::new(&dir)).notes;
        let ids: Vec<&str> = rows.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(ids, [painted.as_deref().unwrap_or("none")], "{ids:?}");
        assert!(painted.is_some_and(|id| id.starts_with("n-")));
    });
}

/// A create held on the git port lands on the tab it was written on, after
/// a switch: that tab's draft clears, and the other tab's draft stays.
#[test]
fn a_create_lands_on_the_tab_it_was_written_on() {
    let (first, second) = (fixture(1), fixture(1));
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let start = plan(&args(vec![path_of(&first), path_of(&second)]));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let gate = Arc::new(Gate::default());
        let ports = Ports {
            git: Arc::new(GatedGit(Arc::clone(&gate))),
            github: super::tests::fake_github(),
        };
        let mut shell = shell_on(ports, &start, storage.path());
        settle(&mut shell, loaded);
        let (a, a_dir) = (active_key(&shell), er_dir(&shell));
        shell.handle_key(key(KeyCode::Char(']')));
        settle(&mut shell, loaded);
        let (b, b_dir) = (active_key(&shell), er_dir(&shell));
        shell.handle_key(key(KeyCode::Char('[')));

        gate.set(true);
        shell.handle_key(key(KeyCode::Char('q')));
        type_text(&mut shell, "on A");
        shell.handle_key(key(KeyCode::Enter));
        shell.handle_key(key(KeyCode::Tab));
        shell.handle_key(key(KeyCode::Char(']')));
        shell.handle_key(key(KeyCode::Char('q')));
        type_text(&mut shell, "on B");
        shell.handle_key(key(KeyCode::Tab));
        gate.set(false);
        settle_replies(&mut shell);

        let rows = read_sidecar::<ErQuestions>(Path::new(&a_dir)).questions;
        let texts: Vec<&str> = rows.iter().map(|row| row.text.as_str()).collect();
        assert_eq!(texts, ["on A"]);
        assert!(!Path::new(&b_dir).join("questions.json").exists());
        assert_eq!(draft_text(&shell, a), "", "the written tab kept its draft");
        assert_eq!(
            draft_text(&shell, b),
            "on B",
            "the other tab lost its draft"
        );
    });
}

/// The shell on a gated git port, its first tab loaded.
fn gated_shell(repo: &tempfile::TempDir, gate: &Arc<Gate>, config_dir: &Path) -> Shell {
    let ports = Ports {
        git: Arc::new(GatedGit(Arc::clone(gate))),
        github: super::tests::fake_github(),
    };
    let mut shell = shell_on(ports, &plan(&args(vec![path_of(repo)])), config_dir);
    settle(&mut shell, loaded);
    shell
}

/// Enter pressed again while the create is held sends nothing more: the
/// service appends every create it is sent, so one draft makes one row.
#[test]
fn enter_while_a_create_is_in_flight_writes_one_row() {
    let repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let gate = Arc::new(Gate::default());
        let mut shell = gated_shell(&repo, &gate, storage.path());
        let dir = er_dir(&shell);
        shell.handle_key(key(KeyCode::Char('q')));
        type_text(&mut shell, "Once");
        gate.set(true);
        shell.handle_key(key(KeyCode::Enter));
        shell.handle_key(key(KeyCode::Enter));
        gate.set(false);
        settle_replies(&mut shell);
        let rows = read_sidecar::<ErQuestions>(Path::new(&dir)).questions;
        assert_eq!(rows.len(), 1, "{rows:?}");
    });
}

/// A create whose worker dies answers as a refusal: the composer keeps its
/// text, says so, and takes the next Enter.
#[test]
fn a_create_that_never_answers_frees_the_composer() {
    let repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let gate = Arc::new(Gate::default());
        let mut shell = gated_shell(&repo, &gate, storage.path());
        let (tab, dir) = (active_key(&shell), er_dir(&shell));
        shell.handle_key(key(KeyCode::Char('q')));
        type_text(&mut shell, "Lost");
        gate.panics.store(true, std::sync::atomic::Ordering::SeqCst);
        shell.handle_key(key(KeyCode::Enter));
        settle_replies(&mut shell);
        gate.panics
            .store(false, std::sync::atomic::Ordering::SeqCst);
        assert!(state(&shell, tab).sent.is_none(), "the composer is stuck");
        assert_eq!(draft_text(&shell, tab), "Lost");
        let notice = shell.view().notification.clone().unwrap_or_default();
        assert!(notice.contains("stopped"), "{notice:?}");
        shell.handle_key(key(KeyCode::Enter));
        settle_replies(&mut shell);
        let rows = read_sidecar::<ErQuestions>(Path::new(&dir)).questions;
        assert_eq!(rows.len(), 1, "{rows:?}");
    });
}

/// A row's fields, less the ones each write makes fresh.
fn comparable(row: &PrivateAnnotation) -> serde_json::Value {
    let mut value = serde_json::to_value(row).unwrap_or_else(|e| panic!("{e}"));
    if let Some(map) = value.as_object_mut() {
        map.remove("id");
        map.remove("timestamp");
    }
    value
}

/// Presses `event` on both paths, the last message gone first, and compares
/// the frames once the flag path's replies have landed.
pub(super) fn step(
    app: &mut er_engine::app::App,
    shell: &mut Shell,
    hl: &mut Highlighter,
    event: KeyEvent,
) {
    app.clear_notification();
    shell.tick(Instant::now() + Duration::from_secs(60));
    let (tx, _rx) = std::sync::mpsc::channel();
    let mut cache = crate::screen::ViewCache::default();
    crate::handle_key(app, event, &tx, &mut None, &mut cache).unwrap_or_else(|e| panic!("{e}"));
    shell.handle_key(event);
    settle_replies(shell);
    let frame = shell_frame(shell, hl);
    assert_parity(app, &frame, hl, &format!("{event:?}"));
}

/// Waits for the start of a wall-clock second, so two writes a step apart
/// draw the same seconds in their time stamps.
pub(super) fn early_in_a_second() {
    let millis = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.subsec_millis())
    };
    let deadline = Instant::now() + Duration::from_secs(2);
    while millis() > 100 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The App and the flag path, each on its own clone of one fixture.
pub(super) fn both_on(
    app_repo: &tempfile::TempDir,
    flag_repo: &tempfile::TempDir,
    config_dir: &Path,
) -> (er_engine::app::App, Shell) {
    let app = app_on(&path_of(app_repo));
    let mut shell = shell(&args(vec![path_of(flag_repo)]), config_dir);
    settle(&mut shell, loaded);
    (app, shell)
}

/// The composer's keys, a pause and a resume, and the rows they write draw
/// and store as the App's do.
#[test]
fn the_composer_draws_and_writes_like_the_app() {
    let (app_repo, flag_repo) = (fixture(2), fixture(2));
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let (mut app, mut shell) = both_on(&app_repo, &flag_repo, storage.path());
        let mut hl = Highlighter::new();
        let mut keys = vec![key(KeyCode::Down); 3];
        keys.push(key(KeyCode::Char('q')));
        keys.extend("Is this right?".chars().map(|c| key(KeyCode::Char(c))));
        keys.extend([ctrl('t'), key(KeyCode::Tab), key(KeyCode::Tab)]);
        keys.push(key(KeyCode::Enter));
        keys.extend([key(KeyCode::Char('q')), key(KeyCode::Char('a'))]);
        keys.push(key(KeyCode::Enter));
        for event in keys {
            if event.code == KeyCode::Enter {
                early_in_a_second();
            }
            step(&mut app, &mut shell, &mut hl, event);
        }
        let rows = |dir: &str| {
            let dir = Path::new(dir);
            let notes = read_sidecar::<ErNotes>(dir).notes;
            let questions = read_sidecar::<ErQuestions>(dir).questions;
            notes
                .iter()
                .chain(&questions)
                .map(comparable)
                .collect::<Vec<_>>()
        };
        let flag_rows = rows(&er_dir(&shell));
        assert_eq!(flag_rows, rows(&app.tab().er_dir()));
        assert_eq!(flag_rows.len(), 2);
    });
}

/// Seeds an open question, a resolved one with a reply, a note, and a
/// review with an informational and a tentative finding; `graded` adds an
/// arbiter's verdict, which moves the Gate's default.
fn seed(dir: &str, hash: &str, graded: bool, note: &str) {
    let dir = Path::new(dir);
    std::fs::create_dir_all(dir).unwrap_or_else(|e| panic!("{e}"));
    let row = |id: &str, file: &str, line: usize, text: &str, resolved: bool| {
        serde_json::json!({
            "id": id, "file": file, "hunk_index": usize::from(line > 20),
            "line_start": line, "line_content": "c", "text": text, "resolved": resolved
        })
    };
    let mut reply = row("q3", "src/f001.rs", 31, "Answered here", false);
    reply["in_reply_to"] = "q2".into();
    let questions = [
        row("q1", "src/f000.rs", 4, "Open question", false),
        row("q2", "src/f001.rs", 31, "Resolved question", true),
        reply,
    ];
    write_sidecar_json::<ErQuestions>(
        dir,
        serde_json::json!({"version": 1, "diff_hash": hash, "questions": questions}),
    );
    let notes = [row("n1", "src/f000.rs", 31, note, false)];
    write_sidecar_json::<ErNotes>(
        dir,
        serde_json::json!({"version": 1, "diff_hash": hash, "notes": notes}),
    );
    let finding = |id: &str, line: usize, confidence: &str| {
        serde_json::json!({
            "id": id, "severity": "medium", "title": format!("Finding {id}"),
            "hunk_index": 0, "line_start": line, "confidence": confidence
        })
    };
    let findings = [
        finding("f1", 4, "informational"),
        finding("f2", 5, "tentative"),
    ];
    write_sidecar_json::<ErReview>(
        dir,
        serde_json::json!({"version": 1, "diff_hash": hash, "files": {
            "src/f000.rs": {"risk": "medium", "findings": findings}
        }}),
    );
    if graded {
        let id = er_engine::ai::finding_key("src/f000.rs", Some(5), "Finding f2");
        let verdicts = [serde_json::json!({
            "id": id, "file": "src/f000.rs", "verdict": "kept", "confidence": "tentative"
        })];
        write_sidecar_json::<er_engine::ai::ArbiterReview>(
            dir,
            serde_json::json!({"version": 1, "diff_hash": hash, "verdicts": verdicts}),
        );
    }
}

/// Seeds both paths' buckets; the App reads them at once, and the flag path
/// on its reload tick.
pub(super) fn seed_both(
    app: &mut er_engine::app::App,
    shell: &mut Shell,
    graded: bool,
    note: &str,
) {
    let hash = app.tab().branch_diff_hash.clone();
    seed(&app.tab().er_dir(), &hash, graded, note);
    app.tab_mut().reload_ai_state();
    app.clear_notification();
    let before = shell
        .active_content()
        .unwrap_or_else(|| panic!("no content"));
    seed(
        &before.context.er_dir,
        &before.context.branch_diff_hash,
        graded,
        note,
    );
    let moved = |shell: &Shell| {
        shell
            .active_content()
            .is_some_and(|now| now.ai.revision > before.ai.revision)
    };
    // A step's tick may have run the clock ahead, so the reload tick runs
    // on a clock that keeps moving past it.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut ahead = 3600;
    while !moved(shell) && Instant::now() < deadline {
        ahead += 1;
        shell.tick(Instant::now() + Duration::from_secs(ahead));
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        moved(shell),
        "the seeded sidecars never reached the AI section"
    );
    super::tests::let_the_reload_notice_go(shell);
}

/// The threads draw from the published AI state as the App draws them, and
/// the File panel, the layer toggles and J and K act as the App's do.
#[test]
fn threads_layers_and_hints_draw_like_the_app() {
    let (app_repo, flag_repo) = (fixture(3), fixture(3));
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let (mut app, mut shell) = both_on(&app_repo, &flag_repo, storage.path());
        seed_both(&mut app, &mut shell, false, "A note");
        let mut hl = Highlighter::new();
        let frame = shell_frame(&mut shell, &mut hl);
        assert_parity(&app, &frame, &mut hl, "seeded");
        let codes = [
            KeyCode::Char('p'),
            KeyCode::Tab,
            KeyCode::Esc,
            KeyCode::Char('C'),
            KeyCode::Char('Q'),
            KeyCode::Char('Q'),
            KeyCode::Char('X'),
            KeyCode::Char('A'),
            KeyCode::Char('A'),
            KeyCode::Char('K'),
            KeyCode::Char('K'),
            KeyCode::Char('K'),
            KeyCode::Char('K'),
            KeyCode::Char('J'),
            KeyCode::Char('P'),
        ];
        for code in codes {
            step(&mut app, &mut shell, &mut hl, key(code));
        }
    });
}

/// The Gate follows a new arbiter grade on reload, as the App's does, until
/// T sets it by hand.
#[test]
fn the_gate_follows_the_review_like_the_app() {
    let (app_repo, flag_repo) = (fixture(1), fixture(1));
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let (mut app, mut shell) = both_on(&app_repo, &flag_repo, storage.path());
        seed_both(&mut app, &mut shell, false, "A note");
        let mut hl = Highlighter::new();
        let frame = shell_frame(&mut shell, &mut hl);
        assert_parity(&app, &frame, &mut hl, "ungraded");
        seed_both(&mut app, &mut shell, true, "A note");
        assert_eq!(
            app.tab().layers.min_trust,
            er_engine::ai::Confidence::Tentative,
            "the arbiter's grade did not move the App's Gate: {:?}",
            app.tab().ai.arbiter_effect
        );
        let frame = shell_frame(&mut shell, &mut hl);
        assert_parity(&app, &frame, &mut hl, "graded");
        for _ in 0..2 {
            step(&mut app, &mut shell, &mut hl, key(KeyCode::Char('T')));
        }
        // A Gate set by hand holds through the next AI revision.
        seed_both(&mut app, &mut shell, true, "A later note");
        let informational = er_engine::ai::Confidence::Informational;
        assert_eq!(app.tab().layers.min_trust, informational);
        let tab = active_key(&shell);
        assert_eq!(state(&shell, tab).layers.min_trust, informational);
        let frame = shell_frame(&mut shell, &mut hl);
        assert_parity(&app, &frame, &mut hl, "pinned");
    });
}

/// K onto a question on a file still a stub scrolls to its hunk once the
/// file parses, where the App's in-place parse scrolls.
#[test]
fn a_hint_on_a_stub_scrolls_once_it_parses() {
    let (app_repo, flag_repo) = (fixture(520), fixture(520));
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let (mut app, mut shell) = both_on(&app_repo, &flag_repo, storage.path());
        seed_both(&mut app, &mut shell, false, "A note");
        let stub = |shell: &Shell| {
            shell
                .active_content()
                .is_some_and(|content| super::tui_view::is_stub(&content.diff.files[1]))
        };
        assert!(stub(&shell), "the hinted file loaded parsed");
        for _ in 0..2 {
            press_app(&mut app, KeyCode::Char('K'));
            shell.handle_key(key(KeyCode::Char('K')));
        }
        app.clear_notification();
        settle_within(&mut shell, Duration::from_secs(5), |shell| !stub(shell));
        let mut hl = Highlighter::new();
        let frame = shell_frame(&mut shell, &mut hl);
        assert_parity(&app, &frame, &mut hl, "parsed");
    });
}

/// No layer, Gate or panel key reaches the service, and no flag-on module
/// orders threads itself: the order is the AI state's.
#[test]
fn layers_stay_in_the_view_and_threads_keep_the_engine_order() {
    let source = |file: &str| {
        SOURCES
            .iter()
            .find(|(name, _)| *name == file)
            .map_or("", |(_, text)| text)
    };
    let shell_rs = source("shell.rs");
    let arms = shell_rs
        .split("A::TogglePanel |")
        .nth(1)
        .and_then(|rest| rest.split("// `filter::normal` took these above.").next())
        .unwrap_or_else(|| panic!("the layer arms moved"));
    assert!(arms.contains("layers::cycle_gate"), "the layer arms moved");
    for (name, text) in [
        ("shell.rs's layer arms", arms),
        ("layers.rs", source("layers.rs")),
    ] {
        for word in [".post(", "Command::", "host", ".send("] {
            assert!(!text.contains(word), "{name} names {word}");
        }
    }
    for (name, text) in SOURCES {
        let flat: String = text.split_whitespace().collect();
        for word in [
            "sort_by(",
            "sort_by_key(",
            "sort_unstable",
            ".sort(",
            "in_reply_to",
        ] {
            assert!(!flat.contains(word), "{name} names {word}");
        }
    }
}
