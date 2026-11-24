//! The finding, checklist and review-artifact writes, run as actor effects.
//! Each writes the tab's view bucket through the engine helper the App's
//! command calls, and the actor then reads its readers' AI state again, so
//! the workspace stays the one writer of the AI section.

use std::path::Path;

use er_engine::ai;

use crate::handle::WorkspaceError;
use crate::protocol::{ReviewArtifact, TabContext};

fn failed(context: &str, error: impl std::fmt::Display) -> WorkspaceError {
    WorkspaceError::Failed(format!("{context}: {error:#}"))
}

fn not_found(finding_id: &str) -> WorkspaceError {
    WorkspaceError::Failed(format!("Finding not found: {finding_id}"))
}

/// Removes a finding from the review, the threads linked to it in both
/// buckets, and its promotion entry.
pub(crate) fn dismiss_finding(ctx: &TabContext, finding_id: &str) -> Result<(), WorkspaceError> {
    let removed = ai::remove_finding_from_sidecars(&ctx.er_dir, finding_id)
        .map_err(|error| failed("Failed to remove finding", error))?;
    if !removed {
        return Err(not_found(finding_id));
    }
    // The finding is gone either way; a thread left behind shows unlinked.
    let _ = ai::delete_threads_linked_to_finding(&ctx.er_dir, &ctx.comments_dir, finding_id);
    let mut promotions = ai::load_finding_promotions(&ctx.er_dir);
    if promotions.remove(finding_id).is_some() {
        ai::save_finding_promotions(&ctx.er_dir, &promotions)
            .map_err(|error| failed("Failed to update finding promotions", error))?;
    }
    Ok(())
}

/// Marks a finding resolved, or reopens it (ADR 0047).
pub(crate) fn set_finding_resolved(
    ctx: &TabContext,
    finding_id: &str,
    resolved: bool,
) -> Result<(), WorkspaceError> {
    let found = ai::set_finding_resolved(&ctx.er_dir, finding_id, resolved)
        .map_err(|error| failed("Failed to update finding", error))?;
    if found {
        Ok(())
    } else {
        Err(not_found(finding_id))
    }
}

/// Deletes the threads linked to a finding; the finding stays.
pub(crate) fn remove_finding_thread(
    ctx: &TabContext,
    finding_id: &str,
) -> Result<(), WorkspaceError> {
    ai::delete_threads_linked_to_finding(&ctx.er_dir, &ctx.comments_dir, finding_id)
        .map(drop)
        .map_err(|error| failed("Failed to remove finding thread", error))
}

pub(crate) fn update_finding_response(
    ctx: &TabContext,
    finding_id: &str,
    response_id: &str,
    body: &str,
) -> Result<(), WorkspaceError> {
    ai::update_finding_response(&ctx.er_dir, finding_id, response_id, body)
        .map_err(|error| failed("Failed to update the response", error))
}

/// Appends an ask's answer to a finding's responses.
pub(crate) fn append_finding_response(
    ctx: &TabContext,
    finding_id: &str,
    body: &str,
) -> Result<(), WorkspaceError> {
    ai::append_finding_response(&ctx.er_dir, finding_id, body)
        .map(drop)
        .map_err(|error| failed("Failed to save finding validation", error))
}

pub(crate) fn delete_finding_response(
    ctx: &TabContext,
    finding_id: &str,
    response_id: &str,
) -> Result<(), WorkspaceError> {
    ai::delete_finding_response(&ctx.er_dir, finding_id, response_id)
        .map_err(|error| failed("Failed to delete the response", error))
}

/// Flips one checklist item's flag from the stored value, so the store's
/// lock orders two toggles sent close together. An item the checklist no
/// longer holds writes nothing: the generator rewrote it since the page
/// read it.
pub(crate) fn toggle_checklist_item(ctx: &TabContext, item_id: &str) -> Result<(), WorkspaceError> {
    let store = er_engine::store_registry::store();
    let scope =
        er_store::Scope::dir(&ctx.er_dir).map_err(|error| failed("open the bucket", error))?;
    match ai::checklist_store::toggle_checked(&store, &scope, item_id) {
        Ok(_) | Err(er_store::UpdateError::Apply(_)) => Ok(()),
        Err(er_store::UpdateError::Store(error)) => {
            Err(failed("Failed to toggle checklist item", error))
        }
    }
}

/// Deletes the triage, or every review artifact, while it holds the bucket's
/// admission: refused while a review flow holds it, and no flow takes it
/// until the delete is done.
pub(crate) fn delete_review_artifact(
    ctx: &TabContext,
    artifact: ReviewArtifact,
) -> Result<(), WorkspaceError> {
    let _admitted = admitted(ctx)?;
    let deleted = match artifact {
        ReviewArtifact::Triage => ai::review_cleanup::cleanup_triage(&ctx.er_dir),
        ReviewArtifact::Review => ai::review_cleanup::cleanup_review_artifacts(&ctx.er_dir),
    };
    deleted.map_err(|error| failed("Failed to delete the review", error))
}

/// Runs one of the draft clears on the view bucket, holding its admission as
/// the review delete does.
pub(crate) fn clear_drafts(
    ctx: &TabContext,
    clear: fn(&str) -> Result<(), er_store::StoreError>,
) -> Result<(), WorkspaceError> {
    let _admitted = admitted(ctx)?;
    clear(&ctx.er_dir).map_err(|error| failed("Failed to clear", error))
}

fn admitted(ctx: &TabContext) -> Result<er_store::LeaseGuard, WorkspaceError> {
    er_engine::arena::hold_bucket_admission(&ctx.er_dir)
        .map_err(|error| WorkspaceError::Failed(format!("{error:#}")))
}

pub(crate) fn delete_diagram(ctx: &TabContext, id: &str) -> Result<(), WorkspaceError> {
    if !ai::is_safe_diagram_id(id) {
        return Err(WorkspaceError::Failed(format!("Invalid diagram id: {id}")));
    }
    let path = Path::new(&ctx.er_dir)
        .join("diagrams")
        .join(format!("{id}.json"));
    if path.exists() {
        std::fs::remove_file(&path).map_err(|error| failed("Failed to delete diagram", error))?;
    }
    Ok(())
}
