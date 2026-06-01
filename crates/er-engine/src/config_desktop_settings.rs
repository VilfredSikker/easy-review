//! Serializable settings schema for the desktop app (excludes diff-view fields).

use super::{split_shell_args, AGENT_EFFORT_OPTIONS, ErConfig};
use serde::{Deserialize, Serialize};

/// Wire value for a single config field patch from the desktop settings UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ConfigFieldValue {
    Bool(bool),
    String(String),
    Number(u64),
}

/// One row in the desktop settings page.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ConfigHubFieldDto {
    Section { title: String },
    Bool {
        key: String,
        label: String,
        description: String,
        value: bool,
    },
    Cycle {
        key: String,
        label: String,
        description: String,
        options: Vec<String>,
        value: String,
    },
    Text {
        key: String,
        label: String,
        description: String,
        placeholder: String,
        value: String,
        strict: bool,
    },
    ListEntry {
        key: String,
        label: String,
        index: usize,
    },
    ListAdd { key: String, label: String },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopSettingsSnapshot {
    pub fields: Vec<ConfigHubFieldDto>,
    pub agent_effort: String,
    pub has_local_config: bool,
    pub repo_root: String,
}

const THEME_OPTIONS: &[&str] = &[
    "ocean-depth",
    "moonlight",
    "daybreak",
    "high-contrast",
    "tokyo-night",
    "tokyo-night-storm",
    "tokyo-night-moon",
    "tokyo-night-day",
];

pub fn desktop_settings_fields(config: &ErConfig) -> Vec<ConfigHubFieldDto> {
    let mut fields: Vec<ConfigHubFieldDto> = vec![
        ConfigHubFieldDto::Section {
            title: "Views".into(),
        },
        ConfigHubFieldDto::Bool {
            key: "features.view_branch".into(),
            label: "Branch diff".into(),
            description: "Show branch diff mode".into(),
            value: config.features.view_branch,
        },
        ConfigHubFieldDto::Bool {
            key: "features.view_unstaged".into(),
            label: "Unstaged changes".into(),
            description: "Show unstaged changes mode".into(),
            value: config.features.view_unstaged,
        },
        ConfigHubFieldDto::Bool {
            key: "features.view_staged".into(),
            label: "Staged changes".into(),
            description: "Show staged changes mode".into(),
            value: config.features.view_staged,
        },
        ConfigHubFieldDto::Bool {
            key: "features.view_history".into(),
            label: "History".into(),
            description: "Show commit history mode".into(),
            value: config.features.view_history,
        },
        ConfigHubFieldDto::Bool {
            key: "features.view_conflicts".into(),
            label: "Conflicts".into(),
            description: "Show merge conflicts mode".into(),
            value: config.features.view_conflicts,
        },
        ConfigHubFieldDto::Bool {
            key: "features.view_hidden".into(),
            label: "Hidden files".into(),
            description: "Show hidden files mode".into(),
            value: config.features.view_hidden,
        },
        ConfigHubFieldDto::Section {
            title: "Appearance".into(),
        },
        ConfigHubFieldDto::Cycle {
            key: "display.theme".into(),
            label: "Theme".into(),
            description: "Application color theme (TUI)".into(),
            options: THEME_OPTIONS.iter().map(|s| s.to_string()).collect(),
            value: config.display.theme.clone(),
        },
        ConfigHubFieldDto::Section {
            title: "Key Hints".into(),
        },
        ConfigHubFieldDto::Bool {
            key: "hints.navigation".into(),
            label: "Navigation hints".into(),
            description: "Show navigation key hints".into(),
            value: config.hints.navigation,
        },
        ConfigHubFieldDto::Bool {
            key: "hints.staging".into(),
            label: "Staging hints".into(),
            description: "Show staging key hints".into(),
            value: config.hints.staging,
        },
        ConfigHubFieldDto::Bool {
            key: "hints.comments".into(),
            label: "Comment hints".into(),
            description: "Show comment action hints".into(),
            value: config.hints.comments,
        },
        ConfigHubFieldDto::Bool {
            key: "hints.verbose".into(),
            label: "Verbose hints".into(),
            description: "Show extended key hints".into(),
            value: config.hints.verbose,
        },
        ConfigHubFieldDto::Section {
            title: "Commands".into(),
        },
        ConfigHubFieldDto::Text {
            key: "commands.summary".into(),
            label: "Summary".into(),
            description: "Generate diff summary".into(),
            placeholder: "e.g. claude -p 'Summarize: {diff}'".into(),
            value: config.commands.summary.clone().unwrap_or_default(),
            strict: true,
        },
        ConfigHubFieldDto::Text {
            key: "commands.test".into(),
            label: "Test".into(),
            description: "Run tests".into(),
            placeholder: "e.g. cargo test".into(),
            value: config.commands.test.clone().unwrap_or_default(),
            strict: true,
        },
        ConfigHubFieldDto::Text {
            key: "commands.lint".into(),
            label: "Lint".into(),
            description: "Run linter".into(),
            placeholder: "e.g. cargo clippy".into(),
            value: config.commands.lint.clone().unwrap_or_default(),
            strict: true,
        },
        ConfigHubFieldDto::Text {
            key: "commands.typecheck".into(),
            label: "Typecheck".into(),
            description: "Run type checker".into(),
            placeholder: "e.g. tsc --noEmit".into(),
            value: config.commands.typecheck.clone().unwrap_or_default(),
            strict: true,
        },
        ConfigHubFieldDto::Text {
            key: "commands.security".into(),
            label: "Security".into(),
            description: "Run security scan".into(),
            placeholder: "e.g. cargo audit".into(),
            value: config.commands.security.clone().unwrap_or_default(),
            strict: true,
        },
        ConfigHubFieldDto::Bool {
            key: "summary.push_to_pr".into(),
            label: "Push summary to PR body".into(),
            description: "Auto-push summary to GitHub PR".into(),
            value: config.summary.push_to_pr,
        },
        ConfigHubFieldDto::Section {
            title: "Agent".into(),
        },
        ConfigHubFieldDto::Text {
            key: "agent.command".into(),
            label: "Command".into(),
            description: "Agent executable".into(),
            placeholder: "e.g. claude".into(),
            value: config.agent.command.clone(),
            strict: true,
        },
        ConfigHubFieldDto::Text {
            key: "agent.args".into(),
            label: "Args".into(),
            description: "Agent arguments (space-separated)".into(),
            placeholder: "e.g. --print -p {prompt}".into(),
            value: config.agent.args.join(" "),
            strict: true,
        },
        ConfigHubFieldDto::Cycle {
            key: "agent.effort".into(),
            label: "Effort".into(),
            description: "Claude effort level".into(),
            options: AGENT_EFFORT_OPTIONS.iter().map(|s| s.to_string()).collect(),
            value: config.agent.effort.clone(),
        },
        ConfigHubFieldDto::Section {
            title: "Watched Paths".into(),
        },
        ConfigHubFieldDto::Cycle {
            key: "watched.diff_mode".into(),
            label: "Diff mode".into(),
            description: "How to diff watched files".into(),
            options: vec!["content".into(), "snapshot".into()],
            value: config.watched.diff_mode.clone(),
        },
    ];

    for (i, path) in config.watched.paths.iter().enumerate() {
        fields.push(ConfigHubFieldDto::ListEntry {
            key: format!("watched.paths.{i}"),
            label: path.clone(),
            index: i,
        });
    }

    fields.push(ConfigHubFieldDto::ListAdd {
        key: "watched.paths.add".into(),
        label: "Add pattern…".into(),
    });

    fields
}

pub fn desktop_settings_snapshot(config: &ErConfig, repo_root: &str) -> DesktopSettingsSnapshot {
    let local_path = format!("{repo_root}/.er-config.toml");
    let has_local_config = std::path::Path::new(&local_path).exists();
    DesktopSettingsSnapshot {
        fields: desktop_settings_fields(config),
        agent_effort: config.agent.effort.clone(),
        has_local_config,
        repo_root: repo_root.to_string(),
    }
}

pub fn validate_config_text_field(key: &str, value: &str) -> Option<String> {
    match key {
        "agent.args" if !value.contains("{prompt}") => {
            Some("Include {prompt} in args so the agent receives user input.".into())
        }
        "agent.command" if value.trim().is_empty() => Some("Command cannot be empty.".into()),
        _ => None,
    }
}

pub fn apply_config_field(config: &mut ErConfig, key: &str, value: ConfigFieldValue) -> bool {
    let mut watched_changed = false;
    match key {
        "features.view_branch" => {
            if let ConfigFieldValue::Bool(v) = value {
                config.features.view_branch = v;
            }
        }
        "features.view_unstaged" => {
            if let ConfigFieldValue::Bool(v) = value {
                config.features.view_unstaged = v;
            }
        }
        "features.view_staged" => {
            if let ConfigFieldValue::Bool(v) = value {
                config.features.view_staged = v;
            }
        }
        "features.view_history" => {
            if let ConfigFieldValue::Bool(v) = value {
                config.features.view_history = v;
            }
        }
        "features.view_conflicts" => {
            if let ConfigFieldValue::Bool(v) = value {
                config.features.view_conflicts = v;
            }
        }
        "features.view_hidden" => {
            if let ConfigFieldValue::Bool(v) = value {
                config.features.view_hidden = v;
            }
        }
        "display.theme" => {
            if let ConfigFieldValue::String(v) = value {
                config.display.theme = v;
            }
        }
        "hints.navigation" => {
            if let ConfigFieldValue::Bool(v) = value {
                config.hints.navigation = v;
            }
        }
        "hints.staging" => {
            if let ConfigFieldValue::Bool(v) = value {
                config.hints.staging = v;
            }
        }
        "hints.comments" => {
            if let ConfigFieldValue::Bool(v) = value {
                config.hints.comments = v;
            }
        }
        "hints.verbose" => {
            if let ConfigFieldValue::Bool(v) = value {
                config.hints.verbose = v;
            }
        }
        "commands.summary" => {
            if let ConfigFieldValue::String(v) = value {
                config.commands.summary = if v.is_empty() { None } else { Some(v) };
            }
        }
        "commands.test" => {
            if let ConfigFieldValue::String(v) = value {
                config.commands.test = if v.is_empty() { None } else { Some(v) };
            }
        }
        "commands.lint" => {
            if let ConfigFieldValue::String(v) = value {
                config.commands.lint = if v.is_empty() { None } else { Some(v) };
            }
        }
        "commands.typecheck" => {
            if let ConfigFieldValue::String(v) = value {
                config.commands.typecheck = if v.is_empty() { None } else { Some(v) };
            }
        }
        "commands.security" => {
            if let ConfigFieldValue::String(v) = value {
                config.commands.security = if v.is_empty() { None } else { Some(v) };
            }
        }
        "summary.push_to_pr" => {
            if let ConfigFieldValue::Bool(v) = value {
                config.summary.push_to_pr = v;
            }
        }
        "agent.command" => {
            if let ConfigFieldValue::String(v) = value {
                if !v.is_empty() {
                    config.agent.command = v;
                }
            }
        }
        "agent.args" => {
            if let ConfigFieldValue::String(v) = value {
                config.agent.args = split_shell_args(&v);
            }
        }
        "agent.effort" => {
            if let ConfigFieldValue::String(v) = value {
                if AGENT_EFFORT_OPTIONS.contains(&v.as_str()) {
                    config.agent.effort = v;
                }
            }
        }
        "watched.diff_mode" => {
            if let ConfigFieldValue::String(v) = value {
                if v == "content" || v == "snapshot" {
                    config.watched.diff_mode = v;
                    watched_changed = true;
                }
            }
        }
        "watched.paths.add" => {
            if let ConfigFieldValue::String(v) = value {
                let trimmed = v.trim();
                if !trimmed.is_empty() && !config.watched.paths.iter().any(|p| p == trimmed) {
                    config.watched.paths.push(trimmed.to_string());
                    watched_changed = true;
                }
            }
        }
        "watched.paths.remove" => {
            if let ConfigFieldValue::Number(idx) = value {
                if (idx as usize) < config.watched.paths.len() {
                    config.watched.paths.remove(idx as usize);
                    watched_changed = true;
                }
            }
        }
        _ => {}
    }
    watched_changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ErConfig;

    #[test]
    fn desktop_settings_excludes_diff_display_fields() {
        let config = ErConfig::default();
        let fields = desktop_settings_fields(&config);
        let keys: Vec<String> = fields
            .iter()
            .filter_map(|f| match f {
                ConfigHubFieldDto::Bool { key, .. }
                | ConfigHubFieldDto::Cycle { key, .. }
                | ConfigHubFieldDto::Text { key, .. } => Some(key.clone()),
                _ => None,
            })
            .collect();
        assert!(!keys.iter().any(|k| k == "display.line_numbers"));
        assert!(!keys.iter().any(|k| k == "display.split_diff"));
        assert!(keys.iter().any(|k| k == "display.theme"));
    }

    #[test]
    fn apply_config_field_agent_effort_round_trip() {
        let mut config = ErConfig::default();
        apply_config_field(
            &mut config,
            "agent.effort",
            ConfigFieldValue::String("high".into()),
        );
        assert_eq!(config.agent.effort, "high");
    }

    #[test]
    fn validate_agent_args_requires_prompt_placeholder() {
        assert!(validate_config_text_field("agent.args", "--print").is_some());
        assert!(validate_config_text_field("agent.args", "-p {prompt}").is_none());
    }
}
