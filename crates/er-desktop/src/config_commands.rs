//! Desktop settings page — load/apply/save `ErConfig` (excludes diff-view fields).

use er_engine::app::DiffMode;
use er_engine::config::{
    apply_config_field, desktop_settings_snapshot, save_config, ConfigFieldValue,
    DesktopSettingsSnapshot, FeatureFlags, ModelUpsert, ProviderUpsert,
};
use tauri::State;

use crate::commands::{map_ai_providers, snap_from, AiProviderInfo, AppState};
use crate::snapshot::AppSnapshot;

#[derive(serde::Deserialize)]
pub struct ConfigPatch {
    pub key: String,
    pub value: ConfigFieldValue,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetConfigHubResponse {
    pub settings: DesktopSettingsSnapshot,
    pub providers: Vec<AiProviderInfo>,
    pub review_models: er_engine::config::ReviewModelGroups,
    pub active_effort: Option<String>,
    /// Soft validation messages from the last upsert (e.g. missing `{prompt}`).
    #[serde(default)]
    pub warnings: Vec<String>,
    /// Known `family =` ids for the provider editor (plus empty = detect).
    pub family_options: Vec<String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshAiModelsResponse {
    pub providers: Vec<AiProviderInfo>,
    /// One line per provider whose listing failed; the prior cache still applies.
    pub errors: Vec<String>,
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

fn clamp_tab_mode_to_features(app: &mut er_engine::app::App) {
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

fn apply_config_side_effects(app: &mut er_engine::app::App, watched_changed: bool) {
    if watched_changed {
        app.tab_mut().watched_config = app.config.watched.clone();
        app.tab_mut().refresh_watched_files();
    }
    // The filter and the header resolve the per-repo tables off the tab, so a
    // config change has to reach the tabs that are already open.
    app.sync_repo_rules_to_tabs();
    clamp_tab_mode_to_features(app);
}

fn list_providers_inner(app: &er_engine::app::App) -> Vec<AiProviderInfo> {
    let hub = &app.config.ai_hub;
    let selection = hub.resolve_default_selection(&app.config.agent);
    map_ai_providers(
        hub,
        selection.provider_id.as_deref(),
        selection.model_id.as_deref(),
    )
}

fn family_options() -> Vec<String> {
    er_engine::config::KNOWN_FAMILY_IDS
        .iter()
        .map(|s| (*s).to_string())
        .collect()
}

#[tauri::command]
pub fn get_config_hub(state: State<AppState>) -> Result<GetConfigHubResponse, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    // Reopening Settings is the desktop's reload point: a file the user repaired
    // in place is adopted here, before the page reads `app.config`.
    app.reload_config_if_failed();
    let repo_key = app.tab().rules_key();
    let settings = desktop_settings_snapshot(
        &app.config,
        &app.tab().repo_root,
        &repo_key,
        &changed_paths(&app),
        er_engine::config::load_risk_rules_table(&repo_key),
    );
    let providers = list_providers_inner(&app);
    let default_selection = app
        .config
        .ai_hub
        .resolve_default_selection(&app.config.agent);
    Ok(GetConfigHubResponse {
        settings,
        providers,
        review_models: app.config.ai_hub.review_models.clone(),
        active_effort: default_selection.effort,
        warnings: Vec::new(),
        family_options: family_options(),
    })
}

#[tauri::command]
pub async fn apply_config_patch(
    patch: ConfigPatch,
    state: State<'_, AppState>,
) -> Result<GetConfigHubResponse, String> {
    let state = state.inner().clone();
    // Off the main thread: this writes the config to disk, runs the side
    // effects (watched-file re-discovery), and builds the whole settings
    // snapshot, all under the App lock.
    crate::commands::run_blocking(move || {
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        let repo_root = app.tab().repo_root.clone();
        let repo_key = app.tab().rules_key();
        let watched_changed = apply_config_field(&mut app.config, &patch.key, patch.value);
        save_config(&app.config).map_err(|e| e.to_string())?;
        apply_config_side_effects(&mut app, watched_changed);
        // Only resync session selection when AI Hub defaults actually changed —
        // theme/display patches must not wipe a palette pick.
        if patch.key.starts_with("ai_hub.") {
            app.sync_ai_selection_from_defaults();
        }
        state
            .desktop_revision
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let settings = desktop_settings_snapshot(
            &app.config,
            &repo_root,
            &repo_key,
            &changed_paths(&app),
            er_engine::config::load_risk_rules_table(&repo_key),
        );
        let providers = list_providers_inner(&app);
        let default_selection = app
            .config
            .ai_hub
            .resolve_default_selection(&app.config.agent);
        Ok(GetConfigHubResponse {
            settings,
            providers,
            review_models: app.config.ai_hub.review_models.clone(),
            active_effort: default_selection.effort,
            warnings: Vec::new(),
            family_options: family_options(),
        })
    })
    .await
}

#[tauri::command]
pub async fn save_config_global_cmd(state: State<'_, AppState>) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    // Off the main thread: writes the config file, then builds a full
    // snapshot, both under the App lock.
    crate::commands::run_blocking(move || {
        let app = state.app.lock().map_err(|e| e.to_string())?;
        save_config(&app.config).map_err(|e| e.to_string())?;
        drop(app);
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        app.notify("Saved to global config");
        state
            .desktop_revision
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(snap_from(&app, &state))
    })
    .await
}

// ── Uninstall ───────────────────────────────────────────────────────────────

pub use er_engine::uninstall::{UninstallPreview, UninstallResult, UninstallTargetDto};

#[tauri::command]
pub async fn preview_uninstall(
    request: Option<er_engine::uninstall::UninstallOptions>,
) -> Result<UninstallPreview, String> {
    crate::commands::run_blocking(move || {
        Ok(er_engine::uninstall::preview(&request.unwrap_or_default()))
    })
    .await
}

#[tauri::command]
pub async fn run_uninstall(
    app_handle: tauri::AppHandle,
    request: Option<er_engine::uninstall::UninstallOptions>,
) -> Result<UninstallResult, String> {
    let result = crate::commands::run_blocking(move || {
        er_engine::uninstall::run(&request.unwrap_or_default()).inspect_err(|msg| {
            log::error!("run_uninstall: {msg}");
        })
    })
    .await?;

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

// ── AI provider / model editor ──────────────────────────────────────────────

fn bump_revision(state: &AppState) {
    state
        .desktop_revision
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

fn hub_response(app: &er_engine::app::App) -> GetConfigHubResponse {
    hub_response_with_warnings(app, Vec::new())
}

/// The active tab's changed paths, for the importance list's per-file answers.
///
/// Read off the tab rather than off the working tree: the files that raise the
/// question "why does this one have that tier" are the ones in front of the
/// reviewer, and the tab is what holds them.
fn changed_paths(app: &er_engine::app::App) -> Vec<String> {
    app.tab().files.iter().map(|f| f.path.clone()).collect()
}

fn hub_response_with_warnings(
    app: &er_engine::app::App,
    warnings: Vec<String>,
) -> GetConfigHubResponse {
    let repo_key = app.tab().rules_key();
    let settings = desktop_settings_snapshot(
        &app.config,
        &app.tab().repo_root,
        &repo_key,
        &changed_paths(app),
        er_engine::config::load_risk_rules_table(&repo_key),
    );
    let providers = list_providers_inner(app);
    let default_selection = app
        .config
        .ai_hub
        .resolve_default_selection(&app.config.agent);
    GetConfigHubResponse {
        settings,
        providers,
        review_models: app.config.ai_hub.review_models.clone(),
        active_effort: default_selection.effort,
        warnings,
        family_options: family_options(),
    }
}

fn persist_review_models(
    config: &mut er_engine::config::ErConfig,
    groups: er_engine::config::ReviewModelGroups,
    save: impl FnOnce(&er_engine::config::ErConfig) -> Result<(), String>,
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

#[tauri::command]
pub async fn set_review_flow_models(
    groups: er_engine::config::ReviewModelGroups,
    state: State<'_, AppState>,
) -> Result<GetConfigHubResponse, String> {
    let state = state.inner().clone();
    crate::commands::run_blocking(move || {
        let mut app = state.app.lock().map_err(|error| error.to_string())?;
        persist_review_models(&mut app.config, groups, |next| {
            save_config(next).map_err(|error| error.to_string())
        })?;
        bump_revision(&state);
        Ok(hub_response(&app))
    })
    .await
}

#[tauri::command]
pub async fn refresh_ai_models(
    provider_id: Option<String>,
    force: Option<bool>,
    state: State<'_, AppState>,
) -> Result<RefreshAiModelsResponse, String> {
    let force = force.unwrap_or(false);
    let state_clone = state.inner().clone();
    crate::commands::run_blocking(move || {
        let mut errors = Vec::new();
        let targets: Vec<(String, Vec<String>)> = {
            let app = state_clone.app.lock().map_err(|e| e.to_string())?;
            if !app.config.features.model_discovery {
                return Ok(RefreshAiModelsResponse {
                    providers: list_providers_inner(&app),
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
                    let mut app = state_clone.app.lock().map_err(|e| e.to_string())?;
                    app.apply_discovered_models(&pid, &cache.models);
                }
                continue;
            }

            {
                let mut app = state_clone.app.lock().map_err(|e| e.to_string())?;
                if !app.model_discovery_inflight.insert(pid.clone()) {
                    continue;
                }
            }

            let result = er_engine::model_discovery::discover_and_cache(&pid, &command);

            let mut app = state_clone.app.lock().map_err(|e| e.to_string())?;
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

        bump_revision(&state_clone);
        let app = state_clone.app.lock().map_err(|e| e.to_string())?;
        Ok(RefreshAiModelsResponse {
            providers: list_providers_inner(&app),
            errors,
        })
    })
    .await
}

#[tauri::command]
pub async fn upsert_ai_provider(
    provider: ProviderUpsert,
    state: State<'_, AppState>,
) -> Result<GetConfigHubResponse, String> {
    let state = state.inner().clone();
    crate::commands::run_blocking(move || {
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        let edit = er_engine::config::hub_edits::upsert_provider(&mut app.config, provider)?;
        save_config(&app.config).map_err(|e| e.to_string())?;
        if edit.defaults_changed {
            app.sync_ai_selection_from_defaults();
        }
        bump_revision(&state);
        Ok(hub_response_with_warnings(&app, edit.warnings))
    })
    .await
}

#[tauri::command]
pub async fn delete_ai_provider(
    provider_id: String,
    state: State<'_, AppState>,
) -> Result<GetConfigHubResponse, String> {
    let state = state.inner().clone();
    crate::commands::run_blocking(move || {
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        er_engine::config::hub_edits::delete_provider(&mut app.config, &provider_id)?;
        save_config(&app.config).map_err(|e| e.to_string())?;
        app.sync_ai_selection_from_defaults();
        bump_revision(&state);
        Ok(hub_response(&app))
    })
    .await
}

#[tauri::command]
pub async fn upsert_ai_model(
    provider_id: String,
    model: ModelUpsert,
    state: State<'_, AppState>,
) -> Result<GetConfigHubResponse, String> {
    let state = state.inner().clone();
    crate::commands::run_blocking(move || {
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        let edit =
            er_engine::config::hub_edits::upsert_model(&mut app.config, &provider_id, model)?;
        save_config(&app.config).map_err(|e| e.to_string())?;
        bump_revision(&state);
        Ok(hub_response_with_warnings(&app, edit.warnings))
    })
    .await
}

#[tauri::command]
pub async fn delete_ai_model(
    provider_id: String,
    model_id: String,
    state: State<'_, AppState>,
) -> Result<GetConfigHubResponse, String> {
    let state = state.inner().clone();
    crate::commands::run_blocking(move || {
        let mut app = state.app.lock().map_err(|e| e.to_string())?;
        er_engine::config::hub_edits::delete_model(&mut app.config, &provider_id, &model_id)?;
        save_config(&app.config).map_err(|e| e.to_string())?;
        app.sync_ai_selection_from_defaults();
        bump_revision(&state);
        Ok(hub_response(&app))
    })
    .await
}

#[cfg(test)]
mod review_model_settings_tests {
    use super::persist_review_models;
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
