//! Agent process runtime for easy-review: cancellable subprocesses, the
//! process-wide slot pool and run timing.
//!
//! This crate owns the process lifecycle and nothing about what to run.
//! Provider resolution, prompts and artifact output stay in the engine, and
//! the crate has no dependency on it, so the engine can re-export these
//! modules without a dependency cycle.

pub mod agent_run;
pub mod agent_slots;
pub mod agent_timing;
pub mod decode;
pub mod job;
pub mod provider;
