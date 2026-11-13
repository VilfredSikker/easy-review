//! Load-time findings projection: one pure function folds the general review,
//! the expert sidecars, the professor file and the arbiter overlay into the
//! review every consumer reads.
//!
//! The stages run in the order the loader applied them in place before this
//! module existed:
//!
//! 1. Backfill lenses on the general review.
//! 2. Append the accepted-findings artifact, in acceptance order.
//! 3. Append accepted expert rows, in ascending `expert_id` order.
//! 4. Merge professor rows.
//! 5. Clear agent-written providers and restore accepted-run provenance.
//! 6. Overlay arbiter verdicts last, so they grade every row above.
//!
//! Scoped re-review overlays replace their paths in [`ProjectionInputs`]
//! before stage 1, and validation runs between provenance and the arbiter.
//! A row added after provenance would keep agent-written providers and miss
//! the arena restore. Only validation sits between provenance and the
//! arbiter; the arbiter stays last so it grades the final rows.

use super::accepted_findings::AcceptedFinding;
use super::accepted_provenance::{ProvenanceRead, ReviewRowIdentity};
use super::arbiter::{merge_arbiter_into_review, ArbiterEffect, ArbiterReview, ArbiterRuling};
use super::experts::{backfill_finding_lenses, expert_by_id, expert_hash_accepted, ExpertReview};
use super::identity::finding_key;
use super::professor::{merge_professor_into_review, ProfessorReview};
use super::review::{ErFileReview, ErReview, RiskLevel};
use super::validation::{apply_validation, ValidationEffect, ValidationReview};
use std::collections::{HashMap, HashSet};

pub struct ProjectionInputs<'a> {
    /// The parsed `review.json`, not yet backfilled.
    pub general: Option<ErReview>,
    /// The bucket's accepted findings, in acceptance order. Appended after the
    /// general review and before the expert rows, which is where the import
    /// left them.
    pub accepted: &'a [AcceptedFinding],
    pub experts: &'a [ExpertReview],
    pub professor: Option<&'a ProfessorReview>,
    /// The arbitrated overlay captured from `arbiter.json`, never a second
    /// disk read.
    pub arbiter: Option<&'a ArbiterReview>,
    /// The validate pass's outcomes; applied between provenance and the
    /// arbiter.
    pub validation: Option<&'a ValidationReview>,
    pub current_diff_hash: &'a str,
    /// The only disk-backed input. `load_ai_state_with_arbiter` binds it to
    /// the bucket's `arena/` runs; tests bind it to a fixed map.
    pub accepted_provenance: &'a dyn Fn(&str, &[ReviewRowIdentity]) -> ProvenanceRead,
}

#[derive(Debug)]
pub struct FindingsProjection {
    pub review: Option<ErReview>,
    pub arbiter_effect: ArbiterEffect,
    pub arbiter_graded_keys: HashSet<String>,
    pub arbiter_verdicts: HashMap<String, (ArbiterRuling, String)>,
    /// Diagnostics from the accepted-run read; `load_ai_state_with_arbiter`
    /// logs each one with `crate::dev_log::arena_line`.
    pub provenance_diagnostics: Vec<String>,
    pub validation_effect: ValidationEffect,
}

pub fn build_findings_projection(inputs: ProjectionInputs<'_>) -> FindingsProjection {
    let ProjectionInputs {
        general,
        accepted,
        experts,
        professor,
        arbiter,
        validation,
        current_diff_hash,
        accepted_provenance,
    } = inputs;

    let mut review = general;
    let general_hash = review
        .as_ref()
        .map(|review| review.diff_hash.clone())
        .unwrap_or_default();
    if let Some(general) = review.as_mut() {
        backfill_finding_lenses(general);
    }

    // Accepted rows sit where the import used to leave them: after the general
    // review's own rows and before the expert rows. Provenance clears their
    // providers and restores them from the artifact row.
    if let Some(general) = review.as_mut() {
        append_accepted_findings(general, accepted);
    } else if let Some(first) = accepted.first() {
        let mut shell = review_shell(current_diff_hash, &first.accepted_at);
        append_accepted_findings(&mut shell, accepted);
        if !shell.files.is_empty() {
            review = Some(shell);
        }
    }

    // The hash rules differ, and stay as they were before this module:
    // experts ride along with a general review's own generation (a
    // selected-files run hashes the filtered diff, which the tab's full diff
    // never matches); without a general review only the current hash counts,
    // because the synthesized review is new output; the professor and the
    // arbiter require the current hash.
    let mut accepted: Vec<&ExpertReview> = experts
        .iter()
        .filter(|expert| expert_hash_accepted(expert, &general_hash, current_diff_hash))
        .collect();
    accepted.sort_by(|left, right| left.expert_id.cmp(&right.expert_id));

    if let Some(general) = review.as_mut() {
        append_expert_findings(general, &accepted);
    } else if let Some(first) = accepted.first() {
        let mut shell = review_shell(current_diff_hash, &first.created_at);
        append_expert_findings(&mut shell, &accepted);
        if !shell.files.is_empty() {
            review = Some(shell);
        }
    }

    if let Some(professor) = professor {
        if let Some(general) = review.as_mut() {
            merge_professor_into_review(general, professor, current_diff_hash);
        } else {
            let mut shell = review_shell(current_diff_hash, &professor.created_at);
            merge_professor_into_review(&mut shell, professor, current_diff_hash);
            if !shell.files.is_empty() {
                review = Some(shell);
            }
        }
    }

    let mut provenance_diagnostics = Vec::new();
    if let Some(general) = review.as_mut() {
        provenance_diagnostics = apply_accepted_provenance(general, accepted_provenance);
    }

    let mut validation_effect = ValidationEffect::default();
    if let (Some(general), Some(validation)) = (review.as_mut(), validation) {
        validation_effect = apply_validation(general, validation);
    }

    let mut arbiter_effect = ArbiterEffect::default();
    let mut arbiter_graded_keys = HashSet::new();
    let mut arbiter_verdicts = HashMap::new();
    if let Some(arbiter) = arbiter {
        if let Some(general) = review.as_mut() {
            if arbiter.diff_hash == general.diff_hash {
                let verdicts: HashMap<&str, (ArbiterRuling, &str)> = arbiter
                    .verdicts
                    .iter()
                    .map(|verdict| {
                        (
                            verdict.id.as_str(),
                            (verdict.verdict, verdict.rationale.as_str()),
                        )
                    })
                    .collect();
                arbiter_verdicts = general
                    .files
                    .iter()
                    .flat_map(|(path, file_review)| {
                        file_review.findings.iter().filter_map(|finding| {
                            let key = finding_key(path, finding.line_start, &finding.title);
                            let (ruling, rationale) = verdicts.get(key.as_str())?;
                            Some((key, (*ruling, (*rationale).to_owned())))
                        })
                    })
                    .collect();
                arbiter_graded_keys = arbiter_verdicts.keys().cloned().collect();
            }
            arbiter_effect = merge_arbiter_into_review(general, arbiter);
        }
    }

    FindingsProjection {
        review,
        arbiter_effect,
        arbiter_graded_keys,
        arbiter_verdicts,
        provenance_diagnostics,
        validation_effect,
    }
}

fn review_shell(diff_hash: &str, created_at: &str) -> ErReview {
    ErReview {
        version: 1,
        diff_hash: diff_hash.to_string(),
        created_at: created_at.to_string(),
        base_branch: String::new(),
        head_branch: String::new(),
        files: HashMap::new(),
        file_hashes: HashMap::new(),
    }
}

fn append_accepted_findings(review: &mut ErReview, accepted: &[AcceptedFinding]) {
    for row in accepted {
        let mut finding = row.finding.clone();
        finding.providers.clear();
        let entry = review
            .files
            .entry(row.file.clone())
            .or_insert_with(|| ErFileReview {
                risk: finding.severity,
                risk_reason: String::new(),
                summary: String::new(),
                findings: Vec::new(),
            });
        entry.findings.push(finding);
    }
}

fn append_expert_findings(review: &mut ErReview, accepted: &[&ExpertReview]) {
    for expert in accepted {
        let Some(def) = expert_by_id(&expert.expert_id) else {
            continue;
        };
        for (path, expert_file) in &expert.files {
            let entry = review
                .files
                .entry(path.clone())
                .or_insert_with(|| ErFileReview {
                    risk: RiskLevel::Info,
                    risk_reason: String::new(),
                    summary: String::new(),
                    findings: Vec::new(),
                });
            for mut finding in expert_file.findings.clone() {
                finding.id = prefix_finding_id(def.id_prefix, &finding.id);
                finding.lens = def.id.to_string();
                // A sidecar's own findings come from this one expert, so it is
                // the whole raiser set until something merges several.
                if finding.raised_by.is_empty() {
                    finding.raised_by = vec![def.id.to_string()];
                }
                entry.findings.push(finding);
            }
        }
    }
}

fn apply_accepted_provenance(
    review: &mut ErReview,
    accepted_provenance: &dyn Fn(&str, &[ReviewRowIdentity]) -> ProvenanceRead,
) -> Vec<String> {
    let rows: Vec<_> = review
        .files
        .iter()
        .flat_map(|(file, file_review)| {
            file_review
                .findings
                .iter()
                .map(move |finding| ReviewRowIdentity {
                    id: finding.id.clone(),
                    file: file.clone(),
                    line: finding.line_start,
                })
        })
        .collect();
    for file_review in review.files.values_mut() {
        for finding in &mut file_review.findings {
            // Provider attribution from review/expert producer JSON is never
            // authoritative. Only accepted host runs or the arbiter restore it.
            finding.providers.clear();
        }
    }
    let restored = accepted_provenance(&review.diff_hash, &rows);
    for file_review in review.files.values_mut() {
        for finding in &mut file_review.findings {
            let Some(provenance) = restored.rows.get(&finding.id) else {
                continue;
            };
            finding.providers = provenance.providers.clone();
            if !provenance.raisers.is_empty() {
                finding.raised_by = provenance.raisers.clone();
                finding.lens = provenance.raisers[0].clone();
            }
        }
    }
    restored.diagnostics
}

fn prefix_finding_id(prefix: &str, id: &str) -> String {
    if id.starts_with(&format!("{prefix}-")) {
        id.to_string()
    } else {
        format!("{prefix}-{id}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::experts::ExpertFileReview;
    use crate::ai::review::{Confidence, Finding};
    use crate::ai::ArbiterVerdict;
    use std::collections::HashMap;

    fn sample_finding(id: &str) -> Finding {
        Finding {
            id: id.to_string(),
            severity: RiskLevel::Medium,
            lens: String::new(),
            category: "correctness".to_string(),
            raised_by: Vec::new(),
            providers: Vec::new(),
            title: "Test".to_string(),
            description: String::new(),
            hunk_index: Some(0),
            line_start: Some(1),
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

    fn project(review: ErReview, experts: &[ExpertReview], current_diff_hash: &str) -> ErReview {
        let projection = build_findings_projection(ProjectionInputs {
            general: Some(review),
            accepted: &[],
            experts,
            professor: None,
            arbiter: None,
            validation: None,
            current_diff_hash,
            accepted_provenance: &|_, _| ProvenanceRead::default(),
        });
        projection.review.expect("a projected review")
    }

    #[test]
    fn merge_prefixes_ids_and_skips_stale() {
        let hash = "abc123";
        let review = ErReview {
            version: 1,
            diff_hash: hash.to_string(),
            created_at: String::new(),
            base_branch: String::new(),
            head_branch: String::new(),
            files: HashMap::new(),
            file_hashes: HashMap::new(),
        };
        let experts = vec![
            ExpertReview {
                version: 1,
                expert_id: "security".to_string(),
                diff_hash: hash.to_string(),
                diff_scope: "branch".to_string(),
                created_at: String::new(),
                summary: String::new(),
                files: HashMap::from([(
                    "src/a.rs".to_string(),
                    ExpertFileReview {
                        findings: vec![sample_finding("1")],
                    },
                )]),
            },
            ExpertReview {
                version: 1,
                expert_id: "patterns".to_string(),
                diff_hash: "stale".to_string(),
                diff_scope: String::new(),
                created_at: String::new(),
                summary: String::new(),
                files: HashMap::from([(
                    "src/b.rs".to_string(),
                    ExpertFileReview {
                        findings: vec![sample_finding("9")],
                    },
                )]),
            },
        ];
        let review = project(review, &experts, hash);
        assert_eq!(review.files.len(), 1);
        let f = &review.files["src/a.rs"];
        assert_eq!(f.findings.len(), 1);
        assert_eq!(f.findings[0].id, "sec-1");
        assert_eq!(f.findings[0].lens, "security");
    }

    /// The projection must not overwrite an expert's own `category`: an expert
    /// that classifies its finding as a correctness issue keeps that
    /// classification, and the producer name goes to `lens` instead.
    #[test]
    fn merge_keeps_the_experts_defect_category() {
        let hash = "abc123";
        let review = ErReview {
            version: 1,
            diff_hash: hash.to_string(),
            created_at: String::new(),
            base_branch: String::new(),
            head_branch: String::new(),
            files: HashMap::new(),
            file_hashes: HashMap::new(),
        };
        let experts = vec![ExpertReview {
            version: 1,
            expert_id: "security".to_string(),
            diff_hash: hash.to_string(),
            diff_scope: "branch".to_string(),
            created_at: String::new(),
            summary: String::new(),
            files: HashMap::from([(
                "src/a.rs".to_string(),
                ExpertFileReview {
                    findings: vec![sample_finding("1")],
                },
            )]),
        }];

        let review = project(review, &experts, hash);

        let f = &review.files["src/a.rs"].findings[0];
        assert_eq!(f.category, "correctness", "defect kind was overwritten");
        assert_eq!(f.lens, "security");
    }

    #[test]
    fn merge_adds_to_existing_file_entry() {
        let hash = "h";
        let review = ErReview {
            version: 1,
            diff_hash: hash.to_string(),
            created_at: String::new(),
            base_branch: String::new(),
            head_branch: String::new(),
            files: HashMap::from([(
                "x.rs".to_string(),
                ErFileReview {
                    risk: RiskLevel::Low,
                    risk_reason: String::new(),
                    summary: "s".to_string(),
                    findings: vec![sample_finding("f-1")],
                },
            )]),
            file_hashes: HashMap::new(),
        };
        let experts = vec![ExpertReview {
            version: 1,
            expert_id: "api".to_string(),
            diff_hash: hash.to_string(),
            diff_scope: String::new(),
            created_at: String::new(),
            summary: String::new(),
            files: HashMap::from([(
                "x.rs".to_string(),
                ExpertFileReview {
                    findings: vec![sample_finding("2")],
                },
            )]),
        }];
        let review = project(review, &experts, hash);
        assert_eq!(review.files["x.rs"].findings.len(), 2);
        assert!(review.files["x.rs"]
            .findings
            .iter()
            .any(|f| f.id == "api-2"));
    }

    #[test]
    fn merge_keeps_experts_sharing_the_review_hash_after_tab_diff_moved_on() {
        // Selected-files run: general + expert sidecars hash the filtered diff
        // ("scoped") while the tab hashes the full diff ("full"). The review
        // loads as stale; its experts must ride along instead of vanishing.
        // An expert from an older generation still stays out.
        let review = ErReview {
            version: 1,
            diff_hash: "scoped".to_string(),
            created_at: String::new(),
            base_branch: String::new(),
            head_branch: String::new(),
            files: HashMap::new(),
            file_hashes: HashMap::new(),
        };
        let expert = |id: &str, hash: &str| ExpertReview {
            version: 1,
            expert_id: id.to_string(),
            diff_hash: hash.to_string(),
            diff_scope: String::new(),
            created_at: String::new(),
            summary: String::new(),
            files: HashMap::from([(
                "m.sql".to_string(),
                ExpertFileReview {
                    findings: vec![sample_finding("1")],
                },
            )]),
        };
        let experts = vec![expert("reliability", "scoped"), expert("security", "older")];
        let review = project(review, &experts, "full");
        let lenses: Vec<&str> = review.files["m.sql"]
            .findings
            .iter()
            .map(|f| f.lens.as_str())
            .collect();
        assert_eq!(lenses, vec!["reliability"]);
        assert!(
            !expert_hash_accepted(&experts[0], "", "full"),
            "without a review hash only the tab's own hash is accepted"
        );
    }

    fn review_with_one_finding(line: usize, title: &str) -> ErReview {
        let mut finding = sample_finding("f-1");
        finding.title = title.to_string();
        finding.line_start = Some(line);
        ErReview {
            version: 1,
            diff_hash: "hash".to_string(),
            created_at: "t0".to_string(),
            base_branch: String::new(),
            head_branch: String::new(),
            files: HashMap::from([(
                "src/a.rs".to_string(),
                ErFileReview {
                    risk: RiskLevel::Medium,
                    risk_reason: String::new(),
                    summary: String::new(),
                    findings: vec![finding],
                },
            )]),
            file_hashes: HashMap::new(),
        }
    }

    fn verdict(id: String) -> ArbiterReview {
        ArbiterReview {
            version: 1,
            diff_hash: "hash".to_string(),
            created_at: "t1".to_string(),
            run_id: String::new(),
            verdicts: vec![ArbiterVerdict {
                id,
                file: "src/a.rs".to_string(),
                verdict: ArbiterRuling::Dropped,
                confidence: None,
                merged_into: None,
                rationale: String::new(),
                raised_by: Vec::new(),
                providers: Vec::new(),
            }],
        }
    }

    fn project_with_validation(
        review: ErReview,
        validation: &crate::ai::validation::ValidationReview,
        arbiter: &ArbiterReview,
    ) -> FindingsProjection {
        build_findings_projection(ProjectionInputs {
            general: Some(review),
            accepted: &[],
            experts: &[],
            professor: None,
            arbiter: Some(arbiter),
            validation: Some(validation),
            current_diff_hash: "hash",
            accepted_provenance: &|_, _| ProvenanceRead::default(),
        })
    }

    /// Validation runs before the arbiter, so a shifted row is re-keyed and an
    /// arbiter verdict keyed to the old anchor no longer matches it.
    #[test]
    fn a_shifted_finding_loses_an_arbiter_verdict_keyed_to_its_old_anchor() {
        use crate::ai::validation::{
            ValidationOutcome, ValidationPatch, ValidationResult, ValidationReview,
        };
        let old_key = finding_key("src/a.rs", Some(9), "claim");
        let mut shifted = ValidationOutcome {
            key: old_key,
            result: ValidationResult::Shifted,
            file: "src/a.rs".to_string(),
            patch: ValidationPatch::default(),
        };
        shifted.patch.line_start = Some(12);
        let validation = ValidationReview {
            version: 1,
            diff_hash: "hash".to_string(),
            review_created_at: "t0".to_string(),
            created_at: "t1".to_string(),
            outcomes: vec![shifted],
        };
        let old_anchor_verdict = verdict(finding_key("src/a.rs", Some(9), "claim"));

        let projection = project_with_validation(
            review_with_one_finding(9, "claim"),
            &validation,
            &old_anchor_verdict,
        );

        assert_eq!(projection.validation_effect.applied, 1);
        assert_eq!(projection.arbiter_effect.unmatched, 1);
        let row = &projection.review.expect("a review").files["src/a.rs"].findings[0];
        assert_eq!(row.line_start, Some(12));
        assert!(row.is_active(), "the dropped verdict did not land");
    }

    #[test]
    fn an_arbiter_verdict_keyed_to_the_shifted_anchor_still_applies() {
        use crate::ai::validation::{
            ValidationOutcome, ValidationPatch, ValidationResult, ValidationReview,
        };
        let old_key = finding_key("src/a.rs", Some(9), "claim");
        let mut shifted = ValidationOutcome {
            key: old_key,
            result: ValidationResult::Shifted,
            file: "src/a.rs".to_string(),
            patch: ValidationPatch::default(),
        };
        shifted.patch.line_start = Some(12);
        let validation = ValidationReview {
            version: 1,
            diff_hash: "hash".to_string(),
            review_created_at: "t0".to_string(),
            created_at: "t1".to_string(),
            outcomes: vec![shifted],
        };
        let shifted_verdict = verdict(finding_key("src/a.rs", Some(12), "claim"));

        let projection = project_with_validation(
            review_with_one_finding(9, "claim"),
            &validation,
            &shifted_verdict,
        );

        assert_eq!(projection.arbiter_effect.unmatched, 0);
        let row = &projection.review.expect("a review").files["src/a.rs"].findings[0];
        assert_eq!(row.confidence, Confidence::Dropped);
        assert!(!row.is_active());
    }
}
