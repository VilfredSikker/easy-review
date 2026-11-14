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
use crate::settings::{spawn_settings, SettingsCommand, SettingsHandle, SettingsReply};

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
        let theme = view.hub.fields.general.iter().find_map(|field| match field {
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
