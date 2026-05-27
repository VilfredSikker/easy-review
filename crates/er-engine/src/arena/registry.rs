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

#[derive(Clone)]
pub struct ArenaRegistry {
    runs: Arc<Mutex<HashMap<String, ArenaRunHandle>>>,
    pub notify: ArenaNotify,
}

impl ArenaRegistry {
    pub fn new(notify: ArenaNotify) -> Self {
        Self {
            runs: Arc::new(Mutex::new(HashMap::new())),
            notify,
        }
    }

    pub fn notify_progress(&self) {
        (self.notify)();
    }

    pub fn insert(&self, run_id: String, handle: ArenaRunHandle) {
        if let Ok(mut map) = self.runs.lock() {
            map.insert(run_id, handle);
        }
    }

    pub fn take(&self, run_id: &str) -> Option<ArenaRunHandle> {
        self.runs.lock().ok()?.remove(run_id)
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
