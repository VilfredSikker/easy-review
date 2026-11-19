use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// What the Settings page shows: the form rows, the provider editor's lists
/// and the AI defaults, as the settings actor last published them.
///
/// Field names follow the page's existing reads, so the flag-off and flag-on
/// answers have one shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub settings: SettingsRows,
    /// Every provider, the saved default selection marked.
    pub providers: Vec<AiProvider>,
    pub review_models: ReviewModelGroups,
    /// The saved default effort; none is Auto.
    pub active_effort: Option<String>,
    /// Known `family =` ids for the provider editor.
    pub family_options: Vec<String>,
    /// What an uninstall would remove, once a preview was asked for.
    pub uninstall_preview: Option<UninstallPreview>,
}

/// The form rows by page section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SettingsRows {
    pub general: Vec<ConfigHubField>,
    pub app: Vec<ConfigHubField>,
    pub terminal: Vec<ConfigHubField>,
}

/// One row of the Settings form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ConfigHubField {
    Section {
        title: String,
    },
    Bool {
        key: String,
        label: String,
        description: String,
        value: bool,
    },
    Cycle {
        key: String,
        label: String,
        description: String,
        options: Vec<String>,
        value: String,
    },
    Text {
        key: String,
        label: String,
        description: String,
        placeholder: String,
        value: String,
        strict: bool,
    },
    ListEntry {
        key: String,
        label: String,
        index: usize,
    },
    ListAdd {
        key: String,
        label: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct AiProvider {
    pub id: String,
    pub label: String,
    pub models: Vec<AiModel>,
    pub is_selected: bool,
    pub family: Option<String>,
    pub has_models_command: bool,
    pub command: Option<String>,
    pub args: Option<String>,
    pub models_command: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct AiModel {
    pub id: String,
    pub label: String,
    pub is_selected: bool,
    pub description: Option<String>,
    pub cost_per_1k_in: Option<f32>,
    pub cost_per_1k_out: Option<f32>,
    pub avg_latency_ms: Option<u32>,
    /// Empty means the provider's default.
    pub effort_levels: Vec<String>,
    /// Listed by the provider's CLI rather than written in the config.
    pub discovered: bool,
    pub args: Option<String>,
}

/// The provider and model pairs each review-flow stage may run, in the order
/// assignment rotates through them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ReviewModelGroups {
    pub fast: Vec<ReviewModelRef>,
    pub standard: Vec<ReviewModelRef>,
    pub advanced: Vec<ReviewModelRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ReviewModelRef {
    pub provider_id: String,
    pub model_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct UninstallPreview {
    pub targets: Vec<UninstallTarget>,
    pub existing_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct UninstallTarget {
    pub kind: String,
    pub path: String,
    pub exists: bool,
    pub description: String,
}

/// One config field's new value. The patch names the field; its value is one
/// scalar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(untagged)]
pub enum ConfigFieldValue {
    Bool(bool),
    String(String),
    // JSON over IPC delivers a number, never a BigInt; config numbers are small.
    Number(#[ts(type = "number")] u64),
}

/// A provider as the editor saves it; `original_id` names it before a rename.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUpsert {
    pub id: String,
    pub original_id: Option<String>,
    pub label: Option<String>,
    pub command: String,
    pub args: String,
    pub family: Option<String>,
    pub models_command: Option<String>,
}

/// A model as the editor saves it; `original_id` names it before a rename.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModelUpsert {
    pub id: String,
    pub original_id: Option<String>,
    pub label: Option<String>,
    pub description: Option<String>,
    pub args: String,
    pub effort_levels: Vec<String>,
    pub cost_per_1k_in: Option<f32>,
    pub cost_per_1k_out: Option<f32>,
    pub avg_latency_ms: Option<u32>,
}

/// Which categories an uninstall removes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct UninstallOptions {
    pub remove_config: bool,
    pub remove_data: bool,
    pub remove_cache: bool,
    pub remove_binaries: bool,
    pub remove_desktop_app: bool,
}
