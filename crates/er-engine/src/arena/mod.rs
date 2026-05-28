//! AI Review Arena — multi-round reviewer debate, storage, and snapshot contract.
//!
//! Orchestration (rounds 1–3) lands in later slices; this module provides the data spine
//! and projection helpers consumed by desktop `arena_get` / the Svelte overlay.

mod adapter;
mod identity;
mod merge;
mod model;
mod orchestrator;
mod projections;
mod registry;
mod schema;
mod storage;
mod voting;

pub use identity::{canonical_finding_text, finding_id};
pub use model::*;
pub use orchestrator::{
    effective_arena_rounds, estimate_cost_usd, reconcile_stale_runs, scope_git_mode,
    start_arena_run, ArenaStartParams, ARENA_ROUNDS_V1, DEFAULT_COST_LIMIT_USD, MIN_QUORUM,
};
pub use adapter::is_cancelled_error;
pub use projections::{build_funnel, build_matrix, build_snapshot, ArenaRunSnapshot};
pub use registry::{ArenaNotify, ArenaRegistry, new_run_id};
pub use schema::{
    validate_round1_output, validate_round2_output, validate_round3_output, Round1Output,
    Round2Output, Round3Output,
};
pub use storage::{
    append_progress_event, latest_arena_mtime, list_run_ids, load_run, save_run, ArenaPaths,
    ProgressEvent,
};
