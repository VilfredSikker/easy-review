//! The settings chrome on the flag path: the theme, the load banner and the
//! AI selection label follow the settings actor's view, a settings write
//! goes through that actor alone, and a failure shows once.

use std::path::Path;
use std::time::{Duration, Instant};

use er_engine::config::{ConfigFieldValue, CONFIG_LOAD_BANNER};
use er_service::SettingsCommand;

use super::shell::Shell;
use super::startup::Args;
use super::tests::{
    app_on, assert_parity, fixture, loaded, path_of, plan, rows, settle, shell_frame, shell_on,
};
use crate::ui::highlight::Highlighter;

fn temp() -> tempfile::TempDir {
    tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"))
}

fn args(path: &str) -> Args {
    Args {
        paths: vec![path.to_string()],
        ..Args::default()
    }
}

fn revision(shell: &Shell) -> u64 {
    shell.host().settings().view().revision
}

fn codex_pick() -> SettingsCommand {
    SettingsCommand::SetDefaultAiSelection {
        provider_id: "codex".into(),
        model_id: Some("gpt-6-sol".into()),
        effort: Some("high".into()),
    }
}

fn files_in(dir: &Path) -> Vec<String> {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{e}"));
    let mut names: Vec<String> = entries
        .map(|entry| {
            let entry = entry.unwrap_or_else(|e| panic!("{e}"));
            entry.file_name().to_string_lossy().into_owned()
        })
        .collect();
    names.sort();
    names
}

/// A settings write from the TUI moves the settings actor's revision and
/// lands in the actor's own files, and in no config file of the App's.
#[test]
fn a_settings_write_goes_through_the_settings_actor() {
    let repo = fixture(1);
    let (storage, config_dir) = (temp(), temp());
    let path = path_of(&repo);
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = shell_on(
            super::tests::test_ports(),
            &plan(&args(&path)),
            config_dir.path(),
        );
        settle(&mut shell, loaded);
        let before = revision(&shell);
        shell.send_settings(codex_pick());
        settle(&mut shell, |_| {
            let saved = std::fs::read_to_string(config_dir.path().join("agents.toml"));
            // `agents.toml` is written first, so `config.toml` ends the save.
            saved.is_ok_and(|text| text.contains("gpt-6-sol"))
                && config_dir.path().join("config.toml").exists()
        });
        assert!(
            revision(&shell) > before,
            "the settings revision did not move"
        );
        assert_eq!(files_in(config_dir.path()), ["agents.toml", "config.toml"]);
        assert!(
            !storage.path().join("config.toml").exists(),
            "the App's config file was written"
        );
    });
}

/// A save the file refuses answers both as an event and as a reply; the
/// status line shows it once, and once it clears the banner says saving is
/// off.
#[test]
fn a_refused_save_shows_once_and_raises_the_banner() {
    let repo = fixture(1);
    let (storage, config_dir) = (temp(), temp());
    let path = path_of(&repo);
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = shell_on(
            super::tests::test_ports(),
            &plan(&args(&path)),
            config_dir.path(),
        );
        settle(&mut shell, loaded);
        std::fs::write(config_dir.path().join("config.toml"), "not = = toml")
            .unwrap_or_else(|e| panic!("{e}"));
        shell.send_settings(SettingsCommand::ApplyConfigPatch {
            key: "display.theme".into(),
            value: ConfigFieldValue::String("paper".into()),
        });
        let failure = |shell: &Shell| shell.host().settings().view().last_error.clone();
        settle(&mut shell, |shell| {
            failure(shell).is_some_and(|message| shell.settings().times_shown(&message) > 0)
        });
        // Both the event and the reply have landed by now.
        let deadline = Instant::now() + Duration::from_millis(300);
        while Instant::now() < deadline {
            shell.tick(Instant::now());
            std::thread::sleep(Duration::from_millis(10));
        }
        let message = failure(&shell).unwrap_or_default();
        assert_eq!(shell.settings().times_shown(&message), 1, "{message}");
        assert_eq!(shell.view().notification.as_deref(), Some(message.as_str()));

        shell.tick(Instant::now() + Duration::from_secs(60));
        let mut hl = Highlighter::new();
        let frame = rows(&shell_frame(&mut shell, &mut hl)).join("\n");
        assert!(frame.contains(CONFIG_LOAD_BANNER), "{frame}");
    });
}

/// Sends a theme write the broken file refuses, waits for its event and its
/// reply, and answers the failure.
fn refused_write(shell: &mut Shell) -> String {
    let before = revision(shell);
    shell.send_settings(SettingsCommand::ApplyConfigPatch {
        key: "display.theme".into(),
        value: ConfigFieldValue::String("paper".into()),
    });
    settle(shell, |shell| revision(shell) > before);
    let deadline = Instant::now() + Duration::from_millis(300);
    while Instant::now() < deadline {
        shell.tick(Instant::now());
        std::thread::sleep(Duration::from_millis(10));
    }
    let view = shell.host().settings().view();
    view.last_error.clone().unwrap_or_default()
}

/// One refused command shows once, though it answers as an event and as a
/// reply; the same refusal from a second command shows again.
#[test]
fn a_repeated_failure_shows_once_per_command() {
    let repo = fixture(1);
    let (storage, config_dir) = (temp(), temp());
    let path = path_of(&repo);
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = shell_on(
            super::tests::test_ports(),
            &plan(&args(&path)),
            config_dir.path(),
        );
        settle(&mut shell, loaded);
        std::fs::write(config_dir.path().join("config.toml"), "not = = toml")
            .unwrap_or_else(|e| panic!("{e}"));
        // The first refusal finds the file broken; later ones name the mark.
        refused_write(&mut shell);
        let message = refused_write(&mut shell);
        assert_eq!(shell.settings().times_shown(&message), 1, "{message}");
        assert_eq!(refused_write(&mut shell), message);
        assert_eq!(shell.settings().times_shown(&message), 2, "{message}");
    });
}

/// A failed first load's reply, kept for the tab's error, takes its event's
/// place, so a later reply-only failure with the same text still shows.
#[test]
fn a_kept_load_reply_pairs_with_its_own_event() {
    let repo = fixture(1);
    let storage = temp();
    let start = plan(&args(&path_of(&repo)));
    std::fs::remove_dir_all(repo.path().join(".git")).unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = shell_on(super::tests::test_ports(), &start, storage.path());
        let mut hl = Highlighter::new();
        settle(&mut shell, |shell| shell.view().notification.is_some());
        let deadline = Instant::now() + Duration::from_secs(20);
        while !rows(&shell_frame(&mut shell, &mut hl))
            .join("\n")
            .contains("did not load")
            && Instant::now() < deadline
        {
            shell.tick(Instant::now());
            std::thread::sleep(Duration::from_millis(10));
        }
        let message = shell.view().notification.clone().unwrap_or_default();
        assert_eq!(shell.settings().times_shown(&message), 1, "{message}");
        shell.land_failed_reply(message.clone());
        assert_eq!(shell.settings().times_shown(&message), 2, "{message}");
    });
}

/// A config that fails to load shows its full message once at start, then
/// keeps the file's banner up, as the App does.
#[test]
fn a_failed_load_draws_the_apps_notice_and_banner() {
    let _golden = super::parity::guard();
    let repo = fixture(1);
    let storage = temp();
    let path = path_of(&repo);
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        std::fs::write(storage.path().join("config.toml"), "not = = toml")
            .unwrap_or_else(|e| panic!("{e}"));
        let mut app = app_on(&path);
        let mut shell = shell_on(
            super::tests::test_ports(),
            &plan(&args(&path)),
            storage.path(),
        );
        settle(&mut shell, loaded);
        let shown_at = Instant::now();
        let mut hl = Highlighter::new();
        assert_parity(&app, &shell_frame(&mut shell, &mut hl), &mut hl, "notice");
        // The App's long notice stays up 5 s; a short one would be gone.
        shell.tick(shown_at + Duration::from_millis(4000));
        assert!(shell.view().notification.is_some(), "the notice left early");
        app.clear_notification();
        shell.tick(Instant::now() + Duration::from_secs(60));
        assert_parity(&app, &shell_frame(&mut shell, &mut hl), &mut hl, "banner");
    });
}

/// A theme set through the settings actor redraws the frame in it, as the
/// App draws its own config's theme.
#[test]
fn the_theme_follows_the_settings_view() {
    let _golden = super::parity::guard();
    let repo = fixture(1);
    let storage = temp();
    let path = path_of(&repo);
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut app = app_on(&path);
        let mut shell = shell_on(
            super::tests::test_ports(),
            &plan(&args(&path)),
            storage.path(),
        );
        settle(&mut shell, loaded);
        let mut hl = Highlighter::new();
        assert_parity(&app, &shell_frame(&mut shell, &mut hl), &mut hl, "start");
        app.config.display.theme = "paper".into();
        shell.send_settings(SettingsCommand::ApplyConfigPatch {
            key: "display.theme".into(),
            value: ConfigFieldValue::String("paper".into()),
        });
        settle(&mut shell, |shell| {
            shell.host().settings().view().config.display.theme == "paper"
        });
        assert_parity(&app, &shell_frame(&mut shell, &mut hl), &mut hl, "theme");
    });
}

/// A saved AI pick shows on the mode row as the App labels the same pick.
#[test]
fn the_saved_selection_follows_the_view() {
    let _golden = super::parity::guard();
    let repo = fixture(1);
    let storage = temp();
    let path = path_of(&repo);
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut app = app_on(&path);
        let mut shell = shell_on(
            super::tests::test_ports(),
            &plan(&args(&path)),
            storage.path(),
        );
        settle(&mut shell, loaded);
        let mut hl = Highlighter::new();
        assert_parity(&app, &shell_frame(&mut shell, &mut hl), &mut hl, "start");
        app.set_ai_default_selection_with_effort("codex", Some("gpt-6-sol"), Some("high"))
            .unwrap_or_else(|e| panic!("{e}"));
        shell.send_settings(codex_pick());
        settle(&mut shell, |shell| {
            let view = shell.host().settings().view();
            view.config.ai_hub.default_provider.as_deref() == Some("codex")
        });
        assert_parity(&app, &shell_frame(&mut shell, &mut hl), &mut hl, "pick");
    });
}
