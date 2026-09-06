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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::experts::EXPERTS;

    const FALLBACK_COLOR: &str = "#8089a0";
    const FALLBACK_ICON: &str = "sparkle";

    #[test]
    fn every_registered_expert_has_a_dedicated_color_and_icon() {
        for def in EXPERTS {
            assert_ne!(
                expert_color(def.id),
                FALLBACK_COLOR,
                "expert `{}` has no arm in expert_color and fell through to the neutral default",
                def.id
            );
            assert_ne!(
                expert_icon(def.id),
                FALLBACK_ICON,
                "expert `{}` has no arm in expert_icon and fell through to the neutral default",
                def.id
            );
        }
    }

    #[test]
    fn expert_colors_are_six_digit_hex() {
        for def in EXPERTS {
            let color = expert_color(def.id);
            assert_eq!(color.len(), 7, "`{}` -> `{color}` is not #rrggbb", def.id);
            assert!(color.starts_with('#'), "`{}` -> `{color}`", def.id);
            assert!(
                color[1..].chars().all(|c| c.is_ascii_hexdigit()),
                "`{}` -> `{color}` has non-hex digits",
                def.id
            );
        }
    }

    #[test]
    fn unknown_expert_id_falls_back_to_neutral_color_and_icon() {
        assert_eq!(expert_color("not-an-expert"), FALLBACK_COLOR);
        assert_eq!(expert_icon("not-an-expert"), FALLBACK_ICON);
        assert_eq!(expert_color(""), FALLBACK_COLOR);
        assert_eq!(expert_icon(""), FALLBACK_ICON);
    }

    #[test]
    fn agent_meta_threads_expert_color_and_icon_into_meta() {
        let meta = agent_meta("expert:security").expect("security expert has meta");
        assert_eq!(meta.kind, "expert:security");
        assert_eq!(meta.label, "Security");
        // Literals, not `expert_color("security")` — comparing the lookup against itself
        // would still pass if security's arm were deleted or swapped with another expert's.
        assert_eq!(meta.color, "#ff6b6b");
        assert_eq!(meta.icon, "shield");
        // ...and the lookups are what feed it, so the two cannot be wired apart.
        assert_eq!(meta.color, expert_color("security"));
        assert_eq!(meta.icon, expert_icon("security"));
    }

    #[test]
    fn agent_meta_rejects_unregistered_kinds() {
        assert!(agent_meta("expert:not-an-expert").is_none());
        assert!(agent_meta("nonsense").is_none());
        assert!(agent_meta("expert:").is_none());
    }

    #[test]
    fn list_arena_agent_kinds_includes_every_expert_with_its_own_color_and_icon() {
        let kinds = list_arena_agent_kinds();
        for def in EXPERTS {
            let want = format!("expert:{}", def.id);
            let meta = kinds
                .iter()
                .find(|m| m.kind == want)
                .unwrap_or_else(|| panic!("`{want}` missing from arena agent kinds"));
            // The list must thread the expert lookups through, not hardcode or drop them...
            assert_eq!(meta.icon, expert_icon(def.id));
            assert_eq!(meta.color, expert_color(def.id));
            // ...and what it threads through must be a real arm, not the neutral default.
            assert_ne!(meta.color, FALLBACK_COLOR, "`{want}` renders as unknown");
            assert_ne!(meta.icon, FALLBACK_ICON, "`{want}` renders as unknown");
        }
    }
}
