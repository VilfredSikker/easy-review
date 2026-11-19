use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::settings::{
    ConfigFieldValue, ModelUpsert, ProviderUpsert, ReviewModelGroups, UninstallOptions,
};
use crate::{Panel, PrOpenHint, ResourceChanged, TabKey};

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
    /// Opens a Working tab on the active tab's repo.
    NewTab,
    /// Closes a tab and drops its content; the last tab stays.
    CloseTab {
        key: TabKey,
    },
    SelectTab {
        key: TabKey,
    },
    /// Moves a tab to `to_idx`; the active tab stays active.
    ReorderTabs {
        key: TabKey,
        to_idx: u32,
    },
    /// Opens a project's PR from its clone. A PR that is already open is
    /// selected. With `replace` the new tab takes the active tab's place.
    OpenPrReview {
        project_id: String,
        #[ts(type = "number")]
        pr_number: u64,
        replace: bool,
    },
    /// Opens a PR with no clone.
    OpenRemotePr {
        owner: String,
        repo: String,
        #[ts(type = "number")]
        number: u64,
        replace: bool,
    },
    /// Opens a pasted GitHub PR link, from a project's clone when one tracks
    /// its repo.
    OpenPrUrl {
        url: String,
        replace: bool,
    },
    /// Opens a branch of a project's checkout.
    OpenLocalBranch {
        project_id: String,
        name: String,
        replace: bool,
    },
    /// Opens a branch of a project's checkout and makes the project active.
    OpenProjectBranch {
        project_id: String,
        branch: String,
        replace: bool,
    },
    /// Asks for a folder and opens a Working tab on it. A cancelled pick
    /// changes nothing.
    OpenWorktree,
    /// Opens a Working tab on the project's root and makes it active.
    SetActiveProject {
        project_id: String,
    },
    /// Warms the open cache for a project's PR. The `Ack` comes at once.
    PrefetchPrOpen {
        project_id: String,
        #[ts(type = "number")]
        pr_number: u64,
        hint: PrOpenHint,
    },
    /// Warms the open cache for a PR with no clone. The `Ack` comes at once.
    PrefetchRemotePrOpen {
        owner: String,
        repo: String,
        #[ts(type = "number")]
        number: u64,
    },
    /// Writes the active tab's review browser; a field left out stays.
    /// `navigate` also loads `url` in the tab's webview.
    UpdateTabBrowser {
        layout: Option<crate::BrowserLayout>,
        url: Option<String>,
        navigate: bool,
        annotate: Option<bool>,
        tooltips: Option<bool>,
        split_ratio: Option<f32>,
    },
    /// Hidden, split, fullscreen, hidden: the active tab's browser layout.
    CycleTabBrowserLayout,
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
