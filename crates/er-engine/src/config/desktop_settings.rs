//! Serializable settings schema for the desktop app (excludes diff-view fields).

use super::{
    apply_risk_rule_edit, split_shell_args, ErConfig, RiskRuleEdit, MAX_CONCURRENT_REVIEWS_RANGE,
};
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

/// The patch key that edits one risk rule. Its value is a [`RiskRuleEdit`] as
/// JSON text: a rule is four fields, and the patch wire carries one scalar.
pub const RISK_RULE_PATCH_KEY: &str = "risk.rule";

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
    if key == RISK_RULE_PATCH_KEY {
        // An edit that does not parse or validate changes nothing, like any
        // other out-of-range patch value: Settings validates before it sends.
        if let ConfigFieldValue::String(json) = value {
            if let Ok(edit) = serde_json::from_str::<RiskRuleEdit>(&json) {
                let _ = apply_risk_rule_edit(config, edit);
            }
        }
        return false;
    }
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
        "agent.model" => {
            if let ConfigFieldValue::String(v) = value {
                config.agent.model = v.trim().to_string();
            }
        }
        "summary.command" => {
            if let ConfigFieldValue::String(v) = value {
                let v = v.trim();
                config.summary.command = (!v.is_empty()).then(|| v.to_string());
            }
        }
        "summary.args" => {
            if let ConfigFieldValue::String(v) = value {
                let args = split_shell_args(&v);
                config.summary.args = (!args.is_empty()).then_some(args);
            }
        }
        "features.view_tour" => {
            if let ConfigFieldValue::Bool(v) = value {
                config.features.view_tour = v;
            }
        }
        "ai_hub.flow_providers" => {
            if let ConfigFieldValue::String(v) = value {
                let mut ids: Vec<String> = Vec::new();
                for id in v.split(|c: char| c.is_whitespace() || c == ',') {
                    if !id.is_empty() && !ids.iter().any(|seen| seen == id) {
                        ids.push(id.to_string());
                    }
                }
                config.ai_hub.flow_providers = ids;
            }
        }
        "ai_hub.agent_timeout_secs" => {
            let parsed = match value {
                ConfigFieldValue::Number(n) => Some(n),
                ConfigFieldValue::String(v) => v.trim().parse::<u64>().ok(),
                ConfigFieldValue::Bool(_) => None,
            };
            if let Some(secs) = parsed {
                config.ai_hub.agent_timeout_secs = secs;
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
    use crate::config::risk_settings::risk_file_rows;
    use crate::config::test_support::risk_rules;
    use crate::config::ErConfig;
    use crate::config::{risk_rules_view, RiskRuleSource, RiskRules, RiskTier};

    /// A hub row's setter and the patch its key names change a config the
    /// same way, so the TUI's hub writes through the settings actor with the
    /// one field writer (ADR 0069).
    #[test]
    fn every_hub_row_key_patches_as_its_setter_sets() {
        use crate::config::{config_hub_items, ConfigItem};

        let mut base = ErConfig::default();
        base.watched.paths = vec!["docs/**".into()];
        let mut keyed = 0;
        for item in config_hub_items(&base) {
            let row = format!("{item:?}");
            let (mut by_set, mut by_key) = (base.clone(), base.clone());
            let (key, value) = match item {
                ConfigItem::BoolToggle { key, get, set, .. } => {
                    set(&mut by_set, !get(&base));
                    (key, ConfigFieldValue::Bool(!get(&base)))
                }
                ConfigItem::StringCycle {
                    key, options, set, ..
                } => {
                    let last = options[options.len() - 1].to_string();
                    set(&mut by_set, last.clone());
                    (key, ConfigFieldValue::String(last))
                }
                ConfigItem::StringEdit { key, set, .. } => {
                    set(&mut by_set, "run --fast {prompt}".into());
                    (key, ConfigFieldValue::String("run --fast {prompt}".into()))
                }
                ConfigItem::NumberEdit { key, set, max, .. } => {
                    set(&mut by_set, max);
                    (key, ConfigFieldValue::Number(u64::from(max)))
                }
                ConfigItem::ListAdd { key, .. } => {
                    by_set.watched.paths.push("src/**".into());
                    (key, ConfigFieldValue::String("src/**".into()))
                }
                ConfigItem::ListEntry { key, index, .. } => {
                    by_set.watched.paths.remove(index);
                    (key, ConfigFieldValue::Number(index as u64))
                }
                _ => continue,
            };
            keyed += 1;
            apply_config_field(&mut by_key, key, value);
            assert_ne!(
                format!("{by_set:?}"),
                format!("{base:?}"),
                "{row} set nothing"
            );
            assert_eq!(format!("{by_set:?}"), format!("{by_key:?}"), "{row}: {key}");
        }
        assert_eq!(keyed, 28, "a keyed row was added or lost");
    }

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

        assert!(!general.iter().any(|k| k == "agent.effort"));
        assert!(!app.iter().any(|k| k == "agent.effort"));
        assert!(!terminal.iter().any(|k| k == "agent.effort"));
        assert!(general.iter().any(|k| k == "agent.args"));

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
    fn validate_agent_args_requires_prompt_placeholder() {
        assert!(validate_config_text_field("agent.args", "--print").is_some());
        assert!(validate_config_text_field("agent.args", "-p {prompt}").is_none());
    }

    /// A remote-only tab's `repo_root` is the process's working directory, so
    /// the view lists the table under the rules key it is given, never one
    /// derived from a directory.
    #[test]
    fn settings_read_risk_rules_under_the_tabs_key() {
        let mut config = ErConfig::default();
        config.risk.items.insert(
            "my-service".to_string(),
            risk_rules(&[("src/**", "high")], None).project,
        );
        let view = risk_rules_view(&config, "my-service", None);
        assert_eq!(view.rules.len(), 1);
        assert_eq!(view.repo, "my-service");
    }

    /// Settings shows the committed artifact's rules, and a config entry for
    /// the same pattern wins over the artifact's.
    #[test]
    fn settings_merge_the_artifact_under_the_users_config() {
        let mut config = ErConfig::default();
        config.risk.items.insert(
            "er".to_string(),
            risk_rules(&[("src/**", "low")], None).project,
        );
        let artifact = risk_rules(&[("src/**", "high"), ("db/**", "med")], None).project;
        let view = risk_rules_view(&config, "er", Some(artifact));

        let row = |p: &str| view.rules.iter().find(|r| r.pattern == p).unwrap();
        assert_eq!(row("src/**").tier, "low", "the user's config wins");
        assert_eq!(row("db/**").tier, "med", "the artifact fills the gap");
        assert_eq!(view.rules.len(), 2);
    }

    #[test]
    fn risk_file_rows_report_the_rule_that_claimed_each_path() {
        let rules = risk_rules(
            &[
                ("crates/er-engine/src/app/filter.rs", "med"),
                ("crates/er-engine/src/**", "high"),
                ("*.rs", "low"),
            ],
            None,
        );
        let changed = vec![
            "crates/er-engine/src/app/filter.rs".to_string(),
            "crates/er-engine/src/git/mod.rs".to_string(),
            "desktop-ui/src/lib/types.ts".to_string(),
        ];

        let rows = risk_file_rows(&rules, &changed);

        assert_eq!(rows.len(), changed.len());
        let key = |i: usize| rows[i].resolved.rule.as_ref().map(|r| r.key.as_str());
        assert_eq!(key(0), Some("crates/er-engine/src/app/filter.rs"));
        assert_eq!(rows[0].resolved.tier, RiskTier::Med);
        assert_eq!(key(1), Some("crates/er-engine/src/**"));
        assert_eq!(rows[1].resolved.tier, RiskTier::High);
        // Nothing claimed this one, so no key is named and the default answers.
        assert_eq!(key(2), None);
        assert_eq!(rows[2].resolved.tier, RiskTier::Ai);
    }

    #[test]
    fn a_repo_without_rules_resolves_no_files() {
        // The card reads "no rules declared for this repo" in this case, so an
        // empty list is the honest payload — a row per file would say the
        // resolution meant something when nothing was ever declared.
        let rows = risk_file_rows(&RiskRules::default(), &["src/main.rs".to_string()]);
        assert!(rows.is_empty());
    }

    #[test]
    fn rule_rows_count_claimed_files_and_keep_an_unreadable_tier_visible() {
        let mut config = ErConfig::default();
        config.risk.items.insert(
            "er".into(),
            risk_rules(&[("src/**", "high"), ("docs/**", "critical")], None).project,
        );
        config.user_risk = risk_rules(&[("*.lock", "low")], None).project;
        let changed = ["src/a.rs", "src/b.rs", "Cargo.lock"].map(str::to_string);
        let resolved = risk_rules_view(&config, "er", None).resolve(&changed);

        let row = |p: &str| resolved.rules.iter().find(|r| r.pattern == p).unwrap();
        assert_eq!(
            (row("src/**").file_count, row("src/**").source),
            (2, RiskRuleSource::Project)
        );
        assert_eq!(
            (row("*.lock").file_count, row("*.lock").source),
            (1, RiskRuleSource::User)
        );
        assert_eq!(row("docs/**").tier, "critical");
        assert!(!row("docs/**").readable);
        assert_eq!(resolved.files.len(), 3);
    }

    #[test]
    fn the_risk_rule_patch_key_edits_and_refuses_without_side_effects() {
        let mut config = ErConfig::default();
        let set = r#"{"source":"project","repo":"er","pattern":"db/**","tier":"high","label":"Migrations"}"#;
        assert!(!apply_config_field(
            &mut config,
            RISK_RULE_PATCH_KEY,
            ConfigFieldValue::String(set.into())
        ));
        let resolved = config.risk_rules("er").resolve("db/0001.sql");
        assert_eq!(resolved.tier, RiskTier::High);
        assert_eq!(resolved.rule.unwrap().label.as_deref(), Some("Migrations"));

        let before = config.risk.items.clone();
        for bad in [
            r#"{"source":"project","repo":"er","pattern":"db/**","tier":"critical"}"#,
            "not json",
        ] {
            apply_config_field(
                &mut config,
                RISK_RULE_PATCH_KEY,
                ConfigFieldValue::String(bad.into()),
            );
        }
        assert_eq!(config.risk.items, before);

        let remove = r#"{"source":"project","repo":"er","pattern":"db/**"}"#;
        apply_config_field(
            &mut config,
            RISK_RULE_PATCH_KEY,
            ConfigFieldValue::String(remove.into()),
        );
        assert!(config.risk.items["er"].rules.is_empty());
    }
}
