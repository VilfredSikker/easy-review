//! Minimal LCOV parser: per-file line hit counts.
//!
//! Only the records we need are read: `SF:` (file section start) and `DA:`
//! (line hit counts). Function records (`FN`/`FNDA`/`FNL`), branch records
//! (`BRDA`) and summary counters (`LF`/`LH`) are ignored — per-function
//! coverage is computed by intersecting the analyzer's own function line
//! ranges with the file's `DA` records, which sidesteps Rust symbol
//! mangling in `FN` names entirely.

use std::collections::HashMap;

use anyhow::Context;

/// Parsed coverage: source-file path → (line number → hit count).
/// File paths are keyed exactly as they appear in `SF:` records.
#[derive(Debug, Default)]
pub struct LcovCoverage {
    files: HashMap<String, HashMap<usize, u64>>,
}

impl LcovCoverage {
    /// Look up the coverage in `[start, end]` (inclusive) for the source
    /// file that best matches `file_path`. Returns `(covered_lines,
    /// total_lines)`; `None` when no matching file section exists.
    pub fn coverage_for(
        &self,
        file_path: &str,
        start: usize,
        end: usize,
    ) -> Option<(usize, usize)> {
        let entry = self.files.iter().find(|(k, _)| paths_match(k, file_path))?;
        let (covered, total) = entry.1.iter().fold((0, 0), |(c, t), (line, hits)| {
            if *line >= start && *line <= end {
                (c + usize::from(*hits > 0), t + 1)
            } else {
                (c, t)
            }
        });
        Some((covered, total))
    }
}

/// Parse LCOV text into [`LcovCoverage`]. Malformed `DA` records (the only
/// ones we consume) are errors; everything else is ignored.
pub fn parse_lcov(contents: &str) -> anyhow::Result<LcovCoverage> {
    let mut cov = LcovCoverage::default();
    let mut current: Option<String> = None;
    for (idx, raw) in contents.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("SF:") {
            current = Some(rest.to_string());
            cov.files.entry(rest.to_string()).or_default();
        } else if let Some(rest) = line.strip_prefix("DA:") {
            let file = current.as_deref().with_context(|| {
                format!(
                    "DA record at line {} appears before any SF record: {raw}",
                    idx + 1
                )
            })?;
            let (line_no, hits) = rest
                .split_once(',')
                .with_context(|| format!("malformed DA record at line {}: {raw}", idx + 1))?;
            let line_no: usize = line_no
                .trim()
                .parse()
                .with_context(|| format!("malformed DA line number at {}: {raw}", idx + 1))?;
            let hits: u64 = hits
                .trim()
                .parse()
                .with_context(|| format!("malformed DA hit count at {}: {raw}", idx + 1))?;
            let map = cov
                .files
                .get_mut(file)
                .with_context(|| format!("internal error: missing SF map for {file}"))?;
            map.insert(line_no, hits);
        }
        // FN:, FNDA:, FNL:, BRDA:, BRF:, BRH:, LF:, LH:, end_of_record — ignored.
    }
    Ok(cov)
}

/// Match an analyzed file path against an LCOV `SF:` path. LCOV paths are
/// relative to where `cargo llvm-cov` ran, so either side may be the longer
/// one; a suffix match on either direction is accepted.
fn paths_match(a: &str, b: &str) -> bool {
    let norm = |p: &str| p.trim_start_matches("./").replace('\\', "/");
    let (a, b) = (norm(a), norm(b));
    a.ends_with(&b) || b.ends_with(&a)
}
#[cfg(test)]
mod tests {
    use super::*;

    fn cov(contents: &str) -> LcovCoverage {
        parse_lcov(contents).expect("valid LCOV input")
    }

    #[test]
    fn parses_da_records_per_file() {
        let c = cov("SF:src/lib.rs\nDA:1,1\nDA:2,0\nend_of_record\n");
        let file = c.files.get("src/lib.rs").expect("file section present");
        assert_eq!(file.get(&1), Some(&1));
        assert_eq!(file.get(&2), Some(&0));
    }

    #[test]
    fn multiple_sf_sections_are_separate() {
        let c = cov("SF:a.rs\nDA:1,1\nend_of_record\nSF:b.rs\nDA:5,3\nend_of_record\n");
        assert_eq!(c.files.len(), 2);
        assert_eq!(c.files["a.rs"].get(&1), Some(&1));
        assert_eq!(c.files["b.rs"].get(&5), Some(&3));
    }

    #[test]
    fn ignores_fn_fnda_brda_and_summary_records() {
        let c = cov(
            "SF:src/lib.rs\nFN:10,do_thing\nFNDA:1,do_thing\nBRDA:12,0,0,1\nDA:12,1\nLF:1\nLH:1\nend_of_record\n",
        );
        assert_eq!(c.files["src/lib.rs"].get(&12), Some(&1));
    }

    #[test]
    fn coverage_for_intersects_function_range() {
        let c = cov("SF:lib.rs\nDA:1,1\nDA:2,0\nDA:3,1\nDA:9,1\nend_of_record\n");
        let (covered, total) = c.coverage_for("lib.rs", 1, 3).expect("matched");
        assert_eq!((covered, total), (2, 3)); // line 2 has 0 hits
    }

    #[test]
    fn coverage_for_matches_paths_by_suffix() {
        let c = cov("SF:crates/er-engine/src/lib.rs\nDA:7,1\nend_of_record\n");
        // Analyzed path may be relative to a different root.
        let (covered, total) = c
            .coverage_for("er-engine/src/lib.rs", 7, 8)
            .expect("matched");
        assert_eq!((covered, total), (1, 1));
        let (covered, total) = c.coverage_for("lib.rs", 7, 8).expect("matched");
        assert_eq!((covered, total), (1, 1));
    }

    #[test]
    fn no_matching_file_section_returns_none() {
        let c = cov("SF:other.rs\nDA:1,1\nend_of_record\n");
        assert!(c.coverage_for("lib.rs", 1, 5).is_none());
    }

    #[test]
    fn empty_range_returns_zero_totals() {
        let c = cov("SF:lib.rs\nDA:1,1\nend_of_record\n");
        assert_eq!(c.coverage_for("lib.rs", 5, 3).unwrap(), (0, 0));
    }

    // ── Negative tests: malformed input must fail loudly ──────────────────

    #[test]
    fn da_before_any_sf_record_is_an_error() {
        assert!(parse_lcov("DA:1,1\n").is_err());
    }

    #[test]
    fn malformed_da_line_number_is_an_error() {
        assert!(parse_lcov("SF:a.rs\nDA:abc,1\n").is_err());
    }

    #[test]
    fn malformed_da_hit_count_is_an_error() {
        assert!(parse_lcov("SF:a.rs\nDA:1,xyz\n").is_err());
    }

    #[test]
    fn da_missing_comma_is_an_error() {
        assert!(parse_lcov("SF:a.rs\nDA:1\n").is_err());
    }

    #[test]
    fn empty_input_parses_to_empty_coverage() {
        let c = cov("");
        assert!(c.files.is_empty());
    }
}
