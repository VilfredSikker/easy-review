//! Turn a `gh` process result into a typed [`GhError`].
//!
//! This is the only module that reads `gh` stderr. With `--include`, `gh api`
//! prints the HTTP status line, the response headers and the body, so a status
//! is known directly; without it, the stderr table is the only signal.

use crate::error::GhError;
use std::collections::BTreeMap;
use std::process::{ExitStatus, Output};
use std::time::{SystemTime, UNIX_EPOCH};

/// One HTTP response block from `gh api --include`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GhHeaders {
    status: u16,
    /// Header names lowercased.
    headers: BTreeMap<String, String>,
}

impl GhHeaders {
    pub fn status(&self) -> u16 {
        self.status
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }
}

#[derive(Debug)]
pub struct GhOutput {
    pub status: ExitStatus,
    /// stdout with every header block removed.
    pub body: Vec<u8>,
    /// The first non-2xx block, else the first block.
    pub http: Option<GhHeaders>,
}

pub fn classify(
    spawn: std::io::Result<Output>,
    include: bool,
    paginated: bool,
) -> Result<GhOutput, GhError> {
    let output = match spawn {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(GhError::NotInstalled);
        }
        Err(error) if error.kind() == std::io::ErrorKind::TimedOut => {
            return Err(GhError::Timeout);
        }
        Err(error) => {
            return Err(GhError::Tool {
                status: -1,
                stderr: error.to_string(),
            });
        }
    };

    let (blocks, body) = split_pages(&output.stdout, paginated);
    let http = blocks
        .iter()
        .find(|block| block.status() >= 400)
        .or_else(|| blocks.first())
        .cloned();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if output.status.success() {
        return Ok(GhOutput {
            status: output.status,
            body,
            http,
        });
    }

    let exit = output.status.code();
    if include {
        if exit == Some(4) {
            return Err(GhError::Unauthenticated {
                message: "GitHub CLI is not authenticated. Run: gh auth login".into(),
            });
        }
        if let Some(block) = blocks.iter().find(|block| block.status() >= 400) {
            return Err(http_error(block, &body, &stderr));
        }
        if let Some(block) = blocks
            .iter()
            .find(|block| block.get("x-ratelimit-remaining") == Some("0"))
        {
            return Err(rate_limited(block, &body));
        }
    }

    Err(stderr_error(&stderr, exit.unwrap_or(-1)))
}

/// Strip `gh api --include` headers. Without `--paginate`, exactly one leading
/// block is removed and the rest is kept byte for byte. With `--paginate`, a
/// status line starts at offset 0, after `\n`, or immediately after `]` or `}`
/// at bracket depth 0. String and bracket state is tracked only in page bodies;
/// header blocks are skipped, so a quote in a header value cannot hide a later
/// page. `--paginate` with `--jq` is not supported: gh filters each page, so the
/// boundary is lost.
pub fn split_pages(stdout: &[u8], paginated: bool) -> (Vec<GhHeaders>, Vec<u8>) {
    if !paginated {
        return match parse_header_block(stdout, 0) {
            Some((headers, body_start)) => (vec![headers], stdout[body_start..].to_vec()),
            None => (Vec::new(), stdout.to_vec()),
        };
    }

    let starts = page_starts(stdout);
    let mut parsed = Vec::new();
    for start in starts {
        if let Some((headers, body_start)) = parse_header_block(stdout, start) {
            parsed.push((start, headers, body_start));
        }
    }
    if parsed.is_empty() {
        return (Vec::new(), stdout.to_vec());
    }

    let mut blocks = Vec::with_capacity(parsed.len());
    let mut body = Vec::new();
    for (index, (start, headers, body_start)) in parsed.iter().enumerate() {
        blocks.push(headers.clone());
        let end = parsed
            .get(index + 1)
            .map_or(stdout.len(), |(next_start, _, _)| {
                if *next_start > *start && stdout[*next_start - 1] == b'\n' {
                    *next_start - 1
                } else {
                    *next_start
                }
            });
        if body_start <= &end {
            body.extend_from_slice(&stdout[*body_start..end]);
        }
    }
    (blocks, body)
}

fn page_starts(stdout: &[u8]) -> Vec<usize> {
    let mut starts = vec![0];
    let mut in_headers = stdout.starts_with(b"HTTP/");
    let mut in_string = false;
    let mut escaped = false;
    let mut depth = 0usize;
    let mut i = 0;
    while i < stdout.len() {
        if in_headers {
            if stdout[i] == b'\n' {
                let rest = &stdout[i + 1..];
                if rest.starts_with(b"\r\n") {
                    in_headers = false;
                    i += 3;
                    continue;
                }
                if rest.starts_with(b"\n") {
                    in_headers = false;
                    i += 2;
                    continue;
                }
            }
            i += 1;
            continue;
        }
        let byte = stdout[i];
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'[' | b'{' => depth += 1,
            b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
        let after_bracket = matches!(byte, b']' | b'}') && depth == 0;
        if after_bracket && stdout[i + 1..].starts_with(b"HTTP/") {
            starts.push(i + 1);
            in_headers = true;
        }
        if byte == b'\n' && stdout[i + 1..].starts_with(b"HTTP/") {
            starts.push(i + 1);
            in_headers = true;
        }
        i += 1;
    }
    starts.sort_unstable();
    starts.dedup();
    starts
}

fn parse_header_block(stdout: &[u8], start: usize) -> Option<(GhHeaders, usize)> {
    let rest = stdout.get(start..)?;
    if !rest.starts_with(b"HTTP/") {
        return None;
    }
    let line_end = start + rest.iter().position(|&b| b == b'\n')?;
    let status_line = String::from_utf8_lossy(&stdout[start..line_end]);
    let status = status_line.split_whitespace().nth(1)?.parse().ok()?;

    let mut headers = BTreeMap::new();
    let mut pos = line_end + 1;
    loop {
        if stdout[pos..].starts_with(b"\r\n") {
            return Some((GhHeaders { status, headers }, pos + 2));
        }
        if stdout[pos..].starts_with(b"\n") {
            return Some((GhHeaders { status, headers }, pos + 1));
        }
        let end = pos + stdout[pos..].iter().position(|&b| b == b'\n')?;
        let mut line = &stdout[pos..end];
        if line.ends_with(b"\r") {
            line = &line[..line.len() - 1];
        }
        let text = String::from_utf8_lossy(line);
        if let Some((name, value)) = text.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
        pos = end + 1;
    }
}

fn http_error(block: &GhHeaders, body: &[u8], stderr: &str) -> GhError {
    let code = block.status();
    let message = if code == 422 {
        unprocessable_message(body, stderr, code)
    } else {
        http_message(body, stderr, code)
    };
    match code {
        401 => GhError::Unauthenticated { message },
        404 => GhError::NotFound { message },
        403 if is_rate_limit(block, body) => rate_limited(block, body),
        403 => GhError::Forbidden { message },
        429 => rate_limited(block, body),
        409 => GhError::Conflict { message },
        422 => GhError::Unprocessable { message },
        406 | 413 if is_too_large(body) => GhError::TooLarge { message },
        other => GhError::Tool {
            status: i32::from(other),
            stderr: stderr.to_string(),
        },
    }
}

fn rate_limited(block: &GhHeaders, body: &[u8]) -> GhError {
    GhError::RateLimited {
        reset_epoch_secs: reset_epoch_secs(block),
        message: http_message(body, "", block.status()),
    }
}

fn is_rate_limit(block: &GhHeaders, body: &[u8]) -> bool {
    block.get("x-ratelimit-remaining") == Some("0")
        || block.get("retry-after").is_some()
        || json_message(body).is_some_and(|m| m.to_ascii_lowercase().contains("rate limit"))
}

fn is_too_large(body: &[u8]) -> bool {
    let text = String::from_utf8_lossy(body).to_ascii_lowercase();
    text.contains("too_large") || text.contains("exceeded the maximum number")
}

fn reset_epoch_secs(block: &GhHeaders) -> Option<u64> {
    if let Some(secs) = block
        .get("retry-after")
        .and_then(|value| value.trim().parse::<u64>().ok())
    {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        return Some(now.saturating_add(secs));
    }
    if block.get("x-ratelimit-remaining") == Some("0") {
        return block
            .get("x-ratelimit-reset")
            .and_then(|value| value.trim().parse().ok());
    }
    None
}

/// GitHub's reason, gh's own line, and the status code when neither carries it.
fn http_message(body: &[u8], stderr: &str, code: u16) -> String {
    let body_message = match (json_message(body), json_error_detail(body)) {
        (Some(message), Some(detail)) => format!("{message}: {detail}"),
        (Some(message), None) => message,
        (None, Some(detail)) => detail,
        (None, None) => String::new(),
    };
    let stderr = stderr.trim();
    let mut message = match (stderr.is_empty(), body_message.is_empty()) {
        (true, _) => body_message,
        (false, true) => stderr.to_string(),
        (false, false) => format!("{stderr}: {body_message}"),
    };
    append_status(&mut message, code);
    message
}

/// A review 422 carries its reason in `errors` as plain strings, so keep every
/// entry: GitHub's top-level message, then each reason, joined with `; `. When
/// the body has neither, gh's stderr line is the only reason left.
fn unprocessable_message(body: &[u8], stderr: &str, code: u16) -> String {
    let mut parts = Vec::new();
    if let Some(message) = json_message(body) {
        parts.push(message);
    }
    for error in json_errors(body) {
        if !error.is_empty() {
            parts.push(error);
        }
    }
    let mut message = if parts.is_empty() {
        stderr.trim().to_string()
    } else {
        parts.join("; ")
    };
    append_status(&mut message, code);
    message
}

fn append_status(message: &mut String, code: u16) {
    let marker = format!("HTTP {code}");
    if !message
        .to_ascii_lowercase()
        .contains(&marker.to_ascii_lowercase())
    {
        if !message.is_empty() {
            message.push(' ');
        }
        message.push_str(&format!("(HTTP {code})"));
    }
}

fn json(body: &[u8]) -> Option<serde_json::Value> {
    serde_json::from_slice(body).ok()
}

fn json_message(body: &[u8]) -> Option<String> {
    json(body)?.get("message")?.as_str().map(str::to_string)
}

fn json_error_detail(body: &[u8]) -> Option<String> {
    json_errors(body).into_iter().next()
}

fn json_errors(body: &[u8]) -> Vec<String> {
    let Some(errors) = json(body).and_then(|value| value.get("errors").cloned()) else {
        return Vec::new();
    };
    let Some(errors) = errors.as_array() else {
        return Vec::new();
    };
    errors
        .iter()
        .filter_map(|error| {
            if let Some(text) = error.as_str() {
                return Some(text.to_string());
            }
            let message = error.get("message").and_then(|m| m.as_str());
            let field = error.get("field").and_then(|f| f.as_str());
            let code = error.get("code").and_then(|c| c.as_str());
            message.map(str::to_string).or_else(|| match (field, code) {
                (Some(field), Some(code)) => Some(format!("{field}: {code}")),
                (Some(field), None) => Some(field.to_string()),
                (None, Some(code)) => Some(code.to_string()),
                (None, None) => None,
            })
        })
        .collect()
}

fn stderr_error(stderr: &str, exit_code: i32) -> GhError {
    let message = stderr.trim().to_string();
    if let Some(code) = stderr_http_code(stderr) {
        return match code {
            401 => GhError::Unauthenticated { message },
            404 => GhError::NotFound { message },
            403 | 429 if message.to_ascii_lowercase().contains("rate limit") => {
                GhError::RateLimited {
                    reset_epoch_secs: None,
                    message,
                }
            }
            403 => GhError::Forbidden { message },
            429 => GhError::RateLimited {
                reset_epoch_secs: None,
                message,
            },
            409 => GhError::Conflict { message },
            422 => GhError::Unprocessable { message },
            406 | 413 => GhError::TooLarge { message },
            _ => GhError::Tool {
                status: exit_code,
                stderr: message,
            },
        };
    }
    let lower = message.to_ascii_lowercase();
    if lower.contains("could not resolve to a") {
        return GhError::NotFound { message };
    }
    if lower.contains("rate limit") {
        return GhError::RateLimited {
            reset_epoch_secs: None,
            message,
        };
    }
    if [
        "error connecting to",
        "dial tcp",
        "x509:",
        "tls:",
        "connection refused",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
    {
        return GhError::Network { message };
    }
    if [
        "gh auth login",
        "not logged into any github hosts",
        "the token in gh_token is invalid",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
    {
        return GhError::Unauthenticated { message };
    }
    GhError::Tool {
        status: exit_code,
        stderr: message,
    }
}

/// The first `HTTP NNN` in stderr, with word boundaries on both sides.
fn stderr_http_code(stderr: &str) -> Option<u16> {
    let bytes = stderr.as_bytes();
    for (i, _) in stderr.char_indices() {
        let Some(rest) = stderr.get(i..) else {
            continue;
        };
        if !rest.starts_with("HTTP ") {
            continue;
        }
        let before_is_word =
            i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_');
        if before_is_word {
            continue;
        }
        let Some(rest) = stderr.get(i + 5..) else {
            continue;
        };
        let digits: String = rest.chars().take(3).collect();
        if digits.len() == 3 && digits.bytes().all(|b| b.is_ascii_digit()) {
            let after_is_digit = rest.chars().nth(3).is_some_and(|c| c.is_ascii_digit());
            if !after_is_digit {
                return digits.parse().ok();
            }
        }
    }
    None
}

/// A `gh` that does not know `pr update-branch`; the caller turns its own
/// sentence around it.
pub fn is_unknown_command(err: &GhError) -> bool {
    matches!(err, GhError::Tool { stderr, .. } if stderr.contains("unknown command"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::output;
    use std::path::{Path, PathBuf};

    fn fixtures_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gh")
    }

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    fn flagged(error: &GhError, variant: &str) -> bool {
        match variant {
            "NotInstalled" => matches!(error, GhError::NotInstalled),
            "Timeout" => matches!(error, GhError::Timeout),
            "Unauthenticated" => matches!(error, GhError::Unauthenticated { .. }),
            "NotFound" => matches!(error, GhError::NotFound { .. }),
            "Forbidden" => matches!(error, GhError::Forbidden { .. }),
            "RateLimited" => matches!(error, GhError::RateLimited { .. }),
            "TooLarge" => matches!(error, GhError::TooLarge { .. }),
            "Conflict" => matches!(error, GhError::Conflict { .. }),
            "Unprocessable" => matches!(error, GhError::Unprocessable { .. }),
            "Network" => matches!(error, GhError::Network { .. }),
            "Tool" => matches!(error, GhError::Tool { .. }),
            "Local" => matches!(error, GhError::Local { .. }),
            other => panic!("unknown expected variant {other}"),
        }
    }

    fn classify_fixture(dir: &Path) -> Result<GhOutput, GhError> {
        let args: Vec<String> = read(&dir.join("args"))
            .lines()
            .map(str::to_string)
            .collect();
        let exit: i32 = read(&dir.join("exit")).trim().parse().unwrap();
        let stdout = std::fs::read(dir.join("stdout")).unwrap();
        let stderr = std::fs::read(dir.join("stderr")).unwrap();
        let include = args.iter().any(|arg| arg == "--include");
        let paginated = args.iter().any(|arg| arg == "--paginate");
        classify(Ok(output(exit, stdout, stderr)), include, paginated)
    }

    #[test]
    fn classify_fixtures_match_their_expected_variant() {
        let mut checked = 0;
        for entry in std::fs::read_dir(fixtures_root()).unwrap() {
            let dir = entry.unwrap().path();
            if !dir.is_dir() || !dir.join("args").is_file() {
                continue;
            }
            let expect = read(&dir.join("expect")).trim().to_string();
            let result = classify_fixture(&dir);
            match expect.as_str() {
                "Ok" => {
                    let output =
                        result.unwrap_or_else(|error| panic!("{}: {error}", dir.display()));
                    if dir
                        .file_name()
                        .is_some_and(|name| name == "api_paginate_raw_two_pages.synthetic")
                    {
                        assert_eq!(output.body, b"[{\"id\":1}][{\"id\":2}]");
                    }
                }
                variant => {
                    let error = result
                        .err()
                        .unwrap_or_else(|| panic!("{}: expected {variant}, got Ok", dir.display()));
                    assert!(
                        flagged(&error, variant),
                        "{}: expected {variant}, got {error:?}",
                        dir.display()
                    );
                    if let Ok(message) = std::fs::read_to_string(dir.join("message")) {
                        assert!(
                            error.to_string().contains(message.trim()),
                            "{}: `{}` lacks `{}`",
                            dir.display(),
                            error,
                            message.trim()
                        );
                    }
                }
            }
            checked += 1;
        }
        assert!(checked >= 19, "only {checked} fixtures checked");
    }

    #[test]
    fn exit_code_4_is_unauthenticated() {
        let error = classify(Ok(output(4, Vec::new(), Vec::new())), true, false).unwrap_err();
        assert!(
            matches!(error, GhError::Unauthenticated { .. }),
            "{error:?}"
        );
    }

    #[test]
    fn a_success_with_an_exhausted_limit_is_not_an_error() {
        let stdout = "HTTP/2.0 200 OK\r\nX-Ratelimit-Remaining: 0\r\n\r\n{\"ok\":true}";
        let result = classify(Ok(output(0, stdout, "")), true, false).unwrap();
        assert_eq!(result.body, b"{\"ok\":true}");
    }

    #[test]
    fn retry_after_takes_precedence_over_the_reset_header() {
        let stdout = "HTTP/2.0 403 Forbidden\r\nRetry-After: 60\r\n\
             X-Ratelimit-Remaining: 0\r\nX-Ratelimit-Reset: 4102444800\r\n\r\n\
             {\"message\":\"API rate limit exceeded\"}";
        let before = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let error = classify(Ok(output(1, stdout, "")), true, false).unwrap_err();
        let after = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert!(
            matches!(
                error,
                GhError::RateLimited {
                    reset_epoch_secs: Some(reset),
                    ..
                } if reset >= before + 60 && reset <= after + 60
            ),
            "{error:?}"
        );
    }

    #[test]
    fn the_reset_header_counts_only_when_the_limit_is_exhausted() {
        let stdout = "HTTP/2.0 403 Forbidden\r\nX-Ratelimit-Remaining: 5\r\n\
             X-Ratelimit-Reset: 4102444800\r\n\r\n\
             {\"message\":\"You have exceeded a secondary rate limit\"}";
        let error = classify(Ok(output(1, stdout, "")), true, false).unwrap_err();
        assert!(
            matches!(
                error,
                GhError::RateLimited {
                    reset_epoch_secs: None,
                    ..
                }
            ),
            "{error:?}"
        );
    }

    #[test]
    fn a_graphql_200_error_falls_to_the_stderr_table() {
        let error = classify_fixture(&fixtures_root().join("graphql_200_not_found")).unwrap_err();
        assert!(matches!(error, GhError::NotFound { .. }), "{error:?}");
    }

    #[test]
    fn split_pages_keeps_a_raw_body_byte_for_byte() {
        let stdout =
            std::fs::read(fixtures_root().join("api_paginate_raw_two_pages.synthetic/stdout"))
                .unwrap();
        let (blocks, body) = split_pages(&stdout, true);
        assert_eq!(blocks.len(), 2);
        assert_eq!(body, b"[{\"id\":1}][{\"id\":2}]");
    }

    #[test]
    fn split_pages_finds_a_status_line_after_a_bracket() {
        let stdout = b"HTTP/2.0 200 OK\r\n\r\n[{\"id\":1}]HTTP/2.0 200 OK\r\n\r\n[{\"id\":2}]";
        let (blocks, body) = split_pages(stdout, true);
        assert_eq!(blocks.len(), 2);
        assert_eq!(body, b"[{\"id\":1}][{\"id\":2}]");
    }

    #[test]
    fn split_pages_ignores_a_status_line_inside_nested_brackets() {
        let stdout = b"HTTP/2.0 200 OK\r\n\r\n[[1]HTTP/2.0 200 OK\r\nX-Test: y\r\n\r\n2]";
        let (blocks, body) = split_pages(stdout, true);
        assert_eq!(blocks.len(), 1);
        assert_eq!(body, b"[[1]HTTP/2.0 200 OK\r\nX-Test: y\r\n\r\n2]");
    }

    #[test]
    fn split_pages_keeps_an_unpaginated_body_byte_for_byte() {
        let stdout = b"HTTP/2.0 200 OK\r\nX-Test: y\r\n\r\n[a]\nHTTP/2.0 200 OK\r\n\r\n[b]";
        let (blocks, body) = split_pages(stdout, false);
        assert_eq!(blocks.len(), 1);
        assert_eq!(body, b"[a]\nHTTP/2.0 200 OK\r\n\r\n[b]");
    }

    #[test]
    fn split_pages_keeps_a_quote_in_a_header_from_hiding_the_next_page() {
        let stdout =
            b"HTTP/2.0 200 OK\r\nX-Reason: say \"hi\r\n\r\n[1]\nHTTP/2.0 200 OK\r\n\r\n[2]";
        let (blocks, body) = split_pages(stdout, true);
        assert_eq!(blocks.len(), 2);
        assert_eq!(body, b"[1][2]");
    }

    #[test]
    fn a_paginated_second_page_failure_is_classified() {
        let error =
            classify_fixture(&fixtures_root().join("api_paginate_second_page_404.synthetic"))
                .unwrap_err();
        assert!(matches!(error, GhError::NotFound { .. }), "{error:?}");
    }

    #[test]
    fn the_422_message_keeps_the_code_and_reason() {
        let error = classify_fixture(&fixtures_root().join("api_422_review")).unwrap_err();
        assert!(
            matches!(error, GhError::Unprocessable { ref message }
                if message.contains("(HTTP 422)")
                    && message.contains("Review Can not approve your own pull request")),
            "{error:?}"
        );
    }

    #[test]
    fn unprocessable_keeps_string_errors() {
        let error =
            classify_fixture(&fixtures_root().join("api_422_review_string_errors.synthetic"))
                .unwrap_err();
        let GhError::Unprocessable { message } = error else {
            panic!("expected Unprocessable");
        };
        assert!(
            message.contains("user_id can only have one pending review per pull request"),
            "{message}"
        );
        assert!(
            message.contains("Review Can not approve your own pull request"),
            "{message}"
        );
        assert!(message.contains("; "), "{message}");
    }

    #[test]
    fn a_422_without_a_json_message_keeps_the_stderr_line() {
        let error = classify(
            Ok(output(
                1,
                "HTTP/2.0 422 Unprocessable Entity\r\n\r\n{}",
                "gh: Reference does not exist (HTTP 422)",
            )),
            true,
            false,
        )
        .unwrap_err();
        assert!(
            matches!(&error, GhError::Unprocessable { message } if message.contains("Reference does not exist")),
            "{error:?}"
        );
    }

    #[test]
    fn split_pages_ignores_http_inside_a_comment_body() {
        let comment = r#"{"body":"see [RFC]HTTP/1.1 200 OK"}"#;
        let stdout =
            format!("HTTP/2.0 200 OK\r\n\r\n[{comment}]\nHTTP/2.0 200 OK\r\n\r\n[{{\"id\":2}}]");
        let (blocks, body) = split_pages(stdout.as_bytes(), true);
        assert_eq!(blocks.len(), 2);
        assert_eq!(body, format!("[{comment}][{{\"id\":2}}]").into_bytes());
    }

    #[test]
    fn split_pages_ignores_a_status_line_inside_a_string() {
        let stdout = b"HTTP/2.0 200 OK\r\n\r\n\"one\nHTTP/1.1 200 OK\r\nX-Test: y\r\n\r\n\"";
        let (blocks, body) = split_pages(stdout, true);
        assert_eq!(blocks.len(), 1);
        assert_eq!(body, b"\"one\nHTTP/1.1 200 OK\r\nX-Test: y\r\n\r\n\"");
    }

    #[test]
    fn a_403_with_remaining_zero_is_rate_limited_with_the_reset_time() {
        let error =
            classify_fixture(&fixtures_root().join("api_403_rate_limit.synthetic")).unwrap_err();
        assert!(
            matches!(
                error,
                GhError::RateLimited {
                    reset_epoch_secs: Some(1793976383),
                    ..
                }
            ),
            "{error:?}"
        );
    }

    #[test]
    fn a_plain_403_is_forbidden_not_rate_limited() {
        let error =
            classify_fixture(&fixtures_root().join("api_403_forbidden.synthetic")).unwrap_err();
        assert!(matches!(error, GhError::Forbidden { .. }), "{error:?}");
    }

    #[test]
    fn spawn_not_found_is_not_installed_and_timed_out_is_timeout() {
        let missing = classify(
            Err(std::io::Error::from(std::io::ErrorKind::NotFound)),
            false,
            false,
        )
        .unwrap_err();
        assert!(matches!(missing, GhError::NotInstalled));
        let timed_out = classify(
            Err(std::io::Error::from(std::io::ErrorKind::TimedOut)),
            false,
            false,
        )
        .unwrap_err();
        assert!(matches!(timed_out, GhError::Timeout));
    }

    #[test]
    fn a_404_in_a_pr_number_is_not_a_not_found() {
        let error = classify(
            Ok(output(
                1,
                Vec::new(),
                "gh: failed on PR #404 (HTTP 500)".to_string(),
            )),
            false,
            false,
        )
        .unwrap_err();
        assert!(matches!(error, GhError::Tool { .. }), "{error:?}");
    }
}
