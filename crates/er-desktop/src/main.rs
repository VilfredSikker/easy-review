#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod snapshot;

use std::sync::Mutex;

use commands::AppState;
use er_engine::app::App;
use er_engine::highlight::Highlighter;

fn main() {
    let app = App::new_with_args(&[]).unwrap_or_else(|e| {
        eprintln!("er-desktop: failed to init engine: {e}");
        std::process::exit(1);
    });

    let state = AppState {
        app: Mutex::new(app),
        highlighter: Mutex::new(Highlighter::new()),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![commands::get_snapshot])
        .run(tauri::generate_context!())
        .expect("error running tauri application");
}
