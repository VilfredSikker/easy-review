//! App-owned control for live arena runs.
//!
//! A run's stop flag, live status and in-memory progress belong to the process
//! that started it. The control is registered before its supervisor thread
//! exists, so a supervisor that finishes at once cannot be lost, and the host
//! can always see the run. Durable state stays in `run.json` and the workflow
//! record; the control is the live half.

use super::progress::{ArenaNotify, ArenaProgress, ArenaProgressState, ProgressEvent};
use super::storage::{load_run, save_run, ArenaPaths};
use super::RunStatus;
use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// One live run's control.
pub struct ArenaRunControl {
    pub run_id: String,
    pub er_dir: String,
    /// When the run started, for "most recent run on this tab" lookups.
    pub started: Instant,
    cancel: Arc<AtomicBool>,
    status: Mutex<RunStatus>,
    progress: Mutex<ArenaProgress>,
    finished: Mutex<bool>,
    finished_cv: Condvar,
    panic: Mutex<Option<String>>,
    notify: Arc<Mutex<ArenaNotify>>,
}

impl ArenaRunControl {
    pub fn new(run_id: &str, er_dir: &str, notify: Arc<Mutex<ArenaNotify>>) -> Arc<Self> {
        Arc::new(Self {
            run_id: run_id.to_string(),
            er_dir: er_dir.to_string(),
            started: Instant::now(),
            cancel: Arc::new(AtomicBool::new(false)),
            status: Mutex::new(RunStatus::Queued),
            progress: Mutex::new(ArenaProgress::default()),
            finished: Mutex::new(false),
            finished_cv: Condvar::new(),
            panic: Mutex::new(None),
            notify,
        })
    }

    /// The run's stop flag. The job bridges watch it; setting it is the one
    /// cancel path, and the runner's process-group kill does the rest.
    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancel)
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        self.notify_progress();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    pub fn status(&self) -> RunStatus {
        self.status
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    pub fn set_status(&self, status: RunStatus) {
        *self
            .status
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = status;
    }

    /// Fold one progress event into the live state and wake the host.
    pub fn emit(&self, event: &ProgressEvent) {
        self.progress
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .apply(event);
        self.notify_progress();
    }

    pub fn progress(&self) -> ArenaProgress {
        self.progress
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    pub fn progress_state(&self) -> ArenaProgressState {
        self.progress().snapshot()
    }

    pub fn notify_progress(&self) {
        let notify = self
            .notify
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        notify();
    }

    /// Mark the supervisor as returned. Every exit path calls this, so a host
    /// waiting on the run cannot be stranded.
    pub fn finish(&self) {
        *self
            .finished
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = true;
        self.finished_cv.notify_all();
        self.notify_progress();
    }

    pub fn is_finished(&self) -> bool {
        *self
            .finished
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    /// Wait for the supervisor to return. `false` on timeout.
    pub fn wait_finished(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut finished = self
            .finished
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        while !*finished {
            let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                return false;
            };
            let (guard, result) = self
                .finished_cv
                .wait_timeout(finished, left)
                .unwrap_or_else(|error| error.into_inner());
            finished = guard;
            if result.timed_out() && !*finished {
                return false;
            }
        }
        true
    }

    pub fn record_panic(&self, message: String) {
        *self.panic.lock().unwrap_or_else(|error| error.into_inner()) = Some(message);
    }

    pub fn panic_message(&self) -> Option<String> {
        self.panic
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
}

/// The App's live runs, keyed by run id. This is not a process-wide registry:
/// it holds only what the host needs to show and stop a run it started.
#[derive(Clone)]
pub struct ArenaRunControls {
    runs: Arc<Mutex<HashMap<String, Arc<ArenaRunControl>>>>,
    notify: Arc<Mutex<ArenaNotify>>,
    /// Makes the next supervisor spawn fail. Test support for the launcher's
    /// failed-start path, which no normal build can reach.
    #[cfg(any(test, feature = "test-support"))]
    fail_next_spawn: Arc<AtomicBool>,
}

impl Default for ArenaRunControls {
    fn default() -> Self {
        Self {
            runs: Arc::new(Mutex::new(HashMap::new())),
            notify: Arc::new(Mutex::new(Arc::new(|| {}))),
            #[cfg(any(test, feature = "test-support"))]
            fail_next_spawn: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl ArenaRunControls {
    pub fn set_notify(&self, notify: ArenaNotify) {
        *self
            .notify
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = notify;
    }

    pub fn new_control(&self, run_id: &str, er_dir: &str) -> Arc<ArenaRunControl> {
        ArenaRunControl::new(run_id, er_dir, Arc::clone(&self.notify))
    }

    pub fn insert(&self, control: &Arc<ArenaRunControl>) {
        self.runs
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(control.run_id.clone(), Arc::clone(control));
    }

    pub fn get(&self, run_id: &str) -> Option<Arc<ArenaRunControl>> {
        self.runs
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(run_id)
            .cloned()
    }

    pub fn remove(&self, run_id: &str) -> Option<Arc<ArenaRunControl>> {
        self.runs
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(run_id)
    }

    pub fn contains(&self, run_id: &str) -> bool {
        self.runs
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .contains_key(run_id)
    }

    /// Is the run's supervisor still running?
    ///
    /// A finished control stays in the map until `prune_finished`, so presence
    /// alone does not mean live: the supervisor has returned and its durable
    /// record is authoritative.
    pub fn is_active(&self, run_id: &str) -> bool {
        self.get(run_id)
            .is_some_and(|control| !control.is_finished())
    }

    pub fn active_ids(&self) -> Vec<String> {
        self.runs
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .values()
            .filter(|control| !control.is_finished())
            .map(|control| control.run_id.clone())
            .collect()
    }

    /// Wake the host, e.g. after a write that changes a run's findings.
    pub fn notify_progress(&self) {
        let notify = self
            .notify
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        notify();
    }

    /// Register a live control no supervisor drives. Test support: recovery
    /// and UI tests need a mid-flight run without spawning an agent.
    #[cfg(any(test, feature = "test-support"))]
    pub fn insert_for_test(
        &self,
        run_id: &str,
        er_dir: &str,
        status: RunStatus,
    ) -> Arc<ArenaRunControl> {
        let control = self.new_control(run_id, er_dir);
        control.set_status(status);
        self.insert(&control);
        Arc::clone(&control)
    }

    /// Mark a test control's supervisor as returned.
    #[cfg(any(test, feature = "test-support"))]
    pub fn finish_for_test(&self, run_id: &str) {
        if let Some(control) = self.get(run_id) {
            control.finish();
        }
    }

    /// Make the next `start_arena_supervisor` fail without spawning.
    #[cfg(any(test, feature = "test-support"))]
    pub fn fail_next_start_for_test(&self) {
        self.fail_next_spawn.store(true, Ordering::SeqCst);
    }

    pub fn ids(&self) -> Vec<String> {
        self.runs
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .keys()
            .cloned()
            .collect()
    }

    pub fn all(&self) -> Vec<Arc<ArenaRunControl>> {
        self.runs
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .values()
            .cloned()
            .collect()
    }

    /// Drop controls whose supervisor has returned. Their durable state is
    /// `run.json`; the live detail has no reader left.
    pub fn prune_finished(&self) {
        self.runs
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .retain(|_, control| !control.is_finished());
    }

    /// Hold the map lock. Test support: the start-order probe proves the
    /// control is registered before its supervisor can run.
    #[cfg(any(test, feature = "test-support"))]
    pub fn hold_for_test(
        &self,
    ) -> std::sync::MutexGuard<'_, HashMap<String, Arc<ArenaRunControl>>> {
        self.runs.lock().unwrap_or_else(|error| error.into_inner())
    }
}

/// Register a run's control, then start its supervisor.
///
/// The registration happens first, under the map lock, so a supervisor that
/// finishes immediately still has a control the host can wait on. If the
/// thread cannot be spawned the error is returned; the caller marks the run
/// failed and closes its record, because nothing else will.
pub(crate) fn start_arena_supervisor(
    controls: &ArenaRunControls,
    control: Arc<ArenaRunControl>,
    supervisor: impl FnOnce(&Arc<ArenaRunControl>) + Send + 'static,
) -> std::io::Result<()> {
    #[cfg(any(test, feature = "test-support"))]
    if controls.fail_next_spawn.swap(false, Ordering::SeqCst) {
        return Err(std::io::Error::other("forced supervisor start failure"));
    }
    controls.insert(&control);
    let thread_control = Arc::clone(&control);
    let spawned = std::thread::Builder::new().spawn(move || {
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| supervisor(&thread_control)));
        if let Err(payload) = result {
            thread_control.record_panic(panic_text(payload));
            mark_run_failed(&thread_control);
        }
        thread_control.finish();
    });
    if spawned.is_err() {
        controls.remove(&control.run_id);
    }
    spawned.map(|_| ())
}

/// A panicking supervisor leaves the run's durable state terminal, so a host
/// that only reads `run.json` is not left waiting on a live status.
fn mark_run_failed(control: &ArenaRunControl) {
    let paths = ArenaPaths::for_run(Path::new(&control.er_dir), &control.run_id);
    if let Ok(mut run) = load_run(&paths) {
        if !matches!(
            run.status,
            RunStatus::Complete | RunStatus::Cancelled | RunStatus::Failed
        ) {
            run.status = RunStatus::Failed;
            run.completed_at = Some(crate::app::chrono_now());
            let _ = save_run(&paths, &run);
        }
    }
    control.set_status(RunStatus::Failed);
}

fn panic_text(payload: Box<dyn std::any::Any + Send>) -> String {
    match payload.downcast::<String>() {
        Ok(message) => *message,
        Err(payload) => match payload.downcast::<&str>() {
            Ok(message) => (*message).to_string(),
            Err(_) => "arena supervisor panicked".to_string(),
        },
    }
}

/// A fresh arena run id: the wall clock plus a per-process sequence.
pub fn new_run_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("arena-{ms}-{n}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn an_immediately_finishing_supervisor_is_recorded_finished() {
        let controls = ArenaRunControls::default();
        let control = controls.new_control("run-fast", "/tmp/bucket");
        start_arena_supervisor(&controls, Arc::clone(&control), |_| {})
            .expect("the supervisor starts");
        assert!(
            control.wait_finished(Duration::from_secs(2)),
            "a supervisor that returns at once must be observable as finished"
        );
        assert!(controls.contains("run-fast"));
        controls.prune_finished();
        assert!(!controls.contains("run-fast"));
    }

    #[test]
    fn a_panicking_supervisor_is_recorded_and_notifies() {
        let controls = ArenaRunControls::default();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let notify_calls = Arc::clone(&calls);
        controls.set_notify(Arc::new(move || {
            notify_calls.fetch_add(1, Ordering::SeqCst);
        }));
        let control = controls.new_control("run-panic", "/tmp/does-not-exist");
        start_arena_supervisor(&controls, Arc::clone(&control), |_| {
            panic!("supervisor boom");
        })
        .expect("the supervisor starts");
        assert!(control.wait_finished(Duration::from_secs(2)));
        assert_eq!(control.panic_message().as_deref(), Some("supervisor boom"));
        assert!(
            calls.load(Ordering::SeqCst) >= 1,
            "the host was never notified"
        );
        assert_eq!(control.status(), RunStatus::Failed);
    }

    #[test]
    fn a_poisoned_progress_lock_still_records_events() {
        let controls = ArenaRunControls::default();
        let control = controls.new_control("run-poisoned", "/tmp/bucket");
        let poisoned = Arc::clone(&control);
        let _ = std::panic::catch_unwind(AssertUnwindSafe(move || {
            let _guard = poisoned.progress.lock().unwrap();
            panic!("poison the progress lock");
        }));
        control.emit(&ProgressEvent::ReviewerThinking {
            reviewer_id: "general::p-m".into(),
            round: 1,
        });
        assert!(
            control.progress().thinking.contains("general::p-m"),
            "a poisoned lock must not take the supervisor down"
        );
    }

    #[test]
    fn a_poisoned_control_map_still_serves_its_runs() {
        let controls = ArenaRunControls::default();
        controls.insert_for_test("run-live", "/tmp/bucket", RunStatus::Running { round: 1 });
        let poisoned = controls.clone();
        let _ = std::panic::catch_unwind(AssertUnwindSafe(move || {
            let _guard = poisoned.runs.lock().unwrap();
            panic!("poison the control map");
        }));

        let live = controls
            .get("run-live")
            .expect("a poisoned map still serves a registered run");
        live.cancel();
        assert!(live.is_cancelled(), "the run could not be cancelled");

        let fresh = controls.new_control("run-new", "/tmp/bucket");
        controls.insert(&fresh);
        assert!(
            controls.get("run-new").is_some(),
            "a new run could not register"
        );
        assert_eq!(controls.active_ids().len(), 2);
    }

    #[test]
    fn a_registered_control_is_visible_before_its_supervisor_runs() {
        let controls = ArenaRunControls::default();
        let control = controls.new_control("run-order", "/tmp/bucket");
        let (started_tx, started_rx) = mpsc::channel();
        let held = controls.hold_for_test();
        let starter = {
            let controls = controls.clone();
            let control = Arc::clone(&control);
            std::thread::spawn(move || {
                start_arena_supervisor(&controls, control, move |_| {
                    let _ = started_tx.send(());
                })
            })
        };
        let early = started_rx.recv_timeout(Duration::from_millis(300));
        drop(held);
        starter.join().unwrap().expect("the supervisor starts");
        assert!(
            early.is_err(),
            "the supervisor ran before its control was registered"
        );
        started_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("the supervisor runs after registration");
    }
}
