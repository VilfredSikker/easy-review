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

fn atomic_write_review(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .context("review.json has no parent directory")?;
    std::fs::create_dir_all(parent).context("create review bucket")?;
    let sequence = ACCEPT_ARCHIVE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temp = parent.join(format!(
        "review.json.rollback-{}-{sequence}.tmp",
        std::process::id()
    ));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .context("create review rollback file")?;
    use std::io::Write;
    if let Err(error) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = std::fs::remove_file(&temp);
        return Err(error).context("write review rollback file");
    }
    std::fs::rename(&temp, path).with_context(|| {
        let _ = std::fs::remove_file(&temp);
        "atomically restore review.json"
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
        atomic_write_review(review_path, original)
    } else {
        std::fs::remove_file(review_path).context("remove uncommitted review.json")
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
        if let Err(error) = save_run(&paths, &run) {
            return match rollback_review_acceptance(&review_path, &post_import, original.as_deref()) {
                Ok(()) => Err(error).context(
                    "accepted IDs could not be saved to the run; review.json import was rolled back",
                ),
                Err(rollback_error) => Err(error).context(format!(
                    "accepted IDs could not be saved and review.json rollback failed: {rollback_error}; recovery archive is {}",
                    archive_dir.display()
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
        _repo: TempDir,
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
            _repo: repo,
            app,
            er_dir,
            run_id,
        }
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

        assert!(fixture
            .app
            .accept_review_flow_findings(&fixture.er_dir, &fixture.run_id, vec!["kept".into()])
            .is_err());
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

        assert!(fixture
            .app
            .accept_review_flow_findings(&fixture.er_dir, &fixture.run_id, vec!["kept".into()])
            .is_err());
        assert_eq!(std::fs::read(review_path).unwrap(), original);
        assert!(load_run(&paths).unwrap().accepted_finding_ids.is_empty());
    }
}
