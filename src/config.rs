use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErConfig {
    #[serde(default)]
    pub features: FeatureFlags,
    #[serde(default)]
    pub agent: AgentConfig,
    #[serde(default)]
    pub display: DisplayConfig,
    #[serde(default)]
    pub watched: WatchedConfig,
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
    pub split_diff: bool,
    #[serde(default = "default_true")]
    pub exit_heatmap: bool,
    #[serde(default)]
    pub blame_annotations: bool,
    #[serde(default = "default_true")]
    pub bookmarks: bool,
    #[serde(default = "default_true")]
    pub view_branch: bool,
    #[serde(default = "default_true")]
    pub view_unstaged: bool,
    #[serde(default = "default_true")]
    pub view_staged: bool,
    #[serde(default = "default_true")]
    pub ai_overlays: bool,
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
    vec!["--print".into(), "-p".into(), "{prompt}".into()]
}

impl Default for ErConfig {
    fn default() -> Self {
        Self {
            features: FeatureFlags::default(),
            agent: AgentConfig::default(),
            display: DisplayConfig::default(),
            watched: WatchedConfig::default(),
        }
    }
}

impl Default for FeatureFlags {
    fn default() -> Self {
        Self {
            split_diff: true,
            exit_heatmap: true,
            blame_annotations: false,
            bookmarks: true,
            view_branch: true,
            view_unstaged: true,
            view_staged: true,
            ai_overlays: true,
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
        }
    }
}

/// Load config by merging global defaults with per-repo overrides.
/// Priority: per-repo `.er-config.toml` > global `~/.config/er/config.toml` > built-in defaults.
/// Merging is deep: individual fields within sections (e.g. `[features]`) override independently.
pub fn load_config(repo_root: &str) -> ErConfig {
    let local_path = format!("{repo_root}/.er-config.toml");
    let global_path = dirs::config_dir()
        .map(|d| d.join("er/config.toml").to_string_lossy().to_string());

    let global_table = global_path
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|c| c.parse::<toml::Value>().ok())
        .and_then(|v| match v {
            toml::Value::Table(t) => Some(t),
            _ => None,
        });

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
        SettingsItem::SectionHeader("Features".into()),
        SettingsItem::BoolToggle {
            label: "Split diff (side-by-side)".into(),
            get: |c| c.features.split_diff,
            set: |c, v| c.features.split_diff = v,
        },
        SettingsItem::BoolToggle {
            label: "Exit heatmap".into(),
            get: |c| c.features.exit_heatmap,
            set: |c, v| c.features.exit_heatmap = v,
        },
        SettingsItem::BoolToggle {
            label: "Blame annotations".into(),
            get: |c| c.features.blame_annotations,
            set: |c, v| c.features.blame_annotations = v,
        },
        SettingsItem::BoolToggle {
            label: "Bookmarks".into(),
            get: |c| c.features.bookmarks,
            set: |c, v| c.features.bookmarks = v,
        },
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
            label: "AI overlays (v/V)".into(),
            get: |c| c.features.ai_overlays,
            set: |c, v| c.features.ai_overlays = v,
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
        SettingsItem::NumberEdit {
            label: "Tab width".into(),
            get: |c| c.display.tab_width,
            set: |c, v| c.display.tab_width = v,
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
