//! The provider and model editor's writes, applied to a config in memory.
//!
//! The flag-off desktop applies these to the App's config and the settings
//! actor to its own, then each saves; one function per edit keeps the two
//! paths writing the same thing.

use super::{
    remove_ai_model, remove_ai_provider, split_shell_args, validate_provider_config, AiModelConfig,
    AiProviderConfig, ErConfig,
};

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUpsert {
    pub id: String,
    pub original_id: Option<String>,
    pub label: Option<String>,
    pub command: String,
    pub args: String,
    pub family: Option<String>,
    pub models_command: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelUpsert {
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

/// What an upsert leaves for the caller.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HubEdit {
    /// Soft validation messages (e.g. missing `{prompt}`).
    pub warnings: Vec<String>,
    /// Whether the edit touched the default selection, so a front end holding
    /// a session selection resyncs it.
    pub defaults_changed: bool,
}

/// Adds or replaces a provider, keeping its models and tombstones across a
/// rename. Refuses an invalid provider without changing `config`.
pub fn upsert_provider(config: &mut ErConfig, provider: ProviderUpsert) -> Result<HubEdit, String> {
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
    if let Some(prev) = config.ai_hub.providers.get(source_key) {
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

    let renamed = original_id.is_some();
    if let Some(old) = original_id {
        config.ai_hub.providers.remove(&old);
        if config.ai_hub.default_provider.as_deref() == Some(old.as_str()) {
            config.ai_hub.default_provider = Some(id.clone());
        }
    }
    config.ai_hub.providers.insert(id.clone(), cfg);
    let defaults_changed =
        renamed || config.ai_hub.default_provider.as_deref() == Some(id.as_str());
    Ok(HubEdit {
        warnings,
        defaults_changed,
    })
}

/// Removes a provider, tombstoning a catalog id, and moves the default off it.
pub fn delete_provider(config: &mut ErConfig, provider_id: &str) -> Result<(), String> {
    let id = provider_id.trim();
    if id.is_empty() {
        return Err("provider_id must not be empty".into());
    }
    remove_ai_provider(&mut config.ai_hub, id);
    if config.ai_hub.default_provider.as_deref() == Some(id) {
        let selection = config.ai_hub.resolve_default_selection(&config.agent);
        config.ai_hub.default_provider = selection.provider_id;
        config.ai_hub.default_model = selection.model_id;
    }
    Ok(())
}

/// Adds or replaces one model of a provider, following a rename of the
/// default model.
pub fn upsert_model(
    config: &mut ErConfig,
    provider_id: &str,
    model: ModelUpsert,
) -> Result<HubEdit, String> {
    let pid = provider_id.trim().to_string();
    let provider = config
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
        edited: true,
    });

    let warnings = validate_provider_config(&pid, provider)?;

    if let Some(old) = original_id {
        if config.ai_hub.default_model.as_deref() == Some(old.as_str())
            && config.ai_hub.default_provider.as_deref() == Some(pid.as_str())
        {
            config.ai_hub.default_model = Some(new_id);
        }
    }
    Ok(HubEdit {
        warnings,
        defaults_changed: false,
    })
}

/// Removes one model, tombstoning a catalog id, and moves the default off it.
pub fn delete_model(
    config: &mut ErConfig,
    provider_id: &str,
    model_id: &str,
) -> Result<(), String> {
    let pid = provider_id.trim();
    let mid = model_id.trim();
    remove_ai_model(&mut config.ai_hub, pid, mid)?;
    if config.ai_hub.default_provider.as_deref() == Some(pid)
        && config.ai_hub.default_model.as_deref() == Some(mid)
    {
        let selection = config.ai_hub.resolve_default_selection(&config.agent);
        config.ai_hub.default_model = selection.model_id;
    }
    Ok(())
}
