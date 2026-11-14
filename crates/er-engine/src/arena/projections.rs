use super::model::{
    ArbiterView, ArenaFinding, ArenaRun, FunnelCounts, FunnelStage, FunnelStages, MatrixRow,
    Verdict, ARENA_ARBITER_ROUND,
};
use super::orchestrator::{arbiter_display_label, ARBITER_REVIEWER_ID};
use crate::config::ErConfig;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArenaRunSnapshot {
    pub run: ArenaRun,
    pub matrix: Vec<MatrixRow>,
    pub funnel: FunnelStages,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arbiter: Option<ArbiterView>,
    /// D17's free round-two measurement, when this bucket has the run's panel
    /// record. Absent for a single-round run or one from before the panel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub panel: Option<PanelSummary>,
}

/// The panel's measurement as a snapshot carries it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelSummary {
    pub rounds: u8,
    /// Claims two or more reviewers backed after round 1.
    pub round1_confirmed: usize,
    /// The same count after the first cross round; `None` for one round.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round2_confirmed: Option<usize>,
    pub quorum_changed: bool,
}

impl PanelSummary {
    /// The summary for a run's own record; another run's record is not it.
    pub fn for_run(record: &crate::ai::panel_run::PanelRun, run_id: &str) -> Option<Self> {
        if record.run_id != run_id {
            return None;
        }
        Some(Self {
            rounds: record.rounds,
            round1_confirmed: record.round1_quorum.confirmed.len(),
            round2_confirmed: record
                .round2_quorum
                .as_ref()
                .map(|agreement| agreement.confirmed.len()),
            quorum_changed: record.quorum_changed,
        })
    }
}

pub fn build_arbiter_view(config: &ErConfig, run: &ArenaRun) -> Option<ArbiterView> {
    if run.config.rounds < 2 || run.reviewers.len() < 2 {
        return None;
    }
    let label = arbiter_display_label(&config.ai_hub, &run.config.arbiter);
    Some(ArbiterView {
        label,
        provider_id: run.config.arbiter.provider_id.clone(),
        model_id: run.config.arbiter.model_id.clone(),
    })
}

pub fn build_snapshot_with_config(config: &ErConfig, run: ArenaRun) -> ArenaRunSnapshot {
    build_snapshot_with_panel(config, run, None)
}

/// The snapshot with the panel measurement a caller already loaded.
pub fn build_snapshot_with_panel(
    config: &ErConfig,
    run: ArenaRun,
    panel: Option<PanelSummary>,
) -> ArenaRunSnapshot {
    let arbiter = build_arbiter_view(config, &run);
    let matrix = build_matrix(&run.findings);
    let funnel = build_funnel(&run.findings);
    ArenaRunSnapshot {
        run,
        matrix,
        funnel,
        arbiter,
        panel,
    }
}

pub fn build_snapshot(run: ArenaRun) -> ArenaRunSnapshot {
    let matrix = build_matrix(&run.findings);
    let funnel = build_funnel(&run.findings);
    ArenaRunSnapshot {
        run,
        matrix,
        funnel,
        arbiter: None,
        panel: None,
    }
}

fn arbiter_ballot(f: &ArenaFinding) -> (Option<super::model::Vote>, String) {
    for round in &f.rounds {
        if round.n == ARENA_ARBITER_ROUND {
            if let Some(b) = round.log.iter().find(|x| x.reviewer == ARBITER_REVIEWER_ID) {
                return (Some(b.vote.clone()), b.note.clone());
            }
        }
    }
    if !f.rationale.is_empty() {
        return (None, f.rationale.clone());
    }
    (None, String::new())
}

/// Collapse round logs → latest vote per reviewer (matrix layout).
pub fn build_matrix(findings: &[ArenaFinding]) -> Vec<MatrixRow> {
    findings
        .iter()
        .map(|f| {
            let mut latest_vote = BTreeMap::new();
            for round in &f.rounds {
                for ballot in &round.log {
                    if ballot.reviewer == ARBITER_REVIEWER_ID {
                        continue;
                    }
                    latest_vote.insert(ballot.reviewer.clone(), ballot.vote.clone());
                }
            }
            let (arbiter_vote, arbiter_note) = arbiter_ballot(f);
            MatrixRow {
                finding_id: f.id.clone(),
                latest_vote,
                verdict: f.verdict.clone(),
                confidence: f.confidence,
                arbiter_vote,
                arbiter_note,
            }
        })
        .collect()
}

pub fn build_funnel(findings: &[ArenaFinding]) -> FunnelStages {
    let proposed = findings.len();
    let mut cross_checked = 0usize;
    let mut resolved = 0usize;
    let mut final_count = 0usize;
    let mut exited_at = BTreeMap::new();

    for f in findings {
        let has_round2 = f.rounds.iter().any(|r| r.n >= 2 && !r.log.is_empty());
        if has_round2 {
            cross_checked += 1;
        }
        if !matches!(f.verdict, Verdict::Pending) {
            resolved += 1;
        }
        match &f.verdict {
            Verdict::Kept | Verdict::Escalated => {
                final_count += 1;
            }
            Verdict::Merged { .. } => {
                final_count += 1;
                exited_at.insert(f.id.clone(), FunnelStage::Resolved);
            }
            Verdict::Dropped => {
                exited_at.insert(f.id.clone(), FunnelStage::CrossChecked);
            }
            Verdict::Pending => {}
        }
    }

    FunnelStages {
        counts: FunnelCounts {
            proposed,
            cross_checked,
            resolved,
            final_count,
        },
        exited_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::RiskLevel;
    use crate::arena::model::{Ballot, RoundLog, Vote};

    fn finding_with_votes(id: &str, ballots: Vec<Ballot>) -> ArenaFinding {
        ArenaFinding {
            id: id.into(),
            file: "f.rs".into(),
            line: None,
            title: "t".into(),
            body: "b".into(),
            severity_by_round: BTreeMap::from([(1, RiskLevel::High)]),
            raised_by: vec!["a".into()],
            verdict: Verdict::Kept,
            confidence: 0.8,
            rationale: String::new(),
            rounds: vec![RoundLog { n: 2, log: ballots }],
            merge_candidates: vec![],
            merged_children: vec![],
            evidence: vec![],
            override_: None,
            accepted_at: None,
        }
    }

    #[test]
    fn funnel_resolved_counts_verdict_not_round3_logs() {
        let f = finding_with_votes("x", vec![]);
        let funnel = build_funnel(&[f]);
        assert_eq!(funnel.counts.proposed, 1);
        assert_eq!(funnel.counts.resolved, 1);
        assert_eq!(funnel.counts.cross_checked, 0);
    }

    #[test]
    fn matrix_uses_latest_vote_per_reviewer() {
        let f = finding_with_votes(
            "x",
            vec![
                Ballot {
                    reviewer: "a".into(),
                    vote: Vote::Keep,
                    note: String::new(),
                    merge_target: None,
                },
                Ballot {
                    reviewer: "b".into(),
                    vote: Vote::Drop,
                    note: String::new(),
                    merge_target: None,
                },
            ],
        );
        let rows = build_matrix(&[f]);
        assert_eq!(rows[0].latest_vote.get("a"), Some(&Vote::Keep));
        assert_eq!(rows[0].latest_vote.get("b"), Some(&Vote::Drop));
    }
}
