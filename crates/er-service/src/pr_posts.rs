//! The PR-level posts: a PR-wide comment, a line comment outside any review,
//! and a bare review decision, run as actor effects.
//!
//! The two comments go through the shared post session, and the line poster's
//! own pending-review check refuses a line comment GitHub would attach to a
//! draft. A PR-wide comment sent with a client id reconciles a resend after an
//! unknown outcome; a line comment and one sent with none take a fresh id, so
//! such a resend can post a second copy, as the flag-off desktop can.

use er_engine::git::{parse_diff, DiffFile};
use er_engine::github_post::PrPostTarget;
use er_engine::github_post_session::NewGitHubComment;

use crate::actor::WorkspaceDeps;
use crate::annotations::fetch_raw;
use crate::github_comments::{post, post_with};
use crate::github_sync::{pr_target, Fetched};
use crate::handle::WorkspaceError;
use crate::pending_review::refuse_own_pr;
use crate::protocol::{PrRef, ReviewEvent, ReviewLineTarget, TabContext};

fn pr_ref(pr: &PrPostTarget) -> PrRef {
    PrRef {
        owner: pr.owner.clone(),
        repo: pr.repo.clone(),
        number: pr.pr,
    }
}

fn posted(context: TabContext, pr: PrRef, decision: bool) -> Fetched {
    Fetched::Posted {
        context,
        pr,
        decision,
    }
}

pub(crate) fn post_pr_comment(
    deps: &WorkspaceDeps,
    context: TabContext,
    body: &str,
    client_id: Option<String>,
) -> Result<Fetched, WorkspaceError> {
    let comment = post(
        deps,
        &context,
        NewGitHubComment::general(body.to_string()),
        client_id,
    )?;
    let pr = pr_ref(&comment.request.pr);
    Ok(posted(context, pr, false))
}

/// The hunk of `file` holding `line` on `side`, so the recorded row anchors
/// where the line is; the first hunk when the line is not in the diff.
fn hunk_index(files: &[DiffFile], file: &str, line: usize, side: &str) -> usize {
    let on_side = |old: Option<usize>, new: Option<usize>| {
        if side == "LEFT" {
            old == Some(line)
        } else {
            new == Some(line)
        }
    };
    files
        .iter()
        .find(|diff| diff.path == file)
        .and_then(|diff| {
            diff.hunks.iter().position(|hunk| {
                hunk.lines
                    .iter()
                    .any(|row| on_side(row.old_num, row.new_num))
            })
        })
        .unwrap_or(0)
}

pub(crate) fn post_line_comment(
    deps: &WorkspaceDeps,
    context: TabContext,
    target: &ReviewLineTarget,
    body: &str,
) -> Result<Fetched, WorkspaceError> {
    let entry = er_github::review_batch_entry(
        &target.file,
        target.line_num,
        target.line_num_end,
        target.side.as_deref(),
        body,
    )
    .map_err(WorkspaceError::Failed)?;
    let files = parse_diff(&fetch_raw(deps, &context)?);
    let hunk_idx = hunk_index(&files, &target.file, target.line_num, &entry.side);
    let intent = NewGitHubComment::line(
        target.file.clone(),
        hunk_idx,
        Some(target.line_num),
        target.line_num_end,
        Some(entry.side),
        body.to_string(),
    );
    let comment = post_with(deps, &context, intent, None, &files)?;
    let pr = pr_ref(&comment.request.pr);
    Ok(posted(context, pr, false))
}

pub(crate) fn submit_decision(
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
    let (event, summary) = (event.as_str(), summary.trim());
    if context.is_remote {
        github.submit_review_remote(&pr.owner, &pr.repo, pr.number, &[], event, summary)?;
    } else {
        let root = context.repo_root.as_str();
        github.submit_review(&pr.owner, &pr.repo, pr.number, &[], root, event, summary)?;
    }
    Ok(posted(context, pr, true))
}

/// The wording for a failed review write. GitHub's classification picks the
/// hints; no error text is read.
pub fn review_write_message(error: WorkspaceError) -> String {
    match error {
        WorkspaceError::Github(gh) => {
            let unprocessable = matches!(gh, er_engine::github::GhError::Unprocessable { .. });
            with_review_hints(gh.to_string(), unprocessable)
        }
        other => other.to_string(),
    }
}

/// A failed review write, with the likely causes of an unprocessable one.
pub fn with_review_hints(raw: String, unprocessable: bool) -> String {
    if !unprocessable {
        return format!("GitHub review submission failed: {raw}");
    }
    let hints = [
        "blank review payload",
        "invalid or stale line anchor",
        "comment position no longer matches PR head",
    ];
    format!(
        "GitHub review submission failed: {raw}\nLikely causes: {}",
        hints.join("; ")
    )
}
