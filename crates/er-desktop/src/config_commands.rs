//! Desktop settings page — load/apply/save `ErConfig` (excludes diff-view fields).

use er_engine::app::DiffMode;
use er_engine::config::{
    apply_config_field, desktop_settings_snapshot, save_config, ConfigFieldValue,
    DesktopSettingsSnapshot, FeatureFlags,
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
    pub active_effort: Option<String>,
}

fn feature_allows_mode(features: &FeatureFlags, mode: DiffMode) -> bool {
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
    clamp_tab_mode_to_features(app);
}

fn list_providers_inner(app: &er_engine::app::App) -> Vec<AiProviderInfo> {
    let hub = &app.config.ai_hub;
    let selection = hub.resolve_default_selection(&app.config.agent);
    let resolved_provider = selection.provider_id.as_deref();

    hub.providers
        .iter()
        .map(|(id, cfg)| {
            let resolved_model = (selection.provider_id.as_deref() == Some(id.as_str()))
                .then(|| selection.model_id.as_deref())
                .flatten();
            AiProviderInfo {
                id: id.clone(),
                label: cfg.display_name(id),
                is_selected: resolved_provider == Some(id.as_str()),
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
                        effort_levels: m.effort_levels.clone(),
                    })
                    .collect(),
            }
        })
        .collect()
}

#[tauri::command]
pub fn get_config_hub(state: State<AppState>) -> Result<GetConfigHubResponse, String> {
    let app = state.app.lock().map_err(|e| e.to_string())?;
    let repo_root = app.tab().repo_root.clone();
    let settings = desktop_settings_snapshot(&app.config, &repo_root);
    let providers = list_providers_inner(&app);
    Ok(GetConfigHubResponse {
        settings,
        providers,
        active_effort: app.current_ai_effort.clone(),
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
    save_config(&app.config).map_err(|e| e.to_string())?;
    apply_config_side_effects(&mut app, watched_changed);
    app.sync_ai_selection_from_defaults();
    state
        .desktop_revision
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let settings = desktop_settings_snapshot(&app.config, &repo_root);
    let providers = list_providers_inner(&app);
    Ok(GetConfigHubResponse {
        settings,
        providers,
        active_effort: app.current_ai_effort.clone(),
    })
}

#[tauri::command]
pub fn save_config_global_cmd(state: State<AppState>) -> Result<AppSnapshot, String> {
    let app = state.app.lock().map_err(|e| e.to_string())?;
    save_config(&app.config).map_err(|e| e.to_string())?;
    drop(app);
    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    app.notify("Saved to global config");
    state
        .desktop_revision
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Ok(snap_from(&app, &state))
}
