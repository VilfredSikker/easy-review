//! Domain types and the artifact contract shared by the engine, the store and
//! the front ends.
//!
//! The crate is pure: it reads no file, spawns no process and starts no thread.
//! `er::core_io` fails the gate on any of those, and on a dependency outside
//! the crate's allow-list.

pub mod accepted_findings;
pub mod annotations;
pub mod arbiter;
pub mod arena;
pub mod artifact;
pub mod diff;
pub mod experts;
pub mod hash;
pub mod identity;
pub mod panel;
pub mod path_rules;
pub mod professor;
pub mod projection;
pub mod relocate;
pub mod review;
pub mod risk_rules;
pub mod slot;
pub mod slug;
pub mod status;
pub mod validation;
