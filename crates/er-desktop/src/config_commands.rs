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
    #[cfg(feature = "v1-service")]
    return service_route::config_hub(&state.service);
    #[cfg(not(feature = "v1-service"))]
    {
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        // Reopening Settings is the desktop's reload point: a file the user
        // repaired in place is adopted here, before the page reads the config.
        app.reload_config_if_failed();
        Ok(flag_off::hub_response(&app, Vec::new()))
    }
}

pub(crate) fn apply_patch(
    state: &AppState,
    patch: ConfigPatch,
) -> Result<GetConfigHubResponse, String> {
    #[cfg(feature = "v1-service")]
    return service_route::apply_patch(&state.service, patch);
    #[cfg(not(feature = "v1-service"))]
    flag_off::apply_patch(state, patch)
}

pub(crate) fn save_global(state: &AppState) -> Result<AppSnapshot, String> {
    #[cfg(feature = "v1-service")]
    return service_route::save_global(&state.service);
    #[cfg(not(feature = "v1-service"))]
    flag_off::save_global(state)
}

pub(crate) fn uninstall_preview(
    state: &AppState,
    options: UninstallOptions,
) -> Result<UninstallPreview, String> {
    #[cfg(feature = "v1-service")]
    return service_route::uninstall_preview(&state.service, options);
    #[cfg(not(feature = "v1-service"))]
    {
        let _ = state;
        Ok(er_engine::uninstall::preview(&options))
    }
}

fn uninstall(state: &AppState, options: UninstallOptions) -> Result<UninstallResult, String> {
    #[cfg(feature = "v1-service")]
    return service_route::uninstall(&state.service, options);
    #[cfg(not(feature = "v1-service"))]
    {
        let _ = state;
        er_engine::uninstall::run(&options)
    }
}

pub(crate) fn review_flow_models(
    state: &AppState,
    groups: ReviewModelGroups,
) -> Result<GetConfigHubResponse, String> {
    #[cfg(feature = "v1-service")]
    return service_route::review_flow_models(&state.service, groups);
    #[cfg(not(feature = "v1-service"))]
    flag_off::review_flow_models(state, groups)
}

pub(crate) fn refresh_models(
    state: &AppState,
    provider_id: Option<String>,
    force: bool,
) -> Result<RefreshAiModelsResponse, String> {
    #[cfg(feature = "v1-service")]
    return service_route::refresh_models(&state.service, provider_id, force);
    #[cfg(not(feature = "v1-service"))]
    flag_off::refresh_models(state, provider_id, force)
}

pub(crate) fn provider_upsert(
    state: &AppState,
    provider: ProviderUpsert,
) -> Result<GetConfigHubResponse, String> {
    #[cfg(feature = "v1-service")]
    return service_route::provider_upsert(&state.service, provider);
    #[cfg(not(feature = "v1-service"))]
    flag_off::provider_upsert(state, provider)
}

pub(crate) fn provider_delete(
    state: &AppState,
    provider_id: String,
) -> Result<GetConfigHubResponse, String> {
    #[cfg(feature = "v1-service")]
    return service_route::provider_delete(&state.service, provider_id);
    #[cfg(not(feature = "v1-service"))]
    flag_off::provider_delete(state, provider_id)
}

pub(crate) fn model_upsert(
    state: &AppState,
    provider_id: String,
    model: ModelUpsert,
) -> Result<GetConfigHubResponse, String> {
    #[cfg(feature = "v1-service")]
    return service_route::model_upsert(&state.service, provider_id, model);
    #[cfg(not(feature = "v1-service"))]
    flag_off::model_upsert(state, provider_id, model)
}

pub(crate) fn model_delete(
    state: &AppState,
    provider_id: String,
    model_id: String,
) -> Result<GetConfigHubResponse, String> {
    #[cfg(feature = "v1-service")]
    return service_route::model_delete(&state.service, provider_id, model_id);
    #[cfg(not(feature = "v1-service"))]
    flag_off::model_delete(state, provider_id, model_id)
}

// ── Uninstall ───────────────────────────────────────────────────────────────

pub use er_engine::uninstall::{UninstallPreview, UninstallResult, UninstallTargetDto};

/// Quits after an uninstall that asked for it, and reports what failed.
fn finish_uninstall(
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

#[cfg(feature = "v1-service")]
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

#[cfg(not(feature = "v1-service"))]
mod flag_off {
    use er_engine::app::{App, DiffMode};
    use er_engine::config::{
        apply_config_field, config_hub_view, save_config, ErConfig, FeatureFlags, ModelUpsert,
        ProviderUpsert, ReviewModelGroups,
    };

    use super::{ConfigPatch, GetConfigHubResponse, RefreshAiModelsResponse};
    use crate::commands::{map_ai_providers, snap_from, AiProviderInfo, AppState};
    use crate::snapshot::AppSnapshot;

    pub(super) fn hub_response(app: &App, warnings: Vec<String>) -> GetConfigHubResponse {
        GetConfigHubResponse::from_view(config_hub_view(&app.config), warnings)
    }

    fn bump_revision(state: &AppState) {
        state
            .desktop_revision
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    const fn feature_allows_mode(features: &FeatureFlags, mode: DiffMode) -> bool {
        match mode {
            DiffMode::Branch => features.view_branch,
            DiffMode::Unstaged => features.view_unstaged,
            DiffMode::Staged => features.view_staged,
            DiffMode::History => features.view_history,
            DiffMode::Conflicts => features.view_conflicts,
            DiffMode::Hidden => features.view_hidden,
            DiffMode::Tour => features.view_tour,
            DiffMode::PrDiff => true, // PrDiff is always allowed when the tab is in PR mode
        }
    }

    fn clamp_tab_mode_to_features(app: &mut App) {
        let current = app.tab().mode;
        if feature_allows_mode(&app.config.features, current) {
            return;
        }
        for candidate in [
            DiffMode::Branch,
            DiffMode::Unstaged,
            DiffMode::Staged,
            DiffMode::History,
            DiffMode::Conflicts,
            DiffMode::Hidden,
        ] {
            if feature_allows_mode(&app.config.features, candidate) {
                app.tab_mut().set_mode(candidate);
                return;
            }
        }
    }

    fn apply_config_side_effects(app: &mut App, watched_changed: bool) {
        if watched_changed {
            app.tab_mut().watched_config = app.config.watched.clone();
            app.tab_mut().refresh_watched_files();
        }
        // The filter and the header resolve the per-repo tables off the tab, so
        // a config change has to reach the tabs that are already open.
        app.sync_repo_rules_to_tabs();
        clamp_tab_mode_to_features(app);
    }

    fn list_providers(app: &App) -> Vec<AiProviderInfo> {
        let hub = &app.config.ai_hub;
        let selection = hub.resolve_default_selection(&app.config.agent);
        map_ai_providers(
            hub,
            selection.provider_id.as_deref(),
            selection.model_id.as_deref(),
        )
    }

    pub(super) fn apply_patch(
        state: &AppState,
        patch: ConfigPatch,
    ) -> Result<GetConfigHubResponse, String> {
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        let watched_changed = apply_config_field(&mut app.config, &patch.key, patch.value);
        save_config(&app.config).map_err(|e| e.to_string())?;
        apply_config_side_effects(&mut app, watched_changed);
        // Only resync session selection when AI Hub defaults actually changed —
        // theme/display patches must not wipe a palette pick.
        if patch.key.starts_with("ai_hub.") {
            app.sync_ai_selection_from_defaults();
        }
        bump_revision(state);
        Ok(hub_response(&app, Vec::new()))
    }

    pub(super) fn save_global(state: &AppState) -> Result<AppSnapshot, String> {
        let app = state.app.lock().map_err(|e| e.to_string())?;
        save_config(&app.config).map_err(|e| e.to_string())?;
        drop(app);
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        app.notify("Saved to global config");
        bump_revision(state);
        Ok(snap_from(&app, state))
    }

    pub(super) fn persist_review_models(
        config: &mut ErConfig,
        groups: ReviewModelGroups,
        save: impl FnOnce(&ErConfig) -> Result<(), String>,
    ) -> Result<(), String> {
        config
            .ai_hub
            .validate_review_models(&groups)
            .map_err(|error| error.to_string())?;
        let mut next = config.clone();
        next.ai_hub.review_models = groups;
        save(&next)?;
        *config = next;
        Ok(())
    }

    pub(super) fn review_flow_models(
        state: &AppState,
        groups: ReviewModelGroups,
    ) -> Result<GetConfigHubResponse, String> {
        let mut app = state.app.lock().map_err(|error| error.to_string())?;
        persist_review_models(&mut app.config, groups, |next| {
            save_config(next).map_err(|error| error.to_string())
        })?;
        bump_revision(state);
        Ok(hub_response(&app, Vec::new()))
    }

    pub(super) fn refresh_models(
        state: &AppState,
        provider_id: Option<String>,
        force: bool,
    ) -> Result<RefreshAiModelsResponse, String> {
        let mut errors = Vec::new();
        let targets: Vec<(String, Vec<String>)> = {
            let app = state.app.lock().map_err(|e| e.to_string())?;
            if !app.config.features.model_discovery {
                return Ok(RefreshAiModelsResponse {
                    providers: list_providers(&app),
                    errors: vec!["Model discovery is turned off in Settings → AI".into()],
                });
            }
            app.config
                .ai_hub
                .providers
                .iter()
                .filter(|(id, p)| {
                    !p.models_command.is_empty()
                        && provider_id
                            .as_ref()
                            .map(|want| want == id.as_str())
                            .unwrap_or(true)
                })
                .map(|(id, p)| (id.clone(), p.models_command.clone()))
                .collect()
        };

        for (pid, command) in targets {
            let cache = er_engine::model_discovery::load_valid_cache(&pid, &command);
            let fresh = cache
                .as_ref()
                .map(er_engine::model_discovery::cache_is_fresh)
                .unwrap_or(false);
            if fresh && !force {
                if let Some(cache) = cache {
                    let mut app = state.app.lock().map_err(|e| e.to_string())?;
                    app.apply_discovered_models(&pid, &cache.models);
                }
                continue;
            }

            {
                let mut app = state.app.lock().map_err(|e| e.to_string())?;
                if !app.model_discovery_inflight.insert(pid.clone()) {
                    continue;
                }
            }

            let result = er_engine::model_discovery::discover_and_cache(&pid, &command);

            let mut app = state.app.lock().map_err(|e| e.to_string())?;
            app.model_discovery_inflight.remove(&pid);
            match result {
                Ok(models) => {
                    app.apply_discovered_models(&pid, &models);
                }
                Err(e) => {
                    // Stale-while-revalidate: keep serving prior cache if present.
                    if let Some(cache) = cache {
                        app.apply_discovered_models(&pid, &cache.models);
                    }
                    log::warn!("refresh_ai_models({pid}): {e}");
                    errors.push(format!("{pid}: {e:#}"));
                }
            }
        }

        bump_revision(state);
        let app = state.app.lock().map_err(|e| e.to_string())?;
        Ok(RefreshAiModelsResponse {
            providers: list_providers(&app),
            errors,
        })
    }

    pub(super) fn provider_upsert(
        state: &AppState,
        provider: ProviderUpsert,
    ) -> Result<GetConfigHubResponse, String> {
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        let edit = er_engine::config::hub_edits::upsert_provider(&mut app.config, provider)?;
        save_config(&app.config).map_err(|e| e.to_string())?;
        if edit.defaults_changed {
            app.sync_ai_selection_from_defaults();
        }
        bump_revision(state);
        Ok(hub_response(&app, edit.warnings))
    }

    pub(super) fn provider_delete(
        state: &AppState,
        provider_id: String,
    ) -> Result<GetConfigHubResponse, String> {
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        er_engine::config::hub_edits::delete_provider(&mut app.config, &provider_id)?;
        save_config(&app.config).map_err(|e| e.to_string())?;
        app.sync_ai_selection_from_defaults();
        bump_revision(state);
        Ok(hub_response(&app, Vec::new()))
    }

    pub(super) fn model_upsert(
        state: &AppState,
        provider_id: String,
        model: ModelUpsert,
    ) -> Result<GetConfigHubResponse, String> {
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        let edit =
            er_engine::config::hub_edits::upsert_model(&mut app.config, &provider_id, model)?;
        save_config(&app.config).map_err(|e| e.to_string())?;
        bump_revision(state);
        Ok(hub_response(&app, edit.warnings))
    }

    pub(super) fn model_delete(
        state: &AppState,
        provider_id: String,
        model_id: String,
    ) -> Result<GetConfigHubResponse, String> {
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        er_engine::config::hub_edits::delete_model(&mut app.config, &provider_id, &model_id)?;
        save_config(&app.config).map_err(|e| e.to_string())?;
        app.sync_ai_selection_from_defaults();
        bump_revision(state);
        Ok(hub_response(&app, Vec::new()))
    }
}

#[cfg(all(test, not(feature = "v1-service")))]
mod review_model_settings_tests {
    use super::flag_off::persist_review_models;
    use er_engine::config::{supplement_ai_hub, ErConfig, ReviewModelRef};

    fn config() -> ErConfig {
        let mut config = ErConfig::default();
        supplement_ai_hub(&mut config.ai_hub);
        config
    }

    #[test]
    fn review_model_save_keeps_standalone_defaults_and_persists_exact_groups() {
        let mut config = config();
        let original_provider = config.ai_hub.default_provider.clone();
        let original_model = config.ai_hub.default_model.clone();
        let mut groups = config.ai_hub.review_models.clone();
        groups.fast = vec![ReviewModelRef {
            provider_id: "codex".into(),
            model_id: "gpt-6-luna".into(),
        }];
        let mut saved = None;
        persist_review_models(&mut config, groups.clone(), |next| {
            saved = Some(next.ai_hub.review_models.clone());
            Ok(())
        })
        .unwrap();
        assert_eq!(saved, Some(groups.clone()));
        assert_eq!(config.ai_hub.review_models, groups);
        assert_eq!(config.ai_hub.default_provider, original_provider);
        assert_eq!(config.ai_hub.default_model, original_model);
    }

    #[test]
    fn review_model_save_failure_leaves_live_config_unchanged() {
        let mut config = config();
        let original = serde_json::to_value(&config).unwrap();
        let mut groups = config.ai_hub.review_models.clone();
        groups.fast.reverse();
        let result = persist_review_models(&mut config, groups, |_| Err("disk full".into()));
        assert_eq!(result.unwrap_err(), "disk full");
        assert_eq!(serde_json::to_value(&config).unwrap(), original);
    }

    #[test]
    fn review_model_validation_failure_does_not_persist_or_mutate() {
        let mut config = config();
        let original = serde_json::to_value(&config).unwrap();
        let mut groups = config.ai_hub.review_models.clone();
        groups.advanced.clear();
        let mut persisted = false;
        let result = persist_review_models(&mut config, groups, |_| {
            persisted = true;
            Ok(())
        });
        assert!(result.is_err());
        assert!(!persisted);
        assert_eq!(serde_json::to_value(&config).unwrap(), original);
    }
}
