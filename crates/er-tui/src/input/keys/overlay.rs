use super::{Edit, OverlayAction, TuiAction};
use crate::screen::view::OverlayData;
use crossterm::event::{KeyCode, KeyEvent};
use er_engine::config::SettingsScope;

/// An open overlay reads the key code alone; modifiers do not change it.
pub fn action(key: KeyEvent, overlay: &OverlayData) -> TuiAction {
    use OverlayAction as O;
    let action = match overlay {
        OverlayData::ReviewFlowProgress { .. } => match key.code {
            KeyCode::Char('j') | KeyCode::Down => O::Next,
            KeyCode::Char('k') | KeyCode::Up => O::Prev,
            KeyCode::PageDown => O::ScrollDetail(8),
            KeyCode::PageUp => O::ScrollDetail(-8),
            KeyCode::Char('r') => O::RerunReviewFlow,
            KeyCode::Esc | KeyCode::Char('q') => O::Close,
            _ => return TuiAction::Noop,
        },
        OverlayData::ReviewFlowResults { .. } => match review_flow_results_key_action(key.code) {
            ReviewFlowResultsKeyAction::Next => O::Next,
            ReviewFlowResultsKeyAction::Previous => O::Prev,
            ReviewFlowResultsKeyAction::PageDown => O::ScrollDetail(8),
            ReviewFlowResultsKeyAction::PageUp => O::ScrollDetail(-8),
            ReviewFlowResultsKeyAction::Toggle => O::ToggleFlowFinding,
            ReviewFlowResultsKeyAction::Accept => O::AcceptFlowFindings,
            ReviewFlowResultsKeyAction::Close => O::Close,
            ReviewFlowResultsKeyAction::Ignore => return TuiAction::Noop,
        },
        // Multiselect; Enter copies the export to the clipboard.
        OverlayData::ExportPicker { .. } => match key.code {
            KeyCode::Char('j') | KeyCode::Down => O::Next,
            KeyCode::Char('k') | KeyCode::Up => O::Prev,
            KeyCode::Char(' ') => O::ToggleExportKind,
            KeyCode::Enter => O::Export,
            KeyCode::Esc | KeyCode::Char('q') => O::Close,
            _ => return TuiAction::Noop,
        },
        OverlayData::ConfigHub {
            editing: Some(_), ..
        } => O::EditSetting(match key.code {
            KeyCode::Enter => Edit::Submit,
            KeyCode::Esc => Edit::Cancel,
            KeyCode::Char(c) => Edit::Insert(c),
            KeyCode::Backspace => Edit::Backspace,
            KeyCode::Left => Edit::Left,
            KeyCode::Right => Edit::Right,
            _ => return TuiAction::Noop,
        }),
        OverlayData::ConfigHub { tab, .. } => match key.code {
            KeyCode::Char('j') | KeyCode::Down => O::Next,
            KeyCode::Char('k') | KeyCode::Up => O::Prev,
            KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Right => O::ActivateSetting,
            KeyCode::Left => O::ActivateSettingBack,
            KeyCode::Char('d') => O::DeleteSetting,
            KeyCode::Char('1') => O::SwitchSettingsTab(SettingsScope::General),
            KeyCode::Char('2') => O::SwitchSettingsTab(SettingsScope::Terminal),
            // Two tabs, so back and forward both go to the other one.
            KeyCode::Char('[' | ']') => O::SwitchSettingsTab(match tab.tab_index() {
                0 => SettingsScope::Terminal,
                _ => SettingsScope::General,
            }),
            KeyCode::Char('s' | 'S' | 'q') | KeyCode::Esc => O::CloseSettings,
            _ => return TuiAction::Noop,
        },
        _ => match key.code {
            KeyCode::Char('j') | KeyCode::Down => O::Next,
            KeyCode::Char('k') | KeyCode::Up => O::Prev,
            KeyCode::Char('b') => O::SecondaryRow,
            KeyCode::Enter => O::SelectRow,
            KeyCode::Backspace => O::GoUp,
            KeyCode::Esc | KeyCode::Char('q') => O::Close,
            _ => return TuiAction::Noop,
        },
    };
    TuiAction::Overlay(action)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewFlowResultsKeyAction {
    Next,
    Previous,
    PageDown,
    PageUp,
    Toggle,
    Accept,
    Close,
    Ignore,
}

pub fn review_flow_results_key_action(key: KeyCode) -> ReviewFlowResultsKeyAction {
    match key {
        KeyCode::Char('j') | KeyCode::Down => ReviewFlowResultsKeyAction::Next,
        KeyCode::Char('k') | KeyCode::Up => ReviewFlowResultsKeyAction::Previous,
        KeyCode::PageDown => ReviewFlowResultsKeyAction::PageDown,
        KeyCode::PageUp => ReviewFlowResultsKeyAction::PageUp,
        KeyCode::Char(' ') | KeyCode::Enter => ReviewFlowResultsKeyAction::Toggle,
        KeyCode::Char('a') => ReviewFlowResultsKeyAction::Accept,
        KeyCode::Esc | KeyCode::Char('q') => ReviewFlowResultsKeyAction::Close,
        _ => ReviewFlowResultsKeyAction::Ignore,
    }
}
