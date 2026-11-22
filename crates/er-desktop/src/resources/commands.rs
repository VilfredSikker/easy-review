//! The three resource commands. The default build registers them too, as
//! every flag-on command has a flag-off twin, and answers each as
//! unsupported.

use er_protocol::{Ack, Command, ErError, Resource, ResourceKey, ResourceVersion, Versioned};
#[cfg(feature = "v1-service")]
use tauri::State;

#[cfg(feature = "v1-service")]
use crate::state::DesktopState;

/// Applies one command and names the versions it moved. Off the main thread:
/// a settings write waits on the actor and its save, and a sidebar command
/// may call git.
#[cfg(feature = "v1-service")]
#[tauri::command]
pub async fn er_send(
    command: Command,
    app: tauri::AppHandle,
    state: State<'_, DesktopState>,
    pick: State<'_, crate::ai_selection_service::SessionPick>,
) -> Result<Ack, ErError> {
    let desktop = state.inner().clone();
    let pick = pick.inner().clone();
    let emitter = app.clone();
    let applied = blocking(move || {
        let notify = |level, message| {
            super::notice::emit(&emitter, &super::notice::notice(level, message));
        };
        let effects = super::send::Effects {
            notify: &notify,
            follow_selection: &|selection| pick.follow(selection),
            check_update: || super::session::app_update(crate::commands::app_update_cached()),
            webviews: &emitter,
            pick_folder: &pick_folder,
        };
        super::send::apply(&desktop, command, &effects)
    })
    .await?;
    match applied.uninstalled {
        Some(result) => crate::config_commands::finish_uninstall(&app, result)
            .map(|_| applied.ack)
            .map_err(|message| ErError {
                kind: er_protocol::ErErrorKind::Refused,
                message,
            }),
        None => Ok(applied.ack),
    }
}

/// Each key's content at its published version, in the order asked. Off the
/// main thread: the first `settings` read starts the settings actor, which
/// loads the config.
#[cfg(feature = "v1-service")]
#[tauri::command]
pub async fn er_fetch(
    keys: Vec<ResourceKey>,
    state: State<'_, DesktopState>,
) -> Result<Vec<Versioned<Resource>>, ErError> {
    let desktop = state.inner().clone();
    blocking(move || super::fetch_all(&desktop, keys)).await
}

/// Each key's published version, so a listener that attached late learns
/// what it missed without fetching the content.
#[cfg(feature = "v1-service")]
#[tauri::command]
pub async fn er_versions(
    keys: Vec<ResourceKey>,
    state: State<'_, DesktopState>,
) -> Result<Vec<ResourceVersion>, ErError> {
    let desktop = state.inner().clone();
    blocking(move || {
        keys.into_iter()
            .map(|key| super::version(&desktop, key))
            .collect()
    })
    .await
}

#[cfg(feature = "v1-service")]
fn pick_folder() -> Option<String> {
    rfd::FileDialog::new()
        .set_title("Select a git repository")
        .pick_folder()
        .map(|path| path.to_string_lossy().into_owned())
}

#[cfg(feature = "v1-service")]
async fn blocking<T, F>(f: F) -> Result<T, ErError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, ErError> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| ErError {
            kind: er_protocol::ErErrorKind::Internal,
            message: format!("blocking task failed: {e}"),
        })?
}

#[cfg(not(feature = "v1-service"))]
#[tauri::command]
pub fn er_send(command: Command) -> Result<Ack, ErError> {
    Err(unsupported(&format!("{command:?}")))
}

#[cfg(not(feature = "v1-service"))]
#[tauri::command]
pub fn er_fetch(keys: Vec<ResourceKey>) -> Result<Vec<Versioned<Resource>>, ErError> {
    Err(unsupported(&format!("{keys:?}")))
}

#[cfg(not(feature = "v1-service"))]
#[tauri::command]
pub fn er_versions(keys: Vec<ResourceKey>) -> Result<Vec<ResourceVersion>, ErError> {
    Err(unsupported(&format!("{keys:?}")))
}

/// The default build serves no resource and applies no wire command.
#[cfg(not(feature = "v1-service"))]
fn unsupported(asked: &str) -> ErError {
    ErError {
        kind: er_protocol::ErErrorKind::Unsupported,
        message: format!("this build serves no resources (asked for {asked})"),
    }
}

#[cfg(all(test, not(feature = "v1-service")))]
mod tests {
    use er_protocol::{Command, ErErrorKind, ResourceKey};

    /// The default build answers a fetch, a version check and a command as
    /// unsupported, so a frontend built for the other path fails loudly.
    #[test]
    fn the_default_build_serves_no_resource() {
        let fetched = super::er_fetch(vec![ResourceKey::Tabs]);
        assert_eq!(fetched.map_err(|e| e.kind), Err(ErErrorKind::Unsupported));
        let versions = super::er_versions(vec![ResourceKey::Tabs]);
        assert_eq!(versions.map_err(|e| e.kind), Err(ErErrorKind::Unsupported));
        let sent = super::er_send(Command::DeleteProject {
            project: "p".into(),
        });
        assert_eq!(sent.map_err(|e| e.kind), Err(ErErrorKind::Unsupported));
        let sent = super::er_send(Command::ReloadSettings);
        assert_eq!(sent.map_err(|e| e.kind), Err(ErErrorKind::Unsupported));
        let sent = super::er_send(Command::SavePr {
            project: "p".into(),
            number: 1,
            title: None,
        });
        assert_eq!(sent.map_err(|e| e.kind), Err(ErErrorKind::Unsupported));
        let fetched = super::er_fetch(vec![ResourceKey::Prs {
            remote: "acme/p".into(),
        }]);
        assert_eq!(fetched.map_err(|e| e.kind), Err(ErErrorKind::Unsupported));
        let sent = super::er_send(Command::MarkInboxItemRead { id: "i".into() });
        assert_eq!(sent.map_err(|e| e.kind), Err(ErErrorKind::Unsupported));
        let fetched = super::er_fetch(vec![ResourceKey::Inbox]);
        assert_eq!(fetched.map_err(|e| e.kind), Err(ErErrorKind::Unsupported));
    }
}
