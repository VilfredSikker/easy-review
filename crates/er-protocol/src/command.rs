use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::settings::{
    ConfigFieldValue, ModelUpsert, ProviderUpsert, ReviewModelGroups, UninstallOptions,
};
use crate::{Panel, ResourceChanged};

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
    /// Reads one repo's committed risk rules and resolves `paths` against
    /// them. The `Ack` names the rules key as `created`, and `risk(key)`
    /// holds the result. `remote_repo` (`owner/repo`) is for a target with no
    /// clone.
    LoadRisk {
        repo_root: String,
        remote_repo: Option<String>,
        paths: Vec<String>,
    },
    /// Saves the default AI provider and model, and the effort when one is
    /// named. Every pick saves: there is no session-only pick on the wire.
    SetAiSelection {
        provider_id: String,
        model_id: Option<String>,
        effort: Option<String>,
    },
    /// Saves the default effort for the default selection; none is Auto.
    SetAiEffort {
        effort: Option<String>,
    },
    /// Compares the running build with the latest release. The `Ack` comes
    /// at once; the answer lands in `session`.
    CheckAppUpdate,
    /// Mirrors a side panel the page toggled into `panels`.
    TogglePanel {
        panel: Panel,
    },
    /// Puts the projects in this order. Ids it does not name keep their order
    /// after the named ones; an empty list changes nothing.
    ReorderProjects {
        ordered: Vec<String>,
    },
    DeleteProject {
        project: String,
    },
    /// Lists a local branch under its project.
    AddTrackedBranch {
        project: String,
        name: String,
    },
    /// Hides a branch from its project's list. The checked-out branch stays.
    RemoveTrackedBranch {
        project: String,
        name: String,
    },
    PatchProjectReviewSettings {
        project: String,
        patch: ProjectReviewSettingsPatch,
    },
    /// Fetches every project remote's pull requests in the background; the
    /// push brings the new `prs` versions. A refresh already running absorbs it.
    RefreshPrList,
    /// Fetches one project's remote in the background; a project with no
    /// remote changes nothing.
    RefreshProjectPrList {
        project: String,
    },
    /// Hides a pull request from the project's lists.
    DismissPr {
        project: String,
        #[ts(type = "number")]
        number: u64,
    },
    UndismissPr {
        project: String,
        #[ts(type = "number")]
        number: u64,
    },
    /// Tracks a pull request, which also undoes its dismissal.
    TrackPr {
        project: String,
        #[ts(type = "number")]
        number: u64,
    },
    UntrackPr {
        project: String,
        #[ts(type = "number")]
        number: u64,
    },
    /// Saves a pull request at the front of the project's saved list, which
    /// keeps the newest 50. With no title the cached one is used.
    SavePr {
        project: String,
        #[ts(type = "number")]
        number: u64,
        title: Option<String>,
    },
    UnsavePr {
        project: String,
        #[ts(type = "number")]
        number: u64,
    },
}

/// The review settings a patch changes; a field left empty stays as it is.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct ProjectReviewSettingsPatch {
    pub auto_triage_max_diff_kb: Option<u32>,
    pub review_ignore_glob_add: Option<String>,
    /// The index of the glob to remove.
    pub review_ignore_glob_remove: Option<u32>,
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
