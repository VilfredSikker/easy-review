use super::{App, CommandStatus};
use anyhow::{bail, Context, Result};
use er_store::StoreError;
use std::time::Duration;

/// Task kind and command name of a tour; both are the same string.
pub(super) const TOUR_TASK_KIND: &str = "tour";

/// Capability naming the one workflow that owns a bucket. The OS lease behind
/// it is the authority across processes; the holder is its identity, so a
/// token from a released flow never matches a later one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ReviewFlowLease {
    bucket: String,
    sequence: u64,
    holder: er_store::HolderInfo,
}

impl ReviewFlowLease {
    pub(super) fn bucket(&self) -> &str {
        &self.bucket
    }

    /// Unique per flow within the process, for archive keys.
    pub(super) fn sequence(&self) -> u64 {
        self.sequence
    }

    pub(super) fn holder(&self) -> &er_store::HolderInfo {
        &self.holder
    }
}

impl App {
    /// Reject ordinary actions only while a workflow owns this exact bucket.
    ///
    /// The probe does not wait: a held lease answers at once, and startup
    /// reconciliation can call this without stalling.
    pub fn check_review_bucket_admission(&self, er_dir: &str) -> Result<()> {
        let scope = er_store::Scope::dir(er_dir)?;
        match crate::store_registry::store().lease(&scope, Duration::ZERO, "admission") {
            Ok(guard) => {
                drop(guard);
                Ok(())
            }
            Err(StoreError::LeaseHeld { .. } | StoreError::AlreadyLocked { .. }) => {
                bail!("A review flow is already using this review bucket")
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Take exclusive ownership after proving that no producer can still write
    /// into this bucket. Ordinary manual lenses are not made mutually exclusive.
    /// The guard stays in `workflow_leases` so the OS lease is held for the
    /// flow's life, even across tab navigation.
    pub(super) fn acquire_review_flow_lease(&mut self, er_dir: &str) -> Result<ReviewFlowLease> {
        self.check_review_bucket_admission(er_dir)?;
        if self.bucket_has_producers(er_dir)? {
            bail!("Review work is already queued or running in this review bucket");
        }
        let sequence = self.next_review_flow_lease_sequence;
        self.next_review_flow_lease_sequence = sequence.wrapping_add(1).max(1);
        let label = format!("flow-{sequence}");
        let scope = er_store::Scope::dir(er_dir)?;
        let guard = crate::store_registry::store()
            .lease(&scope, er_store::DEFAULT_LOCK_TIMEOUT, &label)
            .with_context(|| format!("take the review flow lease for {er_dir}"))?;
        let lease = ReviewFlowLease {
            bucket: er_dir.to_string(),
            sequence,
            holder: guard.holder().clone(),
        };
        self.workflow_leases.insert(er_dir.to_string(), guard);
        Ok(lease)
    }

    pub(super) fn review_flow_lease_is_valid(&self, lease: &ReviewFlowLease) -> bool {
        self.workflow_leases
            .get(lease.bucket())
            .is_some_and(|guard| guard.holder() == lease.holder())
    }

    /// Take the bucket lease for a standalone arena action. The guard travels
    /// with the runs instead of `workflow_leases`, because their supervisors
    /// release it when the last one drains.
    pub(super) fn acquire_detached_workflow_lease(
        &mut self,
        er_dir: &str,
    ) -> Result<er_store::LeaseGuard> {
        self.check_review_bucket_admission(er_dir)?;
        if self.bucket_has_producers(er_dir)? {
            bail!("Review work is already queued or running in this review bucket");
        }
        let sequence = self.next_review_flow_lease_sequence;
        self.next_review_flow_lease_sequence = sequence.wrapping_add(1).max(1);
        let scope = er_store::Scope::dir(er_dir)?;
        let guard = crate::store_registry::store()
            .lease(
                &scope,
                er_store::DEFAULT_LOCK_TIMEOUT,
                &format!("arena-{sequence}"),
            )
            .with_context(|| format!("take the arena lease for {er_dir}"))?;
        Ok(guard)
    }

    /// Release only after all writers, including arena supervisors, have left.
    /// Dropping the guard releases the OS lease every process can see.
    pub(super) fn release_review_flow_lease(&mut self, lease: &ReviewFlowLease) -> Result<()> {
        if !self.review_flow_lease_is_valid(lease) {
            bail!("Review flow lease is stale");
        }
        if self.bucket_has_producers(lease.bucket())? {
            bail!("Review flow still has queued or running work in this bucket");
        }
        self.workflow_leases.remove(lease.bucket());
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

        // Arena runs and every other workflow are covered by the bucket lease,
        // which admission probes before consulting local work.
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

    fn with_bucket<R>(f: impl FnOnce(&str) -> R) -> R {
        let root = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(root.path(), || {
            let bucket = root.path().join("bucket");
            std::fs::create_dir_all(&bucket).unwrap();
            f(bucket.to_str().unwrap())
        })
    }

    fn with_process_bucket<R>(f: impl FnOnce(&str) -> R) -> R {
        let root = tempfile::tempdir().unwrap();
        crate::storage::with_process_test_storage_root(root.path(), || {
            let bucket = root.path().join("bucket");
            std::fs::create_dir_all(&bucket).unwrap();
            f(bucket.to_str().unwrap())
        })
    }

    #[test]
    fn review_flow_admission_a_live_lease_makes_its_bucket_busy() {
        with_bucket(|bucket| {
            let mut app = app();
            let lease = app.acquire_review_flow_lease(bucket).unwrap();
            assert!(app.review_flow_lease_is_valid(&lease));
            assert!(app.check_review_bucket_admission(bucket).is_err());
        });
    }

    #[test]
    fn review_flow_admission_separate_buckets_are_independent() {
        let root = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(root.path(), || {
            let first = root.path().join("first");
            let second = root.path().join("second");
            let mut app = app();
            app.acquire_review_flow_lease(first.to_str().unwrap())
                .unwrap();
            assert!(app
                .check_review_bucket_admission(second.to_str().unwrap())
                .is_ok());
            assert!(app
                .acquire_review_flow_lease(second.to_str().unwrap())
                .is_ok());
        });
    }

    #[test]
    fn review_flow_admission_preexisting_producers_prevent_acquire() {
        with_bucket(|_| {
            let mut app = app();
            // The tab's own bucket makes the tab-status branch see this flow.
            let bucket = app.tab().er_dir();
            let task = BackgroundTask::new("review".into(), target(&bucket));
            app.pending_background_tasks.push_back(
                crate::app::state::background::PendingBackgroundTask {
                    task,
                    command_name: "review".into(),
                    prompt: String::new(),
                    prepared_diff: false,
                    host_write_diagram: None,
                    ai_selection: None,
                    launch_config: None,
                    scoped_stamp: None,
                    staging: None,
                },
            );
            assert!(app.acquire_review_flow_lease(&bucket).is_err());

            app.pending_background_tasks.clear();
            app.tab_mut()
                .command_status
                .insert("summary".into(), CommandStatus::Running);
            assert!(app.acquire_review_flow_lease(&bucket).is_err());
        });
    }

    #[test]
    fn review_flow_admission_manual_concurrency_without_lease_is_allowed() {
        with_bucket(|bucket| {
            let mut app = app();
            app.tab_mut()
                .command_status
                .insert("expert".into(), CommandStatus::Running);
            assert!(app.check_review_bucket_admission(bucket).is_ok());
        });
    }

    #[test]
    fn review_flow_admission_a_live_lease_blocks_acquire() {
        use std::sync::{Arc, Barrier};

        with_process_bucket(|bucket| {
            let scope = er_store::Scope::dir(bucket).unwrap();
            let barrier = Arc::new(Barrier::new(2));
            let holder = {
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    let guard = crate::store_registry::store()
                        .lease(&scope, Duration::from_secs(5), "other-process")
                        .unwrap();
                    barrier.wait();
                    barrier.wait();
                    drop(guard);
                })
            };
            barrier.wait();
            let mut app = app();
            assert!(app.acquire_review_flow_lease(bucket).is_err());
            barrier.wait();
            holder.join().unwrap();
            assert!(app.acquire_review_flow_lease(bucket).is_ok());
        });
    }

    #[test]
    fn review_flow_admission_release_refuses_while_a_local_producer_runs() {
        with_bucket(|bucket| {
            let mut app = app();
            let lease = app.acquire_review_flow_lease(bucket).unwrap();
            let task = BackgroundTask::new("review".into(), target(bucket));
            app.pending_background_tasks.push_back(
                crate::app::state::background::PendingBackgroundTask {
                    task,
                    command_name: "review".into(),
                    prompt: String::new(),
                    prepared_diff: false,
                    host_write_diagram: None,
                    ai_selection: None,
                    launch_config: None,
                    scoped_stamp: None,
                    staging: None,
                },
            );
            assert!(app.release_review_flow_lease(&lease).is_err());
            app.pending_background_tasks.clear();
            assert!(app.release_review_flow_lease(&lease).is_ok());
        });
    }

    #[test]
    fn review_flow_admission_stale_token_is_rejected() {
        with_bucket(|bucket| {
            let mut app = app();
            let lease = app.acquire_review_flow_lease(bucket).unwrap();
            app.release_review_flow_lease(&lease).unwrap();
            assert!(app.release_review_flow_lease(&lease).is_err());
        });
    }

    #[test]
    fn review_flow_admission_tour_runs_beside_a_flow_lease() {
        with_bucket(|bucket| {
            let mut app = app();
            let lease = app.acquire_review_flow_lease(bucket).unwrap();
            app.spawn_background_tour(target(bucket), String::new(), true)
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
        });
    }

    #[test]
    fn review_flow_admission_tour_does_not_block_acquire() {
        with_bucket(|bucket| {
            let mut app = app();
            app.spawn_background_tour(target(bucket), String::new(), true)
                .unwrap();
            app.tab_mut()
                .command_status
                .insert(TOUR_TASK_KIND.into(), CommandStatus::Running);
            assert!(app.acquire_review_flow_lease(bucket).is_ok());
        });
    }

    #[test]
    fn review_flow_admission_other_background_work_still_refused_under_lease() {
        with_bucket(|bucket| {
            let mut app = app();
            let _lease = app.acquire_review_flow_lease(bucket).unwrap();
            assert!(app
                .spawn_background_review(target(bucket), String::new(), true, None)
                .is_err());
        });
    }
}
