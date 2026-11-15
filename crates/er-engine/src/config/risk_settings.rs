//! What Settings and the Risks tab show of one repo's risk rules.
//!
//! The view is built without a diff: it lists the declared rules and the
//! default, and keeps the merged resolver so a caller holding changed paths
//! resolves them itself (`RiskRulesView::resolve`). The flag-off desktop and
//! the settings actor both build it here.

use serde::Serialize;

use super::{ErConfig, ResolvedRisk, RiskRuleSource, RiskRuleTable, RiskRules, RiskTier};

/// One declared risk rule, for the Settings rule table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RiskRuleDto {
    pub pattern: String,
    /// The canonical tier, or the text as written when it does not read — the
    /// table must show a typo rather than hide the rule it broke.
    pub tier: String,
    /// Whether `tier` reads. A rule that does not is listed but claims nothing.
    pub readable: bool,
    pub label: Option<String>,
    pub source: RiskRuleSource,
    /// How many of the caller's changed files this rule claims. Zero is an
    /// idle rule, which the table dims.
    pub file_count: usize,
}

/// One changed file's resolution: the tier it reads as, and the rule that said
/// so. The rules list shows what was declared; this shows what a given file
/// resolved to, which precedence and the defaults sit between.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RiskFileDto {
    pub path: String,
    #[serde(flatten)]
    pub resolved: ResolvedRisk,
}

/// One repo's declared rules, as the rule table shows them.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RiskRulesView {
    /// The rules key (`storage::rules_key`) a project rule edit names.
    pub repo: String,
    /// Raised by the settings actor each time these rules change.
    pub version: u64,
    /// The repo's project rules, then the user's, with no files counted.
    pub rules: Vec<RiskRuleDto>,
    /// What a path no rule claims resolves to.
    pub default: RiskTier,
    #[serde(skip)]
    resolver: RiskRules,
}

/// The caller's changed paths, resolved against one [`RiskRulesView`].
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RiskFilesView {
    /// The view's rules, with `file_count` counted over the paths.
    pub rules: Vec<RiskRuleDto>,
    /// Each path's resolution. Empty when nothing is declared: there is
    /// nothing to explain about a path that reads as `ai` because nobody wrote
    /// a rule.
    pub files: Vec<RiskFileDto>,
}

impl RiskRulesView {
    /// Resolves the caller's paths. No IO: the resolver travels with the view.
    pub fn resolve(&self, paths: &[String]) -> RiskFilesView {
        RiskFilesView {
            rules: risk_rule_rows(&self.resolver, paths),
            files: risk_file_rows(&self.resolver, paths),
        }
    }

    /// Whether `other` shows the same rules, so a rebuild that changed nothing
    /// keeps its version.
    pub fn same_rules(&self, other: &Self) -> bool {
        self.repo == other.repo && self.rules == other.rules && self.default == other.default
    }
}

/// `repo_key` is `storage::rules_key` for the repo, so the view shows the rules
/// the repo's filter and header resolve against. `artifact` is the repo's
/// committed rules table, which the user's config layers over.
pub fn risk_rules_view(
    config: &ErConfig,
    repo_key: &str,
    artifact: Option<RiskRuleTable>,
) -> RiskRulesView {
    let resolver = config.risk_rules_merged(repo_key, artifact);
    RiskRulesView {
        repo: repo_key.to_string(),
        version: 0,
        rules: risk_rule_rows(&resolver, &[]),
        // The default as it resolves, not as it was written: the view has to
        // say what the resolver will do rather than repeat a typo back.
        default: resolver.default_tier(),
        resolver,
    }
}

/// Resolve the changed paths against `rules`.
pub(super) fn risk_file_rows(rules: &RiskRules, changed_paths: &[String]) -> Vec<RiskFileDto> {
    if !rules.is_declared() {
        return Vec::new();
    }
    changed_paths
        .iter()
        .map(|path| RiskFileDto {
            path: path.clone(),
            resolved: rules.resolve(path),
        })
        .collect()
}

pub(super) fn risk_rule_rows(rules: &RiskRules, changed_paths: &[String]) -> Vec<RiskRuleDto> {
    let counts = rules.claim_counts(changed_paths.iter().map(String::as_str));
    [
        (RiskRuleSource::Project, &rules.project),
        (RiskRuleSource::User, &rules.user),
    ]
    .into_iter()
    .flat_map(|(source, table)| {
        let counts = &counts;
        table.rules.iter().map(move |(pattern, rule)| {
            let tier = rule.tier();
            RiskRuleDto {
                pattern: pattern.clone(),
                tier: tier.map_or_else(|| rule.tier.clone(), |t| t.as_str().to_string()),
                readable: tier.is_some(),
                label: rule.label.clone(),
                source,
                file_count: counts.get(&(source, pattern.clone())).copied().unwrap_or(0),
            }
        })
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::test_support::risk_rules;

    /// The caller's paths are what get resolved and counted: the view itself
    /// has no diff, so a count from anywhere else would dim every rule.
    #[test]
    fn risk_files_answer_for_the_query_paths() {
        let mut config = ErConfig::default();
        config.risk.items.insert(
            "er".into(),
            risk_rules(&[("src/**", "high")], Some("low")).project,
        );
        let view = risk_rules_view(&config, "er", None);
        assert_eq!(view.rules[0].file_count, 0, "the view counts no files");

        let paths = ["src/a.rs", "docs/b.md"].map(str::to_string);
        let resolved = view.resolve(&paths);

        let tiers: Vec<(&str, RiskTier)> = resolved
            .files
            .iter()
            .map(|f| (f.path.as_str(), f.resolved.tier))
            .collect();
        assert_eq!(
            tiers,
            [("src/a.rs", RiskTier::High), ("docs/b.md", RiskTier::Low)]
        );
        assert!(
            resolved.files[1].resolved.rule.is_none(),
            "the default answers an unclaimed path"
        );
        assert_eq!(resolved.rules[0].file_count, 1);
    }

    #[test]
    fn a_rebuild_with_the_same_rules_is_the_same_view() {
        let mut config = ErConfig::default();
        config
            .risk
            .items
            .insert("er".into(), risk_rules(&[("src/**", "high")], None).project);
        let before = risk_rules_view(&config, "er", None);
        assert!(before.same_rules(&risk_rules_view(&config, "er", None)));
        config.user_risk = risk_rules(&[("*.lock", "low")], None).project;
        assert!(!before.same_rules(&risk_rules_view(&config, "er", None)));
    }
}
