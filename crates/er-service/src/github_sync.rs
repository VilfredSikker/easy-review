//! GitHub reads the actor applies: the comment sync and the pending review.
//!
//! An effect here calls GitHub and returns a [`Fetched`] value; [`apply`] runs
//! on the actor thread. A sync's sidecar write happens in `apply`, never in
//! the effect, so the actor is the one writer of the pulled comments.

use er_engine::ai;
use er_engine::annotations::WrittenPayload;
use er_engine::git::parse_diff;
use er_engine::github_post::{resolve_post_target_with, CommentPostContext, GitHubPostError};
use er_engine::store_registry;
use er_engine::sync::{
    apply_comment_sync, fetch_comment_sync, CommentSyncContext, FetchedCommentSync,
};
use er_store::{ArtifactStore, Scope};

use crate::actor::WorkspaceDeps;
use crate::annotations::{failed, fetch_raw};
use crate::content::ContentPatch;
use crate::handle::WorkspaceError;
use crate::protocol::{Event, PrRef, Reply, SyncSource, TabContext};

/// What an effect read from GitHub, for the actor to apply.
pub(crate) enum Fetched {
    CommentSync(Box<FetchedSync>),
    /// The viewer's pending review. `changed` when the effect wrote to it
    /// before reading it back; a plain read publishes nothing.
    PendingReview {
        context: TabContext,
        pr: PrRef,
        review: er_github::PendingReview,
        changed: bool,
    },
    ReviewSubmitted {
        context: TabContext,
        pr: PrRef,
    },
    /// A PR-level comment, or a bare decision when `decision`, was posted.
    Posted {
        context: TabContext,
        pr: PrRef,
        decision: bool,
    },
}

/// One sync's fetched data and everything its write needs.
pub(crate) struct FetchedSync {
    context: TabContext,
    sync: CommentSyncContext,
    fetched: FetchedCommentSync,
    /// Resolved by the effect, so the actor's write needs no environment.
    store: ArtifactStore,
}

/// The reply to send, the event to publish when the workspace changed, and
/// what changed in the tabs' content.
pub(crate) struct Applied {
    pub(crate) reply: Reply,
    pub(crate) event: Option<Event>,
    pub(crate) patch: Option<ContentPatch>,
}

/// Applies a fetch on the actor thread. A sync writes the sidecar here.
pub(crate) fn apply(fetched: Fetched) -> Result<Applied, WorkspaceError> {
    match fetched {
        Fetched::CommentSync(sync) => {
            let FetchedSync {
                context,
                sync,
                fetched,
                store,
            } = *sync;
            let result = apply_comment_sync(&store, &sync, fetched)
                .map_err(|error| failed("write the pulled comments", error))?;
            let patch = ContentPatch::stored_sidecar(
                &context.comments_dir,
                WrittenPayload::Comments(result.gc),
            );
            Ok(Applied {
                reply: Reply::CommentsSynced {
                    github_count: result.github_count,
                    local_count: result.local_count,
                },
                event: Some(Event::CommentsSynced {
                    context,
                    github_count: result.github_count,
                    local_count: result.local_count,
                }),
                patch: Some(patch),
            })
        }
        Fetched::PendingReview {
            context,
            pr,
            review,
            changed,
        } => Ok(Applied {
            reply: Reply::PendingReview { pr, review },
            event: changed.then_some(Event::PendingReviewChanged { context }),
            patch: None,
        }),
        Fetched::ReviewSubmitted { context, pr } => Ok(Applied {
            reply: Reply::ReviewSubmitted { pr },
            event: Some(Event::PendingReviewChanged { context }),
            patch: None,
        }),
        Fetched::Posted {
            context,
            pr,
            decision,
        } => Ok(Applied {
            reply: if decision {
                Reply::ReviewSubmitted { pr }
            } else {
                Reply::CommentPosted { pr }
            },
            event: Some(Event::CommentPosted { context }),
            patch: None,
        }),
    }
}

pub(crate) fn post_context(ctx: &TabContext) -> CommentPostContext {
    CommentPostContext {
        repo_root: ctx.repo_root.clone(),
        is_remote: ctx.is_remote,
        remote_repo: ctx.remote_repo.clone(),
        pr_number: ctx.pr_number,
    }
}

/// The tab's PR, resolved through the ports the way a comment create
/// resolves it.
pub(crate) fn pr_target(deps: &WorkspaceDeps, ctx: &TabContext) -> Result<PrRef, WorkspaceError> {
    let ports = deps.ports();
    let pr = resolve_post_target_with(&*ports.github, &*ports.git, &post_context(ctx)).map_err(
        |error| match error {
            GitHubPostError::NotPosted(message) => WorkspaceError::Failed(message),
            other => WorkspaceError::Failed(other.to_string()),
        },
    )?;
    Ok(PrRef {
        owner: pr.owner,
        repo: pr.repo,
        number: pr.pr,
    })
}

/// Reads the tab's PR comments and overview. The diff the comments anchor to
/// is read here through the ports, so the command carries only the tab.
pub(crate) fn fetch_sync(
    deps: &WorkspaceDeps,
    context: TabContext,
    source: SyncSource,
) -> Result<Fetched, WorkspaceError> {
    let pr = pr_target(deps, &context)?;
    let raw = fetch_raw(deps, &context)?;
    let anchor_hash = ai::compute_diff_hash(&raw);
    // The fetched diff is the branch diff on a branch tab; a staged or
    // unstaged tab keeps the branch hash it was given.
    let diff_hash = if context.scope == "branch" || context.branch_diff_hash.is_empty() {
        anchor_hash.clone()
    } else {
        context.branch_diff_hash.clone()
    };
    let store = store_registry::store();
    let scope = Scope::dir(&context.comments_dir)
        .map_err(|error| failed("open the comments bucket", error))?;
    let comments_path = ai::annotation_store::path_of::<ai::ErGithubComments>(&store, &scope)
        .map_err(|error| failed("resolve the comments file", error))?
        .to_string_lossy()
        .into_owned();
    let github = deps.ports().github.clone();
    let sync = CommentSyncContext {
        owner: pr.owner,
        repo_name: pr.repo,
        pr_number: pr.number,
        is_remote: context.is_remote,
        repo_root: context.repo_root.clone(),
        comments_path,
        diff_hash,
        anchor_hash,
        files: parse_diff(&raw),
        pr_number_for_overview: context.pr_number,
        github: github.clone(),
    };
    let use_cache = match source {
        SyncSource::Manual { force } => {
            if force {
                github.invalidate_comments();
            }
            true
        }
        SyncSource::Background => false,
    };
    let fetched = fetch_comment_sync(&sync, use_cache)?;
    if source == SyncSource::Background {
        // This read supersedes any cached bundle, so a manual pull within the
        // bundle's lifetime cannot go back to an older one.
        github.invalidate_comments();
    }
    Ok(Fetched::CommentSync(Box::new(FetchedSync {
        context,
        sync,
        fetched,
        store,
    })))
}
