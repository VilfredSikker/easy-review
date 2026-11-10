//! What to run: the provider program and its fixed inputs.
//!
//! The engine resolves a provider, model and effort into these fields; the
//! runtime only spawns what it is given. Keeping the type here is what lets
//! `er-agent` stay free of the engine's config, AI and app modules.

use crate::decode::OutputProtocol;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSpec {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub protocol: OutputProtocol,
    pub timeout: Duration,
}
