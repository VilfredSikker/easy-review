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

fn lines_close(a: Option<usize>, b: Option<usize>) -> bool {
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

fn severity_rank(s: RiskLevel) -> u8 {
    match s {
        RiskLevel::High => 3,
        RiskLevel::Medium => 2,
        RiskLevel::Low => 1,
        RiskLevel::Info => 0,
    }
}

#[cfg(test)]
mod tests {
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
}
