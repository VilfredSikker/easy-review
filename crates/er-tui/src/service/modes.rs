//! The flag-on mode keys, `R`, the mtime sort and the base change (P71.1).
//! Each sends its service command at the press; when the reply lands the
//! selection follows the App's rule for that step.

use er_engine::tab_identity::DiffMode;
use er_engine::TabKey;
use er_service::{Command, TabContent};

use super::tui_view::TabViewState;
use crate::input::keys::Edit;
use crate::screen::UNPORTED;

/// Where the selection stood when a mode switch or a refresh went out.
pub struct Follow {
    path: Option<String>,
    hunk: usize,
    line: Option<usize>,
    scroll: u16,
}

impl Follow {
    pub fn of(state: &TabViewState, content: &TabContent) -> Self {
        let path = content.diff.files.get(state.selected_file);
        Self {
            path: path.map(|file| file.path.clone()),
            hunk: state.current_hunk,
            line: state.current_line,
            scroll: state.diff_scroll,
        }
    }
}

/// The command a mode key sends, nothing when the tab already shows that
/// mode, or the message for a mode another entry ports.
pub fn switch(
    key: TabKey,
    content: &TabContent,
    mode: DiffMode,
) -> Result<Option<Command>, String> {
    let tab = &content.tab;
    match mode {
        _ if mode == tab.mode => Ok(None),
        DiffMode::History | DiffMode::Conflicts | DiffMode::Hidden | DiffMode::Tour => {
            Err(format!("That view is {UNPORTED}"))
        }
        _ => Ok(Some(Command::SetDiffMode {
            key,
            mode,
            pr_number: (mode == DiffMode::PrDiff)
                .then_some(tab.pr_number)
                .flatten(),
        })),
    }
}

fn hunks(content: &TabContent, index: usize) -> usize {
    content
        .diff
        .files
        .get(index)
        .map_or(0, |file| file.hunks.len())
}

/// A mode switch landed: the file selected before it stays selected when the
/// new view lists it, with its hunk and line while it still has that hunk,
/// else the first file; then the snap, then the scroll to the hunk, as the
/// App's `set_mode`. Answers the file it selected.
pub fn switched(
    state: &mut TabViewState,
    follow: Follow,
    content: &TabContent,
    shown: bool,
) -> Option<usize> {
    state.current_hunk = 0;
    state.current_line = None;
    state.watched.selected = None;
    state.diff_scroll = 0;
    let files = &content.diff.files;
    let at = follow
        .path
        .and_then(|path| files.iter().position(|file| file.path == path));
    state.selected_file = at.unwrap_or(0);
    if at.is_some() && follow.hunk < hunks(content, state.selected_file) {
        state.current_hunk = follow.hunk;
        state.current_line = follow.line;
    }
    state.snap(content, shown);
    state.scroll_to_cursor(files.get(state.selected_file).map(|file| &**file));
    if let Some(saved) = state
        .restore
        .take_if(|saved| saved.mode == content.tab.mode)
    {
        super::session::restore_in_mode(state, saved.session, content);
    }
    Some(state.selected_file)
}

/// A refresh landed: the file selected before it stays selected, at its hunk,
/// line and scroll; a file the diff dropped leaves the selection in range,
/// as the App's refresh.
pub fn refreshed(state: &mut TabViewState, follow: Follow, content: &TabContent) -> Option<usize> {
    let files = &content.diff.files;
    let had_file = follow.path.is_some();
    let at = follow
        .path
        .and_then(|path| files.iter().position(|file| file.path == path));
    state.selected_file = match at {
        Some(at) => at,
        None if had_file => state.selected_file.min(files.len().saturating_sub(1)),
        None => 0,
    };
    let total = hunks(content, state.selected_file);
    if at.is_some() && follow.hunk < total {
        state.current_hunk = follow.hunk;
        state.current_line = follow.line;
    } else if total == 0 {
        state.current_hunk = 0;
        state.current_line = None;
    } else {
        state.current_hunk = state.current_hunk.min(total - 1);
    }
    if at.is_some() {
        state.diff_scroll = follow.scroll;
    } else {
        state.scroll_to_cursor(files.get(state.selected_file).map(|file| &**file));
    }
    Some(state.selected_file)
}

/// What the base input asks once a key ran.
pub enum BaseStep {
    Stay,
    Close,
    Send(String),
}

/// One key in the base input, as the App's: Enter sends a name that is not
/// blank, Esc drops the text.
pub fn base_key(text: &mut String, edit: Edit) -> BaseStep {
    match edit {
        Edit::Submit => {
            let base = std::mem::take(text).trim().to_string();
            if base.is_empty() {
                BaseStep::Close
            } else {
                BaseStep::Send(base)
            }
        }
        Edit::Cancel => {
            text.clear();
            BaseStep::Close
        }
        Edit::Insert(c) => {
            text.push(c);
            BaseStep::Stay
        }
        Edit::Backspace => {
            text.pop();
            BaseStep::Stay
        }
        Edit::Left | Edit::Right => BaseStep::Stay,
    }
}

/// The App's two refresh notices: the marks a change cleared, else the
/// files that changed. A refresh on re-arming counts no files, and says
/// nothing unless it cleared a mark.
pub fn watch_notice(cleared: usize, files: usize) -> Option<String> {
    let plural = |n: usize| if n == 1 { "" } else { "s" };
    if cleared > 0 {
        Some(format!(
            "{cleared} reviewed file{} auto-unmarked (diff changed)",
            plural(cleared)
        ))
    } else {
        (files > 0).then(|| format!("{files} file{} changed", plural(files)))
    }
}
