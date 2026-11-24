//! P71.1 on the flag path.

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

/// One key on both paths, then frame parity.
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

/// `f001` and `f002` edited, `stage` staged, `other` at the tip, set mtimes.
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

/// Mode keys and the mtime sort draw as the App's.
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

/// A watched change refreshes its tab, with the App's notices.
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

/// The base row and input rediff the tab and keep its base.
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

/// `--target` is every start tab's base.
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

/// A session saved in another view restores there, as the App's.
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

/// The rule tables are read again after a refresh moves the rules key.
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

/// Only the view orders files, through `listed_files`.
#[test]
fn the_file_order_comes_from_the_service() {
    for (name, text) in SOURCES {
        let flat: String = text.split_whitespace().collect();
        assert!(!flat.contains("Reverse("), "{name} orders files itself");
        let calls = flat.contains("listed_files(");
        assert_eq!(calls, name == "tui_view.rs", "{name} and listed_files");
    }
}

/// Commits a change to `file` and tells the watcher about it.
fn change(shell: &mut Shell, root: &std::path::Path, file: &str) {
    write(root, file, "let watched = 1;\n");
    git(root, &["commit", "-qam", "watched"]);
    let path = format!("{}/{file}", path_of_dir(root));
    let sender = shell.watch_mut().sender();
    sender
        .send(WatchEvent::FilesChanged(vec![path]))
        .unwrap_or_else(|e| panic!("{e}"));
}

fn path_of_dir(root: &std::path::Path) -> String {
    let canonical = std::fs::canonicalize(root).unwrap_or_else(|e| panic!("{e}"));
    canonical.to_string_lossy().into_owned()
}

fn shows(content: &er_service::TabContent, file: &str) -> bool {
    let diff = content.diff.files.iter().find(|f| f.path == file);
    diff.is_some_and(|f| format!("{:?}", f.hunks).contains("watched"))
}

const MOVED: &str = "the tab changed while it loaded";

/// One watcher refresh out per tab, and one more for what came meanwhile.
#[test]
fn one_watcher_refresh_is_out_per_tab() {
    let repo = fixture(2);
    let storage = storage();
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = start(vec![path_of(&repo)], None, storage.path());
        let content = content(&shell);
        let key = shell.host().registry().tabs_view().active;
        let key = key.unwrap_or_else(|| panic!("no tab"));
        // The watch alone, on its own clock.
        let mut watch = super::watcher::Watch::default();
        watch.on = false;
        let t0 = Instant::now();
        watch.follow(Some((key, &content)), t0);
        let sender = watch.sender();
        let event = || WatchEvent::FilesChanged(vec!["a".into()]);
        sender.send(event()).unwrap_or_else(|e| panic!("{e}"));
        watch.count(t0);
        let later = |secs| t0 + Duration::from_secs(secs);
        assert_eq!(watch.ready(later(1)), Some((key, 1)));
        sender.send(event()).unwrap_or_else(|e| panic!("{e}"));
        watch.count(later(1));
        assert_eq!(watch.ready(later(2)), None, "a second refresh went out");
        watch.landed(key);
        assert_eq!(watch.ready(later(2)), Some((key, 1)));
        assert_eq!(watch.ready(later(3)), None);

        // The shell, with two changes in a row.
        let start = Instant::now();
        change(&mut shell, repo.path(), "src/f000.rs");
        shell.tick(start);
        shell.tick(start + Duration::from_secs(1));
        change(&mut shell, repo.path(), "src/f001.rs");
        shell.tick(start + Duration::from_secs(2));
        shell.tick(start + Duration::from_secs(3));
        assert!(shell.in_flight() <= 1, "two watcher refreshes are out");
        let both = |shell: &Shell| {
            let content = super::modes_tests::content(shell);
            idle(shell) && shows(&content, "src/f000.rs") && shows(&content, "src/f001.rs")
        };
        let deadline = Instant::now() + Duration::from_secs(20);
        while !both(&shell) && Instant::now() < deadline {
            shell.tick(Instant::now() + Duration::from_secs(3));
            let notice = shell.view().notification.clone().unwrap_or_default();
            assert!(!notice.contains(MOVED), "{notice}");
        }
        assert!(both(&shell), "the second change was not refreshed");
    });
}

/// A reload refused as moved is sent again silently.
#[test]
fn a_reload_refused_as_moved_is_sent_again() {
    let repo = fixture(2);
    let storage = storage();
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = start(vec![path_of(&repo)], None, storage.path());
        for _ in 0..3 {
            shell.handle_key(key(KeyCode::Char('R')));
            shell.handle_key(key(KeyCode::Char('R')));
            let deadline = Instant::now() + Duration::from_secs(20);
            while !idle(&shell) && Instant::now() < deadline {
                shell.tick(Instant::now());
                let notice = shell.view().notification.clone().unwrap_or_default();
                assert!(!notice.contains(MOVED), "{notice}");
            }
            assert_eq!(shell.view().notification.as_deref(), Some("Refreshed"));
        }
    });
}

/// Changes refresh the tab they came on; a tab shown again is refreshed.
#[test]
fn changes_refresh_the_tab_they_were_counted_on() {
    let (first, second) = (fixture(2), fixture(2));
    let storage = storage();
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let paths = vec![path_of(&first), path_of(&second)];
        let mut shell = start(paths, None, storage.path());
        shell.tick(Instant::now());
        let tabs = shell.host().registry().tabs_view();
        let tab = |at: usize| {
            let entry = &tabs.tabs[at];
            (entry.key, entry.descriptor.workspace_key())
        };
        let ((one, one_ws), (two, two_ws)) = (tab(0), tab(1));
        let of = |shell: &Shell, key, workspace: &str| {
            let content = shell.host().content(workspace, key);
            content.unwrap_or_else(|| panic!("tab not loaded"))
        };
        change(&mut shell, first.path(), "src/f000.rs");
        shell.tick(Instant::now());
        shell.handle_key(key(KeyCode::Char(']')));
        settle(&mut shell, |shell| {
            idle(shell) && shows(&of(shell, one, &one_ws), "src/f000.rs")
        });
        settle(&mut shell, |shell| {
            shell.host().content(&two_ws, two).is_some()
        });
        shell.tick(Instant::now());
        change(&mut shell, second.path(), "src/f000.rs");
        let refreshed = settle_within(&mut shell, Duration::from_secs(20), |shell| {
            idle(shell) && shows(&of(shell, two, &two_ws), "src/f000.rs")
        });
        assert!(refreshed, "the shown tab's change was not refreshed");
        // A change on the first tab while the second is watched.
        write(first.path(), "src/f001.rs", "let watched = 1;\n");
        git(first.path(), &["commit", "-qam", "unwatched"]);
        shell.watch_mut().on = true;
        shell.handle_key(key(KeyCode::Char('[')));
        // Before the file system could report anything, the re-arm sent one.
        shell.tick(Instant::now());
        assert_eq!(
            shell.in_flight(),
            1,
            "showing the tab again refreshed nothing"
        );
        let caught = settle_within(&mut shell, Duration::from_secs(20), |shell| {
            idle(shell) && shows(&of(shell, one, &one_ws), "src/f001.rs")
        });
        shell.watch_mut().on = false;
        assert!(caught, "the tab shown again was not refreshed");
    });
}

/// A refused refresh leaves a switch alone; a refused restore switch keeps
/// the filter and toggles.
#[test]
fn a_refused_restore_switch_keeps_the_filter_and_toggles() {
    let repo = fixture(3);
    let storage = storage();
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = start(vec![path_of(&repo)], None, storage.path());
        let content = content(&shell);
        let entry = shell.host().registry().tabs_view().active_entry().cloned();
        let entry = entry.unwrap_or_else(|| panic!("no tab"));
        let state = shell.view_mut().tab_mut(entry.key, &content);
        let mut session = super::session::capture(&content, state);
        session.filter_expr = "f001".into();
        session.show_unreviewed_only = true;
        state.restore = Some(super::session::Restore {
            mode: DiffMode::Staged,
            session,
            sent: true,
        });
        state.follow = Some(super::modes::Follow::of(state, &content));
        let workspace = entry.descriptor.workspace_key();
        // A refused refresh leaves the switch's follow and the session alone.
        let refresh = super::startup::After::Refreshed(super::startup::Refresh::Key);
        shell.lose_write(entry.key, workspace.clone(), refresh);
        shell.tick(Instant::now());
        let view = shell.view().tab(entry.key);
        let waiting = view.is_some_and(|state| state.follow.is_some() && state.restore.is_some());
        assert!(waiting, "a refused refresh let go of the switch's follow");
        let after = super::startup::After::ModeSwitched { pr_diff: false };
        shell.lose_write(entry.key, workspace, after);
        shell.tick(Instant::now());
        let state = shell
            .view()
            .tab(entry.key)
            .unwrap_or_else(|| panic!("no view"));
        assert!(state.restore.is_none());
        assert_eq!(state.filter_expr, "f001");
        assert!(state.show_unreviewed_only);
    });
}

/// A reload refused as moved every time is sent four times in all, then
/// the refusal shows, and nothing more is sent.
#[test]
fn a_moved_reload_is_resent_three_times_at_most() {
    let repo = fixture(1);
    let storage = storage();
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = start(vec![path_of(&repo)], None, storage.path());
        let moved = "the tab changed while it loaded; refresh it again";
        assert!(er_service::refused_as_moved(moved));
        shell.refuse_reloads = Some(moved.into());
        shell.handle_key(key(KeyCode::Char('R')));
        for _ in 0..10 {
            shell.tick(Instant::now());
        }
        assert_eq!(shell.reload_sends, 4);
        assert_eq!(shell.view().notification.as_deref(), Some(moved));
        assert_eq!(shell.in_flight(), 0);
    });
}
