//! Import shipped arena findings into `.er/review.json` for the Review tab.

use super::model::{ArenaFinding, ArenaRun, Verdict};
use crate::ai::{Confidence, ErFileReview, ErReview, Finding, RiskLevel};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn review_path(er_dir: &str) -> PathBuf {
    Path::new(er_dir).join("review.json")
}

fn empty_review(run: &ArenaRun) -> ErReview {
    ErReview {
        version: 1,
        diff_hash: run.diff_hash.clone(),
        created_at: crate::app::chrono_now(),
        base_branch: run.base_branch.clone(),
        head_branch: run.branch_ref.clone(),
        files: HashMap::new(),
        file_hashes: HashMap::new(),
    }
}

fn write_json_atomic<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    let json = serde_json::to_string_pretty(value).context("serialize review")?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json.as_bytes()).context("write review tmp")?;
    std::fs::rename(&tmp, path).context("rename review")?;
    Ok(())
}

fn latest_severity(f: &ArenaFinding) -> RiskLevel {
    f.severity_by_round
        .values()
        .next_back()
        .copied()
        .unwrap_or(RiskLevel::Medium)
}

fn arena_finding_to_review(f: &ArenaFinding, run: &ArenaRun) -> Finding {
    let category = run
        .config
        .agent_kind
        .clone()
        .unwrap_or_else(|| "arena".to_string());
    let confidence = if f.confidence >= 0.75 {
        Confidence::Confirmed
    } else if f.confidence >= 0.5 {
        Confidence::Tentative
    } else {
        Confidence::Informational
    };
    Finding {
        id: format!("arena-{}-{}", run.id, f.id),
        severity: latest_severity(f),
        category,
        title: f.title.clone(),
        description: f.body.clone(),
        hunk_index: None,
        line_start: f.line,
        line_end: f.line,
        suggestion: f.rationale.clone(),
        related_files: vec![],
        outside_diff: false,
        confidence,
        verification_plan: String::new(),
        evidence: f.evidence.clone(),
        responses: vec![],
        resolved: false,
        resolved_note: String::new(),
        resolved_at: String::new(),
        promoted_to: None,
    }
}

fn is_importable(f: &ArenaFinding, run: &ArenaRun) -> bool {
    match &f.verdict {
        Verdict::Kept | Verdict::Escalated | Verdict::Merged { .. } => true,
        // Single-round runs complete without arbiter (legacy runs stayed pending).
        Verdict::Pending => run.config.rounds <= 1,
        Verdict::Dropped => false,
    }
}

/// Merge shipped arena findings into `review.json`. Returns count imported.
pub fn import_arena_findings_to_review(
    er_dir: &str,
    run: &mut ArenaRun,
    finding_ids: Option<&[String]>,
) -> Result<usize> {
    let path = review_path(er_dir);
    let mut review: ErReview = if path.is_file() {
        let content = std::fs::read_to_string(&path).context("read review.json")?;
        serde_json::from_str(&content).unwrap_or_else(|_| empty_review(run))
    } else {
        empty_review(run)
    };
    review.diff_hash = run.diff_hash.clone();

    let ids: Vec<String> = match finding_ids {
        Some(ids) => ids.to_vec(),
        None => run
            .findings
            .iter()
            .filter(|f| is_importable(f, run) && f.accepted_at.is_none())
            .map(|f| f.id.clone())
            .collect(),
    };

    let now = crate::app::chrono_now();
    let mut imported = 0usize;
    for fid in &ids {
        let idx = run.findings.iter().position(|x| x.id == *fid);
        let Some(idx) = idx else {
            continue;
        };
        let af = run.findings[idx].clone();
        if !is_importable(&af, run) || af.accepted_at.is_some() {
            continue;
        }
        let finding = arena_finding_to_review(&af, run);
        let file = af.file.clone();
        let risk = latest_severity(&af);
        let entry = review
            .files
            .entry(file.clone())
            .or_insert_with(|| ErFileReview {
                risk,
                risk_reason: String::new(),
                summary: String::new(),
                findings: vec![],
            });
        if let Some(pos) = entry.findings.iter().position(|x| x.id == finding.id) {
            entry.findings[pos] = finding;
        } else {
            entry.findings.push(finding);
        }
        run.findings[idx].accepted_at = Some(now.clone());
        if !run.accepted_finding_ids.contains(fid) {
            run.accepted_finding_ids.push(fid.clone());
        }
        imported += 1;
    }

    if imported > 0 {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        write_json_atomic(&path, &review)?;
    }
    Ok(imported)
}
