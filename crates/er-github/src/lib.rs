//! GitHub access behind a port.
//!
//! This crate owns the `gh` subprocess. `classify` turns exit codes and
//! `gh api --include` status lines into [`GhError`] in one place
//! (docs/adr/0055-gh-errors-are-classified-by-http-status-then-stderr.md), and
//! `cache` holds the TTL caches the adapter reads through.

pub mod cache;
pub mod classify;
mod error;
#[cfg(test)]
mod test_support;

pub use classify::{classify, is_unknown_command, split_pages, GhHeaders, GhOutput};
pub use error::GhError;
