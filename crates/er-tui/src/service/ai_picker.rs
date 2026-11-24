//! The flag-on AI hub and the provider, model and effort pickers, built from
//! the settings view and rebuilt when its revision moves. A pick saves the
//! default selection through the settings actor (decision 11), and a model
//! refresh is the actor's `RefreshAiModels`.
//!
//! Until P74 ports the AI hub, its one live row is "Change agent/model": no
//! other row has a service route on this path yet.

use er_engine::config::{effort_display_label, AiHubConfig, ErConfig};
use er_service::{SettingsCommand, SettingsView};

use super::config_hub::Step;
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
    Some(("Specialized review", "")),
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
    Some(("Cleanup questions & notes", "z")),
    Some(("Cleanup reviews", "Z")),
];

const CHANGE_SELECTION: &str = "Change agent/model";

/// Which list a picker shows, so it can be built again from a newer view.
#[derive(Clone)]
enum Of {
    AiHub,
    Provider,
    Model(String),
    Effort(String, String),
}

/// What choosing a row does.
#[derive(Clone)]
enum Pick {
    Unported,
    ChangeSelection,
    Provider(String),
    Model(String, String),
    Effort(String, String, String),
    Refresh(String),
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
            Of::AiHub => HubKind::Ai,
            Of::Provider => HubKind::AiProvider,
            Of::Model(_) => HubKind::AiModel,
            Of::Effort(..) => HubKind::AiEffort,
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

fn build(of: Of, view: &SettingsView, ai_label: &str) -> Option<Picker> {
    let config = &view.config;
    let hub = &config.ai_hub;
    let current = hub.resolve_default_selection(&config.agent);
    let mut selected = 0;
    let mut title = None;
    let (rows, picks): (Vec<HubItem>, Vec<Pick>) = match &of {
        Of::AiHub => {
            let unported = format!("That action is {UNPORTED}");
            AI_HUB_ROWS
                .iter()
                .map(|entry| match entry {
                    None => (
                        HubItem {
                            is_header: true,
                            ..row("── Advanced ──".into(), "", String::new(), false)
                        },
                        Pick::Unported,
                    ),
                    Some((CHANGE_SELECTION, hint)) => (
                        row(
                            CHANGE_SELECTION.into(),
                            hint,
                            format!("Current: {ai_label}"),
                            hub.has_presets(),
                        ),
                        Pick::ChangeSelection,
                    ),
                    Some((label, hint)) => (
                        row((*label).into(), hint, unported.clone(), false),
                        Pick::Unported,
                    ),
                })
                .unzip()
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

/// The flag-on AI hub, the way to the pickers before P74.
pub fn open_ai_hub(view: &SettingsView, ai_label: &str) -> Step {
    opened(Of::AiHub, view, ai_label, None)
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
