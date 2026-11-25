//! The chrome keys (P75.3): the Help hub, the two layout widths and the
//! split diff's focus side, each by the App's rules.

use crate::input::OverlayAction;
use crate::screen::view::{HubItem, SplitSide};

use super::tui_view::TabViewState;

/// The Help hub: the engine's rows, none of which runs anything.
pub struct HelpHub {
    pub rows: Vec<HubItem>,
    pub selected: usize,
}

impl HelpHub {
    /// Opens on the first row, a header, as the App's does.
    pub fn open() -> Self {
        let rows = er_engine::help::ROWS
            .iter()
            .map(|&(label, description, is_header)| HubItem {
                label: label.into(),
                hint: String::new(),
                description: description.into(),
                is_header,
                enabled: false,
                opens_stack_pr: false,
            })
            .collect();
        Self { rows, selected: 0 }
    }

    /// Moves past headers as the App's `overlay_next` and `overlay_prev`.
    /// Answers whether the hub stays open: Enter closes it, since no row
    /// runs anything.
    pub fn act(&mut self, action: OverlayAction) -> bool {
        let header = |at: usize| self.rows.get(at).is_some_and(|row| row.is_header);
        match action {
            OverlayAction::Next => {
                let mut next = self.selected + 1;
                while next < self.rows.len() && header(next) {
                    next += 1;
                }
                if next < self.rows.len() {
                    self.selected = next;
                }
            }
            OverlayAction::Prev if self.selected > 0 => {
                let mut prev = self.selected - 1;
                while prev > 0 && header(prev) {
                    prev -= 1;
                }
                if !header(prev) {
                    self.selected = prev;
                }
            }
            OverlayAction::Close | OverlayAction::SelectRow => return false,
            _ => {}
        }
        true
    }
}

/// `<` and `>`: the file tree's width, held to 16..=60 and to a diff at
/// least 20 columns wide, as the App's `resize_file_tree`.
pub fn resize_tree(state: &mut TabViewState, delta: i16, terminal_width: u16) {
    let width = clamp(state.file_tree_width, delta, 16, 60);
    let panel = if state.panel.is_some() {
        state.panel_width
    } else {
        0
    };
    if terminal_width.saturating_sub(width + panel) >= 20 {
        state.file_tree_width = width;
    }
}

/// `{` and `}`: the side panel's width, held to 24..=80 and to the same
/// diff minimum, as the App's `resize_panel`.
pub fn resize_panel(state: &mut TabViewState, delta: i16, terminal_width: u16) {
    let width = clamp(state.panel_width, delta, 24, 80);
    if terminal_width.saturating_sub(state.file_tree_width + width) >= 20 {
        state.panel_width = width;
    }
}

fn clamp(width: u16, delta: i16, min: i16, max: i16) -> u16 {
    let width = i16::try_from(width).unwrap_or(max);
    u16::try_from(width.saturating_add(delta).clamp(min, max)).unwrap_or_default()
}

/// The side the split diff's cursor is on, when the split diff shows: it is
/// on in Settings and no side panel is open, as the App's
/// `split_diff_active`.
pub fn split_side(state: &TabViewState, split_diff: bool) -> Option<SplitSide> {
    (split_diff && state.panel.is_none()).then_some(state.split_focus)
}

/// Tab in the split diff: the cursor moves to the other side.
pub fn toggle_split_focus(state: &mut TabViewState) {
    state.split_focus = match state.split_focus {
        SplitSide::Old => SplitSide::New,
        SplitSide::New => SplitSide::Old,
    };
}

/// `l`, `h` and Home in the split diff move the focused side's scroll by
/// one column, as the App's `scroll_right_split` and `scroll_left_split`;
/// Home also resets the shared scroll.
pub fn scroll_split(state: &mut TabViewState, side: SplitSide, by: Option<i16>) {
    let scroll = match side {
        SplitSide::Old => &mut state.h_scroll_old,
        SplitSide::New => &mut state.h_scroll_new,
    };
    match by {
        Some(1) => *scroll = scroll.saturating_add(1),
        Some(_) => *scroll = scroll.saturating_sub(1),
        None => {
            *scroll = 0;
            state.h_scroll = 0;
        }
    }
}
