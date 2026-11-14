//! Diff hashes: the persisted staleness hash and the fast in-process one.

use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

/// Compute SHA-256 hash of raw diff output (for staleness detection).
/// Used for .er-review.json compatibility where the hash is persisted.
pub fn compute_diff_hash(raw_diff: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(raw_diff.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Compute a fast (non-cryptographic) hash for internal change detection.
///
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
    section_hashes(raw_diff, None)
}

/// Per-file hashes for the named paths only.
///
/// Same sectioning and path extraction as [`compute_per_file_hashes`], but a
/// section whose file is not wanted is skipped without being accumulated or
/// hashed. That is the point: a watch-event refresh only needs hashes for the
/// files whose review state something consults without a user action, and
/// hashing the rest is the cost this removes.
///
/// Hashes for a wanted path are identical to [`compute_per_file_hashes`].
pub fn compute_per_file_hashes_for(
    raw_diff: &str,
    wanted: &HashSet<String>,
) -> HashMap<String, String> {
    section_hashes(raw_diff, Some(wanted))
}

/// Hash of a single file's diff section, or `None` when the diff has no such
/// file. Used to resolve a file's hash at the moment it is marked reviewed,
/// after the watch path has stopped caching hashes for every file.
pub fn compute_per_file_hash(raw_diff: &str, path: &str) -> Option<String> {
    let mut wanted = HashSet::new();
    wanted.insert(path.to_string());
    section_hashes(raw_diff, Some(&wanted)).remove(path)
}

/// One pass over the diff. `None` hashes every section; `Some(set)` hashes only
/// those paths, leaving the others unaccumulated as well as unhashed.
fn section_hashes(raw_diff: &str, wanted: Option<&HashSet<String>>) -> HashMap<String, String> {
    let mut hashes = HashMap::new();
    if matches!(wanted, Some(w) if w.is_empty()) {
        return hashes;
    }
    let mut current_file: Option<String> = None;
    let mut current_section = String::new();
    let mut current_wanted = false;

    for line in raw_diff.lines() {
        if line.starts_with("diff --git a/") {
            // Flush previous section
            if let Some(file) = current_file.take() {
                if current_wanted {
                    hashes.insert(file, compute_diff_hash(&current_section));
                }
            }
            // Parse file path from "diff --git a/path b/path".
            // For renames ("diff --git a/old.rs b/new.rs") this extracts the old
            // path, so per-file staleness lookups keyed by the new path miss
            // renamed files.
            let path = line
                .strip_prefix("diff --git a/")
                .and_then(|rest| rest.split(" b/").next())
                .unwrap_or("")
                .to_string();
            current_wanted = wanted.is_none_or(|w| w.contains(&path));
            current_file = Some(path);
            current_section.clear();
            if current_wanted {
                current_section.push_str(line);
                current_section.push('\n');
            }
        } else if current_file.is_some() && current_wanted {
            current_section.push_str(line);
            current_section.push('\n');
        }
    }

    // Flush last section
    if let Some(file) = current_file {
        if current_wanted {
            hashes.insert(file, compute_diff_hash(&current_section));
        }
    }

    hashes
}
