//! P71.1 on the flag path: the mode keys, the mtime sort, the watcher, the
//! base change, `--target`, a session saved in another view, and the rule
//! tables read again after a refresh.

use std::time::{Duration, Instant, SystemTime};

use crossterm::event::KeyCode;
use er_engine::app::App;
use er_engine::tab_identity::DiffMode;
use er_engine::watch::WatchEvent;

use super::shell::Shell;
use super::startup::Args;
use super::tests::{
    app_on, assert_parity, fixture, git, key, loaded, path_of, press_app, settle, settle_within,
    shell, shell_frame, SOURCES,
};
use crate::screen::UNPORTED;
use crate::ui::highlight::Highlighter;

fn storage() -> tempfile::TempDir {
    tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"))
}

fn start(paths: Vec<String>, target: Option<&str>, storage: &std::path::Path) -> Shell {
    let args = Args {
        paths,
        target: target.map(String::from),
        ..Args::default()
    };
    let mut shell = shell(&args, storage);
    settle(&mut shell, loaded);
    shell
}

fn content(shell: &Shell) -> std::sync::Arc<er_service::TabContent> {
    shell
        .active_content()
        .unwrap_or_else(|| panic!("no content"))
}

fn idle(shell: &Shell) -> bool {
    shell.in_flight() == 0
}

/// Presses `code` on both paths, the last step's message gone first, and
/// compares the frames once the flag path's replies have landed.
fn step(app: &mut App, shell: &mut Shell, hl: &mut Highlighter, code: KeyCode) {
    app.clear_notification();
    shell.tick(Instant::now() + Duration::from_secs(60));
    press_app(app, code);
    shell.handle_key(key(code));
    settle(shell, idle);
    let frame = shell_frame(shell, hl);
    assert_parity(app, &frame, hl, &format!("{code:?}"));
}

fn write(root: &std::path::Path, file: &str, text: &str) {
    std::fs::write(root.join(file), text).unwrap_or_else(|e| panic!("{e}"));
}

/// The three-file fixture with `f001` and `f002` edited in the working tree
/// and `stage` staged, a branch `other` at the tip, and mtimes that put
/// `f001`, then `f002`, then `f000` first.
fn worktree_fixture(stage: &[&str]) -> tempfile::TempDir {
    let repo = fixture(3);
    let root = repo.path();
    git(root, &["branch", "other", "feature"]);
    for file in ["src/f001.rs", "src/f002.rs"] {
        write(root, file, "let worktree = 1;\n");
    }
    for file in stage {
        git(root, &["add", file]);
    }
    let now = SystemTime::now();
    for (file, hours) in [("src/f000.rs", 6), ("src/f001.rs", 2), ("src/f002.rs", 4)] {
        let at = now - Duration::from_secs(hours * 3600);
        let handle = std::fs::File::options()
            .write(true)
            .open(root.join(file))
            .unwrap_or_else(|e| panic!("{e}"));
        handle.set_modified(at).unwrap_or_else(|e| panic!("{e}"));
    }
    repo
}

/// Each mode key shows the view the App shows, and the selection follows
/// the file across the switch as the App's does; the mtime sort lists the
/// files in the order the service publishes, as the App sorts them.
#[test]
fn mode_keys_and_the_mtime_sort_draw_like_the_app() {
    let (app_repo, flag_repo) = (
        worktree_fixture(&["src/f002.rs"]),
        worktree_fixture(&["src/f002.rs"]),
    );
    let storage = storage();
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut app = app_on(&path_of(&app_repo));
        let mut shell = start(vec![path_of(&flag_repo)], None, storage.path());
        let mut hl = Highlighter::new();
        for code in ['j', '2', '3', '1', 'm', 'j', 'm'] {
            step(&mut app, &mut shell, &mut hl, KeyCode::Char(code));
        }
        assert_eq!(content(&shell).tab.mode, DiffMode::Branch);
    });
}

/// A change the watcher reports refreshes the tab it watches, and the
/// status line says how many reviewed marks the change cleared, else how
/// many files changed, in the App's words.
#[test]
fn the_watcher_refreshes_its_tab_and_counts_cleared_marks() {
    let repo = fixture(2);
    let storage = storage();
    let path = path_of(&repo);
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = start(vec![path.clone()], None, storage.path());
        shell.watch_mut().on = true;
        shell.tick(Instant::now());
        assert_eq!(shell.watch_mut().root(), Some(path.as_str()));
        // The test feeds the changes itself, so the file system's own
        // events stay out of it.
        shell.watch_mut().on = false;
        shell.tick(Instant::now());
        shell.handle_key(key(KeyCode::Char(' ')));
        settle(&mut shell, |shell| {
            idle(shell) && !content(shell).reviewed.marks.is_empty()
        });
        let sender = shell.watch_mut().sender();
        for (file, notice) in [
            (
                "src/f000.rs",
                "1 reviewed file auto-unmarked (diff changed)",
            ),
            ("src/f001.rs", "1 file changed"),
        ] {
            let before = content(&shell).diff.revision;
            write(repo.path(), file, "let watched = 1;\n");
            git(repo.path(), &["commit", "-qam", "watched"]);
            let changed = vec![format!("{path}/{file}")];
            sender
                .send(WatchEvent::FilesChanged(changed))
                .unwrap_or_else(|e| panic!("{e}"));
            let moved = settle_within(&mut shell, Duration::from_secs(20), |shell| {
                idle(shell) && content(shell).diff.revision > before
            });
            assert!(moved, "{file}: the watcher refreshed nothing");
            let content = content(&shell);
            let diff = content.diff.files.iter().find(|f| f.path == file);
            let text = diff.map(|f| format!("{:?}", f.hunks)).unwrap_or_default();
            assert!(text.contains("watched"), "{file}: {text}");
            assert_eq!(shell.view().notification.as_deref(), Some(notice));
        }
    });
}

/// The Open hub's "Change compare base" row opens the base input; Enter
/// rediffs the tab on the service against the base and keeps it in the
/// registry, and the frame matches the App's after the same change.
#[test]
fn the_base_input_rediffs_the_tab_and_keeps_its_base() {
    let (app_repo, flag_repo) = (worktree_fixture(&[]), worktree_fixture(&[]));
    let storage = storage();
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut app = app_on(&path_of(&app_repo));
        let mut shell = start(vec![path_of(&flag_repo)], None, storage.path());
        let before = content(&shell).diff.files.len();
        // The hub's other rows wait for their entries and say so.
        shell.handle_key(key(KeyCode::Char('o')));
        shell.handle_key(key(KeyCode::Enter));
        let notice = shell.view().notification.clone().unwrap_or_default();
        assert!(notice.contains(UNPORTED), "{notice}");
        let mut codes = vec![KeyCode::Char('o'), KeyCode::Down, KeyCode::Down];
        codes.extend([KeyCode::Down, KeyCode::Enter]);
        codes.extend("other".chars().map(KeyCode::Char));
        codes.push(KeyCode::Enter);
        for code in codes {
            shell.handle_key(key(code));
        }
        settle(&mut shell, idle);
        let content = content(&shell);
        assert_eq!(content.tab.base_branch, "other");
        assert_ne!(content.diff.files.len(), before, "the diff did not change");
        let tabs = shell.host().registry().tabs_view();
        let saved = tabs
            .active_entry()
            .and_then(|e| e.descriptor.base_ref.clone());
        assert_eq!(saved.as_deref(), Some("other"));
        app.tab_mut()
            .set_base_branch("other")
            .unwrap_or_else(|e| panic!("{e}"));
        app.notify("Base set to other");
        let mut hl = Highlighter::new();
        assert_parity(&app, &shell_frame(&mut shell, &mut hl), &mut hl, "base");
    });
}

/// `--target` is every start tab's base from its first load, a tab shown
/// later included, and the start says nothing about it.
#[test]
fn target_is_every_tabs_base() {
    let (first, second) = (worktree_fixture(&[]), worktree_fixture(&[]));
    let storage = storage();
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let paths = vec![path_of(&first), path_of(&second)];
        let mut shell = start(paths, Some("other"), storage.path());
        assert_eq!(shell.view().notification, None);
        assert_eq!(content(&shell).tab.base_branch, "other");
        shell.handle_key(key(KeyCode::Char(']')));
        let second_root = path_of(&second);
        settle(&mut shell, |shell| {
            shell
                .active_content()
                .is_some_and(|content| content.tab.repo_root == second_root)
        });
        assert_eq!(content(&shell).tab.base_branch, "other");
    });
}

/// A session saved in another view than the tab opens in switches there,
/// and its position comes back in that view, as the App's restore does.
#[test]
fn a_session_saved_in_another_view_restores_there_like_the_app() {
    let staged = ["src/f001.rs", "src/f002.rs"];
    let (app_repo, flag_repo) = (worktree_fixture(&staged), worktree_fixture(&staged));
    let storage = storage();
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        for repo in [&app_repo, &flag_repo] {
            let before = app_on(&path_of(repo));
            let mut session = before.tab().capture_session();
            session.diff_mode = "staged".into();
            session.selected_file = 1;
            let scope =
                er_store::Scope::dir(before.tab().er_dir()).unwrap_or_else(|e| panic!("{e}"));
            let store = er_engine::store_registry::store();
            er_engine::session_store::save_session(&store, &scope, &session)
                .unwrap_or_else(|e| panic!("{e}"));
        }
        let mut app = app_on(&path_of(&app_repo));
        assert!(app.tab_mut().restore_session());
        let mut shell = start(vec![path_of(&flag_repo)], None, storage.path());
        let staged = |shell: &Shell| idle(shell) && content(shell).tab.mode == DiffMode::Staged;
        let switched = settle_within(&mut shell, Duration::from_secs(20), staged);
        assert!(switched, "the session's view was not restored");
        let mut hl = Highlighter::new();
        assert_parity(&app, &shell_frame(&mut shell, &mut hl), &mut hl, "restored");
    });
}

/// The repo's rule tables are read again when the diff moves after the first
/// frame: a remote added under a `[file_kinds]` table, then `R`, and a
/// `kind:` filter keeps out the file that table names.
#[test]
fn the_rule_tables_are_read_again_after_a_refresh() {
    let repo = fixture(3);
    let storage = storage();
    let config = "[file_kinds.\"widgetz\"]\n\"src/f001.rs\" = \"test\"\n";
    write(storage.path(), "config.toml", config);
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = start(vec![path_of(&repo)], None, storage.path());
        let mut codes = vec![KeyCode::Char('f')];
        codes.extend("-kind:test".chars().map(KeyCode::Char));
        codes.push(KeyCode::Enter);
        for code in codes {
            shell.handle_key(key(code));
        }
        let listed = |shell: &Shell| {
            let content = content(shell);
            let key = shell.host().registry().tabs_view().active;
            let state = key.and_then(|key| shell.view().tab(key));
            let visible = state.map(|state| state.visible(&content).len());
            visible.unwrap_or_default()
        };
        assert_eq!(listed(&shell), 3);
        let url = "https://example.invalid/acme/widgetz.git";
        git(repo.path(), &["remote", "add", "origin", url]);
        let before = content(&shell).diff.revision;
        shell.handle_key(key(KeyCode::Char('R')));
        settle(&mut shell, |shell| {
            idle(shell) && content(shell).diff.revision > before
        });
        shell.tick(Instant::now());
        assert_eq!(listed(&shell), 2, "the tables were not read again");
    });
}

/// The file order comes from the service's `listed_files`, called by the
/// view alone; no flag-on module orders files itself.
#[test]
fn the_file_order_comes_from_the_service() {
    for (name, text) in SOURCES {
        let flat: String = text.split_whitespace().collect();
        assert!(!flat.contains("Reverse("), "{name} orders files itself");
        let calls = flat.contains("listed_files(");
        assert_eq!(calls, name == "tui_view.rs", "{name} and listed_files");
    }
}
