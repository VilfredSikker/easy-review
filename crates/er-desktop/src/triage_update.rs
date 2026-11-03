//! What a finished review says about triage's routing call.
//!
//! Triage is a fast first read made before anything was reviewed, so its risk
//! and priority files are guesses. Once a review has graded the same diff, its
//! findings and file risks are the better answer. `triage.json` stays as the
//! agent wrote it — the overlay is derived here, so re-running either side
//! never leaves a stale copy of the other behind.

use crate::snapshot::{FileRiskSnapshot, FlatFinding, TriagePriorityFileSnapshot};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct TriageReviewUpdateSnapshot {
    /// `high` | `med` | `low`: the highest open finding, else the highest file
    /// risk the review assigned.
    pub approx_risk: String,
    /// `fix` with open high findings, `check` with medium ones, else `done`.
    pub next: String,
    pub high: usize,
    pub med: usize,
    pub low: usize,
    /// Lenses whose findings are in the review (general and experts alike),
    /// in first-seen order.
    pub reviewed_by: Vec<String>,
    /// Triage's priority files with the review's grade, then files the review
    /// found something in that triage did not list, highest risk first.
    pub priority_files: Vec<TriagePriorityFileSnapshot>,
    pub created_at: String,
}

fn rank(risk: &str) -> u8 {
    match risk {
        "high" => 0,
        "med" => 1,
        _ => 2,
    }
}

fn higher<'a>(a: &'a str, b: &'a str) -> &'a str {
    if rank(b) < rank(a) {
        b
    } else {
        a
    }
}

/// `None` unless a review graded the diff triage is shown with: an overlay
/// from an older diff would be a less precise verdict, not a more precise one.
pub fn triage_review_update(
    review_fresh: bool,
    review_created_at: &str,
    triage_files: &[TriagePriorityFileSnapshot],
    open: &[FlatFinding],
    resolved: &[FlatFinding],
    dropped: &[FlatFinding],
    file_risks: &[FileRiskSnapshot],
) -> Option<TriageReviewUpdateSnapshot> {
    if !review_fresh
        || (file_risks.is_empty() && open.is_empty() && resolved.is_empty() && dropped.is_empty())
    {
        return None;
    }
    let count = |sev: &str| open.iter().filter(|f| f.severity == sev).count();
    let (high, med, low) = (count("high"), count("med"), count("low"));

    let worst_finding = |path: &str| {
        open.iter()
            .filter(|f| f.file == path)
            .map(|f| f.severity.as_str())
            .min_by_key(|s| rank(s))
    };
    let review_risk = |path: &str| -> Option<&str> {
        let file = file_risks
            .iter()
            .find(|r| r.path == path)
            .map(|r| r.risk.as_str());
        match (file, worst_finding(path)) {
            (Some(a), Some(b)) => Some(higher(a, b)),
            (a, b) => a.or(b),
        }
    };

    let mut priority_files: Vec<TriagePriorityFileSnapshot> = triage_files
        .iter()
        .map(|pf| TriagePriorityFileSnapshot {
            path: pf.path.clone(),
            reason: pf.reason.clone(),
            risk: review_risk(&pf.path).unwrap_or(&pf.risk).to_string(),
        })
        .collect();
    for f in open.iter().filter(|f| f.severity != "low") {
        if priority_files.iter().any(|pf| pf.path == f.file) {
            continue;
        }
        priority_files.push(TriagePriorityFileSnapshot {
            path: f.file.clone(),
            reason: f.title.clone(),
            risk: review_risk(&f.file).unwrap_or(&f.severity).to_string(),
        });
    }
    priority_files.sort_by_key(|pf| rank(&pf.risk));

    let approx_risk = if high > 0 {
        "high"
    } else if med > 0 {
        "med"
    } else {
        file_risks
            .iter()
            .map(|r| r.risk.as_str())
            .min_by_key(|s| rank(s))
            .unwrap_or("low")
    };
    let next = if high > 0 {
        "fix"
    } else if med > 0 {
        "check"
    } else {
        "done"
    };

    let mut reviewed_by: Vec<String> = Vec::new();
    for label in open
        .iter()
        .chain(resolved)
        .chain(dropped)
        .map(|f| &f.agent_label)
    {
        if !label.is_empty() && !reviewed_by.contains(label) {
            reviewed_by.push(label.clone());
        }
    }

    Some(TriageReviewUpdateSnapshot {
        approx_risk: approx_risk.to_string(),
        next: next.to_string(),
        high,
        med,
        low,
        reviewed_by,
        priority_files,
        created_at: review_created_at.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finding(file: &str, severity: &str, agent: &str) -> FlatFinding {
        FlatFinding {
            id: format!("{file}-{severity}"),
            file: file.to_string(),
            line: None,
            hunk_index: None,
            severity: severity.to_string(),
            confidence: String::new(),
            lens_category: String::new(),
            resolved: false,
            expert_label: None,
            agent_label: agent.to_string(),
            raised_by: Vec::new(),
            providers: Vec::new(),
            security_stage: false,
            title: format!("{severity} in {file}"),
            message_markdown: String::new(),
            promoted_to: None,
            thread_id: None,
            responses: Vec::new(),
        }
    }

    fn risk(path: &str, risk: &str) -> FileRiskSnapshot {
        FileRiskSnapshot {
            path: path.to_string(),
            risk: risk.to_string(),
            risk_reason: String::new(),
            summary: String::new(),
        }
    }

    fn pf(path: &str, risk: &str) -> TriagePriorityFileSnapshot {
        TriagePriorityFileSnapshot {
            path: path.to_string(),
            reason: "triage said so".to_string(),
            risk: risk.to_string(),
        }
    }

    #[test]
    fn a_stale_review_leaves_triage_alone() {
        let open = [finding("a.rs", "high", "General")];
        assert!(triage_review_update(false, "", &[], &open, &[], &[], &[]).is_none());
    }

    #[test]
    fn no_review_output_leaves_triage_alone() {
        assert!(
            triage_review_update(true, "", &[pf("a.rs", "high")], &[], &[], &[], &[]).is_none()
        );
    }

    #[test]
    fn findings_regrade_triage_files_and_add_the_ones_it_missed() {
        let triage = [pf("a.rs", "high"), pf("b.rs", "low")];
        let open = [
            finding("b.rs", "high", "General"),
            finding("c.rs", "med", "Security"),
            finding("d.rs", "low", "Security"),
        ];
        let risks = [risk("a.rs", "low"), risk("b.rs", "med")];
        let u = triage_review_update(true, "t", &triage, &open, &[], &[], &risks).unwrap();
        assert_eq!(u.approx_risk, "high");
        assert_eq!(u.next, "fix");
        assert_eq!((u.high, u.med, u.low), (1, 1, 1));
        assert_eq!(u.reviewed_by, ["General", "Security"]);
        let files: Vec<_> = u
            .priority_files
            .iter()
            .map(|p| (p.path.as_str(), p.risk.as_str()))
            .collect();
        // b.rs rises to its finding; a.rs falls to the review's grade; c.rs is
        // new; a low finding alone does not make d.rs a priority.
        assert_eq!(files, [("b.rs", "high"), ("c.rs", "med"), ("a.rs", "low")]);
        assert_eq!(u.priority_files[1].reason, "med in c.rs");
    }

    #[test]
    fn a_clean_review_says_done_at_the_file_risk_it_assigned() {
        let other = [finding("a.rs", "high", "Testing")];
        let u =
            triage_review_update(true, "t", &[], &[], &[], &other, &[risk("a.rs", "med")]).unwrap();
        assert_eq!(u.next, "done");
        assert_eq!(u.approx_risk, "med");
        assert_eq!(u.reviewed_by, ["Testing"]);
    }
}
