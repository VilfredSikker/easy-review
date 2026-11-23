//! The review export: the tab's annotations as markdown, returned to the page
//! or written to disk for an agent. Each command renders the tab its key names
//! from that tab's published content.

use er_protocol::{ErError, ExportOptions};
use tauri::State;

use super::failed;
use crate::state::run_blocking;
use crate::state::DesktopState;

/// Render the tab's annotations as markdown and return the body to the UI
/// for clipboard copy / preview.
#[tauri::command]
pub async fn export_review(
    tab: er_protocol::TabKey,
    opts: ExportOptions,
    desktop: State<'_, DesktopState>,
) -> Result<String, ErError> {
    let desktop = desktop.inner().clone();
    run_blocking(move || Ok(render_tab(&desktop, tab, &opts)))
        .await
        .map_err(failed)?
}

/// Render and write to disk. Empty `path` writes `export.md` in the tab's
/// view bucket. Returns the resolved absolute path so the UI can show it in a
/// toast.
#[tauri::command]
pub async fn export_review_to_file(
    tab: er_protocol::TabKey,
    opts: ExportOptions,
    path: String,
    desktop: State<'_, DesktopState>,
) -> Result<String, ErError> {
    let desktop = desktop.inner().clone();
    run_blocking(move || Ok(write_tab(&desktop, tab, &opts, &path)))
        .await
        .map_err(failed)?
}

/// The palette's export: every default, written to `export.md` in the tab's
/// view bucket. Answers the path it wrote.
#[tauri::command]
pub async fn export_to_agent(
    tab: er_protocol::TabKey,
    desktop: State<'_, DesktopState>,
) -> Result<String, ErError> {
    let desktop = desktop.inner().clone();
    run_blocking(move || Ok(write_tab(&desktop, tab, &ExportOptions::default(), "")))
        .await
        .map_err(failed)?
}

/// The export of the tab `wire` names, rendered from its published content.
pub(crate) fn render_tab(
    desktop: &DesktopState,
    wire: er_protocol::TabKey,
    options: &ExportOptions,
) -> Result<String, ErError> {
    let content = super::content_of(desktop, wire)?;
    Ok(crate::export::render_content(&content, options))
}

/// Writes the export of the tab `wire` names to `path`, or to `export.md` in
/// its view bucket when `path` is empty. Answers the path written.
pub(crate) fn write_tab(
    desktop: &DesktopState,
    wire: er_protocol::TabKey,
    options: &ExportOptions,
    path: &str,
) -> Result<String, ErError> {
    let content = super::content_of(desktop, wire)?;
    let body = crate::export::render_content(&content, options);
    write_export(&body, path, &content.context.er_dir).map_err(failed)
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
            let sig = format!("pub async fn {name}(");
            let bodies: Vec<&str> = src
                .split("\n#[")
                .filter(|item| item.contains(&sig))
                .collect();
            assert_eq!(bodies.len(), 1, "{name} must be `pub async fn`");
            for body in bodies {
                assert!(
                    body.contains("run_blocking("),
                    "{name} must run its body with run_blocking"
                );
            }
        }
    }
}
