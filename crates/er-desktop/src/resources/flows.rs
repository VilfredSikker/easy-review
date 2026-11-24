//! The review flows on `er_send` and `er_fetch`. A control names the tab
//! the page painted and the bucket it painted for it; `flow_service`
//! resolves the tab in the registry and builds the context from its content,
//! with no App. `jobs` carries every started actor's flows, so the page
//! reads a flow from there and a flow's run on each fetch.

use er_protocol::{
    Ack, ArenaRun, Command, ErError, ErErrorKind, Resource, ResourceChanged, ResourceKey,
    ResourceVersion, ReviewFlowAction, ReviewFlowJob,
};

use super::tabs::Routed;
use crate::flow_service::{self, Control};
use crate::page_view::ReviewFlowSnapshot;
use crate::state::DesktopState;

/// The review flows' rows, each as the snapshot the flag-off page reads.
pub fn live_flows(flows: &[er_service::LiveReviewFlow]) -> Vec<ReviewFlowJob> {
    flows
        .iter()
        .filter_map(|live| {
            let snapshot = ReviewFlowSnapshot::from_flow(&live.flow, live.draining);
            let json = serde_json::to_string(&snapshot).ok()?;
            Some(ReviewFlowJob {
                er_dir: live.er_dir.clone(),
                json,
            })
        })
        .collect()
}

/// A flow's run, read from the bucket it captured on the actor that runs
/// the flow, else on `wire`'s.
pub fn run(
    desktop: &DesktopState,
    wire: er_protocol::TabKey,
    er_dir: &str,
    run_id: &str,
) -> Result<(u64, Resource), ErError> {
    let tab = flow_service::entry(desktop, Some(wire))?;
    let snapshot = flow_service::get_run(desktop, &tab, er_dir, run_id)?;
    let json = serde_json::to_string(&snapshot).map_err(|error| ErError {
        kind: ErErrorKind::Internal,
        message: format!("write the run as JSON: {error}"),
    })?;
    Ok((0, Resource::ReviewFlowRun(ArenaRun { json })))
}

/// Applies `command` when it is a review-flow command. The `Ack` names
/// `jobs`, which holds the flows, and the tab's `annotations(tab)`, which
/// an acceptance moves.
pub fn apply(desktop: &DesktopState, command: Command) -> Routed {
    let Command::ReviewFlow {
        tab,
        er_dir,
        action,
    } = command
    else {
        return Routed::Other(command);
    };
    let control = match step(action) {
        Step::Accept {
            run_id,
            finding_ids,
        } => {
            let accepted = flow_service::entry(desktop, Some(tab)).and_then(|entry| {
                flow_service::accept(desktop, &entry, er_dir, run_id, finding_ids)
            });
            return Routed::Applied(
                accepted.map(|accepted| ack(desktop, tab, Some(accepted.to_string()))),
            );
        }
        Step::Control(control) => control,
    };
    let applied = crate::arena_service::tab(desktop, Some(tab))
        .and_then(|named| flow_service::run(desktop, &named, er_dir, control));
    Routed::Applied(applied.map(|()| ack(desktop, tab, None)))
}

/// What an action asks of `flow_service`: a control, or an acceptance,
/// which carries the run and the findings it takes.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Step {
    Control(Control),
    Accept {
        run_id: String,
        finding_ids: Vec<String>,
    },
}

pub(crate) fn step(action: ReviewFlowAction) -> Step {
    let control = match action {
        ReviewFlowAction::AcceptFindings {
            run_id,
            finding_ids,
        } => {
            return Step::Accept {
                run_id,
                finding_ids,
            }
        }
        ReviewFlowAction::Start { scope } => Control::Start(scope),
        ReviewFlowAction::Confirm => Control::Confirm,
        ReviewFlowAction::Decline => Control::Decline,
        ReviewFlowAction::Rerun => Control::Rerun,
        ReviewFlowAction::Cancel => Control::Cancel,
        ReviewFlowAction::RunStandard => Control::RunStandard,
    };
    Step::Control(control)
}

fn ack(desktop: &DesktopState, wire: er_protocol::TabKey, created: Option<String>) -> Ack {
    let annotations = flow_service::entry(desktop, Some(wire))
        .ok()
        .and_then(|entry| desktop.service.content(entry.key, &entry.descriptor))
        .map_or(0, |content| er_service::annotations_version(&content));
    Ack {
        changed: vec![
            ResourceChanged {
                key: ResourceKey::Jobs,
                version: ResourceVersion(super::jobs::version(desktop)),
            },
            ResourceChanged {
                key: ResourceKey::Annotations { tab: wire },
                version: ResourceVersion(annotations),
            },
        ],
        created,
    }
}
