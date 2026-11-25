//! The config hub and the AI picker on the flag path: each edit and pick goes
//! through the settings actor, the hub's keys leave the files and the frame
//! the App's keys leave, and a model refresh shows in the open picker.

use std::path::Path;
use std::time::{Duration, Instant};

use crossterm::event::KeyCode;
use er_engine::config::{ConfigFieldValue, ErConfig};
use er_service::SettingsCommand;

use super::parity::assert_json;
use super::shell::Shell;
use super::startup::Args;
use super::tests::{
    app_on, assert_parity, fixture, key, loaded, path_of, plan, press_app, settle, shell_frame,
    shell_on,
};
use super::tui_view::Overlay;
use crate::ui::highlight::Highlighter;

const J: KeyCode = KeyCode::Char('j');

fn temp() -> tempfile::TempDir {
    tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"))
}

fn start(path: &str, config_dir: &Path) -> Shell {
    let args = Args {
        paths: vec![path.to_string()],
        ..Args::default()
    };
    let mut shell = shell_on(super::tests::test_ports(), &plan(&args), config_dir);
    settle(&mut shell, loaded);
    shell
}

fn revision(shell: &Shell) -> u64 {
    shell.host().settings().view().revision
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

fn typed(text: &str) -> Vec<KeyCode> {
    text.chars().map(KeyCode::Char).collect()
}

/// Sends a patch and waits until the actor has applied it.
fn patch(shell: &mut Shell, key: &str, value: ConfigFieldValue) {
    let before = revision(shell);
    shell.send_settings(SettingsCommand::ApplyConfigPatch {
        key: key.into(),
        value,
    });
    settle(shell, |shell| revision(shell) > before);
}

/// The open picker's row labels, and the selected one's.
fn picker(shell: &Shell) -> Option<(Vec<String>, String)> {
    match &shell.view().overlay {
        Some(Overlay::Picker(picker)) => {
            let labels: Vec<String> = picker.rows.iter().map(|row| row.label.clone()).collect();
            let selected = labels.get(picker.selected).cloned().unwrap_or_default();
            Some((labels, selected))
        }
        _ => None,
    }
}

/// Moves down the open picker to the row labelled `label`.
fn select(shell: &mut Shell, label: &str) {
    for _ in 0..40 {
        if picker(shell).is_some_and(|(_, at)| at == label) {
            return;
        }
        shell.handle_key(key(J));
    }
    panic!("no {label:?} row in {:?}", picker(shell));
}

/// Opens the provider picker through the flag-on AI hub's live row, and
/// moves to `provider_id`'s row.
fn open_provider(shell: &mut Shell, provider_id: &str) {
    shell.handle_key(key(KeyCode::Char('a')));
    select(shell, "Change agent/model");
    shell.handle_key(key(KeyCode::Enter));
    let view = shell.host().settings().view();
    let provider = view.config.ai_hub.providers.get(provider_id);
    let label = provider.map_or_else(String::new, |p| p.display_name(provider_id));
    select(shell, &label);
}

/// Ticks until `ready` holds or a few seconds pass; the caller asserts.
fn settle_quietly(shell: &mut Shell, ready: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready() && Instant::now() < deadline {
        shell.tick(Instant::now());
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// A theme cycle and a typed test command, from the hub's keys, move the
/// settings actor's revision and land in its files alone (ADR 0069).
#[test]
fn a_hub_edit_goes_through_the_settings_actor() {
    let repo = fixture(1);
    let (storage, config_dir) = (temp(), temp());
    let path = path_of(&repo);
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = start(&path, config_dir.path());
        let before = revision(&shell);
        let mut keys = vec![KeyCode::Char(','), KeyCode::Char('2')];
        keys.extend([J; 7]);
        keys.extend([KeyCode::Enter, KeyCode::Char('1'), KeyCode::Enter]);
        keys.extend(typed("make"));
        keys.push(KeyCode::Enter);
        for code in keys {
            shell.handle_key(key(code));
        }
        let default_theme = ErConfig::default().display.theme;
        let saved = config_dir.path().join("config.toml");
        settle_quietly(&mut shell, || read(&saved).contains("\"make\""));
        let theme = shell.host().settings().view().config.display.theme.clone();
        assert_ne!(theme, default_theme, "the theme did not cycle");
        assert!(
            revision(&shell) > before,
            "the settings revision did not move"
        );
        let text = read(&saved);
        assert!(text.contains(&theme) && text.contains("\"make\""), "{text}");
        let mut files: Vec<String> = std::fs::read_dir(config_dir.path())
            .unwrap_or_else(|e| panic!("{e}"))
            .map(|entry| entry.map_or_else(|e| panic!("{e}"), |e| e.file_name()))
            .map(|name| name.to_string_lossy().into_owned())
            .collect();
        files.sort();
        assert_eq!(files, ["agents.toml", "config.toml"]);
        assert!(
            !storage.path().join("config.toml").exists(),
            "the App's config file was written"
        );
    });
}

/// The same hub keys (an edit, a toggle, a cycle, a watched path added and
/// removed) leave the App's files and the actor's files equal, and the open
/// hub draws the same.
#[test]
fn hub_keys_match_the_apps() {
    let repo = fixture(1);
    let (storage, config_dir) = (temp(), temp());
    let path = path_of(&repo);
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut app = app_on(&path);
        let mut shell = start(&path, config_dir.path());
        // `app_on` turns context expansion off; the actor's config follows.
        patch(
            &mut shell,
            "display.auto_context",
            ConfigFieldValue::Bool(false),
        );
        let mut steps: Vec<(KeyCode, bool)> = vec![(KeyCode::Char(','), false)];
        let mut add = |codes: Vec<KeyCode>| {
            let count = codes.len();
            steps.extend(
                codes
                    .into_iter()
                    .enumerate()
                    .map(|(at, c)| (c, at + 1 == count)),
            );
        };
        add([vec![KeyCode::Enter], typed("make"), vec![KeyCode::Enter]].concat());
        add(vec![J, J, J, J, KeyCode::Enter]);
        add(vec![J, J, J, J, J, J, J, KeyCode::Enter]);
        add([
            vec![J, KeyCode::Enter],
            typed("src/**"),
            vec![KeyCode::Enter],
        ]
        .concat());
        add(vec![KeyCode::Char('d')]);
        for (code, sends) in steps {
            let before = revision(&shell);
            press_app(&mut app, code);
            shell.handle_key(key(code));
            if sends {
                settle(&mut shell, |shell| revision(shell) > before);
            }
        }
        let files = |dir: &Path| ["config.toml", "agents.toml"].map(|name| read(&dir.join(name)));
        settle_quietly(&mut shell, || {
            files(storage.path()) == files(config_dir.path())
        });
        assert_eq!(files(config_dir.path()), files(storage.path()));
        assert_json("files", &files(config_dir.path()), &files(storage.path()));
        let app_file = &files(storage.path())[0];
        assert!(
            app_file.contains("snapshot"),
            "the cycle did not save: {app_file}"
        );
        let mut hl = Highlighter::new();
        assert_parity(&app, &shell_frame(&mut shell, &mut hl), &mut hl, "hub");
    });
}

/// A Provider cycle in the hub saves the default selection, and the mode
/// row's label and the hub follow the settings view as the App's do.
#[test]
fn a_cycled_provider_saves_and_labels_like_the_app() {
    let repo = fixture(1);
    let (storage, config_dir) = (temp(), temp());
    let path = path_of(&repo);
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut app = app_on(&path);
        let mut shell = start(&path, config_dir.path());
        patch(
            &mut shell,
            "display.auto_context",
            ConfigFieldValue::Bool(false),
        );
        let mut hl = Highlighter::new();
        assert_parity(&app, &shell_frame(&mut shell, &mut hl), &mut hl, "start");
        let mut keys = vec![KeyCode::Char(',')];
        keys.extend([J; 7]);
        keys.push(KeyCode::Enter);
        for code in keys {
            press_app(&mut app, code);
            shell.handle_key(key(code));
        }
        let provider = app.config.ai_hub.default_provider.clone();
        assert!(provider.is_some(), "the App's cycle chose no provider");
        let agents = config_dir.path().join("agents.toml");
        let saved = format!("default_provider = \"{}\"", provider.unwrap_or_default());
        settle_quietly(&mut shell, || read(&agents).contains(&saved));
        assert!(read(&agents).contains(&saved), "{}", read(&agents));
        assert_parity(&app, &shell_frame(&mut shell, &mut hl), &mut hl, "cycle");
        // Then the Model and Effort rows below it, one key each.
        let app_agents = storage.path().join("agents.toml");
        for step in ["model", "effort"] {
            for code in [J, KeyCode::Enter] {
                press_app(&mut app, code);
                shell.handle_key(key(code));
            }
            settle_quietly(&mut shell, || read(&agents) == read(&app_agents));
            assert_eq!(read(&agents), read(&app_agents), "{step}");
            assert_json(step, &read(&agents), &read(&app_agents));
            assert_parity(&app, &shell_frame(&mut shell, &mut hl), &mut hl, step);
        }
        assert!(
            app.config.ai_hub.default_effort.is_some(),
            "no effort was pinned"
        );
    });
}

/// A pick in the picker saves the default selection through the settings
/// actor, and the status line names it.
#[test]
fn a_pick_saves_through_the_settings_actor() {
    let repo = fixture(1);
    let (storage, config_dir) = (temp(), temp());
    let path = path_of(&repo);
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = start(&path, config_dir.path());
        // No provider CLI runs: the picker asks for no refresh.
        patch(
            &mut shell,
            "features.model_discovery",
            ConfigFieldValue::Bool(false),
        );
        open_provider(&mut shell, "codex");
        for _ in 0..3 {
            if picker(&shell).is_some() {
                shell.handle_key(key(KeyCode::Enter));
            }
        }
        assert!(picker(&shell).is_none(), "the pick left a picker open");
        let agents = config_dir.path().join("agents.toml");
        settle(&mut shell, |shell| {
            let note = shell.view().notification.as_deref().unwrap_or_default();
            read(&agents).contains("default_provider = \"codex\"")
                && note.starts_with("AI target: ")
        });
        let view = shell.host().settings().view();
        assert_eq!(
            view.config.ai_hub.default_provider.as_deref(),
            Some("codex")
        );
    });
}

/// Opening a provider's models refreshes them through the settings actor,
/// and the open picker lists the discovered model with no further key; its
/// Refresh row runs the listing again.
#[test]
fn a_model_refresh_shows_in_the_open_picker() {
    let repo = fixture(1);
    let (storage, config_dir, bin) = (temp(), temp(), temp());
    let path = path_of(&repo);
    let runs = bin.path().join("runs");
    let script = bin.path().join("models.sh");
    let body = format!(
        "echo run >> '{}'\necho 'fresh-model - Fresh Model'\n",
        runs.display()
    );
    std::fs::write(&script, body).unwrap_or_else(|e| panic!("{e}"));
    let agents = format!(
        "[ai_hub.providers.fake]\ncommand = \"fake\"\nmodels_command = [\"sh\", \"{}\"]\n\
         [[ai_hub.providers.fake.models]]\nid = \"one\"\n\
         [[ai_hub.providers.fake.models]]\nid = \"two\"\n",
        script.display()
    );
    std::fs::write(config_dir.path().join("agents.toml"), agents).unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = start(&path, config_dir.path());
        assert!(
            shell
                .host()
                .settings()
                .view()
                .config
                .features
                .model_discovery
        );
        open_provider(&mut shell, "fake");
        shell.handle_key(key(KeyCode::Enter));
        settle(&mut shell, |shell| {
            picker(shell).is_some_and(|(rows, _)| rows.iter().any(|row| row == "Fresh Model"))
        });
        select(&mut shell, "Refresh models");
        shell.handle_key(key(KeyCode::Enter));
        assert!(picker(&shell).is_none(), "the refresh left the picker open");
        settle(&mut shell, |_| read(&runs).lines().count() == 2);
        // Opened again on a fresh cache, the picker's refresh lists nothing.
        let before = revision(&shell);
        open_provider(&mut shell, "fake");
        shell.handle_key(key(KeyCode::Enter));
        settle(&mut shell, |shell| revision(shell) > before);
        assert_eq!(read(&runs).lines().count(), 2, "opening forced a listing");
    });
}

/// The hub's tab keys, a cancelled edit with its cursor keys, Left on a
/// cycle row and the close key leave the frame and the files the App's
/// keys leave.
#[test]
fn tab_cancel_back_and_close_keys_match_the_apps() {
    let repo = fixture(1);
    let (storage, config_dir) = (temp(), temp());
    let path = path_of(&repo);
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut app = app_on(&path);
        let mut shell = start(&path, config_dir.path());
        patch(
            &mut shell,
            "display.auto_context",
            ConfigFieldValue::Bool(false),
        );
        let mut hl = Highlighter::new();
        let mut keys = vec![
            KeyCode::Char(','),
            KeyCode::Char('2'),
            KeyCode::Char(']'),
            KeyCode::Char('['),
            KeyCode::Char('1'),
            J,
            KeyCode::Char('k'),
            KeyCode::Enter,
        ];
        keys.extend(typed("xy"));
        keys.extend([
            KeyCode::Backspace,
            KeyCode::Left,
            KeyCode::Right,
            KeyCode::Esc,
        ]);
        keys.extend([J; 7]);
        for code in keys {
            press_app(&mut app, code);
            shell.handle_key(key(code));
            shell.tick(Instant::now());
            let step = format!("{code:?}");
            assert_parity(&app, &shell_frame(&mut shell, &mut hl), &mut hl, &step);
        }
        let agents = config_dir.path().join("agents.toml");
        let app_agents = storage.path().join("agents.toml");
        for code in [KeyCode::Left, KeyCode::Left] {
            press_app(&mut app, code);
            shell.handle_key(key(code));
            settle_quietly(&mut shell, || read(&agents) == read(&app_agents));
            assert_eq!(read(&agents), read(&app_agents), "Left");
            assert_json("Left", &read(&agents), &read(&app_agents));
        }
        press_app(&mut app, KeyCode::Char('s'));
        shell.handle_key(key(KeyCode::Char('s')));
        assert!(shell.view().overlay.is_none(), "s left the hub open");
        assert_parity(&app, &shell_frame(&mut shell, &mut hl), &mut hl, "closed");
    });
}

/// The open picker's kind.
fn picker_kind(shell: &Shell) -> Option<crate::screen::view::HubKind> {
    match shell.view().overlay.as_ref() {
        Some(Overlay::Picker(picker)) => Some(picker.kind()),
        _ => None,
    }
}

/// Backspace and Esc on the effort list go back to the model list, as the
/// App's `pop_ai_effort_picker`; Esc on the model list closes it.
#[test]
fn the_effort_list_goes_back_to_the_models() {
    use crate::screen::view::HubKind;
    let repo = fixture(1);
    let (storage, config_dir) = (temp(), temp());
    let path = path_of(&repo);
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = start(&path, config_dir.path());
        patch(
            &mut shell,
            "features.model_discovery",
            ConfigFieldValue::Bool(false),
        );
        let view = shell.host().settings().view();
        let (provider_id, model) = view
            .config
            .ai_hub
            .providers
            .iter()
            .filter(|(_, provider)| provider.models.len() > 1)
            .find_map(|(id, provider)| {
                let model = provider
                    .models
                    .iter()
                    .find(|m| !m.effort_levels.is_empty())?;
                Some((id.clone(), model.display_name()))
            })
            .unwrap_or_else(|| panic!("no provider lists a model with efforts"));
        open_provider(&mut shell, &provider_id);
        shell.handle_key(key(KeyCode::Enter));
        assert_eq!(picker_kind(&shell), Some(HubKind::AiModel));
        for _ in 0..8 {
            shell.handle_key(key(KeyCode::Char('k')));
        }
        select(&mut shell, &model);
        for back in [KeyCode::Backspace, KeyCode::Esc] {
            shell.handle_key(key(KeyCode::Enter));
            assert_eq!(picker_kind(&shell), Some(HubKind::AiEffort));
            shell.handle_key(key(back));
            assert_eq!(picker_kind(&shell), Some(HubKind::AiModel), "{back:?}");
        }
        shell.handle_key(key(KeyCode::Esc));
        assert_eq!(picker_kind(&shell), None);
    });
}
