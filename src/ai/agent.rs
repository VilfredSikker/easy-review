use std::process::Child;

// ── Message types ──

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MessageRole {
    User,
    Agent,
    System,
}

#[derive(Debug, Clone)]
pub struct AgentMessage {
    pub role: MessageRole,
    pub text: String,
    pub timestamp: String,
}

// ── Context snapshot ──

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct AgentContext {
    pub file: Option<String>,
    pub hunk_index: Option<usize>,
    pub hunk_header: Option<String>,
    pub hunk_diff: Option<String>,
    pub line_number: Option<usize>,
    pub line_content: Option<String>,
    pub finding_title: Option<String>,
    pub finding_severity: Option<String>,
    pub finding_description: Option<String>,
    pub finding_suggestion: Option<String>,
    pub base_branch: String,
    pub head_branch: String,
}

// ── Agent state ──

pub struct AgentState {
    pub messages: Vec<AgentMessage>,
    pub input: String,
    pub scroll: u16,
    pub child: Option<Child>,
    pub is_running: bool,
    pub partial_response: String,
    pub context: AgentContext,
}

impl AgentState {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            input: String::new(),
            scroll: 0,
            child: None,
            is_running: false,
            partial_response: String::new(),
            context: AgentContext::default(),
        }
    }
}

// ── Agent configuration ──

#[derive(Debug, serde::Deserialize)]
pub struct AgentConfig {
    pub agent: AgentConfigInner,
}

#[derive(Debug, serde::Deserialize)]
pub struct AgentConfigInner {
    pub command: String,
    pub args: Vec<String>,
}

impl Default for AgentConfigInner {
    fn default() -> Self {
        Self {
            command: "claude".to_string(),
            args: vec![
                "--print".to_string(),
                "-p".to_string(),
                "{prompt}".to_string(),
            ],
        }
    }
}

pub fn load_agent_config(repo_root: &str) -> AgentConfigInner {
    let local = format!("{repo_root}/.er-agent.toml");
    let global = dirs::config_dir()
        .map(|d| d.join("er/agent.toml").to_string_lossy().to_string());

    for path in [Some(local), global].into_iter().flatten() {
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(config) = toml::from_str::<AgentConfig>(&content) {
                return config.agent;
            }
        }
    }

    AgentConfigInner::default()
}

pub fn resolve_command(
    config: &AgentConfigInner,
    prompt: &str,
    context_file: &str,
    context: &AgentContext,
) -> (String, Vec<String>) {
    let cmd = config.command.clone();
    let args = config
        .args
        .iter()
        .map(|a| {
            a.replace("{prompt}", prompt)
                .replace("{context_file}", context_file)
                .replace("{file}", context.file.as_deref().unwrap_or(""))
                .replace("{hunk}", context.hunk_diff.as_deref().unwrap_or(""))
                .replace("{line}", context.line_content.as_deref().unwrap_or(""))
        })
        .collect();
    (cmd, args)
}

pub fn build_full_prompt(user_prompt: &str, ctx: &AgentContext) -> String {
    let mut parts = Vec::new();

    if let Some(ref file) = ctx.file {
        let mut loc = format!("File: {file}");
        if let Some(idx) = ctx.hunk_index {
            loc.push_str(&format!(", hunk #{idx}"));
        }
        if let Some(ln) = ctx.line_number {
            loc.push_str(&format!(", line {ln}"));
        }
        parts.push(loc);
    }

    if let Some(ref diff) = ctx.hunk_diff {
        parts.push(format!("Diff:\n{diff}"));
    }

    if let Some(ref title) = ctx.finding_title {
        let sev = ctx.finding_severity.as_deref().unwrap_or("?");
        parts.push(format!("Finding: [{sev}] {title}"));
    }

    if parts.is_empty() {
        user_prompt.to_string()
    } else {
        format!("{}\n\nQuestion: {}", parts.join("\n"), user_prompt)
    }
}

pub fn expand_slash_command(input: &str) -> Option<String> {
    match input.trim() {
        "/explain" => Some("Explain this change. Why was it made and what does it do?".into()),
        "/safe" | "/security" => {
            Some("Review this change for security issues. Is it safe?".into())
        }
        "/test" => Some("Write a unit test for this change.".into()),
        "/bug" | "/issues" => {
            Some("What could go wrong with this change? Any edge cases?".into())
        }
        "/suggest" => Some("Suggest improvements to this code.".into()),
        _ => None,
    }
}
