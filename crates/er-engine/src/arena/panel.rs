//! The Panel stage: every reviewer runs as one job on the shared runner.
//!
//! A Deep run's reviewer rounds are `er_agent::JobRunner` jobs: the runner
//! owns the slot, the process group, the deadline and the final-text decode.
//! Two consequences of that are the reason for it — a cancel signals the whole
//! process group, so a reviewer's grandchildren die with it, and a failed
//! spawn is a job error that fails the reviewer instead of leaving it waiting
//! forever.
//!
//! Round 1 proposes findings; each later cross round votes on the findings the
//! previous rounds left standing. A round is atomic: every reviewer votes on
//! one snapshot of the findings, and the votes are applied once the whole
//! round has returned.

use super::adapter::{extract_json_from_text, fake_arena_json_from_dir, resolve_provider_command};
use super::model::{Reviewer, ReviewerRunStatus, RunStatus, Vote};
use super::orchestrator::{
    emit, record_reviewer_failure, Round1ParallelOutcome, Round2ParallelOutcome, SupervisorCtx,
};
use super::schema::{validate_round1_output, validate_round2_output, Round1Output, Round2Output};
use super::storage::{save_round_output, ArenaPaths, ProgressEvent};
use super::voting::quorum_from_findings;
use crate::agent_runtime::OutputProtocol;
use crate::agent_slots::Workload;
use crate::agent_timing::AgentRunTimer;
use crate::ai::panel_run::{self, PanelReviewer, PanelRun};
use crate::ai::prompts::{
    build_arena_round1_prompt_agent, build_arena_round2_prompt, with_reviewed_ref_note,
};
use crate::config::ErConfig;
use anyhow::Result;
use er_agent::agent_run::AgentRunHandle;
use er_agent::job::{JobError, JobKind, JobRun, JobRunner, JobSpec, OutputContract};
use er_agent::provider::ProviderSpec;
use er_store::Artifact;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// How often the cancel bridge checks the run flag.
///
/// Short enough that a user stop reaches every job in a round within a poll
/// interval, long enough not to spin while a round of reviewers thinks.
const CANCEL_POLL: Duration = Duration::from_millis(20);

/// What a worker thread needs: owned copies, so the jobs outlive the borrow
/// of the supervisor's context.
#[derive(Clone)]
struct JobCtx {
    config: ErConfig,
    repo_root: String,
    paths: ArenaPaths,
    registry: super::registry::ArenaRegistry,
    cancel: Arc<AtomicBool>,
    reviewed_ref_note: Option<String>,
    patch_path: String,
    effort: Option<String>,
    er_dir: String,
}

/// The panel's view of one arena run.
#[derive(Clone)]
pub(super) struct Panel {
    ctx: Arc<JobCtx>,
}

impl Panel {
    pub(super) fn new(ctx: &SupervisorCtx<'_>, patch_path: &str, effort: Option<&str>) -> Self {
        Self {
            ctx: Arc::new(JobCtx {
                config: ctx.config.clone(),
                repo_root: ctx.repo_root.to_string(),
                paths: ctx.paths.clone(),
                registry: ctx.registry.clone(),
                cancel: Arc::clone(ctx.cancel),
                reviewed_ref_note: ctx.reviewed_ref_note.map(str::to_owned),
                patch_path: patch_path.to_string(),
                effort: effort.map(str::to_owned),
                er_dir: ctx.er_dir.to_string(),
            }),
        }
    }

    /// Round 1: every reviewer proposes findings independently.
    pub(super) fn round1(&self, reviewers: &[Reviewer]) -> Result<Round1ParallelOutcome> {
        let outcome = self.run_round(Round::Propose, reviewers);
        Ok(Round1ParallelOutcome {
            ok: outcome.proposals,
            failed: outcome.failed,
            cancelled: outcome.cancelled,
        })
    }

    /// A cross round: every surviving reviewer votes on the findings the
    /// previous rounds produced. The caller applies the ballots once the whole
    /// round returns, so a vote never lands on a moving finding set.
    pub(super) fn cross_round(
        &self,
        round: u8,
        findings_json: &str,
        reviewers: &[Reviewer],
    ) -> Result<Round2ParallelOutcome> {
        let outcome = self.run_round(
            Round::Cross {
                round,
                findings: findings_json.to_string(),
            },
            reviewers,
        );
        Ok(Round2ParallelOutcome {
            ok: outcome.ballots,
            failed: outcome.failed,
            cancelled: outcome.cancelled,
        })
    }

    /// Write the panel's durable record once every reviewer round has returned.
    pub(super) fn record(&self, run: &super::model::ArenaRun) -> Result<()> {
        let scope = er_store::Scope::dir(&self.ctx.er_dir)?;
        let record = panel_run_record(run);
        panel_run::save(&crate::store_registry::store(), &scope, &record)?;
        Ok(())
    }

    fn run_round(&self, round: Round, reviewers: &[Reviewer]) -> RoundOutcome {
        let round_number = round.number();
        let arena_cap = self
            .ctx
            .config
            .ai_hub
            .effective_max_concurrent_arena_reviews();
        let worker_count = reviewers.len().min(arena_cap.max(1));
        let queue = Arc::new(Mutex::new(
            reviewers
                .iter()
                .cloned()
                .collect::<std::collections::VecDeque<_>>(),
        ));
        let sink = RoundSink::new();
        let cancelled = Arc::new(AtomicBool::new(false));
        let bridge = CancelBridge::start(Arc::clone(&self.ctx.cancel));
        let review_md = std::fs::read_to_string(self.ctx.paths.review_md()).ok();

        let mut handles = Vec::new();
        for _ in 0..worker_count {
            let queue = Arc::clone(&queue);
            let sink = sink.clone();
            let cancelled = Arc::clone(&cancelled);
            let bridge = bridge.clone();
            let round = round.clone();
            let review_md = review_md.clone();
            let panel = self.clone();
            handles.push(thread::spawn(move || loop {
                if panel.ctx.cancel.load(Ordering::SeqCst) {
                    cancelled.store(true, Ordering::SeqCst);
                    return;
                }
                let Some(reviewer) = queue.lock().unwrap_or_else(|e| e.into_inner()).pop_front()
                else {
                    return;
                };
                let prompt = panel.prompt(&round, &reviewer, review_md.as_deref());
                panel.emit(ProgressEvent::ReviewerThinking {
                    reviewer_id: reviewer.id.clone(),
                    round: round_number,
                });
                match panel.run_job(&reviewer, &prompt, &bridge) {
                    Ok(value) => {
                        panel.record_output(&round, round_number, &reviewer, value, &sink);
                    }
                    Err(JobFailure::Cancelled) => {
                        cancelled.store(true, Ordering::SeqCst);
                        return;
                    }
                    Err(JobFailure::Failed(reason)) => record_reviewer_failure(
                        &panel.ctx.paths,
                        &sink.failed,
                        panel.ctx.registry.notify.as_ref(),
                        &reviewer.id,
                        round_number,
                        reason,
                    ),
                }
            }));
        }

        for handle in handles {
            if handle.join().is_err() {
                sink.failed.lock().unwrap_or_else(|e| e.into_inner()).push((
                    "<panel>".to_string(),
                    "panel reviewer thread panicked".to_string(),
                ));
            }
        }

        let (proposals, ballots, failed) = sink.take();
        RoundOutcome {
            proposals,
            ballots,
            failed,
            cancelled: cancelled.load(Ordering::SeqCst),
        }
    }

    fn prompt(&self, round: &Round, reviewer: &Reviewer, review_md: Option<&str>) -> String {
        let prompt = match round {
            Round::Propose => build_arena_round1_prompt_agent(
                &self.ctx.patch_path,
                &reviewer.name,
                reviewer.agent_kind.as_deref(),
                review_md,
            ),
            Round::Cross { round, findings } => {
                build_arena_round2_prompt(&self.ctx.patch_path, &reviewer.id, *round, findings)
            }
        };
        with_reviewed_ref_note(prompt, self.ctx.reviewed_ref_note.as_deref())
    }

    /// Validate one reviewer's job output, save it and announce it. A reviewer
    /// whose output fails its round's schema is a failed reviewer, not a
    /// crashed panel.
    fn record_output(
        &self,
        round: &Round,
        round_number: u8,
        reviewer: &Reviewer,
        value: Value,
        sink: &RoundSink,
    ) {
        let count = match round {
            Round::Propose => match validate_round1_output(&value) {
                Ok(out) => {
                    let count = out.findings.len();
                    sink.proposals
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .push((reviewer.id.clone(), out));
                    count
                }
                Err(error) => {
                    record_reviewer_failure(
                        &self.ctx.paths,
                        &sink.failed,
                        self.ctx.registry.notify.as_ref(),
                        &reviewer.id,
                        round_number,
                        error.to_string(),
                    );
                    return;
                }
            },
            Round::Cross { .. } => match validate_round2_output(&value) {
                Ok(out) => {
                    let count = out.ballots.len();
                    sink.ballots
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .push((reviewer.id.clone(), out));
                    count
                }
                Err(error) => {
                    record_reviewer_failure(
                        &self.ctx.paths,
                        &sink.failed,
                        self.ctx.registry.notify.as_ref(),
                        &reviewer.id,
                        round_number,
                        error.to_string(),
                    );
                    return;
                }
            },
        };
        let _ = save_round_output(&self.ctx.paths, round_number, &reviewer.id, &value);
        self.emit(ProgressEvent::ReviewerDone {
            reviewer_id: reviewer.id.clone(),
            round: round_number,
            findings_count: count,
        });
    }

    fn emit(&self, event: ProgressEvent) {
        emit(&self.ctx.registry, &self.ctx.paths, &event);
    }

    fn run_job(
        &self,
        reviewer: &Reviewer,
        prompt: &str,
        bridge: &CancelBridge,
    ) -> std::result::Result<Value, JobFailure> {
        // The arena's existing test hook: when a fixture directory is set, the
        // reviewer "answers" from it without spawning anything.
        if let Some(dir) = crate::env_var::var("ER_FAKE_ARENA_DIR") {
            return fake_arena_json_from_dir(&dir)
                .map_err(|error| JobFailure::Failed(error.to_string()));
        }
        let storage_dir = self.ctx.paths.root.to_string_lossy().into_owned();
        let command = resolve_provider_command(
            &self.ctx.config.ai_hub,
            &reviewer.provider_id,
            &reviewer.model_id,
            self.ctx.effort.as_deref(),
            Some(storage_dir.as_str()),
        )
        .map_err(|error| JobFailure::Failed(error.to_string()))?;
        let spec = JobSpec {
            kind: JobKind::ArenaRound,
            provider: ProviderSpec {
                program: command.command,
                args: command.args,
                env: command.env,
                protocol: if command.stream_json {
                    OutputProtocol::ClaudeStreamJson
                } else {
                    OutputProtocol::Plain
                },
                timeout: self.ctx.config.ai_hub.effective_agent_timeout(),
            },
            prompt: prompt.to_string(),
            work_dir: self.ctx.repo_root.clone(),
            workload: Workload::Arena,
            output: OutputContract::Stdout,
        };
        // The shared pool, charged as arena work: a panel reviewer must not
        // take a background review's slot.
        let runner = JobRunner::new(
            self.ctx
                .config
                .ai_hub
                .effective_max_concurrent_arena_reviews(),
            self.ctx.config.ai_hub.effective_max_concurrent_agents(),
        );
        let handle = AgentRunHandle::new();
        bridge.register(&handle);
        let mut timer = AgentRunTimer::start();
        let output = runner.run_controlled(
            &spec,
            JobRun {
                handle,
                progress: None,
                timer: Some(&mut timer),
            },
        );
        match output {
            Ok(output) => extract_json_from_text(&output.text)
                .map_err(|error| JobFailure::Failed(error.to_string())),
            Err(JobError::Cancelled) => Err(JobFailure::Cancelled),
            Err(error) => Err(JobFailure::Failed(error.to_string())),
        }
    }
}

/// One reviewer's named result in a round, as the supervisor records failures
/// and consumes outputs.
type ReviewerOutput<T> = (String, T);
type ReviewerFailure = ReviewerOutput<String>;

/// The collectors one round's workers push their results into.
///
/// Kept together so a worker carries one owned handle rather than three, and
/// so the round hands its results back in one step.
#[derive(Clone)]
struct RoundSink {
    proposals: Arc<Mutex<Vec<ReviewerOutput<Round1Output>>>>,
    ballots: Arc<Mutex<Vec<ReviewerOutput<Round2Output>>>>,
    failed: Arc<Mutex<Vec<ReviewerFailure>>>,
}

impl RoundSink {
    fn new() -> Self {
        Self {
            proposals: Arc::new(Mutex::new(Vec::new())),
            ballots: Arc::new(Mutex::new(Vec::new())),
            failed: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// The round's results, taken once every worker has joined: the sinks are
    /// the only remaining handles, so ownership unwraps cleanly.
    fn take(
        self,
    ) -> (
        Vec<ReviewerOutput<Round1Output>>,
        Vec<ReviewerOutput<Round2Output>>,
        Vec<ReviewerFailure>,
    ) {
        (
            take_one(self.proposals),
            take_one(self.ballots),
            take_one(self.failed),
        )
    }
}

fn take_one<T>(value: Arc<Mutex<Vec<T>>>) -> Vec<T> {
    Arc::try_unwrap(value)
        .map(|mutex| mutex.into_inner().unwrap_or_else(|e| e.into_inner()))
        .unwrap_or_default()
}

/// Which round a worker is running.
#[derive(Clone)]
enum Round {
    Propose,
    Cross { round: u8, findings: String },
}

impl Round {
    const fn number(&self) -> u8 {
        match self {
            Self::Propose => 1,
            Self::Cross { round, .. } => *round,
        }
    }
}

/// A worker's job failure. Cancellation is kept apart from a failure so a
/// stopped round does not mark its reviewers failed.
enum JobFailure {
    Cancelled,
    Failed(String),
}

struct RoundOutcome {
    proposals: Vec<(String, Round1Output)>,
    ballots: Vec<(String, Round2Output)>,
    failed: Vec<(String, String)>,
    cancelled: bool,
}

/// Makes the run's stop flag reach the runner's handles.
///
/// `JobRunner` cancels a job when its `AgentRunHandle` is killed, and the
/// handle is the only thing that can signal the process group. The arena's
/// cancel arrives as the run's shared flag, so one watcher carries it across
/// for the whole round. It keeps polling until the round ends rather than
/// exiting after the first kill: a job that registers just after a kill still
/// has to see a cancelled run on the watcher's next pass.
#[derive(Clone)]
struct CancelBridge(Arc<BridgeInner>);

struct BridgeInner {
    handles: Arc<Mutex<Vec<Arc<AgentRunHandle>>>>,
    stop: Arc<AtomicBool>,
    join: Mutex<Option<thread::JoinHandle<()>>>,
}

impl CancelBridge {
    fn start(cancel: Arc<AtomicBool>) -> Self {
        let handles: Arc<Mutex<Vec<Arc<AgentRunHandle>>>> = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let watched = Arc::clone(&handles);
        let stopped = Arc::clone(&stop);
        let join = thread::spawn(move || {
            while !stopped.load(Ordering::SeqCst) {
                if cancel.load(Ordering::SeqCst) {
                    let handles = watched.lock().unwrap_or_else(|e| e.into_inner());
                    for handle in handles.iter() {
                        handle.kill();
                    }
                }
                thread::sleep(CANCEL_POLL);
            }
        });
        Self(Arc::new(BridgeInner {
            handles,
            stop,
            join: Mutex::new(Some(join)),
        }))
    }

    fn register(&self, handle: &Arc<AgentRunHandle>) {
        self.0
            .handles
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(Arc::clone(handle));
    }
}

impl Drop for BridgeInner {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(join) = self.join.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = join.join();
        }
    }
}

/// Build the durable record from the run's final findings and reviewer states.
///
/// The round-1 votes are the proposals already recorded on each finding; the
/// round-2 votes are the first cross round's ballots. Both are read from the
/// same finding set the arbiter will judge, so the measurement cannot
/// disagree with what was graded.
pub(super) fn panel_run_record(run: &super::model::ArenaRun) -> PanelRun {
    let round1 = quorum_from_findings(&run.findings, 1);
    let round2 = (run.config.rounds >= 2).then(|| quorum_from_findings(&run.findings, 2));
    let changed = round2.as_ref().is_some_and(|round2| *round2 != round1);

    let mut proposed: BTreeMap<&str, usize> = BTreeMap::new();
    let mut voted: BTreeMap<&str, usize> = BTreeMap::new();
    for finding in &run.findings {
        for log in &finding.rounds {
            for ballot in &log.log {
                match log.n {
                    1 if ballot.vote == Vote::Propose => {
                        *proposed.entry(ballot.reviewer.as_str()).or_default() += 1;
                    }
                    2 => {
                        *voted.entry(ballot.reviewer.as_str()).or_default() += 1;
                    }
                    _ => {}
                }
            }
        }
    }
    let reviewers = run
        .reviewers
        .iter()
        .map(|reviewer| PanelReviewer {
            id: reviewer.id.clone(),
            provider_id: reviewer.provider_id.clone(),
            model_id: reviewer.model_id.clone(),
            round1_findings: proposed.get(reviewer.id.as_str()).copied().unwrap_or(0),
            round1_error: match &reviewer.status {
                ReviewerRunStatus::Failed { reason } => Some(reason.clone()),
                ReviewerRunStatus::Ok => None,
            },
            round2_votes: voted.get(reviewer.id.as_str()).copied().unwrap_or(0),
        })
        .collect();

    PanelRun {
        version: PanelRun::VERSION,
        run_id: run.id.clone(),
        diff_hash: run.diff_hash.clone(),
        started_at: run.created_at.clone(),
        rounds: run.config.rounds,
        reviewers,
        round1_quorum: round1,
        round2_quorum: round2,
        quorum_changed: changed,
    }
}

/// The run's status as one of the record's outcomes.
///
/// Anything that is not a clean completion or a user stop is a failure: a
/// supervisor that unwound left the run somewhere it never meant to stop.
pub(super) fn outcome_for(run: &super::model::ArenaRun) -> crate::ai::WorkflowOutcome {
    match run.status {
        RunStatus::Complete => crate::ai::WorkflowOutcome::Complete,
        RunStatus::Cancelled => crate::ai::WorkflowOutcome::Cancelled,
        RunStatus::Queued | RunStatus::Running { .. } | RunStatus::Failed => {
            crate::ai::WorkflowOutcome::Failed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::model::{
        ArenaConfig, ArenaRun, CostEstimate, ReviewerKind, ReviewerRef, ReviewerRunStatus,
    };
    use crate::arena::orchestrator::start_review_flow_run;
    use crate::arena::registry::ArenaRegistry;
    use crate::arena::storage::{load_run, ProgressEvent};
    use crate::arena::{ArenaRunInputs, ArenaScope, ArenaStartParams};
    use crate::config::{AiHubConfig, AiModelConfig, AiProviderConfig};
    use std::ffi::OsString;
    use std::path::Path;
    use std::time::Instant;

    struct Harness {
        panel: Panel,
        cancel: Arc<AtomicBool>,
    }

    /// Serializes every test that reads the arena's global test hook or starts
    /// a supervisor, so one test's fake fixtures cannot answer another's jobs.
    struct ArenaEnv {
        _serial: crate::agent_slots::TestSerialGuard,
        _lock: std::sync::MutexGuard<'static, ()>,
        previous: Option<OsString>,
    }

    impl ArenaEnv {
        fn take() -> Self {
            let serial = crate::agent_slots::test_serial();
            let lock = crate::arena::ARENA_TEST_ENV_LOCK
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let previous = std::env::var_os("ER_FAKE_ARENA_DIR");
            std::env::remove_var("ER_FAKE_ARENA_DIR");
            Self {
                _serial: serial,
                _lock: lock,
                previous,
            }
        }

        fn with_fixtures(mut self) -> (Self, tempfile::TempDir) {
            let fixture = tempfile::tempdir().unwrap();
            for name in ["round1.json", "round2.json", "round3.json"] {
                // Every response satisfies every round schema, so concurrent
                // reviewers can consume the fixtures in any order.
                std::fs::write(
                    fixture.path().join(name),
                    br#"{"findings":[],"ballots":[],"verdicts":[]}"#,
                )
                .unwrap();
            }
            std::env::set_var("ER_FAKE_ARENA_DIR", fixture.path());
            self.previous = None;
            (self, fixture)
        }
    }

    impl Drop for ArenaEnv {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(value) => std::env::set_var("ER_FAKE_ARENA_DIR", value),
                None => std::env::remove_var("ER_FAKE_ARENA_DIR"),
            }
        }
    }

    fn config(command: &str, args: Vec<String>, models: &[&str]) -> ErConfig {
        let provider = AiProviderConfig {
            command: command.into(),
            args,
            models: models
                .iter()
                .map(|id| AiModelConfig {
                    id: (*id).into(),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        let mut hub = AiHubConfig::default();
        hub.providers.insert("fake".into(), provider);
        hub.default_provider = Some("fake".into());
        hub.max_concurrent_reviews = 4;
        hub.max_concurrent_arena_reviews = 4;
        hub.max_concurrent_agents = 4;
        ErConfig {
            ai_hub: hub,
            ..Default::default()
        }
    }

    fn reviewer(id: &str) -> Reviewer {
        Reviewer {
            id: id.into(),
            name: id.into(),
            kind: ReviewerKind::Model,
            provider_id: "fake".into(),
            model_id: "m1".into(),
            system_prompt: String::new(),
            color: String::new(),
            icon: String::new(),
            tagline: String::new(),
            cost_per_1k_in: 0.0,
            cost_per_1k_out: 0.0,
            avg_latency_ms: 0,
            status: ReviewerRunStatus::Ok,
            agent_kind: Some("general".into()),
        }
    }

    fn reviewer_ref(model: &str) -> ReviewerRef {
        ReviewerRef {
            provider_id: "fake".into(),
            model_id: model.into(),
            agent_kind: Some("general".into()),
        }
    }

    fn harness(bucket: &Path, config: &ErConfig, run_id: &str) -> Harness {
        let registry = ArenaRegistry::new(Arc::new(|| {}));
        let paths = ArenaPaths::for_run(bucket, run_id);
        paths.ensure_dirs().unwrap();
        let repo_root = bucket.to_string_lossy().into_owned();
        let er_dir = bucket.to_string_lossy().into_owned();
        let cancel = Arc::new(AtomicBool::new(false));
        let children = Arc::new(Mutex::new(Vec::new()));
        let status = Mutex::new(RunStatus::Running { round: 1 });
        let patch = paths.diff_patch().to_string_lossy().into_owned();
        let ctx = SupervisorCtx {
            registry: &registry,
            config,
            repo_root: &repo_root,
            er_dir: &er_dir,
            reviewed_ref_note: None,
            paths: &paths,
            cancel: &cancel,
            children: &children,
            status: &status,
        };
        Harness {
            panel: Panel::new(&ctx, &patch, None),
            cancel,
        }
    }

    fn bucket(dir: &Path) -> std::path::PathBuf {
        let bucket = dir.join("bucket");
        std::fs::create_dir_all(&bucket).unwrap();
        bucket
    }

    fn params(rounds: u8) -> ArenaStartParams {
        ArenaStartParams {
            title: None,
            reviewers: vec![reviewer_ref("m1"), reviewer_ref("m2")],
            scope: ArenaScope::Branch,
            files: None,
            rounds: Some(rounds),
            arbiter: Some(reviewer_ref("m1")),
            confirm: true,
            agent_kind: None,
            effort: None,
        }
    }

    fn inputs(bucket: &Path) -> ArenaRunInputs {
        ArenaRunInputs {
            repo_root: bucket.to_string_lossy().into_owned(),
            er_dir: bucket.to_string_lossy().into_owned(),
            branch_ref: "feature".into(),
            base_branch: "main".into(),
            raw_diff: "diff --git a/x b/x\n".into(),
            reviewed_ref_note: None,
        }
    }

    fn wait_released(registry: &ArenaRegistry, run_id: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while registry.is_active(run_id) {
            assert!(
                Instant::now() < deadline,
                "run {run_id} never released its registry entry"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    /// The panel's worker is one job per reviewer. A worker that took two
    /// reviewers in one job would spawn half as often.
    #[test]
    fn panel_runs_one_job_per_reviewer() {
        let _env = ArenaEnv::take();
        let dir = tempfile::tempdir().unwrap();
        let bucket = bucket(dir.path());
        let counter = dir.path().join("spawns");
        let config = config(
            "/bin/sh",
            vec!["-c".into(), format!("printf x >> '{}'", counter.display())],
            &["m1"],
        );
        let harness = harness(&bucket, &config, "run-jobs");
        let reviewers = vec![reviewer("fake-a"), reviewer("fake-b"), reviewer("fake-c")];
        let outcome = harness.panel.round1(&reviewers).unwrap();

        assert!(!outcome.cancelled);
        let spawns = std::fs::read_to_string(&counter).unwrap_or_default();
        assert_eq!(
            spawns.matches('x').count(),
            reviewers.len(),
            "each reviewer must be its own job"
        );
    }

    /// The panel reports; it never writes the review. `review.json` has its
    /// own writers, and a reviewer stage that rewrote it would be the arena
    /// import defect again.
    #[test]
    fn panel_never_writes_review_json() {
        let _env = ArenaEnv::take();
        let dir = tempfile::tempdir().unwrap();
        let bucket = bucket(dir.path());
        let seeded: &[u8] = br#"{"version":1,"diff_hash":"hash","files":{}}"#;
        std::fs::write(bucket.join("review.json"), seeded).unwrap();
        let config = config(
            "/bin/sh",
            vec!["-c".into(), r#"printf '{"findings":[]}'"#.into()],
            &["m1"],
        );
        let harness = harness(&bucket, &config, "run-readonly");
        let outcome = harness.panel.round1(&[reviewer("fake-a")]).unwrap();
        assert!(outcome.failed.is_empty(), "{:?}", outcome.failed);
        assert_eq!(
            std::fs::read(bucket.join("review.json")).unwrap(),
            seeded,
            "the panel must not touch the review"
        );
    }

    /// The D17 proxy reads votes the panel already parsed: a two-round panel
    /// makes exactly one arbiter call, and round two itself adds none.
    #[test]
    fn round_two_adds_no_arbiter_call() {
        let env = ArenaEnv::take();
        let (_env, _fixture) = env.with_fixtures();
        let dir = tempfile::tempdir().unwrap();
        let bucket = bucket(dir.path());
        let config = config("/bin/true", Vec::new(), &["m1", "m2"]);
        let registry = Arc::new(ArenaRegistry::new(Arc::new(|| {})));
        let before = crate::arena::fake_arena_call_count();
        crate::storage::with_process_test_storage_root(dir.path(), || {
            let run_id =
                start_review_flow_run(Arc::clone(&registry), config, inputs(&bucket), params(2))
                    .unwrap();
            wait_released(&registry, &run_id);
            let calls = crate::arena::fake_arena_call_count().wrapping_sub(before);
            assert_eq!(
                calls, 5,
                "2 round-1 jobs + 2 cross-round jobs + exactly one arbiter call"
            );
        });
    }

    fn finding(
        id: &str,
        round_logs: Vec<(u8, Vec<(&str, Vote)>)>,
    ) -> super::super::model::ArenaFinding {
        super::super::model::ArenaFinding {
            id: id.into(),
            file: "a.rs".into(),
            line: None,
            title: id.into(),
            body: "b".into(),
            severity_by_round: BTreeMap::new(),
            raised_by: vec![],
            verdict: super::super::model::Verdict::Pending,
            confidence: 0.5,
            rationale: String::new(),
            rounds: round_logs
                .into_iter()
                .map(|(n, ballots)| super::super::model::RoundLog {
                    n,
                    log: ballots
                        .into_iter()
                        .map(|(reviewer, vote)| super::super::model::Ballot {
                            reviewer: reviewer.into(),
                            vote,
                            note: String::new(),
                            merge_target: None,
                        })
                        .collect(),
                })
                .collect(),
            merge_candidates: vec![],
            merged_children: vec![],
            evidence: vec![],
            override_: None,
            accepted_at: None,
        }
    }

    fn run_with(findings: Vec<super::super::model::ArenaFinding>, rounds: u8) -> ArenaRun {
        ArenaRun {
            id: "run-proxy".into(),
            title: None,
            branch_ref: "feature".into(),
            base_branch: "main".into(),
            scope: ArenaScope::Branch,
            diff_hash: "hash".into(),
            created_at: "t0".into(),
            completed_at: None,
            status: RunStatus::Complete,
            config: ArenaConfig {
                reviewers: vec![reviewer_ref("m1")],
                rounds,
                arbiter: reviewer_ref("m1"),
                auto_accept_threshold: 0.75,
                scope: ArenaScope::Branch,
                files: None,
                run_kind: super::super::model::ArenaRunKind::ReviewFlow,
                agent_kind: None,
                effort: None,
            },
            reviewers: vec![reviewer("fake-a"), reviewer("fake-b")],
            findings,
            cost_estimate: CostEstimate {
                tokens_in: 0,
                tokens_out: 0,
                usd: 0.0,
            },
            accepted_finding_ids: vec![],
        }
    }

    /// The free proxy: did the cross round change which claims carry a
    /// quorum? A stable round says the round bought nothing.
    #[test]
    fn panel_run_records_the_free_proxy() {
        let stable = run_with(
            vec![
                finding(
                    "f",
                    vec![
                        (1, vec![("a", Vote::Propose), ("b", Vote::Propose)]),
                        (2, vec![("a", Vote::Keep), ("b", Vote::Keep)]),
                    ],
                ),
                finding("g", vec![(1, vec![("a", Vote::Propose)])]),
            ],
            2,
        );
        let record = panel_run_record(&stable);
        assert_eq!(record.round1_quorum.confirmed, ["f"]);
        assert_eq!(record.round2_quorum.as_ref().unwrap().confirmed, ["f"]);
        assert!(!record.quorum_changed, "an unchanged quorum is the finding");

        let changed = run_with(
            vec![finding(
                "f",
                vec![
                    (1, vec![("a", Vote::Propose)]),
                    (2, vec![("a", Vote::Keep), ("b", Vote::Keep)]),
                ],
            )],
            2,
        );
        let record = panel_run_record(&changed);
        assert!(record.round1_quorum.confirmed.is_empty());
        assert_eq!(record.round2_quorum.as_ref().unwrap().confirmed, ["f"]);
        assert!(record.quorum_changed, "round two changed the outcome");

        let one_round = panel_run_record(&run_with(Vec::new(), 1));
        assert!(one_round.round2_quorum.is_none());
        assert!(!one_round.quorum_changed);
    }

    fn running_pids(path: &Path) -> Vec<String> {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Vec::new();
        };
        text.lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect()
    }

    fn pid_running(pid: &str) -> bool {
        let Ok(output) = std::process::Command::new("ps")
            .args(["-o", "stat=", "-p", pid])
            .output()
        else {
            return false;
        };
        let stat = String::from_utf8_lossy(&output.stdout);
        let stat = stat.trim();
        !stat.is_empty() && !stat.starts_with('Z')
    }

    struct PidGuard(Vec<String>);

    impl Drop for PidGuard {
        fn drop(&mut self) {
            for pid in &self.0 {
                if pid_running(pid) {
                    let _ = std::process::Command::new("/bin/kill")
                        .args(["-KILL", pid])
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status();
                }
            }
        }
    }

    /// A cancel must reach the reviewer's grandchildren. The old arena kill
    /// signalled the registered pid, which left whatever the CLI forked alive.
    #[test]
    fn panel_cancel_kills_grandchildren() {
        let _env = ArenaEnv::take();
        let dir = tempfile::tempdir().unwrap();
        let bucket = bucket(dir.path());
        let pids = dir.path().join("pids");
        let script = format!(
            "echo $$ >> '{pids}'; sleep 30 & echo $! >> '{pids}'; wait",
            pids = pids.display()
        );
        let config = config("/bin/sh", vec!["-c".into(), script], &["m1"]);
        let harness = harness(&bucket, &config, "run-cancel");
        let panel = harness.panel.clone();
        let reviewers = vec![reviewer("fake-a")];
        let running = thread::spawn(move || panel.round1(&reviewers));

        let deadline = Instant::now() + Duration::from_secs(10);
        while running_pids(&pids).len() < 2 {
            assert!(
                Instant::now() < deadline,
                "the reviewer and its grandchild never started"
            );
            thread::sleep(Duration::from_millis(10));
        }
        let started = running_pids(&pids);
        let _guard = PidGuard(started.clone());
        assert!(pid_running(&started[0]));

        harness.cancel.store(true, Ordering::SeqCst);
        // Checked before joining: a pid-only kill leaves the grandchild
        // holding the job's pipes, so the worker would not return until the
        // grandchild finished on its own and hid the defect.
        let deadline = Instant::now() + Duration::from_secs(3);
        while started.iter().any(|pid| pid_running(pid)) {
            assert!(
                Instant::now() < deadline,
                "a reviewer child survived the cancel: {started:?}"
            );
            thread::sleep(Duration::from_millis(20));
        }

        let outcome = running.join().expect("the round returns").unwrap();
        assert!(outcome.cancelled, "a stopped round is cancelled");
    }

    /// A reviewer whose command cannot spawn fails with its reason; the run
    /// must not stay queued or running.
    #[test]
    fn panel_failed_spawn_closes_its_stage() {
        let _env = ArenaEnv::take();
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_process_test_storage_root(dir.path(), || {
            let bucket = bucket(dir.path());
            let config = config("/nonexistent/er-panel-reviewer", Vec::new(), &["m1", "m2"]);
            let registry = Arc::new(ArenaRegistry::new(Arc::new(|| {})));
            let run_id =
                start_review_flow_run(Arc::clone(&registry), config, inputs(&bucket), params(2))
                    .unwrap();
            wait_released(&registry, &run_id);

            let paths = ArenaPaths::for_run(&bucket, &run_id);
            let run = load_run(&paths).unwrap();
            assert_eq!(run.status, RunStatus::Failed);
            assert!(
                run.reviewers.iter().all(|reviewer| matches!(
                    &reviewer.status,
                    ReviewerRunStatus::Failed { reason } if reason.to_lowercase().contains("spawn")
                )),
                "{:?}",
                run.reviewers
            );
            let progress = std::fs::read_to_string(paths.progress_jsonl()).unwrap_or_default();
            let failures = progress
                .lines()
                .filter_map(|line| serde_json::from_str::<ProgressEvent>(line).ok())
                .filter(|event| matches!(event, ProgressEvent::ReviewerFailed { .. }))
                .count();
            assert_eq!(failures, 2, "each reviewer reports why it failed");
        });
    }

    /// A supervisor that unwinds must still close the record: a stage left
    /// running by a dead thread is what the record exists to prevent.
    #[test]
    fn panel_panic_closes_the_stage() {
        let _env = ArenaEnv::take();
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_process_test_storage_root(dir.path(), || {
            let bucket = bucket(dir.path());
            let store = crate::store_registry::store();
            let scope = er_store::Scope::dir(&bucket).unwrap();
            let guard = crate::ai::workflow_record::try_lease(&store, &scope, "panic-test")
                .unwrap()
                .expect("the test owns the bucket");
            let raw_diff = "diff --git a/x b/x\n";
            let workflow = crate::ai::workflow_record::ArenaWorkflow {
                scope: scope.clone(),
                diff_hash: crate::ai::compute_diff_hash(raw_diff),
                started_at: "t0".into(),
                holder: crate::ai::WorkflowHolder {
                    pid: std::process::id(),
                    label: "panic-test".into(),
                },
                guard: Arc::new(guard),
            };
            let registry = Arc::new(ArenaRegistry::new(Arc::new(|| {})));
            let config = config("/bin/true", Vec::new(), &["m1", "m2"]);
            std::env::set_var("ER_TEST_PANIC_ARENA_SUPERVISOR", "1");
            let run_id = crate::arena::start_arena_run(
                Arc::clone(&registry),
                config,
                inputs(&bucket),
                params(2),
                Some(workflow),
            );
            let run_id = match run_id {
                Ok(run_id) => run_id,
                Err(error) => {
                    std::env::remove_var("ER_TEST_PANIC_ARENA_SUPERVISOR");
                    panic!("the run did not start: {error:#}");
                }
            };
            wait_released(&registry, &run_id);
            let panic = registry.join_supervisor(&run_id);
            std::env::remove_var("ER_TEST_PANIC_ARENA_SUPERVISOR");
            assert!(panic.is_err(), "the supervisor panic must surface");

            let record = crate::ai::workflow_record::load(&store, &scope)
                .unwrap()
                .expect("the record remains");
            assert_eq!(
                record.stage(&format!("panel:{run_id}")).unwrap().status,
                crate::ai::StageStatus::Failed
            );
            assert_eq!(
                record.stage(&format!("arbiter:{run_id}")).unwrap().status,
                crate::ai::StageStatus::Failed
            );
            assert_eq!(record.outcome, Some(crate::ai::WorkflowOutcome::Failed));
            assert!(record.holder.is_none());
        });
    }
}
