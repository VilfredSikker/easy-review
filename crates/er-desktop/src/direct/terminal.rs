//! The in-app shell drawer's PTY sessions, and the dev-server URL the
//! review browser opens first.

use std::sync::Arc;

use er_protocol::{ErError, ErErrorKind};
use tauri::State;

use super::failed;
use crate::state::DesktopState;

// ── Terminal (in-app shell drawer) ───────────────────────────────────────────
//
// Each frontend `Terminal.svelte` instance owns a `session_id` (typically
// `tab-<idx>`). On mount it calls `terminal_spawn`, then streams keystrokes via
// `terminal_write` and resize events via `terminal_resize`. Output is pushed
// back as a Tauri event (`terminal-output`) so we don't poll. On unmount,
// `terminal_close` removes the session from the map — `PtySession::drop` then
// kills the child shell.

#[derive(serde::Serialize, Clone)]
struct TerminalOutputPayload {
    session_id: String,
    bytes: Vec<u8>,
}

#[derive(serde::Serialize, Clone)]
struct TerminalExitPayload {
    session_id: String,
}

#[tauri::command]
pub fn terminal_spawn(
    session_id: String,
    cwd: String,
    app_handle: tauri::AppHandle,
    state: State<DesktopState>,
) -> Result<(), ErError> {
    use std::io::Read;
    use tauri::Emitter;

    // Idempotent spawn: keep an existing PTY alive (hide/show must not kill shells).
    {
        let map = state.terminals.lock().map_err(failed)?;
        if map.contains_key(&session_id) {
            return Ok(());
        }
    }

    let (session, mut reader) = crate::terminal::PtySession::spawn(&cwd).map_err(failed)?;
    {
        let mut map = state.terminals.lock().map_err(failed)?;
        map.insert(session_id.clone(), session);
    }

    let handle = app_handle;
    let sid = session_id;
    let terminals_for_thread = Arc::clone(&state.terminals);
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => {
                    let _ = handle.emit(
                        "terminal-exit",
                        TerminalExitPayload {
                            session_id: sid.clone(),
                        },
                    );
                    if let Ok(mut map) = terminals_for_thread.lock() {
                        map.remove(&sid);
                    }
                    break;
                }
                Ok(n) => {
                    let _ = handle.emit(
                        "terminal-output",
                        TerminalOutputPayload {
                            session_id: sid.clone(),
                            bytes: buf[..n].to_vec(),
                        },
                    );
                }
                Err(_) => {
                    let _ = handle.emit(
                        "terminal-exit",
                        TerminalExitPayload {
                            session_id: sid.clone(),
                        },
                    );
                    if let Ok(mut map) = terminals_for_thread.lock() {
                        map.remove(&sid);
                    }
                    break;
                }
            }
        }
    });

    Ok(())
}

#[tauri::command]
pub fn terminal_write(
    session_id: String,
    bytes: Vec<u8>,
    state: State<DesktopState>,
) -> Result<(), ErError> {
    let mut map = state.terminals.lock().map_err(failed)?;
    let session = map
        .get_mut(&session_id)
        .ok_or_else(|| no_session(&session_id))?;
    session.write(&bytes).map_err(failed)
}

#[tauri::command]
pub fn terminal_resize(
    session_id: String,
    rows: u16,
    cols: u16,
    state: State<DesktopState>,
) -> Result<(), ErError> {
    let mut map = state.terminals.lock().map_err(failed)?;
    let session = map
        .get_mut(&session_id)
        .ok_or_else(|| no_session(&session_id))?;
    session.resize(rows, cols).map_err(failed)
}

#[tauri::command]
pub fn terminal_close(session_id: String, state: State<DesktopState>) -> Result<(), ErError> {
    let mut map = state.terminals.lock().map_err(failed)?;
    map.remove(&session_id);
    Ok(())
}

// ── Dev-URL detection ─────────────────────────────────────────────────────────

/// Pure helper: given the textual contents of a `package.json`, return the
/// best-guess local dev-server URL. Returns the Vite default if a
/// `package.json` was found but no signal matched.
fn detect_dev_url_from_package_json(text: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(text).ok()?;
    let scripts = json.get("scripts").and_then(|v| v.as_object());
    let script = scripts
        .and_then(|s| s.get("dev").and_then(|v| v.as_str()))
        .or_else(|| scripts.and_then(|s| s.get("start").and_then(|v| v.as_str())));

    if let Some(cmd) = script {
        // Explicit port flag wins.
        if let Some(port) = explicit_port(cmd) {
            return Some(format!("http://localhost:{port}"));
        }
        if let Some(port) = port_from_command(cmd) {
            return Some(format!("http://localhost:{port}"));
        }
    }
    // package.json present but no signal — fall back to Vite default.
    Some("http://localhost:5173".to_string())
}

/// Scan a script string for `--port N` or `-p N` and return N.
fn explicit_port(cmd: &str) -> Option<u16> {
    let tokens: Vec<&str> = cmd.split_whitespace().collect();
    let mut i = 0;
    while i < tokens.len() {
        let t = tokens[i];
        // --port=N
        if let Some(rest) = t.strip_prefix("--port=") {
            if let Ok(n) = rest.parse() {
                return Some(n);
            }
        }
        if let Some(rest) = t.strip_prefix("-p=") {
            if let Ok(n) = rest.parse() {
                return Some(n);
            }
        }
        // --port N  /  -p N
        if (t == "--port" || t == "-p") && i + 1 < tokens.len() {
            if let Ok(n) = tokens[i + 1].parse() {
                return Some(n);
            }
        }
        i += 1;
    }
    None
}

/// Recognize common dev-server commands and return their conventional port.
fn port_from_command(cmd: &str) -> Option<u16> {
    let c = cmd.to_lowercase();
    // Order matters: check longer/more-specific patterns first.
    if c.contains("next dev") {
        return Some(3000);
    }
    if c.contains("astro dev") {
        return Some(4321);
    }
    if c.contains("webpack-dev-server") || c.contains("webpack serve") {
        return Some(8080);
    }
    if c.contains("rails server") || c.contains("bin/dev") {
        return Some(3000);
    }
    if c.contains("manage.py runserver") {
        return Some(8000);
    }
    if c.contains("fastapi run") {
        return Some(8000);
    }
    if c.contains("bun --hot") || c.contains("bun run --hot") {
        return Some(3000);
    }
    if c.contains("vite") {
        return Some(5173);
    }
    None
}

#[tauri::command]
pub fn detect_dev_url(repo_root: String) -> Result<Option<String>, ErError> {
    if repo_root.is_empty() {
        return Ok(None);
    }
    let pkg = std::path::Path::new(&repo_root).join("package.json");
    if pkg.exists() {
        let text = std::fs::read_to_string(&pkg).map_err(failed)?;
        return Ok(detect_dev_url_from_package_json(&text));
    }
    // No package.json — caller may extend later (pyproject.toml / Cargo.toml).
    Ok(None)
}

fn no_session(session_id: &str) -> ErError {
    ErError {
        kind: ErErrorKind::NotFound,
        message: format!("no terminal session: {session_id}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_pkg(dir: &std::path::Path, body: &str) {
        std::fs::write(dir.join("package.json"), body).unwrap();
    }

    #[test]
    fn detect_dev_url_vite_default() {
        let tmp = tempfile::tempdir().unwrap();
        write_pkg(tmp.path(), r#"{ "scripts": { "dev": "vite" } }"#);
        let got = detect_dev_url(tmp.path().to_string_lossy().to_string()).unwrap();
        assert_eq!(got.as_deref(), Some("http://localhost:5173"));
    }

    #[test]
    fn detect_dev_url_next() {
        let tmp = tempfile::tempdir().unwrap();
        write_pkg(tmp.path(), r#"{ "scripts": { "dev": "next dev" } }"#);
        let got = detect_dev_url(tmp.path().to_string_lossy().to_string()).unwrap();
        assert_eq!(got.as_deref(), Some("http://localhost:3000"));
    }

    #[test]
    fn detect_dev_url_explicit_port() {
        let tmp = tempfile::tempdir().unwrap();
        write_pkg(
            tmp.path(),
            r#"{ "scripts": { "dev": "vite --port 4200" } }"#,
        );
        let got = detect_dev_url(tmp.path().to_string_lossy().to_string()).unwrap();
        assert_eq!(
            got.as_deref(),
            Some("http://localhost:4200"),
            "explicit --port N should win over framework default"
        );
    }

    #[test]
    fn detect_dev_url_no_package_json() {
        let tmp = tempfile::tempdir().unwrap();
        let got = detect_dev_url(tmp.path().to_string_lossy().to_string()).unwrap();
        assert_eq!(got, None);
    }

    #[test]
    fn detect_dev_url_unknown_command() {
        let tmp = tempfile::tempdir().unwrap();
        write_pkg(
            tmp.path(),
            r#"{ "scripts": { "dev": "node scripts/start.js" } }"#,
        );
        let got = detect_dev_url(tmp.path().to_string_lossy().to_string()).unwrap();
        assert_eq!(
            got.as_deref(),
            Some("http://localhost:5173"),
            "unknown command should fall back to vite default when package.json exists"
        );
    }
}
