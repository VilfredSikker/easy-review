//! Settings field catalog split by scope (General / App / Terminal).

use super::config_desktop_settings::ConfigHubFieldDto;
use super::{ErConfig, AGENT_EFFORT_OPTIONS};

pub const THEME_OPTIONS: &[&str] = &[
    "ocean-depth",
    "moonlight",
    "daybreak",
    "high-contrast",
    "tokyo-night",
    "tokyo-night-storm",
    "tokyo-night-moon",
    "tokyo-night-day",
];

/// Which settings surface a field belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SettingsScope {
    General,
    App,
    Terminal,
}

impl SettingsScope {
    pub const ALL: [SettingsScope; 2] = [SettingsScope::General, SettingsScope::Terminal];

    pub fn label(self) -> &'static str {
        match self {
            SettingsScope::General => "General",
            SettingsScope::App => "App",
            SettingsScope::Terminal => "Terminal",
        }
    }

    pub fn from_tab_index(index: usize) -> Self {
        match index {
            1 => SettingsScope::Terminal,
            _ => SettingsScope::General,
        }
    }

    pub fn tab_index(self) -> usize {
        match self {
            SettingsScope::General => 0,
            SettingsScope::Terminal => 1,
            SettingsScope::App => 1,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SettingsFieldsGrouped {
    pub general: Vec<ConfigHubFieldDto>,
    pub app: Vec<ConfigHubFieldDto>,
    pub terminal: Vec<ConfigHubFieldDto>,
}

pub fn agent_effort_label(effort: &Option<String>) -> String {
    effort
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("medium")
        .to_string()
}

pub fn settings_fields_grouped(config: &ErConfig) -> SettingsFieldsGrouped {
    SettingsFieldsGrouped {
        general: general_desktop_fields(config),
        app: Vec::new(),
        terminal: terminal_desktop_fields(config),
    }
}

pub fn desktop_settings_fields_for_scope(
    config: &ErConfig,
    scope: SettingsScope,
) -> Vec<ConfigHubFieldDto> {
    match scope {
        SettingsScope::General => general_desktop_fields(config),
        SettingsScope::App => Vec::new(),
        SettingsScope::Terminal => terminal_desktop_fields(config),
    }
}

/// Flat list (General → App → Terminal) for tests and legacy callers.
pub fn desktop_settings_fields_flat(config: &ErConfig) -> Vec<ConfigHubFieldDto> {
    let grouped = settings_fields_grouped(config);
    let mut fields = grouped.general;
    fields.extend(grouped.app);
    fields.extend(grouped.terminal);
    fields
}

fn general_desktop_fields(config: &ErConfig) -> Vec<ConfigHubFieldDto> {
    let mut fields: Vec<ConfigHubFieldDto> = vec![
        ConfigHubFieldDto::Section {
            title: "Appearance".into(),
        },
        ConfigHubFieldDto::Cycle {
            key: "display.theme".into(),
            label: "Theme".into(),
            description: "Color theme for the desktop app and TUI".into(),
            options: THEME_OPTIONS.iter().map(|s| s.to_string()).collect(),
            value: config.display.theme.clone(),
        },
        ConfigHubFieldDto::Section {
            title: "Commands".into(),
        },
        ConfigHubFieldDto::Text {
            key: "commands.summary".into(),
            label: "Summary".into(),
            description: "Generate diff summary".into(),
            placeholder: "e.g. claude -p 'Summarize: {diff}'".into(),
            value: config.commands.summary.clone().unwrap_or_default(),
            strict: true,
        },
        ConfigHubFieldDto::Text {
            key: "commands.test".into(),
            label: "Test".into(),
            description: "Run tests".into(),
            placeholder: "e.g. cargo test".into(),
            value: config.commands.test.clone().unwrap_or_default(),
            strict: true,
        },
        ConfigHubFieldDto::Text {
            key: "commands.lint".into(),
            label: "Lint".into(),
            description: "Run linter".into(),
            placeholder: "e.g. cargo clippy".into(),
            value: config.commands.lint.clone().unwrap_or_default(),
            strict: true,
        },
        ConfigHubFieldDto::Text {
            key: "commands.typecheck".into(),
            label: "Typecheck".into(),
            description: "Run type checker".into(),
            placeholder: "e.g. tsc --noEmit".into(),
            value: config.commands.typecheck.clone().unwrap_or_default(),
            strict: true,
        },
        ConfigHubFieldDto::Text {
            key: "commands.security".into(),
            label: "Security".into(),
            description: "Run security scan".into(),
            placeholder: "e.g. cargo audit".into(),
            value: config.commands.security.clone().unwrap_or_default(),
            strict: true,
        },
        ConfigHubFieldDto::Bool {
            key: "summary.push_to_pr".into(),
            label: "Push summary to PR body".into(),
            description: "Auto-push summary to GitHub PR".into(),
            value: config.summary.push_to_pr,
        },
        ConfigHubFieldDto::Section {
            title: "Agent".into(),
        },
        ConfigHubFieldDto::Text {
            key: "agent.command".into(),
            label: "Command".into(),
            description: "Agent executable".into(),
            placeholder: "e.g. claude".into(),
            value: config.agent.command.clone(),
            strict: true,
        },
        ConfigHubFieldDto::Text {
            key: "agent.args".into(),
            label: "Args".into(),
            description: "Agent arguments (space-separated)".into(),
            placeholder: "e.g. --print -p {prompt}".into(),
            value: config.agent.args.join(" "),
            strict: true,
        },
        ConfigHubFieldDto::Cycle {
            key: "agent.effort".into(),
            label: "Effort".into(),
            description: "Claude effort level".into(),
            options: AGENT_EFFORT_OPTIONS.iter().map(|s| s.to_string()).collect(),
            value: agent_effort_label(&config.agent.effort),
        },
        ConfigHubFieldDto::Cycle {
            key: "ai_hub.max_concurrent_reviews".into(),
            label: "Max parallel reviews".into(),
            description: "AI review agents running at once; extra reviews queue".into(),
            options: (1..=6).map(|n| n.to_string()).collect(),
            value: config.ai_hub.effective_max_concurrent_reviews().to_string(),
        },
        ConfigHubFieldDto::Section {
            title: "Watched Paths".into(),
        },
        ConfigHubFieldDto::Cycle {
            key: "watched.diff_mode".into(),
            label: "Diff mode".into(),
            description: "How to diff watched files".into(),
            options: vec!["content".into(), "snapshot".into()],
            value: config.watched.diff_mode.clone(),
        },
    ];

    for (i, path) in config.watched.paths.iter().enumerate() {
        fields.push(ConfigHubFieldDto::ListEntry {
            key: format!("watched.paths.{i}"),
            label: path.clone(),
            index: i,
        });
    }

    fields.push(ConfigHubFieldDto::ListAdd {
        key: "watched.paths.add".into(),
        label: "Add pattern…".into(),
    });

    fields
}

fn terminal_desktop_fields(config: &ErConfig) -> Vec<ConfigHubFieldDto> {
    vec![
        ConfigHubFieldDto::Section {
            title: "Views".into(),
        },
        ConfigHubFieldDto::Bool {
            key: "features.view_branch".into(),
            label: "Branch diff".into(),
            description: "Show branch diff mode (TUI tab 1)".into(),
            value: config.features.view_branch,
        },
        ConfigHubFieldDto::Bool {
            key: "features.view_unstaged".into(),
            label: "Unstaged changes".into(),
            description: "Show unstaged changes mode (TUI tab 2)".into(),
            value: config.features.view_unstaged,
        },
        ConfigHubFieldDto::Bool {
            key: "features.view_staged".into(),
            label: "Staged changes".into(),
            description: "Show staged changes mode (TUI tab 3)".into(),
            value: config.features.view_staged,
        },
        ConfigHubFieldDto::Bool {
            key: "features.view_history".into(),
            label: "History".into(),
            description: "Show commit history mode (TUI tab 4)".into(),
            value: config.features.view_history,
        },
        ConfigHubFieldDto::Bool {
            key: "features.view_conflicts".into(),
            label: "Conflicts".into(),
            description: "Show merge conflicts mode (TUI tab 5)".into(),
            value: config.features.view_conflicts,
        },
        ConfigHubFieldDto::Bool {
            key: "features.view_hidden".into(),
            label: "Hidden files".into(),
            description: "Show hidden files mode (TUI tab 6)".into(),
            value: config.features.view_hidden,
        },
        ConfigHubFieldDto::Section {
            title: "Display".into(),
        },
        ConfigHubFieldDto::Bool {
            key: "display.line_numbers".into(),
            label: "Line numbers".into(),
            description: "Show line numbers in diff".into(),
            value: config.display.line_numbers,
        },
        ConfigHubFieldDto::Bool {
            key: "display.wrap_lines".into(),
            label: "Wrap lines".into(),
            description: "Wrap long lines".into(),
            value: config.display.wrap_lines,
        },
        ConfigHubFieldDto::Bool {
            key: "display.split_diff".into(),
            label: "Split diff".into(),
            description: "Side-by-side diff view".into(),
            value: config.display.split_diff,
        },
        ConfigHubFieldDto::Bool {
            key: "display.auto_context".into(),
            label: "Auto-expand context".into(),
            description: "Pick unified context per file (small → more, big → less)".into(),
            value: config.display.auto_context_threshold > 0,
        },
        ConfigHubFieldDto::Cycle {
            key: "display.tab_width".into(),
            label: "Tab width".into(),
            description: "Spaces per tab stop".into(),
            options: (1..=16).map(|n| n.to_string()).collect(),
            value: config.display.tab_width.to_string(),
        },
        ConfigHubFieldDto::Section {
            title: "Key Hints".into(),
        },
        ConfigHubFieldDto::Bool {
            key: "hints.navigation".into(),
            label: "Navigation hints".into(),
            description: "Show j/k, n/N, ␣, / hints".into(),
            value: config.hints.navigation,
        },
        ConfigHubFieldDto::Bool {
            key: "hints.staging".into(),
            label: "Staging hints".into(),
            description: "Show s, c commit hints".into(),
            value: config.hints.staging,
        },
        ConfigHubFieldDto::Bool {
            key: "hints.comments".into(),
            label: "Comment hints".into(),
            description: "Show r, d comment action hints".into(),
            value: config.hints.comments,
        },
        ConfigHubFieldDto::Bool {
            key: "hints.verbose".into(),
            label: "Verbose hints".into(),
            description: "Show all key hints (resize, filters, etc)".into(),
            value: config.hints.verbose,
        },
    ]
}
