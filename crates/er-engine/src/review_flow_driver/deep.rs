//! Deep review confirmation and explicit-input arena start.
//!
//! A Deep run's triage stage reports through the runner's `JobProgress`
//! stream; its verifying stage reports through the live run control's
//! in-memory `ProgressEvent` state, which the reviewer rows read. Both stop
//! once their owner is cancelled, and the control carries the arena's cancel
//! flag so the job bridge can see it.

use super::progress;
use super::{ReviewFlowContext, ReviewFlowHost, ReviewFlows};
use crate::ai::flow_providers::{resolve_review_models, FlowProvider};
use crate::ai::review_flow::{FlowDepth, FlowEvent, FlowState, ReviewFlow};
use crate::arena::{
    effective_arena_rounds, estimate_cost_usd, estimate_latency_sec, ArenaRunInputs,
    ArenaStartParams, ReviewerRef, MIN_QUORUM,
};
use anyhow::{bail, Context, Result};

impl ReviewFlows {
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
    pub fn confirm_review_flow(
        &mut self,
        host: &mut dyn ReviewFlowHost,
        er_dir: &str,
    ) -> Result<()> {
        let Some(flow) = self.flows.get(er_dir) else {
            bail!("no review flow exists for this bucket");
        };
        if flow.state != FlowState::AwaitingConfirmation || flow.depth != Some(FlowDepth::Deep) {
            bail!("Deep review is not awaiting confirmation");
        }
        let diff_hash = flow.diff_hash.clone();
        let (lease, target, inputs, config, reviewers, arbiter, scope, model_pools) = {
            let Some(context) = self.contexts.get(er_dir) else {
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
            let current_model_pools = resolve_review_models(&host.config().ai_hub)
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
            let notice = if let Some(flow) = self.flows.get_mut(er_dir) {
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
                host.notify(&notice);
            }
            if let Some(flow) = self.flows.get_mut(er_dir) {
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

        let run_id = match host.arena_start_for(inputs, config, params, &lease) {
            Ok(run_id) => run_id,
            Err(error) => {
                let notices = if let Some(flow) = self.flows.get_mut(er_dir) {
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
                    host.notify(&notice);
                }
                if let Some(flow) = self.flows.get_mut(er_dir) {
                    progress::fail_waiting_tasks(flow, &error.to_string());
                }
                self.bump_review_flow_revision();
                return Err(error).context("start confirmed Deep arena");
            }
        };

        let notices = {
            let flow = self
                .flows
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
            host.notify(&notice);
        }
        let context = self
            .contexts
            .get_mut(er_dir)
            .context("Deep review execution context disappeared")?;
        context.arena_run_id = Some(run_id.clone());
        context.arena_progress_round = 0;
        self.run_targets
            .insert((er_dir.to_owned(), run_id.clone()), target);
        if let Some(flow) = self.flows.get(er_dir) {
            Self::sync_workflow_record(er_dir, flow);
        }
        self.bump_review_flow_revision();
        Ok(())
    }

    /// Decline only a pending Deep confirmation. The regular advance/cleanup
    /// path releases the bucket lease once any owned work has drained.
    pub fn decline_review_flow(
        &mut self,
        host: &mut dyn ReviewFlowHost,
        er_dir: &str,
    ) -> Result<()> {
        let target = self
            .contexts
            .get(er_dir)
            .map(|context| context.target.clone());
        let draining = self
            .contexts
            .get(er_dir)
            .is_some_and(|context| Self::flow_context_has_producers(host, context));
        let flow = self
            .flows
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
            host.notify(&notice);
        }
        if let Some(flow) = self.flows.get(er_dir) {
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
