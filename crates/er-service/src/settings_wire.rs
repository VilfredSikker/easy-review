//! The settings actor's view and commands in their wire form.
//!
//! The wire types mirror the engine's field for field, so the flag-off
//! commands and the `settings` resource answer the same JSON.

use er_engine::config::{
    AiModelInfo, AiProviderInfo, ConfigFieldValue, ConfigHubFieldDto, DisplayConfig, FeatureFlags,
    ModelUpsert, ProviderUpsert, ReviewModelGroups, ReviewModelRef, RiskRuleDto, RiskRuleSource,
    RiskTier,
};
use er_engine::uninstall::{UninstallOptions, UninstallPreview};
use er_protocol as wire;

use crate::settings::{SettingsCommand, SettingsView};

/// The `settings` resource: the page's rows and AI defaults from `view`.
pub fn settings_resource(view: &SettingsView) -> wire::Settings {
    let hub = &view.hub;
    wire::Settings {
        settings: wire::SettingsRows {
            general: hub.fields.general.iter().map(field).collect(),
            app: hub.fields.app.iter().map(field).collect(),
            terminal: hub.fields.terminal.iter().map(field).collect(),
        },
        providers: hub.providers.iter().map(provider).collect(),
        review_models: review_models(&hub.review_models),
        active_effort: hub.active_effort.clone(),
        family_options: hub.family_options.clone(),
        uninstall_preview: view.uninstall_preview.as_ref().map(uninstall_preview),
        theme: view.config.display.theme.clone(),
        features: features(&view.config.features),
        display: display(&view.config.display),
        arena_enabled: true,
    }
}

fn features(f: &FeatureFlags) -> wire::FeatureFlags {
    wire::FeatureFlags {
        view_branch: f.view_branch,
        view_unstaged: f.view_unstaged,
        view_staged: f.view_staged,
        view_history: f.view_history,
        view_conflicts: f.view_conflicts,
        view_hidden: f.view_hidden,
        view_tour: f.view_tour,
    }
}

fn display(d: &DisplayConfig) -> wire::DisplayConfig {
    wire::DisplayConfig {
        line_numbers: d.line_numbers,
        wrap_lines: d.wrap_lines,
        split_diff: d.split_diff,
        tab_width: d.tab_width,
    }
}

/// The `risk(key)` resource: the rules key's rules with the paths its last
/// load named, counted and resolved. None for a key never loaded.
pub fn risk_resource(view: &SettingsView, key: &str) -> Option<wire::Risk> {
    let rules = view.risk.get(key)?;
    let files = view.risk_files.get(key)?;
    Some(wire::Risk {
        repo: rules.repo.clone(),
        rules: files.rules.iter().map(risk_rule).collect(),
        default: risk_tier(rules.default),
        files: files
            .files
            .iter()
            .map(|file| wire::RiskFile {
                path: file.path.clone(),
                tier: risk_tier(file.resolved.tier),
                rule: file
                    .resolved
                    .rule
                    .as_ref()
                    .map(|rule| wire::MatchedRiskRule {
                        key: rule.key.clone(),
                        label: rule.label.clone(),
                        source: risk_source(rule.source),
                    }),
            })
            .collect(),
    })
}

/// The settings actor's command for a wire command. A command another owner
/// applies comes back unchanged.
pub fn settings_command(command: wire::Command) -> Result<SettingsCommand, Box<wire::Command>> {
    Ok(match command {
        wire::Command::ReloadSettings => SettingsCommand::ReloadIfFailed,
        wire::Command::ApplyConfigPatch { key, value } => SettingsCommand::ApplyConfigPatch {
            key,
            value: match value {
                wire::ConfigFieldValue::Bool(value) => ConfigFieldValue::Bool(value),
                wire::ConfigFieldValue::String(value) => ConfigFieldValue::String(value),
                wire::ConfigFieldValue::Number(value) => ConfigFieldValue::Number(value),
            },
        },
        wire::Command::SetReviewFlowModels { groups } => SettingsCommand::SetReviewFlowModels {
            groups: ReviewModelGroups {
                fast: model_refs(groups.fast),
                standard: model_refs(groups.standard),
                advanced: model_refs(groups.advanced),
            },
        },
        wire::Command::RefreshAiModels { provider_id, force } => {
            SettingsCommand::RefreshAiModels { provider_id, force }
        }
        wire::Command::UpsertAiProvider { provider: p } => SettingsCommand::UpsertAiProvider {
            provider: ProviderUpsert {
                id: p.id,
                original_id: p.original_id,
                label: p.label,
                command: p.command,
                args: p.args,
                family: p.family,
                models_command: p.models_command,
            },
        },
        wire::Command::DeleteAiProvider { provider_id } => {
            SettingsCommand::DeleteAiProvider { provider_id }
        }
        wire::Command::UpsertAiModel {
            provider_id,
            model: m,
        } => SettingsCommand::UpsertAiModel {
            provider_id,
            model: ModelUpsert {
                id: m.id,
                original_id: m.original_id,
                label: m.label,
                description: m.description,
                args: m.args,
                effort_levels: m.effort_levels,
                cost_per_1k_in: m.cost_per_1k_in,
                cost_per_1k_out: m.cost_per_1k_out,
                avg_latency_ms: m.avg_latency_ms,
            },
        },
        wire::Command::DeleteAiModel {
            provider_id,
            model_id,
        } => SettingsCommand::DeleteAiModel {
            provider_id,
            model_id,
        },
        wire::Command::PreviewUninstall { options } => SettingsCommand::PreviewUninstall {
            options: uninstall_options(options),
        },
        wire::Command::RunUninstall { options } => SettingsCommand::RunUninstall {
            options: uninstall_options(options),
        },
        wire::Command::LoadRisk {
            repo_root,
            remote_repo,
            paths,
        } => SettingsCommand::LoadRiskRules {
            repo_root,
            remote_repo,
            paths,
        },
        wire::Command::SetAiSelection {
            provider_id,
            model_id,
            effort,
        } => SettingsCommand::SetDefaultAiSelection {
            provider_id,
            model_id,
            effort,
        },
        wire::Command::SetAiEffort { effort } => SettingsCommand::SetDefaultAiEffort { effort },
        other @ (wire::Command::ReorderProjects { .. }
        | wire::Command::DeleteProject { .. }
        | wire::Command::AddTrackedBranch { .. }
        | wire::Command::RemoveTrackedBranch { .. }
        | wire::Command::PatchProjectReviewSettings { .. }
        | wire::Command::RefreshPrList
        | wire::Command::RefreshProjectPrList { .. }
        | wire::Command::DismissPr { .. }
        | wire::Command::UndismissPr { .. }
        | wire::Command::TrackPr { .. }
        | wire::Command::UntrackPr { .. }
        | wire::Command::SavePr { .. }
        | wire::Command::UnsavePr { .. }
        | wire::Command::MarkInboxItemRead { .. }
        | wire::Command::MarkAllInboxRead
        | wire::Command::MarkInboxItemsRead { .. }
        | wire::Command::ClearReadInboxItems
        | wire::Command::ClearInboxItems { .. }
        | wire::Command::RefreshNotifications
        | wire::Command::TestNativeNotification
        | wire::Command::OpenInboxItem { .. }
        | wire::Command::PrefetchInboxItem { .. }
        | wire::Command::CheckAppUpdate
        | wire::Command::TogglePanel { .. }
        | wire::Command::NewTab
        | wire::Command::CloseTab { .. }
        | wire::Command::SelectTab { .. }
        | wire::Command::ReorderTabs { .. }
        | wire::Command::OpenPrReview { .. }
        | wire::Command::OpenRemotePr { .. }
        | wire::Command::OpenPrUrl { .. }
        | wire::Command::OpenLocalBranch { .. }
        | wire::Command::OpenProjectBranch { .. }
        | wire::Command::OpenWorktree
        | wire::Command::SetActiveProject { .. }
        | wire::Command::PrefetchPrOpen { .. }
        | wire::Command::PrefetchRemotePrOpen { .. }
        | wire::Command::UpdateTabBrowser { .. }
        | wire::Command::CycleTabBrowserLayout
        | wire::Command::SetDiffMode { .. }
        | wire::Command::ChangeBase { .. }
        | wire::Command::SetFilter { .. }
        | wire::Command::RefreshDiff { .. }
        | wire::Command::SelectCommit { .. }
        | wire::Command::ExitHistory { .. }
        | wire::Command::SyncPr { .. }
        | wire::Command::SyncBranch { .. }
        | wire::Command::ToggleReviewed { .. }
        | wire::Command::MarkReviewed { .. }
        | wire::Command::BulkReviewPillar { .. }
        | wire::Command::SetFileCompacted { .. }
        | wire::Command::AddQuestion { .. }
        | wire::Command::AddNote { .. }
        | wire::Command::AddComment { .. }
        | wire::Command::AddDocumentThread { .. }
        | wire::Command::ReplyToThread { .. }
        | wire::Command::UpdateThreadMessage { .. }
        | wire::Command::DeleteThread { .. }
        | wire::Command::ResolveThread { .. }
        | wire::Command::PromoteToNote { .. }
        | wire::Command::PromoteToComment { .. }
        | wire::Command::PromoteFindingToComment { .. }
        | wire::Command::ReplyToFinding { .. }
        | wire::Command::DismissFinding { .. }
        | wire::Command::SetFindingResolved { .. }
        | wire::Command::RemoveFindingThread { .. }
        | wire::Command::UpdateFindingResponse { .. }
        | wire::Command::DeleteFindingResponse { .. }
        | wire::Command::ToggleChecklistItem { .. }
        | wire::Command::DeleteReviewArtifact { .. }
        | wire::Command::DeleteDiagram { .. }
        | wire::Command::AddUiAnnotation { .. }
        | wire::Command::SaveAnnotationScreenshot { .. }
        | wire::Command::DeleteUiAnnotation { .. }
        | wire::Command::ClearUiAnnotationsForPage { .. }
        | wire::Command::ClearUiAnnotations { .. }
        | wire::Command::UpdateUiAnnotationAnchors { .. }
        | wire::Command::RefreshStack { .. }
        | wire::Command::RefreshGithubStatus { .. }
        | wire::Command::ReplyToGithubThread { .. }
        | wire::Command::PullGithubComments { .. }
        | wire::Command::PostGithubPrComment { .. }
        | wire::Command::PostGithubLineComment { .. }
        | wire::Command::SubmitGithubPrDecision { .. }
        | wire::Command::AddPendingReviewComment { .. }
        | wire::Command::RemovePendingReviewComment { .. }
        | wire::Command::SubmitPendingReview { .. }
        | wire::Command::RunGithubPrAction { .. }) => return Err(Box::new(other)),
    })
}

fn risk_rule(rule: &RiskRuleDto) -> wire::RiskRule {
    wire::RiskRule {
        pattern: rule.pattern.clone(),
        tier: rule.tier.clone(),
        readable: rule.readable,
        label: rule.label.clone(),
        source: risk_source(rule.source),
        file_count: rule.file_count,
    }
}

const fn risk_tier(tier: RiskTier) -> wire::RiskTier {
    match tier {
        RiskTier::High => wire::RiskTier::High,
        RiskTier::Med => wire::RiskTier::Med,
        RiskTier::Low => wire::RiskTier::Low,
        RiskTier::Ai => wire::RiskTier::Ai,
    }
}

const fn risk_source(source: RiskRuleSource) -> wire::RiskRuleSource {
    match source {
        RiskRuleSource::Project => wire::RiskRuleSource::Project,
        RiskRuleSource::User => wire::RiskRuleSource::User,
    }
}

fn field(field: &ConfigHubFieldDto) -> wire::ConfigHubField {
    match field.clone() {
        ConfigHubFieldDto::Section { title } => wire::ConfigHubField::Section { title },
        ConfigHubFieldDto::Bool {
            key,
            label,
            description,
            value,
        } => wire::ConfigHubField::Bool {
            key,
            label,
            description,
            value,
        },
        ConfigHubFieldDto::Cycle {
            key,
            label,
            description,
            options,
            value,
        } => wire::ConfigHubField::Cycle {
            key,
            label,
            description,
            options,
            value,
        },
        ConfigHubFieldDto::Text {
            key,
            label,
            description,
            placeholder,
            value,
            strict,
        } => wire::ConfigHubField::Text {
            key,
            label,
            description,
            placeholder,
            value,
            strict,
        },
        ConfigHubFieldDto::ListEntry { key, label, index } => {
            wire::ConfigHubField::ListEntry { key, label, index }
        }
        ConfigHubFieldDto::ListAdd { key, label } => wire::ConfigHubField::ListAdd { key, label },
    }
}

fn provider(p: &AiProviderInfo) -> wire::AiProvider {
    wire::AiProvider {
        id: p.id.clone(),
        label: p.label.clone(),
        models: p.models.iter().map(model).collect(),
        is_selected: p.is_selected,
        family: p.family.clone(),
        has_models_command: p.has_models_command,
        command: p.command.clone(),
        args: p.args.clone(),
        models_command: p.models_command.clone(),
    }
}

fn model(m: &AiModelInfo) -> wire::AiModel {
    wire::AiModel {
        id: m.id.clone(),
        label: m.label.clone(),
        is_selected: m.is_selected,
        description: m.description.clone(),
        cost_per_1k_in: m.cost_per_1k_in,
        cost_per_1k_out: m.cost_per_1k_out,
        avg_latency_ms: m.avg_latency_ms,
        effort_levels: m.effort_levels.clone(),
        discovered: m.discovered,
        args: m.args.clone(),
    }
}

fn review_models(groups: &ReviewModelGroups) -> wire::ReviewModelGroups {
    let refs = |refs: &[ReviewModelRef]| {
        refs.iter()
            .map(|r| wire::ReviewModelRef {
                provider_id: r.provider_id.clone(),
                model_id: r.model_id.clone(),
            })
            .collect()
    };
    wire::ReviewModelGroups {
        fast: refs(&groups.fast),
        standard: refs(&groups.standard),
        advanced: refs(&groups.advanced),
    }
}

fn model_refs(refs: Vec<wire::ReviewModelRef>) -> Vec<ReviewModelRef> {
    refs.into_iter()
        .map(|r| ReviewModelRef {
            provider_id: r.provider_id,
            model_id: r.model_id,
        })
        .collect()
}

fn uninstall_preview(preview: &UninstallPreview) -> wire::UninstallPreview {
    wire::UninstallPreview {
        targets: preview
            .targets
            .iter()
            .map(|t| wire::UninstallTarget {
                kind: t.kind.clone(),
                path: t.path.clone(),
                exists: t.exists,
                description: t.description.clone(),
            })
            .collect(),
        existing_count: preview.existing_count,
    }
}

const fn uninstall_options(options: wire::UninstallOptions) -> UninstallOptions {
    UninstallOptions {
        remove_config: options.remove_config,
        remove_data: options.remove_data,
        remove_cache: options.remove_cache,
        remove_binaries: options.remove_binaries,
        remove_desktop_app: options.remove_desktop_app,
    }
}
