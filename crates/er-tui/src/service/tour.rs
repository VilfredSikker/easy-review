//! The tour on the flag path (P72.2). Its pillars and files come from
//! er-service's tour builder, called again only when what it reads moves.
//! The place in the tour is the TUI's own, kept apart from the diff view's,
//! as the App keeps `TourState`. Space and `b` send the reviewed writes.

use std::collections::HashMap;

use er_engine::ai::strip_diff_links;
use er_engine::git::DiffFile;
use er_engine::tab_identity::DiffMode;
use er_service::{annotations_version, tour_resource, Command, TabContent};

use crate::input::keys::TourAction;
use crate::screen::{view, TourView};

/// One tab's tour rows and its place in them.
#[derive(Default)]
pub struct TourPos {
    /// The builder's version and the diff's revision the rows were built at.
    built: Option<(u64, u64)>,
    /// The mode load the position belongs to; a new entry starts it again.
    entered: Option<u64>,
    pillars: Vec<view::TourPillar>,
    /// Each row's file, by its index in the tab's diff.
    files: Vec<usize>,
    related: Vec<bool>,
    ranges: Vec<(usize, usize)>,
    pub selected_pillar: usize,
    pub selected_file: usize,
    pub current_hunk: usize,
    pub current_line: Option<usize>,
    pub diff_scroll: u16,
    pub h_scroll: u16,
    #[cfg(test)]
    pub builds: usize,
}

/// Builds the rows again when the tour, the marks, the mode or the diff
/// moved. A new entry into the tour, a new diff, or rows that changed put
/// the position on the selected pillar's first file, as the App's rebuild
/// does; a reviewed write moves none of these, and keeps the place.
pub fn follow(pos: &mut TourPos, content: &TabContent) {
    if content.tab.mode != DiffMode::Tour {
        return;
    }
    let at = (annotations_version(content), content.diff.revision);
    if pos.built == Some(at) {
        return;
    }
    let new_diff = pos.built.map(|(_, diff)| diff) != Some(at.1);
    pos.built = Some(at);
    #[cfg(test)]
    {
        pos.builds += 1;
    }
    let tour = tour_resource(content);
    let index: HashMap<&str, usize> = (content.diff.files.iter().enumerate())
        .map(|(at, file)| (file.path.as_str(), at))
        .collect();
    let (mut pillars, mut files, mut related, mut ranges) = (vec![], vec![], vec![], vec![]);
    for pillar in &tour.pillars {
        let start = files.len();
        for file in &pillar.files {
            let rows = std::iter::once((&file.path, false))
                .chain(file.related.iter().map(|r| (&r.path, true)));
            for (path, is_related) in rows {
                if let Some(&at) = index.get(path.as_str()) {
                    files.push(at);
                    related.push(is_related);
                }
            }
        }
        if files.len() == start {
            continue;
        }
        pillars.push(view::TourPillar {
            id: pillar.id.clone(),
            title: pillar.title.clone(),
            description: strip_diff_links(&pillar.description_markdown),
            importance: pillar.importance,
            foundation: pillar.foundation,
        });
        ranges.push((start, files.len()));
    }
    let entered = pos.entered != Some(content.mode_revision);
    let same = pos.pillars == pillars
        && pos.files == files
        && pos.related == related
        && pos.ranges == ranges;
    (pos.pillars, pos.files, pos.related, pos.ranges) = (pillars, files, related, ranges);
    if entered || new_diff || !same {
        pos.entered = Some(content.mode_revision);
        pos.selected_pillar = pos.selected_pillar.min(pos.pillars.len().saturating_sub(1));
        pos.selected_file = pos.ranges.get(pos.selected_pillar).map_or(0, |r| r.0);
        pos.current_hunk = 0;
        pos.current_line = None;
        pos.diff_scroll = 0;
        pos.h_scroll = 0;
    }
}

fn rows<'a>(pos: &TourPos, content: &'a TabContent) -> Vec<&'a DiffFile> {
    let files = &content.diff.files;
    (pos.files.iter())
        .filter_map(|&at| files.get(at).map(|file| &**file))
        .collect()
}

/// Runs a Tour key, as the App's `execute_tour`; answers the reviewed write
/// Space and `b` send.
pub fn key(pos: &mut TourPos, content: &TabContent, action: TourAction) -> Option<Command> {
    use TourAction as T;
    let files = rows(pos, content);
    match action {
        T::NextPillar if pos.selected_pillar + 1 < pos.pillars.len() => to_pillar(pos, &files, 1),
        T::PrevPillar if pos.selected_pillar > 0 => to_pillar(pos, &files, -1),
        T::NextFile if pos.selected_file + 1 < files.len() => to_file(pos, &files, 1),
        T::PrevFile if pos.selected_file > 0 => to_file(pos, &files, -1),
        T::NextLine => next_line(pos, &files),
        T::PrevLine => prev_line(pos, &files),
        T::ScrollRight => pos.h_scroll = pos.h_scroll.saturating_add(8),
        T::ScrollLeft => pos.h_scroll = pos.h_scroll.saturating_sub(8),
        T::ScrollHome => pos.h_scroll = 0,
        T::ScrollDown(rows) => scroll(pos, &files, pos.diff_scroll.saturating_add(rows)),
        T::ScrollUp(rows) => scroll(pos, &files, pos.diff_scroll.saturating_sub(rows)),
        T::ToggleReviewed => {
            return files
                .get(pos.selected_file)
                .map(|file| Command::ToggleReviewed {
                    context: content.context.clone(),
                    path: file.path.clone(),
                });
        }
        T::ReviewPillar => {
            return pos
                .pillars
                .get(pos.selected_pillar)
                .map(|pillar| Command::BulkReviewPillar {
                    context: content.context.clone(),
                    pillar_id: pillar.id.clone(),
                    reviewed: true,
                });
        }
        T::NextPillar | T::PrevPillar | T::NextFile | T::PrevFile => {}
    }
    None
}

fn to_pillar(pos: &mut TourPos, files: &[&DiffFile], by: isize) {
    pos.selected_pillar = pos.selected_pillar.saturating_add_signed(by);
    pos.selected_file = pos.ranges.get(pos.selected_pillar).map_or(0, |r| r.0);
    pos.current_hunk = 0;
    pos.current_line = None;
    scroll_to(pos, files, false);
}

fn to_file(pos: &mut TourPos, files: &[&DiffFile], by: isize) {
    pos.selected_file = pos.selected_file.saturating_add_signed(by);
    pos.current_hunk = 0;
    pos.current_line = None;
    follow_pillar(pos);
    scroll_to(pos, files, false);
}

fn pillar_of(pos: &TourPos, file: usize) -> Option<usize> {
    (pos.ranges.iter()).position(|&(start, end)| file >= start && file < end)
}

fn follow_pillar(pos: &mut TourPos) {
    if let Some(pillar) = pillar_of(pos, pos.selected_file) {
        pos.selected_pillar = pillar;
    }
}

fn lines(file: &DiffFile, hunk: usize) -> usize {
    file.hunks.get(hunk).map_or(0, |hunk| hunk.lines.len())
}

/// Down: the next line, on into the next hunk, then the next file.
fn next_line(pos: &mut TourPos, files: &[&DiffFile]) {
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
            follow_pillar(pos);
            Some(0)
        }
        Some(_) => return,
    };
    scroll_to(pos, files, true);
}

/// Up: the previous line, back into the previous hunk, then the last line of
/// the previous file.
fn prev_line(pos: &mut TourPos, files: &[&DiffFile]) {
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
            let prev = files[pos.selected_file];
            pos.current_hunk = prev.hunks.len().saturating_sub(1);
            follow_pillar(pos);
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
    scroll_to(pos, files, true);
}

/// Per file a header and a blank row, per hunk a header, its lines and a gap,
/// as the tour diff lays them out.
fn height(file: &DiffFile) -> usize {
    2 + file
        .hunks
        .iter()
        .map(|hunk| 2 + hunk.lines.len())
        .sum::<usize>()
}

/// Scrolls to the selected file's header, or with `cursor` to its line; a
/// place the files do not hold leaves the scroll.
fn scroll_to(pos: &mut TourPos, files: &[&DiffFile], cursor: bool) {
    let mut offset: usize = 0;
    for (idx, file) in files.iter().enumerate() {
        if idx == pos.selected_file && !cursor {
            pos.diff_scroll = u16::try_from(offset).unwrap_or(u16::MAX);
            return;
        }
        if idx == pos.selected_file && pos.current_hunk < file.hunks.len() {
            let before: usize = (file.hunks.iter().take(pos.current_hunk))
                .map(|hunk| 2 + hunk.lines.len())
                .sum();
            let row = offset + 2 + before + pos.current_line.unwrap_or(0);
            pos.diff_scroll = u16::try_from(row.saturating_sub(1)).unwrap_or(u16::MAX);
            return;
        }
        offset += height(file);
    }
}

/// A free scroll; the pillar follows the row at the top.
fn scroll(pos: &mut TourPos, files: &[&DiffFile], to: u16) {
    pos.diff_scroll = to;
    if let Some(pillar) = pillar_at_scroll(pos, files) {
        pos.selected_pillar = pillar;
    }
}

/// The pillar whose files hold the row at the top of the scroll, or the last.
fn pillar_at_scroll(pos: &TourPos, files: &[&DiffFile]) -> Option<usize> {
    let target = usize::from(pos.diff_scroll);
    let mut row = 0;
    for (idx, file) in files.iter().enumerate() {
        row += height(file);
        if target < row {
            return pillar_of(pos, idx);
        }
    }
    pos.ranges.len().checked_sub(1)
}

/// The view Tour draws while the tab shows it and has a tour.
pub fn view<'a>(pos: &'a TourPos, content: &'a TabContent) -> Option<TourView<'a>> {
    if content.tab.mode != DiffMode::Tour || content.ai.state.tour.is_none() {
        return None;
    }
    Some(TourView {
        pillars: &pos.pillars,
        selected_pillar: pos.selected_pillar,
        files: rows(pos, content),
        file_is_related: &pos.related,
        pillar_file_ranges: &pos.ranges,
        selected_file: pos.selected_file,
        current_hunk: pos.current_hunk,
        current_line: pos.current_line,
        diff_scroll: pos.diff_scroll,
        h_scroll: pos.h_scroll,
    })
}
