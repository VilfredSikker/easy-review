use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ErConfig {
    #[serde(default)]
    pub features: FeatureFlags,
    #[serde(default)]
    pub agent: AgentConfig,
    #[serde(default)]
    pub display: DisplayConfig,
    #[serde(default)]
    pub watched: WatchedConfig,
    #[serde(default)]
    pub hints: HintConfig,
}

/// [watched] section configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WatchedConfig {
    /// Glob patterns for git-ignored paths to include in the UI
    #[serde(default)]
    pub paths: Vec<String>,
    /// How to diff watched files: "content" or "snapshot"
    #[serde(default = "default_diff_mode")]
    pub diff_mode: String,
}

fn default_diff_mode() -> String {
    "content".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureFlags {
    #[serde(default = "default_true")]
    pub view_branch: bool,
    #[serde(default = "default_true")]
    pub view_unstaged: bool,
    #[serde(default = "default_true")]
    pub view_staged: bool,
    #[serde(default = "default_true")]
    pub view_history: bool,
    #[serde(default = "default_true")]
    pub view_conflicts: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    #[serde(default = "default_agent_cmd")]
    pub command: String,
    #[serde(default = "default_agent_args")]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayConfig {
    #[serde(default = "default_tab_width")]
    pub tab_width: u8,
    #[serde(default = "default_true")]
    pub line_numbers: bool,
    #[serde(default)]
    pub wrap_lines: bool,
    #[serde(default)]
    pub split_diff: bool,
}

/// [hints] section — toggle visibility of key hint groups in the bottom bar
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HintConfig {
    #[serde(default = "default_true")]
    pub navigation: bool,
    #[serde(default = "default_true")]
    pub comments: bool,
    #[serde(default = "default_true")]
    pub github: bool,
    #[serde(default = "default_true")]
    pub staging: bool,
    #[serde(default = "default_true")]
    pub ai: bool,
    #[serde(default = "default_true")]
    pub filter: bool,
    #[serde(default = "default_true")]
    pub sort: bool,
    #[serde(default = "default_true")]
    pub settings: bool,
}

impl Default for HintConfig {
    fn default() -> Self {
        Self {
            navigation: true,
            comments: true,
            github: true,
            staging: true,
            ai: true,
            filter: true,
            sort: true,
            settings: true,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_tab_width() -> u8 {
    4
}

fn default_agent_cmd() -> String {
    "claude".into()
}

fn default_agent_args() -> Vec<String> {
    // TODO(risk:medium): the {prompt} placeholder must be present in args for the agent command
    // to receive user input. If a user overrides `agent.args` in their config and omits
    // {prompt}, the prompt is silently dropped and the agent runs with no meaningful input.
    // Validate that {prompt} appears in args when loading config.
    vec!["--print".into(), "-p".into(), "{prompt}".into()]
}

impl Default for FeatureFlags {
    fn default() -> Self {
        Self {
            view_branch: true,
            view_unstaged: true,
            view_staged: true,
            view_history: true,
            view_conflicts: true,
        }
    }
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            command: default_agent_cmd(),
            args: default_agent_args(),
        }
    }
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            tab_width: default_tab_width(),
            line_numbers: true,
            wrap_lines: false,
            split_diff: false,
        }
    }
}

/// Load config by merging global defaults with per-repo overrides.
/// Priority: per-repo `.er-config.toml` > global `~/.config/er/config.toml` > built-in defaults.
/// Merging is deep: individual fields within sections (e.g. `[features]`) override independently.
pub fn load_config(repo_root: &str) -> ErConfig {
    let local_path = format!("{repo_root}/.er-config.toml");
    let global_path =
        dirs::config_dir().map(|d| d.join("er/config.toml").to_string_lossy().to_string());

    let global_table = global_path
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|c| c.parse::<toml::Value>().ok())
        .and_then(|v| match v {
            toml::Value::Table(t) => Some(t),
            _ => None,
        });

    // TODO(risk:medium): parse errors in the local .er-config.toml are silently ignored via
    // .ok(). The user gets default config with no indication their file has a syntax error.
    // At minimum, log the error to stderr so the user can diagnose misconfigured repos.
    let local_table = std::fs::read_to_string(&local_path)
        .ok()
        .and_then(|c| c.parse::<toml::Value>().ok())
        .and_then(|v| match v {
            toml::Value::Table(t) => Some(t),
            _ => None,
        });

    let merged = match (global_table, local_table) {
        (Some(mut global), Some(local)) => {
            deep_merge(&mut global, local);
            toml::Value::Table(global)
        }
        (Some(global), None) => toml::Value::Table(global),
        (None, Some(local)) => toml::Value::Table(local),
        (None, None) => return ErConfig::default(),
    };

    // TODO(risk:medium): unwrap_or_default silently falls back to built-in defaults when the
    // merged TOML fails to deserialize into ErConfig (e.g. wrong type for a field like
    // tab_width = "four"). The user's entire config is dropped with no diagnostic. Log the
    // deserialization error so the user knows their config was not applied.
    merged.try_into().unwrap_or_default()
}

/// Recursively merge `overlay` into `base`. Overlay values win; nested tables are merged recursively.
fn deep_merge(
    base: &mut toml::map::Map<String, toml::Value>,
    overlay: toml::map::Map<String, toml::Value>,
) {
    for (key, value) in overlay {
        match (base.get_mut(&key), &value) {
            (Some(toml::Value::Table(base_table)), toml::Value::Table(overlay_table)) => {
                deep_merge(base_table, overlay_table.clone());
            }
            _ => {
                base.insert(key, value);
            }
        }
    }
}

/// Save config to the global config dir (~/.config/er/config.toml).
pub fn save_config(config: &ErConfig) -> Result<()> {
    let dir = dirs::config_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?
        .join("er");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("config.toml");
    let content = toml::to_string_pretty(config)?;
    // TODO(risk:high): write is non-atomic — if the process is killed mid-write (e.g. Ctrl+C
    // during save) the config file is left with partial content and becomes unreadable on next
    // launch (silently falls back to defaults, losing all user settings). Write to a .tmp file
    // and rename atomically, the same pattern used for .er-github-comments.json.
    std::fs::write(path, content)?;
    Ok(())
}

/// Settings item types for the settings overlay UI.
#[derive(Debug, Clone)]
pub enum SettingsItem {
    SectionHeader(String),
    BoolToggle {
        label: String,
        get: fn(&ErConfig) -> bool,
        set: fn(&mut ErConfig, bool),
    },
    NumberEdit {
        label: String,
        get: fn(&ErConfig) -> u8,
        #[allow(dead_code)]
        set: fn(&mut ErConfig, u8),
    },
    StringDisplay {
        label: String,
        get: fn(&ErConfig) -> String,
    },
}

/// Build the list of settings items for the settings overlay.
pub fn settings_items() -> Vec<SettingsItem> {
    vec![
        SettingsItem::SectionHeader("Views".into()),
        SettingsItem::BoolToggle {
            label: "Branch diff (1)".into(),
            get: |c| c.features.view_branch,
            set: |c, v| c.features.view_branch = v,
        },
        SettingsItem::BoolToggle {
            label: "Unstaged changes (2)".into(),
            get: |c| c.features.view_unstaged,
            set: |c, v| c.features.view_unstaged = v,
        },
        SettingsItem::BoolToggle {
            label: "Staged changes (3)".into(),
            get: |c| c.features.view_staged,
            set: |c, v| c.features.view_staged = v,
        },
        SettingsItem::BoolToggle {
            label: "History (4)".into(),
            get: |c| c.features.view_history,
            set: |c, v| c.features.view_history = v,
        },
        SettingsItem::BoolToggle {
            label: "Conflicts (5)".into(),
            get: |c| c.features.view_conflicts,
            set: |c, v| c.features.view_conflicts = v,
        },
        SettingsItem::SectionHeader("Display".into()),
        SettingsItem::BoolToggle {
            label: "Line numbers".into(),
            get: |c| c.display.line_numbers,
            set: |c, v| c.display.line_numbers = v,
        },
        SettingsItem::BoolToggle {
            label: "Wrap lines".into(),
            get: |c| c.display.wrap_lines,
            set: |c, v| c.display.wrap_lines = v,
        },
        SettingsItem::BoolToggle {
            label: "Split diff".into(),
            get: |c| c.display.split_diff,
            set: |c, v| c.display.split_diff = v,
        },
        SettingsItem::NumberEdit {
            label: "Tab width".into(),
            get: |c| c.display.tab_width,
            set: |c, v| c.display.tab_width = v,
        },
        SettingsItem::SectionHeader("Key Hints".into()),
        SettingsItem::BoolToggle {
            label: "Navigation (j/k, n/N, ↑↓)".into(),
            get: |c| c.hints.navigation,
            set: |c, v| c.hints.navigation = v,
        },
        SettingsItem::BoolToggle {
            label: "Comments (q, c, J/K, d/r)".into(),
            get: |c| c.hints.comments,
            set: |c, v| c.hints.comments = v,
        },
        SettingsItem::BoolToggle {
            label: "GitHub sync (G, P)".into(),
            get: |c| c.hints.github,
            set: |c, v| c.hints.github = v,
        },
        SettingsItem::BoolToggle {
            label: "Staging (s, S, c commit)".into(),
            get: |c| c.hints.staging,
            set: |c, v| c.hints.staging = v,
        },
        SettingsItem::BoolToggle {
            label: "AI (a, ^j/^k)".into(),
            get: |c| c.hints.ai,
            set: |c, v| c.hints.ai = v,
        },
        SettingsItem::BoolToggle {
            label: "Filter & sort (f, u, m)".into(),
            get: |c| c.hints.filter,
            set: |c, v| c.hints.filter = v,
        },
        SettingsItem::BoolToggle {
            label: "Settings (,)".into(),
            get: |c| c.hints.settings,
            set: |c, v| c.hints.settings = v,
        },
        SettingsItem::SectionHeader("Agent".into()),
        SettingsItem::StringDisplay {
            label: "Command".into(),
            get: |c| c.agent.command.clone(),
        },
        SettingsItem::StringDisplay {
            label: "Args".into(),
            get: |c| c.agent.args.join(" "),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── deep_merge ──

    #[test]
    fn deep_merge_empty_base_overwritten_by_overlay() {
        let mut base = toml::map::Map::new();
        let mut overlay = toml::map::Map::new();
        overlay.insert("key".into(), toml::Value::String("value".into()));
        deep_merge(&mut base, overlay);
        assert_eq!(base.get("key").unwrap().as_str().unwrap(), "value");
    }

    #[test]
    fn deep_merge_empty_overlay_preserves_base() {
        let mut base = toml::map::Map::new();
        base.insert("key".into(), toml::Value::String("original".into()));
        let overlay = toml::map::Map::new();
        deep_merge(&mut base, overlay);
        assert_eq!(base.get("key").unwrap().as_str().unwrap(), "original");
    }

    #[test]
    fn deep_merge_scalar_values_replaced() {
        let mut base = toml::map::Map::new();
        base.insert("key".into(), toml::Value::String("old".into()));
        let mut overlay = toml::map::Map::new();
        overlay.insert("key".into(), toml::Value::String("new".into()));
        deep_merge(&mut base, overlay);
        assert_eq!(base.get("key").unwrap().as_str().unwrap(), "new");
    }

    #[test]
    fn deep_merge_nested_tables_merge_recursively() {
        let mut inner_base = toml::map::Map::new();
        inner_base.insert("a".into(), toml::Value::Boolean(true));
        inner_base.insert("b".into(), toml::Value::Boolean(false));
        let mut base = toml::map::Map::new();
        base.insert("section".into(), toml::Value::Table(inner_base));

        let mut inner_overlay = toml::map::Map::new();
        inner_overlay.insert("b".into(), toml::Value::Boolean(true));
        inner_overlay.insert("c".into(), toml::Value::Boolean(true));
        let mut overlay = toml::map::Map::new();
        overlay.insert("section".into(), toml::Value::Table(inner_overlay));

        deep_merge(&mut base, overlay);

        let section = base.get("section").unwrap().as_table().unwrap();
        assert!(section.get("a").unwrap().as_bool().unwrap());
        assert!(section.get("b").unwrap().as_bool().unwrap()); // overridden
        assert!(section.get("c").unwrap().as_bool().unwrap()); // added
    }

    #[test]
    fn deep_merge_three_levels_deep() {
        let mut l3_base = toml::map::Map::new();
        l3_base.insert("val".into(), toml::Value::Integer(1));
        let mut l2_base = toml::map::Map::new();
        l2_base.insert("inner".into(), toml::Value::Table(l3_base));
        let mut base = toml::map::Map::new();
        base.insert("outer".into(), toml::Value::Table(l2_base));

        let mut l3_overlay = toml::map::Map::new();
        l3_overlay.insert("val".into(), toml::Value::Integer(99));
        let mut l2_overlay = toml::map::Map::new();
        l2_overlay.insert("inner".into(), toml::Value::Table(l3_overlay));
        let mut overlay = toml::map::Map::new();
        overlay.insert("outer".into(), toml::Value::Table(l2_overlay));

        deep_merge(&mut base, overlay);

        let val = base["outer"].as_table().unwrap()["inner"]
            .as_table()
            .unwrap()["val"]
            .as_integer()
            .unwrap();
        assert_eq!(val, 99);
    }

    #[test]
    fn deep_merge_array_values_replaced_not_appended() {
        let mut base = toml::map::Map::new();
        base.insert(
            "arr".into(),
            toml::Value::Array(vec![toml::Value::Integer(1), toml::Value::Integer(2)]),
        );
        let mut overlay = toml::map::Map::new();
        overlay.insert(
            "arr".into(),
            toml::Value::Array(vec![toml::Value::Integer(3)]),
        );
        deep_merge(&mut base, overlay);
        let arr = base.get("arr").unwrap().as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0].as_integer().unwrap(), 3);
    }

    #[test]
    fn deep_merge_type_mismatch_scalar_overwrites_table() {
        let mut inner = toml::map::Map::new();
        inner.insert("a".into(), toml::Value::Boolean(true));
        let mut base = toml::map::Map::new();
        base.insert("key".into(), toml::Value::Table(inner));

        let mut overlay = toml::map::Map::new();
        overlay.insert("key".into(), toml::Value::String("replaced".into()));

        deep_merge(&mut base, overlay);
        assert_eq!(base.get("key").unwrap().as_str().unwrap(), "replaced");
    }

    #[test]
    fn deep_merge_type_mismatch_table_overwrites_scalar() {
        let mut base = toml::map::Map::new();
        base.insert("key".into(), toml::Value::String("old".into()));

        let mut inner = toml::map::Map::new();
        inner.insert("a".into(), toml::Value::Boolean(true));
        let mut overlay = toml::map::Map::new();
        overlay.insert("key".into(), toml::Value::Table(inner));

        deep_merge(&mut base, overlay);
        assert!(base.get("key").unwrap().is_table());
        assert!(base["key"].as_table().unwrap()["a"].as_bool().unwrap());
    }

    // ── load_config ──

    #[test]
    fn load_config_missing_files_produce_defaults() {
        let config = load_config("/nonexistent/repo/path");
        assert!(config.features.view_branch);
        assert!(config.features.view_unstaged);
        assert_eq!(config.display.tab_width, 4);
        assert_eq!(config.agent.command, "claude");
    }

    #[test]
    fn load_config_partial_toml_merges_with_defaults() {
        let dir = std::env::temp_dir().join("er_test_partial_toml");
        let _ = std::fs::create_dir_all(&dir);
        let config_path = dir.join(".er-config.toml");
        std::fs::write(&config_path, "[features]\nview_branch = false\n").unwrap();

        let config = load_config(dir.to_str().unwrap());
        assert!(!config.features.view_branch); // overridden
        assert!(config.features.view_unstaged); // default preserved
        assert_eq!(config.display.tab_width, 4); // default preserved

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_config_malformed_toml_falls_back_to_defaults() {
        let dir = std::env::temp_dir().join("er_test_malformed_toml");
        let _ = std::fs::create_dir_all(&dir);
        let config_path = dir.join(".er-config.toml");
        std::fs::write(&config_path, "this is not valid { toml ]]").unwrap();

        let config = load_config(dir.to_str().unwrap());
        // Should fall back to defaults
        assert!(config.features.view_branch);
        assert_eq!(config.display.tab_width, 4);

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── Default values ──

    #[test]
    fn feature_flags_all_default_to_true() {
        let flags = FeatureFlags::default();
        assert!(flags.view_branch);
        assert!(flags.view_unstaged);
        assert!(flags.view_staged);
        assert!(flags.view_history);
        assert!(flags.view_conflicts);
    }

    #[test]
    fn tab_width_defaults_to_4() {
        let display = DisplayConfig::default();
        assert_eq!(display.tab_width, 4);
    }

    #[test]
    fn agent_command_defaults_to_claude() {
        let agent = AgentConfig::default();
        assert_eq!(agent.command, "claude");
    }

    #[test]
    fn serde_roundtrip_preserves_all_fields() {
        let config = ErConfig {
            features: FeatureFlags {
                view_branch: false,
                view_unstaged: true,
                view_staged: false,
                view_history: true,
                view_conflicts: false,
            },
            display: DisplayConfig {
                tab_width: 8,
                line_numbers: false,
                wrap_lines: true,
                split_diff: true,
            },
            agent: AgentConfig {
                command: "my-agent".into(),
                args: vec!["--flag".into()],
            },
            ..Default::default()
        };
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let restored: ErConfig = toml::from_str(&toml_str).unwrap();

        assert!(!restored.features.view_branch);
        assert!(restored.features.view_history);
        assert_eq!(restored.display.tab_width, 8);
        assert!(restored.display.wrap_lines);
        assert_eq!(restored.agent.command, "my-agent");
        assert_eq!(restored.agent.args, vec!["--flag"]);
    }

    // ── settings_items ──

    #[test]
    fn settings_items_returns_expected_count() {
        let items = settings_items();
        // Count non-header items (BoolToggle, NumberEdit, StringDisplay)
        let toggleable: Vec<_> = items
            .iter()
            .filter(|i| !matches!(i, SettingsItem::SectionHeader(_)))
            .collect();
        assert!(
            toggleable.len() >= 14,
            "Expected at least 14 toggleable items, got {}",
            toggleable.len()
        );
        assert!(
            items.len() >= 18,
            "Expected at least 18 total items, got {}",
            items.len()
        );
    }

    #[test]
    fn settings_items_bool_toggle_get_set_read_write_correct_fields() {
        let mut config = ErConfig::default();

        let items = settings_items();
        // Find the "Branch diff" toggle and verify it reads/writes view_branch
        let branch_toggle = items.iter().find(|i| match i {
            SettingsItem::BoolToggle { label, .. } => label.contains("Branch"),
            _ => false,
        });
        if let Some(SettingsItem::BoolToggle { get, set, .. }) = branch_toggle {
            assert!(get(&config));
            set(&mut config, false);
            assert!(!config.features.view_branch);
            assert!(!get(&config));
        } else {
            panic!("Branch diff toggle not found");
        }

        // Verify line_numbers toggle
        let line_num_toggle = items.iter().find(|i| match i {
            SettingsItem::BoolToggle { label, .. } => label.contains("Line numbers"),
            _ => false,
        });
        if let Some(SettingsItem::BoolToggle { get, set, .. }) = line_num_toggle {
            assert!(get(&config));
            set(&mut config, false);
            assert!(!config.display.line_numbers);
        } else {
            panic!("Line numbers toggle not found");
        }
    }

    #[test]
    fn settings_items_section_headers_present_in_correct_order() {
        let items = settings_items();
        let headers: Vec<&str> = items
            .iter()
            .filter_map(|i| match i {
                SettingsItem::SectionHeader(title) => Some(title.as_str()),
                _ => None,
            })
            .collect();
        assert!(headers.contains(&"Views"));
        assert!(headers.contains(&"Display"));
        assert!(headers.contains(&"Key Hints"));
        assert!(headers.contains(&"Agent"));
        // Views should come before Display
        let views_pos = headers.iter().position(|h| *h == "Views").unwrap();
        let display_pos = headers.iter().position(|h| *h == "Display").unwrap();
        assert!(views_pos < display_pos);
    }
}
