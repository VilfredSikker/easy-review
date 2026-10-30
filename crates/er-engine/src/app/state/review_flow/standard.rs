//! "Run standard instead": turn a pending Deep plan into a Standard one.

use super::progress;
use super::App;
use crate::ai::flow_providers::resolve_review_models;
use crate::ai::review_flow::{FlowDepth, FlowEvent, FlowState, FlowTaskStage};
use anyhow::{bail, Context, Result};

impl App {
    /// Run the lenses triage already chose as ordinary Standard finders. Triage
    /// is not repeated, so its verdict and the prepared diff stay the flow's
    /// own; the finders queue for agent slots like any other flow finder.
    pub fn run_review_flow_standard(&mut self, er_dir: &str) -> Result<()> {
        let Some(flow) = self.review_flows.get(er_dir) else {
            bail!("no review flow exists for this bucket");
        };
        if flow.state != FlowState::AwaitingConfirmation || flow.depth != Some(FlowDepth::Deep) {
            bail!("Deep review is not awaiting confirmation");
        }
        let mut context = self
            .review_flow_contexts
            .remove(er_dir)
            .context("Deep review execution context is missing")?;
        let Some(mut flow) = self.review_flows.remove(er_dir) else {
            self.review_flow_contexts.insert(er_dir.to_owned(), context);
            bail!("no review flow exists for this bucket");
        };

        let lenses = flow.lenses.clone();
        let planned = (|| -> Result<()> {
            if context.target.er_dir != er_dir {
                bail!("review bucket no longer matches its captured target");
            }
            // Same rechecks as confirming Deep: never run finders against
            // model groups or diff bytes other than the ones triage saw.
            let current_model_pools = resolve_review_models(&self.config.ai_hub)
                .context("could not recheck review model groups")?;
            if current_model_pools != context.model_pools {
                bail!("review model groups changed; start a new review flow");
            }
            let persisted_hash = crate::ai::prepared_diff::diff_tmp_hash(er_dir)
                .context("prepared review diff is missing")?;
            if persisted_hash != flow.diff_hash
                || crate::ai::compute_diff_hash(&context.raw_diff) != flow.diff_hash
            {
                bail!("prepared review diff changed; start a new review flow");
            }
            Self::plan_finders(&mut context, FlowDepth::Standard, &lenses)
        })();

        let result = match planned {
            Ok(()) => {
                // The Deep plan's reviewer and arbiter rows describe work that
                // will not run; only the triage row carries over.
                flow.tasks
                    .retain(|task| task.stage == FlowTaskStage::Triage);
                progress::append_planned_tasks(&context, &mut flow, FlowDepth::Standard);
                self.apply_flow_event_notifying(
                    &mut flow,
                    &context.target,
                    er_dir,
                    context.draining,
                    FlowEvent::StandardChosen,
                );
                if flow.state == FlowState::Finding {
                    self.enqueue_finders(&mut context, &mut flow)
                } else {
                    Err(anyhow::anyhow!("review flow could not start its finders"))
                }
            }
            Err(error) => Err(error),
        };
        if let Err(error) = &result {
            self.apply_flow_event_notifying(
                &mut flow,
                &context.target,
                er_dir,
                context.draining,
                FlowEvent::DriverFailed {
                    reason: error.to_string(),
                },
            );
            progress::fail_waiting_tasks(&mut flow, &error.to_string());
        }
        self.review_flows.insert(er_dir.to_owned(), flow);
        self.review_flow_contexts.insert(er_dir.to_owned(), context);
        self.bump_review_flow_revision();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::review_flow::FlowTaskStatus;
    use crate::config::{
        AiHubConfig, AiModelConfig, AiProviderConfig, ReviewModelGroups, ReviewModelRef,
    };
    use crate::paths::ErRoot;
    use std::time::{Duration, Instant};

    /// Triage always picks Deep; finder prompts fail fast, so the test can
    /// drain the queued finders without a real provider.
    fn fake_app(dir: &tempfile::TempDir) -> App {
        let repo = dir.path().join("repo");
        let bucket = dir.path().join("bucket");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir_all(&bucket).unwrap();
        let script = "case \"$1\" in\n\
             *triage.json*) hash=$(cat '__BUCKET__/.diff-tmp.sha256'); printf '{\"version\":1,\"diff_hash\":\"%s\",\"verdict\":{\"primary\":\"arena\",\"experts\":[]}}' \"$hash\" > '__BUCKET__/triage.json' ;;\n\
             *) echo \"finder not faked\" >&2; exit 2 ;;\n\
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
        let model = |provider_id: &str| ReviewModelRef {
            provider_id: provider_id.into(),
            model_id: "fake-model".into(),
        };
        let mut app = App::new_for_test(Vec::new());
        app.config.ai_hub = AiHubConfig {
            default_provider: Some("fake-a".into()),
            providers: [("fake-a".into(), provider()), ("fake-b".into(), provider())].into(),
            review_models: ReviewModelGroups {
                fast: vec![model("fake-a")],
                standard: vec![model("fake-a"), model("fake-b")],
                advanced: vec![model("fake-a"), model("fake-b")],
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

    fn tick_until(app: &mut App, bucket: &str, done: impl Fn(&FlowState) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            app.poll_background_tasks();
            app.advance_review_flows();
            if app
                .review_flows
                .get(bucket)
                .is_some_and(|flow| done(&flow.state))
                && !app.review_flow_is_draining(bucket)
            {
                return;
            }
            assert!(Instant::now() < deadline, "review flow did not settle");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn standard_instead_queues_the_triaged_lenses_without_an_arena() {
        let _serial = crate::agent_slots::test_serial();
        let dir = tempfile::tempdir().unwrap();
        let mut app = fake_app(&dir);
        let bucket = app.tab().er_dir();
        app.start_review_flow("branch").unwrap();
        tick_until(&mut app, &bucket, |s| *s == FlowState::AwaitingConfirmation);
        let lenses = app.review_flow_for(&bucket).unwrap().lenses.clone();

        app.run_review_flow_standard(&bucket).unwrap();

        let flow = app.review_flow_for(&bucket).unwrap();
        assert_eq!(flow.depth, Some(FlowDepth::Standard));
        assert_eq!(flow.state, FlowState::Finding);
        assert_eq!(flow.lenses, lenses, "triage is not repeated");
        assert!(flow.tasks.iter().all(|task| task.round.is_none()));
        for lens in &lenses {
            let row = flow
                .tasks
                .iter()
                .find(|task| task.id == format!("finding:{lens}"))
                .expect("a finder row per lens");
            assert!(row.background_task_id.is_some());
            assert_ne!(row.status, FlowTaskStatus::Waiting);
        }
        assert!(app.arena_registry.active_run_ids().is_empty());
        assert!(app.run_review_flow_standard(&bucket).is_err());
        assert!(app.confirm_review_flow(&bucket).is_err());
        assert!(app.decline_review_flow(&bucket).is_err());

        tick_until(&mut app, &bucket, |s| {
            !matches!(s, FlowState::Finding | FlowState::Verifying)
        });
    }

    #[test]
    fn standard_instead_requires_a_pending_deep_confirmation() {
        let _serial = crate::agent_slots::test_serial();
        let dir = tempfile::tempdir().unwrap();
        let mut app = fake_app(&dir);
        let bucket = app.tab().er_dir();
        assert!(app.run_review_flow_standard(&bucket).is_err());
        app.start_review_flow("branch").unwrap();
        assert!(
            app.run_review_flow_standard(&bucket).is_err(),
            "still triaging"
        );
        tick_until(&mut app, &bucket, |s| *s == FlowState::AwaitingConfirmation);
        app.decline_review_flow(&bucket).unwrap();
        assert!(app.run_review_flow_standard(&bucket).is_err());
        tick_until(&mut app, &bucket, |s| *s == FlowState::Cancelled);
    }
}
