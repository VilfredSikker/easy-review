//! The declared risk tables and the resolution order over them.

use super::path_rules;
use schemars::JsonSchema;
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
    /// section of their config.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "high" => Some(Self::High),
            "med" | "medium" => Some(Self::Med),
            "low" => Some(Self::Low),
            "ai" => Some(Self::Ai),
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
#[derive(Debug, Clone, PartialEq, Eq, JsonSchema)]
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
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq, JsonSchema)]
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
