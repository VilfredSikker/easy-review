//! The stage sequence on the service host with a fake provider CLI: triage
//! routes to Quick, Standard, Deep or a skip, finders run and verification
//! waits for its run's final record; a cancel keeps the lease until the
//! writer drains; a re-run resumes only the unfinished stages. Moved from
//! the driver's `mod.rs` and the engine's Quick-flow integration test,
//! where they ran on the App.

use std::path::Path;
use std::time::Duration;

use er_engine::ai::review_flow::{FlowDepth, FlowState, FlowTaskStatus};
use er_engine::ai::review_md::ReviewMdSource;
use er_engine::ai::workflow_record::{self, WorkflowOutcome};
use er_engine::ai::{StageKind, StageRecord, StageStatus, WorkflowKind, WorkflowRecord};
use er_engine::arena::{ArenaScope, RunStatus};
use er_engine::config::{
    AiHubConfig, AiModelConfig, AiProviderConfig, ErConfig, ReviewModelGroups, ReviewModelRef,
};
use er_engine::ports::Ports;
use er_engine::review_flow_driver::ReviewFlowInputs;
use er_engine::task_target::BackgroundTaskTarget;

use super::Harness;

pub(super) const RAW: &str = "diff --git a/src/lib.rs b/src/lib.rs\nindex 1111111..2222222 100644\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-old\n+new\n";

/// A repo directory and a bucket under `dir`, as the App's test tab had.
pub(super) struct Dirs {
    pub(super) repo: String,
    pub(super) bucket: String,
}

pub(super) fn dirs(dir: &Path) -> Dirs {
    let repo = dir.join("repo");
    let bucket = dir.join("bucket");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::create_dir_all(&bucket).unwrap();
    Dirs {
        repo: repo.to_string_lossy().into_owned(),
        bucket: bucket.to_string_lossy().into_owned(),
    }
}

/// The branch view of a local tab on `repo`, reviewed in `repo` itself.
pub(super) fn view(dirs: &Dirs) -> ReviewFlowInputs {
    ReviewFlowInputs {
        target: BackgroundTaskTarget {
            repo_root: dirs.repo.clone(),
            er_dir: dirs.bucket.clone(),
            branch_label: "feature".into(),
            base_branch: "main".into(),
            scope: "branch".into(),
            pr_number: None,
            remote_repo: None,
            managed_local: true,
        },
        arena_scope: ArenaScope::Branch,
        repo_root: dirs.repo.clone(),
        reviewed_ref_note: None,
        branch_ref: "feature".into(),
        base_branch: "main".into(),
        raw_diff: RAW.into(),
        owned_rules: Default::default(),
        review_md_source: ReviewMdSource::None,
    }
}

pub(super) fn one_provider_review_models(provider_id: &str, model_id: &str) -> ReviewModelGroups {
    let model = ReviewModelRef {
        provider_id: provider_id.into(),
        model_id: model_id.into(),
    };
    ReviewModelGroups {
        fast: vec![model.clone()],
        standard: vec![model.clone()],
        advanced: vec![model],
    }
}

/// One fake provider whose triage picks `triage_primary` and `experts`.
pub(super) fn fake_config(bucket: &str, triage_primary: &str, experts: &[&str]) -> ErConfig {
    fake_config_with(
        bucket,
        triage_primary,
        experts,
        "{\"reasons\":[]}",
        er_engine::test_support::WRITE_REVIEW_SIDECARS,
        "$h",
    )
}

/// The provider answers triage with `triage_primary`, `experts`, the
/// `sensitive` block and the diff hash `triage_hash_expr` evaluates to; the
/// general finder runs `review_behavior`.
pub(super) fn fake_config_with(
    bucket: &str,
    triage_primary: &str,
    experts: &[&str],
    sensitive: &str,
    review_behavior: &str,
    triage_hash_expr: &str,
) -> ErConfig {
    let expert_json = serde_json::to_string(experts).unwrap();
    let script = er_engine::test_support::staged_script(
        &"case \"$1\" in\n\
         *triage.json*) __PREAMBLE__; triage_hash=__TRIAGE_HASH__; printf '{\"version\":1,\"diff_hash\":\"%s\",\"verdict\":{\"primary\":\"__PRIMARY__\",\"experts\":__EXPERTS__},\"sensitive\":__SENSITIVE__}' \"$triage_hash\" > \"$out/triage.json\" ;;\n\
         *experts/security.json*) __PREAMBLE__; mkdir -p \"$out/experts\"; printf '{\"version\":1,\"expert_id\":\"security\",\"diff_hash\":\"%s\",\"files\":{}}' \"$h\" > \"$out/experts/security.json\" ;;\n\
         *review.json*) __PREAMBLE__; __REVIEW_BEHAVIOR__ ;;\n\
         *) echo \"unexpected review-flow prompt\" >&2; exit 2 ;;\n\
         esac"
            .replace("__PREAMBLE__", er_engine::test_support::STAGING_PREAMBLE)
            .replace("__PRIMARY__", triage_primary)
            .replace("__EXPERTS__", &expert_json)
            .replace("__SENSITIVE__", sensitive)
            .replace("__TRIAGE_HASH__", triage_hash_expr)
            .replace("__REVIEW_BEHAVIOR__", review_behavior),
        bucket,
    );
    with_script(script)
}

/// One fake provider running `script` for every stage.
pub(super) fn with_script(script: String) -> ErConfig {
    let provider = AiProviderConfig {
        command: "/bin/sh".into(),
        args: vec!["-c".into(), script, "fake-flow".into(), "{prompt}".into()],
        models: vec![AiModelConfig {
            id: "fake-model".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    ErConfig {
        ai_hub: AiHubConfig {
            default_provider: Some("fake".into()),
            providers: [("fake".into(), provider)].into(),
            review_models: one_provider_review_models("fake", "fake-model"),
            max_concurrent_reviews: 16,
            max_concurrent_arena_reviews: 16,
            max_concurrent_agents: 16,
            ..Default::default()
        },
        ..Default::default()
    }
}

/// A second provider like the first, so Deep has two advanced reviewers.
pub(super) fn with_second_provider(config: &mut ErConfig) {
    let second = config.ai_hub.providers["fake"].clone();
    config.ai_hub.providers.insert("fake-second".into(), second);
    config.ai_hub.review_models.advanced.push(ReviewModelRef {
        provider_id: "fake-second".into(),
        model_id: "fake-model".into(),
    });
}

/// A harness holding `config` and the view of `dirs`, in a process-wide
/// storage root: stage workers commit through the store on their own threads.
fn flow_in(dir: &Path, config: &ErConfig, f: impl FnOnce(&Harness, &Dirs)) {
    let dirs = dirs(dir);
    er_engine::storage::with_process_test_storage_root(dir, || {
        let harness = Harness::new(Ports::cli());
        harness.set_context(config, view(&dirs));
        f(&harness, &dirs);
    });
}

fn admitted(bucket: &str) -> bool {
    er_engine::arena::check_bucket_admission(bucket).is_ok()
}

fn task_status(flow: &er_engine::ai::review_flow::ReviewFlow, id: &str) -> FlowTaskStatus {
    flow.tasks
        .iter()
        .find(|task| task.id == id)
        .unwrap_or_else(|| panic!("no {id} row in {:?}", flow.tasks))
        .status
}

#[test]
fn fake_cli_runs_triage_and_quick_general_before_empty_seeded_completion() {
    let _serial = er_agent::agent_slots::test_serial();
    let dir = tempfile::tempdir().unwrap();
    let config = fake_config(&dirs(dir.path()).bucket, "general", &[]);
    flow_in(dir.path(), &config, |harness, dirs| {
        harness.start("branch").unwrap();
        harness.run_until_terminal(&dirs.bucket);
        let flow = harness.flow(&dirs.bucket);
        assert!(
            matches!(
                flow.state,
                FlowState::Done {
                    kept: 0,
                    dropped: 0,
                    awaiting_accept: 0
                }
            ),
            "state={:?}, failed_finders={:?}",
            flow.state,
            flow.failed_finders
        );
        assert_eq!(flow.lenses, ["general"]);
        assert_eq!(flow.successful_finder_providers, ["fake"]);
        assert_eq!(flow.successful_arbiter_provider, None);
        assert_eq!(flow.providers_used, ["fake"]);
        assert_eq!(flow.tasks[0].id, "triage");
        assert_eq!(flow.tasks[0].status, FlowTaskStatus::Done);
        assert_eq!(flow.tasks[1].id, "finding:general");
        assert_eq!(flow.tasks[1].status, FlowTaskStatus::Done);
        assert_eq!(flow.tasks[2].id, "verifying:arbiter");
        assert_eq!(flow.tasks[2].status, FlowTaskStatus::Skipped);
        assert!(Path::new(&dirs.bucket).join("flow-backups").is_dir());
        assert!(!harness.is_draining(&dirs.bucket));
        assert!(harness.flows().flows.review_flow_revision() > 1);
    });
}

#[test]
fn professor_route_keeps_follow_up_suggestion_after_completion() {
    let _serial = er_agent::agent_slots::test_serial();
    let dir = tempfile::tempdir().unwrap();
    let config = fake_config(&dirs(dir.path()).bucket, "professor", &[]);
    flow_in(dir.path(), &config, |harness, dirs| {
        harness.start("branch").unwrap();
        harness.run_until_terminal(&dirs.bucket);
        let flow = harness.flow(&dirs.bucket);
        assert!(flow.professor_follow_up_suggested);
        assert_eq!(
            flow.state,
            FlowState::Done {
                kept: 0,
                dropped: 0,
                awaiting_accept: 0,
            },
            "failed_finders={:?}",
            flow.failed_finders
        );
    });
}

#[test]
fn successful_finder_output_replaces_same_hash_archived_output_only_after_validation() {
    let _serial = er_agent::agent_slots::test_serial();
    let dir = tempfile::tempdir().unwrap();
    let bucket = dirs(dir.path()).bucket;
    let config = fake_config_with(&bucket, "general", &[], "{\"reasons\":[]}", "exit 0", "$h");
    flow_in(dir.path(), &config, |harness, dirs| {
        let view = view(dirs);
        let hash = er_engine::ai::prepared_diff::ensure_review_inputs(
            &dirs.bucket,
            &view.raw_diff,
            view.owned_rules.as_rules(),
            None,
        )
        .unwrap();
        er_engine::test_support::write_sidecar_json::<er_engine::ai::ErReview>(
            Path::new(&dirs.bucket),
            serde_json::json!({"version":1,"diff_hash":hash,"files":{"old.rs":{"risk":"low"}}}),
        );
        let old = std::fs::read_to_string(Path::new(&dirs.bucket).join("review.json")).unwrap();

        harness.start("branch").unwrap();
        harness.run_until_terminal(&dirs.bucket);
        assert!(matches!(
            harness.flow(&dirs.bucket).state,
            FlowState::Failed { .. }
        ));
        assert_eq!(
            std::fs::read_to_string(Path::new(&dirs.bucket).join("review.json")).unwrap(),
            old
        );
    });
}

#[test]
fn stale_triage_hash_fails_closed() {
    let _serial = er_agent::agent_slots::test_serial();
    let dir = tempfile::tempdir().unwrap();
    let bucket = dirs(dir.path()).bucket;
    let config = fake_config_with(
        &bucket,
        "general",
        &[],
        "{\"reasons\":[]}",
        "exit 0",
        "stale-diff-hash",
    );
    flow_in(dir.path(), &config, |harness, dirs| {
        harness.start("branch").unwrap();
        harness.run_until_terminal(&dirs.bucket);
        let flow = harness.flow(&dirs.bucket);
        assert!(matches!(flow.state, FlowState::Failed { .. }));
        assert_eq!(flow.tasks[0].status, FlowTaskStatus::Failed);
        assert!(!Path::new(&dirs.bucket).join("review.json").exists());
    });
}

#[test]
fn sensitive_skip_routes_to_security_only_quick_finder() {
    let _serial = er_agent::agent_slots::test_serial();
    let dir = tempfile::tempdir().unwrap();
    let config = fake_config_with(
        &dirs(dir.path()).bucket,
        "skip",
        &[],
        "{\"reasons\":[{\"kind\":\"secret\",\"evidence\":\"credential literal\"}]}",
        er_engine::test_support::WRITE_REVIEW_SIDECARS,
        "$h",
    );
    flow_in(dir.path(), &config, |harness, dirs| {
        harness.start("branch").unwrap();
        harness.run_until_terminal(&dirs.bucket);
        let flow = harness.flow(&dirs.bucket);
        assert_eq!(flow.depth, Some(FlowDepth::Quick));
        assert_eq!(flow.lenses, ["security"]);
        assert_eq!(flow.successful_finder_providers, ["fake"]);
        assert!(
            matches!(flow.state, FlowState::Done { .. }),
            "state={:?}, failed_finders={:?}",
            flow.state,
            flow.failed_finders
        );
    });
}

#[test]
fn quick_general_and_security_both_finish_before_verification() {
    let _serial = er_agent::agent_slots::test_serial();
    let dir = tempfile::tempdir().unwrap();
    let config = fake_config_with(
        &dirs(dir.path()).bucket,
        "general",
        &[],
        "{\"reasons\":[{\"kind\":\"secret\",\"evidence\":\"credential literal\"}]}",
        &format!(
            "sleep 1; {}",
            er_engine::test_support::WRITE_REVIEW_SIDECARS
        ),
        "$h",
    );
    flow_in(dir.path(), &config, |harness, dirs| {
        harness.start("branch").unwrap();
        harness.run_until_terminal(&dirs.bucket);
        let flow = harness.flow(&dirs.bucket);
        assert_eq!(flow.lenses, ["general", "security"]);
        assert!(flow.pending_finders.is_empty());
        assert_eq!(flow.successful_finder_providers, ["fake"]);
        assert!(
            matches!(flow.state, FlowState::Done { .. }),
            "state={:?}, failed_finders={:?}",
            flow.state,
            flow.failed_finders
        );
        assert!(Path::new(&dirs.bucket).join("review.json").exists());
        assert!(Path::new(&dirs.bucket)
            .join("experts/security.json")
            .exists());
        assert_eq!(flow.arena_run_id, None);
    });
}

#[test]
fn partial_finder_failure_ignores_unplanned_same_hash_expert_output() {
    let _serial = er_agent::agent_slots::test_serial();
    let dir = tempfile::tempdir().unwrap();
    let config = fake_config_with(
        &dirs(dir.path()).bucket,
        "expert",
        &["security"],
        "{\"reasons\":[]}",
        "exit 7",
        "$h",
    );
    flow_in(dir.path(), &config, |harness, dirs| {
        harness.start("branch").unwrap();
        let hash = harness.flow(&dirs.bucket).diff_hash.clone();
        er_engine::test_support::write_sidecar_json_slot::<er_engine::ai::experts::ExpertReview>(
            Path::new(&dirs.bucket),
            er_store::ExpertId::new("architecture").unwrap(),
            serde_json::json!({"version":1,"expert_id":"architecture","diff_hash":hash,"files":{}}),
        );
        harness.run_until_terminal(&dirs.bucket);
        let flow = harness.flow(&dirs.bucket);
        assert!(flow
            .failed_finders
            .iter()
            .any(|(lens, _)| lens == "general"));
        assert_eq!(flow.successful_finder_providers, ["fake"]);
        assert_eq!(flow.providers_used, ["fake"]);
        assert!(flow.arena_run_id.is_none());
    });
}

/// A cancel while the finder runs keeps the bucket's lease until that
/// writer drains, and the flow stays cancelled once its result lands.
#[test]
fn running_cancellation_keeps_lease_until_writer_drains() {
    let _serial = er_agent::agent_slots::test_serial();
    let dir = tempfile::tempdir().unwrap();
    let config = fake_config_with(
        &dirs(dir.path()).bucket,
        "general",
        &[],
        "{\"reasons\":[]}",
        &format!(
            "sleep 1; {}",
            er_engine::test_support::WRITE_REVIEW_SIDECARS
        ),
        "$h",
    );
    flow_in(dir.path(), &config, |harness, dirs| {
        harness.start("branch").unwrap();
        harness.advance_until(
            &dirs.bucket,
            Duration::from_secs(10),
            "finder did not start",
            |flow| flow.state == FlowState::Finding,
        );
        assert!(harness.cancel(&dirs.bucket));
        // The next advance sees the finder still writing.
        harness.advance();
        assert!(harness.is_draining(&dirs.bucket));
        assert!(harness.start("branch").is_err());
        assert!(!admitted(&dirs.bucket), "the lease is held while draining");

        harness.drain(&dirs.bucket);
        assert!(admitted(&dirs.bucket));
        let flow = harness.flow(&dirs.bucket);
        assert_eq!(flow.state, FlowState::Cancelled);
        assert_eq!(task_status(&flow, "triage"), FlowTaskStatus::Done);
    });
}

/// Every stage's script: triage picks a Quick general review, the general
/// finder reports one finding so verification has work, and every arena call
/// waits for `gate` before it answers.
fn gated_verification(bucket: &str, gate: &Path) -> ErConfig {
    let script = format!(
        "case \"$1\" in\n\
         *triage.json*) {preamble}; printf '{{\"version\":1,\"diff_hash\":\"%s\",\"verdict\":{{\"primary\":\"general\",\"experts\":[]}}}}' \"$h\" > \"$out/triage.json\" ;;\n\
         *review.json*) {preamble}; printf '{{\"version\":1,\"diff_hash\":\"%s\",\"files\":{{\"src/lib.rs\":{{\"risk\":\"high\",\"findings\":[{{\"id\":\"general:1\",\"severity\":\"high\",\"title\":\"Check the source\"}}]}}}}}}' \"$h\" > \"$out/review.json\"; printf '{{\"version\":1,\"diff_hash\":\"%s\",\"order\":[],\"groups\":{{}}}}' \"$h\" > \"$out/order.json\"; printf '{{\"version\":1,\"diff_hash\":\"%s\",\"items\":[]}}' \"$h\" > \"$out/checklist.json\"; printf 'summary\\n' > \"$out/summary.md\" ;;\n\
         *) while [ ! -f '{gate}' ]; do sleep 0.05; done; printf '{{\"findings\":[],\"ballots\":[],\"verdicts\":[]}}' ;;\n\
         esac",
        preamble = er_engine::test_support::STAGING_PREAMBLE,
        gate = gate.display(),
    );
    with_script(er_engine::test_support::staged_script(&script, bucket))
}

/// Opens the gate when the test ends, so a failed assertion leaves no
/// provider waiting on it.
struct OpenGate(std::path::PathBuf);

impl Drop for OpenGate {
    fn drop(&mut self) {
        let _ = std::fs::write(&self.0, "");
    }
}

/// The arbiter run's live status turns Complete before its supervisor
/// returns and saves the final record; only that record ends verification,
/// and the lease is held until then.
#[test]
fn verifying_waits_for_drained_supervisor_and_final_persisted_run() {
    let _serial = er_agent::agent_slots::test_serial();
    let dir = tempfile::tempdir().unwrap();
    let gate = dir.path().join("gate");
    let _open_on_exit = OpenGate(gate.clone());
    let config = gated_verification(&dirs(dir.path()).bucket, &gate);
    flow_in(dir.path(), &config, |harness, dirs| {
        harness.start("branch").unwrap();
        harness.advance_until(
            &dirs.bucket,
            Duration::from_secs(15),
            "verification did not start",
            |flow| flow.state == FlowState::Verifying && flow.arena_run_id.is_some(),
        );
        let run_id = harness.flow(&dirs.bucket).arena_run_id.unwrap();
        let control = harness.flows().host.arena.get(&run_id).unwrap();
        control.set_status(RunStatus::Complete);

        harness.advance();
        assert_eq!(
            harness.flow(&dirs.bucket).state,
            FlowState::Verifying,
            "live Complete plus persisted Running must remain Verifying until drain"
        );
        assert!(!admitted(&dirs.bucket));

        std::fs::write(&gate, "").unwrap();
        harness.run_until_terminal(&dirs.bucket);
        let flow = harness.flow(&dirs.bucket);
        assert!(
            matches!(flow.state, FlowState::Done { .. }),
            "{:?}",
            flow.state
        );
        assert_eq!(flow.providers_used, ["fake"]);
        assert!(admitted(&dirs.bucket));
    });
}

#[test]
fn fake_cli_runs_both_standard_producer_outputs_before_seeded_completion() {
    let _serial = er_agent::agent_slots::test_serial();
    let dir = tempfile::tempdir().unwrap();
    let config = fake_config(&dirs(dir.path()).bucket, "expert", &["security"]);
    flow_in(dir.path(), &config, |harness, dirs| {
        harness.start("branch").unwrap();
        harness.run_until_terminal(&dirs.bucket);
        let flow = harness.flow(&dirs.bucket);
        assert!(
            matches!(
                flow.state,
                FlowState::Done {
                    kept: 0,
                    dropped: 0,
                    awaiting_accept: 0
                }
            ),
            "state={:?}, failed_finders={:?}",
            flow.state,
            flow.failed_finders
        );
        assert_eq!(flow.depth, Some(FlowDepth::Standard));
        assert!(Path::new(&dirs.bucket).join("review.json").is_file());
        assert!(Path::new(&dirs.bucket)
            .join("experts/security.json")
            .is_file());
        assert_eq!(flow.arena_run_id, None);
    });
}

#[test]
fn fake_cli_skip_does_not_launch_finder() {
    let _serial = er_agent::agent_slots::test_serial();
    let dir = tempfile::tempdir().unwrap();
    let config = fake_config(&dirs(dir.path()).bucket, "skip", &[]);
    flow_in(dir.path(), &config, |harness, dirs| {
        harness.start("branch").unwrap();
        harness.run_until_terminal(&dirs.bucket);
        assert_eq!(harness.flow(&dirs.bucket).state, FlowState::Skipped);
        assert!(!Path::new(&dirs.bucket).join("review.json").exists());
    });
}

/// Arena manifests another process left half-written do not block a new
/// flow, which leaves their inputs alone and holds the bucket while it runs.
#[test]
fn recovery_incomplete_historical_manifests_allow_public_review_start() {
    let _serial = er_agent::agent_slots::test_serial();
    for malformed in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let bucket = dirs(dir.path()).bucket;
        let config = fake_config_with(
            &bucket,
            "general",
            &[],
            "{\"reasons\":[]}",
            er_engine::test_support::WRITE_REVIEW_SIDECARS,
            "$(sleep 1; echo \"$h\")",
        );
        flow_in(dir.path(), &config, |harness, dirs| {
            let paths = er_engine::arena::ArenaPaths::for_run(Path::new(&dirs.bucket), "orphan");
            paths.ensure_dirs().unwrap();
            std::fs::write(paths.diff_patch(), "recoverable input").unwrap();
            if malformed {
                std::fs::write(paths.run_json(), "{broken").unwrap();
            }
            harness.start("branch").unwrap();
            assert!(!admitted(&dirs.bucket));
            assert_eq!(
                std::fs::read_to_string(paths.diff_patch()).unwrap(),
                "recoverable input"
            );
            assert!(harness.cancel(&dirs.bucket));
            harness.drain(&dirs.bucket);
        });
    }
}

#[test]
fn provider_admission_fails_before_preparing_or_leasing_bucket() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = fake_config(&dirs(dir.path()).bucket, "general", &[]);
    config.ai_hub.providers.clear();
    flow_in(dir.path(), &config, |harness, dirs| {
        assert!(harness.start("branch").is_err());
        assert!(!Path::new(&dirs.bucket).join("diff-tmp").exists());
        assert!(admitted(&dirs.bucket));
    });
}

/// The record a Standard run left when its owner stopped, every stage at
/// the diff the bucket holds now.
fn interrupted_record(bucket: &str, finder: StageStatus, verifier: StageStatus) {
    let hash = er_engine::ai::prepared_diff::ensure_review_inputs_from_config(
        bucket,
        &er_engine::git::filter_raw_diff_exclude_globs(RAW, &[]),
        "repo",
        None,
    )
    .unwrap();
    let stage = |id: &str, kind: StageKind, status| StageRecord {
        id: id.to_string(),
        stage: kind,
        label: id.to_string(),
        status,
        output: String::new(),
        diff_hash: hash.clone(),
    };
    let record = WorkflowRecord {
        version: 1,
        kind: Some(WorkflowKind::Standard),
        diff_hash: hash.clone(),
        started_at: "t0".to_string(),
        outcome: Some(WorkflowOutcome::Interrupted),
        holder: None,
        stages: vec![
            stage("triage", StageKind::Triage, StageStatus::Complete),
            stage("finding:general", StageKind::Finding, finder),
            stage("verifying:arbiter", StageKind::Verifying, verifier),
        ],
    };
    let scope = er_store::Scope::dir(bucket).unwrap();
    workflow_record::save(&er_engine::store_registry::store(), &scope, &record).unwrap();
}

#[test]
fn a_rerun_skips_completed_triage_and_enqueues_only_the_unfinished_finder() {
    let _serial = er_agent::agent_slots::test_serial();
    let dir = tempfile::tempdir().unwrap();
    let config = fake_config(&dirs(dir.path()).bucket, "general", &[]);
    flow_in(dir.path(), &config, |harness, dirs| {
        interrupted_record(&dirs.bucket, StageStatus::Interrupted, StageStatus::Pending);

        harness.rerun(&dirs.bucket).unwrap();

        let flow = harness.flow(&dirs.bucket);
        assert!(matches!(flow.state, FlowState::Finding), "{:?}", flow.state);
        assert_eq!(
            task_status(&flow, "triage"),
            FlowTaskStatus::Done,
            "the completed triage stage is not re-run"
        );
        let kinds: Vec<String> = harness
            .flows()
            .host
            .tasks
            .values()
            .map(|task| task.kind.clone())
            .collect();
        assert!(
            !kinds.iter().any(|kind| kind == "triage"),
            "no triage task was launched: {kinds:?}"
        );
        assert!(
            kinds.iter().any(|kind| kind == "review"),
            "the unfinished finder was launched: {kinds:?}"
        );
        harness.run_until_terminal(&dirs.bucket);
    });
}

/// Seeded verification answers from fixtures, so a run makes no real
/// provider call. Holds the agent-slot serial lock, as every arena test here
/// does, so no other arena run reads the variable meanwhile.
struct FakeArena {
    _serial: er_agent::agent_slots::TestSerialGuard,
    previous: Option<std::ffi::OsString>,
    _fixture: tempfile::TempDir,
}

impl FakeArena {
    fn install(reply: &serde_json::Value) -> Self {
        let serial = er_agent::agent_slots::test_serial();
        let dir = tempfile::tempdir().unwrap();
        for round in 1..=3 {
            std::fs::write(
                dir.path().join(format!("round{round}.json")),
                serde_json::to_vec(reply).unwrap(),
            )
            .unwrap();
        }
        let previous = std::env::var_os("ER_FAKE_ARENA_DIR");
        std::env::set_var("ER_FAKE_ARENA_DIR", dir.path());
        Self {
            _serial: serial,
            previous,
            _fixture: dir,
        }
    }
}

impl Drop for FakeArena {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(value) => std::env::set_var("ER_FAKE_ARENA_DIR", value),
            None => std::env::remove_var("ER_FAKE_ARENA_DIR"),
        }
    }
}

fn no_verdicts() -> serde_json::Value {
    serde_json::json!({"findings": [], "ballots": [], "verdicts": []})
}

/// A Quick or Standard run interrupted during verification has no
/// unfinished finder. The resume must keep the finders' outputs and run
/// the verification stage alone, not refuse the re-run the UI offered.
#[test]
fn a_run_interrupted_during_verification_reruns_only_that_stage() {
    let _arena = FakeArena::install(&no_verdicts());
    let dir = tempfile::tempdir().unwrap();
    let config = fake_config(&dirs(dir.path()).bucket, "general", &[]);
    flow_in(dir.path(), &config, |harness, dirs| {
        interrupted_record(
            &dirs.bucket,
            StageStatus::Complete,
            StageStatus::Interrupted,
        );
        let scope = er_store::Scope::dir(&dirs.bucket).unwrap();
        let hash = workflow_record::load(&er_engine::store_registry::store(), &scope)
            .unwrap()
            .unwrap()
            .diff_hash;
        // The kept finder's output, which verification reads.
        er_engine::test_support::write_sidecar_json::<er_engine::ai::ErReview>(
            Path::new(&dirs.bucket),
            serde_json::json!({
                "version": 1,
                "diff_hash": hash,
                "files": {"src/lib.rs": {
                    "risk": "medium",
                    "findings": [{
                        "id": "general-1",
                        "severity": "medium",
                        "title": "kept finding",
                        "description": "body",
                        "line_start": 1
                    }]
                }}
            }),
        );

        harness.rerun(&dirs.bucket).unwrap();

        let flow = harness.flow(&dirs.bucket);
        assert!(matches!(flow.state, FlowState::Finding), "{:?}", flow.state);
        assert_eq!(
            task_status(&flow, "finding:general"),
            FlowTaskStatus::Done,
            "the completed finder is reused, not re-run"
        );
        assert_eq!(
            task_status(&flow, "verifying:arbiter"),
            FlowTaskStatus::Waiting,
            "verification is the stage that re-runs"
        );
        assert!(
            harness.flows().host.tasks.is_empty(),
            "no finder runs again"
        );
        assert!(flow.pending_finders.is_empty());

        harness.run_until_terminal(&dirs.bucket);
        let flow = harness.flow(&dirs.bucket);
        // Verification ran on the kept finder's output with an arbiter.
        assert!(
            matches!(flow.state, FlowState::Done { .. }),
            "{:?}",
            flow.state
        );
        assert_eq!(
            task_status(&flow, "verifying:arbiter"),
            FlowTaskStatus::Done
        );
        assert!(flow.arena_run_id.is_some());
        let stored = workflow_record::load(&er_engine::store_registry::store(), &scope)
            .unwrap()
            .unwrap();
        assert_eq!(
            stored.stage("verifying:arbiter").unwrap().status,
            StageStatus::Complete
        );
    });
}

/// End-to-end Quick flow with a fake producer CLI and a fake arbiter: the
/// prepared diff, the flow, the seeded run and the arbiter sidecar share one
/// filtered hash, and the verifier leaves the producer's review unchanged.
#[test]
fn quick_flow_pins_filtered_hash_and_leaves_review_json_unchanged() {
    let finding_id = er_engine::ai::finding_key("src/lib.rs", Some(1), "unchecked input");
    let _arena = FakeArena::install(&serde_json::json!({
        "verdicts": [{
            "finding_id": finding_id,
            "verdict": "kept",
            "confidence": 0.95,
            "rationale": "The finding is present in the prepared diff."
        }]
    }));
    let dir = tempfile::tempdir().unwrap();
    let bucket = dirs(dir.path()).bucket;
    // The provider writes into the job's staging directory; the host commits
    // it. The pre-arbiter copy is the staged bytes, taken before the host has
    // committed anything.
    let script = format!(
        "{preamble}\n\
         case \"$1\" in\n\
         *triage.json*) printf '{{\"version\":1,\"diff_hash\":\"%s\",\"verdict\":{{\"primary\":\"general\"}}}}' \"$h\" > \"$out/triage.json\" ;;\n\
         *review.json*) printf '{{\"version\":1,\"diff_hash\":\"%s\",\"files\":{{\"src/lib.rs\":{{\"risk\":\"high\",\"findings\":[{{\"id\":\"fake-1\",\"severity\":\"high\",\"title\":\"unchecked input\",\"description\":\"The value reaches a command.\",\"line_start\":1,\"confidence\":\"confirmed\"}}]}}}}}}' \"$h\" > \"$out/review.json\"; printf '{{\"version\":1,\"diff_hash\":\"%s\",\"order\":[],\"groups\":{{}}}}' \"$h\" > \"$out/order.json\"; printf '{{\"version\":1,\"diff_hash\":\"%s\",\"items\":[]}}' \"$h\" > \"$out/checklist.json\"; printf 'summary\\n' > \"$out/summary.md\"; cp \"$out/review.json\" '{bucket}/review.before-arbiter' ;;\n\
         *) echo \"unexpected review-flow prompt\" >&2; exit 2 ;;\n\
         esac",
        preamble = er_engine::test_support::STAGING_PREAMBLE,
        bucket = bucket,
    );
    let config = with_script(script);
    flow_in(dir.path(), &config, |harness, dirs| {
        let hash = er_engine::ai::compute_diff_hash(RAW);
        let before_calls = er_engine::arena::fake_arena_call_count();

        harness.start("branch").unwrap();
        harness.run_until_terminal(&dirs.bucket);
        let flow = harness.flow(&dirs.bucket);

        let bucket = Path::new(&dirs.bucket);
        let pinned_hash = std::fs::read_to_string(bucket.join(".diff-tmp.sha256")).unwrap();
        assert_eq!(pinned_hash.trim(), hash);
        assert_eq!(flow.diff_hash, hash);
        assert!(
            matches!(
                flow.state,
                FlowState::Done {
                    kept: 1,
                    dropped: 0,
                    awaiting_accept: 0
                }
            ),
            "{:?}",
            flow.state
        );
        let run_id = flow.arena_run_id.as_deref().unwrap();
        let run =
            er_engine::arena::load_run(&er_engine::arena::ArenaPaths::for_run(bucket, run_id))
                .unwrap();
        assert_eq!(
            run.diff_hash, hash,
            "seeded run uses the pinned filtered hash"
        );
        assert_eq!(run.status, RunStatus::Complete);
        assert_eq!(run.config.reviewers.len(), 1);
        assert_eq!(run.config.reviewers[0].provider_id, "fake");
        assert_eq!(
            run.config.reviewers[0].agent_kind.as_deref(),
            Some("general")
        );
        assert_eq!(run.findings[0].verdict, er_engine::arena::Verdict::Kept);
        assert_eq!(flow.successful_finder_providers, ["fake"]);
        assert_eq!(flow.successful_arbiter_provider.as_deref(), Some("fake"));
        assert_eq!(flow.providers_used, ["fake"]);
        let arbiter: er_engine::ai::ArbiterReview =
            serde_json::from_slice(&std::fs::read(bucket.join("arbiter.json")).unwrap()).unwrap();
        assert_eq!(
            arbiter.diff_hash, hash,
            "arbiter sidecar is pinned to the same diff"
        );
        assert_eq!(
            er_engine::arena::fake_arena_call_count() - before_calls,
            1,
            "the seeded flow makes one arbiter call"
        );
        // The committed review went through the typed store, which fills the
        // producer's omitted default fields, so parse both sides and compare
        // their canonical form: the findings and hash must survive the
        // verifier untouched.
        let before: er_engine::ai::ErReview =
            serde_json::from_slice(&std::fs::read(bucket.join("review.before-arbiter")).unwrap())
                .unwrap();
        let stored = er_store::envelope::parse::<er_engine::ai::ErReview>(
            serde_json::from_slice(&std::fs::read(bucket.join("review.json")).unwrap()).unwrap(),
        );
        let after = match stored {
            er_store::Loaded::Found(stored) => stored.value,
            other => panic!("the committed review.json is a stored review, got {other:?}"),
        };
        assert_eq!(
            serde_json::to_value(&after).unwrap(),
            serde_json::to_value(&before).unwrap(),
            "the seeded verifier leaves producer review.json unchanged"
        );
    });
}
