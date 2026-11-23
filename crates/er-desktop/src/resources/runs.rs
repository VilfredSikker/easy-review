//! The task commands on `er_send`. Each removes or stops a task, and its
//! `Ack` names `jobs`, which lists the task until it finishes.

use er_protocol::{Ack, Command, ResourceChanged, ResourceKey, ResourceVersion};

use super::tabs::Routed;
use crate::state::DesktopState;

/// Applies `command` when it is a task command.
pub fn apply(desktop: &DesktopState, command: Command) -> Routed {
    let ran = match command {
        Command::CancelQueuedTask { id } => crate::tasks::cancel_queued(desktop, &id),
        Command::CancelRunningTask { id } => crate::tasks::cancel_running(desktop, &id),
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
