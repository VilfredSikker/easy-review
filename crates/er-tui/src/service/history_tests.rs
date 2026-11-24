//! P71.2 on the flag path: History, Conflicts and Hidden through the mode
//! keys, History's next page, and sessions saved in History or Conflicts.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::KeyCode;
use er_engine::app::App;
use er_engine::tab_identity::DiffMode;
use er_service::{Command, TabContent};

use super::shell::Shell;
use super::startup::Args;
use super::tests::{
    app_on, assert_parity, fixture, git, key, loaded, path_of, press_app, settle, settle_within,
    shell, shell_frame, SOURCES, WAIT,
};
use crate::ui::highlight::Highlighter;

fn start(path: &str, storage: &Path) -> Shell {
    let args = Args {
        paths: vec![path.to_string()],
        ..Args::default()
    };
    let mut shell = shell(&args, storage);
    settle(&mut shell, loaded);
    shell
}

fn content(shell: &Shell) -> Arc<TabContent> {
    shell
        .active_content()
        .unwrap_or_else(|| panic!("no content"))
}

fn idle(shell: &Shell) -> bool {
    shell.in_flight() == 0
}

/// Presses `code` on both paths and compares the frames once the flag
/// path's replies have landed.
fn step(app: &mut App, shell: &mut Shell, hl: &mut Highlighter, code: KeyCode) {
    app.clear_notification();
    shell.tick(Instant::now() + Duration::from_secs(60));
    press_app(app, code);
    shell.handle_key(key(code));
    settle(shell, idle);
    let frame = shell_frame(shell, hl);
    assert_parity(app, &frame, hl, &format!("{code:?}"));
}

/// Two copies of one repo, so their commits share hashes.
fn twins(make: fn() -> tempfile::TempDir) -> (tempfile::TempDir, tempfile::TempDir) {
    let repo = make();
    let copy = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let from = format!("{}/.", repo.path().display());
    let done = std::process::Command::new("cp")
        .args(["-a", &from])
        .arg(copy.path())
        .status();
    assert!(done.is_ok_and(|status| status.success()));
    (repo, copy)
}

/// The three-file fixture with a second commit on `feature` that edits
/// `f001` and adds `f003`.
fn two_commits() -> tempfile::TempDir {
    let repo = fixture(3);
    let root = repo.path();
    std::fs::write(root.join("src/f001.rs"), "let later = 1;\n").unwrap_or_else(|e| panic!("{e}"));
    std::fs::write(root.join("src/f003.rs"), "let added = 1;\n").unwrap_or_else(|e| panic!("{e}"));
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "later"]);
    repo
}

/// A merge of `other` into `feature` stopped on a conflict in `f000`.
fn merging() -> tempfile::TempDir {
    let repo = fixture(2);
    let root = repo.path();
    git(root, &["checkout", "-qb", "other", "main"]);
    std::fs::write(root.join("src/f000.rs"), "let theirs = 1;\n").unwrap_or_else(|e| panic!("{e}"));
    git(root, &["commit", "-qam", "theirs"]);
    git(root, &["checkout", "-q", "feature"]);
    let merge = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "merge",
            "-q",
            "other",
        ])
        .current_dir(root)
        .output()
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(!merge.status.success(), "the merge did not conflict");
    repo
}

/// History's key shows the commits and the selected commit's files; `k`,
/// `j`, `n` and the line keys move as the App's do, and leaving History
/// returns to the file the diff view had selected.
#[test]
fn history_draws_like_the_app() {
    let (app_repo, flag_repo) = twins(two_commits);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut app = app_on(&path_of(&app_repo));
        let mut shell = start(&path_of(&flag_repo), storage.path());
        let mut hl = Highlighter::new();
        for code in ['j', '4', 'k', 'n', 'j', 'n'] {
            step(&mut app, &mut shell, &mut hl, KeyCode::Char(code));
        }
        for code in [
            KeyCode::Down,
            KeyCode::Down,
            KeyCode::Up,
            KeyCode::Char('1'),
        ] {
            step(&mut app, &mut shell, &mut hl, code);
        }
        assert_eq!(content(&shell).tab.mode, DiffMode::Branch);
    });
}

/// `k` on the first page's last commit, the 50th of 56, lists the next page
/// and selects its first commit, as the App's does.
#[test]
fn history_lists_its_next_page_like_the_app() {
    let (app_repo, flag_repo) = twins(|| {
        let repo = fixture(1);
        for n in 0..55 {
            git(
                repo.path(),
                &["commit", "-q", "--allow-empty", "-m", &format!("c{n}")],
            );
        }
        repo
    });
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut app = app_on(&path_of(&app_repo));
        let mut shell = start(&path_of(&flag_repo), storage.path());
        let mut hl = Highlighter::new();
        step(&mut app, &mut shell, &mut hl, KeyCode::Char('4'));
        // Both go to the 50th commit, the first page's last.
        if let Some(history) = app.tab_mut().history.as_mut() {
            history.selected_commit = 49;
        }
        app.tab_mut().history_load_selected_diff();
        let held = content(&shell).history.clone();
        let sha = held.and_then(|h| h.commits.get(49).map(|c| c.hash.clone()));
        let tabs = shell.host().registry().tabs_view();
        let entry = tabs.active_entry().unwrap_or_else(|| panic!("no tab"));
        let command = Command::SelectCommit {
            key: entry.key,
            sha: sha.unwrap_or_default(),
        };
        let replies = shell
            .host()
            .post(&entry.descriptor.workspace_key(), command);
        assert!(replies.recv_timeout(WAIT).is_ok_and(|reply| reply.is_ok()));
        step(&mut app, &mut shell, &mut hl, KeyCode::Char('k'));
        let listed = content(&shell).history.as_ref().map(|h| h.commits.len());
        assert_eq!(listed, Some(56), "the fixture's 56 commits");
    });
}

/// While a merge runs the mode bar lists Conflicts; its key shows the merge
/// changeset with the unresolved count, as the App's.
#[test]
fn conflicts_show_while_a_merge_runs_like_the_app() {
    let (app_repo, flag_repo) = twins(merging);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut app = app_on(&path_of(&app_repo));
        let mut shell = start(&path_of(&flag_repo), storage.path());
        let mut hl = Highlighter::new();
        assert_parity(&app, &shell_frame(&mut shell, &mut hl), &mut hl, "merging");
        for code in ['j', '5', 'j', '1'] {
            step(&mut app, &mut shell, &mut hl, KeyCode::Char(code));
        }
    });
}

/// Hidden's key shows the watched files though `W` hid them, and selects
/// the first, as the App's switch does.
#[test]
fn hidden_shows_the_watched_files_like_the_app() {
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let config = "[watched]\npaths = [\"*.env\"]\n";
    std::fs::write(storage.path().join("config.toml"), config).unwrap_or_else(|e| panic!("{e}"));
    let make = || {
        let repo = fixture(1);
        std::fs::write(repo.path().join(".git/info/exclude"), "*.env\n")
            .unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(repo.path().join("a.env"), "A=1\n").unwrap_or_else(|e| panic!("{e}"));
        repo
    };
    let (app_repo, flag_repo) = (make(), make());
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut app = app_on(&path_of(&app_repo));
        let mut shell = start(&path_of(&flag_repo), storage.path());
        let mut hl = Highlighter::new();
        step(&mut app, &mut shell, &mut hl, KeyCode::Char('W'));
        press_app(&mut app, KeyCode::Char('5'));
        shell.handle_key(key(KeyCode::Char('5')));
        let listed = settle_within(&mut shell, WAIT, |shell| {
            let key = shell.host().registry().tabs_view().active;
            let state = key.and_then(|key| shell.view().tab(key));
            idle(shell) && state.is_some_and(|state| state.watched.selected.is_some())
        });
        assert!(listed, "Hidden listed no watched file");
        assert_eq!(content(&shell).tab.mode, DiffMode::Hidden);
        let frame = shell_frame(&mut shell, &mut hl);
        assert_parity(&app, &frame, &mut hl, "hidden");
    });
}

/// A session saved in Conflicts comes back there at its file, and one saved
/// in History comes back in History, as the App's restore does.
#[test]
fn sessions_saved_in_history_or_conflicts_restore_there() {
    for (mode, saved, make) in [
        (
            DiffMode::Conflicts,
            "conflicts",
            merging as fn() -> tempfile::TempDir,
        ),
        (DiffMode::History, "history", two_commits),
    ] {
        let (app_repo, flag_repo) = twins(make);
        let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        er_engine::storage::with_process_test_storage_root(storage.path(), || {
            for repo in [&app_repo, &flag_repo] {
                let before = app_on(&path_of(repo));
                let mut session = before.tab().capture_session();
                session.diff_mode = saved.into();
                session.selected_file = 1;
                let scope =
                    er_store::Scope::dir(before.tab().er_dir()).unwrap_or_else(|e| panic!("{e}"));
                let store = er_engine::store_registry::store();
                er_engine::session_store::save_session(&store, &scope, &session)
                    .unwrap_or_else(|e| panic!("{e}"));
            }
            let mut app = app_on(&path_of(&app_repo));
            assert!(app.tab_mut().restore_session());
            let mut shell = start(&path_of(&flag_repo), storage.path());
            let there = |shell: &Shell| idle(shell) && content(shell).tab.mode == mode;
            assert!(
                settle_within(&mut shell, WAIT, there),
                "{saved}: not restored"
            );
            let mut hl = Highlighter::new();
            assert_parity(&app, &shell_frame(&mut shell, &mut hl), &mut hl, saved);
        });
    }
}

/// History's pages and the merge flags come from the service: no flag-on
/// module lists commits or reads the merge itself.
#[test]
fn history_and_conflicts_come_from_the_service() {
    let banned = [
        "more_commits(",
        "history_commits(",
        "is_merge_in_progress",
        "conflict_files(",
        "history_load_more",
        "refresh_conflicts",
        "refresh_watched_files",
    ];
    for (name, text) in SOURCES {
        let flat: String = text.split_whitespace().collect();
        for needle in banned {
            assert!(!flat.contains(needle), "{name} names {needle}");
        }
    }
}
