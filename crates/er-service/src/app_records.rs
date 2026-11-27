//! The app records and their owner: the projects record, the inbox, the PR
//! cache and the GitHub status cache, and the one owner that publishes the
//! sidebar's resources from them. They cover the whole app, so they live
//! here beside `AppRegistry` (ADR 0070), and only the desktop composes them.

pub mod app_owner;
pub mod gh_status_cache;
pub mod inbox;
pub mod meta_refresh;
pub mod pr_cache;
pub mod projects;
pub mod sidebar;

pub use app_owner::{AppOwner, ChangeHook, SidebarOwner};

/// Registers the three app records the store keeps for the desktop alone.
/// The shared composition, which the TUI and MCP use, leaves them out.
pub fn register_desktop_kinds(
    registry: &mut er_store::Registry,
) -> Result<(), er_store::StoreError> {
    gh_status_cache::register(registry)?;
    inbox::register(registry)?;
    pr_cache::register(registry)?;
    Ok(())
}

#[cfg(test)]
mod tests;
