//! The GitHub senders on the flag path: each names the tab the page sent it
//! from and goes to that tab's workspace with its content's context, with no
//! App lock. A key the registry no longer lists answers `NotFound`.
//!
//! The pending review is read on each fetch, off the actor, and never pushed:
//! GitHub holds the draft, and the page reads it again after each change.

use er_engine::TabKey;
use er_protocol::{
    Ack, Command, ErError, ErErrorKind, PendingReview, PendingReviewComment, Resource,
    ResourceChanged, ResourceKey, ResourceVersion, ReviewLine, ReviewRequest,
};
use er_service::{resolve_tab_key, Reply, ReviewEvent, WorkspaceError};

use super::tabs::Routed;
use crate::commands::{self, content_routes};
use crate::service::actor_key;
use crate::state::DesktopState;

/// Applies `command` when it is a GitHub sender.
pub fn apply(desktop: &DesktopState, command: Command) -> Routed {
    let applied = match command {
        Command::PullGithubComments { tab, force } => pull(desktop, tab, force),
        Command::PostGithubPrComment { tab, body } => post_pr_comment(desktop, tab, &body),
        Command::PostGithubLineComment { tab, target, body } => {
            post_line_comment(desktop, tab, target, &body)
        }
        Command::SubmitGithubPrDecision { tab, mode, summary } => {
            submit_decision(desktop, tab, &mode, &summary)
        }
        Command::AddPendingReviewComment { tab, target, body } => {
            add_pending(desktop, tab, target, &body)
        }
        Command::RemovePendingReviewComment { tab, comment_id } => send(desktop, tab, |context| {
            er_service::Command::RemovePendingReviewComment {
                context,
                comment_node_id: comment_id,
            }
        })
        .map_err(plain)
        .map(|_| Ack::default()),
        Command::SubmitPendingReview { tab, mode, summary } => {
            submit_pending(desktop, tab, &mode, summary)
        }
        Command::RunGithubPrAction { tab, pr, action } => {
            crate::gh_pr_actions::run_on_tab(desktop, tab, &pr, &action)
        }
        other => return Routed::Other(other),
    };
    Routed::Applied(applied)
}

/// `wire`'s pending review, read now through its workspace's ports.
pub fn pending_review(
    desktop: &DesktopState,
    wire: er_protocol::TabKey,
) -> Result<(u64, Resource), ErError> {
    // A read never loads: a tab with no content yet has no PR to ask about.
    let entry = super::diff::listed(desktop, wire)?;
    let content = desktop
        .service
        .content(entry.key, &entry.descriptor)
        .ok_or_else(|| refused("The tab has not loaded yet".into()))?;
    let (pr, review) = er_service::read_pending_review(desktop.service.deps(), &content.context)
        .map_err(ErError::from)?;
    let review = PendingReview {
        pr_number: pr.number,
        review_id: review.review_id,
        comments: review
            .comments
            .into_iter()
            .map(|c| PendingReviewComment {
                id: c.id,
                database_id: c.database_id,
                path: c.path,
                line: c.line,
                start_line: c.start_line,
                side: c.side,
                body: c.body,
            })
            .collect(),
        viewer: review.viewer,
        review_request: review.review_request.map(|r| ReviewRequest {
            requested_by: r.requested_by,
            requested_at: r.requested_at,
        }),
    };
    Ok((0, Resource::PendingReview(Box::new(review))))
}

/// Sends one command built from `wire`'s content to its workspace, and
/// answers the reply and the tab it went to.
fn send(
    desktop: &DesktopState,
    wire: er_protocol::TabKey,
    build: impl FnOnce(er_service::TabContext) -> er_service::Command,
) -> Result<(Reply, TabKey), Sent> {
    let key =
        resolve_tab_key(&desktop.service.app_registry().tabs_view(), wire).map_err(Sent::Listed)?;
    let (content, tab) =
        content_routes::keyed_content(desktop, key, None).map_err(|m| Sent::Listed(refused(m)))?;
    let reply = desktop
        .service
        .send(&actor_key(&tab), build(content.context.clone()))
        .map_err(Sent::Workspace)?;
    Ok((reply, key))
}

/// Why a send failed: the key or its content, or the workspace's answer.
enum Sent {
    Listed(ErError),
    Workspace(WorkspaceError),
}

/// A failed send with the workspace's text as it is.
fn plain(sent: Sent) -> ErError {
    worded(sent, |error| error.to_string())
}

/// A failed send, with the workspace's text in the old command's words.
fn worded(sent: Sent, message: impl FnOnce(WorkspaceError) -> String) -> ErError {
    match sent {
        Sent::Listed(error) => error,
        Sent::Workspace(error) => ErError {
            kind: ErError::from(error.clone()).kind,
            message: message(error),
        },
    }
}

/// A pull, answering `annotations(tab)` at the version the sync wrote.
fn pull(desktop: &DesktopState, wire: er_protocol::TabKey, force: bool) -> Result<Ack, ErError> {
    let source = er_service::SyncSource::Manual { force };
    let (reply, key) = send(desktop, wire, |context| {
        er_service::Command::SyncGithubComments { context, source }
    })
    .map_err(|sent| worded(sent, |error| format!("GitHub sync error: {error}")))?;
    match reply {
        Reply::CommentsSynced { .. } => Ok(annotations_moved(desktop, wire, key)),
        other => Err(unexpected(&other)),
    }
}

fn annotations_moved(desktop: &DesktopState, wire: er_protocol::TabKey, key: TabKey) -> Ack {
    let view = desktop.service.app_registry().tabs_view();
    let version = view
        .tabs
        .iter()
        .find(|entry| entry.key == key)
        .and_then(|entry| desktop.service.content(key, &entry.descriptor))
        .map_or(0, |content| er_service::annotations_version(&content));
    Ack {
        changed: vec![ResourceChanged {
            key: ResourceKey::Annotations { tab: wire },
            version: ResourceVersion(version),
        }],
        created: None,
    }
}

fn post_pr_comment(
    desktop: &DesktopState,
    wire: er_protocol::TabKey,
    body: &str,
) -> Result<Ack, ErError> {
    let body = body.trim().to_string();
    if body.is_empty() {
        return Err(refused("Comment body cannot be empty".into()));
    }
    let (reply, _) = send(desktop, wire, |context| {
        er_service::Command::PostGithubPrComment { context, body }
    })
    .map_err(|sent| worded(sent, |error| format!("Failed to post comment: {error}")))?;
    posted(desktop, reply)
}

fn post_line_comment(
    desktop: &DesktopState,
    wire: er_protocol::TabKey,
    target: ReviewLine,
    body: &str,
) -> Result<Ack, ErError> {
    let body = commands::review_comment_body(body).map_err(refused)?;
    let target = line(target);
    let (reply, _) = send(desktop, wire, |context| {
        er_service::Command::PostGithubLineComment {
            context,
            target,
            body,
        }
    })
    .map_err(plain)?;
    posted(desktop, reply)
}

fn submit_decision(
    desktop: &DesktopState,
    wire: er_protocol::TabKey,
    mode: &str,
    summary: &str,
) -> Result<Ack, ErError> {
    let event = event(mode)?;
    let summary = summary.trim().to_string();
    // GitHub rejects a REQUEST_CHANGES or COMMENT review with a blank body.
    if event != ReviewEvent::Approve && summary.is_empty() {
        let event = event.as_str();
        return Err(refused(format!(
            "GitHub requires a comment for {event} reviews. Add a summary first."
        )));
    }
    let (reply, _) = send(desktop, wire, |context| {
        er_service::Command::SubmitGithubPrDecision {
            context,
            event,
            summary,
        }
    })
    .map_err(|sent| worded(sent, commands::review_write_message))?;
    posted(desktop, reply)
}

fn add_pending(
    desktop: &DesktopState,
    wire: er_protocol::TabKey,
    target: ReviewLine,
    body: &str,
) -> Result<Ack, ErError> {
    let body = commands::review_comment_body(body).map_err(refused)?;
    let target = line(target);
    let (reply, _) = send(desktop, wire, |context| {
        er_service::Command::AddPendingReviewComment {
            context,
            target,
            body,
        }
    })
    .map_err(|sent| worded(sent, commands::review_write_message))?;
    match reply {
        Reply::PendingReview { pr, .. } => {
            commands::after_github_write(desktop, pr.owner, pr.repo, pr.number);
            Ok(Ack::default())
        }
        other => Err(unexpected(&other)),
    }
}

fn submit_pending(
    desktop: &DesktopState,
    wire: er_protocol::TabKey,
    mode: &str,
    summary: String,
) -> Result<Ack, ErError> {
    let event = event(mode)?;
    let (reply, _) = send(desktop, wire, |context| {
        er_service::Command::SubmitPendingReview {
            context,
            event,
            summary,
        }
    })
    .map_err(|sent| worded(sent, commands::review_write_message))?;
    posted(desktop, reply)
}

/// A post or a decision landed: drop the comment cache and refresh the
/// status, which the push brings.
fn posted(desktop: &DesktopState, reply: Reply) -> Result<Ack, ErError> {
    match reply {
        Reply::CommentPosted { pr } | Reply::ReviewSubmitted { pr } => {
            commands::after_github_write(desktop, pr.owner, pr.repo, pr.number);
            Ok(Ack::default())
        }
        other => Err(unexpected(&other)),
    }
}

fn event(mode: &str) -> Result<ReviewEvent, ErError> {
    ReviewEvent::parse(mode).ok_or_else(|| {
        refused(format!(
            "Invalid review mode: {mode}. Use COMMENT, APPROVE, or REQUEST_CHANGES."
        ))
    })
}

fn line(target: ReviewLine) -> er_service::ReviewLineTarget {
    let index = |n: u64| usize::try_from(n).unwrap_or(usize::MAX);
    er_service::ReviewLineTarget {
        file: target.file,
        line_num: index(target.line_num),
        line_num_end: target.line_num_end.map(index),
        side: target.side,
    }
}

fn unexpected(reply: &Reply) -> ErError {
    ErError {
        kind: ErErrorKind::Internal,
        message: format!("The workspace answered with an unexpected reply: {reply:?}"),
    }
}

fn refused(message: String) -> ErError {
    ErError {
        kind: ErErrorKind::Refused,
        message,
    }
}
