//! The flag-on thread writes (P67.2): reply, edit and delete go to the actor
//! as `ReplyToThread`, `UpdateThreadMessage` and `DeleteThread`, and the AI
//! hub's layer rows act on the tab's view as the App's hub does.

use std::path::Path;
use std::time::{Duration, Instant};

use crossterm::event::KeyCode;
use er_engine::ai::{ErGithubComments, ErQuestions};
use er_engine::test_support::{read_sidecar, write_sidecar_json};

use std::sync::Arc;

use er_engine::ports::Ports;

use super::loop_tests::{Gate, GatedGit};
use super::shell::Shell;
use super::tests::{
    fixture, key, loaded, path_of, plan, press_app, settle, settle_within, shell, shell_on, SOURCES,
};
use super::threads_tests::{
    active_key, args, both_on, draft_text, er_dir, seed_both, settle_replies, state, step,
    type_text,
};
use super::tui_view::{Mode, Overlay};
use crate::screen::UNPORTED;
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
            github: Ports::cli().github,
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

/// Seeds one comment posted on GitHub, yours, on the first file.
fn seed_posted(shell: &mut Shell) -> std::path::PathBuf {
    let before = shell
        .active_content()
        .unwrap_or_else(|| panic!("no content"));
    let dir = Path::new(&before.context.comments_dir).to_path_buf();
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
    let comment = serde_json::json!({
        "id": "c-posted", "file": "src/f000.rs", "hunk_index": 0, "line_start": 4,
        "line_end": null, "comment": "On GitHub", "source": "github",
        "github_id": 7, "author": "You"
    });
    write_sidecar_json::<ErGithubComments>(
        &dir,
        serde_json::json!({"version": 1, "diff_hash": before.context.diff_hash, "comments": [comment]}),
    );
    let moved = |shell: &Shell| {
        shell
            .active_content()
            .is_some_and(|now| now.ai.revision > before.ai.revision)
    };
    assert!(
        settle_within(shell, Duration::from_secs(10), moved),
        "the posted comment never reached the AI section"
    );
    dir.join("github-comments.json")
}

/// Reply, edit and delete on a comment posted on GitHub answer the unported
/// message until P68, and the comments file is untouched.
#[test]
fn a_posted_comment_takes_no_reply_edit_or_delete() {
    let repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = shell(&args(vec![path_of(&repo)]), storage.path());
        settle(&mut shell, loaded);
        let file = seed_posted(&mut shell);
        let stored = std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("{e}"));
        shell.handle_key(key(KeyCode::Char('K')));
        let tab = active_key(&shell);
        let focused = state(&shell, tab).focused_comment_id.clone();
        assert_eq!(focused.as_deref(), Some("c-posted"));
        for code in ['r', 'e', 'x'] {
            shell.tick(Instant::now() + Duration::from_secs(60));
            shell.handle_key(key(KeyCode::Char(code)));
            settle_replies(&mut shell);
            assert_eq!(
                shell.view().input_mode,
                Mode::Normal,
                "{code} opened a mode"
            );
            let notice = shell.view().notification.clone().unwrap_or_default();
            assert!(notice.contains(UNPORTED), "{code}: {notice:?}");
        }
        let after = std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(after, stored);
    });
}

/// Moves the open AI hub to `label` on both paths, which must agree on it.
fn hub_to(app: &mut er_engine::app::App, shell: &mut Shell, label: &str) {
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
