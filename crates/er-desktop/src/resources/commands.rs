//! The three resource commands. The default build registers them too, as
//! every flag-on command has a flag-off twin, and answers each as
//! unsupported.

use er_protocol::{Ack, Command, ErError, Resource, ResourceKey, ResourceVersion, Versioned};
#[cfg(feature = "v1-service")]
use tauri::State;

#[cfg(feature = "v1-service")]
use crate::state::DesktopState;

/// Applies one command and names the versions it moved. No command is on the
/// wire yet: each joins with the entry that ports its store.
#[cfg(feature = "v1-service")]
#[tauri::command]
pub fn er_send(command: Command, state: State<'_, DesktopState>) -> Result<Ack, ErError> {
    let _ = state;
    match command {}
}

/// Each key's content at its published version, in the order asked.
#[cfg(feature = "v1-service")]
#[tauri::command]
pub fn er_fetch(
    keys: Vec<ResourceKey>,
    state: State<'_, DesktopState>,
) -> Result<Vec<Versioned<Resource>>, ErError> {
    keys.into_iter()
        .map(|key| super::fetch(&state, key))
        .collect()
}

/// Each key's published version, so a listener that attached late learns
/// what it missed without fetching the content.
#[cfg(feature = "v1-service")]
#[tauri::command]
pub fn er_versions(
    keys: Vec<ResourceKey>,
    state: State<'_, DesktopState>,
) -> Result<Vec<ResourceVersion>, ErError> {
    keys.into_iter()
        .map(|key| super::version(&state, key))
        .collect()
}

#[cfg(not(feature = "v1-service"))]
#[tauri::command]
pub fn er_send(command: Command) -> Result<Ack, ErError> {
    match command {}
}

#[cfg(not(feature = "v1-service"))]
#[tauri::command]
pub fn er_fetch(keys: Vec<ResourceKey>) -> Result<Vec<Versioned<Resource>>, ErError> {
    Err(unsupported(&keys))
}

#[cfg(not(feature = "v1-service"))]
#[tauri::command]
pub fn er_versions(keys: Vec<ResourceKey>) -> Result<Vec<ResourceVersion>, ErError> {
    Err(unsupported(&keys))
}

/// The default build serves no resource.
#[cfg(not(feature = "v1-service"))]
fn unsupported(keys: &[ResourceKey]) -> ErError {
    ErError {
        kind: er_protocol::ErErrorKind::Unsupported,
        message: format!("this build serves no resources (asked for {keys:?})"),
    }
}

#[cfg(all(test, not(feature = "v1-service")))]
mod tests {
    use er_protocol::{ErErrorKind, ResourceKey};

    /// The default build answers a fetch and a version check as unsupported,
    /// so a frontend built for the other path fails loudly.
    #[test]
    fn the_default_build_serves_no_resource() {
        let fetched = super::er_fetch(vec![ResourceKey::Tabs]);
        assert_eq!(fetched.map_err(|e| e.kind), Err(ErErrorKind::Unsupported));
        let versions = super::er_versions(vec![ResourceKey::Tabs]);
        assert_eq!(versions.map_err(|e| e.kind), Err(ErErrorKind::Unsupported));
    }
}
