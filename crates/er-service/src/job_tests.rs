//! Jobs on the actor: where they run, that they draw a slot, that a cancel
//! reaches the runner, that a stage launch is a submission and that a load
//! submits nothing. Every test owns its deadline.

use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::ExitStatus;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use er_agent::agent_run::AgentRunHandle;
use er_agent::agent_slots::{self, Workload};
use er_agent::decode::OutputProtocol;
use er_agent::job::{JobKind, JobOutput, JobProgress, JobSpec, OutputContract, RetryPolicy};
use er_agent::provider::ProviderSpec;
use er_engine::arena::{ArenaPaths, ArenaScope, ArenaStartParams, ReviewerRef, RunStatus};
use er_engine::config::{AiModelConfig, AiProviderConfig, ErConfig};
use er_engine::jobs::{JobId, JobLimits, JobSink, JobSubmission, JobTicket};
use er_engine::ports::Ports;
use er_git::FakeGit;
use er_github::FakeGitHub;

use crate::actor::WorkspaceDeps;
use crate::jobs::{ActorJobSink, SPAWNED_ON};
use crate::protocol::{ArenaCommand, ArenaContext, Command, Event, JobOutcome, Reply, TabContext};
use crate::{spawn, WorkspaceHandle};

const DEADLINE: Duration = Duration::from_secs(10);

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

/// A job whose provider is a shell script.
fn script_job(script: &str, work_dir: &Path) -> JobSubmission {
    JobSubmission {
        spec: JobSpec {
            kind: JobKind::Other("test".into()),
            provider: ProviderSpec {
                program: "/bin/sh".into(),
                args: vec!["-c".into(), script.into()],
                env: Vec::new(),
                protocol: OutputProtocol::Plain,
                timeout: Duration::from_secs(60),
            },
            prompt: String::new(),
            work_dir: work_dir.to_string_lossy().into_owned(),
            workload: Workload::Arena,
            output: OutputContract::Stdout,
        },
        limits: JobLimits {
            slot_cap: 4,
            ceiling: 4,
            retry: RetryPolicy::none(),
        },
        handle: AgentRunHandle::new(),
        progress: None,
        label: "test".into(),
    }
}

fn actor_sink(handle: &WorkspaceHandle) -> ActorJobSink {
    ActorJobSink::new(handle.tx.clone())
}

/// Waits for the first event `matches` accepts, failing at the deadline.
fn wait_for(events: &Receiver<Event>, matches: impl Fn(&Event) -> bool) -> Event {
    let deadline = Instant::now() + DEADLINE;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match events.recv_timeout(left) {
            Ok(event) if matches(&event) => return event,
            Ok(_) => {}
            Err(error) => panic!("no matching event within {DEADLINE:?}: {error}"),
        }
    }
}

#[test]
fn a_job_never_spawns_on_the_actor_thread() {
    let dir = tempfile::tempdir().unwrap();
    let handle = spawn(WorkspaceDeps::new(ports()));
    let events = handle.subscribe();
    let ticket = actor_sink(&handle).submit(script_job("echo done", dir.path()));
    let job = ticket.id;

    let finished = wait_for(
        &events,
        |event| matches!(event, Event::JobFinished { job: id, .. } if *id == job),
    );
    assert_eq!(
        finished,
        Event::JobFinished {
            job,
            outcome: JobOutcome::Completed { success: true }
        }
    );
    let spawned = SPAWNED_ON.lock().unwrap();
    let (_, thread) = spawned.iter().find(|(id, _)| *id == job).unwrap();
    assert_eq!(*thread, None, "the job ran on an unnamed effect thread");
}

#[test]
fn an_actor_job_takes_an_agent_slot() {
    let _serial = agent_slots::test_serial();
    let dir = tempfile::tempdir().unwrap();
    let handle = spawn(WorkspaceDeps::new(ports()));
    let ticket = actor_sink(&handle).submit(script_job("sleep 1", dir.path()));

    let (done_tx, done) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = done_tx.send(ticket.wait());
    });
    let mut most = 0;
    let deadline = Instant::now() + DEADLINE;
    let result = loop {
        most = most.max(agent_slots::active_count());
        if let Ok(result) = done.try_recv() {
            break result;
        }
        assert!(Instant::now() < deadline, "the job did not finish");
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(result.is_ok(), "the job ran: {result:?}");
    assert!(most >= 1, "the job held a slot while it ran");
    assert_eq!(agent_slots::active_count(), 0, "the slot was released");
}

#[cfg(test)]
fn pid_alive(pid: &str) -> bool {
    std::process::Command::new("kill")
        .args(["-0", pid])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[test]
fn cancel_job_reaches_the_runner() {
    let dir = tempfile::tempdir().unwrap();
    let pid_file = dir.path().join("pid");
    let handle = spawn(WorkspaceDeps::new(ports()));
    let script = format!("echo $$ > '{}'; sleep 30", pid_file.display());
    let ticket = actor_sink(&handle).submit(script_job(&script, dir.path()));
    let job = ticket.id;

    let deadline = Instant::now() + DEADLINE;
    let pid = loop {
        if let Some(pid) = std::fs::read_to_string(&pid_file)
            .ok()
            .filter(|text| text.ends_with('\n'))
        {
            break pid.trim().to_string();
        }
        assert!(Instant::now() < deadline, "the job never started");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(pid_alive(&pid));

    let reply = handle
        .send_and_wait_with(Command::CancelJob { job }, DEADLINE)
        .unwrap();
    assert_eq!(reply, Reply::JobCancelled { job });
    let deadline = Instant::now() + Duration::from_secs(5);
    while pid_alive(&pid) {
        assert!(
            Instant::now() < deadline,
            "the job's process outlived the cancel"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(matches!(
        ticket.wait(),
        Err(er_agent::job::JobError::Cancelled)
    ));
}

/// Records each submission and answers it with a reviewer's round-one output,
/// so a run's stages can be counted without an agent.
#[derive(Default)]
struct RecordingSink {
    specs: Mutex<Vec<JobSpec>>,
}

impl JobSink for RecordingSink {
    fn submit(&self, job: JobSubmission) -> JobTicket {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/arena/fake/round1.json");
        let text = std::fs::read_to_string(fixture).unwrap_or_default();
        job.handle.mark_finished();
        self.specs.lock().unwrap().push(job.spec);
        let (answer, ticket) = mpsc::channel();
        let _ = answer.send(Ok(JobOutput {
            text,
            stdout: String::new(),
            stderr: String::new(),
            status: ExitStatus::from_raw(0),
        }));
        JobTicket::new(JobId::next(), ticket)
    }
}

fn arena_context(bucket: &str) -> ArenaContext {
    let mut config = ErConfig::default();
    config.ai_hub.providers.insert(
        "fake".to_string(),
        AiProviderConfig {
            command: "fake-provider".to_string(),
            args: vec!["{prompt}".to_string()],
            models: vec![AiModelConfig {
                id: "fake-1".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        },
    );
    ArenaContext {
        tab: TabContext {
            er_dir: bucket.to_string(),
            comments_dir: bucket.to_string(),
            is_remote: false,
            repo_root: bucket.to_string(),
            base_branch: "main".to_string(),
            scope: "branch".to_string(),
            remote_repo: None,
            pr_number: None,
            pr_head_ref: None,
            local_branch_view: Some("feature".to_string()),
            checkout_root: None,
            branch_diff_hash: String::new(),
            diff_hash: String::new(),
        },
        branch_ref: "feature".to_string(),
        config: Arc::new(config),
        buckets: vec![bucket.to_string()],
        review_hash: String::new(),
    }
}

fn fake_ref() -> ReviewerRef {
    ReviewerRef {
        provider_id: "fake".to_string(),
        model_id: "fake-1".to_string(),
        agent_kind: None,
    }
}

/// Polls the run until its record is terminal, so the supervisor is done with
/// the bucket before the test's temp root goes away.
fn wait_terminal(bucket: &str, run_id: &str) {
    let paths = ArenaPaths::for_run(Path::new(bucket), run_id);
    let deadline = Instant::now() + DEADLINE;
    loop {
        if er_engine::arena::load_run(&paths).is_ok_and(|run| {
            matches!(
                run.status,
                RunStatus::Complete | RunStatus::Failed | RunStatus::Cancelled
            )
        }) {
            return;
        }
        assert!(Instant::now() < deadline, "the run did not finish");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_stage_launch_submits_a_job_spec() {
    let dir = tempfile::tempdir().unwrap();
    let bucket = dir.path().join("bucket");
    std::fs::create_dir_all(&bucket).unwrap();
    let bucket = bucket.to_string_lossy().into_owned();
    er_engine::storage::with_process_test_storage_root(dir.path(), || {
        let sink = Arc::new(RecordingSink::default());
        let handle = spawn(WorkspaceDeps::new(ports()).with_jobs(sink.clone()));
        let params = ArenaStartParams {
            title: None,
            reviewers: vec![fake_ref(), fake_ref()],
            scope: ArenaScope::Branch,
            files: None,
            rounds: Some(1),
            arbiter: Some(fake_ref()),
            confirm: true,
            agent_kind: None,
            effort: None,
        };
        let reply = handle
            .send_and_wait_with(
                Command::Arena(ArenaCommand::StartRun {
                    arena: arena_context(&bucket),
                    params,
                }),
                DEADLINE,
            )
            .unwrap();
        let Reply::ArenaStarted { run_ids } = reply else {
            panic!("unexpected reply {reply:?}");
        };
        wait_terminal(&bucket, &run_ids[0]);

        let specs = sink.specs.lock().unwrap();
        assert!(specs.len() >= 2, "one job per reviewer: {}", specs.len());
        for spec in specs.iter() {
            assert_eq!(spec.kind, JobKind::ArenaRound);
            assert_eq!(spec.workload, Workload::Arena);
            assert_eq!(spec.provider.program, "fake-provider");
        }
    });
}

#[test]
fn a_workspace_load_spawns_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let bucket_path = dir.path().join("bucket");
    std::fs::create_dir_all(&bucket_path).unwrap();
    let bucket = bucket_path.to_string_lossy().into_owned();
    er_engine::storage::with_process_test_storage_root(dir.path(), || {
        // An arena run whose owner died mid-round.
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/arena/run-fixture-001/run.json");
        let mut run: er_engine::arena::ArenaRun =
            serde_json::from_str(&std::fs::read_to_string(fixture).unwrap()).unwrap();
        run.status = RunStatus::Running { round: 1 };
        let paths = ArenaPaths::for_run(&bucket_path, &run.id);
        er_engine::arena::save_run(&paths, &run).unwrap();
        // A workflow whose owner died with a stage running.
        er_engine::test_support::write_sidecar_json::<er_engine::ai::workflow_record::WorkflowRecord>(
            &bucket_path,
            serde_json::json!({
                "version": 1, "kind": "deep", "diff_hash": "hash", "started_at": "t0",
                "holder": { "pid": 1, "label": "flow-1" },
                "stages": [{
                    "id": "verifying:arbiter", "stage": "verifying", "label": "Arbiter",
                    "status": "running", "diff_hash": "hash"
                }]
            }),
        );

        let sink = Arc::new(RecordingSink::default());
        let handle = spawn(WorkspaceDeps::new(ports()).with_jobs(sink.clone()));
        let reply = handle
            .send_and_wait_with(
                Command::Reconcile {
                    buckets: vec![bucket.clone()],
                },
                DEADLINE,
            )
            .unwrap();

        assert_eq!(
            reply,
            Reply::Reconciled {
                resumable: vec![bucket.clone()]
            }
        );
        assert!(
            sink.specs.lock().unwrap().is_empty(),
            "a load submits no job"
        );
        let run = er_engine::arena::load_run(&paths).unwrap();
        assert_eq!(run.status, RunStatus::Failed, "the orphaned run is marked");
        let store = er_engine::store_registry::store();
        let scope = er_store::Scope::dir(&bucket).unwrap();
        let record = er_engine::ai::workflow_record::load(&store, &scope)
            .unwrap()
            .unwrap();
        assert_eq!(
            record.stage("verifying:arbiter").unwrap().status,
            er_engine::ai::StageStatus::Interrupted
        );
    });
}

#[test]
fn two_jobs_keep_their_progress_apart() {
    let dir = tempfile::tempdir().unwrap();
    let handle = spawn(WorkspaceDeps::new(ports()));
    let events = handle.subscribe();
    let sink = actor_sink(&handle);
    let first = sink.submit(script_job(
        "for i in 1 2 3; do echo first; done",
        dir.path(),
    ));
    let second = sink.submit(script_job(
        "for i in 1 2 3; do echo second; done",
        dir.path(),
    ));
    let jobs = [(first.id, "first"), (second.id, "second")];

    let mut finished = 0;
    let mut seen = 0;
    while finished < jobs.len() {
        match wait_for(&events, |event| {
            matches!(event, Event::JobProgress { .. } | Event::JobFinished { .. })
        }) {
            Event::JobProgress { job, line } => {
                let Some((_, word)) = jobs.iter().find(|(id, _)| *id == job) else {
                    continue;
                };
                assert_eq!(
                    line,
                    JobProgress::StdoutLine(format!("{word}\n")),
                    "{job}'s line"
                );
                seen += 1;
            }
            Event::JobFinished { job, .. } if jobs.iter().any(|(id, _)| *id == job) => {
                finished += 1;
            }
            _ => {}
        }
    }
    assert_eq!(seen, 6, "every line arrived once, under its own job");
    assert!(first.wait().is_ok() && second.wait().is_ok());
}

#[test]
fn an_estimate_answers_with_its_preview() {
    let dir = tempfile::tempdir().unwrap();
    let bucket = dir.path().to_string_lossy().into_owned();
    let handle = spawn(WorkspaceDeps::new(ports()));
    let reply = handle
        .send_and_wait_with(
            Command::Arena(ArenaCommand::Estimate {
                arena: arena_context(&bucket),
                scope: ArenaScope::Branch,
                files: None,
                reviewers: vec![fake_ref(), fake_ref()],
                rounds: Some(1),
                arbiter: None,
            }),
            DEADLINE,
        )
        .unwrap();
    let Reply::ArenaEstimate(preview) = reply else {
        panic!("unexpected reply {reply:?}");
    };
    assert_eq!(preview.diff_bytes, RAW.len(), "priced over the tab's diff");
}

/// The default tab `er` opens has no branch view and no remote. Its diff comes
/// from `diff_raw`, as `TabState::fetch_tab_raw_diff` fetches it.
#[test]
fn an_estimate_on_a_plain_tab_reads_the_working_diff() {
    let dir = tempfile::tempdir().unwrap();
    let bucket = dir.path().to_string_lossy().into_owned();
    let git = Arc::new(FakeGit::default().with_diff_raw(Ok(RAW.to_string())));
    let ports = Ports {
        git: git.clone(),
        github: Arc::new(FakeGitHub::default()),
    };
    let mut arena = arena_context(&bucket);
    arena.tab.local_branch_view = None;
    let handle = spawn(WorkspaceDeps::new(ports));
    let reply = handle.send_and_wait_with(
        Command::Arena(ArenaCommand::Estimate {
            arena,
            scope: ArenaScope::Branch,
            files: None,
            reviewers: vec![fake_ref(), fake_ref()],
            rounds: Some(1),
            arbiter: None,
        }),
        DEADLINE,
    );
    let preview = match reply {
        Ok(Reply::ArenaEstimate(preview)) => preview,
        other => panic!("a plain tab must estimate: {other:?}"),
    };
    assert_eq!(preview.diff_bytes, RAW.len(), "priced over the tab's diff");
    assert!(
        git.calls().contains(&format!(
            "diff_raw(mode=\"branch\", base=\"main\", repo_root={bucket:?}, head_ref=None)"
        )),
        "fetched through diff_raw: {:?}",
        git.calls()
    );
}

#[test]
fn a_live_run_is_published_in_the_view() {
    let dir = tempfile::tempdir().unwrap();
    let bucket = dir.path().join("bucket");
    std::fs::create_dir_all(&bucket).unwrap();
    let bucket = bucket.to_string_lossy().into_owned();
    er_engine::storage::with_process_test_storage_root(dir.path(), || {
        let sink = Arc::new(RecordingSink::default());
        let handle = spawn(WorkspaceDeps::new(ports()).with_jobs(sink));
        let arena = arena_context(&bucket);
        let params = ArenaStartParams {
            title: None,
            reviewers: vec![fake_ref(), fake_ref()],
            scope: ArenaScope::Branch,
            files: None,
            rounds: Some(1),
            arbiter: Some(fake_ref()),
            confirm: true,
            agent_kind: None,
            effort: None,
        };
        let reply = handle
            .send_and_wait_with(
                Command::Arena(ArenaCommand::StartRun {
                    arena: arena.clone(),
                    params,
                }),
                DEADLINE,
            )
            .unwrap();
        let Reply::ArenaStarted { run_ids } = reply else {
            panic!("unexpected reply {reply:?}");
        };
        let run_id = &run_ids[0];
        wait_terminal(&bucket, run_id);

        // The run's last progress publishes after its record is written.
        let deadline = Instant::now() + DEADLINE;
        let live = loop {
            let view = handle.view();
            if let Some(live) = view.arena_run(run_id).filter(|live| {
                matches!(
                    live.status,
                    RunStatus::Complete | RunStatus::Failed | RunStatus::Cancelled
                )
            }) {
                break live.clone();
            }
            assert!(
                Instant::now() < deadline,
                "the run never reached the view: {:?}",
                view.arena_runs
            );
            std::thread::sleep(Duration::from_millis(20));
        };
        assert_eq!(live.er_dir, bucket);
        let view = handle.view();
        assert_eq!(
            crate::arena::read_progress(&view, run_id),
            live.progress,
            "progress reads from the view"
        );
        let snapshot = crate::arena::read_run(&view, &arena, run_id).unwrap();
        assert_eq!(
            snapshot.run.status, live.status,
            "the snapshot carries the live status"
        );
        let runs = crate::arena::read_runs(&arena).unwrap();
        assert!(runs.iter().any(|run| &run.id == run_id));
    });
}
