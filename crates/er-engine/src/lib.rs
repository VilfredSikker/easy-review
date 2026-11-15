// The engine runs under the TUI's alternate screen and inside the desktop app,
// so a print to stdout paints over the TUI. Report through dev_log or a Result.
#![deny(clippy::print_stdout)]

pub mod ai;
#[cfg(feature = "ui")]
pub mod app;
pub mod arena;
pub mod config;
pub use config::{
    apply_config_field, config_hub_items_for_scope, desktop_settings_snapshot, ConfigFieldValue,
    ConfigHubFieldDto, DesktopSettingsSnapshot, SettingsScope,
};
pub mod agent_runtime;
pub mod annotations;
pub mod dev_log;
pub mod diagram_upload;
pub mod diff_fetch;
pub mod env_path;
#[cfg(feature = "ui")]
pub mod export;
pub use er_github::actions as gh_pr_actions;
mod env_var;
pub mod gh_stack;
pub mod git;
pub mod github;
pub mod github_post;
pub mod github_post_session;
#[cfg(feature = "highlight")]
pub mod highlight;
pub mod jobs;
mod legacy_projects;
pub mod model_discovery;
pub mod os_launch;
pub mod paths;
pub mod ports;
pub mod pr_open_cache;
pub mod pr_open_entry;
pub mod pr_resolve;
pub mod pr_review_feedback;
pub mod projects_pins;
pub mod release_install;
pub mod release_update;
pub mod remote_pr_open_cache;
pub mod review_queue;
pub mod session_store;
pub mod sidecar_summary;
pub mod sidecar_upload;
pub mod storage;
pub mod store_registry;
pub mod sync;
pub mod tab_key;
pub use tab_key::TabKey;
pub mod tabs_record;
pub mod uninstall;
#[cfg(feature = "watch")]
pub mod watch;
pub mod workflow_recovery;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

pub use paths::ErRoot;
