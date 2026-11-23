//! Desktop settings page — load/apply/save `ErConfig` (excludes diff-view fields).
//!
//! With `v1-service` every command here goes to the settings actor (ADR 0069)
//! and none takes the App lock. The flag-off bodies edit `App.config`, and
//! only they run the App-side effects of a write (watched files, the AI
//! selection, the tab's mode); on the flag path that waits for the service to
//! own that state (D21).

use er_engine::config::{ConfigFieldValue, ModelUpsert, ProviderUpsert, ReviewModelGroups};
use er_engine::uninstall::UninstallOptions;
use tauri::State;

use crate::commands::{run_blocking, AiProviderInfo, AppState};
pub use crate::settings::{GetConfigHubResponse, SettingsSnapshot};
use crate::snapshot::AppSnapshot;

#[derive(serde::Deserialize)]
pub struct ConfigPatch {
    pub key: String,
    pub value: ConfigFieldValue,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshAiModelsResponse {
    pub providers: Vec<AiProviderInfo>,
    /// One line per provider whose listing failed; the prior cache still applies.
    pub errors: Vec<String>,
}

#[tauri::command]
pub async fn get_config_hub(state: State<'_, AppState>) -> Result<GetConfigHubResponse, String> {
    let state = state.inner().clone();
    // Off the main thread: the flag path waits on the settings actor, which
    // loads the config on first use (ADR 0015, ADR 0067).
    run_blocking(move || config_hub(&state)).await
}

#[tauri::command]
pub async fn apply_config_patch(
    patch: ConfigPatch,
    state: State<'_, AppState>,
) -> Result<GetConfigHubResponse, String> {
    let state = state.inner().clone();
    // Off the main thread: the write saves the config to disk.
    run_blocking(move || apply_patch(&state, patch)).await
}

#[tauri::command]
pub async fn save_config_global_cmd(state: State<'_, AppState>) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || save_global(&state)).await
}

#[tauri::command]
pub async fn preview_uninstall(
    request: Option<UninstallOptions>,
    state: State<'_, AppState>,
) -> Result<UninstallPreview, String> {
    let state = state.inner().clone();
    run_blocking(move || uninstall_preview(&state, request.unwrap_or_default())).await
}

#[tauri::command]
pub async fn run_uninstall(
    app_handle: tauri::AppHandle,
    request: Option<UninstallOptions>,
    state: State<'_, AppState>,
) -> Result<UninstallResult, String> {
    let state = state.inner().clone();
    let result = run_blocking(move || {
        uninstall(&state, request.unwrap_or_default()).inspect_err(|msg| {
            log::error!("run_uninstall: {msg}");
        })
    })
    .await?;
    finish_uninstall(&app_handle, result)
}

#[tauri::command]
pub async fn set_review_flow_models(
    groups: ReviewModelGroups,
    state: State<'_, AppState>,
) -> Result<GetConfigHubResponse, String> {
    let state = state.inner().clone();
    run_blocking(move || review_flow_models(&state, groups)).await
}

#[tauri::command]
pub async fn refresh_ai_models(
    provider_id: Option<String>,
    force: Option<bool>,
    state: State<'_, AppState>,
) -> Result<RefreshAiModelsResponse, String> {
    let state = state.inner().clone();
    run_blocking(move || refresh_models(&state, provider_id, force.unwrap_or(false))).await
}

#[tauri::command]
pub async fn upsert_ai_provider(
    provider: ProviderUpsert,
    state: State<'_, AppState>,
) -> Result<GetConfigHubResponse, String> {
    let state = state.inner().clone();
    run_blocking(move || provider_upsert(&state, provider)).await
}

#[tauri::command]
pub async fn delete_ai_provider(
    provider_id: String,
    state: State<'_, AppState>,
) -> Result<GetConfigHubResponse, String> {
    let state = state.inner().clone();
    run_blocking(move || provider_delete(&state, provider_id)).await
}

#[tauri::command]
pub async fn upsert_ai_model(
    provider_id: String,
    model: ModelUpsert,
    state: State<'_, AppState>,
) -> Result<GetConfigHubResponse, String> {
    let state = state.inner().clone();
    run_blocking(move || model_upsert(&state, provider_id, model)).await
}

#[tauri::command]
pub async fn delete_ai_model(
    provider_id: String,
    model_id: String,
    state: State<'_, AppState>,
) -> Result<GetConfigHubResponse, String> {
    let state = state.inner().clone();
    run_blocking(move || model_delete(&state, provider_id, model_id)).await
}

// ── Command bodies: one per command, each split by the feature ──────────────

pub(crate) fn config_hub(state: &AppState) -> Result<GetConfigHubResponse, String> {
    return service_route::config_hub(&state.service);
}

pub(crate) fn apply_patch(
    state: &AppState,
    patch: ConfigPatch,
) -> Result<GetConfigHubResponse, String> {
    return service_route::apply_patch(&state.service, patch);
}

pub(crate) fn save_global(state: &AppState) -> Result<AppSnapshot, String> {
    return service_route::save_global(&state.service);
}

pub(crate) fn uninstall_preview(
    state: &AppState,
    options: UninstallOptions,
) -> Result<UninstallPreview, String> {
    return service_route::uninstall_preview(&state.service, options);
}

fn uninstall(state: &AppState, options: UninstallOptions) -> Result<UninstallResult, String> {
    return service_route::uninstall(&state.service, options);
}

pub(crate) fn review_flow_models(
    state: &AppState,
    groups: ReviewModelGroups,
) -> Result<GetConfigHubResponse, String> {
    return service_route::review_flow_models(&state.service, groups);
}

pub(crate) fn refresh_models(
    state: &AppState,
    provider_id: Option<String>,
    force: bool,
) -> Result<RefreshAiModelsResponse, String> {
    return service_route::refresh_models(&state.service, provider_id, force);
}

pub(crate) fn provider_upsert(
    state: &AppState,
    provider: ProviderUpsert,
) -> Result<GetConfigHubResponse, String> {
    return service_route::provider_upsert(&state.service, provider);
}

pub(crate) fn provider_delete(
    state: &AppState,
    provider_id: String,
) -> Result<GetConfigHubResponse, String> {
    return service_route::provider_delete(&state.service, provider_id);
}

pub(crate) fn model_upsert(
    state: &AppState,
    provider_id: String,
    model: ModelUpsert,
) -> Result<GetConfigHubResponse, String> {
    return service_route::model_upsert(&state.service, provider_id, model);
}

pub(crate) fn model_delete(
    state: &AppState,
    provider_id: String,
    model_id: String,
) -> Result<GetConfigHubResponse, String> {
    return service_route::model_delete(&state.service, provider_id, model_id);
}

// ── Uninstall ───────────────────────────────────────────────────────────────

pub use er_engine::uninstall::{UninstallPreview, UninstallResult, UninstallTargetDto};

/// Quits after an uninstall that asked for it, and reports what failed.
pub(crate) fn finish_uninstall(
    app_handle: &tauri::AppHandle,
    result: UninstallResult,
) -> Result<UninstallResult, String> {
    if !result.failed.is_empty() && !result.should_quit {
        let msg = format!("Uninstall failed:\n{}", result.failed.join("\n"));
        log::error!("run_uninstall: {msg}");
        return Err(msg);
    }

    if result.should_quit {
        let handle = app_handle.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(400));
            handle.exit(0);
        });
    }

    if !result.failed.is_empty() {
        let msg = format!(
            "Uninstall partially failed (quitting so in-use paths can be removed):\n{}",
            result.failed.join("\n")
        );
        log::error!("run_uninstall: {msg}");
        return Err(msg);
    }

    Ok(result)
}

// ── The flag path: the settings actor ───────────────────────────────────────

mod service_route {
    use er_engine::config::{ModelUpsert, ProviderUpsert, ReviewModelGroups};
    use er_engine::uninstall::{UninstallOptions, UninstallPreview, UninstallResult};
    use er_service::{SettingsCommand, SettingsReply};

    use super::{ConfigPatch, GetConfigHubResponse, RefreshAiModelsResponse};
    use crate::service::ServiceState;
    use crate::snapshot::AppSnapshot;

    fn send(service: &ServiceState, command: SettingsCommand) -> Result<SettingsReply, String> {
        service
            .settings()
            .send_and_wait(command)
            .map_err(|error| error.to_string())
    }

    /// The rows as the actor published them. A write's reply arrives after
    /// its view is published, so this read already carries the write.
    fn hub(service: &ServiceState, warnings: Vec<String>) -> GetConfigHubResponse {
        GetConfigHubResponse::from_view(service.settings().view().hub.clone(), warnings)
    }

    fn write(
        service: &ServiceState,
        command: SettingsCommand,
    ) -> Result<GetConfigHubResponse, String> {
        let warnings = match send(service, command)? {
            SettingsReply::ConfigChanged { warnings, .. } => warnings,
            _ => Vec::new(),
        };
        Ok(hub(service, warnings))
    }

    pub(super) fn config_hub(service: &ServiceState) -> Result<GetConfigHubResponse, String> {
        // Reopening Settings is the reload point, as on the flag-off path.
        send(service, SettingsCommand::ReloadIfFailed)?;
        Ok(hub(service, Vec::new()))
    }

    pub(super) fn apply_patch(
        service: &ServiceState,
        patch: ConfigPatch,
    ) -> Result<GetConfigHubResponse, String> {
        write(
            service,
            SettingsCommand::ApplyConfigPatch {
                key: patch.key,
                value: patch.value,
            },
        )
    }

    /// The flag-on desktop does not render from the service yet (D21), so
    /// the acknowledgement carries an empty snapshot.
    pub(super) fn save_global(service: &ServiceState) -> Result<AppSnapshot, String> {
        send(service, SettingsCommand::SaveConfig)?;
        Ok(AppSnapshot::default())
    }

    pub(super) fn uninstall_preview(
        service: &ServiceState,
        options: UninstallOptions,
    ) -> Result<UninstallPreview, String> {
        match send(service, SettingsCommand::PreviewUninstall { options })? {
            SettingsReply::UninstallPreviewed(preview) => Ok(preview),
            other => Err(format!(
                "unexpected reply to an uninstall preview: {other:?}"
            )),
        }
    }

    pub(super) fn uninstall(
        service: &ServiceState,
        options: UninstallOptions,
    ) -> Result<UninstallResult, String> {
        match send(service, SettingsCommand::RunUninstall { options })? {
            SettingsReply::Uninstalled(result) => Ok(result),
            other => Err(format!("unexpected reply to an uninstall: {other:?}")),
        }
    }

    pub(super) fn review_flow_models(
        service: &ServiceState,
        groups: ReviewModelGroups,
    ) -> Result<GetConfigHubResponse, String> {
        write(service, SettingsCommand::SetReviewFlowModels { groups })
    }

    pub(super) fn refresh_models(
        service: &ServiceState,
        provider_id: Option<String>,
        force: bool,
    ) -> Result<RefreshAiModelsResponse, String> {
        let errors = match send(
            service,
            SettingsCommand::RefreshAiModels { provider_id, force },
        )? {
            SettingsReply::ModelsRefreshed { errors, .. } => errors,
            _ => Vec::new(),
        };
        Ok(RefreshAiModelsResponse {
            providers: service.settings().view().hub.providers.clone(),
            errors,
        })
    }

    pub(super) fn provider_upsert(
        service: &ServiceState,
        provider: ProviderUpsert,
    ) -> Result<GetConfigHubResponse, String> {
        write(service, SettingsCommand::UpsertAiProvider { provider })
    }

    pub(super) fn provider_delete(
        service: &ServiceState,
        provider_id: String,
    ) -> Result<GetConfigHubResponse, String> {
        write(service, SettingsCommand::DeleteAiProvider { provider_id })
    }

    pub(super) fn model_upsert(
        service: &ServiceState,
        provider_id: String,
        model: ModelUpsert,
    ) -> Result<GetConfigHubResponse, String> {
        write(
            service,
            SettingsCommand::UpsertAiModel { provider_id, model },
        )
    }

    pub(super) fn model_delete(
        service: &ServiceState,
        provider_id: String,
        model_id: String,
    ) -> Result<GetConfigHubResponse, String> {
        write(
            service,
            SettingsCommand::DeleteAiModel {
                provider_id,
                model_id,
            },
        )
    }
}

// ── The flag-off path: `App.config` under the App lock ──────────────────────
