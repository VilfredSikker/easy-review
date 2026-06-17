use crate::arena::{
    build_arena_diff_preview, build_snapshot_with_config, delete_run_dir,
    import_arena_findings_to_review, load_run, parse_progress_state, reconcile_stale_runs,
    save_run, scope_git_mode, start_arena_batch, start_arena_run, ArenaBatchStartParams,
    ArenaDiffPreview, ArenaPaths, ArenaProgressState, ArenaRegistry, ArenaRunSnapshot, ArenaScope,
    ArenaStartParams, HumanOverride, ReviewerRef, Verdict,
};
use crate::git::filter_raw_diff_by_paths;
use anyhow::Result;
use std::path::Path;
use std::sync::Arc;

use super::App;
use super::TabState;

/// Load a run's summary, memoized by `run.json` mtime+size.
///
/// `App::background_arena_runs` runs inside the desktop's per-poll
/// `build_snapshot`, so re-reading + parsing every active run's `run.json`
/// each tick would violate the "per-poll disk reads are mtime-cached"
/// convention. When the file is unchanged we reuse the parsed summary after a
/// single `stat`; a changed mtime/size (new finding, completion) re-parses.
/// Status is intentionally not authoritative here — callers overlay the live
/// registry status, which can lead the on-disk value.
fn load_run_summary_cached(paths: &ArenaPaths) -> Option<crate::arena::ArenaRunSummary> {
    type Key = Option<(std::time::SystemTime, u64)>;
    type SummaryCache =
        std::collections::HashMap<std::path::PathBuf, (Key, crate::arena::ArenaRunSummary)>;
    static CACHE: std::sync::LazyLock<std::sync::Mutex<SummaryCache>> =
        std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

    let json = paths.run_json();
    let key: Key = std::fs::metadata(&json)
        .ok()
        .and_then(|m| m.modified().ok().map(|t| (t, m.len())));

    // Fast path: reuse the cached summary if run.json is unchanged. The lock is
    // released before the disk read below so a slow read on one run can't block
    // the cache for the others.
    if let Ok(cache) = CACHE.lock() {
        if let Some((cached_key, summary)) = cache.get(&json) {
            if *cached_key == key {
                return Some(summary.clone());
            }
        }
    }
    // Slow path: parse without holding the lock, then record. A concurrent miss
    // may parse twice (benign); the per-poll caller is single-threaded anyway.
    let summary = load_run(paths).ok()?.summary();
    if let Ok(mut cache) = CACHE.lock() {
        // Bounded: one entry per run.json path; drop everything if it grows.
        if cache.len() > 256 {
            cache.clear();
        }
        cache.insert(json, (key, summary.clone()));
    }
    Some(summary)
}

impl TabState {
    /// Arena uses the same diff as AI review ([`raw_diff_for_review`]); optional path filter.
    pub fn raw_diff_for_arena(
        &self,
        scope: ArenaScope,
        files: Option<&[String]>,
    ) -> Result<String> {
        let scope_str = scope_git_mode(scope);
        let mut raw = self.raw_diff_for_review(scope_str)?;
        if let Some(paths) = files {
            if !paths.is_empty() {
                raw = filter_raw_diff_by_paths(&raw, paths);
            }
        }
        Ok(raw)
    }
}

impl App {
    pub fn init_arena_registry(notify: crate::arena::ArenaNotify) -> Arc<ArenaRegistry> {
        Arc::new(ArenaRegistry::new(notify))
    }

    pub fn reconcile_arena_runs(&self) {
        for tab in &self.tabs {
            let er_path = tab.er_dir();
            let er = Path::new(&er_path);
            let _ = reconcile_stale_runs(er);
        }
    }

    pub fn arena_preview(
        &self,
        scope: ArenaScope,
        files: Option<&[String]>,
        reviewers: &[ReviewerRef],
        rounds: Option<u8>,
        arbiter: Option<&ReviewerRef>,
    ) -> Result<ArenaDiffPreview> {
        let tab = self.tab();
        let raw_diff = tab.raw_diff_for_arena(scope, files)?;
        build_arena_diff_preview(&self.config, &raw_diff, reviewers, rounds, arbiter)
    }

    pub fn arena_start(&mut self, mut params: ArenaStartParams) -> Result<String> {
        params.effort = crate::config::resolve_effort(
            &self.config.ai_hub,
            &self.config.agent,
            self.current_ai_effort.as_deref(),
            params.effort.as_deref(),
        );
        crate::dev_log::arena_line(format!(
            "App::arena_start reviewers={} scope={:?} rounds={:?}",
            params.reviewers.len(),
            params.scope,
            params.rounds
        ));
        let tab = self.tab();
        let raw_diff = tab.raw_diff_for_arena(params.scope, params.files.as_deref())?;
        let repo_root = tab.repo_root.clone();
        let er_dir = tab.er_dir();
        let er_key = er_dir.clone();
        let branch_ref = tab
            .local_branch_view
            .clone()
            .unwrap_or_else(|| tab.current_branch.clone());
        let base_branch = tab.base_branch.clone();
        let config = self.config.clone();
        let registry = Arc::clone(&self.arena_registry);
        let run_id = start_arena_run(
            registry,
            config,
            repo_root,
            er_dir,
            branch_ref,
            base_branch,
            raw_diff,
            params,
        )?;
        self.active_arena_runs
            .entry(er_key.clone())
            .or_default()
            .push(run_id.clone());
        crate::dev_log::arena_line(format!(
            "App::arena_start ok run_id={run_id} er_dir={er_key}"
        ));
        self.notify(&format!("Arena run started ({run_id})"));
        Ok(run_id)
    }

    pub fn arena_start_batch(&mut self, mut batch: ArenaBatchStartParams) -> Result<Vec<String>> {
        batch.effort = crate::config::resolve_effort(
            &self.config.ai_hub,
            &self.config.agent,
            self.current_ai_effort.as_deref(),
            batch.effort.as_deref(),
        );
        let tab = self.tab();
        let raw_diff = tab.raw_diff_for_arena(batch.scope, batch.files.as_deref())?;
        let repo_root = tab.repo_root.clone();
        let er_dir = tab.er_dir();
        let er_key = er_dir.clone();
        let branch_ref = tab
            .local_branch_view
            .clone()
            .unwrap_or_else(|| tab.current_branch.clone());
        let base_branch = tab.base_branch.clone();
        let config = self.config.clone();
        let registry = Arc::clone(&self.arena_registry);
        let run_ids = start_arena_batch(
            registry,
            config,
            repo_root,
            er_dir,
            branch_ref,
            base_branch,
            raw_diff,
            batch,
        )?;
        self.active_arena_runs
            .entry(er_key)
            .or_default()
            .extend(run_ids.iter().cloned());
        self.notify(&format!("Started {} review run(s)", run_ids.len()));
        Ok(run_ids)
    }

    /// Resolve the storage `er_dir` for a run independent of the active tab.
    ///
    /// Arena supervisor threads run in the background regardless of which tab is
    /// in view, so per-run operations must address a run by its own `er_dir`,
    /// not `self.tab().er_dir()`. Resolution order:
    /// (a) invert `active_arena_runs` (run_id → er_dir);
    /// (b) scan each tab's `er_dir` for an on-disk `arena/<run_id>` dir;
    /// (c) fall back to the active tab (covers a brand-new run before lookup).
    fn arena_er_dir_for_run(&self, run_id: &str) -> std::path::PathBuf {
        for (er, ids) in &self.active_arena_runs {
            if ids.iter().any(|id| id == run_id) {
                return std::path::PathBuf::from(er);
            }
        }
        for tab in &self.tabs {
            let er = tab.er_dir();
            if ArenaPaths::for_run(Path::new(&er), run_id).root.is_dir() {
                return std::path::PathBuf::from(er);
            }
        }
        std::path::PathBuf::from(self.tab().er_dir())
    }

    /// Active arena runs across ALL tabs (tab-independent background runs).
    ///
    /// Driven by the registry's globally-tracked active run ids, so it surfaces
    /// in-flight runs regardless of which tab is in view. Newest first.
    pub fn background_arena_runs(&self) -> Vec<crate::arena::ArenaRunSummary> {
        let mut out = Vec::new();
        for run_id in self.arena_registry.active_run_ids() {
            let er_dir = self.arena_er_dir_for_run(&run_id);
            let paths = ArenaPaths::for_run(&er_dir, &run_id);
            // `build_snapshot` calls this every poll; memoize the disk read by
            // run.json mtime so we don't re-parse each active run every tick.
            if let Some(mut summary) = load_run_summary_cached(&paths) {
                // Status is the registry's live value, not the (possibly older)
                // on-disk one — overlay it after the cached disk read.
                if let Some(live) = self.arena_registry.get_status(&run_id) {
                    summary.status = live;
                }
                out.push(summary);
            }
        }
        out.sort_by_key(|r| std::cmp::Reverse(r.created_at.clone()));
        out
    }

    pub fn arena_accept_findings(
        &mut self,
        run_id: &str,
        finding_ids: Option<Vec<String>>,
    ) -> Result<usize> {
        let er_dir = self.arena_er_dir_for_run(run_id);
        let er_path = er_dir.to_string_lossy().into_owned();
        let paths = ArenaPaths::for_run(&er_dir, run_id);
        let mut run = load_run(&paths)?;
        let ids = finding_ids.as_deref();
        let n = import_arena_findings_to_review(&er_path, &mut run, ids)?;
        save_run(&paths, &run)?;
        self.tab_mut().reload_ai_state();
        self.notify(&format!("Accepted {n} finding(s) into Review"));
        Ok(n)
    }

    pub fn arena_cancel(&mut self, run_id: &str) -> Result<()> {
        if !self.arena_registry.cancel(run_id) {
            anyhow::bail!("no active arena run {run_id}");
        }
        let er_dir = self.arena_er_dir_for_run(run_id);
        let er_key = er_dir.to_string_lossy().into_owned();
        let paths = ArenaPaths::for_run(&er_dir, run_id);
        if let Ok(mut run) = load_run(&paths) {
            run.status = crate::arena::RunStatus::Cancelled;
            run.completed_at = Some(super::chrono_now());
            let _ = crate::arena::save_run(&paths, &run);
        }
        if let Some(ids) = self.active_arena_runs.get_mut(&er_key) {
            ids.retain(|id| id != run_id);
            if ids.is_empty() {
                self.active_arena_runs.remove(&er_key);
            }
        }
        self.arena_registry.notify_progress();
        self.notify("Arena run cancelled");
        Ok(())
    }

    pub fn active_arena_run(&self) -> Option<String> {
        let er = self.tab().er_dir();
        self.active_arena_runs
            .get(&er)
            .and_then(|v| v.last().cloned())
    }

    pub fn active_arena_run_ids(&self) -> Vec<String> {
        let er = self.tab().er_dir();
        self.active_arena_runs.get(&er).cloned().unwrap_or_default()
    }

    pub fn arena_progress(&self, run_id: &str) -> Result<ArenaProgressState> {
        let er_dir = self.arena_er_dir_for_run(run_id);
        let paths = ArenaPaths::for_run(&er_dir, run_id);
        Ok(parse_progress_state(&paths))
    }

    pub fn arena_get_snapshot(&self, run_id: &str) -> Result<ArenaRunSnapshot> {
        let er_dir = self.arena_er_dir_for_run(run_id);
        let paths = ArenaPaths::for_run(&er_dir, run_id);
        let mut run = load_run(&paths)?;
        if let Some(live) = self.arena_registry.get_status(run_id) {
            run.status = live;
        }
        Ok(build_snapshot_with_config(&self.config, run))
    }

    pub fn arena_branch_ref(&self) -> String {
        let tab = self.tab();
        tab.local_branch_view
            .clone()
            .unwrap_or_else(|| tab.current_branch.clone())
    }

    pub fn arena_list_summaries(
        &self,
        branch_ref: Option<&str>,
    ) -> Result<Vec<crate::arena::ArenaRunSummary>> {
        let er_path = self.tab().er_dir();
        let er_dir = Path::new(&er_path);
        let mut out = Vec::new();
        for id in crate::arena::list_run_ids(er_dir)? {
            let paths = ArenaPaths::for_run(er_dir, &id);
            if let Ok(run) = load_run(&paths) {
                if branch_ref.is_some_and(|b| run.branch_ref != b) {
                    continue;
                }
                out.push(run.summary());
            }
        }
        out.sort_by_key(|run| std::cmp::Reverse(run.created_at.clone()));
        Ok(out)
    }

    pub fn arena_delete(&mut self, run_id: &str) -> Result<()> {
        if self.arena_registry.is_active(run_id) {
            anyhow::bail!("cannot delete active arena run {run_id}; cancel it first");
        }
        let er_dir = self.arena_er_dir_for_run(run_id);
        let er_key = er_dir.to_string_lossy().into_owned();
        delete_run_dir(&er_dir, run_id)?;
        if let Some(ids) = self.active_arena_runs.get_mut(&er_key) {
            ids.retain(|id| id != run_id);
            if ids.is_empty() {
                self.active_arena_runs.remove(&er_key);
            }
        }
        self.arena_registry.notify_progress();
        self.notify("Arena run deleted");
        Ok(())
    }

    pub fn arena_override_finding(
        &mut self,
        run_id: &str,
        finding_id: &str,
        verdict: Verdict,
        note: String,
    ) -> Result<crate::arena::ArenaFinding> {
        let er_dir = self.arena_er_dir_for_run(run_id);
        let paths = ArenaPaths::for_run(&er_dir, run_id);
        let mut run = load_run(&paths)?;
        let f = run
            .findings
            .iter_mut()
            .find(|x| x.id == finding_id)
            .ok_or_else(|| anyhow::anyhow!("finding not found"))?;
        f.override_ = Some(HumanOverride {
            verdict: verdict.clone(),
            note: note.clone(),
            at: super::chrono_now(),
        });
        f.verdict = verdict;
        let updated = f.clone();
        crate::arena::save_run(&paths, &run)?;
        self.arena_registry.notify_progress();
        Ok(updated)
    }

    pub fn diff_mode_to_arena_scope(mode: super::DiffMode) -> ArenaScope {
        match mode {
            super::DiffMode::Unstaged => ArenaScope::Unstaged,
            super::DiffMode::Staged => ArenaScope::Staged,
            _ => ArenaScope::Branch,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::ArenaScope;
    use crate::git;
    use std::process::Command;

    fn init_repo_with_branches() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(root)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "t@example.com"])
            .current_dir(root)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "t"])
            .current_dir(root)
            .output()
            .unwrap();
        std::fs::write(root.join("file.txt"), "base\n").unwrap();
        Command::new("git")
            .args(["add", "file.txt"])
            .current_dir(root)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "base"])
            .current_dir(root)
            .output()
            .unwrap();
        Command::new("git")
            .args(["checkout", "-b", "feature"])
            .current_dir(root)
            .output()
            .unwrap();
        std::fs::write(root.join("file.txt"), "base\nfeature\n").unwrap();
        Command::new("git")
            .args(["add", "file.txt"])
            .current_dir(root)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "feature"])
            .current_dir(root)
            .output()
            .unwrap();
        Command::new("git")
            .args(["checkout", "main"])
            .current_dir(root)
            .output()
            .unwrap();
        dir
    }

    #[test]
    fn raw_diff_for_arena_uses_branch_view_not_head() {
        let dir = init_repo_with_branches();
        let root = dir.path().to_string_lossy().into_owned();
        let mut tab = TabState::new_with_base_unloaded(root.clone(), "main".into()).unwrap();
        tab.local_branch_view = Some("feature".into());

        let naive = git::git_diff_raw("branch", "main", &root, None).unwrap();
        assert!(
            naive.trim().is_empty(),
            "HEAD on main should not show feature vs main"
        );

        let arena = tab.raw_diff_for_arena(ArenaScope::Branch, None).unwrap();
        let review = tab.raw_diff_for_review("branch").unwrap();
        assert_eq!(arena, review);
        assert!(
            arena.contains("feature"),
            "arena should use branch-view diff"
        );
    }

    // ── Tab-independent run resolution (background runs) ──────────────────

    fn app_with_two_tabs(root_a: &str, root_b: &str) -> App {
        use crate::paths::ErRoot;
        let mut a = TabState::new_for_test(vec![]);
        a.er_root = ErRoot::RepoLocal(root_a.to_string());
        a.repo_root = root_a.to_string();
        let mut b = TabState::new_for_test(vec![]);
        b.er_root = ErRoot::RepoLocal(root_b.to_string());
        b.repo_root = root_b.to_string();
        let mut app = App::new_for_test(vec![]);
        app.tabs = vec![a, b];
        app.active_tab = 0; // tab A is in view; runs may belong to tab B
        app
    }

    fn sample_run(id: &str) -> crate::arena::ArenaRun {
        use crate::ai::RiskLevel;
        use crate::arena::{
            ArenaConfig, ArenaFinding, ArenaRun, ArenaRunKind, CostEstimate, RunStatus,
        };
        use std::collections::BTreeMap;
        ArenaRun {
            id: id.to_string(),
            title: None,
            branch_ref: "feature".into(),
            base_branch: "main".into(),
            scope: ArenaScope::Branch,
            diff_hash: "abc".into(),
            created_at: "2026-06-17T00:00:00Z".into(),
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
                id: "f1".into(),
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
    fn arena_er_dir_for_run_resolves_via_active_map() {
        let ta = tempfile::tempdir().unwrap();
        let tb = tempfile::tempdir().unwrap();
        let mut app = app_with_two_tabs(ta.path().to_str().unwrap(), tb.path().to_str().unwrap());
        let er_b = app.tabs[1].er_dir();
        app.active_arena_runs
            .entry(er_b.clone())
            .or_default()
            .push("arena-x".to_string());
        assert_eq!(
            app.arena_er_dir_for_run("arena-x"),
            std::path::PathBuf::from(&er_b)
        );
    }

    #[test]
    fn arena_er_dir_for_run_falls_back_to_disk_scan() {
        let ta = tempfile::tempdir().unwrap();
        let tb = tempfile::tempdir().unwrap();
        let app = app_with_two_tabs(ta.path().to_str().unwrap(), tb.path().to_str().unwrap());
        let er_b = app.tabs[1].er_dir();
        let paths = ArenaPaths::for_run(Path::new(&er_b), "arena-y");
        std::fs::create_dir_all(&paths.root).unwrap();
        assert_eq!(
            app.arena_er_dir_for_run("arena-y"),
            std::path::PathBuf::from(&er_b)
        );
    }

    #[test]
    fn arena_er_dir_for_run_falls_back_to_active_tab() {
        let ta = tempfile::tempdir().unwrap();
        let tb = tempfile::tempdir().unwrap();
        let app = app_with_two_tabs(ta.path().to_str().unwrap(), tb.path().to_str().unwrap());
        let er_a = app.tabs[0].er_dir();
        assert_eq!(
            app.arena_er_dir_for_run("missing"),
            std::path::PathBuf::from(&er_a)
        );
    }

    #[test]
    fn arena_get_snapshot_reads_run_from_nonactive_tab() {
        let ta = tempfile::tempdir().unwrap();
        let tb = tempfile::tempdir().unwrap();
        let app = app_with_two_tabs(ta.path().to_str().unwrap(), tb.path().to_str().unwrap());
        let er_b = app.tabs[1].er_dir();
        let paths = ArenaPaths::for_run(Path::new(&er_b), "arena-z");
        crate::arena::save_run(&paths, &sample_run("arena-z")).unwrap();
        // Active tab is A; before the fix this resolved A's er_dir and errored.
        let snap = app.arena_get_snapshot("arena-z").unwrap();
        assert_eq!(snap.run.id, "arena-z");
    }

    #[test]
    fn arena_override_finding_resolves_nonactive_tab() {
        let ta = tempfile::tempdir().unwrap();
        let tb = tempfile::tempdir().unwrap();
        let mut app = app_with_two_tabs(ta.path().to_str().unwrap(), tb.path().to_str().unwrap());
        let er_b = app.tabs[1].er_dir();
        let paths = ArenaPaths::for_run(Path::new(&er_b), "arena-ov");
        crate::arena::save_run(&paths, &sample_run("arena-ov")).unwrap();

        // Active tab is A; the mutating wrapper must still resolve B's er_dir.
        let f = app
            .arena_override_finding("arena-ov", "f1", Verdict::Kept, "looks fine".into())
            .unwrap();
        assert_eq!(f.id, "f1");
        // The override must have been persisted under tab B's er_dir.
        let reloaded = load_run(&paths).unwrap();
        assert!(
            reloaded.findings[0].override_.is_some(),
            "override should persist to the run's own er_dir, not the active tab's"
        );
    }

    // ── background_arena_runs (the headline tab-independent surface) ──────

    #[test]
    fn background_arena_runs_surfaces_nonactive_tab_run_with_live_status() {
        let ta = tempfile::tempdir().unwrap();
        let tb = tempfile::tempdir().unwrap();
        let app = app_with_two_tabs(ta.path().to_str().unwrap(), tb.path().to_str().unwrap());
        let er_b = app.tabs[1].er_dir();
        // On disk the run is Complete; the registry says it is mid-flight.
        let paths = ArenaPaths::for_run(Path::new(&er_b), "bg-run");
        crate::arena::save_run(&paths, &sample_run("bg-run")).unwrap();
        app.arena_registry
            .insert_active_for_test("bg-run", crate::arena::RunStatus::Running { round: 2 });

        // Active tab is A, but the run lives under tab B.
        let runs = app.background_arena_runs();
        assert_eq!(runs.len(), 1, "run on a non-active tab must surface");
        assert_eq!(runs[0].id, "bg-run");
        assert_eq!(
            runs[0].status,
            crate::arena::RunStatus::Running { round: 2 },
            "live registry status must override the (older) on-disk status"
        );
    }

    #[test]
    fn background_arena_runs_sorts_newest_first() {
        let ta = tempfile::tempdir().unwrap();
        let tb = tempfile::tempdir().unwrap();
        let app = app_with_two_tabs(ta.path().to_str().unwrap(), tb.path().to_str().unwrap());
        let er_b = app.tabs[1].er_dir();

        let mut older = sample_run("old");
        older.created_at = "2026-06-17T00:00:00Z".into();
        let mut newer = sample_run("new");
        newer.created_at = "2026-06-17T12:00:00Z".into();
        crate::arena::save_run(&ArenaPaths::for_run(Path::new(&er_b), "old"), &older).unwrap();
        crate::arena::save_run(&ArenaPaths::for_run(Path::new(&er_b), "new"), &newer).unwrap();
        app.arena_registry
            .insert_active_for_test("old", crate::arena::RunStatus::Running { round: 1 });
        app.arena_registry
            .insert_active_for_test("new", crate::arena::RunStatus::Running { round: 1 });

        let ids: Vec<String> = app
            .background_arena_runs()
            .into_iter()
            .map(|r| r.id)
            .collect();
        assert_eq!(ids, vec!["new".to_string(), "old".to_string()]);
    }

    #[test]
    fn background_arena_runs_reparses_when_runjson_changes() {
        let ta = tempfile::tempdir().unwrap();
        let tb = tempfile::tempdir().unwrap();
        let app = app_with_two_tabs(ta.path().to_str().unwrap(), tb.path().to_str().unwrap());
        let er_b = app.tabs[1].er_dir();
        let paths = ArenaPaths::for_run(Path::new(&er_b), "cache-run");
        app.arena_registry
            .insert_active_for_test("cache-run", crate::arena::RunStatus::Running { round: 1 });

        // First read: one finding, populates the mtime cache.
        crate::arena::save_run(&paths, &sample_run("cache-run")).unwrap();
        assert_eq!(app.background_arena_runs()[0].finding_count, 1);

        // Rewrite run.json with a second finding; the size/mtime change must
        // invalidate the cached summary so the new count is re-parsed.
        let mut run = sample_run("cache-run");
        let mut second = run.findings[0].clone();
        second.id = "f2".into();
        run.findings.push(second);
        crate::arena::save_run(&paths, &run).unwrap();
        assert_eq!(
            app.background_arena_runs()[0].finding_count,
            2,
            "changed run.json must invalidate the mtime-keyed summary cache"
        );
    }

    // ── Remaining mutating wrappers resolve a non-active tab's er_dir ─────

    #[test]
    fn arena_cancel_resolves_nonactive_tab() {
        let ta = tempfile::tempdir().unwrap();
        let tb = tempfile::tempdir().unwrap();
        let mut app = app_with_two_tabs(ta.path().to_str().unwrap(), tb.path().to_str().unwrap());
        let er_b = app.tabs[1].er_dir();
        let paths = ArenaPaths::for_run(Path::new(&er_b), "arena-cancel");
        crate::arena::save_run(&paths, &sample_run("arena-cancel")).unwrap();
        app.arena_registry.insert_active_for_test(
            "arena-cancel",
            crate::arena::RunStatus::Running { round: 1 },
        );

        // Active tab is A; cancel must persist to B's run.json.
        app.arena_cancel("arena-cancel").unwrap();
        let reloaded = load_run(&paths).unwrap();
        assert_eq!(reloaded.status, crate::arena::RunStatus::Cancelled);
    }

    #[test]
    fn arena_delete_resolves_nonactive_tab() {
        let ta = tempfile::tempdir().unwrap();
        let tb = tempfile::tempdir().unwrap();
        let mut app = app_with_two_tabs(ta.path().to_str().unwrap(), tb.path().to_str().unwrap());
        let er_b = app.tabs[1].er_dir();
        let paths = ArenaPaths::for_run(Path::new(&er_b), "arena-del");
        crate::arena::save_run(&paths, &sample_run("arena-del")).unwrap();
        assert!(paths.root.is_dir());

        // Active tab is A; delete must remove the run dir under B.
        app.arena_delete("arena-del").unwrap();
        assert!(
            !paths.root.is_dir(),
            "delete must target the run's own er_dir, not the active tab's"
        );
    }

    #[test]
    fn arena_accept_findings_resolves_nonactive_tab() {
        let ta = tempfile::tempdir().unwrap();
        let tb = tempfile::tempdir().unwrap();
        let mut app = app_with_two_tabs(ta.path().to_str().unwrap(), tb.path().to_str().unwrap());
        let er_b = app.tabs[1].er_dir();
        let paths = ArenaPaths::for_run(Path::new(&er_b), "arena-acc");
        crate::arena::save_run(&paths, &sample_run("arena-acc")).unwrap();

        // Active tab is A; the import must write review.json under B's er_dir.
        app.arena_accept_findings("arena-acc", None).unwrap();
        assert!(
            Path::new(&er_b).join("review.json").is_file(),
            "accept must import into the run's own er_dir, not the active tab's"
        );
        assert!(
            !Path::new(&app.tabs[0].er_dir())
                .join("review.json")
                .is_file(),
            "active tab A must be untouched by accepting a run that lives on B"
        );
    }
}
