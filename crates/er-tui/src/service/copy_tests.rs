//! The flag-on copy, export and editor actions (P75.1): each reads the tab
//! as it stands at the press and builds its text with the App's builders,
//! and the clipboard and the editor go through the launcher, which records
//! them here.

use std::path::Path;
use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use er_engine::ai::ErReview;
use er_engine::copy_text::{self, Built};
use er_engine::ports::Ports;
use er_engine::review_export::{render_markdown, ExportOpts};
use er_engine::tab_identity::DiffMode;
use er_engine::test_support::write_sidecar_json;

use super::findings_tests::seed_both;
use super::handoff::Launched;
use super::shell::Shell;
use super::tests::{
    app_on, fixture, git, key, loaded, path_of, plan, press_app, settle, shell, shell_on, SOURCES,
};
use super::threads_tests::{args, both_on, er_dir, settle_replies, step, type_text};
use super::tui_view::Overlay;
use crate::ui::highlight::Highlighter;

fn shift(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::SHIFT)
}

fn notice(shell: &Shell) -> String {
    shell.view().notification.clone().unwrap_or_default()
}

/// The one clipboard write since the last read.
fn copied(shell: &Shell) -> String {
    match shell.recorder.taken().as_slice() {
        [Launched::Clipboard(text)] => text.clone(),
        other => panic!("expected one copy, got {other:?}"),
    }
}

fn assert_copied(shell: &Shell, built: Built, step: &str) {
    let want = built.unwrap_or_else(|message| panic!("{step}: the App copies nothing: {message}"));
    assert_eq!(copied(shell), want.text, "{step}: text");
    assert_eq!(notice(shell), want.notice, "{step}: notice");
}

/// A finding on the first file's second hunk, with a suggestion.
fn seed_review(dir: &str, hash: &str) {
    let finding = serde_json::json!({
        "id": "f1", "severity": "medium", "title": "Edited twice",
        "suggestion": "Name it once", "hunk_index": 1, "line_start": 31,
        "confidence": "informational"
    });
    std::fs::create_dir_all(dir).unwrap_or_else(|e| panic!("{e}"));
    write_sidecar_json::<ErReview>(
        Path::new(dir),
        serde_json::json!({"version": 1, "diff_hash": hash, "files": {
            "src/f000.rs": {"risk": "high", "findings": [finding]}
        }}),
    );
}

/// Moves the open hub's cursor down to the row whose text holds `label`.
fn select(shell: &mut Shell, label: &str) {
    for _ in 0..64 {
        let text = match shell.view().overlay.as_ref() {
            Some(Overlay::Picker(picker)) => picker
                .rows
                .get(picker.selected)
                .map(|row| row.label.clone()),
            Some(Overlay::ConfigHub(hub)) => hub.items.get(hub.selected).map(|i| format!("{i:?}")),
            _ => panic!("no hub open"),
        };
        if text.is_some_and(|text| text.contains(label)) {
            return;
        }
        shell.handle_key(key(KeyCode::Char('j')));
    }
    panic!("no row {label}");
}

/// No flag-on module reaches the App's copy, export or editor code, only the
/// launcher names the OS handoffs, and only the copy module reads a sidecar
/// as stored text.
#[test]
fn copies_reach_no_app_copy_and_launch_only_through_the_seam() {
    let banned = [
        "render_markdown",
        "export_picker_confirm",
        "yank_hunk",
        "copy_context(",
        "copy_full_file",
        "copy_file_path",
        "copy_line(",
        "open_in_editor",
        ".tab()",
    ];
    for (name, text) in SOURCES {
        let flat: String = text.split_whitespace().collect();
        for needle in banned {
            assert!(!flat.contains(needle), "{name} names {needle}");
        }
        if name != "handoff.rs" {
            assert!(!flat.contains("os_launch"), "{name} names os_launch");
        }
        if name != "copy.rs" {
            for read in ["stored_text", "questions_text"] {
                assert!(!flat.contains(read), "{name} names {read}");
            }
        }
    }
}

/// A question written on the flag path is in the export, and the notice
/// counts the text copied.
#[test]
fn an_export_holds_a_question_added_on_the_flag_path() {
    let repo = fixture(2);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = shell(&args(vec![path_of(&repo)]), storage.path());
        settle(&mut shell, loaded);
        for _ in 0..3 {
            shell.handle_key(key(KeyCode::Down));
        }
        shell.handle_key(key(KeyCode::Char('q')));
        type_text(&mut shell, "Why edit this line?");
        shell.handle_key(key(KeyCode::Enter));
        settle_replies(&mut shell);
        settle(&mut shell, |shell| {
            shell
                .active_content()
                .is_some_and(|content| content.ai.state.questions.is_some())
        });
        shell.handle_key(shift(KeyCode::Char('E')));
        shell.handle_key(key(KeyCode::Enter));
        let text = copied(&shell);
        assert!(text.contains("Why edit this line?"), "{text}");
        let want = format!("Copied review export ({} chars)", text.len());
        assert_eq!(notice(&shell), want);
        assert!(shell.view().overlay.is_none(), "the picker stayed open");
    });
}

/// The picker draws and toggles as the App's; Enter renders what the App's
/// renderer makes of the App's tab, and with every row off it says so and
/// opens afresh.
#[test]
fn the_export_picker_toggles_and_renders_like_the_app() {
    let (app_repo, flag_repo) = (fixture(2), fixture(2));
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let (mut app, mut shell) = both_on(&app_repo, &flag_repo, storage.path());
        seed_both(&mut app, &mut shell, seed_review);
        let mut hl = Highlighter::new();
        let keys = [shift(KeyCode::Char('E')), key(KeyCode::Char('j'))];
        for event in keys.into_iter().chain([key(KeyCode::Char(' '))]) {
            step(&mut app, &mut shell, &mut hl, event);
        }
        let Some(er_engine::app::OverlayData::ExportPicker {
            include_comments,
            include_findings,
            include_questions,
            include_notes,
            ..
        }) = app.overlay.clone()
        else {
            panic!("the App's picker closed");
        };
        assert!(!include_findings, "Space did not turn findings off");
        let opts = ExportOpts {
            include_comments,
            include_questions,
            include_notes,
            include_findings,
            only_unresolved: false,
            include_annotations: false,
            ..ExportOpts::default()
        };
        let want = render_markdown(app.tab(), &opts);
        assert!(want.contains("Edited twice") == include_findings, "{want}");
        shell.handle_key(key(KeyCode::Enter));
        assert_eq!(copied(&shell), want);
        let count = want.len();
        assert_eq!(
            notice(&shell),
            format!("Copied review export ({count} chars)")
        );

        // Every row off: both say so and open the picker again, all on.
        app.overlay = None;
        step(&mut app, &mut shell, &mut hl, shift(KeyCode::Char('E')));
        for row in 0..4 {
            if row > 0 {
                step(&mut app, &mut shell, &mut hl, key(KeyCode::Char('j')));
            }
            step(&mut app, &mut shell, &mut hl, key(KeyCode::Char(' ')));
        }
        step(&mut app, &mut shell, &mut hl, key(KeyCode::Enter));
        assert_eq!(notice(&shell), "Select at least one section to export");
        assert!(shell.recorder.taken().is_empty(), "an empty export copied");
    });
}

/// A copy right after a file move reads the newly selected file.
#[test]
fn a_copy_after_a_move_reads_the_new_file() {
    let repo = fixture(2);
    // The second file's first hunk reads apart from the first file's.
    let second = repo.path().join("src/f001.rs");
    let text = std::fs::read_to_string(&second).unwrap_or_else(|e| panic!("{e}"));
    let text = text.replace("changed_3", "second_3");
    std::fs::write(&second, text).unwrap_or_else(|e| panic!("{e}"));
    git(repo.path(), &["commit", "-q", "-am", "second"]);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = shell(&args(vec![path_of(&repo)]), storage.path());
        settle(&mut shell, loaded);
        shell.handle_key(key(KeyCode::Char('k')));
        for code in [KeyCode::Char('y'), KeyCode::Char('j'), KeyCode::Char('j')] {
            shell.handle_key(key(code));
        }
        shell.handle_key(key(KeyCode::Enter));
        let content = shell
            .active_content()
            .unwrap_or_else(|| panic!("no content"));
        let [first, second] = [0, 1].map(|at| copy_text::hunk(Some(&content.diff.files[at]), 0));
        assert_ne!(first, second, "the two hunks read alike");
        let want = second;
        assert_copied(&shell, want, "hunk");
    });
}

/// What the App's builders copy from the App's tab: a Copy hub row by
/// index, else the context.
fn app_copy(app: &er_engine::app::App, pick: usize) -> Built {
    let tab = app.tab();
    let file = tab.selected_diff_file();
    match pick {
        0 => copy_text::full_file(file),
        1 => copy_text::path(file),
        2 => copy_text::hunk(tab.files.get(tab.selected_file), tab.current_hunk),
        3 => copy_text::line(file, &tab.copy_cursor()),
        _ => copy_text::context(
            file,
            &tab.copy_cursor(),
            (&tab.current_branch, &tab.base_branch),
            &tab.ai,
            &tab.layers,
        ),
    }
}

fn copy_context(shell: &mut Shell) {
    shell.handle_key(key(KeyCode::Char('a')));
    select(shell, "Copy context to clipboard");
    shell.handle_key(key(KeyCode::Enter));
}

/// The Copy hub draws as the App's, its Line row live only on a line; the
/// AI hub's "Copy context" copies the shift-arrow selection, then the line
/// once a plain arrow drops it; and each Copy hub row copies what the App's
/// builders make of the App's tab after the same keys.
#[test]
fn copies_match_the_app_after_the_same_keys() {
    let (app_repo, flag_repo) = (fixture(2), fixture(2));
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let (mut app, mut shell) = both_on(&app_repo, &flag_repo, storage.path());
        seed_both(&mut app, &mut shell, seed_review);
        let mut hl = Highlighter::new();
        let hub = [key(KeyCode::Char('y')), key(KeyCode::Esc)];
        let moves = [
            key(KeyCode::Char('n')),
            key(KeyCode::Down),
            key(KeyCode::Down),
            shift(KeyCode::Down),
        ];
        for event in hub.into_iter().chain(moves).chain(hub) {
            step(&mut app, &mut shell, &mut hl, event);
        }
        let selection = app_copy(&app, 4);
        let notice_of = |built: &Built| built.clone().map(|c| c.notice);
        assert_eq!(
            notice_of(&selection),
            Ok("Copied selection (2 lines)".to_string())
        );
        copy_context(&mut shell);
        assert_copied(&shell, selection, "selection");

        step(&mut app, &mut shell, &mut hl, key(KeyCode::Down));
        let line = app_copy(&app, 4);
        assert_eq!(notice_of(&line), Ok("Copied line (1 lines)".to_string()));
        copy_context(&mut shell);
        assert_copied(&shell, line, "line");

        for row in 0..4 {
            shell.handle_key(key(KeyCode::Char('y')));
            for _ in 0..row {
                shell.handle_key(key(KeyCode::Char('j')));
            }
            shell.handle_key(key(KeyCode::Enter));
            assert_copied(&shell, app_copy(&app, row), &format!("row {row}"));
        }
    });
}

/// The config hub's two sidecar rows copy the stored bytes of the files,
/// and say so when a file is missing.
#[test]
fn the_sidecar_rows_copy_the_stored_bytes() {
    let repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = shell(&args(vec![path_of(&repo)]), storage.path());
        settle(&mut shell, loaded);
        let dir = er_dir(&shell);
        let row = |shell: &mut Shell, label: &str| {
            // The two rows are on the Terminal tab.
            for code in [KeyCode::Char(','), KeyCode::Char('2')] {
                shell.handle_key(key(code));
            }
            select(shell, label);
            shell.handle_key(key(KeyCode::Enter));
        };
        row(&mut shell, "Copy questions.json");
        assert_eq!(notice(&shell), "No questions.json found");

        // Spaced as no serializer writes it, so only the file's own bytes match.
        let hash = shell
            .active_content()
            .map(|content| content.context.branch_diff_hash.clone())
            .unwrap_or_default();
        let review = format!("{{ \"version\":1,\n  \"diff_hash\":\"{hash}\", \"files\":{{}} }}\n");
        let questions = "{\"version\": 1,  \"questions\": [] }\n";
        let write = |name: &str, text: &str| {
            std::fs::write(Path::new(&dir).join(name), text).unwrap_or_else(|e| panic!("{e}"));
        };
        std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
        write("review.json", &review);
        write("questions.json", questions);
        row(&mut shell, "Copy review.json");
        assert_eq!(copied(&shell), review);
        let bytes = review.len();
        assert_eq!(
            notice(&shell),
            format!("Copied review.json ({bytes} bytes)")
        );
        row(&mut shell, "Copy questions.json");
        assert_eq!(copied(&shell), questions);
    });
}

/// `e` with no comment focused opens the selected file at the current
/// hunk's first new line; a failed launch says so and the loop goes on; a
/// remote tab opens nothing and says why.
#[test]
fn the_editor_opens_at_the_hunk_and_a_failure_shows() {
    let repo = fixture(2);
    // A line added on top, so the second hunk starts a line later in the
    // new file than in the old.
    let first = repo.path().join("src/f000.rs");
    let text = std::fs::read_to_string(&first).unwrap_or_else(|e| panic!("{e}"));
    std::fs::write(&first, format!("// added\n{text}")).unwrap_or_else(|e| panic!("{e}"));
    git(repo.path(), &["commit", "-q", "-am", "added"]);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = shell(&args(vec![path_of(&repo)]), storage.path());
        settle(&mut shell, loaded);
        shell.handle_key(key(KeyCode::Char('n')));
        shell.handle_key(key(KeyCode::Char('e')));
        let content = shell
            .active_content()
            .unwrap_or_else(|| panic!("no content"));
        let root = content.tab.repo_root.clone();
        let hunks = &content.diff.files[0].hunks;
        let line = hunks[1].new_start;
        assert_ne!(line, hunks[0].new_start);
        assert_ne!(line, hunks[1].old_start);
        let opened = Launched::Editor {
            path: Path::new(&root).join("src/f000.rs"),
            repo_root: root,
            line,
        };
        assert_eq!(shell.recorder.taken(), std::slice::from_ref(&opened));

        // A draft's write in flight leaves the editor to open.
        let sent = |shell: &mut Shell, sent: Option<String>| {
            if let Some(state) = shell.view_mut().tab_mut_if_open(content.key) {
                state.sent = sent;
            }
        };
        sent(&mut shell, Some("Adding question…".into()));
        shell.handle_key(key(KeyCode::Char('e')));
        assert_eq!(shell.recorder.taken(), [opened], "a write in flight");
        sent(&mut shell, None);

        if let Ok(mut fail) = shell.recorder.fail.lock() {
            *fail = Some("Failed to open editor".into());
        }
        shell.handle_key(key(KeyCode::Char('e')));
        assert_eq!(notice(&shell), "Failed to open editor");
        assert!(!shell.should_quit, "a failed launch quit");
        shell.handle_key(key(KeyCode::Char('k')));
        let at = shell.view().tab(content.key).map(|s| s.selected_file);
        assert_eq!(at, Some(1), "the loop stopped taking keys");
    });

    let raw = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n";
    let github = er_github::FakeGitHub::default().with_pr_diff_remote(Ok(raw.to_string()));
    let ports = Ports {
        git: Ports::cli().git,
        github: Arc::new(github),
    };
    let remote = super::startup::Args {
        paths: vec!["https://github.com/acme/widgets/pull/9".into()],
        remote: true,
        ..super::startup::Args::default()
    };
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = shell_on(ports, &plan(&remote), storage.path());
        settle(&mut shell, loaded);
        shell.handle_key(key(KeyCode::Char('e')));
        assert_eq!(notice(&shell), "Editor not available in remote mode");
        assert!(shell.recorder.taken().is_empty(), "a remote tab launched");
    });
}

/// A finding seeded on the second file, for a jump that changes file.
fn seed_second(dir: &str, hash: &str) {
    let finding = serde_json::json!({
        "id": "f2", "severity": "medium", "title": "Second file",
        "suggestion": "", "hunk_index": 0, "line_start": 4,
        "confidence": "informational"
    });
    std::fs::create_dir_all(dir).unwrap_or_else(|e| panic!("{e}"));
    write_sidecar_json::<ErReview>(
        Path::new(dir),
        serde_json::json!({"version": 1, "diff_hash": hash, "files": {
            "src/f001.rs": {"risk": "high", "findings": [finding]}
        }}),
    );
}

/// A finding jump to another file drops the selection, as the App's does,
/// so Shift+Down there starts a new one.
#[test]
fn a_jump_to_another_file_drops_the_selection() {
    let (app_repo, flag_repo) = (fixture(2), fixture(2));
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let (mut app, mut shell) = both_on(&app_repo, &flag_repo, storage.path());
        seed_both(&mut app, &mut shell, seed_second);
        let mut hl = Highlighter::new();
        let keys = [
            key(KeyCode::Down),
            shift(KeyCode::Down),
            KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
            shift(KeyCode::Down),
            shift(KeyCode::Down),
        ];
        for event in keys {
            step(&mut app, &mut shell, &mut hl, event);
        }
        let want = app_copy(&app, 4);
        let head = want
            .clone()
            .map(|c| (c.text.contains("Lines 4-6"), c.notice));
        assert_eq!(head, Ok((true, "Copied selection (3 lines)".to_string())));
        copy_context(&mut shell);
        assert_copied(&shell, want, "selection after the jump");
    });
}

/// In History, `e` and "Copy context" read the commit's selected file, as
/// the App's do.
#[test]
fn history_copies_and_edits_the_commit_file() {
    let repo = fixture(2);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut app = app_on(&path_of(&repo));
        let mut shell = shell(&args(vec![path_of(&repo)]), storage.path());
        settle(&mut shell, loaded);
        for code in [KeyCode::Char('4'), KeyCode::Char('n')] {
            press_app(&mut app, code);
            shell.handle_key(key(code));
            settle(&mut shell, |shell| {
                shell.in_flight() == 0
                    && shell
                        .active_content()
                        .is_some_and(|c| c.tab.mode == DiffMode::History)
            });
        }
        let path = app.tab().selected_diff_file().map(|f| f.path.clone());
        assert_eq!(path.as_deref(), Some("src/f001.rs"));
        shell.handle_key(key(KeyCode::Char('e')));
        let opened = shell.recorder.taken();
        let opened_path = match opened.as_slice() {
            [Launched::Editor { path, .. }] => path.clone(),
            other => panic!("expected the editor, got {other:?}"),
        };
        assert!(opened_path.ends_with("src/f001.rs"), "{opened_path:?}");
        copy_context(&mut shell);
        assert_copied(&shell, app_copy(&app, 4), "history context");

        // A watched file selected: no diff file, in History as elsewhere.
        let tab = shell.active_content().map(|c| c.key);
        if let Some(state) = tab.and_then(|tab| shell.view_mut().tab_mut_if_open(tab)) {
            state.watched.selected = Some(0);
        }
        shell.handle_key(key(KeyCode::Char('e')));
        assert_eq!(
            shell.recorder.taken(),
            [],
            "a watched file opened a diff file"
        );
    });
}

/// A refresh that moves the selected file's index keeps its selection, as
/// the App's does: the anchor belongs to the file, not to its place.
#[test]
fn a_refresh_that_moves_the_file_keeps_its_selection() {
    let (app_repo, flag_repo) = (fixture(2), fixture(2));
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let (mut app, mut shell) = both_on(&app_repo, &flag_repo, storage.path());
        let mut hl = Highlighter::new();
        let keys = [
            key(KeyCode::Char('k')),
            key(KeyCode::Down),
            shift(KeyCode::Down),
        ];
        for event in keys {
            step(&mut app, &mut shell, &mut hl, event);
        }
        // A file that sorts first moves src/f001.rs from index 1 to 2.
        for repo in [&app_repo, &flag_repo] {
            std::fs::write(repo.path().join("src/a.rs"), "fn a() {}\n")
                .unwrap_or_else(|e| panic!("{e}"));
            git(repo.path(), &["add", "."]);
            git(repo.path(), &["commit", "-q", "-m", "a"]);
        }
        press_app(&mut app, KeyCode::Char('R'));
        shell.handle_key(key(KeyCode::Char('R')));
        settle(&mut shell, |shell| {
            shell.in_flight() == 0
                && shell
                    .active_content()
                    .is_some_and(|c| c.diff.files.len() == 3)
        });
        for _ in 0..2 {
            step(&mut app, &mut shell, &mut hl, shift(KeyCode::Down));
        }
        let want = app_copy(&app, 4);
        let notice_of = want.clone().map(|c| c.notice);
        assert_eq!(notice_of, Ok("Copied selection (4 lines)".to_string()));
        copy_context(&mut shell);
        assert_copied(&shell, want, "after the refresh");
    });
}

/// Findings on both files' first hunks, for jumps away and back.
fn seed_both_files(dir: &str, hash: &str) {
    let finding = |id: &str| {
        serde_json::json!({
            "id": id, "severity": "medium", "title": format!("Finding {id}"),
            "suggestion": "", "hunk_index": 0, "line_start": 4,
            "confidence": "informational"
        })
    };
    std::fs::create_dir_all(dir).unwrap_or_else(|e| panic!("{e}"));
    write_sidecar_json::<ErReview>(
        Path::new(dir),
        serde_json::json!({"version": 1, "diff_hash": hash, "files": {
            "src/f000.rs": {"risk": "high", "findings": [finding("f1")]},
            "src/f001.rs": {"risk": "high", "findings": [finding("f2")]}
        }}),
    );
}

/// A finding jump to another file and back leaves no selection, as the
/// App clears it on each jump that changes file.
#[test]
fn a_jump_away_and_back_leaves_no_selection() {
    let (app_repo, flag_repo) = (fixture(2), fixture(2));
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let (mut app, mut shell) = both_on(&app_repo, &flag_repo, storage.path());
        seed_both(&mut app, &mut shell, seed_both_files);
        let mut hl = Highlighter::new();
        let ctrl = |c: char| KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL);
        for event in [key(KeyCode::Down), shift(KeyCode::Down)] {
            step(&mut app, &mut shell, &mut hl, event);
        }
        for _ in 0..3 {
            if app.tab().selected_file == 1 {
                break;
            }
            step(&mut app, &mut shell, &mut hl, ctrl('k'));
        }
        assert_eq!(
            app.tab().selected_file,
            1,
            "the jumps did not reach src/f001.rs"
        );
        step(&mut app, &mut shell, &mut hl, ctrl('j'));
        assert_eq!(app.tab().selected_file, 0, "the jump did not come back");
        let want = app_copy(&app, 4);
        let notice_of = want.clone().map(|c| c.notice);
        assert_eq!(notice_of, Ok("Copied line (1 lines)".to_string()));
        copy_context(&mut shell);
        assert_copied(&shell, want, "after the jumps");
    });
}
