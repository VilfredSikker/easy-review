//! Process-wide cap on concurrently running AI agent subprocesses.
//!
//! Two callers acquire a slot here before spawning: the background review
//! queue (`App::poll_background_tasks` dispatch) and arena reviewer rounds.
//! This is deliberately reported as covering *those two paths only* — it is
//! not a cap on every spawn. `App::spawn_agent_prompt` (the TUI AI Hub, one
//! call per action name), `run_card_ai_subprocess` (desktop card AI),
//! `App::spawn_command` and `model_discovery::run_models_command` all spawn
//! the configured agent CLI without taking a slot, so several can run at once
//! on top of `ai_hub.max_concurrent_reviews`. Closing that gap means routing
//! each of those through this pool; until then, don't describe the cap as
//! global — the review queue does gate on it, so the queue's own limit holds.
//!
//! The pool is a counting semaphore built on `Mutex` + `Condvar` so it works
//! from plain OS threads (no async runtime required). Waiters re-check a
//! cancel flag every 200ms so cancelled arena runs stop waiting promptly.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::Duration;

pub struct SlotPool {
    active: Mutex<usize>,
    cv: Condvar,
}

static GLOBAL: SlotPool = SlotPool::new();

/// RAII guard for one agent slot. Dropping it frees the slot and wakes
/// waiting spawners.
pub struct AgentSlotGuard<'a>(&'a SlotPool);

impl Drop for AgentSlotGuard<'_> {
    fn drop(&mut self) {
        let mut active = self.0.active.lock().unwrap_or_else(|e| e.into_inner());
        *active = active.saturating_sub(1);
        drop(active);
        self.0.cv.notify_all();
    }
}

impl SlotPool {
    pub const fn new() -> Self {
        Self {
            active: Mutex::new(0),
            cv: Condvar::new(),
        }
    }

    /// Block until a slot is free (active < cap) or `cancel` is set.
    /// Returns `None` when cancelled while waiting.
    pub fn acquire(&self, cap: usize, cancel: &AtomicBool) -> Option<AgentSlotGuard<'_>> {
        let cap = cap.max(1);
        let mut active = self.active.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if cancel.load(Ordering::SeqCst) {
                return None;
            }
            if *active < cap {
                *active += 1;
                return Some(AgentSlotGuard(self));
            }
            let (guard, _) = self
                .cv
                .wait_timeout(active, Duration::from_millis(200))
                .unwrap_or_else(|e| e.into_inner());
            active = guard;
        }
    }

    /// Acquire without a cancel path.
    pub fn acquire_blocking(&self, cap: usize) -> AgentSlotGuard<'_> {
        static NEVER: AtomicBool = AtomicBool::new(false);
        self.acquire(cap, &NEVER)
            .expect("acquire with never-set cancel flag")
    }

    /// Number of slots currently held.
    pub fn active_count(&self) -> usize {
        *self.active.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Default for SlotPool {
    fn default() -> Self {
        Self::new()
    }
}

/// Block until a slot in the process-wide pool is free or `cancel` is set.
pub fn acquire(cap: usize, cancel: &AtomicBool) -> Option<AgentSlotGuard<'static>> {
    GLOBAL.acquire(cap, cancel)
}

/// Acquire from the process-wide pool without a cancel path (background
/// review queue — the App-level queue already bounds how many workers wait).
pub fn acquire_blocking(cap: usize) -> AgentSlotGuard<'static> {
    GLOBAL.acquire_blocking(cap)
}

/// Slots currently held in the process-wide pool. For debug output.
pub fn active_count() -> usize {
    GLOBAL.active_count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Arc;

    #[test]
    fn cap_limits_concurrency() {
        let pool = Arc::new(SlotPool::new());
        let peak = Arc::new(AtomicUsize::new(0));
        let current = Arc::new(AtomicUsize::new(0));
        let mut handles = Vec::new();
        for _ in 0..8 {
            let pool = Arc::clone(&pool);
            let peak = Arc::clone(&peak);
            let current = Arc::clone(&current);
            handles.push(std::thread::spawn(move || {
                let _slot = pool.acquire_blocking(2);
                let now = current.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(now, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(30));
                current.fetch_sub(1, Ordering::SeqCst);
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        assert!(peak.load(Ordering::SeqCst) <= 2, "cap exceeded");
        assert_eq!(pool.active_count(), 0, "all slots released");
    }

    #[test]
    fn cancel_aborts_waiting() {
        let pool = Arc::new(SlotPool::new());
        let cancel = Arc::new(AtomicBool::new(false));
        // Hold both slots of a cap-2 pool.
        let a = pool.acquire(2, &cancel).unwrap();
        let b = pool.acquire(2, &cancel).unwrap();
        let pool2 = Arc::clone(&pool);
        let cancel2 = Arc::clone(&cancel);
        let waiter = std::thread::spawn(move || pool2.acquire(2, &cancel2).is_none());
        std::thread::sleep(Duration::from_millis(50));
        cancel.store(true, Ordering::SeqCst);
        assert!(waiter.join().unwrap(), "waiter should observe cancel");
        drop(a);
        drop(b);
        assert_eq!(pool.active_count(), 0);
    }

    #[test]
    fn zero_cap_treated_as_one() {
        let pool = SlotPool::new();
        let slot = pool.acquire_blocking(0);
        assert_eq!(pool.active_count(), 1);
        drop(slot);
        assert_eq!(pool.active_count(), 0);
    }
}
