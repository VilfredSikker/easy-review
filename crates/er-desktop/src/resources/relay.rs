//! The relay: tells the frontend which resources moved.
//!
//! It blocks until an owner publishes, waits out the coalescing window for the
//! rest of the burst, then compares every watched version with the ones it
//! last saw and hands on only the keys that moved. Between bursts it sleeps on
//! the wake channel, so it has no idle loop.

use std::collections::BTreeMap;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use er_protocol::{ResourceChanged, ResourceVersion};

use super::{watched, Watched};
use crate::state::DesktopState;

/// How long after the first unsent change the relay waits for the rest of a
/// burst, the same debounce the revision watcher uses.
pub const COALESCE: Duration = Duration::from_millis(40);

/// A burst's moved keys with their new versions; never empty.
pub type Moved = Vec<(Watched, u64)>;

/// Runs the relay on its own thread until every wake sender is gone. The
/// baseline is read before the thread starts, so a change made once this
/// returns is always handed on.
pub fn spawn(desktop: DesktopState, wake: Receiver<()>, emit: impl Fn(Moved) + Send + 'static) {
    let last = watched(&desktop);
    let _ = std::thread::Builder::new()
        .name("er-resource-relay".into())
        .spawn(move || run(&desktop, last, &wake, &emit));
}

fn run(
    desktop: &DesktopState,
    mut last: BTreeMap<Watched, u64>,
    wake: &Receiver<()>,
    emit: &dyn Fn(Moved),
) {
    while wake.recv().is_ok() {
        let deadline = Instant::now() + COALESCE;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            match wake.recv_timeout(left) {
                Ok(()) => {}
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => break,
            }
        }
        let moved = moved(&mut last, watched(desktop));
        if !moved.is_empty() {
            emit(moved);
        }
    }
}

/// The entries of `now` whose version differs from `last`, with `last`
/// replaced by `now`. A key that left, a closed tab's section, moves nothing:
/// the resource that listed it moved too.
pub fn moved(last: &mut BTreeMap<Watched, u64>, now: BTreeMap<Watched, u64>) -> Moved {
    let moved = now
        .iter()
        .filter(|(key, version)| last.get(key) != Some(version))
        .map(|(key, version)| (*key, *version))
        .collect();
    *last = now;
    moved
}

/// The part of a burst the frontend can fetch.
pub fn wire(moved: &[(Watched, u64)]) -> Vec<ResourceChanged> {
    moved
        .iter()
        .filter_map(|(watched, version)| {
            Some(ResourceChanged {
                key: watched.wire()?,
                version: ResourceVersion(*version),
            })
        })
        .collect()
}
