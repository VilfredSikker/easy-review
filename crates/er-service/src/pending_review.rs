//! The viewer's pending review on the tab's PR, run as actor effects.
//!
//! GitHub holds the draft, so every command is a round trip. A write reads
//! the review back, so the reply carries the view the popover renders.

use crate::actor::WorkspaceDeps;
use crate::github_sync::{pr_target, Fetched};
use crate::handle::WorkspaceError;
use crate::protocol::{PrRef, ReviewEvent, ReviewLineTarget, TabContext};

/// The refusal for approving a PR the viewer opened, as GitHub words the rule.
pub const OWN_PR_APPROVAL_REFUSED: &str = "GitHub does not allow approving your own pull request.";

fn read_back(
    deps: &WorkspaceDeps,
    context: TabContext,
    pr: PrRef,
    changed: bool,
) -> Result<Fetched, WorkspaceError> {
    let review = deps
        .ports()
        .github
        .pending_review(&pr.owner, &pr.repo, pr.number)?;
    Ok(Fetched::PendingReview {
        context,
        pr,
        review,
        changed,
    })
}

pub(crate) fn get(deps: &WorkspaceDeps, context: TabContext) -> Result<Fetched, WorkspaceError> {
    let pr = pr_target(deps, &context)?;
    read_back(deps, context, pr, false)
}

/// The viewer's pending review on the tab's PR, read on the caller's thread.
/// It writes nothing, so a reader needs no actor.
pub fn read_pending_review(
    deps: &WorkspaceDeps,
    context: &TabContext,
) -> Result<(PrRef, er_github::PendingReview), WorkspaceError> {
    let pr = pr_target(deps, context)?;
    let review = deps
        .ports()
        .github
        .pending_review(&pr.owner, &pr.repo, pr.number)?;
    Ok((pr, review))
}

pub(crate) fn add_comment(
    deps: &WorkspaceDeps,
    context: TabContext,
    target: &ReviewLineTarget,
    body: &str,
) -> Result<Fetched, WorkspaceError> {
    let body = body.trim();
    if body.is_empty() {
        return Err(WorkspaceError::Failed(
            "Comment body cannot be empty".to_string(),
        ));
    }
    let entry = er_github::review_batch_entry(
        &target.file,
        target.line_num,
        target.line_num_end,
        target.side.as_deref(),
        body,
    )
    .map_err(WorkspaceError::Failed)?;
    let pr = pr_target(deps, &context)?;
    let github = &deps.ports().github;
    github.pending_review_add_comment(&pr.owner, &pr.repo, pr.number, &entry)?;
    // A posted comment changes what the next pull returns.
    github.invalidate_comments();
    read_back(deps, context, pr, true)
}

pub(crate) fn remove_comment(
    deps: &WorkspaceDeps,
    context: TabContext,
    comment_node_id: &str,
) -> Result<Fetched, WorkspaceError> {
    let pr = pr_target(deps, &context)?;
    deps.ports()
        .github
        .pending_review_remove_comment(comment_node_id)?;
    read_back(deps, context, pr, true)
}

pub(crate) fn submit(
    deps: &WorkspaceDeps,
    context: TabContext,
    event: ReviewEvent,
    summary: &str,
) -> Result<Fetched, WorkspaceError> {
    let pr = pr_target(deps, &context)?;
    if event == ReviewEvent::Approve {
        refuse_own_pr(deps, &context, &pr)?;
    }
    let github = &deps.ports().github;
    github.pending_review_submit(
        &pr.owner,
        &pr.repo,
        pr.number,
        event.as_str(),
        summary.trim(),
    )?;
    github.invalidate_comments();
    Ok(Fetched::ReviewSubmitted { context, pr })
}

/// Refuses an approval of the viewer's own PR before GitHub does. The author
/// comes from the overview the sync reads; when either side is unknown the
/// submit goes ahead and GitHub has the last word.
pub(crate) fn refuse_own_pr(
    deps: &WorkspaceDeps,
    context: &TabContext,
    pr: &PrRef,
) -> Result<(), WorkspaceError> {
    let github = &deps.ports().github;
    let repo_root = (!context.is_remote).then_some(context.repo_root.as_str());
    let Ok(viewer) = github.viewer_login(repo_root) else {
        return Ok(());
    };
    let overview = if context.is_remote {
        github.overview_remote(&pr.owner, &pr.repo, pr.number)
    } else {
        github.overview_no_checks(&context.repo_root, Some(pr.number))
    };
    let author = overview.ok().flatten().map(|overview| overview.author);
    let own = author.is_some_and(|author| {
        let author = author.trim();
        !author.is_empty() && author.eq_ignore_ascii_case(viewer.trim())
    });
    if own {
        return Err(WorkspaceError::Failed(OWN_PR_APPROVAL_REFUSED.to_string()));
    }
    Ok(())
}
