//! Environment reads that a test can override for its own thread.
//!
//! Production reads the process environment. In a test build the read goes
//! through [`crate::test_support`] first, so parallel tests never mutate the
//! process environment another thread is reading.

/// The variable's value: a test's thread-scoped override when present, else the
/// process environment.
pub(crate) fn var(name: &str) -> Option<String> {
    #[cfg(any(test, feature = "test-support"))]
    {
        crate::test_support::var(name)
    }
    #[cfg(not(any(test, feature = "test-support")))]
    {
        std::env::var(name).ok()
    }
}
