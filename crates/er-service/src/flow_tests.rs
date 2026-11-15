//! Review flows on the actor: a Deep flow runs its stages as actor jobs, shows
//! its arena run's live progress, announces its record changes as events and
//! accepts only against a fresh diff of the target it captured; a load shows
//! interrupted flows and starts nothing. Every test owns its deadline.

use std::path::Path;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use er_engine::ai::review_flow::FlowState;
use er_engine::ai::review_md::ReviewMdSource;
use er_engine::arena::{ArenaProgressState, ArenaScope};
use er_engine::config::{
    AiHubConfig, AiModelConfig, AiProviderConfig, ErConfig, ReviewModelGroups, ReviewModelRef,
};
use er_engine::jobs::{JobSink, JobSubmission, JobTicket};
use er_engine::ports::Ports;
use er_engine::review_flow_driver::ReviewFlowInputs;
use er_engine::task_target::BackgroundTaskTarget;
use er_git::FakeGit;
use er_github::FakeGitHub;

use crate::actor::WorkspaceDeps;
use crate::protocol::{AcceptanceDiff, Command, Event, FlowCommand, FlowContext, Reply};
use crate::{spawn, WorkspaceHandle, WorkspaceView};

/// Generous: a Deep flow runs triage, two reviewers over three rounds and an
/// arbiter as real processes.
const DEADLINE: Duration = Duration::from_secs(60);

const RAW: &str = "\
diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,2 +1,3 @@
 fn a() {}
+fn b() {}
 fn c() {}
";

fn ports() -> Ports {
    let git: Arc<dyn er_git::Git> =
        Arc::new(FakeGit::default().with_diff_against_branch(Ok(RAW.to_string())));
    let github: Arc<dyn er_github::GitHub> = Arc::new(FakeGitHub::default());
    Ports { git, github }
}

/// Two fake providers. Triage picks Deep; every arena call waits for `gate`,
/// so a test can look at a run while its reviewers are mid-round.
fn deep_config(bucket: &str, gate: &Path) -> ErConfig {
    let script = format!(
        "case \"$1\" in\n\
         *triage.json*) {preamble}; printf '{{\"version\":1,\"diff_hash\":\"%s\",\"verdict\":{{\"primary\":\"arena\",\"experts\":[\"security\"]}}}}' \"$h\" > \"$out/triage.json\" ;;\n\
         *) while [ ! -f '{gate}' ]; do sleep 0.05; done; printf '{{\"findings\":[],\"ballots\":[],\"verdicts\":[]}}' ;;\n\
         esac",
        preamble = er_engine::test_support::STAGING_PREAMBLE,
        gate = gate.display(),
    );
    let provider = AiProviderConfig {
        command: "/bin/sh".into(),
        args: vec![
            "-c".into(),
            er_engine::test_support::staged_script(&script, bucket),
            "fake-flow".into(),
            "{prompt}".into(),
        ],
        models: vec![AiModelConfig {
            id: "fake-model".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    let model = |provider_id: &str| ReviewModelRef {
        provider_id: provider_id.into(),
        model_id: "fake-model".into(),
    };
    let ai_hub = AiHubConfig {
        default_provider: Some("fake".into()),
        providers: [
            ("fake".into(), provider.clone()),
            ("fake-second".into(), provider),
        ]
        .into(),
        review_models: ReviewModelGroups {
            fast: vec![model("fake")],
            standard: vec![model("fake")],
            advanced: vec![model("fake"), model("fake-second")],
        },
        max_concurrent_reviews: 16,
        max_concurrent_agents: 16,
        ..Default::default()
    };
    ErConfig {
        ai_hub,
        ..Default::default()
    }
}

/// The view the App's rules would build for a local branch tab.
fn view_of(repo: &str, bucket: &str) -> ReviewFlowInputs {
    ReviewFlowInputs {
        target: BackgroundTaskTarget {
            repo_root: repo.into(),
            er_dir: bucket.into(),
            branch_label: "feature".into(),
            base_branch: "main".into(),
            scope: "branch".into(),
            pr_number: None,
            remote_repo: None,
            managed_local: true,
        },
        arena_scope: ArenaScope::Branch,
        repo_root: repo.into(),
        reviewed_ref_note: None,
        branch_ref: "feature".into(),
        base_branch: "main".into(),
        raw_diff: RAW.into(),
        owned_rules: Default::default(),
        review_md_source: ReviewMdSource::Git {
            repo_root: repo.into(),
            base_ref: "main".into(),
        },
    }
}

fn context(config: &ErConfig, view: ReviewFlowInputs) -> FlowContext {
    FlowContext {
        config: Arc::new(config.clone()),
        effort: None,
        view: Ok(view),
    }
}

/// Every event the actor sent, kept so a test can ask what arrived.
struct Events {
    events: Receiver<Event>,
    seen: Vec<Event>,
}

impl Events {
    /// Waits, on events only, until `done` holds for the published view.
    fn until(
        &mut self,
        handle: &WorkspaceHandle,
        what: &str,
        done: impl Fn(&WorkspaceView) -> bool,
    ) {
        let deadline = Instant::now() + DEADLINE;
        while !done(&handle.view()) {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.events.recv_timeout(left) {
                Ok(event) => self.seen.push(event),
                Err(error) => panic!("{what} within {DEADLINE:?}: {error}"),
            }
        }
    }

    /// Waits for an event `matches` accepts, failing at the deadline.
    fn event(&mut self, what: &str, matches: impl Fn(&Event) -> bool) {
        let deadline = Instant::now() + DEADLINE;
        while !self.seen.iter().any(&matches) {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.events.recv_timeout(left) {
                Ok(event) => self.seen.push(event),
                Err(error) => panic!("{what} within {DEADLINE:?}: {error}"),
            }
        }
    }
}

fn flow_state(view: &WorkspaceView, bucket: &str) -> Option<FlowState> {
    view.review_flow(bucket).map(|live| live.flow.state.clone())
}

struct DeepRun {
    handle: WorkspaceHandle,
    events: Events,
    config: ErConfig,
    repo: String,
    bucket: String,
    run_id: String,
}

/// Starts a Deep flow through the actor, confirms it, checks the arena run is
/// live in the published view, then lets the reviewers answer and waits for
/// the flow to finish.
fn run_deep_flow(dir: &Path) -> DeepRun {
    let repo = dir.join("repo");
    let bucket_path = dir.join("bucket");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::create_dir_all(&bucket_path).unwrap();
    let repo = repo.to_string_lossy().into_owned();
    let bucket = bucket_path.to_string_lossy().into_owned();
    let gate = dir.join("gate");
    let config = deep_config(&bucket, &gate);
    let handle = spawn(WorkspaceDeps::new(ports()));
    let mut events = Events {
        events: handle.subscribe(),
        seen: Vec::new(),
    };

    let started = handle
        .send_and_wait_with(
            Command::Flow(FlowCommand::Start {
                context: context(&config, view_of(&repo, &bucket)),
                scope: "branch".into(),
            }),
            DEADLINE,
        )
        .unwrap();
    assert_eq!(
        started,
        Reply::FlowChanged {
            er_dir: bucket.clone()
        }
    );
    events.until(&handle, "Deep triage asks for confirmation", |view| {
        flow_state(view, &bucket) == Some(FlowState::AwaitingConfirmation)
    });
    // Only the triage stage has run so far.
    events.event("the triage stage ran as an actor job", |event| {
        matches!(event, Event::JobFinished { .. })
    });
    handle
        .send_and_wait_with(
            Command::Flow(FlowCommand::Confirm {
                context: context(&config, view_of(&repo, &bucket)),
                er_dir: bucket.clone(),
            }),
            DEADLINE,
        )
        .unwrap();

    events.until(
        &handle,
        "the flow's arena run is live with progress",
        |view| {
            view.review_flow(&bucket)
                .and_then(|live| live.flow.arena_run_id.as_deref())
                .and_then(|run_id| view.arena_run(run_id))
                .is_some_and(|run| run.progress != ArenaProgressState::default())
        },
    );
    let run_id = handle
        .view()
        .review_flow(&bucket)
        .and_then(|live| live.flow.arena_run_id.clone())
        .unwrap();

    std::fs::write(&gate, "").unwrap();
    events.until(&handle, "the Deep flow finishes", |view| {
        matches!(flow_state(view, &bucket), Some(FlowState::Done { .. }))
            && !view.review_flow(&bucket).is_some_and(|live| live.draining)
    });
    DeepRun {
        handle,
        events,
        config,
        repo,
        bucket,
        run_id,
    }
}

/// The whole Deep flow on the flag path: triage runs as an actor job, the
/// arena run shows live progress in the published view (the reader the
/// desktop's arena page uses), and each record change reaches subscribers as
/// an event with no poll. The published flow and target answer a run read.
#[test]
fn a_deep_flow_runs_on_the_actor_with_live_progress_and_events() {
    let _serial = er_agent::agent_slots::test_serial();
    let dir = tempfile::tempdir().unwrap();
    er_engine::storage::with_process_test_storage_root(dir.path(), || {
        let mut run = run_deep_flow(dir.path());
        let bucket = run.bucket.clone();
        let run_id = run.run_id.clone();

        assert!(
            run.events.seen.iter().any(
                |event| matches!(event, Event::WorkflowChanged { er_dir, .. } if *er_dir == bucket)
            ),
            "a stage change is announced"
        );
        run.events.event("the panel record is announced", |event| {
            matches!(event, Event::PanelRunChanged { er_dir, run_id: id }
                if *er_dir == bucket && *id == run_id)
        });

        let snapshot =
            crate::read_flow_run(&run.handle.view(), &run.config, &bucket, &run_id).unwrap();
        assert_eq!(snapshot.run.id, run_id);
    });
}

/// Acceptance checks the run against the fresh diff the command carries for
/// the run's target: another target is refused before anything is read, a
/// fresh diff that moved on is refused even while the view the flow started
/// from still holds the old one, and the same diff gets past the match to the
/// selection check.
#[test]
fn flow_acceptance_checks_the_fresh_diff_for_the_run_target() {
    let _serial = er_agent::agent_slots::test_serial();
    let dir = tempfile::tempdir().unwrap();
    er_engine::storage::with_process_test_storage_root(dir.path(), || {
        let run = run_deep_flow(dir.path());
        let accept = |target: BackgroundTaskTarget, diff: &str| {
            run.handle.send_and_wait_with(
                Command::Flow(FlowCommand::AcceptFindings {
                    er_dir: run.bucket.clone(),
                    run_id: run.run_id.clone(),
                    finding_ids: vec!["missing".into()],
                    current: Some(AcceptanceDiff {
                        target,
                        diff: Ok(diff.into()),
                    }),
                }),
                DEADLINE,
            )
        };
        let target = view_of(&run.repo, &run.bucket).target;

        let mut elsewhere = target.clone();
        elsewhere.branch_label = "another-branch".into();
        let refused = accept(elsewhere, RAW).unwrap_err().to_string();
        assert!(refused.contains("no longer open"), "{refused}");

        let moved_on = format!("{RAW}+fn d() {{}}\n");
        let stale = accept(target.clone(), &moved_on).unwrap_err().to_string();
        assert!(stale.contains("The diff changed"), "{stale}");

        let matched = accept(target, RAW).unwrap_err().to_string();
        assert!(matched.contains("Selection contains"), "{matched}");
    });
}

/// Counts submissions; a load must make none.
#[derive(Default)]
struct CountingSink {
    submitted: Mutex<usize>,
}

impl JobSink for CountingSink {
    fn submit(&self, job: JobSubmission) -> JobTicket {
        *self.submitted.lock().unwrap() += 1;
        job.handle.mark_finished();
        let (_answer, ticket) = std::sync::mpsc::channel();
        JobTicket::new(er_engine::jobs::JobId::next(), ticket)
    }
}

/// A record of `kind` (`None` for one triage never classified), whose finding
/// stage was running when its owner died.
fn write_record(bucket: &Path, kind: Option<&str>, outcome: Option<&str>) {
    std::fs::create_dir_all(bucket).unwrap();
    let mut record = serde_json::json!({
        "version": 1, "diff_hash": "hash", "started_at": "t0",
        "holder": { "pid": 1, "label": "flow-1" },
        "stages": [
            { "id": "triage", "stage": "triage", "label": "Triage",
              "status": "complete", "diff_hash": "hash" },
            { "id": "finding:security", "stage": "finding", "label": "Security",
              "status": "running", "diff_hash": "hash" }
        ]
    });
    if let Some(kind) = kind {
        record["kind"] = kind.into();
    }
    if let Some(outcome) = outcome {
        record["outcome"] = outcome.into();
    }
    er_engine::test_support::write_sidecar_json::<er_engine::ai::workflow_record::WorkflowRecord>(
        bucket, record,
    );
    // The prepared diff the record was planned against is still in place.
    std::fs::write(bucket.join(".diff-tmp.sha256"), "hash").unwrap();
}

/// A load over a dead owner's flow shows it interrupted, with the stages its
/// re-run would run again, and keeps a cancelled flow cancelled. A Deep or
/// kind-less record shows no stages: its re-run starts over. It submits no
/// job: resuming is an explicit re-run.
#[test]
fn a_load_shows_interrupted_flows_and_starts_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let bucket = |name: &str| dir.path().join(name);
    let path = |name: &str| bucket(name).to_string_lossy().into_owned();
    let (interrupted_bucket, cancelled_bucket) = (path("interrupted"), path("cancelled"));
    let (deep_bucket, kindless_bucket) = (path("deep"), path("kindless"));
    er_engine::storage::with_process_test_storage_root(dir.path(), || {
        write_record(&bucket("interrupted"), Some("standard"), None);
        write_record(&bucket("cancelled"), Some("standard"), Some("cancelled"));
        write_record(&bucket("deep"), Some("deep"), None);
        write_record(&bucket("kindless"), None, None);
        let sink = Arc::new(CountingSink::default());
        let handle = spawn(WorkspaceDeps::new(ports()).with_jobs(sink.clone()));
        let mut events = Events {
            events: handle.subscribe(),
            seen: Vec::new(),
        };

        let reply = handle
            .send_and_wait_with(
                Command::Reconcile {
                    buckets: vec![
                        interrupted_bucket.clone(),
                        cancelled_bucket.clone(),
                        deep_bucket.clone(),
                        kindless_bucket.clone(),
                    ],
                },
                DEADLINE,
            )
            .unwrap();

        assert_eq!(
            reply,
            Reply::Reconciled {
                resumable: vec![
                    interrupted_bucket.clone(),
                    deep_bucket.clone(),
                    kindless_bucket.clone()
                ]
            }
        );
        events.until(&handle, "the interrupted flows are published", |view| {
            view.review_flows.len() == 3
        });
        let view = handle.view();
        let live = view.review_flow(&interrupted_bucket).unwrap();
        assert!(matches!(live.flow.state, FlowState::Interrupted { .. }));
        assert_eq!(live.rerun_stages, vec!["finding:security".to_string()]);
        for starts_over in [&deep_bucket, &kindless_bucket] {
            let live = view.review_flow(starts_over).unwrap();
            assert!(matches!(live.flow.state, FlowState::Interrupted { .. }));
            assert!(
                live.rerun_stages.is_empty(),
                "a re-run of {starts_over} starts over, so it names no stages"
            );
        }
        assert!(view.review_flow(&cancelled_bucket).is_none());
        let store = er_engine::store_registry::store();
        let scope = er_store::Scope::dir(&cancelled_bucket).unwrap();
        let record = er_engine::ai::workflow_record::load(&store, &scope)
            .unwrap()
            .unwrap();
        assert_eq!(
            record.outcome,
            Some(er_engine::ai::workflow_record::WorkflowOutcome::Cancelled),
            "a cancelled flow stays cancelled"
        );
        assert_eq!(*sink.submitted.lock().unwrap(), 0, "a load submits no job");
    });
}
