#[path = "config_desktop_settings.rs"]
mod config_desktop_settings;

#[path = "config_settings.rs"]
mod config_settings;

use anyhow::Result;

pub use config_desktop_settings::{
    apply_config_field, desktop_settings_snapshot, validate_config_text_field, ConfigFieldValue,
    ConfigHubFieldDto, DesktopSettingsSnapshot,
};
pub use config_settings::{
    agent_effort_label, desktop_settings_fields_flat, desktop_settings_fields_for_scope,
    settings_fields_grouped, SettingsFieldsGrouped, SettingsScope, THEME_OPTIONS,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ErConfig {
    #[serde(default)]
    pub features: FeatureFlags,
    #[serde(default)]
    pub agent: AgentConfig,
    #[serde(default)]
    pub summary: SummaryConfig,
    #[serde(default)]
    pub display: DisplayConfig,
    #[serde(default)]
    pub watched: WatchedConfig,
    #[serde(default)]
    pub hints: HintConfig,
    #[serde(default)]
    pub commands: CommandsConfig,
    #[serde(default)]
    pub ai_hub: AiHubConfig,
    #[serde(default)]
    pub packages: PackagesConfig,
}

/// [commands] section — configurable shell commands for hub actions.
/// Each command is a shell string run via `sh -c`. Placeholders:
/// `{base}` (base branch), `{branch}` (current branch), `{repo}` (repo root),
/// `{output}` (default output path, e.g. managed `{er_dir}/summary.md`).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CommandsConfig {
    /// Generate diff summary (AI hub)
    #[serde(default)]
    pub summary: Option<String>,
    /// Run tests (Verify hub)
    #[serde(default)]
    pub test: Option<String>,
    /// Run linter (Verify hub)
    #[serde(default)]
    pub lint: Option<String>,
    /// Type check (Verify hub)
    #[serde(default)]
    pub typecheck: Option<String>,
    /// Security scan (Verify hub)
    #[serde(default)]
    pub security: Option<String>,
}

/// Per-package command overrides for mono-repo setups.
/// Each key under [packages] is a package ID with its own commands.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PackageConfig {
    pub label: Option<String>,
    pub test: Option<String>,
    pub lint: Option<String>,
    pub typecheck: Option<String>,
    pub security: Option<String>,
}

impl PackageConfig {
    pub fn command_count(&self) -> usize {
        [&self.test, &self.lint, &self.typecheck, &self.security]
            .iter()
            .filter(|c| c.is_some())
            .count()
    }
}

/// [packages] section — per-package command definitions for mono-repos.
/// Each key maps to a PackageConfig with its own test/lint/typecheck/security commands.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PackagesConfig {
    #[serde(flatten)]
    pub items: BTreeMap<String, PackageConfig>,
}

/// [watched] section configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WatchedConfig {
    /// Glob patterns for git-ignored paths to include in the UI
    #[serde(default)]
    pub paths: Vec<String>,
    /// How to diff watched files: "content" or "snapshot"
    #[serde(default = "default_diff_mode")]
    pub diff_mode: String,
}

fn default_diff_mode() -> String {
    "content".to_string()
}

fn default_theme() -> String {
    "graphite".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureFlags {
    #[serde(default = "default_true")]
    pub view_branch: bool,
    #[serde(default = "default_true")]
    pub view_unstaged: bool,
    #[serde(default = "default_true")]
    pub view_staged: bool,
    #[serde(default = "default_true")]
    pub view_history: bool,
    #[serde(default = "default_true")]
    pub view_conflicts: bool,
    #[serde(default = "default_true")]
    pub view_hidden: bool,
    /// Guided Tour walkthrough mode (AI-grouped pillars). Tab appears only when a
    /// `tour.json` exists for the branch.
    #[serde(default = "default_true")]
    pub view_tour: bool,
    /// Multi-round AI Review Arena (orchestrated debate + consensus UI).
    #[serde(default = "default_true")]
    pub arena: bool,
}

/// Claude-compatible effort levels passed as `--effort` when spawning agents.
pub const AGENT_EFFORT_OPTIONS: &[&str] = &["low", "medium", "high"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    #[serde(default = "default_agent_cmd")]
    pub command: String,
    #[serde(default = "default_agent_args")]
    pub args: Vec<String>,
    #[serde(default)]
    pub model: String,
    /// Claude Code `--effort` when ai_hub is empty (legacy fallback).
    #[serde(default)]
    pub effort: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AiHubConfig {
    #[serde(default)]
    pub default_provider: Option<String>,
    #[serde(default)]
    pub default_model: Option<String>,
    /// Default reasoning/effort override for ordinary AI Hub spawns.
    /// Arena reviewer and arbiter selections remain exact per-run values.
    #[serde(default)]
    pub default_effort: Option<String>,
    #[serde(default)]
    pub providers: BTreeMap<String, AiProviderConfig>,
    /// Max agent processes running at once (background reviews + arena
    /// reviewers). Extra requests queue and start as slots free up.
    /// 0 means "use the default".
    #[serde(default)]
    pub max_concurrent_reviews: usize,
}

/// Default cap on concurrently running agent processes.
pub const DEFAULT_MAX_CONCURRENT_REVIEWS: usize = 3;

/// A validated provider/model/effort choice used by ordinary AI actions.
///
/// Arena reviewers intentionally do not use this type: their `ReviewerRef`
/// values are exact per-run selections.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AiSelection {
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub effort: Option<String>,
}

impl AiHubConfig {
    /// Effective concurrency cap — configured value, or the default when unset.
    pub fn effective_max_concurrent_reviews(&self) -> usize {
        if self.max_concurrent_reviews == 0 {
            DEFAULT_MAX_CONCURRENT_REVIEWS
        } else {
            self.max_concurrent_reviews
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AiProviderConfig {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default = "default_agent_cmd")]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub models: Vec<AiModelConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AiModelConfig {
    pub id: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    /// USD per 1k input tokens (arena cost estimate).
    #[serde(default)]
    pub cost_per_1k_in: Option<f32>,
    #[serde(default)]
    pub cost_per_1k_out: Option<f32>,
    #[serde(default)]
    pub avg_latency_ms: Option<u32>,
    /// Explicit reasoning/effort levels supported by this model. An empty
    /// list means the provider default (Auto); support is never inferred from
    /// a model ID.
    #[serde(default)]
    pub effort_levels: Vec<String>,
}

/// [summary] section — configuration for diff summary / changelog generation
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SummaryConfig {
    /// Command to run for summary generation (defaults to agent.command)
    #[serde(default)]
    pub command: Option<String>,
    /// Args for the summary command ({diff} is replaced with the raw diff)
    #[serde(default)]
    pub args: Option<Vec<String>>,
    /// Whether to auto-push the summary to the GitHub PR body
    #[serde(default)]
    pub push_to_pr: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayConfig {
    #[serde(default = "default_tab_width")]
    pub tab_width: u8,
    #[serde(default = "default_true")]
    pub line_numbers: bool,
    #[serde(default)]
    pub wrap_lines: bool,
    #[serde(default)]
    pub split_diff: bool,
    /// Auto-pick unified context per file using a tiered ladder
    /// (small files → more context, big files → less). Set to `0` to disable.
    #[serde(default = "default_auto_context_threshold")]
    pub auto_context_threshold: usize,
    #[serde(default = "default_theme")]
    pub theme: String,
}

/// [hints] section — toggle visibility of key hint groups in the bottom bar
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HintConfig {
    #[serde(default = "default_true")]
    pub navigation: bool,
    #[serde(default = "default_true")]
    pub comments: bool,
    #[serde(default = "default_true")]
    pub staging: bool,
    #[serde(default)]
    pub verbose: bool,
}

impl Default for HintConfig {
    fn default() -> Self {
        Self {
            navigation: true,
            comments: true,
            staging: true,
            verbose: false,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_tab_width() -> u8 {
    4
}

fn default_auto_context_threshold() -> usize {
    100
}

fn default_agent_cmd() -> String {
    "claude".into()
}

fn default_agent_args() -> Vec<String> {
    // The {prompt} placeholder is how the agent command receives user input;
    // config overrides of `agent.args` that omit it silently drop the prompt.
    vec![
        "--print".into(),
        "--output-format".into(),
        "stream-json".into(),
        "-p".into(),
        "{prompt}".into(),
    ]
}

impl Default for FeatureFlags {
    fn default() -> Self {
        Self {
            view_branch: true,
            view_unstaged: true,
            view_staged: true,
            view_history: true,
            view_conflicts: true,
            view_hidden: true,
            view_tour: true,
            arena: true,
        }
    }
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            command: default_agent_cmd(),
            args: default_agent_args(),
            model: String::new(),
            effort: None,
        }
    }
}

impl AgentConfig {
    /// Human-readable name for the agent (derived from command basename).
    pub fn display_name(&self) -> String {
        let basename = self.command.rsplit('/').next().unwrap_or(&self.command);
        // Capitalize first letter
        let mut chars = basename.chars();
        match chars.next() {
            Some(c) => c.to_uppercase().to_string() + chars.as_str(),
            None => "AI".to_string(),
        }
    }
}

impl AiHubConfig {
    pub fn has_presets(&self) -> bool {
        !self.providers.is_empty()
    }

    pub fn provider_ids(&self) -> Vec<String> {
        self.providers.keys().cloned().collect()
    }

    pub fn resolve_provider_id(&self, preferred: Option<&str>) -> Option<String> {
        if self.providers.is_empty() {
            return None;
        }

        preferred
            .and_then(|id| self.providers.contains_key(id).then(|| id.to_string()))
            .or_else(|| {
                self.default_provider
                    .as_deref()
                    .and_then(|id| self.providers.contains_key(id).then(|| id.to_string()))
            })
            .or_else(|| self.providers.keys().next().cloned())
    }

    pub fn resolve_model_id(&self, provider_id: &str, preferred: Option<&str>) -> Option<String> {
        let provider = self.providers.get(provider_id)?;
        if provider.models.is_empty() {
            return None;
        }

        preferred
            .and_then(|id| {
                provider
                    .models
                    .iter()
                    .any(|m| m.id == id)
                    .then(|| id.to_string())
            })
            .or_else(|| {
                self.default_model.as_deref().and_then(|id| {
                    provider
                        .models
                        .iter()
                        .any(|m| m.id == id)
                        .then(|| id.to_string())
                })
            })
            .or_else(|| provider.models.first().map(|m| m.id.clone()))
    }

    /// Resolve the persisted default into a provider/model/effort tuple that
    /// is valid against the current catalog. This is the single fallback
    /// path shared by Settings, Desktop, TUI, and ordinary AI spawns.
    pub fn resolve_default_selection(&self, agent: &AgentConfig) -> AiSelection {
        let provider_id = self.resolve_provider_id(self.default_provider.as_deref());
        let model_id = provider_id
            .as_deref()
            .and_then(|id| self.resolve_model_id(id, self.default_model.as_deref()));
        let effort = resolve_effort_for_model(
            self,
            agent,
            provider_id.as_deref(),
            model_id.as_deref(),
            self.default_effort.as_deref(),
            None,
        );
        AiSelection {
            provider_id,
            model_id,
            effort,
        }
    }

    /// Validate a provider/model choice and normalize effort without mutating
    /// persisted defaults. Used for session-only (palette) selection.
    pub fn resolve_selection(
        &self,
        provider_id: &str,
        model_id: Option<&str>,
        agent: &AgentConfig,
        runtime_effort: Option<&str>,
    ) -> Result<AiSelection> {
        let provider = self
            .providers
            .get(provider_id)
            .ok_or_else(|| anyhow::anyhow!("Unknown AI provider: {provider_id}"))?;
        if let Some(model_id) = model_id {
            if !provider.models.iter().any(|model| model.id == model_id) {
                return Err(anyhow::anyhow!(
                    "Unknown model '{model_id}' for provider '{provider_id}'"
                ));
            }
        }

        let resolved_model = self.resolve_model_id(provider_id, model_id);
        let effort = resolve_effort_for_model(
            self,
            agent,
            Some(provider_id),
            resolved_model.as_deref(),
            runtime_effort,
            None,
        );
        Ok(AiSelection {
            provider_id: Some(provider_id.to_string()),
            model_id: resolved_model,
            effort,
        })
    }

    /// Validate and persist a new global provider/model selection. A missing
    /// model means the provider's first valid model (or no model for a
    /// command-only provider). Existing effort is retained only when the
    /// selected model still supports it.
    pub fn set_default_selection(
        &mut self,
        provider_id: &str,
        model_id: Option<&str>,
        agent: &AgentConfig,
    ) -> Result<AiSelection> {
        let provider = self
            .providers
            .get(provider_id)
            .ok_or_else(|| anyhow::anyhow!("Unknown AI provider: {provider_id}"))?;
        if let Some(model_id) = model_id {
            if !provider.models.iter().any(|model| model.id == model_id) {
                return Err(anyhow::anyhow!(
                    "Unknown model '{model_id}' for provider '{provider_id}'"
                ));
            }
        }

        let resolved_model = self.resolve_model_id(provider_id, model_id);
        self.default_provider = Some(provider_id.to_string());
        self.default_model = resolved_model;
        self.default_effort = normalize_effort(
            self,
            Some(provider_id),
            self.default_model.as_deref(),
            self.default_effort.as_deref(),
        );

        Ok(self.resolve_default_selection(agent))
    }

    /// Whether any configured provider exposes this model id.
    pub fn hub_model_exists(&self, model_id: &str) -> bool {
        self.providers
            .values()
            .any(|provider| provider.models.iter().any(|model| model.id == model_id))
    }
}

#[derive(Deserialize)]
struct AiHubCatalogFile {
    ai_hub: AiHubConfig,
}

/// Built-in `[ai_hub]` presets shipped with `er` (see `ai_hub_catalog.toml`).
fn ai_hub_catalog() -> AiHubConfig {
    toml::from_str::<AiHubCatalogFile>(include_str!("ai_hub_catalog.toml"))
        .map(|f| f.ai_hub)
        .unwrap_or_default()
}

const DEPRECATED_CLAUDE_MODEL_IDS: &[&str] = &["sonnet-4.6", "opus-4.6", "opus-4.7"];

/// Merge missing catalog providers/models into `hub` (in-memory only; does not write config files).
pub fn supplement_ai_hub(hub: &mut AiHubConfig) {
    let catalog = ai_hub_catalog();
    if hub.providers.is_empty() {
        *hub = catalog;
        return;
    }

    let deprecated_default = hub
        .default_model
        .as_deref()
        .is_some_and(|id| DEPRECATED_CLAUDE_MODEL_IDS.contains(&id));
    if let Some(claude) = hub.providers.get_mut("claude") {
        claude
            .models
            .retain(|model| !DEPRECATED_CLAUDE_MODEL_IDS.contains(&model.id.as_str()));
    }
    if deprecated_default {
        hub.default_model = catalog.default_model.clone();
    }

    for (id, catalog_provider) in catalog.providers {
        match hub.providers.get_mut(&id) {
            Some(existing) => {
                let existing_ids: HashSet<String> =
                    existing.models.iter().map(|m| m.id.clone()).collect();
                for model in catalog_provider.models {
                    if !existing_ids.contains(&model.id) {
                        existing.models.push(model);
                    }
                }
            }
            None => {
                hub.providers.insert(id, catalog_provider);
            }
        }
    }

    for provider in hub.providers.values_mut() {
        for model in &mut provider.models {
            if model.id == "grok-4.5"
                && model.args.len() == 2
                && model.args[0] == "--model"
                && model.args[1] == "grok-4.5"
            {
                model.args = vec!["--model".into(), "cursor-grok-4.5-high".into()];
            }
        }
    }
}

impl AiProviderConfig {
    pub fn display_name(&self, provider_id: &str) -> String {
        self.label
            .clone()
            .unwrap_or_else(|| title_case(provider_id))
    }

    /// True when this provider's CLI emits Claude/Cursor-style `stream-json` on stdout.
    pub fn uses_stream_json_log(&self) -> bool {
        agent_command_uses_stream_json(&self.command)
    }
}

/// CLI commands that emit Claude/Cursor-style `stream-json` on stdout.
pub fn agent_command_uses_stream_json(command: &str) -> bool {
    matches!(agent_command_stem(command), "claude" | "agent")
}

/// Basename stem of a provider command (`/bin/opencode.exe` / `C:\bin\opencode.exe` → `opencode`).
///
/// Uses `/` and `\` so Windows-style command strings still resolve on Unix hosts
/// (Path separators alone would treat `C:\bin\opencode.exe` as a single name).
pub fn agent_command_stem(command: &str) -> &str {
    let trimmed = command.trim();
    let base = trimmed.rsplit(['/', '\\']).next().unwrap_or(trimmed);
    base.strip_suffix(".exe")
        .or_else(|| base.strip_suffix(".EXE"))
        .unwrap_or(base)
}

/// True when the provider command is the Claude CLI (supports `--effort`).
pub fn agent_command_is_claude(command: &str) -> bool {
    agent_command_stem(command) == "claude"
}

/// True when the provider command is the Codex CLI (supports `-c model_reasoning_effort=...`).
pub fn agent_command_is_codex(command: &str) -> bool {
    agent_command_stem(command) == "codex"
}

/// True when the provider command is Cursor Agent (supports `--add-dir`).
pub fn agent_command_is_cursor(command: &str) -> bool {
    agent_command_stem(command) == "agent"
}

/// True when the provider command is OpenCode (`opencode run`).
pub fn agent_command_is_opencode(command: &str) -> bool {
    agent_command_stem(command) == "opencode"
}

/// Merge model args onto provider args.
///
/// OpenCode treats the prompt as a trailing positional (`opencode run [message..]`),
/// so model flags must land *before* `{prompt}` or they become part of the message.
pub fn extend_provider_model_args(command: &str, args: &mut Vec<String>, model_args: &[String]) {
    if model_args.is_empty() {
        return;
    }
    if agent_command_is_opencode(command) {
        let insert_at = args
            .iter()
            .position(|arg| arg.contains("{prompt}"))
            .unwrap_or(args.len());
        for (offset, arg) in model_args.iter().enumerate() {
            args.insert(insert_at + offset, arg.clone());
        }
    } else {
        args.extend(model_args.iter().cloned());
    }
}

/// Ensure OpenCode non-interactive runs include `--auto` (auto-approve asks).
pub fn ensure_opencode_auto(args: &mut Vec<String>) {
    if args.iter().any(|arg| arg == "--auto") {
        return;
    }
    let insert_at = args
        .iter()
        .position(|arg| arg == "run")
        .map(|index| index + 1)
        .or_else(|| args.iter().position(|arg| arg.contains("{prompt}")))
        .unwrap_or(0);
    args.insert(insert_at, "--auto".to_string());
}

/// Env var granting OpenCode write access to a managed review bucket outside cwd.
///
/// OpenCode has no `--add-dir`; `external_directory` defaults to ask. With
/// `opencode run --auto`, asks auto-approve unless explicitly **denied** — so we
/// deny all other external paths and only allow the active review bucket.
///
/// `OPENCODE_PERMISSION` is the bare permission object (not a full config with a
/// nested `permission` key). Last matching rule wins, so `"*":"deny"` is listed
/// before the bucket allow.
pub fn opencode_storage_permission_env(storage_dir: Option<&str>) -> Option<(String, String)> {
    let directory = storage_dir.map(str::trim).filter(|dir| !dir.is_empty())?;
    // OpenCode path globs use `/`; normalize Windows `\` so patterns match.
    let normalized = directory
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_string();
    let pattern = format!("{normalized}/**");
    // Keep key insertion order: deny-all first, then the bucket allow.
    let mut external = serde_json::Map::new();
    external.insert("*".into(), serde_json::json!("deny"));
    external.insert(pattern, serde_json::json!("allow"));
    let value = serde_json::Value::Object(serde_json::Map::from_iter([(
        "external_directory".into(),
        serde_json::Value::Object(external),
    )]))
    .to_string();
    Some(("OPENCODE_PERMISSION".to_string(), value))
}

/// Read-only OpenCode permission object for card AI (Ask / Validate).
///
/// Card AI still uses `--auto` so headless runs do not hang on asks, but edit and
/// unrestricted bash/external paths are denied. Grep/git-show style bash matches
/// Claude's `--allowedTools` allowlist for the same feature.
pub fn opencode_readonly_permission_env() -> (String, String) {
    let value = serde_json::json!({
        "edit": "deny",
        "bash": {
            "*": "deny",
            "grep *": "allow",
            "rg *": "allow",
            "git grep*": "allow",
            "git show*": "allow",
            "git log*": "allow",
        },
        "external_directory": {
            "*": "deny",
        },
    })
    .to_string();
    ("OPENCODE_PERMISSION".to_string(), value)
}

/// Ensure `--auto` and return storage permission env for OpenCode artifact spawns.
///
/// Non-OpenCode commands are left unchanged and return `None`. When OpenCode is
/// detected but `storage_dir` is empty, `--auto` is still applied and the return
/// is `None` (callers that need read-only policy should use
/// [`apply_opencode_readonly_spawn`]).
pub fn apply_opencode_spawn(
    command: &str,
    args: &mut Vec<String>,
    storage_dir: Option<&str>,
) -> Option<(String, String)> {
    if !agent_command_is_opencode(command) {
        return None;
    }
    ensure_opencode_auto(args);
    opencode_storage_permission_env(storage_dir)
}

/// Ensure `--auto` and return a read-only `OPENCODE_PERMISSION` for Card AI / Ask.
///
/// Non-OpenCode commands are left unchanged and return `None`.
pub fn apply_opencode_readonly_spawn(
    command: &str,
    args: &mut Vec<String>,
) -> Option<(String, String)> {
    if !agent_command_is_opencode(command) {
        return None;
    }
    ensure_opencode_auto(args);
    Some(opencode_readonly_permission_env())
}

/// Allow built-in agent CLIs to access a specific Easy Review storage directory.
///
/// Managed sidecars normally live outside the repository, so a child CLI with
/// a workspace sandbox cannot reach them through its current working directory.
/// Callers must pass the active review bucket (`er_dir`), never the global
/// storage root — Codex `--add-dir` under `workspace-write` makes that path
/// writable. Pass `None` for read-only invocations or when artifacts already
/// live inside the worktree. Unknown/custom provider commands are left
/// unchanged because their command line contracts are not known to us.
pub fn inject_agent_storage_access(
    command: &str,
    args: &mut Vec<String>,
    storage_dir: Option<&str>,
) {
    if !(agent_command_is_claude(command)
        || agent_command_is_codex(command)
        || agent_command_is_cursor(command))
    {
        return;
    }

    let Some(directory) = storage_dir.map(str::trim).filter(|dir| !dir.is_empty()) else {
        return;
    };

    let insert_at = if agent_command_is_codex(command) {
        args.iter()
            .position(|arg| arg == "exec")
            .map(|index| index + 1)
            .or_else(|| args.iter().position(|arg| arg.contains("{prompt}")))
            .unwrap_or(args.len())
    } else {
        // Insert before the first dashed option so a bare `{prompt}` token is
        // never treated as another `--add-dir` value. Prefer the `=` form for
        // Claude/Cursor so multi-value `--add-dir` cannot swallow the next arg.
        args.iter()
            .position(|arg| arg.starts_with('-'))
            .unwrap_or(0)
    };

    if agent_command_is_codex(command) {
        inject_additional_dir(args, directory, insert_at, false);
    } else {
        inject_additional_dir(args, directory, insert_at, true);
    }
}

fn inject_additional_dir(
    args: &mut Vec<String>,
    directory: &str,
    insert_at: usize,
    equals_form: bool,
) {
    if args
        .windows(2)
        .any(|pair| pair[0] == "--add-dir" && pair[1] == directory)
        || args
            .iter()
            .any(|arg| arg == &format!("--add-dir={directory}"))
    {
        return;
    }

    if equals_form {
        args.insert(insert_at, format!("--add-dir={directory}"));
    } else {
        args.insert(insert_at, directory.to_string());
        args.insert(insert_at, "--add-dir".to_string());
    }
}

/// App-launched Codex runs should be hermetic from user plugins/MCP hooks.
///
/// Codex still reads auth from CODEX_HOME with this flag, but skips
/// `$CODEX_HOME/config.toml`, which prevents unrelated user-global hooks from
/// aborting Easy Review background jobs before they can write sidecars.
pub fn inject_codex_ignore_user_config(args: &mut Vec<String>) {
    if args.iter().any(|arg| arg == "--ignore-user-config") {
        return;
    }

    let insert_at = args
        .iter()
        .position(|arg| arg == "exec")
        .map(|index| index + 1)
        .or_else(|| args.iter().position(|arg| arg.contains("{prompt}")))
        .unwrap_or(args.len());
    args.insert(insert_at, "--ignore-user-config".to_string());
}

pub const EFFORT_LEVELS: &[&str] = &["low", "medium", "high", "xhigh", "max"];
pub const AUTO_EFFORT: &str = "Auto";

/// Return the effort metadata advertised by the selected hub model.
pub fn effort_levels_for_hub_model<'a>(
    hub: &'a AiHubConfig,
    provider_id: Option<&str>,
    model_id: Option<&str>,
) -> &'a [String] {
    provider_id
        .and_then(|provider| hub.providers.get(provider))
        .and_then(|provider| {
            model_id.and_then(|model| provider.models.iter().find(|m| m.id == model))
        })
        .map(|model| model.effort_levels.as_slice())
        .unwrap_or(&[])
}

/// Normalize an effort selection. Auto and invalid/unsupported values are
/// represented as `None`, so command construction cannot leak them to a CLI.
pub fn normalize_effort(
    hub: &AiHubConfig,
    provider_id: Option<&str>,
    model_id: Option<&str>,
    effort: Option<&str>,
) -> Option<String> {
    let value = effort?.trim();
    if value.is_empty() || value.eq_ignore_ascii_case("auto") {
        return None;
    }
    effort_levels_for_hub_model(hub, provider_id, model_id)
        .iter()
        .any(|level| level == value)
        .then(|| value.to_string())
}

/// Precedence: run override → runtime → hub default → agent fallback.
pub fn resolve_effort(
    hub: &AiHubConfig,
    agent: &AgentConfig,
    runtime: Option<&str>,
    run_override: Option<&str>,
) -> Option<String> {
    let pick = |s: Option<&str>| {
        s.filter(|v| !v.trim().is_empty())
            .map(|v| v.trim().to_string())
    };
    pick(run_override)
        .or_else(|| pick(runtime))
        .or_else(|| pick(hub.default_effort.as_deref()))
        .or_else(|| pick(agent.effort.as_deref()))
}

/// Resolve the configured effort and discard values unsupported by the
/// selected model. Invocation code should use this resolver.
pub fn resolve_effort_for_model(
    hub: &AiHubConfig,
    agent: &AgentConfig,
    provider_id: Option<&str>,
    model_id: Option<&str>,
    runtime: Option<&str>,
    run_override: Option<&str>,
) -> Option<String> {
    let candidate = resolve_effort(hub, agent, runtime, run_override);
    normalize_effort(hub, provider_id, model_id, candidate.as_deref())
}

/// Append `--effort <level>` when not already present.
pub fn inject_claude_effort(args: &mut Vec<String>, effort: Option<&str>) {
    let Some(level) = effort.map(str::trim).filter(|s| !s.is_empty()) else {
        return;
    };
    if args.iter().any(|a| a == "--effort") {
        return;
    }
    args.push("--effort".into());
    args.push(level.to_string());
}

/// Append Codex's per-invocation reasoning override when not already present.
pub fn inject_codex_effort(args: &mut Vec<String>, effort: Option<&str>) {
    let Some(level) = effort.map(str::trim).filter(|s| !s.is_empty()) else {
        return;
    };
    if args
        .iter()
        .any(|arg| arg == "model_reasoning_effort" || arg.starts_with("model_reasoning_effort="))
    {
        return;
    }
    args.push("-c".into());
    args.push(format!("model_reasoning_effort={level}"));
}

/// Append OpenCode `--variant <level>` before `{prompt}` when not already present.
pub fn inject_opencode_effort(args: &mut Vec<String>, effort: Option<&str>) {
    let Some(level) = effort.map(str::trim).filter(|s| !s.is_empty()) else {
        return;
    };
    if args.iter().any(|arg| arg == "--variant") {
        return;
    }
    let insert_at = args
        .iter()
        .position(|arg| arg.contains("{prompt}"))
        .unwrap_or(args.len());
    args.insert(insert_at, "--variant".to_string());
    args.insert(insert_at + 1, level.to_string());
}

/// Inject the configured effort using the target provider CLI's argument format.
///
/// Codex only supports this override for models with advertised effort levels.
pub fn inject_provider_effort(
    command: &str,
    args: &mut Vec<String>,
    model_id: Option<&str>,
    effort: Option<&str>,
) {
    if agent_command_is_claude(command) {
        inject_claude_effort(args, effort);
    } else if agent_command_is_codex(command) && model_id.is_some() {
        inject_codex_effort(args, effort);
    } else if agent_command_is_opencode(command) {
        inject_opencode_effort(args, effort);
    }
}

impl AiModelConfig {
    pub fn display_name(&self) -> String {
        self.label.clone().unwrap_or_else(|| self.id.clone())
    }
}

fn title_case(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
        None => "AI".to_string(),
    }
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            tab_width: default_tab_width(),
            line_numbers: true,
            wrap_lines: false,
            split_diff: false,
            auto_context_threshold: default_auto_context_threshold(),
            theme: default_theme(),
        }
    }
}

impl ErConfig {
    /// Resolve a command by name from the [commands] config section.
    /// Returns None if the command is not configured — callers should
    /// disable the action and show a "not configured" hint.
    pub fn resolve_command(&self, name: &str) -> Option<String> {
        match name {
            "summary" => self.commands.summary.clone(),
            "test" => self.commands.test.clone(),
            "lint" => self.commands.lint.clone(),
            "typecheck" => self.commands.typecheck.clone(),
            "security" => self.commands.security.clone(),
            _ => None,
        }
    }

    pub fn has_packages(&self) -> bool {
        !self.packages.items.is_empty()
    }

    /// Resolve a command for a specific package. Returns None if package or command not configured.
    pub fn resolve_package_command(&self, package_id: &str, command: &str) -> Option<String> {
        let pkg = self.packages.items.get(package_id)?;
        match command {
            "test" => pkg.test.clone(),
            "lint" => pkg.lint.clone(),
            "typecheck" => pkg.typecheck.clone(),
            "security" => pkg.security.clone(),
            _ => None,
        }
    }
}

/// Split a string into args respecting single and double quotes.
/// Unquoted segments are split on whitespace. Quoted segments preserve
/// inner whitespace and strip the outer quotes.
///
/// Examples:
///   `--print -p {prompt}`         → ["--print", "-p", "{prompt}"]
///   `--print -p "hello world"`    → ["--print", "-p", "hello world"]
///   `--flag 'it'\''s quoted'`     → ["--flag", "it's quoted"]
pub fn split_shell_args(s: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut chars = s.chars().peekable();
    let mut in_single = false;
    let mut in_double = false;

    while let Some(c) = chars.next() {
        match c {
            '\'' if !in_double => {
                in_single = !in_single;
            }
            '"' if !in_single => {
                in_double = !in_double;
            }
            '\\' if in_double || !in_single => {
                // Backslash escapes the next character
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            c if c.is_ascii_whitespace() && !in_single && !in_double => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            _ => {
                current.push(c);
            }
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

/// Global config directory (`$XDG_CONFIG_HOME/er`, else `~/.config/er`, else
/// the platform config dir). Shared by load/save and uninstall planning.
pub fn global_config_dir() -> Option<std::path::PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return Some(std::path::PathBuf::from(xdg).join("er"));
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            return Some(std::path::PathBuf::from(home).join(".config").join("er"));
        }
    }
    dirs::config_dir().map(|d| d.join("er"))
}

/// Load the global config (~/.config/er/config.toml).
pub fn load_global_config() -> ErConfig {
    let global_path = global_config_dir()
        .map(|d| d.join("config.toml"))
        .filter(|p| p.exists())
        .or_else(|| {
            dirs::config_dir()
                .map(|d| d.join("er").join("config.toml"))
                .filter(|p| p.exists())
        });

    let global_table = global_path
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|c| c.parse::<toml::Table>().ok());

    let mut config: ErConfig = match global_table {
        Some(table) => toml::Value::Table(table).try_into().unwrap_or_default(),
        None => ErConfig::default(),
    };
    supplement_ai_hub(&mut config.ai_hub);
    config
}

/// Save config to the global config dir (~/.config/er/config.toml).
pub fn save_config(config: &ErConfig) -> Result<()> {
    let dir = global_config_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?;
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("config.toml");
    let tmp_path = dir.join("config.toml.tmp");
    let content = toml::to_string_pretty(config)?;
    std::fs::write(&tmp_path, content)?;
    std::fs::rename(&tmp_path, &path)?;
    Ok(())
}

/// Config hub item types for the enhanced config hub UI.
#[derive(Clone)]
pub enum ConfigItem {
    SectionHeader(String),
    BoolToggle {
        label: String,
        description: String,
        get: fn(&ErConfig) -> bool,
        set: fn(&mut ErConfig, bool),
    },
    StringCycle {
        label: String,
        description: String,
        options: &'static [&'static str],
        get: fn(&ErConfig) -> String,
        set: fn(&mut ErConfig, String),
    },
    DynamicStringCycle {
        label: String,
        description: String,
        options: fn(&ErConfig) -> Vec<String>,
        get: fn(&ErConfig) -> String,
        set: fn(&mut ErConfig, String),
    },
    StringEdit {
        label: String,
        description: String,
        placeholder: String,
        get: fn(&ErConfig) -> String,
        set: fn(&mut ErConfig, String),
    },
    NumberEdit {
        label: String,
        description: String,
        min: u8,
        max: u8,
        get: fn(&ErConfig) -> u8,
        set: fn(&mut ErConfig, u8),
    },
    ListEntry {
        label: String,
        index: usize,
    },
    ListAdd {
        label: String,
        section: String,
    },
    Action {
        label: String,
        description: String,
        action_id: &'static str,
    },
}

impl std::fmt::Debug for ConfigItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigItem::SectionHeader(s) => write!(f, "SectionHeader({:?})", s),
            ConfigItem::BoolToggle { label, .. } => write!(f, "BoolToggle({:?})", label),
            ConfigItem::StringCycle { label, .. } => write!(f, "StringCycle({:?})", label),
            ConfigItem::DynamicStringCycle { label, .. } => {
                write!(f, "DynamicStringCycle({:?})", label)
            }
            ConfigItem::StringEdit { label, .. } => write!(f, "StringEdit({:?})", label),
            ConfigItem::NumberEdit { label, .. } => write!(f, "NumberEdit({:?})", label),
            ConfigItem::ListEntry { label, index } => {
                write!(f, "ListEntry({:?}, {})", label, index)
            }
            ConfigItem::ListAdd { label, section } => {
                write!(f, "ListAdd({:?}, {:?})", label, section)
            }
            ConfigItem::Action { label, .. } => write!(f, "Action({:?})", label),
        }
    }
}

/// Build config hub items for one settings tab.
pub fn config_hub_items_for_scope(config: &ErConfig, scope: SettingsScope) -> Vec<ConfigItem> {
    match scope {
        SettingsScope::General => general_config_hub_items(config),
        SettingsScope::App => Vec::new(),
        SettingsScope::Terminal => terminal_config_hub_items(config),
    }
}

/// Flat list (all tabs) for tests.
pub fn config_hub_items(config: &ErConfig) -> Vec<ConfigItem> {
    let mut items = config_hub_items_for_scope(config, SettingsScope::General);
    items.extend(config_hub_items_for_scope(config, SettingsScope::Terminal));
    items
}

fn general_config_hub_items(config: &ErConfig) -> Vec<ConfigItem> {
    let mut items: Vec<ConfigItem> = vec![
        ConfigItem::SectionHeader("Commands".into()),
        ConfigItem::StringEdit {
            label: "Summary".into(),
            description: "Generate diff summary".into(),
            placeholder: "e.g. claude -p 'Summarize: {diff}'".into(),
            get: |c| c.commands.summary.clone().unwrap_or_default(),
            set: |c, v| c.commands.summary = if v.is_empty() { None } else { Some(v) },
        },
        ConfigItem::StringEdit {
            label: "Test".into(),
            description: "Run tests".into(),
            placeholder: "e.g. cargo test".into(),
            get: |c| c.commands.test.clone().unwrap_or_default(),
            set: |c, v| c.commands.test = if v.is_empty() { None } else { Some(v) },
        },
        ConfigItem::StringEdit {
            label: "Lint".into(),
            description: "Run linter".into(),
            placeholder: "e.g. cargo clippy".into(),
            get: |c| c.commands.lint.clone().unwrap_or_default(),
            set: |c, v| c.commands.lint = if v.is_empty() { None } else { Some(v) },
        },
        ConfigItem::StringEdit {
            label: "Typecheck".into(),
            description: "Run type checker".into(),
            placeholder: "e.g. tsc --noEmit".into(),
            get: |c| c.commands.typecheck.clone().unwrap_or_default(),
            set: |c, v| c.commands.typecheck = if v.is_empty() { None } else { Some(v) },
        },
        ConfigItem::StringEdit {
            label: "Security".into(),
            description: "Run security scan".into(),
            placeholder: "e.g. cargo audit".into(),
            get: |c| c.commands.security.clone().unwrap_or_default(),
            set: |c, v| c.commands.security = if v.is_empty() { None } else { Some(v) },
        },
        ConfigItem::BoolToggle {
            label: "Push summary to PR body".into(),
            description: "Auto-push summary to GitHub PR".into(),
            get: |c| c.summary.push_to_pr,
            set: |c, v| c.summary.push_to_pr = v,
        },
        ConfigItem::SectionHeader("Agent".into()),
        ConfigItem::StringEdit {
            label: "Command".into(),
            description: "Agent executable".into(),
            placeholder: "e.g. claude".into(),
            get: |c| c.agent.command.clone(),
            set: |c, v| {
                if !v.is_empty() {
                    c.agent.command = v
                }
            },
        },
        ConfigItem::StringEdit {
            label: "Args".into(),
            description: "Agent arguments (space-separated)".into(),
            placeholder: "e.g. --print -p {prompt}".into(),
            get: |c| c.agent.args.join(" "),
            set: |c, v| c.agent.args = split_shell_args(&v),
        },
        ConfigItem::StringCycle {
            label: "Effort".into(),
            description: "Claude effort level (--effort)".into(),
            options: AGENT_EFFORT_OPTIONS,
            get: |c| {
                c.agent
                    .effort
                    .clone()
                    .unwrap_or_else(|| "medium".to_string())
            },
            set: |c, v| {
                c.agent.effort = if v.is_empty() { None } else { Some(v) };
            },
        },
        ConfigItem::SectionHeader("AI Hub".into()),
        ConfigItem::DynamicStringCycle {
            label: "Provider".into(),
            description: "Global AI Hub provider".into(),
            options: hub_provider_options,
            get: hub_provider_value,
            set: set_hub_provider,
        },
        ConfigItem::DynamicStringCycle {
            label: "Model".into(),
            description: "Model for the selected provider".into(),
            options: hub_model_options,
            get: hub_model_value,
            set: set_hub_model,
        },
        ConfigItem::DynamicStringCycle {
            label: "Effort / reasoning".into(),
            description: "Auto uses the provider default".into(),
            options: hub_effort_options,
            get: hub_effort_value,
            set: set_hub_effort,
        },
        ConfigItem::SectionHeader("Watched Paths".into()),
        ConfigItem::StringCycle {
            label: "Diff mode".into(),
            description: "How to diff watched files".into(),
            options: &["content", "snapshot"],
            get: |c| c.watched.diff_mode.clone(),
            set: |c, v| c.watched.diff_mode = v,
        },
    ];

    for (i, path) in config.watched.paths.iter().enumerate() {
        items.push(ConfigItem::ListEntry {
            label: path.clone(),
            index: i,
        });
    }

    items.push(ConfigItem::ListAdd {
        label: "Add pattern...".into(),
        section: "watched".into(),
    });

    items
}

fn hub_provider_options(config: &ErConfig) -> Vec<String> {
    config.ai_hub.provider_ids()
}

fn hub_provider_value(config: &ErConfig) -> String {
    config
        .ai_hub
        .resolve_default_selection(&config.agent)
        .provider_id
        .unwrap_or_else(|| "Auto".into())
}

fn hub_model_options(config: &ErConfig) -> Vec<String> {
    let selection = config.ai_hub.resolve_default_selection(&config.agent);
    let Some(provider_id) = selection.provider_id else {
        return vec!["Auto".into()];
    };
    let Some(provider) = config.ai_hub.providers.get(&provider_id) else {
        return vec!["Auto".into()];
    };
    provider
        .models
        .iter()
        .map(|model| model.id.clone())
        .collect()
}

fn hub_model_value(config: &ErConfig) -> String {
    config
        .ai_hub
        .resolve_default_selection(&config.agent)
        .model_id
        .unwrap_or_else(|| "Auto".into())
}

fn hub_effort_options(config: &ErConfig) -> Vec<String> {
    let selection = config.ai_hub.resolve_default_selection(&config.agent);
    let mut options = vec![AUTO_EFFORT.into()];
    options.extend(
        effort_levels_for_hub_model(
            &config.ai_hub,
            selection.provider_id.as_deref(),
            selection.model_id.as_deref(),
        )
        .iter()
        .cloned(),
    );
    options
}

fn hub_effort_value(config: &ErConfig) -> String {
    config
        .ai_hub
        .resolve_default_selection(&config.agent)
        .effort
        .unwrap_or_else(|| AUTO_EFFORT.into())
}

fn set_hub_provider(config: &mut ErConfig, provider: String) {
    let agent = config.agent.clone();
    let _ = config.ai_hub.set_default_selection(&provider, None, &agent);
}

fn set_hub_model(config: &mut ErConfig, model: String) {
    let Some(provider) = config
        .ai_hub
        .resolve_default_selection(&config.agent)
        .provider_id
    else {
        return;
    };
    let agent = config.agent.clone();
    let _ = config
        .ai_hub
        .set_default_selection(&provider, Some(&model), &agent);
}

fn set_hub_effort(config: &mut ErConfig, effort: String) {
    let selection = config.ai_hub.resolve_default_selection(&config.agent);
    config.ai_hub.default_effort = normalize_effort(
        &config.ai_hub,
        selection.provider_id.as_deref(),
        selection.model_id.as_deref(),
        Some(&effort),
    );
}

fn terminal_config_hub_items(_config: &ErConfig) -> Vec<ConfigItem> {
    vec![
        ConfigItem::SectionHeader("Views".into()),
        ConfigItem::BoolToggle {
            label: "Branch diff (1)".into(),
            description: "Show branch diff mode".into(),
            get: |c| c.features.view_branch,
            set: |c, v| c.features.view_branch = v,
        },
        ConfigItem::BoolToggle {
            label: "Unstaged changes (2)".into(),
            description: "Show unstaged changes mode".into(),
            get: |c| c.features.view_unstaged,
            set: |c, v| c.features.view_unstaged = v,
        },
        ConfigItem::BoolToggle {
            label: "Staged changes (3)".into(),
            description: "Show staged changes mode".into(),
            get: |c| c.features.view_staged,
            set: |c, v| c.features.view_staged = v,
        },
        ConfigItem::BoolToggle {
            label: "History (4)".into(),
            description: "Show commit history mode".into(),
            get: |c| c.features.view_history,
            set: |c, v| c.features.view_history = v,
        },
        ConfigItem::BoolToggle {
            label: "Conflicts (5)".into(),
            description: "Show merge conflicts mode".into(),
            get: |c| c.features.view_conflicts,
            set: |c, v| c.features.view_conflicts = v,
        },
        ConfigItem::BoolToggle {
            label: "Hidden files (6)".into(),
            description: "Show hidden files mode".into(),
            get: |c| c.features.view_hidden,
            set: |c, v| c.features.view_hidden = v,
        },
        ConfigItem::BoolToggle {
            label: "Tour".into(),
            description: "Show AI guided tour mode (when a tour exists)".into(),
            get: |c| c.features.view_tour,
            set: |c, v| c.features.view_tour = v,
        },
        ConfigItem::SectionHeader("Display".into()),
        ConfigItem::StringCycle {
            label: "Theme".into(),
            description: "Color theme".into(),
            options: THEME_OPTIONS,
            get: |c| c.display.theme.clone(),
            set: |c, v| c.display.theme = v,
        },
        ConfigItem::BoolToggle {
            label: "Line numbers".into(),
            description: "Show line numbers in diff".into(),
            get: |c| c.display.line_numbers,
            set: |c, v| c.display.line_numbers = v,
        },
        ConfigItem::BoolToggle {
            label: "Wrap lines".into(),
            description: "Wrap long lines".into(),
            get: |c| c.display.wrap_lines,
            set: |c, v| c.display.wrap_lines = v,
        },
        ConfigItem::BoolToggle {
            label: "Split diff".into(),
            description: "Side-by-side diff view".into(),
            get: |c| c.display.split_diff,
            set: |c, v| c.display.split_diff = v,
        },
        ConfigItem::BoolToggle {
            label: "Auto-expand context".into(),
            description: "Pick unified context per file (small → more, big → less)".into(),
            get: |c| c.display.auto_context_threshold > 0,
            set: |c, v| c.display.auto_context_threshold = if v { 1 } else { 0 },
        },
        ConfigItem::NumberEdit {
            label: "Tab width".into(),
            description: "Spaces per tab stop".into(),
            min: 1,
            max: 16,
            get: |c| c.display.tab_width,
            set: |c, v| c.display.tab_width = v,
        },
        ConfigItem::SectionHeader("Key Hints".into()),
        ConfigItem::BoolToggle {
            label: "Navigation hints".into(),
            description: "Show j/k, n/N, ␣, / hints".into(),
            get: |c| c.hints.navigation,
            set: |c, v| c.hints.navigation = v,
        },
        ConfigItem::BoolToggle {
            label: "Staging hints".into(),
            description: "Show s, c commit hints".into(),
            get: |c| c.hints.staging,
            set: |c, v| c.hints.staging = v,
        },
        ConfigItem::BoolToggle {
            label: "Comment hints".into(),
            description: "Show r, d comment action hints".into(),
            get: |c| c.hints.comments,
            set: |c, v| c.hints.comments = v,
        },
        ConfigItem::BoolToggle {
            label: "Verbose hints".into(),
            description: "Show all key hints (resize, filters, etc)".into(),
            get: |c| c.hints.verbose,
            set: |c, v| c.hints.verbose = v,
        },
        ConfigItem::SectionHeader("AI".into()),
        ConfigItem::Action {
            label: "Copy review.json".into(),
            description: "Copy review.json to clipboard".into(),
            action_id: "copy_review_json",
        },
        ConfigItem::Action {
            label: "Copy questions.json".into(),
            description: "Copy questions.json to clipboard".into(),
            action_id: "copy_questions_json",
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── ai_hub supplementation ──

    #[test]
    fn supplement_ai_hub_populates_empty_hub_from_catalog() {
        let mut hub = AiHubConfig::default();
        assert!(hub.providers.is_empty());
        supplement_ai_hub(&mut hub);
        assert!(!hub.providers.is_empty());
    }

    #[test]
    fn supplement_ai_hub_adds_catalog_models_to_user_provider() {
        let mut hub = AiHubConfig::default();
        hub.providers.insert(
            "codex".into(),
            AiProviderConfig {
                command: "codex".into(),
                models: vec![AiModelConfig {
                    id: "gpt-5.4".into(),
                    ..Default::default()
                }],
                ..Default::default()
            },
        );

        supplement_ai_hub(&mut hub);

        let codex = hub.providers.get("codex").unwrap();
        // The user's model is preserved and stays first.
        assert_eq!(codex.models[0].id, "gpt-5.4");
        // The catalog supplements the remaining codex models in-memory.
        assert!(!codex.models.iter().any(|m| m.id == "gpt-5.3-codex"));
        assert!(codex.models.iter().any(|m| m.id == "gpt-5.6-sol"));
        assert!(codex.models.iter().any(|m| m.id == "gpt-5.6-terra"));
        assert!(codex.models.iter().any(|m| m.id == "gpt-5.6-luna"));
        assert!(codex.models.iter().any(|m| m.id == "gpt-5.4-mini"));
        assert!(codex.models.iter().any(|m| m.id == "gpt-5.3-codex-spark"));
    }

    // ── Default values ──

    #[test]
    fn feature_flags_all_default_to_true() {
        let flags = FeatureFlags::default();
        assert!(flags.view_branch);
        assert!(flags.view_unstaged);
        assert!(flags.view_staged);
        assert!(flags.view_history);
        assert!(flags.view_conflicts);
        assert!(flags.view_hidden);
        assert!(flags.view_tour);
    }

    #[test]
    fn tab_width_defaults_to_4() {
        let display = DisplayConfig::default();
        assert_eq!(display.tab_width, 4);
    }

    #[test]
    fn agent_command_defaults_to_claude() {
        let agent = AgentConfig::default();
        assert_eq!(agent.command, "claude");
    }

    #[test]
    fn ai_hub_resolve_provider_and_model_defaults() {
        let mut config = ErConfig::default();
        config.ai_hub.default_provider = Some("codex".into());
        config.ai_hub.default_model = Some("gpt-5.4".into());
        config.ai_hub.providers.insert(
            "codex".into(),
            AiProviderConfig {
                label: Some("Codex".into()),
                command: "codex".into(),
                args: vec!["exec".into(), "{prompt}".into()],
                models: vec![
                    AiModelConfig {
                        id: "gpt-5.4".into(),
                        label: Some("GPT-5.4".into()),
                        description: None,
                        args: vec!["--model".into(), "gpt-5.4".into()],
                        cost_per_1k_in: None,
                        cost_per_1k_out: None,
                        avg_latency_ms: None,
                        effort_levels: vec![],
                    },
                    AiModelConfig {
                        id: "gpt-5.3-codex".into(),
                        label: None,
                        description: None,
                        args: vec!["--model".into(), "gpt-5.3-codex".into()],
                        cost_per_1k_in: None,
                        cost_per_1k_out: None,
                        avg_latency_ms: None,
                        effort_levels: vec![],
                    },
                ],
            },
        );

        let provider_id = config.ai_hub.resolve_provider_id(None).unwrap();
        let model_id = config.ai_hub.resolve_model_id(&provider_id, None).unwrap();
        assert_eq!(provider_id, "codex");
        assert_eq!(model_id, "gpt-5.4");
    }

    #[test]
    fn serde_roundtrip_preserves_all_fields() {
        let config = ErConfig {
            features: FeatureFlags {
                view_branch: false,
                view_unstaged: true,
                view_staged: false,
                view_history: true,
                view_conflicts: false,
                view_hidden: true,
                view_tour: true,
                arena: false,
            },
            display: DisplayConfig {
                tab_width: 8,
                line_numbers: false,
                wrap_lines: true,
                split_diff: true,
                auto_context_threshold: 100,
                theme: "slate".into(),
            },
            agent: AgentConfig {
                command: "my-agent".into(),
                args: vec!["--flag".into()],
                model: String::new(),
                effort: None,
            },
            ..Default::default()
        };
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let restored: ErConfig = toml::from_str(&toml_str).unwrap();

        assert!(!restored.features.view_branch);
        assert!(restored.features.view_history);
        assert_eq!(restored.display.tab_width, 8);
        assert!(restored.display.wrap_lines);
        assert_eq!(restored.agent.command, "my-agent");
        assert_eq!(restored.agent.args, vec!["--flag"]);
    }

    // ── split_shell_args ──

    #[test]
    fn split_shell_args_simple() {
        assert_eq!(
            split_shell_args("--print -p {prompt}"),
            vec!["--print", "-p", "{prompt}"]
        );
    }

    #[test]
    fn split_shell_args_double_quotes() {
        assert_eq!(
            split_shell_args(r#"--print -p "hello world""#),
            vec!["--print", "-p", "hello world"]
        );
    }

    #[test]
    fn split_shell_args_single_quotes() {
        assert_eq!(
            split_shell_args("--print -p 'hello world'"),
            vec!["--print", "-p", "hello world"]
        );
    }

    #[test]
    fn split_shell_args_escaped_quote_in_double() {
        assert_eq!(
            split_shell_args(r#"--flag "it\"s here""#),
            vec!["--flag", r#"it"s here"#]
        );
    }

    #[test]
    fn split_shell_args_backslash_outside_quotes() {
        assert_eq!(
            split_shell_args(r"--flag hello\ world"),
            vec!["--flag", "hello world"]
        );
    }

    #[test]
    fn split_shell_args_empty_string() {
        assert!(split_shell_args("").is_empty());
    }

    #[test]
    fn split_shell_args_only_whitespace() {
        assert!(split_shell_args("   ").is_empty());
    }

    #[test]
    fn split_shell_args_mixed_quotes() {
        assert_eq!(
            split_shell_args(r#"--a 'single' --b "double" --c plain"#),
            vec!["--a", "single", "--b", "double", "--c", "plain"]
        );
    }

    #[test]
    fn split_shell_args_adjacent_quoted_segments() {
        // 'hello'" "'world' should join into "hello world"
        assert_eq!(split_shell_args("'hello'\" \"'world'"), vec!["hello world"]);
    }

    #[test]
    fn split_shell_args_prompt_placeholder() {
        // The default agent args pattern
        assert_eq!(
            split_shell_args("--print --output-format stream-json -p {prompt}"),
            vec![
                "--print",
                "--output-format",
                "stream-json",
                "-p",
                "{prompt}"
            ]
        );
    }

    // ── config_hub_items ──

    #[test]
    fn config_hub_items_returns_expected_sections_and_count() {
        let config = ErConfig::default();
        let items = config_hub_items(&config);
        // Count non-header items
        let toggleable: Vec<_> = items
            .iter()
            .filter(|i| !matches!(i, ConfigItem::SectionHeader(_)))
            .collect();
        assert!(
            toggleable.len() >= 18,
            "Expected at least 18 interactive items, got {}",
            toggleable.len()
        );
        assert!(
            items.len() >= 22,
            "Expected at least 22 total items (with headers), got {}",
            items.len()
        );
    }

    #[test]
    fn config_hub_dynamic_ai_cycles_follow_catalog_metadata_and_auto() {
        let mut config = ErConfig::default();
        supplement_ai_hub(&mut config.ai_hub);
        let items = config_hub_items_for_scope(&config, SettingsScope::General);
        let (provider_options, provider_get, provider_set) = items
            .iter()
            .find_map(|item| match item {
                ConfigItem::DynamicStringCycle {
                    label,
                    options,
                    get,
                    set,
                    ..
                } if label == "Provider" => Some((*options, *get, *set)),
                _ => None,
            })
            .expect("provider cycle");
        assert!(provider_options(&config).contains(&"codex".into()));
        provider_set(&mut config, "codex".into());
        assert_eq!(provider_get(&config), "codex");

        let (model_options, _, model_set) = items
            .iter()
            .find_map(|item| match item {
                ConfigItem::DynamicStringCycle {
                    label,
                    options,
                    get,
                    set,
                    ..
                } if label == "Model" => Some((*options, *get, *set)),
                _ => None,
            })
            .expect("model cycle");
        assert!(model_options(&config).contains(&"gpt-5.4-mini".into()));
        model_set(&mut config, "gpt-5.4-mini".into());

        let (effort_options, effort_get, effort_set) = items
            .iter()
            .find_map(|item| match item {
                ConfigItem::DynamicStringCycle {
                    label,
                    options,
                    get,
                    set,
                    ..
                } if label == "Effort / reasoning" => Some((*options, *get, *set)),
                _ => None,
            })
            .expect("effort cycle");
        assert_eq!(effort_options(&config)[0], AUTO_EFFORT);
        assert!(effort_options(&config).contains(&"xhigh".into()));
        effort_set(&mut config, "xhigh".into());
        assert_eq!(effort_get(&config), "xhigh");
        effort_set(&mut config, AUTO_EFFORT.into());
        assert_eq!(effort_get(&config), AUTO_EFFORT);
    }

    #[test]
    fn config_hub_items_section_headers_present_in_correct_order() {
        let config = ErConfig::default();
        let items = config_hub_items(&config);
        let headers: Vec<&str> = items
            .iter()
            .filter_map(|i| match i {
                ConfigItem::SectionHeader(title) => Some(title.as_str()),
                _ => None,
            })
            .collect();
        assert!(headers.contains(&"Views"));
        assert!(headers.contains(&"Display"));
        assert!(headers.contains(&"Key Hints"));
        assert!(headers.contains(&"Commands"));
        assert!(headers.contains(&"Agent"));
        assert!(headers.contains(&"Watched Paths"));
        // Views should come before Display
        let views_pos = headers.iter().position(|h| *h == "Views").unwrap();
        let display_pos = headers.iter().position(|h| *h == "Display").unwrap();
        assert!(views_pos < display_pos);
    }

    #[test]
    fn config_hub_items_bool_toggle_get_set_round_trip() {
        let mut config = ErConfig::default();
        let items = config_hub_items_for_scope(&config, SettingsScope::Terminal);

        // Find the "Branch diff" toggle
        let branch_toggle = items.iter().find(|i| match i {
            ConfigItem::BoolToggle { label, .. } => label.contains("Branch"),
            _ => false,
        });
        if let Some(ConfigItem::BoolToggle { get, set, .. }) = branch_toggle {
            assert!(get(&config));
            set(&mut config, false);
            assert!(!config.features.view_branch);
            assert!(!get(&config));
        } else {
            panic!("Branch diff toggle not found");
        }
    }

    #[test]
    fn config_hub_items_per_scope_partition() {
        let config = ErConfig::default();
        let general = config_hub_items_for_scope(&config, SettingsScope::General);
        let app = config_hub_items_for_scope(&config, SettingsScope::App);
        let terminal = config_hub_items_for_scope(&config, SettingsScope::Terminal);

        assert!(!general.iter().any(|i| matches!(
            i,
            ConfigItem::BoolToggle { label, .. } if label.contains("Branch")
        )));
        assert!(terminal.iter().any(|i| matches!(
            i,
            ConfigItem::BoolToggle { label, .. } if label.contains("Branch")
        )));
        assert!(!terminal.is_empty());
        assert!(app.is_empty());
        assert!(terminal.iter().any(|i| matches!(
            i,
            ConfigItem::StringCycle { label, .. } if label == "Theme"
        )));
    }

    #[test]
    fn config_hub_items_string_cycle_get_set_round_trip() {
        let mut config = ErConfig::default();
        let items = config_hub_items_for_scope(&config, SettingsScope::Terminal);

        let theme_cycle = items.iter().find(|i| match i {
            ConfigItem::StringCycle { label, .. } => label == "Theme",
            _ => false,
        });
        if let Some(ConfigItem::StringCycle {
            get, set, options, ..
        }) = theme_cycle
        {
            assert_eq!(get(&config), "graphite");
            set(&mut config, options[1].to_string());
            assert_eq!(get(&config), options[1]);
        } else {
            panic!("Theme cycle not found");
        }
    }

    #[test]
    fn config_hub_items_string_edit_get_set_round_trip() {
        let mut config = ErConfig::default();
        let items = config_hub_items_for_scope(&config, SettingsScope::General);

        let cmd_edit = items.iter().find(|i| match i {
            ConfigItem::StringEdit { label, .. } => label == "Command",
            _ => false,
        });
        if let Some(ConfigItem::StringEdit { get, set, .. }) = cmd_edit {
            assert_eq!(get(&config), "claude");
            set(&mut config, "my-agent".to_string());
            assert_eq!(config.agent.command, "my-agent");
        } else {
            panic!("Agent command edit not found");
        }
    }

    #[test]
    fn config_hub_items_number_edit_get_set_round_trip() {
        let mut config = ErConfig::default();
        let items = config_hub_items_for_scope(&config, SettingsScope::Terminal);

        let tab_width = items.iter().find(|i| match i {
            ConfigItem::NumberEdit { label, .. } => label == "Tab width",
            _ => false,
        });
        if let Some(ConfigItem::NumberEdit {
            get, set, min, max, ..
        }) = tab_width
        {
            assert_eq!(get(&config), 4);
            assert_eq!(*min, 1);
            assert_eq!(*max, 16);
            set(&mut config, 8);
            assert_eq!(config.display.tab_width, 8);
        } else {
            panic!("Tab width number edit not found");
        }
    }

    #[test]
    fn config_hub_items_watched_paths_generate_list_entries() {
        let mut config = ErConfig::default();
        config.watched.paths = vec![".work/**".to_string(), "logs/*.log".to_string()];
        let items = config_hub_items_for_scope(&config, SettingsScope::General);

        let list_entries: Vec<_> = items
            .iter()
            .filter(|i| matches!(i, ConfigItem::ListEntry { .. }))
            .collect();
        assert_eq!(list_entries.len(), 2);
        if let ConfigItem::ListEntry { label, index } = &list_entries[0] {
            assert_eq!(label, ".work/**");
            assert_eq!(*index, 0);
        }
        if let ConfigItem::ListEntry { label, index } = &list_entries[1] {
            assert_eq!(label, "logs/*.log");
            assert_eq!(*index, 1);
        }
    }

    #[test]
    fn config_hub_items_includes_list_add_for_watched() {
        let config = ErConfig::default();
        let items = config_hub_items_for_scope(&config, SettingsScope::General);
        let has_add = items
            .iter()
            .any(|i| matches!(i, ConfigItem::ListAdd { .. }));
        assert!(has_add, "Should include a ListAdd item for watched paths");
    }

    // ── AgentConfig::display_name ──

    #[test]
    fn agent_display_name_capitalizes_basename() {
        let agent = AgentConfig {
            command: "claude".into(),
            args: vec![],
            model: String::new(),
            effort: None,
        };
        assert_eq!(agent.display_name(), "Claude");
    }

    #[test]
    fn agent_display_name_extracts_basename_from_path() {
        let agent = AgentConfig {
            command: "/usr/local/bin/claude".into(),
            args: vec![],
            model: String::new(),
            effort: None,
        };
        assert_eq!(agent.display_name(), "Claude");
    }

    #[test]
    fn agent_display_name_empty_command_returns_ai() {
        let agent = AgentConfig {
            command: "".into(),
            args: vec![],
            model: String::new(),
            effort: None,
        };
        assert_eq!(agent.display_name(), "AI");
    }

    #[test]
    fn hub_model_effort_levels() {
        let mut hub = AiHubConfig::default();
        hub.providers.insert(
            "claude".into(),
            AiProviderConfig {
                models: vec![
                    AiModelConfig {
                        id: "sonnet-5".into(),
                        effort_levels: EFFORT_LEVELS.iter().map(|s| s.to_string()).collect(),
                        ..Default::default()
                    },
                    AiModelConfig {
                        id: "haiku-4.5".into(),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
        );
        assert!(
            effort_levels_for_hub_model(&hub, Some("claude"), Some("sonnet-5"))
                .iter()
                .any(|level| level == "xhigh")
        );
        assert!(effort_levels_for_hub_model(&hub, Some("claude"), Some("haiku-4.5")).is_empty());
        assert!(normalize_effort(&hub, Some("claude"), Some("sonnet-5"), Some("Auto")).is_none());
        assert!(normalize_effort(&hub, Some("claude"), Some("haiku-4.5"), Some("high")).is_none());
    }

    #[test]
    fn codex_ignore_user_config_inserts_after_exec_once() {
        let mut args = vec![
            "exec".to_string(),
            "--skip-git-repo-check".to_string(),
            "{prompt}".to_string(),
        ];
        inject_codex_ignore_user_config(&mut args);
        inject_codex_ignore_user_config(&mut args);

        assert_eq!(args[0], "exec");
        assert_eq!(args[1], "--ignore-user-config");
        assert_eq!(
            args.iter()
                .filter(|arg| arg.as_str() == "--ignore-user-config")
                .count(),
            1
        );
    }

    #[test]
    fn supported_agent_commands_receive_managed_storage_access_once() {
        const DIR: &str = "/managed/repos/demo/branches/main/view-buckets/branch";

        for command in ["claude", "codex", "agent"] {
            let mut args = match command {
                "codex" => vec![
                    "exec".to_string(),
                    "--sandbox".to_string(),
                    "workspace-write".to_string(),
                    "{prompt}".to_string(),
                ],
                _ => vec![
                    "--print".to_string(),
                    "-p".to_string(),
                    "{prompt}".to_string(),
                ],
            };
            inject_agent_storage_access(command, &mut args, Some(DIR));
            inject_agent_storage_access(command, &mut args, Some(DIR));

            let add_dir_flag = format!("--add-dir={DIR}");
            let add_dir_count = args
                .windows(2)
                .filter(|pair| pair[0] == "--add-dir" && pair[1] == DIR)
                .count()
                + args.iter().filter(|arg| **arg == add_dir_flag).count();
            assert_eq!(
                add_dir_count, 1,
                "managed storage should be added once for {command}"
            );
            if command == "codex" {
                let add_dir_index = args.iter().position(|arg| arg == "--add-dir").unwrap();
                assert_eq!(add_dir_index, 1);
                assert!(add_dir_index < args.iter().position(|arg| arg == "{prompt}").unwrap());
            } else {
                assert_eq!(args[0], format!("--add-dir={DIR}"));
                assert!(args.iter().any(|arg| arg == "{prompt}"));
            }
        }
    }

    #[test]
    fn storage_access_skipped_without_directory() {
        let mut args = vec![
            "exec".to_string(),
            "--sandbox".to_string(),
            "workspace-write".to_string(),
            "{prompt}".to_string(),
        ];
        inject_agent_storage_access("codex", &mut args, None);
        inject_agent_storage_access("codex", &mut args, Some(""));
        assert!(!args.iter().any(|arg| arg.contains("--add-dir")));
    }

    #[test]
    fn claude_add_dir_does_not_swallow_bare_prompt() {
        let mut args = vec!["{prompt}".to_string()];
        inject_agent_storage_access(
            "claude",
            &mut args,
            Some("/managed/repos/demo/view-buckets/branch"),
        );
        assert_eq!(
            args,
            vec![
                "--add-dir=/managed/repos/demo/view-buckets/branch".to_string(),
                "{prompt}".to_string(),
            ]
        );
    }

    #[test]
    fn custom_agent_commands_do_not_receive_unknown_flags() {
        let mut args = vec!["{prompt}".to_string()];
        inject_agent_storage_access("my-custom-provider", &mut args, Some("/managed/repos/demo"));
        assert_eq!(args, vec!["{prompt}".to_string()]);
    }

    #[test]
    fn supplement_ai_hub_adds_missing_catalog_models() {
        let mut hub = AiHubConfig {
            providers: BTreeMap::from([(
                "claude".into(),
                AiProviderConfig {
                    models: vec![AiModelConfig {
                        id: "custom-model".into(),
                        label: Some("Custom model".into()),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            )]),
            ..Default::default()
        };

        supplement_ai_hub(&mut hub);

        let claude = hub.providers.get("claude").expect("claude provider");
        let ids: Vec<&str> = claude.models.iter().map(|m| m.id.as_str()).collect();
        assert!(ids.contains(&"sonnet-5"), "missing sonnet-5: {ids:?}");
        assert!(ids.contains(&"opus-4.8"), "missing opus-4.8: {ids:?}");
        assert!(ids.contains(&"haiku-4.5"), "missing haiku-4.5: {ids:?}");

        let opus_48 = claude
            .models
            .iter()
            .find(|m| m.id == "opus-4.8")
            .expect("opus-4.8 entry");
        assert_eq!(opus_48.label.as_deref(), Some("Opus 4.8"));
        assert_eq!(
            opus_48.args,
            vec!["--model".to_string(), "claude-opus-4-8".to_string()]
        );

        // User-defined models and order are preserved.
        assert_eq!(claude.models[0].id, "custom-model");
    }

    #[test]
    fn supplement_ai_hub_removes_deprecated_claude_models() {
        let mut hub = AiHubConfig {
            default_model: Some("opus-4.7".into()),
            providers: BTreeMap::from([(
                "claude".into(),
                AiProviderConfig {
                    models: DEPRECATED_CLAUDE_MODEL_IDS
                        .iter()
                        .map(|id| AiModelConfig {
                            id: (*id).into(),
                            ..Default::default()
                        })
                        .collect(),
                    ..Default::default()
                },
            )]),
            ..Default::default()
        };

        supplement_ai_hub(&mut hub);

        let claude = hub.providers.get("claude").expect("claude provider");
        assert!(claude
            .models
            .iter()
            .all(|model| { !DEPRECATED_CLAUDE_MODEL_IDS.contains(&model.id.as_str()) }));
        assert!(claude.models.iter().any(|model| model.id == "sonnet-5"));
        assert_eq!(
            hub.resolve_model_id("claude", None).as_deref(),
            Some("sonnet-5")
        );
    }

    #[test]
    fn supplement_ai_hub_seeds_empty_providers() {
        let mut hub = AiHubConfig::default();
        supplement_ai_hub(&mut hub);
        assert!(hub.providers.contains_key("claude"));
        assert!(hub.providers.contains_key("codex"));
        assert!(hub.providers.contains_key("cursor"));
        assert!(hub.providers.contains_key("opencode"));
        let claude = hub.providers.get("claude").unwrap();
        assert!(
            claude.models.iter().any(|m| m.id == "opus-4.8"),
            "catalog should include opus-4.8"
        );
        let cursor = hub.providers.get("cursor").unwrap();
        let grok = cursor
            .models
            .iter()
            .find(|m| m.id == "grok-4.5")
            .expect("catalog should include grok-4.5");
        assert_eq!(
            grok.args,
            vec!["--model".to_string(), "cursor-grok-4.5-high".to_string()]
        );
        assert!(
            cursor.models.iter().any(|m| m.id == "composer-2.5"),
            "catalog should include composer-2.5"
        );
        let opencode = hub.providers.get("opencode").unwrap();
        assert_eq!(opencode.command, "opencode");
        assert!(opencode.args.iter().any(|a| a == "--auto"));
        assert!(opencode.args.iter().any(|a| a.contains("{prompt}")));
        assert!(
            opencode.models.iter().any(|m| m.id == "default"),
            "catalog should include opencode default model"
        );
        assert!(
            opencode.models.iter().any(|m| m.id == "claude-sonnet-4-5"),
            "catalog should include opencode claude-sonnet-4-5"
        );
    }

    #[test]
    fn supplement_ai_hub_repairs_stale_cursor_grok_slug() {
        let mut hub = AiHubConfig::default();
        hub.providers.insert(
            "cursor".into(),
            AiProviderConfig {
                command: "agent".into(),
                models: vec![AiModelConfig {
                    id: "grok-4.5".into(),
                    args: vec!["--model".into(), "grok-4.5".into()],
                    ..Default::default()
                }],
                ..Default::default()
            },
        );

        supplement_ai_hub(&mut hub);

        let grok = hub
            .providers
            .get("cursor")
            .unwrap()
            .models
            .iter()
            .find(|m| m.id == "grok-4.5")
            .unwrap();
        assert_eq!(grok.args, vec!["--model", "cursor-grok-4.5-high"]);
    }

    #[test]
    fn inject_claude_effort_idempotent() {
        let mut args = vec!["--print".into()];
        inject_claude_effort(&mut args, Some("high"));
        assert!(args.contains(&"--effort".to_string()));
        assert!(args.contains(&"high".to_string()));
        let len = args.len();
        inject_claude_effort(&mut args, Some("low"));
        assert_eq!(args.len(), len);
    }

    #[test]
    fn inject_codex_effort_idempotent() {
        let mut args = vec!["exec".into()];
        inject_codex_effort(&mut args, Some("high"));
        assert!(args
            .windows(2)
            .any(|pair| { pair[0] == "-c" && pair[1] == "model_reasoning_effort=high" }));
        let len = args.len();
        inject_codex_effort(&mut args, Some("low"));
        assert_eq!(args.len(), len);
    }

    #[test]
    fn opencode_model_args_insert_before_prompt() {
        let mut args = vec![
            "run".to_string(),
            "--auto".to_string(),
            "{prompt}".to_string(),
        ];
        extend_provider_model_args(
            "opencode",
            &mut args,
            &["--model".into(), "anthropic/claude-sonnet-4-5".into()],
        );
        assert_eq!(
            args,
            vec![
                "run",
                "--auto",
                "--model",
                "anthropic/claude-sonnet-4-5",
                "{prompt}",
            ]
        );
    }

    #[test]
    fn inject_opencode_effort_before_prompt_idempotent() {
        let mut args = vec![
            "run".to_string(),
            "--auto".to_string(),
            "{prompt}".to_string(),
        ];
        inject_opencode_effort(&mut args, Some("high"));
        assert_eq!(args, vec!["run", "--auto", "--variant", "high", "{prompt}"]);
        let len = args.len();
        inject_opencode_effort(&mut args, Some("low"));
        assert_eq!(args.len(), len);
        inject_provider_effort("opencode", &mut args, Some("default"), Some("max"));
        assert_eq!(args.len(), len);
    }

    #[test]
    fn ensure_opencode_auto_inserts_once_after_run() {
        let mut args = vec!["run".to_string(), "{prompt}".to_string()];
        ensure_opencode_auto(&mut args);
        ensure_opencode_auto(&mut args);
        assert_eq!(args, vec!["run", "--auto", "{prompt}"]);
    }

    #[test]
    fn opencode_storage_permission_env_allows_bucket() {
        let (key, value) =
            opencode_storage_permission_env(Some("/managed/repos/demo/view-buckets/branch"))
                .expect("env should be set");
        assert_eq!(key, "OPENCODE_PERMISSION");
        let parsed: serde_json::Value = serde_json::from_str(&value).unwrap();
        // Bare permission object — not wrapped in {"permission": ...}.
        assert!(parsed.get("permission").is_none());
        let external = parsed["external_directory"].as_object().unwrap();
        // Deny-all first, then bucket allow (last match wins under --auto).
        let keys: Vec<&String> = external.keys().collect();
        assert_eq!(
            keys,
            vec!["*", "/managed/repos/demo/view-buckets/branch/**"]
        );
        assert_eq!(external["*"], "deny");
        assert_eq!(
            external["/managed/repos/demo/view-buckets/branch/**"],
            "allow"
        );
        assert!(opencode_storage_permission_env(None).is_none());
        assert!(opencode_storage_permission_env(Some("")).is_none());
    }

    #[test]
    fn opencode_storage_permission_env_normalizes_windows_separators() {
        let (_, value) = opencode_storage_permission_env(Some(
            r"C:\Users\demo\AppData\Local\easy-review\repos\x\branches\main\view-buckets\branch\",
        ))
        .expect("env should be set");
        let parsed: serde_json::Value = serde_json::from_str(&value).unwrap();
        assert_eq!(parsed["external_directory"]["*"], "deny");
        assert_eq!(
            parsed["external_directory"]
                ["C:/Users/demo/AppData/Local/easy-review/repos/x/branches/main/view-buckets/branch/**"],
            "allow"
        );
    }

    #[test]
    fn opencode_readonly_permission_env_denies_edit_and_external() {
        let (key, value) = opencode_readonly_permission_env();
        assert_eq!(key, "OPENCODE_PERMISSION");
        let parsed: serde_json::Value = serde_json::from_str(&value).unwrap();
        assert_eq!(parsed["edit"], "deny");
        assert_eq!(parsed["external_directory"]["*"], "deny");
        assert_eq!(parsed["bash"]["*"], "deny");
        assert_eq!(parsed["bash"]["grep *"], "allow");
        assert_eq!(parsed["bash"]["rg *"], "allow");
        assert!(parsed.get("permission").is_none());
    }

    #[test]
    fn apply_opencode_spawn_sets_auto_flags_and_storage_env() {
        let mut args = vec!["run".to_string(), "{prompt}".to_string()];
        let env =
            apply_opencode_spawn(r"C:\tools\opencode.exe", &mut args, Some("/managed/review"))
                .expect("env");
        assert!(args.iter().any(|a| a == "--auto"));
        let prompt_idx = args.iter().position(|a| a.contains("{prompt}")).unwrap();
        let auto_idx = args.iter().position(|a| a == "--auto").unwrap();
        assert!(auto_idx < prompt_idx);
        assert_eq!(env.0, "OPENCODE_PERMISSION");
        let parsed: serde_json::Value = serde_json::from_str(&env.1).unwrap();
        assert_eq!(parsed["external_directory"]["*"], "deny");
        assert_eq!(parsed["external_directory"]["/managed/review/**"], "allow");
        assert!(apply_opencode_spawn("claude", &mut args, Some("/managed/review")).is_none());
    }

    #[test]
    fn apply_opencode_readonly_spawn_sets_auto_and_readonly_env() {
        let mut args = vec!["run".to_string(), "{prompt}".to_string()];
        let env = apply_opencode_readonly_spawn("opencode", &mut args).expect("env");
        assert!(args.iter().any(|a| a == "--auto"));
        assert_eq!(env.0, "OPENCODE_PERMISSION");
        let parsed: serde_json::Value = serde_json::from_str(&env.1).unwrap();
        assert_eq!(parsed["edit"], "deny");
        assert_eq!(parsed["external_directory"]["*"], "deny");
        assert!(apply_opencode_readonly_spawn("claude", &mut args).is_none());
    }

    #[test]
    fn agent_command_is_opencode_detects_basename() {
        assert!(agent_command_is_opencode("opencode"));
        assert!(agent_command_is_opencode("/opt/homebrew/bin/opencode"));
        assert!(agent_command_is_opencode(r"C:\bin\opencode.exe"));
        assert!(!agent_command_is_opencode("claude"));
    }

    #[test]
    fn provider_effort_only_injects_for_effort_capable_codex_models() {
        let mut hub = AiHubConfig::default();
        supplement_ai_hub(&mut hub);
        let unsupported_effort =
            normalize_effort(&hub, Some("codex"), Some("legacy-model"), Some("high"));
        let mut unsupported_args = vec!["exec".into()];
        inject_provider_effort(
            "codex",
            &mut unsupported_args,
            Some("legacy-model"),
            unsupported_effort.as_deref(),
        );
        assert!(!unsupported_args
            .iter()
            .any(|arg| arg.starts_with("model_reasoning_effort=")));

        let mut supported_args = vec!["exec".into()];
        inject_provider_effort(
            "codex",
            &mut supported_args,
            Some("gpt-5.6-sol"),
            normalize_effort(&hub, Some("codex"), Some("gpt-5.6-sol"), Some("high")).as_deref(),
        );
        assert!(supported_args
            .iter()
            .any(|arg| arg == "model_reasoning_effort=high"));
    }

    #[test]
    fn resolve_effort_precedence() {
        let hub = AiHubConfig {
            default_effort: Some("medium".into()),
            ..Default::default()
        };
        let agent = AgentConfig {
            effort: Some("low".into()),
            ..Default::default()
        };
        assert_eq!(
            resolve_effort(&hub, &agent, Some("high"), None).as_deref(),
            Some("high")
        );
        assert_eq!(
            resolve_effort(&hub, &agent, Some("high"), Some("max")).as_deref(),
            Some("max")
        );
        assert_eq!(
            resolve_effort(&hub, &agent, None, None).as_deref(),
            Some("medium")
        );
        let hub_empty = AiHubConfig::default();
        assert_eq!(
            resolve_effort(&hub_empty, &agent, None, None).as_deref(),
            Some("low")
        );
    }

    #[test]
    fn default_selection_uses_the_configured_model_for_every_action() {
        let mut hub = AiHubConfig {
            default_provider: Some("codex".into()),
            default_model: Some("gpt-5.6-luna".into()),
            default_effort: Some("high".into()),
            ..Default::default()
        };
        hub.providers.insert(
            "codex".into(),
            AiProviderConfig {
                models: vec![
                    AiModelConfig {
                        id: "gpt-5.6-luna".into(),
                        effort_levels: vec!["high".into()],
                        ..Default::default()
                    },
                    AiModelConfig {
                        id: "gpt-5.3-codex-spark".into(),
                        avg_latency_ms: Some(1),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
        );

        let selection = hub.resolve_default_selection(&AgentConfig::default());
        assert_eq!(selection.provider_id.as_deref(), Some("codex"));
        assert_eq!(selection.model_id.as_deref(), Some("gpt-5.6-luna"));
        assert_eq!(selection.effort.as_deref(), Some("high"));
        assert_eq!(
            hub.resolve_model_id("codex", None).as_deref(),
            Some("gpt-5.6-luna")
        );
    }

    #[test]
    fn legacy_reviewer_models_are_ignored_when_loading_config() {
        let config: ErConfig = toml::from_str(
            r#"
            [ai_hub.reviewer_models]
            triage = "gpt-5.3-codex-spark"
            "#,
        )
        .unwrap();
        assert!(config.ai_hub.providers.is_empty());
    }

    #[test]
    fn set_default_selection_validates_and_normalizes_the_shared_choice() {
        let mut config = ErConfig::default();
        config.ai_hub.providers.insert(
            "codex".into(),
            AiProviderConfig {
                models: vec![AiModelConfig {
                    id: "gpt-5.6-luna".into(),
                    effort_levels: vec!["high".into()],
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        config.ai_hub.default_effort = Some("high".into());

        let selected = config
            .ai_hub
            .set_default_selection("codex", Some("gpt-5.6-luna"), &config.agent)
            .unwrap();
        assert_eq!(config.ai_hub.default_provider.as_deref(), Some("codex"));
        assert_eq!(config.ai_hub.default_model.as_deref(), Some("gpt-5.6-luna"));
        assert_eq!(selected.model_id.as_deref(), Some("gpt-5.6-luna"));
        assert_eq!(
            config
                .ai_hub
                .set_default_selection("codex", Some("missing"), &config.agent)
                .unwrap_err()
                .to_string(),
            "Unknown model 'missing' for provider 'codex'"
        );
    }

    #[test]
    fn resolve_selection_keeps_runtime_effort_without_mutating_defaults() {
        let config = ErConfig {
            ai_hub: AiHubConfig {
                default_provider: Some("codex".into()),
                default_model: Some("gpt-5.6-luna".into()),
                default_effort: Some("medium".into()),
                providers: [(
                    "codex".into(),
                    AiProviderConfig {
                        models: vec![
                            AiModelConfig {
                                id: "gpt-5.6-luna".into(),
                                effort_levels: vec!["low".into(), "medium".into(), "high".into()],
                                ..Default::default()
                            },
                            AiModelConfig {
                                id: "gpt-5.5".into(),
                                effort_levels: vec!["low".into(), "medium".into(), "high".into()],
                                ..Default::default()
                            },
                        ],
                        ..Default::default()
                    },
                )]
                .into_iter()
                .collect(),
                ..Default::default()
            },
            ..Default::default()
        };

        let selected = config
            .ai_hub
            .resolve_selection("codex", Some("gpt-5.5"), &config.agent, Some("high"))
            .unwrap();
        assert_eq!(selected.model_id.as_deref(), Some("gpt-5.5"));
        assert_eq!(selected.effort.as_deref(), Some("high"));
        assert_eq!(config.ai_hub.default_model.as_deref(), Some("gpt-5.6-luna"));
        assert_eq!(config.ai_hub.default_effort.as_deref(), Some("medium"));
    }
}
