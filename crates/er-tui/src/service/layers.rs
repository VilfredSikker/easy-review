//! The flag-on view toggles: the layers, the Gate and the side panel. Each
//! is view state on the tab's `TuiView` entry; none reaches the service.

use er_engine::ai::{self, Confidence, PanelContent};
use er_service::TabContent;

use super::tui_view::TabViewState;

/// The layer a toggle key flips.
#[derive(Clone, Copy)]
pub enum Layer {
    Comments,
    Questions,
    HideResolved,
    AiFindings,
    /// The arbiter's drops, listed in the AI summary.
    Dropped,
}

/// Flips `layer`; answers what the status line says, as the App's key does.
pub fn toggle(state: &mut TabViewState, layer: Layer) -> &'static str {
    let layers = &mut state.layers;
    match layer {
        Layer::Comments => {
            layers.show_github_comments = !layers.show_github_comments;
            if layers.show_github_comments {
                "Comments: visible"
            } else {
                "Comments: hidden"
            }
        }
        Layer::Questions => {
            layers.show_questions = !layers.show_questions;
            if layers.show_questions {
                "Questions: visible"
            } else {
                "Questions: hidden"
            }
        }
        Layer::HideResolved => {
            layers.hide_resolved = !layers.hide_resolved;
            if layers.hide_resolved {
                "Resolved: hidden"
            } else {
                "Resolved: visible"
            }
        }
        Layer::AiFindings => {
            layers.show_ai_findings = !layers.show_ai_findings;
            if layers.show_ai_findings {
                "AI findings: ON"
            } else {
                "AI findings: OFF"
            }
        }
        Layer::Dropped => {
            layers.show_dropped = !layers.show_dropped;
            if layers.show_dropped {
                "Arbiter's drops: listed"
            } else {
                "Arbiter's drops: counted only"
            }
        }
    }
}

/// Moves the Gate to its next level and pins it, so a later AI revision
/// leaves it; answers the level's label.
fn next_gate(state: &mut TabViewState) -> &'static str {
    let layers = &mut state.layers;
    layers.min_trust = match layers.min_trust {
        Confidence::Informational => Confidence::Tentative,
        Confidence::Tentative => Confidence::Confirmed,
        _ => Confidence::Informational,
    };
    layers.min_trust_pinned = true;
    match layers.min_trust {
        Confidence::Informational => "all findings",
        Confidence::Tentative => "informational hidden",
        Confidence::Confirmed | Confidence::Dropped => "confirmed only",
    }
}

/// T: the next Gate level; answers the status line's message, which says
/// whether an arbiter has graded the review, as the App's key does.
pub fn cycle_gate(state: &mut TabViewState, content: &TabContent) -> String {
    let graded = if content.ai.state.arbiter_effect.graded() {
        "arbiter-graded"
    } else {
        "self-reported grades, nothing graded yet"
    };
    let level = next_gate(state);
    format!("Confidence: {level} — {graded}")
}

/// A layer row of the AI hub.
#[derive(Clone, Copy)]
pub enum HubRow {
    Layer(Layer),
    Gate,
}

/// Runs an AI hub layer row; answers the status line's message, as the App's
/// hub does, whose Gate message names the level alone.
pub fn hub_row(state: &mut TabViewState, row: HubRow) -> String {
    match row {
        HubRow::Layer(layer) => toggle(state, layer).to_string(),
        HubRow::Gate => format!("Confidence: {}", next_gate(state)),
    }
}

/// The Gate follows the review: each new AI revision sets it from the
/// arbiter's effect, unless it was set by hand, as the App's reload does.
pub fn follow(state: &mut TabViewState, content: &TabContent) {
    let revision = content.ai.revision;
    if state.gate_followed == Some(revision) {
        return;
    }
    state.gate_followed = Some(revision);
    if !state.layers.min_trust_pinned {
        state.layers.min_trust = ai::min_trust_for(&content.ai.state.arbiter_effect);
    }
}

const PANELS: [PanelContent; 5] = [
    PanelContent::FileDetail,
    PanelContent::AiSummary,
    PanelContent::PrOverview,
    PanelContent::SymbolRefs,
    PanelContent::AgentLog,
];

/// Moves the side panel to the next one with something to show, closing it
/// after the last, as the App's cycle does. This path has no symbol refs.
pub fn cycle_panel(state: &mut TabViewState, content: &TabContent, forward: bool) {
    let available = |panel: &PanelContent| match panel {
        PanelContent::FileDetail | PanelContent::AgentLog => true,
        PanelContent::AiSummary => state.layers.show_ai_findings && content.ai.state.has_data(),
        PanelContent::PrOverview => content.pr.overview.is_some(),
        PanelContent::SymbolRefs => false,
    };
    let at = state
        .panel
        .and_then(|panel| PANELS.iter().position(|&p| p == panel));
    let next = match (forward, at) {
        (true, None) => PANELS.iter().find(|p| available(p)),
        (true, Some(at)) => PANELS[at + 1..].iter().find(|p| available(p)),
        (false, None) => PANELS.iter().rev().find(|p| available(p)),
        (false, Some(at)) => PANELS[..at].iter().rev().find(|p| available(p)),
    };
    state.panel = next.copied();
    state.panel_scroll = 0;
    if state.panel.is_none() {
        state.panel_focus = false;
    }
}
