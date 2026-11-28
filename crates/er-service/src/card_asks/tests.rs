//! The card prompts, the provider run and the ask context.

use er_engine::ai::{AI_ACTION_ASK, AI_ACTION_ELABORATE};
use er_engine::card_job::{run_card_ai_subprocess, CardAiInvocation};

use super::*;

fn with_fake_claude<R>(value: &str, f: impl FnOnce() -> R) -> R {
    er_engine::test_support::with_var("ER_FAKE_CLAUDE", value, f)
}

#[test]
fn card_action_prompt_picks_the_kind_and_action() {
    assert_eq!(
        card_action_prompt("q-1", AI_ACTION_ELABORATE),
        Some(ELABORATE_QUESTION_AI_PROMPT)
    );
    assert_eq!(
        card_action_prompt("n-1", AI_ACTION_ELABORATE),
        Some(ELABORATE_NOTE_AI_PROMPT)
    );
    assert_eq!(
        card_action_prompt("q-1", AI_ACTION_ASK),
        Some(ANSWER_QUESTION_AI_PROMPT)
    );
    assert_eq!(
        card_action_prompt("n-1", AI_ACTION_ASK),
        Some(ANSWER_NOTE_AI_PROMPT)
    );
    // GitHub comments are shared, so the one-click private actions skip them.
    assert_eq!(card_action_prompt("c-1", AI_ACTION_ELABORATE), None);
    assert_eq!(card_action_prompt("q-1", "unknown"), None);
}

#[test]
fn elaborate_prompts_rewrite_rather_than_answer() {
    for prompt in [ELABORATE_QUESTION_AI_PROMPT, ELABORATE_NOTE_AI_PROMPT] {
        assert!(prompt.starts_with("Rewrite"));
        assert!(prompt.contains("Reply with only"));
    }
    assert!(ELABORATE_QUESTION_AI_PROMPT.contains("Do not answer"));
    assert!(ELABORATE_NOTE_AI_PROMPT.contains("Do not edit any file"));
}

#[test]
fn card_ai_subprocess_honors_fake_sentinel_ok() {
    let inv = CardAiInvocation {
        command: "claude".into(),
        args: vec![],
        work_dir: "/tmp".into(),
        is_claude_compatible: true,
        uses_stream_json: false,
        env: vec![],
        // These go through ER_FAKE_CLAUDE and never reach the spawn.
        timeout: std::time::Duration::from_mins(15),
        slot_cap: 3,
        slot_ceiling: 6,
    };
    let body = with_fake_claude("ok", || {
        run_card_ai_subprocess(&inv, "ctx", "prompt", Some("sonnet"), None)
    });
    assert_eq!(body.as_str(), "mocked ok");
}

#[test]
fn card_ai_subprocess_honors_fake_sentinel_fail() {
    let inv = CardAiInvocation {
        command: "claude".into(),
        args: vec![],
        work_dir: "/tmp".into(),
        is_claude_compatible: true,
        uses_stream_json: false,
        env: vec![],
        // These go through ER_FAKE_CLAUDE and never reach the spawn.
        timeout: std::time::Duration::from_mins(15),
        slot_cap: 3,
        slot_ceiling: 6,
    };
    let body = with_fake_claude("fail", || {
        run_card_ai_subprocess(&inv, "ctx", "prompt", Some("sonnet"), None)
    });
    assert!(
        body.as_str().starts_with("Pending — invoke via CLI"),
        "expected fallback message, got: {}",
        body.as_str()
    );
}

#[test]
fn card_ai_subprocess_returns_custom_sentinel_value() {
    let inv = CardAiInvocation {
        command: "claude".into(),
        args: vec![],
        work_dir: "/tmp".into(),
        is_claude_compatible: true,
        uses_stream_json: false,
        env: vec![],
        // These go through ER_FAKE_CLAUDE and never reach the spawn.
        timeout: std::time::Duration::from_mins(15),
        slot_cap: 3,
        slot_ceiling: 6,
    };
    let body = with_fake_claude("custom-response-text", || {
        run_card_ai_subprocess(&inv, "ctx", "prompt", Some("sonnet"), None)
    });
    assert_eq!(body.as_str(), "custom-response-text");
}
