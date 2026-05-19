//! Native child webview for the review browser (`review-browser` label).
//!
//! Loads real `http://localhost` URLs so WKWebView handles auth redirects and cookies.
//! The main window webview stays on top with a transparent browser pane; annotation
//! UI receives messages via `browser://message` events.

use std::sync::Mutex;

use tauri::webview::{PageLoadEvent, WebviewBuilder};
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, State, Url, WebviewUrl};

use crate::frame_script::{BROWSER_MESSAGE_EVENT, FRAME_SCRIPT};

pub const REVIEW_BROWSER_LABEL: &str = "review-browser";

pub struct BrowserWebviewState {
    /// Whether the child webview has been created and attached to the main window.
    pub created: Mutex<bool>,
    /// Last annotate-mode flag requested by the UI (re-applied after each navigation).
    pub annotate_mode: Mutex<bool>,
}

impl BrowserWebviewState {
    pub fn new() -> Self {
        Self {
            created: Mutex::new(false),
            annotate_mode: Mutex::new(false),
        }
    }
}

fn main_tauri_window(app: &AppHandle) -> Result<tauri::Window, String> {
    app.get_window("main")
        .ok_or_else(|| "main window not found".to_string())
}

fn review_webview(app: &AppHandle) -> Option<tauri::Webview> {
    app.get_webview(REVIEW_BROWSER_LABEL)
}

fn parse_nav_url(url: &str) -> Result<Url, String> {
    let trimmed = url.trim();
    if trimmed.is_empty() || trimmed == "about:blank" {
        return Err("blank URL".to_string());
    }
    let with_scheme = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("http://{trimmed}")
    };
    Url::parse(&with_scheme).map_err(|e| e.to_string())
}

/// Push a host payload into the review page (direct API with MessageEvent fallback).
pub fn post_message_to_review_webview(
    wv: &tauri::Webview,
    payload: &serde_json::Value,
) -> Result<(), String> {
    let json = serde_json::to_string(payload).map_err(|e| e.to_string())?;
    let js = format!(
        "(function(){{var d={json};try{{if(typeof window.__er_receiveHostMessage==='function'){{window.__er_receiveHostMessage(d);}}else{{window.dispatchEvent(new MessageEvent('message',{{data:d}}));}}}}catch(e){{console.error('[er] host message failed',e);}}}})();"
    );
    wv.eval(js).map_err(|e| e.to_string())
}

fn sync_annotate_mode_to_page(
    app: &AppHandle,
    browser_state: &BrowserWebviewState,
) -> Result<(), String> {
    let active = *browser_state
        .annotate_mode
        .lock()
        .map_err(|e| e.to_string())?;
    let Some(wv) = review_webview(app) else {
        return Ok(());
    };
    post_message_to_review_webview(
        &wv,
        &serde_json::json!({ "__er_set_annotate_mode": active }),
    )
}

fn ensure_review_webview(
    app: &AppHandle,
    browser_state: &BrowserWebviewState,
    url: &str,
) -> Result<tauri::Webview, String> {
    if let Some(wv) = review_webview(app) {
        return Ok(wv);
    }

    let parsed = parse_nav_url(url)?;
    let window = main_tauri_window(app)?;
    let app_for_load = app.clone();

    let builder = WebviewBuilder::new(
        REVIEW_BROWSER_LABEL,
        WebviewUrl::External(parsed),
    )
    .initialization_script(FRAME_SCRIPT)
    .devtools(cfg!(debug_assertions))
    .on_page_load(move |wv, payload| {
        if payload.event() != PageLoadEvent::Finished {
            return;
        }
        if let Some(state) = app_for_load.try_state::<BrowserWebviewState>() {
            let active = *state.annotate_mode.lock().unwrap_or_else(|e| e.into_inner());
            if active {
                let msg = serde_json::json!({ "__er_set_annotate_mode": true });
                let _ = post_message_to_review_webview(&wv, &msg);
            }
        }
    });

    let wv = window
        .add_child(
            builder,
            LogicalPosition::new(0.0, 0.0),
            LogicalSize::new(800.0, 600.0),
        )
        .map_err(|e| e.to_string())?;

    *browser_state.created.lock().map_err(|e| e.to_string())? = true;
    Ok(wv)
}

/// Create or show the review browser child webview and navigate to `url`.
#[tauri::command]
pub fn browser_ensure(
    app: AppHandle,
    browser_state: State<'_, BrowserWebviewState>,
    url: String,
) -> Result<(), String> {
    let wv = ensure_review_webview(&app, &browser_state, &url)?;
    let parsed = parse_nav_url(&url)?;
    wv.navigate(parsed).map_err(|e| e.to_string())?;
    wv.show().map_err(|e| e.to_string())?;
    Ok(())
}

/// Hide the review browser child webview.
#[tauri::command]
pub fn browser_hide(app: AppHandle) -> Result<(), String> {
    if let Some(wv) = review_webview(&app) {
        wv.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Position and size the review browser (logical pixels, relative to the main window).
#[tauri::command]
pub fn browser_set_bounds(
    app: AppHandle,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<(), String> {
    let Some(wv) = review_webview(&app) else {
        return Ok(());
    };
    wv.set_position(LogicalPosition::new(x, y))
        .map_err(|e| e.to_string())?;
    wv.set_size(LogicalSize::new(width.max(1.0), height.max(1.0)))
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Navigate the review browser to a real HTTP(S) URL (creates the child webview if needed).
#[tauri::command]
pub fn browser_navigate(
    app: AppHandle,
    browser_state: State<'_, BrowserWebviewState>,
    url: String,
) -> Result<(), String> {
    let wv = ensure_review_webview(&app, &browser_state, &url)?;
    let parsed = parse_nav_url(&url)?;
    wv.navigate(parsed).map_err(|e| e.to_string())?;
    wv.show().map_err(|e| e.to_string())?;
    Ok(())
}

/// Enable or disable in-page annotation listeners (native review webview only).
#[tauri::command]
pub fn browser_set_annotate_mode(
    app: AppHandle,
    browser_state: State<'_, BrowserWebviewState>,
    active: bool,
) -> Result<(), String> {
    *browser_state
        .annotate_mode
        .lock()
        .map_err(|e| e.to_string())? = active;
    sync_annotate_mode_to_page(&app, &browser_state)
}

/// Called from the injected content script on the review browser page.
#[tauri::command]
pub fn browser_host_message(app: AppHandle, payload: serde_json::Value) -> Result<(), String> {
    app.emit(BROWSER_MESSAGE_EVENT, payload)
        .map_err(|e| e.to_string())
}

/// Deliver a host message to the review browser page (same shape as `postMessage` payloads).
#[tauri::command]
pub fn browser_send_to_page(app: AppHandle, payload: serde_json::Value) -> Result<(), String> {
    let Some(wv) = review_webview(&app) else {
        return Ok(());
    };
    post_message_to_review_webview(&wv, &payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_nav_url_adds_http_scheme() {
        let u = parse_nav_url("localhost:5173/foo").unwrap();
        assert_eq!(u.scheme(), "http");
        assert_eq!(u.host_str(), Some("localhost"));
        assert_eq!(u.port(), Some(5173));
    }
}
