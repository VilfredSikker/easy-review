//! Compact summaries of managed triage / review / tour sidecars for MCP tooling.

use crate::ai::{load_tour_sidecar, load_triage_review, ErReview, ErTour, RiskLevel, TriageReview};
use crate::github::owner_repo_storage_slug;
use crate::sidecar_upload::SidecarKind;
use crate::storage::{pr_bucket_dir, storage_root};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::UNIX_EPOCH;

/// Slim view of what Easy Review already knows about a PR from local sidecars.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrSidecarSummary {
    pub owner: String,
    pub repo: String,
    pub number: u64,
    pub bucket_path: String,
    pub triage: Option<TriageSummary>,
    pub review: Option<ReviewSummary>,
    pub tour: Option<TourSummary>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TourSummary {
    pub title: String,
    pub overview: String,
    pub pillars: usize,
    pub files: usize,
    pub pillar_titles: Vec<String>,
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

fn summarize_tour(t: &ErTour) -> TourSummary {
    let pillars = t.ordered_pillars();
    let files = pillars
        .iter()
        .map(|p| p.all_file_paths().count())
        .sum::<usize>();
    let pillar_titles = pillars.iter().map(|p| p.title.clone()).collect();
    TourSummary {
        title: t.title.clone(),
        overview: t.overview.clone(),
        pillars: pillars.len(),
        files,
        pillar_titles,
        diff_hash: t.diff_hash.clone(),
        created_at: t.created_at.clone(),
    }
}

/// Kind labels present in a PR bucket (`triage` / `review` / `tour`).
pub fn present_kinds(summary: &PrSidecarSummary) -> Vec<&'static str> {
    let mut kinds = Vec::new();
    if summary.triage.is_some() {
        kinds.push("triage");
    }
    if summary.review.is_some() {
        kinds.push("review");
    }
    if summary.tour.is_some() {
        kinds.push("tour");
    }
    kinds
}

fn kind_matches_filter(kinds: &[&str], filter: Option<&[SidecarKind]>) -> bool {
    let Some(filter) = filter else {
        return !kinds.is_empty();
    };
    if filter.is_empty() {
        return !kinds.is_empty();
    }
    filter.iter().any(|k| match k {
        SidecarKind::Triage => kinds.contains(&"triage"),
        SidecarKind::Review => kinds.contains(&"review"),
        SidecarKind::Tour => kinds.contains(&"tour"),
    })
}

fn file_mtime_secs(path: &Path) -> Option<u64> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?;
    Some(
        modified
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    )
}

fn bucket_sidecar_mtime_secs(bucket: &Path) -> u64 {
    ["triage.json", "review.json", "tour.json"]
        .iter()
        .filter_map(|name| file_mtime_secs(&bucket.join(name)))
        .max()
        .unwrap_or(0)
}

/// One PR bucket that has at least one triage/review/tour sidecar.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListedPrArtifacts {
    #[serde(flatten)]
    pub summary: PrSidecarSummary,
    /// Present kinds: `triage`, `review`, `tour`.
    pub kinds: Vec<String>,
    /// Max mtime (unix seconds) among present sidecar files.
    pub mtime_secs: u64,
}

/// Scan managed `prs/pr-*` buckets for uploaded triage/review/tour sidecars.
///
/// Newest sidecar mtime first. `kinds_filter` keeps PRs that have *any* of the
/// requested kinds. `limit` caps the result (callers should clamp).
pub fn list_repo_pr_artifacts(
    owner: &str,
    repo: &str,
    kinds_filter: Option<&[SidecarKind]>,
    limit: usize,
) -> Vec<ListedPrArtifacts> {
    let slug = owner_repo_storage_slug(owner, repo);
    let prs_dir = storage_root().join("repos").join(&slug).join("prs");
    list_repo_pr_artifacts_in_dir(owner, repo, &prs_dir, kinds_filter, limit)
}

/// Test / custom-root variant of [`list_repo_pr_artifacts`].
pub fn list_repo_pr_artifacts_in_dir(
    owner: &str,
    repo: &str,
    prs_dir: &Path,
    kinds_filter: Option<&[SidecarKind]>,
    limit: usize,
) -> Vec<ListedPrArtifacts> {
    let Ok(entries) = std::fs::read_dir(prs_dir) else {
        return Vec::new();
    };

    let mut listed = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(number_str) = name.strip_prefix("pr-") else {
            continue;
        };
        let Ok(number) = number_str.parse::<u64>() else {
            continue;
        };

        let summary = summarize_pr_bucket(owner, repo, number, &path);
        let kinds = present_kinds(&summary);
        if !kind_matches_filter(&kinds, kinds_filter) {
            continue;
        }
        let mtime_secs = bucket_sidecar_mtime_secs(&path);
        listed.push(ListedPrArtifacts {
            summary,
            kinds: kinds.into_iter().map(str::to_string).collect(),
            mtime_secs,
        });
    }

    listed.sort_by(|a, b| {
        b.mtime_secs
            .cmp(&a.mtime_secs)
            .then_with(|| b.summary.number.cmp(&a.summary.number))
    });
    listed.truncate(limit);
    listed
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

    let tour = if bucket.join("tour.json").exists() {
        load_tour_sidecar(&path_str, "tour.json").map(|t| summarize_tour(&t))
    } else {
        missing.push("tour.json".into());
        None
    };

    PrSidecarSummary {
        owner: owner.to_string(),
        repo: repo.to_string(),
        number,
        bucket_path: path_str,
        triage,
        review,
        tour,
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
        assert!(summary.tour.is_none());
        assert!(summary.missing.contains(&"triage.json".into()));
        assert!(summary.missing.contains(&"tour.json".into()));
    }

    #[test]
    fn summarize_reads_triage_review_and_tour() {
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

        let tour = ErTour {
            version: 1,
            diff_hash: "abc".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            title: "Tour: widgets".into(),
            overview: "Start with core".into(),
            pillars: vec![crate::ai::TourPillar {
                id: "p1".into(),
                title: "Core".into(),
                description: "main path".into(),
                order: 0,
                importance: 90,
                foundation: true,
                files: vec![crate::ai::TourFile {
                    path: "src/a.rs".into(),
                    reason: "entry".into(),
                    finding_ids: vec![],
                    related: vec![],
                }],
            }],
        };
        std::fs::write(
            bucket.join("tour.json"),
            serde_json::to_string(&tour).unwrap(),
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
        let tour_sum = summary.tour.as_ref().unwrap();
        assert_eq!(tour_sum.title, "Tour: widgets");
        assert_eq!(tour_sum.pillars, 1);
        assert_eq!(tour_sum.files, 1);
        assert_eq!(tour_sum.pillar_titles, vec!["Core".to_string()]);
        assert!(summary.missing.is_empty());
    }

    #[test]
    fn list_repo_pr_artifacts_finds_triage_and_tour_only() {
        let dir = tempfile::tempdir().unwrap();
        let prs = dir.path();

        let empty = prs.join("pr-1");
        std::fs::create_dir_all(&empty).unwrap();

        let with_triage = prs.join("pr-10");
        std::fs::create_dir_all(&with_triage).unwrap();
        let triage = TriageReview {
            version: 1,
            diff_hash: "h1".into(),
            diff_scope: "pr".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            first_impression: "ok".into(),
            diff_stats: TriageDiffStats {
                files_changed: 1,
                approx_risk: "low".into(),
                domains: vec![],
            },
            verdict: TriageVerdict {
                primary: TriageVerdictPrimary::General,
                experts: vec![],
                rationale: "r".into(),
                confidence: "high".into(),
            },
            priority_files: vec![],
        };
        std::fs::write(
            with_triage.join("triage.json"),
            serde_json::to_string(&triage).unwrap(),
        )
        .unwrap();

        let with_tour = prs.join("pr-20");
        std::fs::create_dir_all(&with_tour).unwrap();
        let tour = ErTour {
            version: 1,
            diff_hash: "h2".into(),
            created_at: "2026-01-02T00:00:00Z".into(),
            title: "Tour only".into(),
            overview: "o".into(),
            pillars: vec![],
        };
        std::fs::write(
            with_tour.join("tour.json"),
            serde_json::to_string(&tour).unwrap(),
        )
        .unwrap();

        // Questions-only bucket should not appear.
        let questions_only = prs.join("pr-30");
        std::fs::create_dir_all(&questions_only).unwrap();
        std::fs::write(questions_only.join("questions.json"), "[]").unwrap();

        let all = list_repo_pr_artifacts_in_dir("acme", "widgets", prs, None, 50);
        let numbers: Vec<u64> = all.iter().map(|e| e.summary.number).collect();
        assert!(numbers.contains(&10));
        assert!(numbers.contains(&20));
        assert!(!numbers.contains(&1));
        assert!(!numbers.contains(&30));

        let tours_only = list_repo_pr_artifacts_in_dir(
            "acme",
            "widgets",
            prs,
            Some(&[SidecarKind::Tour]),
            50,
        );
        assert_eq!(tours_only.len(), 1);
        assert_eq!(tours_only[0].summary.number, 20);
        assert_eq!(tours_only[0].kinds, vec!["tour".to_string()]);
    }
}
