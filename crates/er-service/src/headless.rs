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
