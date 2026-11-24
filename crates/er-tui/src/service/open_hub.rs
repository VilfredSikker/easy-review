//! The flag-on Open hub (`o`): the App's rows, with "Change compare base"
//! live. One function per section, for P75 (Navigate, Current PR) and P73
//! (Stack) to replace; until then those rows answer unported.

use er_service::TabContent;

use crate::input::keys::OverlayAction;
use crate::screen::view::HubItem;
use crate::screen::UNPORTED;

/// What a row does when chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenPick {
    /// A header or a row with nothing to run.
    Nothing,
    ChangeBase,
    Unported,
}

/// What a key on the hub asks the loop to do.
pub enum OpenStep {
    Stay,
    Close,
    Notify(String),
    Unported,
    ChangeBase,
}

/// The Open hub's rows and the selection.
pub struct OpenHub {
    pub rows: Vec<HubItem>,
    picks: Vec<OpenPick>,
    pub selected: usize,
}

type Row = (HubItem, OpenPick);

fn row(label: &str, description: &str, enabled: bool, pick: OpenPick) -> Row {
    let item = HubItem {
        label: label.into(),
        hint: String::new(),
        description: description.into(),
        is_header: false,
        enabled,
        opens_stack_pr: false,
    };
    (item, pick)
}

fn header(label: &str) -> Row {
    let (mut item, pick) = row(label, "", false, OpenPick::Nothing);
    item.is_header = true;
    (item, pick)
}

/// The Navigate section; no worktree count, since that read waits on git.
fn navigate_rows(content: &TabContent) -> Vec<Row> {
    let tab = &content.tab;
    let remote = tab.identity().is_remote();
    let base = if remote {
        "Not available for remote PRs".to_string()
    } else {
        format!(
            "Diff against a different branch (current: {})",
            tab.base_branch
        )
    };
    vec![
        header("── Navigate ──"),
        row(
            "Browse folders",
            "Open a local git repo",
            true,
            OpenPick::Unported,
        ),
        row(
            "Switch worktree",
            "Jump to another worktree",
            true,
            OpenPick::Unported,
        ),
        row(
            "Open remote PR",
            "Review a GitHub PR by URL",
            true,
            OpenPick::Unported,
        ),
        row("Change compare base", &base, !remote, OpenPick::ChangeBase),
    ]
}

/// The Stack section (P73).
fn stack_rows(_content: &TabContent) -> Vec<Row> {
    let description = format!("Stacked PRs are {UNPORTED}");
    vec![row("Stacked PRs", &description, true, OpenPick::Unported)]
}

/// The Current PR section (P75).
fn current_pr_rows(content: &TabContent) -> Vec<Row> {
    if content.tab.pr_number.is_none() {
        return Vec::new();
    }
    vec![
        header("── Current PR ──"),
        row(
            "Open PR in browser",
            "View on GitHub",
            true,
            OpenPick::Unported,
        ),
    ]
}

impl OpenHub {
    /// The hub as the App opens it on this tab, on its first enabled row.
    pub fn open(content: &TabContent) -> Self {
        let mut rows = navigate_rows(content);
        rows.push(header("── Stack ──"));
        rows.extend(stack_rows(content));
        rows.extend(current_pr_rows(content));
        let selected = rows
            .iter()
            .position(|(item, _)| !item.is_header && item.enabled)
            .unwrap_or(0);
        let (rows, picks) = rows.into_iter().unzip();
        Self {
            rows,
            picks,
            selected,
        }
    }

    /// One key, as the App's modal hub.
    pub fn act(&mut self, action: OverlayAction) -> OpenStep {
        let selectable = (0..self.rows.len()).filter(|&at| !self.rows[at].is_header);
        match action {
            OverlayAction::Next => {
                if let Some(at) = selectable.into_iter().find(|&at| at > self.selected) {
                    self.selected = at;
                }
            }
            OverlayAction::Prev => {
                if let Some(at) = selectable.into_iter().rfind(|&at| at < self.selected) {
                    self.selected = at;
                }
            }
            OverlayAction::Close => return OpenStep::Close,
            OverlayAction::SelectRow => {
                let (Some(row), Some(&pick)) =
                    (self.rows.get(self.selected), self.picks.get(self.selected))
                else {
                    return OpenStep::Close;
                };
                return match pick {
                    OpenPick::Nothing => OpenStep::Close,
                    _ if !row.enabled => OpenStep::Notify(row.description.clone()),
                    OpenPick::Unported => OpenStep::Unported,
                    OpenPick::ChangeBase => OpenStep::ChangeBase,
                };
            }
            _ => {}
        }
        OpenStep::Stay
    }
}
