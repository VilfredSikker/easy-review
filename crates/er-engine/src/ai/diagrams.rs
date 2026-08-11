//! Mermaid diagram sidecars (`diagrams/*.json`) — AI-generated visual overviews
//! of a diff (mental model, subsystems, flows, or a custom prompt).
//!
//! Each diagram lives in its own file under `{er_dir}/diagrams/<id>.json`, so
//! regenerating one preset only overwrites that preset's file and custom
//! diagrams accumulate. Diagrams are per-view-bucket like triage/review: the
//! active view's `TabState::er_dir()` is both the write target
//! (`generate_diagram`) and the read source (`load_ai_state`).

use serde::{Deserialize, Serialize};

pub const DIAGRAM_KIND_MENTAL_MODEL: &str = "mental-model";
pub const DIAGRAM_KIND_SUBSYSTEMS: &str = "subsystems";
pub const DIAGRAM_KIND_FLOWS: &str = "flows";
pub const DIAGRAM_KIND_CUSTOM: &str = "custom";

/// One AI-generated mermaid diagram of a diff.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErDiagram {
    pub version: u32,
    pub diff_hash: String,
    #[serde(default)]
    pub created_at: String,
    /// `mental-model` | `subsystems` | `flows` | `custom`.
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub title: String,
    /// User-supplied prompt for `custom` diagrams (empty for presets). Kept so
    /// a custom diagram can be regenerated with the same intent.
    #[serde(default)]
    pub prompt: String,
    /// Mermaid diagram source (bare source — no ``` fences).
    pub mermaid: String,
    /// File-stem id (`diagrams/<id>.json`); assigned by the loader from the
    /// filename, not trusted from the file body.
    #[serde(default)]
    pub id: String,
    /// Runtime-only staleness flag, set by the loader when the diagram's
    /// `diff_hash` no longer matches the current diff.
    #[serde(skip)]
    pub stale: bool,
}

/// A selectable diagram preset in the UI.
#[derive(Debug, Clone, Serialize)]
pub struct DiagramPresetInfo {
    pub kind: String,
    pub label: String,
    pub description: String,
}

/// Built-in diagram presets, in display order. `custom` is not listed — it is
/// the free-form prompt entry in the UI.
pub fn diagram_presets() -> Vec<DiagramPresetInfo> {
    vec![
        DiagramPresetInfo {
            kind: DIAGRAM_KIND_MENTAL_MODEL.to_string(),
            label: "Mental model".to_string(),
            description: "High-level map of the areas this diff touches and how they relate"
                .to_string(),
        },
        DiagramPresetInfo {
            kind: DIAGRAM_KIND_SUBSYSTEMS.to_string(),
            label: "Subsystems".to_string(),
            description: "Changed files grouped by subsystem, with interactions".to_string(),
        },
        DiagramPresetInfo {
            kind: DIAGRAM_KIND_FLOWS.to_string(),
            label: "Flows".to_string(),
            description: "Runtime flow through the changed code for the main scenarios".to_string(),
        },
    ]
}

pub fn diagram_kind_label(kind: &str) -> &'static str {
    match kind {
        DIAGRAM_KIND_MENTAL_MODEL => "Mental model",
        DIAGRAM_KIND_SUBSYSTEMS => "Subsystems",
        DIAGRAM_KIND_FLOWS => "Flows",
        DIAGRAM_KIND_CUSTOM => "Custom",
        _ => "Diagram",
    }
}

pub fn is_valid_diagram_kind(kind: &str) -> bool {
    matches!(
        kind,
        DIAGRAM_KIND_MENTAL_MODEL
            | DIAGRAM_KIND_SUBSYSTEMS
            | DIAGRAM_KIND_FLOWS
            | DIAGRAM_KIND_CUSTOM
    )
}

/// Background task kind for a diagram run — `diagram:<diagram-kind>`, so two
/// different presets against the same view bucket don't dedupe against each
/// other (and are distinguishable in the UI).
pub fn diagram_task_kind(kind: &str) -> String {
    format!("diagram:{kind}")
}

/// Diagram id sanity check — ids become file names (`diagrams/<id>.json`), so
/// reject anything that could escape the directory.
pub fn is_safe_diagram_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 120
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagram_presets_are_valid_kinds_in_display_order() {
        let presets = diagram_presets();
        let kinds: Vec<&str> = presets.iter().map(|p| p.kind.as_str()).collect();
        assert_eq!(
            kinds,
            vec![
                DIAGRAM_KIND_MENTAL_MODEL,
                DIAGRAM_KIND_SUBSYSTEMS,
                DIAGRAM_KIND_FLOWS
            ]
        );
        assert!(presets.iter().all(|p| is_valid_diagram_kind(&p.kind)));
        assert!(!is_valid_diagram_kind("not-a-kind"));
        assert!(is_valid_diagram_kind(DIAGRAM_KIND_CUSTOM));
    }
}
