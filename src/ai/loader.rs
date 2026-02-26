use super::review::*;
use sha2::{Sha256, Digest};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::Path;

/// Compute SHA-256 hash of raw diff output (for staleness detection).
/// Used for .er-review.json compatibility where the hash is persisted.
pub fn compute_diff_hash(raw_diff: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(raw_diff.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Compute a fast (non-cryptographic) hash for internal change detection.
/// Much faster than SHA-256 — used for detecting if the diff has changed
/// between ticks without the overhead of a full cryptographic hash.
pub fn compute_diff_hash_fast(raw_diff: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    raw_diff.hash(&mut hasher);
    hasher.finish()
}

/// Split a combined diff into per-file sections and hash each one.
/// Returns a map of file path → SHA-256 hash of that file's diff section.
pub fn compute_per_file_hashes(raw_diff: &str) -> HashMap<String, String> {
    let mut hashes = HashMap::new();
    let mut current_file: Option<String> = None;
    let mut current_section = String::new();

    for line in raw_diff.lines() {
        if line.starts_with("diff --git a/") {
            // Flush previous section
            if let Some(ref file) = current_file {
                let hash = compute_diff_hash(&current_section);
                hashes.insert(file.clone(), hash);
            }
            // Parse file path from "diff --git a/path b/path"
            let path = line
                .strip_prefix("diff --git a/")
                .and_then(|rest| rest.split(" b/").next())
                .unwrap_or("")
                .to_string();
            current_file = Some(path);
            current_section.clear();
            current_section.push_str(line);
            current_section.push('\n');
        } else if current_file.is_some() {
            current_section.push_str(line);
            current_section.push('\n');
        }
    }

    // Flush last section
    if let Some(file) = current_file {
        let hash = compute_diff_hash(&current_section);
        hashes.insert(file, hash);
    }

    hashes
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

    // Load .er-questions.json (personal review questions)
    let questions_path = Path::new(repo_root).join(".er-questions.json");
    if let Ok(content) = std::fs::read_to_string(&questions_path) {
        match serde_json::from_str::<ErQuestions>(&content) {
            Ok(mut questions) => {
                // Per-comment staleness: mark all stale if diff changed
                if questions.diff_hash != current_diff_hash {
                    for q in &mut questions.questions {
                        q.stale = true;
                    }
                }
                state.questions = Some(questions);
            }
            Err(e) => {
                log::warn!("Failed to parse .er-questions.json: {}", e);
            }
        }
    }

    // Load .er-github-comments.json (GitHub PR comments)
    let gh_comments_path = Path::new(repo_root).join(".er-github-comments.json");
    if let Ok(content) = std::fs::read_to_string(&gh_comments_path) {
        match serde_json::from_str::<ErGitHubComments>(&content) {
            Ok(mut gh_comments) => {
                // Per-comment staleness
                if gh_comments.diff_hash != current_diff_hash {
                    for c in &mut gh_comments.comments {
                        c.stale = true;
                    }
                }
                state.github_comments = Some(gh_comments);
            }
            Err(e) => {
                log::warn!("Failed to parse .er-github-comments.json: {}", e);
            }
        }
    }

    // Load legacy .er-feedback.json (only if new files don't exist — migration support)
    if state.questions.is_none() && state.github_comments.is_none() {
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
    }

    state
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_diff_hash_empty_string_returns_known_sha256() {
        let hash = compute_diff_hash("");
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn compute_diff_hash_is_deterministic() {
        let input = "diff --git a/foo.rs b/foo.rs\n+let x = 1;";
        let first = compute_diff_hash(input);
        let second = compute_diff_hash(input);
        assert_eq!(first, second);
    }

    #[test]
    fn compute_diff_hash_different_inputs_produce_different_hashes() {
        let hash_a = compute_diff_hash("input a");
        let hash_b = compute_diff_hash("input b");
        assert_ne!(hash_a, hash_b);
    }

    #[test]
    fn compute_diff_hash_non_empty_produces_64_char_hex() {
        let hash = compute_diff_hash("some diff content");
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    // ── compute_per_file_hashes ──

    #[test]
    fn per_file_hashes_empty_diff_returns_empty() {
        let hashes = compute_per_file_hashes("");
        assert!(hashes.is_empty());
    }

    #[test]
    fn per_file_hashes_single_file() {
        let diff = "diff --git a/src/main.rs b/src/main.rs\nindex abc..def 100644\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,3 +1,4 @@\n+use std::io;\n fn main() {\n }\n";
        let hashes = compute_per_file_hashes(diff);
        assert_eq!(hashes.len(), 1);
        assert!(hashes.contains_key("src/main.rs"));
        assert_eq!(hashes["src/main.rs"].len(), 64);
    }

    #[test]
    fn per_file_hashes_multiple_files() {
        let diff = "diff --git a/foo.rs b/foo.rs\n+line1\ndiff --git a/bar.rs b/bar.rs\n+line2\n";
        let hashes = compute_per_file_hashes(diff);
        assert_eq!(hashes.len(), 2);
        assert!(hashes.contains_key("foo.rs"));
        assert!(hashes.contains_key("bar.rs"));
        assert_ne!(hashes["foo.rs"], hashes["bar.rs"]);
    }

    #[test]
    fn per_file_hashes_deterministic() {
        let diff = "diff --git a/x.rs b/x.rs\n+hello\n";
        let first = compute_per_file_hashes(diff);
        let second = compute_per_file_hashes(diff);
        assert_eq!(first, second);
    }

    #[test]
    fn per_file_hashes_changed_content_changes_hash() {
        let diff_v1 = "diff --git a/x.rs b/x.rs\n+version1\n";
        let diff_v2 = "diff --git a/x.rs b/x.rs\n+version2\n";
        let h1 = compute_per_file_hashes(diff_v1);
        let h2 = compute_per_file_hashes(diff_v2);
        assert_ne!(h1["x.rs"], h2["x.rs"]);
    }
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
        ".er-questions.json",
        ".er-github-comments.json",
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
