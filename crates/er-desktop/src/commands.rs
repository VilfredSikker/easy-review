use std::sync::Mutex;
use tauri::State;

use er_engine::ai::CommentType;
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
pub fn toggle_panel(panel: String, state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    app.toggle_panel(&panel);
    Ok(build_snapshot(&app, &mut hl))
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

// ── Hunk navigation ──────────────────────────────────────────────────────────

#[tauri::command]
pub fn next_hunk(state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    app.tab_mut().next_hunk();
    Ok(build_snapshot(&app, &mut hl))
}

#[tauri::command]
pub fn prev_hunk(state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    app.tab_mut().prev_hunk();
    Ok(build_snapshot(&app, &mut hl))
}

#[tauri::command]
pub fn toggle_compacted(state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    app.tab_mut().toggle_compacted().map_err(|e| e.to_string())?;
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

// ── Threads ───────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn add_comment(
    file: String,
    hunk_idx: usize,
    line_num: Option<usize>,
    text: String,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    app.submit_comment_text(file, hunk_idx, line_num, text, CommentType::GitHubComment, None)
        .map_err(|e| e.to_string())?;
    Ok(build_snapshot(&app, &mut hl))
}

#[tauri::command]
pub fn add_question(
    file: String,
    hunk_idx: usize,
    line_num: Option<usize>,
    text: String,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    app.submit_comment_text(file, hunk_idx, line_num, text, CommentType::Question, None)
        .map_err(|e| e.to_string())?;
    Ok(build_snapshot(&app, &mut hl))
}

#[tauri::command]
pub fn reply_to_thread(
    parent_id: String,
    text: String,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    let (file, hunk_idx, line_num, comment_type) = {
        let tab = app.tab();
        if parent_id.starts_with("q-") {
            let q = tab.ai.questions.as_ref()
                .and_then(|qs| qs.questions.iter().find(|q| q.id == parent_id))
                .map(|q| (q.file.clone(), q.hunk_index.unwrap_or(0), q.line_start, CommentType::Question));
            q.ok_or_else(|| "Question not found".to_string())?
        } else {
            let c = tab.ai.github_comments.as_ref()
                .and_then(|gc| gc.comments.iter().find(|c| c.id == parent_id))
                .map(|c| (c.file.clone(), c.hunk_index.unwrap_or(0), c.line_start, CommentType::GitHubComment));
            c.ok_or_else(|| "Comment not found".to_string())?
        }
    };
    app.submit_comment_text(file, hunk_idx, line_num, text, comment_type, Some(parent_id))
        .map_err(|e| e.to_string())?;
    Ok(build_snapshot(&app, &mut hl))
}

#[tauri::command]
pub fn delete_thread(id: String, state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    app.delete_comment_direct(&id).map_err(|e| e.to_string())?;
    Ok(build_snapshot(&app, &mut hl))
}

// ── GitHub sync ───────────────────────────────────────────────────────────────

#[tauri::command]
pub fn refresh_diff(state: State<AppState>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let mut hl = state.highlighter.lock().map_err(|e| e.to_string())?;
    app.tab_mut().refresh_diff().map_err(|e| e.to_string())?;
    Ok(build_snapshot(&app, &mut hl))
}
