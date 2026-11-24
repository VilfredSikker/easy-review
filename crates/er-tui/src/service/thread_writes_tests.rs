//! The flag-on thread writes (P67.2): reply, edit and delete go to the actor
//! as `ReplyToThread`, `UpdateThreadMessage` and `DeleteThread`, and the AI
//! hub's layer rows act on the tab's view as the App's hub does.

use std::path::Path;

use crossterm::event::KeyCode;
use er_engine::ai::ErQuestions;
use er_engine::test_support::read_sidecar;

use std::sync::Arc;

use er_engine::ports::Ports;

use super::loop_tests::{Gate, GatedGit};
use super::shell::Shell;
use super::startup::After;
use super::tests::{
    fixture, key, loaded, path_of, plan, press_app, settle, shell, shell_on, SOURCES,
};
use super::threads_tests::{
    active_key, args, both_on, draft_text, early_in_a_second, er_dir, seed_both, settle_replies,
    state, step, type_text,
};
use super::tui_view::{Mode, Overlay};
use crate::ui::highlight::Highlighter;

/// The rows of a bucket's questions and notes files as stored, less the
/// fields each write makes fresh; the parent id a reply names becomes
/// whether it has one, since each path paints its own ids.
fn rows(dir: &str) -> Vec<serde_json::Value> {
    let mut rows = Vec::new();
    for (file, list) in [("questions.json", "questions"), ("notes.json", "notes")] {
        let Ok(text) = std::fs::read_to_string(Path::new(dir).join(file)) else {
            continue;
        };
        let stored: serde_json::Value =
            serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}"));
        let listed = stored["payload"][list]
            .as_array()
            .cloned()
            .unwrap_or_default();
        for mut row in listed {
            if let Some(map) = row.as_object_mut() {
                map.remove("id");
                map.remove("timestamp");
                let is_reply = map.remove("in_reply_to").is_some_and(|id| !id.is_null());
                map.insert("is_reply".into(), is_reply.into());
            }
            rows.push(row);
        }
    }
    rows
}

/// A question, a reply under it and the thread's delete, typed on both
/// paths, store the same rows and draw the same frames.
#[test]
fn reply_and_delete_write_and_draw_like_the_app() {
    let (app_repo, flag_repo) = (fixture(2), fixture(2));
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let (mut app, mut shell) = both_on(&app_repo, &flag_repo, storage.path());
        let mut hl = Highlighter::new();
        let typed = |text: &str| {
            text.chars()
                .map(|c| key(KeyCode::Char(c)))
                .collect::<Vec<_>>()
        };
        let mut keys = vec![key(KeyCode::Down); 3];
        keys.push(key(KeyCode::Char('q')));
        keys.extend(typed("Why this?"));
        keys.extend([key(KeyCode::Enter), key(KeyCode::Char('J'))]);
        keys.push(key(KeyCode::Char('r')));
        keys.extend(typed("Because"));
        keys.push(key(KeyCode::Enter));
        for event in keys {
            if event.code == KeyCode::Enter {
                early_in_a_second();
            }
            step(&mut app, &mut shell, &mut hl, event);
        }
        let replied = rows(&er_dir(&shell));
        assert_eq!(replied, rows(&app.tab().er_dir()));
        let replies = replied.iter().filter(|row| row["is_reply"] == true);
        assert_eq!(replies.count(), 1, "{replied:?}");

        // J lands on the reply, so the first delete takes it alone; the
        // second lands on the question and takes the thread.
        for left in [1, 0] {
            for code in [KeyCode::Char('J'), KeyCode::Char('x'), KeyCode::Char('y')] {
                step(&mut app, &mut shell, &mut hl, key(code));
            }
            let deleted = rows(&er_dir(&shell));
            assert_eq!(deleted, rows(&app.tab().er_dir()));
            assert_eq!(deleted.len(), left, "{deleted:?}");
            assert!(deleted.iter().all(|row| row["is_reply"] == false));
        }
    });
}

/// A second Enter while a reply is in flight sends nothing, so one reply
/// lands: the service does not deduplicate on the painted id.
#[test]
fn a_reply_in_flight_is_sent_once() {
    let repo = fixture(2);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let start = plan(&args(vec![path_of(&repo)]));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let gate = Arc::new(Gate::default());
        let ports = Ports {
            git: Arc::new(GatedGit(Arc::clone(&gate))),
            github: super::tests::fake_github(),
        };
        let mut shell = shell_on(ports, &start, storage.path());
        settle(&mut shell, loaded);
        shell.handle_key(key(KeyCode::Char('q')));
        type_text(&mut shell, "Why this?");
        shell.handle_key(key(KeyCode::Enter));
        settle_replies(&mut shell);
        shell.handle_key(key(KeyCode::Char('J')));
        shell.handle_key(key(KeyCode::Char('r')));
        type_text(&mut shell, "Because");
        gate.set(true);
        shell.handle_key(key(KeyCode::Enter));
        shell.handle_key(key(KeyCode::Enter));
        gate.set(false);
        settle_replies(&mut shell);
        let stored = rows(&er_dir(&shell));
        let replies = stored.iter().filter(|row| row["is_reply"] == true);
        assert_eq!(replies.count(), 1, "{stored:?}");
        assert_eq!(shell.view().input_mode, Mode::Normal);
    });
}

/// A reply whose worker dies answers as a refusal: the composer keeps its
/// text, says so, and the next Enter writes the reply.
#[test]
fn a_reply_that_never_answers_frees_the_composer() {
    let repo = fixture(2);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let start = plan(&args(vec![path_of(&repo)]));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let gate = Arc::new(Gate::default());
        let ports = Ports {
            git: Arc::new(GatedGit(Arc::clone(&gate))),
            github: super::tests::fake_github(),
        };
        let mut shell = shell_on(ports, &start, storage.path());
        settle(&mut shell, loaded);
        let tab = active_key(&shell);
        shell.handle_key(key(KeyCode::Char('q')));
        type_text(&mut shell, "Why this?");
        shell.handle_key(key(KeyCode::Enter));
        settle_replies(&mut shell);
        shell.handle_key(key(KeyCode::Char('J')));
        shell.handle_key(key(KeyCode::Char('r')));
        type_text(&mut shell, "Lost");
        gate.panics.store(true, std::sync::atomic::Ordering::SeqCst);
        shell.handle_key(key(KeyCode::Enter));
        settle_replies(&mut shell);
        gate.panics
            .store(false, std::sync::atomic::Ordering::SeqCst);
        assert!(state(&shell, tab).sent.is_none(), "the composer is stuck");
        assert_eq!(draft_text(&shell, tab), "Lost");
        shell.handle_key(key(KeyCode::Enter));
        settle_replies(&mut shell);
        let stored = rows(&er_dir(&shell));
        let replies = stored.iter().filter(|row| row["is_reply"] == true);
        assert_eq!(replies.count(), 1, "{stored:?}");
    });
}

/// A delete whose worker dies answers as a refusal, so the status line
/// says the write was lost.
#[test]
fn a_delete_that_never_answers_says_so() {
    let repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = shell(&args(vec![path_of(&repo)]), storage.path());
        settle(&mut shell, loaded);
        let tab = active_key(&shell);
        let tabs = shell.host().registry().tabs_view();
        let workspace = tabs
            .active_entry()
            .map(|entry| entry.descriptor.workspace_key())
            .unwrap_or_else(|| panic!("no active tab"));
        shell.lose_write(tab, workspace, After::Deleted);
        settle_replies(&mut shell);
        let notice = shell.view().notification.clone().unwrap_or_default();
        assert!(notice.contains("stopped"), "{notice:?}");
    });
}

/// `e` on your own question opens it prefilled; Enter changes its text in
/// place, under the same id and on the same anchor (ruling 5 on #653).
#[test]
fn an_edit_changes_the_text_and_keeps_the_row_and_its_anchor() {
    let repo = fixture(2);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = shell(&args(vec![path_of(&repo)]), storage.path());
        settle(&mut shell, loaded);
        let tab = active_key(&shell);
        let dir = er_dir(&shell);
        for _ in 0..3 {
            shell.handle_key(key(KeyCode::Down));
        }
        shell.handle_key(key(KeyCode::Char('q')));
        type_text(&mut shell, "Why this?");
        shell.handle_key(key(KeyCode::Enter));
        settle_replies(&mut shell);
        let before = read_sidecar::<ErQuestions>(Path::new(&dir)).questions;

        shell.handle_key(key(KeyCode::Char('J')));
        shell.handle_key(key(KeyCode::Down));
        shell.handle_key(key(KeyCode::Char('e')));
        assert_eq!(shell.view().input_mode, Mode::Comment);
        assert_eq!(draft_text(&shell, tab), "Why this?");
        type_text(&mut shell, "Still: ");
        shell.handle_key(key(KeyCode::Enter));
        settle_replies(&mut shell);

        let after = read_sidecar::<ErQuestions>(Path::new(&dir)).questions;
        assert_eq!(after.len(), 1, "the edit wrote a new row");
        assert_eq!(after[0].id, before[0].id);
        assert_eq!(after[0].text, "Still: Why this?");
        assert_eq!(
            after[0].line_start, before[0].line_start,
            "the anchor moved"
        );
        assert_eq!(shell.view().input_mode, Mode::Normal);
        assert_eq!(state(&shell, tab).draft.edit_id, None);
        let notice = shell.view().notification.clone().unwrap_or_default();
        assert!(notice.starts_with("Comment updated"), "{notice}");
    });
}

/// A paused edit does not take over the next `q`: the question is a new
/// row, and the edited one keeps its text.
#[test]
fn a_paused_edit_leaves_the_next_question_new() {
    let repo = fixture(2);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = shell(&args(vec![path_of(&repo)]), storage.path());
        settle(&mut shell, loaded);
        shell.handle_key(key(KeyCode::Char('q')));
        type_text(&mut shell, "First");
        shell.handle_key(key(KeyCode::Enter));
        settle_replies(&mut shell);
        for code in [KeyCode::Char('J'), KeyCode::Down, KeyCode::Char('e')] {
            shell.handle_key(key(code));
        }
        shell.handle_key(key(KeyCode::Tab));
        shell.handle_key(key(KeyCode::Down));
        shell.handle_key(key(KeyCode::Char('q')));
        type_text(&mut shell, "Second");
        shell.handle_key(key(KeyCode::Enter));
        settle_replies(&mut shell);
        let texts: Vec<String> = rows(&er_dir(&shell))
            .iter()
            .map(|row| row["text"].as_str().unwrap_or_default().to_string())
            .collect();
        assert_eq!(texts, ["First", "Second"]);
        let notice = shell.view().notification.clone().unwrap_or_default();
        assert_eq!(notice, "Question added: Second");
    });
}

/// A whitespace-only body goes to the service, which refuses it: the
/// composer stays open with the text, and nothing is written (ruling 5).
#[test]
fn a_whitespace_body_is_refused_and_kept() {
    let repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = shell(&args(vec![path_of(&repo)]), storage.path());
        settle(&mut shell, loaded);
        let tab = active_key(&shell);
        shell.handle_key(key(KeyCode::Char('q')));
        type_text(&mut shell, "   ");
        shell.handle_key(key(KeyCode::Enter));
        settle_replies(&mut shell);
        assert_eq!(
            shell.view().input_mode,
            Mode::Comment,
            "the composer closed"
        );
        assert_eq!(draft_text(&shell, tab), "   ");
        let notice = shell.view().notification.clone().unwrap_or_default();
        assert!(notice.contains("empty"), "{notice:?}");
        assert!(rows(&er_dir(&shell)).is_empty());
    });
}

/// Moves the open AI hub to `label` on both paths, which must agree on it.
pub(super) fn hub_to(app: &mut er_engine::app::App, shell: &mut Shell, label: &str) {
    let selected = |shell: &Shell| match &shell.view().overlay {
        Some(Overlay::Picker(picker)) => picker.rows[picker.selected].label.clone(),
        _ => panic!("the AI hub is not open"),
    };
    for _ in 0..30 {
        if selected(shell) == label {
            break;
        }
        press_app(app, KeyCode::Char('j'));
        shell.handle_key(key(KeyCode::Char('j')));
    }
    assert_eq!(selected(shell), label);
    let on_app = match &app.overlay {
        Some(er_engine::app::OverlayData::ModalHub {
            items, selected, ..
        }) => items[*selected].label.clone(),
        _ => panic!("the App's AI hub is not open"),
    };
    assert_eq!(on_app, label);
}

/// Each layer row of the AI hub, chosen on both paths, draws the same
/// frame and says the same thing, the Gate's row included.
#[test]
fn the_hub_layer_rows_act_like_the_app() {
    let (app_repo, flag_repo) = (fixture(3), fixture(3));
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let (mut app, mut shell) = both_on(&app_repo, &flag_repo, storage.path());
        seed_both(&mut app, &mut shell, false, "A note");
        let mut hl = Highlighter::new();
        let labels = [
            "Toggle AI findings",
            "Toggle comments",
            "Toggle questions",
            "Hide resolved",
            "Confidence gate",
            "Confidence gate",
            "Toggle questions",
            "Toggle AI findings",
        ];
        for label in labels {
            press_app(&mut app, KeyCode::Char('a'));
            shell.handle_key(key(KeyCode::Char('a')));
            hub_to(&mut app, &mut shell, label);
            step(&mut app, &mut shell, &mut hl, key(KeyCode::Enter));
            assert!(shell.view().overlay.is_none(), "{label} left the hub open");
        }
    });
}

/// The hub's layer arm in the loop sends nothing to the service: the rows
/// are view state (ADDENDUM-05 risk 6).
#[test]
fn the_hub_layer_rows_stay_in_the_view() {
    let shell_rs = SOURCES
        .iter()
        .find(|(name, _)| *name == "shell.rs")
        .map_or("", |(_, text)| text);
    let arm = shell_rs
        .split("if let Step::Layer(row) = step {")
        .nth(1)
        .and_then(|rest| rest.split("self.run_step(step);").next())
        .unwrap_or_else(|| panic!("the hub's layer arm moved"));
    assert!(arm.contains("layers::hub_row"), "the hub's layer arm moved");
    for word in [".post(", "Command::", "host", ".send("] {
        assert!(!arm.contains(word), "the hub's layer arm names {word}");
    }
}
