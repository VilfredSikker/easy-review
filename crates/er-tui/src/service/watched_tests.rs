//! The flag-on watched files (P69.2): the list and the snapshot diff are
//! read on the reader thread and draw as the App draws them; a snapshot
//! update and a delete go through the service.

use std::path::Path;
use std::time::{Duration, Instant};

use crossterm::event::KeyCode;
use er_engine::config::ConfigFieldValue;
use er_engine::tab_identity::DiffMode;
use er_service::{Command, SettingsCommand};

use super::shell::Shell;
use super::startup::Args;
use super::tests::{
    app_on, assert_parity, fixture, key, loaded, path_of, press_app, settle, settle_within, shell,
    shell_frame, WAIT,
};
use super::watched::TabWatched;
use crate::screen::watched::SnapshotDiff;
use crate::ui::highlight::Highlighter;

/// A repo with one changed file, and `a.env` and an older `b.env` that git
/// ignores and the config watches in snapshot mode. The list is newest
/// first, so `a.env` leads it and stays first when it is edited.
fn watched_repo(storage: &Path) -> tempfile::TempDir {
    let repo = fixture(1);
    std::fs::write(repo.path().join(".git/info/exclude"), "*.env\n")
        .unwrap_or_else(|e| panic!("{e}"));
    for (name, age) in [("a.env", 0), ("b.env", 3600)] {
        let path = repo.path().join(name);
        std::fs::write(&path, "A=1\n").unwrap_or_else(|e| panic!("{e}"));
        let at = std::time::SystemTime::now() - Duration::from_secs(age);
        let file = std::fs::File::options().write(true).open(&path);
        file.and_then(|file| file.set_modified(at))
            .unwrap_or_else(|e| panic!("{e}"));
    }
    let config = "[watched]\npaths = [\"*.env\"]\ndiff_mode = \"snapshot\"\n";
    std::fs::write(storage.join("config.toml"), config).unwrap_or_else(|e| panic!("{e}"));
    repo
}

fn start(path: &str, storage: &Path) -> Shell {
    let args = Args {
        paths: vec![path.to_string()],
        ..Args::default()
    };
    let mut shell = shell(&args, storage);
    settle(&mut shell, loaded);
    settle(&mut shell, |shell| {
        watched(shell).is_some_and(|w| !w.files.is_empty())
    });
    shell
}

fn watched(shell: &Shell) -> Option<&TabWatched> {
    let key = shell.host().registry().tabs_view().active?;
    shell.view().tab(key).map(|state| &state.watched)
}

fn showing(shell: &Shell, want: fn(&SnapshotDiff) -> bool) -> bool {
    watched(shell)
        .and_then(TabWatched::snapshot)
        .is_some_and(want)
}

fn unchanged(diff: &SnapshotDiff) -> bool {
    matches!(diff, SnapshotDiff::Unchanged)
}

fn notice(shell: &Shell) -> String {
    shell.view().notification.clone().unwrap_or_default()
}

fn selected_path(shell: &Shell) -> String {
    let file = watched(shell).and_then(TabWatched::selected_file);
    file.map(|file| file.path.clone()).unwrap_or_default()
}

/// The bucket's snapshot of `name`.
fn snapshot_of(shell: &Shell, name: &str) -> std::path::PathBuf {
    let content = shell.active_content().unwrap_or_else(|| panic!("no tab"));
    Path::new(&content.context.er_dir)
        .join("snapshots")
        .join(name)
}

/// The list draws, then moving into it shows the new file's notice, then
/// its unchanged diff, the diff of an edit, the update `s` saves, and `W`
/// hiding and showing the list, each as the App draws it.
#[test]
fn the_watched_files_draw_like_the_app() {
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let repo = watched_repo(storage.path());
    let path = path_of(&repo);
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut app = app_on(&path);
        let mut shell = start(&path, storage.path());
        let mut hl = Highlighter::new();
        assert_parity(&app, &shell_frame(&mut shell, &mut hl), &mut hl, "listed");

        press_app(&mut app, KeyCode::Char('k'));
        shell.handle_key(key(KeyCode::Char('k')));
        settle(&mut shell, |shell| {
            showing(shell, |diff| matches!(diff, SnapshotDiff::Created))
        });
        let flag_on = shell_frame(&mut shell, &mut hl);
        // The App's draw reads the diff and saves the snapshot it finds
        // missing; it must find none, as the flag path's read did.
        let selected = selected_path(&shell);
        let _ = std::fs::remove_file(snapshot_of(&shell, &selected));
        assert_parity(&app, &flag_on, &mut hl, "created");

        settle(&mut shell, |shell| showing(shell, unchanged));
        assert_parity(
            &app,
            &shell_frame(&mut shell, &mut hl),
            &mut hl,
            "unchanged",
        );

        std::fs::write(repo.path().join(&selected), "A=2\n").unwrap_or_else(|e| panic!("{e}"));
        // Both rescan, the App as its loop does every five seconds.
        app.tab_mut().refresh_watched_files();
        shell.tick(Instant::now() + Duration::from_secs(6));
        settle(&mut shell, |shell| {
            showing(shell, |diff| matches!(diff, SnapshotDiff::Changed(_)))
        });
        assert_parity(&app, &shell_frame(&mut shell, &mut hl), &mut hl, "changed");

        press_app(&mut app, KeyCode::Char('s'));
        shell.handle_key(key(KeyCode::Char('s')));
        settle(&mut shell, |shell| {
            notice(shell) == "Snapshot updated" && showing(shell, unchanged)
        });
        assert_parity(&app, &shell_frame(&mut shell, &mut hl), &mut hl, "updated");

        for (step, listed) in [("hidden", false), ("shown", true)] {
            app.clear_notification();
            shell.tick(Instant::now() + Duration::from_secs(60));
            press_app(&mut app, KeyCode::Char('W'));
            shell.handle_key(key(KeyCode::Char('W')));
            settle(&mut shell, |shell| {
                watched(shell).is_some_and(|w| w.files.is_empty() != listed)
            });
            assert_parity(&app, &shell_frame(&mut shell, &mut hl), &mut hl, step);
        }
    });
}

/// `s` on a watched file saves its snapshot through the service, and the
/// next read shows the file unchanged.
#[test]
fn a_snapshot_update_goes_through_the_service() {
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let repo = watched_repo(storage.path());
    let path = path_of(&repo);
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = start(&path, storage.path());
        shell.handle_key(key(KeyCode::Char('k')));
        settle(&mut shell, |shell| showing(shell, unchanged));

        let selected = selected_path(&shell);
        std::fs::write(repo.path().join(&selected), "A=2\n").unwrap_or_else(|e| panic!("{e}"));
        // A forced rescan; the next one is not due for a while after it.
        shell.tick(Instant::now() + Duration::from_secs(6));
        settle(&mut shell, |shell| {
            showing(shell, |diff| matches!(diff, SnapshotDiff::Changed(_)))
        });

        shell.handle_key(key(KeyCode::Char('s')));
        settle(&mut shell, |shell| notice(shell) == "Snapshot updated");
        let saved = std::fs::read_to_string(snapshot_of(&shell, &selected)).unwrap_or_default();
        assert_eq!(saved, "A=2\n", "the snapshot is not the file");
        let reread = settle_within(&mut shell, Duration::from_secs(3), |shell| {
            showing(shell, unchanged)
        });
        assert!(reread, "the update was not read again");
    });
}

/// In Hidden, `x` then `y` deletes the selected watched file on both paths,
/// and the frames match after.
#[test]
fn a_watched_delete_writes_and_draws_like_the_app() {
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let repo = watched_repo(storage.path());
    let path = path_of(&repo);
    let a_env = repo.path().join("a.env");
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut app = app_on(&path);
        app.tab_mut().set_mode(DiffMode::Hidden);
        press_app(&mut app, KeyCode::Char('x'));
        press_app(&mut app, KeyCode::Char('y'));
        assert!(!a_env.exists(), "the App did not delete");
        std::fs::write(&a_env, "A=1\n").unwrap_or_else(|e| panic!("{e}"));

        let mut shell = start(&path, storage.path());
        let tabs = shell.host().registry().tabs_view();
        let entry = tabs.active_entry().unwrap_or_else(|| panic!("no tab"));
        let command = Command::SetDiffMode {
            key: entry.key,
            mode: DiffMode::Hidden,
            pr_number: None,
        };
        let workspace = entry.descriptor.workspace_key();
        let reply = shell.host().post(&workspace, command).recv_timeout(WAIT);
        assert!(matches!(reply, Ok(Ok(_))), "{reply:?}");
        settle(&mut shell, |shell| {
            watched(shell).is_some_and(|w| w.selected == Some(0))
        });
        shell.handle_key(key(KeyCode::Char('x')));
        shell.handle_key(key(KeyCode::Char('y')));
        settle(&mut shell, |shell| notice(shell) == "Deleted: a.env");
        assert!(!a_env.exists(), "the flag path did not delete");
        settle(&mut shell, |shell| {
            watched(shell).is_some_and(|w| w.files.len() == 1)
                && showing(shell, |diff| matches!(diff, SnapshotDiff::Created))
        });
        let mut hl = Highlighter::new();
        let flag_on = shell_frame(&mut shell, &mut hl);
        let _ = std::fs::remove_file(snapshot_of(&shell, "b.env"));
        assert_parity(&app, &flag_on, &mut hl, "deleted");
    });
}

/// Sends a watched-paths patch through the settings actor, after a forced
/// rescan, so the five-second rescan is not due for the next few seconds.
fn patch_paths(shell: &mut Shell, key: &str, value: ConfigFieldValue) {
    shell.tick(Instant::now() + Duration::from_secs(6));
    shell.send_settings(SettingsCommand::ApplyConfigPatch {
        key: key.into(),
        value,
    });
}

/// A hub edit that adds a watched path lists its files on the next ticks,
/// without waiting for the five-second rescan.
#[test]
fn a_watched_path_added_in_the_hub_lists_at_once() {
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let repo = watched_repo(storage.path());
    std::fs::write(repo.path().join("n.log"), "N=1\n").unwrap_or_else(|e| panic!("{e}"));
    let path = path_of(&repo);
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = start(&path, storage.path());
        let pattern = ConfigFieldValue::String("*.log".into());
        patch_paths(&mut shell, "watched.paths.add", pattern);
        let listed = settle_within(&mut shell, Duration::from_secs(3), |shell| {
            watched(shell).is_some_and(|w| w.files.iter().any(|f| f.path == "n.log"))
        });
        assert!(listed, "the new path waited for the rescan");
    });
}

/// A hub edit that leaves no watched paths clears the shown list and its
/// selection, as the App's rescan does.
#[test]
fn emptied_watched_paths_clear_the_list() {
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let repo = watched_repo(storage.path());
    let path = path_of(&repo);
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = start(&path, storage.path());
        // Hidden, then shown again: `W` now decides, as after the App's `W`.
        for code in [KeyCode::Char('W'), KeyCode::Char('W')] {
            shell.handle_key(key(code));
        }
        settle(&mut shell, |shell| {
            watched(shell).is_some_and(|w| !w.files.is_empty())
        });
        shell.handle_key(key(KeyCode::Char('k')));
        assert!(watched(&shell).is_some_and(|w| w.selected.is_some()));
        patch_paths(
            &mut shell,
            "watched.paths.remove",
            ConfigFieldValue::Number(0),
        );
        let cleared = settle_within(&mut shell, Duration::from_secs(3), |shell| {
            watched(shell).is_some_and(|w| w.files.is_empty() && w.selected.is_none())
        });
        assert!(cleared, "the old list still draws");
    });
}
