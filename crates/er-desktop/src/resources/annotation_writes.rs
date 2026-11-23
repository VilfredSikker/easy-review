//! The annotation writes on the flag path: each names the tab the page
//! painted it on, and the view `annotations(tab)` named when it did, and goes
//! to that tab's own workspace with no App lock. A key the registry no longer
//! lists answers `NotFound`; a tab that shows another view since refuses
//! the write. Which tab is active does not matter: the page may still show
//! the tab the registry left, and the write lands there.

use er_engine::preview_key::{file_preview_key, preview_context_key};
use er_engine::TabKey;
use er_protocol::{
    Ack, Command, ErError, ErErrorKind, LineTarget, PaintedView, ResourceChanged, ResourceKey,
    ResourceVersion,
};
use er_service::{AnnotationKind, AnnotationTarget, Reply, ReviewArtifact, TabContent};

use super::tabs::Routed;
use crate::commands::content_routes;
use crate::state::DesktopState;

const DOCUMENT_CHANGED: &str = "The document changed before the comment was saved";

/// Applies `command` when it is an annotation write.
pub fn apply(desktop: &DesktopState, command: Command) -> Routed {
    let service = |build: Box<dyn FnOnce(er_service::TabContext) -> er_service::Command>| {
        Box::new(move |content: &TabContent| Ok(build(content.context.clone())))
            as Box<dyn FnOnce(&TabContent) -> Result<er_service::Command, String>>
    };
    let (tab, view, build) = match command {
        Command::AddQuestion {
            tab,
            view,
            target,
            body,
            side,
            client_id,
        } => (
            tab,
            view,
            service(Box::new(move |context| er_service::Command::AddQuestion {
                context,
                target: line_target(target),
                body,
                side,
                client_id,
                reply_to: None,
                finding_ref: None,
            })),
        ),
        Command::AddNote {
            tab,
            view,
            target,
            body,
            side,
            client_id,
        } => (
            tab,
            view,
            service(Box::new(move |context| er_service::Command::AddNote {
                context,
                target: line_target(target),
                body,
                side,
                client_id,
                reply_to: None,
                finding_ref: None,
            })),
        ),
        Command::AddComment {
            tab,
            view,
            target,
            body,
            side,
            client_id,
        } => (
            tab,
            view,
            service(Box::new(move |context| er_service::Command::AddComment {
                context,
                target: line_target(target),
                body,
                side,
                client_id,
            })),
        ),
        Command::AddDocumentThread {
            tab,
            path,
            thread_kind,
            line,
            line_end,
            body,
            preview_key,
        } => {
            let thread = DocumentThread {
                path,
                kind: thread_kind,
                line,
                line_end,
                body,
                preview_key,
            };
            return Routed::Applied(document_thread(desktop, tab, thread));
        }
        Command::ReplyToThread {
            tab,
            view,
            parent_id,
            body,
            client_id,
        } => (
            tab,
            view,
            service(Box::new(move |context| {
                er_service::Command::ReplyToThread {
                    context,
                    parent_id,
                    body,
                    client_id,
                }
            })),
        ),
        Command::ReplyToGithubThread {
            tab,
            view,
            parent_id,
            body,
            client_id,
        } => (
            tab,
            view,
            service(Box::new(move |context| {
                er_service::Command::ReplyToGithubThread {
                    context,
                    parent_id,
                    body,
                    client_id,
                }
            })),
        ),
        Command::UpdateThreadMessage {
            tab,
            view,
            id,
            body,
        } => (
            tab,
            view,
            service(Box::new(move |context| {
                er_service::Command::UpdateThreadMessage { context, id, body }
            })),
        ),
        Command::DeleteThread { tab, view, id } => {
            return Routed::Applied(delete_thread(desktop, tab, &view, id));
        }
        Command::ResolveThread { tab, view, id } => (
            tab,
            view,
            service(Box::new(move |context| {
                er_service::Command::ResolveThread { context, id }
            })),
        ),
        Command::PromoteToNote {
            tab,
            view,
            id,
            body,
        } => (
            tab,
            view,
            service(Box::new(move |context| {
                er_service::Command::PromoteToNote { context, id, body }
            })),
        ),
        Command::PromoteToComment {
            tab,
            view,
            id,
            body,
        } => (
            tab,
            view,
            service(Box::new(move |context| {
                er_service::Command::PromoteToComment { context, id, body }
            })),
        ),
        Command::PromoteFindingToComment {
            tab,
            view,
            finding_id,
            body,
        } => (
            tab,
            view,
            service(Box::new(move |context| {
                er_service::Command::PromoteFindingToComment {
                    context,
                    finding_id,
                    body,
                }
            })),
        ),
        Command::ReplyToFinding {
            tab,
            view,
            finding_id,
            body,
            client_id,
        } => (
            tab,
            view,
            service(Box::new(move |context| {
                er_service::Command::ReplyToFinding {
                    context,
                    finding_id,
                    body,
                    ai_assist: false,
                    client_id,
                }
            })),
        ),
        Command::DismissFinding {
            tab,
            view,
            finding_id,
        } => (
            tab,
            view,
            service(Box::new(move |context| {
                er_service::Command::DismissFinding {
                    context,
                    finding_id,
                }
            })),
        ),
        Command::SetFindingResolved {
            tab,
            view,
            finding_id,
            resolved,
        } => (
            tab,
            view,
            service(Box::new(move |context| {
                er_service::Command::SetFindingResolved {
                    context,
                    finding_id,
                    resolved,
                }
            })),
        ),
        Command::RemoveFindingThread {
            tab,
            view,
            finding_id,
        } => return Routed::Applied(remove_finding_thread(desktop, tab, &view, finding_id)),
        Command::UpdateFindingResponse {
            tab,
            view,
            finding_id,
            response_id,
            body,
        } => (
            tab,
            view,
            service(Box::new(move |context| {
                er_service::Command::UpdateFindingResponse {
                    context,
                    finding_id,
                    response_id,
                    body,
                }
            })),
        ),
        Command::DeleteFindingResponse {
            tab,
            view,
            finding_id,
            response_id,
        } => (
            tab,
            view,
            service(Box::new(move |context| {
                er_service::Command::DeleteFindingResponse {
                    context,
                    finding_id,
                    response_id,
                }
            })),
        ),
        Command::ToggleChecklistItem {
            tab,
            view,
            index: at,
        } => (
            tab,
            view,
            service(Box::new(move |context| {
                er_service::Command::ToggleChecklistItem {
                    context,
                    index: index(at),
                }
            })),
        ),
        Command::DeleteReviewArtifact {
            tab,
            view,
            artifact,
        } => {
            let artifact = match artifact.as_str() {
                "triage" => ReviewArtifact::Triage,
                "review" => ReviewArtifact::Review,
                other => {
                    let unknown = format!("Unknown review artifact kind: {other}");
                    return Routed::Applied(Err(refused(unknown)));
                }
            };
            (
                tab,
                view,
                service(Box::new(move |context| {
                    er_service::Command::DeleteReviewArtifact { context, artifact }
                })),
            )
        }
        Command::DeleteDiagram { tab, view, id } => (
            tab,
            view,
            service(Box::new(move |context| {
                er_service::Command::DeleteDiagram { context, id }
            })),
        ),
        other => return Routed::Other(other),
    };
    Routed::Applied(write(desktop, tab, Some(&view), build).map(|(ack, _)| ack))
}

/// Resolves `wire` in the registry and sends the write to that tab, then
/// answers `annotations(tab)` at the version the write published, and the id
/// of what it created.
fn write(
    desktop: &DesktopState,
    wire: er_protocol::TabKey,
    view: Option<&PaintedView>,
    build: impl FnOnce(&TabContent) -> Result<er_service::Command, String>,
) -> Result<(Ack, TabKey), ErError> {
    let key = er_service::resolve_tab_key(&desktop.service.app_registry().tabs_view(), wire)?;
    let (reply, tab) = content_routes::write_on_key(desktop, key, view, build).map_err(refused)?;
    let version = desktop
        .service
        .content(key, &tab)
        .map_or(0, |content| er_service::annotations_version(&content));
    let ack = Ack {
        changed: vec![ResourceChanged {
            key: ResourceKey::Annotations { tab: wire },
            version: ResourceVersion(version),
        }],
        created: created(reply),
    };
    Ok((ack, key))
}

/// The id of the row a create wrote; none for an edit.
fn created(reply: Reply) -> Option<String> {
    match reply {
        Reply::AnnotationWritten { id }
        | Reply::NotePromoted { id }
        | Reply::CommentPromoted { id } => Some(id),
        Reply::FindingReplied { root_id, .. } => Some(root_id),
        _ => None,
    }
}

/// Deletes a thread, then the ask-AI reply still being written for it, which
/// moves `jobs`.
fn delete_thread(
    desktop: &DesktopState,
    wire: er_protocol::TabKey,
    view: &PaintedView,
    id: String,
) -> Result<Ack, ErError> {
    let thread = id.clone();
    let (mut ack, _) = write(desktop, wire, Some(view), |content| {
        Ok(er_service::Command::DeleteThread {
            context: content.context.clone(),
            id: thread,
        })
    })?;
    drop_pending_replies(desktop, &mut ack, &[id]);
    Ok(ack)
}

/// Deletes the threads linked to a finding, then the ask-AI replies still
/// being written for them, read from the content the write was sent with.
fn remove_finding_thread(
    desktop: &DesktopState,
    wire: er_protocol::TabKey,
    view: &PaintedView,
    finding_id: String,
) -> Result<Ack, ErError> {
    let mut pending = Vec::new();
    let (mut ack, _) = write(desktop, wire, Some(view), |content| {
        let ai = &content.ai.state;
        pending = crate::commands::finding_linked_thread_ids(ai, &finding_id);
        pending.extend(er_engine::ai::find_finding_thread_root(ai, &finding_id));
        Ok(er_service::Command::RemoveFindingThread {
            context: content.context.clone(),
            finding_id,
        })
    })?;
    drop_pending_replies(desktop, &mut ack, &pending);
    Ok(ack)
}

/// Drops the in-flight ask-AI replies keyed by `ids`, and names `jobs` in
/// `ack` when that moved it.
fn drop_pending_replies(desktop: &DesktopState, ack: &mut Ack, ids: &[String]) {
    let before = super::jobs::version(desktop);
    desktop
        .pending_ai_replies
        .remove(ids.iter().map(String::as_str));
    let after = super::jobs::version(desktop);
    if after != before {
        ack.changed.push(ResourceChanged {
            key: ResourceKey::Jobs,
            version: ResourceVersion(after),
        });
    }
}

struct DocumentThread {
    path: String,
    kind: String,
    line: u32,
    line_end: Option<u32>,
    body: String,
    preview_key: String,
}

/// A thread on a previewed document line. The text is read off the actor
/// from the tab's own content, which is checked before and after the read;
/// a preview key that differs from the content's means the line was picked
/// in other text, and the write is refused.
fn document_thread(
    desktop: &DesktopState,
    wire: er_protocol::TabKey,
    thread: DocumentThread,
) -> Result<Ack, ErError> {
    let kind = match thread.kind.as_str() {
        "question" => AnnotationKind::Question,
        "note" => AnnotationKind::Note,
        other => return Err(refused(format!("A document anchor cannot hold a {other}"))),
    };
    let entry = super::diff::listed(desktop, wire)?;
    let content = desktop
        .service
        .content(entry.key, &entry.descriptor)
        .ok_or_else(|| refused(DOCUMENT_CHANGED.to_string()))?;
    let context = preview_context_key(&content.preview_key_inputs());
    let preview = super::preview::document(desktop, &entry, &thread.path, &context)?;
    if preview.preview_key != thread.preview_key {
        return Err(refused(DOCUMENT_CHANGED.to_string()));
    }
    let DocumentThread {
        path,
        line,
        line_end,
        body,
        preview_key,
        ..
    } = thread;
    write(desktop, wire, None, move |content| {
        if file_preview_key(&content.preview_key_inputs(), &path) != preview_key {
            return Err(DOCUMENT_CHANGED.to_string());
        }
        Ok(er_service::Command::AddDocumentThread {
            context: content.context.clone(),
            file: path,
            line: index(line),
            line_end: line_end.map(index),
            kind,
            body,
            source: preview.text,
        })
    })
    .map(|(ack, _)| ack)
}

fn line_target(target: LineTarget) -> AnnotationTarget {
    AnnotationTarget {
        file: target.file,
        hunk_idx: index(target.hunk_idx),
        line_num: target.line_num.map(index),
        line_num_end: target.line_num_end.map(index),
    }
}

fn index(n: u32) -> usize {
    usize::try_from(n).unwrap_or(usize::MAX)
}

fn refused(message: String) -> ErError {
    ErError {
        kind: ErErrorKind::Refused,
        message,
    }
}
