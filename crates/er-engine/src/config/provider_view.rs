//! The provider list and the config hub view the Settings page reads.
//!
//! Built from a config alone, so the flag-off desktop and the settings actor
//! produce the same rows from the same code.

use super::settings::{settings_fields_grouped, SettingsFieldsGrouped};
use super::{AiHubConfig, ErConfig, ReviewModelGroups, KNOWN_FAMILY_IDS};

#[derive(Debug, Clone, serde::Serialize)]
pub struct AiProviderInfo {
    pub id: String,
    pub label: String,
    pub models: Vec<AiModelInfo>,
    pub is_selected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    pub has_models_command: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub models_command: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AiModelInfo {
    pub id: String,
    pub label: String,
    pub is_selected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_per_1k_in: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_per_1k_out: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_latency_ms: Option<u32>,
    pub effort_levels: Vec<String>,
    pub discovered: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<String>,
}

/// Map hub providers to wire DTOs. `selected_*` are already-resolved highlight ids
/// (live current selection for the palette and title bar; persisted defaults for Settings).
pub fn map_ai_providers(
    hub: &AiHubConfig,
    selected_provider_id: Option<&str>,
    selected_model_id: Option<&str>,
) -> Vec<AiProviderInfo> {
    hub.providers
        .iter()
        .map(|(id, cfg)| {
            let is_selected = selected_provider_id == Some(id.as_str());
            AiProviderInfo {
                id: id.clone(),
                label: cfg.display_name(id),
                is_selected,
                family: cfg
                    .family
                    .clone()
                    .or_else(|| cfg.cli_family().id().map(|s| s.to_string())),
                has_models_command: !cfg.models_command.is_empty(),
                command: Some(cfg.command.clone()),
                args: Some(cfg.args.join(" ")),
                models_command: (!cfg.models_command.is_empty())
                    .then(|| cfg.models_command.join(" ")),
                models: cfg
                    .models
                    .iter()
                    .map(|m| AiModelInfo {
                        id: m.id.clone(),
                        label: m.display_name(),
                        is_selected: is_selected && selected_model_id == Some(m.id.as_str()),
                        description: m.description.clone(),
                        cost_per_1k_in: m.cost_per_1k_in,
                        cost_per_1k_out: m.cost_per_1k_out,
                        avg_latency_ms: m.avg_latency_ms,
                        effort_levels: m.effort_levels.clone(),
                        discovered: m.discovered,
                        args: Some(m.args.join(" ")),
                    })
                    .collect(),
            }
        })
        .collect()
}

/// Everything the Settings page reads from the config: the form rows and the
/// provider editor's lists, with the persisted defaults highlighted.
#[derive(Debug, Clone)]
pub struct ConfigHubView {
    pub fields: SettingsFieldsGrouped,
    pub providers: Vec<AiProviderInfo>,
    pub review_models: ReviewModelGroups,
    pub active_effort: Option<String>,
    /// Known `family =` ids for the provider editor (plus empty = detect).
    pub family_options: Vec<String>,
}

pub fn config_hub_view(config: &ErConfig) -> ConfigHubView {
    let hub = &config.ai_hub;
    let selection = hub.resolve_default_selection(&config.agent);
    ConfigHubView {
        fields: settings_fields_grouped(config),
        providers: map_ai_providers(
            hub,
            selection.provider_id.as_deref(),
            selection.model_id.as_deref(),
        ),
        review_models: hub.review_models.clone(),
        active_effort: selection.effort,
        family_options: KNOWN_FAMILY_IDS.iter().map(|s| (*s).to_string()).collect(),
    }
}
