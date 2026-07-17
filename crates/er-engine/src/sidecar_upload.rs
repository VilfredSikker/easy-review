//! Prepare PR review kits and upload sidecar JSON for MCP / non-UI callers.
//!
//! Flow: [`prepare_review_kit`] writes shared `diff-tmp` + prompts; the client
//! agent produces artifacts; [`upload_pr_artifacts`] validates and stores them.
//! No agent CLI spawn — avoids competing for [`crate::agent_slots`].

use crate::agent_runtime::{ArtifactBaseline, ArtifactContract};
use crate::ai::compute_diff_hash;
use crate::github::{gh_pr_diff_remote, gh_pr_metadata_remote, owner_repo_storage_slug};
use crate::storage::resolve_managed_root_for_pr_bucket;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Which Easy Review sidecar set to prepare or upload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SidecarKind {
    Triage,
    Review,
    Tour,
}

impl SidecarKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Triage => "triage",
            Self::Review => "review",
            Self::Tour => "tour",
        }
    }
}

/// One artifact kind ready for a client agent to produce.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreparedArtifactSpec {
    pub kind: SidecarKind,
    /// Absolute paths the client must produce (then pass to upload).
    pub required_files: Vec<String>,
    /// Relative names inside the PR bucket.
    pub required_relative: Vec<String>,
    /// Full Easy Review prompt for this kind (same as Desktop prepared-diff).
    pub prompt: String,
}

/// Bucket + diff + prompts for client-side review (no agent spawn).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreparedReviewKit {
    pub owner: String,
    pub repo: String,
    pub pr: u64,
    pub base_ref: String,
    pub head_ref: String,
    pub er_dir: String,
    pub diff_tmp_path: String,
    /// SHA-256 hex of `diff-tmp` — embed this as `diff_hash` in every JSON sidecar.
    pub diff_hash: String,
    pub diff_bytes: u64,
    pub artifacts: Vec<PreparedArtifactSpec>,
    pub instructions: String,
}

/// Files to write for one kind (`relative_name` → content).
#[derive(Debug, Clone)]
pub struct UploadArtifactsRequest {
    pub owner: String,
    pub repo: String,
    pub pr: u64,
    pub kind: SidecarKind,
    pub files: BTreeMap<String, String>,
    /// When true, re-fetch the PR diff before validating (default: reuse existing `diff-tmp`).
    pub refresh_diff: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadArtifactsResult {
    pub er_dir: String,
    pub kind: SidecarKind,
    pub written: Vec<String>,
    pub diff_hash: String,
}

fn relative_files(kind: SidecarKind) -> Vec<&'static str> {
    match kind {
        SidecarKind::Triage => vec!["triage.json"],
        SidecarKind::Tour => vec!["tour.json"],
        SidecarKind::Review => {
            vec![
                "review.json",
                "order.json",
                "checklist.json",
                "summary.md",
            ]
        }
    }
}

fn build_prompt(kind: SidecarKind, er_dir: &str, base: &str, head: &str) -> String {
    use crate::ai::prompts::{
        build_review_prompt_prepared_diff, build_tour_prompt_prepared_diff,
        build_triage_review_prompt_prepared_diff,
    };
    match kind {
        SidecarKind::Triage => build_triage_review_prompt_prepared_diff("branch", er_dir),
        SidecarKind::Review => build_review_prompt_prepared_diff("branch", er_dir, base, head),
        SidecarKind::Tour => build_tour_prompt_prepared_diff("PR diff", er_dir, "tour.json"),
    }
}

fn artifact_contract(kind: SidecarKind) -> ArtifactContract {
    match kind {
        SidecarKind::Triage => ArtifactContract::Triage,
        SidecarKind::Review => ArtifactContract::Review,
        SidecarKind::Tour => ArtifactContract::Tour {
            filename: "tour.json".into(),
        },
    }
}

fn write_atomic(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("mkdir {}", parent.display()))?;
    }
    let tmp_path = PathBuf::from(format!("{}.tmp", path.to_string_lossy()));
    std::fs::write(&tmp_path, content)
        .with_context(|| format!("write {}", tmp_path.display()))?;
    std::fs::rename(&tmp_path, path)
        .with_context(|| format!("rename {} → {}", tmp_path.display(), path.display()))?;
    Ok(())
}

fn resolve_er_dir(owner: &str, repo: &str, pr: u64) -> String {
    let slug = owner_repo_storage_slug(owner, repo);
    resolve_managed_root_for_pr_bucket(&slug, pr).er_dir()
}

/// Resolve managed PR bucket and write `diff-tmp` for a remote PR.
pub fn prepare_pr_diff_tmp(
    owner: &str,
    repo: &str,
    pr: u64,
    ignore_globs: &[String],
) -> Result<(String, String)> {
    let mut raw = gh_pr_diff_remote(owner, repo, pr)?;
    if !ignore_globs.is_empty() {
        raw = crate::git::filter_raw_diff_exclude_globs(&raw, ignore_globs);
    }
    if raw.trim().is_empty() {
        bail!("PR #{pr} has an empty diff after filters");
    }

    let slug = owner_repo_storage_slug(owner, repo);
    let er_dir = resolve_managed_root_for_pr_bucket(&slug, pr).er_dir();
    if er_dir.is_empty() {
        bail!("failed to resolve managed PR storage for {owner}/{repo}#{pr}");
    }
    std::fs::create_dir_all(&er_dir).with_context(|| format!("mkdir {er_dir}"))?;
    let diff_path = format!("{er_dir}/diff-tmp");
    std::fs::write(&diff_path, &raw).with_context(|| format!("write {diff_path}"))?;
    Ok((er_dir, diff_path))
}

/// Fetch PR diff into managed storage and return prompts for the client agent.
pub fn prepare_review_kit(
    owner: &str,
    repo: &str,
    pr: u64,
    kinds: &[SidecarKind],
    ignore_globs: &[String],
) -> Result<PreparedReviewKit> {
    if kinds.is_empty() {
        bail!("at least one artifact kind is required (triage, review, tour)");
    }

    let (meta_base, meta_head) = gh_pr_metadata_remote(owner, repo, pr)
        .unwrap_or_else(|_| ("main".into(), format!("pr-{pr}")));
    let (er_dir, diff_tmp_path) = prepare_pr_diff_tmp(owner, repo, pr, ignore_globs)?;
    let diff_bytes = std::fs::metadata(&diff_tmp_path)
        .map(|m| m.len())
        .unwrap_or(0);
    let diff = std::fs::read_to_string(&diff_tmp_path)
        .with_context(|| format!("read prepared diff {diff_tmp_path}"))?;
    let diff_hash = compute_diff_hash(&diff);

    let mut artifacts = Vec::with_capacity(kinds.len());
    for &kind in kinds {
        let required_relative: Vec<String> = relative_files(kind)
            .into_iter()
            .map(str::to_string)
            .collect();
        let required_files = required_relative
            .iter()
            .map(|name| format!("{er_dir}/{name}"))
            .collect();
        artifacts.push(PreparedArtifactSpec {
            kind,
            required_files,
            required_relative,
            prompt: build_prompt(kind, &er_dir, &meta_base, &meta_head),
        });
    }

    Ok(PreparedReviewKit {
        owner: owner.to_string(),
        repo: repo.to_string(),
        pr,
        base_ref: meta_base,
        head_ref: meta_head,
        er_dir,
        diff_tmp_path,
        diff_hash: diff_hash.clone(),
        diff_bytes,
        artifacts,
        instructions: format!(
            "Read the prepared diff at the diff_tmp_path (do not re-fetch). \
             Embed diff_hash={diff_hash} in every JSON sidecar. \
             Produce the required files for each kind, then call upload_artifacts \
             with the file contents. Do not spawn another agent CLI — you are the reviewer."
        ),
    })
}

/// Validate and atomically write sidecars into an existing PR bucket.
pub fn upload_artifacts_to_dir(
    er_dir: &str,
    kind: SidecarKind,
    files: &BTreeMap<String, String>,
) -> Result<UploadArtifactsResult> {
    let required = relative_files(kind);
    for name in &required {
        if !files.contains_key(*name) {
            bail!(
                "missing required file '{name}' for {}; need: {}",
                kind.as_str(),
                required.join(", ")
            );
        }
    }
    for name in files.keys() {
        if !required.iter().any(|r| r == name) {
            bail!(
                "unexpected file '{name}' for {}; allowed: {}",
                kind.as_str(),
                required.join(", ")
            );
        }
    }

    let diff_path = Path::new(er_dir).join("diff-tmp");
    if !diff_path.exists() {
        bail!(
            "missing {er_dir}/diff-tmp — call prepare_review first so diff_hash can be validated"
        );
    }
    let diff = std::fs::read_to_string(&diff_path)
        .with_context(|| format!("read {}", diff_path.display()))?;
    let expected_hash = compute_diff_hash(&diff);

    let contract = artifact_contract(kind);
    let baseline = ArtifactBaseline::capture(contract, er_dir)?;

    let mut written = Vec::new();
    for name in &required {
        let content = files.get(*name).expect("checked above");
        let path = Path::new(er_dir).join(name);
        write_atomic(&path, content)?;
        written.push(path.to_string_lossy().into_owned());
    }

    baseline.validate(er_dir).with_context(|| {
        format!(
            "uploaded {} artifacts failed validation (expected diff_hash={expected_hash})",
            kind.as_str()
        )
    })?;

    Ok(UploadArtifactsResult {
        er_dir: er_dir.to_string(),
        kind,
        written,
        diff_hash: expected_hash,
    })
}

/// Resolve the PR bucket (optionally refresh `diff-tmp`) and upload artifacts.
pub fn upload_pr_artifacts(req: UploadArtifactsRequest) -> Result<UploadArtifactsResult> {
    let er_dir = if req.refresh_diff {
        prepare_pr_diff_tmp(&req.owner, &req.repo, req.pr, &[])?.0
    } else {
        let er_dir = resolve_er_dir(&req.owner, &req.repo, req.pr);
        let diff_path = Path::new(&er_dir).join("diff-tmp");
        if !diff_path.exists() {
            prepare_pr_diff_tmp(&req.owner, &req.repo, req.pr, &[])?.0
        } else {
            if er_dir.is_empty() {
                bail!("failed to resolve managed PR storage");
            }
            std::fs::create_dir_all(&er_dir).ok();
            er_dir
        }
    };
    upload_artifacts_to_dir(&er_dir, req.kind, &req.files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{
        ErChecklist, ErOrder, ErReview, ErTour, RiskLevel, TourFile, TourPillar, TriageReview,
    };
    use crate::storage::STORAGE_TEST_ENV_LOCK;
    use std::collections::HashMap;

    fn with_storage_root<T>(f: impl FnOnce() -> T) -> T {
        let _guard = STORAGE_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let root = tempfile::tempdir().unwrap();
        std::env::set_var("ER_STORAGE_ROOT", root.path());
        let out = f();
        std::env::remove_var("ER_STORAGE_ROOT");
        out
    }

    fn write_diff_tmp(er_dir: &str, diff: &str) {
        std::fs::create_dir_all(er_dir).unwrap();
        std::fs::write(format!("{er_dir}/diff-tmp"), diff).unwrap();
    }

    #[test]
    fn upload_tour_validates_hash() {
        with_storage_root(|| {
            let er_dir = resolve_managed_root_for_pr_bucket("acme-widgets", 7).er_dir();
            let diff = "diff --git a/x b/x\n+hello\n";
            write_diff_tmp(&er_dir, diff);
            let hash = compute_diff_hash(diff);

            let tour = ErTour {
                version: 1,
                diff_hash: hash,
                created_at: "2026-01-01T00:00:00Z".into(),
                title: "Tour".into(),
                overview: "hi".into(),
                pillars: vec![TourPillar {
                    id: "p1".into(),
                    title: "Core".into(),
                    description: "d".into(),
                    order: 0,
                    importance: 80,
                    foundation: true,
                    files: vec![TourFile {
                        path: "x".into(),
                        reason: "changed".into(),
                        finding_ids: vec![],
                        related: vec![],
                    }],
                }],
            };
            let mut files = BTreeMap::new();
            files.insert(
                "tour.json".into(),
                serde_json::to_string_pretty(&tour).unwrap(),
            );
            let result = upload_artifacts_to_dir(&er_dir, SidecarKind::Tour, &files).unwrap();
            assert_eq!(result.written.len(), 1);
            assert!(Path::new(&er_dir).join("tour.json").exists());
        });
    }

    #[test]
    fn upload_rejects_stale_hash() {
        with_storage_root(|| {
            let er_dir = resolve_managed_root_for_pr_bucket("acme-widgets", 8).er_dir();
            write_diff_tmp(&er_dir, "diff --git a/x b/x\n+hello\n");

            let tour = ErTour {
                version: 1,
                diff_hash: "deadbeef".into(),
                created_at: "".into(),
                title: "Tour".into(),
                overview: "".into(),
                pillars: vec![],
            };
            let mut files = BTreeMap::new();
            files.insert("tour.json".into(), serde_json::to_string(&tour).unwrap());
            assert!(upload_artifacts_to_dir(&er_dir, SidecarKind::Tour, &files).is_err());
        });
    }

    #[test]
    fn relative_files_for_review_include_all_four() {
        assert_eq!(
            relative_files(SidecarKind::Review),
            vec![
                "review.json",
                "order.json",
                "checklist.json",
                "summary.md"
            ]
        );
    }

    #[test]
    fn review_bundle_validates() {
        with_storage_root(|| {
            let er_dir = resolve_managed_root_for_pr_bucket("acme-widgets", 9).er_dir();
            let diff = "diff --git a/a.rs b/a.rs\n+1\n";
            write_diff_tmp(&er_dir, diff);
            let hash = compute_diff_hash(diff);

            let mut review_files = HashMap::new();
            review_files.insert(
                "a.rs".into(),
                crate::ai::ErFileReview {
                    risk: RiskLevel::Low,
                    risk_reason: "".into(),
                    summary: "ok".into(),
                    findings: vec![],
                },
            );
            let review = ErReview {
                version: 1,
                diff_hash: hash.clone(),
                created_at: "2026-01-01T00:00:00Z".into(),
                base_branch: "main".into(),
                head_branch: "feat".into(),
                files: review_files,
                file_hashes: HashMap::new(),
            };
            let order = ErOrder {
                version: 1,
                diff_hash: hash.clone(),
                order: vec![],
                groups: HashMap::new(),
            };
            let checklist = ErChecklist {
                version: 1,
                diff_hash: hash,
                items: vec![],
            };

            let mut files = BTreeMap::new();
            files.insert("review.json".into(), serde_json::to_string(&review).unwrap());
            files.insert("order.json".into(), serde_json::to_string(&order).unwrap());
            files.insert(
                "checklist.json".into(),
                serde_json::to_string(&checklist).unwrap(),
            );
            files.insert("summary.md".into(), "# Summary\n".into());
            upload_artifacts_to_dir(&er_dir, SidecarKind::Review, &files).unwrap();
        });
    }

    #[test]
    fn triage_min_json_ok() {
        with_storage_root(|| {
            let er_dir = resolve_managed_root_for_pr_bucket("acme-widgets", 10).er_dir();
            let diff = "diff --git a/t b/t\n+t\n";
            write_diff_tmp(&er_dir, diff);
            let hash = compute_diff_hash(diff);
            let triage = TriageReview {
                version: 1,
                diff_hash: hash,
                diff_scope: "pr".into(),
                created_at: "2026-01-01T00:00:00Z".into(),
                first_impression: "small".into(),
                diff_stats: Default::default(),
                verdict: Default::default(),
                priority_files: vec![],
            };
            let mut files = BTreeMap::new();
            files.insert("triage.json".into(), serde_json::to_string(&triage).unwrap());
            upload_artifacts_to_dir(&er_dir, SidecarKind::Triage, &files).unwrap();
        });
    }

    #[test]
    fn upload_requires_diff_tmp() {
        with_storage_root(|| {
            let er_dir = resolve_managed_root_for_pr_bucket("acme-widgets", 11).er_dir();
            std::fs::create_dir_all(&er_dir).unwrap();
            let mut files = BTreeMap::new();
            files.insert(
                "tour.json".into(),
                r#"{"version":1,"diff_hash":"x"}"#.into(),
            );
            let err = upload_artifacts_to_dir(&er_dir, SidecarKind::Tour, &files)
                .unwrap_err()
                .to_string();
            assert!(err.contains("diff-tmp"), "{err}");
        });
    }
}
