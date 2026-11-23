#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem, Submenu};
use tauri::Manager;

use er_desktop::browser_webview::BrowserWebviewState;
#[cfg(test)]
use er_desktop::comment_tick::comment_sync_recently_synced;
use er_desktop::frame_script::FRAME_SCRIPT;
use er_desktop::snapshot::{PrInfo, ProjectMeta, WatchStatusSnapshot, WatchStatusState};
use er_desktop::{
    browser_proxy, browser_webview, commands, dev_log, gh_status_cache, inbox, main_webview_policy,
    native_notify, pr_cache, profile_log, projects, snapshot, tabs, terminal, window_placement,
};

/// Inject the annotation content script before `</head>` (or `</body>` as fallback).
fn inject_script(mut html: Vec<u8>) -> Vec<u8> {
    let tag = format!("<script type=\"text/javascript\">{}</script>", FRAME_SCRIPT);
    if let Some(pos) = find_ascii_case_insensitive(&html, b"</head>") {
        html.splice(pos..pos, tag.bytes());
    } else if let Some(pos) = find_ascii_case_insensitive(&html, b"</body>") {
        html.splice(pos..pos, tag.bytes());
    } else {
        html.extend_from_slice(tag.as_bytes());
    }
    html
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

#[cfg_attr(
    not(target_os = "macos"),
    expect(
        unused_variables,
        reason = "`app` is only used inside the #[cfg(target_os = \"macos\")] block below"
    )
)]
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

#[expect(
    clippy::expect_used,
    reason = "a fixed status and static header values cannot make the builder fail"
)]
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
        .expect("static 413 response is always valid")
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

#[expect(
    clippy::expect_used,
    reason = "a fixed status and static header values cannot make the builder fail"
)]
fn proxy_transport_error_response(e: &ureq::Error) -> tauri::http::Response<Vec<u8>> {
    let status = if matches!(e, ureq::Error::Timeout(_)) {
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
        .expect("static transport-error response is always valid")
}

fn upstream_request(
    agent: &ureq::Agent,
    request: &tauri::http::Request<Vec<u8>>,
    target: &str,
) -> Result<ureq::http::Response<ureq::Body>, ureq::Error> {
    let mut builder = ureq::http::Request::builder()
        .method(request.method().as_str())
        .uri(target)
        .header("Accept-Encoding", "identity");
    for (name, value) in forward_request_headers(request.headers()) {
        builder = builder.header(name.as_str(), value.as_str());
    }
    let request = builder
        .body(request.body().clone())
        .map_err(ureq::Error::Http)?;
    agent.run(request)
}

#[expect(
    clippy::expect_used,
    reason = "the fallback is a bare 500 with no headers, which the builder cannot reject"
)]
fn proxied_response(
    request: &tauri::http::Request<Vec<u8>>,
    upstream_scheme: &str,
) -> tauri::http::Response<Vec<u8>> {
    let uri = request.uri();
    let target = upstream_url_for_proxy(uri, upstream_scheme);
    eprintln!("[erp] request: {} {} -> {}", request.method(), uri, target);
    // Document navigations: see `browser_proxy` module for redirect policy.
    let config = ureq::Agent::config_builder()
        .max_redirects(0)
        .timeout_connect(Some(std::time::Duration::from_secs(10)))
        .timeout_per_read(Some(std::time::Duration::from_secs(30)))
        .http_status_as_error(false)
        .build();
    let agent = ureq::Agent::new_with_config(config);

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
            Err(e) => return proxy_transport_error_response(&e),
        }
    };

    let status = resp.status().as_u16();

    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let is_html = is_html_content_type(ct.as_deref());
    let size_limit = proxy_size_limit(is_html);
    let bounded = read_bounded(resp.into_body().into_reader(), size_limit);
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
            .expect("bare 500 response is always valid")
    })
}

/// Panic-isolated wrapper around [`proxied_response`].
///
/// The `erp(s)://` handlers run synchronously inside the native webview's
/// URI-scheme callback. A panic there unwinds into non-Rust (objc/glib) frames,
/// which is undefined behaviour and aborts the entire app — so a single bad
/// upstream response (e.g. a header value the `http` crate rejects) would crash
/// Easy Review rather than failing just the page load. Catch any panic and turn
/// it into a 500 so the embedded browser can never take the app down with it.
#[expect(
    clippy::expect_used,
    reason = "the fallback is a bare 500 with no headers, which the builder cannot reject"
)]
fn safe_proxied_response(
    request: &tauri::http::Request<Vec<u8>>,
    upstream_scheme: &str,
) -> tauri::http::Response<Vec<u8>> {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        proxied_response(request, upstream_scheme)
    }));
    match result {
        Ok(resp) => resp,
        Err(_) => {
            eprintln!("[erp] proxy handler panicked; returning 500 instead of crashing");
            tauri::http::Response::builder()
                .status(500)
                .header("Content-Type", "text/html")
                .body(
                    b"<html><body><p>The embedded browser hit an internal error loading this page.</p></body></html>".to_vec(),
                )
                .unwrap_or_else(|_| {
                    tauri::http::Response::builder()
                        .status(500)
                        .body(Vec::new())
                        .expect("bare 500 response is always valid")
                })
        }
    }
}

/// Install a custom application menu. Mirrors Tauri's default menu but defines
/// Select All as a custom item with a native ⌘A accelerator — desktop-ui does
/// not claim ⌘A, so the app supplies macOS's default Select All itself.
fn install_app_menu(app: &tauri::AppHandle) -> tauri::Result<()> {
    let pkg = app.package_info();
    let app_name = pkg.name.clone();

    // Select All with the native accelerator. Wired in `on_menu_event` below.
    let select_all = MenuItemBuilder::with_id("er.select_all", "Select All")
        .accelerator("CmdOrCtrl+A")
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
            app_name,
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

/// The desktop active-branch watcher thread. Follows the currently active tab's
/// local-branch checkout (project root or linked worktree) and refreshes the
/// diff when tracked files in that checkout change.
///
/// The target comes from the registry's active tab and its content
/// (`watcher::Follower`), and a file change refreshes that tab's content with
/// auto-unmark.
fn run_active_branch_watcher(
    watcher_status: WatchStatusState,
    watcher_session: er_desktop::state::SessionClock,
    watcher_desktop_rev: Arc<std::sync::atomic::AtomicU64>,
    watcher_service: Arc<er_desktop::service::ServiceState>,
    watcher_git: Arc<dyn er_engine::git::Git>,
) {
    use er_engine::watch::{FileWatcher, WatchEvent};
    use std::path::Path;
    use std::sync::mpsc;

    let (tx, rx) = mpsc::channel::<WatchEvent>();
    // Held only for its Drop side effect: dropping stops the watcher.
    let mut _watcher: Option<FileWatcher> = None;
    let mut current_key: Option<(String, String)> = None;
    let poll_interval = std::time::Duration::from_millis(400);
    let mut follower = er_desktop::watcher::Follower::default();

    loop {
        let desired = follower
            .next(&watcher_service, &*watcher_git)
            .and_then(|w| w.target());

        if desired != current_key {
            _watcher = None; // drop old watcher
            if let Some((ref branch, ref root_path)) = desired {
                match FileWatcher::new(Path::new(root_path), 250, tx.clone()) {
                    Ok(w) => {
                        _watcher = Some(w);
                        watcher_session.set_watch_status(
                            &watcher_status,
                            WatchStatusSnapshot {
                                active: true,
                                branch: Some(branch.clone()),
                                root_path: Some(root_path.clone()),
                            },
                        );
                    }
                    Err(e) => {
                        log::error!(
                            "er-desktop: active-branch watcher failed for {root_path}: {e}"
                        );
                        watcher_session
                            .set_watch_status(&watcher_status, WatchStatusSnapshot::default());
                    }
                }
            } else {
                watcher_session.set_watch_status(&watcher_status, WatchStatusSnapshot::default());
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
            Err(mpsc::RecvTimeoutError::Timeout | mpsc::RecvTimeoutError::Disconnected) => {}
        }

        if got_event && current_key.is_some() {
            follower.on_change(&watcher_service);
        }
    }
}

/// The flag path's branch-base probe, on the same cadence. It reads its
/// target from the registry and the content (`watcher::probe_target`), never
/// the App, and posts origin's tip of a branch view's base and the PR list's
/// head for the checkout to the tab's actor, whose content then carries the
/// stale mark and the divergence.
fn spawn_base_probe(
    service: Arc<er_desktop::service::ServiceState>,
    git: Arc<dyn er_engine::git::Git>,
    pr_cache: pr_cache::PrCacheMap,
) {
    use er_desktop::watcher;
    std::thread::spawn(move || {
        let mut interval = PROBE_INTERVAL_SECS;
        loop {
            let probed = watcher::probe_target(&service).map(|target| {
                // A failed read keeps the tip the content holds and backs
                // off, as the flag-off probe does; the PR head still posts.
                let read = target
                    .base
                    .as_deref()
                    .map(|base| watcher::remote_tip(&*git, &target.repo_root, base));
                let base_tip = match &read {
                    Some(Some(tip)) => Some(tip.clone()),
                    Some(None) => target.tips.base_tip.clone(),
                    None => None,
                };
                let pr_head = pr_cache
                    .lock()
                    .ok()
                    .and_then(|lists| watcher::pr_head(&lists, &target));
                let tips = er_service::RemoteTips { base_tip, pr_head };
                let moved = tips != target.tips && read != Some(None);
                watcher::post_tips(&service, &target, tips);
                interval = next_probe_interval_secs(interval, moved);
                target.key
            });
            if probed.is_none() {
                interval = PROBE_INTERVAL_SECS;
            }
            // Sleeps the interval, and probes again within a second when
            // another tab becomes active, or while the active one has not
            // loaded yet.
            for _ in 0..interval {
                std::thread::sleep(std::time::Duration::from_secs(1));
                if service.app_registry().tabs_view().active != probed {
                    break;
                }
            }
        }
    });
}

fn main() {
    er_engine::env_path::init_cli_path();
    dev_log::init();

    let ports = er_engine::ports::Ports::cli();
    let cwd_repo_root = ports.git.repo_root_cwd().ok();

    // With no saved tab, or every saved tab broken, the restore opens a
    // Working tab here, and the launch root falls back to it for a remote PR.
    let restore_root: Option<String> = cwd_repo_root
        .clone()
        .or_else(|| startup_root_from_projects(&projects::load()));

    let pr_cache: Arc<Mutex<HashMap<String, Vec<PrInfo>>>> = Arc::new(Mutex::new(HashMap::new()));
    let pr_cache_fetched_at: Arc<Mutex<HashMap<String, u64>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let meta_cache: Arc<Mutex<HashMap<String, ProjectMeta>>> = Arc::new(Mutex::new(HashMap::new()));
    let gh_user: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let gh_status_cache: snapshot::GhStatusCache = Arc::new(Mutex::new(
        gh_status_cache::load_persisted_gh_status_cache()
            .ok()
            .flatten()
            .unwrap_or_default(),
    ));
    let inbox = Arc::new(Mutex::new(inbox::load_inbox_state()));
    let app_owner = Arc::new(er_desktop::app_owner::AppOwner::new(
        Arc::clone(&meta_cache),
        Arc::clone(&pr_cache),
        Arc::clone(&pr_cache_fetched_at),
        Arc::clone(&gh_user),
        Arc::clone(&inbox),
    ));
    app_owner.read_status_from(Arc::clone(&gh_status_cache));
    let watch_status: WatchStatusState = Arc::new(Mutex::new(WatchStatusSnapshot::default()));
    let tauri_app_handle: Arc<Mutex<Option<tauri::AppHandle>>> = Arc::new(Mutex::new(None));

    // Resolve the gh user login once at launch in a background thread.
    // Don't block startup; if it fails, leave as None.
    {
        let gh_user_bg = Arc::clone(&gh_user);
        let github = Arc::clone(&ports.github);
        std::thread::spawn(move || {
            let Ok(Some(login)) = github.current_login() else {
                return;
            };
            if login.is_empty() {
                return;
            }
            if let Ok(mut g) = gh_user_bg.lock() {
                *g = Some(login);
            }
        });
    }

    let gh_status_in_flight: Arc<Mutex<std::collections::HashSet<(String, String, u64)>>> =
        Arc::new(Mutex::new(std::collections::HashSet::new()));
    let view_arc: er_desktop::view_state::ViewStateHandle = Arc::default();
    let terminals: Arc<Mutex<HashMap<String, terminal::PtySession>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let terminals_for_exit = Arc::clone(&terminals);
    let desktop_revision: Arc<std::sync::atomic::AtomicU64> =
        Arc::new(std::sync::atomic::AtomicU64::new(0));
    let branch_base_remote_oid: Arc<Mutex<HashMap<String, String>>> =
        Arc::new(Mutex::new(HashMap::new()));
    // The resource relay's wakes; the relay starts with the window.
    let (resource_wake, resource_wakes) = std::sync::mpsc::channel();
    let session = er_desktop::state::SessionClock::default();
    session.wake_with(resource_wake.clone());
    let service = Arc::new(
        er_desktop::service::ServiceState::with_revision_relay(ports.clone(), {
            // The revision watcher wakes the tick for this bump.
            let desktop_revision = Arc::clone(&desktop_revision);
            move |_| {
                profile_log::bump_desktop_revision(&desktop_revision, "service_view");
            }
        })
        .with_resource_wake(resource_wake.clone()),
    );
    app_owner.set_wake(resource_wake);
    // The registry loads the saved tabs and issues every tab key.
    tabs::restore(
        &view_arc,
        service.app_registry(),
        &ports,
        restore_root.as_deref(),
    );
    tabs::follow_service(&service, Arc::clone(&view_arc));
    let launch_root = er_desktop::app_owner::startup::launch_root(
        service.app_registry(),
        restore_root.as_deref(),
    );
    // Each listed tab's flow bucket, resolved from its descriptor: no tab has
    // loaded yet.
    er_desktop::flow_service::reconcile_on_load(
        &service.app_registry().tabs_view(),
        &ports,
        &service,
    );
    let desktop = er_desktop::state::DesktopState {
        ports: ports.clone(),
        service: Arc::clone(&service),
        view: Arc::clone(&view_arc),
        pr_cache: Arc::clone(&pr_cache),
        pr_cache_fetched_at: Arc::clone(&pr_cache_fetched_at),
        branch_base_remote_oid: Arc::clone(&branch_base_remote_oid),
        meta_cache: Arc::clone(&meta_cache),
        app_owner: Arc::clone(&app_owner),
        gh_user: Arc::clone(&gh_user),
        terminals: Arc::clone(&terminals),
        pending_ai_replies: er_desktop::state::AiReplyJobs::new(session.clock()),
        ui_annotations: session.clock(),
        ui_annotations_io: Arc::default(),
        gh_status_cache: Arc::clone(&gh_status_cache),
        pr_list_running: Arc::default(),
        pr_list_clock: session.clock(),
        status_fetch_clock: session.clock(),
        gh_status_in_flight: Arc::clone(&gh_status_in_flight),
        gh_status_fetched: Arc::default(),
        desktop_revision: Arc::clone(&desktop_revision),
        watch_status: Arc::clone(&watch_status),
        inbox: Arc::clone(&inbox),
        tauri_app_handle: Arc::clone(&tauri_app_handle),
        auto_triage_in_flight: Arc::new(Mutex::new(std::collections::HashSet::new())),
        line_commits_cache: Arc::default(),
        session: session.clone(),
        live_jobs: Default::default(),
        live_tabs: Default::default(),
        tasks: er_desktop::tasks::TaskHost::new(session.clock()),
    };
    let state = desktop.clone();
    er_desktop::app_owner::startup::read_inbox_prefs_from_settings(&desktop);
    er_desktop::app_owner::startup::register_open_projects(&state);

    match pr_cache::load_persisted_pr_cache() {
        Ok(Some(restored)) => {
            app_owner.restore_prs(restored);
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
    // (the registry's PR-open cap; gh_status is memory-only at runtime) already
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
            // The app registry owns the PR-open cache file and prunes it.
            state
                .service
                .app_registry()
                .prune_pr_open_cache(&pr_open_keep);
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

    // Diff refresh is manual for both remote-PR and local-PR tabs. Nothing
    // auto-swaps the live diff; the 30s PR-head probe (above, "pr_head_probe")
    // is what lights the stale pill telling the user to Sync.

    // Background base-branch staleness probe on a 60s cadence. The ONLY new
    // network cost for branch ("Local Diff") freshness. Mirrors the remote-PR
    // loop's three-phase shape so we never hold the App mutex across `git`:
    //   Phase 1 (brief lock) — read the active tab's identity; bail unless it's
    //                          a branch view (not remote, no PR, has a view).
    //   Phase 2 (no lock)    — `git ls-remote origin <base>` to learn origin's
    //                          tip. ANY failure (error/non-zero/empty) no-ops.
    //   Phase 3 (brief lock) — cache the oid; bump the revision ONLY when it
    //                          actually changed, so we don't churn polls.
    // With `v1-service` the probe posts to the tab's actor (`spawn_base_probe`).
    spawn_base_probe(
        Arc::clone(&service),
        Arc::clone(&ports.git),
        Arc::clone(&pr_cache),
    );

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
    //   Phase 3 (brief lock) — the owner's `patch_pr_head_oid` updates the
    //                          single cache entry in place and publishes;
    //                          bump the revision ONLY when it actually
    //                          changed. Does NOT persist to disk (the next
    //                          10-min sweep persists) so 30s is cheap.
    // The target comes from the registry's tabs on the flag path.
    {
        let probe_state = state.clone();
        let probe_rev = Arc::clone(&desktop_revision);
        let probe_github = Arc::clone(&ports.github);
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
                let Some((slug, pr_number)) =
                    er_desktop::app_owner::startup::pr_head_probe_target(&probe_state)
                else {
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
                let Some((owner, repo)) = slug.split_once('/') else {
                    continue;
                };
                let oid = probe_github
                    .pr_head_oid_remote(owner, repo, pr_number)
                    .ok()
                    .filter(|oid| !oid.is_empty());
                // A failed/empty probe is NOT recorded into probe_throttle —
                // a persistently-failing PR keeps retrying every 30s rather
                // than being throttled to 60s on top of already being broken.
                let Some(oid) = oid else { continue };

                // Phase 3: brief lock — patch the cache entry; bump only on change.
                let changed = probe_state
                    .app_owner
                    .patch_pr_head_oid(&slug, pr_number, &oid);

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
        let loop_state = state.clone();
        let fetches = state.clone();
        let gh_status_desktop_rev = Arc::clone(&desktop_revision);
        std::thread::spawn(move || loop {
            // Sleep FIRST: kills the startup burst — a sleep at the bottom
            // lets the loop fire immediately on launch.
            std::thread::sleep(std::time::Duration::from_secs(30));

            // The flag path reads the registry's active tab, never the App.
            let key = er_desktop::status_loop::pick(&loop_state);
            // Snapshot identity in a short critical section.
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
            let Some(key) = key else {
                continue;
            };
            let Some(_fetch) = fetches.begin_status_fetch(&key) else {
                continue;
            };
            let (owner, repo, number) = &key;
            commands::fetch_and_store_github_status(
                &gh_status_bg,
                &gh_status_desktop_rev,
                owner,
                repo,
                *number,
            );
        });
    }

    er_desktop::comment_tick::spawn(state.clone());

    // Spawn background GitHub comment sync. Three-phase design so network I/O
    // never holds the app mutex:
    //   Phase 1 (brief lock) — snapshot identity + files from active tab.
    //   Phase 2 (no lock)   — fetch comments, review threads, PR overview.
    //   Phase 3 (brief lock) — apply results to the correct tab.

    // Spawn the desktop active-branch watcher. Follows the currently active
    // tab's local-branch checkout (project root or linked worktree) and
    // refreshes the diff when tracked files in that checkout change.
    std::thread::spawn({
        let watcher_status = Arc::clone(&watch_status);
        let watcher_session = session.clone();
        let watcher_desktop_rev = Arc::clone(&desktop_revision);
        let watcher_service = Arc::clone(&service);
        let watcher_git = Arc::clone(&ports.git);
        move || {
            run_active_branch_watcher(
                watcher_status,
                watcher_session,
                watcher_desktop_rev,
                watcher_service,
                watcher_git,
            );
        }
    });

    // Global notifications refresh across ALL configured project remotes.
    // Startup fetch + conservative 10-minute cadence (not high-frequency polling).
    let bg_cache = Arc::clone(&pr_cache);
    let bg_fetched_at = Arc::clone(&pr_cache_fetched_at);
    let bg_owner = Arc::clone(&app_owner);
    let bg_desktop_rev = Arc::clone(&desktop_revision);
    let bg_gh_user = Arc::clone(&gh_user);
    let bg_inbox = Arc::clone(&inbox);
    let bg_handle = Arc::clone(&tauri_app_handle);
    let bg_state = state.clone();
    let bg_github = Arc::clone(&ports.github);
    std::thread::spawn(move || {
        // Inbox native notifications need `tauri_app_handle`; setup stores it after
        // this thread starts (release builds are especially tight on timing).
        for _ in 0..200 {
            if bg_handle.lock().ok().and_then(|g| g.clone()).is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        #[expect(
            clippy::expect_used,
            reason = "a worker thread that cannot build its runtime has nothing to fall back to"
        )]
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime");
        let run_refresh = || {
            let refresh = bg_state.begin_pr_list_refresh();
            let failed = rt.block_on(bg_owner.refresh_prs(Arc::clone(&bg_github)));
            let prefs = er_desktop::app_owner::startup::inbox_prefs(&bg_state);
            let handles = commands::InboxRefreshHandles {
                pr_cache: &bg_cache,
                gh_user: &bg_gh_user,
                inbox: &bg_inbox,
                desktop_revision: &bg_desktop_rev,
                app_handle: &bg_handle,
            };
            for remote in failed {
                commands::process_inbox_after_pr_refresh(handles, &prefs, Some(remote));
            }
            commands::process_inbox_after_pr_refresh(handles, &prefs, None);
            drop(refresh);
            profile_log::bump_desktop_revision(&bg_desktop_rev, "pr_cache_refresh");
        };

        // Startup: fetch the active project's remote first so its sidebar PR
        // list lights up quickly. The full multi-remote sweep follows.
        if let Some(active_remote) = pr_cache::active_project_remote() {
            let refresh = bg_state.begin_pr_list_refresh();
            let success =
                rt.block_on(bg_owner.refresh_remote_prs(Arc::clone(&bg_github), &active_remote));
            let prefs = er_desktop::app_owner::startup::inbox_prefs(&bg_state);
            let handles = commands::InboxRefreshHandles {
                pr_cache: &bg_cache,
                gh_user: &bg_gh_user,
                inbox: &bg_inbox,
                desktop_revision: &bg_desktop_rev,
                app_handle: &bg_handle,
            };
            commands::process_inbox_after_pr_refresh(
                handles,
                &prefs,
                if success { None } else { Some(active_remote) },
            );
            drop(refresh);
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
    // (branches, worktrees, current/base branch) fresh.
    let meta_owner = Arc::clone(&app_owner);
    let meta_desktop_rev = Arc::clone(&desktop_revision);
    let meta_state = state.clone();
    let meta_ports = ports.clone();
    std::thread::spawn(move || {
        #[expect(
            clippy::expect_used,
            reason = "a worker thread that cannot build its runtime has nothing to fall back to"
        )]
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime");
        rt.block_on(async move {
            // Startup: refresh only the active project so its sidebar is ready
            // ASAP. Then a short delay before the full sweep covers everyone else.
            let active_id = projects::load().active_id.clone();
            if let Some(id) = active_id.as_deref() {
                if meta_owner.refresh_project_meta(&*meta_ports.git, id) {
                    profile_log::bump_desktop_revision(&meta_desktop_rev, "meta_startup_project");
                }
            }
            // First full pass — slight delay so we don't compete with the
            // active tab's diff refresh for git CPU during the first frame.
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            if meta_owner.refresh_meta(&*meta_ports.git, &launch_root) {
                profile_log::bump_desktop_revision(&meta_desktop_rev, "meta_startup_full");
            }
            loop {
                profile_log::profile_log("bg_loop", &[("loop", "meta".to_string())]);
                let interval = er_desktop::app_owner::startup::meta_refresh_interval(&meta_state);
                tokio::time::sleep(interval).await;
                let active_id = projects::load().active_id.clone();
                if let Some(id) = active_id.as_deref() {
                    if meta_owner.refresh_project_meta(&*meta_ports.git, id) {
                        profile_log::bump_desktop_revision(&meta_desktop_rev, "meta_tick");
                    }
                }
            }
        });
    });

    // Background warmer: loads the registry's tabs that belong to the active
    // project through the service, after a grace period.
    tabs::spawn_warmer(Arc::clone(&service), std::time::Duration::from_secs(2));

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
    #[expect(
        clippy::expect_used,
        reason = "the app cannot run without its Tauri context; failing to build is fatal at startup"
    )]
    let tauri_app = tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .filter(|metadata| dev_log::enabled_for_log_target(metadata.target()))
                .build(),
        )
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(window_state_save_flags())
                .skip_initial_state("main")
                .build(),
        )
        .manage(desktop)
        .manage(BrowserWebviewState::default())
        // `erp://host/path` proxies `http://host/path`; `erps://host/path`
        // proxies `https://host/path`. HTML responses get the annotation script.
        .register_uri_scheme_protocol("erp", |_app, request| {
            safe_proxied_response(&request, "http")
        })
        .register_uri_scheme_protocol("erps", |_app, request| {
            safe_proxied_response(&request, "https")
        })
        .on_window_event(move |window, event| {
            if window.label() != "main" {
                return;
            }
            match event {
                // The window-state plugin only writes geometry to disk on a
                // graceful RunEvent::Exit; move/resize merely update its in-memory
                // cache. A `tauri dev` rebuild (or a crash) hard-kills the app, so
                // that cache is lost and the window reopens at the last *cleanly*
                // saved geometry. Persist eagerly here (throttled to one write per
                // 400ms) so the last position+size survive rebuilds and crashes.
                tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_) => {
                    use tauri_plugin_window_state::AppHandleExt;
                    if !window_ready_event.load(std::sync::atomic::Ordering::Acquire) {
                        return;
                    }
                    let Ok(mut last) = window_geom_last_save.lock() else {
                        return;
                    };
                    if last.elapsed() >= std::time::Duration::from_millis(400) {
                        *last = std::time::Instant::now();
                        let _ = window
                            .app_handle()
                            .save_window_state(window_state_save_flags());
                    }
                }
                _ => {}
            }
        })
        .setup(move |app| {
            if cfg!(debug_assertions) {
                er_engine::store_registry::startup_audit(
                    er_engine::store_registry::FrontEnd::Desktop,
                    &er_desktop::store_registry::registry(),
                );
            }
            if let Some(state) = app.try_state::<er_desktop::state::DesktopState>() {
                if let Ok(mut h) = state.tauri_app_handle.lock() {
                    *h = Some(app.handle().clone());
                }
                native_notify::initialize(app.handle());
                er_desktop::app_owner::startup::flush_pending_notifications(&state);
                // Revision watcher: each advance of `desktop_revision` wakes
                // the backend tick, which runs the duties that move resources.
                {
                    use er_desktop::resources::tick;
                    let desktop = state.inner().clone();
                    std::thread::spawn(move || {
                        let never = std::sync::atomic::AtomicBool::new(false);
                        let traced = |revision: u64, delta: u64| {
                            profile_log::profile_log(
                                "revision_tick",
                                &[
                                    ("coalesced_rev", revision.to_string()),
                                    ("delta_rev", delta.to_string()),
                                ],
                            );
                        };
                        tick::follow_revisions(desktop, tick::INTERVAL, &never, &traced);
                    });
                }
                {
                    use er_desktop::resources::relay;
                    use tauri::Emitter;
                    let handle = app.handle().clone();
                    relay::spawn(state.inner().clone(), resource_wakes, move |moved| {
                        let changed = relay::wire(&moved);
                        if changed.is_empty() {
                            return;
                        }
                        if let Err(e) = handle.emit("er://resources", &changed) {
                            log::warn!("resource relay emit failed: {e}");
                        }
                    });
                }
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
                // Tauri's OS drag-drop handler unconditionally reports every
                // drag as handled, so wry never forwards to WKWebView's own
                // NSDraggingDestination. That swallows in-page HTML5 drag and
                // drop everywhere (tab strip, sidebar reordering): dragstart
                // fires, the drag session is refused, dragend lands in the same
                // tick. Nothing here consumes tauri://drag-drop file drops.
                .disable_drag_drop_handler()
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
            er_desktop::resources::commands::er_send,
            er_desktop::resources::commands::er_fetch,
            er_desktop::resources::commands::er_versions,
            er_desktop::direct::window::start_window_drag,
            er_desktop::github_image::fetch_github_image,
            er_desktop::direct::os::open_in_editor,
            er_desktop::direct::os::open_in_vscode,
            er_desktop::direct::os::open_source,
            er_desktop::direct::os::open_url_in_browser,
            er_desktop::direct::os::reveal_er_folder,
            er_desktop::direct::os::reveal_path,
            er_desktop::direct::dev_log::dev_log_filter,
            er_desktop::direct::line_commits::line_commits,
            er_desktop::direct::export::export_review,
            er_desktop::direct::export::export_review_to_file,
            er_desktop::direct::export::export_to_agent,
            er_desktop::direct::terminal::terminal_spawn,
            er_desktop::direct::terminal::terminal_write,
            er_desktop::direct::terminal::terminal_resize,
            er_desktop::direct::terminal::terminal_close,
            er_desktop::direct::terminal::detect_dev_url,
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

    tauri_app.run(move |handle, event| {
        #[cfg(not(target_os = "macos"))]
        let _ = handle;
        match event {
            tauri::RunEvent::ExitRequested { .. } => {
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
                reveal_main_window_from_handle(handle, "macos_reopen");
            }
            _ => {}
        }
    });
}

/// Root to open when the CWD is not a repo, which is every Finder launch.
///
/// The active project comes first, but only while its folder still exists:
/// a repo that was moved or deleted would otherwise abort startup before any
/// window appears. Falls back to the first saved project that is still on disk.
fn startup_root_from_projects(file: &projects::ProjectsFile) -> Option<String> {
    let on_disk = |p: &&projects::ProjectRecord| {
        !p.root_path.is_empty() && std::path::Path::new(&p.root_path).is_dir()
    };
    let active = file
        .active_id
        .as_ref()
        .and_then(|id| file.projects.iter().find(|p| &p.id == id))
        .filter(on_disk);
    active
        .or_else(|| file.projects.iter().find(on_disk))
        .map(|p| p.root_path.clone())
}

/// Base cadence for the branch-base staleness probe: one `git ls-remote` a
/// minute while origin's tip is moving.
const PROBE_INTERVAL_SECS: u64 = 60;

/// Ceiling the probe backs off to while that tip stays put.
const PROBE_MAX_INTERVAL_SECS: u64 = 300;

/// Next probe interval in seconds.
///
/// The probe is a network call on a loop that runs for the whole session, and a
/// review can sit on one branch for hours. Backing off while the tip is
/// unchanged bounds that steady-state traffic to a probe every five minutes.
///
/// The cost of that: once backed off, a change is only *noticed* at the next
/// probe, so a branch that starts moving after a long idle wait is up to
/// `PROBE_MAX_INTERVAL_SECS` away from being seen. The snap back to the base
/// cadence happens on detection, not on the change itself.
fn next_probe_interval_secs(current: u64, changed: bool) -> u64 {
    if changed {
        PROBE_INTERVAL_SECS
    } else {
        current.saturating_mul(2).min(PROBE_MAX_INTERVAL_SECS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    // ── comment_sync_recently_synced (skip gate) ──
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

    // ── probe_recently_done (throttle) ──

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

    #[test]
    fn probe_interval_backs_off_while_the_base_tip_is_unchanged() {
        let mut interval = PROBE_INTERVAL_SECS;
        interval = next_probe_interval_secs(interval, false);
        assert_eq!(interval, 120, "backs off while nothing moves");
        interval = next_probe_interval_secs(interval, false);
        assert_eq!(interval, 240);
        interval = next_probe_interval_secs(interval, false);
        assert_eq!(
            interval, PROBE_MAX_INTERVAL_SECS,
            "capped rather than growing without bound"
        );
        interval = next_probe_interval_secs(interval, false);
        assert_eq!(interval, PROBE_MAX_INTERVAL_SECS, "stays at the cap");

        assert_eq!(
            next_probe_interval_secs(interval, true),
            PROBE_INTERVAL_SECS,
            "a tip that moved snaps back to the fast cadence"
        );
    }

    // ── startup_root_from_projects ──

    fn projects_file(active: &str, roots: &[(&str, &str)]) -> projects::ProjectsFile {
        let projects: Vec<_> = roots
            .iter()
            .map(|(id, root)| serde_json::json!({ "id": id, "name": id, "root_path": root }))
            .collect();
        serde_json::from_value(serde_json::json!({ "active_id": active, "projects": projects }))
            .expect("projects file")
    }

    #[test]
    fn startup_root_prefers_active_project_on_disk() {
        let a = tempfile::tempdir().expect("tempdir");
        let b = tempfile::tempdir().expect("tempdir");
        let (a, b) = (a.path().to_str().unwrap(), b.path().to_str().unwrap());
        let file = projects_file("b", &[("a", a), ("b", b)]);
        assert_eq!(startup_root_from_projects(&file).as_deref(), Some(b));
    }

    #[test]
    fn startup_root_skips_active_project_whose_folder_is_gone() {
        // A moved repo left the active project pointing at a missing folder,
        // and the Finder launch exited before any window appeared.
        let live = tempfile::tempdir().expect("tempdir");
        let live = live.path().to_str().unwrap();
        let gone = std::env::temp_dir().join("er-startup-root-missing-repo");
        let file = projects_file("gone", &[("gone", gone.to_str().unwrap()), ("live", live)]);
        assert_eq!(startup_root_from_projects(&file).as_deref(), Some(live));
    }

    #[test]
    fn startup_root_is_none_when_no_project_folder_exists() {
        let gone = std::env::temp_dir().join("er-startup-root-missing-repo");
        let file = projects_file("gone", &[("gone", gone.to_str().unwrap()), ("empty", "")]);
        assert_eq!(startup_root_from_projects(&file), None);
    }
}
