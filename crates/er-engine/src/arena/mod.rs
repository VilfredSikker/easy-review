//! AI Review Arena — multi-round reviewer debate, storage, and snapshot contract.
//!
//! Orchestration (rounds 1–3) lands in later slices; this module provides the data spine
//! and projection helpers consumed by desktop `arena_get` / the Svelte overlay.

mod identity;
mod model;
mod projections;
mod schema;
mod storage;

pub use identity::{canonical_finding_text, finding_id};
pub use model::*;
pub use projections::{build_funnel, build_matrix, ArenaRunSnapshot};
pub use schema::{
    validate_round1_output, validate_round2_output, validate_round3_output, Round1Output,
    Round2Output, Round3Output,
};
pub use storage::{append_progress_event, load_run, save_run, ArenaPaths, ProgressEvent};
