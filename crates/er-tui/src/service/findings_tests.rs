//! The flag-on findings (P72.1): the AI Summary panel and its checklist
//! toggle, the finding focus, and the finding reply draw and act as on the
//! App path, from the published AI state. Unlike the App, the finding steps
//! skip what the Gate hides.

use std::path::Path;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use er_engine::ai::{ErChecklist, ErGithubComments, ErReview};
use er_engine::test_support::{read_sidecar, write_sidecar_json};

use super::shell::Shell;
use super::tests::{
    assert_parity, fixture, key, loaded, path_of, settle, settle_within, shell, shell_frame,
    SOURCES,
};
use super::threads_tests::{
    active_key, args, both_on, draft_text, er_dir, settle_replies, state, step, type_text,
};
use super::tui_view::Mode;
use crate::ui::highlight::Highlighter;

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn finding(id: &str, hunk: usize, line: usize, confidence: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id, "severity": "medium", "title": format!("Finding {id}"),
        "hunk_index": hunk, "line_start": line, "confidence": confidence
    })
}

fn write_review(dir: &str, hash: &str, files: serde_json::Value) {
    let dir = Path::new(dir);
    std::fs::create_dir_all(dir).unwrap_or_else(|e| panic!("{e}"));
    write_sidecar_json::<ErReview>(
        dir,
        serde_json::json!({"version": 1, "diff_hash": hash, "files": files}),
    );
}

/// A review over the first three files at three risks, a summary, and a
/// checklist of three rows; `checked` is the first row's flag.
fn seed_summary(dir: &str, hash: &str, checked: bool) {
    write_review(
        dir,
        hash,
        serde_json::json!({
            "src/f000.rs": {"risk": "high", "findings": [finding("f1", 0, 4, "informational")]},
            "src/f001.rs": {"risk": "medium", "findings": []},
            "src/f002.rs": {"risk": "low", "findings": [finding("f2", 1, 31, "informational")]}
        }),
    );
    let dir = Path::new(dir);
    std::fs::write(dir.join("summary.md"), "Reads the audience early.\n")
        .unwrap_or_else(|e| panic!("{e}"));
    let item = |id: &str, file: &str, checked: bool| {
        serde_json::json!({
            "id": id, "text": format!("Check {id}"), "checked": checked,
            "related_files": [file]
        })
    };
    let items = [
        item("c1", "src/f000.rs", checked),
        item("c2", "src/f002.rs", false),
        item("c3", "src/f001.rs", true),
    ];
    write_sidecar_json::<ErChecklist>(
        dir,
        serde_json::json!({"version": 1, "diff_hash": hash, "items": items}),
    );
}

/// Findings on the first file's two hunks, and one on the second file.
fn seed_findings(dir: &str, hash: &str) {
    write_review(
        dir,
        hash,
        serde_json::json!({
            "src/f000.rs": {"risk": "high", "findings": [
                finding("f1", 0, 4, "informational"),
                finding("f2", 1, 31, "informational")
            ]},
            "src/f001.rs": {"risk": "medium", "findings": [finding("f3", 0, 4, "informational")]}
        }),
    );
}

/// One finding below the Gate once T raises it to Tentative, beside two
/// that stay.
fn seed_gated(dir: &str, hash: &str) {
    write_review(
        dir,
        hash,
        serde_json::json!({
            "src/f000.rs": {"risk": "high", "findings": [
                finding("low", 0, 4, "informational"),
                finding("kept", 0, 5, "tentative"),
                finding("sure", 1, 31, "confirmed")
            ]}
        }),
    );
}

/// Seeds the shell's active tab with `seed`, and waits for its AI section
/// to move, on a clock the reload tick runs on.
pub(super) fn seed_shell(shell: &mut Shell, seed: impl Fn(&str, &str)) {
    let before = shell
        .active_content()
        .unwrap_or_else(|| panic!("no content"));
    seed(&before.context.er_dir, &before.context.branch_diff_hash);
    let moved = |shell: &Shell| {
        shell
            .active_content()
            .is_some_and(|now| now.ai.revision > before.ai.revision)
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut ahead = 3600;
    while !moved(shell) && Instant::now() < deadline {
        ahead += 1;
        shell.tick(Instant::now() + Duration::from_secs(ahead));
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(moved(shell), "the seeded sidecars never reached the shell");
}

/// Seeds both paths with `seed`; the App reads it at once.
pub(super) fn seed_both(
    app: &mut er_engine::app::App,
    shell: &mut Shell,
    seed: impl Fn(&str, &str),
) {
    let hash = app.tab().branch_diff_hash.clone();
    seed(&app.tab().er_dir(), &hash);
    app.tab_mut().reload_ai_state();
    app.clear_notification();
    seed_shell(shell, seed);
}

fn checked_flags(dir: &str) -> Vec<bool> {
    read_sidecar::<ErChecklist>(Path::new(dir))
        .items
        .iter()
        .map(|item| item.checked)
        .collect()
}

/// The AI Summary panel draws from the published AI state as the App draws
/// it; its cursor, its columns, Space on each column, the scrolls and Enter
/// act as the App's do, and the checklist on disk matches.
#[test]
fn the_ai_summary_panel_draws_and_acts_like_the_app() {
    let (app_repo, flag_repo) = (fixture(3), fixture(3));
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let (mut app, mut shell) = both_on(&app_repo, &flag_repo, storage.path());
        seed_both(&mut app, &mut shell, |dir, hash| {
            seed_summary(dir, hash, false);
        });
        let mut hl = Highlighter::new();
        let codes = [
            KeyCode::Char('p'),
            KeyCode::Char('p'),
            KeyCode::Tab,
            KeyCode::Char('k'),
            KeyCode::Char('k'),
            KeyCode::Char('j'),
            // Space on the files column writes nothing.
            KeyCode::Char(' '),
            KeyCode::Tab,
            KeyCode::Char('k'),
            KeyCode::Char(' '),
            KeyCode::Char('d'),
            KeyCode::Char('u'),
            KeyCode::Enter,
            KeyCode::Esc,
        ];
        for code in codes {
            step(&mut app, &mut shell, &mut hl, key(code));
        }
        let flag_dir = er_dir(&shell);
        assert_eq!(checked_flags(&flag_dir), [false, true, true]);
        assert_eq!(checked_flags(&app.tab().er_dir()), checked_flags(&flag_dir));
    });
}

/// Ctrl+j/k across two files, one a stub until the jump parses it, the File
/// panel's j/k, Enter and Esc focus and draw as the App's do.
#[test]
fn the_finding_focus_draws_like_the_app() {
    let (app_repo, flag_repo) = (fixture(520), fixture(520));
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let (mut app, mut shell) = both_on(&app_repo, &flag_repo, storage.path());
        let stub = |shell: &Shell, idx: usize| {
            shell
                .active_content()
                .is_some_and(|content| super::tui_view::is_stub(&content.diff.files[idx]))
        };
        settle_within(&mut shell, Duration::from_secs(5), |shell| !stub(shell, 0));
        seed_both(&mut app, &mut shell, seed_findings);
        assert!(stub(&shell, 1), "the second file loaded parsed");
        let mut hl = Highlighter::new();
        let mut keys = vec![ctrl('k'); 3];
        keys.push(ctrl('j'));
        keys.extend(
            [
                KeyCode::Char('p'),
                KeyCode::Tab,
                KeyCode::Char('j'),
                KeyCode::Char('k'),
                KeyCode::Char('k'),
                KeyCode::Enter,
                KeyCode::Tab,
                KeyCode::Esc,
            ]
            .map(key),
        );
        for event in keys {
            step(&mut app, &mut shell, &mut hl, event);
            if event == ctrl('k') {
                settle_within(&mut shell, Duration::from_secs(5), |shell| !stub(shell, 1));
                let frame = shell_frame(&mut shell, &mut hl);
                assert_parity(&app, &frame, &mut hl, "parsed");
            }
        }
    });
}

/// The finding focus a key left on the flag path.
fn focused(shell: &Shell) -> Option<String> {
    state(shell, active_key(shell)).focused_finding_id.clone()
}

/// With the Gate raised past a finding, Ctrl+j/k and the File panel's j/k
/// never land on it on the flag path, where the App's Ctrl+k does.
#[test]
fn finding_steps_skip_what_the_gate_hides() {
    let (app_repo, flag_repo) = (fixture(1), fixture(1));
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let (mut app, mut shell) = both_on(&app_repo, &flag_repo, storage.path());
        seed_both(&mut app, &mut shell, seed_gated);
        let mut hl = Highlighter::new();
        step(&mut app, &mut shell, &mut hl, key(KeyCode::Char('T')));
        let (mut flag, mut app_seen) = (Vec::new(), Vec::new());
        for _ in 0..3 {
            shell.handle_key(ctrl('k'));
            let (tx, _rx) = std::sync::mpsc::channel();
            let mut cache = crate::screen::ViewCache::default();
            crate::handle_key(&mut app, ctrl('k'), &tx, &mut None, &mut cache)
                .unwrap_or_else(|e| panic!("{e}"));
            flag.extend(focused(&shell));
            app_seen.extend(app.tab().focused_finding_id.clone());
        }
        assert_eq!(flag, ["sure", "kept", "sure"]);
        assert!(app_seen.iter().any(|id| id == "low"), "{app_seen:?}");

        shell.handle_key(key(KeyCode::Char('p')));
        shell.handle_key(key(KeyCode::Tab));
        let mut panel = Vec::new();
        for _ in 0..3 {
            shell.handle_key(key(KeyCode::Char('j')));
            panel.extend(focused(&shell));
        }
        assert_eq!(panel, ["kept", "sure", "kept"]);
    });
}

/// `r` on a focused finding opens a reply whose submit writes the finding's
/// thread: a root that names the finding under the painted id, then the
/// reply (ruling 5 on #653).
#[test]
fn a_finding_reply_writes_a_root_and_the_reply() {
    let repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = shell(&args(vec![path_of(&repo)]), storage.path());
        settle(&mut shell, loaded);
        seed_shell(&mut shell, seed_findings);
        let tab = active_key(&shell);
        shell.handle_key(ctrl('k'));
        shell.handle_key(key(KeyCode::Char('r')));
        assert_eq!(shell.view().input_mode, Mode::Comment);
        type_text(&mut shell, "Looks wrong");
        shell.handle_key(key(KeyCode::Enter));
        let painted = state(&shell, tab).draft.client_id.clone();
        settle_replies(&mut shell);

        let content = shell
            .active_content()
            .unwrap_or_else(|| panic!("no content"));
        let rows =
            read_sidecar::<ErGithubComments>(Path::new(&content.context.comments_dir)).comments;
        let shape: Vec<_> = rows
            .iter()
            .map(|row| {
                (
                    row.comment.as_str(),
                    row.finding_ref.as_deref(),
                    row.in_reply_to.is_some(),
                )
            })
            .collect();
        assert_eq!(
            shape,
            [
                ("Follow-up on this finding.", Some("f2"), false),
                ("Looks wrong", None, true),
            ]
        );
        assert_eq!(Some(&rows[0].id), painted.as_ref());
        assert!(painted.is_some_and(|id| id.starts_with("c-")));
        assert_eq!(rows[1].in_reply_to.as_ref(), Some(&rows[0].id));
        assert_eq!(shell.view().input_mode, Mode::Normal);
        assert_eq!(draft_text(&shell, tab), "");

        // The reply moved the AI section, and the finding is still there, so
        // its focus stays; a review without it drops the focus.
        let mut hl = Highlighter::new();
        shell_frame(&mut shell, &mut hl);
        assert_eq!(focused(&shell).as_deref(), Some("f2"));
        seed_shell(&mut shell, |dir, hash| {
            let only = serde_json::json!({"src/f000.rs": {"risk": "high", "findings": [
                finding("f1", 0, 4, "informational")
            ]}});
            write_review(dir, hash, only);
        });
        shell_frame(&mut shell, &mut hl);
        assert_eq!(focused(&shell), None);
    });
}

/// A checklist toggle lands on the tab it was pressed on, and its notice
/// reads the row as it stood at the press, though another tab is active
/// when the reply lands.
#[test]
fn a_checklist_toggle_lands_on_its_tab_with_the_notice_of_the_press() {
    let (first, second) = (fixture(3), fixture(3));
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = shell(
            &args(vec![path_of(&first), path_of(&second)]),
            storage.path(),
        );
        settle(&mut shell, loaded);
        seed_shell(&mut shell, |dir, hash| seed_summary(dir, hash, false));
        let a_dir = er_dir(&shell);
        shell.handle_key(key(KeyCode::Char(']')));
        settle(&mut shell, loaded);
        seed_shell(&mut shell, |dir, hash| seed_summary(dir, hash, true));
        let b_dir = er_dir(&shell);
        shell.handle_key(key(KeyCode::Char('[')));
        settle(&mut shell, loaded);

        let codes = [
            KeyCode::Char('p'),
            KeyCode::Char('p'),
            KeyCode::Tab,
            KeyCode::Tab,
            KeyCode::Char(' '),
            KeyCode::Esc,
            KeyCode::Char(']'),
        ];
        for code in codes {
            shell.handle_key(key(code));
        }
        settle_replies(&mut shell);
        assert_eq!(
            shell.view().notification.as_deref(),
            Some(super::findings::checklist_notice(true))
        );
        assert_eq!(checked_flags(&a_dir), [true, false, true]);
        assert_eq!(checked_flags(&b_dir), [true, false, true]);
    });
}

/// No flag-on module writes the checklist itself or compares a finding's
/// grade with the Gate: the toggle is the service's, and the Gate's rule is
/// `Finding::passes`.
#[test]
fn findings_keep_the_checklist_write_and_the_gate_rule_out_of_the_tui() {
    let words = [
        "checklist_store",
        "set_checked",
        "toggle_checked",
        "toggle_checklist_item_at",
        "trust_rank(",
    ];
    for (name, text) in SOURCES {
        let flat: String = text.split_whitespace().collect();
        for word in words {
            assert!(!flat.contains(word), "{name} names {word}");
        }
        for line in text.lines() {
            let code = line.split("//").next().unwrap_or_default();
            let bare = code.replace("=>", "").replace("->", "");
            let compares = ["<", ">", "==", "!="].iter().any(|op| bare.contains(op));
            let gate = ["min_trust", "Confidence::"]
                .iter()
                .any(|word| bare.contains(word));
            assert!(
                !(compares && gate),
                "{name} compares a grade with the Gate: {line}"
            );
        }
    }
}

/// Opens a finding reply on the second finding and types its body.
fn compose_finding_reply(shell: &mut Shell) {
    shell.handle_key(ctrl('k'));
    shell.handle_key(key(KeyCode::Char('r')));
    type_text(shell, "Looks wrong");
}

/// Enter pressed again while a finding reply is in flight sends nothing
/// more: one root and one reply land.
#[test]
fn enter_while_a_finding_reply_is_in_flight_writes_one_thread() {
    let repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = shell(&args(vec![path_of(&repo)]), storage.path());
        settle(&mut shell, loaded);
        seed_shell(&mut shell, seed_findings);
        compose_finding_reply(&mut shell);
        shell.handle_key(key(KeyCode::Enter));
        shell.handle_key(key(KeyCode::Enter));
        settle_replies(&mut shell);
        let content = shell
            .active_content()
            .unwrap_or_else(|| panic!("no content"));
        let rows =
            read_sidecar::<ErGithubComments>(Path::new(&content.context.comments_dir)).comments;
        let texts: Vec<&str> = rows.iter().map(|row| row.comment.as_str()).collect();
        assert_eq!(texts, ["Follow-up on this finding.", "Looks wrong"]);
    });
}

/// A finding reply the service refuses keeps the composer, its text and
/// its painted id, and frees the composer for the next Enter.
#[test]
fn a_refused_finding_reply_keeps_its_text_and_id() {
    let repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = shell(&args(vec![path_of(&repo)]), storage.path());
        settle(&mut shell, loaded);
        seed_shell(&mut shell, seed_findings);
        let tab = active_key(&shell);
        compose_finding_reply(&mut shell);
        // The finding leaves the review before the reply goes out.
        let content = shell
            .active_content()
            .unwrap_or_else(|| panic!("no content"));
        let only = serde_json::json!({"src/f000.rs": {"risk": "high", "findings": [
            finding("f1", 0, 4, "informational")
        ]}});
        write_review(
            &content.context.er_dir,
            &content.context.branch_diff_hash,
            only,
        );
        shell.handle_key(key(KeyCode::Enter));
        let painted = state(&shell, tab).draft.client_id.clone();
        settle_replies(&mut shell);
        assert_eq!(shell.view().input_mode, Mode::Comment);
        assert_eq!(draft_text(&shell, tab), "Looks wrong");
        assert!(state(&shell, tab).sent.is_none(), "the composer is stuck");
        assert!(painted.as_deref().is_some_and(|id| id.starts_with("c-")));
        assert_eq!(state(&shell, tab).draft.client_id, painted);
        let notice = shell.view().notification.clone().unwrap_or_default();
        assert!(notice.contains("Finding not found"), "{notice:?}");
    });
}

/// Enter in the AI Summary panel onto a file still a stub selects it, and
/// once it parses the diff draws where the App's in-place parse draws it.
#[test]
fn the_summary_jump_onto_a_stub_draws_like_the_app() {
    let (app_repo, flag_repo) = (fixture(520), fixture(520));
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let (mut app, mut shell) = both_on(&app_repo, &flag_repo, storage.path());
        let stub = |shell: &Shell, idx: usize| {
            shell
                .active_content()
                .is_some_and(|content| super::tui_view::is_stub(&content.diff.files[idx]))
        };
        settle_within(&mut shell, Duration::from_secs(5), |shell| !stub(shell, 0));
        seed_both(&mut app, &mut shell, seed_findings);
        assert!(stub(&shell, 1), "the second file loaded parsed");
        let mut hl = Highlighter::new();
        let codes = [
            KeyCode::Char('p'),
            KeyCode::Char('p'),
            KeyCode::Tab,
            KeyCode::Char('k'),
            KeyCode::Enter,
        ];
        for code in codes {
            step(&mut app, &mut shell, &mut hl, key(code));
        }
        settle_within(&mut shell, Duration::from_secs(5), |shell| !stub(shell, 1));
        let frame = shell_frame(&mut shell, &mut hl);
        assert_parity(&app, &frame, &mut hl, "summary enter onto stub");
    });
}

/// Enter in the AI Summary panel focuses the first anchored finding the
/// Gate shows, as Ctrl+j/k does; the App focuses the first one whatever the
/// Gate.
#[test]
fn the_summary_jump_focuses_the_first_finding_the_gate_shows() {
    let (app_repo, flag_repo) = (fixture(1), fixture(1));
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let (mut app, mut shell) = both_on(&app_repo, &flag_repo, storage.path());
        seed_both(&mut app, &mut shell, seed_gated);
        let codes = [
            KeyCode::Char('T'),
            KeyCode::Char('p'),
            KeyCode::Char('p'),
            KeyCode::Tab,
            KeyCode::Enter,
        ];
        for code in codes {
            super::tests::press_app(&mut app, code);
            shell.handle_key(key(code));
        }
        assert_eq!(focused(&shell).as_deref(), Some("kept"));
        assert_eq!(app.tab().focused_finding_id.as_deref(), Some("low"));
    });
}
