//! The card asks on `er_send`. Each names its tab, and the `Ack` names
//! `jobs`, which shows the reply being written until it lands; an
//! AI-assisted finding reply also names the tab's `annotations(tab)`, which
//! holds the thread it asks in, and creates that thread's root id.
//!
//! er-service plans and runs the asks (`er_service::card_asks`); this module
//! hands it the tab and the desktop's hooks, and keeps the pending set it
//! fills in `DesktopState`, which `jobs` reads.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use er_engine::ai::AI_ACTION_ASK;
use er_protocol::{
    Ack, Command, ErError, ErErrorKind, PaintedView, ResourceChanged, ResourceKey, ResourceVersion,
};
use er_service::card_asks::{context, run, AskHooks, AskedTab, DEFAULT_ASK_AI_PROMPT};
use er_service::Reply;

use super::content_routes;
use super::tabs::Routed;
use crate::arena_service::ArenaTab;
use crate::state::DesktopState;

/// Applies `command` when it is a card ask.
pub fn apply(desktop: &DesktopState, command: Command) -> Routed {
    let asked = match command {
        Command::AskAi {
            tab,
            thread_id,
            prompt,
        } => ask(desktop, Some(tab), thread_id, prompt),
        Command::ValidateWithAi {
            tab,
            thread_id,
            finding_id,
        } => validate(desktop, Some(tab), thread_id, finding_id),
        Command::ElaborateWithAi { tab, thread_id } => elaborate(desktop, Some(tab), thread_id),
        Command::AskAiOnFinding {
            tab,
            view,
            finding_id,
            body,
            client_id,
        } => {
            let finding = FindingAsk {
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

struct FindingAsk {
    view: PaintedView,
    finding_id: String,
    body: String,
    client_id: Option<String>,
}

fn on_finding(
    desktop: &DesktopState,
    wire: er_protocol::TabKey,
    finding: FindingAsk,
) -> Result<Ack, ErError> {
    let root = ask_on_finding(
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

/// The tab an ask is made on, as er-service reads it.
fn asked(tab: &ArenaTab) -> AskedTab {
    AskedTab {
        key: tab.entry.key,
        workspace: tab.workspace(),
        descriptor: tab.entry.descriptor.clone(),
        content: Arc::clone(&tab.content),
    }
}

/// The desktop's side of an ask: its ports, the settings view's config, the
/// tabs' actors, the page's notice, and what moves when an answer lands.
pub fn hooks(desktop: &DesktopState) -> AskHooks {
    let (sender, reader, teller, lander) = (
        desktop.clone(),
        desktop.clone(),
        desktop.clone(),
        desktop.clone(),
    );
    AskHooks {
        ports: desktop.ports.clone(),
        config: crate::arena_service::config(desktop),
        send: Arc::new(move |workspace, command| sender.service.send(workspace, command)),
        content: Arc::new(move |key, tab| reader.service.content(key, tab)),
        notify: Arc::new(move |message| notify(&teller, message)),
        landed: Arc::new(move |repo_root| {
            lander.app_owner.refresh_meta(&*lander.ports.git, repo_root);
            lander.desktop_revision.fetch_add(1, Ordering::Relaxed);
        }),
    }
}

/// Asks about a thread with `prompt`, or with the thread kind's own prompt
/// when it is empty.
pub fn ask(
    desktop: &DesktopState,
    wire: Option<er_protocol::TabKey>,
    thread_id: String,
    prompt: String,
) -> Result<(), ErError> {
    let tab = asked(&crate::arena_service::tab(desktop, wire)?);
    run::ask(
        &hooks(desktop),
        &desktop.pending_ai_replies,
        &tab,
        thread_id,
        prompt,
    )
    .map_err(refused)
}

/// Has the AI rewrite a question or note as a self-contained one.
pub fn elaborate(
    desktop: &DesktopState,
    wire: Option<er_protocol::TabKey>,
    thread_id: String,
) -> Result<(), ErError> {
    let tab = asked(&crate::arena_service::tab(desktop, wire)?);
    run::elaborate(
        &hooks(desktop),
        &desktop.pending_ai_replies,
        &tab,
        thread_id,
    )
    .map_err(refused)
}

/// Has the AI check a finding, or the finding a thread was opened on, or
/// else the thread itself.
pub fn validate(
    desktop: &DesktopState,
    wire: Option<er_protocol::TabKey>,
    thread_id: Option<String>,
    finding_id: Option<String>,
) -> Result<(), ErError> {
    let tab = asked(&crate::arena_service::tab(desktop, wire)?);
    let replies = &desktop.pending_ai_replies;
    run::validate(&hooks(desktop), replies, &tab, thread_id, finding_id).map_err(refused)
}

/// Asks about a finding in its thread: the thread is found or created
/// through the workspace, refused when the tab no longer shows `view`, and
/// the ask answers in it. Answers the thread's root id.
pub fn ask_on_finding(
    desktop: &DesktopState,
    wire: Option<er_protocol::TabKey>,
    view: Option<&PaintedView>,
    finding_id: String,
    body: String,
    client_id: Option<String>,
) -> Result<String, ErError> {
    let tab = crate::arena_service::tab(desktop, wire)?;
    let (_, _, _, title, description) =
        context::lookup_finding_fields(&tab.content.ai.state, &finding_id).map_err(refused)?;
    let thread = finding_id.clone();
    let (reply, _) = content_routes::write_on_key(desktop, tab.entry.key, view, |content| {
        Ok(er_service::Command::ReplyToFinding {
            context: content.context.clone(),
            finding_id: thread,
            body: String::new(),
            ai_assist: true,
            client_id,
        })
    })
    .map_err(refused)?;
    let Reply::FindingReplied { root_id, .. } = reply else {
        return Err(refused(format!("unexpected reply: {reply:?}")));
    };
    let prompt = if body.trim().is_empty() {
        DEFAULT_ASK_AI_PROMPT
    } else {
        body.as_str()
    };
    let prompt = format!("Finding: {title}\n\n{description}\n\n---\n\n{prompt}");
    // The content the write published holds the thread it created.
    let tab = crate::arena_service::tab(desktop, Some(er_service::wire_tab_key(tab.entry.key)))?;
    run::ask_thread(
        &hooks(desktop),
        &desktop.pending_ai_replies,
        &asked(&tab),
        root_id.clone(),
        prompt,
        Some(AI_ACTION_ASK),
    )
    .map_err(refused)?;
    Ok(root_id)
}

/// A notice the page shows once.
fn notify(desktop: &DesktopState, message: String) {
    let handle = desktop
        .tauri_app_handle
        .lock()
        .ok()
        .and_then(|handle| handle.clone());
    match handle {
        Some(handle) => crate::resources::notice::emit(
            &handle,
            &crate::resources::notice::engine_notice(message, false),
        ),
        None => log::warn!("{message}"),
    }
}

fn refused(message: impl Into<String>) -> ErError {
    ErError {
        kind: ErErrorKind::Refused,
        message: message.into(),
    }
}
