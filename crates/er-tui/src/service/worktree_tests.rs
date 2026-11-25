//! The flag-on working-tree keys (P69.1): a refusal shows the port's
//! message, and stage, commit and the Git hub draw and write as the App does.

use std::path::Path;
use std::process::Command as Git;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use er_engine::tab_identity::DiffMode;
use er_service::Command;

use super::parity::assert_json;
use super::shell::Shell;
use super::startup::Args;
use super::tests::{
    app_on, assert_parity, fixture, git, key, loaded, path_of, press_app, settle, shell,
    shell_frame, without_review_row, WAIT,
};
use super::tui_view::Mode;
use crate::ui::highlight::Highlighter;

/// Shows the active tab in `mode` through the service, as P71's mode keys
/// will.
fn to_mode(shell: &mut Shell, mode: DiffMode) {
    let tabs = shell.host().registry().tabs_view();
    let entry = tabs.active_entry().unwrap_or_else(|| panic!("no tab"));
    let command = Command::SetDiffMode {
        key: entry.key,
        mode,
        pr_number: None,
    };
    let reply = shell
        .host()
        .post(&entry.descriptor.workspace_key(), command)
        .recv_timeout(WAIT);
    assert!(matches!(reply, Ok(Ok(_))), "{reply:?}");
    settle(shell, |shell| {
        shell
            .active_content()
            .is_some_and(|content| content.tab.mode == mode)
    });
}

fn notice(shell: &Shell) -> String {
    shell.view().notification.clone().unwrap_or_default()
}

/// Lets the last step's message expire.
fn expire(shell: &mut Shell) {
    shell.tick(Instant::now() + Duration::from_secs(60));
}

// The lint gate reads this file alone, without the module's `cfg(test)`.
#[cfg(test)]
fn porcelain(dir: &Path) -> String {
    let out = Git::new("git")
        .args(["status", "--porcelain"])
        .current_dir(dir)
        .output()
        .unwrap_or_else(|e| panic!("{e}"));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Two changed files, both edited again in the working tree, with an
/// identity a commit through the port can use.
fn edited_fixture() -> tempfile::TempDir {
    let repo = fixture(2);
    for (name, value) in [
        ("user.name", "t"),
        ("user.email", "t@t"),
        ("commit.gpgsign", "false"),
    ] {
        git(repo.path(), &["config", name, value]);
    }
    for idx in 0..2 {
        let path = repo.path().join(format!("src/f{idx:03}.rs"));
        std::fs::write(path, format!("let edited_{idx} = {idx};\n"))
            .unwrap_or_else(|e| panic!("{e}"));
    }
    repo
}

/// A push or a commit the port refuses shows the port's message on the
/// status line, and a refused commit keeps its text in the input. The repo
/// has no remote and nothing staged, so git refuses both.
#[test]
fn a_refused_push_shows_the_ports_message() {
    let repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let args = Args {
        paths: vec![path_of(&repo)],
        ..Args::default()
    };
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = shell(&args, storage.path());
        settle(&mut shell, loaded);
        to_mode(&mut shell, DiffMode::Staged);
        expire(&mut shell);

        shell.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
        shell.handle_key(key(KeyCode::Char('y')));
        settle(&mut shell, |shell| !notice(shell).is_empty());
        let shown = notice(&shell);
        assert!(
            shown.starts_with("Push failed: git push failed"),
            "{shown:?}"
        );

        expire(&mut shell);
        let typed = |shell: &Shell| {
            let active = shell.host().registry().tabs_view().active;
            let state = active.and_then(|key| shell.view().tab(key));
            state.map(|state| state.commit_input.clone())
        };
        for code in [KeyCode::Char('c'), KeyCode::Char('m'), KeyCode::Enter] {
            shell.handle_key(key(code));
        }
        // In flight, the input takes no edits.
        shell.handle_key(key(KeyCode::Char('x')));
        assert_eq!(
            typed(&shell).as_deref(),
            Some("m"),
            "an edit landed in flight"
        );
        settle(&mut shell, |shell| !notice(shell).is_empty());
        let shown = notice(&shell);
        assert!(shown.starts_with("git commit failed"), "{shown:?}");
        assert_eq!(shell.view().input_mode, Mode::Commit);
        assert_eq!(
            typed(&shell).as_deref(),
            Some("m"),
            "the refused commit lost its text"
        );
    });
}

/// `s` in Unstaged stages the selected file on both paths: the index and
/// the frame match the App's.
#[test]
fn a_stage_writes_and_draws_like_the_app() {
    let _golden = super::parity::guard();
    let repo = edited_fixture();
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let path = path_of(&repo);
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut app = app_on(&path);
        press_app(&mut app, KeyCode::Char('2'));
        press_app(&mut app, KeyCode::Char('s'));
        let app_index = porcelain(repo.path());
        git(repo.path(), &["reset", "-q"]);

        let args = Args {
            paths: vec![path.clone()],
            ..Args::default()
        };
        let mut shell = shell(&args, storage.path());
        settle(&mut shell, loaded);
        to_mode(&mut shell, DiffMode::Unstaged);
        shell.handle_key(key(KeyCode::Char('s')));
        settle(&mut shell, |shell| !notice(shell).is_empty());
        assert_eq!(porcelain(repo.path()), app_index, "the index differs");
        assert_json("index", &porcelain(repo.path()), &app_index);
        let mut hl = Highlighter::new();
        assert_parity(&app, &shell_frame(&mut shell, &mut hl), &mut hl, "staged");
    });
}

/// The commit input, then the committed view with its notice, draw as the
/// App draws them.
#[test]
fn a_commit_draws_like_the_app() {
    let _golden = super::parity::guard();
    let repo = edited_fixture();
    git(repo.path(), &["add", "src/f000.rs"]);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let path = path_of(&repo);
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut app = app_on(&path);
        press_app(&mut app, KeyCode::Char('3'));
        app.clear_notification();
        let args = Args {
            paths: vec![path.clone()],
            ..Args::default()
        };
        let mut shell = shell(&args, storage.path());
        settle(&mut shell, loaded);
        to_mode(&mut shell, DiffMode::Staged);
        expire(&mut shell);
        let mut hl = Highlighter::new();
        assert_parity(&app, &shell_frame(&mut shell, &mut hl), &mut hl, "staged");

        for code in [KeyCode::Char('c'), KeyCode::Char('o'), KeyCode::Char('k')] {
            press_app(&mut app, code);
            shell.handle_key(key(code));
        }
        assert_parity(&app, &shell_frame(&mut shell, &mut hl), &mut hl, "typing");

        // The App commits first; the flag path commits the same index again.
        press_app(&mut app, KeyCode::Enter);
        git(repo.path(), &["reset", "-q", "--soft", "HEAD~1"]);
        shell.handle_key(key(KeyCode::Enter));
        settle(&mut shell, |shell| notice(shell).starts_with("Committed!"));
        assert_parity(
            &app,
            &shell_frame(&mut shell, &mut hl),
            &mut hl,
            "committed",
        );
    });
}

/// The Git hub lists the App's rows, with push off outside Staged.
#[test]
fn the_git_hub_draws_like_the_app() {
    let _golden = super::parity::guard();
    let repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let path = path_of(&repo);
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut app = app_on(&path);
        let args = Args {
            paths: vec![path.clone()],
            ..Args::default()
        };
        let mut shell = shell(&args, storage.path());
        settle(&mut shell, loaded);
        let mut hl = Highlighter::new();
        for code in [KeyCode::Char('g'), KeyCode::Char('j')] {
            press_app(&mut app, code);
            shell.handle_key(key(code));
            without_review_row(&mut shell);
            let step = format!("{code:?}");
            assert_parity(&app, &shell_frame(&mut shell, &mut hl), &mut hl, &step);
        }
    });
}
