//! The flag-on Open hub (`o`): the App's rows, one function per section.
//! "Switch worktree" reads the worktree list the hub's opening asked the
//! service for, so it stays disabled until that list lands.

use er_engine::TabKey;
use er_service::TabContent;

use crate::input::OverlayAction;
use crate::screen::view::HubItem;

/// A stack layer's PR, which Enter opens for review.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StackPr {
    pub url: String,
    pub number: u64,
    pub branch: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpenPick {
    Nothing,
    ChangeBase,
    RemoteUrl,
    StackPr(StackPr),
    RefreshStack,
    Browse,
    Worktrees,
    Browser,
}

pub enum OpenStep {
    Stay,
    Close,
    Notify(String),
    ChangeBase,
    RemoteUrl,
    OpenPr(StackPr),
    RefreshStack,
    Browse,
    Worktrees,
    /// "Open PR in browser": the tab's own PR.
    Browser,
    /// `b` on a stack row: that layer's PR.
    StackBrowser(StackPr),
}

pub struct OpenHub {
    pub rows: Vec<HubItem>,
    picks: Vec<OpenPick>,
    pub selected: usize,
    /// The tab, its PR section's revision, whether its stack was being read
    /// again, and how many worktrees were known, when the rows were built.
    built: (TabKey, u64, bool, Option<usize>),
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

/// The Navigate section. `worktrees`: how many the repo has, once the
/// service's list landed.
fn navigate_rows(content: &TabContent, worktrees: Option<usize>) -> Vec<Row> {
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
    let others = worktrees.is_some_and(|count| count > 1);
    let switch = if others {
        "Jump to another worktree"
    } else {
        "No other worktrees"
    };
    vec![
        header("── Navigate ──"),
        row(
            "Browse folders",
            "Open a local git repo",
            true,
            OpenPick::Browse,
        ),
        row("Switch worktree", switch, others, OpenPick::Worktrees),
        row(
            "Open remote PR",
            "Review a GitHub PR by URL",
            true,
            OpenPick::RemoteUrl,
        ),
        row("Change compare base", &base, !remote, OpenPick::ChangeBase),
    ]
}

/// The Stack section, from the PR section's stack as `stack_hub_items`
/// maps the App's: nothing to read, a lookup on the way (`reading`), or the
/// stack's rows and "Refresh stack".
fn stack_rows(content: &TabContent, reading: bool) -> Vec<Row> {
    if er_service::stack_source_of(content).is_none() {
        let reason = "Not available — this branch isn't checked out";
        return vec![row("Stacked PRs", reason, false, OpenPick::Nothing)];
    }
    let Some(read) = content.pr.stack.as_ref().filter(|_| !reading) else {
        let looking = "Looking up the stack";
        return vec![row(
            "Loading stacked PRs…",
            looking,
            false,
            OpenPick::Nothing,
        )];
    };
    let mut rows: Vec<Row> = read
        .info
        .rows()
        .into_iter()
        .map(|layer| {
            let pick = match (layer.pr_number, layer.pr_url) {
                (Some(number), Some(url)) => OpenPick::StackPr(StackPr {
                    url,
                    number,
                    branch: layer.label.clone(),
                }),
                _ => OpenPick::Nothing,
            };
            let item = HubItem {
                label: layer.label,
                hint: layer.hint,
                description: layer.description,
                is_header: false,
                enabled: layer.enabled,
                opens_stack_pr: matches!(pick, OpenPick::StackPr(_)),
            };
            (item, pick)
        })
        .collect();
    let again = "Look up the stack again";
    rows.push(row("Refresh stack", again, true, OpenPick::RefreshStack));
    rows
}

/// The Current PR section.
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
            OpenPick::Browser,
        ),
    ]
}

impl OpenHub {
    /// The hub as the App opens it on this tab, on its first enabled row.
    /// `reading`: a stack lookup for the tab is on the way.
    pub fn open(content: &TabContent, reading: bool, worktrees: Option<usize>) -> Self {
        let mut rows = navigate_rows(content, worktrees);
        rows.push(header("── Stack ──"));
        rows.extend(stack_rows(content, reading));
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
            built: (content.key, content.pr.revision, reading, worktrees),
        }
    }

    /// Builds the rows again once the tab's PR section moved or a lookup
    /// landed, keeping the cursor, as the App does when its lookup lands.
    pub fn follow(&mut self, content: &TabContent, reading: bool, worktrees: Option<usize>) {
        let now = (content.key, content.pr.revision, reading, worktrees);
        if self.built == now || self.built.0 != content.key {
            return;
        }
        let selected = self.selected;
        *self = Self::open(content, reading, worktrees);
        if selected < self.rows.len() {
            self.selected = selected;
        }
    }

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
            // `b` opens an enabled layer in the browser.
            OverlayAction::SecondaryRow => {
                let enabled = self.rows.get(self.selected).is_some_and(|row| row.enabled);
                if let Some(OpenPick::StackPr(pr)) = self.picks.get(self.selected) {
                    if enabled {
                        return OpenStep::StackBrowser(pr.clone());
                    }
                }
            }
            OverlayAction::SelectRow => {
                let (Some(row), Some(pick)) =
                    (self.rows.get(self.selected), self.picks.get(self.selected))
                else {
                    return OpenStep::Close;
                };
                return match pick.clone() {
                    OpenPick::Nothing => OpenStep::Close,
                    _ if !row.enabled => OpenStep::Notify(row.description.clone()),
                    OpenPick::Browse => OpenStep::Browse,
                    OpenPick::Worktrees => OpenStep::Worktrees,
                    OpenPick::Browser => OpenStep::Browser,
                    OpenPick::ChangeBase => OpenStep::ChangeBase,
                    OpenPick::RemoteUrl => OpenStep::RemoteUrl,
                    OpenPick::StackPr(pr) => OpenStep::OpenPr(pr),
                    OpenPick::RefreshStack => OpenStep::RefreshStack,
                };
            }
            _ => {}
        }
        OpenStep::Stay
    }
}
