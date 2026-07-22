//! Desktop settings page — load/apply/save `ErConfig` (excludes diff-view fields).

use er_engine::app::DiffMode;
use er_engine::config::{
    apply_config_field, desktop_settings_snapshot, save_config, ConfigFieldValue,
    DesktopSettingsSnapshot, FeatureFlags,
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
    pub active_effort: Option<String>,
    /// Soft validation messages from the last upsert (e.g. missing `{prompt}`).
    #[serde(default)]
    pub warnings: Vec<String>,
    /// Known `family =` ids for the provider editor (plus empty = detect).
    pub family_options: Vec<String>,
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
        warnings: Vec::new(),
        family_options: family_options(),
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
        warnings: Vec::new(),
        family_options: family_options(),
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

// ── AI provider / model editor ──────────────────────────────────────────────

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUpsertDto {
    pub id: String,
    pub original_id: Option<String>,
    pub label: Option<String>,
    pub command: String,
    pub args: String,
    pub family: Option<String>,
    pub models_command: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelUpsertDto {
    pub id: String,
    pub original_id: Option<String>,
    pub label: Option<String>,
    pub description: Option<String>,
    pub args: String,
    pub effort_levels: Vec<String>,
    pub cost_per_1k_in: Option<f32>,
    pub cost_per_1k_out: Option<f32>,
    pub avg_latency_ms: Option<u32>,
}

fn bump_revision(state: &AppState) {
    state
        .desktop_revision
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

fn hub_response(app: &er_engine::app::App) -> GetConfigHubResponse {
    hub_response_with_warnings(app, Vec::new())
}

fn hub_response_with_warnings(
    app: &er_engine::app::App,
    warnings: Vec<String>,
) -> GetConfigHubResponse {
    let repo_root = app.tab().repo_root.clone();
    let settings = desktop_settings_snapshot(&app.config, &repo_root);
    let providers = list_providers_inner(app);
    let default_selection = app
        .config
        .ai_hub
        .resolve_default_selection(&app.config.agent);
    GetConfigHubResponse {
        settings,
        providers,
        active_effort: default_selection.effort,
        warnings,
        family_options: family_options(),
    }
}

#[tauri::command]
pub async fn refresh_ai_models(
    provider_id: Option<String>,
    force: Option<bool>,
    state: State<'_, AppState>,
) -> Result<Vec<AiProviderInfo>, String> {
    let force = force.unwrap_or(false);
    let state_clone = state.inner().clone();
    crate::commands::run_blocking(move || {
        let targets: Vec<(String, Vec<String>)> = {
            let app = state_clone.app.lock().map_err(|e| e.to_string())?;
            if !app.config.features.model_discovery {
                return Ok(list_providers_inner(&app));
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
                }
            }
        }

        bump_revision(&state_clone);
        let app = state_clone.app.lock().map_err(|e| e.to_string())?;
        Ok(list_providers_inner(&app))
    })
    .await
}

#[tauri::command]
pub fn upsert_ai_provider(
    provider: ProviderUpsertDto,
    state: State<AppState>,
) -> Result<GetConfigHubResponse, String> {
    use er_engine::config::{
        save_config, split_shell_args, validate_provider_config, AiProviderConfig,
    };

    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let id = provider.id.trim().to_string();
    let original_id = provider
        .original_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != id.as_str())
        .map(|s| s.to_string());

    let mut existing_models = Vec::new();
    let mut existing_tombstones = Vec::new();
    let source_key = original_id.as_deref().unwrap_or(id.as_str());
    if let Some(prev) = app.config.ai_hub.providers.get(source_key) {
        existing_models = prev.models.clone();
        existing_tombstones = prev.removed_catalog_models.clone();
    }

    let cfg = AiProviderConfig {
        label: provider.label.filter(|s| !s.trim().is_empty()),
        command: provider.command,
        args: split_shell_args(&provider.args),
        family: provider.family.filter(|s| !s.trim().is_empty()),
        models_command: provider
            .models_command
            .map(|s| split_shell_args(&s))
            .unwrap_or_default(),
        models: existing_models,
        removed_catalog_models: existing_tombstones,
    };
    let warnings = validate_provider_config(&id, &cfg)?;

    let renamed_from = original_id.clone();
    if let Some(old) = original_id {
        app.config.ai_hub.providers.remove(&old);
        if app.config.ai_hub.default_provider.as_deref() == Some(old.as_str()) {
            app.config.ai_hub.default_provider = Some(id.clone());
        }
    }
    app.config.ai_hub.providers.insert(id.clone(), cfg);
    save_config(&app.config).map_err(|e| e.to_string())?;
    if app.config.ai_hub.default_provider.as_deref() == Some(id.as_str()) || renamed_from.is_some()
    {
        app.sync_ai_selection_from_defaults();
    }
    bump_revision(&state);
    Ok(hub_response_with_warnings(&app, warnings))
}

#[tauri::command]
pub fn delete_ai_provider(
    provider_id: String,
    state: State<AppState>,
) -> Result<GetConfigHubResponse, String> {
    use er_engine::config::{remove_ai_provider, save_config};

    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let id = provider_id.trim().to_string();
    if id.is_empty() {
        return Err("provider_id must not be empty".into());
    }
    remove_ai_provider(&mut app.config.ai_hub, &id);
    let agent = app.config.agent.clone();
    if app.config.ai_hub.default_provider.as_deref() == Some(id.as_str()) {
        let selection = app.config.ai_hub.resolve_default_selection(&agent);
        app.config.ai_hub.default_provider = selection.provider_id;
        app.config.ai_hub.default_model = selection.model_id;
    }
    save_config(&app.config).map_err(|e| e.to_string())?;
    app.sync_ai_selection_from_defaults();
    bump_revision(&state);
    Ok(hub_response(&app))
}

#[tauri::command]
pub fn upsert_ai_model(
    provider_id: String,
    model: ModelUpsertDto,
    state: State<AppState>,
) -> Result<GetConfigHubResponse, String> {
    use er_engine::config::{
        save_config, split_shell_args, validate_provider_config, AiModelConfig,
    };

    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let pid = provider_id.trim().to_string();
    let provider = app
        .config
        .ai_hub
        .providers
        .get_mut(&pid)
        .ok_or_else(|| format!("unknown provider: {pid}"))?;

    let new_id = model.id.trim().to_string();
    if new_id.is_empty() {
        return Err("model id must not be empty".into());
    }
    let original_id = model
        .original_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != new_id.as_str())
        .map(|s| s.to_string());

    if let Some(old) = &original_id {
        provider.models.retain(|m| m.id != *old);
    } else {
        provider.models.retain(|m| m.id != new_id);
    }

    provider.models.push(AiModelConfig {
        id: new_id.clone(),
        label: model.label.filter(|s| !s.trim().is_empty()),
        description: model.description.filter(|s| !s.trim().is_empty()),
        args: split_shell_args(&model.args),
        cost_per_1k_in: model.cost_per_1k_in,
        cost_per_1k_out: model.cost_per_1k_out,
        avg_latency_ms: model.avg_latency_ms,
        effort_levels: model.effort_levels,
        discovered: false,
    });

    let warnings_provider = provider.clone();
    let warnings = validate_provider_config(&pid, &warnings_provider)?;

    if let Some(old) = original_id {
        if app.config.ai_hub.default_model.as_deref() == Some(old.as_str())
            && app.config.ai_hub.default_provider.as_deref() == Some(pid.as_str())
        {
            app.config.ai_hub.default_model = Some(new_id);
        }
    }

    save_config(&app.config).map_err(|e| e.to_string())?;
    bump_revision(&state);
    Ok(hub_response_with_warnings(&app, warnings))
}

#[tauri::command]
pub fn delete_ai_model(
    provider_id: String,
    model_id: String,
    state: State<AppState>,
) -> Result<GetConfigHubResponse, String> {
    use er_engine::config::{remove_ai_model, save_config};

    let mut app = state.app.lock().map_err(|e| e.to_string())?;
    let pid = provider_id.trim().to_string();
    let mid = model_id.trim().to_string();
    remove_ai_model(&mut app.config.ai_hub, &pid, &mid)?;

    if app.config.ai_hub.default_provider.as_deref() == Some(pid.as_str())
        && app.config.ai_hub.default_model.as_deref() == Some(mid.as_str())
    {
        let agent = app.config.agent.clone();
        let selection = app.config.ai_hub.resolve_default_selection(&agent);
        app.config.ai_hub.default_model = selection.model_id;
    }

    save_config(&app.config).map_err(|e| e.to_string())?;
    app.sync_ai_selection_from_defaults();
    bump_revision(&state);
    Ok(hub_response(&app))
}
