//! The browser annotation writes on the flag path. The desktop is the one
//! writer of `ui-annotations.json` and its screenshots, so these stay desktop
//! writes; each names the tab and view the page painted, as the other
//! annotation writes do, and writes the bucket of that tab's content,
//! `context.er_dir`. The GitHub-comment bucket, `context.comments_dir`, is a
//! PR tab's other bucket and never holds them.

use er_protocol::{
    Ack, AnchorUpdate, Command, ErError, ErErrorKind, NewUiAnnotation, PaintedView,
    ResourceChanged, ResourceKey, ResourceVersion,
};

use super::tabs::Routed;
use crate::commands::{self, content_routes};
use crate::state::DesktopState;

/// A write in a view bucket, answering the id or path it created.
type BucketWrite<'a> = Box<dyn FnOnce(&str) -> Result<Option<String>, String> + 'a>;

/// Applies `command` when it is a browser annotation write.
pub fn apply(desktop: &DesktopState, command: Command) -> Routed {
    let (tab, view, write): (_, _, BucketWrite<'_>) = match command {
        Command::AddUiAnnotation {
            tab,
            view,
            annotation,
        } => (
            tab,
            view,
            Box::new(move |dir| add(dir, *annotation).map(Some)),
        ),
        Command::SaveAnnotationScreenshot {
            tab,
            view,
            id,
            data_url,
        } => (
            tab,
            view,
            Box::new(move |dir| screenshot(dir, &id, &data_url).map(Some)),
        ),
        Command::DeleteUiAnnotation { tab, view, id } => (
            tab,
            view,
            Box::new(move |dir| commands::delete_ui_annotation_in(dir, &id).map(|()| None)),
        ),
        Command::ClearUiAnnotationsForPage {
            tab,
            view,
            page_url,
        } => (
            tab,
            view,
            Box::new(move |dir| {
                commands::clear_ui_annotations_in(dir, Some(&page_url)).map(|()| None)
            }),
        ),
        Command::ClearUiAnnotations { tab, view } => (
            tab,
            view,
            Box::new(|dir| commands::clear_ui_annotations_in(dir, None).map(|()| None)),
        ),
        Command::UpdateUiAnnotationAnchors { tab, view, updates } => (
            tab,
            view,
            Box::new(move |dir| anchors(dir, updates).map(|()| None)),
        ),
        other => return Routed::Other(other),
    };
    Routed::Applied(written(desktop, tab, &view, write))
}

/// Writes in the view bucket of `wire`'s content, when it still shows
/// `view`, then moves `ui_annotations` and answers it at its new version.
/// The whole of it runs under `ui_annotations_io`, so two writes sent
/// together land one after the other.
fn written(
    desktop: &DesktopState,
    wire: er_protocol::TabKey,
    view: &PaintedView,
    write: BucketWrite<'_>,
) -> Result<Ack, ErError> {
    let _writing = desktop
        .ui_annotations_io
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let key = er_service::resolve_tab_key(&desktop.service.app_registry().tabs_view(), wire)?;
    let (content, _) = content_routes::keyed_content(desktop, key, Some(view)).map_err(refused)?;
    let created = write(&content.context.er_dir).map_err(refused)?;
    desktop.ui_annotations.bump();
    crate::profile_log::bump_desktop_revision(&desktop.desktop_revision, "ui_annotation_write");
    let version = super::annotations::ui_version(desktop, &content);
    Ok(Ack {
        changed: vec![ResourceChanged {
            key: ResourceKey::UiAnnotations { tab: wire },
            version: ResourceVersion(version),
        }],
        created,
    })
}

fn add(dir: &str, annotation: NewUiAnnotation) -> Result<String, String> {
    let dom_context = annotation
        .dom_context_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(|e| format!("The DOM context is not JSON: {e}"))?;
    let new = commands::NewUiAnnotation {
        url: annotation.url,
        selector: annotation.selector,
        bbox: annotation.bbox,
        viewport: annotation.viewport,
        text: annotation.text,
        screenshot_data_url: annotation.screenshot_data_url,
        element_context: annotation.element_context,
        dom_context,
        id: annotation.client_id,
    };
    commands::write_ui_annotation(dir, new)
}

fn screenshot(dir: &str, id: &str, data_url: &str) -> Result<String, String> {
    let bytes = commands::decode_data_url_png(data_url)
        .ok_or_else(|| "The screenshot is not a PNG data URL".to_string())?;
    commands::save_annotation_screenshot_in(dir, id, &bytes).map(|(path, _)| path)
}

fn anchors(dir: &str, updates: Vec<AnchorUpdate>) -> Result<(), String> {
    let updates: Vec<commands::AnchorUpdate> = updates
        .into_iter()
        .map(|update| commands::AnchorUpdate {
            id: update.id,
            fresh: update.fresh,
            new_box: update.new_box,
        })
        .collect();
    commands::apply_anchor_updates(dir, &updates).map_err(|e| e.to_string())
}

fn refused(message: String) -> ErError {
    ErError {
        kind: ErErrorKind::Refused,
        message,
    }
}
