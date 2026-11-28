//! The browser annotation writes. Each names the tab and view the page
//! painted, as the other annotation writes do, and goes to that tab's
//! workspace as a command whose effect writes the bucket of the tab's
//! content, `context.er_dir` (ADR 0067). The GitHub-comment bucket,
//! `context.comments_dir`, is a PR tab's other bucket and never holds them.

use er_protocol::{
    Ack, Command, ErError, ErErrorKind, NewUiAnnotation, PaintedView, ResourceChanged, ResourceKey,
    ResourceVersion,
};
use er_service::ui_annotations::{self as service, UiAnnotationWrite};
use er_service::Reply;

use super::content_routes;
use super::tabs::Routed;
use crate::state::DesktopState;

/// Applies `command` when it is a browser annotation write.
pub fn apply(desktop: &DesktopState, command: Command) -> Routed {
    let (tab, view, write) = match command {
        Command::AddUiAnnotation {
            tab,
            view,
            annotation,
        } => (tab, view, added(*annotation)),
        Command::SaveAnnotationScreenshot {
            tab,
            view,
            id,
            data_url,
        } => (
            tab,
            view,
            Ok(UiAnnotationWrite::Screenshot { id, data_url }),
        ),
        Command::DeleteUiAnnotation { tab, view, id } => {
            (tab, view, Ok(UiAnnotationWrite::Delete { id }))
        }
        Command::ClearUiAnnotationsForPage {
            tab,
            view,
            page_url,
        } => (
            tab,
            view,
            Ok(UiAnnotationWrite::Clear {
                page_url: Some(page_url),
            }),
        ),
        Command::ClearUiAnnotations { tab, view } => {
            (tab, view, Ok(UiAnnotationWrite::Clear { page_url: None }))
        }
        Command::UpdateUiAnnotationAnchors { tab, view, updates } => {
            let updates = updates
                .into_iter()
                .map(|update| service::AnchorUpdate {
                    id: update.id,
                    fresh: update.fresh,
                    new_box: update.new_box,
                })
                .collect();
            (tab, view, Ok(UiAnnotationWrite::Anchors(updates)))
        }
        other => return Routed::Other(other),
    };
    Routed::Applied(
        write
            .map_err(refused)
            .and_then(|write| written(desktop, tab, &view, write)),
    )
}

/// Sends `write` to the workspace of `wire`'s content, when it still shows
/// `view`, and answers `ui_annotations(tab)` at the version the write
/// published.
fn written(
    desktop: &DesktopState,
    wire: er_protocol::TabKey,
    view: &PaintedView,
    write: UiAnnotationWrite,
) -> Result<Ack, ErError> {
    let key = er_service::resolve_tab_key(&desktop.service.app_registry().tabs_view(), wire)?;
    let (reply, tab) = content_routes::write_on_key(desktop, key, Some(view), |content| {
        Ok(er_service::Command::WriteUiAnnotations {
            context: content.context.clone(),
            write,
        })
    })
    .map_err(refused)?;
    let Reply::UiAnnotationsWritten { created } = reply else {
        return Err(refused(format!("unexpected reply: {reply:?}")));
    };
    let version = super::annotations::ui_annotations_version(desktop, key, &tab);
    Ok(Ack {
        changed: vec![ResourceChanged {
            key: ResourceKey::UiAnnotations { tab: wire },
            version: ResourceVersion(version),
        }],
        created,
    })
}

fn added(annotation: NewUiAnnotation) -> Result<UiAnnotationWrite, String> {
    let dom_context = annotation
        .dom_context_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(|e| format!("The DOM context is not JSON: {e}"))?;
    Ok(UiAnnotationWrite::Add(Box::new(service::NewUiAnnotation {
        url: annotation.url,
        selector: annotation.selector,
        bbox: annotation.bbox,
        viewport: annotation.viewport,
        text: annotation.text,
        screenshot_data_url: annotation.screenshot_data_url,
        element_context: annotation.element_context,
        dom_context,
        id: annotation.client_id,
    })))
}

fn refused(message: String) -> ErError {
    ErError {
        kind: ErErrorKind::Refused,
        message,
    }
}
