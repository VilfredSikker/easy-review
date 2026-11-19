//! The flag-on `settings` resource and the settings writes through
//! `er_send`: one writer, the rollback's version, the push, the notices.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use er_protocol::{
    Command, ConfigFieldValue, ConfigHubField, ErErrorKind, NoticeLevel, ProviderUpsert, Resource,
    ResourceChanged, ResourceKey, ResourceVersion, Settings,
};

use super::{review_flow_test_state, while_the_app_is_locked, ReviewFlowTestDir};
use crate::commands::AppState;
use crate::resources::relay;
use crate::resources::send::{apply, Applied};
use crate::resources::{self, Watched};
use crate::service::ServiceState;

const WAIT: Duration = Duration::from_secs(5);

fn with_storage(test: impl FnOnce(&ReviewFlowTestDir)) {
    let dir = ReviewFlowTestDir::new();
    let storage = dir.path().join("storage");
    er_engine::storage::with_process_test_storage_root(storage, || test(&dir));
}

fn config_path(dir: &ReviewFlowTestDir) -> std::path::PathBuf {
    dir.path().join("settings").join("config.toml")
}

fn theme(value: &str) -> Command {
    Command::ApplyConfigPatch {
        key: "display.theme".into(),
        value: ConfigFieldValue::String(value.into()),
    }
}

fn send(state: &AppState, command: Command) -> Result<Applied, er_protocol::ErError> {
    apply(&state.desktop, command, &|_, _| {})
}

fn fetch_settings(state: &AppState) -> (ResourceVersion, Settings) {
    let fetched = resources::fetch(&state.desktop, ResourceKey::Settings)
        .unwrap_or_else(|e| panic!("settings fetch: {e:?}"));
    match fetched.value {
        Resource::Settings(settings) => (fetched.version, *settings),
        Resource::Tabs(tabs) => panic!("not a settings resource: {tabs:?}"),
    }
}

fn theme_row(settings: &Settings) -> Option<String> {
    settings
        .settings
        .general
        .iter()
        .find_map(|field| match field {
            ConfigHubField::Cycle { key, value, .. } if key == "display.theme" => {
                Some(value.clone())
            }
            _ => None,
        })
}

/// A settings write through `er_send` has one writer, the settings actor
/// (ADR 0069): it returns while the App lock is held, the resource and the
/// file carry it at the version its `Ack` names, and the App's copy does not.
#[test]
fn a_settings_write_through_er_send_has_the_actor_as_its_one_writer() {
    with_storage(|dir| {
        let state = review_flow_test_state(dir);
        let before = state.app.lock().map(|app| app.config.display.theme.clone());
        let applied =
            while_the_app_is_locked(&state, "er_send", |state| send(state, theme("paper")));
        let ack = applied
            .unwrap_or_else(|e| panic!("the write failed: {e:?}"))
            .ack;

        let (version, settings) = fetch_settings(&state);
        assert_eq!(theme_row(&settings).as_deref(), Some("paper"));
        assert_eq!(
            ack.changed,
            vec![ResourceChanged {
                key: ResourceKey::Settings,
                version,
            }]
        );
        let written = std::fs::read_to_string(config_path(dir)).unwrap_or_default();
        assert!(written.contains("paper"), "config.toml: {written}");
        let after = state.app.lock().map(|app| app.config.display.theme.clone());
        assert_eq!(after.ok(), before.ok(), "the App's copy was written");
    });
}

/// A save the file refuses rolls the view back, and the rollback publishes
/// past the version the refused write was shown at. A page that fetched the
/// refused value refetches only on a higher version, so a rollback at the
/// same version would leave it on the refused value.
#[test]
fn a_refused_save_rolls_back_at_a_higher_version() {
    with_storage(|dir| {
        let state = review_flow_test_state(dir);
        assert!(send(&state, theme("first-theme")).is_ok());
        let (saved, _) = fetch_settings(&state);
        assert!(std::fs::write(config_path(dir), "not = = toml").is_ok());

        let refused = send(&state, theme("while-broken"));
        assert_eq!(
            refused.map(|applied| applied.ack).map_err(|e| e.kind),
            Err(ErErrorKind::Refused)
        );
        let (version, settings) = fetch_settings(&state);
        assert_eq!(theme_row(&settings).as_deref(), Some("first-theme"));
        // The refused write was published at the next version before its
        // save failed; the rollback must move past it.
        assert!(
            version.0 > saved.0 + 1,
            "rolled back at {version:?}, the refused write was shown at {}",
            saved.0 + 1
        );
    });
}

/// Drops every null, so a wire field the old answer left out compares equal.
fn without_nulls(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => map
            .into_iter()
            .filter(|(_, v)| !v.is_null())
            .map(|(k, v)| (k, without_nulls(v)))
            .collect(),
        serde_json::Value::Array(items) => items.into_iter().map(without_nulls).collect(),
        other => other,
    }
}

/// The resource answers what the flag-off `get_config_hub` answers for the
/// same config, so the page reads one shape in both builds.
#[test]
fn the_settings_resource_matches_the_flag_off_answer() {
    with_storage(|dir| {
        let state = review_flow_test_state(dir);
        let (_, settings) = fetch_settings(&state);
        let view = state.service.settings().view();
        let old = crate::settings::GetConfigHubResponse::from_view(view.hub.clone(), Vec::new());
        let mut old = without_nulls(serde_json::to_value(old).unwrap_or_default());
        let mut new = without_nulls(serde_json::to_value(settings).unwrap_or_default());
        if let Some(old) = old.as_object_mut() {
            old.remove("warnings");
        }
        if let Some(new) = new.as_object_mut() {
            new.remove("uninstallPreview");
        }
        assert!(old
            .get("providers")
            .is_some_and(|p| p != &serde_json::json!([])));
        assert_eq!(new, old);
    });
}

/// A settings write moves `settings` on the wire, once something has read
/// it: the settings actor's publishes wake the relay.
#[test]
fn a_settings_write_reaches_the_wire() {
    with_storage(|dir| {
        let (wake, wakes) = mpsc::channel();
        let mut state = review_flow_test_state(dir);
        state.service = Arc::new(
            ServiceState::with_settings_path(er_engine::ports::Ports::for_test(), config_path(dir))
                .with_resource_wake(wake),
        );
        let _ = fetch_settings(&state);
        std::thread::sleep(Duration::from_millis(100));
        while wakes.try_recv().is_ok() {}
        let (tx, bursts) = mpsc::channel();
        relay::spawn(state.desktop.clone(), wakes, move |moved| {
            let _ = tx.send(moved);
        });

        let ack = send(&state, theme("paper"))
            .unwrap_or_else(|e| panic!("{e:?}"))
            .ack;
        let moved = bursts.recv_timeout(WAIT).expect("no burst after the write");
        assert!(
            moved
                .iter()
                .any(|(watched, _)| *watched == Watched::Settings),
            "{moved:?}"
        );
        assert_eq!(relay::wire(&moved), ack.changed);
    });
}

/// An editor's soft warning reaches the person as one notice, since the
/// `Ack` carries only versions.
#[test]
fn an_editor_warning_arrives_as_a_notice() {
    with_storage(|dir| {
        let state = review_flow_test_state(dir);
        let notices = Mutex::new(Vec::new());
        let upsert = Command::UpsertAiProvider {
            provider: ProviderUpsert {
                id: "local".into(),
                original_id: None,
                label: None,
                command: "/bin/echo".into(),
                args: "--no-prompt".into(),
                family: None,
                models_command: None,
            },
        };
        let applied = apply(&state.desktop, upsert, &|level, message| {
            if let Ok(mut notices) = notices.lock() {
                notices.push((level, message));
            }
        });
        assert!(applied.is_ok(), "{applied:?}");
        let notices = notices.into_inner().unwrap_or_default();
        assert_eq!(notices.len(), 1, "{notices:?}");
        assert_eq!(notices[0].0, NoticeLevel::Info);
        assert!(notices[0].1.contains("{prompt}"), "{notices:?}");
    });
}
