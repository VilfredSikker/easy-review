//! The run and task commands on `er_send`. Each queues an agent task, or
//! removes or stops one, and its `Ack` names `jobs`, which lists the task
//! until it finishes.

use er_protocol::{Ack, Command, ResourceChanged, ResourceKey, ResourceVersion};

use super::tabs::Routed;
use crate::state::DesktopState;

/// Applies `command` when it is a run or task command.
pub fn apply(desktop: &DesktopState, command: Command) -> Routed {
    let ran = match command {
        Command::RunReview { tab, scope } => crate::runs::review(desktop, Some(tab), scope),
        Command::GenerateTour { tab } => crate::runs::tour(desktop, Some(tab)),
        Command::RunImportanceAgent { tab } => crate::runs::importance(desktop, Some(tab)),
        Command::GenerateDiagram {
            tab,
            diagram_kind,
            custom_prompt,
        } => crate::runs::diagram(desktop, Some(tab), diagram_kind, custom_prompt),
        Command::RunExpertReview {
            tab,
            scope,
            expert_id,
        } => crate::runs::expert(desktop, Some(tab), scope, expert_id),
        Command::RunScopedReview {
            tab,
            scope,
            paths,
            reviewers,
            focus_prompt,
        } => crate::runs::scoped(desktop, Some(tab), scope, paths, reviewers, focus_prompt),
        Command::RunValidate { tab, scope } => crate::runs::validate(desktop, Some(tab), scope),
        Command::SendNoteToAgent { tab, note_id } => {
            crate::runs::send_note(desktop, Some(tab), note_id)
        }
        Command::RunPrTriage {
            project_id,
            pr_number,
        } => {
            let ctx = crate::route_helpers::triage_ctx(desktop);
            crate::route_helpers::pr_triage(desktop, &ctx, &project_id, pr_number)
                .map_err(crate::task_hooks::refused)
        }
        Command::RunBranchTriage { project_id, branch } => {
            let ctx = crate::route_helpers::triage_ctx(desktop);
            crate::route_helpers::branch_triage(desktop, &ctx, &project_id, &branch)
                .map_err(crate::task_hooks::refused)
        }
        Command::CancelQueuedTask { id } => crate::task_hooks::cancel_queued(desktop, &id),
        Command::CancelRunningTask { id } => crate::task_hooks::cancel_running(desktop, &id),
        other => return Routed::Other(other),
    };
    Routed::Applied(ran.map(|()| Ack {
        changed: vec![ResourceChanged {
            key: ResourceKey::Jobs,
            version: ResourceVersion(super::jobs::version(desktop)),
        }],
        created: None,
    }))
}
