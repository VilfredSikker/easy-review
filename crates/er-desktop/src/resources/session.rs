//! `session` and `panels`: the chrome that belongs to no tab.
//!
//! `session` has two writers. The settings actor owns the AI pick's labels
//! and the config load error, the desktop the watcher's status, the update
//! check and the rest. Its version is the settings revision plus the
//! desktop's count of its own writes; both only rise, so the sum does too,
//! and a write on either side moves it.

use er_engine::config::ErConfig;
use er_protocol::{AppUpdate, Panels, Reviewer, Session, WatchStatus};
use er_service::SettingsView;

use crate::commands::AppUpdateInfo;
use crate::state::DesktopState;

/// `session`'s version, with the settings revision of `settings`: zero
/// while the settings actor has not started, which is the revision it
/// starts at.
pub fn session_version(desktop: &DesktopState, settings: Option<&SettingsView>) -> u64 {
    settings.map_or(0, |view| view.revision) + desktop.session.revision()
}

/// `session` built from the settings view and the desktop's own state.
pub fn session(desktop: &DesktopState, settings: &SettingsView) -> (u64, Session) {
    let version = session_version(desktop, Some(settings));
    let watch = desktop
        .watch_status
        .lock()
        .map(|status| status.clone())
        .unwrap_or_default();
    let update = desktop
        .session
        .update
        .lock()
        .ok()
        .and_then(|update| update.clone());
    let labels = AiLabels::of(&settings.config);
    let session = Session {
        active_ai_label: labels.label,
        active_ai_provider_label: labels.provider,
        active_ai_model_label: labels.model,
        active_ai_effort: labels.effort,
        config_load_error: settings.load_error.clone(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        watch_status: WatchStatus {
            active: watch.active,
            branch: watch.branch,
            root_path: watch.root_path,
        },
        update,
        reviewers: er_engine::ai::list_ai_reviewers()
            .into_iter()
            .map(|r| Reviewer {
                kind: r.kind,
                label: r.label,
                description: r.description,
            })
            .collect(),
    };
    (version, session)
}

/// The desktop's mirror of the page's panels, at its toggle count.
pub fn panels(desktop: &DesktopState) -> (u64, Panels) {
    let (revision, open) = desktop
        .view
        .lock()
        .map(|view| (view.panels_revision(), view.panels().clone()))
        .unwrap_or_default();
    (
        revision,
        Panels {
            left: open.left,
            tree: open.tree,
            right: open.right,
        },
    )
}

/// The panels' toggle count, read without building them.
pub fn panels_version(desktop: &DesktopState) -> u64 {
    desktop.view.lock().map_or(0, |view| view.panels_revision())
}

pub fn app_update(info: AppUpdateInfo) -> AppUpdate {
    AppUpdate {
        current: info.current,
        latest: info.latest,
        update_available: info.update_available,
        release_url: info.release_url,
    }
}

/// The saved default selection as the snapshot labels the App's pick, which
/// follows it: provider and model by their display names, then the effort.
struct AiLabels {
    label: String,
    provider: Option<String>,
    model: Option<String>,
    effort: Option<String>,
}

impl AiLabels {
    fn of(config: &ErConfig) -> Self {
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
        Self {
            label,
            provider: provider_label,
            model: model_label,
            effort: selection.effort,
        }
    }
}
