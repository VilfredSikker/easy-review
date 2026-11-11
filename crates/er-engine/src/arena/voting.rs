use super::model::{ArenaFinding, Ballot, Verdict, Vote};
use crate::ai::panel_run::Agreement;
use crate::ai::RiskLevel;
use std::collections::{BTreeMap, BTreeSet};

/// The agreement signal over one round's ballots.
///
/// `propose`, `keep`, `escalate` and `lower` all say the claim is real;
/// `drop`, `merge`, `flag` and `abstain` do not back it. A reviewer counts
/// once however many ballots it casts for the same claim.
pub fn quorum<'a>(votes: impl IntoIterator<Item = (&'a str, &'a Ballot)>) -> Agreement {
    let mut backers: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (finding, ballot) in votes {
        if !backs(&ballot.vote) {
            continue;
        }
        backers
            .entry(finding)
            .or_default()
            .insert(ballot.reviewer.as_str());
    }
    Agreement {
        confirmed: backers
            .into_iter()
            .filter(|(_, reviewers)| reviewers.len() >= 2)
            .map(|(finding, _)| finding.to_string())
            .collect(),
    }
}

/// `quorum` over the ballots a finding set already records for `round`.
pub fn quorum_from_findings(findings: &[ArenaFinding], round: u8) -> Agreement {
    let votes = findings
        .iter()
        .flat_map(|finding| {
            finding
                .rounds
                .iter()
                .filter(|log| log.n == round)
                .flat_map(|log| log.log.iter().map(|ballot| (finding.id.as_str(), ballot)))
        })
        .collect::<Vec<_>>();
    quorum(votes)
}

const fn backs(vote: &Vote) -> bool {
    matches!(
        vote,
        Vote::Propose | Vote::Keep | Vote::Escalate | Vote::Lower
    )
}

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
                            f.severity_by_round
                                .get(&1)
                                .copied()
                                .unwrap_or(RiskLevel::Medium),
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

const fn severity_key(s: RiskLevel) -> u8 {
    match s {
        RiskLevel::High => 3,
        RiskLevel::Medium => 2,
        RiskLevel::Low => 1,
        RiskLevel::Info => 0,
    }
}

const fn key_severity(k: u8) -> RiskLevel {
    match k {
        3 => RiskLevel::High,
        2 => RiskLevel::Medium,
        1 => RiskLevel::Low,
        _ => RiskLevel::Info,
    }
}

const fn severity_rank(s: RiskLevel) -> u8 {
    severity_key(s)
}

/// Apply round-3 arbiter verdicts and compute confidence.
pub fn apply_round3_verdicts(
    findings: &mut [ArenaFinding],
    output: &super::schema::Round3Output,
    auto_accept: f32,
) {
    // Raisers a merged-away finding contributes to its survivor. Collected
    // first: the survivor is found by id, and every finding already raised
    // before the merge keeps its own attribution.
    let mut inherited: Vec<(String, Vec<String>)> = Vec::new();

    for v in &output.verdicts {
        let Some(f) = findings.iter_mut().find(|x| x.id == v.finding_id) else {
            continue;
        };
        let verdict = parse_verdict(&v.verdict, v.merged_into.as_deref());
        // A merge folds one claim into another, so the survivor answers for
        // every producer that raised either — CONTEXT.md's "a finding merged
        // from several producers carries all of them". Without this the
        // survivor under-reports, and the merged finding is the one that
        // disappears.
        if let Verdict::Merged { into } = &verdict {
            if !f.raised_by.is_empty() {
                inherited.push((into.clone(), f.raised_by.clone()));
            }
        }
        f.verdict = verdict;
        f.confidence = v.confidence;
        f.rationale = v.rationale.clone();
        if f.confidence >= auto_accept && matches!(f.verdict, Verdict::Pending) {
            f.verdict = Verdict::Kept;
        }
    }

    for (into, raisers) in inherited {
        let Some(survivor) = findings.iter_mut().find(|x| x.id == into) else {
            continue;
        };
        for raiser in raisers {
            if !survivor.raised_by.contains(&raiser) {
                survivor.raised_by.push(raiser);
            }
        }
        survivor.raised_by.sort();
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
            f.rounds.push(super::model::RoundLog {
                n: round,
                log: vec![],
            });
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

    /// A merge folds one claim into another, and CONTEXT.md says the result
    /// carries every producer. The merged finding is the one that disappears, so
    /// without this the survivor under-reports exactly the case merging is for.
    #[test]
    fn a_merged_verdict_moves_its_raisers_onto_the_survivor() {
        use super::super::model::ArenaFinding;
        use super::super::schema::{Round3Output, Round3Verdict};

        let finding = |id: &str, raisers: Vec<&str>| ArenaFinding {
            id: id.into(),
            file: "f.rs".into(),
            line: None,
            title: "t".into(),
            body: "b".into(),
            severity_by_round: BTreeMap::from([(1, RiskLevel::High)]),
            raised_by: raisers.into_iter().map(String::from).collect(),
            verdict: Verdict::Pending,
            confidence: 0.0,
            rationale: String::new(),
            rounds: vec![],
            merge_candidates: vec![],
            merged_children: vec![],
            evidence: vec![],
            override_: None,
            accepted_at: None,
        };
        let mut findings = vec![
            finding("keep", vec!["security"]),
            finding("gone", vec!["reliability", "testing"]),
        ];
        let out = Round3Output {
            verdicts: vec![
                Round3Verdict {
                    finding_id: "keep".into(),
                    verdict: "kept".into(),
                    confidence: 0.9,
                    rationale: "real".into(),
                    merged_into: None,
                },
                Round3Verdict {
                    finding_id: "gone".into(),
                    verdict: "merged".into(),
                    confidence: 0.9,
                    rationale: "same issue".into(),
                    merged_into: Some("keep".into()),
                },
            ],
        };

        apply_round3_verdicts(&mut findings, &out, 0.75);

        assert_eq!(
            findings[0].raised_by,
            vec!["reliability", "security", "testing"],
            "the survivor answers for every producer of either claim"
        );
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

    fn ballot(reviewer: &str, vote: Vote) -> Ballot {
        Ballot {
            reviewer: reviewer.into(),
            vote,
            note: String::new(),
            merge_target: None,
        }
    }

    /// The signal ADR 0024 names: a claim two reviewers reached independently.
    /// One reviewer's opinion, however many ballots it casts, is not a quorum.
    #[test]
    fn a_quorum_needs_two_distinct_backers() {
        let alone = [ballot("a", Vote::Propose)];
        assert_eq!(quorum([("f", &alone[0])]).confirmed, Vec::<String>::new());

        let second = ballot("b", Vote::Keep);
        assert_eq!(
            quorum([("f", &alone[0]), ("f", &second)]).confirmed,
            ["f"],
            "a proposal kept by another reviewer reaches a quorum"
        );

        let twice = ballot("a", Vote::Keep);
        assert_eq!(
            quorum([("f", &alone[0]), ("f", &twice)]).confirmed,
            Vec::<String>::new(),
            "the same reviewer voting twice is still one backer"
        );
    }

    #[test]
    fn only_votes_that_back_the_claim_count() {
        for vote in [Vote::Propose, Vote::Keep, Vote::Escalate, Vote::Lower] {
            let first = ballot("a", vote.clone());
            let second = ballot("b", vote.clone());
            assert_eq!(
                quorum([("f", &first), ("f", &second)]).confirmed,
                ["f"],
                "{vote:?} backs the claim"
            );
        }
        for vote in [Vote::Drop, Vote::Merge, Vote::Flag, Vote::Abstain] {
            let first = ballot("a", vote.clone());
            let second = ballot("b", vote.clone());
            assert!(
                quorum([("f", &first), ("f", &second)]).confirmed.is_empty(),
                "{vote:?} does not back the claim"
            );
        }
    }

    /// Round two is read from the ballots the cross round recorded, not from
    /// the proposals that opened the finding.
    #[test]
    fn quorum_from_findings_reads_one_round_at_a_time() {
        use super::super::model::{ArenaFinding, RoundLog};

        let finding = ArenaFinding {
            id: "f".into(),
            file: "a.rs".into(),
            line: None,
            title: "t".into(),
            body: "b".into(),
            severity_by_round: BTreeMap::from([(1, RiskLevel::High)]),
            raised_by: vec!["a".into()],
            verdict: Verdict::Pending,
            confidence: 0.5,
            rationale: String::new(),
            rounds: vec![
                RoundLog {
                    n: 1,
                    log: vec![ballot("a", Vote::Propose)],
                },
                RoundLog {
                    n: 2,
                    log: vec![ballot("a", Vote::Keep), ballot("b", Vote::Keep)],
                },
            ],
            merge_candidates: vec![],
            merged_children: vec![],
            evidence: vec![],
            override_: None,
            accepted_at: None,
        };

        assert!(quorum_from_findings(std::slice::from_ref(&finding), 1)
            .confirmed
            .is_empty());
        assert_eq!(
            quorum_from_findings(&[finding], 2).confirmed,
            ["f"],
            "the cross round's keep votes are the round-two quorum"
        );
    }

    // NOTE: a previous `confidence_monotonic_with_agreement` test exercised a
    // private re-implementation of a formula that does not exist in production
    // (apply_round3_verdicts copies the arbiter's confidence verbatim), so it
    // could never fail on a real regression. Removed rather than kept as dead weight.
}
