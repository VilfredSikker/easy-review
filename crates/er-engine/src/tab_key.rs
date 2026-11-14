//! The process-local identity of an open tab.
//!
//! Two tabs can describe the same review target: a new tab opens a Working tab
//! on the active repo, and nothing dedupes it. So a key built from the target
//! would merge their state. Each tab gets a fresh key when it is built instead,
//! and keeps it through reorder and close of its neighbours. Keys are never
//! persisted: a restored tab gets a new one (ADR 0068).
//!
//! This half of the engine always compiles, so the desktop and the service
//! name the same type.

use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TabKey(pub(crate) u64);

impl TabKey {
    /// A key no other tab in this process has held.
    pub fn next() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_key_is_new() {
        let a = TabKey::next();
        let b = TabKey::next();
        assert_ne!(a, b);
    }
}
