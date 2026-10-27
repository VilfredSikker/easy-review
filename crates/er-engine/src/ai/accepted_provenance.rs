//! Restores host-owned provenance for findings imported from accepted Arena runs.
//!
//! This module deliberately projects only the persisted fields it needs. It is
//! usable by headless builds and does not depend on Arena or application types.

use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fs,
    path::{Component, Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::SystemTime,
};

const MAX_RUN_BYTES: u64 = 10 * 1024 * 1024;
const CACHE_CAPACITY: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AcceptedProvenance {
    pub providers: Vec<String>,
    pub raisers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewRowIdentity {
    pub id: String,
    pub file: String,
    pub line: Option<usize>,
}

#[derive(Debug, Clone, Default)]
pub struct ProvenanceRead {
    /// Indexed by the exact imported review row ID.
    pub rows: BTreeMap<String, AcceptedProvenance>,
    /// Conflicting host records are omitted and named here for diagnostics.
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RunProjection {
    id: String,
    diff_hash: String,
    #[serde(default)]
    config: ConfigProjection,
    #[serde(default)]
    reviewers: Vec<ReviewerProjection>,
    #[serde(default)]
    findings: Vec<FindingProjection>,
    #[serde(default)]
    accepted_finding_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ConfigProjection {
    #[serde(default)]
    agent_kind: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ReviewerProjection {
    id: String,
    #[serde(default)]
    provider_id: String,
    #[serde(default)]
    agent_kind: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct FindingProjection {
    id: String,
    file: String,
    #[serde(default)]
    line: Option<usize>,
    #[serde(default)]
    raised_by: Vec<String>,
    #[serde(default)]
    accepted_at: Option<String>,
}

#[derive(Debug, Clone)]
struct CachedRun {
    modified: Option<SystemTime>,
    size: u64,
    run: Option<RunProjection>,
}

static CACHE: OnceLock<Mutex<HashMap<PathBuf, CachedRun>>> = OnceLock::new();

/// Restore provider and normalized lens metadata for accepted imported rows.
/// Unreadable or invalid run files are intentionally ignored: review loading
/// must continue when historical provenance has been removed or damaged.
pub fn accepted_provenance_for_rows(
    er_dir: &Path,
    review_hash: &str,
    rows: &[ReviewRowIdentity],
) -> ProvenanceRead {
    let mut result = ProvenanceRead::default();
    let requested: HashMap<_, _> = rows.iter().map(|r| (r.id.as_str(), r)).collect();
    if requested.is_empty() || review_hash.is_empty() {
        return result;
    }
    let arena_dir = er_dir.join("arena");
    let Ok(arena_meta) = fs::symlink_metadata(&arena_dir) else {
        return result;
    };
    if arena_meta.file_type().is_symlink() || !arena_meta.is_dir() {
        return result;
    }
    let Ok(entries) = fs::read_dir(&arena_dir) else {
        return result;
    };
    let mut candidates: HashMap<String, (ReviewRowIdentity, AcceptedProvenance)> = HashMap::new();
    let mut conflicts = HashSet::new();

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let Some(run_id) = name.to_str() else {
            continue;
        };
        if !safe_single_component(run_id) {
            continue;
        }
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() || !meta.is_dir() {
            continue;
        }
        let Some(run) = cached_run(&path) else {
            continue;
        };
        if run.id != run_id || run.diff_hash != review_hash {
            continue;
        }

        let reviewer_by_id: HashMap<&str, &ReviewerProjection> =
            run.reviewers.iter().map(|r| (r.id.as_str(), r)).collect();
        let accepted: HashSet<&str> = run
            .accepted_finding_ids
            .iter()
            .map(String::as_str)
            .collect();
        for finding in &run.findings {
            if !accepted.contains(finding.id.as_str())
                || finding.accepted_at.as_deref().is_none_or(str::is_empty)
            {
                continue;
            }
            let imported_id = format!("arena-{}-{}", run.id, finding.id);
            let Some(row) = requested.get(imported_id.as_str()) else {
                continue;
            };
            if row.file != finding.file || row.line != finding.line {
                continue;
            }

            let provenance = provenance_for(finding, &run, &reviewer_by_id);
            let identity = (**row).clone();
            match candidates.get(&imported_id) {
                Some((_, prior)) if prior != &provenance => {
                    conflicts.insert(imported_id);
                }
                Some(_) => {}
                None => {
                    candidates.insert(imported_id, (identity, provenance));
                }
            }
        }
    }
    for id in conflicts {
        candidates.remove(&id);
        result
            .diagnostics
            .push(format!("conflicting accepted-run provenance for {id}"));
    }
    result.rows = candidates.into_iter().map(|(id, (_, p))| (id, p)).collect();
    result
}

/// Call after acceptance or deletion to discard the bucket's parsed run cache.
pub fn invalidate_accepted_provenance(er_dir: &Path) {
    if let Some(cache) = CACHE.get() {
        if let Ok(mut cache) = cache.lock() {
            // Cache keys use canonical run.json paths. Resolve the same way,
            // including when the arena directory itself was just removed.
            let bucket = fs::canonicalize(er_dir.join("arena"))
                .or_else(|_| fs::canonicalize(er_dir).map(|path| path.join("arena")));
            let Ok(bucket) = bucket else { return };
            cache.retain(|path, _| !path.starts_with(&bucket));
        }
    }
}

fn cached_run(run_dir: &Path) -> Option<RunProjection> {
    let file = run_dir.join("run.json");
    if !safe_single_component(run_dir.file_name()?.to_str()?) {
        return None;
    }
    let dir_meta = fs::symlink_metadata(run_dir).ok()?;
    if dir_meta.file_type().is_symlink() || !dir_meta.is_dir() {
        return None;
    }
    let meta = fs::symlink_metadata(&file).ok()?;
    if meta.file_type().is_symlink() || !meta.is_file() || meta.len() > MAX_RUN_BYTES {
        return None;
    }
    let path = fs::canonicalize(&file).ok()?;
    let canonical_dir = fs::canonicalize(run_dir).ok()?;
    if path.parent() != Some(canonical_dir.as_path()) {
        return None;
    }
    let modified = meta.modified().ok();
    let size = meta.len();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut entries) = cache.lock() {
        if let Some(hit) = entries.get(&path) {
            if hit.modified == modified && hit.size == size {
                return hit.run.clone();
            }
        }
        let parsed = (|| {
            let bytes = fs::read(&path).ok()?;
            if bytes.len() as u64 > MAX_RUN_BYTES {
                return None;
            }
            serde_json::from_slice(&bytes).ok()
        })();
        if entries.len() >= CACHE_CAPACITY {
            if let Some(oldest) = entries.keys().next().cloned() {
                entries.remove(&oldest);
            }
        }
        entries.insert(
            path,
            CachedRun {
                modified,
                size,
                run: parsed.clone(),
            },
        );
        return parsed;
    }
    None
}

fn provenance_for(
    finding: &FindingProjection,
    run: &RunProjection,
    reviewers: &HashMap<&str, &ReviewerProjection>,
) -> AcceptedProvenance {
    let mut providers = BTreeSet::<String>::new();
    let mut raisers = BTreeSet::<String>::new();
    let run_lens = run
        .config
        .agent_kind
        .as_deref()
        .and_then(normalize_agent_kind);
    // Older persisted runs did not retain reviewer records. Their one
    // run-wide kind is the only producer metadata available, even when the
    // finding predates raised_by.
    if reviewers.is_empty() {
        if let Some(lens) = run_lens {
            raisers.insert(lens);
        }
        return AcceptedProvenance {
            providers: Vec::new(),
            raisers: raisers.into_iter().collect(),
        };
    }
    for reviewer_id in &finding.raised_by {
        if let Some(reviewer) = reviewers.get(reviewer_id.as_str()) {
            if !reviewer.provider_id.trim().is_empty() {
                providers.insert(reviewer.provider_id.clone());
            }
            if let Some(lens) = reviewer
                .agent_kind
                .as_deref()
                .and_then(normalize_agent_kind)
            {
                raisers.insert(lens);
            } else if reviewer.agent_kind.is_none() {
                if let Some(lens) = run_lens.clone() {
                    raisers.insert(lens);
                }
            }
        }
    }
    AcceptedProvenance {
        providers: providers.into_iter().collect(),
        raisers: raisers.into_iter().collect(),
    }
}

fn normalize_agent_kind(kind: &str) -> Option<String> {
    if kind == "general" {
        Some("general".into())
    } else {
        kind.strip_prefix("expert:")
            .filter(|lens| !lens.is_empty())
            .map(str::to_owned)
    }
}

fn safe_single_component(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && !value.starts_with('.')
        && Path::new(value)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}

#[cfg(all(test, feature = "ui"))]
mod tests {
    use super::*;
    use crate::arena::{
        ArenaConfig, ArenaFinding, ArenaRun, ArenaRunKind, ArenaScope, CostEstimate, Reviewer,
        ReviewerKind, ReviewerRunStatus, RunStatus, Verdict,
    };
    use std::collections::BTreeMap;

    #[expect(clippy::unwrap_used, reason = "test fixture setup should fail fast")]
    fn fixture(dir: &Path, id: &str, diff_hash: &str, accepted: bool) {
        let run = ArenaRun {
            id: id.into(),
            title: None,
            branch_ref: "main".into(),
            base_branch: "main".into(),
            scope: ArenaScope::Branch,
            diff_hash: diff_hash.into(),
            created_at: "now".into(),
            completed_at: None,
            status: RunStatus::Complete,
            config: ArenaConfig {
                reviewers: vec![],
                rounds: 1,
                arbiter: reviewer_ref(),
                auto_accept_threshold: 1.0,
                scope: ArenaScope::Branch,
                files: None,
                run_kind: ArenaRunKind::Seeded,
                agent_kind: None,
                effort: None,
            },
            reviewers: vec![
                reviewer("reviewer-general", "openai", Some("general")),
                reviewer("reviewer-security", "anthropic", Some("expert:security")),
            ],
            findings: vec![ArenaFinding {
                id: "finding-with-hyphen".into(),
                file: "src/a.rs".into(),
                line: Some(12),
                title: "title".into(),
                body: "body".into(),
                severity_by_round: BTreeMap::new(),
                raised_by: vec!["reviewer-general".into(), "reviewer-security".into()],
                verdict: Verdict::Kept,
                confidence: 0.9,
                rationale: "ok".into(),
                rounds: vec![],
                merge_candidates: vec![],
                merged_children: vec![],
                evidence: vec![],
                override_: None,
                accepted_at: accepted.then(|| "now".into()),
            }],
            cost_estimate: CostEstimate {
                tokens_in: 1,
                tokens_out: 1,
                usd: 0.0,
            },
            accepted_finding_ids: if accepted {
                vec!["finding-with-hyphen".into()]
            } else {
                vec![]
            },
        };
        let path = dir.join("arena").join(id);
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("run.json"), serde_json::to_vec(&run).unwrap()).unwrap();
    }
    fn reviewer_ref() -> crate::arena::ReviewerRef {
        crate::arena::ReviewerRef {
            provider_id: "openai".into(),
            model_id: "m".into(),
            agent_kind: None,
        }
    }
    fn reviewer(id: &str, provider: &str, kind: Option<&str>) -> Reviewer {
        Reviewer {
            id: id.into(),
            name: id.into(),
            kind: ReviewerKind::Agent,
            provider_id: provider.into(),
            model_id: "m".into(),
            system_prompt: "".into(),
            color: "".into(),
            icon: "".into(),
            tagline: "".into(),
            cost_per_1k_in: 0.0,
            cost_per_1k_out: 0.0,
            avg_latency_ms: 0,
            status: ReviewerRunStatus::Ok,
            agent_kind: kind.map(str::to_owned),
        }
    }
    fn row() -> ReviewRowIdentity {
        ReviewRowIdentity {
            id: "arena-run-finding-with-hyphen".into(),
            file: "src/a.rs".into(),
            line: Some(12),
        }
    }

    #[test]
    fn reads_real_arena_run_shape_and_requires_acceptance_and_matching_hash() {
        let temp = tempfile::tempdir().unwrap();
        fixture(temp.path(), "run", "hash", true);
        let row = row();
        let got = accepted_provenance_for_rows(temp.path(), "hash", std::slice::from_ref(&row));
        assert_eq!(
            got.rows["arena-run-finding-with-hyphen"].providers,
            ["anthropic", "openai"]
        );
        assert!(
            accepted_provenance_for_rows(temp.path(), "wrong", std::slice::from_ref(&row))
                .rows
                .is_empty()
        );
    }

    #[test]
    fn malformed_missing_and_symlink_runs_are_ignored() {
        let temp = tempfile::tempdir().unwrap();
        fixture(temp.path(), "run", "hash", true);
        let row = ReviewRowIdentity {
            id: "arena-run-finding-with-hyphen".into(),
            file: "src/a.rs".into(),
            line: Some(12),
        };
        fs::write(temp.path().join("arena/run/run.json"), b"{").unwrap();
        assert!(
            accepted_provenance_for_rows(temp.path(), "hash", std::slice::from_ref(&row))
                .rows
                .is_empty()
        );
        let target = temp.path().join("outside");
        fs::create_dir_all(&target).unwrap();
        let _ = std::os::unix::fs::symlink(target, temp.path().join("arena/linked"));
        fs::remove_dir_all(temp.path().join("arena/run")).unwrap();
        assert!(
            accepted_provenance_for_rows(temp.path(), "hash", std::slice::from_ref(&row))
                .rows
                .is_empty()
        );
    }

    #[test]
    fn cache_observes_file_replacement_and_explicit_invalidation() {
        let temp = tempfile::tempdir().unwrap();
        fixture(temp.path(), "run", "hash", true);
        let row = ReviewRowIdentity {
            id: "arena-run-finding-with-hyphen".into(),
            file: "src/a.rs".into(),
            line: Some(12),
        };
        assert_eq!(
            accepted_provenance_for_rows(temp.path(), "hash", std::slice::from_ref(&row))
                .rows
                .len(),
            1
        );
        fs::remove_dir_all(temp.path().join("arena/run")).unwrap();
        invalidate_accepted_provenance(temp.path());
        assert!(accepted_provenance_for_rows(temp.path(), "hash", &[row])
            .rows
            .is_empty());
    }
}

#[cfg(test)]
mod headless_tests {
    use super::*;
    use serde_json::json;

    fn write_run(er_dir: &Path, provider: &str) -> PathBuf {
        let run_dir = er_dir.join("arena/run-1");
        fs::create_dir_all(&run_dir).unwrap();
        fs::write(
            run_dir.join("run.json"),
            serde_json::to_vec(&json!({
                "id": "run-1",
                "diff_hash": "hash",
                "config": { "agent_kind": "expert:security" },
                "reviewers": [{ "id": "reviewer-1", "provider_id": provider, "agent_kind": "expert:security" }],
                "findings": [{ "id": "finding-x", "file": "src/a.rs", "line": 9, "raised_by": ["reviewer-1"], "accepted_at": "now" }],
                "accepted_finding_ids": ["finding-x"]
            }))
            .unwrap(),
        )
        .unwrap();
        run_dir.join("run.json")
    }

    fn row() -> ReviewRowIdentity {
        ReviewRowIdentity {
            id: "arena-run-1-finding-x".into(),
            file: "src/a.rs".into(),
            line: Some(9),
        }
    }

    #[test]
    fn headless_projection_requires_acceptance_hash_and_exact_row_identity() {
        let temp = tempfile::tempdir().unwrap();
        let run_path = write_run(temp.path(), "provider-a");
        let row = row();
        let result = accepted_provenance_for_rows(temp.path(), "hash", std::slice::from_ref(&row));
        assert_eq!(result.rows[&row.id].providers, ["provider-a"]);
        assert_eq!(result.rows[&row.id].raisers, ["security"]);
        assert!(
            accepted_provenance_for_rows(temp.path(), "other", std::slice::from_ref(&row))
                .rows
                .is_empty()
        );

        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&run_path).unwrap()).unwrap();
        value["accepted_finding_ids"] = json!([]);
        fs::write(&run_path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(
            accepted_provenance_for_rows(temp.path(), "hash", std::slice::from_ref(&row))
                .rows
                .is_empty()
        );

        value["accepted_finding_ids"] = json!(["finding-x"]);
        fs::write(&run_path, serde_json::to_vec(&value).unwrap()).unwrap();
        let mut wrong_location = row;
        wrong_location.line = Some(10);
        assert!(
            accepted_provenance_for_rows(temp.path(), "hash", &[wrong_location])
                .rows
                .is_empty()
        );
    }

    #[test]
    fn legacy_run_kind_fallback_requires_absent_reviewer_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let run_path = write_run(temp.path(), "provider-a");
        let row = row();
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&run_path).unwrap()).unwrap();
        value["reviewers"] = json!([]);
        value["findings"][0]["raised_by"] = json!([]);
        fs::write(&run_path, serde_json::to_vec(&value).unwrap()).unwrap();
        let legacy = accepted_provenance_for_rows(temp.path(), "hash", std::slice::from_ref(&row));
        assert_eq!(legacy.rows[&row.id].raisers, ["security"]);
        assert!(legacy.rows[&row.id].providers.is_empty());

        value["reviewers"] = json!([{ "id": "known", "provider_id": "provider-a" }]);
        value["findings"][0]["raised_by"] = json!(["unknown"]);
        fs::write(&run_path, serde_json::to_vec(&value).unwrap()).unwrap();
        let unknown = accepted_provenance_for_rows(temp.path(), "hash", std::slice::from_ref(&row));
        assert!(unknown.rows[&row.id].raisers.is_empty());
        assert!(unknown.rows[&row.id].providers.is_empty());
    }

    #[test]
    fn invalidation_evicts_canonical_cache_entries_without_deleting_run_files() {
        let temp = tempfile::tempdir().unwrap();
        let run_path = write_run(temp.path(), "provider-a");
        let row = row();
        assert_eq!(
            accepted_provenance_for_rows(temp.path(), "hash", std::slice::from_ref(&row))
                .rows
                .len(),
            1
        );

        let alias = temp.path().with_extension("alias");
        #[cfg(unix)]
        std::os::unix::fs::symlink(temp.path(), &alias).unwrap();
        let alias_bucket = if alias.exists() {
            alias.clone()
        } else {
            temp.path().to_path_buf()
        };
        let canonical_run = fs::canonicalize(&run_path).unwrap();
        assert!(CACHE
            .get()
            .unwrap()
            .lock()
            .unwrap()
            .contains_key(&canonical_run));
        invalidate_accepted_provenance(&alias_bucket);
        assert!(!CACHE
            .get()
            .unwrap()
            .lock()
            .unwrap()
            .contains_key(&canonical_run));
        assert!(run_path.is_file());
        assert_eq!(
            accepted_provenance_for_rows(temp.path(), "hash", &[row])
                .rows
                .len(),
            1
        );
        let _ = fs::remove_file(alias);
    }
}
