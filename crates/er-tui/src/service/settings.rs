//! The settings actor seen from the loop: the replies to settings commands,
//! the notice log, and the chrome the settings view decides.
//!
//! A failed settings command answers twice, as a `CommandFailed` event and as
//! an `Err` reply, so every notice passes through one log that shows each
//! (settings revision, message) once.

use std::sync::mpsc::{Receiver, TryRecvError};

use er_service::{SettingsCommand, SettingsHandle, SettingsReply, SettingsView, WorkspaceError};

type SettingsReceiver = Receiver<Result<SettingsReply, WorkspaceError>>;

#[derive(Default)]
pub struct SettingsBridge {
    replies: Vec<SettingsReceiver>,
    /// Every notice let through, with the settings revision it came at.
    log: Vec<(u64, String)>,
    /// The AI selection label, built once per settings revision.
    label: Option<(u64, String)>,
}

impl SettingsBridge {
    /// Sends `command` without waiting; its reply lands on a later drain.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the config hub sends through it in P66.2")
    )]
    pub fn send(&mut self, settings: &SettingsHandle, command: SettingsCommand) {
        self.replies.push(settings.send(command));
    }

    /// The messages of the replies that landed: a failure, a refresh's
    /// per-provider errors, or an editor's warnings.
    pub fn drain(&mut self) -> Vec<String> {
        let mut messages = Vec::new();
        self.replies.retain(|replies| match replies.try_recv() {
            Ok(reply) => {
                match reply {
                    Err(error) => messages.push(error.to_string()),
                    Ok(SettingsReply::ModelsRefreshed { errors, .. }) => messages.extend(errors),
                    Ok(SettingsReply::ConfigChanged { warnings, .. }) => messages.extend(warnings),
                    Ok(_) => {}
                }
                false
            }
            Err(TryRecvError::Empty) => true,
            Err(TryRecvError::Disconnected) => false,
        });
        messages
    }

    /// Whether `message` is new at the settings `revision`; a new one goes
    /// in the log.
    pub fn admit(&mut self, revision: u64, message: &str) -> bool {
        let entry = (revision, message.to_string());
        if self.log.contains(&entry) {
            return false;
        }
        self.log.push(entry);
        true
    }

    /// How many times `message` was let through.
    #[cfg(test)]
    pub fn times_shown(&self, message: &str) -> usize {
        self.log
            .iter()
            .filter(|(_, shown)| shown == message)
            .count()
    }

    /// The default AI selection's label, from er-service's rule.
    pub fn ai_label(&mut self, view: &SettingsView) -> String {
        match &self.label {
            Some((revision, label)) if *revision == view.revision => label.clone(),
            _ => {
                let label = er_service::ai_labels(&view.config).label;
                self.label = Some((view.revision, label.clone()));
                label
            }
        }
    }
}
