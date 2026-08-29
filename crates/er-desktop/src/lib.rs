// Deliberate style decisions (same rationale as er-engine):
// - option_if_let_else: if-let chains read better than map_or for these branches
// - significant_drop_tightening: the App mutex guard is deliberately held across
//   state work inside commands; dropping it earlier would be churn with no IO win
#![allow(clippy::option_if_let_else, clippy::significant_drop_tightening)]

pub mod arena_commands;
pub mod auto_triage;
pub mod browser_webview;
pub mod commands;
pub mod config_commands;
pub mod dev_log;
pub mod er_storage;
pub mod export;
pub mod frame_script;
pub mod gh_status_cache;
pub mod inbox;
pub mod native_notify;
mod persist;
pub mod pr_cache;
pub mod pr_open_cache;
pub mod profile_log;
pub mod projects;
pub mod remote_pr_open_cache;
pub mod snapshot;
pub mod tabs;
pub mod terminal;
