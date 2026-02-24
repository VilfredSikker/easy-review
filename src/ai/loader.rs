use super::review::*;
use sha2::{Sha256, Digest};
use std::path::Path;

/// Compute SHA-256 hash of raw diff output (for staleness detection)
pub fn compute_diff_hash(raw_diff: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(raw_diff.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Load all .er-* files from a repo root and check staleness against current diff hash
pub fn load_ai_state(repo_root: &str, current_diff_hash: &str) -> AiState {
    let mut state = AiState::default();

    // Load .er-review.json
    let review_path = Path::new(repo_root).join(".er-review.json");
    if let Ok(content) = std::fs::read_to_string(&review_path) {
        match serde_json::from_str::<ErReview>(&content) {
            Ok(review) => {
                state.is_stale = review.diff_hash != current_diff_hash;
                state.review = Some(review);
            }
            Err(e) => {
                log::warn!("Failed to parse .er-review.json: {}", e);
            }
        }
    }

    // Load .er-order.json
    let order_path = Path::new(repo_root).join(".er-order.json");
    if let Ok(content) = std::fs::read_to_string(&order_path) {
        match serde_json::from_str::<ErOrder>(&content) {
            Ok(order) => {
                // Check staleness against review hash or independently
                if !state.is_stale && order.diff_hash != current_diff_hash {
                    state.is_stale = true;
                }
                state.order = Some(order);
            }
            Err(e) => {
                log::warn!("Failed to parse .er-order.json: {}", e);
            }
        }
    }

    // Load .er-summary.md
    let summary_path = Path::new(repo_root).join(".er-summary.md");
    if let Ok(content) = std::fs::read_to_string(&summary_path) {
        if !content.trim().is_empty() {
            state.summary = Some(content);
        }
    }

    // Load .er-checklist.json
    let checklist_path = Path::new(repo_root).join(".er-checklist.json");
    if let Ok(content) = std::fs::read_to_string(&checklist_path) {
        match serde_json::from_str::<ErChecklist>(&content) {
            Ok(checklist) => {
                if !state.is_stale && checklist.diff_hash != current_diff_hash {
                    state.is_stale = true;
                }
                state.checklist = Some(checklist);
            }
            Err(e) => {
                log::warn!("Failed to parse .er-checklist.json: {}", e);
            }
        }
    }

    // Load .er-feedback.json
    let feedback_path = Path::new(repo_root).join(".er-feedback.json");
    if let Ok(content) = std::fs::read_to_string(&feedback_path) {
        match serde_json::from_str::<ErFeedback>(&content) {
            Ok(feedback) => {
                state.feedback = Some(feedback);
            }
            Err(e) => {
                log::warn!("Failed to parse .er-feedback.json: {}", e);
            }
        }
    }

    state
}

/// Get the mtime of the most recently modified .er-* file
pub fn latest_er_mtime(repo_root: &str) -> Option<std::time::SystemTime> {
    let root = Path::new(repo_root);
    let files = [
        ".er-review.json",
        ".er-order.json",
        ".er-summary.md",
        ".er-checklist.json",
        ".er-feedback.json",
    ];

    files
        .iter()
        .filter_map(|name| {
            let path = root.join(name);
            std::fs::metadata(&path)
                .ok()?
                .modified()
                .ok()
        })
        .max()
}
