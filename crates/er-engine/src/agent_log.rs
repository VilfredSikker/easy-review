//! The agent log: the lines a running agent reports, as the TUI's panel and
//! the desktop's task log show them. Always compiled, so a host with no App
//! can launch a tab command and read its log.

use er_agent::job::JobProgress;

#[derive(Debug, Clone)]
pub enum AgentLogSource {
    Stdout,
    Stderr,
    Status,
}

#[derive(Debug, Clone)]
pub struct AgentLogEntry {
    pub timestamp: std::time::Instant,
    /// Shared, not owned: every line of a run carries the same name, and an
    /// agent writing a JSON event per line allocates thousands of copies of a
    /// string that never changes. `Arc<str>` rather than `Arc<String>` so the
    /// allocation is one block.
    pub command_name: std::sync::Arc<str>,
    pub source: AgentLogSource,
    pub text: String,
}

/// What one agent stdout line adds to the agent log: the parsed stream-json
/// event, or the trimmed raw line for other formats. `None` when that is empty.
pub fn stdout_log_text(line: &str, is_stream_json: bool) -> Option<String> {
    let display = if is_stream_json {
        parse_stream_json_line(line)
    } else {
        Some(line.trim().to_string())
    };
    display.filter(|text| !text.is_empty())
}

/// Forward one runner progress event to the agent log, dropping stdout lines
/// the stream-json parser finds empty.
pub(crate) fn log_job_progress(
    event: JobProgress,
    is_stream_json: bool,
    name_stdout: &std::sync::Arc<str>,
    name_stderr: &std::sync::Arc<str>,
    sender: &std::sync::mpsc::Sender<AgentLogEntry>,
) {
    let (command_name, source, text) = match event {
        JobProgress::StdoutLine(line) => {
            let line = line.trim_end_matches(['\r', '\n']);
            let Some(text) = stdout_log_text(line, is_stream_json) else {
                return;
            };
            (name_stdout.clone(), AgentLogSource::Stdout, text)
        }
        JobProgress::StderrLine(line) => (
            name_stderr.clone(),
            AgentLogSource::Stderr,
            line.trim_end_matches(['\r', '\n']).to_string(),
        ),
    };
    let _ = sender.send(AgentLogEntry {
        timestamp: std::time::Instant::now(),
        command_name,
        source,
        text,
    });
}

/// Parse a Claude Code `--output-format stream-json` line into a human-readable string.
/// Returns `None` for events that should be suppressed (noise).
///
/// The JSON shape is decoded in `er_agent::decode`, the one place that knows
/// a provider's event format; this only renders what it reports.
pub(crate) fn parse_stream_json_line(line: &str) -> Option<String> {
    use er_agent::decode::{LogEvent, LogPart};

    match er_agent::decode::decode_log_line(line)? {
        LogEvent::Assistant(parts) => {
            let rendered: Vec<String> = parts
                .into_iter()
                .map(|part| match part {
                    LogPart::Text(text) => text,
                    LogPart::ToolUse { name, input } => render_tool_use(&name, input.as_ref()),
                })
                .collect();
            (!rendered.is_empty()).then(|| rendered.join("  "))
        }
        LogEvent::SystemNote(text) => Some(format!("⊘ {}", text)),
    }
}

fn render_tool_use(name: &str, input: Option<&serde_json::Value>) -> String {
    let input = input.and_then(|input| input.as_object());
    let detail = match name {
        "Read" | "Write" | "Edit" => input
            .and_then(|input| input.get("file_path"))
            .and_then(|path| path.as_str())
            .map(shorten_path)
            .unwrap_or_default(),
        "Bash" => input
            .and_then(|input| input.get("command"))
            .and_then(|command| command.as_str())
            .map(str::to_string)
            .unwrap_or_default(),
        "Glob" | "Grep" => input
            .and_then(|input| input.get("pattern"))
            .and_then(|pattern| pattern.as_str())
            .map(str::to_string)
            .unwrap_or_default(),
        _ => String::new(),
    };
    if detail.is_empty() {
        format!("→ {}", name)
    } else {
        format!("→ {} {}", name, detail)
    }
}

pub(crate) fn shorten_path(path: &str) -> String {
    // Show last 2 components for readability
    let parts: Vec<&str> = path.rsplit('/').take(2).collect();
    if parts.len() == 2 {
        format!("{}/{}", parts[1], parts[0])
    } else {
        parts[0].to_string()
    }
}
