use super::{AiSummaryAction, HistoryAction, NormalAction, TourAction, TuiAction};
use crate::screen::Screen;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use er_engine::ai::PanelContent;
use er_engine::tab_identity::DiffMode;

/// Normal mode tries its key groups in a fixed order: the global keys, the
/// focused panel, the keys every diff mode shares, then History's, Tour's or
/// the diff view's navigation. A group that owns the key ends the search,
/// even when its guard leaves nothing to do.
pub fn action(key: KeyEvent, screen: &Screen) -> TuiAction {
    global(key, screen)
        .or_else(|| panel(key, screen))
        .or_else(|| shared(key, screen))
        .unwrap_or_else(|| match screen.tab.mode {
            DiffMode::History => history(key),
            DiffMode::Tour => tour(key),
            _ => navigation(key),
        })
}

fn global(key: KeyEvent, screen: &Screen) -> Option<TuiAction> {
    use NormalAction as A;
    let tab = &screen.tab;
    let plain = key.modifiers == KeyModifiers::NONE;
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let action = match key.code {
        KeyCode::Char('q') if ctrl => A::Quit,
        KeyCode::Char('q') => A::StartQuestion,
        KeyCode::Char(c @ '1'..='9') => {
            let mode = tab.modes.get((c as usize) - ('1' as usize));
            return Some(mode.map_or(TuiAction::Noop, |&mode| A::SwitchMode(mode).into()));
        }
        KeyCode::Char('m') => A::ToggleMtimeSort,
        KeyCode::Char('R') => A::Refresh,
        KeyCode::Char('w') => A::ToggleWatch,
        KeyCode::Char('e') => A::Edit,
        KeyCode::Char('J') => A::PrevHint,
        KeyCode::Char('K') => A::NextHint,
        KeyCode::Char('k') if ctrl => A::NextFinding,
        KeyCode::Char('j') if ctrl => A::PrevFinding,
        KeyCode::Char('x') if plain && tab.mode == DiffMode::Hidden => A::AskDeleteWatchedFile,
        KeyCode::Char('x') if plain && tab.focused_comment_id.is_some() => A::AskDeleteComment,
        KeyCode::Char('r') => A::Reply,
        KeyCode::Char('z') if plain => A::AskClearQuestions,
        KeyCode::Char('Z') => A::AskClearReviews,
        KeyCode::Char('x') => A::CloseTab,
        KeyCode::Char(']') => A::NextTab,
        KeyCode::Char('[') => A::PrevTab,
        KeyCode::Char('o') => A::OpenOpenHub,
        KeyCode::Char('W') => A::ToggleWatchedFiles,
        KeyCode::Char('<') => A::ResizeFileTree(-2),
        KeyCode::Char('>') => A::ResizeFileTree(2),
        KeyCode::Char('{') => A::ResizePanel(-4),
        KeyCode::Char('}') => A::ResizePanel(4),
        _ => return None,
    };
    Some(action.into())
}

/// A focused panel. The AI summary takes every key; the others take their
/// navigation keys and leave the rest to the shared group.
fn panel(key: KeyEvent, screen: &Screen) -> Option<TuiAction> {
    use NormalAction as A;
    let tab = &screen.tab;
    if !tab.panel_focus || tab.panel.is_none() {
        return None;
    }
    if tab.panel == Some(PanelContent::AiSummary) {
        return Some(ai_summary(key));
    }
    let has_findings = tab.panel == Some(PanelContent::FileDetail)
        && screen
            .data
            .file(tab.selected_file)
            .and_then(|f| screen.data.ai()?.file_review(&f.path))
            .is_some_and(|fr| !fr.findings.is_empty());
    let action = match key.code {
        KeyCode::Char('j') | KeyCode::Down if has_findings => A::NavigatePanelFinding(true),
        KeyCode::Char('k') | KeyCode::Up if has_findings => A::NavigatePanelFinding(false),
        KeyCode::Enter if tab.focused_finding_id.is_some() => A::JumpToFocusedFinding,
        KeyCode::Char('k') | KeyCode::Down => A::PanelLineDown,
        KeyCode::Char('j') | KeyCode::Up => A::PanelLineUp,
        KeyCode::Esc => A::UnfocusPanel,
        _ => return None,
    };
    Some(action.into())
}

fn shared(key: KeyEvent, screen: &Screen) -> Option<TuiAction> {
    use NormalAction as A;
    let tab = &screen.tab;
    let plain = key.modifiers == KeyModifiers::NONE;
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    // History and Tour have no stage, review marks, copy or export.
    let diff = !matches!(tab.mode, DiffMode::History | DiffMode::Tour);
    let action = match key.code {
        KeyCode::Char('/') => A::StartSearch,
        KeyCode::Char('f') => A::StartFilter,
        KeyCode::Char('F') => A::OpenFilterHistory,
        KeyCode::Char(',') => A::OpenConfigHub,
        KeyCode::Char('A') => A::ToggleAiFindings,
        KeyCode::Char('p') if ctrl => A::AskPush,
        KeyCode::Char('p') => A::TogglePanel,
        KeyCode::Tab if screen.chrome.has_comment_draft => A::ResumeComment,
        KeyCode::Tab if diff && screen.chrome.split_diff => A::ToggleSplitFocus,
        KeyCode::Tab => A::TogglePanelFocus,
        KeyCode::Char('c') if tab.mode == DiffMode::Staged => A::StartCommit,
        KeyCode::Char('c') => A::StartGithubComment,
        KeyCode::Char('C') => A::ToggleComments,
        KeyCode::Char('Q') => A::ToggleQuestions,
        KeyCode::Char('X') => A::ToggleHideResolved,
        KeyCode::Char('T') => A::CycleMinTrust,
        KeyCode::Char('G') => A::SyncGithubComments,
        KeyCode::Char('P') => A::TogglePanelReverse,
        KeyCode::Char('a') => A::OpenAiHub,
        KeyCode::Char('g') => A::OpenGitHub,
        KeyCode::Char('v') => A::OpenVerifyHub,
        KeyCode::Char('?') => A::OpenHelpHub,
        KeyCode::Enter if tab.selected_diff_file.is_some_and(|f| f.compacted) => A::ToggleCompacted,
        KeyCode::Char('+' | '=') => A::ExpandContext,
        KeyCode::Char('-') => A::CollapseContext,
        KeyCode::Esc if !tab.search_query.is_empty() => A::ClearSearch,
        KeyCode::Esc if !tab.filter_expr.is_empty() => A::ClearFilter,
        KeyCode::Enter | KeyCode::Esc => return Some(TuiAction::Noop),
        KeyCode::Char('s') if diff && !tab.is_remote && plain => {
            if tab.selected_watched_index.is_some() {
                A::UpdateWatchedSnapshot
            } else {
                A::ToggleStaged
            }
        }
        KeyCode::Char('!') if diff => A::ToggleUnreviewedOnly,
        KeyCode::Char(' ') if diff => A::ToggleReviewed,
        KeyCode::Char('U') if diff && plain => A::NextUnreviewedFile,
        KeyCode::Char('y') if diff && plain => A::OpenCopyHub,
        KeyCode::Char('E') if diff && key.modifiers.contains(KeyModifiers::SHIFT) => {
            A::OpenExportPicker
        }
        _ => return None,
    };
    Some(action.into())
}

/// `d` and `u` scroll bare or with Ctrl; with Shift they do nothing.
fn scrolls(key: KeyEvent) -> bool {
    key.modifiers == KeyModifiers::NONE || key.modifiers.contains(KeyModifiers::CONTROL)
}

fn navigation(key: KeyEvent) -> TuiAction {
    use NormalAction as A;
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let action = match key.code {
        KeyCode::Char('j') => A::PrevFile,
        KeyCode::Char('k') => A::NextFile,
        KeyCode::Down if shift => A::ExtendSelectionDown,
        KeyCode::Up if shift => A::ExtendSelectionUp,
        KeyCode::Down => A::NextLine,
        KeyCode::Up => A::PrevLine,
        KeyCode::Char('n') => A::NextHunk,
        KeyCode::Char('N') => A::PrevHunk,
        KeyCode::Char('l') | KeyCode::Right => A::ScrollRight,
        KeyCode::Char('h') | KeyCode::Left => A::ScrollLeft,
        KeyCode::Home => A::ScrollHome,
        KeyCode::Char('d') if scrolls(key) => A::ScrollDown(10),
        KeyCode::Char('u') if scrolls(key) => A::ScrollUp(10),
        KeyCode::PageDown => A::ScrollDown(20),
        KeyCode::PageUp => A::ScrollUp(20),
        _ => return TuiAction::Noop,
    };
    action.into()
}

fn ai_summary(key: KeyEvent) -> TuiAction {
    use AiSummaryAction as S;
    let action = match key.code {
        KeyCode::Char('k') | KeyCode::Down => S::Next,
        KeyCode::Char('j') | KeyCode::Up => S::Prev,
        KeyCode::Tab
        | KeyCode::Char('l' | 'h')
        | KeyCode::Right
        | KeyCode::BackTab
        | KeyCode::Left => S::ToggleFocus,
        KeyCode::Char(' ') => S::ToggleChecklistItem,
        KeyCode::Enter => S::JumpToFile,
        KeyCode::Char('d') if scrolls(key) => S::ScrollDown(10),
        KeyCode::Char('u') if scrolls(key) => S::ScrollUp(10),
        KeyCode::PageDown => S::ScrollDown(20),
        KeyCode::PageUp => S::ScrollUp(20),
        KeyCode::Esc => S::Unfocus,
        _ => return TuiAction::Noop,
    };
    TuiAction::AiSummary(action)
}

fn history(key: KeyEvent) -> TuiAction {
    use HistoryAction as H;
    let action = match key.code {
        KeyCode::Char('k') => H::NextCommit,
        KeyCode::Char('j') => H::PrevCommit,
        KeyCode::Char('n') => H::NextFile,
        KeyCode::Char('N') => H::PrevFile,
        KeyCode::Down => H::NextLine,
        KeyCode::Up => H::PrevLine,
        KeyCode::Char('l') | KeyCode::Right => H::ScrollRight,
        KeyCode::Char('h') | KeyCode::Left => H::ScrollLeft,
        KeyCode::Home => H::ScrollHome,
        KeyCode::Char('d') if scrolls(key) => H::ScrollDown(10),
        KeyCode::Char('u') if scrolls(key) => H::ScrollUp(10),
        KeyCode::PageDown => H::ScrollDown(20),
        KeyCode::PageUp => H::ScrollUp(20),
        _ => return TuiAction::Noop,
    };
    TuiAction::History(action)
}

fn tour(key: KeyEvent) -> TuiAction {
    use TourAction as T;
    let action = match key.code {
        KeyCode::Char('k') => T::NextPillar,
        KeyCode::Char('j') => T::PrevPillar,
        KeyCode::Char('n') => T::NextFile,
        KeyCode::Char('N') => T::PrevFile,
        KeyCode::Down => T::NextLine,
        KeyCode::Up => T::PrevLine,
        KeyCode::Char('l') | KeyCode::Right => T::ScrollRight,
        KeyCode::Char('h') | KeyCode::Left => T::ScrollLeft,
        KeyCode::Home => T::ScrollHome,
        KeyCode::Char('d') if scrolls(key) => T::ScrollDown(10),
        KeyCode::Char('u') if scrolls(key) => T::ScrollUp(10),
        KeyCode::PageDown => T::ScrollDown(20),
        KeyCode::PageUp => T::ScrollUp(20),
        KeyCode::Char(' ') => T::ToggleReviewed,
        KeyCode::Char('b') => T::ReviewPillar,
        _ => return TuiAction::Noop,
    };
    TuiAction::Tour(action)
}
