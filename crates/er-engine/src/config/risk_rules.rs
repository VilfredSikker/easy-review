//! Risk rules: the declared tables that say how closely a path needs reading.
//!
//! One rule system covers both the declared ranking and the review's per-file
//! risk: a rule either fixes a path's tier, or says `ai` and leaves the tier to
//! the review. `docs/adr/0048-risk-rules-merge-importance.md`.

use super::path_rules;
use glob::Pattern;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;

/// How closely a path needs reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskTier {
    High,
    Med,
    Low,
    /// No fixed tier: the review rates the file from what the code does. Also
    /// the answer for a path nothing claims, because a declared tier the user
    /// never wrote would read as a judgement nobody made.
    #[default]
    Ai,
}

impl RiskTier {
    /// Read a tier out of a config value.
    ///
    /// An unrecognized spelling resolves to nothing rather than failing the
    /// load: one typo in a rule table must not cost the user every other
    /// section of their config. The `[importance]` spellings are read too, so
    /// an `[importance.<repo>]` table keeps its meaning.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "high" | "foundational" => Some(Self::High),
            "med" | "medium" => Some(Self::Med),
            "low" | "isolated" => Some(Self::Low),
            "ai" | "normal" => Some(Self::Ai),
            _ => None,
        }
    }

    /// The canonical spelling, which `parse` reads back.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Med => "med",
            Self::Low => "low",
            Self::Ai => "ai",
        }
    }
}

/// One rule's value: a tier, and optionally the label a reader sees beside it.
///
/// Written as a bare tier (`"db/**" = "high"`) or as an inline table
/// (`"db/**" = { tier = "high", label = "Migrations" }`). The tier stays text
/// until resolution so an unreadable one can be dropped at its level instead of
/// failing the load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RiskRule {
    pub tier: String,
    pub label: Option<String>,
}

impl RiskRule {
    pub fn tier(&self) -> Option<RiskTier> {
        RiskTier::parse(&self.tier)
    }
}

impl From<&str> for RiskRule {
    fn from(tier: &str) -> Self {
        Self {
            tier: tier.to_string(),
            label: None,
        }
    }
}

/// Every shape a rule value can take on disk, including the wrong ones.
///
/// An unreadable shape (a typo'd `teir` key, a number, a non-text label) still
/// deserializes: it becomes a rule with no readable tier, which is listed as
/// unreadable and claims nothing. Failing here would fail the whole config
/// parse, and a save after that writes the defaults back over the user's file.
#[derive(Deserialize)]
#[serde(untagged)]
enum RiskRuleWire {
    Tier(String),
    Labelled {
        #[serde(default)]
        tier: Option<WireText>,
        #[serde(default)]
        label: Option<WireText>,
    },
    Other(serde::de::IgnoredAny),
}

#[derive(Deserialize)]
#[serde(untagged)]
enum WireText {
    Text(String),
    Other(serde::de::IgnoredAny),
}

impl WireText {
    fn into_text(self) -> Option<String> {
        match self {
            Self::Text(text) => Some(text),
            Self::Other(_) => None,
        }
    }
}

impl<'de> Deserialize<'de> for RiskRule {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match RiskRuleWire::deserialize(deserializer)? {
            RiskRuleWire::Tier(tier) => Self { tier, label: None },
            RiskRuleWire::Labelled { tier, label } => Self {
                tier: tier.and_then(WireText::into_text).unwrap_or_default(),
                label: label.and_then(WireText::into_text),
            },
            RiskRuleWire::Other(_) => Self {
                tier: String::new(),
                label: None,
            },
        })
    }
}

impl Serialize for RiskRule {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        match &self.label {
            None => serializer.serialize_str(&self.tier),
            Some(label) => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("tier", &self.tier)?;
                map.serialize_entry("label", label)?;
                map.end()
            }
        }
    }
}

/// One source's rule table: keys are paths, globs or file types.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(default)]
pub struct RiskRuleTable {
    /// Tier for a path no rule in either source claims.
    pub default: Option<String>,
    #[serde(flatten)]
    pub rules: BTreeMap<String, RiskRule>,
}

impl RiskRuleTable {
    pub fn is_empty(&self) -> bool {
        self.default.is_none() && self.rules.is_empty()
    }

    /// The rule that claims `path`: its key as written, the rule, and its tier.
    /// Precedence is [`path_rules::matching_rule`]'s.
    pub fn matching_rule(&self, path: &str) -> Option<(&str, &RiskRule, RiskTier)> {
        path_rules::matching_entry(&self.rules, path, RiskRule::tier)
    }

    /// The default as it resolves; `None` when unset or unreadable.
    pub fn default_tier(&self) -> Option<RiskTier> {
        self.default.as_deref().and_then(RiskTier::parse)
    }

    /// A table read from the `[importance.<repo>]` section.
    pub fn from_importance(legacy: &ImportanceRepoConfig) -> Self {
        Self {
            default: legacy.default.clone(),
            rules: legacy
                .rules
                .iter()
                .map(|(key, tier)| (key.clone(), RiskRule::from(tier.as_str())))
                .collect(),
        }
    }
}

/// `[risk.<repo>]`: each repo's project rules, keyed by `storage::rules_key`.
///
/// Keyed by repo rather than carried in a repo file: per-repo config was removed
/// for cause (`docs/adr/0005-global-only-config.md`).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct RiskConfig {
    #[serde(flatten)]
    pub items: BTreeMap<String, RiskRuleTable>,
}

/// The `[importance.<repo>]` section, read so a table generated
/// in the old vocabulary keeps working until something rewrites it.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ImportanceConfig {
    #[serde(flatten)]
    pub items: BTreeMap<String, ImportanceRepoConfig>,
}

impl ImportanceConfig {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// One repo's `[importance]` table; values are `foundational|normal|isolated`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ImportanceRepoConfig {
    pub default: Option<String>,
    #[serde(flatten)]
    pub rules: BTreeMap<String, String>,
}

/// Which table a rule came from. Ordered the way resolution consults them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskRuleSource {
    /// `[risk.<repo>]`: rules for this repo.
    Project,
    /// `[user_risk]`: the user's rules, applied in every repo.
    User,
}

impl RiskRuleSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::User => "user",
        }
    }
}

/// The rule that decided a path's tier, for "why is this one high?".
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchedRiskRule {
    /// The rule key as written.
    pub key: String,
    pub label: Option<String>,
    pub source: RiskRuleSource,
}

/// A path's tier and the rule behind it; `rule` is `None` when a default (or
/// nothing at all) answered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedRisk {
    pub tier: RiskTier,
    pub rule: Option<MatchedRiskRule>,
}

/// Everything one repo's paths resolve against: its project rules, then the
/// user's. What a tab carries.
#[derive(Debug, Clone, Default)]
pub struct RiskRules {
    pub project: RiskRuleTable,
    pub user: RiskRuleTable,
}

impl RiskRules {
    /// Whether anything was declared at all. A repo with nothing declared reads
    /// every path as `ai`, and a surface must not present that as a judgement.
    pub fn is_declared(&self) -> bool {
        !self.project.is_empty() || !self.user.is_empty()
    }

    /// The rule that claims `path`, and the table it came from.
    ///
    /// Project rules are searched to the end of their precedence before the
    /// user's are consulted, so a broad project glob beats a narrow user rule:
    /// the project table is the one written about this repo. Within a table the
    /// order is [`path_rules::matching_rule`]'s, and this is the only place the
    /// two tables are combined, so no caller can order them differently.
    pub fn matching_rule(&self, path: &str) -> Option<(RiskRuleSource, &str, &RiskRule, RiskTier)> {
        [
            (RiskRuleSource::Project, &self.project),
            (RiskRuleSource::User, &self.user),
        ]
        .into_iter()
        .find_map(|(source, table)| {
            table
                .matching_rule(path)
                .map(|(key, rule, tier)| (source, key, rule, tier))
        })
    }

    /// What a path no rule claims resolves to. A default is weaker than any
    /// rule, so the project's default never hides a user rule.
    pub fn default_tier(&self) -> RiskTier {
        self.project
            .default_tier()
            .or_else(|| self.user.default_tier())
            .unwrap_or_default()
    }

    /// The tier alone, without allocating: for filters run per file per
    /// keystroke.
    pub fn tier_of(&self, path: &str) -> RiskTier {
        self.matching_rule(path)
            .map_or_else(|| self.default_tier(), |(_, _, _, tier)| tier)
    }

    /// The tier and the rule that produced it.
    pub fn resolve(&self, path: &str) -> ResolvedRisk {
        match self.matching_rule(path) {
            Some((source, key, rule, tier)) => ResolvedRisk {
                tier,
                rule: Some(MatchedRiskRule {
                    key: key.to_string(),
                    label: rule.label.clone(),
                    source,
                }),
            },
            None => ResolvedRisk {
                tier: self.default_tier(),
                rule: None,
            },
        }
    }

    /// How many of `paths` each rule claims, keyed by (source, rule key).
    /// A rule that claims nothing is absent, which is how an idle rule reads.
    pub fn claim_counts<'p>(
        &self,
        paths: impl IntoIterator<Item = &'p str>,
    ) -> BTreeMap<(RiskRuleSource, String), usize> {
        let mut counts = BTreeMap::new();
        for path in paths {
            if let Some((source, key, _, _)) = self.matching_rule(path) {
                *counts.entry((source, key.to_string())).or_insert(0) += 1;
            }
        }
        counts
    }
}

/// Canonicalize a rule for writing: the tier must read and a pattern key must
/// compile. A pattern the matcher cannot compile would be listed as an active
/// rule and match nothing; exact keys are looked up rather than compiled.
fn validated(key: &str, rule: RiskRule) -> Option<RiskRule> {
    let tier = rule.tier()?;
    let key = key.trim();
    if key.is_empty() || key == "default" {
        return None;
    }
    if path_rules::is_pattern(key) && Pattern::new(key).is_err() {
        return None;
    }
    Some(RiskRule {
        tier: tier.as_str().to_string(),
        label: rule
            .label
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty()),
    })
}

/// A rule table an agent proposed, before the host merges it into the config.
///
/// The agent prints this; the host validates and writes it. Rules whose tier
/// does not read, or whose pattern does not compile, are dropped and counted
/// rather than failing the whole table — one bad line should not cost the
/// reviewer the rest of it, and dropping it silently would be worse than either.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct RiskRuleProposal {
    /// The repo the table is for. Only the one the agent was asked about is
    /// accepted; anything else is refused rather than written.
    pub repo: String,
    pub default: Option<String>,
    pub rules: BTreeMap<String, RiskRule>,
    /// What the agent measured about its own table, shown to the reviewer.
    pub report: String,
}

impl RiskRuleProposal {
    /// Pull the proposal out of an agent's reply, if it printed one.
    pub fn from_reply(reply: &str) -> Option<Self> {
        let (_, rest) = reply.split_once(crate::ai::prompts::IMPORTANCE_JSON_BEGIN)?;
        let (body, _) = rest.split_once(crate::ai::prompts::IMPORTANCE_JSON_END)?;
        serde_json::from_str(body.trim()).ok()
    }

    /// Merge a proposal into the on-disk global config and save it.
    ///
    /// The background worker that ran the agent holds no `App`, so this is the
    /// file path: read the config, replace one repo's project table, write it
    /// back with every other section as it was found. An
    /// `[importance.<repo>]` table is removed in the same write so the repo is
    /// left with one table.
    pub fn merge_into_global_config(repo: &str, proposal: Self) -> anyhow::Result<(usize, usize)> {
        if proposal.repo != repo {
            anyhow::bail!("risk rule proposal names `{}`, not `{repo}`", proposal.repo);
        }
        let mut config = super::load_global_config();
        let (table, dropped) = proposal.into_table();
        let kept = table.rules.len();
        config.risk.items.insert(repo.to_string(), table);
        config.importance.items.remove(repo);
        super::save_config(&config)?;
        Ok((kept, dropped))
    }

    /// The project table this proposes, plus how many entries were unusable.
    ///
    /// The default is treated the same way: an unreadable default is dropped
    /// rather than taking the table down with it.
    pub fn into_table(self) -> (RiskRuleTable, usize) {
        let mut dropped = 0;
        let rules = self
            .rules
            .into_iter()
            .filter_map(|(key, rule)| {
                let kept = validated(&key, rule).map(|rule| (key.trim().to_string(), rule));
                dropped += usize::from(kept.is_none());
                kept
            })
            .collect();
        let default = self.default.and_then(|d| match RiskTier::parse(&d) {
            Some(tier) => Some(tier.as_str().to_string()),
            None => {
                dropped += 1;
                None
            }
        });
        (RiskRuleTable { default, rules }, dropped)
    }
}

/// One edit to a rule, from the desktop's Settings. `tier: None` removes the
/// rule; `replaces` names the key being renamed, so a rename is one write.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RiskRuleEdit {
    pub source: RiskRuleSource,
    /// The repo's rules key; ignored for user rules.
    #[serde(default)]
    pub repo: String,
    pub pattern: String,
    #[serde(default)]
    pub tier: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub replaces: Option<String>,
}

/// Apply `edit` to `config`, or say why it was refused and change nothing.
///
/// Editing a repo that still has only a `[importance]` table first
/// carries that table over, so the edit lands beside the rules the reviewer was
/// looking at rather than replacing them.
pub fn apply_risk_rule_edit(
    config: &mut super::ErConfig,
    edit: RiskRuleEdit,
) -> Result<(), String> {
    let new_rule = match &edit.tier {
        Some(tier) => Some(
            validated(
                &edit.pattern,
                RiskRule {
                    tier: tier.clone(),
                    label: edit.label.clone(),
                },
            )
            .ok_or_else(|| {
                format!(
                    "`{}` = `{tier}` is not a usable rule: the tier must be high, med, low or ai, \
                     and a pattern must compile",
                    edit.pattern
                )
            })?,
        ),
        None => None,
    };
    let table = match edit.source {
        RiskRuleSource::User => &mut config.user_risk,
        RiskRuleSource::Project => {
            let repo = edit.repo.trim();
            if repo.is_empty() {
                return Err("a project rule needs the repo it belongs to".into());
            }
            let legacy = config.importance.items.remove(repo);
            config
                .risk
                .items
                .entry(repo.to_string())
                .or_insert_with(|| {
                    legacy
                        .as_ref()
                        .map(RiskRuleTable::from_importance)
                        .unwrap_or_default()
                })
        }
    };
    if let Some(old) = edit.replaces.as_deref() {
        table.rules.remove(old);
    }
    let key = edit.pattern.trim().to_string();
    match new_rule {
        Some(rule) => {
            table.rules.insert(key, rule);
        }
        None => {
            table.rules.remove(&key);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ErConfig;

    fn table(entries: &[(&str, &str)], default: Option<&str>) -> RiskRuleTable {
        RiskRuleTable {
            default: default.map(str::to_string),
            rules: entries
                .iter()
                .map(|(k, v)| ((*k).to_string(), RiskRule::from(*v)))
                .collect(),
        }
    }

    fn project(entries: &[(&str, &str)]) -> RiskRules {
        RiskRules {
            project: table(entries, None),
            user: RiskRuleTable::default(),
        }
    }

    #[test]
    fn tiers_read_their_own_and_the_importance_spellings() {
        for (text, tier) in [
            ("high", RiskTier::High),
            ("MED", RiskTier::Med),
            ("medium", RiskTier::Med),
            (" low ", RiskTier::Low),
            ("ai", RiskTier::Ai),
            ("foundational", RiskTier::High),
            ("normal", RiskTier::Ai),
            ("isolated", RiskTier::Low),
        ] {
            assert_eq!(RiskTier::parse(text), Some(tier), "{text}");
            assert_eq!(RiskTier::parse(tier.as_str()), Some(tier));
        }
        assert_eq!(RiskTier::parse("critical"), None);
    }

    #[test]
    fn resolution_takes_the_most_specific_rule_within_a_table() {
        // Every level answers differently, and each row is claimed by one.
        let rules = project(&[
            ("crates/er-engine/src/app/filter.rs", "med"),
            ("crates/er-engine/src/**", "high"),
            ("*.rs", "low"),
        ]);
        for (path, tier, key) in [
            (
                "crates/er-engine/src/app/filter.rs",
                RiskTier::Med,
                Some("crates/er-engine/src/app/filter.rs"),
            ),
            (
                "crates/er-engine/src/git/mod.rs",
                RiskTier::High,
                Some("crates/er-engine/src/**"),
            ),
            ("crates/er-tui/src/main.rs", RiskTier::Low, Some("*.rs")),
            ("desktop-ui/src/lib/types.ts", RiskTier::Ai, None),
        ] {
            let resolved = rules.resolve(path);
            assert_eq!(resolved.tier, tier, "{path}");
            assert_eq!(
                resolved.rule.as_ref().map(|r| r.key.as_str()),
                key,
                "{path}"
            );
            assert_eq!(
                rules.tier_of(path),
                tier,
                "tier_of agrees with resolve: {path}"
            );
        }
    }

    #[test]
    fn the_longest_of_two_matching_globs_wins() {
        let rules = project(&[("crates/**", "low"), ("crates/er-engine/**", "high")]);
        assert_eq!(rules.tier_of("crates/er-engine/src/lib.rs"), RiskTier::High);
        assert_eq!(rules.tier_of("crates/er-tui/src/main.rs"), RiskTier::Low);
    }

    /// The project table is written about this repo; the user's applies to
    /// every repo. So the project answers first, however broad its rule.
    #[test]
    fn project_rules_come_before_user_rules() {
        let rules = RiskRules {
            project: table(&[("src/**", "high")], None),
            user: table(&[("src/generated/api.rs", "low"), ("*.lock", "low")], None),
        };
        let claimed = rules.resolve("src/generated/api.rs");
        assert_eq!(claimed.tier, RiskTier::High);
        assert_eq!(claimed.rule.unwrap().source, RiskRuleSource::Project);

        let fallback = rules.resolve("Cargo.lock");
        assert_eq!(fallback.tier, RiskTier::Low);
        let rule = fallback.rule.unwrap();
        assert_eq!(
            (rule.source, rule.key.as_str()),
            (RiskRuleSource::User, "*.lock")
        );
    }

    /// A default is weaker than any rule: a project default must not hide a
    /// user rule, or setting one would silently switch off every user rule.
    #[test]
    fn a_default_answers_only_when_no_rule_in_either_table_claims_the_path() {
        let rules = RiskRules {
            project: table(&[], Some("med")),
            user: table(&[("*.md", "low")], Some("high")),
        };
        assert_eq!(rules.tier_of("README.md"), RiskTier::Low);
        assert_eq!(
            rules.tier_of("src/lib.rs"),
            RiskTier::Med,
            "project default first"
        );

        let user_default_only = RiskRules {
            project: RiskRuleTable::default(),
            user: table(&[], Some("high")),
        };
        assert_eq!(user_default_only.tier_of("src/lib.rs"), RiskTier::High);
        assert_eq!(RiskRules::default().tier_of("src/lib.rs"), RiskTier::Ai);
    }

    /// A typo in the winning rule resolves to nothing at its level, never to
    /// the next-shortest key: path_rules' fall-through, kept across tables.
    #[test]
    fn an_unreadable_tier_falls_through_without_promoting_a_broader_rule() {
        let rules = RiskRules {
            project: table(&[("src/**", "high"), ("src/a/**", "critical")], None),
            user: table(&[("*.rs", "low")], None),
        };
        // The glob level's winner does not read, so the file-type level and
        // then the user table answer.
        let resolved = rules.resolve("src/a/x.rs");
        assert_eq!(resolved.tier, RiskTier::Low);
        assert_eq!(resolved.rule.unwrap().source, RiskRuleSource::User);
    }

    #[test]
    fn a_table_reads_bare_tiers_labelled_rules_and_a_default() {
        let config: ErConfig = toml::from_str(
            r#"
            [risk.easy-review]
            default = "low"
            "db/migrations/**" = { tier = "high", label = "Migrations" }
            "*.md" = "low"

            [user_risk]
            "*.lock" = { tier = "low" }
            "#,
        )
        .unwrap();
        let rules = config.risk_rules("easy-review");
        assert_eq!(rules.project.rules.len(), 2, "`default` is not a rule");
        let resolved = rules.resolve("db/migrations/0001.sql");
        assert_eq!(resolved.tier, RiskTier::High);
        assert_eq!(resolved.rule.unwrap().label.as_deref(), Some("Migrations"));
        assert_eq!(rules.tier_of("src/lib.rs"), RiskTier::Low);
        assert_eq!(rules.tier_of("Cargo.lock"), RiskTier::Low);

        // Another repo gets only the user's rules.
        let other = config.risk_rules("other");
        assert!(other.project.is_empty());
        assert_eq!(other.tier_of("Cargo.lock"), RiskTier::Low);
        assert_eq!(other.tier_of("README.md"), RiskTier::Ai);
    }

    #[test]
    fn a_table_round_trips_through_toml_with_bare_tiers_kept_bare() {
        let mut config = ErConfig::default();
        config.risk.items.insert(
            "er".into(),
            RiskRuleTable {
                default: None,
                rules: BTreeMap::from([
                    ("*.md".to_string(), RiskRule::from("low")),
                    (
                        "db/**".to_string(),
                        RiskRule {
                            tier: "high".into(),
                            label: Some("Migrations".into()),
                        },
                    ),
                ]),
            },
        );
        let text = toml::to_string_pretty(&config).unwrap();
        assert!(text.contains(r#""*.md" = "low""#), "{text}");
        let back: ErConfig = toml::from_str(&text).unwrap();
        assert_eq!(back.risk.items["er"], config.risk.items["er"]);
    }

    /// An empty `[user_risk]` or legacy table is not written back as an empty
    /// section: the file is the user's, and noise in it is not free.
    #[test]
    fn empty_rule_sections_are_not_written() {
        let text = toml::to_string_pretty(&ErConfig::default()).unwrap();
        assert!(!text.contains("user_risk"), "{text}");
        assert!(!text.contains("importance"), "{text}");
    }

    #[test]
    fn a_pre_merge_importance_table_is_read_as_project_rules() {
        let config: ErConfig = toml::from_str(
            r#"
            [importance.easy-review]
            "crates/er-engine/src/**" = "foundational"
            "*.md" = "isolated"
            "src/odd/**" = "normal"
            default = "normal"
            "#,
        )
        .unwrap();
        let rules = config.risk_rules("easy-review");
        assert!(rules.is_declared());
        assert_eq!(rules.tier_of("crates/er-engine/src/lib.rs"), RiskTier::High);
        assert_eq!(rules.tier_of("README.md"), RiskTier::Low);
        assert_eq!(rules.tier_of("src/odd/x.rs"), RiskTier::Ai);
        assert_eq!(rules.tier_of("desktop-ui/x.ts"), RiskTier::Ai);
        assert_eq!(
            rules.resolve("src/odd/x.rs").rule.map(|r| r.key),
            Some("src/odd/**".to_string()),
            "an `ai` rule still names itself: it was declared"
        );
    }

    /// Once a repo has a `[risk]` table it is the one table; a stale
    /// `[importance]` table beside it is not consulted.
    /// A malformed rule value must not fail the parse: a failed parse loads the
    /// defaults, and the next save writes them over every other section.
    #[test]
    fn a_malformed_rule_is_unreadable_and_the_rest_of_the_config_loads() {
        let config: ErConfig = toml::from_str(
            r#"
            [features]
            view_branch = false
            [risk.er]
            "db/**" = { teir = "high" }
            "src/**" = { tier = "high", label = 7 }
            "lib/**" = 3
            "docs/**" = "low"
            "#,
        )
        .expect("a malformed rule does not fail the load");
        assert!(!config.features.view_branch, "other sections survive");

        let rules = config.risk_rules("er");
        assert_eq!(rules.project.rules["db/**"].tier(), None);
        assert_eq!(rules.project.rules["lib/**"].tier(), None);
        assert_eq!(rules.tier_of("db/0001.sql"), RiskTier::Ai);
        assert_eq!(rules.resolve("db/0001.sql").rule, None, "claims nothing");
        assert_eq!(rules.tier_of("lib/x.rs"), RiskTier::Ai);
        let src = &rules.project.rules["src/**"];
        assert_eq!(
            (src.tier(), src.label.as_deref()),
            (Some(RiskTier::High), None)
        );
        assert_eq!(rules.tier_of("docs/a.md"), RiskTier::Low);
    }

    #[test]
    fn a_risk_table_shadows_the_importance_table_for_the_same_repo() {
        let config: ErConfig = toml::from_str(
            r#"
            [importance.er]
            "*.md" = "foundational"
            [risk.er]
            "src/**" = "high"
            "#,
        )
        .unwrap();
        assert_eq!(config.risk_rules("er").tier_of("README.md"), RiskTier::Ai);
    }

    #[test]
    fn claim_counts_name_each_rule_and_skip_idle_ones() {
        let rules = RiskRules {
            project: table(&[("src/**", "high"), ("docs/**", "low")], None),
            user: table(&[("*.lock", "low")], None),
        };
        let counts = rules.claim_counts(["src/a.rs", "src/b.rs", "Cargo.lock", "README.md"]);
        assert_eq!(
            counts.get(&(RiskRuleSource::Project, "src/**".into())),
            Some(&2)
        );
        assert_eq!(
            counts.get(&(RiskRuleSource::User, "*.lock".into())),
            Some(&1)
        );
        assert_eq!(
            counts.get(&(RiskRuleSource::Project, "docs/**".into())),
            None
        );
    }

    // ── proposals ──

    fn marked(json: &str) -> String {
        format!(
            "Here is the table.\n{}\n{json}\n{}\n",
            crate::ai::prompts::IMPORTANCE_JSON_BEGIN,
            crate::ai::prompts::IMPORTANCE_JSON_END
        )
    }

    #[test]
    fn a_proposal_is_read_out_of_the_reply_with_labels() {
        let reply = marked(
            r#"{"repo":"er","default":"ai","rules":{"src/auth/**":{"tier":"high","label":"Auth"},"*.md":"low"},"report":"6 rules"}"#,
        );
        let proposal = RiskRuleProposal::from_reply(&reply).expect("proposal");
        assert_eq!(proposal.repo, "er");
        assert_eq!(proposal.report, "6 rules");
        assert_eq!(proposal.rules["src/auth/**"].label.as_deref(), Some("Auth"));
        assert_eq!(proposal.rules["*.md"].tier, "low");
    }

    #[test]
    fn a_reply_without_a_marked_table_proposes_nothing() {
        assert!(RiskRuleProposal::from_reply("I could not read the tree.").is_none());
    }

    /// One bad line must not take the rest of the table with it, and must not
    /// vanish without a word either. A pattern that cannot compile is the same
    /// kind of bad line as an unreadable tier.
    #[test]
    fn unusable_rules_are_dropped_counted_and_the_rest_canonicalized() {
        let proposal = RiskRuleProposal {
            repo: "er".into(),
            default: Some("banana".into()),
            rules: BTreeMap::from([
                ("src/**".to_string(), RiskRule::from("foundational")),
                ("docs/**".to_string(), RiskRule::from("LOW")),
                ("weird/**".to_string(), RiskRule::from("nonsense")),
                ("crates/[".to_string(), RiskRule::from("high")),
            ]),
            report: String::new(),
        };
        let (table, dropped) = proposal.into_table();
        assert_eq!(dropped, 3, "bad tier, bad pattern and bad default");
        assert_eq!(table.default, None);
        assert_eq!(
            table.rules["src/**"].tier, "high",
            "written in today's spelling"
        );
        assert_eq!(table.rules["docs/**"].tier, "low");
        assert_eq!(table.rules.len(), 2);
    }

    #[test]
    fn a_proposal_naming_another_repo_is_refused() {
        let proposal = RiskRuleProposal {
            repo: "someone-elses".into(),
            ..Default::default()
        };
        let err = RiskRuleProposal::merge_into_global_config("er", proposal).unwrap_err();
        assert!(err.to_string().contains("someone-elses"), "{err}");
    }

    /// The config also holds the reviewer's provider and display settings, so a
    /// generated table must replace one repo's rules and nothing else — and it
    /// retires that repo's `[importance]` table so one table is left.
    #[test]
    fn merging_a_proposal_replaces_only_that_repos_table() {
        let _guard = crate::storage::STORAGE_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::TempDir::new().unwrap();
        std::env::set_var("ER_STORAGE_ROOT", tmp.path());
        std::env::set_var("ER_CONFIG_PATH", tmp.path().join("config.toml"));

        let mut config = ErConfig::default();
        config.features.view_branch = false;
        config
            .risk
            .items
            .insert("another-repo".into(), table(&[("keep/**", "low")], None));
        config.importance.items.insert(
            "er".into(),
            ImportanceRepoConfig {
                default: None,
                rules: BTreeMap::from([("old/**".to_string(), "foundational".to_string())]),
            },
        );
        config.user_risk = table(&[("*.lock", "low")], None);
        crate::config::save_config(&config).unwrap();

        let proposal = RiskRuleProposal {
            repo: "er".into(),
            default: None,
            rules: BTreeMap::from([("src/**".to_string(), RiskRule::from("high"))]),
            report: "1 rule".into(),
        };
        let (kept, dropped) = RiskRuleProposal::merge_into_global_config("er", proposal).unwrap();

        let loaded = crate::config::load_global_config();
        assert_eq!((kept, dropped), (1, 0));
        assert!(!loaded.features.view_branch, "other sections survive");
        assert!(loaded.risk.items.contains_key("another-repo"));
        assert!(
            !loaded.importance.items.contains_key("er"),
            "the old table is retired"
        );
        assert_eq!(
            loaded.user_risk, config.user_risk,
            "the user's rules are untouched"
        );
        let rules = loaded.risk_rules("er");
        assert_eq!(rules.tier_of("src/lib.rs"), RiskTier::High);
        assert_eq!(rules.tier_of("old/x.rs"), RiskTier::Ai);

        std::env::remove_var("ER_STORAGE_ROOT");
        std::env::remove_var("ER_CONFIG_PATH");
    }

    // ── edits ──

    fn edit(source: RiskRuleSource, pattern: &str, tier: Option<&str>) -> RiskRuleEdit {
        RiskRuleEdit {
            source,
            repo: "er".into(),
            pattern: pattern.into(),
            tier: tier.map(str::to_string),
            label: None,
            replaces: None,
        }
    }

    #[test]
    fn an_edit_sets_renames_and_removes_a_rule() {
        let mut config = ErConfig::default();
        let mut set = edit(RiskRuleSource::Project, "db/**", Some("HIGH"));
        set.label = Some(" Migrations ".into());
        apply_risk_rule_edit(&mut config, set).unwrap();
        assert_eq!(
            config.risk.items["er"].rules["db/**"],
            RiskRule {
                tier: "high".into(),
                label: Some("Migrations".into())
            }
        );

        let mut rename = edit(RiskRuleSource::Project, "db/migrations/**", Some("high"));
        rename.replaces = Some("db/**".into());
        apply_risk_rule_edit(&mut config, rename).unwrap();
        assert_eq!(
            config.risk.items["er"].rules.keys().collect::<Vec<_>>(),
            vec!["db/migrations/**"]
        );

        apply_risk_rule_edit(
            &mut config,
            edit(RiskRuleSource::Project, "db/migrations/**", None),
        )
        .unwrap();
        assert!(config.risk.items["er"].rules.is_empty());
    }

    #[test]
    fn a_user_edit_lands_in_the_table_every_repo_reads() {
        let mut config = ErConfig::default();
        apply_risk_rule_edit(
            &mut config,
            edit(RiskRuleSource::User, "*.lock", Some("low")),
        )
        .unwrap();
        assert!(config.risk.items.is_empty());
        assert_eq!(
            config.risk_rules("anything").tier_of("Cargo.lock"),
            RiskTier::Low
        );
    }

    #[test]
    fn an_unusable_edit_is_refused_and_changes_nothing() {
        let mut config = ErConfig::default();
        for bad in [
            edit(RiskRuleSource::Project, "src/**", Some("critical")),
            edit(RiskRuleSource::Project, "src/[", Some("high")),
            edit(RiskRuleSource::Project, "default", Some("high")),
            edit(RiskRuleSource::Project, "  ", Some("high")),
            RiskRuleEdit {
                repo: String::new(),
                ..edit(RiskRuleSource::Project, "src/**", Some("high"))
            },
        ] {
            assert!(apply_risk_rule_edit(&mut config, bad).is_err());
        }
        assert!(config.risk.items.is_empty());
        assert!(config.user_risk.is_empty());
    }

    /// Editing a repo that only has a `[importance]` table carries those rules over,
    /// so the edit adds to what the reviewer saw rather than replacing it.
    #[test]
    fn editing_a_pre_merge_repo_carries_its_importance_rules_over() {
        let mut config = ErConfig::default();
        config.importance.items.insert(
            "er".into(),
            ImportanceRepoConfig {
                default: Some("isolated".into()),
                rules: BTreeMap::from([("src/**".to_string(), "foundational".to_string())]),
            },
        );
        apply_risk_rule_edit(
            &mut config,
            edit(RiskRuleSource::Project, "*.md", Some("low")),
        )
        .unwrap();
        assert!(config.importance.items.is_empty(), "one table is left");
        let rules = config.risk_rules("er");
        assert_eq!(rules.tier_of("src/lib.rs"), RiskTier::High);
        assert_eq!(rules.tier_of("README.md"), RiskTier::Low);
        assert_eq!(
            rules.tier_of("other/x"),
            RiskTier::Low,
            "the default came too"
        );
    }
}
