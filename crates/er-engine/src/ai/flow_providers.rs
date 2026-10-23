//! Provider admission for review flows.
//!
//! Catalog entries describe possible providers. A flow may use only providers
//! that are configured, installed on PATH, and allowed by the user's list.

use super::review_flow::FlowDepth;
use crate::config::{AiHubConfig, ReviewModelTier};

/// Installed configured providers allowed to receive review-flow input.
///
/// The default provider is first when eligible. Remaining IDs follow the
/// stable `BTreeMap` order used by `AiHubConfig::providers`.
pub fn installed_providers(hub: &AiHubConfig) -> Vec<String> {
    installed_providers_with(hub, crate::env_path::command_on_path)
}

fn installed_providers_with(
    hub: &AiHubConfig,
    mut is_installed: impl FnMut(&str) -> bool,
) -> Vec<String> {
    let allowed =
        |id: &str| hub.flow_providers.is_empty() || hub.flow_providers.iter().any(|x| x == id);
    let mut ids: Vec<_> = hub
        .providers
        .iter()
        .filter(|(id, provider)| allowed(id) && is_installed(&provider.command))
        .map(|(id, _)| id.clone())
        .collect();
    if let Some(default) = hub.default_provider.as_ref() {
        if let Some(index) = ids.iter().position(|id| id == default) {
            ids.remove(index);
            ids.insert(0, default.clone());
        }
    }
    ids
}

/// An admitted provider and the model the existing config resolver selects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowProvider {
    pub provider_id: String,
    pub model_id: String,
}

/// Exact provider/model choices assigned to one finder lens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowLensAssignment {
    pub lens: String,
    pub providers: Vec<FlowProvider>,
}

/// Resolved exact provider/model pools captured for one flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedReviewModels {
    pub fast: Vec<FlowProvider>,
    pub standard: Vec<FlowProvider>,
    pub advanced: Vec<FlowProvider>,
}

/// Resolve one configured model group against installed, allowed providers.
/// Missing providers are filtered; a missing exact model never falls back to
/// another model from that provider.
pub fn eligible_review_models(
    hub: &AiHubConfig,
    tier: ReviewModelTier,
) -> anyhow::Result<Vec<FlowProvider>> {
    eligible_review_models_with(hub, tier, crate::env_path::command_on_path)
}

fn eligible_review_models_with(
    hub: &AiHubConfig,
    tier: ReviewModelTier,
    mut is_installed: impl FnMut(&str) -> bool,
) -> anyhow::Result<Vec<FlowProvider>> {
    // Provider removal can leave selections in saved or default groups. Keep
    // validating exact models for providers that remain, without requiring an
    // unavailable provider to run or substituting an unselected model.
    let mut available_groups = hub.review_models.clone();
    for models in [
        &mut available_groups.fast,
        &mut available_groups.standard,
        &mut available_groups.advanced,
    ] {
        models.retain(|selection| hub.providers.contains_key(&selection.provider_id));
    }
    hub.validate_review_models(&available_groups)?;
    let installed = installed_providers_with(hub, &mut is_installed)
        .into_iter()
        .collect::<std::collections::HashSet<_>>();
    let mut default = Vec::new();
    let mut rest = Vec::new();
    for selection in available_groups.models(tier) {
        if !installed.contains(&selection.provider_id) {
            continue;
        }
        let resolved = FlowProvider {
            provider_id: selection.provider_id.clone(),
            model_id: selection.model_id.clone(),
        };
        if hub.default_provider.as_deref() == Some(selection.provider_id.as_str()) {
            default.push(resolved);
        } else {
            rest.push(resolved);
        }
    }
    default.extend(rest);
    if default.is_empty() {
        anyhow::bail!(
            "no installed allowed models are available for the {tier:?} review-flow group"
        );
    }
    Ok(default)
}

/// Resolve all configured pools before starting triage. A flow is admitted
/// only when every depth has at least one installed exact model selection.
pub fn resolve_review_models(hub: &AiHubConfig) -> anyhow::Result<ResolvedReviewModels> {
    resolve_review_models_with(hub, crate::env_path::command_on_path)
}

fn resolve_review_models_with(
    hub: &AiHubConfig,
    mut is_installed: impl FnMut(&str) -> bool,
) -> anyhow::Result<ResolvedReviewModels> {
    Ok(ResolvedReviewModels {
        fast: eligible_review_models_with(hub, ReviewModelTier::Fast, &mut is_installed)?,
        standard: eligible_review_models_with(hub, ReviewModelTier::Standard, &mut is_installed)?,
        advanced: eligible_review_models_with(hub, ReviewModelTier::Advanced, &mut is_installed)?,
    })
}

/// Assign exact model pairs in configured order. Deep uses two adjacent
/// distinct pairs when the pool has more than one unique selection.
pub fn assign_lenses_to_models(
    depth: FlowDepth,
    lenses: &[String],
    models: &[FlowProvider],
) -> anyhow::Result<Vec<FlowLensAssignment>> {
    if models.is_empty() {
        anyhow::bail!("no eligible models are available for lens assignment");
    }
    if models
        .iter()
        .any(|model| model.provider_id.trim().is_empty() || model.model_id.trim().is_empty())
    {
        anyhow::bail!("review-flow model selections must not be empty");
    }
    if lenses.iter().any(|lens| lens.trim().is_empty()) {
        anyhow::bail!("review-flow lens IDs must not be empty");
    }
    let count = models.len();
    Ok(lenses
        .iter()
        .enumerate()
        .map(|(index, lens)| {
            let start = index % count;
            let first = models[start].clone();
            let mut providers = vec![first.clone()];
            if depth == FlowDepth::Deep && count > 1 {
                let next = (1..count)
                    .map(|offset| (start + offset) % count)
                    .find(|candidate| models[*candidate] != first);
                if let Some(candidate) = next {
                    providers.push(models[candidate].clone());
                }
            }
            FlowLensAssignment {
                lens: lens.clone(),
                providers,
            }
        })
        .collect())
}

/// Assign each lens to exact providers from the captured eligible set.
///
/// Quick and Standard rotate one provider across lenses. Deep uses adjacent
/// providers for each lens when at least two are eligible.
pub fn assign_lenses(
    depth: FlowDepth,
    lenses: &[String],
    provider_ids: &[String],
    hub: &AiHubConfig,
) -> anyhow::Result<Vec<FlowLensAssignment>> {
    if provider_ids.is_empty() {
        anyhow::bail!("no eligible providers are available for lens assignment");
    }
    if provider_ids.iter().any(|id| id.trim().is_empty()) {
        anyhow::bail!("eligible provider IDs must not be empty");
    }
    if lenses.iter().any(|lens| lens.trim().is_empty()) {
        anyhow::bail!("review-flow lens IDs must not be empty");
    }

    let provider_models = provider_ids
        .iter()
        .map(|provider_id| {
            let model_id = hub.resolve_model_id(provider_id, None).ok_or_else(|| {
                anyhow::anyhow!("eligible provider `{provider_id}` has no resolvable model")
            })?;
            Ok(FlowProvider {
                provider_id: provider_id.clone(),
                model_id,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    let count = provider_models.len();
    Ok(lenses
        .iter()
        .enumerate()
        .map(|(index, lens)| {
            let start = index % count;
            let mut assigned = vec![provider_models[start].clone()];
            if depth == FlowDepth::Deep && count > 1 {
                assigned.push(provider_models[(start + 1) % count].clone());
            }
            FlowLensAssignment {
                lens: lens.clone(),
                providers: assigned,
            }
        })
        .collect())
}

/// Resolve and validate models for every installed, allowed provider.
///
/// An empty result is an admission error. In particular, this never falls
/// back to `resolve_provider_id`, which could choose a disallowed provider.
pub fn eligible_flow_providers(hub: &AiHubConfig) -> anyhow::Result<Vec<FlowProvider>> {
    eligible_flow_providers_with(hub, crate::env_path::command_on_path)
}

fn eligible_flow_providers_with(
    hub: &AiHubConfig,
    is_installed: impl FnMut(&str) -> bool,
) -> anyhow::Result<Vec<FlowProvider>> {
    let ids = installed_providers_with(hub, is_installed);
    if ids.is_empty() {
        anyhow::bail!("no installed configured provider is allowed for review flows");
    }
    ids.into_iter()
        .map(|provider_id| {
            let model_id = hub.resolve_model_id(&provider_id, None).ok_or_else(|| {
                anyhow::anyhow!("review-flow provider `{provider_id}` has no resolvable model")
            })?;
            Ok(FlowProvider {
                provider_id,
                model_id,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AiModelConfig, AiProviderConfig, ReviewModelRef};

    fn hub() -> AiHubConfig {
        let provider = |command: &str, model: bool| AiProviderConfig {
            command: command.into(),
            models: if model {
                vec![AiModelConfig {
                    id: "m1".into(),
                    ..Default::default()
                }]
            } else {
                vec![]
            },
            ..Default::default()
        };
        AiHubConfig {
            default_provider: Some("beta".into()),
            providers: [
                ("alpha".into(), provider("shared", true)),
                ("beta".into(), provider("shared", true)),
                ("gamma".into(), provider("missing", true)),
            ]
            .into(),
            ..Default::default()
        }
    }

    fn catalog_hub() -> AiHubConfig {
        let mut hub = AiHubConfig {
            default_provider: Some("claude".into()),
            providers: [
                (
                    "claude".into(),
                    AiProviderConfig {
                        command: "claude".into(),
                        ..Default::default()
                    },
                ),
                (
                    "codex".into(),
                    AiProviderConfig {
                        command: "codex".into(),
                        ..Default::default()
                    },
                ),
            ]
            .into(),
            ..Default::default()
        };
        crate::config::supplement_ai_hub(&mut hub);
        hub
    }

    #[test]
    fn flow_providers_respect_allowlist_installation_and_default_order() {
        let mut config = hub();
        config.flow_providers = vec!["alpha".into(), "beta".into(), "unknown".into()];
        assert_eq!(
            installed_providers_with(&config, |cmd| cmd == "shared"),
            vec!["beta", "alpha"]
        );
        config.flow_providers = vec!["alpha".into()];
        assert_eq!(
            installed_providers_with(&config, |cmd| cmd == "shared"),
            vec!["alpha"]
        );
        config.flow_providers = vec!["unknown".into()];
        assert!(installed_providers_with(&config, |_| true).is_empty());
        config.flow_providers.clear();
        assert_eq!(
            installed_providers_with(&config, |_| false),
            Vec::<String>::new()
        );
    }

    #[test]
    fn flow_providers_report_empty_admission_and_missing_models() {
        let mut config = hub();
        assert!(eligible_flow_providers_with(&config, |_| false).is_err());
        config.providers.get_mut("beta").unwrap().models.clear();
        let error = eligible_flow_providers_with(&config, |cmd| cmd == "shared").unwrap_err();
        assert!(error.to_string().contains("beta"));
    }

    fn provider_ids(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|id| (*id).into()).collect()
    }

    fn assigned_ids(assignments: &[FlowLensAssignment]) -> Vec<Vec<&str>> {
        assignments
            .iter()
            .map(|assignment| {
                assignment
                    .providers
                    .iter()
                    .map(|provider| provider.provider_id.as_str())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn flow_providers_assign_quick_and_standard_lenses_round_robin() {
        let config = hub();
        let lenses = provider_ids(&["general", "security", "api", "performance"]);
        let two = provider_ids(&["alpha", "beta"]);
        let three = provider_ids(&["alpha", "beta", "gamma"]);
        assert_eq!(
            assigned_ids(&assign_lenses(FlowDepth::Quick, &lenses, &two, &config).unwrap()),
            vec![vec!["alpha"], vec!["beta"], vec!["alpha"], vec!["beta"]]
        );
        assert_eq!(
            assigned_ids(&assign_lenses(FlowDepth::Standard, &lenses, &three, &config).unwrap()),
            vec![vec!["alpha"], vec!["beta"], vec!["gamma"], vec!["alpha"]]
        );
    }

    #[test]
    fn flow_providers_assign_deep_adjacent_pairs_and_rotate() {
        let config = hub();
        let lenses = provider_ids(&["general", "security", "api", "performance"]);
        assert_eq!(
            assigned_ids(
                &assign_lenses(
                    FlowDepth::Deep,
                    &lenses,
                    &provider_ids(&["alpha", "beta", "gamma"]),
                    &config,
                )
                .unwrap()
            ),
            vec![
                vec!["alpha", "beta"],
                vec!["beta", "gamma"],
                vec!["gamma", "alpha"],
                vec!["alpha", "beta"],
            ]
        );
        assert_eq!(
            assigned_ids(
                &assign_lenses(FlowDepth::Deep, &lenses, &provider_ids(&["alpha"]), &config,)
                    .unwrap()
            ),
            vec![vec!["alpha"], vec!["alpha"], vec!["alpha"], vec!["alpha"]]
        );
    }

    #[test]
    fn flow_providers_reject_empty_or_unresolvable_assignments() {
        let config = hub();
        let lenses = provider_ids(&["general"]);
        assert!(assign_lenses(FlowDepth::Quick, &lenses, &[], &config).is_err());
        assert!(assign_lenses(FlowDepth::Quick, &lenses, &provider_ids(&[""]), &config,).is_err());
        assert!(assign_lenses(
            FlowDepth::Quick,
            &lenses,
            &provider_ids(&["unknown"]),
            &config,
        )
        .is_err());
        assert!(assign_lenses(
            FlowDepth::Quick,
            &lenses,
            &provider_ids(&["gamma"]),
            &config,
        )
        .is_ok()); // Assignment trusts the captured eligible set; no PATH lookup.
        let mut missing_model = config.clone();
        missing_model
            .providers
            .get_mut("gamma")
            .unwrap()
            .models
            .clear();
        assert!(assign_lenses(
            FlowDepth::Quick,
            &lenses,
            &provider_ids(&["gamma"]),
            &missing_model,
        )
        .is_err());
        assert!(assign_lenses(
            FlowDepth::Quick,
            &provider_ids(&[""]),
            &provider_ids(&["alpha"]),
            &config,
        )
        .is_err());
    }

    #[test]
    fn review_flow_model_eligibility_uses_exact_group_and_allowlist_pairs() {
        let mut hub = catalog_hub();
        hub.review_models.fast = vec![
            ReviewModelRef {
                provider_id: "codex".into(),
                model_id: "gpt-6-luna".into(),
            },
            ReviewModelRef {
                provider_id: "claude".into(),
                model_id: "haiku-5.5".into(),
            },
        ];
        hub.flow_providers = vec!["codex".into(), "claude".into()];

        let selected = eligible_review_models_with(&hub, ReviewModelTier::Fast, |command| {
            command == "claude" || command == "codex"
        })
        .unwrap();
        assert_eq!(
            selected,
            vec![
                FlowProvider {
                    provider_id: "claude".into(),
                    model_id: "haiku-5.5".into(),
                },
                FlowProvider {
                    provider_id: "codex".into(),
                    model_id: "gpt-6-luna".into(),
                },
            ]
        );

        hub.flow_providers = vec!["codex".into()];
        let selected = eligible_review_models_with(&hub, ReviewModelTier::Fast, |command| {
            command == "claude" || command == "codex"
        })
        .unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].provider_id, "codex");
        assert_eq!(selected[0].model_id, "gpt-6-luna");

        assert!(eligible_review_models_with(&hub, ReviewModelTier::Fast, |_| false).is_err());
    }

    #[test]
    fn review_flow_model_eligibility_preserves_exact_custom_selection() {
        let mut hub = catalog_hub();
        hub.providers
            .get_mut("codex")
            .unwrap()
            .models
            .push(AiModelConfig {
                id: "my-codex-model".into(),
                ..Default::default()
            });
        hub.review_models.fast = vec![ReviewModelRef {
            provider_id: "codex".into(),
            model_id: "my-codex-model".into(),
        }];
        hub.flow_providers = vec!["codex".into()];

        let selected =
            eligible_review_models_with(&hub, ReviewModelTier::Fast, |cmd| cmd == "codex").unwrap();
        assert_eq!(selected[0].model_id, "my-codex-model");

        hub.review_models.fast[0].model_id = "unconfigured-model".into();
        assert!(
            eligible_review_models_with(&hub, ReviewModelTier::Fast, |cmd| cmd == "codex").is_err()
        );
    }

    #[test]
    fn review_flow_model_eligibility_survives_provider_deletion() {
        let mut hub = catalog_hub();
        hub.flow_providers = vec!["codex".into()];
        crate::config::remove_ai_provider(&mut hub, "claude");
        crate::config::supplement_ai_hub(&mut hub);

        let pools = resolve_review_models_with(&hub, |command| command == "codex").unwrap();
        for (models, expected) in [
            (&pools.fast, "gpt-6-luna"),
            (&pools.standard, "gpt-6-sol"),
            (&pools.advanced, "gpt-6-astra"),
        ] {
            assert_eq!(models.len(), 1);
            assert_eq!(models[0].provider_id, "codex");
            assert_eq!(models[0].model_id, expected);
        }
    }

    #[test]
    fn review_flow_model_eligibility_survives_legacy_provider_tombstone() {
        let mut hub = catalog_hub();
        crate::config::remove_ai_provider(&mut hub, "claude");
        hub.flow_providers = vec!["codex".into()];
        let mut saved = serde_json::to_value(&hub).unwrap();
        saved.as_object_mut().unwrap().remove("review_models");
        let mut loaded: AiHubConfig = serde_json::from_value(saved).unwrap();
        crate::config::supplement_ai_hub(&mut loaded);

        let pools = resolve_review_models_with(&loaded, |command| command == "codex").unwrap();
        assert_eq!(pools.fast[0].model_id, "gpt-6-luna");
        assert_eq!(pools.standard[0].model_id, "gpt-6-sol");
        assert_eq!(pools.advanced[0].model_id, "gpt-6-astra");
        assert!(pools
            .fast
            .iter()
            .chain(&pools.standard)
            .chain(&pools.advanced)
            .all(|model| model.provider_id == "codex"));
    }

    #[test]
    fn review_flow_resolves_all_pools_before_triage() {
        let mut hub = catalog_hub();
        hub.flow_providers = vec!["codex".into()];
        hub.review_models.advanced = vec![ReviewModelRef {
            provider_id: "claude".into(),
            model_id: "opus-5.5".into(),
        }];

        assert!(resolve_review_models_with(&hub, |command| command == "codex").is_err());
    }

    #[test]
    fn review_flow_assigns_exact_pairs_and_deep_uses_distinct_adjacent_choices() {
        let models = vec![
            FlowProvider {
                provider_id: "claude".into(),
                model_id: "sonnet-5.5".into(),
            },
            FlowProvider {
                provider_id: "codex".into(),
                model_id: "gpt-6-sol".into(),
            },
        ];
        let lenses = provider_ids(&["general", "security", "api"]);
        let assignments = assign_lenses_to_models(FlowDepth::Deep, &lenses, &models).unwrap();
        assert_eq!(
            assignments
                .iter()
                .map(|assignment| assignment
                    .providers
                    .iter()
                    .map(|provider| (provider.provider_id.as_str(), provider.model_id.as_str()))
                    .collect::<Vec<_>>())
                .collect::<Vec<_>>(),
            vec![
                vec![("claude", "sonnet-5.5"), ("codex", "gpt-6-sol")],
                vec![("codex", "gpt-6-sol"), ("claude", "sonnet-5.5")],
                vec![("claude", "sonnet-5.5"), ("codex", "gpt-6-sol")],
            ]
        );
    }
}
