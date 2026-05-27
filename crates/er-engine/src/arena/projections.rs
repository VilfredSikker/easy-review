use super::model::{
    ArenaFinding, ArenaRun, FunnelCounts, FunnelStage, FunnelStages, MatrixRow, Verdict, Vote,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArenaRunSnapshot {
    pub run: ArenaRun,
    pub matrix: Vec<MatrixRow>,
    pub funnel: FunnelStages,
}

#[allow(dead_code)]
pub fn build_snapshot(run: ArenaRun) -> ArenaRunSnapshot {
    let matrix = build_matrix(&run.findings);
    let funnel = build_funnel(&run.findings);
    ArenaRunSnapshot { run, matrix, funnel }
}

/// Collapse round logs → latest vote per reviewer (matrix layout).
pub fn build_matrix(findings: &[ArenaFinding]) -> Vec<MatrixRow> {
    findings
        .iter()
        .map(|f| {
            let mut latest_vote = BTreeMap::new();
            for round in &f.rounds {
                for ballot in &round.log {
                    latest_vote.insert(ballot.reviewer.clone(), ballot.vote.clone());
                }
            }
            MatrixRow {
                finding_id: f.id.clone(),
                latest_vote,
                verdict: f.verdict.clone(),
                confidence: f.confidence,
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
        let has_round3 = f.rounds.iter().any(|r| r.n >= 3 && !r.log.is_empty());
        if has_round2 {
            cross_checked += 1;
        }
        if has_round3 || !matches!(f.verdict, Verdict::Pending) {
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
    use crate::arena::model::{Ballot, RoundLog};

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
        }
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
