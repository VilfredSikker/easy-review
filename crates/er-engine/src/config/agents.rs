//! `agents.toml`: the agent and provider settings split out of `config.toml`.
//!
//! `config.toml` keeps preferences, `[risk]` and `[commands]`; the `[agent]`
//! and `[ai_hub]` tables live here. The move is a raw-table migration: the
//! tables are copied verbatim, so a key this build does not know survives.
//! `config.toml` is only rewritten after `agents.toml` has been written and
//! read back, and the loader completes an interrupted move on the next load.

use super::{AgentConfig, AiHubConfig, ConfigLoadError, ConfigReadError, ErConfig};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// The file name next to `config.toml`.
pub const AGENTS_FILE: &str = "agents.toml";

/// The two sections that live in `agents.toml`, in their serde shapes.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentsConfig {
    #[serde(default)]
    pub agent: AgentConfig,
    #[serde(default)]
    pub ai_hub: AiHubConfig,
}

/// `config.toml`'s sibling file.
pub fn agents_path(config_path: &Path) -> PathBuf {
    config_path.with_file_name(AGENTS_FILE)
}

/// Paths whose last agents load failed. Saves refuse while the path is here,
/// so a defaulted in-memory hub cannot replace a file that still holds data.
static FAILED_AGENTS_LOADS: OnceLock<Mutex<std::collections::HashSet<PathBuf>>> = OnceLock::new();

fn failed_agents_loads() -> &'static Mutex<std::collections::HashSet<PathBuf>> {
    FAILED_AGENTS_LOADS.get_or_init(|| Mutex::new(std::collections::HashSet::new()))
}

pub(super) fn record_agents_load_failure(path: &Path) {
    if let Ok(mut failed) = failed_agents_loads().lock() {
        failed.insert(path.to_path_buf());
    }
}

pub(super) fn clear_agents_load_failure(path: &Path) {
    if let Ok(mut failed) = failed_agents_loads().lock() {
        failed.remove(path);
    }
}

/// Whether the agents file has a recorded failed load.
pub fn agents_load_failed(path: &Path) -> bool {
    failed_agents_loads()
        .lock()
        .map(|failed| failed.contains(path))
        .unwrap_or(false)
}

fn parse_agents_from_path(path: &Path) -> Result<AgentsConfig, ConfigReadError> {
    let content = std::fs::read_to_string(path).map_err(ConfigReadError::Read)?;
    toml::from_str(&content).map_err(ConfigReadError::Parse)
}

enum AgentsState {
    Present(Box<AgentsConfig>),
    Absent,
    Failed(ConfigLoadError),
}

fn read_agents_state(path: &Path, adopt: bool) -> AgentsState {
    match parse_agents_from_path(path) {
        Ok(agents) => {
            if adopt {
                clear_agents_load_failure(path);
            }
            AgentsState::Present(Box::new(agents))
        }
        Err(ConfigReadError::Read(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            AgentsState::Absent
        }
        Err(error) => {
            record_agents_load_failure(path);
            let (backup, backup_error) = super::backup_invalid_config(path);
            AgentsState::Failed(ConfigLoadError {
                path: path.to_path_buf(),
                message: error.to_string(),
                backup,
                backup_error,
            })
        }
    }
}

/// Load `agents.toml`, defaulting on absence and reporting a bad file.
pub fn load_agents_from(path: &Path) -> (AgentsConfig, Option<ConfigLoadError>) {
    match read_agents_state(path, true) {
        AgentsState::Present(agents) => (*agents, None),
        AgentsState::Absent => (AgentsConfig::default(), None),
        AgentsState::Failed(error) => (AgentsConfig::default(), Some(error)),
    }
}

/// Save the agent settings to `path`, refusing to overwrite a file that does
/// not parse. Discovery-stripped values are written, matching `config.toml`.
pub fn save_agents_to(path: &Path, config: &ErConfig) -> anyhow::Result<()> {
    if agents_load_failed(path) {
        anyhow::bail!(
            "Refusing to save: {} failed to load earlier in this session. Fix or remove the file, then reopen Settings or restart to reload it.",
            path.display()
        );
    }
    match parse_agents_from_path(path) {
        Ok(_) => {}
        Err(ConfigReadError::Read(error)) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            record_agents_load_failure(path);
            anyhow::bail!(
                "Refusing to save: {} could not be loaded: {error}. Fix or remove the file, then reopen Settings or restart to reload it.",
                path.display()
            );
        }
    }
    let persisted = super::persisted_config(config);
    let agents = AgentsConfig {
        agent: persisted.agent,
        ai_hub: persisted.ai_hub,
    };
    let content = toml::to_string_pretty(&agents)?;
    er_store::atomic::write_atomic_durable(path, content.as_bytes())?;
    Ok(())
}

/// Complete the split for a load of `config_path`, whose parsed `table` is
/// passed in. Returns an agents load or migration error for the caller to show.
///
/// Runs only when a write may be needed, and takes the store lock for it. A
/// second call with both files settled writes nothing.
pub(super) fn reconcile_on_load(
    config_path: &Path,
    table: toml::Table,
    config: &mut ErConfig,
    adopt: bool,
) -> Option<ConfigLoadError> {
    let agents_file = agents_path(config_path);
    let state = read_agents_state(&agents_file, adopt);
    let has_leftovers = has_agent_sections(&table);
    if !has_leftovers {
        return match state {
            AgentsState::Present(agents) => {
                overlay(config, *agents);
                None
            }
            AgentsState::Absent => None,
            AgentsState::Failed(error) => Some(error),
        };
    }

    // A write may be needed: take the lock and re-read, so a move another
    // process completed is not repeated.
    let Some(_guard) = lock_config_dir(config_path) else {
        return match state {
            AgentsState::Failed(error) => Some(error),
            _ => None,
        };
    };
    let table = match super::read_config_table(config_path) {
        Ok(table) => table,
        Err(ConfigReadError::Read(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            toml::Table::new()
        }
        Err(error) => {
            return Some(ConfigLoadError {
                path: config_path.to_path_buf(),
                message: error.to_string(),
                backup: None,
                backup_error: None,
            });
        }
    };
    let state = read_agents_state(&agents_file, adopt);
    let has_leftovers = has_agent_sections(&table);
    match state {
        AgentsState::Present(agents) => {
            overlay(config, *agents);
            if has_leftovers {
                let _ = strip_leftovers(config_path, table);
            }
            None
        }
        AgentsState::Failed(error) => Some(error),
        AgentsState::Absent if has_leftovers => match migrate(config_path, table) {
            Ok(agents) => {
                overlay(config, agents);
                None
            }
            Err(error) => Some(error),
        },
        AgentsState::Absent => None,
    }
}

fn overlay(config: &mut ErConfig, agents: AgentsConfig) {
    config.agent = agents.agent;
    config.ai_hub = agents.ai_hub;
}

fn has_agent_sections(table: &toml::Table) -> bool {
    table.contains_key("agent") || table.contains_key("ai_hub")
}

/// Take the two raw tables out of `table`, keeping every key verbatim.
fn take_sections(table: &mut toml::Table) -> toml::Table {
    let mut sections = toml::Table::new();
    for name in ["agent", "ai_hub"] {
        if let Some(value) = table.remove(name) {
            sections.insert(name.to_string(), value);
        }
    }
    sections
}

fn sections_text(sections: &toml::Table) -> anyhow::Result<String> {
    Ok(toml::to_string_pretty(&toml::Value::Table(
        sections.clone(),
    ))?)
}

/// Write `agents.toml` and read it back. Any mismatch aborts before
/// `config.toml` is touched.
fn write_and_verify_agents(
    agents_file: &Path,
    sections: &toml::Table,
) -> anyhow::Result<AgentsConfig> {
    let content = sections_text(sections)?;
    #[cfg(test)]
    if TRUNCATE_AGENTS_WRITE.with(|flag| flag.replace(false)) {
        let half = content.len() / 2;
        er_store::atomic::write_atomic_durable(agents_file, &content.as_bytes()[..half])?;
    } else {
        er_store::atomic::write_atomic_durable(agents_file, content.as_bytes())?;
    }
    #[cfg(not(test))]
    er_store::atomic::write_atomic_durable(agents_file, content.as_bytes())?;

    let written = std::fs::read_to_string(agents_file)?;
    let read_back: toml::Table = written.parse()?;
    if &read_back != sections {
        anyhow::bail!("agents.toml did not read back as written");
    }
    toml::from_str::<AgentsConfig>(&written).map_err(anyhow::Error::from)
}

/// Move the two sections out of `config.toml`: write `agents.toml`, verify it,
/// back up `config.toml`, then rewrite it without the sections.
fn migrate(config_path: &Path, mut table: toml::Table) -> Result<AgentsConfig, ConfigLoadError> {
    let agents_file = agents_path(config_path);
    let sections = take_sections(&mut table);
    let agents = write_and_verify_agents(&agents_file, &sections)
        .map_err(|error| migration_error(config_path, error))?;

    #[cfg(test)]
    if STOP_AFTER_AGENTS_WRITE.with(|flag| flag.replace(false)) {
        return Err(migration_error(
            config_path,
            anyhow::anyhow!("migration stopped after writing agents.toml"),
        ));
    }

    let (_, backup_error) = super::backup_invalid_config(config_path);
    if let Some(error) = backup_error {
        return Err(migration_error(
            config_path,
            anyhow::anyhow!("could not back up config.toml: {error}"),
        ));
    }
    let content = toml::to_string_pretty(&toml::Value::Table(table))
        .map_err(|error| migration_error(config_path, anyhow::Error::from(error)))?;
    er_store::atomic::write_atomic_durable(config_path, content.as_bytes())
        .map_err(|error| migration_error(config_path, anyhow::Error::from(error)))?;
    Ok(agents)
}

/// Remove leftovers from `config.toml` once `agents.toml` holds them.
fn strip_leftovers(config_path: &Path, mut table: toml::Table) -> anyhow::Result<()> {
    take_sections(&mut table);
    let content = toml::to_string_pretty(&toml::Value::Table(table))?;
    er_store::atomic::write_atomic_durable(config_path, content.as_bytes())?;
    Ok(())
}

fn migration_error(config_path: &Path, error: anyhow::Error) -> ConfigLoadError {
    ConfigLoadError {
        path: config_path.to_path_buf(),
        message: error.to_string(),
        backup: None,
        backup_error: None,
    }
}

fn lock_config_dir(config_path: &Path) -> Option<er_store::ScopeGuard> {
    let parent = config_path.parent()?;
    if parent.as_os_str().is_empty() || !parent.is_dir() {
        return None;
    }
    let scope = er_store::Scope::dir(parent).ok()?;
    crate::store_registry::store()
        .lock_scope(&scope, er_store::DEFAULT_LOCK_TIMEOUT)
        .ok()
}

#[cfg(test)]
thread_local! {
    static STOP_AFTER_AGENTS_WRITE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static TRUNCATE_AGENTS_WRITE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Simulate a crash after the agents write and before the config rewrite.
#[cfg(test)]
pub fn stop_after_agents_write_for_test() {
    STOP_AFTER_AGENTS_WRITE.with(|flag| flag.set(true));
}

/// Make the next agents write a truncated prefix, so the read-back fails.
#[cfg(test)]
pub fn truncate_next_agents_write_for_test() {
    TRUNCATE_AGENTS_WRITE.with(|flag| flag.set(true));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::with_managed_config_path;

    // The tests use explicit config paths; keep the process store rooted in a
    // temp dir for the lock helper.
    fn temp_config(dir: &tempfile::TempDir) -> PathBuf {
        dir.path().join("config.toml")
    }

    fn load(path: &Path) -> (ErConfig, Option<ConfigLoadError>) {
        super::super::load_config_from(path)
    }

    fn write_agent_sections(path: &Path) {
        std::fs::write(
            path,
            r#"[agent]
command = "my-agent"
args = ["--flag", "-v"]
model = "fallback-model"

[ai_hub]
default_provider = "my-provider"
default_model = "my-model"
flow_providers = ["my-provider"]

[ai_hub.providers.my-provider]
command = "my-provider-cli"
args = ["--print", "{prompt}"]

[[ai_hub.providers.my-provider.models]]
id = "my-model"
label = "My model"

[display]
theme = "paper"
"#,
        )
        .unwrap();
    }

    #[test]
    fn the_agent_settings_move_field_for_field() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let path = temp_config(&dir);
            write_agent_sections(&path);
            let source: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();

            let (config, error) = load(&path);
            assert!(error.is_none(), "{error:?}");
            assert_eq!(config.agent.command, "my-agent");
            assert_eq!(config.agent.args, ["--flag", "-v"]);
            assert_eq!(
                config.ai_hub.default_provider.as_deref(),
                Some("my-provider")
            );

            let agents_file = agents_path(&path);
            let moved: toml::Table = std::fs::read_to_string(&agents_file)
                .unwrap()
                .parse()
                .unwrap();
            let mut expected = toml::Table::new();
            for name in ["agent", "ai_hub"] {
                expected.insert(name.to_string(), source[name].clone());
            }
            assert_eq!(moved, expected, "the raw tables moved verbatim");

            let stripped: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
            assert!(!stripped.contains_key("agent"));
            assert!(!stripped.contains_key("ai_hub"));
            assert_eq!(stripped["display"]["theme"].as_str(), Some("paper"));
        });
    }

    #[test]
    fn a_migration_keeps_keys_this_build_does_not_know() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let path = temp_config(&dir);
            std::fs::write(
                &path,
                "[agent]\ncommand = \"x\"\nfuture_key = \"kept\"\n\n[ai_hub]\nfuture_hub_key = 7\n",
            )
            .unwrap();
            let (_, error) = load(&path);
            assert!(error.is_none(), "{error:?}");
            let moved: toml::Table = std::fs::read_to_string(agents_path(&path))
                .unwrap()
                .parse()
                .unwrap();
            assert_eq!(moved["agent"]["future_key"].as_str(), Some("kept"));
            assert_eq!(moved["ai_hub"]["future_hub_key"].as_integer(), Some(7));
        });
    }

    #[test]
    fn an_unparseable_config_is_not_migrated() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let path = temp_config(&dir);
            std::fs::write(&path, "{").unwrap();
            let (_, error) = load(&path);
            assert!(error.is_some());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), "{");
            assert!(!agents_path(&path).exists(), "no agents.toml was created");
        });
    }

    #[test]
    fn a_crash_between_the_writes_is_completed_on_restart() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let path = temp_config(&dir);
            write_agent_sections(&path);
            stop_after_agents_write_for_test();
            let (_, error) = load(&path);
            assert!(error.is_some(), "the phase stop reported itself");
            assert!(agents_path(&path).exists(), "the agents write landed");
            let before: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
            assert!(
                before.contains_key("agent"),
                "config.toml still has the leftovers"
            );

            let (config, error) = load(&path);
            assert!(error.is_none(), "{error:?}");
            assert_eq!(config.agent.command, "my-agent");
            let after: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
            assert!(
                !after.contains_key("agent"),
                "the restart stripped the leftovers"
            );
        });
    }

    #[test]
    fn a_save_after_a_failed_load_does_not_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            with_managed_config_path(&temp_config(&dir), || {
                let path = crate::config::managed_config_path();
                std::fs::write(&path, "{").unwrap();
                let (_, error) = crate::config::load_config_from(&path);
                assert!(error.is_some());
                assert!(super::super::save_config(&ErConfig::default()).is_err());
                assert_eq!(std::fs::read_to_string(&path).unwrap(), "{");
                assert!(!agents_path(&path).exists());
            });
        });
    }

    #[test]
    fn a_second_run_changes_neither_file() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let path = temp_config(&dir);
            write_agent_sections(&path);
            let (_, error) = load(&path);
            assert!(error.is_none(), "{error:?}");
            let agents_file = agents_path(&path);
            let config_before = std::fs::read_to_string(&path).unwrap();
            let agents_before = std::fs::read_to_string(&agents_file).unwrap();
            let config_mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
            let agents_mtime = std::fs::metadata(&agents_file).unwrap().modified().unwrap();

            let (_, error) = load(&path);
            assert!(error.is_none(), "{error:?}");
            assert_eq!(std::fs::read_to_string(&path).unwrap(), config_before);
            assert_eq!(
                std::fs::read_to_string(&agents_file).unwrap(),
                agents_before
            );
            assert_eq!(
                std::fs::metadata(&path).unwrap().modified().unwrap(),
                config_mtime
            );
            assert_eq!(
                std::fs::metadata(&agents_file).unwrap().modified().unwrap(),
                agents_mtime
            );
        });
    }

    #[test]
    fn invalid_backups_do_not_collide() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let path = temp_config(&dir);
            std::fs::write(&path, "{").unwrap();
            assert!(load(&path).1.is_some());
            std::fs::write(&path, "not = [valid").unwrap();
            assert!(load(&path).1.is_some());
            // A migration backup through the same helper, with a third payload.
            std::fs::write(&path, "[agent]\ncommand = \"x\"\n").unwrap();
            let (_, error) = load(&path);
            assert!(error.is_none(), "{error:?}");

            let mut backups: Vec<String> = std::fs::read_dir(dir.path())
                .unwrap()
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| name.starts_with("config.toml.invalid-"))
                .collect();
            backups.sort();
            assert_eq!(backups.len(), 3, "{backups:?}");
            let mut contents: Vec<String> = backups
                .iter()
                .map(|name| std::fs::read_to_string(dir.path().join(name)).unwrap())
                .collect();
            contents.sort();
            assert_eq!(
                contents,
                ["[agent]\ncommand = \"x\"\n", "not = [valid", "{"]
            );
        });
    }

    #[test]
    fn an_unparseable_agents_file_is_never_overwritten_by_a_save() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            with_managed_config_path(&temp_config(&dir), || {
                let path = crate::config::managed_config_path();
                std::fs::write(&path, "[display]\ntheme = \"paper\"\n").unwrap();
                let agents_file = agents_path(&path);
                std::fs::write(&agents_file, "{").unwrap();
                let (_, error) = crate::config::load_config_from(&path);
                assert!(error.is_some());
                let config_before = std::fs::read_to_string(&path).unwrap();

                assert!(super::super::save_config(&ErConfig::default()).is_err());
                assert_eq!(std::fs::read_to_string(&agents_file).unwrap(), "{");
                assert_eq!(std::fs::read_to_string(&path).unwrap(), config_before);
            });
        });
    }

    #[test]
    fn a_read_back_mismatch_leaves_config_untouched() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let path = temp_config(&dir);
            write_agent_sections(&path);
            let before = std::fs::read_to_string(&path).unwrap();
            truncate_next_agents_write_for_test();
            let (_, error) = load(&path);
            assert!(error.is_some(), "the read-back failure surfaced");
            assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
        });
    }

    #[test]
    fn agents_toml_wins_when_both_files_carry_agent_sections() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let path = temp_config(&dir);
            std::fs::write(
                &path,
                "[agent]\ncommand = \"from-config\"\n\n[ai_hub.providers.shared]\ncommand = \"config-cli\"\n",
            )
            .unwrap();
            std::fs::write(
                agents_path(&path),
                "[agent]\ncommand = \"from-agents\"\n\n[ai_hub.providers.shared]\ncommand = \"agents-cli\"\n",
            )
            .unwrap();

            let (config, error) = load(&path);
            assert!(error.is_none(), "{error:?}");
            assert_eq!(config.agent.command, "from-agents");
            assert_eq!(config.ai_hub.providers["shared"].command, "agents-cli");
            let stripped: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
            assert!(!stripped.contains_key("agent"));
            assert!(!stripped.contains_key("ai_hub"));
        });
    }
}
