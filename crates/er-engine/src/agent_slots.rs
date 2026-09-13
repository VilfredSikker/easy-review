//! Process-wide cap on concurrently running AI agent subprocesses.
//!
//! The background review queue (`App::poll_background_tasks` dispatch) and
//! arena reviewer rounds acquire a slot here before spawning an agent process,
//! so starting many reviews or several arena runs at once cannot fork more than
//! `ai_hub.max_concurrent_reviews` of *those* processes in parallel.
//!
//! This is not a cap on every spawn path. The tab-level `spawn_agent_prompt`,
//! card AI, the configured shell command (`App::spawn_command`), and the arena
//! arbiter spawn without taking a slot. See
//! `docs/plans/plan-agent-runner-and-cpu-audit.md` for the full site list.
//!
//! The pool is a counting semaphore built on `Mutex` + `Condvar` so it works
//! from plain OS threads (no async runtime required). Waiters re-check a
//! cancel flag every 200ms so cancelled arena runs stop waiting promptly.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

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

/// Tally a granted slot acquisition.
///
/// Only the success path calls this. A cancelled waiter never held a slot, so
/// counting it would inflate `acquires` and — because it has usually waited
/// longer than a millisecond — let a cancelled wait read as evidence that the
/// cap binds, which is the reading the plan's Phase 0 numbers lean on.
///
/// Exists as a named function so the call sites can be counted in a test: the
/// statistics themselves sit behind `ER_AGENT_TIMING`, and `enabled()` caches
/// its answer once per process, so a test cannot turn them on for itself.
fn record_grant(waited: Duration) {
    #[cfg(test)]
    GRANTS.with(|c| c.set(c.get() + 1));
    crate::agent_timing::record_slot_wait(waited);
}

#[cfg(test)]
thread_local! {
    static GRANTS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Granted acquisitions on this thread. Tests only.
#[cfg(test)]
fn grants() -> u64 {
    GRANTS.with(|c| c.get())
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
        // Started after the lock, so the measured span is slot contention.
        // Time spent acquiring the mutex is not that, and counting it would
        // make a busy mutex look like a saturated pool.
        let started = Instant::now();
        loop {
            if cancel.load(Ordering::SeqCst) {
                // A cancelled waiter never held a slot, so it is not an
                // acquisition; counting it would inflate `acquires` and let a
                // cancelled wait read as evidence the cap binds.
                return None;
            }
            if *active < cap {
                *active += 1;
                record_grant(started.elapsed());
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
    fn a_cancelled_waiter_is_not_counted_as_an_acquisition() {
        // `SlotWaitStats::acquires` is documented as slot acquisitions and the
        // plan reads `blocked` off it as proof the cap binds. A cancelled
        // waiter waits just as long as a granted one but holds nothing, so it
        // must not land in either number.
        let pool = Arc::new(SlotPool::new());
        let cancel = Arc::new(AtomicBool::new(false));
        let a = pool.acquire(1, &cancel).unwrap();

        let before = grants();
        let pool2 = Arc::clone(&pool);
        let cancel2 = Arc::clone(&cancel);
        let waiter = std::thread::spawn(move || {
            let outcome = pool2.acquire(1, &cancel2).is_none();
            (outcome, grants())
        });
        std::thread::sleep(Duration::from_millis(50));
        cancel.store(true, Ordering::SeqCst);

        let (cancelled, grants_on_waiter) = waiter.join().unwrap();
        assert!(cancelled, "waiter should observe cancel");
        assert_eq!(
            grants_on_waiter, 0,
            "a cancelled wait must not be tallied as an acquisition"
        );
        assert_eq!(grants(), before, "and not from this thread either");

        drop(a);

        // The same pool still tallies a real grant, so the assertion above is
        // about the cancel path and not about the counter being dead.
        let cancel3 = AtomicBool::new(false);
        let _b = pool.acquire(1, &cancel3).unwrap();
        assert_eq!(grants(), before + 1, "a granted acquire is counted");
    }

    #[test]
    fn contended_acquire_parks_the_second_spawner() {
        // Phase 0 shape: once the cap is reached a second spawner blocks here
        // for as long as the holder runs. This is exactly the time the
        // App-level queue cannot see, which is why a task parked at this line
        // is still reported to the user as "running".
        let pool = Arc::new(SlotPool::new());
        let holder = {
            let pool = Arc::clone(&pool);
            std::thread::spawn(move || {
                let _slot = pool.acquire_blocking(1);
                std::thread::sleep(Duration::from_millis(120));
            })
        };
        // Let the holder take the only slot before the waiter asks for it.
        std::thread::sleep(Duration::from_millis(20));
        let started = Instant::now();
        let slot = pool.acquire_blocking(1);
        let waited = started.elapsed();
        drop(slot);
        holder.join().unwrap();
        assert!(
            waited >= Duration::from_millis(50),
            "second spawner should park behind the cap, waited {waited:?}"
        );
        assert_eq!(pool.active_count(), 0, "all slots released");
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
