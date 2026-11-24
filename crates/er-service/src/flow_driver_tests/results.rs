//! A finished flow's results and their guarded acceptance: the results read
//! writes nothing, acceptance takes only kept findings of the flow's current
//! run, checks the fresh diff the command carries, recovers the bucket's
//! stale history first and rolls back when the run cannot be saved. Moved
//! from the driver's `results.rs`, where they ran on the App.

use std::path::Path;
use std::process::Command;

use er_engine::ai::review_flow::{FlowState, ReviewFlow};
use er_engine::arena::{load_run, save_run, ArenaPaths, ArenaRun, ArenaRunKind, RunStatus};
use er_engine::config::ErConfig;
use er_engine::ports::Ports;
use er_engine::review_flow_driver::{filtered_review_diff, ReviewFlowHost, ReviewFlowResults};
use er_engine::tab_identity::{DiffMode, TabIdentity};
use er_engine::task_target::BackgroundTaskTarget;
use serde_json::json;
use tempfile::TempDir;

use super::Harness;
use crate::protocol::AcceptanceDiff;

struct Fixture {
    repo: TempDir,
    harness: Harness,
    target: BackgroundTaskTarget,
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

/// The diff the open view of the target shows now, read by the rules the
/// desktop reads it by before it sends an acceptance (`current_review_diff`).
fn fresh_diff(target: &BackgroundTaskTarget) -> String {
    let id = TabIdentity {
        repo_root: &target.repo_root,
        current_branch: &target.branch_label,
        base_branch: &target.base_branch,
        mode: DiffMode::Branch,
        tour_is_pr: false,
        remote_repo: None,
        pr_number: None,
        local_branch_view: Some(&target.branch_label),
        pr_head_ref: None,
        checkout_root: Some(&target.repo_root),
    };
    let raw = er_engine::diff_load::fetch_raw(&Ports::cli(), &id, "branch", false).unwrap();
    filtered_review_diff(&raw, &target.repo_root, None)
}

/// A `feature` branch one commit ahead of `main`, and a finished flow whose
/// run kept one finding and dropped another.
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
    let er_dir = format!("{root}/.er");
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
    let hash = er_engine::ai::compute_diff_hash(&fresh_diff(&target));
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
    }))
    .unwrap();
    save_run(&ArenaPaths::for_run(Path::new(&er_dir), &run_id), &run).unwrap();

    let mut flow = ReviewFlow::new(hash);
    flow.arena_run_id = Some(run_id.clone());
    flow.state = FlowState::Done {
        kept: 1,
        dropped: 1,
        awaiting_accept: 1,
    };
    let harness = Harness::new(Ports::cli());
    {
        let mut flows = harness.flows();
        flows.flows.insert(er_dir.clone(), flow);
        flows
            .flows
            .remember_review_flow_run_target(&target, &run_id);
    }
    Fixture {
        repo,
        harness,
        target,
        er_dir,
        run_id,
    }
}

impl Fixture {
    /// Accepts `ids` from the flow's run with the fresh diff of its target,
    /// as `FlowCommand::AcceptFindings` does.
    fn accept(&self, ids: &[&str]) -> anyhow::Result<usize> {
        let current = AcceptanceDiff {
            target: self.target.clone(),
            diff: Ok(fresh_diff(&self.target)),
        };
        self.accept_with(Some(current), ids)
    }

    fn accept_with(&self, current: Option<AcceptanceDiff>, ids: &[&str]) -> anyhow::Result<usize> {
        let mut guard = self.harness.flows();
        let flows = &mut *guard;
        flows.host.acceptance = current;
        let ids = ids.iter().map(|id| (*id).to_owned()).collect();
        let accepted = flows.flows.accept_review_flow_findings(
            &mut flows.host,
            &self.er_dir,
            &self.run_id,
            ids,
        );
        flows.host.acceptance = None;
        accepted
    }

    fn results(&self, run_id: &str) -> anyhow::Result<ReviewFlowResults> {
        self.harness
            .flows()
            .flows
            .review_flow_results(&self.er_dir, run_id)
    }

    fn paths(&self) -> ArenaPaths {
        ArenaPaths::for_run(Path::new(&self.er_dir), &self.run_id)
    }

    /// Runs `f` with the bucket's lease held by the flow host.
    fn with_lease<R>(&self, f: impl FnOnce() -> R) -> R {
        let lease = self
            .harness
            .flows()
            .host
            .acquire_review_flow_lease(&self.er_dir)
            .unwrap();
        let result = f();
        self.harness
            .flows()
            .host
            .release_review_flow_lease(&lease)
            .unwrap();
        result
    }

    fn storage<R>(&self, f: impl FnOnce() -> R) -> R {
        er_engine::storage::with_test_storage_root(self.repo.path(), f)
    }
}

fn historical_run(fixture: &Fixture, id: &str, status: RunStatus) -> ArenaPaths {
    let paths = ArenaPaths::for_run(Path::new(&fixture.er_dir), id);
    let mut run = load_run(&fixture.paths()).unwrap();
    run.id = id.into();
    run.status = status;
    save_run(&paths, &run).unwrap();
    paths
}

fn already_in_use(error: &anyhow::Error) -> bool {
    error
        .to_string()
        .contains("A review flow is already using this review bucket")
}

/// A run another process left running or queued in the captured bucket is
/// marked failed by the acceptance itself; listing the bucket's runs first
/// writes nothing.
#[test]
fn acceptance_recovers_an_interrupted_history_run_in_the_captured_bucket() {
    for status in [RunStatus::Running { round: 1 }, RunStatus::Queued] {
        let fixture = fixture();
        fixture.storage(|| {
            let paths = historical_run(&fixture, "interrupted", status.clone());
            let before = std::fs::read(paths.run_json()).unwrap();
            er_engine::arena::list_summaries(Path::new(&fixture.er_dir), None).unwrap();
            assert_eq!(std::fs::read(paths.run_json()).unwrap(), before);

            assert_eq!(fixture.accept(&["kept"]).unwrap(), 1);
            assert!(
                matches!(load_run(&paths).unwrap().status, RunStatus::Failed),
                "{status:?}"
            );
        });
    }
}

#[test]
fn recovery_preserves_registered_running_and_terminal_writers() {
    for status in [RunStatus::Running { round: 1 }, RunStatus::Cancelled] {
        let fixture = fixture();
        fixture.storage(|| {
            let paths = historical_run(&fixture, "owned", RunStatus::Running { round: 1 });
            let before = std::fs::read(paths.run_json()).unwrap();
            let arena = fixture.harness.flows().host.arena.clone();
            arena.insert_for_test("owned", &fixture.er_dir, status);
            // The control map is authoritative for live writers.
            fixture
                .harness
                .reconcile(std::slice::from_ref(&fixture.er_dir));
            er_engine::arena::list_summaries(Path::new(&fixture.er_dir), None).unwrap();
            assert_eq!(std::fs::read(paths.run_json()).unwrap(), before);
            let error = fixture.with_lease(|| fixture.accept(&["kept"]).unwrap_err());
            assert!(already_in_use(&error), "{error:#}");
            arena.remove("owned");
        });
    }
}

#[test]
fn recovery_incomplete_historical_manifests_allow_guarded_acceptance() {
    for malformed in [false, true] {
        let fixture = fixture();
        fixture.storage(|| {
            let paths = ArenaPaths::for_run(Path::new(&fixture.er_dir), "orphan");
            paths.ensure_dirs().unwrap();
            std::fs::write(paths.diff_patch(), "recoverable input").unwrap();
            if malformed {
                std::fs::write(paths.run_json(), "{broken").unwrap();
            }
            assert_eq!(fixture.accept(&["kept"]).unwrap(), 1);
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
        let fixture = fixture();
        fixture.storage(|| {
            let paths = ArenaPaths::for_run(Path::new(&fixture.er_dir), "owned-orphan");
            paths.ensure_dirs().unwrap();
            if malformed {
                std::fs::write(paths.run_json(), "{broken").unwrap();
            }
            let error = fixture.with_lease(|| fixture.accept(&["kept"]).unwrap_err());
            assert!(already_in_use(&error), "{error:#}");
        });
    }
}

#[cfg(unix)]
#[test]
fn recovery_never_writes_through_manifest_or_temporary_symlinks() {
    use std::os::unix::fs::symlink;
    for temporary in [false, true] {
        let fixture = fixture();
        fixture.storage(|| {
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
            let error = fixture.accept(&["kept"]).unwrap_err();
            assert!(error.to_string().contains("symlink"), "{error:#}");
            assert_eq!(std::fs::read(&external).unwrap(), before);
            let error = fixture.with_lease(|| fixture.accept(&["kept"]).unwrap_err());
            assert!(already_in_use(&error), "{error:#}");
        });
    }
}

#[test]
fn recovery_manifest_directory_io_error_is_not_silenced() {
    let fixture = fixture();
    fixture.storage(|| {
        let paths = ArenaPaths::for_run(Path::new(&fixture.er_dir), "unreadable");
        paths.ensure_dirs().unwrap();
        std::fs::create_dir(paths.run_json()).unwrap();
        let error = fixture.accept(&["kept"]).unwrap_err();
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

    let raw = fresh_diff(&fixture.target);
    let run = load_run(&fixture.paths()).unwrap();
    assert_eq!(er_engine::ai::compute_diff_hash(&raw), run.diff_hash);
    assert!(!raw.contains(".er/"));
}

#[test]
fn opening_and_closing_results_is_read_only_and_run_identity_is_explicit() {
    let fixture = fixture();
    let paths = fixture.paths();
    let run_before = std::fs::read(paths.run_json()).unwrap();
    let results = fixture.results(&fixture.run_id).unwrap();
    assert_eq!(results.findings.len(), 1);
    assert_eq!(results.findings[0].id, "kept");
    assert!(results.findings[0].selectable);
    assert!(!Path::new(&fixture.er_dir).join("review.json").exists());
    drop(results);
    assert_eq!(std::fs::read(paths.run_json()).unwrap(), run_before);
    assert!(fixture.results("unknown").is_err());

    let mut run = load_run(&paths).unwrap();
    run.id = "wrong-generation".into();
    save_run(&paths, &run).unwrap();
    assert!(fixture.results(&fixture.run_id).is_err());
}

/// A flow run's record says it is flow-owned, so the service's generic arena
/// acceptance (`ArenaCommand::AcceptFindings`) refuses it with no flow in
/// memory, as after a restart.
#[test]
fn persisted_flow_run_cannot_use_generic_acceptance_after_restart() {
    let fixture = fixture();
    fixture.storage(|| {
        let paths = fixture.paths();
        let mut run = load_run(&paths).unwrap();
        run.config.run_kind = ArenaRunKind::ReviewFlow;
        save_run(&paths, &run).unwrap();
        let run_before = std::fs::read(paths.run_json()).unwrap();

        let result = er_engine::arena::accept_findings(
            Path::new(&fixture.er_dir),
            &fixture.run_id,
            Some(vec!["kept".into()]),
            crate::arena::is_flow_run,
        );

        assert!(
            result.is_err(),
            "flow ownership must survive loss of in-memory context"
        );
        assert!(!Path::new(&fixture.er_dir).join("review.json").exists());
        assert_eq!(std::fs::read(paths.run_json()).unwrap(), run_before);
    });
}

/// The service's run read keeps a seeded verification's kind and writes
/// nothing.
#[test]
fn a_run_snapshot_keeps_a_seeded_verification_and_writes_nothing() {
    let fixture = fixture();
    let paths = fixture.paths();
    let mut run = load_run(&paths).unwrap();
    run.config.run_kind = ArenaRunKind::Seeded;
    save_run(&paths, &run).unwrap();
    let run_before = std::fs::read(paths.run_json()).unwrap();
    let snapshot = er_engine::arena::run_snapshot_with_status(
        &ErConfig::default(),
        None,
        Path::new(&fixture.er_dir),
        &fixture.run_id,
        crate::arena::is_flow_run,
    )
    .unwrap();
    assert_eq!(snapshot.run.config.run_kind, ArenaRunKind::Seeded);
    assert_eq!(std::fs::read(paths.run_json()).unwrap(), run_before);
}

fn seed_old_overlay(fixture: &Fixture) -> Vec<u8> {
    let paths = fixture.paths();
    let mut run = load_run(&paths).unwrap();
    run.config.agent_kind = Some("general".into());
    run.reviewers.push(serde_json::from_value(json!({
        "id":"deep-finder","name":"Deep finder","kind":"model","provider_id":"deep-provider","model_id":"test",
        "system_prompt":"","color":"","icon":"","tagline":"","cost_per_1k_in":0.0,"cost_per_1k_out":0.0,"avg_latency_ms":0,"status":"ok","agent_kind":"general"
    })).unwrap());
    run.findings[0].raised_by = vec!["deep-finder".into()];
    save_run(&paths, &run).unwrap();
    er_engine::test_support::write_sidecar_json::<er_engine::ai::ArbiterReview>(
        Path::new(&fixture.er_dir),
        json!({
            "version": 1, "diff_hash": run.diff_hash, "run_id": "old-quick",
            "verdicts": [
                {"id": er_engine::ai::finding_key("file.txt", Some(2), "Kept finding"), "file":"file.txt", "verdict":"dropped", "raised_by":["security"], "providers":["old-provider"]},
                {"id": er_engine::ai::finding_key("file.txt", Some(1), "Unrelated finding"), "file":"file.txt", "verdict":"dropped"}
            ]
        }),
    );
    let bytes = std::fs::read(Path::new(&fixture.er_dir).join("arbiter.json")).unwrap();
    er_engine::test_support::write_sidecar_json::<er_engine::ai::ErReview>(
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
    let fixture = fixture();
    fixture.storage(|| {
        seed_old_overlay(&fixture);
        fixture.accept(&["kept"]).unwrap();
        let run = load_run(&fixture.paths()).unwrap();
        let state = er_engine::ai::load_ai_state(&fixture.er_dir, &run.diff_hash, None);
        let review = state.review.unwrap();
        let rows = &review.files["file.txt"].findings;
        assert_eq!(
            er_engine::ai::project_visible_findings(
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
        let mut later = er_engine::ai::load_arbiter_review(&fixture.er_dir).unwrap();
        assert!(
            later.run_id.is_empty(),
            "mixed rulings do not belong to the old Quick run"
        );
        assert!(later.created_at.is_empty());
        later.run_id = "later-verification".into();
        later.verdicts.push(
            serde_json::from_value(json!({
                "id":er_engine::ai::finding_key("file.txt", Some(2), "Kept finding"),"verdict":"dropped"
            }))
            .unwrap(),
        );
        er_engine::ai::write_arbiter_review(&fixture.er_dir, &later).unwrap();
        let state = er_engine::ai::load_ai_state(&fixture.er_dir, &run.diff_hash, None);
        assert!(!state.review.unwrap().files["file.txt"]
            .findings
            .iter()
            .find(|f| f.id == "arena-flow-run-kept")
            .unwrap()
            .is_active());
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

fn save_refused(error: &anyhow::Error) -> bool {
    error
        .to_string()
        .contains("accepted IDs could not be saved to the run")
}

#[test]
fn run_save_failure_restores_superseded_overlay_byte_for_byte() {
    let fixture = fixture();
    fixture.storage(|| {
        let original = seed_old_overlay(&fixture);
        let _fail_save = fail_run_save(&fixture.paths());
        assert!(fixture.accept(&["kept"]).is_err());
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

/// Acceptance writes into the bucket the flow captured, whatever the reader
/// shows now, and records the accepted rows apart from `review.json`.
#[test]
fn acceptance_uses_the_captured_bucket_and_leaves_review_alone() {
    let fixture = fixture();
    fixture.storage(|| {
        let review_path = Path::new(&fixture.er_dir).join("review.json");
        er_engine::test_support::write_sidecar_json::<er_engine::ai::ErReview>(
            Path::new(&fixture.er_dir),
            json!({"version":1,"diff_hash":"old-generation","files":{"legacy.rs":{"risk":"medium","findings":[{"id":"legacy","severity":"medium","title":"Old generation finding"}]}}}),
        );
        let old_review = std::fs::read(&review_path).unwrap();

        assert_eq!(fixture.accept(&["kept"]).unwrap(), 1);
        assert_eq!(
            std::fs::read(&review_path).unwrap(),
            old_review,
            "acceptance writes the artifact, not review.json"
        );
        let artifact: er_engine::ai::accepted_findings::AcceptedFindings =
            er_engine::test_support::read_sidecar(Path::new(&fixture.er_dir));
        assert_eq!(artifact.findings.len(), 1);
        let row = &artifact.findings[0];
        assert_eq!(row.finding.id, format!("arena-{}-kept", fixture.run_id));
        assert_eq!(row.run_finding_id, "kept");
        assert_eq!(row.run_id, fixture.run_id);
        assert_eq!(row.file, "file.txt");
        let run = load_run(&fixture.paths()).unwrap();
        assert_eq!(run.accepted_finding_ids, vec!["kept"]);
        assert!(matches!(
            fixture
                .harness
                .flows()
                .flows
                .review_flow_for(&fixture.er_dir)
                .map(|flow| flow.state.clone()),
            Some(FlowState::Done {
                awaiting_accept: 0,
                ..
            })
        ));
    });
}

/// Acceptance used to re-stamp the general review's diff hash with the
/// run's, marking a review from another generation as matching this diff
/// (ADR 0035). The accepted row is visible and the review stays stale.
#[test]
fn acceptance_does_not_restamp_the_general_review() {
    let fixture = fixture();
    fixture.storage(|| {
        let review_path = Path::new(&fixture.er_dir).join("review.json");
        er_engine::test_support::write_sidecar_json::<er_engine::ai::ErReview>(
            Path::new(&fixture.er_dir),
            json!({
                "version": 1,
                "diff_hash": "older-generation",
                "files": {"file.txt": {"risk":"low","findings":[{"id":"older","severity":"low","title":"Older generation finding"}]}}
            }),
        );
        let before = std::fs::read(&review_path).unwrap();
        fixture.accept(&["kept"]).unwrap();
        assert_eq!(
            std::fs::read(&review_path).unwrap(),
            before,
            "acceptance leaves review.json byte-identical"
        );
        let run = load_run(&fixture.paths()).unwrap();
        let state = er_engine::ai::load_ai_state(&fixture.er_dir, &run.diff_hash, None);
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
    let fixture = fixture();
    fixture.storage(|| {
        assert!(fixture.accept(&["dropped"]).is_err());
        assert!(fixture.accept(&["missing"]).is_err());
        assert!(!Path::new(&fixture.er_dir).join("review.json").exists());

        fixture.results(&fixture.run_id).unwrap();
        let mut guard = fixture.harness.flows();
        let mut flow = guard
            .flows
            .review_flow_for(&fixture.er_dir)
            .unwrap()
            .clone();
        flow.arena_run_id = Some("new-flow-run".into());
        guard.flows.insert(fixture.er_dir.clone(), flow);
        drop(guard);
        assert!(fixture.accept(&["kept"]).is_err());
    });
}

#[test]
fn acceptance_rejects_a_changed_working_diff() {
    let fixture = fixture();
    fixture.storage(|| {
        std::fs::write(
            fixture.repo.path().join("file.txt"),
            "changed after review\n",
        )
        .unwrap();
        let error = fixture.accept(&["kept"]).unwrap_err();
        assert!(error.to_string().contains("The diff changed"), "{error:#}");
        assert!(!Path::new(&fixture.er_dir).join("review.json").exists());
    });
}

/// With no open view of the target the command carries no fresh diff.
#[test]
fn acceptance_refuses_when_captured_target_is_closed() {
    let fixture = fixture();
    fixture.storage(|| {
        let error = fixture.accept_with(None, &["kept"]).unwrap_err();
        assert!(error.to_string().contains("no longer open"), "{error:#}");
        assert!(!Path::new(&fixture.er_dir).join("review.json").exists());
    });
}

#[test]
fn run_save_failure_rolls_back_a_new_accepted_findings_artifact() {
    let fixture = fixture();
    fixture.storage(|| {
        let paths = fixture.paths();
        let _fail_save = fail_run_save(&paths);

        let error = fixture.accept(&["kept"]).unwrap_err();
        assert!(save_refused(&error), "{error:#}");
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
    let fixture = fixture();
    fixture.storage(|| {
        let paths = fixture.paths();
        er_engine::test_support::write_sidecar_json::<
            er_engine::ai::accepted_findings::AcceptedFindings,
        >(
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

        let error = fixture.accept(&["kept"]).unwrap_err();
        assert!(save_refused(&error), "{error:#}");
        assert_eq!(std::fs::read(artifact_path).unwrap(), original);
        assert!(load_run(&paths).unwrap().accepted_finding_ids.is_empty());
    });
}
