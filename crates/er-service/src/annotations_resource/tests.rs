//! Triage's review regrade on the `annotations(tab)` path, against the cases
//! the flag-off function in er-desktop's `triage_update.rs` is held to.

use er_protocol as wire;

use super::triage_review_update;

fn finding(file: &str, severity: &str, agent: &str) -> wire::Finding {
    wire::Finding {
        id: format!("{file}-{severity}"),
        file: file.to_string(),
        line: None,
        hunk_index: None,
        severity: severity.to_string(),
        confidence: String::new(),
        arbiter_verdict: None,
        arbiter_rationale: None,
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

fn risk(path: &str, risk: &str) -> wire::FileRisk {
    wire::FileRisk {
        path: path.to_string(),
        risk: risk.to_string(),
        risk_reason: String::new(),
        summary: String::new(),
    }
}

fn pf(path: &str, risk: &str) -> wire::TriagePriorityFile {
    wire::TriagePriorityFile {
        path: path.to_string(),
        reason: "triage said so".to_string(),
        risk: risk.to_string(),
    }
}

#[test]
fn a_stale_review_leaves_triage_alone() {
    let open = [finding("a.rs", "high", "General")];
    assert!(triage_review_update(false, "", &[], [&open, &[], &[]], &[]).is_none());
}

#[test]
fn no_review_output_leaves_triage_alone() {
    let triage = [pf("a.rs", "high")];
    assert!(triage_review_update(true, "", &triage, [&[], &[], &[]], &[]).is_none());
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
    let u = triage_review_update(true, "t", &triage, [&open, &[], &[]], &risks).unwrap();
    assert_eq!((u.approx_risk.as_str(), u.next.as_str()), ("high", "fix"));
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
    let risks = [risk("a.rs", "med")];
    let u = triage_review_update(true, "t", &[], [&[], &[], &other], &risks).unwrap();
    assert_eq!((u.approx_risk.as_str(), u.next.as_str()), ("med", "done"));
    assert_eq!(u.reviewed_by, ["Testing"]);
}
