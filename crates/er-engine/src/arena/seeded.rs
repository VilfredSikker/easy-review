//! Collapse expert findings that describe the same issue.
//!
//! The first step of the arbiter path, and the only one that needs no model
//! call: the expert sidecars are already on disk, and the arena's existing
//! duplicate proposer does the grouping. What the arbiter adds on top — a
//! regraded confidence and a verdict per finding — is a later slice.

use super::identity::finding_id;
use super::merge::{propose_merge_candidates, severity_rank};
use super::model::{ArenaFinding, Ballot, RoundLog, Verdict, Vote};
use crate::ai::{expert_by_id, expert_hash_accepted, load_expert_reviews, Confidence, Finding};
use std::collections::{BTreeMap, HashMap};

/// Collapse the expert findings under `er_dir` into one finding per issue.
///
/// Findings sharing a content-addressed id are the same claim and merge
/// outright. Findings that merely look alike — same file, lines within 5, title
/// Jaccard at or above 0.78 — are grouped by `propose_merge_candidates` and
/// collapse to one survivor carrying every raiser.
///
/// `review_hash` is the loaded review's `diff_hash`, if there is one: an expert
/// sidecar from the same generation as a stale review is still accepted on that
/// hash, matching `merge_experts_into_review`. Pass `""` when no review is
/// loaded, and only sidecars matching `current_diff_hash` are read.
///
/// Writes nothing and calls no model. Findings from an unregistered expert are
/// skipped, so every raiser recorded here is a real lens id.
pub fn dedupe_expert_findings(
    er_dir: &str,
    current_diff_hash: &str,
    review_hash: &str,
) -> Vec<ArenaFinding> {
    let mut out: Vec<ArenaFinding> = Vec::new();
    for expert in load_expert_reviews(er_dir) {
        if !expert_hash_accepted(&expert, review_hash, current_diff_hash) {
            continue;
        }
        let Some(def) = expert_by_id(&expert.expert_id) else {
            continue;
        };
        for (path, file_review) in &expert.files {
            for finding in &file_review.findings {
                let id = finding_id(path, "", &finding.title);
                match out.iter_mut().find(|f| f.id == id) {
                    // The same claim from a second expert: keep one row, record
                    // both raisers, take the worse severity.
                    Some(existing) => {
                        if !existing.raised_by.iter().any(|r| r == def.id) {
                            existing.raised_by.push(def.id.to_string());
                        }
                        let worse = existing.severity_by_round.get(&1).is_none_or(|current| {
                            severity_rank(*current) < severity_rank(finding.severity)
                        });
                        if worse {
                            existing.severity_by_round.insert(1, finding.severity);
                        }
                    }
                    None => out.push(from_expert_finding(path, finding, def.id, id)),
                }
            }
        }
    }

    propose_merge_candidates(&mut out);
    let mut collapsed = collapse_groups(out);
    // `load_expert_reviews` reads the directory in filesystem order, so without
    // this the same sidecars produce a different raiser order on another
    // machine — and a different `raised_by`, which is content the UI shows.
    for finding in &mut collapsed {
        finding.raised_by.sort();
    }
    collapsed
}

fn from_expert_finding(path: &str, finding: &Finding, lens: &str, id: String) -> ArenaFinding {
    let mut severity_by_round = BTreeMap::new();
    severity_by_round.insert(1, finding.severity);
    ArenaFinding {
        id,
        file: path.to_string(),
        line: finding.line_start,
        title: finding.title.clone(),
        body: finding.description.clone(),
        severity_by_round,
        raised_by: vec![lens.to_string()],
        // Nothing has judged these yet — that is the arbiter's job.
        verdict: Verdict::Pending,
        confidence: confidence_score(finding.confidence),
        rationale: finding.suggestion.clone(),
        rounds: vec![RoundLog {
            n: 1,
            log: vec![Ballot {
                reviewer: lens.to_string(),
                vote: Vote::Propose,
                note: finding.description.clone(),
                merge_target: None,
            }],
        }],
        merge_candidates: Vec::new(),
        merged_children: Vec::new(),
        evidence: finding.evidence.clone(),
        override_: None,
        accepted_at: None,
    }
}

/// A self-reported confidence as the 0..1 the arena carries.
///
/// The thresholds mirror `arena_finding_to_review`, so a score round-trips back
/// to the level it came from.
const fn confidence_score(confidence: Confidence) -> f32 {
    match confidence {
        Confidence::Confirmed => 0.9,
        Confidence::Tentative => 0.6,
        Confidence::Informational => 0.3,
        Confidence::Dropped => 0.0,
    }
}

/// One survivor per group, in the order the groups' first members appeared.
fn collapse_groups(findings: Vec<ArenaFinding>) -> Vec<ArenaFinding> {
    let index: HashMap<&str, usize> = findings
        .iter()
        .enumerate()
        .map(|(i, f)| (f.id.as_str(), i))
        .collect();

    // Edges first: `index` borrows `findings`, so it has to be gone before the
    // findings are moved out below.
    let mut edges: Vec<(usize, usize)> = Vec::new();
    for (i, finding) in findings.iter().enumerate() {
        for candidate in &finding.merge_candidates {
            if let Some(&j) = index.get(candidate.as_str()) {
                edges.push((i, j));
            }
        }
    }
    drop(index);

    let mut parent: Vec<usize> = (0..findings.len()).collect();
    for (i, j) in edges {
        let (ri, rj) = (root(&mut parent, i), root(&mut parent, j));
        if ri != rj {
            parent[ri] = rj;
        }
    }

    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..findings.len() {
        let r = root(&mut parent, i);
        groups.entry(r).or_default().push(i);
    }

    let mut slots: Vec<Option<ArenaFinding>> = findings.into_iter().map(Some).collect();
    let mut out = Vec::with_capacity(groups.len());
    for members in groups.into_values() {
        let mut group: Vec<ArenaFinding> = members
            .into_iter()
            .filter_map(|i| slots[i].take())
            .collect();
        if group.len() == 1 {
            out.push(group.pop().expect("one member"));
        } else {
            out.push(collapse_one(group));
        }
    }
    out
}

/// Union-find root with path halving.
fn root(parent: &mut [usize], mut x: usize) -> usize {
    while parent[x] != x {
        parent[x] = parent[parent[x]];
        x = parent[x];
    }
    x
}

/// Collapse a group to one finding.
///
/// The survivor is the most severe, then the most detailed; ties break on id,
/// so the choice is stable across runs and re-running the dedupe cannot
/// reshuffle which claim survives.
fn collapse_one(mut group: Vec<ArenaFinding>) -> ArenaFinding {
    group.sort_by(|a, b| {
        let (sa, sb) = (worst_severity(a), worst_severity(b));
        severity_rank(sb)
            .cmp(&severity_rank(sa))
            .then_with(|| b.body.len().cmp(&a.body.len()))
            .then_with(|| a.id.cmp(&b.id))
    });

    let mut survivor = group.remove(0);
    for child in group {
        for raiser in &child.raised_by {
            if !survivor.raised_by.contains(raiser) {
                survivor.raised_by.push(raiser.clone());
            }
        }
        for (round, severity) in &child.severity_by_round {
            let worse = survivor
                .severity_by_round
                .get(round)
                .is_none_or(|current| severity_rank(*current) < severity_rank(*severity));
            if worse {
                survivor.severity_by_round.insert(*round, *severity);
            }
        }
        survivor.merged_children.push(child);
    }

    // The group is resolved, so the proposals that built it would only point at
    // ids no longer present at the top level.
    survivor.merge_candidates.clear();
    for child in &mut survivor.merged_children {
        child.merge_candidates.clear();
    }
    survivor
}

fn worst_severity(finding: &ArenaFinding) -> crate::ai::RiskLevel {
    finding
        .severity_by_round
        .values()
        .copied()
        .max_by_key(|s| severity_rank(*s))
        .unwrap_or(crate::ai::RiskLevel::Info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::Path;
    use tempfile::tempdir;

    const HASH: &str = "diff-hash";

    /// One expert sidecar, one finding per `(path, title, line)`.
    fn write_expert(er_dir: &Path, expert_id: &str, entries: &[(&str, &str, usize)]) {
        let mut by_file: BTreeMap<&str, Vec<serde_json::Value>> = BTreeMap::new();
        for (path, title, line) in entries {
            by_file.entry(path).or_default().push(json!({
                "id": format!("{expert_id}-1"),
                "severity": "medium",
                "title": title,
                "description": format!("{title} — body"),
                "line_start": line,
            }));
        }
        let files: serde_json::Map<String, serde_json::Value> = by_file
            .into_iter()
            .map(|(path, findings)| (path.to_string(), json!({ "findings": findings })))
            .collect();
        let sidecar = json!({
            "version": 1,
            "expert_id": expert_id,
            "diff_hash": HASH,
            "files": files,
        });
        let dir = er_dir.join("experts");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(format!("{expert_id}.json")),
            serde_json::to_string(&sidecar).unwrap(),
        )
        .unwrap();
    }

    /// An identical claim from three lenses is one row, and the row names all
    /// three. This is the case the dedupe exists for: five experts noticing the
    /// same missing check used to produce five rows.
    #[test]
    fn the_same_claim_from_three_experts_collapses_to_one_row() {
        let dir = tempdir().unwrap();
        write_expert(
            dir.path(),
            "security",
            &[("src/a.rs", "unchecked user input", 10)],
        );
        write_expert(
            dir.path(),
            "reliability",
            &[("src/a.rs", "unchecked user input", 10)],
        );
        write_expert(
            dir.path(),
            "patterns",
            &[("src/a.rs", "unchecked user input", 10)],
        );

        let out = dedupe_expert_findings(dir.path().to_str().unwrap(), HASH, "");

        assert_eq!(out.len(), 1, "one issue, one row");
        assert_eq!(
            out[0].raised_by,
            vec!["patterns", "reliability", "security"]
        );
        assert!(
            out[0].merged_children.is_empty(),
            "an identical id merges outright, with nothing left over"
        );
    }

    /// Near-duplicates (different wording, nearby lines) go through the arena's
    /// merge proposer instead, and collapse to one survivor that keeps the
    /// others as children.
    #[test]
    fn near_duplicates_collapse_with_children_recorded() {
        let dir = tempdir().unwrap();
        write_expert(
            dir.path(),
            "security",
            &[("src/a.rs", "unvalidated input in cache key", 10)],
        );
        write_expert(
            dir.path(),
            "reliability",
            &[("src/a.rs", "unvalidated input in cache key.", 12)],
        );

        let out = dedupe_expert_findings(dir.path().to_str().unwrap(), HASH, "");

        assert_eq!(out.len(), 1, "near-duplicates collapse");
        assert_eq!(out[0].raised_by, vec!["reliability", "security"]);
        assert_eq!(out[0].merged_children.len(), 1, "the other is recorded");
        assert!(
            out[0].merge_candidates.is_empty(),
            "the group is resolved, so the proposal would dangle"
        );
        assert!(out[0].merged_children[0].merge_candidates.is_empty());
    }

    #[test]
    fn distinct_issues_stay_separate() {
        let dir = tempdir().unwrap();
        write_expert(
            dir.path(),
            "security",
            &[
                ("src/a.rs", "unchecked user input", 10),
                ("src/a.rs", "missing timeout on retry", 40),
            ],
        );

        let out = dedupe_expert_findings(dir.path().to_str().unwrap(), HASH, "");

        assert_eq!(out.len(), 2);
        let titles: Vec<&str> = out.iter().map(|f| f.title.as_str()).collect();
        assert!(titles.contains(&"unchecked user input"));
        assert!(titles.contains(&"missing timeout on retry"));
    }

    /// The arena's proposer guards this; the adapter must not lose the guard.
    #[test]
    fn the_same_title_in_different_files_never_merges() {
        let dir = tempdir().unwrap();
        write_expert(
            dir.path(),
            "security",
            &[("src/a.rs", "missing null check", 10)],
        );
        write_expert(
            dir.path(),
            "reliability",
            &[("src/b.rs", "missing null check", 10)],
        );

        let out = dedupe_expert_findings(dir.path().to_str().unwrap(), HASH, "");

        assert_eq!(out.len(), 2, "same title, different file");
        assert!(out.iter().all(|f| f.raised_by.len() == 1));
    }

    /// The survivor is the worst claim in the group, so collapsing cannot
    /// quietly downgrade a high-severity finding.
    #[test]
    fn the_survivor_carries_the_worst_severity() {
        let dir = tempdir().unwrap();
        write_expert(
            dir.path(),
            "security",
            &[("src/a.rs", "unvalidated input in cache key", 10)],
        );
        write_expert(
            dir.path(),
            "reliability",
            &[("src/a.rs", "unvalidated input in cache key.", 12)],
        );
        // Raise the second one to high without changing its identity.
        let path = dir.path().join("experts/reliability.json");
        let bumped = std::fs::read_to_string(&path)
            .unwrap()
            .replace("\"severity\":\"medium\"", "\"severity\":\"high\"");
        std::fs::write(&path, bumped).unwrap();

        let out = dedupe_expert_findings(dir.path().to_str().unwrap(), HASH, "");

        assert_eq!(out.len(), 1);
        assert_eq!(
            out[0].severity_by_round.get(&1),
            Some(&crate::ai::RiskLevel::High)
        );
    }

    #[test]
    fn sidecars_from_another_diff_are_skipped() {
        let dir = tempdir().unwrap();
        write_expert(
            dir.path(),
            "security",
            &[("src/a.rs", "unchecked user input", 10)],
        );

        let out = dedupe_expert_findings(dir.path().to_str().unwrap(), "other-hash", "");

        assert!(out.is_empty());
    }

    /// An expert file whose id is not in the registry would put a non-lens
    /// string in `raised_by`, so it is skipped rather than attributed.
    #[test]
    fn an_unregistered_expert_contributes_nothing() {
        let dir = tempdir().unwrap();
        write_expert(
            dir.path(),
            "retired-lens",
            &[("src/a.rs", "unchecked user input", 10)],
        );

        let out = dedupe_expert_findings(dir.path().to_str().unwrap(), HASH, "");

        assert!(out.is_empty());
    }

    #[test]
    fn confidence_maps_onto_the_arena_scale() {
        assert_eq!(confidence_score(Confidence::Confirmed), 0.9);
        assert_eq!(confidence_score(Confidence::Tentative), 0.6);
        assert_eq!(confidence_score(Confidence::Informational), 0.3);
        assert_eq!(confidence_score(Confidence::Dropped), 0.0);
    }
}
