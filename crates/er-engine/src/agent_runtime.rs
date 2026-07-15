//! Shared provider/model invocation and artifact contracts for app-launched AI.

use crate::ai::{
    ErChecklist, ErGitHubComments, ErOrder, ErQuestions, ErReview, ErTour, ExpertReview,
    ProfessorReview, TriageReview,
};
use crate::config::{inject_provider_effort, ErConfig};
use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliFamily {
    Claude,
    Codex,
    Cursor,
    Other,
}

impl CliFamily {
    pub fn detect(command: &str) -> Self {
        match Path::new(command)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(command)
        {
            "claude" => Self::Claude,
            "codex" => Self::Codex,
            "agent" => Self::Cursor,
            _ => Self::Other,
        }
    }

    fn supports_claude_stream_json(self) -> bool {
        matches!(self, Self::Claude | Self::Cursor)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentTaskKind {
    Review,
    Expert(String),
    Professor,
    Triage,
    Tour { filename: String },
    Questions,
    Summary,
    ValidateReview,
    ValidateComments,
    CardReply,
    ArenaRound,
    Other(String),
}

impl AgentTaskKind {
    pub fn from_command_name(name: &str) -> Self {
        if let Some(id) = name.strip_prefix("expert-") {
            return Self::Expert(id.to_string());
        }
        match name {
            "review" => Self::Review,
            "professor" => Self::Professor,
            "triage" => Self::Triage,
            "questions" => Self::Questions,
            "summary" => Self::Summary,
            "validate" => Self::ValidateReview,
            "validate-comments" => Self::ValidateComments,
            other => Self::Other(other.to_string()),
        }
    }

    pub fn artifact_contract(&self) -> ArtifactContract {
        match self {
            Self::Review => ArtifactContract::Review,
            Self::Expert(id) => ArtifactContract::Expert { id: id.clone() },
            Self::Professor => ArtifactContract::Professor,
            Self::Triage => ArtifactContract::Triage,
            Self::Tour { filename } => ArtifactContract::Tour {
                filename: filename.clone(),
            },
            Self::Questions => ArtifactContract::Questions,
            Self::Summary => ArtifactContract::Summary,
            Self::ValidateReview => ArtifactContract::ValidateReview,
            Self::ValidateComments => ArtifactContract::ValidateComments,
            Self::CardReply | Self::ArenaRound | Self::Other(_) => ArtifactContract::None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentAccessProfile {
    ReadOnly,
    PreparedArtifacts { output_dir: String },
    LocalArtifacts { output_dir: String },
    RemoteArtifacts { output_dir: String },
}

impl AgentAccessProfile {
    fn output_dir(&self) -> Option<&str> {
        match self {
            Self::ReadOnly => None,
            Self::PreparedArtifacts { output_dir }
            | Self::LocalArtifacts { output_dir }
            | Self::RemoteArtifacts { output_dir } => Some(output_dir),
        }
    }

    fn claude_tools(&self) -> &'static [&'static str] {
        const READ_ONLY: &[&str] = &[
            "Read",
            "Bash(grep *)",
            "Bash(rg *)",
            "Bash(git grep*)",
            "Bash(git show*)",
            "Bash(git log*)",
        ];
        const PREPARED: &[&str] = &[
            "Read",
            "Write",
            "Edit",
            "Bash(grep *)",
            "Bash(rg *)",
            "Bash(git grep*)",
            "Bash(cp *)",
            "Bash(shasum*)",
            "Bash(sha256sum*)",
            "Bash(mkdir*)",
            "Bash(awk*)",
        ];
        const LOCAL: &[&str] = &[
            "Read",
            "Write",
            "Edit",
            "Bash(grep *)",
            "Bash(rg *)",
            "Bash(git grep*)",
            "Bash(git diff*)",
            "Bash(cp *)",
            "Bash(shasum*)",
            "Bash(sha256sum*)",
            "Bash(mkdir*)",
            "Bash(awk*)",
        ];
        const REMOTE: &[&str] = &[
            "Read",
            "Write",
            "Edit",
            "Bash(gh pr *)",
            "Bash(grep *)",
            "Bash(rg *)",
            "Bash(git grep*)",
            "Bash(cp *)",
            "Bash(shasum*)",
            "Bash(sha256sum*)",
            "Bash(mkdir*)",
            "Bash(awk*)",
        ];
        match self {
            Self::ReadOnly => READ_ONLY,
            Self::PreparedArtifacts { .. } => PREPARED,
            Self::LocalArtifacts { .. } => LOCAL,
            Self::RemoteArtifacts { .. } => REMOTE,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputProtocol {
    Plain,
    ClaudeStreamJson,
}

#[derive(Debug, Clone)]
pub struct AgentInvocation {
    pub command: String,
    pub args: Vec<String>,
    pub work_dir: String,
    pub family: CliFamily,
    pub output_protocol: OutputProtocol,
}

#[derive(Debug, Clone)]
pub enum AgentSelection<'a> {
    Runtime {
        provider_id: Option<&'a str>,
        model_id: Option<&'a str>,
    },
    Exact {
        provider_id: &'a str,
        model_id: &'a str,
    },
}

#[derive(Debug, Clone)]
pub struct AgentInvocationRequest<'a> {
    pub selection: AgentSelection<'a>,
    pub task: &'a AgentTaskKind,
    pub effort: Option<&'a str>,
    pub effort_override: Option<&'a str>,
    pub work_dir: String,
    pub access: AgentAccessProfile,
    pub live_logs: bool,
}

pub fn resolve_invocation(
    config: &ErConfig,
    request: AgentInvocationRequest<'_>,
) -> Result<AgentInvocation> {
    let (command, mut args, resolved_provider_id, resolved_model_id) = match request.selection {
        AgentSelection::Runtime {
            provider_id,
            model_id,
        } => {
            if let Some(pid) = config.ai_hub.resolve_provider_id(provider_id) {
                let provider = config
                    .ai_hub
                    .providers
                    .get(&pid)
                    .with_context(|| format!("unknown provider: {pid}"))?;
                let mut args = provider.args.clone();
                let resolved_model = config.ai_hub.resolve_model_id(&pid, model_id);
                if let Some(model_id) = &resolved_model {
                    if let Some(model) = provider.models.iter().find(|m| m.id == *model_id) {
                        args.extend(model.args.clone());
                    }
                }
                (provider.command.clone(), args, Some(pid), resolved_model)
            } else {
                (
                    config.agent.command.clone(),
                    config.agent.args.clone(),
                    None,
                    (!config.agent.model.is_empty()).then(|| config.agent.model.clone()),
                )
            }
        }
        AgentSelection::Exact {
            provider_id,
            model_id,
        } => {
            let provider = config
                .ai_hub
                .providers
                .get(provider_id)
                .with_context(|| format!("unknown provider: {provider_id}"))?;
            let model = provider
                .models
                .iter()
                .find(|m| m.id == model_id)
                .with_context(|| format!("unknown model {model_id} for provider {provider_id}"))?;
            let mut args = provider.args.clone();
            args.extend(model.args.clone());
            (provider.command.clone(), args, Some(provider_id.to_string()), Some(model_id.to_string()))
        }
    };

    let family = CliFamily::detect(&command);
    if family == CliFamily::Claude {
        inject_allowed_tools(&mut args, request.access.claude_tools());
    }
    crate::config::inject_agent_storage_access(&command, &mut args);
    if family == CliFamily::Codex {
        if let Some(output_dir) = request.access.output_dir() {
            inject_codex_writable_dir(&mut args, output_dir);
        }
    }
    if matches!(family, CliFamily::Claude | CliFamily::Codex) {
        let effort = crate::config::resolve_effort_for_model(
            &config.ai_hub,
            &config.agent,
            resolved_provider_id.as_deref(),
            resolved_model_id.as_deref(),
            request.effort,
            request.effort_override,
        );
        inject_provider_effort(
            &command,
            &mut args,
            resolved_model_id.as_deref(),
            effort.as_deref(),
        );
    }
    if request.live_logs && family.supports_claude_stream_json() {
        inject_stream_json(&mut args, family);
    }

    let output_protocol = if args.iter().any(|arg| arg == "stream-json") {
        OutputProtocol::ClaudeStreamJson
    } else {
        OutputProtocol::Plain
    };

    Ok(AgentInvocation {
        command,
        args,
        work_dir: request.work_dir,
        family,
        output_protocol,
    })
}

fn inject_allowed_tools(args: &mut Vec<String>, tools: &[&str]) {
    for tool in tools.iter().rev() {
        if has_option_value(args, "--allowedTools", tool) {
            continue;
        }
        args.insert(0, (*tool).to_string());
        args.insert(0, "--allowedTools".to_string());
    }
}

fn inject_codex_writable_dir(args: &mut Vec<String>, output_dir: &str) {
    if has_option_value(args, "--add-dir", output_dir)
        || args
            .iter()
            .any(|arg| arg == &format!("--add-dir={output_dir}"))
    {
        return;
    }
    args.push("--add-dir".to_string());
    args.push(output_dir.to_string());
}

fn inject_stream_json(args: &mut Vec<String>, family: CliFamily) {
    if !args.iter().any(|arg| arg == "--output-format") {
        args.push("--output-format".to_string());
        args.push("stream-json".to_string());
    }
    if family == CliFamily::Claude
        && args.iter().any(|arg| arg == "--print")
        && args.iter().any(|arg| arg == "stream-json")
        && !args.iter().any(|arg| arg == "--verbose")
    {
        args.push("--verbose".to_string());
    }
}

fn has_option_value(args: &[String], option: &str, value: &str) -> bool {
    args.windows(2)
        .any(|pair| pair[0] == option && pair[1] == value)
}

#[derive(Debug, Clone, Copy)]
pub struct AgentPrompt<'a> {
    pub system: Option<&'a str>,
    pub user: &'a str,
}

pub fn build_argv(invocation: &AgentInvocation, prompt: AgentPrompt<'_>) -> Vec<String> {
    let mut args = invocation.args.clone();
    let has_placeholder = args.iter().any(|arg| arg.contains("{prompt}"));
    let combined = match (invocation.family, prompt.system) {
        (CliFamily::Claude, _) | (_, None) => prompt.user.to_string(),
        (_, Some(system)) => format!("{system}\n\nUser request:\n{}", prompt.user),
    };

    for arg in &mut args {
        if arg.contains("{prompt}") {
            *arg = arg.replace("{prompt}", &combined);
        }
    }

    if invocation.family == CliFamily::Claude {
        if let Some(system) = prompt.system {
            if let Some(index) = args.iter().position(|arg| arg == "--append-system-prompt") {
                if index + 1 < args.len() {
                    args[index + 1] = system.to_string();
                } else {
                    args.push(system.to_string());
                }
            } else {
                args.push("--append-system-prompt".to_string());
                args.push(system.to_string());
            }
        }
    }

    if !has_placeholder {
        args.push(combined);
    }
    args
}

pub fn decode_final_text(stdout: &str, protocol: OutputProtocol) -> String {
    if protocol == OutputProtocol::Plain {
        return stdout.to_string();
    }

    let mut last_result = None;
    let mut assistant_text = Vec::new();
    for line in stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        match value.get("type").and_then(|kind| kind.as_str()) {
            Some("result") => {
                if let Some(result) = value.get("result").and_then(|result| result.as_str()) {
                    last_result = Some(result.to_string());
                }
            }
            Some("assistant") => {
                if let Some(content) = value
                    .get("message")
                    .and_then(|message| message.get("content"))
                    .and_then(|content| content.as_array())
                {
                    for item in content {
                        if item.get("type").and_then(|kind| kind.as_str()) == Some("text") {
                            if let Some(text) = item.get("text").and_then(|text| text.as_str()) {
                                if !text.trim().is_empty() {
                                    assistant_text.push(text.trim().to_string());
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    last_result
        .filter(|text| !text.trim().is_empty())
        .or_else(|| (!assistant_text.is_empty()).then(|| assistant_text.join("\n\n")))
        .unwrap_or_else(|| stdout.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtifactContract {
    None,
    Review,
    Expert { id: String },
    Professor,
    Triage,
    Tour { filename: String },
    Questions,
    Summary,
    ValidateReview,
    ValidateComments,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileFingerprint {
    len: u64,
    modified: Option<SystemTime>,
    digest: String,
}

#[derive(Debug, Clone)]
pub struct ArtifactBaseline {
    contract: ArtifactContract,
    files: BTreeMap<String, Option<FileFingerprint>>,
}

impl ArtifactBaseline {
    pub fn capture(contract: ArtifactContract, output_dir: &str) -> Result<Self> {
        let mut files = BTreeMap::new();
        for relative in contract.required_paths() {
            files.insert(
                relative.clone(),
                fingerprint(Path::new(output_dir).join(relative))?,
            );
        }
        if contract == ArtifactContract::ValidateComments {
            let summary = "summary.md".to_string();
            files.insert(
                summary.clone(),
                fingerprint(Path::new(output_dir).join(summary))?,
            );
        }
        Ok(Self { contract, files })
    }

    pub fn validate(&self, output_dir: &str) -> Result<()> {
        if self.contract == ArtifactContract::None {
            return Ok(());
        }
        let output_dir = Path::new(output_dir);
        for relative in self.contract.required_paths() {
            self.require_changed(output_dir, &relative)?;
        }
        if self.contract == ArtifactContract::ValidateComments
            && self
                .files
                .get("summary.md")
                .and_then(Option::as_ref)
                .is_some()
        {
            self.require_changed(output_dir, "summary.md")?;
        }

        let expected_hash = if self.contract.requires_diff_hash() {
            let diff_path = output_dir.join("diff-tmp");
            let diff = std::fs::read_to_string(&diff_path).with_context(|| {
                format!("could not read prepared diff at {}", diff_path.display())
            })?;
            Some(crate::ai::compute_diff_hash(&diff))
        } else {
            None
        };

        match &self.contract {
            ArtifactContract::None => {}
            ArtifactContract::Review => {
                validate_json_hash::<ErReview>(
                    output_dir,
                    "review.json",
                    expected_hash.as_deref(),
                    |v| &v.diff_hash,
                )?;
                validate_json_hash::<ErOrder>(
                    output_dir,
                    "order.json",
                    expected_hash.as_deref(),
                    |v| &v.diff_hash,
                )?;
                validate_json_hash::<ErChecklist>(
                    output_dir,
                    "checklist.json",
                    expected_hash.as_deref(),
                    |v| &v.diff_hash,
                )?;
                validate_non_empty(output_dir.join("summary.md"))?;
            }
            ArtifactContract::Expert { id } => validate_json_hash::<ExpertReview>(
                output_dir,
                &format!("experts/{id}.json"),
                expected_hash.as_deref(),
                |v| &v.diff_hash,
            )?,
            ArtifactContract::Professor => validate_json_hash::<ProfessorReview>(
                output_dir,
                "professor.json",
                expected_hash.as_deref(),
                |v| &v.diff_hash,
            )?,
            ArtifactContract::Triage => validate_json_hash::<TriageReview>(
                output_dir,
                "triage.json",
                expected_hash.as_deref(),
                |v| &v.diff_hash,
            )?,
            ArtifactContract::Tour { filename } => {
                validate_json_hash::<ErTour>(output_dir, filename, expected_hash.as_deref(), |v| {
                    &v.diff_hash
                })?
            }
            ArtifactContract::Questions => {
                validate_json_hash::<ErQuestions>(
                    output_dir,
                    "questions.json",
                    expected_hash.as_deref(),
                    |v| &v.diff_hash,
                )?;
                validate_json_hash::<ErQuestions>(
                    output_dir,
                    "questions.prev.json",
                    expected_hash.as_deref(),
                    |v| &v.diff_hash,
                )?;
            }
            ArtifactContract::Summary => validate_non_empty(output_dir.join("summary.md"))?,
            ArtifactContract::ValidateReview => {
                validate_json_hash::<ErReview>(
                    output_dir,
                    "review.json",
                    expected_hash.as_deref(),
                    |v| &v.diff_hash,
                )?;
                validate_non_empty(output_dir.join("summary.md"))?;
            }
            ArtifactContract::ValidateComments => validate_json_hash::<ErGitHubComments>(
                output_dir,
                "github-comments.json",
                expected_hash.as_deref(),
                |v| &v.diff_hash,
            )?,
        }
        Ok(())
    }

    fn require_changed(&self, output_dir: &Path, relative: &str) -> Result<()> {
        let path = output_dir.join(relative);
        let after = fingerprint(&path)?.with_context(|| {
            format!(
                "agent exited successfully but did not write {}",
                path.display()
            )
        })?;
        if self.files.get(relative).and_then(Option::as_ref) == Some(&after) {
            anyhow::bail!(
                "agent exited successfully but did not update {}",
                path.display()
            );
        }
        Ok(())
    }
}

impl ArtifactContract {
    fn required_paths(&self) -> Vec<String> {
        match self {
            Self::None => vec![],
            Self::Review => vec![
                "review.json".into(),
                "order.json".into(),
                "checklist.json".into(),
                "summary.md".into(),
            ],
            Self::Expert { id } => vec![format!("experts/{id}.json")],
            Self::Professor => vec!["professor.json".into()],
            Self::Triage => vec!["triage.json".into()],
            Self::Tour { filename } => vec![filename.clone()],
            Self::Questions => vec!["questions.json".into(), "questions.prev.json".into()],
            Self::Summary => vec!["summary.md".into()],
            Self::ValidateReview => vec!["review.json".into(), "summary.md".into()],
            Self::ValidateComments => vec!["github-comments.json".into()],
        }
    }

    fn requires_diff_hash(&self) -> bool {
        !matches!(self, Self::None | Self::Summary)
    }
}

fn fingerprint(path: impl AsRef<Path>) -> Result<Option<FileFingerprint>> {
    let path = path.as_ref();
    let Ok(metadata) = std::fs::metadata(path) else {
        return Ok(None);
    };
    let bytes = std::fs::read(path)
        .with_context(|| format!("read artifact fingerprint {}", path.display()))?;
    Ok(Some(FileFingerprint {
        len: metadata.len(),
        modified: metadata.modified().ok(),
        digest: format!("{:x}", Sha256::digest(&bytes)),
    }))
}

fn validate_json_hash<T: DeserializeOwned>(
    output_dir: &Path,
    relative: &str,
    expected_hash: Option<&str>,
    hash: impl FnOnce(&T) -> &str,
) -> Result<()> {
    let path = output_dir.join(relative);
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("read required artifact {}", path.display()))?;
    let parsed: T = serde_json::from_str(&text)
        .with_context(|| format!("agent wrote invalid JSON to {}", path.display()))?;
    if let Some(expected) = expected_hash {
        let actual = hash(&parsed);
        if actual != expected {
            anyhow::bail!(
                "agent wrote stale {} (expected diff hash {}, got {})",
                relative,
                expected,
                actual
            );
        }
    }
    Ok(())
}

fn validate_non_empty(path: PathBuf) -> Result<()> {
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("read required artifact {}", path.display()))?;
    if text.trim().is_empty() {
        anyhow::bail!("agent wrote an empty artifact to {}", path.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("er-agent-runtime-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn write_review_contract(dir: &Path, diff_hash: &str) {
        std::fs::write(
            dir.join("review.json"),
            format!(r#"{{"version":1,"diff_hash":"{diff_hash}","files":{{}}}}"#),
        )
        .unwrap();
        std::fs::write(
            dir.join("order.json"),
            format!(r#"{{"version":1,"diff_hash":"{diff_hash}","order":[],"groups":{{}}}}"#),
        )
        .unwrap();
        std::fs::write(
            dir.join("checklist.json"),
            format!(r#"{{"version":1,"diff_hash":"{diff_hash}","items":[]}}"#),
        )
        .unwrap();
        std::fs::write(dir.join("summary.md"), "Review summary").unwrap();
    }

    fn codex_config() -> ErConfig {
        let mut config = ErConfig::default();
        crate::config::supplement_ai_hub(&mut config.ai_hub);
        config.ai_hub.default_provider = Some("codex".into());
        config.ai_hub.default_model = Some("gpt-5.4".into());
        config
    }

    #[test]
    fn codex_artifact_invocation_adds_managed_dir_once() {
        let config = codex_config();
        let task = AgentTaskKind::Review;
        let request = AgentInvocationRequest {
            selection: AgentSelection::Runtime {
                provider_id: Some("codex"),
                model_id: Some("gpt-5.4"),
            },
            task: &task,
            effort: None,
            effort_override: None,
            work_dir: "/repo".into(),
            access: AgentAccessProfile::PreparedArtifacts {
                output_dir: "/managed/review".into(),
            },
            live_logs: true,
        };
        let invocation = resolve_invocation(&config, request).unwrap();
        assert_eq!(invocation.family, CliFamily::Codex);
        assert_eq!(
            invocation
                .args
                .windows(2)
                .filter(|pair| pair[0] == "--add-dir" && pair[1] == "/managed/review")
                .count(),
            1
        );
    }

    #[test]
    fn codex_invocation_injects_reasoning_effort() {
        let config = codex_config();
        let task = AgentTaskKind::Review;
        let invocation = resolve_invocation(
            &config,
            AgentInvocationRequest {
                selection: AgentSelection::Runtime {
                    provider_id: Some("codex"),
                    model_id: Some("gpt-5.6-sol"),
                },
                task: &task,
                effort: Some("high"),
                effort_override: None,
                work_dir: "/repo".into(),
                access: AgentAccessProfile::ReadOnly,
                live_logs: false,
            },
        )
        .unwrap();
        assert_eq!(invocation.family, CliFamily::Codex);
        assert!(has_option_value(
            &invocation.args,
            "-c",
            "model_reasoning_effort=high"
        ));
    }

    #[test]
    fn codex_invocation_skips_effort_for_unsupported_model() {
        let config = codex_config();
        let task = AgentTaskKind::Review;
        let invocation = resolve_invocation(
            &config,
            AgentInvocationRequest {
                selection: AgentSelection::Runtime {
                    provider_id: Some("codex"),
                    model_id: Some("gpt-5.4"),
                },
                task: &task,
                effort: Some("high"),
                effort_override: None,
                work_dir: "/repo".into(),
                access: AgentAccessProfile::ReadOnly,
                live_logs: false,
            },
        )
        .unwrap();
        assert!(!invocation
            .args
            .iter()
            .any(|arg| arg.starts_with("model_reasoning_effort=")));
    }

    #[test]
    fn ordinary_tour_and_triage_use_the_configured_default_model() {
        let mut config = codex_config();
        config.ai_hub.default_model = Some("gpt-5.6-luna".into());

        for task in [
            AgentTaskKind::Tour {
                filename: "tour.json".into(),
            },
            AgentTaskKind::Triage,
        ] {
            let invocation = resolve_invocation(
                &config,
                AgentInvocationRequest {
                    selection: AgentSelection::Runtime {
                        provider_id: Some("codex"),
                        model_id: None,
                    },
                    task: &task,
                    effort: None,
                    effort_override: None,
                    work_dir: "/repo".into(),
                    access: AgentAccessProfile::ReadOnly,
                    live_logs: false,
                },
            )
            .unwrap();
            assert!(has_option_value(
                &invocation.args,
                "--model",
                "gpt-5.6-luna"
            ));
            assert!(!has_option_value(
                &invocation.args,
                "--model",
                "gpt-5.3-codex-spark"
            ));
        }
    }

    #[test]
    fn explicit_runtime_model_overrides_the_default_for_one_invocation() {
        let mut config = codex_config();
        config.ai_hub.default_model = Some("gpt-5.6-luna".into());
        let task = AgentTaskKind::Review;
        let invocation = resolve_invocation(
            &config,
            AgentInvocationRequest {
                selection: AgentSelection::Runtime {
                    provider_id: Some("codex"),
                    model_id: Some("gpt-5.5"),
                },
                task: &task,
                effort: None,
                effort_override: None,
                work_dir: "/repo".into(),
                access: AgentAccessProfile::ReadOnly,
                live_logs: false,
            },
        )
        .unwrap();
        assert!(has_option_value(&invocation.args, "--model", "gpt-5.5"));
        assert!(!has_option_value(
            &invocation.args,
            "--model",
            "gpt-5.6-luna"
        ));
    }

    #[test]
    fn codex_card_prompt_combines_system_without_claude_flag() {
        let config = codex_config();
        let task = AgentTaskKind::CardReply;
        let invocation = resolve_invocation(
            &config,
            AgentInvocationRequest {
                selection: AgentSelection::Runtime {
                    provider_id: Some("codex"),
                    model_id: Some("gpt-5.4"),
                },
                task: &task,
                effort: None,
                effort_override: None,
                work_dir: "/repo".into(),
                access: AgentAccessProfile::ReadOnly,
                live_logs: false,
            },
        )
        .unwrap();
        let args = build_argv(
            &invocation,
            AgentPrompt {
                system: Some("system context"),
                user: "question",
            },
        );
        assert!(!args.iter().any(|arg| arg == "--append-system-prompt"));
        assert!(args.iter().any(|arg| {
            arg.contains("system context") && arg.contains("User request:\nquestion")
        }));
    }

    #[test]
    fn claude_invocation_injects_tools_effort_and_live_protocol() {
        let mut config = ErConfig::default();
        crate::config::supplement_ai_hub(&mut config.ai_hub);
        let task = AgentTaskKind::Review;
        let invocation = resolve_invocation(
            &config,
            AgentInvocationRequest {
                selection: AgentSelection::Runtime {
                    provider_id: Some("claude"),
                    model_id: Some("sonnet-5"),
                },
                task: &task,
                effort: Some("high"),
                effort_override: None,
                work_dir: "/repo".into(),
                access: AgentAccessProfile::PreparedArtifacts {
                    output_dir: "/managed/review".into(),
                },
                live_logs: true,
            },
        )
        .unwrap();
        assert_eq!(invocation.family, CliFamily::Claude);
        assert_eq!(invocation.output_protocol, OutputProtocol::ClaudeStreamJson);
        assert!(has_option_value(
            &invocation.args,
            "--allowedTools",
            "Write"
        ));
        assert!(has_option_value(&invocation.args, "--effort", "high"));
        assert!(has_option_value(
            &invocation.args,
            "--output-format",
            "stream-json"
        ));
        assert!(invocation.args.iter().any(|arg| arg == "--verbose"));
    }

    #[test]
    fn exact_selection_uses_requested_arena_model() {
        let config = codex_config();
        let task = AgentTaskKind::ArenaRound;
        let invocation = resolve_invocation(
            &config,
            AgentInvocationRequest {
                selection: AgentSelection::Exact {
                    provider_id: "codex",
                    model_id: "gpt-5.4",
                },
                task: &task,
                effort: None,
                effort_override: None,
                work_dir: "/repo".into(),
                access: AgentAccessProfile::ReadOnly,
                live_logs: false,
            },
        )
        .unwrap();
        assert!(has_option_value(
            &invocation.args,
            "--model",
            "gpt-5.4"
        ));
        assert!(!invocation.args.iter().any(|arg| arg == "--add-dir"));
    }

    #[test]
    fn cli_family_detection_uses_command_basename() {
        assert_eq!(CliFamily::detect("codex"), CliFamily::Codex);
        assert_eq!(
            CliFamily::detect("/opt/homebrew/bin/codex"),
            CliFamily::Codex
        );
        assert_eq!(CliFamily::detect("/custom/provider"), CliFamily::Other);
    }

    #[test]
    fn decode_stream_json_prefers_result() {
        let stdout = concat!(
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"working"}]}}"#,
            "\n",
            r#"{"type":"result","result":"done"}"#,
        );
        assert_eq!(
            decode_final_text(stdout, OutputProtocol::ClaudeStreamJson),
            "done"
        );
    }

    #[test]
    fn review_contract_requires_all_fresh_artifacts() {
        let dir = temp_dir("review-success");
        let diff = "diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1 +1 @@\n-old\n+new\n";
        std::fs::write(dir.join("diff-tmp"), diff).unwrap();
        let baseline =
            ArtifactBaseline::capture(ArtifactContract::Review, dir.to_str().unwrap()).unwrap();
        write_review_contract(&dir, &crate::ai::compute_diff_hash(diff));
        baseline.validate(dir.to_str().unwrap()).unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn review_contract_rejects_missing_unchanged_and_stale_artifacts() {
        let missing_dir = temp_dir("review-missing");
        std::fs::write(missing_dir.join("diff-tmp"), "diff").unwrap();
        let missing =
            ArtifactBaseline::capture(ArtifactContract::Review, missing_dir.to_str().unwrap())
                .unwrap()
                .validate(missing_dir.to_str().unwrap())
                .unwrap_err()
                .to_string();
        assert!(missing.contains("did not write"), "{missing}");

        let unchanged_dir = temp_dir("review-unchanged");
        let diff = "same diff";
        std::fs::write(unchanged_dir.join("diff-tmp"), diff).unwrap();
        write_review_contract(&unchanged_dir, &crate::ai::compute_diff_hash(diff));
        let unchanged =
            ArtifactBaseline::capture(ArtifactContract::Review, unchanged_dir.to_str().unwrap())
                .unwrap()
                .validate(unchanged_dir.to_str().unwrap())
                .unwrap_err()
                .to_string();
        assert!(unchanged.contains("did not update"), "{unchanged}");

        let stale_dir = temp_dir("review-stale");
        std::fs::write(stale_dir.join("diff-tmp"), "new diff").unwrap();
        let stale =
            ArtifactBaseline::capture(ArtifactContract::Review, stale_dir.to_str().unwrap())
                .unwrap();
        write_review_contract(&stale_dir, "old-hash");
        let stale_error = stale
            .validate(stale_dir.to_str().unwrap())
            .unwrap_err()
            .to_string();
        assert!(stale_error.contains("stale review.json"), "{stale_error}");

        let _ = std::fs::remove_dir_all(missing_dir);
        let _ = std::fs::remove_dir_all(unchanged_dir);
        let _ = std::fs::remove_dir_all(stale_dir);
    }

    #[test]
    fn validate_comments_requires_existing_summary_to_change() {
        let dir = temp_dir("validate-comments");
        let diff = "comment diff";
        let hash = crate::ai::compute_diff_hash(diff);
        std::fs::write(dir.join("diff-tmp"), diff).unwrap();
        std::fs::write(dir.join("summary.md"), "before").unwrap();
        std::fs::write(
            dir.join("github-comments.json"),
            format!(r#"{{"version":1,"diff_hash":"{hash}","comments":[]}}"#),
        )
        .unwrap();
        let baseline =
            ArtifactBaseline::capture(ArtifactContract::ValidateComments, dir.to_str().unwrap())
                .unwrap();
        std::fs::write(
            dir.join("github-comments.json"),
            format!(r#"{{"version":1,"diff_hash":"{hash}","comments":[],"github":null}}"#),
        )
        .unwrap();
        let error = baseline
            .validate(dir.to_str().unwrap())
            .unwrap_err()
            .to_string();
        assert!(error.contains("summary.md"), "{error}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn specialized_artifact_contracts_parse_and_match_diff() {
        let diff = "specialized diff";
        let hash = crate::ai::compute_diff_hash(diff);
        let cases = vec![
            (
                "expert",
                ArtifactContract::Expert {
                    id: "security".into(),
                },
                vec![(
                    "experts/security.json",
                    format!(
                        r#"{{"version":1,"expert_id":"security","diff_hash":"{hash}","files":{{}}}}"#
                    ),
                )],
            ),
            (
                "professor",
                ArtifactContract::Professor,
                vec![(
                    "professor.json",
                    format!(r#"{{"version":1,"diff_hash":"{hash}","files":{{}}}}"#),
                )],
            ),
            (
                "triage",
                ArtifactContract::Triage,
                vec![(
                    "triage.json",
                    format!(r#"{{"version":1,"diff_hash":"{hash}"}}"#),
                )],
            ),
            (
                "tour",
                ArtifactContract::Tour {
                    filename: "tour.pr.json".into(),
                },
                vec![(
                    "tour.pr.json",
                    format!(r#"{{"version":1,"diff_hash":"{hash}","pillars":[]}}"#),
                )],
            ),
            (
                "questions",
                ArtifactContract::Questions,
                vec![
                    (
                        "questions.json",
                        format!(r#"{{"version":1,"diff_hash":"{hash}","questions":[]}}"#),
                    ),
                    (
                        "questions.prev.json",
                        format!(r#"{{"version":1,"diff_hash":"{hash}","questions":[]}}"#),
                    ),
                ],
            ),
            (
                "validate-review",
                ArtifactContract::ValidateReview,
                vec![
                    (
                        "review.json",
                        format!(r#"{{"version":1,"diff_hash":"{hash}","files":{{}}}}"#),
                    ),
                    ("summary.md", "validated".into()),
                ],
            ),
        ];

        for (name, contract, files) in cases {
            let dir = temp_dir(name);
            std::fs::write(dir.join("diff-tmp"), diff).unwrap();
            let baseline = ArtifactBaseline::capture(contract, dir.to_str().unwrap()).unwrap();
            for (relative, content) in files {
                let path = dir.join(relative);
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).unwrap();
                }
                std::fs::write(path, content).unwrap();
            }
            baseline.validate(dir.to_str().unwrap()).unwrap();
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}
