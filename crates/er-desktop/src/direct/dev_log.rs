//! The frontend's dev-log filter, read once at startup.

#[tauri::command]
pub fn dev_log_filter() -> Option<Vec<String>> {
    crate::dev_log::filter_groups()
}
