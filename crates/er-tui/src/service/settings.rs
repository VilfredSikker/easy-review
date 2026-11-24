//! The settings actor seen from the loop: the replies to settings commands,
//! the notices, and the chrome the settings view decides.
//!
//! A failed command answers twice, as a `CommandFailed` event and as an `Err`
//! reply, and the event names no command. So a failure shown from one side
//! waits briefly for its twin from the other, and the pair shows once; the
//! same failure from a second command shows again. A reply the loop keeps for
//! itself still meets its event here, so that event never waits on as an
//! unpaired entry another command's failure could pair with.

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

/// What a drain found: failures, which have a twin event, and other
/// messages, which have none.
#[derive(Default)]
pub struct Landed {
    pub failures: Vec<String>,
    pub messages: Vec<String>,
}

#[derive(Default)]
pub struct SettingsBridge {
    replies: Vec<SettingsReceiver>,
    /// The start notices shown, with the settings revision they came at.
    start_log: Vec<(u64, String)>,
    /// Failures that still wait for their twin, and whether that twin shows.
    unpaired: Vec<(Side, String, Instant, bool)>,
    /// Every notice shown through the bridge.
    #[cfg(test)]
    shown: Vec<String>,
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

    /// The replies that landed: failures, a refresh's per-provider errors,
    /// and an editor's warnings.
    pub fn drain(&mut self) -> Landed {
        let mut landed = Landed::default();
        self.replies.retain(|replies| match replies.try_recv() {
            Ok(reply) => {
                let messages = &mut landed.messages;
                match reply {
                    Err(error) => landed.failures.push(error.to_string()),
                    Ok(SettingsReply::ModelsRefreshed { errors, .. }) => messages.extend(errors),
                    Ok(SettingsReply::ConfigChanged { warnings, .. }) => messages.extend(warnings),
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
        let show = match self.take_twin(side, message) {
            Some(twin_shows) => twin_shows,
            None => {
                self.unpaired
                    .push((side, message.to_string(), Instant::now(), false));
                true
            }
        };
        if show {
            self.record(message);
        }
        show
    }

    /// Takes a reply the loop shows its own way (a first load's error): it
    /// pairs with its event, and that event shows as it would alone.
    pub fn absorb_reply(&mut self, message: &str) {
        if self.take_twin(Side::Reply, message).is_none() {
            self.unpaired
                .push((Side::Reply, message.to_string(), Instant::now(), true));
        }
    }

    /// Removes the unpaired failure from the other side with this text, and
    /// answers whether `side`'s report of it is to show.
    fn take_twin(&mut self, side: Side, message: &str) -> Option<bool> {
        let now = Instant::now();
        self.unpaired
            .retain(|(_, _, at, _)| now.duration_since(*at) < PAIR_WINDOW);
        let at = self
            .unpaired
            .iter()
            .position(|(from, shown, _, _)| *from != side && shown == message)?;
        Some(self.unpaired.remove(at).3)
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
