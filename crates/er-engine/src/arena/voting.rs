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

/// Back-compat alias for round-2 cross-check.
#[allow(dead_code)]
pub fn severity_from_round2(
    findings: &mut [ArenaFinding],
    ballots: &[(String, super::schema::Round2Output)],
) {
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
    fn parse_vote_enum_maps_every_schema_vote_token() {
        assert_eq!(parse_vote_enum("propose"), Vote::Propose);
        assert_eq!(parse_vote_enum("keep"), Vote::Keep);
        assert_eq!(parse_vote_enum("drop"), Vote::Drop);
        assert_eq!(parse_vote_enum("merge"), Vote::Merge);
        assert_eq!(parse_vote_enum("escalate"), Vote::Escalate);
        assert_eq!(parse_vote_enum("lower"), Vote::Lower);
        assert_eq!(parse_vote_enum("flag"), Vote::Flag);
        // "abstain" is the one schema token with no arm of its own — it rides
        // the catch-all. This pins the schema↔enum contract, not a branch.
        assert_eq!(parse_vote_enum("abstain"), Vote::Abstain);
    }

    #[test]
    fn parse_vote_enum_is_case_insensitive() {
        // Providers are not required to emit lowercase in their JSON ballots.
        assert_eq!(parse_vote_enum("ESCALATE"), Vote::Escalate);
        assert_eq!(parse_vote_enum("Merge"), Vote::Merge);
        assert_eq!(parse_vote_enum("DrOp"), Vote::Drop);
    }

    #[test]
    fn parse_vote_enum_does_not_trim_surrounding_whitespace() {
        // Characterization: only case is normalized, so a padded token falls
        // through to the catch-all instead of matching "keep".
        assert_eq!(parse_vote_enum(" keep "), Vote::Abstain);
        assert_eq!(parse_vote_enum("keep\n"), Vote::Abstain);
    }

    #[test]
    fn parse_vote_enum_falls_back_to_abstain_for_unknown_tokens() {
        assert_eq!(parse_vote_enum("veto"), Vote::Abstain);
        assert_eq!(parse_vote_enum(""), Vote::Abstain);
    }

    #[test]
    fn parse_verdict_maps_known_verdicts_and_ignores_merge_target() {
        assert_eq!(parse_verdict("kept", None), Verdict::Kept);
        assert_eq!(parse_verdict("escalated", None), Verdict::Escalated);
        assert_eq!(parse_verdict("dropped", None), Verdict::Dropped);
        assert_eq!(parse_verdict("pending", None), Verdict::Pending);
        // merged_into is only consumed by the "merged" arm.
        assert_eq!(parse_verdict("kept", Some("other-id")), Verdict::Kept);
        assert_eq!(parse_verdict("DROPPED", Some("other-id")), Verdict::Dropped);
    }

    #[test]
    fn parse_verdict_merged_carries_target_and_empties_it_when_absent() {
        assert_eq!(
            parse_verdict("merged", Some("dup-42")),
            Verdict::Merged {
                into: "dup-42".to_string()
            }
        );
        // An arbiter that says "merged" without naming a parent yields an empty
        // target rather than dropping the verdict — the finding stays merged.
        assert_eq!(
            parse_verdict("Merged", None),
            Verdict::Merged {
                into: String::new()
            }
        );
    }

    #[test]
    fn parse_verdict_falls_back_to_pending_for_unknown_verdicts() {
        // Pending is the safe fallback: apply_round3_verdicts only auto-accepts
        // a Pending verdict when confidence clears the threshold.
        assert_eq!(parse_verdict("acquitted", None), Verdict::Pending);
        assert_eq!(parse_verdict("", None), Verdict::Pending);
        assert_eq!(parse_verdict("merged ", None), Verdict::Pending);
    }

    fn pending_finding() -> ArenaFinding {
        ArenaFinding {
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
        }
    }

    fn arbiter_says(verdict: &str, confidence: f32) -> super::super::schema::Round3Output {
        super::super::schema::Round3Output {
            verdicts: vec![super::super::schema::Round3Verdict {
                finding_id: "fid".into(),
                verdict: verdict.to_string(),
                confidence,
                rationale: "why".into(),
                merged_into: None,
            }],
        }
    }

    #[test]
    fn auto_accept_promotes_only_pending_verdicts_above_threshold() {
        // Ties parse_verdict's Pending fallback to its one production consumer.
        // An unparseable verdict lands as Pending, so a confident arbiter still
        // gets the finding kept rather than silently losing it.
        let mut findings = vec![pending_finding()];
        apply_round3_verdicts(&mut findings, &arbiter_says("not-a-verdict", 0.95), 0.8);
        assert_eq!(findings[0].verdict, Verdict::Kept);
        assert_eq!(findings[0].rationale, "why");
        assert_eq!(findings[0].confidence, 0.95);

        // The promotion is gated on Pending: a verdict the arbiter actually
        // named is never overwritten, however confident it was. Without this,
        // deleting the `matches!(.., Verdict::Pending)` guard stays green.
        let mut dropped = vec![pending_finding()];
        apply_round3_verdicts(&mut dropped, &arbiter_says("dropped", 0.95), 0.8);
        assert_eq!(dropped[0].verdict, Verdict::Dropped);

        // ...and it is gated on the threshold: below auto_accept an
        // unparseable verdict stays Pending for a human to adjudicate.
        let mut unsure = vec![pending_finding()];
        apply_round3_verdicts(&mut unsure, &arbiter_says("not-a-verdict", 0.5), 0.8);
        assert_eq!(unsure[0].verdict, Verdict::Pending);
    }

    // NOTE: a previous `confidence_monotonic_with_agreement` test exercised a
    // private re-implementation of a formula that does not exist in production
    // (apply_round3_verdicts copies the arbiter's confidence verbatim), so it
    // could never fail on a real regression. Removed rather than kept as dead weight.

    // ---- severity_from_cross_check -------------------------------------------------

    use super::super::schema::{Round2Ballot, Round2Output};

    fn cross_check_finding(id: &str, round1: Option<RiskLevel>) -> ArenaFinding {
        let mut severity_by_round = BTreeMap::new();
        if let Some(sev) = round1 {
            severity_by_round.insert(1, sev);
        }
        ArenaFinding {
            id: id.into(),
            file: "f.rs".into(),
            line: None,
            title: "t".into(),
            body: "b".into(),
            severity_by_round,
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
        }
    }

    /// One reviewer's cross-check ballots, in the `(reviewer_id, output)` shape
    /// `run_round2_parallel` hands to `severity_from_cross_check`.
    fn cast(reviewer: &str, votes: &[(&str, &str)]) -> (String, Round2Output) {
        (
            reviewer.to_string(),
            Round2Output {
                ballots: votes
                    .iter()
                    .map(|(finding_id, vote)| Round2Ballot {
                        finding_id: (*finding_id).to_string(),
                        vote: (*vote).to_string(),
                        note: format!("{reviewer} voted {vote}"),
                        merge_target: (*vote == "merge").then(|| "other-finding".to_string()),
                    })
                    .collect(),
            },
        )
    }

    /// Every ballot recorded against round `n` on a finding.
    fn log_for(f: &ArenaFinding, n: u8) -> Vec<Ballot> {
        f.rounds
            .iter()
            .filter(|r| r.n == n)
            .flat_map(|r| r.log.clone())
            .collect()
    }

    fn round_logs(f: &ArenaFinding) -> Vec<u8> {
        f.rounds.iter().map(|r| r.n).collect()
    }

    #[test]
    fn escalate_votes_raise_the_round_severity_and_leave_round_one_alone() {
        let mut findings = vec![cross_check_finding("f1", Some(RiskLevel::Low))];
        let ballots = vec![
            cast("rev-a", &[("f1", "escalate")]),
            cast("rev-b", &[("f1", "escalate")]),
        ];

        severity_from_cross_check(&mut findings, &ballots, 2);

        assert_eq!(
            findings[0].severity_by_round.get(&2),
            Some(&RiskLevel::High)
        );
        // Round 1 is the proposal severity and must survive the cross check —
        // the projections read both to show the escalation as a delta.
        assert_eq!(findings[0].severity_by_round.get(&1), Some(&RiskLevel::Low));

        // Both reviewers land in the *same* RoundLog: the `rounds.iter().all(..)`
        // guard must create the log once, not once per ballot.
        assert_eq!(round_logs(&findings[0]), vec![2]);
        let log = log_for(&findings[0], 2);
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].reviewer, "rev-a");
        assert_eq!(log[0].vote, Vote::Escalate);
        assert_eq!(log[0].note, "rev-a voted escalate");
        assert_eq!(log[1].reviewer, "rev-b");
    }

    #[test]
    fn lower_votes_drop_the_round_severity() {
        let mut findings = vec![cross_check_finding("f1", Some(RiskLevel::High))];
        let ballots = vec![
            cast("rev-a", &[("f1", "lower")]),
            cast("rev-b", &[("f1", "lower")]),
        ];

        severity_from_cross_check(&mut findings, &ballots, 2);

        assert_eq!(findings[0].severity_by_round.get(&2), Some(&RiskLevel::Low));
        assert_eq!(
            findings[0].severity_by_round.get(&1),
            Some(&RiskLevel::High),
            "de-escalation must not rewrite the round-1 proposal"
        );
        assert_eq!(log_for(&findings[0], 2)[0].vote, Vote::Lower);
    }

    #[test]
    fn keep_votes_carry_the_round_one_severity_forward() {
        let mut findings = vec![cross_check_finding("f1", Some(RiskLevel::High))];
        let ballots = vec![
            cast("rev-a", &[("f1", "keep")]),
            cast("rev-b", &[("f1", "keep")]),
        ];

        severity_from_cross_check(&mut findings, &ballots, 2);

        // "keep" is not a severity of its own — it re-casts the round-1 severity,
        // so an agreed-on High stays High rather than collapsing to the Low that
        // `majority_severity` starts from.
        assert_eq!(
            findings[0].severity_by_round.get(&2),
            Some(&RiskLevel::High)
        );
        assert_eq!(log_for(&findings[0], 2)[0].vote, Vote::Keep);
    }

    #[test]
    fn keep_votes_fall_back_to_medium_when_round_one_has_no_severity() {
        // A finding imported without a round-1 severity (or one whose proposal
        // severity was unparseable) still has to resolve to something.
        let mut findings = vec![cross_check_finding("f1", None)];
        let ballots = vec![cast("rev-a", &[("f1", "keep")])];

        severity_from_cross_check(&mut findings, &ballots, 2);

        assert_eq!(
            findings[0].severity_by_round.get(&2),
            Some(&RiskLevel::Medium)
        );
        assert!(
            findings[0].severity_by_round.get(&1).is_none(),
            "the fallback must not backfill a round-1 severity that was never proposed"
        );
    }

    #[test]
    fn non_severity_votes_are_logged_but_leave_the_round_severity_unset() {
        let mut findings = vec![cross_check_finding("f1", Some(RiskLevel::High))];
        let ballots = vec![
            cast("rev-a", &[("f1", "abstain")]),
            cast("rev-b", &[("f1", "merge")]),
            cast("rev-c", &[("f1", "drop")]),
            cast("rev-d", &[("f1", "flag")]),
        ];

        severity_from_cross_check(&mut findings, &ballots, 2);

        // abstain/merge/drop/flag say nothing about severity, so the round gets
        // no entry at all — the UI then falls back to the round-1 severity
        // rather than showing a fabricated Low.
        assert!(
            findings[0].severity_by_round.get(&2).is_none(),
            "no severity vote was cast, so round 2 must stay absent"
        );
        // They are still ballots, and the consensus matrix renders them.
        let log = log_for(&findings[0], 2);
        assert_eq!(
            log.iter().map(|b| b.vote.clone()).collect::<Vec<_>>(),
            vec![Vote::Abstain, Vote::Merge, Vote::Drop, Vote::Flag]
        );
        assert_eq!(log[1].merge_target.as_deref(), Some("other-finding"));
    }

    #[test]
    fn unknown_votes_are_dropped_before_they_reach_the_round_log() {
        let mut findings = vec![cross_check_finding("f1", Some(RiskLevel::High))];
        let ballots = vec![cast("rev-a", &[("f1", "veto")])];

        severity_from_cross_check(&mut findings, &ballots, 2);

        // The `parse_vote(..).is_ok()` gate is what stops a garbage vote from
        // being recorded; without it `parse_vote_enum` would silently store it
        // as an Abstain ballot from a reviewer who never abstained.
        assert!(
            findings[0].rounds.is_empty(),
            "an unparseable vote must not open a round log"
        );
        assert!(findings[0].severity_by_round.get(&2).is_none());
    }

    #[test]
    fn ballots_are_matched_to_their_own_finding_only() {
        let mut findings = vec![
            cross_check_finding("f1", Some(RiskLevel::Low)),
            cross_check_finding("f2", Some(RiskLevel::Low)),
        ];
        let ballots = vec![cast("rev-a", &[("f2", "escalate")])];

        severity_from_cross_check(&mut findings, &ballots, 2);

        assert!(
            findings[0].rounds.is_empty(),
            "f1 was never voted on and must collect no ballots"
        );
        assert!(findings[0].severity_by_round.get(&2).is_none());
        assert_eq!(
            findings[1].severity_by_round.get(&2),
            Some(&RiskLevel::High)
        );
        assert_eq!(log_for(&findings[1], 2).len(), 1);
    }

    #[test]
    fn a_majority_severity_beats_a_lone_dissenter() {
        let mut escalating = vec![cross_check_finding("f1", Some(RiskLevel::Medium))];
        severity_from_cross_check(
            &mut escalating,
            &[
                cast("rev-a", &[("f1", "escalate")]),
                cast("rev-b", &[("f1", "escalate")]),
                cast("rev-c", &[("f1", "lower")]),
            ],
            2,
        );
        assert_eq!(
            escalating[0].severity_by_round.get(&2),
            Some(&RiskLevel::High)
        );

        // Same shape, opposite majority — so the result tracks the vote count
        // rather than the tie-break's standing preference for higher severity.
        let mut lowering = vec![cross_check_finding("f1", Some(RiskLevel::Medium))];
        severity_from_cross_check(
            &mut lowering,
            &[
                cast("rev-a", &[("f1", "lower")]),
                cast("rev-b", &[("f1", "lower")]),
                cast("rev-c", &[("f1", "escalate")]),
            ],
            2,
        );
        assert_eq!(lowering[0].severity_by_round.get(&2), Some(&RiskLevel::Low));
    }

    #[test]
    fn each_round_gets_its_own_log_and_severity_entry() {
        let mut findings = vec![cross_check_finding("f1", Some(RiskLevel::Low))];

        // Rounds 4 and 5 rather than 2 and 3: the round is a parameter, not a
        // constant, and cross-check rounds run for `2..=total_rounds`.
        severity_from_cross_check(&mut findings, &[cast("rev-a", &[("f1", "escalate")])], 4);
        severity_from_cross_check(&mut findings, &[cast("rev-b", &[("f1", "lower")])], 5);

        assert_eq!(round_logs(&findings[0]), vec![4, 5]);
        assert_eq!(
            findings[0].severity_by_round.get(&4),
            Some(&RiskLevel::High)
        );
        assert_eq!(findings[0].severity_by_round.get(&5), Some(&RiskLevel::Low));

        // A second pass over a round that already has a log appends to it
        // instead of opening a duplicate RoundLog with the same `n`.
        severity_from_cross_check(&mut findings, &[cast("rev-c", &[("f1", "escalate")])], 4);
        assert_eq!(round_logs(&findings[0]), vec![4, 5]);
        assert_eq!(
            log_for(&findings[0], 4)
                .iter()
                .map(|b| b.reviewer.clone())
                .collect::<Vec<_>>(),
            vec!["rev-a".to_string(), "rev-c".to_string()]
        );
    }

    #[test]
    fn severity_from_round2_alias_targets_round_two() {
        let mut findings = vec![cross_check_finding("f1", Some(RiskLevel::Low))];
        severity_from_round2(&mut findings, &[cast("rev-a", &[("f1", "escalate")])]);
        assert_eq!(round_logs(&findings[0]), vec![2]);
        assert_eq!(
            findings[0].severity_by_round.get(&2),
            Some(&RiskLevel::High)
        );
    }
}
