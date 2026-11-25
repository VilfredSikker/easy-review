//! The flag-on review hub (P68.2, D3, ADR 0049): the viewer's pending review
//! on the tab's PR, its comments, a decision and a summary, submitted
//! through the actor. GitHub holds the draft, so the hub shows what the last
//! reply read back, and reads again when another write changes it.

use er_engine::github::PendingReview;
use er_engine::TabKey;
use er_service::{Command, ReviewEvent, TabContent, TabContext};

use crate::input::{Edit, OverlayAction};
use crate::screen::view::HubItem;

/// What a hub row does when chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Pick {
    Header,
    /// Asks to remove the comment with this node id.
    Remove(String),
    Decision,
    Summary,
    Submit,
}

/// One tab's review hub, open while the overlay or its summary input shows.
pub struct ReviewHub {
    pub key: TabKey,
    pub workspace: String,
    /// The bucket the tab's PR comments live in, which names its PR.
    comments_dir: String,
    review: Option<PendingReview>,
    event: ReviewEvent,
    pub summary: String,
    /// The summary as it was before the input opened, for Esc.
    summary_before: String,
    pub rows: Vec<HubItem>,
    picks: Vec<Pick>,
    pub selected: usize,
    /// The comment the remove prompt asks about.
    pub removing: Option<String>,
}

/// What a key on the hub asks the loop to do.
pub enum HubStep {
    Stay,
    Close,
    AskRemove,
    EditSummary,
    Submit,
}

fn row(label: String, description: &str, is_header: bool) -> HubItem {
    HubItem {
        label,
        hint: String::new(),
        description: description.into(),
        is_header,
        enabled: true,
        opens_stack_pr: false,
    }
}

fn first_line(text: &str, max: usize) -> String {
    let line = text.lines().next().unwrap_or_default();
    if line.chars().count() <= max {
        return line.to_string();
    }
    let kept: String = line.chars().take(max.saturating_sub(1)).collect();
    format!("{kept}…")
}

impl ReviewHub {
    /// The hub on `content`'s tab, before its first read lands.
    pub fn open(key: TabKey, workspace: &str, content: &TabContent) -> Self {
        let mut hub = Self {
            key,
            workspace: workspace.to_string(),
            comments_dir: content.context.comments_dir.clone(),
            review: None,
            event: ReviewEvent::Comment,
            summary: String::new(),
            summary_before: String::new(),
            rows: Vec::new(),
            picks: Vec::new(),
            selected: 0,
            removing: None,
        };
        hub.build();
        hub
    }

    pub fn loaded(&self) -> bool {
        self.review.is_some()
    }

    /// A read-back landed: the list is GitHub's, the selection stays on its
    /// row where it can.
    pub fn load(&mut self, review: PendingReview) {
        self.review = Some(review);
        self.build();
    }

    /// Whether `context`'s change is this hub's PR: the same workspace and
    /// the same comments bucket.
    pub fn watches(&self, context: &TabContext) -> bool {
        context.workspace_key() == self.workspace && context.comments_dir == self.comments_dir
    }

    fn build(&mut self) {
        let comments = self.review.as_ref().map(|review| &review.comments[..]);
        let title = match comments {
            None => "Pending review: reading…".to_string(),
            Some(comments) => format!("Pending review: {} comments", comments.len()),
        };
        let mut rows = vec![(row(title, "", true), Pick::Header)];
        for comment in comments.unwrap_or_default() {
            let line = comment
                .line
                .map_or_else(String::new, |line| format!(":{line}"));
            let label = format!("{}{line}", comment.path);
            let body = first_line(&comment.body, 40);
            rows.push((row(label, &body, false), Pick::Remove(comment.id.clone())));
        }
        let decision = match self.event {
            ReviewEvent::Comment => "Comment",
            ReviewEvent::Approve => "Approve",
            ReviewEvent::RequestChanges => "Request changes",
        };
        let summary = match self.summary.trim() {
            "" => "(none)".to_string(),
            text => first_line(text, 30),
        };
        rows.extend([
            (row("Submit".into(), "", true), Pick::Header),
            (
                row(format!("Decision: {decision}"), "Enter changes it", false),
                Pick::Decision,
            ),
            (
                row(format!("Summary: {summary}"), "Enter writes it", false),
                Pick::Summary,
            ),
            (
                row("Submit review".into(), "Send it to GitHub", false),
                Pick::Submit,
            ),
        ]);
        let was = self.picks.get(self.selected).cloned();
        (self.rows, self.picks) = rows.into_iter().unzip();
        let at = was.and_then(|was| self.picks.iter().position(|pick| *pick == was));
        self.selected = at.unwrap_or(self.selected).min(self.picks.len() - 1);
        if self.picks[self.selected] == Pick::Header {
            self.step(true);
        }
    }

    /// Moves the selection past the headers, staying put at either end.
    fn step(&mut self, forward: bool) {
        let mut at = self.selected;
        loop {
            at = match (forward, at) {
                (true, at) if at + 1 < self.picks.len() => at + 1,
                (false, at) if at > 0 => at - 1,
                _ => return,
            };
            if self.picks[at] != Pick::Header {
                self.selected = at;
                return;
            }
        }
    }

    /// Runs one key on the hub.
    pub fn act(&mut self, action: OverlayAction) -> HubStep {
        match action {
            OverlayAction::Next => self.step(true),
            OverlayAction::Prev => self.step(false),
            OverlayAction::Close => return HubStep::Close,
            OverlayAction::SelectRow => match self.picks.get(self.selected).cloned() {
                Some(Pick::Remove(id)) => {
                    self.removing = Some(id);
                    return HubStep::AskRemove;
                }
                Some(Pick::Decision) => {
                    self.event = match self.event {
                        ReviewEvent::Comment => ReviewEvent::Approve,
                        ReviewEvent::Approve => ReviewEvent::RequestChanges,
                        ReviewEvent::RequestChanges => ReviewEvent::Comment,
                    };
                    self.build();
                }
                Some(Pick::Summary) => {
                    self.summary_before.clone_from(&self.summary);
                    return HubStep::EditSummary;
                }
                Some(Pick::Submit) => return HubStep::Submit,
                Some(Pick::Header) | None => {}
            },
            _ => {}
        }
        HubStep::Stay
    }

    /// Runs one key in the summary input; answers whether it closed. Esc
    /// puts the summary back as it was.
    pub fn summary_key(&mut self, edit: Edit) -> bool {
        match edit {
            Edit::Insert(c) => self.summary.push(c),
            Edit::Backspace => {
                self.summary.pop();
            }
            Edit::Left | Edit::Right => {}
            Edit::Submit => {}
            Edit::Cancel => self.summary.clone_from(&self.summary_before),
        }
        let done = matches!(edit, Edit::Submit | Edit::Cancel);
        if done {
            self.build();
        }
        done
    }

    /// One submit covers both cases: the service submits the draft when one
    /// exists, and otherwise posts the decision with the summary.
    pub fn submit(&self, content: &TabContent) -> Command {
        Command::SubmitPendingReview {
            context: content.context.clone(),
            event: self.event,
            summary: self.summary.clone(),
        }
    }
}

pub fn read(content: &TabContent) -> Command {
    Command::GetPendingReview {
        context: content.context.clone(),
    }
}

pub fn remove(content: &TabContent, comment_node_id: String) -> Command {
    Command::RemovePendingReviewComment {
        context: content.context.clone(),
        comment_node_id,
    }
}
