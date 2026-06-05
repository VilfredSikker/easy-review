//! Specialized agent metadata for arena runs (aligned with `list_ai_reviewers`).

use crate::ai::experts::{expert_by_id, list_ai_reviewers};
use crate::ai::professor::{PROFESSOR_ID, PROFESSOR_LABEL};
use crate::ai::triage::{TRIAGE_ID, TRIAGE_LABEL};

#[derive(Debug, Clone)]
pub struct AgentMeta {
    pub kind: String,
    pub label: String,
    pub description: String,
    pub color: String,
    pub icon: String,
}

pub fn agent_meta(agent_kind: &str) -> Option<AgentMeta> {
    if agent_kind == TRIAGE_ID {
        return Some(AgentMeta {
            kind: TRIAGE_ID.into(),
            label: TRIAGE_LABEL.into(),
            description: "Fast branch scan — first impression and review routing".into(),
            color: "#22d3ee".into(),
            icon: "radar".into(),
        });
    }
    if agent_kind == "general" {
        return Some(AgentMeta {
            kind: "general".into(),
            label: "General".into(),
            description: "Risk, order, checklist, and summary".into(),
            color: "#ff7a2b".into(),
            icon: "sparkle".into(),
        });
    }
    if agent_kind == PROFESSOR_ID {
        return Some(AgentMeta {
            kind: PROFESSOR_ID.into(),
            label: PROFESSOR_LABEL.into(),
            description: "Learn the implementation — key mechanisms in this diff".into(),
            color: "#9b87f5".into(),
            icon: "graduation-cap".into(),
        });
    }
    if let Some(id) = agent_kind.strip_prefix("expert:") {
        let def = expert_by_id(id)?;
        return Some(AgentMeta {
            kind: agent_kind.to_string(),
            label: def.label.to_string(),
            description: def.description.to_string(),
            color: expert_color(id),
            icon: expert_icon(id),
        });
    }
    None
}

pub fn list_arena_agent_kinds() -> Vec<AgentMeta> {
    list_ai_reviewers()
        .into_iter()
        .filter_map(|r| agent_meta(&r.kind))
        .collect()
}

fn expert_color(id: &str) -> String {
    match id {
        "security" => "#ff6b6b",
        "performance" => "#7f87ff",
        "reliability" => "#5fd970",
        "testing" => "#ffc457",
        "api" => "#4ec9a4",
        "patterns" => "#ff7a2b",
        "simplifying" => "#9b87f5",
        "mentorship" => "#4ec9a4",
        _ => "#8089a0",
    }
    .to_string()
}

fn expert_icon(id: &str) -> String {
    match id {
        "security" => "shield",
        "performance" => "lightning",
        "reliability" => "shield-check",
        "testing" => "tube",
        "api" => "plugs",
        "patterns" => "magnifying-glass",
        "simplifying" => "scissors",
        "mentorship" => "hand-heart",
        _ => "sparkle",
    }
    .into()
}
