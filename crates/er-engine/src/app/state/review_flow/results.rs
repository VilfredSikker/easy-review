//! Read-only projection and explicit acceptance for review-flow arena results.

use super::super::background::BackgroundTaskTarget;
use super::super::{App, DiffMode};
use crate::ai::accepted_provenance::invalidate_accepted_provenance;
use crate::ai::review_flow::FlowState;
use crate::app::state::review_flow_output::{archive_flow_output, FlowProducer};
use crate::arena::{
    import_arena_findings_to_review, load_run, save_run, ArenaPaths, ArenaRun, ArenaRunKind,
    RunStatus, Verdict,
};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static ACCEPT_ARCHIVE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

fn atomic_write_sidecar(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("sidecar has no parent directory")?;
    std::fs::create_dir_all(parent).context("create review bucket")?;
    let sequence = ACCEPT_ARCHIVE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temp = parent.join(format!(
        "{}.rollback-{}-{sequence}.tmp",
        path.file_name()
            .context("sidecar has no file name")?
            .to_string_lossy(),
        std::process::id()
    ));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .context("create sidecar rollback file")?;
    use std::io::Write;
    if let Err(error) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = std::fs::remove_file(&temp);
        return Err(error).context("write sidecar rollback file");
    }
    std::fs::rename(&temp, path).with_context(|| {
        let _ = std::fs::remove_file(&temp);
        "atomically replace sidecar"
    })
}

fn rollback_review_acceptance(
    review_path: &Path,
    expected_post_import: &[u8],
    original: Option<&[u8]>,
) -> Result<()> {
    let current = std::fs::read(review_path).context("read review.json before rollback")?;
    if current != expected_post_import {
        bail!("review.json changed after import; refusing to overwrite the newer file");
    }
    if let Some(original) = original {
        atomic_write_sidecar(review_path, original)
    } else {
        std::fs::remove_file(review_path).context("remove uncommitted review.json")
    }
}

/// Accepted Deep rulings supersede older grades for exactly the accepted keys.
/// Keep a byte snapshot so failure can restore both sidecars without clobbering
/// an external writer that replaced either one during acceptance.
struct AcceptedOverlay {
    original: Vec<u8>,
    replacement: Vec<u8>,
}

impl AcceptedOverlay {
    fn prepare(er_dir: &Path, run: &ArenaRun, selected: &[String]) -> Result<Option<Self>> {
        let path = er_dir.join("arbiter.json");
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error).context("inspect arbiter overlay before acceptance"),
        };
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            bail!("arbiter.json must be a regular file for acceptance");
        }
        const LIMIT: u64 = 10_000_000;
        if metadata.len() > LIMIT {
            bail!("arbiter.json is too large for recoverable acceptance");
        }
        use std::io::Read;
        let mut original = Vec::new();
        std::fs::File::open(&path)?
            .take(LIMIT + 1)
            .read_to_end(&mut original)?;
        if original.len() as u64 > LIMIT {
            bail!("arbiter.json grew beyond the acceptance size limit");
        }
        let Ok(mut overlay) = serde_json::from_slice::<crate::ai::ArbiterReview>(&original) else {
            return Ok(None);
        };
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
            original,
            replacement: serde_json::to_vec_pretty(&overlay)?,
        }))
    }

    fn replace_if_unchanged(path: &Path, expected: &[u8], replacement: &[u8]) -> Result<()> {
        let metadata =
            std::fs::symlink_metadata(path).context("inspect arbiter.json before replacement")?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            bail!("arbiter.json is no longer a regular file");
        }
        if metadata.len() != expected.len() as u64 {
            bail!("arbiter.json changed during acceptance; refusing to overwrite the newer file");
        }
        use std::io::Read;
        let mut current = Vec::new();
        std::fs::File::open(path)?
            .take(expected.len() as u64 + 1)
            .read_to_end(&mut current)?;
        if current != expected {
            bail!("arbiter.json changed during acceptance; refusing to overwrite the newer file");
        }
        atomic_write_sidecar(path, replacement)
    }

    fn archive(&self, archive_dir: &Path) -> Result<()> {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(archive_dir.join("arbiter.json"))
            .context("archive superseded arbiter verdicts")?;
        file.write_all(&self.original)?;
        file.sync_all().context("persist archived arbiter verdicts")
    }
}

fn restore_after_import_error(
    review_path: &Path,
    archive: &crate::app::state::review_flow_output::FlowOutputBackup,
    same_generation: bool,
    original: Option<&[u8]>,
) -> Result<()> {
    match std::fs::read(review_path) {
        Ok(current) if same_generation && original == Some(current.as_slice()) => Ok(()),
        Ok(_) => bail!("review.json changed during failed import; leaving it in place"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => archive.restore(),
        Err(error) => Err(error).context("inspect review.json after import failure"),
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

        let overlay = AcceptedOverlay::prepare(Path::new(er_dir), &run, &finding_ids)?;
        let overlay_path = Path::new(er_dir).join("arbiter.json");
        let review_path = Path::new(er_dir).join("review.json");
        let prior_hash = std::fs::read(&review_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<crate::ai::ErReview>(&bytes).ok())
            .map(|review| review.diff_hash);
        let same_generation = prior_hash.as_deref() == Some(run.diff_hash.as_str());
        let sequence = ACCEPT_ARCHIVE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let archive_key = format!("accept-{}-{sequence}", std::process::id());
        let archive_dir = Path::new(er_dir).join("flow-backups").join(&archive_key);
        // Archive every prior state. Same-generation reviews are copied back so
        // the importer can merge them; another generation stays out of the new
        // review until the selected flow findings are written.
        let archive = archive_flow_output(Path::new(er_dir), &archive_key, &FlowProducer::General)?;
        let original = archive
            .archive_path()
            .map(std::fs::read)
            .transpose()
            .context("read the archived review for recoverable acceptance")?;
        if let Some(overlay) = &overlay {
            if let Err(error) = overlay.archive(&archive_dir) {
                archive
                    .restore()
                    .context("restore review after arbiter archive failed")?;
                return Err(error);
            }
        }
        if same_generation {
            archive
                .restore()
                .context("restore the matching review before merging accepted findings")?;
        }

        let imported = match import_arena_findings_to_review(er_dir, &mut run, Some(&finding_ids)) {
            Ok(imported) => imported,
            Err(error) => {
                if let Err(restore_error) = restore_after_import_error(
                    &review_path,
                    &archive,
                    same_generation,
                    original.as_deref(),
                ) {
                    return Err(error).context(format!(
                        "import failed and prior review restoration failed: {restore_error}; recovery archive is {}",
                        archive_dir.display()
                    ));
                }
                return Err(error.context("import selected review-flow findings"));
            }
        };
        let post_import = std::fs::read(&review_path)
            .context("read review.json after importing selected findings")?;
        let mut written_overlay = None;
        let committed = (|| {
            if let Some(overlay) = &overlay {
                AcceptedOverlay::replace_if_unchanged(
                    &overlay_path,
                    &overlay.original,
                    &overlay.replacement,
                )?;
                written_overlay = Some(overlay);
            }
            save_run(&paths, &run)
        })();
        if let Err(error) = committed {
            let review_rollback =
                rollback_review_acceptance(&review_path, &post_import, original.as_deref());
            let overlay_rollback = if let Some(overlay) = written_overlay {
                AcceptedOverlay::replace_if_unchanged(
                    &overlay_path,
                    &overlay.replacement,
                    &overlay.original,
                )
            } else {
                Ok(())
            };
            return match (review_rollback, overlay_rollback) {
                (Ok(()), Ok(())) => Err(error).context(
                    "accepted IDs could not be saved to the run; acceptance sidecars were rolled back",
                ),
                (review, arbiter) => Err(error).context(format!(
                    "accepted IDs could not be saved to the run; rollback failed for review.json {:?} or arbiter.json {:?}; recovery archive is {}",
                    review.err(), arbiter.err(), archive_dir.display()
                )),
            };
        }

        invalidate_accepted_provenance(Path::new(er_dir));
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
        self.notify(&format!("Accepted {imported} finding(s) into Review"));
        Ok(imported)
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
        tab.mode = DiffMode::Branch;
        tab.repo_root.clone_from(&root);
        tab.er_root = ErRoot::RepoLocal(root.clone());
        tab.base_branch = "main".into();
        tab.current_branch = "feature".into();
        tab.local_branch_view = Some("feature".into());
        tab.local_branch_checkout_root = Some(root.clone());
        let er_dir = tab.er_dir();

        let mut app = App::new_for_test(Vec::new());
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
                .accept_review_flow_findings(&fixture.er_dir, &fixture.run_id, vec!["kept".into()])
                .unwrap(),
            1
        );
        assert!(matches!(
            load_run(&paths).unwrap().status,
            RunStatus::Failed
        ));
    }

    #[test]
    fn recovery_noninitial_mode_bucket_history_unblocks_guarded_acceptance() {
        let mut fixture = fixture();
        let paths = historical_run(&fixture, "interrupted", RunStatus::Queued);
        let captured_root = fixture.app.tab().er_root.clone();
        fixture.app.tab_mut().mode = DiffMode::Staged;
        fixture.app.tab_mut().er_root = ErRoot::RepoLocal(
            fixture
                .repo
                .path()
                .join("other-view")
                .to_string_lossy()
                .into_owned(),
        );
        fixture.app.reconcile_arena_runs();
        fixture.app.tab_mut().mode = DiffMode::Branch;
        fixture.app.tab_mut().er_root = captured_root;
        let before = std::fs::read(paths.run_json()).unwrap();
        fixture.app.arena_list_summaries(None).unwrap();
        assert_eq!(std::fs::read(paths.run_json()).unwrap(), before);
        assert_eq!(
            fixture
                .app
                .accept_review_flow_findings(&fixture.er_dir, &fixture.run_id, vec!["kept".into()])
                .unwrap(),
            1
        );
        assert!(matches!(
            load_run(&paths).unwrap().status,
            RunStatus::Failed
        ));
    }

    #[test]
    fn recovery_preserves_registered_running_and_terminal_writers() {
        for status in [RunStatus::Running { round: 1 }, RunStatus::Cancelled] {
            let mut fixture = fixture();
            let paths = historical_run(&fixture, "owned", RunStatus::Running { round: 1 });
            let before = std::fs::read(paths.run_json()).unwrap();
            fixture
                .app
                .arena_registry
                .insert_active_for_test("owned", status);
            // No active_arena_runs index entry. The registry is authoritative.
            fixture.app.reconcile_arena_runs();
            fixture.app.arena_list_summaries(None).unwrap();
            assert_eq!(std::fs::read(paths.run_json()).unwrap(), before);
            assert!(fixture
                .app
                .accept_review_flow_findings(&fixture.er_dir, &fixture.run_id, vec!["kept".into()])
                .is_err());
            fixture.app.arena_registry.take("owned");
        }
    }

    #[test]
    fn recovery_incomplete_historical_manifests_allow_guarded_acceptance() {
        for malformed in [false, true] {
            let mut fixture = fixture();
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
        }
    }

    #[test]
    fn recovery_registered_incomplete_manifests_still_block_acceptance() {
        for malformed in [false, true] {
            let mut fixture = fixture();
            let paths = ArenaPaths::for_run(Path::new(&fixture.er_dir), "owned-orphan");
            paths.ensure_dirs().unwrap();
            if malformed {
                std::fs::write(paths.run_json(), "{broken").unwrap();
            }
            fixture
                .app
                .arena_registry
                .insert_active_for_test("owned-orphan", RunStatus::Cancelled);
            let error = fixture
                .app
                .accept_review_flow_findings(&fixture.er_dir, &fixture.run_id, vec!["kept".into()])
                .unwrap_err();
            assert!(
                error.to_string().contains("already queued or running"),
                "{error:#}"
            );
            fixture.app.arena_registry.take("owned-orphan");
        }
    }

    #[cfg(unix)]
    #[test]
    fn recovery_never_writes_through_manifest_or_temporary_symlinks() {
        use std::os::unix::fs::symlink;
        for temporary in [false, true] {
            let mut fixture = fixture();
            let paths = historical_run(&fixture, "interrupted", RunStatus::Queued);
            let external = fixture.repo.path().join("outside.json");
            let before = std::fs::read(paths.run_json()).unwrap();
            std::fs::write(&external, &before).unwrap();
            let destination = if temporary {
                paths.run_json().with_extension("tmp")
            } else {
                std::fs::remove_file(paths.run_json()).unwrap();
                paths.run_json()
            };
            symlink(&external, &destination).unwrap();
            let error = fixture
                .app
                .accept_review_flow_findings(&fixture.er_dir, &fixture.run_id, vec!["kept".into()])
                .unwrap_err();
            assert!(error.to_string().contains("symlink"), "{error:#}");
            assert_eq!(std::fs::read(&external).unwrap(), before);
            fixture
                .app
                .arena_registry
                .insert_active_for_test("interrupted", RunStatus::Cancelled);
            let error = fixture
                .app
                .accept_review_flow_findings(&fixture.er_dir, &fixture.run_id, vec!["kept".into()])
                .unwrap_err();
            assert!(
                error.to_string().contains("already queued or running"),
                "{error:#}"
            );
            fixture.app.arena_registry.take("interrupted");
        }
    }

    #[test]
    fn recovery_manifest_directory_io_error_is_not_silenced() {
        let mut fixture = fixture();
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
        std::fs::write(
            fixture.repo.path().join("file.txt"),
            "new review generation\n",
        )
        .unwrap();
        let review_path = Path::new(&fixture.er_dir).join("review.json");
        let review = br#"{"version":1,"diff_hash":"new-review-generation","files":{}}"#;
        std::fs::write(&review_path, review).unwrap();
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
    }

    #[test]
    fn persisted_flow_run_cannot_use_generic_acceptance_after_restart() {
        let mut fixture = fixture();
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
        let bytes = serde_json::to_vec(&json!({
            "version": 1, "diff_hash": run.diff_hash, "run_id": "old-quick",
            "verdicts": [
                {"id": crate::ai::finding_key("file.txt", Some(2), "Kept finding"), "file":"file.txt", "verdict":"dropped", "raised_by":["security"], "providers":["old-provider"]},
                {"id": crate::ai::finding_key("file.txt", Some(1), "Unrelated finding"), "file":"file.txt", "verdict":"dropped"}
            ]
        })).unwrap();
        std::fs::write(Path::new(&fixture.er_dir).join("arbiter.json"), &bytes).unwrap();
        std::fs::write(Path::new(&fixture.er_dir).join("review.json"), serde_json::to_vec(&json!({
            "version":1,"diff_hash":run.diff_hash,"files":{"file.txt":{"risk":"medium","findings":[
                {"id":"unrelated","severity":"medium","title":"Unrelated finding","line_start":1},
                {"id":"producer","severity":"medium","title":"Kept finding","line_start":2}
            ]}}
        })).unwrap()).unwrap();
        bytes
    }

    #[test]
    fn acceptance_supersedes_old_overlay_but_allows_later_verification() {
        let mut fixture = fixture();
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
    }

    #[test]
    fn overlay_rollback_preserves_intervening_external_edits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("arbiter.json");
        std::fs::write(&path, b"external replacement").unwrap();
        let error = AcceptedOverlay::replace_if_unchanged(
            &path,
            b"our committed overlay",
            b"original overlay",
        )
        .unwrap_err();
        assert!(error.to_string().contains("refusing to overwrite"));
        assert_eq!(std::fs::read(&path).unwrap(), b"external replacement");
    }

    #[cfg(unix)]
    #[test]
    fn overlay_replacement_refuses_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("external.json");
        std::fs::write(&target, b"original").unwrap();
        let path = dir.path().join("arbiter.json");
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert!(AcceptedOverlay::replace_if_unchanged(&path, b"original", b"replacement").is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"original");
    }

    #[test]
    fn run_save_failure_restores_superseded_overlay_byte_for_byte() {
        let mut fixture = fixture();
        let original = seed_old_overlay(&fixture);
        let paths = ArenaPaths::for_run(Path::new(&fixture.er_dir), &fixture.run_id);
        std::fs::create_dir(paths.run_json().with_extension("tmp")).unwrap();
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
    }

    #[test]
    fn acceptance_after_tab_switch_uses_captured_bucket_and_archives_old_review() {
        let mut fixture = fixture();
        let old_review = br#"{"version":1,"diff_hash":"old-generation","files":{"legacy.rs":{"risk":"medium","findings":[{"id":"legacy","severity":"medium","title":"Old generation finding"}]}}}"#;
        let review_path = Path::new(&fixture.er_dir).join("review.json");
        std::fs::create_dir_all(Path::new(&fixture.er_dir)).unwrap();
        std::fs::write(&review_path, old_review).unwrap();
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
        let review: crate::ai::ErReview =
            serde_json::from_slice(&std::fs::read(&review_path).unwrap()).unwrap();
        assert_eq!(
            review.diff_hash,
            load_run(&ArenaPaths::for_run(
                Path::new(&fixture.er_dir),
                &fixture.run_id
            ))
            .unwrap()
            .diff_hash
        );
        assert_eq!(
            review.files["file.txt"].findings[0].id,
            format!("arena-{}-kept", fixture.run_id)
        );
        let backup_root = review_path.parent().unwrap().join("flow-backups");
        let archived = std::fs::read_dir(backup_root)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path().join("review.json"))
            .find(|path| path.is_file())
            .expect("older review should remain in the recovery archive");
        assert_eq!(std::fs::read(archived).unwrap().as_slice(), old_review);
        assert!(!review.files.contains_key("legacy.rs"));
        assert_eq!(review.files["file.txt"].findings.len(), 1);
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
    }

    #[test]
    fn acceptance_rejects_dropped_unknown_changed_and_stale_selection() {
        let mut fixture = fixture();
        assert!(fixture
            .app
            .accept_review_flow_findings(&fixture.er_dir, &fixture.run_id, vec!["dropped".into()])
            .is_err());
        assert!(fixture
            .app
            .accept_review_flow_findings(&fixture.er_dir, &fixture.run_id, vec!["missing".into()])
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
    }

    #[test]
    fn acceptance_rejects_a_changed_working_diff() {
        let mut fixture = fixture();
        let repo = Path::new(&fixture.app.tabs[0].repo_root);
        std::fs::write(repo.join("file.txt"), "changed after review\n").unwrap();
        assert!(fixture
            .app
            .accept_review_flow_findings(&fixture.er_dir, &fixture.run_id, vec!["kept".into()])
            .is_err());
        assert!(!Path::new(&fixture.er_dir).join("review.json").exists());
    }

    #[test]
    fn acceptance_refuses_when_captured_target_is_closed() {
        let mut fixture = fixture();
        fixture.app.tabs.clear();
        assert!(fixture
            .app
            .accept_review_flow_findings(&fixture.er_dir, &fixture.run_id, vec!["kept".into()])
            .is_err());
        assert!(!Path::new(&fixture.er_dir).join("review.json").exists());
    }

    #[test]
    fn run_save_failure_rolls_back_a_new_review_when_none_existed() {
        let mut fixture = fixture();
        let paths = ArenaPaths::for_run(Path::new(&fixture.er_dir), &fixture.run_id);
        std::fs::create_dir(paths.run_json().with_extension("tmp")).unwrap();

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
        assert!(!Path::new(&fixture.er_dir).join("review.json").exists());
        assert!(load_run(&paths).unwrap().accepted_finding_ids.is_empty());
        assert!(Path::new(&fixture.er_dir)
            .join("flow-backups")
            .read_dir()
            .unwrap()
            .next()
            .is_some());
    }

    #[test]
    fn run_save_failure_restores_same_generation_review_byte_for_byte() {
        let mut fixture = fixture();
        let paths = ArenaPaths::for_run(Path::new(&fixture.er_dir), &fixture.run_id);
        let run = load_run(&paths).unwrap();
        let review_path = Path::new(&fixture.er_dir).join("review.json");
        std::fs::create_dir_all(Path::new(&fixture.er_dir)).unwrap();
        let original = serde_json::to_vec(&json!({
            "version": 1,
            "diff_hash": run.diff_hash,
            "base_branch": "main",
            "head_branch": "feature",
            "files": {"legacy.rs": {"risk":"medium","findings":[{"id":"legacy","severity":"medium","title":"Existing finding"}]}}
        })).unwrap();
        std::fs::write(&review_path, &original).unwrap();
        std::fs::create_dir(paths.run_json().with_extension("tmp")).unwrap();

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
        assert_eq!(std::fs::read(review_path).unwrap(), original);
        assert!(load_run(&paths).unwrap().accepted_finding_ids.is_empty());
    }
}
