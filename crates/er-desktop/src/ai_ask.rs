//! The card asks on the flag path: Ask AI, Validate and Elaborate on a
//! thread or a finding, and the AI-assisted finding reply. Each names a tab,
//! or takes the registry's active one, and builds its prompt from that tab's
//! published content by the rules the App's asks use (`ask_context`), with
//! the provider, model and effort of the settings actor's saved selection.
//! The reply being written shows in `jobs` until the answer lands, which
//! goes to the workspace as `SubmitAiReply` or `AppendFindingResponse` with
//! the context the ask captured: a tab that moved to another view, lost the
//! focus or closed meanwhile still gets the answer in the bucket it asked
//! from. Every function takes `DesktopState`, so no ask waits on the App.
//!
//! Only compiled with `v1-service`.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use er_engine::ai::{CommentType, AI_ACTION_ASK, AI_ACTION_ELABORATE};
use er_engine::app::{plan_card_ai_invocation, run_card_ai_subprocess, CardAiInvocation};
use er_engine::config::ErConfig;
use er_engine::git::DiffFile;
use er_engine::TabKey;
use er_protocol::{ErError, ErErrorKind, PaintedView};
use er_service::{AnnotationTarget, Command, Reply, TabContent, TabContext};

use crate::arena_service::ArenaTab;
use crate::ask_context::{self, AnswerTarget, AskContext, AskTab};
use crate::commands::{
    card_action_prompt, card_model, card_progress, content_routes, now_ms, DEFAULT_ASK_AI_PROMPT,
    VALIDATE_CARD_AI_PROMPT,
};
use crate::snapshot::PendingAiReply;
use crate::state::DesktopState;

/// Asks about a thread with `prompt`, or with the thread kind's own prompt
/// when it is empty.
pub fn ask(
    desktop: &DesktopState,
    wire: Option<er_protocol::TabKey>,
    thread_id: String,
    prompt: String,
) -> Result<(), ErError> {
    let prompt = if prompt.trim().is_empty() {
        card_action_prompt(&thread_id, AI_ACTION_ASK).map_or(prompt, str::to_string)
    } else {
        prompt
    };
    let tab = crate::arena_service::tab(desktop, wire)?;
    ask_thread(desktop, &tab, thread_id, prompt, Some(AI_ACTION_ASK))
}

/// Has the AI rewrite a question or note as a self-contained one.
pub fn elaborate(
    desktop: &DesktopState,
    wire: Option<er_protocol::TabKey>,
    thread_id: String,
) -> Result<(), ErError> {
    let prompt = card_action_prompt(&thread_id, AI_ACTION_ELABORATE)
        .ok_or_else(|| refused("Elaborate works on questions and notes only"))?;
    let tab = crate::arena_service::tab(desktop, wire)?;
    ask_thread(
        desktop,
        &tab,
        thread_id,
        prompt.to_string(),
        Some(AI_ACTION_ELABORATE),
    )
}

/// Has the AI check a finding, or the finding a thread was opened on, or
/// else the thread itself.
pub fn validate(
    desktop: &DesktopState,
    wire: Option<er_protocol::TabKey>,
    thread_id: Option<String>,
    finding_id: Option<String>,
) -> Result<(), ErError> {
    let tab = crate::arena_service::tab(desktop, wire)?;
    let finding = finding_id.or_else(|| {
        let thread = thread_id.as_deref()?;
        ask_context::finding_id_for_thread(&tab.content.ai.state, thread)
    });
    let prompt = VALIDATE_CARD_AI_PROMPT.to_string();
    if let Some(finding_id) = finding {
        return ask_finding(desktop, &tab, finding_id, prompt);
    }
    let thread_id = thread_id.ok_or_else(|| refused("thread_id or finding_id is required"))?;
    ask_thread(desktop, &tab, thread_id, prompt, None)
}

/// Asks about a finding in its thread: the thread is found or created
/// through the workspace, refused when the tab no longer shows `view`, and
/// the ask answers in it. Answers the thread's root id.
pub fn ask_on_finding(
    desktop: &DesktopState,
    wire: Option<er_protocol::TabKey>,
    view: Option<&PaintedView>,
    finding_id: String,
    body: String,
    client_id: Option<String>,
) -> Result<String, ErError> {
    let tab = crate::arena_service::tab(desktop, wire)?;
    let (_, _, _, title, description) =
        ask_context::lookup_finding_fields(&tab.content.ai.state, &finding_id).map_err(refused)?;
    let thread = finding_id.clone();
    let (reply, _) = content_routes::write_on_key(desktop, tab.entry.key, view, |content| {
        Ok(Command::ReplyToFinding {
            context: content.context.clone(),
            finding_id: thread,
            body: String::new(),
            ai_assist: true,
            client_id,
        })
    })
    .map_err(refused)?;
    let Reply::FindingReplied { root_id, .. } = reply else {
        return Err(refused(format!("unexpected reply: {reply:?}")));
    };
    let prompt = if body.trim().is_empty() {
        DEFAULT_ASK_AI_PROMPT
    } else {
        body.as_str()
    };
    let prompt = format!("Finding: {title}\n\n{description}\n\n---\n\n{prompt}");
    // The content the write published holds the thread it created.
    let tab = crate::arena_service::tab(desktop, Some(er_service::wire_tab_key(tab.entry.key)))?;
    ask_thread(desktop, &tab, root_id.clone(), prompt, Some(AI_ACTION_ASK))?;
    Ok(root_id)
}

/// What an ask reads from a tab's content: the diff a review of its view
/// reads, and the checkout, or a remote tab's bucket, as the provider's
/// directory.
pub fn content_ask_tab<'a>(desktop: &DesktopState, content: &'a TabContent) -> AskTab<'a> {
    let tab = &content.tab;
    AskTab {
        repo_root: &tab.repo_root,
        base_branch: &tab.base_branch,
        current_branch: &tab.current_branch,
        work_dir: if tab.identity().is_remote() {
            content.context.er_dir.clone()
        } else {
            tab.repo_root.clone()
        },
        files: content.diff.files.iter().map(|f| DiffFile::clone(f)).collect(),
        raw_diff: er_service::ask_raw_diff(&desktop.ports, content),
        ai: &content.ai.state,
    }
}

/// The tab's content with the finding's file parsed, so the finding's line
/// content reads its hunks. A stub is parsed on the tab's actor, the one
/// parse path, within its reply bound.
pub fn parsed_for_finding(
    desktop: &DesktopState,
    tab: &ArenaTab,
    finding_id: &str,
) -> Arc<TabContent> {
    let content = Arc::clone(&tab.content);
    let Ok((file, ..)) = ask_context::lookup_finding_fields(&content.ai.state, finding_id) else {
        return content;
    };
    let stub = content.diff.meta.lazy
        && content
            .diff
            .files
            .iter()
            .any(|f| f.path == file && f.hunks.is_empty() && !f.compacted);
    if !stub {
        return content;
    }
    let parse = Command::ParseFiles {
        key: tab.entry.key,
        paths: vec![file],
        expand: false,
    };
    if desktop.service.send(&tab.workspace(), parse).is_err() {
        return content;
    }
    desktop
        .service
        .content(tab.entry.key, &tab.entry.descriptor)
        .unwrap_or(content)
}

/// The provider, model and effort an ask runs: the saved selection,
/// resolved as the App resolves its session pick.
pub fn selection(config: &ErConfig) -> (Option<String>, Option<String>, Option<String>) {
    let hub = &config.ai_hub;
    let saved = hub.resolve_default_selection(&config.agent);
    let provider = hub.resolve_provider_id(saved.provider_id.as_deref());
    let model = provider
        .as_deref()
        .and_then(|provider| hub.resolve_model_id(provider, saved.model_id.as_deref()));
    let effort = er_engine::config::resolve_effort_for_model(
        hub,
        &config.agent,
        provider.as_deref(),
        model.as_deref(),
        saved.effort.as_deref(),
        None,
    );
    (provider, model, effort)
}

/// Where an ask's answer goes.
enum Answer {
    Thread {
        target: AnswerTarget,
        reply_to: String,
        ai_action: Option<&'static str>,
    },
    Finding {
        finding_id: String,
    },
}

impl Answer {
    /// The key its reply is pending under in `jobs`, as the page joins it.
    fn pending(&self) -> (String, Option<&'static str>) {
        match self {
            Self::Thread {
                reply_to,
                ai_action,
                ..
            } => (reply_to.clone(), *ai_action),
            Self::Finding { finding_id } => (format!("finding:{finding_id}"), None),
        }
    }
}

/// One ask, as it starts: the tab it was made on, that tab's context then,
/// what it sends and where the answer goes.
struct Asked {
    key: TabKey,
    context: TabContext,
    ask: AskContext,
    system: String,
    answer: Answer,
    prompt: String,
}

fn ask_thread(
    desktop: &DesktopState,
    tab: &ArenaTab,
    thread_id: String,
    prompt: String,
    ai_action: Option<&'static str>,
) -> Result<(), ErError> {
    let (ask, target) = ask_context::thread_ask(content_ask_tab(desktop, &tab.content), &thread_id)
        .map_err(refused)?;
    let answer = Answer::Thread {
        target,
        reply_to: thread_id,
        ai_action,
    };
    start(desktop, tab.entry.key, &tab.content.context, ask, answer, prompt);
    Ok(())
}

fn ask_finding(
    desktop: &DesktopState,
    tab: &ArenaTab,
    finding_id: String,
    prompt: String,
) -> Result<(), ErError> {
    let content = parsed_for_finding(desktop, tab, &finding_id);
    let ask = ask_context::finding_ask(content_ask_tab(desktop, &content), &finding_id)
        .map_err(refused)?;
    let answer = Answer::Finding { finding_id };
    start(desktop, tab.entry.key, &content.context, ask, answer, prompt);
    Ok(())
}

/// Marks the reply pending in `jobs`, then runs the provider on a thread of
/// its own and lands its answer.
fn start(
    desktop: &DesktopState,
    key: TabKey,
    context: &TabContext,
    ask: AskContext,
    answer: Answer,
    prompt: String,
) {
    let config = crate::arena_service::config(desktop);
    let (provider, model, effort) = selection(&config);
    let invocation = plan_card_ai_invocation(
        &config,
        provider.as_deref(),
        model.as_deref(),
        effort.as_deref(),
        ask.work_dir.clone(),
    );
    let model = card_model(model, config.agent.model.clone());
    let (pending, ai_action) = answer.pending();
    desktop.pending_ai_replies.insert(
        pending.clone(),
        PendingAiReply {
            started_at_ms: now_ms(),
            ai_action,
        },
    );
    let asked = Asked {
        key,
        context: context.clone(),
        system: ask.system_prompt(),
        ask,
        answer,
        prompt: if prompt.trim().is_empty() {
            DEFAULT_ASK_AI_PROMPT.to_string()
        } else {
            prompt
        },
    };
    let desktop = desktop.clone();
    std::thread::spawn(move || {
        let body = answer_of(&invocation, &asked, model.as_deref());
        land(&desktop, asked.key, &asked.context, asked.answer, body);
        desktop.pending_ai_replies.remove([pending.as_str()]);
        desktop
            .app_owner
            .refresh_meta(&*desktop.ports.git, &asked.ask.repo_root);
        desktop.desktop_revision.fetch_add(1, Ordering::Relaxed);
    });
}

fn answer_of(invocation: &CardAiInvocation, asked: &Asked, model: Option<&str>) -> String {
    run_card_ai_subprocess(
        invocation,
        &asked.system,
        &asked.prompt,
        model,
        Some(card_progress()),
    )
    .into_inner()
}

/// Sends the answer to the workspace of the context the ask captured,
/// started when none runs, whatever the registry lists or shows by now.
fn land(desktop: &DesktopState, key: TabKey, context: &TabContext, answer: Answer, body: String) {
    let command = match answer {
        Answer::Thread {
            target,
            reply_to,
            ai_action,
        } => Command::SubmitAiReply {
            key,
            context: context.clone(),
            target: AnnotationTarget {
                file: target.file,
                hunk_idx: target.hunk_idx,
                line_num: target.line_num,
                line_num_end: None,
            },
            body,
            ai_action: match target.kind {
                CommentType::Question | CommentType::Note => ai_action.map(str::to_string),
                CommentType::GitHubComment => None,
            },
            kind: target.kind,
            reply_to,
        },
        Answer::Finding { finding_id } => Command::AppendFindingResponse {
            key,
            context: context.clone(),
            finding_id,
            body,
        },
    };
    if let Err(error) = desktop.service.send(&context.workspace_key(), command) {
        log::warn!("an AI answer did not land: {error}");
    }
}

fn refused(message: impl Into<String>) -> ErError {
    ErError {
        kind: ErErrorKind::Refused,
        message: message.into(),
    }
}
