use super::model::{ArenaFinding, Ballot, Verdict, Vote};
use crate::ai::RiskLevel;
use std::collections::BTreeMap;

/// Majority severity from cross-check votes in round `round`; ties break toward higher severity.
pub fn severity_from_cross_check(
    findings: &mut [ArenaFinding],
    ballots: &[(String, super::schema::Round2Output)],
    round: u8,
) {
    for f in findings.iter_mut() {
        let mut votes: Vec<RiskLevel> = Vec::new();
        for (reviewer_id, out) in ballots {
            for b in &out.ballots {
                if b.finding_id != f.id {
                    continue;
                }
                if super::schema::parse_vote(&b.vote).is_ok() {
                    if f.rounds.iter().all(|r| r.n != round) {
                        f.rounds.push(super::model::RoundLog {
                            n: round,
                            log: vec![],
                        });
                    }
                    if let Some(r) = f.rounds.iter_mut().find(|r| r.n == round) {
                        r.log.push(Ballot {
                            reviewer: reviewer_id.clone(),
                            vote: parse_vote_enum(&b.vote),
                            note: b.note.clone(),
                            merge_target: b.merge_target.clone(),
                        });
                    }
                    match b.vote.to_ascii_lowercase().as_str() {
                        "escalate" => votes.push(RiskLevel::High),
                        "lower" => votes.push(RiskLevel::Low),
                        "keep" => votes.push(
                            f.severity_by_round.get(&1).copied().unwrap_or(RiskLevel::Medium),
                        ),
                        "abstain" | "flag" | "merge" | "drop" => {}
                        _ => {}
                    }
                }
            }
        }
        if !votes.is_empty() {
            let sev = majority_severity(&votes);
            f.severity_by_round.insert(round, sev);
        }
    }
}

/// Back-compat alias for round-2 cross-check.
#[allow(dead_code)]
pub fn severity_from_round2(findings: &mut [ArenaFinding], ballots: &[(String, super::schema::Round2Output)]) {
    severity_from_cross_check(findings, ballots, 2);
}

fn parse_vote_enum(s: &str) -> Vote {
    match s.to_ascii_lowercase().as_str() {
        "propose" => Vote::Propose,
        "keep" => Vote::Keep,
        "drop" => Vote::Drop,
        "merge" => Vote::Merge,
        "escalate" => Vote::Escalate,
        "lower" => Vote::Lower,
        "flag" => Vote::Flag,
        _ => Vote::Abstain,
    }
}

fn majority_severity(votes: &[RiskLevel]) -> RiskLevel {
    let mut counts = BTreeMap::new();
    for v in votes {
        *counts.entry(severity_key(*v)).or_insert(0usize) += 1;
    }
    let mut best = RiskLevel::Low;
    let mut best_n = 0usize;
    for (k, n) in counts {
        let sev = key_severity(k);
        if n > best_n || (n == best_n && severity_rank(sev) > severity_rank(best)) {
            best_n = n;
            best = sev;
        }
    }
    best
}

fn severity_key(s: RiskLevel) -> u8 {
    match s {
        RiskLevel::High => 3,
        RiskLevel::Medium => 2,
        RiskLevel::Low => 1,
        RiskLevel::Info => 0,
    }
}

fn key_severity(k: u8) -> RiskLevel {
    match k {
        3 => RiskLevel::High,
        2 => RiskLevel::Medium,
        1 => RiskLevel::Low,
        _ => RiskLevel::Info,
    }
}

fn severity_rank(s: RiskLevel) -> u8 {
    severity_key(s)
}

/// Apply round-3 arbiter verdicts and compute confidence.
pub fn apply_round3_verdicts(
    findings: &mut [ArenaFinding],
    output: &super::schema::Round3Output,
    auto_accept: f32,
) {
    for v in &output.verdicts {
        let Some(f) = findings.iter_mut().find(|x| x.id == v.finding_id) else {
            continue;
        };
        f.verdict = parse_verdict(&v.verdict, v.merged_into.as_deref());
        f.confidence = v.confidence;
        f.rationale = v.rationale.clone();
        if f.confidence >= auto_accept && matches!(f.verdict, Verdict::Pending) {
            f.verdict = Verdict::Kept;
        }
    }
}

/// Record arbiter ballots on each finding for the consensus matrix.
pub fn record_arbiter_ballots(
    findings: &mut [ArenaFinding],
    output: &super::schema::Round3Output,
    arbiter_id: &str,
) {
    let round = super::model::ARENA_ARBITER_ROUND;
    for v in &output.verdicts {
        let Some(f) = findings.iter_mut().find(|x| x.id == v.finding_id) else {
            continue;
        };
        if f.rounds.iter().all(|r| r.n != round) {
            f.rounds.push(super::model::RoundLog { n: round, log: vec![] });
        }
        if let Some(r) = f.rounds.iter_mut().find(|r| r.n == round) {
            r.log.push(Ballot {
                reviewer: arbiter_id.to_string(),
                vote: verdict_to_vote(&v.verdict),
                note: v.rationale.clone(),
                merge_target: v.merged_into.clone(),
            });
        }
    }
}

fn verdict_to_vote(verdict: &str) -> Vote {
    match verdict.to_ascii_lowercase().as_str() {
        "kept" => Vote::Keep,
        "escalated" => Vote::Escalate,
        "dropped" => Vote::Drop,
        "merged" => Vote::Merge,
        _ => Vote::Abstain,
    }
}

fn parse_verdict(s: &str, merged_into: Option<&str>) -> Verdict {
    match s.to_ascii_lowercase().as_str() {
        "kept" => Verdict::Kept,
        "escalated" => Verdict::Escalated,
        "dropped" => Verdict::Dropped,
        "merged" => Verdict::Merged {
            into: merged_into.unwrap_or("").to_string(),
        },
        _ => Verdict::Pending,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn confidence_score(
        agreement_votes: usize,
        total_voters: usize,
        severity_stable: bool,
    ) -> f32 {
        let agree = if total_voters == 0 {
            0.5
        } else {
            agreement_votes as f32 / total_voters as f32
        };
        let stability = if severity_stable { 1.0 } else { 0.85 };
        let bonus = 1.0 + (total_voters as f32 * 0.02).min(0.1);
        (agree * stability * bonus).clamp(0.0, 1.0)
    }

    #[test]
    fn tie_breaks_toward_higher_severity() {
        let votes = vec![RiskLevel::High, RiskLevel::Medium, RiskLevel::Medium];
        assert_eq!(majority_severity(&votes), RiskLevel::Medium);
        let votes2 = vec![RiskLevel::High, RiskLevel::High, RiskLevel::Medium];
        assert_eq!(majority_severity(&votes2), RiskLevel::High);
    }

    #[test]
    fn round3_ballots_recorded_for_matrix() {
        use super::super::model::ArenaFinding;
        use super::super::schema::Round3Output;
        use crate::ai::RiskLevel;
        let mut findings = vec![ArenaFinding {
            id: "fid".into(),
            file: "f.rs".into(),
            line: None,
            title: "t".into(),
            body: "b".into(),
            severity_by_round: BTreeMap::from([(1, RiskLevel::High)]),
            raised_by: vec![],
            verdict: Verdict::Pending,
            confidence: 0.0,
            rationale: String::new(),
            rounds: vec![],
            merge_candidates: vec![],
            merged_children: vec![],
            evidence: vec![],
            override_: None,
            accepted_at: None,
        }];
        let out = Round3Output {
            verdicts: vec![super::super::schema::Round3Verdict {
                finding_id: "fid".into(),
                verdict: "kept".into(),
                confidence: 0.9,
                rationale: "ok".into(),
                merged_into: None,
            }],
        };
        record_arbiter_ballots(&mut findings, &out, "arbiter-1");
        assert_eq!(findings[0].rounds.len(), 1);
        assert_eq!(findings[0].rounds[0].n, crate::arena::ARENA_ARBITER_ROUND);
        assert_eq!(findings[0].rounds[0].log[0].reviewer, "arbiter-1");
    }

    #[test]
    fn confidence_monotonic_with_agreement() {
        let low = confidence_score(1, 5, true);
        let high = confidence_score(4, 5, true);
        assert!(high > low);
    }
}
