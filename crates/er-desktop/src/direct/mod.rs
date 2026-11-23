//! The commands that stay direct `#[tauri::command]` functions: each acts on
//! the window, a PTY, the OS shell or git for one file, and has no resource
//! to serve. They take `DesktopState`.

pub mod dev_log;
pub mod export;
pub mod line_commits;
pub mod os;
pub mod terminal;
pub mod window;

use er_protocol::{ErError, ErErrorKind};

/// The published content of the tab `wire` names, loaded on its first use. A
/// key the registry does not list answers `NotFound`.
pub(crate) fn content_of(
    desktop: &crate::state::DesktopState,
    wire: er_protocol::TabKey,
) -> Result<std::sync::Arc<er_service::TabContent>, ErError> {
    let key = er_service::resolve_tab_key(&desktop.service.app_registry().tabs_view(), wire)?;
    crate::commands::content_routes::keyed_content(desktop, key, None)
        .map(|(content, _)| content)
        .map_err(failed)
}

/// A direct command's failure: the OS, the PTY, git or the disk said no.
pub(crate) fn failed(message: impl std::fmt::Display) -> ErError {
    ErError {
        kind: ErErrorKind::Internal,
        message: message.to_string(),
    }
}
