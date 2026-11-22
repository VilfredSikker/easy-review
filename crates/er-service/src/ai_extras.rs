//! What `annotations(tab)` reads beside the AI state: the finding promotions and whether
//! the review file exists. The load and reload effects read them under the AI section's
//! stamp, so its revision covers them too.

use std::collections::HashMap;

use er_engine::ai;

#[derive(Clone, Debug, Default)]
pub struct AiExtras {
    /// Finding id to the GitHub comment it was promoted to.
    pub promotions: HashMap<String, String>,
    pub has_review_json: bool,
}

/// Reads the extras of a tab showing `er_dir`. Runs on an effect thread.
pub(crate) fn read_extras(er_dir: &str) -> AiExtras {
    AiExtras {
        promotions: ai::load_finding_promotions(er_dir),
        has_review_json: ai::review_file_exists(er_dir),
    }
}
