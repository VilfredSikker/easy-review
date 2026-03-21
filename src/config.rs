use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ErConfig {
    #[serde(default)]
    pub features: FeatureFlags,
    #[serde(default)]
    pub agent: AgentConfig,
    #[serde(default)]
    pub summary: SummaryConfig,
    #[serde(default)]
    pub display: DisplayConfig,
    #[serde(default)]
    pub watched: WatchedConfig,
    #[serde(default)]
    pub hints: HintConfig,
    #[serde(default)]
    pub commands: CommandsConfig,
}

/// [commands] section — configurable shell commands for hub actions.
/// Each command is a shell string run via `sh -c`. Placeholders:
/// `{base}` (base branch), `{branch}` (current branch), `{repo}` (repo root),
/// `{output}` (default output path, e.g. `{repo}/.er/summary.md`).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CommandsConfig {
    /// Generate diff summary (AI hub)
    #[serde(default)]
    pub summary: Option<String>,
    /// Run tests (Verify hub)
    #[serde(default)]
    pub test: Option<String>,
    /// Run linter (Verify hub)
    #[serde(default)]
    pub lint: Option<String>,
    /// Type check (Verify hub)
    #[serde(default)]
    pub typecheck: Option<String>,
    /// Security scan (Verify hub)
    #[serde(default)]
    pub security: Option<String>,
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

fn default_theme() -> String {
    "ocean-depth".to_string()
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
    #[serde(default = "default_true")]
    pub view_hidden: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    #[serde(default = "default_agent_cmd")]
    pub command: String,
    #[serde(default = "default_agent_args")]
    pub args: Vec<String>,
}

/// [summary] section — configuration for diff summary / changelog generation
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SummaryConfig {
    /// Command to run for summary generation (defaults to agent.command)
    #[serde(default)]
    pub command: Option<String>,
    /// Args for the summary command ({diff} is replaced with the raw diff)
    #[serde(default)]
    pub args: Option<Vec<String>>,
    /// Whether to auto-push the summary to the GitHub PR body
    #[serde(default)]
    pub push_to_pr: bool,
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
    /// Auto-expand context for files with ≤ this many diff lines (0 to disable)
    #[serde(default = "default_auto_context_threshold")]
    pub auto_context_threshold: usize,
    #[serde(default = "default_theme")]
    pub theme: String,
}

/// [hints] section — toggle visibility of key hint groups in the bottom bar
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HintConfig {
    #[serde(default = "default_true")]
    pub navigation: bool,
    #[serde(default = "default_true")]
    pub comments: bool,
    #[serde(default = "default_true")]
    pub staging: bool,
    #[serde(default)]
    pub verbose: bool,
}

impl Default for HintConfig {
    fn default() -> Self {
        Self {
            navigation: true,
            comments: true,
            staging: true,
            verbose: false,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_tab_width() -> u8 {
    4
}

fn default_auto_context_threshold() -> usize {
    50
}

fn default_agent_cmd() -> String {
    "claude".into()
}

fn default_agent_args() -> Vec<String> {
    // TODO(risk:medium): the {prompt} placeholder must be present in args for the agent command
    // to receive user input. If a user overrides `agent.args` in their config and omits
    // {prompt}, the prompt is silently dropped and the agent runs with no meaningful input.
    // Validate that {prompt} appears in args when loading config.
    vec![
        "--print".into(),
        "--output-format".into(),
        "stream-json".into(),
        "-p".into(),
        "{prompt}".into(),
    ]
}

impl Default for FeatureFlags {
    fn default() -> Self {
        Self {
            view_branch: true,
            view_unstaged: true,
            view_staged: true,
            view_history: true,
            view_conflicts: true,
            view_hidden: true,
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

impl AgentConfig {
    /// Human-readable name for the agent (derived from command basename).
    pub fn display_name(&self) -> String {
        let basename = self.command.rsplit('/').next().unwrap_or(&self.command);
        // Capitalize first letter
        let mut chars = basename.chars();
        match chars.next() {
            Some(c) => c.to_uppercase().to_string() + chars.as_str(),
            None => "AI".to_string(),
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
            auto_context_threshold: default_auto_context_threshold(),
            theme: default_theme(),
        }
    }
}

impl ErConfig {
    /// Resolve a command by name from the [commands] config section.
    /// Returns None if the command is not configured — callers should
    /// disable the action and show a "not configured" hint.
    pub fn resolve_command(&self, name: &str) -> Option<String> {
        match name {
            "summary" => self.commands.summary.clone(),
            "test" => self.commands.test.clone(),
            "lint" => self.commands.lint.clone(),
            "typecheck" => self.commands.typecheck.clone(),
            "security" => self.commands.security.clone(),
            _ => None,
        }
    }
}

/// Load config by merging global defaults with per-repo overrides.
/// Priority: per-repo `.er-config.toml` > global `~/.config/er/config.toml` > built-in defaults.
/// Merging is deep: individual fields within sections (e.g. `[features]`) override independently.
pub fn load_config(repo_root: &str) -> ErConfig {
    let local_path = format!("{repo_root}/.er-config.toml");
    // Prefer XDG (~/.config/er/config.toml), fall back to dirs::config_dir()
    let global_path = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .map(|xdg| format!("{xdg}/er/config.toml"))
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| format!("{h}/.config/er/config.toml"))
        })
        .filter(|p| std::path::Path::new(p).exists())
        .or_else(|| {
            dirs::config_dir().map(|d| d.join("er/config.toml").to_string_lossy().to_string())
        });

    let global_table = global_path
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|c| c.parse::<toml::Table>().ok());

    let local_table = std::fs::read_to_string(&local_path)
        .ok()
        .and_then(|c| c.parse::<toml::Table>().ok());
    let user_table = match (global_table, local_table) {
        (Some(mut global), Some(local)) => {
            deep_merge(&mut global, local);
            global
        }
        (Some(global), None) => global,
        (None, Some(local)) => local,
        (None, None) => return ErConfig::default(),
    };

    toml::Value::Table(user_table)
        .try_into()
        .unwrap_or_default()
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
    let dir = std::env::var("XDG_CONFIG_HOME")
        .map(|xdg| std::path::PathBuf::from(format!("{xdg}/er")))
        .or_else(|_| {
            std::env::var("HOME").map(|h| std::path::PathBuf::from(format!("{h}/.config/er")))
        })
        .or_else(|_| {
            dirs::config_dir()
                .map(|d| d.join("er"))
                .ok_or(std::env::VarError::NotPresent)
        })
        .map_err(|_| anyhow::anyhow!("Could not determine config directory"))?;
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

/// Config hub item types for the enhanced config hub UI.
#[derive(Clone)]
pub enum ConfigItem {
    SectionHeader(String),
    BoolToggle {
        label: String,
        description: String,
        get: fn(&ErConfig) -> bool,
        set: fn(&mut ErConfig, bool),
    },
    StringCycle {
        label: String,
        description: String,
        options: &'static [&'static str],
        get: fn(&ErConfig) -> String,
        set: fn(&mut ErConfig, String),
    },
    StringEdit {
        label: String,
        description: String,
        placeholder: String,
        get: fn(&ErConfig) -> String,
        set: fn(&mut ErConfig, String),
    },
    NumberEdit {
        label: String,
        description: String,
        min: u8,
        max: u8,
        get: fn(&ErConfig) -> u8,
        set: fn(&mut ErConfig, u8),
    },
    ListEntry {
        label: String,
        index: usize,
    },
    ListAdd {
        label: String,
        section: String,
    },
}

impl std::fmt::Debug for ConfigItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigItem::SectionHeader(s) => write!(f, "SectionHeader({:?})", s),
            ConfigItem::BoolToggle { label, .. } => write!(f, "BoolToggle({:?})", label),
            ConfigItem::StringCycle { label, .. } => write!(f, "StringCycle({:?})", label),
            ConfigItem::StringEdit { label, .. } => write!(f, "StringEdit({:?})", label),
            ConfigItem::NumberEdit { label, .. } => write!(f, "NumberEdit({:?})", label),
            ConfigItem::ListEntry { label, index } => {
                write!(f, "ListEntry({:?}, {})", label, index)
            }
            ConfigItem::ListAdd { label, section } => {
                write!(f, "ListAdd({:?}, {:?})", label, section)
            }
        }
    }
}

/// Build the list of config hub items for the config hub overlay.
pub fn config_hub_items(config: &ErConfig) -> Vec<ConfigItem> {
    let mut items: Vec<ConfigItem> = vec![
        // ── Views ──
        ConfigItem::SectionHeader("Views".into()),
        ConfigItem::BoolToggle {
            label: "Branch diff (1)".into(),
            description: "Show branch diff mode".into(),
            get: |c| c.features.view_branch,
            set: |c, v| c.features.view_branch = v,
        },
        ConfigItem::BoolToggle {
            label: "Unstaged changes (2)".into(),
            description: "Show unstaged changes mode".into(),
            get: |c| c.features.view_unstaged,
            set: |c, v| c.features.view_unstaged = v,
        },
        ConfigItem::BoolToggle {
            label: "Staged changes (3)".into(),
            description: "Show staged changes mode".into(),
            get: |c| c.features.view_staged,
            set: |c, v| c.features.view_staged = v,
        },
        ConfigItem::BoolToggle {
            label: "History (4)".into(),
            description: "Show commit history mode".into(),
            get: |c| c.features.view_history,
            set: |c, v| c.features.view_history = v,
        },
        ConfigItem::BoolToggle {
            label: "Conflicts (5)".into(),
            description: "Show merge conflicts mode".into(),
            get: |c| c.features.view_conflicts,
            set: |c, v| c.features.view_conflicts = v,
        },
        ConfigItem::BoolToggle {
            label: "Hidden files (6)".into(),
            description: "Show hidden files mode".into(),
            get: |c| c.features.view_hidden,
            set: |c, v| c.features.view_hidden = v,
        },
        // ── Display ──
        ConfigItem::SectionHeader("Display".into()),
        ConfigItem::StringCycle {
            label: "Theme".into(),
            description: "Color theme".into(),
            options: &["ocean-depth", "moonlight", "daybreak", "high-contrast"],
            get: |c| c.display.theme.clone(),
            set: |c, v| c.display.theme = v,
        },
        ConfigItem::BoolToggle {
            label: "Line numbers".into(),
            description: "Show line numbers in diff".into(),
            get: |c| c.display.line_numbers,
            set: |c, v| c.display.line_numbers = v,
        },
        ConfigItem::BoolToggle {
            label: "Wrap lines".into(),
            description: "Wrap long lines".into(),
            get: |c| c.display.wrap_lines,
            set: |c, v| c.display.wrap_lines = v,
        },
        ConfigItem::BoolToggle {
            label: "Split diff".into(),
            description: "Side-by-side diff view".into(),
            get: |c| c.display.split_diff,
            set: |c, v| c.display.split_diff = v,
        },
        ConfigItem::NumberEdit {
            label: "Tab width".into(),
            description: "Spaces per tab stop".into(),
            min: 1,
            max: 16,
            get: |c| c.display.tab_width,
            set: |c, v| c.display.tab_width = v,
        },
        // ── Key Hints ──
        ConfigItem::SectionHeader("Key Hints".into()),
        ConfigItem::BoolToggle {
            label: "Navigation hints".into(),
            description: "Show j/k, n/N, ␣, / hints".into(),
            get: |c| c.hints.navigation,
            set: |c, v| c.hints.navigation = v,
        },
        ConfigItem::BoolToggle {
            label: "Staging hints".into(),
            description: "Show s, c commit hints".into(),
            get: |c| c.hints.staging,
            set: |c, v| c.hints.staging = v,
        },
        ConfigItem::BoolToggle {
            label: "Comment hints".into(),
            description: "Show r, d comment action hints".into(),
            get: |c| c.hints.comments,
            set: |c, v| c.hints.comments = v,
        },
        ConfigItem::BoolToggle {
            label: "Verbose hints".into(),
            description: "Show all key hints".into(),
            get: |c| c.hints.verbose,
            set: |c, v| c.hints.verbose = v,
        },
        // ── Commands ──
        ConfigItem::SectionHeader("Commands".into()),
        ConfigItem::StringEdit {
            label: "Summary".into(),
            description: "Generate diff summary".into(),
            placeholder: "e.g. claude -p 'Summarize: {diff}'".into(),
            get: |c| c.commands.summary.clone().unwrap_or_default(),
            set: |c, v| c.commands.summary = if v.is_empty() { None } else { Some(v) },
        },
        ConfigItem::StringEdit {
            label: "Test".into(),
            description: "Run tests".into(),
            placeholder: "e.g. cargo test".into(),
            get: |c| c.commands.test.clone().unwrap_or_default(),
            set: |c, v| c.commands.test = if v.is_empty() { None } else { Some(v) },
        },
        ConfigItem::StringEdit {
            label: "Lint".into(),
            description: "Run linter".into(),
            placeholder: "e.g. cargo clippy".into(),
            get: |c| c.commands.lint.clone().unwrap_or_default(),
            set: |c, v| c.commands.lint = if v.is_empty() { None } else { Some(v) },
        },
        ConfigItem::StringEdit {
            label: "Typecheck".into(),
            description: "Run type checker".into(),
            placeholder: "e.g. tsc --noEmit".into(),
            get: |c| c.commands.typecheck.clone().unwrap_or_default(),
            set: |c, v| c.commands.typecheck = if v.is_empty() { None } else { Some(v) },
        },
        ConfigItem::StringEdit {
            label: "Security".into(),
            description: "Run security scan".into(),
            placeholder: "e.g. cargo audit".into(),
            get: |c| c.commands.security.clone().unwrap_or_default(),
            set: |c, v| c.commands.security = if v.is_empty() { None } else { Some(v) },
        },
        ConfigItem::BoolToggle {
            label: "Push summary to PR body".into(),
            description: "Auto-push summary to GitHub PR".into(),
            get: |c| c.summary.push_to_pr,
            set: |c, v| c.summary.push_to_pr = v,
        },
        // ── Agent ──
        ConfigItem::SectionHeader("Agent".into()),
        ConfigItem::StringEdit {
            label: "Command".into(),
            description: "Agent executable".into(),
            placeholder: "e.g. claude".into(),
            get: |c| c.agent.command.clone(),
            set: |c, v| {
                if !v.is_empty() {
                    c.agent.command = v
                }
            },
        },
        ConfigItem::StringEdit {
            label: "Args".into(),
            description: "Agent arguments (space-separated)".into(),
            placeholder: "e.g. --print -p {prompt}".into(),
            get: |c| c.agent.args.join(" "),
            set: |c, v| c.agent.args = v.split_whitespace().map(|s| s.to_string()).collect(),
        },
        // ── Watched Paths ──
        ConfigItem::SectionHeader("Watched Paths".into()),
        ConfigItem::StringCycle {
            label: "Diff mode".into(),
            description: "How to diff watched files".into(),
            options: &["content", "snapshot"],
            get: |c| c.watched.diff_mode.clone(),
            set: |c, v| c.watched.diff_mode = v,
        },
    ];

    // One ListEntry per watched path
    for (i, path) in config.watched.paths.iter().enumerate() {
        items.push(ConfigItem::ListEntry {
            label: path.clone(),
            index: i,
        });
    }

    items.push(ConfigItem::ListAdd {
        label: "Add pattern...".into(),
        section: "watched".into(),
    });

    items
}

/// Save config to the repo-local `.er-config.toml` (atomic tmp+rename).
pub fn save_config_local(config: &ErConfig, repo_root: &str) -> Result<()> {
    let dir = std::path::Path::new(repo_root);
    let path = dir.join(".er-config.toml");
    let tmp_path = dir.join(".er-config.toml.tmp");
    let content = toml::to_string_pretty(config)?;
    std::fs::write(&tmp_path, content)?;
    std::fs::rename(&tmp_path, &path)?;
    Ok(())
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
        assert!(flags.view_hidden);
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
                view_hidden: true,
            },
            display: DisplayConfig {
                tab_width: 8,
                line_numbers: false,
                wrap_lines: true,
                split_diff: true,
                auto_context_threshold: 100,
                theme: "moonlight".into(),
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
}
