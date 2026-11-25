//! The flag-on config hub. Its rows are built from the settings view and
//! rebuilt whenever the view's revision moves; each edit goes to the settings
//! actor as a command, which owns the write and the save (ADR 0069).

use er_engine::config::{
    config_hub_items_for_scope, AiSelectionPart, ConfigFieldValue, ConfigItem, ErConfig,
    SettingsScope, AUTO_EFFORT,
};
use er_service::{SettingsCommand, SettingsView};

use super::copy::Pick;
use super::tui_view::Overlay;
use crate::input::keys::{Edit, OverlayAction};

/// What an overlay key asks of the loop.
pub enum Step {
    Stay,
    Close,
    Send(SettingsCommand),
    /// Closes the overlay, then sends.
    Leave(SettingsCommand),
    /// Closes the overlay and sends a default pick, which the status line
    /// announces once it is saved.
    Pick(SettingsCommand),
    /// Replaces the overlay, and sends the command an opening asks for.
    Open(Box<Overlay>, Option<SettingsCommand>),
    /// Closes the overlay and says why.
    Notify(String),
    /// Closes the overlay and runs an AI hub layer row on the tab's view.
    Layer(super::layers::HubRow),
    /// Closes the overlay and asks before a Cleanup row's clear.
    Clear(super::clears::Clear),
    /// Closes the overlay and copies from the active tab.
    Copy(super::copy::Pick),
    /// Closes the overlay and starts an AI hub or Verify run.
    Run(super::ai_runs::Start),
    /// Closes the overlay and runs a Review row.
    Flow(super::flows::Act),
}

pub struct ConfigHub {
    pub scope: SettingsScope,
    pub items: Vec<ConfigItem>,
    pub selected: usize,
    pub editing: Option<EditBuffer>,
    revision: u64,
}

/// A row's inline edit: the typed text and the cursor's byte offset.
pub struct EditBuffer {
    pub item_index: usize,
    pub buffer: String,
    cursor: usize,
}

impl ConfigHub {
    pub fn open(view: &SettingsView) -> Self {
        let scope = SettingsScope::General;
        let items = config_hub_items_for_scope(&view.config, scope);
        let selected = first_row(&items);
        Self {
            scope,
            items,
            selected,
            editing: None,
            revision: view.revision,
        }
    }

    /// Rebuilds the rows once the settings revision moves, keeping the
    /// selection on a row as the App's rebuild after a write does.
    pub fn sync(&mut self, view: &SettingsView) {
        if view.revision == self.revision {
            return;
        }
        self.revision = view.revision;
        self.items = config_hub_items_for_scope(&view.config, self.scope);
        let len = self.items.len();
        self.selected = self.selected.min(len.saturating_sub(1));
        while self.selected + 1 < len && is_header(&self.items[self.selected]) {
            self.selected += 1;
        }
        if self
            .editing
            .as_ref()
            .is_some_and(|edit| edit.item_index >= len)
        {
            self.editing = None;
        }
    }

    fn step(&mut self, forward: bool) {
        let rows = (0..self.items.len()).filter(|&at| !is_header(&self.items[at]));
        let to = if forward {
            rows.into_iter().find(|&at| at > self.selected)
        } else {
            rows.into_iter().rfind(|&at| at < self.selected)
        };
        if let Some(at) = to {
            self.selected = at;
        }
    }
}

fn is_header(item: &ConfigItem) -> bool {
    matches!(item, ConfigItem::SectionHeader(_))
}

fn first_row(items: &[ConfigItem]) -> usize {
    items.iter().position(|item| !is_header(item)).unwrap_or(0)
}

fn patch(key: &str, value: ConfigFieldValue) -> Step {
    Step::Send(SettingsCommand::ApplyConfigPatch {
        key: key.to_string(),
        value,
    })
}

/// The option after or before `current`, wrapping; an unknown value counts
/// as the first.
fn cycle<T: PartialEq + Clone>(options: &[T], current: &T, forward: bool) -> Option<T> {
    let len = options.len();
    let at = options.iter().position(|o| o == current).unwrap_or(0);
    let to = if forward { at + 1 } else { at + len - 1 };
    options.get(to.checked_rem(len)?).cloned()
}

/// Runs one key on the open hub.
pub fn act(hub: &mut ConfigHub, action: OverlayAction, view: &SettingsView) -> Step {
    use OverlayAction as O;
    let config = &view.config;
    match action {
        O::Next | O::Prev if hub.editing.is_none() => hub.step(action == O::Next),
        O::ActivateSetting if hub.editing.is_none() => return activate(hub, config, true),
        O::ActivateSettingBack if hub.editing.is_none() => return activate(hub, config, false),
        O::DeleteSetting => {
            if let Some(ConfigItem::ListEntry { key, index, .. }) = hub.items.get(hub.selected) {
                return patch(key, ConfigFieldValue::Number(*index as u64));
            }
        }
        O::SwitchSettingsTab(scope) if scope != hub.scope => {
            hub.scope = scope;
            hub.items = config_hub_items_for_scope(config, scope);
            hub.selected = first_row(&hub.items);
            hub.editing = None;
        }
        O::CloseSettings | O::Close => return Step::Close,
        O::EditSetting(key) => return edit(hub, key),
        _ => {}
    }
    Step::Stay
}

fn activate(hub: &mut ConfigHub, config: &ErConfig, forward: bool) -> Step {
    let Some(item) = hub.items.get(hub.selected) else {
        return Step::Stay;
    };
    let edit_from = |buffer: String| EditBuffer {
        item_index: hub.selected,
        cursor: buffer.len(),
        buffer,
    };
    match item {
        ConfigItem::BoolToggle { key, get, .. } if forward => {
            patch(key, ConfigFieldValue::Bool(!get(config)))
        }
        ConfigItem::StringCycle {
            key, options, get, ..
        } => match cycle(options, &get(config).as_str(), forward) {
            Some(next) => patch(key, ConfigFieldValue::String(next.to_string())),
            None => Step::Stay,
        },
        ConfigItem::DynamicStringCycle {
            part, options, get, ..
        } => match cycle(&options(config), &get(config), forward) {
            Some(next) => selection(*part, next, config),
            None => Step::Stay,
        },
        ConfigItem::NumberEdit {
            key, min, max, get, ..
        } => {
            let current = get(config);
            let next = match forward {
                true if current >= *max => *min,
                true => current + 1,
                false if current <= *min => *max,
                false => current - 1,
            };
            patch(key, ConfigFieldValue::Number(u64::from(next)))
        }
        ConfigItem::StringEdit { get, .. } if forward => {
            hub.editing = Some(edit_from(get(config)));
            Step::Stay
        }
        ConfigItem::ListAdd { .. } if forward => {
            hub.editing = Some(edit_from(String::new()));
            Step::Stay
        }
        ConfigItem::Action { action_id, .. } if forward => match *action_id {
            "copy_review_json" => Step::Copy(Pick::ReviewJson),
            "copy_questions_json" => Step::Copy(Pick::QuestionsJson),
            _ => Step::Close,
        },
        _ => Step::Stay,
    }
}

/// A Provider, Model or Effort cycle writes the default selection, which
/// the actor resolves and saves as one choice. Auto effort is no pin.
fn selection(part: AiSelectionPart, value: String, config: &ErConfig) -> Step {
    let current = config.ai_hub.resolve_default_selection(&config.agent);
    let command = match part {
        AiSelectionPart::Provider => SettingsCommand::SetDefaultAiSelection {
            provider_id: value,
            model_id: None,
            effort: None,
        },
        AiSelectionPart::Model => match current.provider_id {
            Some(provider_id) => SettingsCommand::SetDefaultAiSelection {
                provider_id,
                model_id: Some(value),
                effort: None,
            },
            None => return Step::Stay,
        },
        AiSelectionPart::Effort => SettingsCommand::SetDefaultAiEffort {
            effort: (value != AUTO_EFFORT).then_some(value),
        },
    };
    Step::Send(command)
}

fn edit(hub: &mut ConfigHub, key: Edit) -> Step {
    let Some(edit) = hub.editing.as_mut() else {
        return Step::Stay;
    };
    let before = |edit: &EditBuffer| {
        let head = edit.buffer.get(..edit.cursor).unwrap_or_default();
        head.chars().next_back().map_or(0, char::len_utf8)
    };
    match key {
        Edit::Insert(c) => {
            edit.buffer.insert(edit.cursor, c);
            edit.cursor += c.len_utf8();
        }
        Edit::Backspace if edit.cursor > 0 => {
            edit.cursor -= before(edit);
            edit.buffer.remove(edit.cursor);
        }
        Edit::Left => edit.cursor -= before(edit),
        Edit::Right => {
            let tail = edit.buffer.get(edit.cursor..).unwrap_or_default();
            let after = tail.chars().next();
            edit.cursor += after.map_or(0, char::len_utf8);
        }
        Edit::Cancel => hub.editing = None,
        Edit::Submit => {
            let Some(edit) = hub.editing.take() else {
                return Step::Stay;
            };
            return match hub.items.get(edit.item_index) {
                Some(ConfigItem::StringEdit { key, .. }) => {
                    patch(key, ConfigFieldValue::String(edit.buffer))
                }
                Some(ConfigItem::ListAdd { key, .. }) if !edit.buffer.trim().is_empty() => patch(
                    key,
                    ConfigFieldValue::String(edit.buffer.trim().to_string()),
                ),
                _ => Step::Stay,
            };
        }
        Edit::Backspace => {}
    }
    Step::Stay
}
