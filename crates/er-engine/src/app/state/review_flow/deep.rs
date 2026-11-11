//! Deep review confirmation and explicit-input arena start.
//!
//! A Deep run spans two progress channels until P33b: its triage stage reports
//! through the runner's `JobProgress` stream, and its verifying stage reports
//! through the arena's `ProgressEvent` state. Both stop once their owner is
//! cancelled.

use super::progress;
use super::{App, ReviewFlowContext};
use crate::ai::flow_providers::{resolve_review_models, FlowProvider};
use crate::ai::review_flow::{FlowDepth, FlowEvent, FlowState, ReviewFlow};
use crate::arena::{
    effective_arena_rounds, estimate_cost_usd, estimate_latency_sec, ArenaRunInputs,
    ArenaStartParams, ReviewerRef, MIN_QUORUM,
};
use anyhow::{bail, Context, Result};

impl App {
    /// Freeze the exact Deep reviewer and arbiter plan and attach its estimate
    /// before the reducer exposes confirmation. No process is started here.
    pub(super) fn prepare_deep(context: &ReviewFlowContext, flow: &mut ReviewFlow) -> Result<()> {
        let reviewers = deep_reviewers(context)?;
        if reviewers.len() < MIN_QUORUM {
            bail!("Deep review requires at least {MIN_QUORUM} reviewers");
        }
        let arbiter = context
            .arbiter
            .as_ref()
            .context("Deep review arbiter was not captured")?;
        let rounds = effective_arena_rounds(None);
        flow.estimated_cost = Some(f64::from(estimate_cost_usd(
            context.raw_diff.len(),
            &reviewers,
            Some(rounds),
            Some(arbiter),
            &context.config.ai_hub,
        )));
        flow.estimated_latency_seconds = Some(f64::from(estimate_latency_sec(
            &reviewers,
            Some(rounds),
            &context.config.ai_hub,
        )));
        Ok(())
    }

    /// Start the already-approved Deep plan against its captured bucket and
    /// bytes. The flow state itself is the single-confirmation guard.
    pub fn confirm_review_flow(&mut self, er_dir: &str) -> Result<()> {
        let Some(flow) = self.review_flows.get(er_dir) else {
            bail!("no review flow exists for this bucket");
        };
        if flow.state != FlowState::AwaitingConfirmation || flow.depth != Some(FlowDepth::Deep) {
            bail!("Deep review is not awaiting confirmation");
        }
        let diff_hash = flow.diff_hash.clone();
        let (lease, target, inputs, config, reviewers, arbiter, scope, model_pools) = {
            let Some(context) = self.review_flow_contexts.get(er_dir) else {
                bail!("Deep review execution context is missing");
            };
            if context.target.er_dir != er_dir {
                bail!("Deep review bucket no longer matches its captured target");
            }
            let reviewers = deep_reviewers(context)?;
            if reviewers.len() < MIN_QUORUM {
                bail!("Deep review requires at least {MIN_QUORUM} reviewers");
            }
            let arbiter = context
                .arbiter
                .clone()
                .context("Deep review arbiter was not captured")?;
            (
                context.lease.clone(),
                context.target.clone(),
                ArenaRunInputs {
                    repo_root: context.repo_root.clone(),
                    er_dir: context.target.er_dir.clone(),
                    branch_ref: context.branch_ref.clone(),
                    base_branch: context.base_branch.clone(),
                    raw_diff: context.raw_diff.clone(),
                    reviewed_ref_note: context.reviewed_ref_note.clone(),
                },
                context.config.clone(),
                reviewers,
                arbiter,
                context.arena_scope,
                context.model_pools.clone(),
            )
        };
        let preflight = (|| -> Result<()> {
            // `arena_start_for` rechecks this lease immediately before the run is
            // admitted, after the captured input has been assembled.
            let current_model_pools = resolve_review_models(&self.config.ai_hub)
                .context("could not recheck review model groups")?;
            if current_model_pools != model_pools {
                bail!("review model groups changed; start a new review flow");
            }
            let persisted_hash = crate::ai::prepared_diff::diff_tmp_hash(er_dir)
                .context("prepared review diff is missing")?;
            if persisted_hash != diff_hash
                || crate::ai::compute_diff_hash(&inputs.raw_diff) != diff_hash
            {
                bail!("prepared review diff changed; start a new review flow");
            }
            Ok(())
        })();
        if let Err(error) = preflight {
            let notice = if let Some(flow) = self.review_flows.get_mut(er_dir) {
                let previous = flow.state.clone();
                flow.apply(FlowEvent::DriverFailed {
                    reason: error.to_string(),
                });
                (flow.state != previous)
                    .then(|| super::flow_notification_message(flow, &target, false))
            } else {
                None
            };
            if let Some(notice) = notice {
                self.notify(&notice);
            }
            if let Some(flow) = self.review_flows.get_mut(er_dir) {
                progress::fail_waiting_tasks(flow, &error.to_string());
            }
            self.bump_review_flow_revision();
            return Err(error);
        }

        let params = ArenaStartParams {
            title: Some("Deep review flow".into()),
            reviewers,
            scope,
            files: None,
            rounds: None,
            arbiter: Some(arbiter),
            confirm: true,
            // Every reviewer already carries its own lens in ReviewerRef.
            agent_kind: None,
            effort: config.ai_hub.default_effort.clone(),
        };

        let run_id = match self.arena_start_for(inputs, config, params, &lease) {
            Ok(run_id) => run_id,
            Err(error) => {
                let notices = if let Some(flow) = self.review_flows.get_mut(er_dir) {
                    let previous = flow.state.clone();
                    flow.apply(FlowEvent::Confirmed);
                    let confirmed_notice = (flow.state != previous)
                        .then(|| super::flow_notification_message(flow, &target, false));
                    let previous = flow.state.clone();
                    flow.apply(FlowEvent::DriverFailed {
                        reason: error.to_string(),
                    });
                    let failed_notice = (flow.state != previous)
                        .then(|| super::flow_notification_message(flow, &target, false));
                    [confirmed_notice, failed_notice]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>()
                } else {
                    Vec::new()
                };
                for notice in notices {
                    self.notify(&notice);
                }
                if let Some(flow) = self.review_flows.get_mut(er_dir) {
                    progress::fail_waiting_tasks(flow, &error.to_string());
                }
                self.bump_review_flow_revision();
                return Err(error).context("start confirmed Deep arena");
            }
        };

        let notices = {
            let flow = self
                .review_flows
                .get_mut(er_dir)
                .context("review flow disappeared while starting Deep arena")?;
            let previous = flow.state.clone();
            flow.apply(FlowEvent::Confirmed);
            let confirmed_notice = (flow.state != previous)
                .then(|| super::flow_notification_message(flow, &target, false));
            let previous = flow.state.clone();
            flow.apply(FlowEvent::DeepStarted {
                run_id: run_id.clone(),
            });
            let verifying_notice = (flow.state != previous)
                .then(|| super::flow_notification_message(flow, &target, false));
            if flow.state != FlowState::Verifying {
                bail!("Deep review could not enter verification");
            }
            [confirmed_notice, verifying_notice]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
        };
        for notice in notices {
            self.notify(&notice);
        }
        let context = self
            .review_flow_contexts
            .get_mut(er_dir)
            .context("Deep review execution context disappeared")?;
        context.arena_run_id = Some(run_id.clone());
        context.arena_progress_offset = 0;
        context.arena_progress_round = 0;
        self.review_flow_run_targets
            .insert((er_dir.to_owned(), run_id.clone()), target);
        if let Some(flow) = self.review_flows.get(er_dir) {
            Self::sync_workflow_record(er_dir, flow);
        }
        self.bump_review_flow_revision();
        Ok(())
    }

    /// Decline only a pending Deep confirmation. The regular advance/cleanup
    /// path releases the bucket lease once any owned work has drained.
    pub fn decline_review_flow(&mut self, er_dir: &str) -> Result<()> {
        let target = self
            .review_flow_contexts
            .get(er_dir)
            .map(|context| context.target.clone());
        let draining = self
            .review_flow_contexts
            .get(er_dir)
            .is_some_and(|context| self.flow_context_has_producers(context));
        let flow = self
            .review_flows
            .get_mut(er_dir)
            .context("no review flow exists for this bucket")?;
        if flow.state != FlowState::AwaitingConfirmation || flow.depth != Some(FlowDepth::Deep) {
            bail!("Deep review is not awaiting confirmation");
        }
        let previous = flow.state.clone();
        flow.apply(FlowEvent::Declined);
        progress::cancel_waiting_tasks(flow);
        let notice = (flow.state != previous).then(|| {
            target.as_ref().map_or_else(
                || "Deep review was declined.".to_owned(),
                |target| super::flow_notification_message(flow, target, draining),
            )
        });
        if let Some(notice) = notice {
            self.notify(&notice);
        }
        if let Some(flow) = self.review_flows.get(er_dir) {
            Self::sync_workflow_record(er_dir, flow);
        }
        self.bump_review_flow_revision();
        Ok(())
    }
}

fn deep_reviewers(context: &ReviewFlowContext) -> Result<Vec<ReviewerRef>> {
    let mut reviewers = Vec::new();
    for assignment in &context.assignments {
        let agent_kind = if assignment.lens == "general" {
            "general".to_owned()
        } else {
            format!("expert:{}", assignment.lens)
        };
        for FlowProvider {
            provider_id,
            model_id,
        } in &assignment.providers
        {
            reviewers.push(ReviewerRef {
                provider_id: provider_id.clone(),
                model_id: model_id.clone(),
                agent_kind: Some(agent_kind.clone()),
            });
        }
    }
    if reviewers.is_empty() {
        bail!("Deep review has no assigned reviewers");
    }
    Ok(reviewers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::review_flow::FlowState;
    use crate::config::{
        AiHubConfig, AiModelConfig, AiProviderConfig, ReviewModelGroups, ReviewModelRef,
    };
    use crate::paths::ErRoot;
    use std::path::Path;
    use std::time::{Duration, Instant};

    fn serial() -> crate::agent_slots::TestSerialGuard {
        crate::agent_slots::test_serial()
    }

    struct TestDir(tempfile::TempDir);

    impl TestDir {
        fn new() -> Self {
            Self(tempfile::tempdir().unwrap())
        }

        fn path(&self) -> &Path {
            self.0.path()
        }
    }

    struct FakeArenaEnv {
        _guard: std::sync::MutexGuard<'static, ()>,
        previous: Option<std::ffi::OsString>,
        _fixture: tempfile::TempDir,
        registry: std::sync::Arc<crate::arena::ArenaRegistry>,
    }

    impl FakeArenaEnv {
        fn install(app: &App) -> Self {
            let guard = crate::arena::ARENA_TEST_ENV_LOCK
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let fixture = tempfile::tempdir().unwrap();
            for name in ["round1.json", "round2.json", "round3.json"] {
                // Concurrent reviewers advance the fake call counter in any
                // order, so every response must satisfy every round schema.
                std::fs::write(
                    fixture.path().join(name),
                    br#"{"findings":[],"ballots":[],"verdicts":[]}"#,
                )
                .unwrap();
            }
            let previous = std::env::var_os("ER_FAKE_ARENA_DIR");
            std::env::set_var("ER_FAKE_ARENA_DIR", fixture.path());
            Self {
                _guard: guard,
                previous,
                _fixture: fixture,
                registry: std::sync::Arc::clone(&app.arena_registry),
            }
        }
    }

    impl Drop for FakeArenaEnv {
        fn drop(&mut self) {
            for run_id in self.registry.active_run_ids() {
                let deadline = Instant::now() + Duration::from_secs(30);
                while self.registry.is_active(&run_id) {
                    assert!(
                        Instant::now() < deadline,
                        "arena run {run_id} never released its registry entry"
                    );
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
            match self.previous.take() {
                Some(value) => std::env::set_var("ER_FAKE_ARENA_DIR", value),
                None => std::env::remove_var("ER_FAKE_ARENA_DIR"),
            }
        }
    }

    fn fake_app(dir: &TestDir) -> App {
        let bucket = dir.path().join("bucket");
        let script = crate::test_support::staged_script(
            "case \"$1\" in\n\
             *triage.json*) hash=$(cat '__BUCKET__/.diff-tmp.sha256'); printf '{\"version\":1,\"diff_hash\":\"%s\",\"verdict\":{\"primary\":\"arena\",\"experts\":[]}}' \"$hash\" > '__BUCKET__/triage.json' ;;\n\
             *) echo \"unexpected review-flow prompt\" >&2; exit 2 ;;\n\
             esac",
            &bucket.to_string_lossy(),
        );
        fake_app_with_script(dir, &script)
    }

    fn fake_app_with_script(dir: &TestDir, script: &str) -> App {
        let repo = dir.path().join("repo");
        let bucket = dir.path().join("bucket");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir_all(&bucket).unwrap();
        let provider = || AiProviderConfig {
            command: "/bin/sh".into(),
            args: vec![
                "-c".into(),
                script.to_string(),
                "fake-flow".into(),
                "{prompt}".into(),
            ],
            models: vec![AiModelConfig {
                id: "fake-model".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut app = App::new_for_test(Vec::new());
        app.config.ai_hub = AiHubConfig {
            default_provider: Some("fake-a".into()),
            providers: [("fake-a".into(), provider()), ("fake-b".into(), provider())].into(),
            review_models: ReviewModelGroups {
                fast: vec![ReviewModelRef {
                    provider_id: "fake-a".into(),
                    model_id: "fake-model".into(),
                }],
                standard: ["fake-a", "fake-b"]
                    .into_iter()
                    .map(|provider_id| ReviewModelRef {
                        provider_id: provider_id.into(),
                        model_id: "fake-model".into(),
                    })
                    .collect(),
                advanced: ["fake-a", "fake-b"]
                    .into_iter()
                    .map(|provider_id| ReviewModelRef {
                        provider_id: provider_id.into(),
                        model_id: "fake-model".into(),
                    })
                    .collect(),
            },
            max_concurrent_reviews: 16,
            max_concurrent_agents: 16,
            ..Default::default()
        };
        let tab = app.tab_mut();
        tab.repo_root = repo.to_string_lossy().into_owned();
        tab.er_root = ErRoot::Managed {
            agent_dir: bucket.to_string_lossy().into_owned(),
            session_dir: bucket.to_string_lossy().into_owned(),
        };
        tab.set_raw_diff_for_test(
            "diff --git a/src/lib.rs b/src/lib.rs\nindex 1111111..2222222 100644\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-old\n+new\n",
        );
        app
    }

    fn await_confirmation(app: &mut App, bucket: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            app.poll_background_tasks();
            app.advance_review_flows();
            if app
                .review_flows
                .get(bucket)
                .is_some_and(|flow| flow.state == FlowState::AwaitingConfirmation)
            {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "Deep flow did not request confirmation"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn deep_waits_for_confirmation_and_decline_starts_no_arena() {
        let _serial = serial();
        let dir = TestDir::new();
        crate::storage::with_test_storage_root(dir.path(), || {
            let mut app = fake_app(&dir);
            let bucket = app.tab().er_dir();
            app.start_review_flow("branch").unwrap();
            await_confirmation(&mut app, &bucket);

            let flow = app.review_flow_for(&bucket).unwrap();
            assert_eq!(flow.depth, Some(FlowDepth::Deep));
            assert!(flow.estimated_cost.is_some());
            assert!(flow.estimated_latency_seconds.is_some());
            assert!(flow
                .tasks
                .iter()
                .filter(|task| task.round.is_some() || task.id == "verifying:arbiter")
                .all(|task| task.status == crate::ai::review_flow::FlowTaskStatus::Waiting));
            assert!(app.arena_registry.active_run_ids().is_empty());
            assert!(!Path::new(&bucket).join("arena").exists());

            app.decline_review_flow(&bucket).unwrap();
            app.advance_review_flows();
            assert_eq!(
                app.review_flow_for(&bucket).unwrap().state,
                FlowState::Cancelled
            );
            assert!(app
                .review_flow_for(&bucket)
                .unwrap()
                .tasks
                .iter()
                .all(|task| {
                    task.status == crate::ai::review_flow::FlowTaskStatus::Done
                        || task.status == crate::ai::review_flow::FlowTaskStatus::Cancelled
                }));
            assert!(app.confirm_review_flow(&bucket).is_err());
        });
    }

    #[test]
    fn deep_preflight_rejects_changed_allowlist_or_model_groups_and_cancel_starts_no_arena() {
        let _serial = serial();
        let dir = TestDir::new();
        crate::storage::with_test_storage_root(dir.path(), || {
            let mut app = fake_app(&dir);
            let bucket = app.tab().er_dir();
            app.start_review_flow("branch").unwrap();
            await_confirmation(&mut app, &bucket);
            app.config.ai_hub.flow_providers = vec!["fake-a".into()];

            assert!(app.confirm_review_flow(&bucket).is_err());
            assert!(matches!(
                app.review_flow_for(&bucket).unwrap().state,
                FlowState::Failed { .. }
            ));
            assert!(app.arena_registry.active_run_ids().is_empty());
            app.advance_review_flows();

            let dir = TestDir::new();
            let mut app = fake_app(&dir);
            let bucket = app.tab().er_dir();
            app.start_review_flow("branch").unwrap();
            await_confirmation(&mut app, &bucket);
            app.config.ai_hub.review_models.advanced.pop();
            assert!(app.confirm_review_flow(&bucket).is_err());
            assert!(matches!(
                app.review_flow_for(&bucket).unwrap().state,
                FlowState::Failed { .. }
            ));
            assert!(app.arena_registry.active_run_ids().is_empty());
            app.advance_review_flows();

            let dir = TestDir::new();
            let mut app = fake_app(&dir);
            let bucket = app.tab().er_dir();
            app.start_review_flow("branch").unwrap();
            await_confirmation(&mut app, &bucket);
            assert!(app.cancel_review_flow(&bucket));
            assert_eq!(
                app.review_flow_for(&bucket).unwrap().state,
                FlowState::Cancelled
            );
            assert!(app.arena_registry.active_run_ids().is_empty());
        });
    }

    #[test]
    fn deep_confirm_starts_once_from_the_captured_bucket_and_hash() {
        let _serial = serial();
        let dir = TestDir::new();
        crate::storage::with_process_test_storage_root(dir.path(), || {
            let mut app = fake_app(&dir);
            app.current_ai_effort = Some("high".into());
            let bucket = app.tab().er_dir();
            app.start_review_flow("branch").unwrap();
            await_confirmation(&mut app, &bucket);

            app.tabs
                .push(crate::app::TabState::new_for_test(Vec::new()));
            app.active_tab = 1;

            // Unknown prompts exit in the fake shell command, so loss of the
            // arena hook cannot invoke an actual configured provider.
            let fake_arena = FakeArenaEnv::install(&app);
            app.confirm_review_flow(&bucket).unwrap();

            let flow = app.review_flow_for(&bucket).unwrap();
            assert_eq!(flow.state, FlowState::Verifying);
            let run_id = flow.arena_run_id.clone().unwrap();
            let run = crate::arena::load_run(&crate::arena::ArenaPaths::for_run(
                Path::new(&bucket),
                &run_id,
            ))
            .unwrap();
            assert_eq!(run.diff_hash, flow.diff_hash);
            assert_eq!(run.config.run_kind, crate::arena::ArenaRunKind::ReviewFlow);
            assert_eq!(
                serde_json::to_value(&run).unwrap()["config"]["run_kind"],
                "review_flow"
            );
            assert_eq!(run.config.reviewers.len(), 4);
            assert_eq!(run.config.arbiter.provider_id, "fake-b");
            assert_eq!(run.config.effort.as_deref(), Some("high"));
            assert_eq!(
                run.config
                    .reviewers
                    .iter()
                    .map(|reviewer| reviewer.provider_id.as_str())
                    .collect::<Vec<_>>(),
                ["fake-a", "fake-b", "fake-b", "fake-a"]
            );
            assert_eq!(
                run.config
                    .reviewers
                    .iter()
                    .map(|reviewer| reviewer.agent_kind.as_deref().unwrap_or_default())
                    .collect::<Vec<_>>(),
                ["general", "general", "expert:security", "expert:security"]
            );
            assert_eq!(
                flow.estimated_cost,
                Some(f64::from(crate::arena::estimate_cost_usd(
                    std::fs::read(
                        crate::arena::ArenaPaths::for_run(Path::new(&bucket), &run_id).diff_patch()
                    )
                    .unwrap()
                    .len(),
                    &run.config.reviewers,
                    Some(run.config.rounds),
                    Some(&run.config.arbiter),
                    &app.config.ai_hub,
                )))
            );
            assert!(app.review_flow_run_target(&bucket, &run_id).is_some());
            assert!(app.confirm_review_flow(&bucket).is_err());
            let deadline = Instant::now() + Duration::from_secs(10);
            while app.arena_registry.is_active(&run_id) {
                assert!(Instant::now() < deadline, "Deep arena did not finish");
                std::thread::sleep(Duration::from_millis(20));
            }
            app.advance_review_flows();
            let completed = crate::arena::load_run(&crate::arena::ArenaPaths::for_run(
                Path::new(&bucket),
                &run_id,
            ))
            .unwrap();
            let awaiting_accept = completed
                .findings
                .iter()
                .filter(|finding| {
                    matches!(
                        finding.verdict,
                        crate::arena::Verdict::Kept | crate::arena::Verdict::Escalated
                    )
                })
                .count();
            assert_eq!(
                app.review_flow_for(&bucket).unwrap().state,
                FlowState::Done {
                    kept: 0,
                    dropped: 0,
                    awaiting_accept,
                }
            );
            let tasks = &app.review_flow_for(&bucket).unwrap().tasks;
            assert!(
                tasks
                    .iter()
                    .filter(|task| task.round.is_some())
                    .all(|task| task.status == crate::ai::review_flow::FlowTaskStatus::Done),
                "{tasks:#?}"
            );
            assert_eq!(
                tasks
                    .iter()
                    .find(|task| task.id == "verifying:arbiter")
                    .unwrap()
                    .status,
                crate::ai::review_flow::FlowTaskStatus::Done
            );
            assert!(!Path::new(&bucket).join("review.json").exists());
            drop(fake_arena);
        });
    }

    /// Whether the pid names a live process. A zombie is dead: the arena
    /// registry clears its child list without reaping, so its killed children
    /// stay visible to `kill -0` until the process exits (P33b closes that).
    ///
    /// This needs `ps`, which a sandbox may deny; run the suite outside one.
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

    /// Kill any pid still running on drop, so a failing assertion or plant
    /// leaves no sleeper behind.
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

    fn read_pids(path: &Path) -> Vec<String> {
        std::fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect()
    }

    /// Poll until `condition` holds, failing at `timeout` with the thing being
    /// waited for. Every wait in this test goes through it, so none can hang.
    fn wait_until(what: &str, timeout: Duration, mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + timeout;
        while !condition() {
            assert!(
                Instant::now() < deadline,
                "timed out after {timeout:?} waiting for {what}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn wait_for_pids(path: &Path, wanted: usize) -> Vec<String> {
        wait_until(
            &format!("{wanted} children to start"),
            Duration::from_secs(10),
            || read_pids(path).len() >= wanted,
        );
        read_pids(path)
    }

    /// `ER_TEST_SLOW_ARENA` makes the fake provider sleep instead of exiting,
    /// so the arena has live children to cancel. The lock serializes it with
    /// the other arena tests that touch the process environment.
    struct SlowArenaEnv {
        _guard: std::sync::MutexGuard<'static, ()>,
        previous: Option<std::ffi::OsString>,
    }

    impl SlowArenaEnv {
        fn install() -> Self {
            // Bounded acquisition: another test that owns the lock should
            // finish long before this, and a deadlock here must fail the test,
            // not hang the suite.
            let deadline = Instant::now() + Duration::from_secs(10);
            let guard = loop {
                match crate::arena::ARENA_TEST_ENV_LOCK.try_lock() {
                    Ok(guard) => break guard,
                    Err(std::sync::TryLockError::Poisoned(poisoned)) => {
                        break poisoned.into_inner();
                    }
                    Err(std::sync::TryLockError::WouldBlock) => {
                        assert!(
                            Instant::now() < deadline,
                            "timed out waiting for the arena env lock"
                        );
                        std::thread::sleep(Duration::from_millis(10));
                    }
                }
            };
            let previous = std::env::var_os("ER_TEST_SLOW_ARENA");
            std::env::set_var("ER_TEST_SLOW_ARENA", "1");
            Self {
                _guard: guard,
                previous,
            }
        }
    }

    impl Drop for SlowArenaEnv {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(value) => std::env::set_var("ER_TEST_SLOW_ARENA", value),
                None => std::env::remove_var("ER_TEST_SLOW_ARENA"),
            }
        }
    }

    /// A Deep run spans both owners: the runner owns its triage stage, the
    /// arena registry owns its verifying stage. Cancelling either must end its
    /// children, and the lease must outlive the runner child it is draining.
    #[test]
    fn a_deep_cancel_kills_every_child_across_the_runner_and_the_arena() {
        let _serial = serial();
        let dir = TestDir::new();
        crate::storage::with_process_test_storage_root(dir.path(), || {
            let pids_path = dir.path().join("pids");
            let bucket = dir.path().join("bucket");
            // Every child writes its pid. With the slow-arena flag set it
            // sleeps; otherwise it is the triage run, which writes the plan and
            // exits. Prompt matching cannot tell the two apart reliably.
            let script = crate::test_support::staged_script(
                &r#"echo $$ >> '__PIDS__'
if [ -n "${ER_TEST_SLOW_ARENA-}" ]; then exec sleep 30; fi
sleep 1
hash=$(cat '__BUCKET__/.diff-tmp.sha256')
printf '{"version":1,"diff_hash":"%s","verdict":{"primary":"arena","experts":[]}}' "$hash" > '__BUCKET__/triage.json'
"#
                .replace("__PIDS__", &pids_path.to_string_lossy()),
                &bucket.to_string_lossy(),
            );
            let mut app = fake_app_with_script(&dir, &script);
            // Round 1's reviewers must all be live before the cancel, so the
            // test sees every child the arena starts.
            app.config.ai_hub.max_concurrent_arena_reviews = 16;
            let bucket_key = app.tab().er_dir();

            // Runner half: the triage agent is alive when the flow is
            // cancelled, and the lease stays held until that child is gone.
            app.start_review_flow("branch").unwrap();
            wait_until("the triage agent to start", Duration::from_secs(10), || {
                app.poll_background_tasks();
                app.review_flow_for(&bucket_key)
                    .is_some_and(|flow| flow.state == FlowState::Triaging)
                    && !read_pids(&pids_path).is_empty()
            });
            let triage_pid = read_pids(&pids_path)
                .first()
                .cloned()
                .expect("the triage agent wrote its pid");
            let _triage_guard = PidGuard(vec![triage_pid.clone()]);
            assert!(
                pid_running(&triage_pid),
                "the triage agent should be alive now"
            );

            assert!(app.cancel_review_flow(&bucket_key));
            assert!(
                app.review_flow_is_draining(&bucket_key),
                "the flow released its lease at cancel while the triage agent still ran"
            );
            assert!(
                app.check_review_bucket_admission(&bucket_key).is_err(),
                "the lease must stay held while the triage agent drains"
            );
            assert!(
                pid_running(&triage_pid),
                "the triage agent should still be running right after the cancel"
            );
            assert!(
                app.running_background_task_count() >= 1,
                "the draining triage agent should still be a running task"
            );
            assert!(
                crate::agent_slots::active_count() >= 1,
                "the draining triage agent should still hold a slot"
            );

            wait_until(
                "the cancelled flow to drain",
                Duration::from_secs(5),
                || {
                    app.poll_background_tasks();
                    !app.review_flow_is_draining(&bucket_key)
                },
            );
            assert!(
                !pid_running(&triage_pid),
                "the lease was released before the triage agent died"
            );
            assert!(app.check_review_bucket_admission(&bucket_key).is_ok());
            assert_eq!(crate::agent_slots::active_count(), 0);

            app.poll_background_tasks();
            let log_tail: usize = app
                .background_tasks
                .values()
                .map(|handle| handle.recent_log.len())
                .sum();
            // The completion is drained and its worker has exited, so no
            // further entry can arrive; a second poll must see the same log.
            app.poll_background_tasks();
            assert_eq!(
                app.background_tasks
                    .values()
                    .map(|handle| handle.recent_log.len())
                    .sum::<usize>(),
                log_tail,
                "the runner progress channel moved after the drain"
            );

            // Arena half: confirm a second Deep flow and cancel it while the
            // round-1 reviewers are alive.
            app.start_review_flow("branch").unwrap();
            await_confirmation(&mut app, &bucket_key);
            let before = read_pids(&pids_path).len();
            let slow = SlowArenaEnv::install();
            app.confirm_review_flow(&bucket_key).unwrap();
            let _ = wait_for_pids(&pids_path, before + 4);
            let run_id = app
                .review_flow_for(&bucket_key)
                .and_then(|flow| flow.arena_run_id.clone())
                .expect("the Deep arena started");
            // A cancel reaches only children the supervisor has registered; a
            // pid is visible just before that. Wait for the registration, not
            // for a fixed delay.
            wait_until(
                "the arena to register its round-1 reviewers",
                Duration::from_secs(10),
                || app.arena_registry.registered_child_count(&run_id) >= 4,
            );
            let all = read_pids(&pids_path);
            let arena_pids = all[before..].to_vec();
            let _arena_guard = PidGuard(arena_pids.clone());
            let first_command = std::process::Command::new("ps")
                .args(["-o", "command=", "-p", &arena_pids[0]])
                .output()
                .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
                .unwrap_or_default();
            assert!(
                first_command.contains("sleep 30"),
                "the arena child is not sleeping: {first_command}"
            );
            assert!(app.arena_registry.is_active(&run_id));

            assert!(app.cancel_review_flow(&bucket_key));
            wait_until(
                "the cancelled arena to release",
                Duration::from_secs(10),
                || {
                    app.poll_background_tasks();
                    !app.arena_registry.is_active(&run_id)
                },
            );
            drop(slow);
            let survivors: Vec<&String> =
                arena_pids.iter().filter(|pid| pid_running(pid)).collect();
            assert!(
                survivors.is_empty(),
                "arena children still running after the cancel: {survivors:?}"
            );

            let paths = crate::arena::ArenaPaths::for_run(Path::new(&bucket_key), &run_id);
            let run = crate::arena::load_run(&paths).unwrap();
            assert_eq!(run.status, crate::arena::RunStatus::Cancelled);
            let progress = std::fs::read_to_string(paths.progress_jsonl()).unwrap_or_default();
            // Join the supervisor before comparing: it has already released
            // the run, so a final write can only be one it made before this
            // read, and nothing can append after the join.
            app.arena_registry.join_supervisor(&run_id).unwrap();
            assert_eq!(
                std::fs::read_to_string(paths.progress_jsonl()).unwrap_or_default(),
                progress,
                "the arena progress channel moved after the cancel"
            );
        });
    }
}
