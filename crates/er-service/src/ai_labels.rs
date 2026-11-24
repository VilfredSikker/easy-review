//! The AI selection's labels, built from the saved default selection. Both
//! front ends show them, so the rule lives here once.

use er_engine::config::ErConfig;

/// The default selection as a front end labels it: provider and model by
/// their display names, then the effort.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AiLabels {
    /// `provider / model · effort`, or the agent's name with no provider.
    pub label: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
}

/// The labels of `config`'s default AI selection.
pub fn ai_labels(config: &ErConfig) -> AiLabels {
    let hub = &config.ai_hub;
    let selection = hub.resolve_default_selection(&config.agent);
    let provider = selection
        .provider_id
        .as_deref()
        .and_then(|id| hub.providers.get(id).map(|provider| (id, provider)));
    let (provider_label, model_label) = match provider {
        Some((id, provider)) => (
            Some(provider.display_name(id)),
            selection.model_id.as_deref().and_then(|model_id| {
                provider
                    .models
                    .iter()
                    .find(|m| m.id == model_id)
                    .map(|m| m.display_name())
            }),
        ),
        None => (None, None),
    };
    let label = match (&provider_label, &model_label) {
        (None, _) => config.agent.display_name(),
        (Some(provider), Some(model)) => format!("{provider} / {model}"),
        (Some(provider), None) => provider.clone(),
    };
    let label = match (&provider_label, selection.effort.as_deref()) {
        (Some(_), Some(effort)) => format!("{label} · {effort}"),
        _ => label,
    };
    AiLabels {
        label,
        provider: provider_label,
        model: model_label,
        effort: selection.effort,
    }
}
