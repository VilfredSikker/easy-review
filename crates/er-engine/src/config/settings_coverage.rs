//! The settings table is the schema (ADR 0068): every config leaf is either a
//! row in it, under a bespoke editor's root, or named here as non-form.
//!
//! The fixtures name every field of every config struct without
//! `..Default::default()`, so a new field does not compile here until its
//! author sets it, and once set its leaf fails the partition test until it is
//! classified.

use std::collections::{BTreeMap, BTreeSet};

use super::desktop_settings::ConfigHubFieldDto;
use super::settings::settings_fields_grouped;
use super::{
    AgentConfig, AgentsConfig, AiHubConfig, AiModelConfig, AiProviderConfig, CommandsConfig,
    DisplayConfig, ErConfig, FeatureFlags, FileKindRepoConfig, FileKindsConfig, HintConfig,
    InboxConfig, InboxKindToggles, PackageConfig, PackagesConfig, ReviewModelGroups,
    ReviewModelRef, RiskConfig, RiskRule, RiskRuleTable, SummaryConfig, WatchedConfig,
};

/// Roots a bespoke editor owns: the provider and model editor, the risk rule
/// table, and the project settings. A leaf under one is not a form row.
const BESPOKE_ROOTS: &[&str] = &[
    "ai_hub.providers",
    "ai_hub.review_models",
    "ai_hub.default_provider",
    "ai_hub.default_model",
    "ai_hub.default_effort",
    "risk",
    "user_risk",
    "packages",
];

/// Leaves no surface edits.
const NON_FORM: &[&str] = &[
    // Discovery tombstones: written when a catalog provider is deleted.
    "ai_hub.removed_catalog_providers",
    // The arena is always on (c54a18bdd); the flag is still read, never shown.
    "features.arena",
    // Per-repo file kinds are edited in config.toml; no settings editor.
    "file_kinds",
];

fn rule(tier: &str, label: &str) -> RiskRule {
    RiskRule {
        tier: tier.to_string(),
        label: Some(label.to_string()),
    }
}

fn table() -> RiskRuleTable {
    RiskRuleTable {
        default: Some("low".to_string()),
        rules: BTreeMap::from([("src/**".to_string(), rule("high", "Source"))]),
    }
}

pub(super) fn full_agent() -> AgentConfig {
    AgentConfig {
        command: "fixture-agent".to_string(),
        args: vec!["-p".to_string(), "{prompt}".to_string()],
        model: "fixture-model".to_string(),
    }
}

pub(super) fn full_ai_hub() -> AiHubConfig {
    // Not a catalog id: `persisted_config` drops unedited catalog presets,
    // which would take this provider's leaves out of the saved file.
    let model = AiModelConfig {
        id: "fixture-model".to_string(),
        label: Some("Fixture".to_string()),
        description: Some("A model for the fixture".to_string()),
        args: vec!["--model".to_string(), "fixture-model".to_string()],
        cost_per_1k_in: Some(0.5),
        cost_per_1k_out: Some(1.5),
        avg_latency_ms: Some(900),
        effort_levels: vec!["high".to_string()],
        // Runtime-only; `#[serde(skip)]`, so never a leaf.
        discovered: false,
        edited: true,
    };
    let provider = AiProviderConfig {
        label: Some("Fixture CLI".to_string()),
        command: "fixture-cli".to_string(),
        args: vec!["{prompt}".to_string()],
        family: Some("claude".to_string()),
        models_command: vec!["fixture-cli".to_string(), "--list".to_string()],
        models: vec![model],
        removed_catalog_models: vec!["retired".to_string()],
    };
    let model_ref = || {
        vec![ReviewModelRef {
            provider_id: "fixture-cli".to_string(),
            model_id: "fixture-model".to_string(),
        }]
    };
    AiHubConfig {
        flow_providers: vec!["fixture-cli".to_string()],
        review_models: ReviewModelGroups {
            fast: model_ref(),
            standard: model_ref(),
            advanced: model_ref(),
        },
        default_provider: Some("fixture-cli".to_string()),
        default_model: Some("fixture-model".to_string()),
        default_effort: Some("high".to_string()),
        providers: BTreeMap::from([("fixture-cli".to_string(), provider)]),
        removed_catalog_providers: vec!["retired-cli".to_string()],
        max_concurrent_reviews: 3,
        agent_timeout_secs: 120,
        max_concurrent_arena_reviews: 2,
        max_concurrent_agents: 4,
    }
}

pub(super) fn full_agents() -> AgentsConfig {
    AgentsConfig {
        agent: full_agent(),
        ai_hub: full_ai_hub(),
    }
}

pub(super) fn full_config() -> ErConfig {
    ErConfig {
        features: FeatureFlags {
            view_branch: true,
            view_unstaged: true,
            view_staged: true,
            view_history: true,
            view_conflicts: true,
            view_hidden: true,
            view_tour: true,
            arena: true,
            model_discovery: true,
        },
        agent: full_agent(),
        summary: SummaryConfig {
            command: Some("fixture-summary".to_string()),
            args: Some(vec!["{diff}".to_string()]),
            push_to_pr: true,
        },
        display: DisplayConfig {
            tab_width: 4,
            line_numbers: true,
            wrap_lines: true,
            split_diff: true,
            auto_context_threshold: 1,
            theme: "paper".to_string(),
        },
        watched: WatchedConfig {
            paths: vec!["notes/*.md".to_string()],
            diff_mode: "snapshot".to_string(),
        },
        hints: HintConfig {
            navigation: true,
            comments: true,
            staging: true,
            verbose: true,
        },
        commands: CommandsConfig {
            test: Some("cargo test".to_string()),
            lint: Some("cargo clippy".to_string()),
            typecheck: Some("tsc --noEmit".to_string()),
            security: Some("cargo audit".to_string()),
        },
        ai_hub: full_ai_hub(),
        packages: PackagesConfig {
            items: BTreeMap::from([(
                "web".to_string(),
                PackageConfig {
                    label: Some("Web".to_string()),
                    test: Some("npm test".to_string()),
                    lint: Some("npm run lint".to_string()),
                    typecheck: Some("npm run check".to_string()),
                    security: Some("npm audit".to_string()),
                },
            )]),
        },
        risk: RiskConfig {
            items: BTreeMap::from([("er".to_string(), table())]),
        },
        // Skipped when empty, so it must hold a rule to show its leaves.
        user_risk: table(),
        file_kinds: FileKindsConfig {
            items: BTreeMap::from([(
                "er".to_string(),
                FileKindRepoConfig::new(BTreeMap::from([(
                    "*.snap".to_string(),
                    "generated".to_string(),
                )])),
            )]),
        },
        // The toggles and their rows come from one macro list, so a new kind
        // gets its row by construction; every toggle serializes as a leaf.
        inbox: InboxConfig {
            show: InboxKindToggles::default(),
            notify: InboxKindToggles::default(),
        },
    }
}

/// Dotted leaves of a serialized value. Lists collapse to their root
/// (`watched.paths.0` is `watched.paths`); map keys stay.
fn leaves(value: &toml::Value) -> BTreeSet<String> {
    fn walk(prefix: &str, value: &toml::Value, out: &mut BTreeSet<String>) {
        match value {
            toml::Value::Table(table) if !table.is_empty() => {
                for (key, child) in table {
                    let path = if prefix.is_empty() {
                        key.clone()
                    } else {
                        format!("{prefix}.{key}")
                    };
                    walk(&path, child, out);
                }
            }
            _ => {
                out.insert(prefix.to_string());
            }
        }
    }
    let mut out = BTreeSet::new();
    walk("", value, &mut out);
    out
}

fn config_leaves(config: &ErConfig) -> BTreeSet<String> {
    leaves(&toml::Value::try_from(config).expect("the fixture serializes"))
}

/// The leaf a row edits. Most rows are keyed by their leaf; these are not.
fn row_leaf(key: &str) -> String {
    if key == "display.auto_context" {
        return "display.auto_context_threshold".to_string();
    }
    if key.starts_with("watched.paths.") {
        return "watched.paths".to_string();
    }
    key.to_string()
}

fn row_keys(config: &ErConfig) -> BTreeSet<String> {
    let grouped = settings_fields_grouped(config);
    grouped
        .general
        .iter()
        .chain(&grouped.app)
        .chain(&grouped.terminal)
        .filter_map(|field| match field {
            ConfigHubFieldDto::Bool { key, .. }
            | ConfigHubFieldDto::Cycle { key, .. }
            | ConfigHubFieldDto::Text { key, .. }
            | ConfigHubFieldDto::ListEntry { key, .. }
            | ConfigHubFieldDto::ListAdd { key, .. } => Some(row_leaf(key)),
            ConfigHubFieldDto::Section { .. } => None,
        })
        .collect()
}

fn under(leaf: &str, root: &str) -> bool {
    leaf == root || leaf.starts_with(&format!("{root}."))
}

#[test]
fn the_schema_covers_every_config_leaf() {
    let config = full_config();
    let leaves = config_leaves(&config);
    let rows = row_keys(&config);

    let mut unclaimed = Vec::new();
    let mut doubly_claimed = Vec::new();
    for leaf in &leaves {
        let claims = usize::from(rows.contains(leaf))
            + BESPOKE_ROOTS.iter().filter(|r| under(leaf, r)).count()
            + NON_FORM.iter().filter(|r| under(leaf, r)).count();
        match claims {
            0 => unclaimed.push(leaf.clone()),
            1 => {}
            _ => doubly_claimed.push(leaf.clone()),
        }
    }
    assert!(
        unclaimed.is_empty(),
        "config leaves with no row, bespoke editor or non-form entry: {unclaimed:?}"
    );
    assert!(
        doubly_claimed.is_empty(),
        "config leaves claimed twice: {doubly_claimed:?}"
    );

    let dangling: Vec<&String> = rows.iter().filter(|key| !leaves.contains(*key)).collect();
    assert!(dangling.is_empty(), "rows that edit no leaf: {dangling:?}");
    for root in BESPOKE_ROOTS.iter().chain(NON_FORM) {
        assert!(
            leaves.iter().any(|leaf| under(leaf, root)),
            "{root} names no leaf of the fixture"
        );
    }
}

#[test]
fn the_split_puts_each_leaf_in_its_file() {
    let dir = tempfile::tempdir().expect("temp dir");
    let config_path = dir.path().join("config.toml");
    let config = full_config();
    super::save_config_pair_to(&config_path, &config).expect("save the pair");

    let read = |path: &std::path::Path| -> BTreeSet<String> {
        let text = std::fs::read_to_string(path).expect("read a saved file");
        leaves(&toml::from_str::<toml::Value>(&text).expect("a saved file parses"))
    };
    let in_config = read(&config_path);
    let in_agents = read(&super::agents_path(&config_path));

    let expected_agents = leaves(&toml::Value::try_from(full_agents()).expect("serialize"));
    let expected_config: BTreeSet<String> = config_leaves(&config)
        .into_iter()
        .filter(|leaf| !under(leaf, "agent") && !under(leaf, "ai_hub"))
        .collect();

    assert_eq!(
        in_agents, expected_agents,
        "agents.toml holds [agent] and [ai_hub]"
    );
    assert_eq!(
        in_config, expected_config,
        "config.toml holds everything else"
    );
}

/// A row with no `apply_config_field` arm renders a control that writes
/// nothing. Each scalar row's value must change when its control is used.
#[test]
fn every_row_has_a_setter() {
    use super::desktop_settings::{apply_config_field, ConfigFieldValue};

    fn row_value(config: &ErConfig, want: &str) -> Option<String> {
        let grouped = settings_fields_grouped(config);
        grouped
            .general
            .into_iter()
            .chain(grouped.terminal)
            .find_map(|field| match field {
                ConfigHubFieldDto::Bool { key, value, .. } if key == want => {
                    Some(value.to_string())
                }
                ConfigHubFieldDto::Cycle { key, value, .. }
                | ConfigHubFieldDto::Text { key, value, .. }
                    if key == want =>
                {
                    Some(value)
                }
                _ => None,
            })
    }

    let base = ErConfig::default();
    let grouped = settings_fields_grouped(&base);
    for field in grouped.general.iter().chain(&grouped.terminal) {
        let (key, next) = match field {
            ConfigHubFieldDto::Bool { key, value, .. } => (key, ConfigFieldValue::Bool(!value)),
            ConfigHubFieldDto::Cycle {
                key,
                options,
                value,
                ..
            } => {
                let other = options.iter().find(|o| *o != value).expect("two options");
                (key, ConfigFieldValue::String(other.clone()))
            }
            // Numeric text rows read a number; the others read any text.
            ConfigHubFieldDto::Text { key, .. } => (key, ConfigFieldValue::String("7".into())),
            _ => continue,
        };
        let mut config = base.clone();
        let before = row_value(&config, key);
        apply_config_field(&mut config, key, next);
        assert_ne!(row_value(&config, key), before, "{key} has no setter");
    }
}
