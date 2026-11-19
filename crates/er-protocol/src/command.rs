use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::settings::{
    ConfigFieldValue, ModelUpsert, ProviderUpsert, ReviewModelGroups, UninstallOptions,
};
use crate::ResourceChanged;

/// A command the frontend sends to the backend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum Command {
    /// Settings opened: adopt a config file repaired after a failed load.
    ReloadSettings,
    /// Writes one config field, then saves.
    ApplyConfigPatch {
        key: String,
        value: ConfigFieldValue,
    },
    SetReviewFlowModels {
        groups: ReviewModelGroups,
    },
    /// Lists models from the provider CLIs into `settings`; never saved.
    /// `force` skips a fresh cache.
    RefreshAiModels {
        provider_id: Option<String>,
        force: bool,
    },
    UpsertAiProvider {
        provider: ProviderUpsert,
    },
    DeleteAiProvider {
        provider_id: String,
    },
    UpsertAiModel {
        provider_id: String,
        model: ModelUpsert,
    },
    DeleteAiModel {
        provider_id: String,
        model_id: String,
    },
    /// Lists what an uninstall would remove into `settings`.
    PreviewUninstall {
        options: UninstallOptions,
    },
    /// Removes what the options name; the app quits once it is done.
    RunUninstall {
        options: UninstallOptions,
    },
}

/// A command was applied.
///
/// It names the versions the command moved, so the frontend refetches them
/// as soon as the answer arrives instead of waiting for the push.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Ack {
    pub changed: Vec<ResourceChanged>,
    /// The id of what a create command made; empty for every other command.
    pub created: Option<String>,
}
