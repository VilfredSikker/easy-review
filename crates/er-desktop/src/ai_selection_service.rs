//! The flag path of the AI picker's saves (ADR 0069).
//!
//! With `v1-service`, `set_ai_model` and the persisted `set_ai_selection` and
//! `set_ai_effort` are saved by the settings actor, the one writer of
//! `config.toml` and `agents.toml`; the App never saves its copy of the
//! config. The App takes only the saved selection as its session selection,
//! because the palette and the snapshot these commands return still read it
//! there until the service owns the session (D21). Its config copy stays
//! stale, like every flag-path settings write.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use er_engine::app::App;
use er_engine::config::{AiSelection, ConfigFieldValue};
use er_service::{SettingsCommand, SettingsReply};

use crate::commands::{snap_from, AppState};
use crate::snapshot::AppSnapshot;

/// The one App write the wire's AI picks keep: the saved selection copied
/// into the App's session pick, which the ask runs still read on the App.
/// Managed as its own state, so `er_send` reaches this write and nothing
/// else of the App.
#[derive(Clone)]
pub struct SessionPick {
    app: Arc<Mutex<App>>,
    desktop_revision: Arc<AtomicU64>,
}

impl SessionPick {
    pub fn new(app: Arc<Mutex<App>>, desktop_revision: Arc<AtomicU64>) -> Self {
        Self {
            app,
            desktop_revision,
        }
    }

    /// Copies `selection` into the App's session pick. Called once the save
    /// has landed, so the actor never waits on the App lock.
    pub fn follow(&self, selection: &AiSelection) {
        let mut app = self.app.lock().unwrap_or_else(PoisonError::into_inner);
        follow(&mut app, selection.clone());
        self.desktop_revision.fetch_add(1, Ordering::Relaxed);
    }
}

fn follow(app: &mut App, selection: AiSelection) {
    app.current_ai_provider = selection.provider_id;
    app.current_ai_model = selection.model_id;
    app.current_ai_effort = selection.effort;
}

fn save(state: &AppState, command: SettingsCommand) -> Result<SettingsReply, String> {
    state
        .service
        .settings()
        .send_and_wait(command)
        .map_err(|error| error.to_string())
}

/// The App's snapshot once the save has landed. The App lock is taken only
/// now, so the actor never waits on it.
fn snapshot(state: &AppState, selection: Option<AiSelection>) -> Result<AppSnapshot, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    if let Some(selection) = selection {
        follow(&mut app, selection);
        state.desktop_revision.fetch_add(1, Ordering::Relaxed);
    }
    Ok(snap_from(&app, state))
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
pub(crate) fn ai_model(state: &AppState, model: String) -> Result<AppSnapshot, String> {
    save(
        state,
        SettingsCommand::ApplyConfigPatch {
            key: "agent.model".to_string(),
            value: ConfigFieldValue::String(model),
        },
    )?;
    snapshot(state, None)
}

pub(crate) fn ai_selection(
    state: &AppState,
    provider_id: String,
    model_id: Option<String>,
    effort: Option<String>,
) -> Result<AppSnapshot, String> {
    let reply = save(
        state,
        SettingsCommand::SetDefaultAiSelection {
            provider_id,
            model_id,
            effort,
        },
    )?;
    snapshot(state, Some(saved_selection(reply)?))
}

pub(crate) fn ai_effort(state: &AppState, effort: Option<String>) -> Result<AppSnapshot, String> {
    let reply = save(state, SettingsCommand::SetDefaultAiEffort { effort })?;
    snapshot(state, Some(saved_selection(reply)?))
}
