//! The revision watcher: follows `desktop_revision` and wakes the backend
//! tick on each value it settles on, so the tick runs within about 50 ms of
//! a real change. With no change the tick still runs every 2 s, the floor
//! `resources::tick::INTERVAL` sets.
//!
//! It lives in the lib, so the tick it wakes is tested on the same loop the
//! app runs.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

/// How long a change waits for the rest of its burst, such as several
/// background threads bumping the revision at once.
pub const DEBOUNCE: Duration = Duration::from_millis(40);

/// The pause between reads of the counter.
pub const PERIOD: Duration = Duration::from_millis(80);

/// Follows `counter` until `stop` is set; the app never sets it. Each time
/// the counter moved, waits out the debounce, hands `emit` the settled value
/// and how far it moved since the last emit, then runs `after_emit`. The
/// first read always emits, so a listener learns the starting revision.
pub fn follow(
    counter: &AtomicU64,
    stop: &AtomicBool,
    emit: &dyn Fn(u64, u64),
    after_emit: &dyn Fn(),
) {
    let mut last_emitted = u64::MAX;
    while !stop.load(Ordering::Relaxed) {
        if counter.load(Ordering::Relaxed) != last_emitted {
            std::thread::sleep(DEBOUNCE);
            let coalesced = counter.load(Ordering::Relaxed);
            let delta = coalesced.wrapping_sub(last_emitted);
            last_emitted = coalesced;
            emit(coalesced, delta);
            after_emit();
        }
        std::thread::sleep(PERIOD);
    }
}
