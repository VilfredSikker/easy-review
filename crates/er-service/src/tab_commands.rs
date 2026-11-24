//! The tab commands run through the agent launcher: questions, notes, the
//! summary and the named `[commands]` entries.

use er_engine::ai::prompts;
use er_engine::tab_identity::DiffMode;
use er_protocol::ErError;

use crate::content::TabContent;
use crate::runs::{agent, target, RunTab};
use crate::tasks::{self, refused};

pub fn questions(run: &RunTab<'_>) -> Result<(), ErError> {
    tab_command(run, TabCommand::Questions)
}

pub fn notes(run: &RunTab<'_>) -> Result<(), ErError> {
    tab_command(run, TabCommand::Notes)
}

pub fn summary(run: &RunTab<'_>) -> Result<(), ErError> {
    tab_command(run, TabCommand::Summary)
}

/// Its agent reads the bucket's sidecars and the diff itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabCommand {
    Questions,
    Notes,
    Summary,
}

impl TabCommand {
    pub fn name(self) -> &'static str {
        match self {
            Self::Questions => "questions",
            Self::Notes => "notes",
            Self::Summary => "summary",
        }
    }
}

fn tab_command(run: &RunTab<'_>, command: TabCommand) -> Result<(), ErError> {
    let prompt = tab_command_prompt(run.content, command)?;
    let content = run.content;
    let target = target(
        content,
        content.context.er_dir.clone(),
        content.context.scope.clone(),
    );
    agent(run, target, command.name(), &prompt)
}

/// By the App's rules: a remote tab names its PR, a local tab its git view.
pub fn tab_command_prompt(content: &TabContent, command: TabCommand) -> Result<String, ErError> {
    let tab = &content.tab;
    let output_dir = &content.context.er_dir;
    if content.context.is_remote {
        let (Some(slug), Some(pr_number)) = (&tab.remote_repo, tab.pr_number) else {
            return Err(refused("Remote mode missing repo or PR number"));
        };
        let Some((owner, repo)) = slug.split_once('/').filter(|(_, r)| !r.contains('/')) else {
            return Err(refused(format!("Invalid remote repo slug: {slug}")));
        };
        let remote = match command {
            TabCommand::Questions => prompts::build_questions_prompt_remote,
            TabCommand::Notes => prompts::build_notes_prompt_remote,
            TabCommand::Summary => prompts::build_summary_prompt_remote,
        };
        return Ok(remote(owner, repo, pr_number, output_dir));
    }
    if !matches!(
        tab.mode,
        DiffMode::Branch | DiffMode::Unstaged | DiffMode::Staged
    ) {
        return Err(refused(match command {
            TabCommand::Questions => "AI questions not available in this mode",
            TabCommand::Notes => "AI notes not available in this mode",
            TabCommand::Summary => "Summary generation not available in this mode",
        }));
    }
    let local = match command {
        TabCommand::Questions => prompts::build_questions_prompt_local_managed,
        TabCommand::Notes => prompts::build_notes_prompt_local_managed,
        TabCommand::Summary => prompts::build_summary_prompt_local_managed,
    };
    Ok(local(&tab.base_branch, tab.mode.git_mode(), output_dir))
}

/// Held on the host, so it shows in the rows and the bucket's log.
pub fn command(run: &RunTab<'_>, name: &str, shell_cmd: &str) -> Result<(), ErError> {
    let content = run.content;
    let tab = &content.tab;
    let er_dir = content.context.er_dir.clone();
    let target = target(content, er_dir.clone(), content.context.scope.clone());
    let config = (run.hooks.config)();
    tasks::hold_agent(run.host, run.hooks, target, name, |log_tx| {
        er_engine::shell_command::launch_shell_command(
            er_engine::shell_command::ShellCommandRequest {
                name,
                shell_cmd,
                repo_root: tab.repo_root.clone(),
                base_branch: &tab.base_branch,
                branch: &tab.current_branch,
                er_dir,
                config: &config,
                log_tx,
            },
        )
    })
    .map(drop)
    .map_err(refused)
}
