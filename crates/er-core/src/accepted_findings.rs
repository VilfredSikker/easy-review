//! One finding the user accepted from an arena run.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::review::Finding;

/// One finding the user accepted from an arena run.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AcceptedFinding {
    /// The review file the row belongs to; `Finding` carries no path.
    pub file: String,
    /// The run finding id (`kept`), for the run's own accept bookkeeping.
    pub run_finding_id: String,
    pub run_id: String,
    pub run_diff_hash: String,
    pub accepted_at: String,
    /// Host-derived provider ids. `Finding.providers` never serializes, so the
    /// row keeps them and the projection restores them after the provenance
    /// clear.
    #[serde(default)]
    pub providers: Vec<String>,
    pub finding: Finding,
}
