//! Parent-side preparation of agent diff artifacts.
//!
//! Every agent command (review, expert, professor, triage, tour, validate,
//! scoped multi-reviewer) hands the agent a prepared diff. The parent already
//! holds the raw bytes in memory, so having each agent subprocess re-hash
//! (`sha256sum`) and re-annotate (`awk`) them repeats work the parent can do
//! once. This module centralizes that preparation:
//!
//! - [`ensure_diff_artifacts`] writes `diff-tmp` + `diff-annotated` only when
//!   the content changed (marker files) and returns the SHA-256 hash the
//!   agent must record as `diff_hash` — one hash + one annotation per changed
//!   diff instead of N subprocesses.
//! - [`annotate_diff_raw`] is the byte-identical Rust port of the agent-side
//!   `awk` script (prompts.rs `annotate_diff_command`).

use std::path::Path;
use std::sync::Mutex;

/// Serializes the check+write of the diff artifacts across threads: two
/// concurrent commands on the same er_dir with different raws must not
/// interleave, or one's prompt pins hash A while diff-tmp holds writer B's
/// bytes. Held for microseconds per command — commands are seconds apart, so
/// contention is negligible.
static ARTIFACT_LOCK: Mutex<()> = Mutex::new(());

/// Write `diff-tmp` and `diff-annotated` under `er_dir` when their content
/// changed since the last command, and return the SHA-256 of `raw` — the
/// `diff_hash` the agent must record (contract: SHA-256 of `diff-tmp`).
///
/// Both markers record the **raw** hash they were derived from, not the hash
/// of the file they name. `diff-annotated` is a pure function of `raw`, so one
/// hash proves both artifacts current and the whole preparation can be skipped
/// — otherwise every call rebuilds the annotated string and hashes it, only to
/// discard both when the write turns out to be unnecessary.
///
/// Content files are written via tmp+rename (atomic): with the hash pinned
/// into the prompt, a torn read would silently mismatch the pinned hash.
pub fn ensure_diff_artifacts(er_dir: &str, raw: &str) -> Result<String, String> {
    let _guard = ARTIFACT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = Path::new(er_dir);
    let hash = crate::ai::compute_diff_hash(raw);

    if artifacts_current(dir, &hash) {
        return Ok(hash);
    }

    if content_changed(dir, ".diff-tmp.sha256", &hash, "diff-tmp") {
        atomic_write(dir, "diff-tmp", raw)?;
        let _ = std::fs::write(dir.join(".diff-tmp.sha256"), &hash);
    }

    #[cfg(test)]
    crate::agent_timing::record_annotate_pass();
    let annotated = annotate_diff_raw(raw);
    if content_changed(dir, ".diff-annotated.sha256", &hash, "diff-annotated") {
        atomic_write(dir, "diff-annotated", &annotated)?;
        let _ = std::fs::write(dir.join(".diff-annotated.sha256"), &hash);
    }
    Ok(hash)
}

/// Whether both artifacts on disk were already derived from `hash`.
///
/// The content files must exist as well as the markers: a surviving marker
/// must never suppress a rewrite for a missing content file.
fn artifacts_current(dir: &Path, hash: &str) -> bool {
    dir.join("diff-tmp").exists()
        && dir.join("diff-annotated").exists()
        && marker_matches(dir, ".diff-tmp.sha256", hash)
        && marker_matches(dir, ".diff-annotated.sha256", hash)
}

fn marker_matches(dir: &Path, marker: &str, hash: &str) -> bool {
    std::fs::read_to_string(dir.join(marker))
        .map(|prev| prev.trim() == hash)
        .unwrap_or(false)
}

/// tmp+rename write (same pattern as the durable sidecar writers).
fn atomic_write(dir: &Path, name: &str, content: &str) -> Result<(), String> {
    let path = dir.join(name);
    let tmp = dir.join(format!("{name}.tmp"));
    std::fs::write(&tmp, content).map_err(|e| format!("Failed to write {name}: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("Failed to finalize {name}: {e}")
    })
}

/// Read the recorded `diff-tmp` hash without touching the diff (used by the
/// MCP sidecar-upload path when the raw diff is not in memory).
pub fn diff_tmp_hash(er_dir: &str) -> Option<String> {
    std::fs::read_to_string(Path::new(er_dir).join(".diff-tmp.sha256"))
        .ok()
        .map(|s| s.trim().to_string())
}

fn content_changed(dir: &Path, marker: &str, hash: &str, content_file: &str) -> bool {
    // Same rule as `artifacts_current`: a marker alone must not suppress a
    // rewrite for a missing content file (e.g. external cleanup).
    if !dir.join(content_file).exists() {
        return true;
    }
    match std::fs::read_to_string(dir.join(marker)) {
        Ok(prev) => prev.trim() != hash,
        Err(_) => true,
    }
}

/// Parent-side port of the agent's `awk` annotation step (prompts.rs
///
/// `annotate_diff_command`): each content line is prefixed with
/// `[h<hunk> L<file_line>]` (`L-<old>` for deleted lines), hunk headers get
/// `[h<hunk>] `, and `diff --git`/`---`/`+++`/fallback lines pass through
/// unchanged. Deterministic — the golden tests assert byte-identical output
/// with the awk script it replaces.
pub fn annotate_diff_raw(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() * 2);
    let mut h: i64 = -1;
    let mut n: i64 = 0;
    let mut o: i64 = 0;
    for line in raw.lines() {
        if line.starts_with("diff --git") {
            h = -1;
        } else if line.starts_with("+++") || line.starts_with("---") {
            // header lines pass through unchanged
        } else if let Some(rest) = line.strip_prefix("@@ ") {
            h += 1;
            n = parse_hunk_new_start(rest).map(|v| v - 1).unwrap_or(0);
            o = parse_hunk_old_start(rest).map(|v| v - 1).unwrap_or(0);
            out.push_str(&format!("[h{h}] "));
        } else if line.starts_with('+') {
            n += 1;
            out.push_str(&format!("[h{h} L{n}] "));
        } else if line.starts_with('-') {
            o += 1;
            out.push_str(&format!("[h{h} L-{o}] "));
        } else if line.starts_with(' ') {
            o += 1;
            n += 1;
            out.push_str(&format!("[h{h} L{n}] "));
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// The hunks that `targets` anchor to, as a diff excerpt.
///
/// An arbiter that only sees the findings is judging claims about code it cannot
/// read, and dropping one as wrong is exactly that judgement. Sending the hunks
/// the findings point at keeps the cost proportional to how many findings there
/// are, not to how big the diff is: N findings anchor to at most N hunks whether
/// the change is 50 lines or 5,000.
///
/// Each target is a `(path, line)` pair; a `None` line (a hunk-level finding)
/// takes the file's first hunk. Paths and lines that no longer match the diff
/// contribute nothing, so a stale finding cannot drag an unrelated hunk in.
pub fn hunks_for_findings(raw_diff: &str, targets: &[(String, Option<usize>)]) -> String {
    let files = crate::git::parse_diff(raw_diff);
    let mut out = String::new();
    let mut seen: Vec<(String, usize)> = Vec::new();

    for (path, line) in targets {
        let Some(file) = files.iter().find(|f| &f.path == path) else {
            continue;
        };
        let found = file.hunks.iter().enumerate().find(|(_, hunk)| match line {
            Some(target) => hunk.lines.iter().any(|dl| dl.new_num == Some(*target)),
            None => true,
        });
        let Some((index, hunk)) = found else {
            continue;
        };
        // Two findings in one hunk share it rather than repeating it.
        if seen.contains(&(path.clone(), index)) {
            continue;
        }
        seen.push((path.clone(), index));

        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str("--- a/");
        out.push_str(path);
        out.push_str("\n+++ b/");
        out.push_str(path);
        out.push('\n');
        out.push_str(&hunk.header);
        out.push('\n');
        for dl in &hunk.lines {
            out.push(line_prefix(dl.line_type));
            out.push_str(&dl.content);
            out.push('\n');
        }
    }
    out
}

const fn line_prefix(line_type: crate::git::LineType) -> char {
    match line_type {
        crate::git::LineType::Add => '+',
        crate::git::LineType::Delete => '-',
        crate::git::LineType::Context | crate::git::LineType::Fold(_) => ' ',
    }
}

/// New-side start number from `@@ -old(,count)? +new(,count)? @@ ...`.
fn parse_hunk_new_start(rest: &str) -> Option<i64> {
    let plus_idx = rest.find('+')?;
    let num: String = rest[plus_idx + 1..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    num.parse().ok()
}

/// Old-side start number from `@@ -old(,count)? +new(,count)? @@ ...`.
fn parse_hunk_old_start(rest: &str) -> Option<i64> {
    let num: String = rest
        .strip_prefix('-')?
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    num.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_DIFF: &str = "diff --git a/src/foo.rs b/src/foo.rs\nindex 0000000..1111111 100644\n--- a/src/foo.rs\n+++ b/src/foo.rs\n@@ -1,2 +1,3 @@\n fn foo() {}\n+fn bar() {}\n-fn baz() {}\n fn qux() {}\n";

    #[test]
    fn hunks_for_findings_takes_only_the_anchored_hunk() {
        let diff = concat!(
            "diff --git a/src/a.rs b/src/a.rs\n",
            "--- a/src/a.rs\n",
            "+++ b/src/a.rs\n",
            "@@ -1,3 +1,4 @@\n",
            " fn first() {}\n",
            "+    let key = input;\n",
            " }\n",
            "@@ -40,3 +41,3 @@\n",
            " fn far_away() {}\n",
            "-    old();\n",
            "+    new();\n",
            " }\n",
        );

        let out = hunks_for_findings(diff, &[("src/a.rs".to_string(), Some(2))]);

        assert!(
            out.contains("let key = input"),
            "the anchored hunk is there"
        );
        assert!(
            !out.contains("far_away"),
            "the rest of the diff is not paid for"
        );
        assert!(out.starts_with("--- a/src/a.rs"), "a real diff excerpt");
    }

    #[test]
    fn hunks_for_findings_dedupes_and_skips_unmatched_targets() {
        let diff = concat!(
            "diff --git a/src/a.rs b/src/a.rs\n",
            "--- a/src/a.rs\n",
            "+++ b/src/a.rs\n",
            "@@ -1,3 +1,4 @@\n",
            " fn first() {}\n",
            "+    let key = input;\n",
            " }\n",
        );

        let out = hunks_for_findings(
            diff,
            &[
                // Two findings in the same hunk, one of them on the added line.
                ("src/a.rs".to_string(), Some(2)),
                ("src/a.rs".to_string(), None),
                // A file the diff no longer contains.
                ("src/gone.rs".to_string(), Some(1)),
                // A line the hunk does not cover.
                ("src/a.rs".to_string(), Some(900)),
            ],
        );

        assert_eq!(
            out.matches("fn first() {}").count(),
            1,
            "one hunk is sent once, however many findings anchor to it"
        );
        assert!(!out.contains("gone.rs"));
    }

    #[test]
    fn hunks_for_findings_without_targets_is_empty() {
        assert!(hunks_for_findings(FIXTURE_DIFF, &[]).is_empty());
    }

    #[test]
    fn ensure_artifacts_writes_once_then_skips_identical_content() {
        let dir = tempfile::tempdir().unwrap();
        let er_dir = dir.path().to_str().unwrap();
        let hash1 = ensure_diff_artifacts(er_dir, FIXTURE_DIFF).unwrap();
        let path = dir.path().join("diff-tmp");
        let mtime1 = std::fs::metadata(&path).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let hash2 = ensure_diff_artifacts(er_dir, FIXTURE_DIFF).unwrap();
        let mtime2 = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(hash1, hash2, "stable hash across calls");
        assert_eq!(
            mtime1, mtime2,
            "identical content must not rewrite diff-tmp"
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            FIXTURE_DIFF,
            "content preserved"
        );
        assert_eq!(
            diff_tmp_hash(er_dir).as_deref(),
            Some(hash1.as_str()),
            "marker readable for MCP path"
        );
    }

    #[test]
    fn ensure_artifacts_rewrites_when_content_changes_and_writes_annotated() {
        let dir = tempfile::tempdir().unwrap();
        let er_dir = dir.path().to_str().unwrap();
        ensure_diff_artifacts(er_dir, FIXTURE_DIFF).unwrap();
        let changed = FIXTURE_DIFF.replace("fn bar() {}", "fn bar2() {}");
        ensure_diff_artifacts(er_dir, &changed).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("diff-tmp")).unwrap(),
            changed
        );
        let annotated = std::fs::read_to_string(dir.path().join("diff-annotated")).unwrap();
        assert!(annotated.starts_with("diff --git"), "annotated written");
        assert!(
            annotated.contains("[h0 L2] +fn bar2() {}"),
            "matches changed diff"
        );
    }

    #[test]
    fn annotate_matches_the_awk_contract() {
        // Byte-identical to `awk` from prompts.rs `annotate_diff_command`.
        let expected = concat!(
            "diff --git a/src/foo.rs b/src/foo.rs\n",
            "index 0000000..1111111 100644\n",
            "--- a/src/foo.rs\n",
            "+++ b/src/foo.rs\n",
            "[h0] @@ -1,2 +1,3 @@\n",
            "[h0 L1]  fn foo() {}\n",
            "[h0 L2] +fn bar() {}\n",
            "[h0 L-2] -fn baz() {}\n",
            "[h0 L3]  fn qux() {}\n",
        );
        assert_eq!(annotate_diff_raw(FIXTURE_DIFF), expected);
    }

    #[test]
    fn annotate_tracks_multi_hunk_and_new_file_indexes() {
        let diff = concat!(
            "diff --git a/a.rs b/a.rs\n",
            "index 0000000..1111111 100644\n",
            "--- a/a.rs\n",
            "+++ b/a.rs\n",
            "@@ -1 +1,2 @@\n",
            " fn one() {}\n",
            "+fn two() {}\n",
            "@@ -10,3 +11,4 @@\n",
            " fn ten() {}\n",
            "+fn eleven() {}\n",
            "-fn nine() {}\n",
        );
        let expected = concat!(
            "diff --git a/a.rs b/a.rs\n",
            "index 0000000..1111111 100644\n",
            "--- a/a.rs\n",
            "+++ b/a.rs\n",
            "[h0] @@ -1 +1,2 @@\n",
            "[h0 L1]  fn one() {}\n",
            "[h0 L2] +fn two() {}\n",
            "[h1] @@ -10,3 +11,4 @@\n",
            "[h1 L11]  fn ten() {}\n",
            "[h1 L12] +fn eleven() {}\n",
            "[h1 L-11] -fn nine() {}\n",
        );
        assert_eq!(annotate_diff_raw(diff), expected);
    }

    #[test]
    fn unchanged_diff_skips_the_annotation_pass() {
        let dir = tempfile::tempdir().unwrap();
        let er_dir = dir.path().to_str().unwrap();

        ensure_diff_artifacts(er_dir, FIXTURE_DIFF).unwrap();
        let after_first = crate::agent_timing::annotate_passes();

        ensure_diff_artifacts(er_dir, FIXTURE_DIFF).unwrap();
        ensure_diff_artifacts(er_dir, FIXTURE_DIFF).unwrap();
        assert_eq!(
            crate::agent_timing::annotate_passes(),
            after_first,
            "repeat calls on an unchanged diff must not rebuild the annotation"
        );

        let changed = FIXTURE_DIFF.replace("fn bar() {}", "fn bar2() {}");
        ensure_diff_artifacts(er_dir, &changed).unwrap();
        assert_eq!(
            crate::agent_timing::annotate_passes(),
            after_first + 1,
            "a changed diff must be re-annotated"
        );
    }

    #[test]
    fn artifacts_current_needs_both_markers_and_both_files() {
        let dir = tempfile::tempdir().unwrap();
        let er_dir = dir.path().to_str().unwrap();
        let hash = ensure_diff_artifacts(er_dir, FIXTURE_DIFF).unwrap();
        let path = Path::new(er_dir);

        assert!(
            artifacts_current(path, &hash),
            "freshly written artifacts are current"
        );
        assert!(
            !artifacts_current(path, "0000"),
            "a different raw is not current"
        );

        // A marker that outlives its content file must not suppress the rewrite.
        std::fs::remove_file(path.join("diff-annotated")).unwrap();
        assert!(
            !artifacts_current(path, &hash),
            "missing content file is not current"
        );
        ensure_diff_artifacts(er_dir, FIXTURE_DIFF).unwrap();
        assert!(
            path.join("diff-annotated").exists(),
            "the missing file is rebuilt"
        );

        // Markers written by an older build recorded the annotated hash, not
        // the raw one. Those must read as stale exactly once, not be trusted.
        std::fs::write(path.join(".diff-annotated.sha256"), "legacy-annotated-hash").unwrap();
        assert!(
            !artifacts_current(path, &hash),
            "a pre-upgrade marker is not current"
        );
        ensure_diff_artifacts(er_dir, FIXTURE_DIFF).unwrap();
        assert!(
            artifacts_current(path, &hash),
            "rewriting restores the new marker semantics"
        );
    }
}
