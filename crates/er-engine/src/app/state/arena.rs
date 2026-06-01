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

    pub fn arena_accept_findings(
        &mut self,
        run_id: &str,
        finding_ids: Option<Vec<String>>,
    ) -> Result<usize> {
        let er_path = self.tab().er_dir();
        let paths = ArenaPaths::for_run(Path::new(&er_path), run_id);
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
        let er_path = self.tab().er_dir();
        let paths = ArenaPaths::for_run(Path::new(&er_path), run_id);
        if let Ok(mut run) = load_run(&paths) {
            run.status = crate::arena::RunStatus::Cancelled;
            run.completed_at = Some(super::chrono_now());
            let _ = crate::arena::save_run(&paths, &run);
        }
        let er_key = self.tab().er_dir();
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
        let er_path = self.tab().er_dir();
        let paths = ArenaPaths::for_run(Path::new(&er_path), run_id);
        Ok(parse_progress_state(&paths))
    }

    pub fn arena_get_snapshot(&self, run_id: &str) -> Result<ArenaRunSnapshot> {
        let er_path = self.tab().er_dir();
        let paths = ArenaPaths::for_run(Path::new(&er_path), run_id);
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
        out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(out)
    }

    pub fn arena_delete(&mut self, run_id: &str) -> Result<()> {
        if self.arena_registry.is_active(run_id) {
            anyhow::bail!("cannot delete active arena run {run_id}; cancel it first");
        }
        let er_path = self.tab().er_dir();
        let er_key = er_path.clone();
        delete_run_dir(Path::new(&er_path), run_id)?;
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
        let er_path = self.tab().er_dir();
        let paths = ArenaPaths::for_run(Path::new(&er_path), run_id);
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
            .args(["init"])
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
        tab.local_branch_diff_ref = Some("feature".into());

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
}
