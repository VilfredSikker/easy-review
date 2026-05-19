//! Embedded dev-browser reverse proxy (`erp://` / `erps://`).
//!
//! # Why a custom scheme?
//!
//! The review browser iframe could load `http://localhost:5173` directly, but then we
//! could not reliably inject the annotation script or strip embed-blocking headers
//! (`X-Frame-Options`, `Content-Security-Policy`). The proxy fetches the real URL,
//! rewrites the response for embedding, and exposes it as `erp://host/path`.
//!
//! # Provider-agnostic navigation policy
//!
//! WKWebView does not consistently auto-follow HTTP redirects on custom URI schemes.
//! OAuth/OIDC/session flows also require redirects to run in the browser cookie jar,
//! not in a server-side HTTP client. The rules below apply to any dev app (SvelteKit,
//! Next, Rails, etc.) and any identity provider.
//!
//! | Case | Behaviour |
//! |------|-----------|
//! | Subresources (JS, CSS, …) | Single upstream hop; no redirect following |
//! | `GET`/`HEAD` document, same-origin redirect | Single hop; return HTTP `Location` as `erp(s)://…` |
//! | `GET`/`HEAD` document, cross-origin redirect | HTML `location.replace` to `erp(s)://…` (iframes ignore custom-scheme `Location` headers) |
//! | OAuth loops | Never follow redirects server-side; never strip handshake query before the app has handled it |
//! | Still cycling after many WebView navigations | HTML error page (rare once redirects are pass-through) |
//!
//! Redirect `Location` values are rewritten to `erp://` or `erps://` so the WebView issues
//! the next hop on the custom scheme with its cookie jar (auth hosts, CDNs, etc.).

use std::collections::HashSet;

use tauri::http;

#[derive(Clone, Debug)]
pub struct ProxyHeader {
    pub name: String,
    pub value: String,
}

/// Query keys commonly used once during OAuth/OIDC handshakes. Stripped when
/// detecting redirect cycles so `/` and `/?__clerk_handshake=…` are not treated
/// as revisiting the same URL.
const TRANSIENT_REDIRECT_QUERY_KEYS: &[&str] = &[
    "__clerk_handshake",
    "__clerk_db_jwt",
    "__session",
    "code",
    "state",
    "session_state",
    "oauth_token",
    "oauth_verifier",
    "access_token",
    "id_token",
    "token",
    "nonce",
];

/// Normalize a URL for redirect-cycle detection: origin + path, ignoring
/// transient OAuth query parameters and hash fragments.
/// True when the URL carries one-time OAuth/OIDC query parameters (Clerk handshake, etc.).
pub fn url_has_transient_oauth_query(url: &str) -> bool {
    let Some(q_start) = url.find('?') else {
        return false;
    };
    let query = url[q_start + 1..]
        .split('#')
        .next()
        .unwrap_or("");
    if query.is_empty() {
        return false;
    }
    query.split('&').any(|pair| {
        let key = pair.split('=').next().unwrap_or(pair);
        TRANSIENT_REDIRECT_QUERY_KEYS
            .iter()
            .any(|k| key.eq_ignore_ascii_case(k))
    })
}

pub fn redirect_visit_key(url: &str) -> String {
    let scheme_end = match url.find("://") {
        Some(i) => i,
        None => return url.to_string(),
    };
    let after_scheme = &url[scheme_end + 3..];
    let path_start = after_scheme.find('/').unwrap_or(after_scheme.len());
    let authority = &after_scheme[..path_start];
    let rest = &after_scheme[path_start..];
    let path_end = rest.find(['?', '#']).unwrap_or(rest.len());
    let path = if path_end == 0 {
        "/".to_string()
    } else {
        rest[..path_end].to_string()
    };
    let path = if path.is_empty() { "/".to_string() } else { path };

    let query = if let Some(q_start) = rest.find('?') {
        let q_end = rest[q_start + 1..]
            .find('#')
            .map(|i| q_start + 1 + i)
            .unwrap_or(rest.len());
        &rest[q_start + 1..q_end]
    } else {
        ""
    };

    let mut kept = Vec::new();
    if !query.is_empty() {
        for pair in query.split('&') {
            let key = pair.split('=').next().unwrap_or(pair);
            let transient = TRANSIENT_REDIRECT_QUERY_KEYS
                .iter()
                .any(|k| key.eq_ignore_ascii_case(k));
            if !transient {
                kept.push(pair);
            }
        }
    }
    kept.sort_unstable();
    if kept.is_empty() {
        format!("{}://{}{}", &url[..scheme_end], authority, path)
    } else {
        format!(
            "{}://{}{}?{}",
            &url[..scheme_end],
            authority,
            path,
            kept.join("&")
        )
    }
}

pub fn is_redirect_status(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

pub fn rewrite_proxy_location(location: &str, upstream_scheme: &str) -> String {
    let (upstream_prefix, proxy_prefix) = if upstream_scheme == "https" {
        ("https://", "erps://")
    } else {
        ("http://", "erp://")
    };
    if location.starts_with(upstream_prefix) {
        format!("{proxy_prefix}{}", &location[upstream_prefix.len()..])
    } else {
        location.to_string()
    }
}

pub fn upstream_origin(url: &str) -> Option<String> {
    let scheme_end = url.find("://")?;
    let after_scheme = &url[scheme_end + 3..];
    let host_end = after_scheme.find('/').unwrap_or(after_scheme.len());
    let authority = &after_scheme[..host_end];
    if authority.is_empty() {
        return None;
    }
    Some(format!("{}://{}", &url[..scheme_end], authority))
}

pub fn same_upstream_origin(a: &str, b: &str) -> bool {
    match (upstream_origin(a), upstream_origin(b)) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

pub fn to_proxy_scheme_url(http_url: &str) -> String {
    if http_url.starts_with("https://") {
        rewrite_proxy_location(http_url, "https")
    } else if http_url.starts_with("http://") {
        rewrite_proxy_location(http_url, "http")
    } else {
        http_url.to_string()
    }
}

pub fn resolve_upstream_redirect_location(location: &str, current_target: &str) -> String {
    let trimmed = location.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return trimmed.to_string();
    }
    if trimmed.starts_with('/') {
        if let Some(scheme_end) = current_target.find("://") {
            let after_scheme = &current_target[scheme_end + 3..];
            if let Some(slash) = after_scheme.find('/') {
                let origin = &current_target[..scheme_end + 3 + slash];
                return format!("{origin}{trimmed}");
            }
            return format!("{current_target}{trimmed}");
        }
    }
    current_target.to_string()
}

fn json_string_literal(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                use std::fmt::Write;
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Pass an upstream redirect to the embedded WebView (rewrites `Location` to `erp(s)://`).
pub fn browser_redirect_response(status: u16, http_location: &str) -> http::Response<Vec<u8>> {
    let proxy_location = to_proxy_scheme_url(http_location);
    log::info!("[erp] pass_redirect status={status} -> {proxy_location}");
    http::Response::builder()
        .status(status)
        .header("Location", proxy_location)
        .header("Cache-Control", "no-cache")
        .header("Access-Control-Allow-Origin", "*")
        .body(Vec::new())
        .unwrap()
}

/// Cross-origin redirect via HTML `location.replace` — **deprecated for documents** (causes
/// OAuth loops). Kept for tests; document navigations use [`browser_redirect_response`].
pub fn webview_navigation_handoff(http_location: &str) -> http::Response<Vec<u8>> {
    let proxy_url = to_proxy_scheme_url(http_location);
    log::info!("[erp] hop class=cross_origin_handoff -> {proxy_url}");
    let json = json_string_literal(&proxy_url);
    let body = format!(
        concat!(
            "<!DOCTYPE html><html><head><meta charset=\"utf-8\">",
            "<script>location.replace({json});</script>",
            "</head><body></body></html>"
        ),
        json = json
    );
    http::Response::builder()
        .status(200)
        .header("Content-Type", "text/html; charset=utf-8")
        .header("Cache-Control", "no-cache")
        .header("Access-Control-Allow-Origin", "*")
        .body(body.into_bytes())
        .unwrap()
}

#[allow(dead_code)] // reserved for client-side redirect-loop detection
pub fn redirect_loop_error_response(visited: &HashSet<String>) -> http::Response<Vec<u8>> {
    let hops = visited.iter().take(8).cloned().collect::<Vec<_>>().join("\n  ");
    log::warn!("[erp] redirect loop:\n  {hops}");
    let body = format!(
        concat!(
            "<!DOCTYPE html><html><head><meta charset=\"utf-8\"></head>",
            "<body style=\"font-family:system-ui,sans-serif;padding:2rem;max-width:40rem;line-height:1.5\">",
            "<h1>Redirect loop</h1>",
            "<p>The server bounced between URLs repeatedly. Common causes:</p>",
            "<ul><li>Stale session cookies in this embedded browser</li>",
            "<li>SSO/OAuth handshakes that must run in a normal browser tab first</li>",
            "<li><code>localhost</code> vs <code>127.0.0.1</code> — pick one host and stick to it</li></ul>",
            "<p>Use the <strong>Sign in</strong> button in the toolbar to complete auth in your ",
            "system browser, then return here and reload. You can also clear site data for this ",
            "host or restart Easy Review.</p>",
            "<pre style=\"background:#f4f4f4;padding:1rem;overflow:auto;font-size:12px\">{hops}</pre>",
            "</body></html>"
        ),
        hops = hops
    );
    http::Response::builder()
        .status(200)
        .header("Content-Type", "text/html; charset=utf-8")
        .header("Cache-Control", "no-cache")
        .header("Access-Control-Allow-Origin", "*")
        .body(body.into_bytes())
        .unwrap()
}

pub struct UpstreamFetch {
    pub response: ureq::Response,
    pub headers: Vec<ProxyHeader>,
}

pub enum UpstreamFetchError {
    Transport(ureq::Error),
    /// Same-origin redirect — pass through as HTTP `Location` on `erp(s)://`.
    BrowserRedirect {
        status: u16,
        location: String,
    },
    /// Cross-origin redirect — iframe cannot follow `Location: erps://`; use HTML handoff.
    CrossOriginHandoff(String),
}

pub fn collect_ureq_headers(resp: &ureq::Response) -> Vec<ProxyHeader> {
    resp.headers_names()
        .into_iter()
        .flat_map(|name| {
            resp.all(&name).into_iter().map(move |value| ProxyHeader {
                name: name.clone(),
                value: value.to_string(),
            })
        })
        .collect()
}

/// Single-hop document `GET` — redirects are returned to the WebView, not followed server-side.
pub fn fetch_upstream_get(
    agent: &ureq::Agent,
    forward_headers: &[(String, String)],
    initial_target: &str,
    forward_cookies: bool,
) -> Result<UpstreamFetch, UpstreamFetchError> {
    log::info!(
        "[erp] document_get url={initial_target} cookies={forward_cookies}"
    );
    let mut req = agent
        .get(initial_target)
        .set("Accept-Encoding", "identity");
    for (name, value) in forward_headers {
        if !forward_cookies && name.eq_ignore_ascii_case("cookie") {
            continue;
        }
        req = req.set(name, value);
    }
    let resp = match req.call() {
        Ok(resp) => resp,
        Err(ureq::Error::Status(_, resp)) => resp,
        Err(e) => return Err(UpstreamFetchError::Transport(e)),
    };
    let status = resp.status();
    let headers = collect_ureq_headers(&resp);
    if is_redirect_status(status) {
        let Some(location) = headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case("location"))
            .map(|h| h.value.as_str())
        else {
            return Ok(UpstreamFetch { response: resp, headers });
        };
        let next = resolve_upstream_redirect_location(location, initial_target);
        if same_upstream_origin(initial_target, &next) {
            log::info!("[erp] same_origin_redirect status={status} -> {next}");
            return Err(UpstreamFetchError::BrowserRedirect {
                status,
                location: next,
            });
        }
        log::info!("[erp] cross_origin_handoff -> {next}");
        return Err(UpstreamFetchError::CrossOriginHandoff(next));
    }
    Ok(UpstreamFetch {
        response: resp,
        headers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_upstream_origin_compares_scheme_and_host() {
        assert!(same_upstream_origin(
            "http://localhost:5173/",
            "http://localhost:5173/auth/signin"
        ));
        assert!(!same_upstream_origin(
            "http://localhost:5173/",
            "https://auth.example.com/oauth/callback"
        ));
    }

    #[test]
    fn resolve_relative_redirect_location() {
        assert_eq!(
            resolve_upstream_redirect_location("/login", "http://127.0.0.1:5173/"),
            "http://127.0.0.1:5173/login"
        );
    }

    #[test]
    fn handoff_rewrites_https_to_erps() {
        let resp = webview_navigation_handoff("https://auth.example.com/oauth?state=1");
        let body = String::from_utf8(resp.body().clone()).unwrap();
        assert!(body.contains("erps://auth.example.com/oauth?state=1"));
    }

    #[test]
    fn pass_redirect_rewrites_location_header() {
        let resp = browser_redirect_response(302, "https://auth.example.com/oauth?state=1");
        assert_eq!(resp.status(), 302);
        let loc = resp
            .headers()
            .get("Location")
            .and_then(|v| v.to_str().ok())
            .unwrap();
        assert_eq!(loc, "erps://auth.example.com/oauth?state=1");
    }

    #[test]
    fn redirect_visit_key_strips_clerk_handshake_query() {
        assert_eq!(
            redirect_visit_key("http://localhost:5173/"),
            redirect_visit_key("http://localhost:5173/?__clerk_handshake=abc123")
        );
    }

    #[test]
    fn url_has_transient_oauth_query_detects_clerk() {
        assert!(url_has_transient_oauth_query(
            "http://localhost:5173/?__clerk_handshake=abc"
        ));
        assert!(!url_has_transient_oauth_query("http://localhost:5173/"));
    }

    #[test]
    fn redirect_visit_key_keeps_non_transient_query() {
        assert_ne!(
            redirect_visit_key("http://localhost:5173/?tab=1"),
            redirect_visit_key("http://localhost:5173/")
        );
    }
}
