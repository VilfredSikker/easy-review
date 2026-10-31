//! Images embedded in a PR description, fetched as the signed-in `gh` user.
//!
//! Attachments dragged into a GitHub PR live behind the viewer's GitHub
//! session when the repo is private, so the webview cannot load them with a
//! plain `<img>`. The token never reaches the webview: this module fetches the
//! bytes and hands back a `data:` URL. The token is sent only to hosts GitHub
//! serves attachments from, and ureq drops it on the redirect to the signed
//! CDN URL.

use std::io::Read as _;
use std::time::Duration;

use er_engine::proc::CommandTimeoutExt as _;

use crate::commands::run_blocking;

/// Larger attachments are screenshots of a whole screen at most; a cap keeps a
/// hostile body from streaming an unbounded response into memory.
const MAX_IMAGE_BYTES: u64 = 15 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Auth {
    /// github.com attachment paths and raw files of private repos need the token.
    Token,
    /// Already signed or public CDN URLs.
    Anonymous,
}

/// Which GitHub image URLs this command will fetch, and whether the token goes
/// with the request. Anything else is refused: the command must not become a
/// general authenticated fetch the webview can point anywhere.
fn route(url: &str) -> Option<Auth> {
    let rest = url.strip_prefix("https://")?;
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    if host.contains(['@', ':']) {
        return None;
    }
    match host {
        "github.com" => {
            let segments: Vec<&str> = path.split(['?', '#']).next()?.split('/').collect();
            let attachment =
                segments.first() == Some(&"user-attachments") && segments.get(1) == Some(&"assets");
            // Older uploads: /<owner>/<repo>/assets/<id>/<uuid>.
            let repo_asset = segments.len() >= 4 && segments[2] == "assets";
            (attachment || repo_asset).then_some(Auth::Token)
        }
        "raw.githubusercontent.com" => Some(Auth::Token),
        "user-images.githubusercontent.com" | "private-user-images.githubusercontent.com" => {
            Some(Auth::Anonymous)
        }
        _ => None,
    }
}

fn gh_token() -> Option<String> {
    let output = std::process::Command::new("gh")
        .args(["auth", "token", "--hostname", "github.com"])
        .output_timed(er_engine::proc::GH_TIMEOUT)
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let token = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (!token.is_empty()).then_some(token)
}

/// `image/png` from `image/png; charset=binary`; `None` for anything that is
/// not an image, so an HTML login page never renders as a broken picture.
fn image_mime(content_type: &str) -> Option<String> {
    let mime = content_type.split(';').next()?.trim().to_ascii_lowercase();
    mime.starts_with("image/").then_some(mime)
}

fn fetch(url: &str) -> Result<String, String> {
    let auth = route(url).ok_or("Not a GitHub image URL")?;
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(20))
        .build();
    let mut request = agent.get(url).set("User-Agent", "easy-review-desktop");
    if auth == Auth::Token {
        if let Some(token) = gh_token() {
            request = request.set("Authorization", &format!("token {token}"));
        }
    }
    let response = request
        .call()
        .map_err(|e| format!("Image request failed: {e}"))?;
    let mime = image_mime(response.content_type()).ok_or("Response is not an image")?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(MAX_IMAGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Image read failed: {e}"))?;
    if bytes.len() as u64 > MAX_IMAGE_BYTES {
        return Err("Image is too large to preview".to_string());
    }
    use base64::Engine as _;
    Ok(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

#[tauri::command]
pub async fn fetch_github_image(url: String) -> Result<String, String> {
    run_blocking(move || fetch(&url)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attachment_paths_on_github_get_the_token() {
        assert_eq!(
            route("https://github.com/user-attachments/assets/1b2c-3d"),
            Some(Auth::Token)
        );
        assert_eq!(
            route("https://github.com/acme/web/assets/123/abc-def"),
            Some(Auth::Token)
        );
        assert_eq!(
            route("https://raw.githubusercontent.com/acme/web/main/shot.png"),
            Some(Auth::Token)
        );
    }

    #[test]
    fn signed_cdn_urls_are_fetched_without_the_token() {
        assert_eq!(
            route("https://private-user-images.githubusercontent.com/1/2.png?jwt=x"),
            Some(Auth::Anonymous)
        );
        assert_eq!(
            route("https://user-images.githubusercontent.com/1/2.png"),
            Some(Auth::Anonymous)
        );
    }

    #[test]
    fn everything_else_is_refused() {
        for url in [
            "http://github.com/user-attachments/assets/x",
            "https://github.com/acme/web/pull/12",
            "https://github.com/settings/tokens",
            "https://github.com.evil.example/user-attachments/assets/x",
            "https://user@github.com/user-attachments/assets/x",
            "https://github.com:444/user-attachments/assets/x",
            "https://example.com/a.png",
            "file:///etc/passwd",
        ] {
            assert_eq!(route(url), None, "{url}");
        }
    }

    #[test]
    fn only_image_responses_are_accepted() {
        assert_eq!(
            image_mime("image/png; charset=binary").as_deref(),
            Some("image/png")
        );
        assert_eq!(image_mime("IMAGE/GIF").as_deref(), Some("image/gif"));
        assert_eq!(image_mime("text/html; charset=utf-8"), None);
    }
}
