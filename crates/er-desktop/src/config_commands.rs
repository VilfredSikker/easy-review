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
                .then_some(selection.model_id.as_deref())
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
                        is_selected: resolved_model == Some(m.id.as_str()),
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
    let default_selection = app
        .config
        .ai_hub
        .resolve_default_selection(&app.config.agent);
    Ok(GetConfigHubResponse {
        settings,
        providers,
        active_effort: default_selection.effort,
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
    // Only resync session selection when AI Hub defaults actually changed —
    // theme/display patches must not wipe a palette pick.
    if patch.key.starts_with("ai_hub.") {
        app.sync_ai_selection_from_defaults();
    }
    state
        .desktop_revision
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let settings = desktop_settings_snapshot(&app.config, &repo_root);
    let providers = list_providers_inner(&app);
    let default_selection = app
        .config
        .ai_hub
        .resolve_default_selection(&app.config.agent);
    Ok(GetConfigHubResponse {
        settings,
        providers,
        active_effort: default_selection.effort,
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

// ── Uninstall ───────────────────────────────────────────────────────────────

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UninstallTargetDto {
    pub kind: String,
    pub path: String,
    pub exists: bool,
    pub description: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UninstallPreview {
    pub targets: Vec<UninstallTargetDto>,
    pub existing_count: usize,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UninstallResult {
    pub removed: Vec<String>,
    pub deferred: Vec<String>,
    pub missing: Vec<String>,
    pub failed: Vec<String>,
    /// Frontend should quit the app after a successful uninstall.
    pub should_quit: bool,
}

#[tauri::command]
pub async fn preview_uninstall(
    request: Option<er_engine::uninstall::UninstallOptions>,
) -> Result<UninstallPreview, String> {
    crate::commands::run_blocking(move || {
        let opts = request.unwrap_or_default();
        let targets = er_engine::uninstall::plan(&opts);
        let existing_count = targets.iter().filter(|t| t.exists).count();
        Ok(UninstallPreview {
            targets: targets
                .into_iter()
                .map(|t| UninstallTargetDto {
                    kind: t.kind.label().to_string(),
                    path: t.path.display().to_string(),
                    exists: t.exists,
                    description: t.description(),
                })
                .collect(),
            existing_count,
        })
    })
    .await
}

#[tauri::command]
pub async fn run_uninstall(
    app_handle: tauri::AppHandle,
    request: Option<er_engine::uninstall::UninstallOptions>,
) -> Result<UninstallResult, String> {
    let result = crate::commands::run_blocking(move || {
        let opts = request.unwrap_or_default();
        let existing = er_engine::uninstall::existing_targets(&opts);
        if existing.is_empty() {
            return Ok(UninstallResult {
                removed: vec![],
                deferred: vec![],
                missing: vec![],
                failed: vec![],
                should_quit: false,
            });
        }

        let report = er_engine::uninstall::execute(&existing);
        if !report.deferred.is_empty() {
            if let Err(e) = er_engine::uninstall::schedule_deferred_removal(&report.deferred) {
                let msg = format!("Could not schedule removal of in-use paths: {e}");
                log::error!("run_uninstall: {msg}");
                return Err(msg);
            }
        }

        Ok(UninstallResult {
            removed: report
                .removed
                .iter()
                .map(|p| p.display().to_string())
                .collect(),
            deferred: report
                .deferred
                .iter()
                .map(|p| p.display().to_string())
                .collect(),
            missing: report
                .missing
                .iter()
                .map(|p| p.display().to_string())
                .collect(),
            failed: report
                .failed
                .iter()
                .map(|(p, e)| format!("{}: {e}", p.display()))
                .collect(),
            // Must quit whenever deferred deletes were scheduled — otherwise the
            // waiter never sees this PID exit.
            should_quit: report.is_success() || !report.deferred.is_empty(),
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
