//! Compact summaries of managed triage / review sidecars for MCP tooling.

use crate::ai::{load_triage_review, ErReview, RiskLevel, TriageReview};
use crate::github::owner_repo_storage_slug;
use crate::storage::pr_bucket_dir;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// Slim view of what Easy Review already knows about a PR from local sidecars.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrSidecarSummary {
    pub owner: String,
    pub repo: String,
    pub number: u64,
    pub bucket_path: String,
    pub triage: Option<TriageSummary>,
    pub review: Option<ReviewSummary>,
    pub missing: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriageSummary {
    pub first_impression: String,
    pub primary: String,
    pub confidence: String,
    pub rationale: String,
    pub approx_risk: String,
    pub domains: Vec<String>,
    pub priority_files: Vec<String>,
    pub diff_hash: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewSummary {
    pub files: usize,
    pub findings: usize,
    pub by_risk: BTreeMap<String, usize>,
    pub high_risk_files: Vec<String>,
    pub diff_hash: String,
    pub created_at: String,
}

fn load_er_review(er_dir: &Path) -> Option<ErReview> {
    let path = er_dir.join("review.json");
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn summarize_triage(t: &TriageReview) -> TriageSummary {
    TriageSummary {
        first_impression: t.first_impression.clone(),
        primary: crate::ai::verdict_primary_str(&t.verdict.primary).to_string(),
        confidence: t.verdict.confidence.clone(),
        rationale: t.verdict.rationale.clone(),
        approx_risk: t.diff_stats.approx_risk.clone(),
        domains: t.diff_stats.domains.clone(),
        priority_files: t.priority_files.iter().map(|f| f.path.clone()).collect(),
        diff_hash: t.diff_hash.clone(),
        created_at: t.created_at.clone(),
    }
}

fn summarize_review(r: &ErReview) -> ReviewSummary {
    let mut by_risk = BTreeMap::from([
        ("high".to_string(), 0usize),
        ("medium".to_string(), 0usize),
        ("low".to_string(), 0usize),
        ("info".to_string(), 0usize),
    ]);
    let mut findings = 0usize;
    let mut high_risk_files = Vec::new();
    for (path, file) in &r.files {
        if matches!(file.risk, RiskLevel::High) {
            high_risk_files.push(path.clone());
        }
        for f in &file.findings {
            findings += 1;
            let key = match f.severity {
                RiskLevel::High => "high",
                RiskLevel::Medium => "medium",
                RiskLevel::Low => "low",
                RiskLevel::Info => "info",
            };
            *by_risk.entry(key.to_string()).or_insert(0) += 1;
        }
    }
    high_risk_files.sort();
    ReviewSummary {
        files: r.files.len(),
        findings,
        by_risk,
        high_risk_files,
        diff_hash: r.diff_hash.clone(),
        created_at: r.created_at.clone(),
    }
}

/// Read managed PR-bucket sidecars for `owner/repo` PR `#number`.
pub fn summarize_pr_sidecars(owner: &str, repo: &str, number: u64) -> PrSidecarSummary {
    let slug = owner_repo_storage_slug(owner, repo);
    let bucket = pr_bucket_dir(&slug, number);
    summarize_pr_bucket(owner, repo, number, &bucket)
}

/// Summarize sidecars already located at `bucket` (test / custom roots).
pub fn summarize_pr_bucket(
    owner: &str,
    repo: &str,
    number: u64,
    bucket: &Path,
) -> PrSidecarSummary {
    let path_str = bucket.to_string_lossy().into_owned();
    let mut missing = Vec::new();

    let triage = if bucket.join("triage.json").exists() {
        load_triage_review(&path_str).map(|t| summarize_triage(&t))
    } else {
        missing.push("triage.json".into());
        None
    };

    let review = if bucket.join("review.json").exists() {
        load_er_review(bucket).map(|r| summarize_review(&r))
    } else {
        missing.push("review.json".into());
        None
    };

    PrSidecarSummary {
        owner: owner.to_string(),
        repo: repo.to_string(),
        number,
        bucket_path: path_str,
        triage,
        review,
        missing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{TriageDiffStats, TriagePriorityFile, TriageVerdict, TriageVerdictPrimary};
    use std::collections::HashMap;

    #[test]
    fn summarize_empty_bucket() {
        let dir = tempfile::tempdir().unwrap();
        let summary = summarize_pr_bucket("acme", "widgets", 9, dir.path());
        assert!(summary.triage.is_none());
        assert!(summary.review.is_none());
        assert!(summary.missing.contains(&"triage.json".into()));
    }

    #[test]
    fn summarize_reads_triage_and_review() {
        let dir = tempfile::tempdir().unwrap();
        let bucket = dir.path();

        let triage = TriageReview {
            version: 1,
            diff_hash: "abc".into(),
            diff_scope: "pr".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            first_impression: "Looks small".into(),
            diff_stats: TriageDiffStats {
                files_changed: 2,
                approx_risk: "low".into(),
                domains: vec!["ui".into()],
            },
            verdict: TriageVerdict {
                primary: TriageVerdictPrimary::General,
                experts: vec![],
                rationale: "tiny".into(),
                confidence: "high".into(),
            },
            priority_files: vec![TriagePriorityFile {
                path: "src/a.rs".into(),
                reason: "core".into(),
                risk: "low".into(),
            }],
        };
        std::fs::write(
            bucket.join("triage.json"),
            serde_json::to_string(&triage).unwrap(),
        )
        .unwrap();

        let mut files = HashMap::new();
        files.insert(
            "src/a.rs".into(),
            crate::ai::ErFileReview {
                risk: RiskLevel::High,
                risk_reason: "".into(),
                summary: "".into(),
                findings: vec![],
            },
        );
        let review = ErReview {
            version: 1,
            diff_hash: "abc".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            base_branch: "main".into(),
            head_branch: "feat".into(),
            files,
            file_hashes: HashMap::new(),
        };
        std::fs::write(
            bucket.join("review.json"),
            serde_json::to_string(&review).unwrap(),
        )
        .unwrap();

        let summary = summarize_pr_bucket("acme", "widgets", 3, bucket);
        assert_eq!(
            summary.triage.as_ref().unwrap().first_impression,
            "Looks small"
        );
        assert_eq!(summary.review.as_ref().unwrap().files, 1);
        assert_eq!(
            summary.review.as_ref().unwrap().high_risk_files,
            vec!["src/a.rs".to_string()]
        );
    }
}
