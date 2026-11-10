//! `validation.json`: the validate pass's outcomes, applied to the projection
//! between provenance and the arbiter.
//!
//! The file is agent-written and bare (D5), so its freshness lives in the
//! projection, not in the store: a file is applied only when it names the same
//! diff hash and the same review generation it read.

use super::identity::finding_key;
use super::review::{Confidence, ErReview, EvidenceItem};
use er_store::{Artifact, Mode};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ValidationReview {
    pub version: u32,
    #[serde(default)]
    pub diff_hash: String,
    /// The review's `created_at` this pass read. A scoped general rerun keeps
    /// the base's `created_at`, so the earlier outcomes survive; a full rerun
    /// writes a new value, so an old file is ignored even if it survives.
    #[serde(default)]
    pub review_created_at: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default, deserialize_with = "lenient_outcomes")]
    pub outcomes: Vec<ValidationOutcome>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ValidationOutcome {
    pub key: String,
    pub result: ValidationResult,
    /// The finding's path, copied from the table. Validation never changes a
    /// row's file, and the host uses it to drop outcomes for re-reviewed paths.
    #[serde(default)]
    pub file: String,
    #[serde(default)]
    pub patch: ValidationPatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ValidationResult {
    Resolved,
    Persists,
    Shifted,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ValidationPatch {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub suggestion: Option<String>,
    #[serde(default)]
    pub confidence: Option<Confidence>,
    #[serde(default)]
    pub evidence: Option<Vec<EvidenceItem>>,
    #[serde(default)]
    pub hunk_index: Option<usize>,
    #[serde(default)]
    pub line_start: Option<usize>,
    #[serde(default)]
    pub line_end: Option<usize>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ValidationEffect {
    pub applied: usize,
    pub unmatched: usize,
    pub ignored: bool,
}

impl Artifact for ValidationReview {
    const KIND: &'static str = "validation";
    const VERSION: u32 = 1;
    const MODE: Mode = Mode::Bare;
    const FILE: &'static str = "validation.json";
    type Slot = ();

    fn diff_hash(&self) -> Option<&str> {
        Some(&self.diff_hash)
    }
}

/// Drop individual outcomes that fail to parse rather than the whole file,
/// mirroring `lenient_findings` for agent-written JSON.
fn lenient_outcomes<'de, D>(deserializer: D) -> Result<Vec<ValidationOutcome>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = Vec::<serde_json::Value>::deserialize(deserializer)?;
    Ok(raw
        .into_iter()
        .filter_map(|value| serde_json::from_value(value).ok())
        .collect())
}

/// Apply the validate pass to the merged review, before the arbiter overlay.
pub fn apply_validation(review: &mut ErReview, validation: &ValidationReview) -> ValidationEffect {
    let mut effect = ValidationEffect::default();
    if validation.review_created_at != review.created_at || validation.diff_hash != review.diff_hash
    {
        effect.ignored = true;
        return effect;
    }

    let mut rows: HashMap<String, Vec<(String, usize)>> = HashMap::new();
    for (path, file_review) in &review.files {
        for (index, finding) in file_review.findings.iter().enumerate() {
            rows.entry(finding_key(path, finding.line_start, &finding.title))
                .or_default()
                .push((path.clone(), index));
        }
    }

    for outcome in &validation.outcomes {
        let Some(matches) = rows.get(&outcome.key) else {
            effect.unmatched += 1;
            continue;
        };
        let mut applied = false;
        for (path, index) in matches {
            let Some(finding) = review
                .files
                .get_mut(path)
                .and_then(|file_review| file_review.findings.get_mut(*index))
            else {
                continue;
            };
            match outcome.result {
                ValidationResult::Resolved => {
                    finding.confidence = Confidence::Dropped;
                }
                ValidationResult::Persists | ValidationResult::Shifted => {
                    let patch = &outcome.patch;
                    if let Some(title) = &patch.title {
                        finding.title.clone_from(title);
                    }
                    if let Some(description) = &patch.description {
                        finding.description.clone_from(description);
                    }
                    if let Some(suggestion) = &patch.suggestion {
                        finding.suggestion.clone_from(suggestion);
                    }
                    if let Some(confidence) = patch.confidence {
                        finding.confidence = confidence;
                    }
                    if let Some(evidence) = &patch.evidence {
                        finding.evidence.clone_from(evidence);
                    }
                    if let Some(hunk_index) = patch.hunk_index {
                        finding.hunk_index = Some(hunk_index);
                    }
                    if let Some(line_start) = patch.line_start {
                        finding.line_start = Some(line_start);
                    }
                    if let Some(line_end) = patch.line_end {
                        finding.line_end = Some(line_end);
                    }
                }
            }
            applied = true;
        }
        if applied {
            effect.applied += 1;
        }
    }
    effect
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::review::{ErFileReview, Finding, RiskLevel};
    use std::collections::HashMap;

    fn finding(id: &str, title: &str, line: usize) -> Finding {
        Finding {
            id: id.to_string(),
            severity: RiskLevel::Medium,
            lens: String::new(),
            category: "correctness".to_string(),
            raised_by: Vec::new(),
            providers: Vec::new(),
            title: title.to_string(),
            description: String::new(),
            hunk_index: Some(0),
            line_start: Some(line),
            line_end: None,
            line_content: String::new(),
            stale: false,
            suggestion: String::new(),
            related_files: vec![],
            outside_diff: false,
            confidence: Confidence::Confirmed,
            verification_plan: String::new(),
            evidence: vec![],
            responses: vec![],
            resolved: false,
            resolved_note: String::new(),
            resolved_at: String::new(),
            promoted_to: None,
        }
    }

    fn sample_review(files: Vec<(&str, Vec<Finding>)>) -> ErReview {
        ErReview {
            version: 1,
            diff_hash: "hash".to_string(),
            created_at: "t0".to_string(),
            base_branch: "main".to_string(),
            head_branch: "feat".to_string(),
            files: files
                .into_iter()
                .map(|(path, findings)| {
                    (
                        path.to_string(),
                        ErFileReview {
                            risk: RiskLevel::Medium,
                            risk_reason: String::new(),
                            summary: String::new(),
                            findings,
                        },
                    )
                })
                .collect(),
            file_hashes: HashMap::new(),
        }
    }

    fn validation(outcomes: Vec<ValidationOutcome>) -> ValidationReview {
        ValidationReview {
            version: 1,
            diff_hash: "hash".to_string(),
            review_created_at: "t0".to_string(),
            created_at: "t1".to_string(),
            outcomes,
        }
    }

    fn outcome(key: String, result: ValidationResult) -> ValidationOutcome {
        ValidationOutcome {
            key,
            result,
            file: String::new(),
            patch: ValidationPatch::default(),
        }
    }

    #[test]
    fn validation_resolved_hides_the_finding() {
        let mut review = sample_review(vec![("src/a.rs", vec![finding("a1", "Bug", 3)])]);
        let key = finding_key("src/a.rs", Some(3), "Bug");
        let effect = apply_validation(
            &mut review,
            &validation(vec![outcome(key, ValidationResult::Resolved)]),
        );
        assert_eq!(effect.applied, 1);
        let row = &review.files["src/a.rs"].findings[0];
        assert_eq!(row.confidence, Confidence::Dropped);
        assert!(!row.is_active());
    }

    #[test]
    fn validation_shifted_moves_the_anchor() {
        let mut review = sample_review(vec![("src/a.rs", vec![finding("a1", "Bug", 3)])]);
        let key = finding_key("src/a.rs", Some(3), "Bug");
        let mut outcome = outcome(key, ValidationResult::Shifted);
        outcome.patch.line_start = Some(9);
        outcome.patch.line_end = Some(11);
        apply_validation(&mut review, &validation(vec![outcome]));
        let row = &review.files["src/a.rs"].findings[0];
        assert_eq!(row.line_start, Some(9));
        assert_eq!(row.line_end, Some(11));
        assert!(row.is_active());
    }

    #[test]
    fn validation_persists_patches_fields() {
        let mut review = sample_review(vec![("src/a.rs", vec![finding("a1", "Bug", 3)])]);
        let key = finding_key("src/a.rs", Some(3), "Bug");
        let mut outcome = outcome(key, ValidationResult::Persists);
        outcome.patch.title = Some("Better bug".to_string());
        outcome.patch.confidence = Some(Confidence::Informational);
        apply_validation(&mut review, &validation(vec![outcome]));
        let row = &review.files["src/a.rs"].findings[0];
        assert_eq!(row.title, "Better bug");
        assert_eq!(row.confidence, Confidence::Informational);
        assert_eq!(row.line_start, Some(3), "an unpatched anchor stays put");
    }

    #[test]
    fn validation_for_another_diff_hash_is_ignored() {
        let mut review = sample_review(vec![("src/a.rs", vec![finding("a1", "Bug", 3)])]);
        let key = finding_key("src/a.rs", Some(3), "Bug");
        let mut mismatched = validation(vec![outcome(key.clone(), ValidationResult::Resolved)]);
        mismatched.diff_hash = "other".to_string();
        let effect = apply_validation(&mut review, &mismatched);
        assert!(effect.ignored);
        assert_eq!(effect.applied, 0);
        assert!(review.files["src/a.rs"].findings[0].is_active());

        let mut other_review = sample_review(vec![("src/a.rs", vec![finding("a1", "Bug", 3)])]);
        let mut newer = validation(vec![outcome(key, ValidationResult::Resolved)]);
        newer.review_created_at = "t9".to_string();
        let effect = apply_validation(&mut other_review, &newer);
        assert!(effect.ignored);
        assert!(other_review.files["src/a.rs"].findings[0].is_active());
    }

    #[test]
    fn validation_entry_with_unknown_key_is_counted_not_applied() {
        let mut review = sample_review(vec![("src/a.rs", vec![finding("a1", "Bug", 3)])]);
        let effect = apply_validation(
            &mut review,
            &validation(vec![outcome(
                "deadbeef".to_string(),
                ValidationResult::Resolved,
            )]),
        );
        assert_eq!(effect.applied, 0);
        assert_eq!(effect.unmatched, 1);
        assert!(review.files["src/a.rs"].findings[0].is_active());
    }

    /// Keys are the pre-validation identity: the row map is built once from the
    /// loaded rows, so an outcome keyed to the original anchor still matches
    /// after an earlier outcome in the same file moved it.
    #[test]
    fn second_validation_resolves_a_row_the_first_moved() {
        let mut review = sample_review(vec![("src/a.rs", vec![finding("a1", "Bug", 3)])]);
        let key = finding_key("src/a.rs", Some(3), "Bug");
        let mut first = outcome(key.clone(), ValidationResult::Shifted);
        first.patch.line_start = Some(40);
        apply_validation(
            &mut review,
            &validation(vec![first, outcome(key, ValidationResult::Resolved)]),
        );
        assert_eq!(
            review.files["src/a.rs"].findings[0].confidence,
            Confidence::Dropped
        );
    }
}
