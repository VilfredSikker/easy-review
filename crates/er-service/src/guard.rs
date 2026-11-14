//! The actor-thread guard.
//!
//! The actor must never block on a port: one `gh` call on its thread would
//! serialize every other command behind it. The guard makes a violation loud
//! at the call site instead of showing up as a slow, ordered queue: the thread
//! that runs the actor loop is marked, and [`WorkspaceDeps::ports`] panics on
//! a marked thread. Effect threads are fresh threads and are not marked.

use std::cell::Cell;

thread_local! {
    static ON_ACTOR: Cell<bool> = const { Cell::new(false) };
}

/// True on a thread running a workspace actor loop.
pub fn on_actor_thread() -> bool {
    ON_ACTOR.with(Cell::get)
}

/// Marks the current thread as an actor thread for the guard's lifetime. The
/// actor loop holds one for its whole run.
pub(crate) struct ActorThread;

impl ActorThread {
    pub(crate) fn enter() -> Self {
        ON_ACTOR.with(|flag| flag.set(true));
        Self
    }
}

impl Drop for ActorThread {
    fn drop(&mut self) {
        ON_ACTOR.with(|flag| flag.set(false));
    }
}

/// Test support: how many times the guard has refused a call. The probe test
/// reads this instead of installing a panic hook, which would be global.
#[cfg(test)]
pub(crate) static TRIPS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Panics when called on an actor thread.
///
/// # Panics
///
/// Always, on an actor thread. Off an actor thread it returns.
pub(crate) fn assert_off_actor() {
    if !on_actor_thread() {
        return;
    }
    #[cfg(test)]
    TRIPS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    panic!("port call on the workspace actor thread");
}
