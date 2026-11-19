//! `er_send`'s body: each wire command applied by the one owner of what it
//! writes. Every command so far is a settings write, so the owner is the
//! settings actor (ADR 0069).

use er_engine::uninstall::UninstallResult;
use er_protocol::{
    Ack, Command, ErError, NoticeLevel, ResourceChanged, ResourceKey, ResourceVersion,
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

/// Applies `command` and waits for its owner's answer. What the person should
/// read once (an editor's soft warning, a provider whose model listing
/// failed) goes to `notify`, since an `Ack` carries only versions.
pub fn apply(
    desktop: &DesktopState,
    command: Command,
    notify: &dyn Fn(NoticeLevel, String),
) -> Result<Applied, ErError> {
    let settings = desktop.service.settings();
    let reply = settings.send_and_wait(er_service::settings_command(command))?;
    let mut uninstalled = None;
    match reply {
        SettingsReply::ConfigChanged { warnings, .. } => {
            for warning in warnings {
                notify(NoticeLevel::Info, warning);
            }
        }
        SettingsReply::ModelsRefreshed { errors, .. } => {
            for error in errors {
                notify(NoticeLevel::Error, error);
            }
        }
        SettingsReply::Uninstalled(result) => uninstalled = Some(result),
        SettingsReply::AiSelectionSaved { .. }
        | SettingsReply::UninstallPreviewed(_)
        | SettingsReply::RiskRulesLoaded { .. } => {}
    }
    // The actor publishes before it answers, so the view read now carries
    // the command.
    let version = ResourceVersion(settings.view().revision);
    Ok(Applied {
        ack: Ack {
            changed: vec![ResourceChanged {
                key: ResourceKey::Settings,
                version,
            }],
            created: None,
        },
        uninstalled,
    })
}
