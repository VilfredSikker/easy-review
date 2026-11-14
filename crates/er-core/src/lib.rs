//! Domain types and the artifact contract shared by the engine, the store and
//! the front ends.
//!
//! The crate is pure: it reads no file, spawns no process and starts no thread.
//! `er::core_io` fails the gate on any of those, and on a dependency outside
//! the crate's allow-list.

pub mod artifact;
pub mod diff;
pub mod slot;
pub mod status;
