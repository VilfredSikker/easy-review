use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ResourceChanged;

/// A command the frontend sends to the backend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Command {}

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
