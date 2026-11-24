//! The flag-on context-line keys and the automatic pick (P71.3). The counts
//! are the App's, from `er_engine::git`; the service fetches the file.

use er_engine::git::{self, DiffFile, DEFAULT_CONTEXT_LINES};
use er_engine::tab_identity::DiffMode;
use er_engine::TabKey;
use er_service::{Command, TabContent};

use super::tui_view::TabViewState;

/// What a `+` or `-` asks for.
pub enum Step {
    Send(Command),
    Notice(String),
    Nothing,
}

/// The count `path` shows once the sends in flight land.
fn current(state: &TabViewState, content: &TabContent, path: &str) -> Option<usize> {
    let sent = state.context_sent.get(path);
    sent.or_else(|| content.diff.context.get(path)).copied()
}

fn send(key: TabKey, state: &mut TabViewState, path: &str, lines: usize) -> Command {
    state.context_sent.insert(path.to_string(), lines);
    let path = path.to_string();
    Command::SetFileContext { key, path, lines }
}

/// `+` or `-` on the selected file, with the App's guards.
pub fn step(
    key: TabKey,
    state: &mut TabViewState,
    content: &TabContent,
    file: Option<&DiffFile>,
    expand: bool,
) -> Step {
    if content.tab.identity().is_remote() {
        return Step::Notice("Context expansion not available in remote mode".into());
    }
    let Some(file) = file.filter(|_| content.tab.mode != DiffMode::History) else {
        return Step::Nothing;
    };
    if file.compacted {
        // `+` on a compacted file expands it, as Enter does.
        return if expand {
            let paths = vec![file.path.clone()];
            Step::Send(Command::ParseFiles { key, paths, expand })
        } else {
            Step::Nothing
        };
    }
    if expand && git::shows_whole_file(file) {
        return Step::Nothing;
    }
    let at = current(state, content, &file.path).unwrap_or(DEFAULT_CONTEXT_LINES);
    let to = if expand {
        git::next_context_step(at)
    } else {
        git::prev_context_step(at)
    };
    to.map_or(Step::Nothing, |lines| {
        Step::Send(send(key, state, &file.path, lines))
    })
}

/// The automatic pick for the file a move selected, as the App's
/// `maybe_auto_expand_context`. A stub has no hunks to measure yet.
pub fn auto(
    key: TabKey,
    state: &mut TabViewState,
    content: &TabContent,
    index: usize,
    threshold: usize,
) -> Option<Command> {
    let tab = &content.tab;
    if threshold == 0 || tab.mode == DiffMode::History || tab.identity().is_remote() {
        return None;
    }
    let file = content.diff.files.get(index)?;
    if current(state, content, &file.path).is_some() {
        return None;
    }
    let lines = git::auto_context(file)?;
    Some(send(key, state, &file.path, lines))
}

/// A count's reply landed: what the content shows is no longer in flight,
/// and a refusal lets go of every send.
pub fn landed(state: &mut TabViewState, content: &TabContent, landed: bool) {
    if !landed {
        state.context_sent.clear();
        return;
    }
    let shown = &content.diff.context;
    state
        .context_sent
        .retain(|path, lines| shown.get(path).copied().unwrap_or(DEFAULT_CONTEXT_LINES) != *lines);
}
