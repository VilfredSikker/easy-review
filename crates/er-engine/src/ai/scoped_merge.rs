//! Merge selected-file (scoped) review sidecars into a previous full review.
//!
//! A scoped run writes a filtered `diff-tmp` and agents typically overwrite
//! `review.json` with findings for only those paths. Callers snapshot the
//! previous sidecar to `*.prev.json` before the agent starts, then invoke
//! [`apply_scoped_sidecar_merge`] after a successful exit so non-scoped
//! findings are preserved.

use super::experts::ExpertReview;
use super::professor::ProfessorReview;
use super::review::ErReview;
use anyhow::{Context, Result};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const REVIEW_PREV: &str = "review.prev.json";
const REVIEW_FILES: &str = "review-files.txt";

/// Read paths from `review-files.txt` (one path per line).
pub fn read_review_file_manifest(er_dir: &Path) -> Result<Vec<String>> {
    let path = er_dir.join(REVIEW_FILES);
    let content =
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    Ok(content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect())
}

/// Snapshot existing general/expert/professor sidecars before a scoped run.
///
/// `reviewer_kinds` uses the same labels as `run_ai_scoped_review` (`general`,
/// `professor`, `security`, …). Full (non-scoped) runs should call
/// [`clear_scoped_review_snapshots`] instead.
pub fn snapshot_before_scoped_review(er_dir: &Path, reviewer_kinds: &[String]) -> Result<()> {
    for kind in reviewer_kinds {
        match kind.as_str() {
            "general" | "review" => {
                snapshot_if_exists(er_dir, "review.json", REVIEW_PREV)?;
            }
            "professor" => {
                snapshot_if_exists(er_dir, "professor.json", "professor.prev.json")?;
            }
            "triage" => {
                // Triage is a full-branch scan artifact; scoped triage is rare
                // and overwriting is acceptable.
            }
            expert_id => {
                let rel = format!("experts/{expert_id}.json");
                let prev = format!("experts/{expert_id}.prev.json");
                if let Some(parent) = er_dir.join(&rel).parent() {
                    std::fs::create_dir_all(parent)?;
                }
                snapshot_if_exists(er_dir, &rel, &prev)?;
            }
        }
    }
    Ok(())
}

/// Remove scoped-run helpers (manifest + prev snapshots) after a full review.
pub fn clear_scoped_review_snapshots(er_dir: &Path) {
    let _ = std::fs::remove_file(er_dir.join(REVIEW_FILES));
    let _ = std::fs::remove_file(er_dir.join(REVIEW_PREV));
    let _ = std::fs::remove_file(er_dir.join("professor.prev.json"));
    if let Ok(entries) = std::fs::read_dir(er_dir.join("experts")) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.ends_with(".prev.json") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

fn snapshot_if_exists(er_dir: &Path, relative: &str, prev_relative: &str) -> Result<()> {
    let src = er_dir.join(relative);
    let dst = er_dir.join(prev_relative);
    if src.is_file() {
        std::fs::copy(&src, &dst)
            .with_context(|| format!("snapshot {} → {}", src.display(), dst.display()))?;
    } else {
        let _ = std::fs::remove_file(&dst);
    }
    Ok(())
}

/// After a scoped agent exits successfully, merge its sidecar into the
/// pre-scoped snapshot when `review-files.txt` is present.
pub fn apply_scoped_sidecar_merge(er_dir: &Path, command_name: &str) -> Result<()> {
    let manifest_path = er_dir.join(REVIEW_FILES);
    if !manifest_path.is_file() {
        return Ok(());
    }
    let paths = read_review_file_manifest(er_dir)?;
    if paths.is_empty() {
        return Ok(());
    }

    if command_name == "review" {
        merge_review_file(er_dir, &paths)?;
        return Ok(());
    }
    if command_name == "professor" {
        merge_professor_file(er_dir, &paths)?;
        return Ok(());
    }
    if let Some(expert_id) = command_name.strip_prefix("expert-") {
        merge_expert_file(er_dir, expert_id, &paths)?;
    }
    Ok(())
}

/// Replace file entries for `scoped_paths` with those from `scoped`, keep the
///
/// rest from `previous`. Preserves `previous.diff_hash` so a partial re-review
/// does not look stale against the full-tab diff the prior review targeted.
pub fn merge_scoped_review(
    previous: ErReview,
    scoped: ErReview,
    scoped_paths: &[String],
) -> ErReview {
    let scoped_set: HashSet<&str> = scoped_paths.iter().map(String::as_str).collect();
    let mut merged = previous;
    // Fresh metadata from the latest pass, but keep the prior full-diff hash.
    let preserved_hash = merged.diff_hash.clone();
    merged.version = scoped.version;
    merged.created_at = scoped.created_at;
    if !scoped.base_branch.is_empty() {
        merged.base_branch = scoped.base_branch;
    }
    if !scoped.head_branch.is_empty() {
        merged.head_branch = scoped.head_branch;
    }
    merged.diff_hash = preserved_hash;

    for path in &scoped_set {
        // Drop prior entry for this path when the scoped pass omitted it
        // (agent found nothing worth recording).
        merged.files.remove(*path);
        merged.file_hashes.remove(*path);
    }
    for (path, file) in scoped.files {
        if scoped_set.contains(path.as_str()) {
            merged.files.insert(path, file);
        }
    }
    for (path, hash) in scoped.file_hashes {
        if scoped_set.contains(path.as_str()) {
            merged.file_hashes.insert(path, hash);
        }
    }
    merged
}

fn merge_scoped_expert(
    previous: ExpertReview,
    scoped: ExpertReview,
    scoped_paths: &[String],
) -> ExpertReview {
    let scoped_set: HashSet<&str> = scoped_paths.iter().map(String::as_str).collect();
    let mut merged = previous;
    let preserved_hash = merged.diff_hash.clone();
    merged.version = scoped.version;
    merged.created_at = scoped.created_at;
    if !scoped.diff_scope.is_empty() {
        merged.diff_scope = scoped.diff_scope;
    }
    if !scoped.summary.is_empty() {
        merged.summary = scoped.summary;
    }
    merged.diff_hash = preserved_hash;
    for path in &scoped_set {
        merged.files.remove(*path);
    }
    for (path, file) in scoped.files {
        if scoped_set.contains(path.as_str()) {
            merged.files.insert(path, file);
        }
    }
    merged
}

fn merge_scoped_professor(
    previous: ProfessorReview,
    scoped: ProfessorReview,
    scoped_paths: &[String],
) -> ProfessorReview {
    let scoped_set: HashSet<&str> = scoped_paths.iter().map(String::as_str).collect();
    let mut merged = previous;
    let preserved_hash = merged.diff_hash.clone();
    merged.version = scoped.version;
    merged.created_at = scoped.created_at;
    if !scoped.diff_scope.is_empty() {
        merged.diff_scope = scoped.diff_scope;
    }
    if !scoped.focus_prompt.is_empty() {
        merged.focus_prompt = scoped.focus_prompt;
    }
    if !scoped.summary.is_empty() {
        merged.summary = scoped.summary;
    }
    merged.diff_hash = preserved_hash;
    for path in &scoped_set {
        merged.files.remove(*path);
    }
    for (path, file) in scoped.files {
        if scoped_set.contains(path.as_str()) {
            merged.files.insert(path, file);
        }
    }
    merged
}

fn merge_review_file(er_dir: &Path, paths: &[String]) -> Result<()> {
    let prev_path = er_dir.join(REVIEW_PREV);
    if !prev_path.is_file() {
        return Ok(());
    }
    let review_path = er_dir.join("review.json");
    if !review_path.is_file() {
        return Ok(());
    }
    let previous: ErReview = read_json(&prev_path)?;
    let scoped: ErReview = read_json(&review_path)?;
    let merged = merge_scoped_review(previous, scoped, paths);
    write_json_atomic(&review_path, &merged)?;
    let _ = std::fs::remove_file(prev_path);
    Ok(())
}

fn merge_expert_file(er_dir: &Path, expert_id: &str, paths: &[String]) -> Result<()> {
    let prev_path = er_dir.join(format!("experts/{expert_id}.prev.json"));
    if !prev_path.is_file() {
        return Ok(());
    }
    let path = er_dir.join(format!("experts/{expert_id}.json"));
    if !path.is_file() {
        return Ok(());
    }
    let previous: ExpertReview = read_json(&prev_path)?;
    let scoped: ExpertReview = read_json(&path)?;
    let merged = merge_scoped_expert(previous, scoped, paths);
    write_json_atomic(&path, &merged)?;
    let _ = std::fs::remove_file(prev_path);
    Ok(())
}

fn merge_professor_file(er_dir: &Path, paths: &[String]) -> Result<()> {
    let prev_path = er_dir.join("professor.prev.json");
    if !prev_path.is_file() {
        return Ok(());
    }
    let path = er_dir.join("professor.json");
    if !path.is_file() {
        return Ok(());
    }
    let previous: ProfessorReview = read_json(&prev_path)?;
    let scoped: ProfessorReview = read_json(&path)?;
    let merged = merge_scoped_professor(previous, scoped, paths);
    write_json_atomic(&path, &merged)?;
    let _ = std::fs::remove_file(prev_path);
    Ok(())
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let content =
        std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&content).with_context(|| format!("parse {}", path.display()))
}

fn write_json_atomic<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let tmp = unique_tmp_path(parent, path);
    let json = serde_json::to_string_pretty(value).context("serialize sidecar")?;
    std::fs::write(&tmp, json).with_context(|| format!("write {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("rename onto {}", path.display()))?;
    Ok(())
}

fn unique_tmp_path(parent: &Path, final_path: &Path) -> PathBuf {
    let stem = final_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("sidecar");
    parent.join(format!(".{stem}.{}.tmp", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::experts::ExpertFileReview;
    use crate::ai::professor::ProfessorFileReview;
    use crate::ai::review::{Confidence, ErFileReview, Finding, RiskLevel};
    use std::collections::HashMap;

    fn finding(id: &str, title: &str) -> Finding {
        Finding {
            id: id.to_string(),
            severity: RiskLevel::Medium,
            category: "general".to_string(),
            title: title.to_string(),
            description: String::new(),
            hunk_index: Some(0),
            line_start: Some(1),
            line_end: None,
            suggestion: String::new(),
            related_files: vec![],
            outside_diff: false,
            confidence: Confidence::Confirmed,
            verification_plan: String::new(),
            evidence: vec![],
            responses: vec![],
            resolved: false,
            resolved_note: String::new(),
            resolved_at: String::new(),
            promoted_to: None,
        }
    }

    fn file_review(findings: Vec<Finding>) -> ErFileReview {
        ErFileReview {
            risk: RiskLevel::Medium,
            risk_reason: String::new(),
            summary: String::new(),
            findings,
        }
    }

    fn review_with(files: Vec<(&str, Vec<Finding>)>, hash: &str) -> ErReview {
        let mut map = HashMap::new();
        for (path, findings) in files {
            map.insert(path.to_string(), file_review(findings));
        }
        ErReview {
            version: 1,
            diff_hash: hash.to_string(),
            created_at: "old".into(),
            base_branch: "main".into(),
            head_branch: "feature".into(),
            files: map,
            file_hashes: HashMap::new(),
        }
    }

    /// Reproduces the user bug: a selected-file re-review must not drop
    /// findings for files outside the selection.
    #[test]
    fn scoped_merge_keeps_non_scoped_findings() {
        let previous = review_with(
            vec![
                ("a.ts", vec![finding("f-1", "Old A")]),
                ("b.ts", vec![finding("f-2", "Old B")]),
                ("c.ts", vec![finding("f-3", "Old C")]),
            ],
            "full-hash",
        );
        let scoped = review_with(
            vec![("b.ts", vec![finding("f-10", "New B")])],
            "scoped-hash",
        );

        let merged = merge_scoped_review(previous, scoped, &["b.ts".into()]);

        assert_eq!(merged.diff_hash, "full-hash");
        assert_eq!(merged.files.len(), 3);
        assert_eq!(merged.files["a.ts"].findings[0].id, "f-1");
        assert_eq!(merged.files["c.ts"].findings[0].id, "f-3");
        assert_eq!(merged.files["b.ts"].findings[0].id, "f-10");
        assert_eq!(merged.files["b.ts"].findings[0].title, "New B");
    }

    #[test]
    fn scoped_merge_removes_prior_findings_when_scoped_pass_clears_file() {
        let previous = review_with(
            vec![
                ("a.ts", vec![finding("f-1", "A")]),
                ("b.ts", vec![finding("f-2", "B")]),
            ],
            "h",
        );
        let scoped = review_with(vec![], "scoped");
        let merged = merge_scoped_review(previous, scoped, &["b.ts".into()]);
        assert!(merged.files.contains_key("a.ts"));
        assert!(!merged.files.contains_key("b.ts"));
    }

    #[test]
    fn apply_merge_on_disk_preserves_siblings() {
        let dir = tempfile::tempdir().unwrap();
        let er = dir.path();
        let previous = review_with(
            vec![
                ("experiment.ts", vec![finding("f-1", "Hidden analyses")]),
                ("other.ts", vec![finding("f-2", "Keep me")]),
            ],
            "full-hash",
        );
        let scoped = review_with(
            vec![(
                "plate-template.ts",
                vec![finding("f-9", "Rejects valid types")],
            )],
            "scoped-hash",
        );
        std::fs::write(
            er.join("review.prev.json"),
            serde_json::to_string(&previous).unwrap(),
        )
        .unwrap();
        std::fs::write(
            er.join("review.json"),
            serde_json::to_string(&scoped).unwrap(),
        )
        .unwrap();
        std::fs::write(er.join("review-files.txt"), "plate-template.ts\n").unwrap();

        apply_scoped_sidecar_merge(er, "review").unwrap();

        let merged: ErReview =
            serde_json::from_str(&std::fs::read_to_string(er.join("review.json")).unwrap())
                .unwrap();
        assert_eq!(merged.files.len(), 3);
        assert!(merged.files.contains_key("other.ts"));
        assert!(merged.files.contains_key("experiment.ts"));
        assert_eq!(
            merged.files["plate-template.ts"].findings[0].title,
            "Rejects valid types"
        );
        assert_eq!(merged.diff_hash, "full-hash");
        assert!(!er.join("review.prev.json").exists());
    }

    #[test]
    fn apply_merge_noop_without_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let er = dir.path();
        let scoped = review_with(vec![("a.ts", vec![finding("f-1", "Only")])], "h");
        std::fs::write(
            er.join("review.json"),
            serde_json::to_string(&scoped).unwrap(),
        )
        .unwrap();
        apply_scoped_sidecar_merge(er, "review").unwrap();
        let after: ErReview =
            serde_json::from_str(&std::fs::read_to_string(er.join("review.json")).unwrap())
                .unwrap();
        assert_eq!(after.files.len(), 1);
    }

    // ── snapshot_before_scoped_review ──

    #[test]
    fn snapshot_routes_each_reviewer_kind_to_its_own_prev_file() {
        let dir = tempfile::tempdir().unwrap();
        let er = dir.path();
        std::fs::create_dir_all(er.join("experts")).unwrap();
        std::fs::write(er.join("review.json"), r#"{"marker":"general"}"#).unwrap();
        std::fs::write(er.join("professor.json"), r#"{"marker":"professor"}"#).unwrap();
        std::fs::write(er.join("experts/security.json"), r#"{"marker":"security"}"#).unwrap();
        std::fs::write(er.join("triage.json"), r#"{"marker":"triage"}"#).unwrap();
        // Bait for the expert catch-all: if the dedicated `"triage"` arm were
        // removed, `triage` would fall through and snapshot this file.
        std::fs::write(
            er.join("experts/triage.json"),
            r#"{"marker":"triage-expert"}"#,
        )
        .unwrap();

        snapshot_before_scoped_review(
            er,
            &[
                "general".into(),
                "professor".into(),
                "security".into(),
                "triage".into(),
            ],
        )
        .unwrap();

        assert_eq!(
            std::fs::read_to_string(er.join("review.prev.json")).unwrap(),
            r#"{"marker":"general"}"#
        );
        assert_eq!(
            std::fs::read_to_string(er.join("professor.prev.json")).unwrap(),
            r#"{"marker":"professor"}"#
        );
        assert_eq!(
            std::fs::read_to_string(er.join("experts/security.prev.json")).unwrap(),
            r#"{"marker":"security"}"#
        );
        // Triage is a full-branch artifact — it is deliberately never
        // snapshotted, to *either* destination: not the root (a misroute into
        // the general/professor arms) and not `experts/` (a fall-through into
        // the catch-all).
        assert!(!er.join("triage.prev.json").exists());
        assert!(!er.join("experts/triage.prev.json").exists());
    }

    #[test]
    fn snapshot_treats_review_alias_the_same_as_general() {
        let dir = tempfile::tempdir().unwrap();
        let er = dir.path();
        std::fs::write(er.join("review.json"), r#"{"marker":"general"}"#).unwrap();

        snapshot_before_scoped_review(er, &["review".into()]).unwrap();

        assert_eq!(
            std::fs::read_to_string(er.join("review.prev.json")).unwrap(),
            r#"{"marker":"general"}"#
        );
    }

    /// A leftover `*.prev.json` from an earlier run must not be merged into a
    /// sidecar that no longer exists — the snapshot pass clears it instead.
    #[test]
    fn snapshot_clears_stale_prev_when_sidecar_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let er = dir.path();
        std::fs::create_dir_all(er.join("experts")).unwrap();
        std::fs::write(er.join("review.prev.json"), "stale").unwrap();
        std::fs::write(er.join("professor.prev.json"), "stale").unwrap();
        std::fs::write(er.join("experts/security.prev.json"), "stale").unwrap();

        snapshot_before_scoped_review(
            er,
            &["general".into(), "professor".into(), "security".into()],
        )
        .unwrap();

        assert!(!er.join("review.prev.json").exists());
        assert!(!er.join("professor.prev.json").exists());
        assert!(!er.join("experts/security.prev.json").exists());
    }

    #[test]
    fn snapshot_creates_experts_dir_for_a_first_time_expert_run() {
        let dir = tempfile::tempdir().unwrap();
        let er = dir.path();

        snapshot_before_scoped_review(er, &["performance".into()]).unwrap();

        assert!(er.join("experts").is_dir());
        assert!(!er.join("experts/performance.prev.json").exists());
    }

    // ── merge_scoped_expert / merge_scoped_professor ──

    fn expert_file_review(findings: Vec<Finding>) -> ExpertFileReview {
        ExpertFileReview { findings }
    }

    fn expert_review_with(
        files: Vec<(&str, Vec<Finding>)>,
        hash: &str,
        scope: &str,
        summary: &str,
    ) -> ExpertReview {
        let mut map = HashMap::new();
        for (path, findings) in files {
            map.insert(path.to_string(), expert_file_review(findings));
        }
        ExpertReview {
            version: 1,
            expert_id: "security".into(),
            diff_hash: hash.to_string(),
            diff_scope: scope.to_string(),
            created_at: "old".into(),
            summary: summary.to_string(),
            files: map,
        }
    }

    fn professor_review_with(
        files: Vec<(&str, Vec<Finding>)>,
        hash: &str,
        scope: &str,
        focus: &str,
        summary: &str,
    ) -> ProfessorReview {
        let mut map = HashMap::new();
        for (path, findings) in files {
            map.insert(path.to_string(), ProfessorFileReview { findings });
        }
        ProfessorReview {
            version: 1,
            diff_hash: hash.to_string(),
            diff_scope: scope.to_string(),
            created_at: "old".into(),
            focus_prompt: focus.to_string(),
            summary: summary.to_string(),
            files: map,
        }
    }

    #[test]
    fn scoped_expert_merge_keeps_non_scoped_files_and_the_full_diff_hash() {
        let previous = expert_review_with(
            vec![
                ("a.ts", vec![finding("sec-1", "Old A")]),
                ("b.ts", vec![finding("sec-2", "Old B")]),
            ],
            "full-hash",
            "branch",
            "old summary",
        );
        let mut scoped = expert_review_with(
            vec![("b.ts", vec![finding("sec-9", "New B")])],
            "scoped-hash",
            "selected",
            "new summary",
        );
        scoped.version = 2;
        scoped.created_at = "new".into();
        scoped.expert_id = "performance".into();

        let merged = merge_scoped_expert(previous, scoped, &["b.ts".into()]);

        assert_eq!(merged.diff_hash, "full-hash");
        assert_eq!(merged.version, 2);
        assert_eq!(merged.created_at, "new");
        assert_eq!(merged.diff_scope, "selected");
        assert_eq!(merged.summary, "new summary");
        // The sidecar's identity comes from the file it lives in, not the scoped pass.
        assert_eq!(merged.expert_id, "security");
        assert_eq!(merged.files.len(), 2);
        assert_eq!(merged.files["a.ts"].findings[0].id, "sec-1");
        assert_eq!(merged.files["b.ts"].findings[0].id, "sec-9");
    }

    #[test]
    fn scoped_expert_merge_keeps_prior_scope_and_summary_when_scoped_leaves_them_blank() {
        let previous = expert_review_with(
            vec![
                ("a.ts", vec![finding("sec-1", "Old A")]),
                ("b.ts", vec![finding("sec-2", "Old B")]),
            ],
            "full-hash",
            "branch",
            "old summary",
        );
        let scoped = expert_review_with(vec![], "scoped-hash", "", "");

        let merged = merge_scoped_expert(previous, scoped, &["b.ts".into()]);

        assert_eq!(merged.diff_scope, "branch");
        assert_eq!(merged.summary, "old summary");
        // Scoped pass reported nothing for b.ts → its prior findings are dropped.
        assert!(merged.files.contains_key("a.ts"));
        assert!(!merged.files.contains_key("b.ts"));
    }

    #[test]
    fn scoped_professor_merge_keeps_non_scoped_files_and_the_full_diff_hash() {
        let previous = professor_review_with(
            vec![
                ("a.ts", vec![finding("prof-1", "Old A")]),
                ("b.ts", vec![finding("prof-2", "Old B")]),
            ],
            "full-hash",
            "branch",
            "old focus",
            "old summary",
        );
        let mut scoped = professor_review_with(
            vec![("b.ts", vec![finding("prof-9", "New B")])],
            "scoped-hash",
            "selected",
            "new focus",
            "new summary",
        );
        scoped.version = 2;
        scoped.created_at = "new".into();

        let merged = merge_scoped_professor(previous, scoped, &["b.ts".into()]);

        assert_eq!(merged.diff_hash, "full-hash");
        assert_eq!(merged.version, 2);
        assert_eq!(merged.created_at, "new");
        assert_eq!(merged.diff_scope, "selected");
        assert_eq!(merged.focus_prompt, "new focus");
        assert_eq!(merged.summary, "new summary");
        assert_eq!(merged.files.len(), 2);
        assert_eq!(merged.files["a.ts"].findings[0].id, "prof-1");
        assert_eq!(merged.files["b.ts"].findings[0].id, "prof-9");
    }

    #[test]
    fn scoped_professor_merge_keeps_prior_scope_focus_and_summary_when_scoped_leaves_them_blank() {
        let previous = professor_review_with(
            vec![
                ("a.ts", vec![finding("prof-1", "Old A")]),
                ("b.ts", vec![finding("prof-2", "Old B")]),
            ],
            "full-hash",
            "branch",
            "old focus",
            "old summary",
        );
        let scoped = professor_review_with(vec![], "scoped-hash", "", "", "");

        let merged = merge_scoped_professor(previous, scoped, &["b.ts".into()]);

        assert_eq!(merged.diff_scope, "branch");
        assert_eq!(merged.focus_prompt, "old focus");
        assert_eq!(merged.summary, "old summary");
        assert!(merged.files.contains_key("a.ts"));
        assert!(!merged.files.contains_key("b.ts"));
    }

    /// `apply_scoped_sidecar_merge` must route `expert-<id>` to the matching
    /// expert sidecar — a broken prefix strip would silently skip the merge.
    #[test]
    fn apply_merge_routes_expert_command_to_the_matching_expert_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let er = dir.path();
        std::fs::create_dir_all(er.join("experts")).unwrap();
        let previous = expert_review_with(
            vec![
                ("keep.ts", vec![finding("sec-1", "Keep me")]),
                ("scoped.ts", vec![finding("sec-2", "Stale")]),
            ],
            "full-hash",
            "branch",
            "old summary",
        );
        let scoped = expert_review_with(
            vec![("scoped.ts", vec![finding("sec-9", "Fresh")])],
            "scoped-hash",
            "selected",
            "new summary",
        );
        std::fs::write(
            er.join("experts/security.prev.json"),
            serde_json::to_string(&previous).unwrap(),
        )
        .unwrap();
        std::fs::write(
            er.join("experts/security.json"),
            serde_json::to_string(&scoped).unwrap(),
        )
        .unwrap();
        std::fs::write(er.join("review-files.txt"), "scoped.ts\n").unwrap();

        apply_scoped_sidecar_merge(er, "expert-security").unwrap();

        let merged: ExpertReview = serde_json::from_str(
            &std::fs::read_to_string(er.join("experts/security.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(merged.files.len(), 2);
        assert_eq!(merged.files["keep.ts"].findings[0].title, "Keep me");
        assert_eq!(merged.files["scoped.ts"].findings[0].title, "Fresh");
        assert_eq!(merged.diff_hash, "full-hash");
        assert!(!er.join("experts/security.prev.json").exists());
    }

    /// The professor branch of the same router.
    #[test]
    fn apply_merge_routes_professor_command_to_the_professor_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let er = dir.path();
        let previous = professor_review_with(
            vec![
                ("keep.ts", vec![finding("prof-1", "Keep me")]),
                ("scoped.ts", vec![finding("prof-2", "Stale")]),
            ],
            "full-hash",
            "branch",
            "old focus",
            "old summary",
        );
        let scoped = professor_review_with(
            vec![("scoped.ts", vec![finding("prof-9", "Fresh")])],
            "scoped-hash",
            "selected",
            "new focus",
            "new summary",
        );
        std::fs::write(
            er.join("professor.prev.json"),
            serde_json::to_string(&previous).unwrap(),
        )
        .unwrap();
        std::fs::write(
            er.join("professor.json"),
            serde_json::to_string(&scoped).unwrap(),
        )
        .unwrap();
        std::fs::write(er.join("review-files.txt"), "scoped.ts\n").unwrap();

        apply_scoped_sidecar_merge(er, "professor").unwrap();

        let merged: ProfessorReview =
            serde_json::from_str(&std::fs::read_to_string(er.join("professor.json")).unwrap())
                .unwrap();
        assert_eq!(merged.files.len(), 2);
        assert_eq!(merged.files["keep.ts"].findings[0].title, "Keep me");
        assert_eq!(merged.files["scoped.ts"].findings[0].title, "Fresh");
        assert_eq!(merged.diff_hash, "full-hash");
        assert!(!er.join("professor.prev.json").exists());
    }
}
