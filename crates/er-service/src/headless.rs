//! What the MCP server calls: PR resolution, the review queue, the sidecar
//! summary, diagram uploads, and the review feedback reader and writers.
//!
//! These are plain functions over the store and the ports. MCP calls them
//! from its own runtime and starts no workspace actor (ADR 0075).

pub mod diagram_upload;
pub mod pr_resolve;
pub mod pr_review_feedback;
pub mod review_queue;
pub mod sidecar_summary;

pub use er_engine::git::ProdDiffStats;
pub use er_engine::github::{
    gh_pr_prod_diff_stats, parse_github_pr_url, with_gh_sentence, RepoSlug,
};
pub use er_engine::ports::Ports;
pub use er_engine::sidecar_upload::{
    artifact_specs_for_dir, prepare_review_kit, upload_pr_artifacts, SidecarKind,
    UploadArtifactsRequest,
};

/// The store's shared composition, as MCP registers it.
pub fn store_registry() -> er_store::Registry {
    er_engine::store_registry::registry()
}

/// The debug-startup audit of the store, as MCP runs it.
pub fn store_audit() {
    er_engine::store_registry::startup_audit(
        er_engine::store_registry::FrontEnd::Mcp,
        &store_registry(),
    );
}
