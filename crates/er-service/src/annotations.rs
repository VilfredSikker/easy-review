//! The annotation use cases, run as actor effects.
//!
//! Each effect fetches the diff it needs through the ports, resolves the
//! anchor with the shared writer, and writes the store. The actor applies
//! nothing here: the sidecars are the state, and the reply tells the caller
//! which row landed.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};

use er_engine::ai::{self, CommentType};
use er_engine::annotations::{self, AnnotationInput, LineAnchor, ReplyAttribution, WrittenPayload};
use er_engine::diff_fetch::{fetch_branch_scope_raw, BranchScopeFetchInputs};
use er_engine::git::{parse_diff, parse_diff_headers, DiffFile};
use er_engine::session_store;
use er_engine::store_registry;
use er_store::{ArtifactStore, Loaded, Scope};

use crate::actor::WorkspaceDeps;
use crate::content::ContentPatch;
use crate::handle::WorkspaceError;
use crate::protocol::{AnnotationKind, AnnotationTarget, TabContext};

/// A write's answer and what it stored, for the actor to patch the tabs that
/// read the same bucket.
pub(crate) type Written<T> = Result<(T, ContentPatch), WorkspaceError>;

fn sidecar(dir: &str, payload: WrittenPayload) -> ContentPatch {
    ContentPatch::stored_sidecar(dir, payload)
}

/// The last sequence a reviewed write took, process-wide.
static MARKS_SEQ: AtomicU64 = AtomicU64::new(0);

/// `session_store::update_reviewed`, with a sequence taken while the bucket's
/// lock is held, so the sequences follow the order the writes saved in.
pub(crate) fn update_marks(
    store: &ArtifactStore,
    scope: &Scope,
    change: impl FnOnce(&mut HashMap<String, String>) -> usize,
) -> Result<Marks, WorkspaceError> {
    let mut seq = 0;
    let (changed, map) = session_store::update_reviewed(store, scope, |map| {
        seq = MARKS_SEQ.fetch_add(1, Ordering::SeqCst) + 1;
        change(map)
    })
    .map_err(|error| failed("update the reviewed marks", error))?;
    Ok(Marks { changed, map, seq })
}

/// What a reviewed write saved.
pub(crate) struct Marks {
    pub changed: usize,
    pub map: HashMap<String, String>,
    pub seq: u64,
}

/// The saved marks when a write changed them; nothing otherwise.
pub(crate) fn marks_patch(ctx: &TabContext, marks: Marks) -> Written<usize> {
    let patch = if marks.changed > 0 {
        ContentPatch::Reviewed {
            er_dir: ctx.er_dir.clone(),
            marks: marks.map,
            seq: marks.seq,
        }
    } else {
        ContentPatch::Several(Vec::new())
    };
    Ok((marks.changed, patch))
}

pub(crate) fn failed(context: &str, error: impl std::fmt::Display) -> WorkspaceError {
    WorkspaceError::Failed(format!("{context}: {error:#}"))
}

pub(crate) fn bucket_scope(dir: &str) -> Result<Scope, WorkspaceError> {
    Scope::dir(dir).map_err(|error| failed("open the bucket", error))
}

/// Fetch the tab's raw diff through the ports. Runs on an effect thread, never
/// on the actor.
///
/// The engine helper covers branch-view and remote tabs only. A plain repo tab,
/// the one `er` opens by default, has neither and takes the same `diff_raw`
/// fallthrough as `TabState::fetch_tab_raw_diff`.
pub(crate) fn fetch_raw(deps: &WorkspaceDeps, ctx: &TabContext) -> Result<String, WorkspaceError> {
    fetch_raw_with(deps.ports(), ctx)
}

/// [`fetch_raw`] over the ports alone, for a reader off the actor.
pub(crate) fn fetch_raw_with(
    ports: &er_engine::ports::Ports,
    ctx: &TabContext,
) -> Result<String, WorkspaceError> {
    if ctx.local_branch_view.is_none() && ctx.remote_repo.is_none() {
        return ports
            .git
            .diff_raw(
                &ctx.scope,
                &ctx.base_branch,
                &ctx.repo_root,
                ctx.pr_head_ref.as_deref(),
            )
            .map_err(|error| failed("fetch the diff", error));
    }
    let inputs = BranchScopeFetchInputs {
        ports: ports.clone(),
        repo_root: ctx.repo_root.clone(),
        base_branch: ctx.base_branch.clone(),
        local_branch_view: ctx.local_branch_view.clone(),
        pr_head_ref: ctx.pr_head_ref.clone(),
        pr_number: ctx.pr_number,
        checkout_root: ctx.checkout_root.clone(),
        remote_repo: ctx.remote_repo.clone(),
    };
    fetch_branch_scope_raw(&ctx.scope, &inputs).map_err(|error| failed("fetch the diff", error))
}

fn kind_to_comment_type(kind: AnnotationKind) -> CommentType {
    match kind {
        AnnotationKind::Question => CommentType::Question,
        AnnotationKind::Note => CommentType::Note,
    }
}

/// The branch diff hash a question or note is stored against. The tab usually
/// has one; a tab that never loaded branch scope hashes the diff this effect
/// fetched instead of running a second git call.
fn branch_diff_hash(ctx: &TabContext, raw: &str) -> String {
    if !ctx.branch_diff_hash.is_empty() {
        return ctx.branch_diff_hash.clone();
    }
    ai::compute_diff_hash(raw)
}

/// The parent thread's document anchor, when a reply hangs off one.
fn parent_document_anchor(
    store: &er_store::ArtifactStore,
    ctx: &TabContext,
    reply_to: Option<&str>,
) -> Option<LineAnchor> {
    let id = reply_to?;
    let scope = Scope::dir(&ctx.er_dir).ok()?;
    let questions = match ai::annotation_store::load_questions(store, &scope).ok()? {
        Loaded::Found(stored) => stored.value.questions,
        _ => Vec::new(),
    };
    let notes = match ai::annotation_store::load_notes(store, &scope).ok()? {
        Loaded::Found(stored) => stored.value.notes,
        _ => Vec::new(),
    };
    questions
        .iter()
        .chain(notes.iter())
        .find(|annotation| annotation.id == id && annotation.is_document_anchor())
        .map(|annotation| LineAnchor {
            line_start: annotation.line_start,
            line_content: annotation.line_content.clone(),
            context_before: annotation.context_before.clone(),
            context_after: annotation.context_after.clone(),
            old_line_start: None,
            hunk_header: String::new(),
        })
}

#[expect(
    clippy::too_many_arguments,
    reason = "the parameters are the command's explicit fields"
)]
fn add_private(
    deps: &WorkspaceDeps,
    ctx: &TabContext,
    kind: CommentType,
    target: &AnnotationTarget,
    body: &str,
    side: Option<&str>,
    client_id: Option<String>,
    reply_to: Option<String>,
    finding_ref: Option<String>,
    document_anchor: Option<LineAnchor>,
    attribution: ReplyAttribution,
) -> Written<String> {
    if body.trim().is_empty() {
        return Err(WorkspaceError::Failed(
            "The annotation body is empty".to_string(),
        ));
    }
    let raw = fetch_raw(deps, ctx)?;
    let files = parse_diff(&raw);
    let file_data: Option<&DiffFile> = files.iter().find(|file| file.path == target.file);
    let store = store_registry::store();
    let hash = branch_diff_hash(ctx, &raw);
    let parent_anchor = parent_document_anchor(&store, ctx, reply_to.as_deref());
    let input = AnnotationInput {
        kind,
        file: &target.file,
        hunk_idx: target.hunk_idx,
        line_num: target.line_num,
        line_num_end: target.line_num_end,
        text: body,
        side,
        reply_to,
        finding_ref,
        client_id,
        attribution,
        branch_diff_hash: &hash,
        diff_hash: &ctx.diff_hash,
        file_data,
        document_anchor,
        parent_document_anchor: parent_anchor,
        er_dir: &ctx.er_dir,
        comments_dir: &ctx.comments_dir,
    };
    let written = annotations::write_annotation(&store, &input)
        .map_err(|error| failed("write the annotation", error))?;
    let dir = match written.payload {
        WrittenPayload::Comments(_) => &ctx.comments_dir,
        WrittenPayload::Questions(_) | WrittenPayload::Notes(_) => &ctx.er_dir,
    };
    Ok((written.id, sidecar(dir, written.payload)))
}

#[expect(
    clippy::too_many_arguments,
    reason = "the parameters are the command's explicit fields"
)]
pub(crate) fn add_question(
    deps: &WorkspaceDeps,
    ctx: &TabContext,
    target: &AnnotationTarget,
    body: &str,
    side: Option<&str>,
    client_id: Option<String>,
    reply_to: Option<String>,
    finding_ref: Option<String>,
) -> Written<String> {
    add_private(
        deps,
        ctx,
        CommentType::Question,
        target,
        body,
        side,
        client_id,
        reply_to,
        finding_ref,
        None,
        ReplyAttribution::default(),
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "the parameters are the command's explicit fields"
)]
pub(crate) fn add_note(
    deps: &WorkspaceDeps,
    ctx: &TabContext,
    target: &AnnotationTarget,
    body: &str,
    side: Option<&str>,
    client_id: Option<String>,
    reply_to: Option<String>,
    finding_ref: Option<String>,
) -> Written<String> {
    add_private(
        deps,
        ctx,
        CommentType::Note,
        target,
        body,
        side,
        client_id,
        reply_to,
        finding_ref,
        None,
        ReplyAttribution::default(),
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "the parameters are the command's explicit fields"
)]
pub(crate) fn add_document_thread(
    deps: &WorkspaceDeps,
    ctx: &TabContext,
    file: &str,
    line: usize,
    line_end: Option<usize>,
    kind: AnnotationKind,
    body: &str,
    source: &str,
) -> Written<String> {
    let anchor = annotations::document_line_anchor(source, line)
        .ok_or_else(|| WorkspaceError::Failed(format!("Line {line} is past the end of {file}")))?;
    let target = AnnotationTarget {
        file: file.to_string(),
        hunk_idx: 0,
        line_num: Some(line),
        line_num_end: line_end,
    };
    add_private(
        deps,
        ctx,
        kind_to_comment_type(kind),
        &target,
        body,
        None,
        None,
        None,
        None,
        Some(anchor),
        ReplyAttribution::default(),
    )
}

/// The placement and kind of a thread a reply targets.
fn thread_placement(
    state: &ai::AiState,
    parent_id: &str,
) -> Result<(AnnotationTarget, CommentType), WorkspaceError> {
    let missing = || WorkspaceError::Failed(format!("Thread not found: {parent_id}"));
    if parent_id.starts_with("q-") {
        let question = state
            .questions
            .as_ref()
            .and_then(|questions| questions.questions.iter().find(|q| q.id == parent_id))
            .ok_or_else(missing)?;
        Ok((
            AnnotationTarget {
                file: question.file.clone(),
                hunk_idx: question.hunk_index.unwrap_or(0),
                line_num: question.line_start,
                line_num_end: None,
            },
            CommentType::Question,
        ))
    } else if parent_id.starts_with("n-") {
        let note = state
            .notes
            .as_ref()
            .and_then(|notes| notes.notes.iter().find(|n| n.id == parent_id))
            .ok_or_else(missing)?;
        Ok((
            AnnotationTarget {
                file: note.file.clone(),
                hunk_idx: note.hunk_index.unwrap_or(0),
                line_num: note.line_start,
                line_num_end: None,
            },
            CommentType::Note,
        ))
    } else {
        let comment = state
            .github_comments
            .as_ref()
            .and_then(|comments| comments.comments.iter().find(|c| c.id == parent_id))
            .ok_or_else(missing)?;
        if comment.github_id.is_some() {
            return Err(WorkspaceError::Failed(
                "This comment is on GitHub; the GitHub reply path is not on the service yet".into(),
            ));
        }
        Ok((
            AnnotationTarget {
                file: comment.file.clone(),
                hunk_idx: comment.hunk_index.unwrap_or(0),
                line_num: comment.line_start,
                line_num_end: None,
            },
            CommentType::GitHubComment,
        ))
    }
}

pub(crate) fn reply_to_thread(
    deps: &WorkspaceDeps,
    ctx: &TabContext,
    parent_id: &str,
    body: &str,
    client_id: Option<String>,
) -> Written<String> {
    let state = ai::load_ai_state(&ctx.er_dir, &ctx.diff_hash, None);
    let (target, kind) = thread_placement(&state, parent_id)?;
    add_private(
        deps,
        ctx,
        kind,
        &target,
        body,
        None,
        client_id,
        Some(parent_id.to_string()),
        None,
        None,
        ReplyAttribution::default(),
    )
}

pub(crate) fn update_thread_message(ctx: &TabContext, id: &str, body: &str) -> Written<String> {
    let text = body.trim().to_string();
    if text.is_empty() {
        return Err(WorkspaceError::Failed("Message cannot be empty".into()));
    }
    let store = store_registry::store();
    let scope = bucket_scope(&ctx.er_dir)?;
    let id_owned = id.to_string();
    if id.starts_with("q-") {
        let updated = ai::annotation_store::update_questions(&store, &scope, move |current| {
            let mut questions =
                current.ok_or_else(|| anyhow::anyhow!("the questions file is missing"))?;
            let row = questions
                .questions
                .iter_mut()
                .find(|q| q.id == id_owned)
                .ok_or_else(|| anyhow::anyhow!("Thread message not found"))?;
            if row.author == "ai" {
                anyhow::bail!("Cannot edit AI-generated text");
            }
            if !row.author.is_empty() && row.author != "You" {
                anyhow::bail!("Can only edit your own messages");
            }
            row.text = text;
            Ok::<_, anyhow::Error>(questions)
        })
        .map_err(|error| failed("update the question", error))?;
        let path = ai::annotation_store::path_of::<ai::ErQuestions>(&store, &scope)
            .map_err(|error| failed("resolve the questions path", error))?;
        Ok((
            path.to_string_lossy().into_owned(),
            sidecar(&ctx.er_dir, WrittenPayload::Questions(updated.value)),
        ))
    } else if id.starts_with("n-") {
        let updated = ai::annotation_store::update_notes(&store, &scope, move |current| {
            let mut notes = current.ok_or_else(|| anyhow::anyhow!("the notes file is missing"))?;
            let row = notes
                .notes
                .iter_mut()
                .find(|n| n.id == id_owned)
                .ok_or_else(|| anyhow::anyhow!("Thread message not found"))?;
            if row.author == "ai" {
                anyhow::bail!("Cannot edit AI-generated text");
            }
            if !row.author.is_empty() && row.author != "You" {
                anyhow::bail!("Can only edit your own messages");
            }
            row.text = text;
            Ok::<_, anyhow::Error>(notes)
        })
        .map_err(|error| failed("update the note", error))?;
        let path = ai::annotation_store::path_of::<ai::ErNotes>(&store, &scope)
            .map_err(|error| failed("resolve the notes path", error))?;
        Ok((
            path.to_string_lossy().into_owned(),
            sidecar(&ctx.er_dir, WrittenPayload::Notes(updated.value)),
        ))
    } else {
        let scope = bucket_scope(&ctx.comments_dir)?;
        let updated =
            ai::annotation_store::update_github_comments(&store, &scope, move |current| {
                let mut comments =
                    current.ok_or_else(|| anyhow::anyhow!("the comments file is missing"))?;
                let row = comments
                    .comments
                    .iter_mut()
                    .find(|c| c.id == id_owned)
                    .ok_or_else(|| anyhow::anyhow!("Thread message not found"))?;
                if row.author == "ai" {
                    anyhow::bail!("Cannot edit AI-generated text");
                }
                if !row.author.is_empty() && row.author != "You" {
                    anyhow::bail!("Can only edit your own messages");
                }
                row.comment = text;
                Ok::<_, anyhow::Error>(comments)
            })
            .map_err(|error| failed("update the comment", error))?;
        let path = ai::annotation_store::path_of::<ai::ErGithubComments>(&store, &scope)
            .map_err(|error| failed("resolve the comments path", error))?;
        Ok((
            path.to_string_lossy().into_owned(),
            sidecar(&ctx.comments_dir, WrittenPayload::Comments(updated.value)),
        ))
    }
}

pub(crate) fn delete_thread(ctx: &TabContext, id: &str) -> Written<String> {
    let store = store_registry::store();
    let id_owned = id.to_string();
    if id.starts_with("q-") {
        let scope = bucket_scope(&ctx.er_dir)?;
        let updated = ai::annotation_store::update_questions(&store, &scope, move |current| {
            let mut questions =
                current.ok_or_else(|| anyhow::anyhow!("the questions file is missing"))?;
            questions.questions.retain(|q| {
                q.id != id_owned && q.in_reply_to.as_deref() != Some(id_owned.as_str())
            });
            Ok::<_, anyhow::Error>(questions)
        })
        .map_err(|error| failed("delete the question", error))?;
        let path = ai::annotation_store::path_of::<ai::ErQuestions>(&store, &scope)
            .map_err(|error| failed("resolve the questions path", error))?;
        Ok((
            path.to_string_lossy().into_owned(),
            sidecar(&ctx.er_dir, WrittenPayload::Questions(updated.value)),
        ))
    } else if id.starts_with("n-") {
        let scope = bucket_scope(&ctx.er_dir)?;
        let updated = ai::annotation_store::update_notes(&store, &scope, move |current| {
            let mut notes = current.ok_or_else(|| anyhow::anyhow!("the notes file is missing"))?;
            notes.notes.retain(|n| {
                n.id != id_owned && n.in_reply_to.as_deref() != Some(id_owned.as_str())
            });
            Ok::<_, anyhow::Error>(notes)
        })
        .map_err(|error| failed("delete the note", error))?;
        let path = ai::annotation_store::path_of::<ai::ErNotes>(&store, &scope)
            .map_err(|error| failed("resolve the notes path", error))?;
        Ok((
            path.to_string_lossy().into_owned(),
            sidecar(&ctx.er_dir, WrittenPayload::Notes(updated.value)),
        ))
    } else {
        let scope = bucket_scope(&ctx.comments_dir)?;
        let updated =
            ai::annotation_store::update_github_comments(&store, &scope, move |current| {
                let mut comments =
                    current.ok_or_else(|| anyhow::anyhow!("the comments file is missing"))?;
                comments.comments.retain(|c| {
                    c.id != id_owned && c.in_reply_to.as_deref() != Some(id_owned.as_str())
                });
                Ok::<_, anyhow::Error>(comments)
            })
            .map_err(|error| failed("delete the comment", error))?;
        let path = ai::annotation_store::path_of::<ai::ErGithubComments>(&store, &scope)
            .map_err(|error| failed("resolve the comments path", error))?;
        Ok((
            path.to_string_lossy().into_owned(),
            sidecar(&ctx.comments_dir, WrittenPayload::Comments(updated.value)),
        ))
    }
}

pub(crate) fn resolve_thread(ctx: &TabContext, id: &str) -> Written<String> {
    let store = store_registry::store();
    let id_owned = id.to_string();
    let q_scope = bucket_scope(&ctx.er_dir)?;
    if let Ok(Loaded::Found(stored)) = ai::annotation_store::load_questions(&store, &q_scope) {
        if stored.value.questions.iter().any(|q| q.id == id) {
            let mut patch = ContentPatch::Several(Vec::new());
            if stored
                .value
                .questions
                .iter()
                .any(|q| q.id == id && !q.resolved)
            {
                let updated =
                    ai::annotation_store::update_questions(&store, &q_scope, move |current| {
                        let mut questions = current
                            .ok_or_else(|| anyhow::anyhow!("the questions file vanished"))?;
                        if let Some(row) = questions.questions.iter_mut().find(|q| q.id == id_owned)
                        {
                            row.resolved = true;
                        }
                        Ok::<_, anyhow::Error>(questions)
                    })
                    .map_err(|error| failed("resolve the question", error))?;
                patch = sidecar(&ctx.er_dir, WrittenPayload::Questions(updated.value));
            }
            let path = ai::annotation_store::path_of::<ai::ErQuestions>(&store, &q_scope)
                .map_err(|error| failed("resolve the questions path", error))?;
            return Ok((path.to_string_lossy().into_owned(), patch));
        }
    }
    if let Ok(Loaded::Found(stored)) = ai::annotation_store::load_notes(&store, &q_scope) {
        if stored.value.notes.iter().any(|n| n.id == id) {
            let mut patch = ContentPatch::Several(Vec::new());
            if stored.value.notes.iter().any(|n| n.id == id && !n.resolved) {
                let updated =
                    ai::annotation_store::update_notes(&store, &q_scope, move |current| {
                        let mut notes =
                            current.ok_or_else(|| anyhow::anyhow!("the notes file vanished"))?;
                        if let Some(row) = notes.notes.iter_mut().find(|n| n.id == id_owned) {
                            row.resolved = true;
                        }
                        Ok::<_, anyhow::Error>(notes)
                    })
                    .map_err(|error| failed("resolve the note", error))?;
                patch = sidecar(&ctx.er_dir, WrittenPayload::Notes(updated.value));
            }
            let path = ai::annotation_store::path_of::<ai::ErNotes>(&store, &q_scope)
                .map_err(|error| failed("resolve the notes path", error))?;
            return Ok((path.to_string_lossy().into_owned(), patch));
        }
    }
    let gc_scope = bucket_scope(&ctx.comments_dir)?;
    if let Ok(Loaded::Found(stored)) = ai::annotation_store::load_github_comments(&store, &gc_scope)
    {
        if stored.value.comments.iter().any(|c| c.id == id) {
            let mut patch = ContentPatch::Several(Vec::new());
            if stored
                .value
                .comments
                .iter()
                .any(|c| c.id == id && !c.resolved)
            {
                let updated = ai::annotation_store::update_github_comments(
                    &store,
                    &gc_scope,
                    move |current| {
                        let mut comments =
                            current.ok_or_else(|| anyhow::anyhow!("the comments file vanished"))?;
                        if let Some(row) = comments.comments.iter_mut().find(|c| c.id == id_owned) {
                            row.resolved = true;
                        }
                        Ok::<_, anyhow::Error>(comments)
                    },
                )
                .map_err(|error| failed("resolve the comment", error))?;
                patch = sidecar(&ctx.comments_dir, WrittenPayload::Comments(updated.value));
            }
            let path = ai::annotation_store::path_of::<ai::ErGithubComments>(&store, &gc_scope)
                .map_err(|error| failed("resolve the comments path", error))?;
            return Ok((path.to_string_lossy().into_owned(), patch));
        }
    }
    Err(WorkspaceError::Failed(format!("Thread not found: {id}")))
}

pub(crate) fn promote_to_note(
    deps: &WorkspaceDeps,
    ctx: &TabContext,
    question_id: &str,
    body: Option<String>,
) -> Written<String> {
    let store = store_registry::store();
    let q_scope = bucket_scope(&ctx.er_dir)?;
    let questions = match ai::annotation_store::load_questions(&store, &q_scope) {
        Ok(Loaded::Found(stored)) => stored.value,
        Ok(_) => return Err(WorkspaceError::Failed("No questions loaded".into())),
        Err(error) => return Err(failed("load the questions", error)),
    };
    let source = questions
        .questions
        .iter()
        .find(|question| question.id == question_id)
        .cloned()
        .ok_or_else(|| WorkspaceError::Failed(format!("Question not found: {question_id}")))?;
    let thread = questions.questions.clone();
    let file = if source.file.is_empty() {
        String::new()
    } else {
        source.file.clone()
    };
    let anchor = if source.is_document_anchor() {
        ai::promote::PromotionAnchor {
            line_content: source.line_content.clone(),
            context_before: source.context_before.clone(),
            context_after: source.context_after.clone(),
            old_line_start: source.old_line_start,
            hunk_header: source.hunk_header.clone(),
        }
    } else {
        let raw = fetch_raw(deps, ctx)?;
        let files = parse_diff(&raw);
        let line_anchor = annotations::line_anchor_at(
            files.iter().find(|diff_file| diff_file.path == file),
            source.hunk_index.unwrap_or(0),
            source.line_start,
            Some(source.side.as_str()),
        );
        ai::promote::PromotionAnchor {
            line_content: line_anchor.line_content,
            context_before: line_anchor.context_before,
            context_after: line_anchor.context_after,
            old_line_start: line_anchor.old_line_start,
            hunk_header: line_anchor.hunk_header,
        }
    };
    let note =
        ai::promote::promote_to_note(&source, &thread, body, file, anchor, ctx.diff_hash.clone());
    let note_id = note.id.clone();
    let branch_hash = ctx.branch_diff_hash.clone();
    let notes = ai::annotation_store::update_notes(&store, &q_scope, move |current| {
        let mut notes = current.unwrap_or_else(|| ai::ErNotes {
            version: 1,
            diff_hash: branch_hash.clone(),
            notes: Vec::new(),
        });
        notes.diff_hash = branch_hash;
        notes.notes.push(note);
        Ok::<_, anyhow::Error>(notes)
    })
    .map_err(|error| failed("write the note", error))?;
    let source_id = question_id.to_string();
    let questions = ai::annotation_store::update_questions(&store, &q_scope, move |current| {
        let mut questions =
            current.ok_or_else(|| anyhow::anyhow!("the questions file is missing"))?;
        questions
            .questions
            .retain(|q| q.id != source_id && q.in_reply_to.as_deref() != Some(source_id.as_str()));
        Ok::<_, anyhow::Error>(questions)
    })
    .map_err(|error| failed("remove the promoted question", error))?;
    Ok((
        note_id,
        ContentPatch::Several(vec![
            sidecar(&ctx.er_dir, WrittenPayload::Notes(notes.value)),
            sidecar(&ctx.er_dir, WrittenPayload::Questions(questions.value)),
        ]),
    ))
}

pub(crate) fn reply_to_finding(
    deps: &WorkspaceDeps,
    ctx: &TabContext,
    finding_id: &str,
    body: &str,
    ai_assist: bool,
    client_id: Option<String>,
) -> Written<(String, String)> {
    let state = finding_thread_state(ctx);
    let mut placement: Option<(String, usize, Option<usize>)> = None;
    if let Some(review) = state.review.as_ref() {
        'outer: for (path, file) in review.files.iter() {
            for finding in file.findings.iter() {
                if finding.id == finding_id {
                    placement = Some((
                        path.clone(),
                        finding.hunk_index.unwrap_or(0),
                        finding.line_start,
                    ));
                    break 'outer;
                }
            }
        }
    }
    let (file, hunk_idx, line_start) = placement
        .ok_or_else(|| WorkspaceError::Failed(format!("Finding not found: {finding_id}")))?;
    let target = AnnotationTarget {
        file: file.clone(),
        hunk_idx,
        line_num: line_start,
        line_num_end: None,
    };
    if ai_assist {
        return ai_follow_up_thread(deps, ctx, &state, &target, finding_id, client_id);
    }
    if let Some(root_id) = ai::find_finding_thread_root(&state, finding_id) {
        let (reply_id, patch) = add_private(
            deps,
            ctx,
            CommentType::GitHubComment,
            &target,
            body,
            None,
            client_id,
            Some(root_id.clone()),
            None,
            None,
            ReplyAttribution::default(),
        )?;
        return Ok(((root_id, reply_id), patch));
    }
    let (root_id, root_patch) = add_private(
        deps,
        ctx,
        CommentType::GitHubComment,
        &target,
        "Follow-up on this finding.",
        None,
        client_id,
        None,
        Some(finding_id.to_string()),
        None,
        ReplyAttribution::default(),
    )?;
    let (reply_id, reply_patch) = add_private(
        deps,
        ctx,
        CommentType::GitHubComment,
        &target,
        body,
        None,
        None,
        Some(root_id.clone()),
        None,
        None,
        ReplyAttribution::default(),
    )?;
    Ok((
        (root_id, reply_id),
        ContentPatch::Several(vec![root_patch, reply_patch]),
    ))
}

/// The tab's AI state with the GitHub comments read from `comments_dir`,
/// where [`add_private`] writes a finding's thread root. A tab whose PR
/// bucket differs from its private one (a local PR tab in Branch view) would
/// otherwise miss the root and write another one on every reply.
fn finding_thread_state(ctx: &TabContext) -> ai::AiState {
    let mut state = ai::load_ai_state(&ctx.er_dir, &ctx.diff_hash, None);
    if ctx.comments_dir != ctx.er_dir {
        state.github_comments = Scope::dir(&ctx.comments_dir).ok().and_then(|scope| {
            match ai::annotation_store::load_github_comments(&store_registry::store(), &scope) {
                Ok(Loaded::Found(stored)) => Some(stored.value),
                _ => None,
            }
        });
    }
    state
}

/// The thread an AI-assisted finding reply asks in: the finding's own, or a
/// new one whose root says a follow-up was asked for. It writes no reply: the
/// answer is the ask's, which lands later through [`submit_ai_reply`].
fn ai_follow_up_thread(
    deps: &WorkspaceDeps,
    ctx: &TabContext,
    state: &ai::AiState,
    target: &AnnotationTarget,
    finding_id: &str,
    client_id: Option<String>,
) -> Written<(String, String)> {
    if let Some(root_id) = ai::find_finding_thread_root(state, finding_id) {
        return Ok(((root_id, String::new()), ContentPatch::Several(Vec::new())));
    }
    let (root_id, patch) = add_private(
        deps,
        ctx,
        CommentType::GitHubComment,
        target,
        "AI follow-up requested for this finding.",
        None,
        client_id,
        None,
        Some(finding_id.to_string()),
        None,
        ReplyAttribution::default(),
    )?;
    Ok(((root_id, String::new()), patch))
}

/// Writes an ask's answer into the thread it asked about, in the buckets of
/// `ctx`, the context the ask captured when it started, signed by the AI as
/// the App signs it.
pub(crate) fn submit_ai_reply(
    deps: &WorkspaceDeps,
    ctx: &TabContext,
    target: &AnnotationTarget,
    body: &str,
    kind: CommentType,
    reply_to: String,
    ai_action: Option<String>,
) -> Written<String> {
    add_private(
        deps,
        ctx,
        kind,
        target,
        body,
        None,
        None,
        Some(reply_to),
        None,
        None,
        ReplyAttribution {
            author: Some("ai".to_string()),
            ai_action,
        },
    )
}

fn reviewed_paths(diff_paths: &HashSet<String>, pillar_paths: Vec<String>) -> Vec<String> {
    pillar_paths
        .into_iter()
        .filter(|path| diff_paths.contains(path))
        .collect()
}

pub(crate) fn bulk_review_pillar(
    deps: &WorkspaceDeps,
    ctx: &TabContext,
    tour: Option<&ai::ErTour>,
    pillar_id: &str,
    reviewed: bool,
) -> Written<usize> {
    let Some(tour) = tour else {
        return Ok((0, ContentPatch::Several(Vec::new())));
    };
    let raw = fetch_raw(deps, ctx)?;
    let headers = parse_diff_headers(&raw);
    let diff_paths: HashSet<String> = headers.iter().map(|header| header.path.clone()).collect();
    let ownership = tour.pillar_ownership(|path| diff_paths.contains(path));
    let paths = if pillar_id == "__other__" {
        let assigned: HashSet<&str> = ownership
            .iter()
            .flat_map(|(_, paths)| paths.iter().map(String::as_str))
            .collect();
        headers
            .iter()
            .map(|header| header.path.clone())
            .filter(|path| !assigned.contains(path.as_str()))
            .collect()
    } else {
        ownership
            .into_iter()
            .find(|(id, _)| id == pillar_id)
            .map(|(_, paths)| paths)
            .unwrap_or_default()
    };
    let paths = reviewed_paths(&diff_paths, paths);
    let store = store_registry::store();
    let scope = bucket_scope(&ctx.er_dir)?;
    let marks = update_marks(&store, &scope, |map| {
        let mut changed = 0usize;
        for path in paths {
            let hash = ai::compute_per_file_hash(&raw, &path).unwrap_or_default();
            if annotations::apply_reviewed(map, &path, hash, reviewed) {
                changed += 1;
            }
        }
        changed
    })?;
    marks_patch(ctx, marks)
}

pub(crate) fn toggle_reviewed(
    deps: &WorkspaceDeps,
    ctx: &TabContext,
    path: &str,
) -> Written<usize> {
    let raw = fetch_raw(deps, ctx)?;
    let headers = parse_diff_headers(&raw);
    if !headers.iter().any(|header| header.path == path) {
        return Err(WorkspaceError::Failed(format!(
            "{path} is not in the active diff"
        )));
    }
    let store = store_registry::store();
    let scope = bucket_scope(&ctx.er_dir)?;
    let hash = ai::compute_per_file_hash(&raw, path).unwrap_or_default();
    let marks = update_marks(&store, &scope, |map| {
        let reviewed = !map.contains_key(path);
        usize::from(annotations::apply_reviewed(map, path, hash, reviewed))
    })?;
    marks_patch(ctx, marks)
}

pub(crate) fn mark_reviewed(
    deps: &WorkspaceDeps,
    ctx: &TabContext,
    path: &str,
    reviewed: bool,
) -> Written<usize> {
    let raw = fetch_raw(deps, ctx)?;
    let headers = parse_diff_headers(&raw);
    if !headers.iter().any(|header| header.path == path) {
        return Ok((0, ContentPatch::Several(Vec::new())));
    }
    let store = store_registry::store();
    let scope = bucket_scope(&ctx.er_dir)?;
    let hash = ai::compute_per_file_hash(&raw, path).unwrap_or_default();
    let marks = update_marks(&store, &scope, |map| {
        usize::from(annotations::apply_reviewed(map, path, hash, reviewed))
    })?;
    marks_patch(ctx, marks)
}
