//! In-memory progress for a live arena run.
//!
//! The running UI needs per-reviewer detail inside a round: who is thinking,
//! who has finished, and why a reviewer failed. That detail belongs to the
//! process that owns the run, so it lives on the run's control and is updated
//! by the supervisor and the panel's jobs as they work. `run.json` carries the
//! per-round and per-reviewer status any process can read; nothing here is a
//! durable artifact.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// Wakes the host after a live arena write.
pub type ArenaNotify = Arc<dyn Fn() + Send + Sync>;

/// One progress event from a running supervisor or panel.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProgressEvent {
    RoundStarted {
        round: u8,
        total_rounds: u8,
    },
    ReviewerThinking {
        reviewer_id: String,
        round: u8,
    },
    ReviewerDone {
        reviewer_id: String,
        round: u8,
        findings_count: usize,
    },
    ReviewerFailed {
        reviewer_id: String,
        round: u8,
        reason: String,
    },
    FindingVerdict {
        finding_id: String,
        verdict: String,
        confidence: f32,
    },
    ArbiterStarted {
        arbiter_label: String,
    },
    RunComplete {
        run_id: String,
    },
}

/// What a run's live progress currently is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArenaProgress {
    pub round: u8,
    pub total_rounds: u8,
    /// Empty during reviewer rounds; `"arbiter"` during the final phase.
    pub phase: String,
    pub thinking: BTreeSet<String>,
    pub done: BTreeSet<String>,
    /// Reviewer failures this round, with their reasons.
    pub failed: BTreeMap<String, String>,
    /// Every reviewer done, by round. The sets above are the current round
    /// only; this history is what a subscriber that polls late reads, so a
    /// fast round cannot vanish between two polls.
    pub done_by_round: BTreeMap<u8, BTreeSet<String>>,
    pub failed_by_round: BTreeMap<u8, BTreeMap<String, String>>,
}

impl ArenaProgress {
    /// Fold one event into the state. Applying an event twice is a no-op, and
    /// a round start clears the previous round's live sets.
    pub fn apply(&mut self, event: &ProgressEvent) {
        match event {
            ProgressEvent::RoundStarted {
                round,
                total_rounds,
            } => {
                self.thinking.clear();
                self.done.clear();
                self.failed.clear();
                self.round = *round;
                self.total_rounds = *total_rounds;
                self.phase.clear();
            }
            ProgressEvent::ArbiterStarted { .. } => {
                self.thinking.clear();
                self.done.clear();
                self.phase = "arbiter".into();
            }
            ProgressEvent::ReviewerThinking { reviewer_id, round } => {
                self.round = *round;
                self.thinking.insert(reviewer_id.clone());
            }
            ProgressEvent::ReviewerDone {
                reviewer_id, round, ..
            } => {
                self.round = *round;
                self.thinking.remove(reviewer_id);
                self.done.insert(reviewer_id.clone());
                self.done_by_round
                    .entry(*round)
                    .or_default()
                    .insert(reviewer_id.clone());
            }
            ProgressEvent::ReviewerFailed {
                reviewer_id,
                round,
                reason,
            } => {
                self.round = *round;
                self.thinking.remove(reviewer_id);
                self.failed.insert(reviewer_id.clone(), reason.clone());
                self.failed_by_round
                    .entry(*round)
                    .or_default()
                    .insert(reviewer_id.clone(), reason.clone());
            }
            ProgressEvent::RunComplete { .. } => {
                self.thinking.clear();
            }
            ProgressEvent::FindingVerdict { .. } => {}
        }
    }

    /// The shape the desktop and the running pill render.
    pub fn snapshot(&self) -> ArenaProgressState {
        ArenaProgressState {
            round: self.round,
            total_rounds: self.total_rounds,
            phase: self.phase.clone(),
            thinking: self.thinking.iter().cloned().collect(),
            done: self.done.iter().cloned().collect(),
        }
    }
}

/// Latest reviewer activity for the running UI.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ArenaProgressState {
    pub round: u8,
    pub total_rounds: u8,
    /// Empty during reviewer rounds; `"arbiter"` during final arbiter phase.
    #[serde(default)]
    pub phase: String,
    #[serde(default)]
    pub thinking: Vec<String>,
    #[serde(default)]
    pub done: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_reviewer_stops_thinking_and_keeps_its_reason() {
        let mut progress = ArenaProgress::default();
        progress.apply(&ProgressEvent::RoundStarted {
            round: 1,
            total_rounds: 3,
        });
        progress.apply(&ProgressEvent::ReviewerThinking {
            reviewer_id: "reviewer-a".into(),
            round: 1,
        });
        progress.apply(&ProgressEvent::ReviewerFailed {
            reviewer_id: "reviewer-a".into(),
            round: 1,
            reason: "provider exited".into(),
        });

        assert!(progress.thinking.is_empty());
        assert_eq!(
            progress.failed.get("reviewer-a").map(String::as_str),
            Some("provider exited")
        );
        let snapshot = progress.snapshot();
        assert_eq!(snapshot.round, 1);
        assert_eq!(snapshot.total_rounds, 3);
    }

    #[test]
    fn a_round_start_clears_the_previous_rounds_live_state() {
        let mut progress = ArenaProgress::default();
        progress.apply(&ProgressEvent::RoundStarted {
            round: 1,
            total_rounds: 3,
        });
        progress.apply(&ProgressEvent::ReviewerDone {
            reviewer_id: "a".into(),
            round: 1,
            findings_count: 1,
        });
        progress.apply(&ProgressEvent::ReviewerFailed {
            reviewer_id: "b".into(),
            round: 1,
            reason: "bad output".into(),
        });

        progress.apply(&ProgressEvent::RoundStarted {
            round: 2,
            total_rounds: 3,
        });
        assert_eq!(progress.round, 2);
        assert!(progress.done.is_empty());
        assert!(progress.failed.is_empty());
        assert_eq!(
            progress.done_by_round.get(&1),
            Some(&BTreeSet::from(["a".to_string()])),
            "a late poll still sees round 1 finish"
        );
        assert_eq!(
            progress
                .failed_by_round
                .get(&1)
                .and_then(|round| round.get("b"))
                .map(String::as_str),
            Some("bad output")
        );
    }

    #[test]
    fn the_arbiter_phase_and_its_thinking_are_visible() {
        let mut progress = ArenaProgress::default();
        progress.apply(&ProgressEvent::ArbiterStarted {
            arbiter_label: "Arbiter".into(),
        });
        assert_eq!(progress.phase, "arbiter");
        assert_eq!(progress.snapshot().phase, "arbiter");

        progress.apply(&ProgressEvent::ReviewerThinking {
            reviewer_id: "arbiter".into(),
            round: 3,
        });
        assert_eq!(progress.snapshot().thinking, ["arbiter"]);
    }

    /// The desktop's `arena_progress` response keeps its four fields and their
    /// types; the state is serialized where the command returns it.
    #[test]
    fn the_public_progress_shape_is_unchanged() {
        let mut progress = ArenaProgress::default();
        progress.apply(&ProgressEvent::RoundStarted {
            round: 2,
            total_rounds: 3,
        });
        progress.apply(&ProgressEvent::ReviewerThinking {
            reviewer_id: "a".into(),
            round: 2,
        });
        progress.apply(&ProgressEvent::ReviewerDone {
            reviewer_id: "b".into(),
            round: 2,
            findings_count: 0,
        });
        let value = serde_json::to_value(progress.snapshot()).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "round": 2,
                "total_rounds": 3,
                "phase": "",
                "thinking": ["a"],
                "done": ["b"],
            })
        );
    }
}
