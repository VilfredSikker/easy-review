pub mod agent_slots;
pub mod ai;
#[cfg(feature = "ui")]
pub mod app;
#[cfg(feature = "ui")]
pub mod arena;
pub mod config;
#[allow(unused_imports)]
pub use config::{
    apply_config_field, config_hub_items_for_scope, desktop_settings_snapshot, ConfigFieldValue,
    ConfigHubFieldDto, DesktopSettingsSnapshot, SettingsScope,
};
pub mod agent_runtime;
pub mod dev_log;
pub mod diagram_upload;
pub mod env_path;
#[cfg(feature = "ui")]
pub mod export;
pub mod git;
pub mod github;
#[cfg(feature = "highlight")]
pub mod highlight;
pub mod model_discovery;
pub mod paths;
pub mod pr_resolve;
pub mod pr_review_feedback;
pub mod projects_pins;
pub mod review_queue;
pub mod sidecar_specs;
pub mod sidecar_summary;
pub mod sidecar_upload;
pub mod storage;
pub mod sync;
pub mod uninstall;
#[cfg(feature = "watch")]
pub mod watch;

pub use paths::ErRoot;
