//! The main window's own mechanics.

use er_protocol::ErError;

use super::failed;

#[tauri::command]
pub fn start_window_drag(window: tauri::Window) -> Result<(), ErError> {
    window.start_dragging().map_err(failed)
}
