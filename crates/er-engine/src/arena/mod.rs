//! AI Review Arena — multi-round reviewer debate, storage, and snapshot contract.
//!
//! Orchestration (rounds 1–3) lands in later slices; this module provides the data spine
//! and projection helpers consumed by desktop `arena_get` / the Svelte overlay.

mod agents;
mod control;
mod identity;
mod import;
pub(crate) mod jobs;
mod merge;
mod model;
mod orchestrator;
mod panel;
mod progress;
mod projections;
mod provenance;
mod schema;
mod seeded;
mod storage;
mod voting;

#[cfg(test)]
pub(crate) use jobs::ARENA_TEST_ENV_LOCK;

pub use agents::{agent_meta, list_arena_agent_kinds, AgentMeta};
pub use control::{ArenaRunControl, ArenaRunControls};
pub use identity::finding_id;
pub use import::import_arena_findings_to_review;
pub use jobs::fake_arena_call_count;
pub use model::{ARENA_ARBITER_ROUND, *};
pub use orchestrator::{
    arbiter_display_label, build_arena_diff_preview, cross_model_arbiter, cross_provider_arbiter,
    default_arbiter_from_hub, effective_arena_rounds, estimate_batch_cost_usd, estimate_cost_usd,
    estimate_latency_sec, min_survivors_required, reconcile_stale_runs, resolve_arbiter,
    scope_git_mode, start_arena_batch, start_arena_run, start_seeded_run, AgentGroupStart,
    ArenaBatchStartParams, ArenaDiffPreview, ArenaRunInputs, ArenaStartParams, SeededStartParams,
    ARBITER_REVIEWER_ID, ARENA_ROUNDS_V1, DEFAULT_COST_LIMIT_USD, MIN_QUORUM,
};
pub(crate) use orchestrator::{reconcile_stale_runs_excluding, start_review_flow_run};
pub use progress::{ArenaNotify, ArenaProgress, ArenaProgressState, ProgressEvent};
pub use projections::{
    build_arbiter_view, build_funnel, build_matrix, build_snapshot, build_snapshot_with_config,
    ArenaRunSnapshot,
};
pub use provenance::{normalized_raisers_for, providers_for};
pub use schema::{
    validate_round1_output, validate_round2_output, validate_round3_output, Round1Output,
    Round2Output, Round3Output,
};
// `seeded` is reached by `orchestrator` through `super::seeded::`, and nothing
// outside the arena uses it, so it stays unexported rather than widening the
// public surface with no caller.
pub use storage::{
    delete_run_dir, latest_arena_mtime, list_run_ids, load_run, save_run, ArenaPaths,
};
