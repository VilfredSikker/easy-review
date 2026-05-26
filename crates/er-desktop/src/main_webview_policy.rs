//! Navigation policy for the main app webview — block external URLs in-app.

use tauri::webview::NewWindowResponse;
use tauri::Url;

use crate::commands;

fn is_local_app_host(host: &str) -> bool {
    host == "localhost" || host == "127.0.0.1" || host == "[::1]" || host.ends_with(".localhost")
}

fn log_policy_event(event: &str, url: &Url) {
    crate::profile_log::profile_log(
        event,
        &[
            ("scheme", url.scheme().to_string()),
            ("host", url.host_str().unwrap_or("").to_string()),
        ],
    );
}

/// Returns true when the main webview may navigate to `url` in-process.
pub fn is_allowed_main_webview_url(url: &Url) -> bool {
    match url.scheme() {
        "tauri" => true,
        "http" | "https" => url.host_str().is_some_and(is_local_app_host),
        _ => false,
    }
}

/// `on_navigation` handler: allow in-app URLs; open others in the system browser.
pub fn handle_main_webview_navigation(url: &Url) -> bool {
    if is_allowed_main_webview_url(url) {
        log_policy_event("main_webview_navigation_allow", url);
        return true;
    }
    if url.scheme() == "http" || url.scheme() == "https" {
        log_policy_event("main_webview_navigation_external", url);
        let _ = commands::open_external_url(url.as_str());
    } else {
        log_policy_event("main_webview_navigation_deny", url);
    }
    false
}

/// `on_new_window` handler: never spawn a second app window for external links.
pub fn handle_main_webview_new_window<R: tauri::Runtime>(url: &Url) -> NewWindowResponse<R> {
    if url.scheme() == "http" || url.scheme() == "https" {
        log_policy_event("main_webview_new_window_external", url);
        let _ = commands::open_external_url(url.as_str());
    } else {
        log_policy_event("main_webview_new_window_deny", url);
    }
    NewWindowResponse::Deny
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Url {
        s.parse().expect("test url")
    }

    #[test]
    fn allows_tauri_scheme() {
        assert!(is_allowed_main_webview_url(&parse("tauri://localhost")));
    }

    #[test]
    fn allows_dev_server() {
        assert!(is_allowed_main_webview_url(&parse(
            "http://localhost:5183/"
        )));
        assert!(is_allowed_main_webview_url(&parse(
            "http://127.0.0.1:5183/"
        )));
        assert!(is_allowed_main_webview_url(&parse("http://[::1]:5183/")));
    }

    #[test]
    fn allows_asset_localhost() {
        assert!(is_allowed_main_webview_url(&parse(
            "https://asset.localhost/index.html"
        )));
        assert!(is_allowed_main_webview_url(&parse(
            "https://tauri.localhost/"
        )));
    }

    #[test]
    fn denies_github() {
        assert!(!is_allowed_main_webview_url(&parse(
            "https://github.com/owner/repo/pull/1"
        )));
    }
}
