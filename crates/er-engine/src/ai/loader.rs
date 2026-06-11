use super::comments::{ErFeedback, ErGitHubComments, ErQuestions};
use super::experts::{
    expert_by_id, load_expert_reviews, merge_experts_into_review, synthesize_review_from_experts,
};
use super::professor::{load_professor_review, merge_professor_into_review};
use super::review::*;
use super::triage::load_triage_review;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::Path;

const MAX_SIDECAR_BYTES: u64 = 10_000_000;

/// Read a sidecar file with a size limit to prevent memory spikes from large/adversarial files.
fn read_sidecar(path: &Path) -> std::io::Result<String> {
    let metadata = std::fs::metadata(path)?;
    if metadata.len() > MAX_SIDECAR_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "Sidecar file too large: {} bytes (limit {})",
                metadata.len(),
                MAX_SIDECAR_BYTES
            ),
        ));
    }
    std::fs::read_to_string(path)
}

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
// `DefaultHasher` has no stability guarantee across Rust releases or program runs,
// so this hash is for in-process change detection only — never persist or compare it across processes.
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
            // For renames ("diff --git a/old.rs b/new.rs") this extracts the old path,
            // so per-file staleness lookups keyed by the new path miss renamed files.
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

/// True when `stored` names the same branch as `expected` (exact or slug match).
pub fn storage_branches_match(expected: &str, stored: &str) -> bool {
    if expected == stored {
        return true;
    }
    crate::storage::slug_branch(expected) == crate::storage::slug_branch(stored)
}

/// Branch name from AI-generated `summary.md` titles (`# Branch Review: …`).
pub fn summary_declares_branch(summary: &str) -> Option<String> {
    for line in summary.lines() {
        let trimmed = line.trim();
        let rest = trimmed
            .strip_prefix("# Branch Review:")
            .or_else(|| trimmed.strip_prefix("## Branch Review:"))
            .map(str::trim);
        if let Some(branch) = rest {
            if !branch.is_empty() {
                return Some(branch.to_string());
            }
        }
    }
    None
}

/// True when sidecars in `er_dir` clearly belong to another branch than `scope`.
pub fn artifacts_branch_mismatch(er_dir: &Path, scope: &str) -> bool {
    let review_path = er_dir.join("review.json");
    if let Ok(content) = read_sidecar(&review_path) {
        if let Ok(review) = serde_json::from_str::<ErReview>(&content) {
            if !review.head_branch.is_empty() && !storage_branches_match(scope, &review.head_branch)
            {
                return true;
            }
        }
    }
    let summary_path = er_dir.join("summary.md");
    if let Ok(content) = read_sidecar(&summary_path) {
        if let Some(declared) = summary_declares_branch(&content) {
            if !storage_branches_match(scope, &declared) {
                return true;
            }
        }
    }
    false
}

/// Load all .er-* files from the er_dir and check staleness against current diff hash.
///
/// When `branch_scope` is set (viewed branch or checkout), `review.json` with a
/// non-empty `head_branch` that does not match is ignored — including `summary.md`
/// and other review artifacts from the same directory. This avoids showing a prior
/// branch's review after mistaken migration into the wrong managed folder.
pub fn load_ai_state(er_dir: &str, current_diff_hash: &str, branch_scope: Option<&str>) -> AiState {
    let mut state = AiState::default();
    let er_path = Path::new(er_dir);

    if let Some(scope) = branch_scope {
        if artifacts_branch_mismatch(er_path, scope) {
            return state;
        }
    }

    // Load .er/review.json
    let review_path = er_path.join("review.json");
    if let Ok(content) = read_sidecar(&review_path) {
        // A sidecar that fails to deserialize is treated the same as an absent file.
        if let Ok(review) = serde_json::from_str::<ErReview>(&content) {
            state.is_stale = review.diff_hash != current_diff_hash;
            state.review = Some(review);
        }
    }

    // Load .er/order.json
    let order_path = Path::new(er_dir).join("order.json");
    if let Ok(content) = read_sidecar(&order_path) {
        if let Ok(order) = serde_json::from_str::<ErOrder>(&content) {
            // Check staleness against review hash or independently
            if !state.is_stale && order.diff_hash != current_diff_hash {
                state.is_stale = true;
            }
            state.order = Some(order);
        }
    }

    // Load .er/summary.md
    let summary_path = Path::new(er_dir).join("summary.md");
    if let Ok(content) = read_sidecar(&summary_path) {
        if !content.trim().is_empty() {
            state.summary = Some(content);
        }
    }

    // Load .er/checklist.json
    let checklist_path = Path::new(er_dir).join("checklist.json");
    if let Ok(content) = read_sidecar(&checklist_path) {
        if let Ok(checklist) = serde_json::from_str::<ErChecklist>(&content) {
            if !state.is_stale && checklist.diff_hash != current_diff_hash {
                state.is_stale = true;
            }
            state.checklist = Some(checklist);
        }
    }

    // Load .er/questions.json (personal review questions)
    let questions_path = Path::new(er_dir).join("questions.json");
    if let Ok(content) = read_sidecar(&questions_path) {
        if let Ok(questions) = serde_json::from_str::<ErQuestions>(&content) {
            state.questions = Some(questions);
        }
    }

    // Load .er/github-comments.json (GitHub PR comments)
    let gh_comments_path = Path::new(er_dir).join("github-comments.json");
    if let Ok(content) = read_sidecar(&gh_comments_path) {
        if let Ok(gh_comments) = serde_json::from_str::<ErGitHubComments>(&content) {
            state.github_comments = Some(gh_comments);
        }
    }

    // Merge specialized expert sidecars into review (load-time only).
    let experts = load_expert_reviews(er_dir);
    for expert in &experts {
        if expert.diff_hash != current_diff_hash {
            continue;
        }
        let summary = expert.summary.trim();
        if summary.is_empty() {
            continue;
        }
        if let Some(def) = expert_by_id(&expert.expert_id) {
            state
                .agent_summaries
                .insert(def.label.to_string(), summary.to_string());
        }
    }
    if let Some(review) = state.review.as_mut() {
        merge_experts_into_review(review, &experts, current_diff_hash);
    } else if let Some(synthetic) = synthesize_review_from_experts(&experts, current_diff_hash) {
        state.review = Some(synthetic);
    }

    if let Some(triage) = load_triage_review(er_dir) {
        if triage.diff_hash != current_diff_hash {
            state.is_stale = true;
        }
        state.triage = Some(triage);
    }

    if let Some(prof) = load_professor_review(er_dir) {
        let summary = prof.summary.trim();
        if prof.diff_hash == current_diff_hash && !summary.is_empty() {
            state.agent_summaries.insert(
                super::professor::PROFESSOR_LABEL.to_string(),
                summary.to_string(),
            );
        }
        if let Some(review) = state.review.as_mut() {
            merge_professor_into_review(review, &prof, current_diff_hash);
        } else {
            let mut review = ErReview {
                version: 1,
                diff_hash: current_diff_hash.to_string(),
                created_at: prof.created_at.clone(),
                base_branch: String::new(),
                head_branch: String::new(),
                files: HashMap::new(),
                file_hashes: HashMap::new(),
            };
            merge_professor_into_review(&mut review, &prof, current_diff_hash);
            if !review.files.is_empty() {
                state.review = Some(review);
            }
        }
    }

    // Load legacy .er-feedback.json (only if new files don't exist — migration support)
    if state.questions.is_none() && state.github_comments.is_none() {
        let feedback_path = Path::new(er_dir).join("feedback.json");
        if let Ok(content) = read_sidecar(&feedback_path) {
            if let Ok(feedback) = serde_json::from_str::<ErFeedback>(&content) {
                state.feedback = Some(feedback);
            }
        }
    }

    state
}

/// Get the mtime of the most recently modified .er-* file
pub fn latest_er_mtime(er_dir: &str) -> Option<std::time::SystemTime> {
    let er_dir = Path::new(er_dir);
    let files = [
        "review.json",
        "order.json",
        "summary.md",
        "checklist.json",
        "feedback.json",
        "questions.json",
        "github-comments.json",
        "triage.json",
        "professor.json",
    ];

    let mut latest = files
        .iter()
        .filter_map(|name| {
            let path = er_dir.join(name);
            std::fs::metadata(&path).ok()?.modified().ok()
        })
        .max();

    let experts_dir = er_dir.join("experts");
    if let Ok(entries) = std::fs::read_dir(&experts_dir) {
        for entry in entries.flatten() {
            if let Ok(m) = entry.metadata().and_then(|m| m.modified()) {
                latest = Some(match latest {
                    Some(prev) => prev.max(m),
                    None => m,
                });
            }
        }
    }

    #[cfg(feature = "ui")]
    if let Some(arena_mtime) = crate::arena::latest_arena_mtime(er_dir) {
        latest = Some(match latest {
            Some(prev) => prev.max(arena_mtime),
            None => arena_mtime,
        });
    }

    latest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_branches_match_compares_slugs() {
        assert!(storage_branches_match("main", "main"));
        assert!(!storage_branches_match(
            "claude/dev-5067-remove-async-media-export",
            "dependabot/npm_and_yarn/foo",
        ));
    }

    #[test]
    fn summary_declares_branch_parses_heading() {
        let md = "# Branch Review: claude/dev-5067\n\nBody.";
        assert_eq!(
            summary_declares_branch(md).as_deref(),
            Some("claude/dev-5067")
        );
    }

    #[test]
    fn load_ai_state_ignores_summary_only_wrong_branch() {
        let dir = tempfile::tempdir().unwrap();
        let er_dir = dir.path().to_str().unwrap();
        std::fs::write(
            dir.path().join("summary.md"),
            "# Branch Review: claude/dev-5067\n\nOld summary.",
        )
        .unwrap();
        let state = load_ai_state(er_dir, "abc", Some("dependabot/npm_and_yarn/foo"));
        assert!(state.review.is_none());
        assert!(state.summary.is_none());
    }

    #[test]
    fn load_ai_state_ignores_review_for_wrong_branch_scope() {
        let dir = tempfile::tempdir().unwrap();
        let er_dir = dir.path().to_str().unwrap();
        let review = serde_json::json!({
            "version": 1,
            "diff_hash": "abc",
            "head_branch": "claude/dev-5067",
            "files": {
                "a.rs": {
                    "risk": "low",
                    "findings": [{
                        "id": "f1",
                        "title": "t",
                        "description": "d",
                        "severity": "low",
                        "category": "logic",
                        "hunk_index": 0
                    }]
                }
            }
        });
        std::fs::write(
            dir.path().join("review.json"),
            serde_json::to_string(&review).unwrap(),
        )
        .unwrap();
        std::fs::write(
            dir.path().join("summary.md"),
            "# Branch Review: claude/dev-5067\n\nOld summary.",
        )
        .unwrap();

        let state = load_ai_state(er_dir, "abc", Some("dependabot/npm_and_yarn/foo"));
        assert!(state.review.is_none());
        assert!(state.summary.is_none());
    }

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
