use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ResourceChanged;

/// A command the frontend sends to the backend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Command {
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
