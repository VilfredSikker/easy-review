use super::model::ArenaRun;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
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

    pub fn progress_jsonl(&self) -> PathBuf {
        self.root.join("progress.jsonl")
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

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)
        .with_context(|| format!("rename {} -> {}", tmp.display(), path.display()))
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
        &paths.run_json().with_extension("tmp"),
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProgressEvent {
    RoundStarted {
        round: u8,
        total_rounds: u8,
    },
    ReviewerThinking {
        reviewer_id: String,
        round: u8,
    },
    ReviewerDone {
        reviewer_id: String,
        round: u8,
        findings_count: usize,
    },
    ReviewerFailed {
        reviewer_id: String,
        round: u8,
        reason: String,
    },
    FindingVerdict {
        finding_id: String,
        verdict: String,
        confidence: f32,
    },
    ArbiterStarted {
        arbiter_label: String,
    },
    RunComplete {
        run_id: String,
    },
}

/// Latest reviewer activity derived from `progress.jsonl` (for running UI).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ArenaProgressState {
    pub round: u8,
    pub total_rounds: u8,
    /// Empty during reviewer rounds; `"arbiter"` during final arbiter phase.
    #[serde(default)]
    pub phase: String,
    #[serde(default)]
    pub thinking: Vec<String>,
    #[serde(default)]
    pub done: Vec<String>,
}

pub fn parse_progress_state(paths: &ArenaPaths) -> ArenaProgressState {
    let path = paths.progress_jsonl();
    let Ok(file) = fs::File::open(&path) else {
        return ArenaProgressState::default();
    };
    let mut state = ArenaProgressState::default();
    let mut thinking: HashSet<String> = HashSet::new();
    let mut done: HashSet<String> = HashSet::new();
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(event) = serde_json::from_str::<ProgressEvent>(line) else {
            continue;
        };
        match event {
            ProgressEvent::RoundStarted {
                round,
                total_rounds,
            } => {
                thinking.clear();
                done.clear();
                state.round = round;
                state.total_rounds = total_rounds;
                state.phase.clear();
            }
            ProgressEvent::ArbiterStarted { .. } => {
                thinking.clear();
                done.clear();
                state.phase = "arbiter".into();
            }
            ProgressEvent::ReviewerThinking { reviewer_id, round } => {
                state.round = round;
                thinking.insert(reviewer_id);
            }
            ProgressEvent::ReviewerDone {
                reviewer_id, round, ..
            } => {
                state.round = round;
                thinking.remove(&reviewer_id);
                done.insert(reviewer_id);
            }
            ProgressEvent::ReviewerFailed {
                reviewer_id, round, ..
            } => {
                state.round = round;
                thinking.remove(&reviewer_id);
            }
            ProgressEvent::RunComplete { .. } => {
                thinking.clear();
            }
            ProgressEvent::FindingVerdict { .. } => {}
        }
    }
    state.thinking = thinking.into_iter().collect();
    state.done = done.into_iter().collect();
    state
}

pub fn append_progress_event(paths: &ArenaPaths, event: &ProgressEvent) -> Result<()> {
    paths.ensure_dirs()?;
    let mut record = serde_json::to_vec(event)?;
    record.push(b'\n');
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(paths.progress_jsonl())?;
    // Formatting the record and newline separately lets concurrent appenders
    // place another record between them.
    file.write_all(&record)?;
    Ok(())
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

    #[test]
    fn concurrent_progress_events_remain_complete_json_lines() {
        let dir = tempdir().unwrap();
        let paths = ArenaPaths::for_run(dir.path(), "parallel-events");
        paths.ensure_dirs().unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let handles: Vec<_> = (0..8)
            .map(|worker| {
                let paths = paths.clone();
                let barrier = std::sync::Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    for event in 0..100 {
                        append_progress_event(
                            &paths,
                            &ProgressEvent::ReviewerThinking {
                                reviewer_id: format!("reviewer-{worker}-{event}"),
                                round: 1,
                            },
                        )
                        .unwrap();
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
        let contents = fs::read_to_string(paths.progress_jsonl()).unwrap();
        let mut reviewers = std::collections::HashSet::new();
        for line in contents.lines() {
            let event: ProgressEvent = serde_json::from_str(line)
                .unwrap_or_else(|error| panic!("incomplete progress record: {error}: {line}"));
            if let ProgressEvent::ReviewerThinking { reviewer_id, .. } = event {
                reviewers.insert(reviewer_id);
            }
        }
        assert_eq!(reviewers.len(), 800);
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
        append_progress_event(
            &paths,
            &ProgressEvent::RoundStarted {
                round: 1,
                total_rounds: 3,
            },
        )
        .unwrap();

        let loaded = load_run(&paths).unwrap();
        assert_eq!(loaded.id, run.id);
        assert_eq!(loaded.findings.len(), 1);
        assert!(paths.diff_patch().is_file());
        assert!(paths.progress_jsonl().is_file());
        assert_eq!(
            list_run_ids(&er).unwrap(),
            vec!["run-fixture-001".to_string()]
        );
    }

    #[test]
    fn failed_reviewer_stops_thinking_and_keeps_reason_in_event_log() {
        let dir = tempdir().unwrap();
        let paths = ArenaPaths::for_run(&dir.path().join(".er"), "run-failure-001");
        append_progress_event(
            &paths,
            &ProgressEvent::RoundStarted {
                round: 1,
                total_rounds: 3,
            },
        )
        .unwrap();
        append_progress_event(
            &paths,
            &ProgressEvent::ReviewerThinking {
                reviewer_id: "reviewer-a".into(),
                round: 1,
            },
        )
        .unwrap();
        append_progress_event(
            &paths,
            &ProgressEvent::ReviewerFailed {
                reviewer_id: "reviewer-a".into(),
                round: 1,
                reason: "provider exited".into(),
            },
        )
        .unwrap();

        let state = parse_progress_state(&paths);
        assert!(state.thinking.is_empty());
        let log = fs::read_to_string(paths.progress_jsonl()).unwrap();
        assert!(log.contains("provider exited"));
    }
}
