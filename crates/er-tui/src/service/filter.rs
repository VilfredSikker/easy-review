//! The file filter, its presets and history, and the file search, on the
//! flag path's view state. Each runs as the App's does, over the pure
//! `visible_files`, and snaps the selection after a change the App snaps on.

use er_engine::filter::FILTER_PRESETS;
use er_engine::TabKey;
use er_service::TabContent;

use super::tui_view::{Mode, Overlay, TuiView};
use crate::input::keys::{Edit, NormalAction, OverlayAction};

/// What a key did: the status-line message, and a diff file the snap
/// selected, which may be a stub to parse.
#[derive(Default)]
pub struct Done {
    pub notice: Option<String>,
    pub selected: Option<usize>,
}

/// Runs a Normal-mode filter or search key; `None` when `action` is not one.
pub fn normal(
    view: &mut TuiView,
    key: TabKey,
    content: &TabContent,
    shown: bool,
    action: NormalAction,
) -> Option<Done> {
    use NormalAction as A;
    let state = view.tab_mut(key, content);
    let mut done = Done::default();
    match action {
        A::StartSearch => {
            state.search_query.clear();
            view.input_mode = Mode::Search;
        }
        A::StartFilter => {
            state.filter_input.clone_from(&state.filter_expr);
            view.input_mode = Mode::Filter;
        }
        A::OpenFilterHistory => {
            let history = state.filter_history.clone();
            view.overlay = Some(Overlay::FilterHistory {
                history,
                selected: 0,
            });
        }
        A::ClearSearch => state.search_query.clear(),
        A::ClearFilter => {
            done.selected = state.clear_filter(content, shown);
            done.notice = Some("Filter cleared".into());
        }
        _ => return None,
    }
    Some(done)
}

/// A key typed into the search: Enter keeps the query and snaps, Esc drops it.
pub fn search(
    view: &mut TuiView,
    key: TabKey,
    content: &TabContent,
    shown: bool,
    edit: Edit,
) -> Done {
    let state = view.tab_mut(key, content);
    let mut done = Done::default();
    match edit {
        Edit::Submit => {
            done.selected = state.snap(content, shown);
            view.input_mode = Mode::Normal;
        }
        Edit::Cancel => {
            state.search_query.clear();
            view.input_mode = Mode::Normal;
        }
        Edit::Insert(c) => state.search_query.push(c),
        Edit::Backspace => {
            state.search_query.pop();
        }
        _ => {}
    }
    done
}

/// A key typed into the filter: Enter applies it and says how many files it
/// keeps, Esc leaves the applied filter as it was.
pub fn filter(
    view: &mut TuiView,
    key: TabKey,
    content: &TabContent,
    shown: bool,
    edit: Edit,
) -> Done {
    let state = view.tab_mut(key, content);
    let mut done = Done::default();
    match edit {
        Edit::Submit => {
            let expr = state.filter_input.clone();
            done.selected = state.apply_filter(content, &expr, shown);
            let expr = expr.trim();
            done.notice = Some(if expr.is_empty() {
                "Filter cleared".into()
            } else {
                let (visible, total) = (state.visible(content).len(), content.diff.files.len());
                format!("Filter: {expr} ({visible}/{total})")
            });
            view.input_mode = Mode::Normal;
        }
        Edit::Cancel => {
            state.filter_input.clear();
            view.input_mode = Mode::Normal;
        }
        Edit::Insert(c) => state.filter_input.push(c),
        Edit::Backspace => {
            state.filter_input.pop();
        }
        _ => {}
    }
    done
}

/// A key on the history overlay: its rows are the presets, then the
/// history. Enter closes it and applies the row's filter.
pub fn history(
    view: &mut TuiView,
    key: TabKey,
    content: &TabContent,
    shown: bool,
    action: OverlayAction,
) -> Done {
    let mut done = Done::default();
    let Some(Overlay::FilterHistory { history, selected }) = view.overlay.as_mut() else {
        return done;
    };
    match action {
        OverlayAction::Next if *selected + 1 < FILTER_PRESETS.len() + history.len() => {
            *selected += 1;
        }
        OverlayAction::Prev => *selected = selected.saturating_sub(1),
        OverlayAction::Close => view.overlay = None,
        OverlayAction::SelectRow => {
            let expr = match FILTER_PRESETS.get(*selected) {
                Some(preset) => Some(preset.expr.to_string()),
                None => history.get(*selected - FILTER_PRESETS.len()).cloned(),
            };
            view.overlay = None;
            if let Some(expr) = expr {
                let state = view.tab_mut(key, content);
                done.selected = state.apply_filter(content, &expr, shown);
                done.notice = Some(format!("Filter: {expr}"));
            }
        }
        _ => {}
    }
    done
}
