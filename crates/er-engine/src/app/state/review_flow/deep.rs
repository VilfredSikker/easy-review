//! Deep review confirmation and explicit-input arena start.

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
    use std::path::PathBuf;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    fn serial() -> crate::agent_slots::TestSerialGuard {
        crate::agent_slots::test_serial()
    }

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "er-review-flow-deep-{}-{nonce}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
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
                while self.registry.is_active(&run_id) {
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
        let repo = dir.0.join("repo");
        let bucket = dir.0.join("bucket");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir_all(&bucket).unwrap();
        let script = "case \"$1\" in\n\
             *triage.json*) hash=$(cat '__BUCKET__/.diff-tmp.sha256'); printf '{\"version\":1,\"diff_hash\":\"%s\",\"verdict\":{\"primary\":\"arena\",\"experts\":[]}}' \"$hash\" > '__BUCKET__/triage.json' ;;\n\
             *) echo \"unexpected review-flow prompt\" >&2; exit 2 ;;\n\
             esac"
            .replace("__BUCKET__", &bucket.to_string_lossy());
        let provider = || AiProviderConfig {
            command: "/bin/sh".into(),
            args: vec![
                "-c".into(),
                script.clone(),
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
    }

    #[test]
    fn deep_preflight_rejects_changed_allowlist_or_model_groups_and_cancel_starts_no_arena() {
        let _serial = serial();
        let dir = TestDir::new();
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
    }

    #[test]
    fn deep_confirm_starts_once_from_the_captured_bucket_and_hash() {
        let _serial = serial();
        let dir = TestDir::new();
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
    }
}
