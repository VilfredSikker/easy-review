//! Read-only projection and explicit acceptance for review-flow arena results.

use super::super::background::BackgroundTaskTarget;
use super::super::{App, DiffMode};
use crate::ai::accepted_findings::record_accepted;
use crate::ai::review_flow::FlowState;
use crate::arena::{
    accepted_finding_rows, load_run, mark_accepted, save_run, ArenaPaths, ArenaRun, ArenaRunKind,
    RunStatus, Verdict,
};
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

impl App {
    /// Load a flow's surviving results without importing or changing any files.
    /// Both the bucket and run ID are required; this never follows the active tab.
    pub fn review_flow_results(&self, er_dir: &str, run_id: &str) -> Result<ReviewFlowResults> {
        let target = self.require_review_flow_run_target(er_dir, run_id)?;
        self.validate_current_flow_identity(er_dir, run_id)?;
        let run = load_run(&ArenaPaths::for_run(Path::new(er_dir), run_id))?;
        validate_flow_result_run(&run, run_id)?;
        validate_run_target(&run, target, &self.review_flows[er_dir].diff_hash)?;

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

    /// Explicitly import only selected, still-eligible findings from a flow run.
    /// Admission, captured-view checks, and a fresh diff hash guard the write.
    pub fn accept_review_flow_findings(
        &mut self,
        er_dir: &str,
        run_id: &str,
        finding_ids: Vec<String>,
    ) -> Result<usize> {
        if finding_ids.is_empty() {
            bail!("Select at least one finding to accept");
        }
        let target = self.require_review_flow_run_target(er_dir, run_id)?.clone();
        self.check_review_bucket_admission(er_dir)?;
        self.recover_arena_bucket(er_dir)?;
        let lease = self.acquire_review_flow_lease(er_dir)?;

        let result = self.accept_flow_findings_with_lease(er_dir, run_id, &target, finding_ids);
        let release = self.release_review_flow_lease(&lease);
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
        er_dir: &str,
        run_id: &str,
        target: &BackgroundTaskTarget,
        finding_ids: Vec<String>,
    ) -> Result<usize> {
        self.validate_current_flow_identity(er_dir, run_id)?;

        let tab_index = self
            .tabs
            .iter()
            .position(|tab| tab_matches_target(tab, target))
            .context("The captured review target is no longer open; start a new Review")?;
        let raw_diff = self.tabs[tab_index]
            .fetch_tab_raw_diff(&target.scope)
            .context("fetch the current diff for review acceptance")?;
        let globs = crate::projects_pins::review_ignore_globs_for_repo(
            &target.repo_root,
            target.remote_repo.as_deref(),
        );
        let current_diff = crate::git::filter_raw_diff_exclude_globs(&raw_diff, &globs);
        if current_diff.trim().is_empty()
            || crate::ai::compute_diff_hash(&current_diff)
                != load_run(&ArenaPaths::for_run(Path::new(er_dir), run_id))?.diff_hash
        {
            bail!("The diff changed since this Review; start a new Review before accepting");
        }

        let paths = ArenaPaths::for_run(Path::new(er_dir), run_id);
        let mut run = load_run(&paths)?;
        validate_flow_result_run(&run, run_id)?;
        validate_run_target(&run, target, &self.review_flows[er_dir].diff_hash)?;
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
        let now = crate::app::chrono_now();
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

        for tab in self.tabs.iter_mut().filter(|tab| tab.er_dir() == er_dir) {
            tab.reload_ai_state();
        }
        if let Some(flow) = self.review_flows.get_mut(er_dir) {
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
        self.notify(&format!("Accepted {} finding(s) into Review", rows.len()));
        Ok(rows.len())
    }

    fn require_review_flow_run_target(
        &self,
        er_dir: &str,
        run_id: &str,
    ) -> Result<&BackgroundTaskTarget> {
        let target = self
            .review_flow_run_target(er_dir, run_id)
            .context("Unknown or stale review-flow run")?;
        if target.er_dir != er_dir {
            bail!("Review-flow run bucket does not match");
        }
        Ok(target)
    }

    fn validate_current_flow_identity(&self, er_dir: &str, run_id: &str) -> Result<()> {
        if self
            .review_flows
            .get(er_dir)
            .is_none_or(|flow| flow.arena_run_id.as_deref() != Some(run_id))
        {
            bail!("This review-flow result no longer matches the current flow");
        }
        Ok(())
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

fn tab_matches_target(tab: &crate::app::TabState, target: &BackgroundTaskTarget) -> bool {
    let branch_ref = tab
        .local_branch_view
        .as_deref()
        .unwrap_or(tab.current_branch.as_str());
    let scope_matches = match target.scope.as_str() {
        "branch" => matches!(
            tab.mode,
            DiffMode::Branch | DiffMode::PrDiff | DiffMode::Tour
        ),
        "staged" => tab.mode == DiffMode::Staged,
        "unstaged" => tab.mode == DiffMode::Unstaged,
        _ => false,
    };
    tab.repo_root == target.repo_root
        && tab.er_dir() == target.er_dir
        && branch_ref == target.branch_label
        && tab.base_branch == target.base_branch
        && tab.pr_number == target.pr_number
        && tab.remote_repo == target.remote_repo
        && tab.is_remote() == (target.remote_repo.is_some() && tab.local_branch_view.is_none())
        && scope_matches
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::review_flow::ReviewFlow;
    use crate::arena::save_run;
    use crate::paths::ErRoot;
    use serde_json::json;
    use std::process::Command;
    use tempfile::TempDir;

    struct Fixture {
        repo: TempDir,
        app: App,
        er_dir: String,
        run_id: String,
    }

    fn git(root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn fixture() -> Fixture {
        let repo = tempfile::tempdir().unwrap();
        let root = repo.path();
        git(root, &["init", "-b", "main"]);
        std::fs::write(root.join(".git/info/exclude"), ".er/\n").unwrap();
        git(root, &["config", "user.email", "test@example.com"]);
        git(root, &["config", "user.name", "Test"]);
        git(root, &["config", "commit.gpgsign", "false"]);
        std::fs::write(root.join("file.txt"), "base\n").unwrap();
        git(root, &["add", "file.txt"]);
        git(root, &["commit", "-m", "base"]);
        git(root, &["checkout", "-b", "feature"]);
        std::fs::write(root.join("file.txt"), "base\nfeature\n").unwrap();
        git(root, &["add", "file.txt"]);
        git(root, &["commit", "-m", "feature"]);

        let root = root.to_string_lossy().into_owned();
        let mut tab = crate::app::TabState::new_for_test(Vec::new());
        tab.ports = crate::ports::Ports::cli();
        tab.mode = DiffMode::Branch;
        tab.repo_root.clone_from(&root);
        tab.er_root = ErRoot::managed(format!("{root}/.er"));
        tab.base_branch = "main".into();
        tab.current_branch = "feature".into();
        tab.local_branch_view = Some("feature".into());
        tab.local_branch_checkout_root = Some(root.clone());
        let er_dir = tab.er_dir();

        let mut app = App::new_for_test(Vec::new());
        app.ports = crate::ports::Ports::cli();
        app.tabs[0] = tab;
        let target = BackgroundTaskTarget {
            repo_root: root.clone(),
            er_dir: er_dir.clone(),
            branch_label: "feature".into(),
            base_branch: "main".into(),
            scope: "branch".into(),
            pr_number: None,
            remote_repo: None,
            managed_local: true,
        };
        let run_id = "flow-run".to_string();
        let raw = app.tabs[0].fetch_tab_raw_diff("branch").unwrap();
        let hash = crate::ai::compute_diff_hash(&raw);
        let run: ArenaRun = serde_json::from_value(json!({
            "id": run_id,
            "branch_ref": "feature",
            "base_branch": "main",
            "scope": "branch",
            "diff_hash": hash,
            "created_at": "2026-10-23T00:00:00Z",
            "completed_at": "2026-10-23T00:01:00Z",
            "status": "complete",
            "config": {
                "reviewers": [], "rounds": 2,
                "arbiter": {"provider_id": "fake", "model_id": "test"},
                "auto_accept_threshold": 0.0, "scope": "branch", "run_kind": "models"
            },
            "reviewers": [],
            "findings": [
                {"id":"kept","file":"file.txt","line":2,"title":"Kept finding","body":"Details","severity_by_round":{"1":"medium"},"raised_by":[],"verdict":"kept","confidence":0.9,"rationale":"","rounds":[]},
                {"id":"dropped","file":"file.txt","line":1,"title":"Dropped finding","body":"Details","severity_by_round":{"1":"low"},"raised_by":[],"verdict":"dropped","confidence":0.4,"rationale":"","rounds":[]}
            ],
            "cost_estimate": {"tokens_in":0,"tokens_out":0,"usd":0.0},
            "accepted_finding_ids": []
        })).unwrap();
        let paths = ArenaPaths::for_run(Path::new(&er_dir), &run_id);
        save_run(&paths, &run).unwrap();

        let mut flow = ReviewFlow::new(hash);
        flow.arena_run_id = Some(run_id.clone());
        flow.state = FlowState::Done {
            kept: 1,
            dropped: 1,
            awaiting_accept: 1,
        };
        app.review_flows.insert(er_dir.clone(), flow);
        app.remember_review_flow_run_target(&target, &run_id);

        Fixture {
            repo,
            app,
            er_dir,
            run_id,
        }
    }

    fn historical_run(fixture: &Fixture, id: &str, status: RunStatus) -> ArenaPaths {
        let source = ArenaPaths::for_run(Path::new(&fixture.er_dir), &fixture.run_id);
        let paths = ArenaPaths::for_run(Path::new(&fixture.er_dir), id);
        let mut run = load_run(&source).unwrap();
        run.id = id.into();
        run.status = status;
        save_run(&paths, &run).unwrap();
        paths
    }

    #[test]
    fn recovery_noninitial_tab_bucket_unblocks_guarded_acceptance() {
        let mut fixture = fixture();
        let storage = fixture.repo.path().to_path_buf();
        crate::storage::with_test_storage_root(&storage, || {
            let paths = historical_run(&fixture, "interrupted", RunStatus::Running { round: 1 });
            let captured = fixture.app.tabs.remove(0);
            fixture
                .app
                .tabs
                .push(crate::app::TabState::new_for_test(Vec::new()));
            fixture.app.reconcile_arena_runs();
            // Startup visited the other bucket. Enter the captured target later.
            fixture.app.tabs.push(captured);
            fixture.app.select_tab(1);
            assert!(matches!(
                load_run(&paths).unwrap().status,
                RunStatus::Failed
            ));
            assert_eq!(
                fixture
                    .app
                    .accept_review_flow_findings(
                        &fixture.er_dir,
                        &fixture.run_id,
                        vec!["kept".into()]
                    )
                    .unwrap(),
                1
            );
            assert!(matches!(
                load_run(&paths).unwrap().status,
                RunStatus::Failed
            ));
        });
    }

    #[test]
    fn recovery_noninitial_mode_bucket_history_unblocks_guarded_acceptance() {
        let mut fixture = fixture();
        let storage = fixture.repo.path().to_path_buf();
        crate::storage::with_test_storage_root(&storage, || {
            let paths = historical_run(&fixture, "interrupted", RunStatus::Queued);
            let captured_root = fixture.app.tab().er_root.clone();
            fixture.app.tab_mut().mode = DiffMode::Staged;
            fixture.app.tab_mut().er_root = ErRoot::managed(format!(
                "{}/.er",
                fixture.repo.path().join("other-view").to_string_lossy()
            ));
            fixture.app.reconcile_arena_runs();
            fixture.app.tab_mut().mode = DiffMode::Branch;
            fixture.app.tab_mut().er_root = captured_root;
            let before = std::fs::read(paths.run_json()).unwrap();
            fixture.app.arena_list_summaries(None).unwrap();
            assert_eq!(std::fs::read(paths.run_json()).unwrap(), before);
            assert_eq!(
                fixture
                    .app
                    .accept_review_flow_findings(
                        &fixture.er_dir,
                        &fixture.run_id,
                        vec!["kept".into()]
                    )
                    .unwrap(),
                1
            );
            assert!(matches!(
                load_run(&paths).unwrap().status,
                RunStatus::Failed
            ));
        });
    }

    #[test]
    fn recovery_preserves_registered_running_and_terminal_writers() {
        for status in [RunStatus::Running { round: 1 }, RunStatus::Cancelled] {
            let mut fixture = fixture();
            let storage = fixture.repo.path().to_path_buf();
            crate::storage::with_test_storage_root(&storage, || {
                let paths = historical_run(&fixture, "owned", RunStatus::Running { round: 1 });
                let before = std::fs::read(paths.run_json()).unwrap();
                fixture
                    .app
                    .arena_runs
                    .insert_for_test("owned", &fixture.er_dir, status);
                // The control map is authoritative for live writers.
                fixture.app.reconcile_arena_runs();
                fixture.app.arena_list_summaries(None).unwrap();
                assert_eq!(std::fs::read(paths.run_json()).unwrap(), before);
                let lease = fixture
                    .app
                    .acquire_review_flow_lease(&fixture.er_dir)
                    .unwrap();
                let error = fixture
                    .app
                    .accept_review_flow_findings(
                        &fixture.er_dir,
                        &fixture.run_id,
                        vec!["kept".into()],
                    )
                    .unwrap_err();
                assert!(
                    error
                        .to_string()
                        .contains("A review flow is already using this review bucket"),
                    "{error:#}"
                );
                fixture.app.release_review_flow_lease(&lease).unwrap();
                fixture.app.arena_runs.remove("owned");
            });
        }
    }

    #[test]
    fn recovery_incomplete_historical_manifests_allow_guarded_acceptance() {
        for malformed in [false, true] {
            let mut fixture = fixture();
            let storage = fixture.repo.path().to_path_buf();
            crate::storage::with_test_storage_root(&storage, || {
                let paths = ArenaPaths::for_run(Path::new(&fixture.er_dir), "orphan");
                paths.ensure_dirs().unwrap();
                std::fs::write(paths.diff_patch(), "recoverable input").unwrap();
                if malformed {
                    std::fs::write(paths.run_json(), "{broken").unwrap();
                }
                assert_eq!(
                    fixture
                        .app
                        .accept_review_flow_findings(
                            &fixture.er_dir,
                            &fixture.run_id,
                            vec!["kept".into()]
                        )
                        .unwrap(),
                    1
                );
                assert_eq!(
                    std::fs::read_to_string(paths.diff_patch()).unwrap(),
                    "recoverable input"
                );
            });
        }
    }

    #[test]
    fn recovery_registered_incomplete_manifests_still_block_acceptance() {
        for malformed in [false, true] {
            let mut fixture = fixture();
            let storage = fixture.repo.path().to_path_buf();
            crate::storage::with_test_storage_root(&storage, || {
                let paths = ArenaPaths::for_run(Path::new(&fixture.er_dir), "owned-orphan");
                paths.ensure_dirs().unwrap();
                if malformed {
                    std::fs::write(paths.run_json(), "{broken").unwrap();
                }
                let lease = fixture
                    .app
                    .acquire_review_flow_lease(&fixture.er_dir)
                    .unwrap();
                let error = fixture
                    .app
                    .accept_review_flow_findings(
                        &fixture.er_dir,
                        &fixture.run_id,
                        vec!["kept".into()],
                    )
                    .unwrap_err();
                assert!(
                    error
                        .to_string()
                        .contains("A review flow is already using this review bucket"),
                    "{error:#}"
                );
                fixture.app.release_review_flow_lease(&lease).unwrap();
            });
        }
    }

    #[cfg(unix)]
    #[test]
    fn recovery_never_writes_through_manifest_or_temporary_symlinks() {
        use std::os::unix::fs::symlink;
        for temporary in [false, true] {
            let mut fixture = fixture();
            let storage = fixture.repo.path().to_path_buf();
            crate::storage::with_test_storage_root(&storage, || {
                let paths = historical_run(&fixture, "interrupted", RunStatus::Queued);
                let external = fixture.repo.path().join("outside.json");
                let before = std::fs::read(paths.run_json()).unwrap();
                std::fs::write(&external, &before).unwrap();
                let destination = if temporary {
                    // The historical fixed name: `run.json` with its extension
                    // replaced, as `with_extension("tmp")` produced.
                    paths.run_json().with_file_name("run.tmp")
                } else {
                    std::fs::remove_file(paths.run_json()).unwrap();
                    paths.run_json()
                };
                symlink(&external, &destination).unwrap();
                let error = fixture
                    .app
                    .accept_review_flow_findings(
                        &fixture.er_dir,
                        &fixture.run_id,
                        vec!["kept".into()],
                    )
                    .unwrap_err();
                assert!(error.to_string().contains("symlink"), "{error:#}");
                assert_eq!(std::fs::read(&external).unwrap(), before);
                let lease = fixture
                    .app
                    .acquire_review_flow_lease(&fixture.er_dir)
                    .unwrap();
                let error = fixture
                    .app
                    .accept_review_flow_findings(
                        &fixture.er_dir,
                        &fixture.run_id,
                        vec!["kept".into()],
                    )
                    .unwrap_err();
                assert!(
                    error
                        .to_string()
                        .contains("A review flow is already using this review bucket"),
                    "{error:#}"
                );
                fixture.app.release_review_flow_lease(&lease).unwrap();
            });
        }
    }

    #[test]
    fn recovery_manifest_directory_io_error_is_not_silenced() {
        let mut fixture = fixture();
        let storage = fixture.repo.path().to_path_buf();
        crate::storage::with_test_storage_root(&storage, || {
            let paths = ArenaPaths::for_run(Path::new(&fixture.er_dir), "unreadable");
            paths.ensure_dirs().unwrap();
            std::fs::create_dir(paths.run_json()).unwrap();
            let error = fixture
                .app
                .accept_review_flow_findings(&fixture.er_dir, &fixture.run_id, vec!["kept".into()])
                .unwrap_err();
            assert!(error
                .chain()
                .any(|cause| cause.downcast_ref::<std::io::Error>().is_some()));
        });
    }

    #[test]
    fn review_artifacts_do_not_change_diff_without_global_git_ignores() {
        let fixture = fixture();
        let root = fixture.repo.path();
        let excludes = root.join(".git/empty-global-excludes");
        std::fs::write(&excludes, "").unwrap();
        git(
            root,
            &["config", "core.excludesFile", excludes.to_str().unwrap()],
        );

        let raw = fixture.app.tabs[0].fetch_tab_raw_diff("branch").unwrap();
        let run = load_run(&ArenaPaths::for_run(
            Path::new(&fixture.er_dir),
            &fixture.run_id,
        ))
        .unwrap();
        assert_eq!(crate::ai::compute_diff_hash(&raw), run.diff_hash);
        assert!(!raw.contains(".er/"));
    }

    #[test]
    fn opening_and_closing_results_is_read_only_and_run_identity_is_explicit() {
        let fixture = fixture();
        let paths = ArenaPaths::for_run(Path::new(&fixture.er_dir), &fixture.run_id);
        let run_before = std::fs::read(paths.run_json()).unwrap();
        let results = fixture
            .app
            .review_flow_results(&fixture.er_dir, &fixture.run_id)
            .unwrap();
        assert_eq!(results.findings.len(), 1);
        assert_eq!(results.findings[0].id, "kept");
        assert!(results.findings[0].selectable);
        assert!(!Path::new(&fixture.er_dir).join("review.json").exists());
        drop(results);
        assert_eq!(std::fs::read(paths.run_json()).unwrap(), run_before);
        assert!(fixture
            .app
            .review_flow_results(&fixture.er_dir, "unknown")
            .is_err());

        let mut run = load_run(&paths).unwrap();
        run.id = "wrong-generation".into();
        save_run(&paths, &run).unwrap();
        assert!(fixture
            .app
            .review_flow_results(&fixture.er_dir, &fixture.run_id)
            .is_err());
    }

    #[test]
    fn generic_arena_acceptance_cannot_import_stale_flow_findings() {
        let mut fixture = fixture();
        let storage = fixture.repo.path().to_path_buf();
        crate::storage::with_test_storage_root(&storage, || {
            std::fs::write(
                fixture.repo.path().join("file.txt"),
                "new review generation\n",
            )
            .unwrap();
            let review_path = Path::new(&fixture.er_dir).join("review.json");
            crate::test_support::write_sidecar_json::<crate::ai::ErReview>(
                Path::new(&fixture.er_dir),
                json!({"version": 1, "diff_hash": "new-review-generation", "files": {}}),
            );
            let review = std::fs::read(&review_path).unwrap();
            let paths = ArenaPaths::for_run(Path::new(&fixture.er_dir), &fixture.run_id);
            let run_before = std::fs::read(paths.run_json()).unwrap();

            let result = fixture
                .app
                .arena_accept_findings(&fixture.run_id, Some(vec!["kept".into()]));

            assert!(
                result.is_err(),
                "stale flow findings must use guarded acceptance"
            );
            assert_eq!(std::fs::read(&review_path).unwrap(), review);
            assert_eq!(std::fs::read(paths.run_json()).unwrap(), run_before);
        });
    }

    #[test]
    fn persisted_flow_run_cannot_use_generic_acceptance_after_restart() {
        let mut fixture = fixture();
        let storage = fixture.repo.path().to_path_buf();
        crate::storage::with_test_storage_root(&storage, || {
            let paths = ArenaPaths::for_run(Path::new(&fixture.er_dir), &fixture.run_id);
            let mut run = load_run(&paths).unwrap();
            run.config.run_kind = ArenaRunKind::ReviewFlow;
            save_run(&paths, &run).unwrap();
            fixture.app.review_flow_run_targets.clear();
            fixture.app.review_flows.clear();
            let run_before = std::fs::read(paths.run_json()).unwrap();

            let result = fixture
                .app
                .arena_accept_findings(&fixture.run_id, Some(vec!["kept".into()]));

            assert!(
                result.is_err(),
                "flow ownership must survive loss of in-memory context"
            );
            assert!(!Path::new(&fixture.er_dir).join("review.json").exists());
            assert_eq!(std::fs::read(paths.run_json()).unwrap(), run_before);
        });
    }

    #[test]
    fn history_snapshot_marks_legacy_deep_without_relabelling_seeded_verification() {
        let fixture = fixture();
        let snapshot = fixture.app.arena_get_snapshot(&fixture.run_id).unwrap();
        assert_eq!(snapshot.run.config.run_kind, ArenaRunKind::ReviewFlow);

        let paths = ArenaPaths::for_run(Path::new(&fixture.er_dir), &fixture.run_id);
        let mut run = load_run(&paths).unwrap();
        assert_eq!(run.config.run_kind, ArenaRunKind::Models);
        run.config.run_kind = ArenaRunKind::Seeded;
        save_run(&paths, &run).unwrap();
        let run_before = std::fs::read(paths.run_json()).unwrap();
        let snapshot = fixture.app.arena_get_snapshot(&fixture.run_id).unwrap();
        assert_eq!(snapshot.run.config.run_kind, ArenaRunKind::Seeded);
        assert_eq!(std::fs::read(paths.run_json()).unwrap(), run_before);
    }

    fn seed_old_overlay(fixture: &Fixture) -> Vec<u8> {
        let paths = ArenaPaths::for_run(Path::new(&fixture.er_dir), &fixture.run_id);
        let mut run = load_run(&paths).unwrap();
        run.config.agent_kind = Some("general".into());
        run.reviewers.push(serde_json::from_value(json!({
            "id":"deep-finder","name":"Deep finder","kind":"model","provider_id":"deep-provider","model_id":"test",
            "system_prompt":"","color":"","icon":"","tagline":"","cost_per_1k_in":0.0,"cost_per_1k_out":0.0,"avg_latency_ms":0,"status":"ok","agent_kind":"general"
        })).unwrap());
        run.findings[0].raised_by = vec!["deep-finder".into()];
        save_run(&paths, &run).unwrap();
        crate::test_support::write_sidecar_json::<crate::ai::ArbiterReview>(
            Path::new(&fixture.er_dir),
            json!({
                "version": 1, "diff_hash": run.diff_hash, "run_id": "old-quick",
                "verdicts": [
                    {"id": crate::ai::finding_key("file.txt", Some(2), "Kept finding"), "file":"file.txt", "verdict":"dropped", "raised_by":["security"], "providers":["old-provider"]},
                    {"id": crate::ai::finding_key("file.txt", Some(1), "Unrelated finding"), "file":"file.txt", "verdict":"dropped"}
                ]
            }),
        );
        let bytes = std::fs::read(Path::new(&fixture.er_dir).join("arbiter.json")).unwrap();
        crate::test_support::write_sidecar_json::<crate::ai::ErReview>(
            Path::new(&fixture.er_dir),
            json!({
                "version":1,"diff_hash":run.diff_hash,"files":{"file.txt":{"risk":"medium","findings":[
                    {"id":"unrelated","severity":"medium","title":"Unrelated finding","line_start":1},
                    {"id":"producer","severity":"medium","title":"Kept finding","line_start":2}
                ]}}
            }),
        );
        bytes
    }

    #[test]
    fn acceptance_supersedes_old_overlay_but_allows_later_verification() {
        let mut fixture = fixture();
        let storage = fixture.repo.path().to_path_buf();
        crate::storage::with_test_storage_root(&storage, || {
            seed_old_overlay(&fixture);
            fixture
                .app
                .accept_review_flow_findings(&fixture.er_dir, &fixture.run_id, vec!["kept".into()])
                .unwrap();
            let paths = ArenaPaths::for_run(Path::new(&fixture.er_dir), &fixture.run_id);
            let run = load_run(&paths).unwrap();
            let state = crate::ai::load_ai_state(&fixture.er_dir, &run.diff_hash, None);
            let review = state.review.unwrap();
            let rows = &review.files["file.txt"].findings;
            assert_eq!(
                crate::ai::project_visible_findings(
                    rows.iter()
                        .filter(|f| f.is_active())
                        .map(|f| ("file.txt", f)),
                    &state.arbiter_graded_keys
                )
                .len(),
                1
            );
            let accepted = rows.iter().find(|f| f.id == "arena-flow-run-kept").unwrap();
            assert!(
                accepted.is_active(),
                "old Quick verdict must not hide accepted Deep findings"
            );
            assert_eq!(accepted.raised_by, vec!["general"]);
            assert_eq!(accepted.providers, vec!["deep-provider"]);
            assert!(!rows
                .iter()
                .find(|f| f.id == "unrelated")
                .unwrap()
                .is_active());
            let mut later = crate::ai::load_arbiter_review(&fixture.er_dir).unwrap();
            assert!(
                later.run_id.is_empty(),
                "mixed rulings do not belong to the old Quick run"
            );
            assert!(later.created_at.is_empty());
            later.run_id = "later-verification".into();
            later.verdicts.push(
            serde_json::from_value(json!({
                "id":crate::ai::finding_key("file.txt", Some(2), "Kept finding"),"verdict":"dropped"
            }))
            .unwrap(),
        );
            crate::ai::write_arbiter_review(&fixture.er_dir, &later).unwrap();
            let state = crate::ai::load_ai_state(&fixture.er_dir, &run.diff_hash, None);
            assert!(!state.review.unwrap().files["file.txt"]
                .findings
                .iter()
                .find(|f| f.id == "arena-flow-run-kept")
                .unwrap()
                .is_active());
        });
    }

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

    /// Make the run's own save fail: its directory becomes read-only, so
    /// `save_run` cannot create its temporary file. The bucket stays writable
    /// for the review and arbiter sidecars the rollback restores.
    fn fail_run_save(paths: &ArenaPaths) -> RestoreRunDirPermissions {
        let permissions = std::fs::metadata(&paths.root).unwrap().permissions();
        let mut read_only = permissions.clone();
        read_only.set_readonly(true);
        std::fs::set_permissions(&paths.root, read_only).unwrap();
        RestoreRunDirPermissions {
            path: paths.root.clone(),
            permissions,
        }
    }

    struct RestoreRunDirPermissions {
        path: std::path::PathBuf,
        permissions: std::fs::Permissions,
    }

    impl Drop for RestoreRunDirPermissions {
        fn drop(&mut self) {
            let _ = std::fs::set_permissions(&self.path, self.permissions.clone());
        }
    }

    #[test]
    fn run_save_failure_restores_superseded_overlay_byte_for_byte() {
        let mut fixture = fixture();
        let storage = fixture.repo.path().to_path_buf();
        crate::storage::with_test_storage_root(&storage, || {
            let original = seed_old_overlay(&fixture);
            let paths = ArenaPaths::for_run(Path::new(&fixture.er_dir), &fixture.run_id);
            let _fail_save = fail_run_save(&paths);
            assert!(fixture
                .app
                .accept_review_flow_findings(&fixture.er_dir, &fixture.run_id, vec!["kept".into()])
                .is_err());
            assert_eq!(
                std::fs::read(Path::new(&fixture.er_dir).join("arbiter.json")).unwrap(),
                original
            );
            let archived = Path::new(&fixture.er_dir)
                .join("flow-backups")
                .read_dir()
                .unwrap()
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.path().join("arbiter.json"))
                .find(|path| path.is_file())
                .expect("original overlay remains recoverable");
            assert_eq!(std::fs::read(archived).unwrap(), original);
        });
    }

    #[test]
    fn acceptance_after_tab_switch_uses_captured_bucket_and_leaves_review_alone() {
        let mut fixture = fixture();
        let storage = fixture.repo.path().to_path_buf();
        crate::storage::with_test_storage_root(&storage, || {
            let review_path = Path::new(&fixture.er_dir).join("review.json");
            crate::test_support::write_sidecar_json::<crate::ai::ErReview>(
                Path::new(&fixture.er_dir),
                json!({"version":1,"diff_hash":"old-generation","files":{"legacy.rs":{"risk":"medium","findings":[{"id":"legacy","severity":"medium","title":"Old generation finding"}]}}}),
            );
            let old_review = std::fs::read(&review_path).unwrap();
            fixture
                .app
                .tabs
                .push(crate::app::TabState::new_for_test(Vec::new()));
            fixture.app.active_tab = 1;

            let accepted = fixture
                .app
                .accept_review_flow_findings(&fixture.er_dir, &fixture.run_id, vec!["kept".into()])
                .unwrap();
            assert_eq!(accepted, 1);
            assert_eq!(
                std::fs::read(&review_path).unwrap(),
                old_review,
                "acceptance writes the artifact, not review.json"
            );
            let artifact: crate::ai::accepted_findings::AcceptedFindings =
                crate::test_support::read_sidecar(Path::new(&fixture.er_dir));
            assert_eq!(artifact.findings.len(), 1);
            let row = &artifact.findings[0];
            assert_eq!(row.finding.id, format!("arena-{}-kept", fixture.run_id));
            assert_eq!(row.run_finding_id, "kept");
            assert_eq!(row.run_id, fixture.run_id);
            assert_eq!(row.file, "file.txt");
            let run = load_run(&ArenaPaths::for_run(
                Path::new(&fixture.er_dir),
                &fixture.run_id,
            ))
            .unwrap();
            assert_eq!(run.accepted_finding_ids, vec!["kept"]);
            assert!(matches!(
                &fixture.app.review_flows[&fixture.er_dir].state,
                FlowState::Done {
                    awaiting_accept: 0,
                    ..
                }
            ));
        });
    }

    /// Acceptance used to re-stamp the general review's diff hash with the
    /// run's, marking a review from another generation as matching this diff
    /// (ADR 0035). The accepted row is visible and the review stays stale.
    #[test]
    fn acceptance_does_not_restamp_the_general_review() {
        let mut fixture = fixture();
        let storage = fixture.repo.path().to_path_buf();
        crate::storage::with_test_storage_root(&storage, || {
            let review_path = Path::new(&fixture.er_dir).join("review.json");
            crate::test_support::write_sidecar_json::<crate::ai::ErReview>(
                Path::new(&fixture.er_dir),
                json!({
                    "version": 1,
                    "diff_hash": "older-generation",
                    "files": {"file.txt": {"risk":"low","findings":[{"id":"older","severity":"low","title":"Older generation finding"}]}}
                }),
            );
            let before = std::fs::read(&review_path).unwrap();
            fixture
                .app
                .accept_review_flow_findings(&fixture.er_dir, &fixture.run_id, vec!["kept".into()])
                .unwrap();
            assert_eq!(
                std::fs::read(&review_path).unwrap(),
                before,
                "acceptance leaves review.json byte-identical"
            );
            let run = load_run(&ArenaPaths::for_run(
                Path::new(&fixture.er_dir),
                &fixture.run_id,
            ))
            .unwrap();
            let state = crate::ai::load_ai_state(&fixture.er_dir, &run.diff_hash, None);
            let review = state.review.expect("the general review still loads");
            assert_eq!(
                review.diff_hash, "older-generation",
                "the general review keeps its generation"
            );
            assert!(state.is_stale, "another generation's review stays stale");
            assert!(
                review.files["file.txt"]
                    .findings
                    .iter()
                    .any(|finding| finding.id == format!("arena-{}-kept", fixture.run_id)),
                "the accepted row is projected alongside the older review"
            );
        });
    }

    #[test]
    fn acceptance_rejects_dropped_unknown_changed_and_stale_selection() {
        let mut fixture = fixture();
        let storage = fixture.repo.path().to_path_buf();
        crate::storage::with_test_storage_root(&storage, || {
            assert!(fixture
                .app
                .accept_review_flow_findings(
                    &fixture.er_dir,
                    &fixture.run_id,
                    vec!["dropped".into()]
                )
                .is_err());
            assert!(fixture
                .app
                .accept_review_flow_findings(
                    &fixture.er_dir,
                    &fixture.run_id,
                    vec!["missing".into()]
                )
                .is_err());
            assert!(!Path::new(&fixture.er_dir).join("review.json").exists());

            fixture
                .app
                .review_flow_results(&fixture.er_dir, &fixture.run_id)
                .unwrap();
            fixture
                .app
                .review_flows
                .get_mut(&fixture.er_dir)
                .unwrap()
                .arena_run_id = Some("new-flow-run".into());
            assert!(fixture
                .app
                .accept_review_flow_findings(&fixture.er_dir, &fixture.run_id, vec!["kept".into()])
                .is_err());
        });
    }

    #[test]
    fn acceptance_rejects_a_changed_working_diff() {
        let mut fixture = fixture();
        let storage = fixture.repo.path().to_path_buf();
        crate::storage::with_test_storage_root(&storage, || {
            let repo = Path::new(&fixture.app.tabs[0].repo_root);
            std::fs::write(repo.join("file.txt"), "changed after review\n").unwrap();
            assert!(fixture
                .app
                .accept_review_flow_findings(&fixture.er_dir, &fixture.run_id, vec!["kept".into()])
                .is_err());
            assert!(!Path::new(&fixture.er_dir).join("review.json").exists());
        });
    }

    #[test]
    fn acceptance_refuses_when_captured_target_is_closed() {
        let mut fixture = fixture();
        let storage = fixture.repo.path().to_path_buf();
        crate::storage::with_test_storage_root(&storage, || {
            fixture.app.tabs.clear();
            assert!(fixture
                .app
                .accept_review_flow_findings(&fixture.er_dir, &fixture.run_id, vec!["kept".into()])
                .is_err());
            assert!(!Path::new(&fixture.er_dir).join("review.json").exists());
        });
    }

    #[test]
    fn run_save_failure_rolls_back_a_new_accepted_findings_artifact() {
        let mut fixture = fixture();
        let storage = fixture.repo.path().to_path_buf();
        crate::storage::with_test_storage_root(&storage, || {
            let paths = ArenaPaths::for_run(Path::new(&fixture.er_dir), &fixture.run_id);
            let _fail_save = fail_run_save(&paths);

            let error = fixture
                .app
                .accept_review_flow_findings(&fixture.er_dir, &fixture.run_id, vec!["kept".into()])
                .unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("accepted IDs could not be saved to the run"),
                "{error:#}"
            );
            assert!(
                !Path::new(&fixture.er_dir)
                    .join("accepted-findings.json")
                    .exists(),
                "a failed run save removes the artifact it wrote"
            );
            assert!(!Path::new(&fixture.er_dir).join("review.json").exists());
            assert!(load_run(&paths).unwrap().accepted_finding_ids.is_empty());
        });
    }

    #[test]
    fn run_save_failure_restores_a_previous_accepted_findings_artifact_byte_for_byte() {
        let mut fixture = fixture();
        let storage = fixture.repo.path().to_path_buf();
        crate::storage::with_test_storage_root(&storage, || {
            let paths = ArenaPaths::for_run(Path::new(&fixture.er_dir), &fixture.run_id);
            crate::test_support::write_sidecar_json::<crate::ai::accepted_findings::AcceptedFindings>(
                Path::new(&fixture.er_dir),
                json!({
                    "version": 1,
                    "findings": [{
                        "file": "legacy.rs",
                        "run_finding_id": "legacy",
                        "run_id": "older-run",
                        "run_diff_hash": "older",
                        "accepted_at": "t0",
                        "providers": [],
                        "finding": {"id": "arena-older-run-legacy", "severity": "medium", "title": "Older finding"}
                    }]
                }),
            );
            let artifact_path = Path::new(&fixture.er_dir).join("accepted-findings.json");
            let original = std::fs::read(&artifact_path).unwrap();
            let _fail_save = fail_run_save(&paths);

            let error = fixture
                .app
                .accept_review_flow_findings(&fixture.er_dir, &fixture.run_id, vec!["kept".into()])
                .unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("accepted IDs could not be saved to the run"),
                "{error:#}"
            );
            assert_eq!(std::fs::read(artifact_path).unwrap(), original);
            assert!(load_run(&paths).unwrap().accepted_finding_ids.is_empty());
        });
    }
}
