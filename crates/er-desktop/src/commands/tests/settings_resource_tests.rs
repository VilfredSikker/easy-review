//! The flag-on `settings` resource and the settings writes through
//! `er_send`: one writer, the rollback's version, the push, the notices.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use er_protocol::{
    Command, ConfigFieldValue, ConfigHubField, ErErrorKind, NoticeLevel, ProviderUpsert, Resource,
    ResourceChanged, ResourceKey, ResourceVersion, RiskTier, Settings,
};

use super::{on_a_thread, review_flow_test_state, ReviewFlowTestDir};
use crate::resources::relay;
use crate::resources::send::{apply, Applied, Effects};
use crate::resources::{self, Watched};
use crate::service::ServiceState;
use crate::state::DesktopState;

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

/// Effects that reach nothing outside the owner but `notify`.
fn effects(notify: &dyn Fn(NoticeLevel, String)) -> Effects<'_> {
    Effects {
        notify,
        check_update: || panic!("no update check in this test"),
        webviews: &crate::browser_webview::NoWebviews,
        pick_folder: &|| None,
    }
}

fn send(state: &DesktopState, command: Command) -> Result<Applied, er_protocol::ErError> {
    apply(&state, command, &effects(&|_, _| {}))
}

fn fetch_settings(state: &DesktopState) -> (ResourceVersion, Settings) {
    let fetched = resources::fetch(&state, ResourceKey::Settings)
        .unwrap_or_else(|e| panic!("settings fetch: {e:?}"));
    match fetched.value {
        Resource::Settings(settings) => (fetched.version, *settings),
        other => panic!("not a settings resource: {other:?}"),
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
/// (ADR 0069): the resource and the file carry it at the version its `Ack`
/// names.
#[test]
fn a_settings_write_through_er_send_has_the_actor_as_its_one_writer() {
    with_storage(|dir| {
        let state = review_flow_test_state(dir);
        let applied = on_a_thread(&state, "er_send", |state| send(state, theme("paper")));
        let ack = applied
            .unwrap_or_else(|e| panic!("the write failed: {e:?}"))
            .ack;

        let (version, settings) = fetch_settings(&state);
        assert_eq!(theme_row(&settings).as_deref(), Some("paper"));
        assert_eq!(
            ack.changed.first(),
            Some(&ResourceChanged {
                key: ResourceKey::Settings,
                version,
            })
        );
        let written = std::fs::read_to_string(config_path(dir)).unwrap_or_default();
        assert!(written.contains("paper"), "config.toml: {written}");
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

/// The `importance_*` fields are gone from the settings resource.
#[test]
fn the_settings_resource_has_no_importance_keys() {
    fn keys(value: &serde_json::Value, out: &mut Vec<String>) {
        match value {
            serde_json::Value::Object(map) => {
                for (key, child) in map {
                    out.push(key.clone());
                    keys(child, out);
                }
            }
            serde_json::Value::Array(items) => items.iter().for_each(|item| keys(item, out)),
            _ => {}
        }
    }

    with_storage(|dir| {
        let state = review_flow_test_state(dir);
        let (_, settings) = fetch_settings(&state);
        let json = serde_json::to_value(settings).unwrap_or_default();
        let mut found = Vec::new();
        keys(&json, &mut found);
        assert!(found.iter().any(|key| key == "general"), "{found:?}");
        let importance: Vec<&String> = found
            .iter()
            .filter(|key| key.to_lowercase().contains("importance"))
            .collect();
        assert!(importance.is_empty(), "importance keys: {importance:?}");
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
        relay::spawn(state.clone(), wakes, move |moved| {
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

/// A risk read goes through `er_send` and `er_fetch` without the App lock:
/// the load names the rules key it resolved, and `risk(key)` answers the
/// rules with the load's paths counted and resolved. A key no load named is
/// not found.
#[test]
fn a_risk_read_is_a_load_then_a_fetch_without_the_app() {
    with_storage(|dir| {
        let state = review_flow_test_state(dir);
        let rule = r#"{"source":"user","pattern":"db/**","tier":"high"}"#;
        let ruled = send(
            &state,
            Command::ApplyConfigPatch {
                key: er_engine::config::RISK_RULE_PATCH_KEY.into(),
                value: ConfigFieldValue::String(rule.into()),
            },
        );
        assert!(ruled.is_ok(), "{ruled:?}");
        let root = dir.path().join("repo").to_string_lossy().into_owned();
        let (ack, fetched) = on_a_thread(&state, "the risk read", move |state| {
            let ack = send(
                state,
                Command::LoadRisk {
                    repo_root: root,
                    remote_repo: None,
                    paths: vec!["db/schema.sql".into(), "src/a.rs".into()],
                },
            )
            .unwrap_or_else(|e| panic!("the load failed: {e:?}"))
            .ack;
            let key = ack.created.clone().unwrap_or_default();
            (ack, resources::fetch(&state, ResourceKey::Risk { key }))
        });
        let fetched = fetched.unwrap_or_else(|e| panic!("risk fetch: {e:?}"));
        assert_eq!(
            ack.changed,
            vec![ResourceChanged {
                key: fetched.key.clone(),
                version: fetched.version,
            }]
        );
        let Resource::Risk(risk) = fetched.value else {
            panic!("not a risk resource: {:?}", fetched.value);
        };
        let tiers: Vec<_> = risk
            .files
            .iter()
            .map(|f| (f.path.as_str(), f.tier))
            .collect();
        assert_eq!(
            tiers,
            [
                ("db/schema.sql", RiskTier::High),
                ("src/a.rs", RiskTier::Ai)
            ]
        );
        assert_eq!(risk.rules.iter().map(|r| r.file_count).sum::<usize>(), 1);

        let unknown = resources::fetch(
            &state,
            ResourceKey::Risk {
                key: "never-loaded".into(),
            },
        );
        assert_eq!(
            unknown.map(|_| ()).map_err(|e| e.kind),
            Err(ErErrorKind::NotFound)
        );
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
        let notify = |level, message| {
            if let Ok(mut notices) = notices.lock() {
                notices.push((level, message));
            }
        };
        let applied = apply(&state, upsert, &effects(&notify));
        assert!(applied.is_ok(), "{applied:?}");
        let notices = notices.into_inner().unwrap_or_default();
        assert_eq!(notices.len(), 1, "{notices:?}");
        assert_eq!(notices[0].0, NoticeLevel::Info);
        assert!(notices[0].1.contains("{prompt}"), "{notices:?}");
    });
}
