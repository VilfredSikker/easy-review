//! Read-only projection and explicit acceptance for review-flow arena results.

use super::{ReviewFlowHost, ReviewFlows};
use crate::ai::accepted_findings::record_accepted;
use crate::ai::review_flow::{FlowState, ReviewFlow};
use crate::arena::{
    accepted_finding_rows, load_run, mark_accepted, save_run, ArenaPaths, ArenaRun, ArenaRunKind,
    RunStatus, Verdict,
};
use crate::task_target::BackgroundTaskTarget;
use anyhow::{bail, Context, Result};
use er_store::{ArtifactKey, ArtifactSnapshot, ArtifactStore, Loaded, RelPath, Scope};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static ACCEPT_ARCHIVE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// Accepted Deep rulings supersede older grades for exactly the accepted keys.
/// Keep a byte snapshot so failure can restore both sidecars without clobbering
/// an external writer that replaced either one during acceptance.
struct AcceptedOverlay {
    original: ArtifactSnapshot<crate::ai::ArbiterReview>,
    replacement: crate::ai::ArbiterReview,
}

impl AcceptedOverlay {
    fn prepare(
        store: &ArtifactStore,
        er_dir: &Path,
        run: &ArenaRun,
        selected: &[String],
    ) -> Result<Option<Self>> {
        let key = ArtifactKey::<crate::ai::ArbiterReview>::new(Scope::dir(er_dir)?, ())?;
        let snapshot = match store
            .snapshot(&key)
            .context("inspect arbiter overlay before acceptance")?
        {
            Loaded::Absent => return Ok(None),
            Loaded::Found(stored) => stored.value,
            Loaded::Unreadable(reason) => {
                bail!("arbiter.json must be a regular readable file for acceptance: {reason:?}")
            }
        };
        let Loaded::Found(stored) = snapshot.parse() else {
            return Ok(None);
        };
        let mut overlay = stored.value;
        if overlay.diff_hash != run.diff_hash {
            return Ok(None);
        }
        let replacements: std::collections::HashMap<_, _> = run
            .findings
            .iter()
            .filter(|finding| selected.contains(&finding.id))
            .map(|finding| {
                (
                    crate::ai::finding_key(&finding.file, finding.line, &finding.title),
                    finding,
                )
            })
            .collect();
        let mut changed = false;
        for verdict in &mut overlay.verdicts {
            let Some(finding) = replacements.get(&verdict.id) else {
                continue;
            };
            *verdict = crate::ai::ArbiterVerdict {
                id: verdict.id.clone(),
                file: finding.file.clone(),
                verdict: if finding.verdict == Verdict::Escalated {
                    crate::ai::ArbiterRuling::Escalated
                } else {
                    crate::ai::ArbiterRuling::Kept
                },
                confidence: Some(crate::ai::Confidence::from_score(finding.confidence)),
                merged_into: None,
                rationale: finding.rationale.clone(),
                raised_by: crate::arena::normalized_raisers_for(
                    finding,
                    &run.reviewers,
                    run.config.agent_kind.as_deref(),
                ),
                providers: crate::arena::providers_for(finding, &run.reviewers),
            };
            changed = true;
        }
        if !changed {
            return Ok(None);
        }
        // This sidecar now contains rulings from several runs. Neither run ID
        // nor creation time describes the complete set anymore.
        overlay.run_id.clear();
        overlay.created_at.clear();
        Ok(Some(Self {
            original: snapshot,
            replacement: overlay,
        }))
    }

    fn write_replacement(
        &self,
        store: &ArtifactStore,
    ) -> Result<ArtifactSnapshot<crate::ai::ArbiterReview>> {
        match store.write_typed_if_unchanged(&self.original, &self.replacement)? {
            Some(written) => Ok(written),
            None => bail!(
                "arbiter.json changed during acceptance; refusing to overwrite the newer file"
            ),
        }
    }

    fn restore_original(
        &self,
        store: &ArtifactStore,
        written: &ArtifactSnapshot<crate::ai::ArbiterReview>,
    ) -> Result<()> {
        if store.restore_if_unchanged(&self.original, written)? {
            Ok(())
        } else {
            bail!("arbiter.json changed during acceptance; refusing to overwrite the newer file")
        }
    }

    fn archive(&self, store: &ArtifactStore, archive_dir: &RelPath) -> Result<()> {
        store
            .write_backup_to_dir(&self.original, archive_dir)
            .context("archive superseded arbiter verdicts")
    }
}

/// Findings visible in the results overlay for one exact flow run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewFlowResults {
    pub er_dir: String,
    pub run_id: String,
    pub diff_hash: String,
    pub findings: Vec<ReviewFlowResultFinding>,
}

/// A finding that survived flow arbitration and can be shown to the reviewer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewFlowResultFinding {
    pub id: String,
    pub file: String,
    pub line: Option<usize>,
    pub title: String,
    pub body: String,
    pub confidence: f32,
    pub raised_by: Vec<String>,
    pub verdict: Verdict,
    pub accepted: bool,
    pub selectable: bool,
}

impl ReviewFlows {
    /// Load a flow's surviving results without importing or changing any files.
    /// Both the bucket and run ID are required; this never follows the active tab.
    pub fn review_flow_results(&self, er_dir: &str, run_id: &str) -> Result<ReviewFlowResults> {
        flow_run_results(
            self.flows.get(er_dir),
            self.review_flow_run_target(er_dir, run_id),
            er_dir,
            run_id,
        )
    }

    /// Explicitly import only selected, still-eligible findings from a flow run.
    /// Admission, captured-view checks, and a fresh diff hash guard the write.
    pub fn accept_review_flow_findings(
        &mut self,
        host: &mut dyn ReviewFlowHost,
        er_dir: &str,
        run_id: &str,
        finding_ids: Vec<String>,
    ) -> Result<usize> {
        if finding_ids.is_empty() {
            bail!("Select at least one finding to accept");
        }
        let target =
            require_run_target(self.review_flow_run_target(er_dir, run_id), er_dir)?.clone();
        crate::arena::check_bucket_admission(er_dir)?;
        host.recover_arena_bucket(er_dir)?;
        let lease = host.acquire_review_flow_lease(er_dir)?;

        let result =
            self.accept_flow_findings_with_lease(host, er_dir, run_id, &target, finding_ids);
        let release = host.release_review_flow_lease(&lease);
        match (result, release) {
            (Ok(count), Ok(())) => Ok(count),
            (Err(error), Ok(())) => Err(error),
            (Ok(_), Err(release_error)) => Err(release_error),
            (Err(error), Err(release_error)) => Err(error.context(format!(
                "also failed to release the short-lived review bucket lease: {release_error}"
            ))),
        }
    }

    fn accept_flow_findings_with_lease(
        &mut self,
        host: &mut dyn ReviewFlowHost,
        er_dir: &str,
        run_id: &str,
        target: &BackgroundTaskTarget,
        finding_ids: Vec<String>,
    ) -> Result<usize> {
        validate_current_flow_identity(self.flows.get(er_dir), run_id)?;

        let current_diff = host.current_review_diff(target)?;
        if current_diff.trim().is_empty()
            || crate::ai::compute_diff_hash(&current_diff)
                != load_run(&ArenaPaths::for_run(Path::new(er_dir), run_id))?.diff_hash
        {
            bail!("The diff changed since this Review; start a new Review before accepting");
        }

        let paths = ArenaPaths::for_run(Path::new(er_dir), run_id);
        let mut run = load_run(&paths)?;
        validate_flow_result_run(&run, run_id)?;
        validate_run_target(&run, target, &self.flows[er_dir].diff_hash)?;
        if run.status != RunStatus::Complete {
            bail!("Only completed review-flow results can be accepted");
        }
        let eligible: std::collections::HashSet<&str> = run
            .findings
            .iter()
            .filter(|finding| {
                matches!(finding.verdict, Verdict::Kept | Verdict::Escalated)
                    && finding.accepted_at.is_none()
                    && !run.accepted_finding_ids.contains(&finding.id)
            })
            .map(|finding| finding.id.as_str())
            .collect();
        let mut unique = std::collections::HashSet::new();
        if finding_ids
            .iter()
            .any(|id| !unique.insert(id.as_str()) || !eligible.contains(id.as_str()))
        {
            bail!("Selection contains a missing, dropped, or already accepted finding");
        }

        let store = crate::store_registry::store();
        let scope = Scope::dir(Path::new(er_dir))?;
        let overlay = AcceptedOverlay::prepare(&store, Path::new(er_dir), &run, &finding_ids)?;
        let sequence = ACCEPT_ARCHIVE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let archive_key = format!("accept-{}-{sequence}", std::process::id());
        let archive_dir = RelPath::new(Path::new("flow-backups").join(&archive_key))?;
        if let Some(overlay) = &overlay {
            overlay.archive(&store, &archive_dir)?;
        }

        // `review.json` stays untouched: the accepted rows are host output and
        // live in their own artifact. The run's own record is saved after the
        // artifact, and a failed save rolls the artifact back so the two never
        // disagree.
        let now = crate::sync::chrono_now();
        let rows = accepted_finding_rows(&run, &finding_ids, &now);
        if rows.is_empty() {
            bail!("Selection contains a missing, dropped, or already accepted finding");
        }
        let accepted =
            record_accepted(&store, &scope, &rows).context("record the accepted findings")?;
        mark_accepted(&mut run, &rows, &now);
        let written_overlay = match &overlay {
            Some(overlay) => match overlay.write_replacement(&store) {
                Ok(written) => Some(written),
                Err(error) => {
                    return match accepted.rollback(&store) {
                        Ok(()) => Err(error).context(
                            "arbiter.json could not be written; accepted-findings.json was rolled back",
                        ),
                        Err(rollback) => Err(error).context(format!(
                            "arbiter.json could not be written and the accepted-findings.json rollback failed: {rollback}"
                        )),
                    };
                }
            },
            None => None,
        };
        if let Err(error) = save_run(&paths, &run) {
            let artifact_rollback = accepted.rollback(&store);
            let overlay_rollback = match (&overlay, &written_overlay) {
                (Some(overlay), Some(written)) => overlay.restore_original(&store, written),
                _ => Ok(()),
            };
            return match (artifact_rollback, overlay_rollback) {
                (Ok(()), Ok(())) => Err(error).context(
                    "accepted IDs could not be saved to the run; acceptance sidecars were rolled back",
                ),
                (artifact, arbiter) => Err(error).context(format!(
                    "accepted IDs could not be saved to the run; rollback failed for accepted-findings.json {:?} or arbiter.json {:?}; recovery archive is {}",
                    artifact.err(), arbiter.err(), archive_dir.as_path().display()
                )),
            };
        }

        if let Some(flow) = self.flows.get_mut(er_dir) {
            if let FlowState::Done {
                awaiting_accept, ..
            } = &mut flow.state
            {
                let remaining = run
                    .findings
                    .iter()
                    .filter(|finding| {
                        matches!(finding.verdict, Verdict::Kept | Verdict::Escalated)
                            && finding.accepted_at.is_none()
                            && !run.accepted_finding_ids.contains(&finding.id)
                    })
                    .count();
                *awaiting_accept = remaining;
            }
        }
        self.bump_review_flow_revision();
        host.notify(&format!("Accepted {} finding(s) into Review", rows.len()));
        Ok(rows.len())
    }
}

/// The results of `run_id` for a flow a host published: its projection and
/// the target it captured when the run started. A reader that holds only the
/// published copy checks a run the same way the flow's owner does.
pub fn flow_run_results(
    flow: Option<&ReviewFlow>,
    target: Option<&BackgroundTaskTarget>,
    er_dir: &str,
    run_id: &str,
) -> Result<ReviewFlowResults> {
    let target = require_run_target(target, er_dir)?;
    let flow = validate_current_flow_identity(flow, run_id)?;
    let run = load_run(&ArenaPaths::for_run(Path::new(er_dir), run_id))?;
    validate_flow_result_run(&run, run_id)?;
    validate_run_target(&run, target, &flow.diff_hash)?;

    let findings = run
        .findings
        .iter()
        .filter(|finding| matches!(finding.verdict, Verdict::Kept | Verdict::Escalated))
        .map(|finding| {
            let accepted =
                finding.accepted_at.is_some() || run.accepted_finding_ids.contains(&finding.id);
            ReviewFlowResultFinding {
                id: finding.id.clone(),
                file: finding.file.clone(),
                line: finding.line,
                title: finding.title.clone(),
                body: finding.body.clone(),
                confidence: finding.confidence,
                raised_by: crate::arena::normalized_raisers_for(
                    finding,
                    &run.reviewers,
                    run.config.agent_kind.as_deref(),
                ),
                verdict: finding.verdict.clone(),
                accepted,
                selectable: !accepted
                    && matches!(finding.verdict, Verdict::Kept | Verdict::Escalated),
            }
        })
        .collect();

    Ok(ReviewFlowResults {
        er_dir: target.er_dir.clone(),
        run_id: run.id,
        diff_hash: run.diff_hash,
        findings,
    })
}

fn require_run_target<'a>(
    target: Option<&'a BackgroundTaskTarget>,
    er_dir: &str,
) -> Result<&'a BackgroundTaskTarget> {
    let target = target.context("Unknown or stale review-flow run")?;
    if target.er_dir != er_dir {
        bail!("Review-flow run bucket does not match");
    }
    Ok(target)
}

fn validate_current_flow_identity<'a>(
    flow: Option<&'a ReviewFlow>,
    run_id: &str,
) -> Result<&'a ReviewFlow> {
    match flow {
        Some(flow) if flow.arena_run_id.as_deref() == Some(run_id) => Ok(flow),
        _ => bail!("This review-flow result no longer matches the current flow"),
    }
}

fn validate_flow_result_run(run: &ArenaRun, run_id: &str) -> Result<()> {
    if run.id != run_id {
        bail!("Arena run identity does not match the requested flow run");
    }
    if run.config.run_kind == ArenaRunKind::Seeded {
        bail!("Seeded validation results are not review-flow results");
    }
    if run.status != RunStatus::Complete {
        bail!("Review-flow results are available after the run completes");
    }
    Ok(())
}

fn validate_run_target(
    run: &ArenaRun,
    target: &BackgroundTaskTarget,
    flow_diff_hash: &str,
) -> Result<()> {
    if run.diff_hash != flow_diff_hash
        || run.scope != arena_scope(&target.scope)?
        || run.branch_ref != target.branch_label
        || run.base_branch != target.base_branch
    {
        bail!("Review-flow run no longer matches its captured target");
    }
    Ok(())
}

fn arena_scope(scope: &str) -> Result<crate::arena::ArenaScope> {
    match scope {
        "branch" => Ok(crate::arena::ArenaScope::Branch),
        "staged" => Ok(crate::arena::ArenaScope::Staged),
        "unstaged" => Ok(crate::arena::ArenaScope::Unstaged),
        _ => bail!("Unsupported review-flow result scope"),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn overlay_rollback_preserves_intervening_external_edits() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let store = crate::store_registry::store();
            let key =
                ArtifactKey::<crate::ai::ArbiterReview>::new(Scope::dir(dir.path()).unwrap(), ())
                    .unwrap();
            let path = store.path(&key);
            std::fs::write(&path, b"original overlay").unwrap();
            let original = match store.snapshot(&key).unwrap() {
                Loaded::Found(stored) => stored.value,
                _ => panic!("expected a readable snapshot"),
            };
            std::fs::write(&path, b"our committed overlay").unwrap();
            let committed = match store.snapshot(&key).unwrap() {
                Loaded::Found(stored) => stored.value,
                _ => panic!("expected a readable snapshot"),
            };

            std::fs::write(&path, b"external replacement").unwrap();
            assert!(!store.restore_if_unchanged(&original, &committed).unwrap());
            assert_eq!(std::fs::read(&path).unwrap(), b"external replacement");
        });
    }

    #[test]
    fn an_overlay_replacement_refuses_a_file_changed_behind_it() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let store = crate::store_registry::store();
            let key =
                ArtifactKey::<crate::ai::ArbiterReview>::new(Scope::dir(dir.path()).unwrap(), ())
                    .unwrap();
            let original: crate::ai::ArbiterReview = serde_json::from_value(json!({
                "version": 1, "diff_hash": "hash",
                "verdicts": [{"id": "kept", "verdict": "dropped"}]
            }))
            .unwrap();
            store.write(&key, &original).unwrap();
            let snapshot = match store.snapshot(&key).unwrap() {
                Loaded::Found(stored) => stored.value,
                _ => panic!("expected a readable snapshot"),
            };
            let replacement: crate::ai::ArbiterReview = serde_json::from_value(json!({
                "version": 1, "diff_hash": "hash",
                "verdicts": [{"id": "kept", "verdict": "kept"}]
            }))
            .unwrap();
            let overlay = AcceptedOverlay {
                original: snapshot,
                replacement,
            };

            // Another writer replaces the file after the snapshot was taken.
            let external: crate::ai::ArbiterReview = serde_json::from_value(json!({
                "version": 1, "diff_hash": "hash", "verdicts": []
            }))
            .unwrap();
            store.write(&key, &external).unwrap();

            let error = overlay.write_replacement(&store).unwrap_err();
            assert!(
                error.to_string().contains("changed during acceptance"),
                "{error}"
            );
            match store.read(&key).unwrap() {
                Loaded::Found(stored) => assert!(stored.value.verdicts.is_empty()),
                _ => panic!("the external write survives"),
            }
        });
    }

    #[cfg(unix)]
    #[test]
    fn overlay_replacement_refuses_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let target = dir.path().join("external.json");
            std::fs::write(&target, b"original").unwrap();
            let path = dir.path().join("arbiter.json");
            std::os::unix::fs::symlink(&target, &path).unwrap();
            let store = crate::store_registry::store();
            let key =
                ArtifactKey::<crate::ai::ArbiterReview>::new(Scope::dir(dir.path()).unwrap(), ())
                    .unwrap();
            assert!(store.snapshot(&key).is_err());
            assert_eq!(std::fs::read(&target).unwrap(), b"original");
        });
    }
}
