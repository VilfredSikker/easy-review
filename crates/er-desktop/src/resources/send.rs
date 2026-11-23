//! `er_send`'s body: each wire command applied by the one owner of what it
//! writes. A settings write, an AI pick or a risk read goes to the settings
//! actor (ADR 0069); a project, branch, PR or inbox edit goes to the
//! sidebar's owner; the panel mirror and the update check are the desktop's
//! own. A tab command or an open goes to the app registry (`tabs`); a diff
//! command or a reviewed write goes to the tab's workspace actor.

use er_engine::config::AiSelection;
use er_engine::uninstall::UninstallResult;
use er_protocol::{
    Ack, AppUpdate, Command, ErError, NoticeLevel, Panel, ResourceChanged, ResourceKey,
    ResourceVersion,
};
use er_service::SettingsReply;

use crate::state::DesktopState;

/// What a command leaves for the caller beside its `Ack`.
#[derive(Debug)]
pub struct Applied {
    pub ack: Ack,
    /// An uninstall's outcome, which decides whether the app quits.
    pub uninstalled: Option<UninstallResult>,
}

/// What a command reaches beyond its owner, handed in by the caller.
pub struct Effects<'a> {
    /// What the person should read once (an editor's soft warning, a
    /// provider whose model listing failed), since an `Ack` carries only
    /// versions.
    pub notify: &'a dyn Fn(NoticeLevel, String),
    /// Copies a saved AI pick into the App's session pick, which the ask
    /// runs still read.
    pub follow_selection: &'a dyn Fn(&AiSelection),
    /// Compares the running build with the latest release. Blocking; runs
    /// on a thread of its own.
    pub check_update: fn() -> AppUpdate,
    pub webviews: &'a dyn crate::browser_webview::TabWebviews,
    /// Asks for a folder to open; none when the pick is cancelled.
    pub pick_folder: &'a dyn Fn() -> Option<String>,
}

/// Applies `command` and waits for its owner's answer.
pub fn apply(
    desktop: &DesktopState,
    command: Command,
    effects: &Effects<'_>,
) -> Result<Applied, ErError> {
    let command = match super::tabs::apply(desktop, command, effects) {
        super::tabs::Routed::Applied(ack) => {
            return ack.map(|ack| Applied {
                ack,
                uninstalled: None,
            })
        }
        super::tabs::Routed::Other(command) => command,
    };
    let command = match super::diff::apply(desktop, command, effects) {
        super::tabs::Routed::Applied(ack) => {
            return ack.map(|ack| Applied {
                ack,
                uninstalled: None,
            })
        }
        super::tabs::Routed::Other(command) => command,
    };
    let command = match super::reviewed::apply(desktop, command) {
        super::tabs::Routed::Applied(ack) => {
            return ack.map(|ack| Applied {
                ack,
                uninstalled: None,
            })
        }
        super::tabs::Routed::Other(command) => command,
    };
    let command = match super::annotation_writes::apply(desktop, command) {
        super::tabs::Routed::Applied(ack) => {
            return ack.map(|ack| Applied {
                ack,
                uninstalled: None,
            })
        }
        super::tabs::Routed::Other(command) => command,
    };
    let command = match super::arena::apply(desktop, command) {
        super::tabs::Routed::Applied(ack) => {
            return ack.map(|ack| Applied {
                ack,
                uninstalled: None,
            })
        }
        super::tabs::Routed::Other(command) => command,
    };
    let command = match super::ai_asks::apply(desktop, command) {
        super::tabs::Routed::Applied(ack) => {
            return ack.map(|ack| Applied {
                ack,
                uninstalled: None,
            })
        }
        super::tabs::Routed::Other(command) => command,
    };
    let command = match super::runs::apply(desktop, command) {
        super::tabs::Routed::Applied(ack) => {
            return ack.map(|ack| Applied {
                ack,
                uninstalled: None,
            })
        }
        super::tabs::Routed::Other(command) => command,
    };
    let command = match super::flows::apply(desktop, command) {
        super::tabs::Routed::Applied(ack) => {
            return ack.map(|ack| Applied {
                ack,
                uninstalled: None,
            })
        }
        super::tabs::Routed::Other(command) => command,
    };
    let command = match super::ui_annotation_writes::apply(desktop, command) {
        super::tabs::Routed::Applied(ack) => {
            return ack.map(|ack| Applied {
                ack,
                uninstalled: None,
            })
        }
        super::tabs::Routed::Other(command) => command,
    };
    let command = match super::github_writes::apply(desktop, command) {
        super::tabs::Routed::Applied(ack) => {
            return ack.map(|ack| Applied {
                ack,
                uninstalled: None,
            })
        }
        super::tabs::Routed::Other(command) => command,
    };
    let command = match super::pr::apply(desktop, command) {
        super::tabs::Routed::Applied(ack) => {
            return ack.map(|ack| Applied {
                ack,
                uninstalled: None,
            })
        }
        super::tabs::Routed::Other(command) => command,
    };
    let refreshed = match &command {
        Command::RefreshAiModels {
            provider_id: Some(id),
            ..
        } => Some(id.clone()),
        _ => None,
    };
    let settings_command = match er_service::settings_command(command) {
        Ok(command) => command,
        Err(command) => return apply_elsewhere(desktop, *command, effects),
    };
    let settings = desktop.service.settings();
    let reply = settings.send_and_wait(settings_command)?;
    let mut uninstalled = None;
    match reply {
        SettingsReply::ConfigChanged { warnings, .. } => {
            for warning in warnings {
                (effects.notify)(NoticeLevel::Info, warning);
            }
        }
        SettingsReply::ModelsRefreshed { errors, .. } => {
            if errors.is_empty() {
                if let Some(id) = refreshed {
                    if let Some(count) = model_count(&settings.view(), &id) {
                        (effects.notify)(NoticeLevel::Success, count);
                    }
                }
            }
            for error in errors {
                (effects.notify)(NoticeLevel::Error, error);
            }
        }
        SettingsReply::Uninstalled(result) => uninstalled = Some(result),
        // The load is how a repo becomes a rules key, so the key is what it
        // created; the frontend reads `risk(key)` from here on.
        SettingsReply::RiskRulesLoaded { key, version } => {
            return Ok(Applied {
                ack: Ack {
                    changed: vec![ResourceChanged {
                        key: ResourceKey::Risk { key: key.clone() },
                        version: ResourceVersion(version),
                    }],
                    created: Some(key),
                },
                uninstalled: None,
            });
        }
        SettingsReply::AiSelectionSaved { selection, .. } => {
            (effects.follow_selection)(&selection);
        }
        SettingsReply::UninstallPreviewed(_) => {}
    }
    // The actor publishes before it answers, so the view read now carries
    // the command.
    // `session` carries the pick's labels and moves with every settings
    // revision, so it is named too and the picker repaints without the push.
    let view = settings.view();
    Ok(Applied {
        ack: Ack {
            changed: vec![
                ResourceChanged {
                    key: ResourceKey::Settings,
                    version: ResourceVersion(view.revision),
                },
                ResourceChanged {
                    key: ResourceKey::Session,
                    version: ResourceVersion(super::session::session_version(desktop, Some(&view))),
                },
            ],
            created: None,
        },
        uninstalled,
    })
}

/// A command the settings actor does not own: the panel mirror and the update
/// check are the desktop's own, and the rest go to the sidebar's owner.
fn apply_elsewhere(
    desktop: &DesktopState,
    command: Command,
    effects: &Effects<'_>,
) -> Result<Applied, ErError> {
    match command {
        Command::TogglePanel { panel } => Ok(toggle_panel(desktop, panel)),
        Command::CheckAppUpdate => Ok(check_update(desktop, effects.check_update)),
        command => super::host::send(desktop, command).map(|ack| Applied {
            ack,
            uninstalled: None,
        }),
    }
}

/// "Codex: 12 models", what the page showed after a refresh with the flag
/// off; none for a provider the view does not list.
fn model_count(view: &er_service::SettingsView, provider_id: &str) -> Option<String> {
    let provider = view.hub.providers.iter().find(|p| p.id == provider_id)?;
    Some(format!(
        "{}: {} models",
        provider.label,
        provider.models.len()
    ))
}

/// The page painted the toggle itself; this keeps the desktop's copy, which
/// the default build's snapshot reads, in step.
fn toggle_panel(desktop: &DesktopState, panel: Panel) -> Applied {
    let name = match panel {
        Panel::Left => "left",
        Panel::Tree => "tree",
        Panel::Right => "right",
    };
    let version = match desktop.view.lock() {
        Ok(mut view) => {
            view.toggle_panel(name);
            view.panels_revision()
        }
        Err(_) => super::session::panels_version(desktop),
    };
    desktop.session.wake();
    Applied {
        ack: Ack {
            changed: vec![ResourceChanged {
                key: ResourceKey::Panels,
                version: ResourceVersion(version),
            }],
            created: None,
        },
        uninstalled: None,
    }
}

/// Answers at once; the check asks GitHub on a thread of its own and lands
/// in `session`, which the push then brings.
fn check_update(desktop: &DesktopState, check: fn() -> AppUpdate) -> Applied {
    let session = desktop.session.clone();
    let _ = std::thread::Builder::new()
        .name("er-update-check".into())
        .spawn(move || {
            let update = check();
            if let Ok(mut held) = session.update.lock() {
                *held = Some(update);
            }
            session.bump();
        });
    Applied {
        ack: Ack::default(),
        uninstalled: None,
    }
}
