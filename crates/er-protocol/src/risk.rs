use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One repo's risk rules and the caller's changed paths resolved against
/// them, as Settings and the Risks tab show them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Risk {
    /// The rules key a project rule edit names.
    pub repo: String,
    /// The repo's project rules, then the user's, each with the paths it claims counted.
    pub rules: Vec<RiskRule>,
    /// What a path no rule claims resolves to.
    pub default: RiskTier,
    /// Each path's resolution; empty when nothing is declared.
    pub files: Vec<RiskFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RiskRule {
    pub pattern: String,
    /// The canonical tier, or the text as written when it does not read.
    pub tier: String,
    /// Whether `tier` reads. A rule that does not is listed but claims nothing.
    pub readable: bool,
    pub label: Option<String>,
    pub source: RiskRuleSource,
    pub file_count: usize,
}

/// One changed path, the tier it reads as, and the rule that said so.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RiskFile {
    pub path: String,
    pub tier: RiskTier,
    /// None when a default, or nothing, answered.
    pub rule: Option<MatchedRiskRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MatchedRiskRule {
    /// The rule key as written.
    pub key: String,
    pub label: Option<String>,
    pub source: RiskRuleSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum RiskTier {
    High,
    Med,
    Low,
    /// No fixed tier: the review rates the file from what the code does.
    Ai,
}

/// Which table a rule came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum RiskRuleSource {
    Project,
    User,
}
