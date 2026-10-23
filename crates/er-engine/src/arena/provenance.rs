//! Resolve arena reviewer identities to the lens and provider that raised a finding.

use super::model::{ArenaFinding, Reviewer};
use crate::ai::{expert_by_id, lens_for_producer_id, GENERAL_LENS};
use std::collections::BTreeSet;

/// Lens names accepted from a reviewer task kind.
fn lens_for_kind(kind: &str) -> Option<&'static str> {
    match kind {
        "general" => Some(GENERAL_LENS),
        "professor" => Some(lens_for_producer_id(kind)),
        value => {
            let expert_id = value.strip_prefix("expert:")?;
            expert_by_id(expert_id).map(|_| lens_for_producer_id(kind))
        }
    }
}

/// Normalize the stored raisers without losing reviewer metadata.
///
/// Deep findings store reviewer IDs, while seeded findings store lens IDs.
/// The optional run-wide kind supports older imported rows whose reviewer
/// records were not persisted. Unknown raisers are ignored when reviewer
/// metadata exists so they cannot be attributed to a guessed lens.
pub fn normalized_raisers_for(
    finding: &ArenaFinding,
    reviewers: &[Reviewer],
    fallback_kind: Option<&str>,
) -> Vec<String> {
    let mut lenses = BTreeSet::new();
    if finding.raised_by.is_empty() {
        if let Some(lens) = fallback_kind.and_then(lens_for_kind) {
            lenses.insert(lens.to_string());
        }
        return lenses.into_iter().collect();
    }
    for raiser in &finding.raised_by {
        if let Some(reviewer) = reviewers.iter().find(|reviewer| reviewer.id == *raiser) {
            let lens = match reviewer.agent_kind.as_deref() {
                Some(kind) => lens_for_kind(kind),
                None => fallback_kind.and_then(lens_for_kind),
            };
            if let Some(lens) = lens {
                lenses.insert(lens.to_string());
            }
            continue;
        }

        if let Some(lens) = known_lens(raiser) {
            lenses.insert(lens.to_string());
        } else if reviewers.is_empty() {
            if let Some(lens) = fallback_kind.and_then(lens_for_kind) {
                lenses.insert(lens.to_string());
            }
        }
    }
    lenses.into_iter().collect()
}

fn known_lens(value: &str) -> Option<&'static str> {
    lens_for_kind(value).or_else(|| expert_by_id(value).map(|expert| expert.id))
}

/// Return distinct provider IDs for the reviewers that raised this finding.
///
/// Exact reviewer IDs are used for Deep runs. Seeded runs store bare lens IDs,
/// which are matched against normalized reviewer task kinds. Arbiter
/// endorsement is not another independent raiser.
pub fn providers_for(finding: &ArenaFinding, reviewers: &[Reviewer]) -> Vec<String> {
    let mut providers = BTreeSet::new();
    for raiser in &finding.raised_by {
        if let Some(reviewer) = reviewers.iter().find(|reviewer| reviewer.id == *raiser) {
            if !reviewer.provider_id.is_empty() {
                providers.insert(reviewer.provider_id.clone());
            }
            continue;
        }

        let Some(lens) = known_lens(raiser) else {
            continue;
        };
        for reviewer in reviewers {
            let Some(kind) = reviewer.agent_kind.as_deref() else {
                continue;
            };
            if lens_for_kind(kind) == Some(lens) && !reviewer.provider_id.is_empty() {
                providers.insert(reviewer.provider_id.clone());
            }
        }
    }
    providers.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::RiskLevel;
    use crate::arena::model::{Ballot, RoundLog, Verdict, Vote};
    use std::collections::BTreeMap;

    fn finding(raised_by: &[&str]) -> ArenaFinding {
        ArenaFinding {
            id: "finding".into(),
            file: "src/a.rs".into(),
            line: Some(3),
            title: "claim".into(),
            body: "body".into(),
            severity_by_round: BTreeMap::from([(1, RiskLevel::Medium)]),
            raised_by: raised_by.iter().map(|value| (*value).into()).collect(),
            verdict: Verdict::Pending,
            confidence: 0.6,
            rationale: String::new(),
            rounds: vec![RoundLog {
                n: 1,
                log: vec![Ballot {
                    reviewer: "reviewer".into(),
                    vote: Vote::Propose,
                    note: String::new(),
                    merge_target: None,
                }],
            }],
            merge_candidates: Vec::new(),
            merged_children: Vec::new(),
            evidence: Vec::new(),
            override_: None,
            accepted_at: None,
        }
    }

    fn reviewer(id: &str, provider: &str, kind: Option<&str>) -> Reviewer {
        Reviewer {
            id: id.into(),
            name: id.into(),
            kind: crate::arena::ReviewerKind::Agent,
            provider_id: provider.into(),
            model_id: "model-with-hyphens".into(),
            system_prompt: String::new(),
            color: String::new(),
            icon: String::new(),
            tagline: String::new(),
            cost_per_1k_in: 0.0,
            cost_per_1k_out: 0.0,
            avg_latency_ms: 0,
            status: crate::arena::ReviewerRunStatus::Ok,
            agent_kind: kind.map(str::to_string),
        }
    }

    #[test]
    fn deep_reviewer_ids_resolve_exactly_and_unknown_ids_are_ignored() {
        let reviewers = vec![
            reviewer(
                "expert:security::alpha-model-a",
                "alpha",
                Some("expert:security"),
            ),
            reviewer("expert:general::beta-model-b", "beta", Some("general")),
        ];
        let finding = finding(&[
            "expert:security::alpha-model-a",
            "expert:general::beta-model-b",
            "unknown-with-hyphens",
        ]);

        assert_eq!(
            normalized_raisers_for(&finding, &reviewers, None),
            vec!["general", "security"]
        );
        assert_eq!(providers_for(&finding, &reviewers), vec!["alpha", "beta"]);
    }

    #[test]
    fn exact_reviewer_id_precedes_seeded_lens_fallback() {
        let reviewers = vec![
            reviewer("security", "exact-provider", Some("general")),
            reviewer(
                "security-reviewer",
                "security-provider",
                Some("expert:security"),
            ),
        ];
        let finding = finding(&["security"]);

        assert_eq!(
            normalized_raisers_for(&finding, &reviewers, None),
            vec!["general"]
        );
        assert_eq!(providers_for(&finding, &reviewers), vec!["exact-provider"]);
    }

    #[test]
    fn seeded_lens_ids_match_normalized_kinds_and_dedupe_providers() {
        let reviewers = vec![
            reviewer("general::alpha", "shared", Some("general")),
            reviewer("expert:security::alpha", "shared", Some("expert:security")),
            reviewer("expert:security::beta", "beta", Some("expert:security")),
        ];
        let finding = finding(&["security", "general", "security", "missing"]);

        assert_eq!(
            normalized_raisers_for(&finding, &reviewers, None),
            vec!["general", "security"]
        );
        assert_eq!(providers_for(&finding, &reviewers), vec!["beta", "shared"]);
    }

    #[test]
    fn missing_reviewer_metadata_uses_only_known_run_fallback() {
        let claim = finding(&["old-reviewer-id"]);
        assert_eq!(
            normalized_raisers_for(&claim, &[], Some("expert:security")),
            vec!["security"]
        );
        assert!(providers_for(&claim, &[]).is_empty());
        assert!(normalized_raisers_for(&claim, &[], Some("unknown")).is_empty());
        assert_eq!(
            normalized_raisers_for(&finding(&[]), &[], Some("expert:security")),
            vec!["security"]
        );
    }

    #[test]
    fn reviewer_metadata_does_not_turn_unknown_ids_into_fallback_security() {
        let reviewers = vec![reviewer("known", "provider", Some("expert:security"))];
        let finding = finding(&["unknown"]);
        assert!(normalized_raisers_for(&finding, &reviewers, Some("expert:security")).is_empty());
        assert!(providers_for(&finding, &reviewers).is_empty());
    }

    #[test]
    fn duplicate_and_unknown_provider_records_stay_honest() {
        let reviewers = vec![
            reviewer("one", "alpha", Some("expert:security")),
            reviewer("two", "alpha", Some("expert:security")),
        ];
        assert_eq!(
            providers_for(&finding(&["security"]), &reviewers),
            vec!["alpha"]
        );
        assert!(providers_for(&finding(&["expert:unregistered"]), &reviewers).is_empty());
    }
}
