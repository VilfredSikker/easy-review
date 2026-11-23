//! The three resource commands. The default build registers them too, as
//! every flag-on command has a flag-off twin, and answers each as
//! unsupported.

use er_protocol::{Ack, Command, ErError, Resource, ResourceKey, ResourceVersion, Versioned};
use tauri::State;

use crate::state::DesktopState;

/// Applies one command and names the versions it moved. Off the main thread:
/// a settings write waits on the actor and its save, and a sidebar command
/// may call git.
#[tauri::command]
pub async fn er_send(
    command: Command,
    app: tauri::AppHandle,
    state: State<'_, DesktopState>,
) -> Result<Ack, ErError> {
    let desktop = state.inner().clone();
    let emitter = app.clone();
    let applied = blocking(move || {
        let notify = |level, message| {
            super::notice::emit(&emitter, &super::notice::notice(level, message));
        };
        let effects = super::send::Effects {
            notify: &notify,
            check_update: || super::session::app_update(crate::commands::app_update_cached()),
            webviews: &emitter,
            pick_folder: &pick_folder,
        };
        super::send::apply(&desktop, command, &effects)
    })
    .await?;
    match applied.uninstalled {
        Some(result) => finish_uninstall(&app, result)
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

fn pick_folder() -> Option<String> {
    rfd::FileDialog::new()
        .set_title("Select a git repository")
        .pick_folder()
        .map(|path| path.to_string_lossy().into_owned())
}

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

/// Quits after an uninstall that asked for it, and reports what failed.
fn finish_uninstall(
    app_handle: &tauri::AppHandle,
    result: er_engine::uninstall::UninstallResult,
) -> Result<er_engine::uninstall::UninstallResult, String> {
    if !result.failed.is_empty() && !result.should_quit {
        let msg = format!("Uninstall failed:\n{}", result.failed.join("\n"));
        log::error!("run_uninstall: {msg}");
        return Err(msg);
    }

    if result.should_quit {
        let handle = app_handle.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(400));
            handle.exit(0);
        });
    }

    if !result.failed.is_empty() {
        let msg = format!(
            "Uninstall partially failed (quitting so in-use paths can be removed):\n{}",
            result.failed.join("\n")
        );
        log::error!("run_uninstall: {msg}");
        return Err(msg);
    }

    Ok(result)
}
