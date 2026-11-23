//! The `session` and `panels` resources, the AI picks through `er_send`, the
//! update check and the panel mirror.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use er_engine::config::{AiModelConfig, AiProviderConfig};
use er_protocol::{
    AppUpdate, Command, NoticeLevel, Panel, Resource, ResourceChanged, ResourceKey,
    ResourceVersion, Session, Settings,
};

use super::{on_a_thread, review_flow_test_state, ReviewFlowTestDir};
use crate::page_view::WatchStatusSnapshot;
use crate::resources;
use crate::resources::send::{apply, Applied, Effects};
use crate::service::ServiceState;
use crate::state::DesktopState;

const WAIT: Duration = Duration::from_secs(20);

fn with_storage(test: impl FnOnce(&ReviewFlowTestDir)) {
    let dir = ReviewFlowTestDir::new();
    let storage = dir.path().join("storage");
    er_engine::storage::with_process_test_storage_root(storage, || test(&dir));
}

fn model(id: &str) -> AiModelConfig {
    AiModelConfig {
        id: id.into(),
        label: Some(format!("{id} label")),
        // The provider prints its arguments, so the reply names the model.
        args: vec![format!("mark-{id}")],
        effort_levels: vec!["low".into(), "high".into()],
        ..Default::default()
    }
}

/// A provider that answers with its own arguments, two models it tells
/// apart, and a listing command; saved as the managed config pair, which the
/// settings actor owns and the ask runs read.
fn echo_state(dir: &ReviewFlowTestDir) -> DesktopState {
    let mut state = review_flow_test_state(dir);
    let path = er_engine::config::managed_config_path();
    {
        let mut config = er_engine::config::ErConfig::default();
        let provider = AiProviderConfig {
            label: Some("Echo".into()),
            command: "/bin/sh".into(),
            args: vec![
                "-c".into(),
                "printf '%s\\n' \"$@\"".into(),
                "echo-agent".into(),
                "{prompt}".into(),
            ],
            models: vec![model("first"), model("second")],
            models_command: vec![
                "/bin/sh".into(),
                "-c".into(),
                "printf 'listed-a\\nlisted-b\\n'".into(),
            ],
            ..Default::default()
        };
        config.ai_hub.providers.insert("echo".into(), provider);
        config.ai_hub.default_provider = Some("echo".into());
        config.ai_hub.default_model = Some("first".into());
        config.ai_hub.default_effort = Some("high".into());
        config.display.theme = "paper".into();
        config.display.split_diff = true;
        config.display.tab_width = 2;
        config.features.view_tour = false;
        config.features.model_discovery = true;
        assert!(er_engine::config::save_config_pair_to(&path, &config).is_ok());
    }
    state.service = Arc::new(ServiceState::with_settings_path(
        er_engine::ports::Ports::for_test(),
        path,
    ));
    let _ = state.service.settings().view();
    state
}

fn effects(notify: &dyn Fn(NoticeLevel, String)) -> Effects<'_> {
    Effects {
        notify,
        check_update: || panic!("no update check in this test"),
        webviews: &crate::browser_webview::NoWebviews,
        pick_folder: &|| None,
    }
}

fn send(state: &DesktopState, command: Command) -> Applied {
    apply(state, command, &effects(&|_, _| {})).unwrap_or_else(|e| panic!("er_send failed: {e:?}"))
}

fn pick_second() -> Command {
    Command::SetAiSelection {
        provider_id: "echo".into(),
        model_id: Some("second".into()),
        effort: None,
    }
}

fn fetch_session(state: &DesktopState) -> (ResourceVersion, Session) {
    let fetched = resources::fetch(state, ResourceKey::Session)
        .unwrap_or_else(|e| panic!("session fetch: {e:?}"));
    match fetched.value {
        Resource::Session(session) => (fetched.version, *session),
        other => panic!("not a session resource: {other:?}"),
    }
}

fn fetch_settings(state: &DesktopState) -> Settings {
    match resources::fetch(state, ResourceKey::Settings).map(|f| f.value) {
        Ok(Resource::Settings(settings)) => *settings,
        other => panic!("not a settings resource: {other:?}"),
    }
}

fn wait_until(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The picker's providers and the session labels answer the pick just
/// saved: both read the settings actor's default selection.
#[test]
fn the_picker_answers_the_saved_pick_on_a_thread() {
    with_storage(|dir| {
        let state = echo_state(dir);
        let applied = send(&state, pick_second());
        let (providers, (_, session)) = on_a_thread(&state, "the picker's reads", |state| {
            (fetch_settings(state).providers, fetch_session(state))
        });
        let echo = providers.iter().find(|p| p.id == "echo");
        let selected: Vec<_> = echo
            .map(|p| {
                p.models
                    .iter()
                    .filter(|m| m.is_selected)
                    .map(|m| m.id.as_str())
                    .collect()
            })
            .unwrap_or_default();
        assert!(echo.is_some_and(|p| p.is_selected), "{providers:?}");
        assert_eq!(selected, ["second"]);
        assert_eq!(
            session.active_ai_model_label.as_deref(),
            Some("second label")
        );
        assert!(
            session.active_ai_label.starts_with("Echo / second label"),
            "{}",
            session.active_ai_label
        );

        let (version, _) = fetch_session(&state);
        assert!(
            applied.ack.changed.contains(&ResourceChanged {
                key: ResourceKey::Session,
                version,
            }),
            "the pick's Ack names session: {:?}",
            applied.ack.changed
        );
    });
}

/// The wire's pick carries no persist flag, and every pick is saved by the
/// settings actor: it is in `agents.toml` once the `Ack` arrives.
#[test]
fn a_pick_through_er_send_is_saved() {
    with_storage(|dir| {
        let state = echo_state(dir);
        let _ = send(&state, pick_second());
        let _ = send(
            &state,
            Command::SetAiEffort {
                effort: Some("low".into()),
            },
        );
        let path = er_engine::config::managed_config_path().with_file_name("agents.toml");
        let agents = std::fs::read_to_string(path).unwrap_or_default();
        assert!(agents.contains("default_model = \"second\""), "{agents}");
        assert!(agents.contains("default_effort = \"low\""), "{agents}");
    });
}

/// A watcher write that changes the status moves `session`, so the page's
/// watch badge follows the push; one that changes nothing does not.
#[test]
fn a_watch_status_change_moves_session() {
    with_storage(|dir| {
        let state = echo_state(dir);
        let (before, _) = fetch_session(&state);
        let watching = WatchStatusSnapshot {
            active: true,
            branch: Some("feature".into()),
            root_path: Some("/work/feature".into()),
        };
        state
            .session
            .set_watch_status(&state.watch_status, watching.clone());
        let (moved, session) = fetch_session(&state);
        assert!(moved > before, "{moved:?} after {before:?}");
        assert_eq!(session.watch_status.branch.as_deref(), Some("feature"));
        state
            .session
            .set_watch_status(&state.watch_status, watching);
        assert_eq!(fetch_session(&state).0, moved, "an unchanged status moved");
    });
}

/// The mirror of a toggle moves `panels`, and its `Ack` names the version
/// that holds it.
#[test]
fn a_panel_toggle_moves_panels() {
    with_storage(|dir| {
        let state = echo_state(dir);
        let before =
            resources::version(&state, ResourceKey::Panels).unwrap_or_else(|e| panic!("{e:?}"));
        let ack = send(&state, Command::TogglePanel { panel: Panel::Left }).ack;
        let fetched =
            resources::fetch(&state, ResourceKey::Panels).unwrap_or_else(|e| panic!("{e:?}"));
        assert!(
            fetched.version > before,
            "{:?} after {before:?}",
            fetched.version
        );
        assert_eq!(
            ack.changed,
            vec![ResourceChanged {
                key: ResourceKey::Panels,
                version: fetched.version,
            }]
        );
        assert!(matches!(fetched.value, Resource::Panels(p) if !p.left));
    });
}

/// The update check answers at once and its result lands in `session` at a
/// higher version.
#[test]
fn an_update_check_lands_in_session() {
    with_storage(|dir| {
        let state = echo_state(dir);
        let (before, _) = fetch_session(&state);
        let applied = apply(
            &state,
            Command::CheckAppUpdate,
            &Effects {
                notify: &|_, _| {},
                check_update: || AppUpdate {
                    current: "1.0.0".into(),
                    latest: Some("9.9.9".into()),
                    update_available: true,
                    release_url: Some("https://example.invalid/r".into()),
                },
                webviews: &crate::browser_webview::NoWebviews,
                pick_folder: &|| None,
            },
        );
        assert!(applied.is_ok(), "{applied:?}");
        wait_until("the update check", || {
            fetch_session(&state).1.update.is_some()
        });
        let (after, session) = fetch_session(&state);
        assert!(after > before, "{after:?} after {before:?}");
        assert_eq!(
            session.update.and_then(|u| u.latest).as_deref(),
            Some("9.9.9")
        );
    });
}

/// A model listing that worked says how many models the provider has, as
/// the flag-off page did; the `Ack` carries no count.
#[test]
fn a_model_refresh_names_the_count() {
    with_storage(|dir| {
        let state = echo_state(dir);
        let notices = Mutex::new(Vec::new());
        let notify = |level, message| {
            if let Ok(mut notices) = notices.lock() {
                notices.push((level, message));
            }
        };
        let applied = apply(
            &state,
            Command::RefreshAiModels {
                provider_id: Some("echo".into()),
                force: true,
            },
            &effects(&notify),
        );
        assert!(applied.is_ok(), "{applied:?}");
        let notices = notices.into_inner().unwrap_or_default();
        let count = fetch_settings(&state)
            .providers
            .iter()
            .find(|p| p.id == "echo")
            .map_or(0, |p| p.models.len());
        assert!(count > 2, "the listing added models: {count}");
        assert_eq!(
            notices,
            vec![(NoticeLevel::Success, format!("Echo: {count} models"))]
        );
    });
}
