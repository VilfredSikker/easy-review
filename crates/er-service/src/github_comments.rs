//! GitHub comment creates and promotes, run as actor effects.
//!
//! The create reuses the shared session (begin off-lock, execute through the
//! poster, finish) and the same sidecar writer the App uses. The promote
//! effects post first and remove the source only once GitHub holds the comment,
//! so a failure keeps the source and the next attempt adopts the post.

use er_engine::ai::{self, PrivateAnnotation};
use er_engine::git::parse_diff;
use er_engine::github_post::{
    execute_comment_post, new_client_id, resolve_post_target_with, CommentPostContext,
    GhCommentPoster, GitHubPostRequest,
};
use er_engine::github_post_session::{NewGitHubComment, PostBegin};
use er_engine::store_registry;
use er_store::{Loaded, Scope};

use crate::actor::WorkspaceDeps;
use crate::annotations::{failed, fetch_raw};
use crate::handle::WorkspaceError;
use crate::protocol::{AnnotationTarget, TabContext};

/// What one create left behind.
struct PostedComment {
    /// The stored row's id, or the client id when the comment has no row.
    id: String,
    /// The request, so a failed source removal can be reconciled on retry.
    request: GitHubPostRequest,
}

fn post_context(ctx: &TabContext) -> CommentPostContext {
    CommentPostContext {
        repo_root: ctx.repo_root.clone(),
        is_remote: ctx.is_remote,
        remote_repo: ctx.remote_repo.clone(),
        pr_number: ctx.pr_number,
    }
}

fn comments_payload(ctx: &TabContext) -> Option<ai::ErGithubComments> {
    let scope = Scope::dir(&ctx.comments_dir).ok()?;
    match ai::annotation_store::load_github_comments(&store_registry::store(), &scope) {
        Ok(Loaded::Found(stored)) => Some(stored.value),
        _ => None,
    }
}

fn post(
    deps: &WorkspaceDeps,
    ctx: &TabContext,
    intent: NewGitHubComment,
    client_id: Option<String>,
) -> Result<PostedComment, WorkspaceError> {
    let raw = fetch_raw(deps, ctx)?;
    let files = parse_diff(&raw);
    let diff_file = match &intent {
        NewGitHubComment::Line { file, .. } => files.iter().find(|f| f.path == *file),
        NewGitHubComment::Reply { .. } | NewGitHubComment::General { .. } => None,
    };
    let post_ctx = post_context(ctx);
    let pr = resolve_post_target_with(&*deps.ports().github, &*deps.ports().git, &post_ctx)
        .map_err(|error| WorkspaceError::Failed(error.to_string()))?;
    let client_id = client_id
        .filter(|id| !id.is_empty())
        .unwrap_or_else(new_client_id);
    let comments = comments_payload(ctx);
    let session = deps.post_session();
    let ticket = {
        let mut guard = session
            .lock()
            .map_err(|_| failed("lock the post session", "poisoned"))?;
        guard
            .begin(
                intent,
                PostBegin {
                    active: post_ctx.clone(),
                    ctx: post_ctx,
                    pr,
                    comments: comments.as_ref(),
                    diff_file,
                    comments_dir: &ctx.comments_dir,
                    branch_diff_hash: ctx.branch_diff_hash.clone(),
                    diff_hash: ctx.diff_hash.clone(),
                    client_id,
                },
            )
            .map_err(|error| WorkspaceError::Failed(error.to_string()))?
    };
    let request = ticket.request.clone();
    let poster = GhCommentPoster::new(deps.ports().github.clone());
    let execution = execute_comment_post(&poster, &ticket.request, ticket.pending.as_ref());
    let record = {
        let mut guard = session
            .lock()
            .map_err(|_| failed("lock the post session", "poisoned"))?;
        guard
            .finish(ticket, execution)
            .map_err(|error| WorkspaceError::Failed(error.to_string()))?
    };
    if record.posted.kept_earlier_body {
        return Err(WorkspaceError::Failed(
            "Your earlier version was posted; this edit was not".to_string(),
        ));
    }
    let id = record
        .posted
        .store_id
        .clone()
        .unwrap_or_else(|| request.client_id.clone());
    Ok(PostedComment { id, request })
}

pub(crate) fn add_comment(
    deps: &WorkspaceDeps,
    ctx: &TabContext,
    target: &AnnotationTarget,
    body: &str,
    side: Option<String>,
    client_id: Option<String>,
) -> Result<String, WorkspaceError> {
    let intent = NewGitHubComment::Line {
        file: target.file.clone(),
        hunk_idx: target.hunk_idx,
        line_start: target.line_num,
        line_end: target.line_num_end,
        side,
        body: body.to_string(),
    };
    post(deps, ctx, intent, client_id).map(|posted| posted.id)
}

pub(crate) fn reply_to_github_thread(
    deps: &WorkspaceDeps,
    ctx: &TabContext,
    parent_id: &str,
    body: &str,
    client_id: Option<String>,
) -> Result<String, WorkspaceError> {
    let intent = NewGitHubComment::Reply {
        parent_id: parent_id.to_string(),
        body: body.to_string(),
    };
    post(deps, ctx, intent, client_id).map(|posted| posted.id)
}

fn private_items<'a>(
    state: &'a ai::AiState,
    id: &str,
) -> Result<(&'a [PrivateAnnotation], &'a PrivateAnnotation), WorkspaceError> {
    if id.starts_with("n-") {
        let notes = state
            .notes
            .as_ref()
            .ok_or_else(|| WorkspaceError::Failed("No notes loaded".to_string()))?;
        let note = notes
            .notes
            .iter()
            .find(|note| note.id == id)
            .ok_or_else(|| WorkspaceError::Failed(format!("Note not found: {id}")))?;
        Ok((notes.notes.as_slice(), note))
    } else {
        let questions = state
            .questions
            .as_ref()
            .ok_or_else(|| WorkspaceError::Failed("No questions loaded".to_string()))?;
        let question = questions
            .questions
            .iter()
            .find(|question| question.id == id)
            .ok_or_else(|| WorkspaceError::Failed(format!("Question not found: {id}")))?;
        Ok((questions.questions.as_slice(), question))
    }
}

pub(crate) fn promote_to_comment(
    deps: &WorkspaceDeps,
    ctx: &TabContext,
    id: &str,
    body: Option<String>,
) -> Result<String, WorkspaceError> {
    let state = ai::load_ai_state(&ctx.er_dir, &ctx.diff_hash, None);
    let (items, item) = private_items(&state, id)?;
    let promoted = ai::promote::promote_to_github(item, items, body)
        .map_err(|error| WorkspaceError::Failed(error.to_string()))?;
    let posted = post(
        deps,
        ctx,
        NewGitHubComment::from(promoted),
        Some(format!("c-promote-{id}")),
    )?;
    // The source is replaced only once GitHub holds the comment, so a failure
    // keeps it and a retry adopts the post.
    if let Err(error) = crate::annotations::delete_thread(ctx, id) {
        remember_for_retry(deps, posted.request);
        return Err(error);
    }
    Ok(posted.id)
}

pub(crate) fn promote_finding_to_comment(
    deps: &WorkspaceDeps,
    ctx: &TabContext,
    finding_id: &str,
    body: Option<String>,
) -> Result<String, WorkspaceError> {
    let state = ai::load_ai_state(&ctx.er_dir, &ctx.diff_hash, None);
    let review = state
        .review
        .as_ref()
        .ok_or_else(|| WorkspaceError::Failed("No AI review loaded".to_string()))?;
    let (file, finding) = review
        .files
        .iter()
        .find_map(|(path, file)| {
            file.findings
                .iter()
                .find(|finding| finding.id == finding_id)
                .map(|finding| (path, finding))
        })
        .ok_or_else(|| WorkspaceError::Failed(format!("Finding not found: {finding_id}")))?;
    let replies = ai::collect_finding_promote_replies(&state, finding_id);
    let promoted = ai::promote::promote_finding(file, finding, &replies, body)
        .map_err(|error| WorkspaceError::Failed(error.to_string()))?;
    let posted = post(
        deps,
        ctx,
        NewGitHubComment::from(promoted),
        Some(format!("c-promote-finding-{finding_id}")),
    )?;
    // The finding is removed only once GitHub holds the comment, so a failure
    // keeps it and a retry adopts the post.
    if let Err(error) = ai::remove_finding_from_sidecars(&ctx.er_dir, finding_id) {
        remember_for_retry(deps, posted.request);
        return Err(WorkspaceError::Failed(format!(
            "Failed to remove finding after promote: {error}"
        )));
    }
    let _ = ai::delete_threads_linked_to_finding(&ctx.er_dir, &ctx.comments_dir, finding_id);
    let mut promotions = ai::load_finding_promotions(&ctx.er_dir);
    if promotions.remove(finding_id).is_some() {
        let _ = ai::save_finding_promotions(&ctx.er_dir, &promotions);
    }
    Ok(posted.id)
}

fn remember_for_retry(deps: &WorkspaceDeps, request: GitHubPostRequest) {
    if let Ok(mut session) = deps.post_session().lock() {
        session.remember_posted_for_retry(request);
    }
}
