//! The settings actor: live rows, the file split, and versioned risk views.
//!
//! Every actor here gets an explicit config path in a temp dir and runs under a
//! process-wide temp storage root, so neither the actor nor its effect threads
//! can reach the owner's files.

use std::path::Path;
use std::time::Duration;

use er_engine::config::{ConfigFieldValue, ConfigHubFieldDto, RISK_RULE_PATCH_KEY};
use er_engine::ports::Ports;

use crate::actor::WorkspaceDeps;
use crate::settings::{spawn_settings, SaveSlot, SettingsCommand, SettingsHandle, SettingsReply};

const DEADLINE: Duration = Duration::from_secs(10);

fn in_temp_storage(test: impl FnOnce(&Path) -> anyhow::Result<()>) -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let config = tempfile::tempdir()?;
    er_engine::storage::with_process_test_storage_root(root.path(), || test(config.path()))
}

fn actor(dir: &Path) -> SettingsHandle {
    spawn_settings(
        WorkspaceDeps::new(Ports::for_test()),
        dir.join("config.toml"),
    )
}

fn patch(handle: &SettingsHandle, key: &str, value: ConfigFieldValue) -> anyhow::Result<()> {
    handle.send_and_wait_with(
        SettingsCommand::ApplyConfigPatch {
            key: key.to_string(),
            value,
        },
        DEADLINE,
    )?;
    Ok(())
}

fn text(value: &str) -> ConfigFieldValue {
    ConfigFieldValue::String(value.to_string())
}

/// The published rows carry the actor's config after a patch, not a config
/// rebuilt from scratch.
#[test]
fn the_config_hub_view_carries_the_live_rows() -> anyhow::Result<()> {
    in_temp_storage(|dir| {
        let handle = actor(dir);
        let before = handle.view().revision;
        patch(&handle, "display.theme", text("paper"))?;

        let view = handle.view();
        assert!(view.revision > before, "the patch published a revision");
        let theme = view
            .hub
            .fields
            .general
            .iter()
            .find_map(|field| match field {
                ConfigHubFieldDto::Cycle { key, value, .. } if key == "display.theme" => {
                    Some(value.clone())
                }
                _ => None,
            });
        assert_eq!(theme.as_deref(), Some("paper"));
        assert_eq!(view.config.display.theme, "paper");
        Ok(())
    })
}

/// A write through the service lands in the file its section lives in.
#[test]
fn a_settings_write_lands_in_the_right_file() -> anyhow::Result<()> {
    in_temp_storage(|dir| {
        let handle = actor(dir);
        let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap_or_default();

        patch(&handle, "agent.command", text("fixture-agent"))?;
        assert!(read("agents.toml").contains("fixture-agent"));
        assert!(!read("config.toml").contains("fixture-agent"));

        patch(&handle, "display.theme", text("ember"))?;
        assert!(read("config.toml").contains("theme = \"ember\""));
        assert!(!read("agents.toml").contains("ember"));
        Ok(())
    })
}

/// A risk view's version rises once per change to its rules, and a read or
/// a re-read with nothing changed keeps it.
#[test]
fn the_risk_view_versions_once_per_change() -> anyhow::Result<()> {
    in_temp_storage(|dir| {
        let handle = actor(dir);
        let load = || {
            handle.send_and_wait_with(
                SettingsCommand::LoadRiskRules {
                    repo_root: "/work/er".to_string(),
                    remote_repo: None,
                    paths: Vec::new(),
                },
                DEADLINE,
            )
        };
        let SettingsReply::RiskRulesLoaded { key, version } = load()? else {
            anyhow::bail!("the load answered with another reply");
        };
        // No remote is scripted, so the key falls back to the root's name.
        assert_eq!(key, "er");

        let edit = r#"{"source":"project","repo":"er","pattern":"db/**","tier":"high"}"#;
        patch(&handle, RISK_RULE_PATCH_KEY, text(edit))?;

        let view = handle.view();
        let risk = view.risk_for("/work/er", None).expect("the repo's view");
        assert_eq!(risk.version, version + 1, "one change, one version");
        assert!(risk.rules.iter().any(|rule| rule.pattern == "db/**"));

        assert_eq!(
            handle.view().risk_for("/work/er", None).map(|r| r.version),
            Some(version + 1),
            "a second read keeps the version"
        );
        let SettingsReply::RiskRulesLoaded { version: again, .. } = load()? else {
            anyhow::bail!("the reload answered with another reply");
        };
        assert_eq!(again, version + 1, "a re-read with no change keeps it");
        Ok(())
    })
}

/// The paths a load resolves are part of the risk view: new paths raise its
/// version and are resolved in it, the same paths again keep it.
#[test]
fn a_risk_load_with_new_paths_raises_the_version() -> anyhow::Result<()> {
    in_temp_storage(|dir| {
        let handle = actor(dir);
        let edit = r#"{"source":"user","pattern":"db/**","tier":"high"}"#;
        patch(&handle, RISK_RULE_PATCH_KEY, text(edit))?;
        let load = |paths: &[&str]| -> anyhow::Result<(String, u64)> {
            match handle.send_and_wait_with(
                SettingsCommand::LoadRiskRules {
                    repo_root: "/work/er".to_string(),
                    remote_repo: None,
                    paths: paths.iter().map(|p| (*p).to_string()).collect(),
                },
                DEADLINE,
            )? {
                SettingsReply::RiskRulesLoaded { key, version } => Ok((key, version)),
                other => anyhow::bail!("the load answered {other:?}"),
            }
        };
        let (key, first) = load(&["src/a.rs"])?;
        let (_, moved) = load(&["db/schema.sql", "src/a.rs"])?;
        assert_eq!(moved, first + 1, "new paths, a new version");
        let view = handle.view();
        let files = view.risk_files.get(&key).expect("the resolved paths");
        let paths: Vec<&str> = files.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["db/schema.sql", "src/a.rs"]);
        let (_, again) = load(&["db/schema.sql", "src/a.rs"])?;
        assert_eq!(again, moved, "the same paths keep the version");
        Ok(())
    })
}

/// An edit the config refuses publishes nothing and saves nothing.
#[test]
fn a_refused_edit_changes_nothing() -> anyhow::Result<()> {
    in_temp_storage(|dir| {
        let handle = actor(dir);
        let before = handle.view().revision;
        let result = handle.send_and_wait_with(
            SettingsCommand::DeleteAiModel {
                provider_id: "no-such-provider".to_string(),
                model_id: "m".to_string(),
            },
            DEADLINE,
        );
        assert!(result.is_err());
        assert_eq!(handle.view().revision, before);
        assert!(!dir.join("config.toml").exists());
        Ok(())
    })
}

/// Breaks `file` after a good save, repairs it with `repaired` swapped in for
/// `first`, and checks that reopening Settings adopts the repair and unlocks
/// saves. The broken file is found by a save, not a load, so the actor's own
/// load error stays empty throughout.
fn recover_from_a_file_broken_mid_session(
    dir: &Path,
    file: &str,
    key: &str,
    first: &str,
    repaired: &str,
    adopted: impl Fn(&er_engine::config::ErConfig) -> String,
) -> anyhow::Result<()> {
    let handle = actor(dir);
    patch(&handle, key, text(first))?;
    let path = dir.join(file);
    let good = std::fs::read_to_string(&path)?;
    assert!(good.contains(first), "the first save landed in {file}");

    std::fs::write(&path, "not = = toml")?;
    let refused = patch(&handle, key, text("while-broken"));
    assert!(refused.is_err(), "a save over a broken {file} is refused");
    assert!(handle.view().load_error.is_none());

    std::fs::write(&path, good.replace(first, repaired))?;
    handle.send_and_wait_with(SettingsCommand::ReloadIfFailed, DEADLINE)?;
    let view = handle.view();
    assert!(view.load_error.is_none(), "the repaired file loads");
    assert_eq!(adopted(&view.config), repaired, "the repair is adopted");

    patch(&handle, key, text("after-repair"))?;
    assert!(std::fs::read_to_string(&path)?.contains("after-repair"));
    Ok(())
}

/// A config.toml that breaks mid-session is reloaded once repaired.
#[test]
fn a_config_file_broken_mid_session_recovers_on_reopen() -> anyhow::Result<()> {
    in_temp_storage(|dir| {
        recover_from_a_file_broken_mid_session(
            dir,
            "config.toml",
            "display.theme",
            "first-theme",
            "repaired-theme",
            |config| config.display.theme.clone(),
        )
    })
}

/// An agents.toml that breaks mid-session is reloaded once repaired.
#[test]
fn an_agents_file_broken_mid_session_recovers_on_reopen() -> anyhow::Result<()> {
    in_temp_storage(|dir| {
        recover_from_a_file_broken_mid_session(
            dir,
            "agents.toml",
            "agent.command",
            "first-agent",
            "repaired-agent",
            |config| config.agent.command.clone(),
        )
    })
}

fn theme_row(handle: &SettingsHandle) -> Option<String> {
    handle
        .view()
        .hub
        .fields
        .general
        .iter()
        .find_map(|field| match field {
            ConfigHubFieldDto::Cycle { key, value, .. } if key == "display.theme" => {
                Some(value.clone())
            }
            _ => None,
        })
}

/// A save the file refuses leaves the published view equal to the disk: the
/// refused edit is undone, and the failure is shown.
#[test]
fn a_refused_save_leaves_the_view_as_saved() -> anyhow::Result<()> {
    in_temp_storage(|dir| {
        let handle = actor(dir);
        patch(&handle, "display.theme", text("first-theme"))?;
        let path = dir.join("config.toml");
        std::fs::write(&path, "not = = toml")?;

        let refused = patch(&handle, "display.theme", text("while-broken"));
        assert!(refused.is_err(), "a save over a broken file is refused");
        let view = handle.view();
        assert_eq!(view.config.display.theme, "first-theme");
        assert_eq!(theme_row(&handle).as_deref(), Some("first-theme"));
        assert!(view.last_error.is_some(), "the refusal is shown");
        assert_eq!(std::fs::read_to_string(&path)?, "not = = toml");
        Ok(())
    })
}

fn themed(theme: &str) -> er_engine::config::ErConfig {
    let mut config = er_engine::config::ErConfig::default();
    config.display.theme = theme.to_string();
    config
}

/// A write still in flight when a rollback dropped it reports an error once it
/// runs, even after a later save reached the disk.
#[test]
fn a_write_in_flight_during_a_rollback_reports_an_error() -> anyhow::Result<()> {
    in_temp_storage(|dir| {
        let path = dir.join("config.toml");
        let slot = SaveSlot::new(&themed("loaded"));
        slot.stage(1, &themed("one"));
        slot.stage(2, &themed("two"));

        let on_disk = slot.reset(2, None);
        assert_eq!(on_disk.map(|c| c.display.theme).as_deref(), Some("loaded"));

        slot.stage(4, &themed("four"));
        assert_eq!(slot.write_latest(&path, 4), Ok(()));
        assert!(
            slot.write_latest(&path, 2).is_err(),
            "the dropped write must not report success"
        );
        assert!(slot.write_latest(&path, 1).is_err());
        let (saved, error) = er_engine::config::adopt_config_from(&path);
        assert!(error.is_none());
        assert_eq!(saved.display.theme, "four");
        Ok(())
    })
}

/// A failed load publishes the failed file's banner beside the full message;
/// a save that finds the file broken later shows the generic banner, and a
/// repaired reload clears it.
#[test]
fn the_load_banner_follows_the_load_and_a_refused_save() -> anyhow::Result<()> {
    in_temp_storage(|dir| {
        let path = dir.join("config.toml");
        std::fs::write(&path, "not = = toml")?;
        let handle = actor(dir);
        let view = handle.view();
        assert_eq!(
            view.load_banner.as_deref(),
            Some("config.toml failed to load; saving is disabled until it is reloaded")
        );
        assert!(view
            .load_error
            .as_deref()
            .is_some_and(|e| e.contains("could not be loaded")));

        std::fs::remove_file(&path)?;
        handle.send_and_wait_with(SettingsCommand::ReloadIfFailed, DEADLINE)?;
        assert_eq!(handle.view().load_banner, None, "the reload clears it");

        patch(&handle, "display.theme", text("first-theme"))?;
        std::fs::write(&path, "not = = toml")?;
        assert!(patch(&handle, "display.theme", text("broken")).is_err());
        assert_eq!(
            handle.view().load_banner.as_deref(),
            Some(er_engine::config::CONFIG_LOAD_BANNER)
        );
        Ok(())
    })
}

/// A save refused because only `agents.toml` broke names that file in the
/// banner, not `config.toml`.
#[test]
fn a_refused_save_names_the_file_that_broke() -> anyhow::Result<()> {
    in_temp_storage(|dir| {
        let path = dir.join("config.toml");
        let handle = actor(dir);
        patch(&handle, "display.theme", text("first-theme"))?;
        std::fs::write(er_engine::config::agents_path(&path), "{")?;
        assert!(patch(&handle, "display.theme", text("broken")).is_err());
        let banner = handle.view().load_banner.clone().unwrap_or_default();
        assert_eq!(
            banner,
            "agents.toml failed to load; saving is disabled until it is reloaded"
        );
        Ok(())
    })
}

/// The actor starts with each provider's cached discovered models, as the
/// App does, and runs no discovery to get them.
#[test]
fn the_actor_starts_with_the_cached_models() -> anyhow::Result<()> {
    in_temp_storage(|dir| {
        let (config, _) = er_engine::config::adopt_config_from(&dir.join("absent.toml"));
        let (id, provider) = config
            .ai_hub
            .providers
            .iter()
            .find(|(_, provider)| !provider.models_command.is_empty())
            .ok_or_else(|| anyhow::anyhow!("no provider lists its models"))?;
        let cache = er_engine::model_discovery::ModelCache {
            version: 1,
            command: provider.models_command.clone(),
            fetched_at: String::new(),
            fetched_at_unix: 0,
            models: vec![er_engine::model_discovery::DiscoveredModel {
                id: "cached-model".into(),
                label: "Cached".into(),
                effort_levels: Vec::new(),
            }],
        };
        er_engine::model_discovery::save_cache(id, &cache)?;
        let handle = actor(dir);
        let view = handle.view();
        let models = &view.config.ai_hub.providers[id].models;
        assert!(
            models
                .iter()
                .any(|m| m.id == "cached-model" && m.discovered),
            "the cached model is missing"
        );
        Ok(())
    })
}
