//! The settings actor seen from the loop: the replies to settings commands,
//! the notices, and the chrome the settings view decides.
//!
//! A failed command answers twice, as a `CommandFailed` event and as an `Err`
//! reply, and the event names no command. So a failure shown from one side
//! waits briefly for its twin from the other, and the pair shows once; the
//! same failure from a second command shows again.

use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use er_service::{SettingsCommand, SettingsHandle, SettingsReply, SettingsView, WorkspaceError};

type SettingsReceiver = Receiver<Result<SettingsReply, WorkspaceError>>;

/// How long a shown failure waits for its twin. An actor sends both in one
/// step, so this only covers the event relay and a tick.
const PAIR_WINDOW: Duration = Duration::from_secs(1);

/// Where a failure reached the loop.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Event,
    Reply,
}

/// What a reply asks of the loop once it lands, beyond its messages.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Then {
    Nothing,
    /// A picker's pick: the status line names the saved selection.
    AnnouncePick,
    /// The hub's reload: a file that still fails says why again.
    ReportReload,
}

/// What a drain found: failures, which have a twin event, other messages,
/// which have none, and whether a pick saved or a reload answered.
#[derive(Default)]
pub struct Landed {
    pub failures: Vec<String>,
    pub messages: Vec<String>,
    pub picked: bool,
    pub reloaded: bool,
}

#[derive(Default)]
pub struct SettingsBridge {
    replies: Vec<(Then, SettingsReceiver)>,
    /// The start notices shown, with the settings revision they came at.
    start_log: Vec<(u64, String)>,
    /// Failures shown that still wait for their twin.
    unpaired: Vec<(Side, String, Instant)>,
    /// Every notice shown through the bridge.
    #[cfg(test)]
    shown: Vec<String>,
    /// The AI selection label, built once per settings revision.
    label: Option<(u64, String)>,
}

impl SettingsBridge {
    /// Sends `command` without waiting; its reply lands on a later drain.
    pub fn send(&mut self, settings: &SettingsHandle, command: SettingsCommand, then: Then) {
        self.replies.push((then, settings.send(command)));
    }

    /// The replies that landed: failures, a refresh's per-provider errors,
    /// and an editor's warnings.
    pub fn drain(&mut self) -> Landed {
        let mut landed = Landed::default();
        self.replies
            .retain(|(then, replies)| match replies.try_recv() {
                Ok(reply) => {
                    let messages = &mut landed.messages;
                    landed.picked |= *then == Then::AnnouncePick && reply.is_ok();
                    landed.reloaded |= *then == Then::ReportReload && reply.is_ok();
                    match reply {
                        Err(error) => landed.failures.push(error.to_string()),
                        Ok(SettingsReply::ModelsRefreshed { errors, .. }) => {
                            messages.extend(errors)
                        }
                        Ok(SettingsReply::ConfigChanged { warnings, .. }) => {
                            messages.extend(warnings)
                        }
                        Ok(_) => {}
                    }
                    false
                }
                Err(TryRecvError::Empty) => true,
                Err(TryRecvError::Disconnected) => false,
            });
        landed
    }

    /// Whether a start notice is new at the settings `revision`.
    pub fn admit_once(&mut self, revision: u64, message: &str) -> bool {
        let entry = (revision, message.to_string());
        if self.start_log.contains(&entry) {
            return false;
        }
        self.start_log.push(entry);
        self.record(message);
        true
    }

    /// Whether a failure shows: it does unless it is the twin of one shown
    /// from the other side within the window.
    pub fn admit_failure(&mut self, side: Side, message: &str) -> bool {
        let now = Instant::now();
        self.unpaired
            .retain(|(_, _, at)| now.duration_since(*at) < PAIR_WINDOW);
        let twin = self
            .unpaired
            .iter()
            .position(|(from, shown, _)| *from != side && shown == message);
        if let Some(at) = twin {
            self.unpaired.remove(at);
            return false;
        }
        self.unpaired.push((side, message.to_string(), now));
        self.record(message);
        true
    }

    /// Records a notice that has no twin to wait for.
    pub fn admit_message(&mut self, message: &str) {
        self.record(message);
    }

    #[cfg(test)]
    fn record(&mut self, message: &str) {
        self.shown.push(message.to_string());
    }

    #[cfg(not(test))]
    #[expect(clippy::unused_self, reason = "only a test reads the notices shown")]
    fn record(&mut self, _message: &str) {}

    /// How many times `message` was shown.
    #[cfg(test)]
    pub fn times_shown(&self, message: &str) -> usize {
        self.shown.iter().filter(|shown| *shown == message).count()
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
