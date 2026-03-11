use crate::ai::{AiState, RiskLevel};
use crate::config::ArchwatchConfig;
use crate::git::DiffFile;
use anyhow::{Context, Result};
use std::net::TcpStream;
use std::process::Command;
use std::time::Duration;

/// Collected highlight data for Archwatch: file paths with optional risk levels.
struct HighlightEntry {
    path: String,
    risk: Option<RiskLevel>,
}

/// Gather changed file paths from the current diff, enriched with AI risk levels.
fn collect_highlights(files: &[DiffFile], ai: &AiState) -> Vec<HighlightEntry> {
    files
        .iter()
        .map(|f| HighlightEntry {
            path: f.path.clone(),
            risk: ai.file_review(&f.path).map(|r| r.risk),
        })
        .collect()
}

/// Build `--highlight` arguments for the archwatch CLI.
/// Format: `--highlight path1,path2,...`
fn build_highlight_arg(entries: &[HighlightEntry]) -> String {
    entries
        .iter()
        .map(|e| e.path.as_str())
        .collect::<Vec<_>>()
        .join(",")
}

/// Build risk-level query parameter string.
/// Format: `high=path1,path2&medium=path3&low=path4`
fn build_risk_params(entries: &[HighlightEntry]) -> String {
    let mut high = Vec::new();
    let mut medium = Vec::new();
    let mut low = Vec::new();
    let mut info = Vec::new();

    for entry in entries {
        match entry.risk {
            Some(RiskLevel::High) => high.push(entry.path.as_str()),
            Some(RiskLevel::Medium) => medium.push(entry.path.as_str()),
            Some(RiskLevel::Low) => low.push(entry.path.as_str()),
            Some(RiskLevel::Info) => info.push(entry.path.as_str()),
            None => {}
        }
    }

    let mut params = Vec::new();
    if !high.is_empty() {
        params.push(format!("high={}", high.join(",")));
    }
    if !medium.is_empty() {
        params.push(format!("medium={}", medium.join(",")));
    }
    if !low.is_empty() {
        params.push(format!("low={}", low.join(",")));
    }
    if !info.is_empty() {
        params.push(format!("info={}", info.join(",")));
    }
    params.join("&")
}

/// Try to send highlight updates to an already-running Archwatch instance via WebSocket.
/// Returns `true` if update was sent successfully, `false` if no instance is reachable.
fn try_websocket_update(config: &ArchwatchConfig, entries: &[HighlightEntry]) -> bool {
    let ws_url = format!("ws://127.0.0.1:{}/ws", config.port);

    // Quick connectivity check with a short timeout
    let addr = format!("127.0.0.1:{}", config.port);
    if TcpStream::connect_timeout(
        &addr
            .parse()
            .unwrap_or_else(|_| std::net::SocketAddr::from(([127, 0, 0, 1], config.port))),
        Duration::from_millis(500),
    )
    .is_err()
    {
        return false;
    }

    // Build the highlight update payload
    let files: Vec<serde_json::Value> = entries
        .iter()
        .map(|e| {
            serde_json::json!({
                "path": e.path,
                "risk": e.risk.map(|r| match r {
                    RiskLevel::High => "high",
                    RiskLevel::Medium => "medium",
                    RiskLevel::Low => "low",
                    RiskLevel::Info => "info",
                })
            })
        })
        .collect();

    let message = serde_json::json!({
        "type": "highlight",
        "files": files,
    });

    // Attempt WebSocket connection and send
    match tungstenite::connect(&ws_url) {
        Ok((mut ws, _)) => {
            let payload = message.to_string();
            let result = ws.send(tungstenite::Message::Text(payload));
            let _ = ws.close(None);
            result.is_ok()
        }
        Err(_) => false,
    }
}

/// Launch Archwatch with highlighting of changed modules.
/// First attempts to update a running instance via WebSocket; if no instance
/// is reachable, spawns a new archwatch process.
pub fn launch_archwatch(
    config: &ArchwatchConfig,
    repo_root: &str,
    files: &[DiffFile],
    ai: &AiState,
) -> Result<String> {
    let entries = collect_highlights(files, ai);

    if entries.is_empty() {
        return Ok("No changed files to highlight".to_string());
    }

    // Try updating a running instance first
    if try_websocket_update(config, &entries) {
        return Ok(format!(
            "Updated running Archwatch ({} files)",
            entries.len()
        ));
    }

    // No running instance — spawn a new one
    let highlight_arg = build_highlight_arg(&entries);
    let risk_params = build_risk_params(&entries);

    let mut cmd = Command::new(&config.binary);
    cmd.current_dir(repo_root)
        .arg("--highlight")
        .arg(&highlight_arg)
        .arg("--port")
        .arg(config.port.to_string());

    if !risk_params.is_empty() {
        cmd.arg("--risk").arg(&risk_params);
    }

    cmd.stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context(format!(
            "Failed to launch archwatch (binary: '{}'). Is it installed?",
            config.binary
        ))?;

    Ok(format!(
        "Launched Archwatch ({} files highlighted)",
        entries.len()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_highlight_arg() {
        let entries = vec![
            HighlightEntry {
                path: "src/main.rs".to_string(),
                risk: Some(RiskLevel::High),
            },
            HighlightEntry {
                path: "src/lib.rs".to_string(),
                risk: None,
            },
        ];
        assert_eq!(build_highlight_arg(&entries), "src/main.rs,src/lib.rs");
    }

    #[test]
    fn test_build_risk_params() {
        let entries = vec![
            HighlightEntry {
                path: "src/main.rs".to_string(),
                risk: Some(RiskLevel::High),
            },
            HighlightEntry {
                path: "src/config.rs".to_string(),
                risk: Some(RiskLevel::High),
            },
            HighlightEntry {
                path: "src/lib.rs".to_string(),
                risk: Some(RiskLevel::Medium),
            },
            HighlightEntry {
                path: "README.md".to_string(),
                risk: None,
            },
        ];
        assert_eq!(
            build_risk_params(&entries),
            "high=src/main.rs,src/config.rs&medium=src/lib.rs"
        );
    }

    #[test]
    fn test_build_risk_params_empty() {
        let entries = vec![HighlightEntry {
            path: "src/main.rs".to_string(),
            risk: None,
        }];
        assert_eq!(build_risk_params(&entries), "");
    }

    #[test]
    fn test_collect_highlights_empty() {
        let files: Vec<DiffFile> = vec![];
        let ai = AiState::default();
        let entries = collect_highlights(&files, &ai);
        assert!(entries.is_empty());
    }
}
