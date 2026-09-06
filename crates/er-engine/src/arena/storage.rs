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

pub fn save_diff_patch(paths: &ArenaPaths, patch: &str) -> Result<()> {
    paths.ensure_dirs()?;
    write_atomic(&paths.diff_patch(), patch.as_bytes())
}

#[allow(dead_code)]
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
    let line = serde_json::to_string(event)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(paths.progress_jsonl())?;
    writeln!(file, "{line}")?;
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
    if !arena_root.is_dir() {
        return Ok(Vec::new());
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

/// Latest mtime under `.er/arena/` for poll fallback (§B2).
#[allow(dead_code)]
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

    fn ev(event: &ProgressEvent) -> String {
        serde_json::to_string(event).unwrap()
    }

    fn write_progress(paths: &ArenaPaths, lines: &[String]) {
        paths.ensure_dirs().unwrap();
        fs::write(paths.progress_jsonl(), lines.join("\n")).unwrap();
    }

    fn sorted(mut v: Vec<String>) -> Vec<String> {
        v.sort();
        v
    }

    #[test]
    fn parse_progress_state_is_default_when_progress_file_missing() {
        let dir = tempdir().unwrap();
        let paths = ArenaPaths::for_run(dir.path(), "run-never-started");
        let state = parse_progress_state(&paths);
        assert_eq!(state.round, 0);
        assert_eq!(state.total_rounds, 0);
        assert!(state.phase.is_empty());
        assert!(state.thinking.is_empty());
        assert!(state.done.is_empty());
    }

    #[test]
    fn parse_progress_state_skips_blank_and_unparseable_lines() {
        let dir = tempdir().unwrap();
        let paths = ArenaPaths::for_run(dir.path(), "run-noise");
        write_progress(
            &paths,
            &[
                String::new(),
                "   ".to_string(),
                "{ not json at all".to_string(),
                r#"{"type":"from_a_newer_version"}"#.to_string(),
                ev(&ProgressEvent::RoundStarted {
                    round: 2,
                    total_rounds: 3,
                }),
                ev(&ProgressEvent::ReviewerThinking {
                    reviewer_id: "r1".into(),
                    round: 2,
                }),
            ],
        );
        let state = parse_progress_state(&paths);
        assert_eq!(
            state.round, 2,
            "events following junk lines must still be applied"
        );
        assert_eq!(state.total_rounds, 3);
        assert_eq!(state.thinking, vec!["r1".to_string()]);
    }

    #[test]
    fn parse_progress_state_moves_reviewer_from_thinking_to_done() {
        let dir = tempdir().unwrap();
        let paths = ArenaPaths::for_run(dir.path(), "run-progress");
        write_progress(
            &paths,
            &[
                ev(&ProgressEvent::RoundStarted {
                    round: 1,
                    total_rounds: 3,
                }),
                ev(&ProgressEvent::ReviewerThinking {
                    reviewer_id: "r1".into(),
                    round: 1,
                }),
                ev(&ProgressEvent::ReviewerThinking {
                    reviewer_id: "r2".into(),
                    round: 1,
                }),
                ev(&ProgressEvent::ReviewerDone {
                    reviewer_id: "r1".into(),
                    round: 1,
                    findings_count: 4,
                }),
                ev(&ProgressEvent::FindingVerdict {
                    finding_id: "f1".into(),
                    verdict: "kept".into(),
                    confidence: 0.9,
                }),
            ],
        );
        let state = parse_progress_state(&paths);
        assert_eq!(state.round, 1);
        assert_eq!(state.total_rounds, 3);
        assert_eq!(
            state.thinking,
            vec!["r2".to_string()],
            "a done reviewer must stop being reported as thinking"
        );
        assert_eq!(state.done, vec!["r1".to_string()]);
        assert!(
            state.phase.is_empty(),
            "reviewer rounds leave phase empty (arbiter only)"
        );
    }

    #[test]
    fn parse_progress_state_round_start_clears_previous_round_activity() {
        let dir = tempdir().unwrap();
        let paths = ArenaPaths::for_run(dir.path(), "run-rounds");
        write_progress(
            &paths,
            &[
                ev(&ProgressEvent::RoundStarted {
                    round: 1,
                    total_rounds: 3,
                }),
                ev(&ProgressEvent::ReviewerThinking {
                    reviewer_id: "r1".into(),
                    round: 1,
                }),
                ev(&ProgressEvent::ReviewerDone {
                    reviewer_id: "r2".into(),
                    round: 1,
                    findings_count: 0,
                }),
                ev(&ProgressEvent::RoundStarted {
                    round: 2,
                    total_rounds: 3,
                }),
                ev(&ProgressEvent::ReviewerThinking {
                    reviewer_id: "r3".into(),
                    round: 2,
                }),
            ],
        );
        let state = parse_progress_state(&paths);
        assert_eq!(state.round, 2);
        assert_eq!(
            state.thinking,
            vec!["r3".to_string()],
            "round 1 thinking must not leak into round 2"
        );
        assert!(
            state.done.is_empty(),
            "round 1 done must not leak into round 2"
        );
    }

    #[test]
    fn parse_progress_state_new_round_leaves_the_arbiter_phase() {
        let dir = tempdir().unwrap();
        let paths = ArenaPaths::for_run(dir.path(), "run-rerun");
        write_progress(
            &paths,
            &[
                ev(&ProgressEvent::RoundStarted {
                    round: 2,
                    total_rounds: 3,
                }),
                ev(&ProgressEvent::ArbiterStarted {
                    arbiter_label: "Opus".into(),
                }),
                ev(&ProgressEvent::RoundStarted {
                    round: 3,
                    total_rounds: 3,
                }),
                ev(&ProgressEvent::ReviewerThinking {
                    reviewer_id: "r1".into(),
                    round: 3,
                }),
            ],
        );
        let state = parse_progress_state(&paths);
        assert!(
            state.phase.is_empty(),
            "a round starting after the arbiter phase must drop back to reviewer phase"
        );
        assert_eq!(state.round, 3);
        assert_eq!(state.thinking, vec!["r1".to_string()]);
    }

    #[test]
    fn parse_progress_state_arbiter_phase_clears_reviewer_activity() {
        let dir = tempdir().unwrap();
        let paths = ArenaPaths::for_run(dir.path(), "run-arbiter");
        write_progress(
            &paths,
            &[
                ev(&ProgressEvent::RoundStarted {
                    round: 3,
                    total_rounds: 3,
                }),
                ev(&ProgressEvent::ReviewerThinking {
                    reviewer_id: "r1".into(),
                    round: 3,
                }),
                ev(&ProgressEvent::ReviewerDone {
                    reviewer_id: "r2".into(),
                    round: 3,
                    findings_count: 2,
                }),
                ev(&ProgressEvent::ArbiterStarted {
                    arbiter_label: "Opus".into(),
                }),
            ],
        );
        let state = parse_progress_state(&paths);
        assert_eq!(state.phase, "arbiter");
        assert_eq!(state.round, 3, "arbiter phase keeps the last round number");
        assert!(state.thinking.is_empty());
        assert!(state.done.is_empty());
    }

    #[test]
    fn parse_progress_state_run_complete_clears_thinking_but_keeps_done() {
        let dir = tempdir().unwrap();
        let paths = ArenaPaths::for_run(dir.path(), "run-complete");
        write_progress(
            &paths,
            &[
                ev(&ProgressEvent::RoundStarted {
                    round: 3,
                    total_rounds: 3,
                }),
                ev(&ProgressEvent::ReviewerThinking {
                    reviewer_id: "r1".into(),
                    round: 3,
                }),
                ev(&ProgressEvent::ReviewerDone {
                    reviewer_id: "r2".into(),
                    round: 3,
                    findings_count: 1,
                }),
                ev(&ProgressEvent::ReviewerDone {
                    reviewer_id: "r3".into(),
                    round: 3,
                    findings_count: 0,
                }),
                ev(&ProgressEvent::RunComplete {
                    run_id: "run-complete".into(),
                }),
            ],
        );
        let state = parse_progress_state(&paths);
        assert!(
            state.thinking.is_empty(),
            "a finished run must not leave a reviewer spinning"
        );
        assert_eq!(
            sorted(state.done),
            vec!["r2".to_string(), "r3".to_string()],
            "completed reviewers stay listed after the run finishes"
        );
    }

    #[test]
    fn latest_arena_mtime_is_none_without_arena_dir() {
        let dir = tempdir().unwrap();
        assert!(latest_arena_mtime(dir.path()).is_none());
    }

    #[test]
    fn latest_arena_mtime_is_none_for_empty_arena_dir() {
        let dir = tempdir().unwrap();
        let er = dir.path().join(".er");
        fs::create_dir_all(er.join("arena")).unwrap();
        assert!(
            latest_arena_mtime(&er).is_none(),
            "an arena dir holding no files has no mtime"
        );
    }

    #[test]
    fn latest_arena_mtime_finds_newest_file_in_a_nested_round_dir() {
        let dir = tempdir().unwrap();
        let er = dir.path().join(".er");
        let nested = er.join("arena").join("run-1").join("round-1");
        fs::create_dir_all(&nested).unwrap();

        let shallow = er.join("arena").join("shallow.txt");
        fs::write(&shallow, b"old").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let deep = nested.join("reviewer.json");
        fs::write(&deep, b"new").unwrap();

        let shallow_mtime = fs::metadata(&shallow).unwrap().modified().unwrap();
        let deep_mtime = fs::metadata(&deep).unwrap().modified().unwrap();
        assert!(
            deep_mtime > shallow_mtime,
            "fixture requires a strictly newer nested file"
        );

        assert_eq!(
            latest_arena_mtime(&er),
            Some(deep_mtime),
            "the walk must descend into round dirs, not stop at the arena root"
        );
    }

    #[test]
    fn walk_mtime_skips_unreadable_dirs_instead_of_aborting() {
        let dir = tempdir().unwrap();
        let missing = dir.path().join("does-not-exist");
        let readable = dir.path().join("readable");
        fs::create_dir_all(&readable).unwrap();
        let file = readable.join("a.txt");
        fs::write(&file, b"x").unwrap();
        let expected = fs::metadata(&file).unwrap().modified().unwrap();

        // The stack is popped LIFO, so `missing` is visited FIRST — the readable dir is
        // only reached if the failed `read_dir` is `continue`d rather than ending the walk.
        let mut latest = None;
        walk_mtime(vec![readable, missing], &mut latest);
        assert_eq!(
            latest,
            Some(expected),
            "an unreadable dir must be skipped, leaving the rest of the stack walked"
        );
    }

    #[test]
    fn walk_mtime_keeps_the_newest_mtime_when_an_older_file_is_visited_later() {
        let dir = tempdir().unwrap();
        let older_dir = dir.path().join("older");
        let newer_dir = dir.path().join("newer");
        fs::create_dir_all(&older_dir).unwrap();
        fs::create_dir_all(&newer_dir).unwrap();

        let older = older_dir.join("a.txt");
        fs::write(&older, b"old").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let newer = newer_dir.join("b.txt");
        fs::write(&newer, b"new").unwrap();
        let older_mtime = fs::metadata(&older).unwrap().modified().unwrap();
        let newer_mtime = fs::metadata(&newer).unwrap().modified().unwrap();
        assert!(
            newer_mtime > older_mtime,
            "fixture requires a strictly newer second file, or the keep-arm is untested"
        );

        // The stack is popped LIFO, so `newer` is recorded first and `older` must not
        // overwrite it.
        let mut latest = None;
        walk_mtime(vec![older_dir, newer_dir], &mut latest);
        assert_eq!(
            latest,
            Some(newer_mtime),
            "a later-visited older file must not replace the newest mtime seen"
        );
    }
}
