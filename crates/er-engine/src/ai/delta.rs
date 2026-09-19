//! Delta re-review: the subset of a Stale review the person is asked to look at.
//!
//! Not a Tour. A Tour that no longer matches its diff is regenerated (ADR 0025).
//! This path filters the Tab's file list. See ADR 0038.

use super::identity::finding_key;
use super::relocate::{relocate_comment, CommentAnchor, RelocationResult};
use super::review::{ErReview, Finding};
use crate::git::DiffFile;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// How many unchanged claims to pull back into view so a skip is not trusted
/// forever. The failure this catches is a bug that was always present and is
/// only now reachable because of a hunk elsewhere.
pub const UNCHANGED_SAMPLE_SIZE: usize = 3;

/// Host-written baseline of per-file hashes for the review's `diff_hash`.
///
/// `review.json` is AI-owned, and agents leave `file_hashes` empty. Without a
/// baseline the skip has nothing to compare. This file is mechanical, not a
/// Finding, and is ignored by the sidecar mtime scan.
pub const HOST_HASHES_FILE: &str = "review-hashes.json";

/// The files and Findings a Stale review asks the person to look at.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeltaSet {
    /// Files whose per-file hash moved, plus files new in the current diff.
    pub changed_files: HashSet<String>,
    /// Union of `changed_files`, files of moved Findings, and files of sampled
    /// unchanged claims. This is the file list.
    pub files: HashSet<String>,
    /// Files whose hash did not move and that were not pulled in for a Finding.
    pub skipped_files: HashSet<String>,
    /// `finding_key` values that relocated or were lost.
    pub moved_finding_keys: HashSet<String>,
    /// Unchanged claims drawn so silent persistence is not trusted forever.
    pub sample_finding_keys: HashSet<String>,
}

impl DeltaSet {
    /// True when at least one file would be hidden. Without a baseline the skip
    /// cannot run, and toggling would be a no-op.
    pub fn can_filter(&self) -> bool {
        !self.skipped_files.is_empty()
    }

    pub fn contains_file(&self, path: &str) -> bool {
        self.files.contains(path)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HostFileHashes {
    review_diff_hash: String,
    file_hashes: HashMap<String, String>,
}

/// Hashes to compare the current diff against.
///
/// Prefer the review sidecar's `file_hashes`. If the agent left them empty and
/// the review still matches the current diff, snapshot current hashes into the
/// host file so a later Stale load can skip. If the review is already Stale,
/// read that host file when it still names this review's `diff_hash`.
pub fn resolve_baseline_hashes(
    review: &ErReview,
    current_hashes: &HashMap<String, String>,
    current_diff_hash: &str,
    er_dir: Option<&Path>,
) -> HashMap<String, String> {
    if !review.file_hashes.is_empty() {
        return review.file_hashes.clone();
    }
    if review.diff_hash == current_diff_hash {
        if let Some(dir) = er_dir {
            write_host_hashes(dir, &review.diff_hash, current_hashes);
        }
        return current_hashes.clone();
    }
    if let Some(dir) = er_dir {
        if let Some((stored_hash, hashes)) = read_host_hashes(dir) {
            if stored_hash == review.diff_hash {
                return hashes;
            }
        }
    }
    HashMap::new()
}

fn host_hashes_path(er_dir: &Path) -> std::path::PathBuf {
    er_dir.join(HOST_HASHES_FILE)
}

fn read_host_hashes(er_dir: &Path) -> Option<(String, HashMap<String, String>)> {
    let raw = std::fs::read_to_string(host_hashes_path(er_dir)).ok()?;
    let parsed: HostFileHashes = serde_json::from_str(&raw).ok()?;
    Some((parsed.review_diff_hash, parsed.file_hashes))
}

fn write_host_hashes(er_dir: &Path, review_diff_hash: &str, file_hashes: &HashMap<String, String>) {
    if !er_dir.is_dir() || file_hashes.is_empty() {
        return;
    }
    let payload = HostFileHashes {
        review_diff_hash: review_diff_hash.to_string(),
        file_hashes: file_hashes.clone(),
    };
    let Ok(json) = serde_json::to_string_pretty(&payload) else {
        return;
    };
    let path = host_hashes_path(er_dir);
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, json).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

/// Build the delta set for a Stale review.
///
/// `baseline` is the per-file hashes from when the review was produced.
/// `current_hashes` is the live diff. `diff_files` are parsed files used to
/// relocate Finding anchors; an unparsed stub does not count as a move.
pub fn compute_delta_set(
    review: &ErReview,
    baseline: &HashMap<String, String>,
    current_hashes: &HashMap<String, String>,
    diff_files: &[DiffFile],
) -> DeltaSet {
    let files_by_path: HashMap<&str, &DiffFile> =
        diff_files.iter().map(|f| (f.path.as_str(), f)).collect();

    let mut changed_files = HashSet::new();
    let mut skipped_files = HashSet::new();

    if baseline.is_empty() {
        return DeltaSet {
            changed_files: current_hashes.keys().cloned().collect(),
            files: current_hashes.keys().cloned().collect(),
            skipped_files,
            moved_finding_keys: HashSet::new(),
            sample_finding_keys: HashSet::new(),
        };
    }

    for (path, current) in current_hashes {
        match baseline.get(path) {
            Some(old) if old == current => {
                skipped_files.insert(path.clone());
            }
            Some(_) => {
                changed_files.insert(path.clone());
            }
            None => {
                changed_files.insert(path.clone());
            }
        }
    }

    let mut moved_finding_keys = HashSet::new();
    let mut moved_finding_files = HashSet::new();
    let mut unchanged_on_skipped: Vec<(String, String)> = Vec::new();

    for (path, file_review) in &review.files {
        for finding in &file_review.findings {
            if !finding.is_active() {
                continue;
            }
            let key = finding_key(path, finding.line_start, &finding.title);
            if finding_key_moved(path, finding, current_hashes, &files_by_path) {
                moved_finding_keys.insert(key);
                skipped_files.remove(path);
                if current_hashes.contains_key(path) {
                    moved_finding_files.insert(path.clone());
                }
            } else if skipped_files.contains(path) {
                unchanged_on_skipped.push((key, path.clone()));
            }
        }
    }

    unchanged_on_skipped.sort_by(|a, b| a.0.cmp(&b.0));
    let sample_finding_keys = sample_keys(
        &unchanged_on_skipped
            .iter()
            .map(|(k, _)| k.clone())
            .collect::<Vec<_>>(),
        UNCHANGED_SAMPLE_SIZE,
    );
    let sample_set: HashSet<String> = sample_finding_keys.iter().cloned().collect();
    for (key, path) in &unchanged_on_skipped {
        if sample_set.contains(key) {
            skipped_files.remove(path);
        }
    }

    let mut files = changed_files.clone();
    files.extend(moved_finding_files);
    for (key, path) in &unchanged_on_skipped {
        if sample_set.contains(key) && current_hashes.contains_key(path) {
            files.insert(path.clone());
        }
    }

    DeltaSet {
        changed_files,
        files,
        skipped_files,
        moved_finding_keys,
        sample_finding_keys: sample_set,
    }
}

fn sample_keys(sorted_keys: &[String], n: usize) -> Vec<String> {
    if sorted_keys.len() <= n {
        return sorted_keys.to_vec();
    }
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let idx = i * sorted_keys.len() / n;
        let key = &sorted_keys[idx];
        if out.last() == Some(key) {
            continue;
        }
        out.push(key.clone());
    }
    out
}

fn finding_key_moved(
    path: &str,
    finding: &Finding,
    current_hashes: &HashMap<String, String>,
    files_by_path: &HashMap<&str, &DiffFile>,
) -> bool {
    if finding.line_content.is_empty() || finding.line_start.is_none() {
        return false;
    }
    if !current_hashes.contains_key(path) {
        return true;
    }
    let Some(diff_file) = files_by_path.get(path) else {
        return false;
    };
    if diff_file.hunks.is_empty() {
        return false;
    }
    let orig_line = finding.line_start;
    let orig_key = finding_key(path, orig_line, &finding.title);
    let anchor = CommentAnchor {
        hunk_index: finding.hunk_index,
        line_start: finding.line_start,
        line_content: finding.line_content.clone(),
        context_before: Vec::new(),
        context_after: Vec::new(),
        old_line_start: None,
        hunk_header: String::new(),
    };
    match relocate_comment(&anchor, diff_file) {
        RelocationResult::Unchanged => false,
        RelocationResult::Relocated { new_line_start, .. } => {
            finding_key(path, Some(new_line_start), &finding.title) != orig_key
        }
        RelocationResult::Lost => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::review::{Confidence, RiskLevel};
    use crate::git::{DiffHunk, DiffLine, FileStatus, LineType};

    fn hashes(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(p, h)| ((*p).to_string(), (*h).to_string()))
            .collect()
    }

    fn finding_at(id: &str, title: &str, line: usize, content: &str) -> Finding {
        Finding {
            id: id.to_string(),
            severity: RiskLevel::Low,
            lens: String::new(),
            category: String::new(),
            raised_by: Vec::new(),
            title: title.to_string(),
            description: String::new(),
            hunk_index: Some(0),
            line_start: Some(line),
            line_end: Some(line),
            line_content: content.to_string(),
            stale: false,
            suggestion: String::new(),
            related_files: Vec::new(),
            outside_diff: false,
            confidence: Confidence::default(),
            verification_plan: String::new(),
            evidence: Vec::new(),
            responses: Vec::new(),
            resolved: false,
            resolved_note: String::new(),
            resolved_at: String::new(),
            promoted_to: None,
        }
    }

    fn review_with(
        files: Vec<(&str, Vec<Finding>)>,
        file_hashes: HashMap<String, String>,
    ) -> ErReview {
        let mut map = HashMap::new();
        for (path, findings) in files {
            map.insert(
                path.to_string(),
                super::super::review::ErFileReview {
                    risk: RiskLevel::Low,
                    risk_reason: String::new(),
                    summary: String::new(),
                    findings,
                },
            );
        }
        ErReview {
            version: 1,
            diff_hash: "old".into(),
            created_at: String::new(),
            base_branch: String::new(),
            head_branch: String::new(),
            files: map,
            file_hashes,
        }
    }

    fn diff_file(path: &str, lines: Vec<(&str, usize)>) -> DiffFile {
        let parsed: Vec<DiffLine> = lines
            .into_iter()
            .map(|(content, num)| DiffLine {
                line_type: LineType::Context,
                content: content.to_string(),
                old_num: Some(num),
                new_num: Some(num),
            })
            .collect();
        let n = parsed.len();
        DiffFile {
            path: path.to_string(),
            status: FileStatus::Modified,
            hunks: vec![DiffHunk {
                header: "@@ -1,3 +1,3 @@".into(),
                old_start: 1,
                old_count: n,
                new_start: 1,
                new_count: n,
                lines: parsed,
            }],
            adds: 0,
            dels: 0,
            compacted: false,
            raw_hunk_count: 1,
        }
    }

    #[test]
    fn hash_moved_file_is_shown_and_unchanged_file_is_skipped() {
        let review = review_with(
            vec![("a.rs", vec![]), ("b.rs", vec![])],
            hashes(&[("a.rs", "old-a"), ("b.rs", "old-b")]),
        );
        let current = hashes(&[("a.rs", "new-a"), ("b.rs", "old-b")]);
        let delta = compute_delta_set(&review, &review.file_hashes, &current, &[]);
        assert!(delta.files.contains("a.rs"));
        assert!(delta.changed_files.contains("a.rs"));
        assert!(delta.skipped_files.contains("b.rs"));
        assert!(!delta.files.contains("b.rs"));
        assert!(delta.can_filter());
    }

    #[test]
    fn new_file_is_shown() {
        let review = review_with(vec![], hashes(&[("a.rs", "old-a")]));
        let current = hashes(&[("a.rs", "old-a"), ("c.rs", "new-c")]);
        let delta = compute_delta_set(&review, &review.file_hashes, &current, &[]);
        assert!(delta.files.contains("c.rs"));
        assert!(delta.skipped_files.contains("a.rs"));
    }

    #[test]
    fn relocated_finding_pulls_its_file_out_of_the_skip() {
        let finding = finding_at("f1", "null check", 1, "fn foo() {");
        let review = review_with(vec![("a.rs", vec![finding])], hashes(&[("a.rs", "same")]));
        let current = hashes(&[("a.rs", "same")]);
        let file = diff_file(
            "a.rs",
            vec![("// header", 1), ("fn foo() {", 2), ("    let x = 1;", 3)],
        );
        let delta = compute_delta_set(&review, &review.file_hashes, &current, &[file]);
        assert!(
            delta.files.contains("a.rs"),
            "relocated claim must be shown"
        );
        assert!(delta.skipped_files.is_empty());
        assert_eq!(delta.moved_finding_keys.len(), 1);
    }

    #[test]
    fn lost_finding_counts_as_moved() {
        let finding = finding_at("f1", "gone", 2, "    let gone = 2;");
        let review = review_with(vec![("a.rs", vec![finding])], hashes(&[("a.rs", "same")]));
        let current = hashes(&[("a.rs", "same")]);
        let file = diff_file("a.rs", vec![("fn foo() {", 1), ("    let kept = 1;", 2)]);
        let delta = compute_delta_set(&review, &review.file_hashes, &current, &[file]);
        assert_eq!(delta.moved_finding_keys.len(), 1);
        assert!(delta.files.contains("a.rs"));
    }

    #[test]
    fn unchanged_claims_on_skipped_files_are_sampled() {
        let mut findings = Vec::new();
        let mut files = Vec::new();
        let mut baseline = HashMap::new();
        let mut current = HashMap::new();
        let mut diffs = Vec::new();
        for i in 0..6 {
            let path = format!("f{i}.rs");
            let content = "    let kept = 1;";
            findings.push((
                path.clone(),
                vec![finding_at(
                    &format!("f{i}"),
                    &format!("claim {i}"),
                    1,
                    content,
                )],
            ));
            files.push(path.clone());
            baseline.insert(path.clone(), "same".into());
            current.insert(path.clone(), "same".into());
            diffs.push(diff_file(&path, vec![(content, 1)]));
        }
        let review = review_with(
            findings
                .iter()
                .map(|(p, fs)| (p.as_str(), fs.clone()))
                .collect(),
            baseline.clone(),
        );
        let delta = compute_delta_set(&review, &baseline, &current, &diffs);
        assert_eq!(delta.sample_finding_keys.len(), UNCHANGED_SAMPLE_SIZE);
        assert_eq!(delta.files.len(), UNCHANGED_SAMPLE_SIZE);
        assert_eq!(delta.skipped_files.len(), 6 - UNCHANGED_SAMPLE_SIZE);
        assert!(delta.moved_finding_keys.is_empty());
        let again = compute_delta_set(&review, &baseline, &current, &diffs);
        assert_eq!(delta.sample_finding_keys, again.sample_finding_keys);
    }

    #[test]
    fn empty_baseline_shows_every_current_file() {
        let review = review_with(vec![("a.rs", vec![]), ("b.rs", vec![])], HashMap::new());
        let current = hashes(&[("a.rs", "x"), ("b.rs", "y")]);
        let delta = compute_delta_set(&review, &HashMap::new(), &current, &[]);
        assert_eq!(delta.files.len(), 2);
        assert!(delta.skipped_files.is_empty());
        assert!(!delta.can_filter());
    }

    #[test]
    fn resolved_finding_is_ignored() {
        let mut finding = finding_at("f1", "fixed", 1, "fn foo() {");
        finding.resolved = true;
        let review = review_with(vec![("a.rs", vec![finding])], hashes(&[("a.rs", "same")]));
        let current = hashes(&[("a.rs", "same")]);
        let file = diff_file("a.rs", vec![("fn foo() {", 2)]);
        let delta = compute_delta_set(&review, &review.file_hashes, &current, &[file]);
        assert!(delta.moved_finding_keys.is_empty());
        assert!(delta.skipped_files.contains("a.rs"));
    }

    #[test]
    fn host_hashes_round_trip_for_a_fresh_review() {
        let dir = tempfile::tempdir().unwrap();
        let current = hashes(&[("a.rs", "abc")]);
        let review = review_with(vec![], HashMap::new());
        let mut review = review;
        review.diff_hash = "live".into();
        let got = resolve_baseline_hashes(&review, &current, "live", Some(dir.path()));
        assert_eq!(got, current);
        let later = hashes(&[("a.rs", "changed")]);
        let stale = resolve_baseline_hashes(&review, &later, "now", Some(dir.path()));
        assert_eq!(stale.get("a.rs").map(String::as_str), Some("abc"));
    }

    #[test]
    fn sidecar_file_hashes_win_over_the_host_file() {
        let dir = tempfile::tempdir().unwrap();
        let current = hashes(&[("a.rs", "from-host")]);
        let mut review = review_with(vec![], hashes(&[("a.rs", "from-review")]));
        review.diff_hash = "live".into();
        let got = resolve_baseline_hashes(&review, &current, "live", Some(dir.path()));
        assert_eq!(got.get("a.rs").map(String::as_str), Some("from-review"));
    }
}
