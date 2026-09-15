//! Serializable settings schema for the desktop app (excludes diff-view fields).

use super::settings::{agent_effort_label, settings_fields_grouped};
use super::{split_shell_args, ErConfig, AGENT_EFFORT_OPTIONS, MAX_CONCURRENT_REVIEWS_RANGE};
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
    Section {
        title: String,
    },
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
    ListAdd {
        key: String,
        label: String,
    },
}

/// One declared importance rule, as written, for the read-only list.
///
/// The key is shown rather than resolved: the question a reviewer has is "why
/// is this file ranked foundational?", and the answer is which rule claimed it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportanceRuleDto {
    pub matcher: String,
    pub tier: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopSettingsSnapshot {
    pub general: Vec<ConfigHubFieldDto>,
    pub app: Vec<ConfigHubFieldDto>,
    pub terminal: Vec<ConfigHubFieldDto>,
    pub agent_effort: String,
    pub repo_root: String,
    /// The active repo's declared importance rules, for the read-only view.
    pub importance_rules: Vec<ImportanceRuleDto>,
    /// What a path no rule claims resolves to.
    pub importance_default: String,
}

pub fn desktop_settings_snapshot(config: &ErConfig, repo_root: &str) -> DesktopSettingsSnapshot {
    let grouped = settings_fields_grouped(config);

    // Keyed the way managed storage keys a repo, so the table the agent writes
    // and the bucket a review lands in agree on the repo's name.
    let repo_slug = crate::storage::slug_repo(repo_root);
    let rules = config.importance.repo(&repo_slug);
    let importance_rules = rules
        .map(|table| {
            table
                .rules
                .iter()
                .map(|(matcher, tier)| ImportanceRuleDto {
                    matcher: matcher.clone(),
                    tier: tier.clone(),
                })
                .collect()
        })
        .unwrap_or_default();
    let importance_default = rules
        .and_then(|table| table.default.clone())
        .unwrap_or_else(|| "normal".to_string());

    DesktopSettingsSnapshot {
        general: grouped.general,
        app: grouped.app,
        terminal: grouped.terminal,
        agent_effort: agent_effort_label(&config.agent.effort),
        repo_root: repo_root.to_string(),
        importance_rules,
        importance_default,
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
    if let Some(kind) = key.strip_prefix("inbox.show.") {
        if let ConfigFieldValue::Bool(v) = value {
            let _ = config.inbox.show.set(kind, v);
        }
        return false;
    }
    if let Some(kind) = key.strip_prefix("inbox.notify.") {
        if let ConfigFieldValue::Bool(v) = value {
            let _ = config.inbox.notify.set(kind, v);
        }
        return false;
    }

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
        "features.model_discovery" => {
            if let ConfigFieldValue::Bool(v) = value {
                config.features.model_discovery = v;
            }
        }
        "display.theme" => {
            if let ConfigFieldValue::String(v) = value {
                config.display.theme = v;
            }
        }
        "display.line_numbers" => {
            if let ConfigFieldValue::Bool(v) = value {
                config.display.line_numbers = v;
            }
        }
        "display.wrap_lines" => {
            if let ConfigFieldValue::Bool(v) = value {
                config.display.wrap_lines = v;
            }
        }
        "display.split_diff" => {
            if let ConfigFieldValue::Bool(v) = value {
                config.display.split_diff = v;
            }
        }
        "display.auto_context" => {
            if let ConfigFieldValue::Bool(v) = value {
                config.display.auto_context_threshold = if v { 1 } else { 0 };
            }
        }
        "display.tab_width" => match value {
            ConfigFieldValue::Number(n) if (1..=16).contains(&(n as u8)) => {
                config.display.tab_width = n as u8;
            }
            ConfigFieldValue::String(v) => {
                if let Ok(n) = v.parse::<u8>() {
                    if (1..=16).contains(&n) {
                        config.display.tab_width = n;
                    }
                }
            }
            _ => {}
        },
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
                    config.agent.effort = Some(v);
                }
            }
        }
        "ai_hub.max_concurrent_reviews" => {
            let parsed = match value {
                ConfigFieldValue::Number(n) => Some(n as usize),
                ConfigFieldValue::String(v) => v.parse::<usize>().ok(),
                ConfigFieldValue::Bool(_) => None,
            };
            if let Some(n) = parsed.filter(|n| MAX_CONCURRENT_REVIEWS_RANGE.contains(n)) {
                config.ai_hub.max_concurrent_reviews = n;
            }
        }
        "ai_hub.max_concurrent_arena_reviews" => {
            let parsed = match value {
                ConfigFieldValue::Number(n) => Some(n as usize),
                ConfigFieldValue::String(v) => v.parse::<usize>().ok(),
                ConfigFieldValue::Bool(_) => None,
            };
            if let Some(n) = parsed.filter(|n| MAX_CONCURRENT_REVIEWS_RANGE.contains(n)) {
                config.ai_hub.max_concurrent_arena_reviews = n;
            }
        }
        "ai_hub.max_concurrent_agents" => {
            let parsed = match value {
                ConfigFieldValue::Number(n) => Some(n as usize),
                ConfigFieldValue::String(v) => v.parse::<usize>().ok(),
                ConfigFieldValue::Bool(_) => None,
            };
            if let Some(n) = parsed.filter(|n| MAX_CONCURRENT_REVIEWS_RANGE.contains(n)) {
                config.ai_hub.max_concurrent_agents = n;
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
    fn the_cap_picker_offers_exactly_what_the_setter_accepts() {
        // These disagreed: the picker offered 1-6 while the write path
        // accepted 1-16, so a value in 7-16 was kept but unselectable, and the
        // picker could not reach its own maximum. Both now read one constant.
        use crate::config::settings::{settings_fields_grouped, SettingsScope};

        let grouped = settings_fields_grouped(&ErConfig::default());
        let field = grouped
            .general
            .iter()
            .chain(grouped.app.iter())
            .chain(grouped.terminal.iter())
            .find(|f| matches!(f, ConfigHubFieldDto::Cycle { key, .. } if key == "ai_hub.max_concurrent_reviews"))
            .expect("the cap has a settings row");

        let ConfigHubFieldDto::Cycle { options, .. } = field else {
            unreachable!("matched on Cycle above");
        };
        let offered: Vec<usize> = options
            .iter()
            .map(|s| s.parse::<usize>().expect("numeric options"))
            .collect();
        assert_eq!(offered, MAX_CONCURRENT_REVIEWS_RANGE.collect::<Vec<_>>());

        // And the setter agrees with the picker at both ends.
        let mut config = ErConfig::default();
        for n in [
            *MAX_CONCURRENT_REVIEWS_RANGE.start(),
            *MAX_CONCURRENT_REVIEWS_RANGE.end(),
        ] {
            apply_config_field(
                &mut config,
                "ai_hub.max_concurrent_reviews",
                ConfigFieldValue::Number(n as u64),
            );
            assert_eq!(
                config.ai_hub.max_concurrent_reviews, n,
                "the setter must accept what the picker offers"
            );
        }
        let _ = SettingsScope::General;
    }

    #[test]
    fn settings_scopes_partition_fields() {
        use crate::config::settings::{settings_fields_grouped, SettingsScope};

        let config = ErConfig::default();
        let grouped = settings_fields_grouped(&config);

        fn keys(fields: &[ConfigHubFieldDto]) -> Vec<String> {
            fields
                .iter()
                .filter_map(|f| match f {
                    ConfigHubFieldDto::Bool { key, .. }
                    | ConfigHubFieldDto::Cycle { key, .. }
                    | ConfigHubFieldDto::Text { key, .. } => Some(key.clone()),
                    _ => None,
                })
                .collect()
        }

        let general = keys(&grouped.general);
        let app = keys(&grouped.app);
        let terminal = keys(&grouped.terminal);

        assert!(!general.iter().any(|k| k == "features.view_branch"));
        assert!(!general.iter().any(|k| k == "display.line_numbers"));
        assert!(!general.iter().any(|k| k == "features.arena"));
        assert!(app.is_empty());

        // Theme is shared — the desktop app follows it too.
        assert!(general.iter().any(|k| k == "display.theme"));
        assert!(!terminal.iter().any(|k| k == "display.theme"));

        assert!(terminal.iter().any(|k| k == "features.view_branch"));
        assert!(general.iter().any(|k| k == "features.model_discovery"));
        assert!(general.iter().any(|k| k == "inbox.show.review_requested"));
        assert!(general.iter().any(|k| k == "inbox.notify.ci_failed"));
        assert!(!terminal.iter().any(|k| k == "inbox.show.review_requested"));
        assert!(!terminal.iter().any(|k| k == "features.model_discovery"));
        assert!(terminal.iter().any(|k| k == "display.line_numbers"));

        assert_eq!(
            crate::config::desktop_settings_fields_for_scope(&config, SettingsScope::General).len(),
            grouped.general.len()
        );
    }

    #[test]
    fn apply_config_field_inbox_show_and_notify_round_trip() {
        let mut config = ErConfig::default();
        assert!(config.inbox.shows("ci_failed"));
        assert!(config.inbox.notifies("review_requested"));
        apply_config_field(
            &mut config,
            "inbox.show.ci_failed",
            ConfigFieldValue::Bool(false),
        );
        apply_config_field(
            &mut config,
            "inbox.notify.review_requested",
            ConfigFieldValue::Bool(false),
        );
        assert!(!config.inbox.shows("ci_failed"));
        assert!(config.inbox.shows("pr_merged"));
        assert!(!config.inbox.notifies("review_requested"));
        assert!(config.inbox.notifies("ci_failed"));
    }

    #[test]
    fn apply_config_field_model_discovery_round_trip() {
        let mut config = ErConfig::default();
        assert!(config.features.model_discovery);
        apply_config_field(
            &mut config,
            "features.model_discovery",
            ConfigFieldValue::Bool(false),
        );
        assert!(!config.features.model_discovery);
    }

    #[test]
    fn apply_config_field_agent_effort_round_trip() {
        let mut config = ErConfig::default();
        apply_config_field(
            &mut config,
            "agent.effort",
            ConfigFieldValue::String("high".into()),
        );
        assert_eq!(config.agent.effort.as_deref(), Some("high"));
    }

    #[test]
    fn validate_agent_args_requires_prompt_placeholder() {
        assert!(validate_config_text_field("agent.args", "--print").is_some());
        assert!(validate_config_text_field("agent.args", "-p {prompt}").is_none());
    }
}
