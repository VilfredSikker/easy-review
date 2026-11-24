//! History on the flag path (P71.2). The commits and the selected commit's
//! files come from the service's History section and diff; `k` and `j` send
//! `SelectCommit`, and `k` past the last commit sends `LoadMoreHistory`, one
//! at a time per tab, the presses waiting their turn in order. The
//! position in the commit's files is the TUI's own, kept apart from the diff
//! view's, as the App keeps `HistoryState`.

use std::collections::VecDeque;
use std::sync::Arc;

use er_engine::git::DiffFile;
use er_engine::tab_identity::DiffMode;
use er_engine::TabKey;
use er_service::{Command, TabContent};

use super::modes::Follow;
use crate::input::keys::HistoryAction;
use crate::screen::HistoryView;

/// One tab's place in History.
#[derive(Default)]
pub struct HistoryPos {
    /// The commit and source the position below belongs to; a load that
    /// shows another puts it back at the top, as the App's commit load does.
    shown: Option<(Option<String>, DiffMode)>,
    pub selected_file: usize,
    pub current_hunk: usize,
    pub current_line: Option<usize>,
    pub diff_scroll: u16,
    pub h_scroll: u16,
    /// The `k` (true) and `j` presses not yet shown, oldest first; the front
    /// one leaves when its commit lands.
    pub queued: VecDeque<bool>,
    /// Whether a commit load or a page is in flight.
    pub in_flight: bool,
    /// Where the diff view stood when History was entered, which leaving
    /// History returns to.
    pub left: Option<Follow>,
}

impl HistoryPos {
    /// A load showed another commit, or History listed from another source:
    /// the position goes back to the top.
    pub fn follow(&mut self, content: &TabContent) {
        let Some(history) = content.history.as_deref() else {
            return;
        };
        if content.tab.mode != DiffMode::History {
            return;
        }
        let shown = (history.selected.clone(), history.return_mode);
        if self.shown.as_ref() != Some(&shown) {
            self.shown = Some(shown);
            self.top(0);
        }
    }

    fn top(&mut self, file: usize) {
        self.selected_file = file;
        self.current_hunk = 0;
        self.current_line = None;
        self.diff_scroll = 0;
        self.h_scroll = 0;
    }
}

/// Runs a History key, as the App's `execute_history`; answers the command
/// a commit move sends.
pub fn key(
    pos: &mut HistoryPos,
    key: TabKey,
    content: &TabContent,
    action: HistoryAction,
) -> Option<Command> {
    use HistoryAction as H;
    let files = &content.diff.files;
    match action {
        H::NextCommit | H::PrevCommit => {
            pos.queued.push_back(action == H::NextCommit);
            return pump(pos, key, content);
        }
        H::NextFile if pos.selected_file + 1 < files.len() => to_file(pos, files, 1),
        H::PrevFile if pos.selected_file > 0 => to_file(pos, files, -1),
        H::NextLine => next_line(pos, files),
        H::PrevLine => prev_line(pos, files),
        H::ScrollRight => pos.h_scroll = pos.h_scroll.saturating_add(8),
        H::ScrollLeft => pos.h_scroll = pos.h_scroll.saturating_sub(8),
        H::ScrollHome => pos.h_scroll = 0,
        H::ScrollDown(rows) => pos.diff_scroll = pos.diff_scroll.saturating_add(rows),
        H::ScrollUp(rows) => pos.diff_scroll = pos.diff_scroll.saturating_sub(rows),
        H::NextFile | H::PrevFile => {}
    }
    None
}

/// The next press's command once nothing is in flight: the commit after or
/// before the one shown, else History's next page for a `k` past the last
/// commit. A press with nowhere to go is dropped, as on the App, and
/// leaving History drops them all.
pub fn pump(pos: &mut HistoryPos, key: TabKey, content: &TabContent) -> Option<Command> {
    if content.tab.mode != DiffMode::History {
        pos.queued.clear();
    }
    let history = content.history.as_deref()?;
    while let Some(&forward) = pos.queued.front().filter(|_| !pos.in_flight) {
        let selected = history.selected.as_deref();
        let commits = &history.commits;
        let index = commits
            .iter()
            .position(|c| Some(c.hash.as_str()) == selected);
        let index = index.unwrap_or(0);
        let to = if forward {
            index.checked_add(1)
        } else {
            index.checked_sub(1)
        };
        if let Some(commit) = to.and_then(|to| commits.get(to)) {
            pos.in_flight = true;
            let sha = commit.hash.clone();
            return Some(Command::SelectCommit { key, sha });
        }
        if forward && !history.all_loaded {
            pos.in_flight = true;
            return Some(Command::LoadMoreHistory { key });
        }
        pos.queued.pop_front();
    }
    None
}

fn to_file(pos: &mut HistoryPos, files: &[Arc<DiffFile>], by: isize) {
    pos.selected_file = pos.selected_file.saturating_add_signed(by);
    pos.current_hunk = 0;
    pos.current_line = None;
    pos.diff_scroll = scroll_to(files, pos.selected_file, None);
}

fn lines(file: &DiffFile, hunk: usize) -> usize {
    file.hunks.get(hunk).map_or(0, |hunk| hunk.lines.len())
}

/// Down: the next line, on into the next hunk, then the next file.
fn next_line(pos: &mut HistoryPos, files: &[Arc<DiffFile>]) {
    let Some(file) = files.get(pos.selected_file) else {
        return;
    };
    let count = lines(file, pos.current_hunk);
    pos.current_line = match pos.current_line {
        None if count > 0 => Some(0),
        None => return,
        Some(line) if line + 1 < count => Some(line + 1),
        Some(_) if pos.current_hunk + 1 < file.hunks.len() => {
            pos.current_hunk += 1;
            Some(0)
        }
        Some(_) if pos.selected_file + 1 < files.len() => {
            pos.selected_file += 1;
            pos.current_hunk = 0;
            Some(0)
        }
        Some(_) => return,
    };
    pos.diff_scroll = scroll_to(files, pos.selected_file, Some(&*pos));
}

/// Up: the previous line, back into the previous hunk, then the last line of
/// the previous file.
fn prev_line(pos: &mut HistoryPos, files: &[Arc<DiffFile>]) {
    let Some(file) = files.get(pos.selected_file) else {
        return;
    };
    let last = |file: &DiffFile, hunk: usize| lines(file, hunk).checked_sub(1);
    pos.current_line = match pos.current_line {
        None => match last(file, pos.current_hunk) {
            Some(line) => Some(line),
            None => return,
        },
        Some(0) if pos.current_hunk > 0 => {
            pos.current_hunk -= 1;
            last(file, pos.current_hunk)
        }
        Some(0) if pos.selected_file > 0 => {
            pos.selected_file -= 1;
            let prev = &files[pos.selected_file];
            pos.current_hunk = prev.hunks.len().saturating_sub(1);
            prev.hunks
                .last()
                .and_then(|hunk| hunk.lines.len().checked_sub(1))
        }
        Some(0) => {
            pos.current_line = None;
            return;
        }
        Some(line) => Some(line - 1),
    };
    pos.diff_scroll = scroll_to(files, pos.selected_file, Some(&*pos));
}

/// The scroll that shows `file`'s header, or with `cursor` its line: per
/// file a header and a blank row, per hunk a header, its lines and a gap,
/// as the History view lays them out.
fn scroll_to(files: &[Arc<DiffFile>], file: usize, cursor: Option<&HistoryPos>) -> u16 {
    let mut offset: usize = 0;
    for (idx, each) in files.iter().enumerate() {
        if idx == file && cursor.is_none() {
            break;
        }
        offset += 2;
        for (hunk_idx, hunk) in each.hunks.iter().enumerate() {
            if let Some(pos) = cursor.filter(|pos| idx == file && hunk_idx == pos.current_hunk) {
                offset = (offset + pos.current_line.unwrap_or(0)).saturating_sub(1);
                return u16::try_from(offset).unwrap_or(u16::MAX);
            }
            offset += 1 + hunk.lines.len() + 1;
        }
    }
    match cursor {
        Some(pos) => pos.diff_scroll,
        None => u16::try_from(offset).unwrap_or(u16::MAX),
    }
}

/// The view History draws while the tab shows it: the service's commits,
/// those the search leaves, and the selected commit's files.
pub fn view<'a>(
    pos: &HistoryPos,
    content: &'a TabContent,
    search: &str,
) -> Option<HistoryView<'a>> {
    if content.tab.mode != DiffMode::History {
        return None;
    }
    let history = content.history.as_deref()?;
    let commits = &history.commits;
    let query = search.to_lowercase();
    let visible_commits = commits
        .iter()
        .enumerate()
        .filter(|(_, c)| {
            query.is_empty()
                || c.subject.to_lowercase().contains(&query)
                || c.short_hash.contains(&query)
                || c.author.to_lowercase().contains(&query)
        })
        .collect();
    let selected = history.selected.as_deref();
    Some(HistoryView {
        commits,
        visible_commits,
        selected_commit: commits
            .iter()
            .position(|c| Some(c.hash.as_str()) == selected)
            .unwrap_or(0),
        commit_files: content.diff.files.iter().map(|file| &**file).collect(),
        selected_file: pos.selected_file,
        current_hunk: pos.current_hunk,
        current_line: pos.current_line,
        diff_scroll: pos.diff_scroll,
        h_scroll: pos.h_scroll,
    })
}
