//! What a tab's buckets hold, on the flag path: `annotations(tab)`, built
//! from the AI section its workspace published, the browser annotations,
//! which only the desktop writes, and the review reads computed on each
//! fetch. Each reads the bucket of the content the
//! tab's workspace published, `context.er_dir`; the GitHub-comment bucket is
//! never one of them.

use std::sync::Arc;

use er_engine::tabs_record::TabDescriptor;
use er_engine::TabKey;
use er_protocol::{
    AnnotationScreenshot, ErError, ErErrorKind, Resource, ReviewJson, ReviewRevision,
    ReviewRevisions, UiAnnotation, UiAnnotations,
};
use er_service::TabContent;

use crate::state::DesktopState;

/// `annotations(tab)` from the AI section the tab's workspace published. A
/// listed tab nothing loaded posts `LoadTab` and answers empty at version 0.
pub fn annotations(
    desktop: &DesktopState,
    wire: er_protocol::TabKey,
) -> Result<(u64, Resource), ErError> {
    let Some(content) = loaded(desktop, wire)? else {
        return Ok((0, Resource::Annotations(Box::default())));
    };
    let annotations = er_service::annotations_resource(&content);
    let version = er_service::annotations_version(&content);
    Ok((version, Resource::Annotations(Box::new(annotations))))
}

/// `annotations(tab)`'s version, read without building it or loading anything.
pub fn annotations_version(desktop: &DesktopState, key: TabKey, tab: &TabDescriptor) -> u64 {
    desktop
        .service
        .content(key, tab)
        .map_or(0, |content| er_service::annotations_version(&content))
}

/// `ui_annotations(tab)` from the annotations file in the tab's view bucket.
/// A listed tab nothing loaded posts `LoadTab` and answers none at version 0.
pub fn ui_annotations(
    desktop: &DesktopState,
    wire: er_protocol::TabKey,
) -> Result<(u64, Resource), ErError> {
    ui_annotations_read(desktop, wire, er_engine::ai::load_ui_annotations)
}

/// `ui_annotations(tab)` with the rows read by `read`. The version is read
/// first, so a write landing during the read is pushed again under a newer
/// version. Read after, it would pair the old rows with the new version, the
/// relay would announce that version, and the page would never refetch.
pub(crate) fn ui_annotations_read(
    desktop: &DesktopState,
    wire: er_protocol::TabKey,
    read: impl FnOnce(&str) -> Vec<er_engine::ai::UiAnnotation>,
) -> Result<(u64, Resource), ErError> {
    let Some(content) = loaded(desktop, wire)? else {
        return Ok((0, Resource::UiAnnotations(UiAnnotations::default())));
    };
    let version = ui_version(desktop, &content);
    let rows = read(&content.context.er_dir)
        .into_iter()
        .map(ui_row)
        .collect();
    Ok((version, Resource::UiAnnotations(UiAnnotations { rows })))
}

/// The desktop's count of its annotation writes plus the load that chose the
/// bucket: both only rise, so a write or a move to another bucket moves it.
pub fn ui_version(desktop: &DesktopState, content: &TabContent) -> u64 {
    desktop.ui_annotations.revision() + content.mode_revision
}

/// `ui_annotations(tab)`'s version, read without loading anything.
pub fn ui_annotations_version(desktop: &DesktopState, key: TabKey, tab: &TabDescriptor) -> u64 {
    desktop
        .service
        .content(key, tab)
        .map_or(0, |content| ui_version(desktop, &content))
}

/// The tab's review, projected as the reviewer sees it. A listed tab nothing
/// loaded posts `LoadTab` and answers none at version 0, as does a bucket
/// with no review, so neither fails a batched fetch.
pub fn review_json(
    desktop: &DesktopState,
    wire: er_protocol::TabKey,
) -> Result<(u64, Resource), ErError> {
    let Some(content) = loaded(desktop, wire)? else {
        return Ok((0, Resource::ReviewJson(ReviewJson::default())));
    };
    let scope = content.tab.identity().storage_branch_scope();
    let json = crate::commands::review_projection(
        &content.context.er_dir,
        &content.context.branch_diff_hash,
        scope,
    )
    .map_err(|message| not_found(&message))?;
    Ok((0, Resource::ReviewJson(ReviewJson { json })))
}

pub fn review_revisions(
    desktop: &DesktopState,
    wire: er_protocol::TabKey,
) -> Result<(u64, Resource), ErError> {
    let revisions = loaded(desktop, wire)?.map_or_else(Vec::new, |content| {
        let context = &content.context;
        crate::commands::review_revisions(
            &context.repo_root,
            &context.er_dir,
            &context.branch_diff_hash,
        )
    });
    let revisions = revisions
        .into_iter()
        .map(|r| ReviewRevision {
            revision_id: r.revision_id,
            created_at: r.created_at,
            scope: r.scope,
            diff_hash: r.diff_hash,
            active: r.active,
            agents: r.agents,
        })
        .collect();
    Ok((0, Resource::ReviewRevisions(ReviewRevisions { revisions })))
}

/// A saved screenshot by its path, which reads no bucket.
pub fn screenshot(path: &str) -> Result<(u64, Resource), ErError> {
    let data_url = crate::commands::read_annotation_screenshot(path.to_string())
        .map_err(|message| not_found(&message))?;
    Ok((
        0,
        Resource::AnnotationScreenshot(AnnotationScreenshot { data_url }),
    ))
}

/// The content a listed tab's workspace published; none, with a load
/// posted, while nothing has loaded.
fn loaded(
    desktop: &DesktopState,
    wire: er_protocol::TabKey,
) -> Result<Option<Arc<TabContent>>, ErError> {
    let entry = super::diff::listed(desktop, wire)?;
    let content = desktop.service.content(entry.key, &entry.descriptor);
    if content.is_none() {
        desktop.service.request_load(entry.key, &entry.descriptor);
    }
    Ok(content)
}

fn ui_row(row: er_engine::ai::UiAnnotation) -> UiAnnotation {
    UiAnnotation {
        id: row.id,
        url: row.url,
        selector: row.selector,
        box_x: row.box_x,
        box_y: row.box_y,
        box_w: row.box_w,
        box_h: row.box_h,
        viewport_w: row.viewport_w,
        viewport_h: row.viewport_h,
        text: row.text,
        timestamp: row.timestamp,
        author: row.author,
        screenshot_path: row.screenshot_path,
        stale: row.stale,
        element_context: row.element_context,
        dom_context_json: row.dom_context.map(|value| value.to_string()),
    }
}

fn not_found(message: &str) -> ErError {
    ErError {
        kind: ErErrorKind::NotFound,
        message: message.to_string(),
    }
}
