//! Running a card ask: Ask AI, Validate and Elaborate on a thread or a
//! finding. Each reads the tab it is handed, by the rules in `context`, and
//! the provider, model and effort of the saved selection. The reply being
//! written stays in the pending set until the answer lands, which goes to the
//! tab's actor as `SubmitAiReply` or `AppendFindingResponse` with the
//! context the ask captured: a tab that moved to another view, lost the focus
//! or closed meanwhile still gets the answer in the bucket it asked from.
//!
//! The provider runs on a thread of its own, off the actor and off the task
//! host's queue, so an ask never waits behind a review task (ADR 0074). Like
//! every agent spawn it runs through `JobRunner`, in the background workload
//! of the slot pool (ADR 0021).

use std::sync::Arc;

use er_engine::ai::{CommentType, AI_ACTION_ASK, AI_ACTION_ELABORATE};
use er_engine::card_job::{plan_card_ai_invocation, run_card_ai_subprocess, CardAiInvocation};
use er_engine::config::ErConfig;
use er_engine::ports::Ports;
use er_engine::tabs_record::TabDescriptor;
use er_engine::TabKey;

use super::context::{self, content_ask_tab, AnswerTarget, AskContext};
use super::{
    card_action_prompt, card_model, card_progress, now_ms, AiReplyJobs, PendingAiReply,
    DEFAULT_ASK_AI_PROMPT, VALIDATE_CARD_AI_PROMPT,
};
use crate::content::TabContent;
use crate::handle::WorkspaceError;
use crate::protocol::{AnnotationTarget, Command, Reply, TabContext};

/// The tab an ask is made on: its key, the actor that holds it, and the
/// content it was read from.
#[derive(Clone)]
pub struct AskedTab {
    pub key: TabKey,
    pub workspace: String,
    pub descriptor: TabDescriptor,
    pub content: Arc<TabContent>,
}

/// What a front end does for an ask. Taken per call, so er-service holds no
/// front end's state.
#[derive(Clone)]
pub struct AskHooks {
    pub ports: Ports,
    /// The settings view's config, read when the ask starts.
    pub config: Arc<ErConfig>,
    /// Sends a command to the actor that holds a workspace.
    pub send: Arc<dyn Fn(&str, Command) -> Result<Reply, WorkspaceError> + Send + Sync>,
    /// A tab's published content now.
    pub content: Arc<dyn Fn(TabKey, &TabDescriptor) -> Option<Arc<TabContent>> + Send + Sync>,
    /// Tells the person.
    pub notify: Arc<dyn Fn(String) + Send + Sync>,
    /// Runs after an answer lands, with the asking tab's repo root.
    pub landed: Arc<dyn Fn(&str) + Send + Sync>,
}

/// Asks about a thread with `prompt`, or with the thread kind's own prompt
/// when it is empty.
pub fn ask(
    hooks: &AskHooks,
    replies: &AiReplyJobs,
    tab: &AskedTab,
    thread_id: String,
    prompt: String,
) -> Result<(), String> {
    let prompt = if prompt.trim().is_empty() {
        card_action_prompt(&thread_id, AI_ACTION_ASK).map_or(prompt, str::to_string)
    } else {
        prompt
    };
    ask_thread(hooks, replies, tab, thread_id, prompt, Some(AI_ACTION_ASK))
}

/// Has the AI rewrite a question or note as a self-contained one.
pub fn elaborate(
    hooks: &AskHooks,
    replies: &AiReplyJobs,
    tab: &AskedTab,
    thread_id: String,
) -> Result<(), String> {
    let prompt = card_action_prompt(&thread_id, AI_ACTION_ELABORATE)
        .ok_or_else(|| "Elaborate works on questions and notes only".to_string())?;
    ask_thread(
        hooks,
        replies,
        tab,
        thread_id,
        prompt.to_string(),
        Some(AI_ACTION_ELABORATE),
    )
}

/// Has the AI check a finding, or the finding a thread was opened on, or
/// else the thread itself.
pub fn validate(
    hooks: &AskHooks,
    replies: &AiReplyJobs,
    tab: &AskedTab,
    thread_id: Option<String>,
    finding_id: Option<String>,
) -> Result<(), String> {
    let finding = finding_id.or_else(|| {
        let thread = thread_id.as_deref()?;
        context::finding_id_for_thread(&tab.content.ai.state, thread)
    });
    let prompt = VALIDATE_CARD_AI_PROMPT.to_string();
    if let Some(finding_id) = finding {
        return ask_finding(hooks, replies, tab, finding_id, prompt);
    }
    let thread_id = thread_id.ok_or_else(|| "thread_id or finding_id is required".to_string())?;
    ask_thread(hooks, replies, tab, thread_id, prompt, None)
}

/// The tab's content with the finding's file parsed, so the finding's line
/// content reads its hunks. A stub is parsed on the tab's actor, the one
/// parse path, within its reply bound.
pub fn parsed_for_finding(hooks: &AskHooks, tab: &AskedTab, finding_id: &str) -> Arc<TabContent> {
    let content = Arc::clone(&tab.content);
    let Ok((file, ..)) = context::lookup_finding_fields(&content.ai.state, finding_id) else {
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
        key: tab.key,
        paths: vec![file],
        expand: false,
    };
    if (hooks.send)(&tab.workspace, parse).is_err() {
        return content;
    }
    (hooks.content)(tab.key, &tab.descriptor).unwrap_or(content)
}

/// The provider, model and effort an ask runs: the saved selection.
pub fn selection(config: &ErConfig) -> (Option<String>, Option<String>, Option<String>) {
    let selection = crate::runs::selection(config);
    (selection.provider_id, selection.model_id, selection.effort)
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

/// One ask, as it starts: the tab it was made on and the actor that holds
/// it, that tab's context then, what it sends and where the answer goes.
struct Asked {
    key: TabKey,
    workspace: String,
    context: TabContext,
    ask: AskContext,
    system: String,
    answer: Answer,
    prompt: String,
}

/// Asks about the thread `thread_id` on `tab`, its answer a reply in the
/// thread.
pub fn ask_thread(
    hooks: &AskHooks,
    replies: &AiReplyJobs,
    tab: &AskedTab,
    thread_id: String,
    prompt: String,
    ai_action: Option<&'static str>,
) -> Result<(), String> {
    let (ask, target) =
        context::thread_ask(content_ask_tab(&hooks.ports, &tab.content), &thread_id)?;
    let answer = Answer::Thread {
        target,
        reply_to: thread_id,
        ai_action,
    };
    start(
        hooks,
        replies,
        tab,
        &tab.content.context,
        ask,
        answer,
        prompt,
    );
    Ok(())
}

fn ask_finding(
    hooks: &AskHooks,
    replies: &AiReplyJobs,
    tab: &AskedTab,
    finding_id: String,
    prompt: String,
) -> Result<(), String> {
    let content = parsed_for_finding(hooks, tab, &finding_id);
    let ask = context::finding_ask(content_ask_tab(&hooks.ports, &content), &finding_id)?;
    let answer = Answer::Finding { finding_id };
    start(hooks, replies, tab, &content.context, ask, answer, prompt);
    Ok(())
}

/// Marks the reply pending, then runs the provider on a thread of its own
/// and lands its answer.
fn start(
    hooks: &AskHooks,
    replies: &AiReplyJobs,
    tab: &AskedTab,
    context: &TabContext,
    ask: AskContext,
    answer: Answer,
    prompt: String,
) {
    let config = &hooks.config;
    let (provider, model, effort) = selection(config);
    let invocation = plan_card_ai_invocation(
        config,
        provider.as_deref(),
        model.as_deref(),
        effort.as_deref(),
        ask.work_dir.clone(),
    );
    let model = card_model(model, config.agent.model.clone());
    let (pending, ai_action) = answer.pending();
    replies.insert(
        pending.clone(),
        PendingAiReply {
            started_at_ms: now_ms(),
            ai_action,
        },
    );
    let asked = Asked {
        key: tab.key,
        workspace: tab.workspace.clone(),
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
    let (hooks, replies) = (hooks.clone(), replies.clone());
    std::thread::spawn(move || {
        let body = answer_of(&invocation, &asked, model.as_deref());
        land(&hooks, &asked, body);
        replies.remove([pending.as_str()]);
        (hooks.landed)(&asked.ask.repo_root);
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

/// Sends the answer, with the context the ask captured, to the actor that
/// held the asking tab, whatever the registry lists or shows by now.
fn land(hooks: &AskHooks, asked: &Asked, body: String) {
    let (key, context) = (asked.key, asked.context.clone());
    let command = match &asked.answer {
        Answer::Thread {
            target,
            reply_to,
            ai_action,
        } => Command::SubmitAiReply {
            key,
            context,
            target: AnnotationTarget {
                file: target.file.clone(),
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
            reply_to: reply_to.clone(),
        },
        Answer::Finding { finding_id } => Command::AppendFindingResponse {
            key,
            context,
            finding_id: finding_id.clone(),
            body,
        },
    };
    if let Err(error) = (hooks.send)(&asked.workspace, command) {
        (hooks.notify)(format!("Failed to save the AI answer: {error}"));
    }
}
