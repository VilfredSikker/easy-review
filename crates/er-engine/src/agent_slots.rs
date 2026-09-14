//! Process-wide cap on concurrently running AI agent subprocesses.
//!
//! Every path that launches a provider CLI acquires a slot here first, so
//! starting many reviews or several arena runs at once cannot fork more agent
//! processes than the caps allow.
//!
//! Two caps, not one, because the two workloads starve each other. A long
//! background review and a short arena round competing for a single pool means
//! whichever arrives first wins; with separate caps each makes progress, and a
//! shared ceiling still bounds the total so the machine is not asked to run
//! both at full size at once.
//!
//! All three counters live under one mutex, so an acquisition that satisfies
//! both conditions takes both in one step. Acquiring them one after the other
//! would need a lock ordering and could deadlock.
//!
//! The pool is a counting semaphore built on `Mutex` + `Condvar` so it works
//! from plain OS threads (no async runtime required). Waiters re-check a
//! cancel flag every 200ms so cancelled arena runs stop waiting promptly.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

/// Which cap a run is charged against.
///
/// Arena rounds are one workload because they are already bounded by their own
/// round structure; everything a user starts directly is the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Workload {
    Background,
    Arena,
}

#[derive(Default)]
struct PoolState {
    total: usize,
    background: usize,
    arena: usize,
}

impl PoolState {
    fn held(&self, workload: Workload) -> usize {
        match workload {
            Workload::Background => self.background,
            Workload::Arena => self.arena,
        }
    }

    fn held_mut(&mut self, workload: Workload) -> &mut usize {
        match workload {
            Workload::Background => &mut self.background,
            Workload::Arena => &mut self.arena,
        }
    }
}

pub struct SlotPool {
    state: Mutex<PoolState>,
    cv: Condvar,
}

static GLOBAL: SlotPool = SlotPool::new();

/// RAII guard for one agent slot. Dropping it frees the slot and wakes
/// waiting spawners.
pub struct AgentSlotGuard<'a> {
    pool: &'a SlotPool,
    workload: Workload,
}

impl Drop for AgentSlotGuard<'_> {
    fn drop(&mut self) {
        let mut state = self.pool.lock_state();
        state.total = state.total.saturating_sub(1);
        let held = state.held(self.workload).saturating_sub(1);
        *state.held_mut(self.workload) = held;
        drop(state);
        self.pool.cv.notify_all();
    }
}

/// Tally a granted slot acquisition.
///
/// Only the success path calls this: a cancelled waiter never held a slot, so
/// counting it would inflate `acquires` and — usually having waited over a
/// millisecond — also `blocked`, which is the reading the plan's Phase 0
/// numbers lean on.
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
            state: Mutex::new(PoolState {
                total: 0,
                background: 0,
                arena: 0,
            }),
            cv: Condvar::new(),
        }
    }

    fn lock_state(&self) -> std::sync::MutexGuard<'_, PoolState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Block until this workload has room *and* the shared ceiling does, or
    /// `cancel` is set. Returns `None` when cancelled while waiting.
    ///
    /// Both conditions are checked under the same lock and both counters move
    /// in one step, so a caller can never hold one and wait for the other.
    pub fn acquire(
        &self,
        workload: Workload,
        cap: usize,
        ceiling: usize,
        cancel: &AtomicBool,
    ) -> Option<AgentSlotGuard<'_>> {
        let cap = cap.max(1);
        let ceiling = ceiling.max(1);
        let mut state = self.lock_state();
        // Started after the lock, so the measured span is slot contention.
        // Time spent acquiring the mutex is not that, and counting it would
        // make a busy mutex look like a saturated pool.
        let started = Instant::now();
        loop {
            if cancel.load(Ordering::SeqCst) {
                // Deliberately does not record — see `record_grant`.
                return None;
            }
            if state.held(workload) < cap && state.total < ceiling {
                *state.held_mut(workload) += 1;
                state.total += 1;
                record_grant(started.elapsed());
                return Some(AgentSlotGuard {
                    pool: self,
                    workload,
                });
            }
            let (guard, _) = self
                .cv
                .wait_timeout(state, Duration::from_millis(200))
                .unwrap_or_else(|e| e.into_inner());
            state = guard;
        }
    }

    /// Acquire without a cancel path.
    pub fn acquire_blocking(
        &self,
        workload: Workload,
        cap: usize,
        ceiling: usize,
    ) -> AgentSlotGuard<'_> {
        static NEVER: AtomicBool = AtomicBool::new(false);
        self.acquire(workload, cap, ceiling, &NEVER)
            .expect("acquire with never-set cancel flag")
    }

    /// Total slots currently held, across both workloads.
    pub fn active_count(&self) -> usize {
        self.lock_state().total
    }

    /// Slots currently held by one workload.
    pub fn active_for(&self, workload: Workload) -> usize {
        self.lock_state().held(workload)
    }
}

impl Default for SlotPool {
    fn default() -> Self {
        Self::new()
    }
}

/// Block until a slot in the process-wide pool is free or `cancel` is set.
pub fn acquire(
    workload: Workload,
    cap: usize,
    ceiling: usize,
    cancel: &AtomicBool,
) -> Option<AgentSlotGuard<'static>> {
    GLOBAL.acquire(workload, cap, ceiling, cancel)
}

/// Acquire from the process-wide pool without a cancel path (background
/// review queue — the App-level queue already bounds how many workers wait).
pub fn acquire_blocking(workload: Workload, cap: usize, ceiling: usize) -> AgentSlotGuard<'static> {
    GLOBAL.acquire_blocking(workload, cap, ceiling)
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
                let _slot = pool.acquire_blocking(Workload::Background, 2, 2);
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
    fn a_full_background_pool_does_not_starve_an_arena_round() {
        // The starvation this split exists to remove: with one shared pool, a
        // long background review holding every slot stopped arena rounds
        // entirely, and vice versa.
        let pool = SlotPool::new();
        let cancel = AtomicBool::new(false);
        let _background: Vec<_> = (0..2)
            .map(|_| pool.acquire_blocking(Workload::Background, 2, 4))
            .collect();

        let arena = pool.acquire(Workload::Arena, 2, 4, &cancel);
        assert!(
            arena.is_some(),
            "an arena round must not wait on background work"
        );
        assert_eq!(pool.active_for(Workload::Background), 2);
        assert_eq!(pool.active_for(Workload::Arena), 1);
    }

    #[test]
    fn the_shared_ceiling_bounds_both_workloads_together() {
        // Per-workload caps say how big each may get; the ceiling says how big
        // they may get together, so raising one cannot quietly double the load.
        let pool = Arc::new(SlotPool::new());
        let held: Vec<_> = (0..2)
            .map(|_| pool.acquire_blocking(Workload::Background, 3, 3))
            .chain(std::iter::once(pool.acquire_blocking(
                Workload::Arena,
                3,
                3,
            )))
            .collect();
        assert_eq!(pool.active_count(), 3);
        assert!(
            pool.active_for(Workload::Background) < 3,
            "each workload is still under its own cap — only the ceiling is reached"
        );

        let pool2 = Arc::clone(&pool);
        let waiter = std::thread::spawn(move || {
            let cancel = AtomicBool::new(false);
            pool2.acquire(Workload::Arena, 3, 3, &cancel).is_some()
        });
        std::thread::sleep(Duration::from_millis(150));
        assert!(
            !waiter.is_finished(),
            "the ceiling must park a fourth run even with arena room to spare"
        );

        drop(held);
        assert!(waiter.join().unwrap(), "and release it once a slot frees");
        // The waiter's own guard went out of scope with its expression, so the
        // pool is empty again rather than sitting at one.
        assert_eq!(pool.active_count(), 0);
    }

    #[test]
    fn cancel_aborts_waiting() {
        let pool = Arc::new(SlotPool::new());
        let cancel = Arc::new(AtomicBool::new(false));
        // Hold both slots of a cap-2 pool.
        let a = pool.acquire(Workload::Background, 2, 2, &cancel).unwrap();
        let b = pool.acquire(Workload::Background, 2, 2, &cancel).unwrap();
        let pool2 = Arc::clone(&pool);
        let cancel2 = Arc::clone(&cancel);
        let waiter = std::thread::spawn(move || {
            pool2
                .acquire(Workload::Background, 2, 2, &cancel2)
                .is_none()
        });
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
        let a = pool.acquire(Workload::Background, 1, 1, &cancel).unwrap();

        let before = grants();
        let pool2 = Arc::clone(&pool);
        let cancel2 = Arc::clone(&cancel);
        let waiter = std::thread::spawn(move || {
            let outcome = pool2
                .acquire(Workload::Background, 1, 1, &cancel2)
                .is_none();
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
        let _b = pool.acquire(Workload::Background, 1, 1, &cancel3).unwrap();
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
                let _slot = pool.acquire_blocking(Workload::Background, 1, 1);
                std::thread::sleep(Duration::from_millis(120));
            })
        };
        // Let the holder take the only slot before the waiter asks for it.
        std::thread::sleep(Duration::from_millis(20));
        let started = Instant::now();
        let slot = pool.acquire_blocking(Workload::Background, 1, 1);
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
        let slot = pool.acquire_blocking(Workload::Background, 0, 0);
        assert_eq!(pool.active_count(), 1);
        drop(slot);
        assert_eq!(pool.active_count(), 0);
    }
}
