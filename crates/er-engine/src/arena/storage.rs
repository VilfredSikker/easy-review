use super::model::ArenaRun;
use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct ArenaPaths {
    pub root: PathBuf,
}

impl ArenaPaths {
    pub fn for_run(er_dir: &Path, run_id: &str) -> Self {
        Self {
            root: er_dir.join("arena").join(run_id),
        }
    }

    pub fn run_json(&self) -> PathBuf {
        self.root.join("run.json")
    }

    pub fn diff_patch(&self) -> PathBuf {
        self.root.join("diff.patch")
    }

    pub fn review_md(&self) -> PathBuf {
        self.root.join(crate::ai::review_md::REVIEW_MD_FILE)
    }

    pub fn round_dir(&self, round: u8) -> PathBuf {
        self.root.join(format!("round-{round}"))
    }

    pub fn round_reviewer_json(&self, round: u8, reviewer_id: &str) -> PathBuf {
        self.round_dir(round).join(format!("{reviewer_id}.json"))
    }

    pub fn arbiter_dir(&self) -> PathBuf {
        self.root.join("arbiter")
    }

    pub fn arbiter_output_json(&self) -> PathBuf {
        self.arbiter_dir().join("output.json")
    }

    pub fn ensure_dirs(&self) -> Result<()> {
        fs::create_dir_all(&self.root)?;
        for round in 1..=5 {
            fs::create_dir_all(self.round_dir(round))?;
        }
        fs::create_dir_all(self.arbiter_dir())?;
        Ok(())
    }
}

/// Write `bytes` to `path` through the shared atomic writer.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    er_store::atomic::write_atomic(path, bytes).with_context(|| format!("write {}", path.display()))
}

/// The fixed temporary path builds before the shared writer existed:
/// `with_extension("tmp")` turned `run.json` into `run.tmp`. Recovery refuses
/// to write through a symlink placed where one could be.
pub(crate) fn legacy_tmp_path(path: &Path) -> std::path::PathBuf {
    let mut name = path
        .file_stem()
        .map(std::ffi::OsStr::to_os_string)
        .unwrap_or_default();
    name.push(".tmp");
    path.with_file_name(name)
}

pub fn save_run(paths: &ArenaPaths, run: &ArenaRun) -> Result<()> {
    paths.ensure_dirs()?;
    let json = serde_json::to_string_pretty(run)?;
    write_atomic(&paths.run_json(), json.as_bytes())
}

pub fn load_run(paths: &ArenaPaths) -> Result<ArenaRun> {
    let content = fs::read_to_string(paths.run_json())
        .with_context(|| format!("read {}", paths.run_json().display()))?;
    serde_json::from_str(&content).context("parse run.json")
}

/// Historical run directories can survive a failed start before run.json exists.
/// Only absent or malformed metadata is recoverable. Filesystem failures remain
/// admission failures, and recovery must never write through a symlink.
pub(crate) fn load_historical_run(paths: &ArenaPaths) -> Result<Option<ArenaRun>> {
    for path in [
        &paths.root,
        &paths.run_json(),
        &legacy_tmp_path(&paths.run_json()),
    ] {
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                anyhow::bail!("arena recovery path contains a symlink: {}", path.display());
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).with_context(|| format!("inspect {}", path.display())),
        }
    }
    match load_run(paths) {
        Ok(run) => Ok(Some(run)),
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
        {
            Ok(None)
        }
        Err(error)
            if error
                .downcast_ref::<serde_json::Error>()
                .is_some_and(|error| !error.is_io()) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

pub fn save_diff_patch(paths: &ArenaPaths, patch: &str) -> Result<()> {
    paths.ensure_dirs()?;
    write_atomic(&paths.diff_patch(), patch.as_bytes())
}

pub fn save_round_output(
    paths: &ArenaPaths,
    round: u8,
    reviewer_id: &str,
    value: &serde_json::Value,
) -> Result<()> {
    paths.ensure_dirs()?;
    let path = paths.round_reviewer_json(round, reviewer_id);
    let json = serde_json::to_string_pretty(value)?;
    write_atomic(&path, json.as_bytes())
}

pub fn save_arbiter_output(paths: &ArenaPaths, value: &serde_json::Value) -> Result<()> {
    paths.ensure_dirs()?;
    let json = serde_json::to_string_pretty(value)?;
    write_atomic(&paths.arbiter_output_json(), json.as_bytes())
}

pub fn delete_run_dir(er_dir: &Path, run_id: &str) -> Result<()> {
    let paths = ArenaPaths::for_run(er_dir, run_id);
    if paths.root.is_dir() {
        fs::remove_dir_all(&paths.root)
            .with_context(|| format!("delete arena run {}", paths.root.display()))?;
    }
    Ok(())
}

pub fn list_run_ids(er_dir: &Path) -> Result<Vec<String>> {
    let arena_root = er_dir.join("arena");
    match fs::symlink_metadata(&arena_root) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            anyhow::bail!("arena directory is a symlink: {}", arena_root.display());
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).context("inspect arena directory"),
    }
    let mut ids = Vec::new();
    for entry in fs::read_dir(&arena_root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            if let Some(name) = entry.file_name().to_str() {
                ids.push(name.to_string());
            }
        }
    }
    ids.sort();
    Ok(ids)
}

/// Latest mtime under `.er/arena/` for poll fallback.
pub fn latest_arena_mtime(er_dir: &Path) -> Option<std::time::SystemTime> {
    let arena_root = er_dir.join("arena");
    if !arena_root.is_dir() {
        return None;
    }
    let mut latest: Option<std::time::SystemTime> = None;
    let stack = vec![arena_root];
    walk_mtime(stack, &mut latest);
    latest
}

fn walk_mtime(mut dirs: Vec<PathBuf>, latest: &mut Option<std::time::SystemTime>) {
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            } else if let Ok(meta) = entry.metadata() {
                if let Ok(mtime) = meta.modified() {
                    *latest = Some(match latest {
                        Some(prev) if mtime > *prev => mtime,
                        Some(prev) => *prev,
                        None => mtime,
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::RiskLevel;
    use crate::arena::model::*;
    use std::collections::BTreeMap;
    use tempfile::tempdir;

    /// Two writers saving one artifact at once must both finish: a cancel and
    /// the supervisor write `run.json` concurrently.
    #[test]
    fn concurrent_atomic_writes_to_one_artifact_both_succeed() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("run.json");
        for _ in 0..64 {
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
            let handles: Vec<_> = (0..2)
                .map(|writer| {
                    let path = path.clone();
                    let barrier = std::sync::Arc::clone(&barrier);
                    std::thread::spawn(move || {
                        barrier.wait();
                        write_atomic(&path, format!("writer-{writer}").as_bytes())
                    })
                })
                .collect();
            for handle in handles {
                handle.join().unwrap().unwrap();
            }
            let contents = std::fs::read_to_string(&path).unwrap();
            assert!(
                contents == "writer-0" || contents == "writer-1",
                "{contents}"
            );
        }
        let leftovers: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    fn sample_run(id: &str) -> ArenaRun {
        ArenaRun {
            id: id.to_string(),
            title: Some("fixture".into()),
            branch_ref: "feature/x".into(),
            base_branch: "main".into(),
            scope: ArenaScope::Branch,
            diff_hash: "abc".into(),
            created_at: "2026-05-27T00:00:00Z".into(),
            completed_at: None,
            status: RunStatus::Complete,
            config: ArenaConfig {
                reviewers: vec![ReviewerRef {
                    provider_id: "anthropic".into(),
                    model_id: "sonnet".into(),
                    agent_kind: None,
                }],
                rounds: 3,
                arbiter: ReviewerRef {
                    provider_id: "anthropic".into(),
                    model_id: "opus".into(),
                    agent_kind: None,
                },
                auto_accept_threshold: 0.75,
                scope: ArenaScope::Branch,
                files: None,
                run_kind: ArenaRunKind::Models,
                agent_kind: None,
                effort: None,
            },
            reviewers: vec![],
            accepted_finding_ids: vec![],
            findings: vec![ArenaFinding {
                id: "deadbeef".into(),
                file: "src/a.rs".into(),
                line: Some(1),
                title: "t".into(),
                body: "b".into(),
                severity_by_round: BTreeMap::from([(1, RiskLevel::High)]),
                raised_by: vec!["r1".into()],
                verdict: Verdict::Kept,
                confidence: 0.9,
                rationale: "ok".into(),
                rounds: vec![],
                merge_candidates: vec![],
                merged_children: vec![],
                evidence: vec![],
                override_: None,
                accepted_at: None,
            }],
            cost_estimate: CostEstimate {
                tokens_in: 0,
                tokens_out: 0,
                usd: 0.0,
            },
        }
    }

    #[test]
    fn round_trip_run_fixture() {
        let dir = tempdir().unwrap();
        let er = dir.path().join(".er");
        fs::create_dir_all(&er).unwrap();
        let paths = ArenaPaths::for_run(&er, "run-fixture-001");
        let run = sample_run("run-fixture-001");
        save_run(&paths, &run).unwrap();
        save_diff_patch(&paths, "diff --git a/foo\n").unwrap();

        let loaded = load_run(&paths).unwrap();
        assert_eq!(loaded.id, run.id);
        assert_eq!(loaded.findings.len(), 1);
        assert!(paths.diff_patch().is_file());
        assert_eq!(
            list_run_ids(&er).unwrap(),
            vec!["run-fixture-001".to_string()]
        );
    }
}
