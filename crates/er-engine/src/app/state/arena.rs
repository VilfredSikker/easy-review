use crate::arena::{
    build_arena_diff_preview, build_snapshot_with_config, delete_run_dir,
    import_arena_findings_to_review, load_run, parse_progress_state, reconcile_stale_runs,
    save_run, scope_git_mode, start_arena_batch, start_arena_run, start_seeded_run,
    ArenaBatchStartParams, ArenaDiffPreview, ArenaPaths, ArenaProgressState, ArenaRegistry,
    ArenaRunInputs, ArenaRunKind, ArenaRunSnapshot, ArenaScope, ArenaStartParams, HumanOverride,
    ReviewerRef, SeededStartParams, Verdict,
};
use crate::git::filter_raw_diff_by_paths;
use anyhow::Result;
use std::path::Path;
use std::sync::Arc;

use super::review_flow_admission::ReviewFlowLease;
use super::App;
use super::TabState;

/// The `diff_hash` a review's sidecars carry, which is what a verdict's hash has
/// to match for the overlay to apply.
///
/// Read off disk rather than recomputed, because the callers do not all hash the
/// same bytes: the desktop pipeline filters review-ignore globs out of the diff
/// before preparing it, the TUI does not. Recomputing here would produce a hash
/// the review and its expert sidecars disagree with, and since the overlay
/// compares against theirs, every verdict would silently fail to apply — with a
/// UI line telling the user to re-run validation, which could never help.
fn review_diff_hash(er_dir: &str, raw_diff: &str) -> String {
    crate::ai::prepared_diff::diff_tmp_hash(er_dir)
        .unwrap_or_else(|| crate::ai::compute_diff_hash(raw_diff))
}

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
        let er_dir = self.tab().er_dir();
        self.check_review_bucket_admission(&er_dir)?;
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
        let inputs = ArenaRunInputs {
            repo_root: tab.repo_root.clone(),
            er_dir,
            branch_ref: tab
                .local_branch_view
                .clone()
                .unwrap_or_else(|| tab.current_branch.clone()),
            base_branch: tab.base_branch.clone(),
            raw_diff,
        };
        let config = self.config.clone();
        let run_id = self.arena_start_with_inputs(inputs, config, params)?;
        self.notify(&format!("Arena run started ({run_id})"));
        Ok(run_id)
    }

    /// Start an arena for captured review-flow inputs. The lease binds this
    /// operation to the bucket admitted by the flow, even after tab navigation.
    pub(super) fn arena_start_for(
        &mut self,
        inputs: ArenaRunInputs,
        config: crate::config::ErConfig,
        params: ArenaStartParams,
        lease: &ReviewFlowLease,
    ) -> Result<String> {
        self.check_review_flow_arena_lease(&inputs.er_dir, lease)?;
        let config = self.arena_config_with_live_limits(config);
        let run_id = self.arena_start_with_inputs(inputs, config, params)?;
        self.notify(&format!("Arena run started ({run_id})"));
        Ok(run_id)
    }

    fn arena_start_with_inputs(
        &mut self,
        inputs: ArenaRunInputs,
        config: crate::config::ErConfig,
        params: ArenaStartParams,
    ) -> Result<String> {
        let er_key = inputs.er_dir.clone();
        let run_id = start_arena_run(Arc::clone(&self.arena_registry), config, inputs, params)?;
        self.active_arena_runs
            .entry(er_key.clone())
            .or_default()
            .push(run_id.clone());
        crate::dev_log::arena_line(format!(
            "App::arena_start ok run_id={run_id} er_dir={er_key}"
        ));
        Ok(run_id)
    }

    /// Validate the current tab's expert findings with a seeded arbiter pass.
    ///
    /// The cheap path end to end: the experts already ran, so this dedupes what
    /// they produced and makes one arbiter call. Returns the run id, or `None`
    /// when there is nothing to validate — a caller can then say so rather than
    /// starting a run that would rule on nothing.
    pub fn arena_start_seeded(&mut self) -> Result<Option<String>> {
        let er_dir = self.tab().er_dir();
        self.check_review_bucket_admission(&er_dir)?;
        let tab = self.tab();
        let raw_diff = tab.raw_diff_for_arena(ArenaScope::Branch, None)?;
        let repo_root = tab.repo_root.clone();
        let er_dir = tab.er_dir();
        let branch_ref = tab
            .local_branch_view
            .clone()
            .unwrap_or_else(|| tab.current_branch.clone());
        let base_branch = tab.base_branch.clone();
        let diff_hash = review_diff_hash(&er_dir, &raw_diff);
        // An expert set generated alongside a review that has since gone stale
        // is still this generation's work — same accommodation as the merge.
        let review_hash = tab
            .ai
            .review
            .as_ref()
            .map(|r| r.diff_hash.clone())
            .unwrap_or_default();
        let params = SeededStartParams {
            er_dir,
            branch_ref,
            base_branch,
            scope: ArenaScope::Branch,
            diff_hash,
            review_hash,
            raw_diff,
            arbiter: None,
            include_general: false,
            source_lenses: None,
            lens_providers: std::collections::BTreeMap::new(),
        };
        let config = self.config.clone();
        let run_id = self.arena_start_seeded_with_inputs(repo_root, config, params)?;

        let Some(run_id) = run_id else {
            return Ok(None);
        };
        self.notify(&format!("Validating findings ({run_id})"));
        Ok(Some(run_id))
    }

    /// Start seeded verification from the flow's captured bucket and producer
    /// set. This adapter never consults the active tab or current AI selection.
    pub(super) fn arena_start_seeded_for(
        &mut self,
        repo_root: String,
        config: crate::config::ErConfig,
        params: SeededStartParams,
        lease: &ReviewFlowLease,
    ) -> Result<Option<String>> {
        self.check_review_flow_arena_lease(&params.er_dir, lease)?;
        let config = self.arena_config_with_live_limits(config);
        self.arena_start_seeded_with_inputs(repo_root, config, params)
    }

    fn arena_start_seeded_with_inputs(
        &mut self,
        repo_root: String,
        config: crate::config::ErConfig,
        params: SeededStartParams,
    ) -> Result<Option<String>> {
        let er_key = params.er_dir.clone();
        let run_id = start_seeded_run(Arc::clone(&self.arena_registry), config, repo_root, params)?;
        if let Some(run_id) = &run_id {
            self.active_arena_runs
                .entry(er_key.clone())
                .or_default()
                .push(run_id.clone());
            crate::dev_log::arena_line(format!(
                "App::arena_start_seeded ok run_id={run_id} er_dir={er_key}"
            ));
        }
        Ok(run_id)
    }

    fn check_review_flow_arena_lease(&self, er_dir: &str, lease: &ReviewFlowLease) -> Result<()> {
        if er_dir != lease.bucket() || !self.review_flow_lease_is_valid(lease) {
            anyhow::bail!("Review flow lease does not own this arena bucket");
        }
        Ok(())
    }

    fn arena_config_with_live_limits(
        &self,
        mut captured: crate::config::ErConfig,
    ) -> crate::config::ErConfig {
        captured.ai_hub.max_concurrent_reviews = self.config.ai_hub.max_concurrent_reviews;
        captured.ai_hub.max_concurrent_arena_reviews =
            self.config.ai_hub.max_concurrent_arena_reviews;
        captured.ai_hub.max_concurrent_agents = self.config.ai_hub.max_concurrent_agents;
        captured
    }

    pub fn arena_start_batch(&mut self, mut batch: ArenaBatchStartParams) -> Result<Vec<String>> {
        self.check_review_bucket_admission(&self.tab().er_dir())?;
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
            ArenaRunInputs {
                repo_root,
                er_dir,
                branch_ref,
                base_branch,
                raw_diff,
            },
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
        self.check_review_bucket_admission(&er_path)?;
        let paths = ArenaPaths::for_run(&er_dir, run_id);
        let mut run = load_run(&paths)?;
        // A seeded run's verdicts reach the review through `arbiter.json`, which
        // merges at load. Importing them as well would do the write ADR 0001
        // routed around — `review.json` gains a fourth writer, and
        // `import_arena_findings_to_review` re-stamps its `diff_hash` with this
        // run's, marking a review that may be newer as matching a different diff.
        if run.config.run_kind == ArenaRunKind::Seeded {
            anyhow::bail!(
                "a seeded run's verdicts are applied from arbiter.json on load; \
                 there is nothing to import into review.json"
            );
        }
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
        self.check_review_bucket_admission(&er_key)?;
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
            note,
            at: super::chrono_now(),
        });
        f.verdict = verdict;
        let updated = f.clone();
        crate::arena::save_run(&paths, &run)?;
        self.arena_registry.notify_progress();
        Ok(updated)
    }

    pub const fn diff_mode_to_arena_scope(mode: super::DiffMode) -> ArenaScope {
        match mode {
            super::DiffMode::Unstaged => ArenaScope::Unstaged,
            super::DiffMode::Staged => ArenaScope::Staged,
            _ => ArenaScope::Branch,
        }
    }
}

#[cfg(test)]
mod review_flow_arena_inputs_tests {
    use super::*;
    use crate::arena::{ArenaPaths, ReviewerRef};
    use crate::config::{supplement_ai_hub, ErConfig};
    use serde_json::json;
    use std::sync::{Arc, MutexGuard};
    use tempfile::tempdir;

    struct FakeArenaEnv {
        _guard: MutexGuard<'static, ()>,
        _pool_guard: crate::agent_slots::TestSerialGuard,
        previous: Option<std::ffi::OsString>,
        _fixture: tempfile::TempDir,
        registry: Arc<ArenaRegistry>,
    }

    impl Drop for FakeArenaEnv {
        fn drop(&mut self) {
            for run_id in self.registry.active_run_ids() {
                while self.registry.is_active(&run_id) {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
            if let Some(previous) = self.previous.take() {
                std::env::set_var("ER_FAKE_ARENA_DIR", previous);
            } else {
                std::env::remove_var("ER_FAKE_ARENA_DIR");
            }
        }
    }

    fn fake_arena(app: &App) -> FakeArenaEnv {
        let pool_guard = crate::agent_slots::test_serial();
        let guard = crate::arena::ARENA_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let source = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/arena/fake"
        ));
        let fixture = tempdir().unwrap();
        for name in ["round1.json", "round2.json", "round3.json"] {
            std::fs::copy(source.join(name), fixture.path().join(name)).unwrap();
        }
        let previous = std::env::var_os("ER_FAKE_ARENA_DIR");
        std::env::set_var("ER_FAKE_ARENA_DIR", fixture.path());
        FakeArenaEnv {
            _pool_guard: pool_guard,
            _guard: guard,
            previous,
            _fixture: fixture,
            registry: Arc::clone(&app.arena_registry),
        }
    }

    fn reviewer() -> ReviewerRef {
        ReviewerRef {
            provider_id: "claude".into(),
            model_id: "sonnet-5.5".into(),
            agent_kind: Some("general".into()),
        }
    }

    fn params() -> ArenaStartParams {
        ArenaStartParams {
            title: Some("captured flow".into()),
            reviewers: vec![reviewer()],
            scope: ArenaScope::Branch,
            files: None,
            rounds: Some(1),
            arbiter: Some(reviewer()),
            confirm: true,
            agent_kind: Some("general".into()),
            effort: Some("high".into()),
        }
    }

    fn inputs(bucket: &str) -> ArenaRunInputs {
        ArenaRunInputs {
            repo_root: "/captured/repo".into(),
            er_dir: bucket.into(),
            branch_ref: "captured/head".into(),
            base_branch: "captured/base".into(),
            raw_diff: "captured raw diff".into(),
        }
    }

    fn configured_app() -> App {
        let mut app = App::new_for_test(Vec::new());
        supplement_ai_hub(&mut app.config.ai_hub);
        app
    }

    fn prepared_config() -> ErConfig {
        let mut config = ErConfig::default();
        supplement_ai_hub(&mut config.ai_hub);
        for provider in config.ai_hub.providers.values_mut() {
            provider.command = "/bin/false".into();
        }
        config
    }

    #[test]
    fn review_flow_arena_inputs_reject_mismatched_bucket_before_write() {
        let tmp = tempdir().unwrap();
        let owned = tmp.path().join("owned");
        let wrong = tmp.path().join("wrong");
        let mut app = configured_app();
        let lease = app
            .acquire_review_flow_lease(owned.to_str().unwrap())
            .unwrap();
        let result = app.arena_start_for(
            inputs(wrong.to_str().unwrap()),
            prepared_config(),
            params(),
            &lease,
        );
        assert!(result.is_err());
        assert!(!wrong.exists(), "rejection must happen before arena writes");
    }

    #[test]
    fn review_flow_arena_inputs_reject_stale_lease_before_write() {
        let tmp = tempdir().unwrap();
        let bucket = tmp.path().join("bucket");
        let mut app = configured_app();
        let lease = app
            .acquire_review_flow_lease(bucket.to_str().unwrap())
            .unwrap();
        app.release_review_flow_lease(&lease).unwrap();
        let result = app.arena_start_for(
            inputs(bucket.to_str().unwrap()),
            prepared_config(),
            params(),
            &lease,
        );
        assert!(result.is_err());
        assert!(
            !bucket.exists(),
            "rejection must happen before arena writes"
        );
    }

    #[test]
    fn review_flow_arena_inputs_keep_captured_values_after_tab_switch() {
        let mut app = configured_app();
        let tmp = tempdir().unwrap();
        let _env = fake_arena(&app);
        let bucket = tmp.path().join("captured-bucket");
        let lease = app
            .acquire_review_flow_lease(bucket.to_str().unwrap())
            .unwrap();
        let captured = inputs(bucket.to_str().unwrap());
        let run_config = prepared_config();
        app.tabs.push(TabState::new_for_test(Vec::new()));
        app.active_tab = 1;
        let run_id = app
            .arena_start_for(captured, run_config, params(), &lease)
            .unwrap();
        let run = load_run(&ArenaPaths::for_run(&bucket, &run_id)).unwrap();
        assert_eq!(run.branch_ref, "captured/head");
        assert_eq!(run.base_branch, "captured/base");
        assert_eq!(
            run.diff_hash,
            crate::ai::compute_diff_hash("captured raw diff")
        );
        assert_eq!(run.config.effort.as_deref(), Some("high"));
        assert_eq!(run.title.as_deref(), Some("captured flow"));
    }

    #[test]
    fn review_flow_seeded_inputs_preserve_sources_and_general_findings() {
        let mut app = configured_app();
        app.config.ai_hub.default_effort = Some("low".into());
        let tmp = tempdir().unwrap();
        let _env = fake_arena(&app);
        let bucket = tmp.path().join("captured-bucket");
        std::fs::create_dir_all(&bucket).unwrap();
        std::fs::write(
            bucket.join("review.json"),
            serde_json::to_vec(&json!({
                "version": 1, "diff_hash": "flow-hash",
                "files": { "src/a.rs": { "risk": "medium", "findings": [{
                    "id": "general-1", "severity": "medium", "title": "general claim",
                    "description": "body", "line_start": 5
                }] }}
            }))
            .unwrap(),
        )
        .unwrap();
        let lease = app
            .acquire_review_flow_lease(bucket.to_str().unwrap())
            .unwrap();
        app.tabs.push(TabState::new_for_test(Vec::new()));
        app.active_tab = 1;
        let params = SeededStartParams {
            er_dir: bucket.to_string_lossy().into_owned(),
            branch_ref: "captured/head".into(),
            base_branch: "captured/base".into(),
            scope: ArenaScope::Branch,
            diff_hash: "flow-hash".into(),
            review_hash: String::new(),
            raw_diff: "captured raw diff".into(),
            arbiter: Some(reviewer()),
            include_general: true,
            source_lenses: Some(vec!["general".into()]),
            lens_providers: std::collections::BTreeMap::from([("general".into(), reviewer())]),
        };
        let mut captured_config = prepared_config();
        captured_config.ai_hub.default_effort = Some("high".into());
        let run_id = app
            .arena_start_seeded_for("/captured/repo".into(), captured_config, params, &lease)
            .unwrap()
            .unwrap();
        let run = load_run(&ArenaPaths::for_run(&bucket, &run_id)).unwrap();
        assert_eq!(run.branch_ref, "captured/head");
        assert_eq!(run.findings.len(), 1);
        assert_eq!(run.findings[0].raised_by, vec!["general"]);
        assert_eq!(
            run.config.reviewers[0].agent_kind.as_deref(),
            Some("general")
        );
        assert_eq!(run.config.effort.as_deref(), Some("high"));
    }

    #[test]
    fn review_flow_seeded_legacy_wrapper_keeps_false_none_defaults() {
        let tmp = tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let run_git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git command failed: {:?}: {}",
                args,
                String::from_utf8_lossy(&output.stderr)
            );
            output
        };
        run_git(&["init", "-q"]);
        run_git(&["config", "user.name", "Test"]);
        run_git(&["config", "user.email", "test@example.invalid"]);
        std::fs::write(repo.join("file.txt"), "before\n").unwrap();
        run_git(&["add", "file.txt"]);
        run_git(&["-c", "commit.gpgsign=false", "commit", "-qm", "base"]);
        std::fs::write(repo.join("file.txt"), "after\n").unwrap();
        let raw_diff = String::from_utf8(run_git(&["diff", "--binary"]).stdout).unwrap();
        let current_branch = String::from_utf8(run_git(&["branch", "--show-current"]).stdout)
            .unwrap()
            .trim()
            .to_string();

        let bucket = tmp.path().join("active");
        std::fs::create_dir_all(&bucket).unwrap();
        let mut app = configured_app();
        app.tab_mut().repo_root = repo.to_string_lossy().into_owned();
        app.tab_mut().current_branch = current_branch;
        app.tab_mut().local_branch_checkout_root = Some(repo.to_string_lossy().into_owned());
        app.tab_mut().set_raw_diff_for_test(&raw_diff);
        app.tab_mut().er_root = crate::paths::ErRoot::Managed {
            agent_dir: bucket.to_string_lossy().into_owned(),
            session_dir: bucket.to_string_lossy().into_owned(),
        };
        let review = json!({
            "version": 1,
            "diff_hash": crate::ai::compute_diff_hash(&raw_diff),
            "files": { "file.txt": { "risk": "medium", "findings": [{
                "id": "general-1", "severity": "medium", "title": "general claim",
                "description": "body", "line_start": 1
            }] }}
        });
        std::fs::write(
            bucket.join("review.json"),
            serde_json::to_vec(&review).unwrap(),
        )
        .unwrap();
        assert!(app.arena_start_seeded().unwrap().is_none());
    }

    #[test]
    fn review_flow_arena_uses_live_resource_caps_with_captured_provider_config() {
        let mut app = configured_app();
        app.config.ai_hub.max_concurrent_reviews = 9;
        app.config.ai_hub.max_concurrent_arena_reviews = 8;
        app.config.ai_hub.max_concurrent_agents = 10;
        let mut captured = prepared_config();
        captured.ai_hub.max_concurrent_reviews = 1;
        captured.ai_hub.max_concurrent_arena_reviews = 2;
        captured.ai_hub.max_concurrent_agents = 3;
        let provider_command = captured
            .ai_hub
            .providers
            .values()
            .next()
            .unwrap()
            .command
            .clone();
        let effective = app.arena_config_with_live_limits(captured);
        assert_eq!(effective.ai_hub.max_concurrent_reviews, 9);
        assert_eq!(effective.ai_hub.max_concurrent_arena_reviews, 8);
        assert_eq!(effective.ai_hub.max_concurrent_agents, 10);
        assert_eq!(
            effective.ai_hub.providers.values().next().unwrap().command,
            provider_command
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::ArenaScope;
    use crate::git;
    use std::process::Command;

    /// The recorded hash wins over a recomputed one. This is the review-ignore
    /// case: the desktop filters globs out before preparing the diff, so the
    /// bytes this function is handed hash differently from the bytes the review
    /// and its sidecars were built from. Recomputing made every verdict fail to
    /// apply, silently.
    #[test]
    fn review_diff_hash_prefers_what_the_agent_recorded() {
        let dir = tempfile::tempdir().unwrap();
        let er = dir.path().to_str().unwrap();
        let prepared = "sha-the-agent-recorded";
        std::fs::write(dir.path().join(".diff-tmp.sha256"), prepared).unwrap();

        assert_eq!(
            review_diff_hash(er, "a diff that would hash differently"),
            prepared,
            "the on-disk marker decides, not the bytes passed in"
        );
    }

    #[test]
    fn review_diff_hash_falls_back_to_hashing_the_raw_diff() {
        let dir = tempfile::tempdir().unwrap();
        let er = dir.path().to_str().unwrap();
        let raw = "diff --git a/x b/x\n";

        assert_eq!(review_diff_hash(er, raw), crate::ai::compute_diff_hash(raw));
    }

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
        Command::new("git")
            .args(["config", "commit.gpgsign", "false"])
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
    fn arena_delete_refuses_a_bucket_owned_by_review_flow() {
        let ta = tempfile::tempdir().unwrap();
        let tb = tempfile::tempdir().unwrap();
        let mut app = app_with_two_tabs(ta.path().to_str().unwrap(), tb.path().to_str().unwrap());
        let er_b = app.tabs[1].er_dir();
        let paths = ArenaPaths::for_run(Path::new(&er_b), "arena-flow-owned");
        crate::arena::save_run(&paths, &sample_run("arena-flow-owned")).unwrap();
        let lease = app.acquire_review_flow_lease(&er_b).unwrap();

        assert!(app.arena_delete("arena-flow-owned").is_err());
        assert!(paths.root.is_dir(), "the owned run must remain on disk");
        app.release_review_flow_lease(&lease).unwrap();
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
