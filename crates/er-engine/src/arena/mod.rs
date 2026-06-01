//! AI Review Arena — multi-round reviewer debate, storage, and snapshot contract.
//!
//! Orchestration (rounds 1–3) lands in later slices; this module provides the data spine
//! and projection helpers consumed by desktop `arena_get` / the Svelte overlay.

mod adapter;
mod agents;
mod identity;
mod import;
mod merge;
mod model;
mod orchestrator;
mod projections;
mod registry;
mod schema;
mod storage;
mod voting;

pub use identity::{canonical_finding_text, finding_id};
pub use model::{ARENA_ARBITER_ROUND, *};
pub use agents::{agent_meta, list_arena_agent_kinds, AgentMeta};
pub use import::import_arena_findings_to_review;
pub use orchestrator::{
    arbiter_display_label, build_arena_diff_preview, default_arbiter_from_hub,
    effective_arena_rounds, estimate_batch_cost_usd, estimate_cost_usd, estimate_latency_sec,
    min_survivors_required, reconcile_stale_runs, resolve_arbiter, scope_git_mode,
    start_arena_batch, start_arena_run, AgentGroupStart, ArenaBatchStartParams, ArenaDiffPreview,
    ArenaStartParams, ARBITER_REVIEWER_ID, ARENA_ROUNDS_V1, DEFAULT_COST_LIMIT_USD, MIN_QUORUM,
};
pub use adapter::is_cancelled_error;
pub use projections::{
    build_arbiter_view, build_funnel, build_matrix, build_snapshot, build_snapshot_with_config,
    ArenaRunSnapshot,
};
pub use registry::{ArenaNotify, ArenaRegistry, new_run_id};
pub use schema::{
    validate_round1_output, validate_round2_output, validate_round3_output, Round1Output,
    Round2Output, Round3Output,
};
pub use storage::{
    append_progress_event, delete_run_dir, latest_arena_mtime, list_run_ids, load_run,
    parse_progress_state, save_run, ArenaPaths, ArenaProgressState, ProgressEvent,
};
