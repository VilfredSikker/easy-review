use crate::arena::{
    build_snapshot, load_run, reconcile_stale_runs, start_arena_run, ArenaPaths, ArenaRegistry,
    ArenaRunSnapshot, ArenaScope, ArenaStartParams, HumanOverride, Verdict,
};
use anyhow::Result;
use std::path::Path;
use std::sync::Arc;

use super::App;

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

    pub fn arena_start(&mut self, params: ArenaStartParams) -> Result<String> {
        let tab = self.tab();
        let repo_root = tab.repo_root.clone();
        let er_dir = tab.er_dir();
        let er_key = er_dir.clone();
        let branch_ref = tab.current_branch.clone();
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
            params,
        )?;
        self.active_arena_runs.insert(er_key, run_id.clone());
        self.notify(&format!("Arena run started ({run_id})"));
        Ok(run_id)
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
        self.arena_registry.notify_progress();
        self.notify("Arena run cancelled");
        Ok(())
    }

    pub fn active_arena_run(&self) -> Option<String> {
        let er = self.tab().er_dir();
        self.active_arena_runs.get(&er).cloned()
    }

    pub fn arena_get_snapshot(&self, run_id: &str) -> Result<ArenaRunSnapshot> {
        let er_path = self.tab().er_dir();
        let paths = ArenaPaths::for_run(Path::new(&er_path), run_id);
        let mut run = load_run(&paths)?;
        if let Some(live) = self.arena_registry.get_status(run_id) {
            run.status = live;
        }
        Ok(build_snapshot(run))
    }

    pub fn arena_list_summaries(&self) -> Result<Vec<crate::arena::ArenaRunSummary>> {
        let er_path = self.tab().er_dir();
        let er_dir = Path::new(&er_path);
        let mut out = Vec::new();
        for id in crate::arena::list_run_ids(er_dir)? {
            let paths = ArenaPaths::for_run(er_dir, &id);
            if let Ok(run) = load_run(&paths) {
                out.push(run.summary());
            }
        }
        out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(out)
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
