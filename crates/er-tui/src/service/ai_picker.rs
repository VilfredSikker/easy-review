//! The flag-on AI hub and the provider, model and effort pickers, built from
//! the settings view and rebuilt when its revision moves. A pick saves the
//! default selection through the settings actor (decision 11), and a model
//! refresh is the actor's `RefreshAiModels`.
//!
//! The run rows plan on the service (P74.2) and the Review rows send flow
//! requests (P74.3); the context copy has no service route on this path yet.
//! The Specialized review and the Verify hubs are pickers here too.

use er_engine::ai::AiState;
use er_engine::config::{effort_display_label, AiHubConfig, ErConfig};
use er_service::{SettingsCommand, SettingsView};

use super::ai_runs::{self, Choice, Start};
use super::clears::Clear;
use super::config_hub::Step;
use super::flows::{self, Act, FlowFacts};
use super::layers::{HubRow, Layer};
use super::tui_view::Overlay;
use crate::input::keys::OverlayAction;
use crate::screen::view::{HubItem, HubKind};
use crate::screen::UNPORTED;

/// The AI hub's rows as the App lists them with no review flow: a label and
/// its key hint. `None` is the Advanced header.
const AI_HUB_ROWS: [Option<(&str, &str)>; 22] = [
    Some(("Review", "")),
    Some(("Triage branch", "")),
    None,
    Some(("Review work", "")),
    Some(("Generate summary", "")),
    Some(("Generate guided tour", "")),
    Some((SPECIALIZED, "")),
    Some(("Professor", "")),
    Some(("Validate review", "")),
    Some(("Answer questions", "")),
    Some(("Address notes", "")),
    Some((CHANGE_SELECTION, "")),
    Some(("Copy context to clipboard", "")),
    Some(("Toggle AI findings", "A")),
    Some(("Toggle comments", "")),
    Some(("Toggle questions", "")),
    Some(("Hide resolved", "X")),
    Some(("Confidence gate", "T")),
    Some(("Show arbiter's drops", "")),
    Some(("Propose risk rules", "")),
    Some((CLEANUP_DRAFTS, "z")),
    Some((CLEANUP_REVIEWS, "Z")),
];

const CHANGE_SELECTION: &str = "Change agent/model";
const SPECIALIZED: &str = "Specialized review";
const SPECIALIZED_ABOUT: &str = "Focused expert lens — security, patterns, testing, …";
const CLEANUP_DRAFTS: &str = "Cleanup questions & notes";
const CLEANUP_REVIEWS: &str = "Cleanup reviews";

/// What the AI hub's rows read from the tab, as the App's hub reads it.
#[derive(Clone)]
pub struct HubFacts {
    has_ai: bool,
    pub(super) has_review: bool,
    has_drafts: bool,
    pub(super) has_tour: bool,
    /// The tab's diff has a file to review.
    pub(super) has_diff: bool,
    pub(super) has_unanswered: bool,
    pub(super) has_unaddressed: bool,
    /// The GitHub comments a validation re-anchors.
    pub(super) comment_count: usize,
    flow: FlowFacts,
}

impl HubFacts {
    pub fn of(ai: &AiState, has_diff: bool, flow: FlowFacts) -> Self {
        let has_questions = ai
            .questions
            .as_ref()
            .is_some_and(|q| !q.questions.is_empty());
        Self {
            has_ai: ai.has_data(),
            has_review: ai.review.is_some(),
            has_drafts: has_questions || ai.has_notes(),
            has_tour: ai.has_tour(),
            has_diff,
            has_unanswered: ai.has_unanswered_questions(),
            has_unaddressed: ai.has_unaddressed_notes(),
            comment_count: ai
                .github_comments
                .as_ref()
                .map_or(0, er_engine::ai::count_eligible_github_comments),
            flow,
        }
    }
}

/// The layer rows the App's hub lists: label, description and row. Toggle
/// AI findings is live only with AI data, as on the App.
const LAYER_ROWS: [(&str, &str, HubRow); 6] = [
    (
        "Toggle AI findings",
        "Show/hide inline AI findings",
        HubRow::Layer(Layer::AiFindings),
    ),
    (
        "Toggle comments",
        "Show/hide GitHub comment layer",
        HubRow::Layer(Layer::Comments),
    ),
    (
        "Toggle questions",
        "Show/hide question layer",
        HubRow::Layer(Layer::Questions),
    ),
    (
        "Hide resolved",
        "Toggle hiding resolved comments",
        HubRow::Layer(Layer::HideResolved),
    ),
    (
        "Confidence gate",
        "Cycle which grades show: all, tentative+, confirmed only",
        HubRow::Gate,
    ),
    (
        "Show arbiter's drops",
        "List the findings the arbiter ruled out",
        HubRow::Layer(Layer::Dropped),
    ),
];

/// Which list a picker shows, so it can be built again from a newer view.
#[derive(Clone)]
enum Of {
    /// The hub, and what its rows read from the tab.
    AiHub(HubFacts),
    Provider,
    Model(String),
    Effort(String, String),
    Experts,
    Verify,
    Package(String),
}

/// What choosing a row does.
#[derive(Clone)]
enum Pick {
    Unported,
    Layer(HubRow),
    Run(Start),
    Flow(Act),
    Experts,
    Package(String),
    ChangeSelection,
    Provider(String),
    Model(String, String),
    Effort(String, String, String),
    Refresh(String),
    Clear(Clear),
}

pub struct Picker {
    of: Of,
    pub title: Option<String>,
    pub rows: Vec<HubItem>,
    picks: Vec<Pick>,
    pub selected: usize,
    revision: u64,
}

impl Picker {
    pub fn kind(&self) -> HubKind {
        match self.of {
            Of::AiHub(_) => HubKind::Ai,
            Of::Provider => HubKind::AiProvider,
            Of::Model(_) => HubKind::AiModel,
            Of::Effort(..) => HubKind::AiEffort,
            Of::Experts => HubKind::AiExpert,
            Of::Verify => HubKind::Verify,
            Of::Package(_) => HubKind::VerifyPackage,
        }
    }

    /// Rebuilds the rows once the settings revision moves, so a refresh that
    /// lands while a picker is open shows in it.
    pub fn sync(&mut self, view: &SettingsView, ai_label: &str) {
        if view.revision == self.revision {
            return;
        }
        if let Some(next) = build(self.of.clone(), view, ai_label) {
            let selected = self.selected.min(next.rows.len().saturating_sub(1));
            *self = Self { selected, ..next };
        }
    }

    fn step(&mut self, forward: bool) {
        let rows = (0..self.rows.len()).filter(|&at| !self.rows[at].is_header);
        let to = if forward {
            rows.into_iter().find(|&at| at > self.selected)
        } else {
            rows.into_iter().rfind(|&at| at < self.selected)
        };
        if let Some(at) = to {
            self.selected = at;
        }
    }
}

fn row(label: String, hint: &str, description: String, enabled: bool) -> HubItem {
    HubItem {
        label,
        hint: hint.to_string(),
        description,
        is_header: false,
        enabled,
        opens_stack_pr: false,
    }
}

/// A layer row, live by the App's rule, or a row with no route yet.
fn layer_or_unported(label: &str, hint: &str, facts: &HubFacts, unported: &str) -> (HubItem, Pick) {
    match LAYER_ROWS.iter().find(|row| row.0 == label) {
        Some(&(_, description, layer)) => {
            let enabled = facts.has_ai || !matches!(layer, HubRow::Layer(Layer::AiFindings));
            (
                row(label.into(), hint, description.into(), enabled),
                Pick::Layer(layer),
            )
        }
        None => (
            row(label.into(), hint, unported.into(), false),
            Pick::Unported,
        ),
    }
}

/// The expert and Verify rows as the picker lists them.
fn listed(rows: Vec<ai_runs::Row>) -> (Vec<HubItem>, Vec<Pick>) {
    rows.into_iter()
        .map(|listed| {
            let pick = match listed.choice {
                Some(Choice::Start(start)) => Pick::Run(start),
                Some(Choice::Package(id)) => Pick::Package(id),
                None => Pick::Unported,
            };
            let item = HubItem {
                is_header: listed.header,
                ..row(
                    listed.label,
                    &listed.hint,
                    listed.description,
                    listed.enabled,
                )
            };
            (item, pick)
        })
        .unzip()
}

fn build(of: Of, view: &SettingsView, ai_label: &str) -> Option<Picker> {
    let config = &view.config;
    let hub = &config.ai_hub;
    let current = hub.resolve_default_selection(&config.agent);
    let mut selected = 0;
    let mut title = None;
    let (rows, picks): (Vec<HubItem>, Vec<Pick>) = match &of {
        Of::AiHub(facts) => {
            let unported = format!("That action is {UNPORTED}");
            AI_HUB_ROWS
                .iter()
                .flat_map(|entry| match entry {
                    Some(("Review", _)) => flows::rows(&facts.flow)
                        .into_iter()
                        .map(|(label, description, enabled, act)| {
                            (row(label.into(), "", description, enabled), Pick::Flow(act))
                        })
                        .collect(),
                    None => vec![(
                        HubItem {
                            is_header: true,
                            ..row("── Advanced ──".into(), "", String::new(), false)
                        },
                        Pick::Unported,
                    )],
                    Some((CHANGE_SELECTION, hint)) => vec![(
                        row(
                            CHANGE_SELECTION.into(),
                            hint,
                            format!("Current: {ai_label}"),
                            hub.has_presets(),
                        ),
                        Pick::ChangeSelection,
                    )],
                    Some((CLEANUP_DRAFTS, hint)) => vec![(
                        row(
                            CLEANUP_DRAFTS.into(),
                            hint,
                            if facts.has_drafts {
                                "Delete the question and note files"
                            } else {
                                "no questions or notes to clean up"
                            }
                            .into(),
                            facts.has_drafts,
                        ),
                        Pick::Clear(Clear::QuestionsAndNotes),
                    )],
                    Some((CLEANUP_REVIEWS, hint)) => vec![(
                        row(
                            CLEANUP_REVIEWS.into(),
                            hint,
                            if facts.has_review {
                                "Delete review sidecars (review.json, …)"
                            } else {
                                "no review data to clean up"
                            }
                            .into(),
                            facts.has_review,
                        ),
                        Pick::Clear(Clear::Reviews),
                    )],
                    Some((SPECIALIZED, hint)) => vec![(
                        row(SPECIALIZED.into(), hint, SPECIALIZED_ABOUT.into(), true),
                        Pick::Experts,
                    )],
                    Some((label, hint)) => vec![match ai_runs::hub_row(label, facts, ai_label) {
                        Some((description, enabled, start)) => (
                            row((*label).into(), hint, description, enabled),
                            Pick::Run(start),
                        ),
                        None => layer_or_unported(label, hint, facts, &unported),
                    }],
                })
                .unzip()
        }
        Of::Experts => listed(ai_runs::expert_rows()),
        Of::Verify => {
            let rows = ai_runs::verify_rows(config);
            selected = ai_runs::first_selected(&rows);
            listed(rows)
        }
        Of::Package(id) => {
            let (heading, rows) = ai_runs::package_rows(config, id)?;
            selected = ai_runs::first_selected(&rows);
            title = Some(heading);
            listed(rows)
        }
        Of::Provider => {
            let chosen = hub.resolve_provider_id(current.provider_id.as_deref());
            let ids = hub.provider_ids();
            selected = ids
                .iter()
                .position(|id| Some(id) == chosen.as_ref())
                .unwrap_or(0);
            ids.into_iter()
                .filter_map(|id| {
                    let provider = hub.providers.get(&id)?;
                    let count = provider.models.len();
                    let summary = match count {
                        0 => "no model presets".to_string(),
                        1 => "1 model".to_string(),
                        n => format!("{n} models"),
                    };
                    Some((
                        row(provider.display_name(&id), "", summary, true),
                        Pick::Provider(id),
                    ))
                })
                .unzip()
        }
        Of::Model(provider_id) => {
            let provider = hub.providers.get(provider_id)?;
            let chosen = hub.resolve_model_id(provider_id, current.model_id.as_deref());
            let mut built: (Vec<HubItem>, Vec<Pick>) = provider
                .models
                .iter()
                .map(|model| {
                    let description = if model.discovered {
                        format!("{} · discovered", model.id)
                    } else {
                        model.id.clone()
                    };
                    (
                        row(model.display_name(), "", description, true),
                        Pick::Model(provider_id.clone(), model.id.clone()),
                    )
                })
                .unzip();
            selected = provider
                .models
                .iter()
                .position(|model| Some(&model.id) == chosen.as_ref())
                .unwrap_or(0);
            if refreshes(config, provider_id) {
                built.0.push(row(
                    "Refresh models".into(),
                    "↻",
                    "Re-run provider models_command".into(),
                    true,
                ));
                built.1.push(Pick::Refresh(provider_id.clone()));
            }
            built
        }
        Of::Effort(provider_id, model_id) => {
            let model = hub
                .providers
                .get(provider_id)?
                .models
                .iter()
                .find(|model| &model.id == model_id)?;
            let is_current = current.provider_id.as_ref() == Some(provider_id)
                && current.model_id.as_ref() == Some(model_id);
            let mut built: (Vec<HubItem>, Vec<Pick>) = (Vec::new(), Vec::new());
            for (at, level) in model.effort_levels.iter().enumerate() {
                let chosen = is_current && current.effort.as_ref() == Some(level);
                if chosen {
                    selected = at;
                }
                let note = if chosen { "currently selected" } else { "" };
                built
                    .0
                    .push(row(effort_display_label(level), "", note.into(), true));
                let pick = Pick::Effort(provider_id.clone(), model_id.clone(), level.clone());
                built.1.push(pick);
            }
            title = Some(model.display_name());
            built
        }
    };
    Some(Picker {
        of,
        title,
        rows,
        picks,
        selected,
        revision: view.revision,
    })
}

/// Whether the model picker offers a refresh: discovery is on and the
/// provider can list its models.
fn refreshes(config: &ErConfig, provider_id: &str) -> bool {
    config.features.model_discovery
        && config
            .ai_hub
            .providers
            .get(provider_id)
            .is_some_and(|provider| !provider.models_command.is_empty())
}

fn opened(of: Of, view: &SettingsView, ai_label: &str, also: Option<SettingsCommand>) -> Step {
    match build(of, view, ai_label) {
        Some(picker) => Step::Open(Box::new(Overlay::Picker(picker)), also),
        None => Step::Notify("Unknown AI provider".into()),
    }
}

/// The Verify hub over the settings view's `[commands]` and `[packages]`.
pub fn open_verify_hub(view: &SettingsView, ai_label: &str) -> Step {
    opened(Of::Verify, view, ai_label, None)
}

/// The flag-on AI hub: the way to the pickers, the run rows, the layer rows
/// and the Cleanup rows.
pub fn open_ai_hub(view: &SettingsView, ai_label: &str, facts: HubFacts) -> Step {
    opened(Of::AiHub(facts), view, ai_label, None)
}

fn open_providers(view: &SettingsView, ai_label: &str) -> Step {
    if !view.config.ai_hub.has_presets() {
        return Step::Notify("No [ai_hub] providers configured".into());
    }
    opened(Of::Provider, view, ai_label, None)
}

/// Opens a provider's models. Opening asks for a refresh that runs only when
/// the provider's cached listing is stale, as the App's picker does.
fn open_models(view: &SettingsView, ai_label: &str, provider_id: &str) -> Step {
    let Some(provider) = view.config.ai_hub.providers.get(provider_id) else {
        return Step::Notify("Unknown AI provider".into());
    };
    if provider.models.is_empty() {
        return pick(provider_id, None, None);
    }
    let soft = refreshes(&view.config, provider_id).then(|| SettingsCommand::RefreshAiModels {
        provider_id: Some(provider_id.to_string()),
        force: false,
    });
    opened(Of::Model(provider_id.to_string()), view, ai_label, soft)
}

fn open_efforts(view: &SettingsView, ai_label: &str, provider_id: &str, model_id: &str) -> Step {
    if !has_efforts(&view.config.ai_hub, provider_id, model_id) {
        return Step::Notify("This model has no thinking levels".into());
    }
    let of = Of::Effort(provider_id.to_string(), model_id.to_string());
    opened(of, view, ai_label, None)
}

fn has_efforts(hub: &AiHubConfig, provider_id: &str, model_id: &str) -> bool {
    hub.providers.get(provider_id).is_some_and(|provider| {
        provider
            .models
            .iter()
            .any(|model| model.id == model_id && !model.effort_levels.is_empty())
    })
}

fn pick(provider_id: &str, model_id: Option<&str>, effort: Option<&str>) -> Step {
    Step::Pick(SettingsCommand::SetDefaultAiSelection {
        provider_id: provider_id.to_string(),
        model_id: model_id.map(str::to_string),
        effort: effort.map(str::to_string),
    })
}

/// Runs one key on the open picker.
pub fn act(picker: &mut Picker, action: OverlayAction, view: &SettingsView, label: &str) -> Step {
    use OverlayAction as O;
    match action {
        O::Next | O::Prev => picker.step(action == O::Next),
        O::Close | O::GoUp => {
            return match &picker.of {
                Of::Effort(provider_id, _) => {
                    let provider_id = provider_id.clone();
                    open_models(view, label, &provider_id)
                }
                _ if action == O::Close => Step::Close,
                _ => Step::Stay,
            };
        }
        O::SelectRow => return select(picker, view, label),
        _ => {}
    }
    Step::Stay
}

fn select(picker: &Picker, view: &SettingsView, label: &str) -> Step {
    let (Some(row), Some(choice)) = (
        picker.rows.get(picker.selected),
        picker.picks.get(picker.selected),
    ) else {
        return Step::Close;
    };
    if row.is_header {
        return Step::Close;
    }
    match choice {
        Pick::Unported => Step::Unported,
        _ if !row.enabled => Step::Notify(row.description.clone()),
        Pick::Layer(layer) => Step::Layer(*layer),
        Pick::Clear(clear) => Step::Clear(*clear),
        Pick::Run(start) => Step::Run(start.clone()),
        Pick::Flow(act) => Step::Flow(act.clone()),
        Pick::Experts => opened(Of::Experts, view, label, None),
        Pick::Package(id) => match ai_runs::package_rows(&view.config, id) {
            Some(_) => opened(Of::Package(id.clone()), view, label, None),
            None => Step::Notify(format!("Package '{id}' not found in config")),
        },
        Pick::ChangeSelection => open_providers(view, label),
        Pick::Provider(provider_id) => {
            let Some(provider) = view.config.ai_hub.providers.get(provider_id) else {
                return Step::Notify("Unknown AI provider".into());
            };
            let first = provider.models.first().map(|model| model.id.as_str());
            match first {
                _ if provider.models.len() > 1 => open_models(view, label, provider_id),
                Some(model_id) if has_efforts(&view.config.ai_hub, provider_id, model_id) => {
                    open_efforts(view, label, provider_id, model_id)
                }
                _ => pick(provider_id, first, None),
            }
        }
        Pick::Model(provider_id, model_id) => {
            if has_efforts(&view.config.ai_hub, provider_id, model_id) {
                open_efforts(view, label, provider_id, model_id)
            } else {
                pick(provider_id, Some(model_id), None)
            }
        }
        Pick::Effort(provider_id, model_id, effort) => {
            pick(provider_id, Some(model_id), Some(effort))
        }
        Pick::Refresh(provider_id) => Step::Leave(SettingsCommand::RefreshAiModels {
            provider_id: Some(provider_id.clone()),
            force: true,
        }),
    }
}
