//! The card asks on `er_send`. Each names its tab, and the `Ack` names
//! `jobs`, which shows the reply being written until it lands; an
//! AI-assisted finding reply also names the tab's `annotations(tab)`, which
//! holds the thread it asks in, and creates that thread's root id.

use er_protocol::{
    Ack, Command, ErError, PaintedView, ResourceChanged, ResourceKey, ResourceVersion,
};

use super::tabs::Routed;
use crate::state::DesktopState;

/// Applies `command` when it is a card ask.
pub fn apply(desktop: &DesktopState, command: Command) -> Routed {
    let asked = match command {
        Command::AskAi {
            tab,
            thread_id,
            prompt,
        } => crate::ai_ask::ask(desktop, Some(tab), thread_id, prompt),
        Command::ValidateWithAi {
            tab,
            thread_id,
            finding_id,
        } => crate::ai_ask::validate(desktop, Some(tab), thread_id, finding_id),
        Command::ElaborateWithAi { tab, thread_id } => {
            crate::ai_ask::elaborate(desktop, Some(tab), thread_id)
        }
        Command::AskAiOnFinding {
            tab,
            view,
            finding_id,
            body,
            client_id,
        } => {
            let finding = Finding {
                view,
                finding_id,
                body,
                client_id,
            };
            return Routed::Applied(on_finding(desktop, tab, finding));
        }
        other => return Routed::Other(other),
    };
    Routed::Applied(asked.map(|()| Ack {
        changed: vec![jobs(desktop)],
        created: None,
    }))
}

struct Finding {
    view: PaintedView,
    finding_id: String,
    body: String,
    client_id: Option<String>,
}

fn on_finding(
    desktop: &DesktopState,
    wire: er_protocol::TabKey,
    finding: Finding,
) -> Result<Ack, ErError> {
    let root = crate::ai_ask::ask_on_finding(
        desktop,
        Some(wire),
        Some(&finding.view),
        finding.finding_id,
        finding.body,
        finding.client_id,
    )?;
    let tab = crate::arena_service::tab(desktop, Some(wire))?;
    let annotations = ResourceChanged {
        key: ResourceKey::Annotations { tab: wire },
        version: ResourceVersion(er_service::annotations_version(&tab.content)),
    };
    Ok(Ack {
        changed: vec![annotations, jobs(desktop)],
        created: Some(root),
    })
}

fn jobs(desktop: &DesktopState) -> ResourceChanged {
    ResourceChanged {
        key: ResourceKey::Jobs,
        version: ResourceVersion(super::jobs::version(desktop)),
    }
}
