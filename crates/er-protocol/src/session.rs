use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The app chrome that belongs to no tab: the AI pick's labels, the config
/// load error, the active-branch watcher, the app version, the last update
/// check and the reviewer list. The settings actor owns the first two, the
/// desktop the rest.
///
/// Field names follow the snapshot's, so the flag-off and flag-on reads have
/// one shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Session {
    /// The saved default selection as one label: provider, model and effort.
    pub active_ai_label: String,
    pub active_ai_provider_label: Option<String>,
    pub active_ai_model_label: Option<String>,
    pub active_ai_effort: Option<String>,
    /// Why the config file failed to load; saves refuse until it reloads.
    pub config_load_error: Option<String>,
    pub app_version: String,
    pub watch_status: WatchStatus,
    /// The last update check's answer; none until one finished.
    pub update: Option<AppUpdate>,
    /// The specialized reviewers, in the order the picker lists them.
    pub reviewers: Vec<Reviewer>,
}

/// The active-branch watcher: whether it follows a checkout, and which.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct WatchStatus {
    pub active: bool,
    pub branch: Option<String>,
    pub root_path: Option<String>,
}

/// The running build compared with the latest release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AppUpdate {
    pub current: String,
    /// None when the release could not be read.
    pub latest: Option<String>,
    pub update_available: bool,
    pub release_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Reviewer {
    pub kind: String,
    pub label: String,
    pub description: String,
}

/// Which side panels are open. The page paints its panels itself; this is
/// the backend's mirror of each toggle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Panels {
    pub left: bool,
    pub tree: bool,
    pub right: bool,
}

/// One side panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum Panel {
    Left,
    Tree,
    Right,
}
