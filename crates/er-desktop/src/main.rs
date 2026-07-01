#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod arena_commands;
mod auto_triage;
mod browser_proxy;
mod browser_webview;
mod commands;
mod config_commands;
mod dev_log;
mod er_storage;
mod export;
mod frame_script;
mod gh_status_cache;
mod inbox;
mod main_webview_policy;
mod pr_cache;
mod pr_open_cache;
mod profile_log;
mod projects;
mod snapshot;
mod tabs;
mod terminal;
mod window_placement;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem, Submenu};
use tauri::Manager;

use browser_webview::BrowserWebviewState;
use commands::AppState;
use er_engine::app::App;
use frame_script::FRAME_SCRIPT;
use snapshot::{
    GithubStatusSnapshot, LoadingFlags, LoadingState, PrInfo, ProjectMeta, WatchStatusSnapshot,
    WatchStatusState,
};

/// Inject the annotation content script before `</head>` (or `</body>` as fallback).
fn inject_script(mut html: Vec<u8>) -> Vec<u8> {
    let tag = format!("<script type=\"text/javascript\">{}</script>", FRAME_SCRIPT);
    if let Some(pos) = find_ascii_case_insensitive(&html, b"</head>") {
        html.splice(pos..pos, tag.bytes());
        html
    } else if let Some(pos) = find_ascii_case_insensitive(&html, b"</body>") {
        html.splice(pos..pos, tag.bytes());
        html
    } else {
        html.extend_from_slice(tag.as_bytes());
        html
    }
}

fn find_ascii_case_insensitive(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|w| w.eq_ignore_ascii_case(needle))
}

fn is_html_content_type(content_type: Option<&str>) -> bool {
    content_type
        .map(|ct| ct.to_ascii_lowercase().contains("text/html"))
        .unwrap_or(false)
}

fn should_forward_header(name: &str, is_html: bool) -> bool {
    let name = name.to_ascii_lowercase();
    if matches!(
        name.as_str(),
        "content-length"
            | "content-encoding"
            | "transfer-encoding"
            | "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "upgrade"
    ) {
        return false;
    }
    if is_html
        && matches!(
            name.as_str(),
            "content-security-policy"
                | "content-security-policy-report-only"
                | "x-content-security-policy"
                | "x-webkit-csp"
                | "x-frame-options"
        )
    {
        return false;
    }
    true
}

fn filtered_proxy_headers(
    headers: &[browser_proxy::ProxyHeader],
    is_html: bool,
) -> Vec<browser_proxy::ProxyHeader> {
    headers
        .iter()
        .filter(|h| should_forward_header(&h.name, is_html))
        .cloned()
        .collect()
}

fn upstream_url_for_proxy(uri: &tauri::http::Uri, upstream_scheme: &str) -> String {
    let authority = uri.authority().map(|a| a.as_str()).unwrap_or("localhost");
    let path = uri.path();
    let path = if path.is_empty() { "/" } else { path };
    match uri.query() {
        Some(q) => format!("{}://{}{}?{}", upstream_scheme, authority, path, q),
        None => format!("{}://{}{}", upstream_scheme, authority, path),
    }
}

const PROXY_HTML_SIZE_LIMIT: usize = 10 * 1024 * 1024; // 10 MB

// `app` is only used inside the `#[cfg(target_os = "macos")]` block below.
#[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
fn reveal_main_window(
    window: &tauri::WebviewWindow,
    app: &tauri::AppHandle,
    reason: &str,
) -> tauri::Result<()> {
    // Pre-show clamp: best-effort, avoids a visible flash at a stale position.
    if let Err(e) = window_placement::ensure_window_visible(window) {
        log::warn!("window placement failed during {reason}, recentering: {e}");
        if let Err(center_err) = window.center() {
            log::warn!("window recenter failed during {reason}: {center_err}");
        }
    }
    if let Ok(p) = window.outer_position() {
        log::info!(
            "window placement {reason}: pre-show position ({}, {})",
            p.x,
            p.y
        );
    }

    window.show()?;

    // Post-show clamp: a hidden macOS NSWindow can report a stale position, so
    // the pre-show pass may no-op and `show()` then reveals the window at the
    // restored (possibly off-screen) coordinates. Re-running after show reads
    // the real position and reliably pulls it back onto a monitor.
    if let Err(e) = window_placement::ensure_window_visible(window) {
        log::warn!("post-show window placement failed during {reason}, recentering: {e}");
        if let Err(center_err) = window.center() {
            log::warn!("post-show window recenter failed during {reason}: {center_err}");
        }
    }
    if let Ok(p) = window.outer_position() {
        log::info!(
            "window placement {reason}: post-show position ({}, {})",
            p.x,
            p.y
        );
    }

    if let Err(e) = window.unminimize() {
        log::warn!("window unminimize failed during {reason}: {e}");
    }
    if let Err(e) = window.set_focus() {
        log::warn!("window focus failed during {reason}: {e}");
    }
    #[cfg(target_os = "macos")]
    {
        if let Err(e) = app.show() {
            log::warn!("app show failed during {reason}: {e}");
        }
    }

    Ok(())
}

// Only reachable from the macOS `Reopen` run event.
#[cfg(target_os = "macos")]
fn reveal_main_window_from_handle(app: &tauri::AppHandle, reason: &str) {
    match app.get_webview_window("main") {
        Some(window) => {
            if let Err(e) = reveal_main_window(&window, app, reason) {
                log::warn!("main window reveal failed during {reason}: {e}");
            }
        }
        None => log::warn!("main window missing during {reason}"),
    }
}
// Vite/SvelteKit dev-server JS dependency chunks can be large, especially when
// source maps or prebundled dependencies are served through the dev server.
// Non-HTML assets are still bounded to avoid unbounded memory use during proxying.
const PROXY_ASSET_SIZE_LIMIT: usize = 128 * 1024 * 1024; // 128 MB

fn proxy_size_limit(is_html: bool) -> usize {
    if is_html {
        return std::env::var("ER_PROXY_HTML_LIMIT_BYTES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(PROXY_HTML_SIZE_LIMIT);
    }
    std::env::var("ER_PROXY_ASSET_LIMIT_BYTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(PROXY_ASSET_SIZE_LIMIT)
}

fn oversized_response(
    bytes: usize,
    size_limit: usize,
    is_html: bool,
) -> tauri::http::Response<Vec<u8>> {
    let (content_type, body) = if is_html {
        (
            "text/html",
            format!(
                "<html><body><p>Response too large ({} bytes exceeds {} byte limit).</p></body></html>",
                bytes, size_limit
            ),
        )
    } else {
        (
            "text/plain",
            format!(
                "Response too large ({} bytes exceeds {} byte limit).",
                bytes, size_limit
            ),
        )
    };
    tauri::http::Response::builder()
        .status(413)
        .header("Content-Type", content_type)
        .body(body.into_bytes())
        .unwrap()
}

/// Read at most `limit` bytes from `reader`. Returns `Ok(bytes)` on success,
/// `Err(bytes_read)` if the limit was exceeded.
/// Read at most `limit` bytes from `reader`.
/// Returns `Ok(bytes)` on clean EOF within the cap.
/// Returns `Err(bytes_so_far)` if the limit is exceeded.
/// Propagates I/O errors (e.g. connection reset) as `Err(bytes_so_far)`.
fn read_bounded(mut reader: impl std::io::Read, limit: usize) -> Result<Vec<u8>, usize> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if buf.len() + n > limit {
                    return Err(buf.len() + n);
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            Err(_) => return Err(buf.len()),
        }
    }
    Ok(buf)
}

/// Request headers to forward from the WebView to the upstream dev server.
/// Cookie is required so Clerk/session SSR matches the logged-in client;
/// without it SvelteKit renders unauthenticated (e.g. experiments 404) while
/// the client still hydrates with a session → blank page.
fn should_forward_request_header(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    if name.starts_with("sec-") {
        return false;
    }
    !matches!(
        name.as_str(),
        "host"
            | "connection"
            | "content-length"
            | "transfer-encoding"
            | "upgrade"
            | "accept-encoding"
    )
}

fn forward_request_headers(headers: &tauri::http::HeaderMap) -> Vec<(String, String)> {
    headers
        .iter()
        .filter(|(name, _)| should_forward_request_header(name.as_str()))
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|v| (name.as_str().to_string(), v.to_string()))
        })
        .collect()
}

fn proxy_transport_error_response(e: &ureq::Error) -> tauri::http::Response<Vec<u8>> {
    let msg = e.to_string();
    let status = if msg.contains("timed out") || msg.contains("TimedOut") {
        504
    } else {
        502
    };
    let label = if status == 504 {
        "Request timed out"
    } else {
        "Connection failed"
    };
    eprintln!("[erp] connection error ({}): {}", status, e);
    tauri::http::Response::builder()
        .status(status)
        .header("Content-Type", "text/html")
        .body(format!("<html><body><p>{}: {}</p></body></html>", label, e).into_bytes())
        .unwrap()
}

#[allow(clippy::result_large_err)]
fn upstream_request(
    agent: &ureq::Agent,
    request: &tauri::http::Request<Vec<u8>>,
    target: &str,
) -> Result<ureq::Response, ureq::Error> {
    let mut req = agent
        .request(request.method().as_str(), target)
        .set("Accept-Encoding", "identity");
    for (name, value) in forward_request_headers(request.headers()) {
        req = req.set(&name, &value);
    }
    let body = request.body();
    if body.is_empty() {
        req.call()
    } else {
        req.send_bytes(body)
    }
}

fn proxied_response(
    request: &tauri::http::Request<Vec<u8>>,
    upstream_scheme: &str,
) -> tauri::http::Response<Vec<u8>> {
    let uri = request.uri();
    let target = upstream_url_for_proxy(uri, upstream_scheme);
    eprintln!("[erp] request: {} {} -> {}", request.method(), uri, target);
    // Document navigations: see `browser_proxy` module for redirect policy.
    let agent = ureq::AgentBuilder::new()
        .redirects(0)
        .timeout_connect(std::time::Duration::from_secs(10))
        .timeout_read(std::time::Duration::from_secs(30))
        .build();

    let forward_headers = forward_request_headers(request.headers());
    let method = request.method().as_str();
    let (resp, headers) = if method == "GET" || method == "HEAD" {
        use browser_proxy::{fetch_upstream_get, UpstreamFetchError};
        let fetched = match fetch_upstream_get(&agent, &forward_headers, &target, true) {
            Ok(f) => f,
            Err(UpstreamFetchError::BrowserRedirect { status, location }) => {
                return browser_proxy::browser_redirect_response(status, &location);
            }
            Err(UpstreamFetchError::CrossOriginHandoff(location)) => {
                return browser_proxy::webview_navigation_handoff(&location);
            }
            Err(UpstreamFetchError::Transport(e)) => {
                return proxy_transport_error_response(e.as_ref());
            }
        };
        (fetched.response, fetched.headers)
    } else {
        let result = upstream_request(&agent, request, &target);
        match result {
            Ok(resp) => {
                let headers = browser_proxy::collect_ureq_headers(&resp);
                (resp, headers)
            }
            Err(ureq::Error::Status(_, resp)) => {
                let headers = browser_proxy::collect_ureq_headers(&resp);
                (resp, headers)
            }
            Err(e) => return proxy_transport_error_response(&e),
        }
    };

    let status = resp.status();

    let ct = resp.header("Content-Type").map(str::to_string);
    let is_html = is_html_content_type(ct.as_deref());
    let size_limit = proxy_size_limit(is_html);
    let bounded = read_bounded(resp.into_reader(), size_limit);
    let mut body = match bounded {
        Ok(b) => b,
        Err(bytes) => {
            eprintln!(
                "[erp] response too large: {} bytes exceeds limit {}",
                bytes, size_limit
            );
            return oversized_response(bytes, size_limit, is_html);
        }
    };
    if is_html {
        body = inject_script(body);
    }

    eprintln!(
        "[erp] response: status={} content-type={:?} is_html={}",
        status, ct, is_html
    );
    let mut builder = tauri::http::Response::builder()
        .status(status)
        .header("Access-Control-Allow-Origin", "*");
    for h in filtered_proxy_headers(&headers, is_html) {
        builder = builder.header(&h.name, &h.value);
    }
    if is_html
        && !headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case("cache-control"))
    {
        builder = builder.header("Cache-Control", "no-cache");
    }
    builder.body(body).unwrap_or_else(|_| {
        tauri::http::Response::builder()
            .status(500)
            .body(vec![])
            .unwrap()
    })
}

/// Install a custom application menu. Mirrors Tauri's default menu but defines
/// Select All as a custom item with no accelerator, so ⌘A is no longer claimed
/// by macOS at the menu-bar level and can reach desktop-ui's JS handler
/// (which opens the AI palette).
fn install_app_menu(app: &tauri::AppHandle) -> tauri::Result<()> {
    let pkg = app.package_info();
    let app_name = pkg.name.clone();

    // Select All without an accelerator. Wired in `on_menu_event` below.
    let select_all = MenuItemBuilder::with_id("er.select_all", "Select All")
        .accelerator("")
        .build(app)?;

    let edit_menu = Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(app, None)?,
            &PredefinedMenuItem::redo(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &select_all,
        ],
    )?;

    let window_menu = Submenu::with_items(
        app,
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(app, None)?,
            &PredefinedMenuItem::maximize(app, None)?,
            #[cfg(target_os = "macos")]
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::close_window(app, None)?,
        ],
    )?;

    let mut builder = MenuBuilder::new(app);

    #[cfg(target_os = "macos")]
    {
        let app_submenu = Submenu::with_items(
            app,
            app_name.clone(),
            true,
            &[
                &PredefinedMenuItem::about(app, None, None)?,
                &PredefinedMenuItem::separator(app)?,
                &PredefinedMenuItem::services(app, None)?,
                &PredefinedMenuItem::separator(app)?,
                &PredefinedMenuItem::hide(app, None)?,
                &PredefinedMenuItem::hide_others(app, None)?,
                &PredefinedMenuItem::show_all(app, None)?,
                &PredefinedMenuItem::separator(app)?,
                &PredefinedMenuItem::quit(app, None)?,
            ],
        )?;
        builder = builder.item(&app_submenu);
    }

    #[cfg(not(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    )))]
    {
        let file_submenu = Submenu::with_items(
            app,
            "File",
            true,
            &[
                &PredefinedMenuItem::close_window(app, None)?,
                #[cfg(not(target_os = "macos"))]
                &PredefinedMenuItem::quit(app, None)?,
            ],
        )?;
        builder = builder.item(&file_submenu);
    }

    builder = builder.item(&edit_menu);

    #[cfg(target_os = "macos")]
    {
        let view_menu = Submenu::with_items(
            app,
            "View",
            true,
            &[&PredefinedMenuItem::fullscreen(app, None)?],
        )?;
        builder = builder.item(&view_menu);
    }

    let menu = builder.item(&window_menu).build()?;
    app.set_menu(menu)?;

    let _ = app_name;
    app.on_menu_event(|app_handle, event| {
        if event.id() == "er.select_all" {
            if let Some(wv) = app_handle.get_webview_window("main") {
                let _ = wv.eval("document.execCommand('selectAll')");
            }
        }
    });

    Ok(())
}

/// Geometry the `window-state` plugin *saves*. MAXIMIZED is kept so the plugin
/// detects the maximized state and preserves the last *windowed* size+position
/// (it skips overwriting them while maximized) instead of persisting the zoomed
/// dimensions as if they were the windowed size.
fn window_state_save_flags() -> tauri_plugin_window_state::StateFlags {
    use tauri_plugin_window_state::StateFlags;
    StateFlags::SIZE
        | StateFlags::POSITION
        | StateFlags::MAXIMIZED
        | StateFlags::FULLSCREEN
        | StateFlags::DECORATIONS
}

/// Geometry we *restore* on launch. MAXIMIZED is dropped so the window always
/// reopens as a normal resizable window at its last windowed size+position,
/// never auto-zoomed to fill the screen.
fn window_state_restore_flags() -> tauri_plugin_window_state::StateFlags {
    use tauri_plugin_window_state::StateFlags;
    StateFlags::SIZE | StateFlags::POSITION | StateFlags::FULLSCREEN | StateFlags::DECORATIONS
}

/// Look up the cached head commit OID for `(owner, repo, number)` in the
/// shared PR cache. Returns `""` when the remote/PR isn't cached yet —
/// callers must treat that as "unknown" (fail open, never skip on it), since
/// an empty OID is also `PrInfo::head_oid`'s `#[serde(default)]` value, not a
/// real "no head" signal.
fn cached_head_oid(
    pr_cache: &pr_cache::PrCacheMap,
    owner: &str,
    repo: &str,
    number: u64,
) -> String {
    pr_cache
        .lock()
        .ok()
        .and_then(|cache| {
            cache
                .get(&format!("{owner}/{repo}"))
                .and_then(|prs| prs.iter().find(|p| p.number == number))
                .map(|p| p.head_oid.clone())
        })
        .unwrap_or_default()
}

/// Whether the background comment-sync loop should SKIP this tick: this PR's
/// head OID is unchanged since the last successful sync AND that sync was less
/// than `ttl` ago. This is a poll THROTTLE, not change-detection — PR comments
/// and reviews are posted independently of any push, so an unchanged head OID
/// does NOT mean "nothing new". `ttl` therefore bounds how stale the comment
/// panel can get on a push-idle PR; the OID check only ACCELERATES resync after
/// a push (a new head OID drops through the throttle so freshly-pushed review
/// threads appear promptly). An empty/unknown `head_oid` never skips (fail open —
/// keep syncing every tick until the probe loop populates a real OID). Pure so
/// the skip decision is unit-testable without the surrounding thread/loop.
fn comment_sync_recently_synced(
    last_entry: Option<&(String, std::time::Instant)>,
    head_oid: &str,
    now: std::time::Instant,
    ttl: std::time::Duration,
) -> bool {
    !head_oid.is_empty()
        && last_entry.is_some_and(|(last_oid, last_at)| {
            last_oid == head_oid && now.saturating_duration_since(*last_at) < ttl
        })
}

/// Whether the pr-head-probe loop should SKIP the `gh` subprocess this tick: it
/// already probed this PR less than `ttl` ago (the throttle), regardless of
/// whether the OID changed. Pure so the throttle is unit-testable.
fn probe_recently_done(
    last_probe: Option<&std::time::Instant>,
    now: std::time::Instant,
    ttl: std::time::Duration,
) -> bool {
    last_probe.is_some_and(|last_at| now.saturating_duration_since(*last_at) < ttl)
}

fn main() {
    er_engine::env_path::init_cli_path();
    dev_log::init();

    // When a persisted tabs.json exists we're going to replace `app.tabs`
    // entirely below, so the engine init only needs a placeholder tab —
    // running the initial `refresh_diff()` here would be wasted work
    // (~900ms on a large repo). Use the unloaded constructor in that case.
    let has_persisted_tabs = tabs::load_tabs()
        .map(|f| !f.tabs.is_empty())
        .unwrap_or(false);
    let cwd_repo_root = er_engine::git::get_repo_root().ok();

    let mut app = match (has_persisted_tabs, cwd_repo_root.clone()) {
        (true, Some(root)) => App::new_unloaded(root).unwrap_or_else(|e| {
            eprintln!("er-desktop: failed to init engine: {e}");
            std::process::exit(1);
        }),
        (true, None) => {
            // No CWD repo but we have tabs to restore: open against the last
            // active project so the engine has a valid root.
            let fallback = active_root_from_projects();
            match fallback
                .as_deref()
                .map(|p| App::new_unloaded(p.to_string()))
            {
                Some(Ok(a)) => a,
                _ => {
                    eprintln!("er-desktop: cwd not a repo and no active project on disk; aborting");
                    std::process::exit(1);
                }
            }
        }
        (false, _) => match App::new_with_args(&[]) {
            Ok(a) => a,
            Err(cwd_err) => {
                let fallback = active_root_from_projects();
                match fallback
                    .as_deref()
                    .map(|p| App::new_with_args(&[p.to_string()]))
                {
                    Some(Ok(a)) => {
                        log::info!(
                            "er-desktop: cwd not a repo ({cwd_err}); opened last active project: {}",
                            fallback.as_deref().unwrap_or("?")
                        );
                        a
                    }
                    _ => {
                        eprintln!("er-desktop: failed to init engine: {cwd_err}");
                        std::process::exit(1);
                    }
                }
            }
        },
    };

    // Restore persisted tab list, if present. Failures are non-fatal: we
    // simply keep the default single-tab launch.
    //
    // Two-phase startup: refresh the diff eagerly only for the single restored
    // active tab. Every other tab is rebuilt as a stub (`needs_initial_refresh`
    // set) so a fresh launch with many tabs across many repos doesn't pay N
    // cold `git diff` calls before the window appears. Each stub is upgraded
    // when its tab gains focus (see `commands::kick_deferred_tab_refresh`).
    let mut deferred_tab_indices: Vec<usize> = Vec::new();
    if let Some(file) = tabs::load_tabs() {
        let active_idx = file.active_idx.min(file.tabs.len().saturating_sub(1));
        let mut rebuilt: Vec<er_engine::app::TabState> = Vec::new();
        for (i, d) in file.tabs.iter().enumerate() {
            let eager = i == active_idx;
            let result = if eager {
                tabs::rebuild_tab(d)
            } else {
                tabs::rebuild_tab_stub(d)
            };
            match result {
                Ok(t) => {
                    if !eager {
                        deferred_tab_indices.push(rebuilt.len());
                    }
                    rebuilt.push(t);
                }
                Err(e) => log::warn!(
                    "er-desktop: skipping persisted tab {:?} ({}): {e}",
                    d.kind,
                    d.repo_root
                ),
            }
        }
        if !rebuilt.is_empty() {
            app.tabs = rebuilt;
            app.active_tab = active_idx.min(app.tabs.len() - 1);
        }
    }

    // Register every unique repo root / remote referenced by open tabs.
    projects::sync_projects_from_tabs(&app.tabs);
    if projects::load().projects.is_empty() {
        let root = app.tab().repo_root.clone();
        if !root.is_empty() {
            let _ = projects::auto_register(&root);
        }
    }

    // Scope background tab warmup to the active project (or the restored active
    // tab's repo). Never warm cross-project stubs when active_id is missing.
    let warmer_scope_root: Option<String> = active_root_from_projects()
        .or_else(|| {
            app.tabs
                .get(app.active_tab)
                .map(|t| t.repo_root.clone())
                .filter(|r| !r.is_empty())
        })
        .or(cwd_repo_root);

    let pr_cache: Arc<Mutex<HashMap<String, Vec<PrInfo>>>> = Arc::new(Mutex::new(HashMap::new()));
    let pr_cache_fetched_at: Arc<Mutex<HashMap<String, u64>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let pr_open_cache: Arc<Mutex<HashMap<commands::PrOpenCacheKey, commands::PrOpenCacheEntry>>> =
        Arc::new(Mutex::new(
            pr_open_cache::load_persisted_pr_open_cache()
                .ok()
                .flatten()
                .unwrap_or_default(),
        ));
    let meta_cache: Arc<Mutex<HashMap<String, ProjectMeta>>> = Arc::new(Mutex::new(HashMap::new()));
    let gh_user: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    #[allow(clippy::type_complexity)]
    let gh_status_cache: Arc<Mutex<HashMap<(String, String, u64), GithubStatusSnapshot>>> =
        Arc::new(Mutex::new(
            gh_status_cache::load_persisted_gh_status_cache()
                .ok()
                .flatten()
                .unwrap_or_default(),
        ));
    let loading: LoadingState = Arc::new(Mutex::new(LoadingFlags::default()));
    let watch_status: WatchStatusState = Arc::new(Mutex::new(WatchStatusSnapshot::default()));
    let inbox = Arc::new(Mutex::new(inbox::load_inbox_state()));
    let tauri_app_handle: Arc<Mutex<Option<tauri::AppHandle>>> = Arc::new(Mutex::new(None));

    // Resolve the gh user login once at launch in a background thread.
    // Don't block startup; if it fails, leave as None.
    {
        let gh_user_bg = Arc::clone(&gh_user);
        std::thread::spawn(move || {
            if let Ok(out) = std::process::Command::new("gh")
                .args(["api", "user", "--jq", ".login"])
                .output()
            {
                if out.status.success() {
                    let login = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    if !login.is_empty() {
                        if let Ok(mut g) = gh_user_bg.lock() {
                            *g = Some(login);
                        }
                    }
                }
            }
        });
    }

    // Capture the launch-time active root for the first meta-refresh iteration.
    let launch_root: String = {
        let initial = app.tab().repo_root.clone();
        initial
    };

    let gh_status_in_flight: Arc<Mutex<std::collections::HashSet<(String, String, u64)>>> =
        Arc::new(Mutex::new(std::collections::HashSet::new()));
    let app_arc: Arc<Mutex<App>> = Arc::new(Mutex::new(app));
    let terminals: Arc<Mutex<HashMap<String, terminal::PtySession>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let terminals_for_exit = Arc::clone(&terminals);
    let desktop_revision: Arc<std::sync::atomic::AtomicU64> =
        Arc::new(std::sync::atomic::AtomicU64::new(0));
    if let Ok(mut app) = app_arc.lock() {
        arena_commands::attach_arena_notify(&mut app, Arc::clone(&desktop_revision));
    }
    let last_sent_content_revision: Arc<std::sync::atomic::AtomicU64> =
        Arc::new(std::sync::atomic::AtomicU64::new(u64::MAX));
    let last_sent_chrome_revision: Arc<std::sync::atomic::AtomicU64> =
        Arc::new(std::sync::atomic::AtomicU64::new(u64::MAX));
    let last_sent_reviewed_revision: Arc<std::sync::atomic::AtomicU64> =
        Arc::new(std::sync::atomic::AtomicU64::new(u64::MAX));
    let branch_base_remote_oid: Arc<Mutex<HashMap<String, String>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let state = AppState {
        app: Arc::clone(&app_arc),
        pr_cache: Arc::clone(&pr_cache),
        pr_cache_fetched_at: Arc::clone(&pr_cache_fetched_at),
        branch_base_remote_oid: Arc::clone(&branch_base_remote_oid),
        pr_open_cache: Arc::clone(&pr_open_cache),
        meta_cache: Arc::clone(&meta_cache),
        gh_user: Arc::clone(&gh_user),
        terminals: Arc::clone(&terminals),
        pending_ai_replies: Arc::new(Mutex::new(HashMap::new())),
        gh_status_cache: Arc::clone(&gh_status_cache),
        loading: Arc::clone(&loading),
        gh_status_in_flight: Arc::clone(&gh_status_in_flight),
        pr_open_prefetch_in_flight: Arc::new(Mutex::new(std::collections::HashSet::new())),
        desktop_revision: Arc::clone(&desktop_revision),
        last_sent_content_revision: Arc::clone(&last_sent_content_revision),
        last_sent_chrome_revision: Arc::clone(&last_sent_chrome_revision),
        last_sent_reviewed_revision: Arc::clone(&last_sent_reviewed_revision),
        watch_status: Arc::clone(&watch_status),
        inbox: Arc::clone(&inbox),
        tauri_app_handle: Arc::clone(&tauri_app_handle),
        auto_triage_in_flight: Arc::new(Mutex::new(std::collections::HashSet::new())),
        sent_files: Arc::new(Mutex::new(Default::default())),
    };

    match pr_cache::load_persisted_pr_cache() {
        Ok(Some((cached_prs, cached_fetched_at))) => {
            if let Ok(mut g) = pr_cache.lock() {
                *g = cached_prs;
            }
            if let Ok(mut g) = pr_cache_fetched_at.lock() {
                *g = cached_fetched_at;
            }
            profile_log::bump_desktop_revision(&desktop_revision, "pr_cache_restore");
        }
        Ok(None) => {}
        Err(e) => {
            log::warn!("failed to load persisted PR cache: {e}");
        }
    }

    // Prune both persisted caches to each project's `saved ∪ top-10 recent` PRs
    // so the on-disk files stay small across launches. Skip entirely when no
    // projects are known: an empty keep-set would wipe otherwise-valid caches
    // (e.g. a missing/corrupt projects.json), and the in-memory caps
    // (MAX_PR_OPEN_CACHE_ENTRIES; gh_status is memory-only at runtime) already
    // bound in-session growth. The two caches live in different key spaces, so
    // each is pruned against its own derived keep-set.
    {
        let projects_file = projects::load();
        if !projects_file.projects.is_empty() {
            use std::collections::HashSet;
            const KEEP_RECENT: usize = 10;
            let mut pr_open_keep: HashSet<(String, u64)> = HashSet::new();
            let mut gh_status_keep: HashSet<(String, String, u64)> = HashSet::new();
            for proj in &projects_file.projects {
                // gh_status keep-set holds lowercased (owner, repo). Cache keys
                // preserve the opening URL's casing (`tab.remote_repo`), so
                // `prune_gh_status_cache` folds case before comparing — the
                // keep-set must be lowercase, which `normalize_remote_slug` gives.
                let owner_repo = proj
                    .remote
                    .as_deref()
                    .map(projects::normalize_remote_slug)
                    .and_then(|slug| {
                        slug.split_once('/')
                            .map(|(o, r)| (o.to_string(), r.to_string()))
                    });
                let kept_numbers = proj
                    .saved_prs
                    .iter()
                    .map(|e| e.number)
                    .chain(proj.recent_prs.iter().take(KEEP_RECENT).map(|e| e.number));
                for number in kept_numbers {
                    pr_open_keep.insert((proj.id.clone(), number));
                    if let Some((owner, repo)) = owner_repo.as_ref() {
                        gh_status_keep.insert((owner.clone(), repo.clone(), number));
                    }
                }
            }
            if let Ok(mut map) = pr_open_cache.lock() {
                pr_open_cache::prune_pr_open_cache(&mut map, &pr_open_keep);
            }
            pr_open_cache::save_persisted_pr_open_cache(&pr_open_cache);
            if let Ok(mut map) = gh_status_cache.lock() {
                gh_status_cache::prune_gh_status_cache(&mut map, &gh_status_keep);
            }
            gh_status_cache::save_persisted_gh_status_cache(&gh_status_cache);
        }
    }

    // The 30s `gh_status` loop below now sleeps before its first iteration (to
    // avoid a startup gh burst), so a cold-restored active PR tab shows its
    // disk-cached status until that first tick (~30s) — an intentional tradeoff
    // to keep launch gh-free. We deliberately don't kick a refresh here; an
    // explicit tab open/switch/close still refreshes via `kick_active_gh_status`
    // (itself 10s-gated, sharing dedup state through `gh_status_in_flight`).

    // NOTE: a remote-PR auto-swap loop used to live here (300s for remote
    // tabs, 60s otherwise), calling snapshot_for_remote_diff_refresh →
    // fetch_remote_diff_data → apply_remote_diff_result. It was removed for
    // consistency: it auto-swapped the live diff for remote tabs only, while
    // local-PR tabs required manual Sync. Now BOTH require manual Sync, and
    // the 30s PR-head probe (above, "pr_head_probe") lights the stale pill
    // quickly for both. The underlying fetch/apply plumbing is retained in
    // crates/er-engine/src/app/state/remote_diff_sync.rs and crates/er-engine/src/sync.rs
    // for the manual Sync path (force_refresh_diff / refetch_and_refresh_diff).

    // Background base-branch staleness probe on a 60s cadence. The ONLY new
    // network cost for branch ("Local Diff") freshness. Mirrors the remote-PR
    // loop's three-phase shape so we never hold the App mutex across `git`:
    //   Phase 1 (brief lock) — read the active tab's identity; bail unless it's
    //                          a branch view (not remote, no PR, has a view).
    //   Phase 2 (no lock)    — `git ls-remote origin <base>` to learn origin's
    //                          tip. ANY failure (error/non-zero/empty) no-ops.
    //   Phase 3 (brief lock) — cache the oid; bump the revision ONLY when it
    //                          actually changed, so we don't churn polls.
    {
        let probe_app = Arc::clone(&app_arc);
        let probe_cache = Arc::clone(&branch_base_remote_oid);
        let probe_rev = Arc::clone(&desktop_revision);
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(60));

            // Phase 1: brief lock — capture the active branch tab's identity.
            let identity = {
                let guard = match probe_app.try_lock() {
                    Ok(g) => g,
                    Err(_) => continue,
                };
                let tab = guard.tab();
                if !tab.shows_branch_base_diff() {
                    None
                } else {
                    let base_short = tab.base_branch.trim_start_matches("origin/").to_string();
                    Some((tab.repo_root.clone(), base_short))
                }
            };
            let Some((repo_root, base_short)) = identity else {
                continue;
            };
            if base_short.is_empty() {
                continue;
            }

            // Phase 2: no lock — ask origin for the base branch tip.
            let out = std::process::Command::new("git")
                .args(["ls-remote", "origin", &base_short])
                .current_dir(&repo_root)
                .output();
            let oid = match out {
                Ok(o) if o.status.success() => {
                    let stdout = String::from_utf8_lossy(&o.stdout);
                    stdout
                        .lines()
                        .next()
                        .and_then(|line| line.split_whitespace().next())
                        .map(|s| s.to_string())
                        .filter(|s| !s.is_empty())
                }
                _ => None,
            };
            let Some(oid) = oid else {
                continue;
            };

            // Phase 3: brief lock — store + bump only on a real change.
            let key = format!("{repo_root}\u{0}{base_short}");
            let changed = match probe_cache.lock() {
                Ok(mut cache) => {
                    if cache.get(&key).map(|v| v.as_str()) == Some(oid.as_str()) {
                        false
                    } else {
                        cache.insert(key, oid);
                        true
                    }
                }
                Err(_) => false,
            };
            if changed {
                profile_log::bump_desktop_revision(&probe_rev, "branch_stale_probe");
            }
        });
    }

    // Background PR-head staleness probe on a 30s cadence. Detection only —
    // lights the "PR head updated on origin" pill fast (vs the 10-min pr_cache
    // sweep) for BOTH remote and local-PR tabs. The actual diff swap stays
    // manual (Sync button). Mirrors the branch-base probe's three-phase shape
    // so the App mutex is never held across `gh`:
    //   Phase 1 (brief lock) — read the active tab's identity; bail unless it's
    //                          a PR tab (remote_repo set OR pr_number set).
    //   Phase 2 (no lock)    — `gh pr view <n> --repo <owner/repo> --json
    //                          headRefOid --jq .headRefOid`. ANY failure no-ops
    //                          (preserves the "don't guess" stale-pill rule).
    //   Phase 3 (brief lock) — `patch_pr_head_oid` updates the single cache
    //                          entry in place; bump the revision ONLY when it
    //                          actually changed. Does NOT persist to disk (the
    //                          next 10-min sweep persists) so 30s is cheap.
    {
        let probe_app = Arc::clone(&app_arc);
        let probe_pr_cache = Arc::clone(&pr_cache);
        let probe_rev = Arc::clone(&desktop_revision);
        std::thread::spawn(move || {
            // (slug, pr_number) -> when we last actually probed. Throttles the
            // gh subprocess to once per 60s per PR even though the loop ticks
            // every 30s. Refreshed on EVERY successful probe, not only when the
            // OID changed — recording it conditionally would make the throttle a
            // no-op after the first window, since `elapsed()` would keep growing
            // against a frozen Instant and never re-arm.
            let mut probe_throttle: HashMap<(String, u64), std::time::Instant> = HashMap::new();
            loop {
                std::thread::sleep(std::time::Duration::from_secs(30));

                // Phase 1: brief lock — capture the active PR tab's identity.
                // `(repo_root, owner, repo, pr_number)`. For remote tabs use
                // `remote_repo`; for local-PR tabs resolve owner/repo from origin.
                let identity = {
                    let guard = match probe_app.try_lock() {
                        Ok(g) => g,
                        Err(_) => continue,
                    };
                    let tab = guard.tab();
                    let pr_number = match tab.pr_number {
                        Some(n) => n,
                        None => continue,
                    };
                    if let Some(slug) = tab.remote_repo.as_ref() {
                        if slug.split_once('/').is_some() {
                            Some((tab.repo_root.clone(), slug.clone(), pr_number))
                        } else {
                            None
                        }
                    } else if !tab.repo_root.is_empty() {
                        match er_engine::github::get_repo_info(&tab.repo_root) {
                            Ok((owner, repo)) => {
                                Some((tab.repo_root.clone(), format!("{owner}/{repo}"), pr_number))
                            }
                            Err(_) => None,
                        }
                    } else {
                        None
                    }
                };
                let Some((_repo_root, slug, pr_number)) = identity else {
                    continue;
                };

                // Throttle: skip the gh subprocess if this PR was probed
                // within the last 60s, regardless of whether the OID changed
                // last time.
                if probe_recently_done(
                    probe_throttle.get(&(slug.clone(), pr_number)),
                    std::time::Instant::now(),
                    std::time::Duration::from_secs(60),
                ) {
                    continue;
                }

                // Phase 2: no lock — ask gh for the PR's latest head oid.
                let out = std::process::Command::new("gh")
                    .args([
                        "pr",
                        "view",
                        &pr_number.to_string(),
                        "--repo",
                        &slug,
                        "--json",
                        "headRefOid",
                        "--jq",
                        ".headRefOid",
                    ])
                    .output();
                let oid = match out {
                    Ok(o) if o.status.success() => {
                        let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                        if s.is_empty() {
                            None
                        } else {
                            Some(s)
                        }
                    }
                    _ => None,
                };
                // A failed/empty probe is NOT recorded into probe_throttle —
                // a persistently-failing PR keeps retrying every 30s rather
                // than being throttled to 60s on top of already being broken.
                let Some(oid) = oid else { continue };

                // Phase 3: brief lock — patch the cache entry; bump only on change.
                let changed = pr_cache::patch_pr_head_oid(&probe_pr_cache, &slug, pr_number, &oid);

                // Re-arm the 60s throttle window on every successful probe.
                probe_throttle.insert((slug.clone(), pr_number), std::time::Instant::now());

                if changed {
                    profile_log::bump_desktop_revision(&probe_rev, "pr_head_probe");
                }
            }
        });
    }

    // Spawn background GitHub-status refresh: every 30s, identify the active
    // tab's PR identity (briefly locking app), then shell out to gh and update
    // the cache. NEVER holds the app mutex while running gh.
    // Shares gh_status_in_flight with kick_github_status_refresh to avoid
    // duplicate concurrent fetches when a branch switch fires during this loop.
    {
        let gh_status_bg = Arc::clone(&gh_status_cache);
        let app_bg = Arc::clone(&app_arc);
        let pr_cache_bg = Arc::clone(&pr_cache);
        let gh_status_loading = Arc::clone(&loading);
        let gh_status_in_flight_bg = Arc::clone(&gh_status_in_flight);
        let gh_status_desktop_rev = Arc::clone(&desktop_revision);
        std::thread::spawn(move || loop {
            // Sleep FIRST: kills the startup burst (this loop used to fire
            // immediately on launch because the sleep sat at the bottom).
            std::thread::sleep(std::time::Duration::from_secs(30));

            // Snapshot identity in a short critical section.
            let key: Option<(String, String, u64)> = match app_bg.lock() {
                Ok(g) => {
                    let tab = g.tab();
                    // Remote PR tab — use remote_repo + pr_number directly.
                    if let (Some(slug), Some(n)) = (tab.remote_repo.as_ref(), tab.pr_number) {
                        slug.split_once('/')
                            .map(|(o, r)| (o.to_string(), r.to_string(), n))
                    } else {
                        // Working-tree or local-branch tab — look up by head ref.
                        let branch = tab
                            .local_branch_view
                            .as_deref()
                            .unwrap_or(&tab.current_branch)
                            .to_string();
                        pr_cache_bg.lock().ok().and_then(|cache| {
                            cache.iter().find_map(|(slug, prs)| {
                                prs.iter()
                                    .filter(|p| p.head_ref == branch)
                                    .min_by_key(|p| if p.state == "OPEN" { 0 } else { 1 })
                                    .and_then(|p| {
                                        slug.split_once('/')
                                            .map(|(o, r)| (o.to_string(), r.to_string(), p.number))
                                    })
                            })
                        })
                    }
                }
                Err(_) => None,
            };
            // Lock released. Skip the fetch entirely when the cached snapshot
            // for this key is still fresh (<90s old) — most ticks on an idle
            // PR now do nothing at all.
            let key = key.filter(|(owner, repo, number)| {
                let last_updated = gh_status_bg.lock().ok().and_then(|cache| {
                    cache
                        .get(&(owner.clone(), repo.clone(), *number))
                        .and_then(|snap| snap.last_updated.clone())
                });
                !gh_status_cache::status_is_fresh(
                    last_updated.as_deref(),
                    gh_status_cache::now_epoch_secs(),
                    90,
                )
            });
            // Register in-flight and shell out — skip if already fetching.
            if let Some((owner, repo, number)) = key {
                let registered = gh_status_in_flight_bg
                    .lock()
                    .map(|mut s| s.insert((owner.clone(), repo.clone(), number)))
                    .unwrap_or(false);
                if registered {
                    if let Ok(mut f) = gh_status_loading.lock() {
                        f.gh_status = true;
                    }
                    if let Some(snap) = commands::fetch_github_status(&owner, &repo, number) {
                        if let Ok(mut g) = gh_status_bg.lock() {
                            g.insert((owner.clone(), repo.clone(), number), snap);
                        }
                        profile_log::bump_desktop_revision(
                            &gh_status_desktop_rev,
                            "gh_status_cache",
                        );
                    }
                    if let Ok(mut f) = gh_status_loading.lock() {
                        f.gh_status = false;
                    }
                    let _ = gh_status_in_flight_bg
                        .lock()
                        .map(|mut s| s.remove(&(owner, repo, number)));
                }
            }
        });
    }

    // Spawn background GitHub comment sync. Three-phase design so network I/O
    // never holds the app mutex:
    //   Phase 1 (brief lock) — snapshot identity + files from active tab.
    //   Phase 2 (no lock)   — fetch comments, review threads, PR overview.
    //   Phase 3 (brief lock) — apply results to the correct tab.
    {
        let comments_app = Arc::clone(&app_arc);
        let comments_pr_cache = Arc::clone(&pr_cache);
        let comments_loading = Arc::clone(&loading);
        let comments_desktop_rev = Arc::clone(&desktop_revision);
        std::thread::spawn(move || {
            // (owner, repo, number) -> (head OID synced against, when).
            // Throttles the comment/review pull: within `ttl` of a sync against
            // the same head OID the 45s tick skips the network I/O. Comments and
            // reviews are independent of pushes, so `ttl` is the worst-case
            // comment-panel latency on a push-idle PR — NOT a "nothing changed"
            // guarantee. A push changes head_oid (via the pr-head-probe loop,
            // itself throttled to ~60s), so a fresh push resyncs within ~1-2 min
            // instead of waiting out `ttl`. Manual sync (`G` key →
            // `pull_github_comments` → `App::sync_github_comments`) is a separate
            // synchronous path that never touches this loop or map, unaffected.
            let mut last_synced: HashMap<(String, String, u64), (String, std::time::Instant)> =
                HashMap::new();
            loop {
                std::thread::sleep(std::time::Duration::from_secs(45));

                // Phase 1: brief lock — snapshot identity + files, then release.
                let resolved = {
                    let guard = match comments_app.try_lock() {
                        Ok(g) => g,
                        Err(_) => continue,
                    };
                    let tab = guard.tab();
                    let identity: Option<(String, String, u64)> = if let (Some(slug), Some(n)) =
                        (tab.remote_repo.as_ref(), tab.pr_number)
                    {
                        slug.split_once('/')
                            .map(|(o, r)| (o.to_string(), r.to_string(), n))
                    } else {
                        let branch = tab
                            .local_branch_view
                            .as_deref()
                            .unwrap_or(&tab.current_branch)
                            .to_string();
                        comments_pr_cache.lock().ok().and_then(|cache| {
                            cache.iter().find_map(|(slug, prs)| {
                                prs.iter()
                                    .filter(|p| p.head_ref == branch)
                                    .min_by_key(|p| if p.state == "OPEN" { 0 } else { 1 })
                                    .and_then(|p| {
                                        slug.split_once('/')
                                            .map(|(o, r)| (o.to_string(), r.to_string(), p.number))
                                    })
                            })
                        })
                    };

                    // Throttle gate: same head OID already synced within the
                    // window → skip this tick (bounds comment-panel latency to
                    // ~90s on a push-idle PR; does not guarantee nothing
                    // changed). An empty/unknown OID never skips (fail open —
                    // keep syncing every tick until the probe loop populates
                    // pr_cache).
                    identity.and_then(|(owner, repo, number)| {
                        let head_oid = cached_head_oid(&comments_pr_cache, &owner, &repo, number);
                        let recently_synced = comment_sync_recently_synced(
                            last_synced.get(&(owner.clone(), repo.clone(), number)),
                            &head_oid,
                            std::time::Instant::now(),
                            std::time::Duration::from_secs(90),
                        );
                        if recently_synced {
                            None
                        } else {
                            // Snapshot all data needed for the fetch — releases
                            // the lock after this block. Carry the gate-time
                            // head_oid forward (instead of re-reading it after
                            // the fetch) so a push landing mid-fetch isn't
                            // mistaken for "already synced" on the next tick.
                            Some((
                                guard.snapshot_for_comment_sync(owner, repo, number),
                                head_oid,
                            ))
                        }
                    })
                };

                // Phase 2: network I/O — no lock held.
                if let Some((ctx, head_oid)) = resolved {
                    if let Ok(mut f) = comments_loading.lock() {
                        f.gh_comments = true;
                    }
                    let applied = match er_engine::app::fetch_comment_sync_data(&ctx) {
                        Ok(result) => {
                            // Phase 3: brief lock — apply pre-fetched results to the correct tab.
                            match comments_app.lock() {
                                Ok(mut g) => {
                                    g.apply_comment_sync_result(result);
                                    true
                                }
                                Err(e) => {
                                    log::error!("comment sync apply lock failed: {e}");
                                    false
                                }
                            }
                        }
                        Err(e) => {
                            log::error!("github comment sync failed: {e}");
                            false
                        }
                    };
                    if let Ok(mut f) = comments_loading.lock() {
                        f.gh_comments = false;
                    }
                    if applied {
                        last_synced.insert(
                            (ctx.owner.clone(), ctx.repo_name.clone(), ctx.pr_number),
                            (head_oid, std::time::Instant::now()),
                        );
                        profile_log::bump_desktop_revision(
                            &comments_desktop_rev,
                            "comment_sync_applied",
                        );
                    }
                }
            }
        });
    }

    // Spawn the desktop active-branch watcher. Follows the currently active
    // tab's local-branch checkout (project root or linked worktree) and
    // refreshes the diff when tracked files in that checkout change.
    {
        let watcher_app = Arc::clone(&app_arc);
        let watcher_status = Arc::clone(&watch_status);
        let watcher_desktop_rev = Arc::clone(&desktop_revision);
        #[allow(unused_assignments)]
        std::thread::spawn(move || {
            use er_engine::watch::{FileWatcher, WatchEvent};
            use std::path::Path;
            use std::sync::mpsc;

            let (tx, rx) = mpsc::channel::<WatchEvent>();
            // Held only for its Drop side effect: dropping stops the watcher.
            #[allow(unused_assignments, unused_variables)]
            let mut watcher: Option<FileWatcher> = None;
            let mut current_key: Option<(String, String)> = None;
            let poll_interval = std::time::Duration::from_millis(400);

            loop {
                let desired = match watcher_app.lock() {
                    Ok(g) => desired_local_branch_watch(&g),
                    Err(_) => None,
                };

                if desired != current_key {
                    watcher = None; // drop old watcher
                    if let Some((ref branch, ref root_path)) = desired {
                        match FileWatcher::new(Path::new(root_path), 250, tx.clone()) {
                            Ok(w) => {
                                watcher = Some(w);
                                if let Ok(mut s) = watcher_status.lock() {
                                    *s = WatchStatusSnapshot {
                                        active: true,
                                        branch: Some(branch.clone()),
                                        root_path: Some(root_path.clone()),
                                    };
                                }
                            }
                            Err(e) => {
                                log::error!(
                                    "er-desktop: active-branch watcher failed for {root_path}: {e}"
                                );
                                if let Ok(mut s) = watcher_status.lock() {
                                    *s = WatchStatusSnapshot::default();
                                }
                            }
                        }
                    } else if let Ok(mut s) = watcher_status.lock() {
                        *s = WatchStatusSnapshot::default();
                    }

                    // Mirror checkout root onto the active tab so refresh_diff
                    // uses the working-tree helper.
                    if let Ok(mut g) = watcher_app.lock() {
                        let checkout_root = desired.as_ref().map(|(_, r)| r.clone());
                        let desired_branch = desired.as_ref().map(|(b, _)| b.clone());
                        if active_tab_watched_branch(&g) == desired_branch {
                            g.tab_mut().local_branch_checkout_root = checkout_root;
                        }
                    }
                    current_key = desired;
                    profile_log::bump_desktop_revision(&watcher_desktop_rev, "watcher_status");
                }

                // Drain any pending watch events. Coalesce — we only need to
                // know "something changed" to trigger one refresh.
                let mut got_event = false;
                match rx.recv_timeout(poll_interval) {
                    Ok(WatchEvent::FilesChanged(_)) => {
                        got_event = true;
                        while let Ok(WatchEvent::FilesChanged(_)) = rx.try_recv() {}
                    }
                    Err(mpsc::RecvTimeoutError::Timeout)
                    | Err(mpsc::RecvTimeoutError::Disconnected) => {}
                }

                if got_event {
                    if let Some((watched_branch, _root_path)) = current_key.clone() {
                        let app = Arc::clone(&watcher_app);
                        let rev = Arc::clone(&watcher_desktop_rev);
                        std::thread::spawn(move || {
                            let result = app.lock().ok().and_then(|mut g| {
                                if active_tab_watched_branch(&g).as_deref()
                                    != Some(watched_branch.as_str())
                                {
                                    return None;
                                }
                                Some(g.tab_mut().refresh_diff_quick())
                            });
                            match result {
                                Some(Ok(())) => {
                                    profile_log::bump_desktop_revision(&rev, "watcher_refresh");
                                }
                                Some(Err(e)) => {
                                    log::error!("active-branch watcher refresh failed: {e}");
                                }
                                None => {}
                            }
                        });
                    }
                }
            }
        });
    }

    // Global notifications refresh across ALL configured project remotes.
    // Startup fetch + conservative 10-minute cadence (not high-frequency polling).
    let bg_cache = Arc::clone(&pr_cache);
    let bg_fetched_at = Arc::clone(&pr_cache_fetched_at);
    let bg_loading = Arc::clone(&loading);
    let bg_desktop_rev = Arc::clone(&desktop_revision);
    let bg_gh_user = Arc::clone(&gh_user);
    let bg_inbox = Arc::clone(&inbox);
    let bg_handle = Arc::clone(&tauri_app_handle);
    std::thread::spawn(move || {
        // Inbox native notifications need `tauri_app_handle`; setup stores it after
        // this thread starts (release builds are especially tight on timing).
        for _ in 0..200 {
            if bg_handle.lock().ok().and_then(|g| g.clone()).is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime");
        let run_refresh = || {
            if let Ok(mut f) = bg_loading.lock() {
                f.pr_list = true;
            }
            let failed =
                rt.block_on(async { pr_cache::refresh_pr_cache(&bg_cache, &bg_fetched_at).await });
            for remote in failed {
                commands::process_inbox_after_pr_refresh(
                    &bg_cache,
                    &bg_gh_user,
                    &bg_inbox,
                    &bg_desktop_rev,
                    &bg_handle,
                    Some(remote),
                    None,
                );
            }
            commands::process_inbox_after_pr_refresh(
                &bg_cache,
                &bg_gh_user,
                &bg_inbox,
                &bg_desktop_rev,
                &bg_handle,
                None,
                None,
            );
            if let Ok(mut f) = bg_loading.lock() {
                f.pr_list = false;
            }
            profile_log::bump_desktop_revision(&bg_desktop_rev, "pr_cache_refresh");
        };

        // Startup: fetch the active project's remote first so its sidebar PR
        // list lights up quickly. The full multi-remote sweep follows.
        if let Some(active_remote) = pr_cache::active_project_remote() {
            if let Ok(mut f) = bg_loading.lock() {
                f.pr_list = true;
            }
            rt.block_on(async {
                pr_cache::refresh_pr_cache_for_remote(&active_remote, &bg_cache, &bg_fetched_at)
                    .await
            });
            commands::process_inbox_after_pr_refresh(
                &bg_cache,
                &bg_gh_user,
                &bg_inbox,
                &bg_desktop_rev,
                &bg_handle,
                Some(active_remote),
                None,
            );
            if let Ok(mut f) = bg_loading.lock() {
                f.pr_list = false;
            }
            profile_log::bump_desktop_revision(&bg_desktop_rev, "pr_cache_active_remote");
        }

        // Full sweep across all remotes — skip when persisted cache is still fresh.
        if pr_cache::startup_full_refresh_due(&bg_fetched_at) {
            run_refresh();
        } else {
            log::info!("pr_list: skipping startup full sweep (cache still fresh)");
        }

        // Conservative cadence: every 10 minutes.
        loop {
            std::thread::sleep(std::time::Duration::from_secs(600));
            run_refresh();
        }
    });

    // Spawn background meta-cache refresh: keeps per-project git metadata
    // (branches, worktrees, current/base branch) fresh without ever taking
    // the AppState.app mutex.
    let bg_meta = Arc::clone(&meta_cache);
    let meta_desktop_rev = Arc::clone(&desktop_revision);
    let meta_app = Arc::clone(&app_arc);
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime");
        rt.block_on(async move {
            // Startup: refresh only the active project so its sidebar is ready
            // ASAP. Then a short delay before the full sweep covers everyone else.
            let active_id = projects::load().active_id.clone();
            if let Some(id) = active_id.as_deref() {
                if snapshot::refresh_meta_cache_for_project(id, &bg_meta) {
                    profile_log::bump_desktop_revision(&meta_desktop_rev, "meta_startup_project");
                }
            }
            // First full pass — slight delay so we don't compete with the
            // active tab's diff refresh for git CPU during the first frame.
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            if snapshot::refresh_meta_cache(&launch_root, &bg_meta) {
                profile_log::bump_desktop_revision(&meta_desktop_rev, "meta_startup_full");
            }
            loop {
                profile_log::profile_log("bg_loop", &[("loop", "meta".to_string())]);
                let interval_secs = if meta_app.try_lock().is_ok_and(|g| g.tab().is_remote()) {
                    120
                } else {
                    60
                };
                tokio::time::sleep(std::time::Duration::from_secs(interval_secs)).await;
                let active_id = projects::load().active_id.clone();
                if let Some(id) = active_id.as_deref() {
                    if snapshot::refresh_meta_cache_for_project(id, &bg_meta) {
                        profile_log::bump_desktop_revision(&meta_desktop_rev, "meta_tick");
                    }
                }
            }
        });
    });

    // Background warmer: refresh persisted tab stubs that belong to the
    // **active project**. Tabs from other projects stay stubs until the user
    // focuses them (`commands::kick_deferred_tab_refresh` loads the diff then).
    // Avoids paying `git diff` cost on launch for repos the user didn't open.
    if !deferred_tab_indices.is_empty() {
        let warmer_app = Arc::clone(&app_arc);
        let warmer_rev = Arc::clone(&desktop_revision);
        std::thread::spawn(move || {
            // Brief grace period so the active tab's diff + the first frame land
            // before we start consuming CPU on background tabs.
            std::thread::sleep(std::time::Duration::from_secs(2));
            loop {
                let next_idx: Option<usize> = match warmer_app.try_lock() {
                    Ok(g) => g.tabs.iter().position(|t| {
                        if !t.needs_initial_refresh {
                            return false;
                        }
                        warmer_scope_root
                            .as_deref()
                            .is_some_and(|root| t.repo_root == root)
                    }),
                    Err(_) => None,
                };
                let Some(idx) = next_idx else {
                    break;
                };
                let Ok(mut g) = warmer_app.lock() else { break };
                if idx >= g.tabs.len() || !g.tabs[idx].needs_initial_refresh {
                    continue;
                }
                g.tabs[idx].needs_initial_refresh = false;
                let t = std::time::Instant::now();
                let is_local_pr = g.tabs[idx].pr_number.is_some() && !g.tabs[idx].is_remote();
                let res = if is_local_pr {
                    g.tabs[idx].refetch_and_refresh_diff()
                } else {
                    g.tabs[idx].refresh_diff()
                };
                drop(g);
                match res {
                    Ok(()) => {
                        profile_log::profile_log(
                            "background_tab_warmup",
                            &[
                                ("tab_idx", idx.to_string()),
                                ("ms", t.elapsed().as_millis().to_string()),
                            ],
                        );
                        profile_log::bump_desktop_revision(&warmer_rev, "background_tab_warmup");
                    }
                    Err(e) => log::warn!("background tab warmup failed: {e}"),
                }
                // Yield between tabs so the UI thread can grab the mutex if needed.
                std::thread::sleep(std::time::Duration::from_millis(150));
            }
        });
    }

    let persist_app = Arc::clone(&app_arc);
    let persist_app_on_close = Arc::clone(&app_arc);
    // Throttles eager geometry saves on move/resize (see the on_window_event
    // handler below). Init to now() so the programmatic resizes fired during the
    // startup restore_state don't trigger a redundant save.
    let window_geom_last_save = Arc::new(Mutex::new(std::time::Instant::now()));
    // Gates eager saves until the window has been placed + shown. The plugin's
    // restore_state fires Moved/Resized while it sets the (possibly off-screen,
    // pre-clamp) position; saving then would persist that bad geometry. Flipped
    // true at the end of the startup reveal.
    let window_ready = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let window_ready_event = Arc::clone(&window_ready);
    let tauri_app = tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .filter(|metadata| dev_log::enabled_for_log_target(metadata.target()))
                .build(),
        )
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(window_state_save_flags())
                .skip_initial_state("main")
                .build(),
        )
        .manage(state)
        .manage(BrowserWebviewState::default())
        // `erp://host/path` proxies `http://host/path`; `erps://host/path`
        // proxies `https://host/path`. HTML responses get the annotation script.
        .register_uri_scheme_protocol("erp", |_app, request| proxied_response(&request, "http"))
        .register_uri_scheme_protocol("erps", |_app, request| proxied_response(&request, "https"))
        .on_window_event(move |window, event| {
            if window.label() != "main" {
                return;
            }
            match event {
                tauri::WindowEvent::CloseRequested { .. } => {
                    if let Ok(guard) = persist_app_on_close.lock() {
                        tabs::persist_app_tabs(&guard);
                    }
                }
                // The window-state plugin only writes geometry to disk on a
                // graceful RunEvent::Exit; move/resize merely update its in-memory
                // cache. A `tauri dev` rebuild (or a crash) hard-kills the app, so
                // that cache is lost and the window reopens at the last *cleanly*
                // saved geometry. Persist eagerly here (throttled to one write per
                // 400ms) so the last position+size survive rebuilds and crashes.
                tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_) => {
                    use tauri_plugin_window_state::AppHandleExt;
                    if window_ready_event.load(std::sync::atomic::Ordering::Acquire) {
                        if let Ok(mut last) = window_geom_last_save.lock() {
                            if last.elapsed() >= std::time::Duration::from_millis(400) {
                                *last = std::time::Instant::now();
                                let _ = window
                                    .app_handle()
                                    .save_window_state(window_state_save_flags());
                            }
                        }
                    }
                }
                _ => {}
            }
        })
        .setup(move |app| {
            if let Some(state) = app.try_state::<AppState>() {
                if let Ok(mut h) = state.tauri_app_handle.lock() {
                    *h = Some(app.handle().clone());
                }
                commands::prepare_macos_notifications(app.handle());
                commands::flush_pending_native_notifications(&state.inbox, &state.tauri_app_handle);
                // Revision watcher: emit a Tauri event whenever the backend's
                // `desktop_revision` advances. The frontend listens and only
                // calls `poll` on demand instead of every 2s — cuts idle
                // backend mutex/lock contention to ~zero, and lets the UI
                // react within ~50ms of a real change.
                let watch_handle = app.handle().clone();
                let watch_rev = Arc::clone(&state.desktop_revision);
                std::thread::spawn(move || {
                    use tauri::Emitter;
                    let mut last_emitted = u64::MAX;
                    loop {
                        let current = watch_rev.load(std::sync::atomic::Ordering::Relaxed);
                        if current != last_emitted {
                            // Brief debounce to coalesce bursts (e.g. several
                            // background threads bumping the revision at once).
                            std::thread::sleep(std::time::Duration::from_millis(40));
                            let coalesced = watch_rev.load(std::sync::atomic::Ordering::Relaxed);
                            let delta_rev = coalesced.wrapping_sub(last_emitted);
                            last_emitted = coalesced;
                            if let Err(e) = watch_handle.emit("er://revision", coalesced) {
                                log::warn!("revision watcher emit failed: {e}");
                            } else {
                                profile_log::profile_log(
                                    "revision_emit",
                                    &[
                                        ("coalesced_rev", coalesced.to_string()),
                                        ("delta_rev", delta_rev.to_string()),
                                    ],
                                );
                            }
                        }
                        std::thread::sleep(std::time::Duration::from_millis(80));
                    }
                });
            }

            install_app_menu(app.handle())?;

            #[cfg(target_os = "macos")]
            {
                use tauri::ActivationPolicy;
                app.set_activation_policy(ActivationPolicy::Regular);
            }

            let window_builder = tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::App("index.html".into()),
            )
            .title("Easy Review")
            .inner_size(1400.0, 900.0)
            .min_inner_size(900.0, 600.0);
            #[cfg(target_os = "macos")]
            let window_builder = window_builder
                .title_bar_style(tauri::TitleBarStyle::Overlay)
                .hidden_title(true);
            let window = window_builder
                .visible(false)
                .transparent(true)
                .initialization_script_for_all_frames(FRAME_SCRIPT)
                .on_navigation(main_webview_policy::handle_main_webview_navigation)
                .on_new_window(|url, _features| {
                    main_webview_policy::handle_main_webview_new_window(&url)
                })
                .build()?;

            use tauri_plugin_window_state::WindowExt;
            // Restore size+position only — NOT visibility. The window stays
            // hidden (`.visible(false)`) until we've clamped it onto a valid
            // monitor, then we show. Restoring VISIBLE would flash the window at
            // the stale off-screen position.
            window.restore_state(window_state_restore_flags())?;
            reveal_main_window(&window, app.handle(), "startup")?;
            // The window is placed and shown; eager geometry saves may now run.
            window_ready.store(true, std::sync::atomic::Ordering::Release);
            // Heal any stale off-screen/maximized geometry the restore clamped
            // away: persist the now-valid on-screen position so the next launch
            // restores it directly instead of re-clamping a poisoned prev_x.
            {
                use tauri_plugin_window_state::AppHandleExt;
                let _ = app.handle().save_window_state(window_state_save_flags());
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::start_window_drag,
            commands::get_snapshot,
            commands::toggle_panel,
            commands::request_file_content,
            commands::select_file,
            commands::next_file,
            commands::prev_file,
            commands::jump_to_unreviewed,
            commands::set_mode,
            commands::toggle_reviewed,
            commands::mark_reviewed,
            commands::unmark_reviewed,
            commands::bulk_review_pillar,
            commands::unbulk_review_pillar,
            commands::generate_tour,
            commands::open_in_editor,
            commands::open_in_vscode,
            commands::open_source,
            commands::open_url_in_browser,
            commands::reveal_er_folder,
            commands::reveal_path,
            commands::list_review_revisions,
            commands::read_review_json,
            commands::next_hunk,
            commands::prev_hunk,
            commands::toggle_compacted,
            commands::set_filter,
            commands::clear_filter,
            commands::add_comment,
            commands::add_question,
            commands::add_note,
            commands::reply_to_thread,
            commands::delete_thread,
            commands::resolve_thread,
            commands::refresh_diff,
            commands::force_refresh_diff,
            commands::refresh_github_status,
            commands::pull_github_comments,
            commands::push_github_comments,
            commands::push_github_comment_thread,
            commands::push_github_comment_reply,
            commands::submit_github_review,
            commands::submit_github_pr_decision,
            commands::post_github_pr_comment,
            commands::run_ai_review,
            commands::run_ai_expert_review,
            commands::run_ai_professor_review,
            commands::run_ai_triage_review,
            commands::run_pr_triage,
            commands::cancel_queued_review,
            commands::run_branch_triage,
            commands::list_ai_experts,
            commands::list_ai_reviewers,
            commands::run_ai_review_files,
            commands::run_ai_scoped_review,
            commands::run_ai_validate,
            commands::list_diff_paths,
            commands::set_ai_model,
            commands::list_ai_providers,
            arena_commands::arena_estimate,
            arena_commands::arena_start,
            arena_commands::arena_start_batch,
            arena_commands::arena_estimate_batch,
            arena_commands::arena_accept_findings,
            arena_commands::arena_progress,
            arena_commands::arena_get,
            arena_commands::arena_list,
            arena_commands::arena_delete,
            arena_commands::arena_cancel,
            arena_commands::arena_override,
            arena_commands::dev_log_filter,
            commands::set_ai_selection,
            config_commands::get_config_hub,
            config_commands::apply_config_patch,
            config_commands::save_config_global_cmd,
            config_commands::set_ai_hub_defaults,
            commands::set_ai_effort,
            commands::promote_to_comment,
            commands::promote_to_note,
            commands::ask_ai,
            commands::validate_with_ai,
            commands::elaborate_with_ai,
            commands::open_pr_url,
            commands::open_remote_pr,
            commands::open_worktree,
            commands::dismiss_finding,
            commands::delete_review_artifact,
            commands::remove_finding_thread,
            commands::promote_finding_to_comment,
            commands::reply_to_finding,
            commands::update_thread_message,
            commands::update_finding_response,
            commands::delete_finding_response,
            commands::export_review,
            commands::export_review_to_file,
            commands::export_to_agent,
            commands::open_commit_composer,
            commands::select_commit,
            commands::poll,
            commands::open_local_branch,
            commands::open_pr_branch,
            commands::open_pr_review,
            commands::prefetch_pr_open,
            commands::refresh_pr_list,
            commands::refresh_project_pr_list,
            commands::open_inbox_item,
            commands::mark_inbox_item_read,
            commands::mark_all_inbox_read,
            commands::clear_read_inbox_items,
            commands::refresh_notifications,
            commands::dismiss_remote_pr,
            commands::undismiss_remote_pr,
            commands::sync_pr,
            commands::sync_branch,
            commands::track_pr,
            commands::untrack_pr,
            commands::save_pr,
            commands::unsave_pr,
            commands::list_available_prs,
            commands::set_active_project,
            commands::set_project_auto_triage,
            commands::set_project_auto_triage_own_prs,
            commands::patch_project_review_settings,
            commands::add_tracked_branch,
            commands::remove_tracked_branch,
            commands::list_available_branches,
            commands::delete_project,
            commands::open_project_branch,
            commands::new_tab,
            commands::close_tab,
            commands::select_tab,
            commands::reorder_tabs,
            commands::add_ui_annotation,
            commands::delete_ui_annotation,
            commands::clear_ui_annotations,
            commands::clear_ui_annotations_for_page,
            commands::update_tab_browser,
            commands::cycle_tab_browser_layout,
            commands::list_ui_annotations,
            commands::update_ui_annotation_anchors,
            commands::save_annotation_screenshot,
            commands::read_annotation_screenshot,
            commands::terminal_spawn,
            commands::terminal_write,
            commands::terminal_resize,
            commands::terminal_close,
            commands::detect_dev_url,
            commands::get_background_task_log,
            browser_webview::browser_ensure,
            browser_webview::browser_hide,
            browser_webview::browser_suspend_for_overlay,
            browser_webview::browser_set_bounds,
            browser_webview::browser_navigate,
            browser_webview::browser_host_message,
            browser_webview::browser_send_to_page,
            browser_webview::browser_set_annotate_mode,
            browser_webview::browser_reload,
        ])
        .build(tauri::generate_context!())
        .expect("error building tauri application");

    tauri_app.run(move |_handle, event| {
        match event {
            tauri::RunEvent::ExitRequested { .. } => {
                if let Ok(guard) = persist_app.lock() {
                    tabs::persist_app_tabs(&guard);
                }
                // Kill any live shell sessions. Drop runs PtySession::drop on
                // each, which kills + reaps the child.
                if let Ok(mut map) = terminals_for_exit.lock() {
                    map.clear();
                }
            }
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen {
                has_visible_windows,
                ..
            } => {
                log::info!(
                    "macOS reopen event received; has_visible_windows={has_visible_windows}"
                );
                reveal_main_window_from_handle(_handle, "macos_reopen");
            }
            _ => {}
        }
    });
}

/// Checkout root + branch for the active tab when that branch is checked out
/// (project root HEAD or a linked worktree). Returns None for remote tabs or
/// tabs whose branch isn't checked out anywhere. PR tabs are allowed: if their
/// head branch happens to be checked out, the watcher attaches a checkout root
/// so Unstaged/Staged/working-tree Branch views become available (matching the
/// Tracked-branch entry point).
fn desired_local_branch_watch(app: &App) -> Option<(String, String)> {
    let tab = app.tab();
    if tab.remote_repo.is_some() {
        return None;
    }

    let branch = tab
        .local_branch_view
        .clone()
        .unwrap_or_else(|| tab.current_branch.clone());
    let root = commands::resolve_head_checkout(&tab.repo_root, &branch)?;
    Some((branch, root))
}

/// Branch name the active tab is reviewing (for watch refresh guard).
fn active_tab_watched_branch(app: &App) -> Option<String> {
    let tab = app.tab();
    if tab.remote_repo.is_some() {
        return None;
    }
    let branch = tab
        .local_branch_view
        .clone()
        .unwrap_or_else(|| tab.current_branch.clone());
    if branch.is_empty() {
        None
    } else {
        Some(branch)
    }
}

/// Read the persisted active project's root path, if any.
fn active_root_from_projects() -> Option<String> {
    let file = projects::load();
    let active_id = file.active_id.as_ref()?;
    file.projects
        .iter()
        .find(|p| &p.id == active_id)
        .map(|p| p.root_path.clone())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    // ── comment_sync_recently_synced (finding 2a skip gate) ──
    // `base` is the synced-at instant; `base + elapsed` is "now", so the gate
    // sees exactly `elapsed` since the last sync without depending on wall time.

    #[test]
    fn comment_sync_skips_when_same_oid_within_ttl() {
        let base = Instant::now();
        let entry = ("oid-abc".to_string(), base);
        assert!(comment_sync_recently_synced(
            Some(&entry),
            "oid-abc",
            base + Duration::from_secs(100),
            Duration::from_secs(300),
        ));
    }

    #[test]
    fn comment_sync_syncs_when_same_oid_but_ttl_expired() {
        let base = Instant::now();
        let entry = ("oid-abc".to_string(), base);
        assert!(!comment_sync_recently_synced(
            Some(&entry),
            "oid-abc",
            base + Duration::from_secs(400),
            Duration::from_secs(300),
        ));
    }

    #[test]
    fn comment_sync_syncs_when_oid_differs_even_if_recent() {
        // A new push (different head OID) must sync immediately regardless of TTL.
        let base = Instant::now();
        let entry = ("oid-old".to_string(), base);
        assert!(!comment_sync_recently_synced(
            Some(&entry),
            "oid-new",
            base + Duration::from_secs(5),
            Duration::from_secs(300),
        ));
    }

    #[test]
    fn comment_sync_never_skips_on_empty_oid() {
        // Fail open: an unknown/empty cached OID must keep syncing every tick.
        let base = Instant::now();
        let entry = ("".to_string(), base);
        assert!(!comment_sync_recently_synced(
            Some(&entry),
            "",
            base + Duration::from_secs(1),
            Duration::from_secs(300),
        ));
    }

    #[test]
    fn comment_sync_never_skips_with_no_prior_entry() {
        assert!(!comment_sync_recently_synced(
            None,
            "oid-abc",
            Instant::now(),
            Duration::from_secs(300),
        ));
    }

    // ── probe_recently_done (finding 3 throttle) ──

    #[test]
    fn probe_skips_within_throttle_window() {
        let base = Instant::now();
        assert!(probe_recently_done(
            Some(&base),
            base + Duration::from_secs(30),
            Duration::from_secs(60),
        ));
    }

    #[test]
    fn probe_runs_after_throttle_window() {
        let base = Instant::now();
        assert!(!probe_recently_done(
            Some(&base),
            base + Duration::from_secs(90),
            Duration::from_secs(60),
        ));
    }

    #[test]
    fn probe_at_exactly_ttl_boundary_runs() {
        // age == ttl → `<` excludes it → not throttled (probe runs).
        let base = Instant::now();
        assert!(!probe_recently_done(
            Some(&base),
            base + Duration::from_secs(60),
            Duration::from_secs(60),
        ));
    }

    #[test]
    fn probe_runs_with_no_prior_entry() {
        assert!(!probe_recently_done(
            None,
            Instant::now(),
            Duration::from_secs(60),
        ));
    }

    #[test]
    fn bounded_read_under_limit_succeeds() {
        let data = b"hello world";
        let result = read_bounded(std::io::Cursor::new(data), 100);
        assert_eq!(result.unwrap(), data);
    }

    #[test]
    fn bounded_read_at_limit_succeeds() {
        let data = vec![0u8; 100];
        let result = read_bounded(std::io::Cursor::new(data.clone()), 100);
        assert_eq!(result.unwrap(), data);
    }

    #[test]
    fn bounded_read_over_limit_returns_err() {
        let data = vec![0u8; 101];
        let result = read_bounded(std::io::Cursor::new(data), 100);
        assert!(result.is_err());
    }

    #[test]
    fn injects_before_head_case_insensitively() {
        let out = inject_script(b"<HTML><HEAD></HEAD><body></body></HTML>".to_vec());
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("<script type=\"text/javascript\">"));
        assert!(
            s.find("<script").unwrap() < s.find("</HEAD>").unwrap(),
            "script should be inserted before uppercase head close: {s}"
        );
    }

    #[test]
    fn injects_before_body_case_insensitively() {
        let out = inject_script(b"<html><BODY><main></main></BODY></html>".to_vec());
        let s = String::from_utf8(out).unwrap();
        assert!(
            s.find("<script").unwrap() < s.find("</BODY>").unwrap(),
            "script should be inserted before uppercase body close: {s}"
        );
    }

    #[test]
    fn strips_frame_blocking_headers_for_html() {
        let headers = vec![
            browser_proxy::ProxyHeader {
                name: "Content-Type".into(),
                value: "text/html".into(),
            },
            browser_proxy::ProxyHeader {
                name: "Content-Security-Policy".into(),
                value: "default-src 'self'".into(),
            },
            browser_proxy::ProxyHeader {
                name: "X-Frame-Options".into(),
                value: "DENY".into(),
            },
            browser_proxy::ProxyHeader {
                name: "Cache-Control".into(),
                value: "max-age=60".into(),
            },
        ];
        let names = filtered_proxy_headers(&headers, true)
            .into_iter()
            .map(|h| h.name)
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["Content-Type", "Cache-Control"]);
    }

    #[test]
    fn keeps_non_html_body_unmodified_and_headers_unstripped() {
        let body = b"body { color: red; }".to_vec();
        assert_eq!(body, b"body { color: red; }".to_vec());
        let headers = vec![
            browser_proxy::ProxyHeader {
                name: "Content-Type".into(),
                value: "text/css".into(),
            },
            browser_proxy::ProxyHeader {
                name: "Content-Security-Policy".into(),
                value: "default-src 'self'".into(),
            },
        ];
        let names = filtered_proxy_headers(&headers, false)
            .into_iter()
            .map(|h| h.name)
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["Content-Type", "Content-Security-Policy"]);
    }

    #[test]
    fn asset_size_limit_exceeds_observed_vite_chunks() {
        // Observed Vite dep chunks that triggered reload loops were ~5.2 MB
        // and later ~26.2 MB.
        const { assert!(PROXY_ASSET_SIZE_LIMIT > 25 * 1024 * 1024) };
        const { assert!(PROXY_ASSET_SIZE_LIMIT >= 26_219_024) };
    }

    #[test]
    fn oversized_html_response_uses_text_html() {
        let resp = oversized_response(99, 50, true);
        assert_eq!(resp.status(), 413);
        let ct = resp
            .headers()
            .get("Content-Type")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(ct, "text/html");
    }

    #[test]
    fn oversized_non_html_response_uses_text_plain() {
        let resp = oversized_response(99, 50, false);
        assert_eq!(resp.status(), 413);
        let ct = resp
            .headers()
            .get("Content-Type")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(ct, "text/plain");
        let body = String::from_utf8(resp.body().clone()).unwrap();
        assert!(!body.contains("<html>"));
    }

    #[test]
    fn upstream_url_preserves_proxy_scheme_intent() {
        let http_uri: tauri::http::Uri = "erp://localhost:6006/iframe.html".parse().unwrap();
        assert_eq!(
            upstream_url_for_proxy(&http_uri, "http"),
            "http://localhost:6006/iframe.html"
        );
        let https_uri: tauri::http::Uri = "erps://google.com/search?q=x".parse().unwrap();
        assert_eq!(
            upstream_url_for_proxy(&https_uri, "https"),
            "https://google.com/search?q=x"
        );
    }

    #[test]
    fn forwards_cookie_but_not_hop_by_hop_request_headers() {
        let mut headers = tauri::http::HeaderMap::new();
        headers.insert(
            tauri::http::header::COOKIE,
            tauri::http::HeaderValue::from_static("session=abc"),
        );
        headers.insert(
            tauri::http::header::AUTHORIZATION,
            tauri::http::HeaderValue::from_static("Bearer token"),
        );
        headers.insert(
            tauri::http::header::HOST,
            tauri::http::HeaderValue::from_static("localhost"),
        );
        headers.insert(
            "sec-fetch-mode",
            tauri::http::HeaderValue::from_static("navigate"),
        );

        let forwarded = forward_request_headers(&headers);
        let names: Vec<_> = forwarded.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"cookie"));
        assert!(names.contains(&"authorization"));
        assert!(!names.contains(&"host"));
        assert!(!names.iter().any(|n| n.starts_with("sec-")));
    }
}
