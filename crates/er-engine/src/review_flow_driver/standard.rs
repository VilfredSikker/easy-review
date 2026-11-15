//! "Run standard instead": turn a pending Deep plan into a Standard one.

use super::progress;
use super::{ReviewFlowHost, ReviewFlows};
use crate::ai::flow_providers::resolve_review_models;
use crate::ai::review_flow::{FlowDepth, FlowEvent, FlowState, FlowTaskStage};
use anyhow::{bail, Context, Result};

impl ReviewFlows {
    /// Run the lenses triage already chose as ordinary Standard finders. Triage
    /// is not repeated, so its verdict and the prepared diff stay the flow's
    /// own; the finders queue for agent slots like any other flow finder.
    pub fn run_review_flow_standard(
        &mut self,
        host: &mut dyn ReviewFlowHost,
        er_dir: &str,
    ) -> Result<()> {
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
            let current_model_pools = resolve_review_models(&host.config().ai_hub)
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
                host.apply_flow_event_notifying(
                    &mut flow,
                    &context.target,
                    er_dir,
                    context.draining,
                    FlowEvent::StandardChosen,
                );
                if flow.state == FlowState::Finding {
                    self.enqueue_finders(host, &mut context, &mut flow)
                } else {
                    Err(anyhow::anyhow!("review flow could not start its finders"))
                }
            }
            Err(error) => Err(error),
        };
        if let Err(error) = &result {
            host.apply_flow_event_notifying(
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
        if let Some(flow) = self.review_flows.get(er_dir) {
            Self::sync_workflow_record(er_dir, flow);
        }
        self.bump_review_flow_revision();
        result
    }
}
