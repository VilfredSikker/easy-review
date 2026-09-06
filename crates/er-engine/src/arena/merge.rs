use super::model::ArenaFinding;
use crate::ai::RiskLevel;
use std::collections::{BTreeMap, BTreeSet};

const LINE_PROXIMITY: isize = 5;
const TITLE_JACCARD_THRESHOLD: f64 = 0.78;

/// Heuristic merge-candidate proposer (round 1 only — does not merge).
pub fn propose_merge_candidates(findings: &mut [ArenaFinding]) {
    let n = findings.len();
    for i in 0..n {
        for j in (i + 1)..n {
            if should_propose_merge(&findings[i], &findings[j]) {
                if !findings[i].merge_candidates.contains(&findings[j].id) {
                    findings[i].merge_candidates.push(findings[j].id.clone());
                }
                if !findings[j].merge_candidates.contains(&findings[i].id) {
                    findings[j].merge_candidates.push(findings[i].id.clone());
                }
            }
        }
    }
}

fn should_propose_merge(a: &ArenaFinding, b: &ArenaFinding) -> bool {
    if a.file != b.file {
        return false;
    }
    if !lines_close(a.line, b.line) {
        return false;
    }
    title_jaccard(&a.title, &b.title) >= TITLE_JACCARD_THRESHOLD
}

const fn lines_close(a: Option<usize>, b: Option<usize>) -> bool {
    match (a, b) {
        (Some(la), Some(lb)) => {
            let d = la.abs_diff(lb);
            d <= LINE_PROXIMITY as usize
        }
        (None, None) => true,
        _ => false,
    }
}

fn title_trigrams(s: &str) -> BTreeSet<String> {
    let lower: String = s.to_lowercase();
    let chars: Vec<char> = lower.chars().collect();
    if chars.len() < 3 {
        return BTreeSet::from([lower]);
    }
    chars
        .windows(3)
        .map(|w| w.iter().collect::<String>())
        .collect()
}

fn title_jaccard(a: &str, b: &str) -> f64 {
    let ta = title_trigrams(a);
    let tb = title_trigrams(b);
    if ta.is_empty() && tb.is_empty() {
        return 1.0;
    }
    let inter = ta.intersection(&tb).count() as f64;
    let union = ta.union(&tb).count() as f64;
    if union == 0.0 {
        1.0
    } else {
        inter / union
    }
}

/// Build findings from round-1 reviewer outputs (before merge proposal).
pub fn findings_from_round1(
    proposals: &[(String, super::schema::Round1Output)],
) -> Vec<ArenaFinding> {
    let mut out: Vec<ArenaFinding> = Vec::new();
    for (reviewer_id, round) in proposals {
        for f in &round.findings {
            let id = super::identity::finding_id(&f.file, "", &f.title);
            let sev = super::schema::parse_severity(&f.severity).unwrap_or(RiskLevel::Medium);
            if let Some(existing) = out.iter_mut().find(|x| x.id == id) {
                if !existing.raised_by.contains(reviewer_id) {
                    existing.raised_by.push(reviewer_id.clone());
                }
                let cur = existing.severity_by_round.get(&1).copied();
                if cur
                    .map(|c| severity_rank(c) < severity_rank(sev))
                    .unwrap_or(true)
                {
                    existing.severity_by_round.insert(1, sev);
                }
            } else {
                let mut severity_by_round = BTreeMap::new();
                severity_by_round.insert(1, sev);
                out.push(ArenaFinding {
                    id,
                    file: f.file.clone(),
                    line: f.line,
                    title: f.title.clone(),
                    body: f.body.clone(),
                    severity_by_round,
                    raised_by: vec![reviewer_id.clone()],
                    verdict: super::model::Verdict::Pending,
                    confidence: f.confidence.unwrap_or(0.5),
                    rationale: String::new(),
                    rounds: vec![super::model::RoundLog {
                        n: 1,
                        log: vec![super::model::Ballot {
                            reviewer: reviewer_id.clone(),
                            vote: super::model::Vote::Propose,
                            note: f.body.clone(),
                            merge_target: None,
                        }],
                    }],
                    merge_candidates: vec![],
                    merged_children: vec![],
                    evidence: vec![],
                    override_: None,
                    accepted_at: None,
                });
            }
        }
    }
    propose_merge_candidates(&mut out);
    out
}

const fn severity_rank(s: RiskLevel) -> u8 {
    match s {
        RiskLevel::High => 3,
        RiskLevel::Medium => 2,
        RiskLevel::Low => 1,
        RiskLevel::Info => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::super::model::{Verdict, Vote};
    use super::super::schema::{Round1Finding, Round1Output};
    use super::*;

    #[test]
    fn proposes_candidates_for_similar_titles_same_file() {
        let mut findings = vec![
            ArenaFinding {
                id: "a".into(),
                file: "x.rs".into(),
                line: Some(10),
                title: "User input in map key".into(),
                body: "b1".into(),
                severity_by_round: BTreeMap::new(),
                raised_by: vec![],
                verdict: super::super::model::Verdict::Pending,
                confidence: 0.5,
                rationale: String::new(),
                rounds: vec![],
                merge_candidates: vec![],
                merged_children: vec![],
                evidence: vec![],
                override_: None,
                accepted_at: None,
            },
            ArenaFinding {
                id: "b".into(),
                file: "x.rs".into(),
                line: Some(12),
                title: "User input in map key".into(),
                body: "b2".into(),
                severity_by_round: BTreeMap::new(),
                raised_by: vec![],
                verdict: super::super::model::Verdict::Pending,
                confidence: 0.5,
                rationale: String::new(),
                rounds: vec![],
                merge_candidates: vec![],
                merged_children: vec![],
                evidence: vec![],
                override_: None,
                accepted_at: None,
            },
        ];
        propose_merge_candidates(&mut findings);
        assert!(findings[0].merge_candidates.contains(&"b".to_string()));
    }

    fn proposal(reviewer: &str, findings: Vec<Round1Finding>) -> (String, Round1Output) {
        (reviewer.to_string(), Round1Output { findings })
    }

    fn r1(
        file: &str,
        line: Option<usize>,
        title: &str,
        severity: &str,
        confidence: Option<f32>,
    ) -> Round1Finding {
        Round1Finding {
            file: file.to_string(),
            line,
            title: title.to_string(),
            body: format!("body of {title}"),
            severity: severity.to_string(),
            confidence,
            tags: vec![],
        }
    }

    #[test]
    fn round1_finding_carries_a_propose_ballot_from_its_reviewer() {
        let out = findings_from_round1(&[proposal(
            "rev-a",
            vec![r1("x.rs", Some(4), "Unbounded loop", "high", Some(0.9))],
        )]);

        assert_eq!(out.len(), 1);
        let f = &out[0];
        assert_eq!(f.file, "x.rs");
        assert_eq!(f.line, Some(4));
        assert_eq!(f.raised_by, vec!["rev-a".to_string()]);
        assert_eq!(f.severity_by_round.get(&1), Some(&RiskLevel::High));
        assert_eq!(f.confidence, 0.9);
        assert_eq!(f.verdict, Verdict::Pending);
        assert_eq!(f.rounds.len(), 1);
        assert_eq!(f.rounds[0].n, 1);
        assert_eq!(f.rounds[0].log.len(), 1);
        assert_eq!(f.rounds[0].log[0].reviewer, "rev-a");
        assert_eq!(f.rounds[0].log[0].vote, Vote::Propose);
        assert_eq!(f.rounds[0].log[0].note, "body of Unbounded loop");
    }

    #[test]
    fn identical_findings_from_two_reviewers_collapse_into_one_with_both_credited() {
        let out = findings_from_round1(&[
            proposal(
                "rev-a",
                vec![r1("x.rs", Some(4), "Unbounded   loop", "low", None)],
            ),
            proposal(
                "rev-b",
                vec![r1("x.rs", Some(9), "unbounded loop", "high", Some(0.95))],
            ),
        ]);

        // The id is sha1(file + canonicalized title), so case and repeated
        // whitespace do not split one issue into two findings.
        assert_eq!(out.len(), 1);
        assert_eq!(
            out[0].raised_by,
            vec!["rev-a".to_string(), "rev-b".to_string()]
        );
        // The first proposal owns the mutable fields; the duplicate only adds
        // attribution and (below) severity. rev-b differs on all three —
        // line 9, lowercase title, confidence 0.95 — and none of them land.
        assert_eq!(out[0].line, Some(4));
        assert_eq!(out[0].title, "Unbounded   loop");
        assert_eq!(out[0].confidence, 0.5);
    }

    #[test]
    fn duplicate_findings_take_the_highest_proposed_severity() {
        let escalating = findings_from_round1(&[
            proposal(
                "rev-a",
                vec![r1("x.rs", Some(4), "Race on cache", "low", None)],
            ),
            proposal(
                "rev-b",
                vec![r1("x.rs", Some(4), "Race on cache", "high", None)],
            ),
        ]);
        assert_eq!(
            escalating[0].severity_by_round.get(&1),
            Some(&RiskLevel::High)
        );

        // ...and a later, milder opinion must not walk the severity back down.
        let de_escalating = findings_from_round1(&[
            proposal(
                "rev-a",
                vec![r1("x.rs", Some(4), "Race on cache", "high", None)],
            ),
            proposal(
                "rev-b",
                vec![r1("x.rs", Some(4), "Race on cache", "low", None)],
            ),
        ]);
        assert_eq!(
            de_escalating[0].severity_by_round.get(&1),
            Some(&RiskLevel::High)
        );
    }

    #[test]
    fn a_reviewer_repeating_itself_is_credited_only_once() {
        let out = findings_from_round1(&[proposal(
            "rev-a",
            vec![
                r1("x.rs", Some(4), "Unbounded   loop", "low", None),
                r1("x.rs", Some(9), "unbounded loop", "high", None),
            ],
        )]);

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].raised_by, vec!["rev-a".to_string()]);
        assert_eq!(out[0].severity_by_round.get(&1), Some(&RiskLevel::High));
    }

    #[test]
    fn unparseable_severity_and_absent_confidence_get_defaults() {
        let out = findings_from_round1(&[proposal(
            "rev-a",
            vec![r1("x.rs", None, "Nit: naming", "catastrophic", None)],
        )]);

        assert_eq!(out[0].severity_by_round.get(&1), Some(&RiskLevel::Medium));
        assert_eq!(out[0].confidence, 0.5);
    }

    #[test]
    fn near_duplicate_titles_stay_separate_but_are_cross_linked_as_merge_candidates() {
        let out = findings_from_round1(&[
            proposal(
                "rev-a",
                vec![r1("x.rs", Some(10), "User input in map key", "high", None)],
            ),
            proposal(
                "rev-b",
                vec![r1("x.rs", Some(12), "User input in map keys", "high", None)],
            ),
        ]);

        // Different titles → different ids, so no automatic collapse...
        assert_eq!(out.len(), 2);
        assert_ne!(out[0].id, out[1].id);
        // ...but findings_from_round1 runs the merge proposer before returning,
        // so the arbiter sees the pair as merge candidates in both directions.
        assert!(out[0].merge_candidates.contains(&out[1].id));
        assert!(out[1].merge_candidates.contains(&out[0].id));
    }

    #[test]
    fn distant_findings_are_not_proposed_for_merge() {
        // Same near-identical title as the cross-linking test above, so the
        // Jaccard gate is satisfied in both halves and only the guard under
        // test can be what rejects the pair.

        // Different file → rejected by the `a.file != b.file` guard.
        let other_file = findings_from_round1(&[
            proposal(
                "rev-a",
                vec![r1("x.rs", Some(10), "User input in map key", "high", None)],
            ),
            proposal(
                "rev-b",
                vec![r1("y.rs", Some(10), "User input in map keys", "high", None)],
            ),
        ]);
        assert_eq!(other_file.len(), 2);
        assert!(other_file[0].merge_candidates.is_empty());
        assert!(other_file[1].merge_candidates.is_empty());

        // Same file, but further apart than LINE_PROXIMITY → rejected by the
        // `lines_close` guard. Without this half, deleting the proximity check
        // outright leaves every test in this file green.
        let far_apart = findings_from_round1(&[
            proposal(
                "rev-a",
                vec![r1("x.rs", Some(10), "User input in map key", "high", None)],
            ),
            proposal(
                "rev-b",
                vec![r1("x.rs", Some(200), "User input in map keys", "high", None)],
            ),
        ]);
        assert_eq!(far_apart.len(), 2);
        assert!(far_apart[0].merge_candidates.is_empty());
        assert!(far_apart[1].merge_candidates.is_empty());

        // A finding with no line never merges with one that has a line — the
        // `(Some, None)` arm of lines_close, distinct from the distance check.
        let one_unlocated = findings_from_round1(&[
            proposal(
                "rev-a",
                vec![r1("x.rs", Some(10), "User input in map key", "high", None)],
            ),
            proposal(
                "rev-b",
                vec![r1("x.rs", None, "User input in map keys", "high", None)],
            ),
        ]);
        assert_eq!(one_unlocated.len(), 2);
        assert!(one_unlocated[0].merge_candidates.is_empty());
        assert!(one_unlocated[1].merge_candidates.is_empty());
    }

    #[test]
    fn no_proposals_yields_no_findings() {
        // Narrow claim: this is a degenerate-input guard, not a behavioral
        // pin — empty-in/empty-out survives every mutation of the dedup body.
        // What it does catch is an off-by-one in propose_merge_candidates'
        // bounds: `for i in 0..n - 1` underflows and panics when n == 0.
        assert!(findings_from_round1(&[]).is_empty());
        assert!(findings_from_round1(&[proposal("rev-a", vec![])]).is_empty());
    }
}
