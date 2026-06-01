//! Desktop settings page — load/apply/save `ErConfig` (excludes diff-view fields).

use er_engine::app::DiffMode;
use er_engine::config::{
    apply_config_field, desktop_settings_snapshot, save_config, save_config_local,
    ConfigFieldValue, DesktopSettingsSnapshot, ErConfig, FeatureFlags,
};
use tauri::State;

use crate::commands::{snap_from, AiModelInfo, AiProviderInfo, AppState};
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
}

fn capture_baseline(state: &AppState, config: &ErConfig) {
    if let Ok(mut g) = state.config_edit_baseline.lock() {
        *g = Some(config.clone());
    }
}

fn feature_allows_mode(features: &FeatureFlags, mode: DiffMode) -> bool {
    match mode {
        DiffMode::Branch => features.view_branch,
        DiffMode::Unstaged => features.view_unstaged,
        DiffMode::Staged => features.view_staged,
        DiffMode::History => features.view_history,
        DiffMode::Conflicts => features.view_conflicts,
        DiffMode::Hidden => features.view_hidden,
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
    clamp_tab_mode_to_features(app);
}

fn list_providers_inner(app: &er_engine::app::App) -> Vec<AiProviderInfo> {
    let hub = &app.config.ai_hub;
    let current_provider = app.current_ai_provider.as_deref();
    let current_model = app.current_ai_model.as_deref();
    let resolved_provider = hub.resolve_provider_id(current_provider);

    hub.providers
        .iter()
        .map(|(id, cfg)| {
            let resolved_model = hub.resolve_model_id(id, current_model);
            AiProviderInfo {
                id: id.clone(),
                label: cfg.display_name(id),
                is_selected: resolved_provider.as_deref() == Some(id.as_str()),
                models: cfg
                    .models
                    .iter()
                    .map(|m| AiModelInfo {
                        id: m.id.clone(),
                        label: m.display_name(),
                        is_selected: resolved_model.as_deref() == Some(m.id.as_str()),
                        description: m.description.clone(),
                        cost_per_1k_in: m.cost_per_1k_in,
                        cost_per_1k_out: m.cost_per_1k_out,
                        avg_latency_ms: m.avg_latency_ms,
                    })
                    .collect(),
            }
        })
        .collect()
}

#[tauri::command]
pub fn get_config_hub(
    reset_baseline: Option<bool>,
    state: State<AppState>,
) -> Result<GetConfigHubResponse, String> {
    let app = state.app.lock().map_err(|e| e.to_string())?;
    let repo_root = app.tab().repo_root.clone();
    let settings = desktop_settings_snapshot(&app.config, &repo_root);
    let providers = list_providers_inner(&app);
    if reset_baseline.unwrap_or(true) {
        capture_baseline(&state, &app.config);
    }
    Ok(GetConfigHubResponse {
        settings,
        providers,
    })
}

#[tauri::command]
pub fn apply_config_patch(
    patch: ConfigPatch,
    state: State<AppState>,
) -> Result<GetConfigHubResponse, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let repo_root = app.tab().repo_root.clone();
    let watched_changed = apply_config_field(&mut app.config, &patch.key, patch.value);
    apply_config_side_effects(&mut app, watched_changed);
    state
        .desktop_revision
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let settings = desktop_settings_snapshot(&app.config, &repo_root);
    let providers = list_providers_inner(&app);
    Ok(GetConfigHubResponse {
        settings,
        providers,
    })
}

#[tauri::command]
pub fn reset_config_draft(state: State<AppState>) -> Result<GetConfigHubResponse, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let baseline = state
        .config_edit_baseline
        .lock()
        .map_err(|e| e.to_string())?
        .clone()
        .ok_or_else(|| "No settings baseline — open settings first".to_string())?;
    app.config = baseline;
    let repo_root = app.tab().repo_root.clone();
    app.tab_mut().watched_config = app.config.watched.clone();
    app.tab_mut().refresh_watched_files();
    state
        .desktop_revision
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let settings = desktop_settings_snapshot(&app.config, &repo_root);
    let providers = list_providers_inner(&app);
    Ok(GetConfigHubResponse {
        settings,
        providers,
    })
}

#[tauri::command]
pub fn save_config_local_cmd(state: State<AppState>) -> Result<AppSnapshot, String> {
    let app = state.app.lock().map_err(|e| e.to_string())?;
    let repo_root = app.tab().repo_root.clone();
    save_config_local(&app.config, &repo_root).map_err(|e| e.to_string())?;
    capture_baseline(&state, &app.config);
    drop(app);
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    app.notify("Saved to .er-config.toml");
    state
        .desktop_revision
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Ok(snap_from(&app, &state))
}

#[tauri::command]
pub fn save_config_global_cmd(state: State<AppState>) -> Result<AppSnapshot, String> {
    let app = state.app.lock().map_err(|e| e.to_string())?;
    save_config(&app.config).map_err(|e| e.to_string())?;
    capture_baseline(&state, &app.config);
    drop(app);
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    app.notify("Saved to global config");
    state
        .desktop_revision
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Ok(snap_from(&app, &state))
}

#[tauri::command]
pub fn set_ai_hub_defaults(
    provider_id: String,
    model_id: Option<String>,
    state: State<AppState>,
) -> Result<GetConfigHubResponse, String> {
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    if !app.config.ai_hub.providers.contains_key(&provider_id) {
        return Err(format!("Unknown provider: {provider_id}"));
    }
    if let Some(ref mid) = model_id {
        let provider = app.config.ai_hub.providers.get(&provider_id).unwrap();
        if !provider.models.is_empty() && !provider.models.iter().any(|m| &m.id == mid) {
            return Err(format!(
                "Unknown model '{mid}' for provider '{provider_id}'"
            ));
        }
    }
    app.config.ai_hub.default_provider = Some(provider_id.clone());
    app.config.ai_hub.default_model = model_id.clone();
    app.current_ai_provider = Some(provider_id);
    app.current_ai_model = model_id;
    let repo_root = app.tab().repo_root.clone();
    state
        .desktop_revision
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let settings = desktop_settings_snapshot(&app.config, &repo_root);
    let providers = list_providers_inner(&app);
    Ok(GetConfigHubResponse {
        settings,
        providers,
    })
}
