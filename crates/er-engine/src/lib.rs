pub mod agent_slots;
pub mod ai;
#[cfg(feature = "ui")]
pub mod app;
#[cfg(feature = "ui")]
pub mod arena;
pub mod cache;
pub mod config;
#[allow(unused_imports)]
pub use config::{
    apply_config_field, config_hub_items_for_scope, desktop_settings_snapshot, ConfigFieldValue,
    ConfigHubFieldDto, DesktopSettingsSnapshot, SettingsScope,
};
pub mod dev_log;
pub mod env_path;
pub mod git;
pub mod github;
#[cfg(feature = "highlight")]
pub mod highlight;
pub mod paths;
pub mod storage;
pub mod sync;
#[cfg(feature = "watch")]
pub mod watch;

pub use paths::ErRoot;
