//! Per-tab native child webviews for the review browser (`review-browser-{n}` labels).
//!
//! Loads real `http://localhost` URLs so WKWebView handles auth redirects and cookies.
//! The main window webview stays on top with a transparent browser pane; annotation
//! UI receives messages via `browser://message` events.

use std::collections::HashMap;
use std::sync::Mutex;

use er_engine::app::{App, BrowserLayout};
use tauri::webview::{PageLoadEvent, WebviewBuilder};
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, State, Url, WebviewUrl};

use crate::frame_script::{BROWSER_MESSAGE_EVENT, FRAME_SCRIPT};

pub const REVIEW_BROWSER_LABEL_PREFIX: &str = "review-browser";

pub fn webview_label(tab_idx: usize) -> String {
    format!("{REVIEW_BROWSER_LABEL_PREFIX}-{tab_idx}")
}

#[derive(Default)]
pub struct BrowserWebviewState {
    /// tab_idx → child webview has been created
    created: Mutex<HashMap<usize, bool>>,
    active_tab: Mutex<usize>,
    /// Per-tab annotate flag (mirrors TabState; used on page load before engine lock).
    annotate_by_tab: Mutex<HashMap<usize, bool>>,
}

impl BrowserWebviewState {
    pub fn set_tab_annotate_mode(&self, tab_idx: usize, active: bool) {
        if let Ok(mut m) = self.annotate_by_tab.lock() {
            m.insert(tab_idx, active);
        }
    }

    pub fn tab_annotate_mode(&self, tab_idx: usize) -> bool {
        self.annotate_by_tab
            .lock()
            .ok()
            .and_then(|m| m.get(&tab_idx).copied())
            .unwrap_or(false)
    }
}

fn main_tauri_window(app: &AppHandle) -> Result<tauri::Window, String> {
    app.get_window("main")
        .ok_or_else(|| "main window not found".to_string())
}

fn tab_webview(app: &AppHandle, tab_idx: usize) -> Option<tauri::Webview> {
    app.get_webview(&webview_label(tab_idx))
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

/// Strip `erp://` / `erps://` proxy schemes (matches desktop `fromProxyUrl`).
fn from_proxy_url(url: &str) -> String {
    if let Some(rest) = url.strip_prefix("erps://") {
        return format!("https://{rest}");
    }
    if let Some(rest) = url.strip_prefix("erp://") {
        return format!("http://{rest}");
    }
    url.to_string()
}

/// Same page identity as desktop `canonicalizeBrowserUrl` (hash excluded).
pub fn browser_urls_equivalent(a: &str, b: &str) -> bool {
    fn canon(raw: &str) -> Option<String> {
        let real = from_proxy_url(raw.trim());
        if real.is_empty() || real == "about:blank" {
            return Some(real);
        }
        let with_scheme = if real.contains("://") {
            real
        } else {
            format!("http://{real}")
        };
        let u = Url::parse(&with_scheme).ok()?;
        let pathname = if u.path().is_empty() {
            "/".to_string()
        } else {
            u.path().to_string()
        };
        let host = u.host_str().unwrap_or("").to_lowercase();
        Some(format!(
            "{}://{}{}{}",
            u.scheme().to_lowercase(),
            host,
            pathname,
            u.query().map(|q| format!("?{q}")).unwrap_or_default()
        ))
    }
    match (canon(a), canon(b)) {
        (Some(x), Some(y)) => x == y,
        _ => a.trim() == b.trim(),
    }
}

fn webview_current_url(wv: &tauri::Webview) -> Option<String> {
    wv.url().ok().map(|u| u.to_string())
}

/// Push a host payload into a review page (direct API with MessageEvent fallback).
pub fn post_message_to_webview(
    wv: &tauri::Webview,
    payload: &serde_json::Value,
) -> Result<(), String> {
    let json = serde_json::to_string(payload).map_err(|e| e.to_string())?;
    let js = format!(
        "(function(){{var d={json};try{{if(typeof window.__er_receiveHostMessage==='function'){{window.__er_receiveHostMessage(d);}}else{{window.dispatchEvent(new MessageEvent('message',{{data:d}}));}}}}catch(e){{console.error('[er] host message failed',e);}}}})();"
    );
    wv.eval(js).map_err(|e| e.to_string())
}

fn sync_annotate_mode_to_page(wv: &tauri::Webview, active: bool) -> Result<(), String> {
    post_message_to_webview(wv, &serde_json::json!({ "__er_set_annotate_mode": active }))
}

/// (Re)inject the annotation frame script. `initialization_script` only runs when the
/// child webview is first created — not on later `navigate()` — so we eval on each load.
fn inject_frame_script(wv: &tauri::Webview) -> Result<(), String> {
    wv.eval(FRAME_SCRIPT).map_err(|e| e.to_string())
}

fn on_review_page_loaded(
    app: &AppHandle,
    wv: &tauri::Webview,
    browser_state: &BrowserWebviewState,
    tab_idx: usize,
) {
    if inject_frame_script(wv).is_err() {
        return;
    }
    let active = browser_state.tab_annotate_mode(tab_idx);
    let _ = sync_annotate_mode_to_page(wv, active);
    // Frame script reports __er_ready via IPC; emit from host too so the UI
    // becomes ready even when invoke is slow or the page is still settling.
    let _ = app.emit(
        BROWSER_MESSAGE_EVENT,
        serde_json::json!({ "__er_ready": true, "__er_host_inject": true }),
    );
}

fn ensure_tab_webview(
    app: &AppHandle,
    browser_state: &BrowserWebviewState,
    tab_idx: usize,
    url: &str,
) -> Result<(tauri::Webview, bool), String> {
    if let Some(wv) = tab_webview(app, tab_idx) {
        return Ok((wv, false));
    }

    let parsed = parse_nav_url(url)?;
    let window = main_tauri_window(app)?;
    let label = webview_label(tab_idx);
    let tab_idx_capture = tab_idx;
    let app_for_load = app.clone();

    let builder = WebviewBuilder::new(label, WebviewUrl::External(parsed))
        .initialization_script(FRAME_SCRIPT)
        .devtools(cfg!(debug_assertions))
        .on_page_load(move |wv, payload| {
            if payload.event() != PageLoadEvent::Finished {
                return;
            }
            if let Some(state) = app_for_load.try_state::<BrowserWebviewState>() {
                on_review_page_loaded(&app_for_load, &wv, &state, tab_idx_capture);
            }
        });

    let wv = window
        .add_child(
            builder,
            LogicalPosition::new(0.0, 0.0),
            LogicalSize::new(800.0, 600.0),
        )
        .map_err(|e| e.to_string())?;

    browser_state
        .created
        .lock()
        .map_err(|e| e.to_string())?
        .insert(tab_idx, true);
    Ok((wv, true))
}

fn finish_browser_navigate(
    app: &AppHandle,
    browser_state: &BrowserWebviewState,
    tab_idx: usize,
    wv: &tauri::Webview,
    url: &str,
    newly_created: bool,
) -> Result<(), String> {
    if !newly_created {
        let should_navigate = webview_current_url(wv)
            .map(|current| !browser_urls_equivalent(&current, url))
            .unwrap_or(true);
        if should_navigate {
            let parsed = parse_nav_url(url)?;
            wv.navigate(parsed).map_err(|e| e.to_string())?;
        }
    }
    show_tab_webview(app, browser_state, tab_idx)?;
    let active = browser_state.tab_annotate_mode(tab_idx);
    sync_annotate_mode_to_page(wv, active)?;
    Ok(())
}

/// Annotate / tooltips / split-ratio updates — never reload the page.
pub fn sync_tab_browser_chrome(
    app: &AppHandle,
    browser_state: &BrowserWebviewState,
    engine: &App,
    tab_idx: usize,
    sync_annotate: bool,
) -> Result<(), String> {
    *browser_state.active_tab.lock().map_err(|e| e.to_string())? = tab_idx;
    let tab = engine
        .tabs
        .get(tab_idx)
        .ok_or_else(|| format!("tab index {tab_idx} out of range"))?;
    if tab.browser_layout == BrowserLayout::Hidden {
        return Ok(());
    }
    browser_state.set_tab_annotate_mode(tab_idx, tab.browser_annotate_mode);
    let url = tab.browser_url.trim();
    if url.is_empty() || url == "about:blank" {
        return Ok(());
    }
    show_tab_webview(app, browser_state, tab_idx)?;
    if sync_annotate {
        if let Some(wv) = tab_webview(app, tab_idx) {
            sync_annotate_mode_to_page(&wv, tab.browser_annotate_mode)?;
        }
    }
    Ok(())
}

/// Move a child webview off-screen and shrink it so a stale `hide()` cannot keep
/// intercepting clicks or keyboard focus over the main UI.
fn park_tab_webview(wv: &tauri::Webview) {
    let _ = wv.set_position(LogicalPosition::new(-10_000.0, -10_000.0));
    let _ = wv.set_size(LogicalSize::new(1.0, 1.0));
    let _ = wv.hide();
}

fn refocus_main_window(app: &AppHandle) {
    if let Ok(window) = main_tauri_window(app) {
        let _ = window.set_focus();
    }
}

pub fn hide_all_webviews(
    app: &AppHandle,
    browser_state: &BrowserWebviewState,
) -> Result<(), String> {
    let created = browser_state.created.lock().map_err(|e| e.to_string())?;
    for idx in created.keys() {
        if let Some(wv) = tab_webview(app, *idx) {
            park_tab_webview(&wv);
        }
    }
    refocus_main_window(app);
    Ok(())
}

/// Close every per-tab review webview (e.g. after tab close/reorder).
pub fn reset_all_tab_webviews(
    app: &AppHandle,
    browser_state: &BrowserWebviewState,
) -> Result<(), String> {
    let indices: Vec<usize> = browser_state
        .created
        .lock()
        .map_err(|e| e.to_string())?
        .keys()
        .copied()
        .collect();
    for idx in indices {
        destroy_tab_webview(app, idx)?;
    }
    browser_state
        .created
        .lock()
        .map_err(|e| e.to_string())?
        .clear();
    Ok(())
}

pub fn show_tab_webview(
    app: &AppHandle,
    browser_state: &BrowserWebviewState,
    tab_idx: usize,
) -> Result<(), String> {
    hide_all_webviews(app, browser_state)?;
    if let Some(wv) = tab_webview(app, tab_idx) {
        wv.show().map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn destroy_tab_webview(app: &AppHandle, tab_idx: usize) -> Result<(), String> {
    let label = webview_label(tab_idx);
    if let Some(wv) = app.get_webview(&label) {
        let _ = wv.close();
    }
    Ok(())
}

/// After tab switch or layout change: hide others, show active tab's webview when visible.
pub fn on_tab_selected(
    app: &AppHandle,
    browser_state: &BrowserWebviewState,
    engine: &App,
    tab_idx: usize,
) -> Result<(), String> {
    *browser_state.active_tab.lock().map_err(|e| e.to_string())? = tab_idx;
    hide_all_webviews(app, browser_state)?;
    let tab = engine
        .tabs
        .get(tab_idx)
        .ok_or_else(|| format!("tab index {tab_idx} out of range"))?;
    if tab.browser_layout == BrowserLayout::Hidden {
        return Ok(());
    }
    let url = tab.browser_url.trim();
    if url.is_empty() || url == "about:blank" {
        return Ok(());
    }
    browser_state.set_tab_annotate_mode(tab_idx, tab.browser_annotate_mode);
    let (wv, newly_created) = ensure_tab_webview(app, browser_state, tab_idx, url)?;
    finish_browser_navigate(app, browser_state, tab_idx, &wv, url, newly_created)
}

/// Create or show a tab's review browser child webview and navigate to `url`.
#[tauri::command]
#[allow(non_snake_case)]
pub fn browser_ensure(
    app: AppHandle,
    browser_state: State<'_, BrowserWebviewState>,
    url: String,
    tabIdx: Option<usize>,
) -> Result<(), String> {
    let idx = tabIdx.unwrap_or_else(|| browser_state.active_tab.lock().map(|g| *g).unwrap_or(0));
    let (wv, newly_created) = ensure_tab_webview(&app, &browser_state, idx, &url)?;
    finish_browser_navigate(&app, &browser_state, idx, &wv, &url, newly_created)
}

/// Tear down all review child webviews before showing a Svelte modal. `hide()` alone
/// is not always enough on macOS — destroyed webviews cannot intercept clicks.
#[tauri::command]
pub fn browser_suspend_for_overlay(
    app: AppHandle,
    browser_state: State<'_, BrowserWebviewState>,
) -> Result<(), String> {
    reset_all_tab_webviews(&app, &browser_state)?;
    refocus_main_window(&app);
    Ok(())
}

/// Hide one tab's webview, or all when `tabIdx` is None.
#[tauri::command]
#[allow(non_snake_case)]
pub fn browser_hide(
    app: AppHandle,
    tabIdx: Option<usize>,
    browser_state: State<'_, BrowserWebviewState>,
) -> Result<(), String> {
    if let Some(idx) = tabIdx {
        if let Some(wv) = tab_webview(&app, idx) {
            park_tab_webview(&wv);
        }
        refocus_main_window(&app);
    } else {
        hide_all_webviews(&app, &browser_state)?;
    }
    Ok(())
}

/// Position and size a tab's review browser (logical pixels, relative to the main window).
#[tauri::command]
#[allow(non_snake_case)]
pub fn browser_set_bounds(
    app: AppHandle,
    tabIdx: Option<usize>,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    browser_state: State<'_, BrowserWebviewState>,
) -> Result<(), String> {
    let idx = tabIdx.unwrap_or(browser_state.active_tab.lock().map(|g| *g).unwrap_or(0));
    let Some(wv) = tab_webview(&app, idx) else {
        return Ok(());
    };
    wv.set_position(LogicalPosition::new(x, y))
        .map_err(|e| e.to_string())?;
    wv.set_size(LogicalSize::new(width.max(1.0), height.max(1.0)))
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Navigate a tab's review browser to a real HTTP(S) URL.
#[tauri::command]
#[allow(non_snake_case)]
pub fn browser_navigate(
    app: AppHandle,
    browser_state: State<'_, BrowserWebviewState>,
    url: String,
    tabIdx: Option<usize>,
) -> Result<(), String> {
    let idx = tabIdx.unwrap_or(browser_state.active_tab.lock().map(|g| *g).unwrap_or(0));
    let (wv, newly_created) = ensure_tab_webview(&app, &browser_state, idx, &url)?;
    finish_browser_navigate(&app, &browser_state, idx, &wv, &url, newly_created)
}

/// Enable or disable in-page annotation listeners for a tab's webview.
#[tauri::command]
#[allow(non_snake_case)]
pub fn browser_set_annotate_mode(
    app: AppHandle,
    active: bool,
    tabIdx: Option<usize>,
    browser_state: State<'_, BrowserWebviewState>,
) -> Result<(), String> {
    let idx = tabIdx.unwrap_or(browser_state.active_tab.lock().map(|g| *g).unwrap_or(0));
    browser_state.set_tab_annotate_mode(idx, active);
    let Some(wv) = tab_webview(&app, idx) else {
        return Ok(());
    };
    sync_annotate_mode_to_page(&wv, active)
}

/// Called from the injected content script on a review browser page.
#[tauri::command]
pub fn browser_host_message(app: AppHandle, payload: serde_json::Value) -> Result<(), String> {
    app.emit(BROWSER_MESSAGE_EVENT, payload)
        .map_err(|e| e.to_string())
}

/// Reload the current page in a tab's review browser.
#[tauri::command]
#[allow(non_snake_case)]
pub fn browser_reload(
    app: AppHandle,
    tabIdx: Option<usize>,
    browser_state: State<'_, BrowserWebviewState>,
) -> Result<(), String> {
    let idx = tabIdx.unwrap_or(browser_state.active_tab.lock().map(|g| *g).unwrap_or(0));
    let Some(wv) = tab_webview(&app, idx) else {
        return Ok(());
    };
    wv.reload().map_err(|e| e.to_string())
}

/// Deliver a host message to a tab's review browser page.
#[tauri::command]
#[allow(non_snake_case)]
pub fn browser_send_to_page(
    app: AppHandle,
    payload: serde_json::Value,
    tabIdx: Option<usize>,
    browser_state: State<'_, BrowserWebviewState>,
) -> Result<(), String> {
    let idx = tabIdx.unwrap_or(browser_state.active_tab.lock().map(|g| *g).unwrap_or(0));
    let Some(wv) = tab_webview(&app, idx) else {
        return Ok(());
    };
    post_message_to_webview(&wv, &payload)
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

    #[test]
    fn webview_label_includes_tab_index() {
        assert_eq!(webview_label(3), "review-browser-3");
    }

    #[test]
    fn browser_urls_equivalent_normalizes_host_and_path() {
        assert!(browser_urls_equivalent(
            "http://localhost:5173",
            "http://localhost:5173/"
        ));
        assert!(!browser_urls_equivalent(
            "http://localhost:5173/foo",
            "http://localhost:5173/foo?x=1"
        ));
        assert!(browser_urls_equivalent(
            "http://localhost:5173/foo#bar",
            "http://localhost:5173/foo#baz"
        ));
    }
}
