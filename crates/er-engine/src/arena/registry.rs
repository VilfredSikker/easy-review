use super::model::RunStatus;
use std::collections::HashMap;
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

/// Bumped by the desktop host on every durable arena progress write.
pub type ArenaNotify = Arc<dyn Fn() + Send + Sync>;

pub struct ArenaRunHandle {
    pub cancel: Arc<AtomicBool>,
    pub children: Arc<Mutex<Vec<Child>>>,
    pub status: Arc<Mutex<RunStatus>>,
    pub(crate) join: Option<JoinHandle<()>>,
}

impl ArenaRunHandle {
    pub fn kill_children(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        if let Ok(mut kids) = self.children.lock() {
            for child in kids.iter_mut() {
                let _ = child.kill();
            }
            kids.clear();
        }
    }
}

impl Drop for ArenaRunHandle {
    fn drop(&mut self) {
        self.kill_children();
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

/// Release a run when its supervisor exits, including on panic.
///
/// `release_run` at the end of the supervisor only covers a clean return. A
/// supervisor that unwinds leaves its registry entry behind, and every
/// `is_active` wait then treats the run as live forever.
pub(crate) struct ReleaseOnExit<'a> {
    registry: &'a ArenaRegistry,
    run_id: String,
}

impl<'a> ReleaseOnExit<'a> {
    pub(crate) fn new(registry: &'a ArenaRegistry, run_id: String) -> Self {
        Self { registry, run_id }
    }
}

impl Drop for ReleaseOnExit<'_> {
    fn drop(&mut self) {
        self.registry.release_run(&self.run_id);
    }
}

#[derive(Clone)]
pub struct ArenaRegistry {
    runs: Arc<Mutex<HashMap<String, ArenaRunHandle>>>,
    /// Joins of supervisors that finished and released their run. Test support
    /// only: it lets `join_supervisor` still surface a panic after the entry is
    /// gone.
    #[cfg(any(test, feature = "test-support"))]
    finished_joins: Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
    pub notify: ArenaNotify,
}

impl ArenaRegistry {
    pub fn new(notify: ArenaNotify) -> Self {
        Self {
            runs: Arc::new(Mutex::new(HashMap::new())),
            #[cfg(any(test, feature = "test-support"))]
            finished_joins: Arc::new(Mutex::new(HashMap::new())),
            notify,
        }
    }

    pub fn notify_progress(&self) {
        (self.notify)();
    }

    /// Register a run and start its supervisor, in that order.
    ///
    /// The run is inserted before any thread exists, the join is attached, and
    /// only then is the supervisor released to run. A supervisor that finishes
    /// at once still finds its entry and releases it; one that panics releases
    /// it through [`ReleaseOnExit`]. If the thread cannot be spawned, the
    /// inserted entry is taken back out, so a failed start cannot leak a run.
    pub(crate) fn start_supervised(
        self: &Arc<Self>,
        run_id: String,
        cancel: Arc<AtomicBool>,
        children: Arc<Mutex<Vec<Child>>>,
        status: Arc<Mutex<RunStatus>>,
        supervisor: impl FnOnce(&Arc<ArenaRegistry>) + Send + 'static,
    ) -> std::io::Result<()> {
        self.insert(
            run_id.clone(),
            ArenaRunHandle {
                cancel,
                children,
                status,
                join: None,
            },
        );
        let registry = Arc::clone(self);
        let id = run_id.clone();
        let (start_tx, start_rx) = std::sync::mpsc::channel::<()>();
        // Unnamed on purpose: `storage::storage_root_override` lets only
        // unnamed workers borrow the process-wide test root, and the arena
        // supervisor writes sidecars from this thread.
        let join = std::thread::Builder::new().spawn(move || {
            // The join must be stored before the supervisor is observable;
            // otherwise a fast finish releases a run whose join nobody can
            // reach, and `join_supervisor` would report success.
            let _ = start_rx.recv();
            let _release = ReleaseOnExit::new(&registry, id);
            supervisor(&registry);
        });
        match join {
            Ok(join) => {
                self.attach_join(&run_id, join);
                let _ = start_tx.send(());
                Ok(())
            }
            Err(error) => {
                self.take(&run_id);
                Err(error)
            }
        }
    }

    pub fn insert(&self, run_id: String, handle: ArenaRunHandle) {
        if let Ok(mut map) = self.runs.lock() {
            map.insert(run_id, handle);
        }
    }

    /// Join a run's supervisor thread, surfacing a panic it hit.
    ///
    /// Test-support only: lets an integration test fail with the worker's own
    /// message instead of waiting out its poll deadline. The join is taken from
    /// the live handle, or from the finished set once the supervisor has
    /// released its run, so a panic that races the release still surfaces.
    #[cfg(any(test, feature = "test-support"))]
    pub fn join_supervisor(&self, run_id: &str) -> std::thread::Result<()> {
        let live = self
            .runs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_mut(run_id)
            .and_then(|handle| handle.join.take());
        if let Some(join) = live {
            return join.join();
        }
        let finished = self
            .finished_joins
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(run_id);
        match finished {
            Some(join) => join.join(),
            None => Ok(()),
        }
    }

    pub fn take(&self, run_id: &str) -> Option<ArenaRunHandle> {
        self.runs.lock().ok()?.remove(run_id)
    }

    /// Remove an active run from the registry without joining its supervisor thread.
    ///
    /// Must be called from the supervisor thread when the run finishes. Joining the
    /// stored `JoinHandle` from that same thread would deadlock on macOS (EDEADLK).
    /// Test-support builds keep the join in `finished_joins` so `join_supervisor`
    /// can still wait on it; other builds drop it.
    pub fn release_run(&self, run_id: &str) {
        let Ok(mut map) = self.runs.lock() else {
            return;
        };
        let Some(mut handle) = map.remove(run_id) else {
            return;
        };
        handle.kill_children();
        if let Some(join) = handle.join.take() {
            #[cfg(any(test, feature = "test-support"))]
            self.finished_joins
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(run_id.to_string(), join);
            #[cfg(not(any(test, feature = "test-support")))]
            std::mem::forget(join);
        }
    }

    pub fn get_status(&self, run_id: &str) -> Option<RunStatus> {
        let map = self.runs.lock().ok()?;
        map.get(run_id)
            .and_then(|h| h.status.lock().ok().map(|s| s.clone()))
    }

    pub fn cancel(&self, run_id: &str) -> bool {
        let map = match self.runs.lock() {
            Ok(m) => m,
            Err(_) => return false,
        };
        if let Some(handle) = map.get(run_id) {
            handle.kill_children();
            if let Ok(mut st) = handle.status.lock() {
                *st = RunStatus::Cancelled;
            }
            drop(map);
            self.notify_progress();
            return true;
        }
        false
    }

    pub fn register_child(&self, run_id: &str, child: Child) {
        if let Ok(map) = self.runs.lock() {
            if let Some(h) = map.get(run_id) {
                if let Ok(mut kids) = h.children.lock() {
                    kids.push(child);
                }
            }
        }
    }

    pub fn is_cancelled(&self, run_id: &str) -> bool {
        self.runs
            .lock()
            .ok()
            .and_then(|m| m.get(run_id).map(|h| h.cancel.load(Ordering::SeqCst)))
            .unwrap_or(true)
    }

    /// How many children the run's supervisor has registered with the
    /// registry. A cancel only reaches registered children, so a test that
    /// cancels a live round waits on this rather than on the clock.
    #[cfg(any(test, feature = "test-support"))]
    pub fn registered_child_count(&self, run_id: &str) -> usize {
        self.runs
            .lock()
            .ok()
            .and_then(|map| {
                map.get(run_id)
                    .and_then(|handle| handle.children.lock().ok().map(|kids| kids.len()))
            })
            .unwrap_or(0)
    }

    pub fn set_status(&self, run_id: &str, status: RunStatus) {
        if let Ok(map) = self.runs.lock() {
            if let Some(h) = map.get(run_id) {
                if let Ok(mut st) = h.status.lock() {
                    *st = status;
                }
            }
        }
    }

    pub fn attach_join(&self, run_id: &str, join: JoinHandle<()>) {
        if let Ok(mut map) = self.runs.lock() {
            if let Some(h) = map.get_mut(run_id) {
                h.join = Some(join);
            }
        }
    }

    pub fn active_run_ids(&self) -> Vec<String> {
        self.runs
            .lock()
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default()
    }

    pub fn is_active(&self, run_id: &str) -> bool {
        self.runs.lock().is_ok_and(|m| m.contains_key(run_id))
    }

    /// Register a run as active with a given status, without spawning a
    /// supervisor thread. Test-only: lets unit tests exercise the
    /// active-run-driven paths (e.g. `App::background_arena_runs`).
    #[cfg(test)]
    pub fn insert_active_for_test(&self, run_id: &str, status: RunStatus) {
        self.insert(
            run_id.to_string(),
            ArenaRunHandle {
                cancel: Arc::new(AtomicBool::new(false)),
                children: Arc::new(Mutex::new(Vec::new())),
                status: Arc::new(Mutex::new(status)),
                join: None,
            },
        );
    }
}

pub fn new_run_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("arena-{ms}-{n}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> Arc<ArenaRegistry> {
        Arc::new(ArenaRegistry::new(Arc::new(|| {})))
    }

    fn wait_released(registry: &ArenaRegistry, run_id: &str) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while registry.is_active(run_id) {
            assert!(
                std::time::Instant::now() < deadline,
                "run {run_id} was never released"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    fn start<F>(registry: &Arc<ArenaRegistry>, run_id: &str, supervisor: F)
    where
        F: FnOnce(&Arc<ArenaRegistry>) + Send + 'static,
    {
        registry
            .start_supervised(
                run_id.to_string(),
                Arc::new(AtomicBool::new(false)),
                Arc::new(Mutex::new(Vec::new())),
                Arc::new(Mutex::new(RunStatus::Running { round: 1 })),
                supervisor,
            )
            .expect("the supervisor starts");
    }

    /// Holding the registry map blocks the start before any thread can run.
    /// The old spawn-then-insert order let the supervisor run while the run was
    /// still unregistered, which is the leak this fix closes.
    #[test]
    fn the_supervisor_cannot_start_before_its_run_is_registered() {
        let registry = Arc::new(ArenaRegistry::new(Arc::new(|| {})));
        let held = registry.runs.lock().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let starter = {
            let registry = Arc::clone(&registry);
            std::thread::spawn(move || {
                registry
                    .start_supervised(
                        "ordered".into(),
                        Arc::new(AtomicBool::new(false)),
                        Arc::new(Mutex::new(Vec::new())),
                        Arc::new(Mutex::new(RunStatus::Running { round: 1 })),
                        move |_| {
                            let _ = tx.send(());
                        },
                    )
                    .expect("the supervisor starts");
            })
        };
        let early = rx.recv_timeout(std::time::Duration::from_millis(300));
        drop(held);
        starter.join().unwrap();
        assert!(
            early.is_err(),
            "the supervisor ran before its run was registered"
        );
    }

    /// A panicking supervisor releases through the drop guard instead of
    /// leaving the run active forever.
    #[test]
    fn a_panicking_supervisor_still_releases_its_run() {
        let registry = registry();
        let (tx, rx) = std::sync::mpsc::channel();
        start(&registry, "panics", move |_| {
            let _ = tx.send(());
            panic!("simulated supervisor panic");
        });
        rx.recv_timeout(std::time::Duration::from_secs(2))
            .expect("the supervisor ran");
        wait_released(&registry, "panics");
    }

    /// The panic diagnostic keeps working after the guard released the run:
    /// the join is handed back, so `join_supervisor` waits and returns the
    /// panic instead of reporting a clean finish.
    #[test]
    fn join_supervisor_surfaces_a_panic_after_the_run_released() {
        let registry = registry();
        start(&registry, "panics-join", |_| {
            panic!("supervisor boom");
        });
        wait_released(&registry, "panics-join");
        let payload = registry
            .join_supervisor("panics-join")
            .expect_err("the supervisor panic must surface");
        let message = payload
            .downcast_ref::<&str>()
            .map(|message| (*message).to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_default();
        assert!(message.contains("supervisor boom"), "{message}");
    }
}
