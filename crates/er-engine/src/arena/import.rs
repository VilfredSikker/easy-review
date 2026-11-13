//! Import shipped arena findings into `.er/review.json` for the Review tab.

use super::model::{ArenaFinding, ArenaRun, Verdict};
use crate::ai::accepted_findings::AcceptedFinding;
use crate::ai::{Confidence, ErFileReview, ErReview, Finding, RiskLevel, GENERAL_LENS};
use anyhow::{Context, Result};
use er_store::{ArtifactKey, Loaded, Scope};
use std::collections::HashMap;

fn empty_review(run: &ArenaRun) -> ErReview {
    ErReview {
        version: 1,
        diff_hash: run.diff_hash.clone(),
        created_at: crate::app::chrono_now(),
        base_branch: run.base_branch.clone(),
        head_branch: run.branch_ref.clone(),
        files: HashMap::new(),
        file_hashes: HashMap::new(),
    }
}

fn latest_severity(f: &ArenaFinding) -> RiskLevel {
    f.severity_by_round
        .values()
        .next_back()
        .copied()
        .unwrap_or(RiskLevel::Medium)
}

fn arena_finding_to_review(f: &ArenaFinding, run: &ArenaRun) -> Finding {
    // Which producer raised it. The arena reports no defect kind, so `category`
    // stays empty. `agent_kind` is a task kind (`expert:security`, `professor`,
    // …), not a lens, so it is normalized rather than copied into the flat
    // vocabulary.
    let raised_by = super::provenance::normalized_raisers_for(
        f,
        &run.reviewers,
        run.config.agent_kind.as_deref(),
    );
    let lens = raised_by
        .first()
        .cloned()
        .unwrap_or_else(|| GENERAL_LENS.to_string());
    let providers = super::provenance::providers_for(f, &run.reviewers);
    let confidence = Confidence::from_score(f.confidence);
    Finding {
        id: format!("arena-{}-{}", run.id, f.id),
        severity: latest_severity(f),
        lens,
        category: String::new(),
        raised_by,
        providers,
        title: f.title.clone(),
        description: f.body.clone(),
        hunk_index: None,
        line_start: f.line,
        line_end: f.line,
        line_content: String::new(),
        stale: false,
        suggestion: f.rationale.clone(),
        related_files: vec![],
        outside_diff: false,
        confidence,
        verification_plan: String::new(),
        evidence: f.evidence.clone(),
        responses: vec![],
        resolved: false,
        resolved_note: String::new(),
        resolved_at: String::new(),
        promoted_to: None,
    }
}

const fn is_importable(f: &ArenaFinding, run: &ArenaRun) -> bool {
    match &f.verdict {
        Verdict::Kept | Verdict::Escalated | Verdict::Merged { .. } => true,
        // Single-round runs complete without arbiter (legacy runs stayed pending).
        Verdict::Pending => run.config.rounds <= 1,
        Verdict::Dropped => false,
    }
}

/// Build the artifact rows for the selected findings, in the given order.
///
/// The row id is the one the import produced, `arena-{run}-{finding}`, so ids
/// and order match what the front ends saw before the artifact existed.
pub fn accepted_finding_rows(
    run: &ArenaRun,
    ids: &[String],
    accepted_at: &str,
) -> Vec<AcceptedFinding> {
    ids.iter()
        .filter_map(|id| {
            let finding = run.findings.iter().find(|f| f.id == *id)?;
            if !is_importable(finding, run) || finding.accepted_at.is_some() {
                return None;
            }
            let row = arena_finding_to_review(finding, run);
            Some(AcceptedFinding {
                file: finding.file.clone(),
                run_finding_id: finding.id.clone(),
                run_id: run.id.clone(),
                run_diff_hash: run.diff_hash.clone(),
                accepted_at: accepted_at.to_string(),
                providers: row.providers.clone(),
                finding: row,
            })
        })
        .collect()
}

/// Mark the run's own record for the rows the artifact just took.
pub fn mark_accepted(run: &mut ArenaRun, rows: &[AcceptedFinding], accepted_at: &str) {
    for row in rows {
        if let Some(finding) = run
            .findings
            .iter_mut()
            .find(|finding| finding.id == row.run_finding_id)
        {
            finding.accepted_at = Some(accepted_at.to_string());
        }
        if !run.accepted_finding_ids.contains(&row.run_finding_id) {
            run.accepted_finding_ids.push(row.run_finding_id.clone());
        }
    }
}

/// Merge shipped arena findings into `review.json`. Returns count imported.
pub fn import_arena_findings_to_review(
    er_dir: &str,
    run: &mut ArenaRun,
    finding_ids: Option<&[String]>,
) -> Result<usize> {
    let store = crate::store_registry::store();
    let scope = Scope::dir(er_dir)?;
    let key = ArtifactKey::<ErReview>::new(scope, ())?;
    let mut review: ErReview = match store.read(&key).context("read review.json")? {
        Loaded::Found(stored) => stored.value,
        Loaded::Absent | Loaded::Unreadable(_) => empty_review(run),
    };
    review.diff_hash = run.diff_hash.clone();

    let ids: Vec<String> = match finding_ids {
        Some(ids) => ids.to_vec(),
        None => run
            .findings
            .iter()
            .filter(|f| is_importable(f, run) && f.accepted_at.is_none())
            .map(|f| f.id.clone())
            .collect(),
    };

    let now = crate::app::chrono_now();
    let mut imported = 0usize;
    for fid in &ids {
        let idx = run.findings.iter().position(|x| x.id == *fid);
        let Some(idx) = idx else {
            continue;
        };
        let af = run.findings[idx].clone();
        if !is_importable(&af, run) || af.accepted_at.is_some() {
            continue;
        }
        let finding = arena_finding_to_review(&af, run);
        let file = af.file.clone();
        let risk = latest_severity(&af);
        let entry = review
            .files
            .entry(file.clone())
            .or_insert_with(|| ErFileReview {
                risk,
                risk_reason: String::new(),
                summary: String::new(),
                findings: vec![],
            });
        if let Some(pos) = entry.findings.iter().position(|x| x.id == finding.id) {
            entry.findings[pos] = finding;
        } else {
            entry.findings.push(finding);
        }
        run.findings[idx].accepted_at = Some(now.clone());
        if !run.accepted_finding_ids.contains(fid) {
            run.accepted_finding_ids.push(fid.clone());
        }
        imported += 1;
    }

    if imported > 0 {
        store.write(&key, &review).context("write review")?;
    }
    Ok(imported)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `config.agent_kind` is a *task kind* (`expert:security`, `professor`),
    /// not a lens. Copying it through verbatim put a value outside the flat
    /// vocabulary into `Finding.lens`, where `expert_label_for_id` could not
    /// resolve it and the reviewer pill fell back to "General".
    #[test]
    fn arena_finding_lens_is_normalized_from_the_task_kind() {
        let run = |agent_kind: Option<&str>| -> ArenaRun {
            serde_json::from_value(serde_json::json!({
                "id": "arena-1",
                "branch_ref": "feature",
                "base_branch": "main",
                "scope": "branch",
                "diff_hash": "abc",
                "created_at": "2026-06-17T00:00:00Z",
                "status": "complete",
                "reviewers": [],
                "findings": [],
                "cost_estimate": { "tokens_in": 0, "tokens_out": 0, "usd": 0.0 },
                "config": {
                    "reviewers": [],
                    "rounds": 3,
                    "arbiter": { "provider_id": "anthropic", "model_id": "opus" },
                    "auto_accept_threshold": 0.75,
                    "scope": "branch",
                    "agent_kind": agent_kind,
                },
            }))
            .expect("a minimal run deserializes")
        };
        let finding: ArenaFinding = serde_json::from_value(serde_json::json!({
            "id": "f1",
            "file": "src/a.rs",
            "line": 1,
            "title": "t",
            "body": "b",
            "severity_by_round": { "1": "high" },
            "raised_by": [],
            "verdict": "kept",
            "confidence": 0.9,
            "rationale": "ok",
            "rounds": [],
            "merge_candidates": [],
        }))
        .expect("a minimal finding deserializes");

        let lens = |kind: Option<&str>| arena_finding_to_review(&finding, &run(kind)).lens;

        assert_eq!(lens(Some("expert:security")), "security");
        assert_eq!(lens(Some("professor")), "professor");
        assert_eq!(lens(Some("general")), GENERAL_LENS);
        assert_eq!(
            lens(None),
            GENERAL_LENS,
            "an unset kind is the general pass"
        );
    }

    #[test]
    fn an_existing_review_is_merged_through_the_store() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let store = crate::store_registry::store();
            let key = ArtifactKey::<ErReview>::new(Scope::dir(dir.path()).unwrap(), ()).unwrap();
            let existing: ErReview = serde_json::from_value(serde_json::json!({
                "version": 1,
                "diff_hash": "old",
                "files": {
                    "src/existing.rs": {
                        "risk": "low",
                        "findings": [{"id": "existing", "severity": "low", "title": "Existing"}]
                    }
                }
            }))
            .unwrap();
            store.write(&key, &existing).unwrap();

            let mut run: ArenaRun = serde_json::from_value(serde_json::json!({
                "id": "run",
                "branch_ref": "feature",
                "base_branch": "main",
                "scope": "branch",
                "diff_hash": "new",
                "created_at": "now",
                "status": "complete",
                "reviewers": [],
                "findings": [{
                    "id": "f1", "file": "src/new.rs", "line": 3,
                    "title": "Imported", "body": "body",
                    "severity_by_round": {"1": "high"}, "raised_by": [],
                    "verdict": "kept", "confidence": 0.9, "rationale": "", "rounds": []
                }],
                "cost_estimate": {"tokens_in": 0, "tokens_out": 0, "usd": 0.0},
                "config": {
                    "reviewers": [], "rounds": 1,
                    "arbiter": {"provider_id": "p", "model_id": "m"},
                    "auto_accept_threshold": 0.0, "scope": "branch"
                }
            }))
            .unwrap();

            let imported =
                import_arena_findings_to_review(dir.path().to_str().unwrap(), &mut run, None)
                    .unwrap();
            assert_eq!(imported, 1);
            match store.read(&key).unwrap() {
                Loaded::Found(stored) => {
                    assert_eq!(stored.value.diff_hash, "new");
                    assert!(
                        stored.value.files.contains_key("src/existing.rs"),
                        "the existing review survives the import"
                    );
                    assert!(
                        stored.value.files.contains_key("src/new.rs"),
                        "the imported finding lands in the stored review"
                    );
                }
                other => panic!("expected Found, got {other:?}"),
            }
        });
    }

    #[test]
    fn deep_import_normalizes_exact_reviewer_ids_and_carries_provider_ids() {
        let run = serde_json::from_value::<ArenaRun>(serde_json::json!({
            "id": "run-with-hyphens",
            "branch_ref": "feature",
            "base_branch": "main",
            "scope": "branch",
            "diff_hash": "abc",
            "created_at": "now",
            "status": "complete",
            "reviewers": [{
                "id": "reviewer-id-with-many-hyphens",
                "name": "Security reviewer",
                "kind": "agent",
                "provider_id": "provider-a",
                "model_id": "model-with-hyphens",
                "system_prompt": "",
                "color": "",
                "icon": "",
                "tagline": "",
                "cost_per_1k_in": 0.0,
                "cost_per_1k_out": 0.0,
                "avg_latency_ms": 0,
                "status": "ok",
                "agent_kind": "expert:security"
            }],
            "findings": [],
            "cost_estimate": { "tokens_in": 0, "tokens_out": 0, "usd": 0.0 },
            "config": {
                "reviewers": [], "rounds": 3,
                "arbiter": { "provider_id": "arbiter", "model_id": "model" },
                "auto_accept_threshold": 0.75, "scope": "branch"
            }
        }))
        .unwrap();
        let mut finding: ArenaFinding = serde_json::from_value(serde_json::json!({
            "id": "finding", "file": "src/a.rs", "line": 5, "title": "claim", "body": "body",
            "severity_by_round": { "1": "high" }, "raised_by": ["reviewer-id-with-many-hyphens"],
            "verdict": "kept", "confidence": 0.9, "rationale": "supported", "rounds": []
        }))
        .unwrap();
        let imported = arena_finding_to_review(&finding, &run);
        assert_eq!(imported.lens, "security");
        assert_eq!(imported.raised_by, ["security"]);
        assert_eq!(imported.providers, ["provider-a"]);

        finding.raised_by = vec!["unknown-reviewer-id".into()];
        let unknown = arena_finding_to_review(&finding, &run);
        assert_eq!(unknown.lens, GENERAL_LENS);
        assert!(unknown.raised_by.is_empty());
        assert!(unknown.providers.is_empty());
    }
}
