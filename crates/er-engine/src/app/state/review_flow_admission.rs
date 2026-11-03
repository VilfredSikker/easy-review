use super::{App, CommandStatus};
use anyhow::{bail, Result};

/// Task kind and command name of a tour; both are the same string.
pub(super) const TOUR_TASK_KIND: &str = "tour";

/// Capability held by the one review flow that owns a bucket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ReviewFlowLease {
    bucket: String,
    sequence: u64,
}

impl ReviewFlowLease {
    pub(super) fn bucket(&self) -> &str {
        &self.bucket
    }

    pub(super) fn sequence(&self) -> u64 {
        self.sequence
    }
}

impl App {
    /// Reject ordinary actions only while a review flow owns this exact bucket.
    pub fn check_review_bucket_admission(&self, er_dir: &str) -> Result<()> {
        if self.review_flow_leases.contains_key(er_dir) {
            bail!("A review flow is already using this review bucket");
        }
        Ok(())
    }

    /// Take exclusive ownership after proving that no producer can still write
    /// into this bucket. Ordinary manual lenses are not made mutually exclusive.
    pub(super) fn acquire_review_flow_lease(&mut self, er_dir: &str) -> Result<ReviewFlowLease> {
        self.check_review_bucket_admission(er_dir)?;
        if self.bucket_has_producers(er_dir)? {
            bail!("Review work is already queued or running in this review bucket");
        }
        let sequence = self.next_review_flow_lease_sequence;
        self.next_review_flow_lease_sequence = sequence.wrapping_add(1).max(1);
        let lease = ReviewFlowLease {
            bucket: er_dir.to_string(),
            sequence,
        };
        self.review_flow_leases
            .insert(er_dir.to_string(), lease.clone());
        Ok(lease)
    }

    pub(super) fn review_flow_lease_is_valid(&self, lease: &ReviewFlowLease) -> bool {
        self.review_flow_leases.get(lease.bucket()) == Some(lease)
    }

    /// Release only after all writers, including arena supervisors, have left.
    pub(super) fn release_review_flow_lease(&mut self, lease: &ReviewFlowLease) -> Result<()> {
        if !self.review_flow_lease_is_valid(lease) {
            bail!("Review flow lease is stale");
        }
        if self.bucket_has_producers(lease.bucket())? {
            bail!("Review flow still has queued or running work in this bucket");
        }
        self.review_flow_leases.remove(lease.bucket());
        Ok(())
    }

    /// A tour is not a producer: it reads its own prepared diff
    /// (`prepared_diff::tour_input_dir`) and writes only `tour.json`, which no
    /// review-flow stage reads or writes. Counting it would let a running tour
    /// block a flow's acquire and its release.
    fn bucket_has_producers(&self, er_dir: &str) -> Result<bool> {
        if self.background_tasks.values().any(|task| {
            task.task.target.er_dir == er_dir
                && task.task.kind != TOUR_TASK_KIND
                && task.task.status == CommandStatus::Running
        }) || self
            .pending_background_tasks
            .iter()
            .any(|task| task.task.target.er_dir == er_dir && task.task.kind != TOUR_TASK_KIND)
        {
            return Ok(true);
        }

        if self.tabs.iter().any(|tab| {
            tab.er_dir() == er_dir
                && tab.command_status.iter().any(|(name, status)| {
                    name != TOUR_TASK_KIND && status == &CommandStatus::Running
                })
        }) {
            return Ok(true);
        }

        let indexed_active = self.active_arena_runs.get(er_dir).is_some_and(|run_ids| {
            run_ids
                .iter()
                .any(|run_id| self.arena_registry.is_active(run_id))
        });
        if indexed_active {
            return Ok(true);
        }

        let bucket = std::path::Path::new(er_dir);
        for run_id in crate::arena::list_run_ids(bucket)? {
            if self.arena_registry.is_active(&run_id) {
                return Ok(true);
            }
            let paths = crate::arena::ArenaPaths::for_run(bucket, &run_id);
            let Some(run) = crate::arena::load_historical_run(&paths)? else {
                continue;
            };
            if matches!(
                run.status,
                crate::arena::RunStatus::Queued | crate::arena::RunStatus::Running { .. }
            ) {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::background::{BackgroundTask, BackgroundTaskTarget};

    fn app() -> App {
        App::new_for_test(Vec::new())
    }

    fn target(er_dir: &str) -> BackgroundTaskTarget {
        BackgroundTaskTarget {
            repo_root: "/tmp/test".into(),
            er_dir: er_dir.into(),
            branch_label: "feature".into(),
            base_branch: "main".into(),
            scope: "branch".into(),
            pr_number: None,
            remote_repo: None,
            managed_local: false,
        }
    }

    fn persist_arena_status(er_dir: &std::path::Path, run_id: &str, status: &str) {
        let run_dir = er_dir.join("arena").join(run_id);
        std::fs::create_dir_all(&run_dir).unwrap();
        let status = if status == "running" {
            serde_json::json!({ "running": { "round": 1 } })
        } else {
            serde_json::json!(status)
        };
        let run = serde_json::json!({
            "id": run_id,
            "branch_ref": "feature",
            "base_branch": "main",
            "scope": "branch",
            "diff_hash": "hash",
            "created_at": "now",
            "status": status,
            "config": {
                "reviewers": [],
                "rounds": 1,
                "arbiter": { "provider_id": "p", "model_id": "m" },
                "auto_accept_threshold": 0.0,
                "scope": "branch"
            },
            "reviewers": [],
            "findings": [],
            "cost_estimate": { "tokens_in": 0, "tokens_out": 0, "usd": 0.0 }
        });
        std::fs::write(run_dir.join("run.json"), serde_json::to_vec(&run).unwrap()).unwrap();
    }

    fn temp_bucket(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "er-review-flow-admission-{label}-{}",
            std::process::id()
        ))
    }

    #[test]
    fn review_flow_admission_same_bucket_tabs_share_lease() {
        let mut app = app();
        app.tabs
            .push(crate::app::TabState::new_for_test(Vec::new()));
        let lease = app.acquire_review_flow_lease("/tmp/test/.er").unwrap();
        assert!(app.review_flow_lease_is_valid(&lease));
        assert!(app.check_review_bucket_admission("/tmp/test/.er").is_err());
    }

    #[test]
    fn review_flow_admission_separate_buckets_are_independent() {
        let mut app = app();
        app.acquire_review_flow_lease("/tmp/a").unwrap();
        assert!(app.check_review_bucket_admission("/tmp/b").is_ok());
        assert!(app.acquire_review_flow_lease("/tmp/b").is_ok());
    }

    #[test]
    fn review_flow_admission_preexisting_producers_prevent_acquire() {
        let mut app = app();
        let task = BackgroundTask::new("review".into(), target("/tmp/test/.er"));
        app.pending_background_tasks.push_back(
            crate::app::state::background::PendingBackgroundTask {
                task,
                command_name: "review".into(),
                prompt: String::new(),
                prepared_diff: false,
                host_write_diagram: None,
                ai_selection: None,
                launch_config: None,
            },
        );
        assert!(app.acquire_review_flow_lease("/tmp/test/.er").is_err());

        app.pending_background_tasks.clear();
        app.tab_mut()
            .command_status
            .insert("summary".into(), CommandStatus::Running);
        assert!(app.acquire_review_flow_lease("/tmp/test/.er").is_err());
    }

    #[test]
    fn review_flow_admission_manual_concurrency_without_lease_is_allowed() {
        let mut app = app();
        app.tab_mut()
            .command_status
            .insert("expert".into(), CommandStatus::Running);
        assert!(app.check_review_bucket_admission("/tmp/test/.er").is_ok());
    }

    #[test]
    fn review_flow_admission_completed_arena_index_alone_is_not_busy() {
        let mut app = app();
        app.active_arena_runs
            .insert("/tmp/test/.er".into(), vec!["finished".into()]);
        assert!(app.acquire_review_flow_lease("/tmp/test/.er").is_ok());
    }

    #[test]
    fn review_flow_admission_persisted_running_arena_blocks_acquire() {
        let bucket = temp_bucket("persisted-running");
        let _ = std::fs::remove_dir_all(&bucket);
        persist_arena_status(&bucket, "running", "running");
        let mut app = app();
        assert!(app
            .acquire_review_flow_lease(bucket.to_str().unwrap())
            .is_err());
        std::fs::remove_dir_all(bucket).unwrap();
    }

    #[test]
    fn review_flow_admission_persisted_completed_arena_allows_acquire() {
        let bucket = temp_bucket("persisted-complete");
        let _ = std::fs::remove_dir_all(&bucket);
        persist_arena_status(&bucket, "complete", "complete");
        let mut app = app();
        assert!(app
            .acquire_review_flow_lease(bucket.to_str().unwrap())
            .is_ok());
        std::fs::remove_dir_all(bucket).unwrap();
    }

    #[test]
    fn review_flow_admission_terminal_registered_arena_blocks_release() {
        let mut app = app();
        let lease = app.acquire_review_flow_lease("/tmp/test/.er").unwrap();
        app.active_arena_runs
            .insert("/tmp/test/.er".into(), vec!["arena-terminal".into()]);
        app.arena_registry
            .insert_active_for_test("arena-terminal", crate::arena::RunStatus::Cancelled);
        assert!(app.release_review_flow_lease(&lease).is_err());
        app.arena_registry.take("arena-terminal");
        assert!(app.release_review_flow_lease(&lease).is_ok());
    }

    #[test]
    fn review_flow_admission_stale_token_is_rejected() {
        let mut app = app();
        let lease = app.acquire_review_flow_lease("/tmp/test/.er").unwrap();
        app.release_review_flow_lease(&lease).unwrap();
        assert!(app.release_review_flow_lease(&lease).is_err());
    }

    #[test]
    fn review_flow_admission_tour_runs_beside_a_flow_lease() {
        let mut app = app();
        let lease = app.acquire_review_flow_lease("/tmp/test/.er").unwrap();
        app.spawn_background_tour(target("/tmp/test/.er"), String::new(), true)
            .unwrap();
        assert!(
            app.pending_background_tasks
                .iter()
                .any(|task| task.task.kind == TOUR_TASK_KIND)
                || app
                    .background_tasks
                    .values()
                    .any(|task| task.task.kind == TOUR_TASK_KIND)
        );
        // A queued or running tour must not hold the flow's lease open.
        assert!(app.release_review_flow_lease(&lease).is_ok());
    }

    #[test]
    fn review_flow_admission_tour_does_not_block_acquire() {
        let mut app = app();
        app.spawn_background_tour(target("/tmp/test/.er"), String::new(), true)
            .unwrap();
        app.tab_mut()
            .command_status
            .insert(TOUR_TASK_KIND.into(), CommandStatus::Running);
        assert!(app.acquire_review_flow_lease("/tmp/test/.er").is_ok());
    }

    #[test]
    fn review_flow_admission_other_background_work_still_refused_under_lease() {
        let mut app = app();
        let _lease = app.acquire_review_flow_lease("/tmp/test/.er").unwrap();
        assert!(app
            .spawn_background_review(target("/tmp/test/.er"), String::new(), true)
            .is_err());
    }
}
