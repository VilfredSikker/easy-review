//! The flag path of the AI picker's saves (ADR 0069).
//!
//! With `v1-service`, `set_ai_model` and the persisted `set_ai_selection` and
//! `set_ai_effort` are saved by the settings actor, the one writer of
//! `config.toml` and `agents.toml`; the App never saves its copy of the
//! config. Nothing copies the saved selection into the App either: every
//! flag-on run reads its pick from the settings view (A18). The Tauri
//! commands build the snapshot they still return once the save has landed.

use er_engine::config::{AiSelection, ConfigFieldValue};
use er_service::{SettingsCommand, SettingsReply};

use crate::state::DesktopState;

fn save(desktop: &DesktopState, command: SettingsCommand) -> Result<SettingsReply, String> {
    desktop
        .service
        .settings()
        .send_and_wait(command)
        .map_err(|error| error.to_string())
}

fn saved_selection(reply: SettingsReply) -> Result<AiSelection, String> {
    match reply {
        SettingsReply::AiSelectionSaved { selection, .. } => Ok(selection),
        other => Err(format!(
            "unexpected reply to an AI selection save: {other:?}"
        )),
    }
}

/// `apply_config_field` already writes `agent.model`, so the patch command
/// carries it.
pub(crate) fn ai_model(desktop: &DesktopState, model: String) -> Result<(), String> {
    save(
        desktop,
        SettingsCommand::ApplyConfigPatch {
            key: "agent.model".to_string(),
            value: ConfigFieldValue::String(model),
        },
    )
    .map(drop)
}

pub(crate) fn ai_selection(
    desktop: &DesktopState,
    provider_id: String,
    model_id: Option<String>,
    effort: Option<String>,
) -> Result<AiSelection, String> {
    let reply = save(
        desktop,
        SettingsCommand::SetDefaultAiSelection {
            provider_id,
            model_id,
            effort,
        },
    )?;
    saved_selection(reply)
}

pub(crate) fn ai_effort(
    desktop: &DesktopState,
    effort: Option<String>,
) -> Result<AiSelection, String> {
    saved_selection(save(
        desktop,
        SettingsCommand::SetDefaultAiEffort { effort },
    )?)
}
