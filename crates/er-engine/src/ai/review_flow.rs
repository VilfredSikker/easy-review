//! Pure state machine and triage routing for an automated review flow.

use super::experts::expert_by_id;
use super::triage::{TriageReview, TriageVerdictPrimary};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowDepth {
    Quick,
    Standard,
    Deep,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlowTaskStage {
    Triage,
    Finding,
    Verifying,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlowTaskStatus {
    Waiting,
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
    Skipped,
}

/// One retained, model-specific step in the captured flow plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowTaskProgress {
    /// Stable logical ID. Background IDs are kept separately because they are
    /// runtime IDs and may not exist for arena reviewers.
    pub id: String,
    pub stage: FlowTaskStage,
    pub lens: Option<String>,
    pub label: String,
    pub purpose: String,
    pub provider_id: String,
    pub provider_label: String,
    pub model_id: String,
    pub model_label: String,
    pub status: FlowTaskStatus,
    pub error: Option<String>,
    pub background_task_id: Option<String>,
    pub round: Option<u32>,
    pub total_rounds: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlowState {
    Triaging,
    Skipped,
    AwaitingConfirmation,
    Finding,
    Verifying,
    Done {
        kept: usize,
        dropped: usize,
        awaiting_accept: usize,
    },
    Failed {
        reason: String,
    },
    Cancelled,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReviewFlow {
    pub diff_hash: String,
    pub depth: Option<FlowDepth>,
    pub lenses: Vec<String>,
    pub pending_finders: Vec<String>,
    pub failed_finders: Vec<(String, String)>,
    pub arena_run_id: Option<String>,
    pub estimated_cost: Option<f64>,
    pub estimated_latency_seconds: Option<f64>,
    /// Professor triage routes through the ordinary Quick general review and
    /// keeps the older tutorial follow-up suggestion available to the UI.
    pub professor_follow_up_suggested: bool,
    /// Providers whose finder tasks completed successfully.
    pub successful_finder_providers: Vec<String>,
    /// Provider that completed arbitration for a successful verification.
    pub successful_arbiter_provider: Option<String>,
    /// Sorted unique providers used by a successful verification.
    pub providers_used: Vec<String>,
    /// Captured, retained work plan and per-task progress for UI projection.
    pub tasks: Vec<FlowTaskProgress>,
    pub state: FlowState,
    successful_finders: HashSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlowEvent {
    TriageSucceeded {
        plan: Option<(FlowDepth, Vec<String>)>,
    },
    TriageFailed {
        reason: String,
    },
    FinderSucceeded {
        lens: String,
    },
    FinderFailed {
        lens: String,
        reason: String,
    },
    Confirmed,
    Declined,
    DeepStarted {
        run_id: String,
    },
    VerifyStarted,
    VerifyDone {
        kept: usize,
        dropped: usize,
        awaiting_accept: usize,
    },
    VerifyFailed {
        reason: String,
    },
    /// A driver or arena-start failure that cannot be represented by the
    /// stage-specific failure events above.
    DriverFailed {
        reason: String,
    },
    NoCandidates,
    Cancel,
}

impl ReviewFlow {
    pub fn new(diff_hash: impl Into<String>) -> Self {
        Self {
            diff_hash: diff_hash.into(),
            depth: None,
            lenses: Vec::new(),
            pending_finders: Vec::new(),
            failed_finders: Vec::new(),
            arena_run_id: None,
            estimated_cost: None,
            estimated_latency_seconds: None,
            professor_follow_up_suggested: false,
            successful_finder_providers: Vec::new(),
            successful_arbiter_provider: None,
            providers_used: Vec::new(),
            tasks: Vec::new(),
            state: FlowState::Triaging,
            successful_finders: HashSet::new(),
        }
    }

    pub fn is_running(&self) -> bool {
        matches!(
            self.state,
            FlowState::Triaging
                | FlowState::AwaitingConfirmation
                | FlowState::Finding
                | FlowState::Verifying
        )
    }

    /// Apply one event. Events that are late, duplicated, or invalid for the
    /// current state are ignored without changing any field.
    pub fn apply(&mut self, event: FlowEvent) {
        match event {
            FlowEvent::TriageSucceeded { plan } if self.state == FlowState::Triaging => {
                let Some((depth, lenses)) = plan else {
                    self.state = FlowState::Skipped;
                    return;
                };
                self.depth = Some(depth);
                let mut seen = HashSet::new();
                self.lenses = lenses
                    .into_iter()
                    .filter(|lens| seen.insert(lens.clone()))
                    .collect();
                match depth {
                    FlowDepth::Quick | FlowDepth::Standard => {
                        self.pending_finders.clone_from(&self.lenses);
                        self.state = FlowState::Finding;
                    }
                    FlowDepth::Deep => self.state = FlowState::AwaitingConfirmation,
                }
            }
            FlowEvent::TriageFailed { reason } if self.state == FlowState::Triaging => {
                self.state = FlowState::Failed { reason };
            }
            FlowEvent::Confirmed if self.state == FlowState::AwaitingConfirmation => {
                self.state = FlowState::Finding;
            }
            FlowEvent::Declined if self.state == FlowState::AwaitingConfirmation => {
                self.state = FlowState::Cancelled;
            }
            FlowEvent::DeepStarted { run_id }
                if self.state == FlowState::Finding
                    && self.depth == Some(FlowDepth::Deep)
                    && !run_id.is_empty() =>
            {
                self.arena_run_id = Some(run_id);
                self.state = FlowState::Verifying;
            }
            FlowEvent::FinderSucceeded { lens } if self.state == FlowState::Finding => {
                if take_pending(&mut self.pending_finders, &lens) {
                    self.successful_finders.insert(lens);
                }
            }
            FlowEvent::FinderFailed { lens, reason } if self.state == FlowState::Finding => {
                if take_pending(&mut self.pending_finders, &lens) {
                    self.failed_finders.push((lens, reason));
                    if self.pending_finders.is_empty() && self.successful_finders.is_empty() {
                        self.state = FlowState::Failed {
                            reason: "all planned finders failed".into(),
                        };
                    }
                }
            }
            FlowEvent::VerifyStarted if self.can_verify() => self.state = FlowState::Verifying,
            FlowEvent::NoCandidates if self.can_verify() => {
                self.state = FlowState::Done {
                    kept: 0,
                    dropped: 0,
                    awaiting_accept: 0,
                };
            }
            FlowEvent::VerifyDone {
                kept,
                dropped,
                awaiting_accept,
            } if self.state == FlowState::Verifying => {
                self.state = FlowState::Done {
                    kept,
                    dropped,
                    awaiting_accept,
                };
            }
            FlowEvent::VerifyFailed { reason } if self.state == FlowState::Verifying => {
                self.state = FlowState::Failed { reason };
            }
            FlowEvent::DriverFailed { reason } if self.is_running() => {
                self.state = FlowState::Failed { reason };
            }
            FlowEvent::Cancel if self.is_running() => self.state = FlowState::Cancelled,
            _ => {}
        }
    }

    fn can_verify(&self) -> bool {
        self.state == FlowState::Finding
            && self.pending_finders.is_empty()
            && !self.successful_finders.is_empty()
    }
}

fn take_pending(pending: &mut Vec<String>, lens: &str) -> bool {
    if let Some(index) = pending.iter().position(|item| item == lens) {
        pending.remove(index);
        true
    } else {
        false
    }
}

/// Map a triage verdict to a stable, deduplicated review plan.
pub fn plan_for_triage(triage: &TriageReview) -> Option<(FlowDepth, Vec<String>)> {
    let flagged = triage.sensitive.is_flagged();
    let (depth, needs_experts) = match triage.verdict.primary {
        TriageVerdictPrimary::Skip if flagged => {
            return Some((FlowDepth::Quick, vec!["security".into()]));
        }
        TriageVerdictPrimary::Skip => return None,
        TriageVerdictPrimary::General => (FlowDepth::Quick, false),
        TriageVerdictPrimary::Professor => (FlowDepth::Quick, false),
        TriageVerdictPrimary::Expert => (FlowDepth::Standard, true),
        TriageVerdictPrimary::Arena => (FlowDepth::Deep, true),
    };
    let mut lenses = vec!["general".to_owned()];
    if flagged {
        lenses.push("security".to_owned());
    }
    if needs_experts {
        let mut seen = HashSet::new();
        let experts: Vec<String> = triage
            .verdict
            .experts
            .iter()
            .filter(|id| id.as_str() != "mentorship" && expert_by_id(id).is_some())
            .filter(|id| !flagged || id.as_str() != "security")
            .filter(|id| seen.insert(id.as_str()))
            .cloned()
            .collect();
        if experts.is_empty() && !lenses.iter().any(|lens| lens == "security") {
            lenses.push("security".to_owned());
        } else {
            lenses.extend(experts);
        }
    }
    Some((depth, lenses))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::triage::{TriageVerdict, TriageVerdictPrimary};

    fn flow() -> ReviewFlow {
        ReviewFlow::new("hash")
    }
    fn planned(depth: FlowDepth, lenses: &[&str]) -> ReviewFlow {
        let mut f = flow();
        f.apply(FlowEvent::TriageSucceeded {
            plan: Some((depth, lenses.iter().map(|s| s.to_string()).collect())),
        });
        f
    }
    fn triage(primary: TriageVerdictPrimary, experts: &[&str]) -> TriageReview {
        TriageReview {
            version: 1,
            diff_hash: "h".into(),
            diff_scope: String::new(),
            created_at: String::new(),
            first_impression: String::new(),
            diff_stats: Default::default(),
            verdict: TriageVerdict {
                primary,
                experts: experts.iter().map(|s| s.to_string()).collect(),
                rationale: String::new(),
                confidence: String::new(),
            },
            priority_files: Vec::new(),
            reach: Default::default(),
            sensitive: Default::default(),
        }
    }

    fn flagged(mut triage: TriageReview) -> TriageReview {
        triage
            .sensitive
            .reasons
            .push(crate::ai::triage::TriageSensitiveReason {
                kind: "auth".into(),
                evidence: "src/auth.rs:4".into(),
                note: String::new(),
            });
        triage
    }

    #[test]
    fn transitions_triage_success_skip_and_failure() {
        let mut f = flow();
        assert!(f.is_running());
        f.apply(FlowEvent::TriageSucceeded {
            plan: Some((FlowDepth::Quick, vec!["general".into()])),
        });
        assert_eq!(f.state, FlowState::Finding);
        assert_eq!(f.pending_finders, ["general"]);
        let mut skip = flow();
        skip.apply(FlowEvent::TriageSucceeded { plan: None });
        assert_eq!(skip.state, FlowState::Skipped);
        assert!(!skip.is_running());
        let mut failed = flow();
        failed.apply(FlowEvent::TriageFailed {
            reason: "missing or stale".into(),
        });
        assert_eq!(
            failed.state,
            FlowState::Failed {
                reason: "missing or stale".into()
            }
        );
        let mut duplicates = flow();
        duplicates.apply(FlowEvent::TriageSucceeded {
            plan: Some((FlowDepth::Quick, vec!["general".into(), "general".into()])),
        });
        assert_eq!(duplicates.lenses, ["general"]);
        assert_eq!(duplicates.pending_finders, ["general"]);
    }

    #[test]
    fn finder_events_consume_once_and_partial_failure_can_verify() {
        let mut f = planned(FlowDepth::Standard, &["general", "security"]);
        f.apply(FlowEvent::FinderSucceeded {
            lens: "general".into(),
        });
        let after_first = f.clone();
        f.apply(FlowEvent::FinderSucceeded {
            lens: "general".into(),
        });
        f.apply(FlowEvent::FinderFailed {
            lens: "unknown".into(),
            reason: "x".into(),
        });
        assert_eq!(f, after_first);
        f.apply(FlowEvent::FinderFailed {
            lens: "security".into(),
            reason: "offline".into(),
        });
        assert_eq!(f.state, FlowState::Finding);
        assert!(f.pending_finders.is_empty());
        f.apply(FlowEvent::VerifyStarted);
        assert_eq!(f.state, FlowState::Verifying);
        f.apply(FlowEvent::VerifyDone {
            kept: 1,
            dropped: 2,
            awaiting_accept: 3,
        });
        assert_eq!(
            f.state,
            FlowState::Done {
                kept: 1,
                dropped: 2,
                awaiting_accept: 3
            }
        );
        assert!(!f.is_running());
        let done = f.clone();
        f.apply(FlowEvent::FinderSucceeded {
            lens: "security".into(),
        });
        assert_eq!(f, done);
    }

    #[test]
    fn driver_failure_is_terminal_from_any_running_stage_and_ignored_late() {
        for mut f in [
            flow(),
            planned(FlowDepth::Deep, &["general"]),
            planned(FlowDepth::Quick, &["general"]),
        ] {
            if f.state == FlowState::Finding && f.depth == Some(FlowDepth::Quick) {
                f.apply(FlowEvent::FinderSucceeded {
                    lens: "general".into(),
                });
                f.apply(FlowEvent::VerifyStarted);
            }
            f.apply(FlowEvent::DriverFailed {
                reason: "start failed".into(),
            });
            assert_eq!(
                f.state,
                FlowState::Failed {
                    reason: "start failed".into()
                }
            );
            let failed = f.clone();
            f.apply(FlowEvent::VerifyDone {
                kept: 1,
                dropped: 0,
                awaiting_accept: 0,
            });
            assert_eq!(f, failed);
        }
    }

    #[test]
    fn all_finder_failures_fail_flow() {
        let mut f = planned(FlowDepth::Quick, &["general"]);
        f.apply(FlowEvent::FinderFailed {
            lens: "general".into(),
            reason: "no output".into(),
        });
        assert_eq!(f.failed_finders, [("general".into(), "no output".into())]);
        assert_eq!(
            f.state,
            FlowState::Failed {
                reason: "all planned finders failed".into()
            }
        );
    }

    #[test]
    fn no_candidates_finishes_without_run_id() {
        let mut f = planned(FlowDepth::Quick, &["general"]);
        f.apply(FlowEvent::FinderSucceeded {
            lens: "general".into(),
        });
        f.apply(FlowEvent::NoCandidates);
        assert_eq!(
            f.state,
            FlowState::Done {
                kept: 0,
                dropped: 0,
                awaiting_accept: 0
            }
        );
        assert_eq!(f.arena_run_id, None);
    }

    #[test]
    fn deep_requires_confirmation_and_explicit_start() {
        let mut f = planned(FlowDepth::Deep, &["general", "security"]);
        assert_eq!(f.state, FlowState::AwaitingConfirmation);
        assert!(f.pending_finders.is_empty());
        f.apply(FlowEvent::DeepStarted {
            run_id: "early".into(),
        });
        assert_eq!(f.state, FlowState::AwaitingConfirmation);
        f.apply(FlowEvent::Confirmed);
        assert_eq!(f.state, FlowState::Finding);
        f.apply(FlowEvent::DeepStarted {
            run_id: "run-1".into(),
        });
        assert_eq!(f.state, FlowState::Verifying);
        assert_eq!(f.arena_run_id.as_deref(), Some("run-1"));
        assert!(f.successful_finders.is_empty());
        f.apply(FlowEvent::VerifyFailed {
            reason: "arena failed".into(),
        });
        assert_eq!(
            f.state,
            FlowState::Failed {
                reason: "arena failed".into()
            }
        );
        let mut declined = planned(FlowDepth::Deep, &[]);
        declined.apply(FlowEvent::Declined);
        assert_eq!(declined.state, FlowState::Cancelled);
    }

    #[test]
    fn invalid_events_and_cancellation_are_guarded() {
        let mut f = flow();
        let before = f.clone();
        f.apply(FlowEvent::VerifyStarted);
        assert_eq!(f, before);
        f.apply(FlowEvent::Cancel);
        assert_eq!(f.state, FlowState::Cancelled);
        assert!(!f.is_running());
        let cancelled = f.clone();
        f.apply(FlowEvent::TriageFailed {
            reason: "late".into(),
        });
        assert_eq!(f, cancelled);
        let mut no_success = planned(FlowDepth::Quick, &["general"]);
        no_success.pending_finders.clear();
        let before = no_success.clone();
        no_success.apply(FlowEvent::NoCandidates);
        assert_eq!(no_success, before);
        no_success.apply(FlowEvent::DeepStarted { run_id: "".into() });
        assert_eq!(no_success, before);
    }

    #[test]
    fn triage_routes_all_verdicts() {
        assert_eq!(
            plan_for_triage(&triage(TriageVerdictPrimary::Skip, &[])),
            None
        );
        assert_eq!(
            plan_for_triage(&triage(TriageVerdictPrimary::General, &[])),
            Some((FlowDepth::Quick, vec!["general".into()]))
        );
        assert_eq!(
            plan_for_triage(&triage(TriageVerdictPrimary::Professor, &[])),
            Some((FlowDepth::Quick, vec!["general".into()]))
        );
        assert_eq!(
            plan_for_triage(&triage(TriageVerdictPrimary::Expert, &["api", "security"])),
            Some((
                FlowDepth::Standard,
                vec!["general".into(), "api".into(), "security".into()]
            ))
        );
        assert_eq!(
            plan_for_triage(&triage(TriageVerdictPrimary::Arena, &["performance"])),
            Some((
                FlowDepth::Deep,
                vec!["general".into(), "performance".into()]
            ))
        );
    }

    #[test]
    fn triage_filters_unknown_mentorship_and_duplicates_with_fallback() {
        assert_eq!(
            plan_for_triage(&triage(
                TriageVerdictPrimary::Expert,
                &["unknown", "mentorship", "security", "security", "api"]
            )),
            Some((
                FlowDepth::Standard,
                vec!["general".into(), "security".into(), "api".into()]
            ))
        );
        assert_eq!(
            plan_for_triage(&triage(
                TriageVerdictPrimary::Arena,
                &["unknown", "mentorship"]
            )),
            Some((FlowDepth::Deep, vec!["general".into(), "security".into()]))
        );
    }

    #[test]
    fn evidenced_sensitivity_routes_security_at_every_verdict() {
        assert_eq!(
            plan_for_triage(&flagged(triage(TriageVerdictPrimary::Skip, &[]))),
            Some((FlowDepth::Quick, vec!["security".into()]))
        );
        assert_eq!(
            plan_for_triage(&flagged(triage(TriageVerdictPrimary::General, &[]))),
            Some((FlowDepth::Quick, vec!["general".into(), "security".into()]))
        );
        assert_eq!(
            plan_for_triage(&flagged(triage(TriageVerdictPrimary::Professor, &[]))),
            Some((FlowDepth::Quick, vec!["general".into(), "security".into()]))
        );
        assert_eq!(
            plan_for_triage(&flagged(triage(
                TriageVerdictPrimary::Expert,
                &["api", "security", "api"]
            ))),
            Some((
                FlowDepth::Standard,
                vec!["general".into(), "security".into(), "api".into()]
            ))
        );
        assert_eq!(
            plan_for_triage(&flagged(triage(
                TriageVerdictPrimary::Arena,
                &["performance", "security"]
            ))),
            Some((
                FlowDepth::Deep,
                vec!["general".into(), "security".into(), "performance".into()]
            ))
        );
    }
}
