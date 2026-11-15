//! Selection moves over a tab's files, hunks and lines, as pure functions of
//! the current position and the tab's shape.
//!
//! The engine tab applies them, and keeps the side effects a move has there:
//! parsing the newly selected file, the hunk offsets, the scroll and the
//! notice. A front end that holds its own selection applies the same moves
//! to it, so both agree on where a key press lands.

/// What a file move selects. Each variant names what the move resets, since
/// the engine tab resets different fields on different branches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileMove {
    /// Nothing to move to.
    Stay,
    /// Selects the watched file at this index; the diff and horizontal
    /// scroll reset.
    Watched(usize),
    /// Leaves the watched section with no diff file to select.
    LeaveWatched,
    /// Selects this diff file and leaves the watched section. The hunk, line,
    /// line anchor and every scroll reset.
    File(usize),
    /// The selection is filtered out and snaps to this file, the first visible
    /// one. The hunk and the scrolls reset; the line and its anchor do not.
    Snap(usize),
}

/// The next file down: through the visible diff files, on into the watched
/// files, and back round to the first diff file.
///
/// `visible` and `watched` are the indices of the visible diff and watched
/// files, in display order.
pub fn next_file(
    selected_file: usize,
    selected_watched: Option<usize>,
    visible: &[usize],
    watched: &[usize],
) -> FileMove {
    if let Some(current) = selected_watched {
        let Some(pos) = watched.iter().position(|&idx| idx == current) else {
            return FileMove::Stay;
        };
        if let Some(&next) = watched.get(pos + 1) {
            return FileMove::Watched(next);
        }
        return visible
            .first()
            .map_or(FileMove::LeaveWatched, |&first| FileMove::File(first));
    }
    let Some(&first) = visible.first() else {
        return watched.first().map_or(FileMove::Stay, |&w| FileMove::Watched(w));
    };
    let Some(pos) = visible.iter().position(|&idx| idx == selected_file) else {
        return FileMove::Snap(first);
    };
    if let Some(&next) = visible.get(pos + 1) {
        return FileMove::File(next);
    }
    watched
        .first()
        .map_or(FileMove::File(first), |&w| FileMove::Watched(w))
}

/// The next file up: the mirror of [`next_file`]. From the first diff file it
/// wraps to the last watched file, or to the last diff file without one.
pub fn prev_file(
    selected_file: usize,
    selected_watched: Option<usize>,
    visible: &[usize],
    watched: &[usize],
) -> FileMove {
    if let Some(current) = selected_watched {
        let Some(pos) = watched.iter().position(|&idx| idx == current) else {
            return FileMove::Stay;
        };
        if pos > 0 {
            return FileMove::Watched(watched[pos - 1]);
        }
        return visible
            .last()
            .map_or(FileMove::LeaveWatched, |&last| FileMove::File(last));
    }
    let (Some(&first), Some(&last)) = (visible.first(), visible.last()) else {
        return FileMove::Stay;
    };
    let Some(pos) = visible.iter().position(|&idx| idx == selected_file) else {
        return FileMove::Snap(first);
    };
    if pos > 0 {
        return FileMove::File(visible[pos - 1]);
    }
    watched
        .last()
        .map_or(FileMove::File(last), |&w| FileMove::Watched(w))
}

/// The hunk after `current`, if the file has one.
pub fn next_hunk(current: usize, hunk_count: usize) -> Option<usize> {
    (current + 1 < hunk_count).then_some(current + 1)
}

/// The hunk before `current`, if there is one.
pub fn prev_hunk(current: usize) -> Option<usize> {
    current.checked_sub(1)
}

/// A line position: the hunk, and the line in it when line mode is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineCursor {
    pub hunk: usize,
    pub line: Option<usize>,
}

/// What a line move does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineMove {
    Stay,
    /// Moves the cursor here; the view scrolls to it.
    To(LineCursor),
    /// Leaves line mode where it is, without scrolling.
    Exit,
}

/// One line down, skipping fold markers, and on to the first line of the next
/// hunk at the end of one. `hunk_lines` holds each hunk's line count;
/// `is_fold(hunk, line)` says whether that line is a fold marker.
pub fn next_line(
    cursor: LineCursor,
    hunk_lines: &[usize],
    is_fold: impl Fn(usize, usize) -> bool,
) -> LineMove {
    let hunk = cursor.hunk;
    let total = hunk_lines.get(hunk).copied().unwrap_or(0);
    if total == 0 {
        return LineMove::Stay;
    }
    let first_after = |from: usize| (from..total).find(|&idx| !is_fold(hunk, idx));
    let next = match cursor.line {
        None => {
            return LineMove::To(LineCursor {
                hunk,
                line: first_after(0),
            })
        }
        Some(line) => first_after(line + 1),
    };
    if let Some(line) = next {
        return LineMove::To(LineCursor {
            hunk,
            line: Some(line),
        });
    }
    if hunk + 1 < hunk_lines.len() {
        return LineMove::To(LineCursor {
            hunk: hunk + 1,
            line: Some(0),
        });
    }
    LineMove::Stay
}

/// One line up, skipping fold markers. Entering line mode starts at the
/// hunk's last line; from a hunk's first line it moves to the previous hunk's
/// last, and from the file's first line it leaves line mode.
pub fn prev_line(
    cursor: LineCursor,
    hunk_lines: &[usize],
    is_fold: impl Fn(usize, usize) -> bool,
) -> LineMove {
    // The last line at or above `from` that is not a fold, scanning no
    // further than line 0, which is taken only when it is not a fold.
    let last_from = |hunk: usize, from: usize| {
        let mut idx = from;
        while idx > 0 && is_fold(hunk, idx) {
            idx -= 1;
        }
        (!is_fold(hunk, idx)).then_some(idx)
    };
    let count = |hunk: usize| hunk_lines.get(hunk).copied().unwrap_or(0);
    let hunk = cursor.hunk;
    match cursor.line {
        None => match count(hunk) {
            0 => LineMove::Stay,
            lines => LineMove::To(LineCursor {
                hunk,
                line: last_from(hunk, lines - 1),
            }),
        },
        Some(0) if hunk == 0 => LineMove::Exit,
        Some(0) => {
            let prev = hunk - 1;
            let line = match count(prev) {
                0 => None,
                lines => last_from(prev, lines - 1),
            };
            LineMove::To(LineCursor { hunk: prev, line })
        }
        Some(line) => LineMove::To(LineCursor {
            hunk,
            line: last_from(hunk, line - 1),
        }),
    }
}

/// The first unreviewed file after the selected one, wrapping, with the
/// selected file itself last. `visible` pairs each visible file's index with
/// whether it is reviewed. `None` when every visible file is reviewed or
/// there is none.
pub fn next_unreviewed_file(selected_file: usize, visible: &[(usize, bool)]) -> Option<usize> {
    let len = visible.len();
    let pos = visible
        .iter()
        .position(|&(idx, _)| idx == selected_file)
        .unwrap_or(0);
    (1..=len)
        .map(|offset| visible[(pos + offset) % len])
        .find(|&(_, reviewed)| !reviewed)
        .map(|(idx, _)| idx)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every branch of a file move, both ways. Diff files 0, 2 and 4 are
    /// visible (1 and 3 filtered out); watched files 0 and 1 follow them.
    #[test]
    fn file_moves_cover_every_section_transition() {
        use FileMove::{File, LeaveWatched, Snap, Stay, Watched};
        let visible = [0, 2, 4];
        let watched = [0, 1];
        type Case = (&'static str, usize, Option<usize>, &'static [usize], &'static [usize], FileMove, FileMove);
        let cases: [Case; 10] = [
            // name, file, watched sel, visible, watched, next, prev
            ("start", 0, None, &[0, 2, 4], &[0, 1], File(2), Watched(1)),
            ("middle", 2, None, &[0, 2, 4], &[0, 1], File(4), File(0)),
            ("end", 4, None, &[0, 2, 4], &[0, 1], Watched(0), File(2)),
            ("wrap without watched", 4, None, &[0, 2, 4], &[], File(0), File(2)),
            ("start without watched", 0, None, &[0, 2, 4], &[], File(2), File(4)),
            ("filtered out", 3, None, &[0, 2, 4], &[0, 1], Snap(0), Snap(0)),
            ("first watched", 4, Some(0), &[0, 2, 4], &[0, 1], Watched(1), File(4)),
            ("last watched", 4, Some(1), &[0, 2, 4], &[0, 1], File(0), Watched(0)),
            ("watched with no diff", 0, Some(1), &[], &[0, 1], LeaveWatched, Watched(0)),
            ("empty visible", 0, None, &[], &[0, 1], Watched(0), Stay),
        ];
        for (name, file, sel, vis, wat, next, prev) in cases {
            assert_eq!(next_file(file, sel, vis, wat), next, "next_file: {name}");
            assert_eq!(prev_file(file, sel, vis, wat), prev, "prev_file: {name}");
        }
        assert_eq!(next_file(0, None, &[], &[]), Stay, "nothing at all");
        assert_eq!(
            next_file(0, Some(7), &visible, &watched),
            Stay,
            "a watched selection that is filtered out stays"
        );
    }

    #[test]
    fn hunk_moves_stop_at_the_ends() {
        assert_eq!(next_hunk(0, 3), Some(1));
        assert_eq!(next_hunk(2, 3), None);
        assert_eq!(next_hunk(0, 0), None);
        assert_eq!(prev_hunk(1), Some(0));
        assert_eq!(prev_hunk(0), None);
    }

    /// Two hunks of four and three lines. Lines 1 and 3 of hunk 0 and line 0
    /// of hunk 1 are fold markers.
    #[test]
    fn line_moves_skip_folds_and_cross_hunks() {
        let hunks = [4, 3];
        let fold = |hunk: usize, line: usize| matches!((hunk, line), (0, 1) | (0, 3) | (1, 0));
        let at = |hunk, line| LineCursor { hunk, line };
        let to = |hunk, line| LineMove::To(LineCursor { hunk, line });
        let cases = [
            ("enter at the top", at(0, None), to(0, Some(0)), to(0, Some(2))),
            ("over a fold", at(0, Some(0)), to(0, Some(2)), LineMove::Exit),
            ("folds to the end", at(0, Some(2)), to(1, Some(0)), to(0, Some(0))),
            ("next hunk's first", at(1, Some(0)), to(1, Some(1)), to(0, Some(2))),
            ("last line of the file", at(1, Some(2)), LineMove::Stay, to(1, Some(1))),
            ("enter on a fold-led hunk", at(1, None), to(1, Some(1)), to(1, Some(2))),
        ];
        for (name, cursor, next, prev) in cases {
            assert_eq!(next_line(cursor, &hunks, fold), next, "next_line: {name}");
            assert_eq!(prev_line(cursor, &hunks, fold), prev, "prev_line: {name}");
        }
        assert_eq!(next_line(at(0, None), &[], fold), LineMove::Stay);
        assert_eq!(prev_line(at(0, None), &[0], fold), LineMove::Stay);
        assert_eq!(
            prev_line(at(1, Some(1)), &hunks, fold),
            to(1, None),
            "only folds above: line mode ends in place"
        );
    }

    #[test]
    fn next_unreviewed_scans_forward_and_wraps() {
        let visible = [(0, false), (2, true), (4, false), (6, true)];
        assert_eq!(next_unreviewed_file(0, &visible), Some(4), "skips reviewed");
        assert_eq!(next_unreviewed_file(4, &visible), Some(0), "wraps");
        assert_eq!(
            next_unreviewed_file(5, &visible),
            Some(4),
            "a filtered-out selection counts as the first file"
        );
        assert_eq!(
            next_unreviewed_file(0, &[(0, false), (2, true)]),
            Some(0),
            "the selected file comes last"
        );
        assert_eq!(next_unreviewed_file(0, &[(0, true), (2, true)]), None);
        assert_eq!(next_unreviewed_file(0, &[]), None);
    }
}
