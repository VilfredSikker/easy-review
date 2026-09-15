//! Collapse expert findings that describe the same issue, and assemble the run
//! that validates them.
//!
//! The only step of the arbiter path that needs no model call: the expert
//! sidecars are already on disk, and the arena's existing duplicate proposer
//! does the grouping. The arbiter then rules on the result — a regraded
//! confidence and a verdict per finding — which is one call however many experts
//! contributed.

use super::merge::{propose_merge_candidates, raise_severity, severity_rank};
use super::model::{
    ArenaConfig, ArenaFinding, ArenaRun, ArenaRunKind, ArenaScope, Ballot, CostEstimate,
    ReviewerRef, RoundLog, RunStatus, Verdict, Vote,
};
use super::orchestrator::resolve_reviewers;
use crate::ai::finding_key;
use crate::ai::{expert_by_id, expert_hash_accepted, load_expert_reviews, Finding};
use crate::config::ErConfig;
use anyhow::Result;
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
    // `load_expert_reviews` reads the directory in filesystem order, which is
    // not stable across machines or runs. Left alone it would decide which
    // claim survives a collision, which raiser is listed first, and which row
    // comes out on top — all of it content the UI shows.
    let mut experts = load_expert_reviews(er_dir);
    experts.sort_by(|a, b| a.expert_id.cmp(&b.expert_id));

    let mut out: Vec<ArenaFinding> = Vec::new();
    for expert in experts {
        if !expert_hash_accepted(&expert, review_hash, current_diff_hash) {
            continue;
        }
        let Some(def) = expert_by_id(&expert.expert_id) else {
            continue;
        };
        for (path, file_review) in &expert.files {
            for finding in &file_review.findings {
                let id = finding_key(path, finding.line_start, &finding.title);
                match out.iter_mut().find(|f| f.id == id) {
                    // The same claim from a second expert: keep one row, record
                    // both raisers, take the worse severity. Same key means same
                    // file, same anchor and same claim — two issues sharing a
                    // title are different keys now.
                    Some(existing) => {
                        if !existing.raised_by.iter().any(|r| r == def.id) {
                            existing.raised_by.push(def.id.to_string());
                        }
                        raise_severity(existing, 1, finding.severity);
                    }
                    None => out.push(from_expert_finding(path, finding, def.id, id)),
                }
            }
        }
    }

    propose_merge_candidates(&mut out);
    let mut collapsed = collapse_groups(out);
    for finding in &mut collapsed {
        finding.raised_by.sort();
    }
    collapsed
}

/// Every lens that raised something in `findings`, deduplicated and sorted.
///
/// This is the run's reviewer list: an expert that contributed a finding is a
/// reviewer on the seeded run, and an expert that contributed nothing is not.
pub fn contributing_lenses(findings: &[ArenaFinding]) -> Vec<String> {
    let mut lenses: Vec<&str> = findings
        .iter()
        .flat_map(|f| f.raised_by.iter().map(String::as_str))
        .collect();
    lenses.sort_unstable();
    lenses.dedup();
    lenses.into_iter().map(str::to_string).collect()
}

/// What a seeded run needs that cannot be read off the findings.
pub struct SeededRunParams {
    pub id: String,
    pub diff_hash: String,
    pub base_branch: String,
    pub branch_ref: String,
    pub scope: ArenaScope,
    /// The model that will do the ruling.
    pub arbiter: ReviewerRef,
}

/// Assemble the run record for a seeded pass over already-deduped findings.
///
/// `status` is `Queued`: nothing has run yet. The caller saves this and hands it
/// to the executor, which is what actually reaches the arbiter.
pub fn build_seeded_run(
    config: &ErConfig,
    params: SeededRunParams,
    findings: Vec<ArenaFinding>,
) -> Result<ArenaRun> {
    // The experts are ordinary agent spawns, so they carry the hub's default
    // selection rather than the arbiter's — these records only label the run,
    // but a reviewer list naming the arbiter's model would be a lie the process
    // matrix repeats.
    let provider_id = config
        .ai_hub
        .default_provider
        .clone()
        .unwrap_or_else(|| params.arbiter.provider_id.clone());
    let model_id = config
        .ai_hub
        .default_model
        .clone()
        .unwrap_or_else(|| params.arbiter.model_id.clone());

    let refs: Vec<ReviewerRef> = contributing_lenses(&findings)
        .into_iter()
        .map(|lens| ReviewerRef {
            provider_id: provider_id.clone(),
            model_id: model_id.clone(),
            agent_kind: Some(format!("expert:{lens}")),
        })
        .collect();
    let reviewers = resolve_reviewers(config, &refs)?;

    Ok(ArenaRun {
        id: params.id,
        title: Some(format!("Validate {} expert findings", findings.len())),
        branch_ref: params.branch_ref,
        base_branch: params.base_branch,
        scope: params.scope,
        diff_hash: params.diff_hash,
        created_at: crate::app::chrono_now(),
        completed_at: None,
        status: RunStatus::Queued,
        config: ArenaConfig {
            reviewers: refs,
            // Two rounds so the run never satisfies `total_rounds < 2`, the
            // short circuit that finalises single-round runs and skips the
            // arbiter entirely. Nothing debates; the round count is a guard.
            rounds: 2,
            arbiter: params.arbiter,
            auto_accept_threshold: 0.75,
            scope: params.scope,
            files: None,
            run_kind: ArenaRunKind::Seeded,
            agent_kind: None,
            effort: config.ai_hub.default_effort.clone(),
        },
        reviewers,
        findings,
        accepted_finding_ids: Vec::new(),
        // No reviewer calls happen, so the only spend is the arbiter's one pass,
        // which the executor records once it knows the real usage.
        cost_estimate: CostEstimate {
            tokens_in: 0,
            tokens_out: 0,
            usd: 0.0,
        },
    })
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
        confidence: finding.confidence.score(),
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
            raise_severity(&mut survivor, *round, *severity);
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
    use crate::arena::model::ReviewerRunStatus;
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

    /// The id keys on file + title, so two genuinely different issues sharing a
    /// title in one file collide. They collapse — the id has to stay unique per
    /// row — but the second claim's text survives as a child instead of being
    /// dropped on the floor.
    #[test]
    fn a_title_collision_keeps_the_second_claims_text() {
        let dir = tempdir().unwrap();
        write_expert(
            dir.path(),
            "security",
            &[("src/a.rs", "missing null check", 10)],
        );
        write_expert(
            dir.path(),
            "reliability",
            &[("src/a.rs", "missing null check", 400)],
        );

        let out = dedupe_expert_findings(dir.path().to_str().unwrap(), HASH, "");

        // The key carries the anchor line, so these are different claims and
        // stay two rows. Title alone could not tell them apart, and merging them
        // would have quietly dropped one.
        assert_eq!(out.len(), 2, "different anchors, different findings");
        let mut anchors: Vec<Option<usize>> = out.iter().map(|f| f.line).collect();
        anchors.sort();
        assert_eq!(anchors, vec![Some(10), Some(400)]);
        for finding in &out {
            assert_eq!(finding.raised_by.len(), 1, "each has its own raiser");
            assert!(finding.merged_children.is_empty());
        }
    }

    /// The same claim at the same anchor from two experts is still one row.
    #[test]
    fn the_same_claim_at_the_same_anchor_still_merges() {
        let dir = tempdir().unwrap();
        write_expert(
            dir.path(),
            "security",
            &[("src/a.rs", "missing null check", 10)],
        );
        write_expert(
            dir.path(),
            "reliability",
            &[("src/a.rs", "missing null check", 10)],
        );

        let out = dedupe_expert_findings(dir.path().to_str().unwrap(), HASH, "");

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].raised_by, vec!["reliability", "security"]);
    }

    /// The run's reviewers are the experts that actually raised something —
    /// `min_survivors_required` and the process matrix both read that list, and
    /// the seeded path has no round 1 to fill it in.
    #[test]
    fn contributing_lenses_names_every_raiser_once() {
        let dir = tempdir().unwrap();
        write_expert(
            dir.path(),
            "security",
            &[("src/a.rs", "unchecked input", 10)],
        );
        write_expert(
            dir.path(),
            "reliability",
            &[("src/a.rs", "unchecked input", 10)],
        );
        write_expert(
            dir.path(),
            "patterns",
            &[("src/b.rs", "missing null check", 5)],
        );

        let findings = dedupe_expert_findings(dir.path().to_str().unwrap(), HASH, "");
        let lenses = contributing_lenses(&findings);

        assert_eq!(lenses, vec!["patterns", "reliability", "security"]);
    }

    #[test]
    fn contributing_lenses_is_empty_without_findings() {
        assert!(contributing_lenses(&[]).is_empty());
    }

    /// The seeded path exists to reach the arbiter, so the run must never be
    /// shaped like the single-round case that finalises verdicts without it.
    #[test]
    fn a_seeded_run_is_not_shaped_like_the_single_round_short_circuit() {
        let config = ErConfig::default();
        let findings = vec![];
        let run = build_seeded_run(
            &config,
            SeededRunParams {
                id: "arena-seeded-1".to_string(),
                diff_hash: HASH.to_string(),
                base_branch: "main".to_string(),
                branch_ref: "feature".to_string(),
                scope: ArenaScope::Branch,
                arbiter: ReviewerRef {
                    provider_id: "anthropic".to_string(),
                    model_id: "opus".to_string(),
                    agent_kind: None,
                },
            },
            findings,
        )
        .expect("builds without providers in the hub");

        assert_eq!(run.config.run_kind, ArenaRunKind::Seeded);
        assert!(
            run.config.rounds >= 2,
            "a run with fewer than 2 rounds skips the arbiter entirely"
        );
        assert_eq!(run.status, RunStatus::Queued);
    }

    /// Reviewers are labelled with the expert that raised the finding, and the
    /// hub default is what an ordinary agent spawn uses.
    #[test]
    fn a_seeded_run_lists_its_contributing_experts_as_reviewers() {
        let dir = tempdir().unwrap();
        write_expert(
            dir.path(),
            "security",
            &[("src/a.rs", "unchecked input", 10)],
        );
        write_expert(
            dir.path(),
            "testing",
            &[("src/b.rs", "no negative case", 4)],
        );
        let findings = dedupe_expert_findings(dir.path().to_str().unwrap(), HASH, "");

        // `resolve_reviewers` looks providers up in the hub, and a bare
        // `ErConfig::default()` has an empty one — the catalog is what a real
        // load seeds it with.
        let mut config = ErConfig::default();
        crate::config::supplement_ai_hub(&mut config.ai_hub);
        let provider_id = config
            .ai_hub
            .default_provider
            .clone()
            .expect("catalog names a default provider");
        let model_id = config
            .ai_hub
            .default_model
            .clone()
            .expect("catalog names a default model");

        let run = build_seeded_run(
            &config,
            SeededRunParams {
                id: "arena-seeded-2".to_string(),
                diff_hash: HASH.to_string(),
                base_branch: "main".to_string(),
                branch_ref: "feature".to_string(),
                scope: ArenaScope::Branch,
                arbiter: ReviewerRef {
                    provider_id,
                    model_id,
                    agent_kind: None,
                },
            },
            findings,
        )
        .expect("builds");

        assert_eq!(run.config.reviewers.len(), 2);
        let kinds: Vec<&str> = run
            .config
            .reviewers
            .iter()
            .filter_map(|r| r.agent_kind.as_deref())
            .collect();
        assert_eq!(kinds, vec!["expert:security", "expert:testing"]);
        assert!(run
            .reviewers
            .iter()
            .all(|r| matches!(r.status, ReviewerRunStatus::Ok)));
    }
}
