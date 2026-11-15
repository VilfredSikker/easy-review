//! What the Settings page reads: the form rows and the provider editor's
//! lists, built from one `ConfigHubView`. The flag-off desktop builds that view
//! from `App.config`; the flag-on desktop reads the settings actor's published
//! one (ADR 0069). Risk rules are their own pair of commands
//! (`risk_commands`), since they belong to a repo and the rows do not.

use er_engine::config::{AiProviderInfo, ConfigHubFieldDto, ConfigHubView, ReviewModelGroups};

/// The form rows by page section.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSnapshot {
    pub general: Vec<ConfigHubFieldDto>,
    pub app: Vec<ConfigHubFieldDto>,
    pub terminal: Vec<ConfigHubFieldDto>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetConfigHubResponse {
    pub settings: SettingsSnapshot,
    pub providers: Vec<AiProviderInfo>,
    pub review_models: ReviewModelGroups,
    pub active_effort: Option<String>,
    /// Soft validation messages from the last upsert (e.g. missing `{prompt}`).
    #[serde(default)]
    pub warnings: Vec<String>,
    /// Known `family =` ids for the provider editor (plus empty = detect).
    pub family_options: Vec<String>,
}

impl GetConfigHubResponse {
    pub fn from_view(view: ConfigHubView, warnings: Vec<String>) -> Self {
        Self {
            settings: SettingsSnapshot {
                general: view.fields.general,
                app: view.fields.app,
                terminal: view.fields.terminal,
            },
            providers: view.providers,
            review_models: view.review_models,
            active_effort: view.active_effort,
            warnings,
            family_options: view.family_options,
        }
    }
}
