//! The flag-on working-tree keys and the Git hub (P69.1). Each write goes to
//! the service as a command; the reply's notice is what the status line
//! says, and a refused write shows the port's message.

use er_engine::tab_identity::DiffMode;
use er_engine::TabKey;
use er_service::{Command, TabContent};

use super::tui_view::{Mode, TabViewState};
use crate::input::keys::{Edit, OverlayAction};
use crate::screen::view::HubItem;

/// What a Git hub row does when chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitPick {
    Push,
    StageFile,
    StageAll,
    /// A row whose action has no service route on this path yet.
    Unported,
}

/// The Git hub: the App's rows, with their labels, hints and enabled states.
pub struct GitHub {
    pub rows: Vec<HubItem>,
    picks: Vec<GitPick>,
    pub selected: usize,
}

/// What a key on the Git hub asks the loop to do.
pub enum HubStep {
    Stay,
    Close,
    Notify(String),
    Unported,
    Run(GitPick),
}

fn row(label: &str, hint: &str, description: &str, enabled: bool) -> HubItem {
    HubItem {
        label: label.into(),
        hint: hint.into(),
        description: description.into(),
        is_header: false,
        enabled,
        opens_stack_pr: false,
    }
}

impl GitHub {
    /// The hub as the App opens it on this tab. Refresh and the GitHub rows
    /// wait for their entries (P68, P71).
    pub fn open(content: &TabContent) -> Self {
        let tab = &content.tab;
        let has_pr = tab.pr_number.is_some();
        let approve = if has_pr {
            "Submit approval review on GitHub"
        } else {
            "No PR detected"
        };
        let rows = [
            (
                row(
                    "Push to remote",
                    "",
                    "Push current branch to origin",
                    tab.mode == DiffMode::Staged,
                ),
                GitPick::Push,
            ),
            (
                row(
                    "Stage current file",
                    "s",
                    "Stage or unstage the selected file",
                    true,
                ),
                GitPick::StageFile,
            ),
            (
                row("Stage all files", "", "Stage all changed files", true),
                GitPick::StageAll,
            ),
            (
                row("Refresh diff", "R", "Re-run git diff and reload", true),
                GitPick::Unported,
            ),
            (
                row(
                    "Pull GitHub comments",
                    "",
                    "Sync review comments from GitHub PR",
                    true,
                ),
                GitPick::Unported,
            ),
            (
                row(
                    "Comment on PR",
                    "",
                    "Post a general comment on the PR (not attached to a line)",
                    true,
                ),
                GitPick::Unported,
            ),
            (row("Approve PR", "", approve, has_pr), GitPick::Unported),
        ];
        let (rows, picks) = rows.into_iter().unzip();
        Self {
            rows,
            picks,
            selected: 0,
        }
    }

    /// Runs one key on the hub, as the App's modal hub does: a disabled row
    /// says why and closes.
    pub fn act(&mut self, action: OverlayAction) -> HubStep {
        match action {
            OverlayAction::Next => {
                self.selected = (self.selected + 1).min(self.rows.len().saturating_sub(1));
            }
            OverlayAction::Prev => self.selected = self.selected.saturating_sub(1),
            OverlayAction::Close => return HubStep::Close,
            OverlayAction::SelectRow => {
                let (Some(row), Some(&pick)) =
                    (self.rows.get(self.selected), self.picks.get(self.selected))
                else {
                    return HubStep::Close;
                };
                return match pick {
                    _ if !row.enabled => HubStep::Notify(row.description.clone()),
                    GitPick::Unported => HubStep::Unported,
                    pick => HubStep::Run(pick),
                };
            }
            _ => {}
        }
        HubStep::Stay
    }
}

/// The write `s` sends for the selected file, by the view the tab shows, or
/// why it sends none. No file selected sends nothing.
pub fn stage_command(
    key: TabKey,
    content: &TabContent,
    selected: usize,
) -> Result<Option<Command>, String> {
    let Some(file) = content.diff.files.get(selected) else {
        return Ok(None);
    };
    let path = file.path.clone();
    match content.tab.mode {
        DiffMode::Branch | DiffMode::Unstaged | DiffMode::Conflicts => {
            Ok(Some(Command::StageFile { key, path }))
        }
        DiffMode::Staged => Ok(Some(Command::UnstageFile { key, path })),
        _ => Err("Staging not available in this mode".into()),
    }
}

/// Runs one key in the commit input. While a commit is in flight the text
/// stays as sent; Enter answers the commit to send, and an empty message
/// leaves the input as the App does.
pub fn commit_key(
    key: TabKey,
    state: &mut TabViewState,
    mode: &mut Mode,
    edit: Edit,
    sending: bool,
) -> Option<Command> {
    match edit {
        _ if sending => {}
        Edit::Submit => {
            let message = state.commit_input.trim().to_string();
            if !message.is_empty() {
                return Some(Command::Commit { key, message });
            }
            *mode = Mode::Normal;
        }
        Edit::Cancel => {
            state.commit_input.clear();
            *mode = Mode::Normal;
        }
        Edit::Insert(c) => state.commit_input.push(c),
        Edit::Backspace => {
            state.commit_input.pop();
        }
        Edit::Left | Edit::Right => {}
    }
    None
}
