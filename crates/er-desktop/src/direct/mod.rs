//! The commands that stay direct `#[tauri::command]` functions: each acts on
//! the window, a PTY, the OS shell or git for one file, and has no resource
//! to serve. They take `DesktopState`; the few that still read the App on the
//! flag-off path take it as their own argument, so deleting that path deletes
//! only those twins.

pub mod dev_log;
pub mod export;
pub mod line_commits;
pub mod os;
pub mod terminal;
pub mod window;

use er_protocol::{ErError, ErErrorKind};

/// A direct command's failure: the OS, the PTY, git or the disk said no.
pub(crate) fn failed(message: impl std::fmt::Display) -> ErError {
    ErError {
        kind: ErErrorKind::Internal,
        message: message.to_string(),
    }
}
