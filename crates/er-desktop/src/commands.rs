use std::sync::Mutex;
use tauri::State;

use er_engine::app::{App, DiffMode};
use er_engine::highlight::Highlighter;

use crate::snapshot::{build_snapshot, AppSnapshot};

pub struct AppState {
    pub app: Mutex<App>,
    pub highlighter: Mutex<Highlighter>,
}

macro_rules! snap {
    ($state:expr) => {{
        let app = $state.app.lock().map_err(|e| e.to_string())?;
        let mut hl = $state.highlighter.lock().map_err(|e| e.to_string())?;
        Ok(build_snapshot(&app, &mut hl))
    }};
}

#[tauri::command]
pub fn get_snapshot(state: State<AppState>) -> Result<AppSnapshot, String> {
    snap!(state)
}

#[tauri::command]
pub fn toggle_panel(panel: String, state: State<AppState>) -> Result<(), String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    app.toggle_panel(&panel);
    Ok(())
}

// ── Navigation ────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn select_file(idx: usize, state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    {
        let tab = app.tab_mut();
        if idx < tab.files.len() {
            tab.selected_file = idx;
            tab.current_hunk = 0;
            tab.current_line = None;
            tab.diff_scroll = 0;
            tab.h_scroll = 0;
            tab.ensure_file_parsed();
            tab.rebuild_hunk_offsets();
        }
    }
    Ok(build_snapshot(&app, &mut hl))
}

#[tauri::command]
pub fn next_file(state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    app.tab_mut().next_file();
    Ok(build_snapshot(&app, &mut hl))
}

#[tauri::command]
pub fn prev_file(state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    app.tab_mut().prev_file();
    Ok(build_snapshot(&app, &mut hl))
}

#[tauri::command]
pub fn jump_to_unreviewed(state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    {
        let tab = app.tab_mut();
        let visible: Vec<usize> = tab
            .visible_files()
            .into_iter()
            .filter(|(_, f)| !tab.reviewed.contains_key(&f.path))
            .map(|(i, _)| i)
            .collect();
        if let Some(&first) = visible.first() {
            tab.selected_file = first;
            tab.current_hunk = 0;
            tab.current_line = None;
            tab.diff_scroll = 0;
            tab.h_scroll = 0;
            tab.ensure_file_parsed();
            tab.rebuild_hunk_offsets();
        }
    }
    Ok(build_snapshot(&app, &mut hl))
}

// ── Mode ──────────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn set_mode(mode: String, state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    let diff_mode = match mode.as_str() {
        "unstaged" => DiffMode::Unstaged,
        "staged" => DiffMode::Staged,
        "history" => DiffMode::History,
        _ => DiffMode::Branch,
    };
    app.tab_mut().set_mode(diff_mode);
    Ok(build_snapshot(&app, &mut hl))
}

// ── Reviewed state ────────────────────────────────────────────────────────────

#[tauri::command]
pub fn toggle_reviewed(state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    app.toggle_reviewed().map_err(|e| e.to_string())?;
    Ok(build_snapshot(&app, &mut hl))
}

#[tauri::command]
pub fn mark_reviewed(file_idx: usize, state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    {
        let tab = app.tab_mut();
        if let Some(file) = tab.files.get(file_idx) {
            let path = file.path.clone();
            let hash = tab.current_per_file_hashes.get(&path).cloned().unwrap_or_default();
            tab.reviewed.insert(path.clone(), hash);
            let _ = tab.save_reviewed_files();
        }
    }
    Ok(build_snapshot(&app, &mut hl))
}

#[tauri::command]
pub fn unmark_reviewed(file_idx: usize, state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    {
        let tab = app.tab_mut();
        if let Some(file) = tab.files.get(file_idx) {
            let path = file.path.clone();
            tab.reviewed.remove(&path);
            let _ = tab.save_reviewed_files();
        }
    }
    Ok(build_snapshot(&app, &mut hl))
}

// ── Editor ────────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn open_in_editor(state: State<AppState>) -> Result<(), String> {
    let app = state.app.lock().map_err(|e| e.to_string())?;
    app.tab().open_in_editor().map_err(|e| e.to_string())
}

// ── Filter / search ───────────────────────────────────────────────────────────

#[tauri::command]
pub fn set_filter(query: String, state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    app.tab_mut().apply_filter_expr(&query);
    Ok(build_snapshot(&app, &mut hl))
}

#[tauri::command]
pub fn clear_filter(state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    app.tab_mut().apply_filter_expr("");
    Ok(build_snapshot(&app, &mut hl))
}
