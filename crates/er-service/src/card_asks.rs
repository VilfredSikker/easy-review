//! The card asks: Ask AI, Validate and Elaborate on a thread or a finding,
//! and the AI-assisted finding reply. They are planned and run here, beside
//! the task host (ADR 0074), and stay off its queue: `run` starts each on a
//! thread of its own. The front end sends the ask, hands over the tab and its
//! hooks, and reads the pending set for `jobs`.

pub mod context;
pub mod run;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub use context::{content_ask_tab, AnswerTarget, AskContext, AskTab};
pub use run::{AskHooks, AskedTab};

pub const DEFAULT_ASK_AI_PROMPT: &str = "Elaborate on this and answer any question directly.";

/// Per-card validation: check whether a comment, question, or finding still holds.
pub(crate) const VALIDATE_CARD_AI_PROMPT: &str = r#"Validate whether this review note is still accurate against the current code context provided.

Use Read / grep / rg in the repository (see system context for repo_root and diff excerpt) before concluding. Up to ~10 reads for this finding; cite file:line evidence.

Reply in markdown with:
1. **Verdict**: Confirmed | Outdated | Needs context | Unclear
2. **Evidence**: What in the code supports your verdict (cite file:line when possible)
3. **Recommendation**: What the reviewer should do next

Be concise. If the concern is already addressed in the current diff, say so clearly."#;

/// One-click Ask AI on a question: investigate and answer. "Validate" does not
/// fit a question — there is nothing to confirm or refute.
const ANSWER_QUESTION_AI_PROMPT: &str = r#"Answer and elaborate on this review question against the current code.

Use Read / grep / rg in the repository (see system context for repo_root and diff excerpt) before answering. Up to ~10 reads; cite file:line evidence.

Reply in markdown with:
1. **Answer**: A direct answer to the question.
2. **Details**: Relevant context from the code (cite file:line when possible).
3. **Follow-ups**: Anything the reviewer should double-check or decide.

Be concise and concrete."#;

/// One-click Ask AI on a note. A note is an instruction for a coding agent, so
/// the useful answer is whether the change is right and what it would touch —
/// not the change itself, which stays the agent's job once the note is sent.
const ANSWER_NOTE_AI_PROMPT: &str = r#"Assess this review note against the current code. The note is an instruction the reviewer intends to hand to a coding agent.

Use Read / grep / rg in the repository (see system context for repo_root and diff excerpt) before answering. Up to ~10 reads; cite file:line evidence. Do not edit any file.

Reply in markdown with:
1. **Assessment**: Whether the requested change is right, and why.
2. **Details**: What the change would touch (cite file:line when possible).
3. **Risks**: Anything that could break, or that the note leaves undecided.

Be concise and concrete."#;

/// Elaborate on a question: rewrite it, do not answer it. A terse question
/// written mid-review ("why fetch-depth?") only makes sense next to the line it
/// sits on; the rewrite has to survive export and being read by someone, or
/// some agent, who never saw that line.
const ELABORATE_QUESTION_AI_PROMPT: &str = r#"Rewrite this review question so it stands on its own, without the diff beside it.

Use Read / grep / rg in the repository (see system context for repo_root and diff excerpt) to learn what the question refers to. Up to ~10 reads.

The rewritten question must:
- name the file and the lines or symbol it is about;
- state the relevant facts from the code that make the question worth asking;
- say what a good answer has to settle.

Do not answer the question. Keep the reviewer's intent and tone; do not add concerns they did not raise.
Reply with only the rewritten question in markdown — no preamble, no heading."#;

/// Elaborate on a note: rewrite it into an instruction a coding agent can carry
/// out without the review open. Same reason as the question variant; the note
/// variant names the change and its outcome instead of what to settle.
const ELABORATE_NOTE_AI_PROMPT: &str = r#"Rewrite this review note into a self-contained instruction for a coding agent that will not see the diff.

Use Read / grep / rg in the repository (see system context for repo_root and diff excerpt) to learn what the note refers to. Up to ~10 reads. Do not edit any file.

The instruction must:
- name the file and the lines, job, function or other symbol to change;
- state the exact change to make;
- state the expected outcome, and anything that must stay unchanged when the note implies it.

Keep the reviewer's intent; do not widen the change beyond what the note asks.
Reply with only the instruction in markdown — no preamble, no heading."#;

/// The prompt for a one-click AI action on a question or note, keyed on the
/// thread id prefix the stores use (`q-` question, `n-` note). `None` for any
/// other thread: Ask AI and Elaborate act on the private stores only.
pub fn card_action_prompt(thread_id: &str, ai_action: &str) -> Option<&'static str> {
    let is_note = thread_id.starts_with("n-");
    if !is_note && !thread_id.starts_with("q-") {
        return None;
    }
    match (ai_action, is_note) {
        (er_engine::ai::AI_ACTION_ELABORATE, false) => Some(ELABORATE_QUESTION_AI_PROMPT),
        (er_engine::ai::AI_ACTION_ELABORATE, true) => Some(ELABORATE_NOTE_AI_PROMPT),
        (er_engine::ai::AI_ACTION_ASK, false) => Some(ANSWER_QUESTION_AI_PROMPT),
        (er_engine::ai::AI_ACTION_ASK, true) => Some(ANSWER_NOTE_AI_PROMPT),
        _ => None,
    }
}

/// The model a card ask runs: the one picked, else the agent's.
pub(crate) fn card_model(picked: Option<String>, agent_model: String) -> Option<String> {
    picked.or_else(|| (!agent_model.trim().is_empty()).then_some(agent_model))
}

/// Where a card ask's progress lines go: the card UI has no log panel, so
/// they go to the debug log, drained on their own thread while the job
/// blocks.
pub(crate) fn card_progress() -> er_engine::card_job::ProgressSink {
    Arc::new(|line: &str| {
        log::debug!("card ai: {line}");
    })
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// One provider run answering a thread or a finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingAiReply {
    pub started_at_ms: u64,
    /// Which action is running (`AI_ACTION_ASK` / `AI_ACTION_ELABORATE`); the
    /// placeholder carries it so the card can say "Elaborating" before the
    /// rewrite lands, rather than guessing from the thread.
    pub ai_action: Option<&'static str>,
}

/// Map of `thread_id -> PendingAiReply`, which a front end renders as a
/// synthetic "…thinking" reply while the provider runs.
pub type PendingAiReplies = Arc<Mutex<HashMap<String, PendingAiReply>>>;

/// The in-flight card replies, by thread id or `finding:<id>`: what `jobs`
/// serves. Every insert and removal goes through here and moves the
/// revision, so no writer leaves the page on a stale "…thinking" reply.
#[derive(Clone, Default)]
pub struct AiReplyJobs {
    replies: PendingAiReplies,
    revision: Arc<AtomicU64>,
    /// Called after each move, so a front end can push the change.
    moved: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl AiReplyJobs {
    pub fn new(moved: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            replies: Arc::default(),
            revision: Arc::default(),
            moved: Some(Arc::new(moved)),
        }
    }

    /// The map a front end joins into its threads and findings.
    pub fn replies(&self) -> &PendingAiReplies {
        &self.replies
    }

    /// How many times the set has moved.
    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }

    pub fn insert(&self, key: String, reply: PendingAiReply) {
        if let Ok(mut replies) = self.replies.lock() {
            replies.insert(key, reply);
        }
        self.bump();
    }

    /// Removes each key that is pending; moves the revision when one was.
    pub fn remove<'a>(&self, keys: impl IntoIterator<Item = &'a str>) {
        let mut removed = false;
        if let Ok(mut replies) = self.replies.lock() {
            for key in keys {
                removed |= replies.remove(key).is_some();
            }
        }
        if removed {
            self.bump();
        }
    }

    fn bump(&self) {
        self.revision.fetch_add(1, Ordering::AcqRel);
        if let Some(moved) = &self.moved {
            moved();
        }
    }
}

#[cfg(test)]
mod context_tests;
#[cfg(test)]
mod tests;
