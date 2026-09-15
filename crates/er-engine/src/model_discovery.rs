//! Discover AI Hub models from provider CLIs (`models_command`).
//!
//! Always-on module: std + serde + storage only — no ui/watch/highlight deps.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How long a model-cache entry stays "fresh" before background refresh.
pub const MODEL_CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);

const CACHE_VERSION: u32 = 1;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredModel {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCache {
    pub version: u32,
    pub command: Vec<String>,
    /// RFC3339 UTC timestamp (same format as `sync::chrono_now`).
    pub fetched_at: String,
    /// Unix seconds — authoritative for TTL checks (avoids a chrono dependency).
    #[serde(default)]
    pub fetched_at_unix: u64,
    pub models: Vec<DiscoveredModel>,
}

/// Tolerant line parser for Cursor (`id - Label`) and OpenCode (bare ids).
pub fn parse_models_output(stdout: &str) -> Vec<DiscoveredModel> {
    let mut out = Vec::new();
    for raw in stdout.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some((id, label)) = line.split_once(" - ") {
            let id = id.trim();
            let label = label.trim();
            if id.is_empty() || id.chars().any(char::is_whitespace) {
                continue;
            }
            out.push(DiscoveredModel {
                id: id.to_string(),
                label: if label.is_empty() {
                    id.to_string()
                } else {
                    label.to_string()
                },
            });
        } else if !line.chars().any(char::is_whitespace) {
            out.push(DiscoveredModel {
                id: line.to_string(),
                label: line.to_string(),
            });
        }
    }
    out
}

/// Run `cmd` (first element = program) with a 10s kill timeout. Returns stdout.
pub fn run_models_command(cmd: &[String]) -> Result<String> {
    if cmd.is_empty() || cmd[0].trim().is_empty() {
        bail!("models_command is empty");
    }
    let program = &cmd[0];
    let args = &cmd[1..];
    let output = crate::proc::run_with_timeout(Command::new(program).args(args), COMMAND_TIMEOUT)
        .with_context(|| format!("failed to run models_command: {program}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let snippet: String = stderr.chars().take(200).collect();
        bail!(
            "models_command exited with {}: {}",
            output.status,
            snippet.trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn cache_dir() -> PathBuf {
    crate::storage::storage_root().join("model-cache")
}

fn cache_path(provider_id: &str) -> PathBuf {
    let safe: String = provider_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    cache_dir().join(format!("{safe}.json"))
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn load_cache(provider_id: &str) -> Option<ModelCache> {
    let path = cache_path(provider_id);
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Cache is invalid when the stored command differs from the provider's current command.
pub fn cache_command_matches(cache: &ModelCache, command: &[String]) -> bool {
    cache.command == command
}

pub fn cache_is_fresh(cache: &ModelCache) -> bool {
    let fetched = if cache.fetched_at_unix > 0 {
        cache.fetched_at_unix
    } else {
        return false;
    };
    unix_now().saturating_sub(fetched) < MODEL_CACHE_TTL.as_secs()
}

pub fn save_cache(provider_id: &str, cache: &ModelCache) -> Result<()> {
    let dir = cache_dir();
    std::fs::create_dir_all(&dir)?;
    let path = cache_path(provider_id);
    let tmp = dir.join(format!(
        "{}.tmp.{}",
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("cache.json"),
        std::process::id()
    ));
    let json = serde_json::to_string_pretty(cache)?;
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(json.as_bytes())?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// Fetch models for a provider command, update the cache, and return them.
pub fn discover_and_cache(provider_id: &str, command: &[String]) -> Result<Vec<DiscoveredModel>> {
    let stdout = run_models_command(command)?;
    let models = parse_models_output(&stdout);
    let now = unix_now();
    let cache = ModelCache {
        version: CACHE_VERSION,
        command: command.to_vec(),
        fetched_at: crate::sync::chrono_now(),
        fetched_at_unix: now,
        models: models.clone(),
    };
    save_cache(provider_id, &cache)?;
    Ok(models)
}

/// Load cached models when the command still matches (stale-while-revalidate OK).
pub fn load_valid_cache(provider_id: &str, command: &[String]) -> Option<ModelCache> {
    let cache = load_cache(provider_id)?;
    if !cache_command_matches(&cache, command) {
        return None;
    }
    Some(cache)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::STORAGE_TEST_ENV_LOCK;
    use tempfile::TempDir;

    #[test]
    fn parse_cursor_fixture() {
        let stdout = "Available models\n\ngpt-5.2 - GPT-5.2\ncomposer-2.5 - Composer 2.5\n";
        let models = parse_models_output(stdout);
        assert_eq!(
            models,
            vec![
                DiscoveredModel {
                    id: "gpt-5.2".into(),
                    label: "GPT-5.2".into(),
                },
                DiscoveredModel {
                    id: "composer-2.5".into(),
                    label: "Composer 2.5".into(),
                },
            ]
        );
    }

    #[test]
    fn parse_opencode_fixture() {
        let stdout = "anthropic/claude-sonnet-4-5\nopenai/gpt-5.2\n";
        let models = parse_models_output(stdout);
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "anthropic/claude-sonnet-4-5");
        assert_eq!(models[0].label, "anthropic/claude-sonnet-4-5");
    }

    #[test]
    fn parse_skips_garbage_and_empty() {
        assert!(parse_models_output("").is_empty());
        assert!(parse_models_output("hello world no dash\n  \n").is_empty());
        assert!(parse_models_output("bad id - Label\n").is_empty());
    }

    #[test]
    fn cache_roundtrip_and_command_mismatch() {
        let _guard = STORAGE_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = TempDir::new().unwrap();
        std::env::set_var("ER_STORAGE_ROOT", tmp.path());

        let cmd = vec!["agent".into(), "--list-models".into()];
        let cache = ModelCache {
            version: 1,
            command: cmd.clone(),
            fetched_at: crate::sync::chrono_now(),
            fetched_at_unix: unix_now(),
            models: vec![DiscoveredModel {
                id: "m1".into(),
                label: "M1".into(),
            }],
        };
        save_cache("cursor", &cache).unwrap();
        let loaded = load_valid_cache("cursor", &cmd).unwrap();
        assert!(cache_is_fresh(&loaded));
        assert_eq!(loaded.models.len(), 1);

        let other = vec!["agent".into(), "--other".into()];
        assert!(load_valid_cache("cursor", &other).is_none());

        std::env::remove_var("ER_STORAGE_ROOT");
    }

    #[test]
    fn returns_output_larger_than_the_pipe_buffer() {
        // A model list that big is unusual, but a CLI that prints a long
        // banner on stderr is not, and either one overruns the pipe buffer.
        // This path drains both pipes only after the child exits, so it used
        // to report a 10s timeout for a command that had already succeeded.
        let cmd = vec![
            "sh".to_string(),
            "-c".to_string(),
            "head -c 200000 /dev/zero".to_string(),
        ];
        let stdout = run_models_command(&cmd).expect("a 200 KB listing is not a hung command");
        assert_eq!(stdout.len(), 200_000);
    }

    #[test]
    fn ttl_expired_cache_still_served_but_not_fresh() {
        let _guard = STORAGE_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = TempDir::new().unwrap();
        std::env::set_var("ER_STORAGE_ROOT", tmp.path());

        let cmd = vec!["agent".into(), "--list-models".into()];
        let expired = unix_now().saturating_sub(MODEL_CACHE_TTL.as_secs() + 60);
        let cache = ModelCache {
            version: 1,
            command: cmd.clone(),
            fetched_at: "2020-01-01T00:00:00Z".into(),
            fetched_at_unix: expired,
            models: vec![DiscoveredModel {
                id: "stale-m".into(),
                label: "Stale".into(),
            }],
        };
        save_cache("cursor", &cache).unwrap();

        // Command still matches → stale-while-revalidate serves the entry.
        let loaded = load_valid_cache("cursor", &cmd).expect("stale cache still loadable");
        assert!(!cache_is_fresh(&loaded));
        assert_eq!(loaded.models[0].id, "stale-m");

        // Zero unix timestamp is never fresh.
        let no_unix = ModelCache {
            fetched_at_unix: 0,
            ..cache
        };
        assert!(!cache_is_fresh(&no_unix));

        std::env::remove_var("ER_STORAGE_ROOT");
    }
}
