use std::sync::Mutex;
use tauri::State;

use er_engine::app::App;
use er_engine::highlight::Highlighter;

use crate::snapshot::{build_snapshot, AppSnapshot};

pub struct AppState {
    pub app: Mutex<App>,
    pub highlighter: Mutex<Highlighter>,
}

#[tauri::command]
pub fn get_snapshot(state: State<AppState>) -> Result<AppSnapshot, String> {
    let app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    Ok(build_snapshot(&app, &mut hl))
}
