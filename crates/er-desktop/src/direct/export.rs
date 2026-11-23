//! The review export: the tab's annotations as markdown, returned to the page
//! or written to disk for an agent.

use er_protocol::ErError;
use tauri::State;

use super::failed;
use crate::commands::AppState;
use crate::export::{render_markdown, ExportOpts};
use crate::state::run_blocking;

/// Render the active tab's annotations as markdown and return the body to
/// the UI for clipboard copy / preview.
#[tauri::command]
pub async fn export_review(
    opts: ExportOpts,
    state: State<'_, AppState>,
) -> Result<String, ErError> {
    let state = state.inner().clone();
    run_blocking(move || {
        let app = state.app.lock().map_err(|e| e.to_string())?;
        Ok(render_markdown(app.tab(), &opts))
    })
    .await
    .map_err(failed)
}

/// Render and write to disk. Empty `path` writes to `<comments_dir>/export.md`.
/// Returns the resolved absolute path so the UI can show it in a toast.
#[tauri::command]
pub async fn export_review_to_file(
    opts: ExportOpts,
    path: String,
    state: State<'_, AppState>,
) -> Result<String, ErError> {
    let state = state.inner().clone();
    run_blocking(move || {
        let app = state.app.lock().map_err(|e| e.to_string())?;
        let tab = app.tab();
        write_export(&render_markdown(tab, &opts), &path, &tab.comments_dir())
    })
    .await
    .map_err(failed)
}

/// The palette's export: every default, written to `<comments_dir>/export.md`.
/// Answers the path it wrote.
#[tauri::command]
pub async fn export_to_agent(state: State<'_, AppState>) -> Result<String, ErError> {
    let state = state.inner().clone();
    run_blocking(move || {
        let app = state.app.lock().map_err(|e| e.to_string())?;
        let tab = app.tab();
        let body = render_markdown(tab, &ExportOpts::default());
        write_export(&body, "", &tab.comments_dir())
    })
    .await
    .map_err(failed)
}

/// Writes `body` to `path`, or to `export.md` in `dir` when `path` is empty.
/// Answers the path written.
fn write_export(body: &str, path: &str, dir: &str) -> Result<String, String> {
    let target = if path.trim().is_empty() {
        std::fs::create_dir_all(dir).map_err(|e| format!("Failed to create {dir}: {e}"))?;
        format!("{dir}/export.md")
    } else {
        path.to_owned()
    };
    std::fs::write(&target, body).map_err(|e| format!("Failed to write {target}: {e}"))?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    /// A sync command runs on the main thread, and each of these renders under
    /// a lock, so each is async and runs its body on the blocking pool.
    #[test]
    fn the_export_commands_run_off_the_main_thread() {
        let src = include_str!("export.rs");
        for name in ["export_review", "export_review_to_file", "export_to_agent"] {
            let start = src
                .find(&format!("pub async fn {name}("))
                .unwrap_or_else(|| panic!("{name} must be `pub async fn`"));
            let rest = &src[start..];
            let end = rest[1..].find("\n#[tauri::command]").map_or(rest.len(), |i| i + 1);
            assert!(
                rest[..end].contains("run_blocking("),
                "{name} must run its body with run_blocking"
            );
        }
    }
}
