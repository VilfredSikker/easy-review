//! CRAP (Change Risk Anti-Patterns) metric for Rust functions.
//!
//! Deliberate style decision (matches er-engine/er-tui/er-desktop): keep
//! `match Some/None` / if-let chains over `Option::map_or[_else]`.
#![allow(clippy::option_if_let_else)]
//!
//! CRAP combines a function's cyclomatic complexity with its unit-test
//! coverage into a single risk score:
//!
//! ```text
//! CRAP(m) = CC(m)² × (1 − cov(m)/100)³ + CC(m)
//! ```
//!
//! where `CC(m)` is the cyclomatic complexity of the method and `cov(m)` its
//! test coverage in percent. Scores above the default threshold of 30 mark a
//! function as risky to change. The metric was introduced by Savoia & Evans
//! (2007) and popularized by NDepend:
//! <https://blog.ndepend.com/crap-metric-thing-tells-risk-code/>
//!
//! This crate is the in-repo implementation of the metric for Easy Review:
//! [`complexity`] computes cyclomatic complexity from Rust source (via
//! `syn`), [`lcov`] reads per-line coverage from an LCOV report (as produced
//! by `cargo llvm-cov --lcov`), and [`run`] ties the two together into a
//! per-function report and an optional CI gate (`--fail-above`).

pub mod cli;
pub mod complexity;
pub mod lcov;
pub mod report;

use std::path::{Path, PathBuf};

use anyhow::Context;

/// Default threshold above which a function is flagged as CRAPpy.
pub const DEFAULT_THRESHOLD: f64 = 30.0;

/// Compute the CRAP score for one function.
///
/// `cyclomatic` is the function's cyclomatic complexity (≥ 1 by definition,
/// but clamped defensively); `coverage_pct` is its test coverage in percent.
/// Coverage is clamped to `[0, 100]` and the uncovered rate to `[0, 1]`, so
/// out-of-range inputs never produce NaN or negative scores.
pub fn crap_score(cyclomatic: f64, coverage_pct: f64) -> f64 {
    // Non-finite inputs (NaN, ±inf) are treated as "no data": complexity 0,
    // coverage 0% (pessimistic), so the score is always a real number.
    let cc = if cyclomatic.is_finite() {
        cyclomatic.max(0.0)
    } else {
        0.0
    };
    let cov = if coverage_pct.is_finite() {
        coverage_pct.clamp(0.0, 100.0)
    } else {
        0.0
    };
    let uncovered = (1.0 - cov / 100.0).clamp(0.0, 1.0);
    (cc * cc).mul_add(uncovered.powi(3), cc)
}

/// Whether a score is CRAPpy, i.e. strictly above the threshold.
pub fn is_crappy(score: f64, threshold: f64) -> bool {
    score > threshold
}

/// Options for a [`run`] invocation.
#[derive(Debug, Clone)]
pub struct Opts {
    /// Path to an LCOV coverage report (from `cargo llvm-cov --lcov`).
    pub lcov_path: Option<PathBuf>,
    /// Root directories to walk for `.rs` files (repeatable, e.g. scope to
    /// exactly the crates a coverage run measured).
    pub path: Vec<PathBuf>,
    /// Score above which a function is flagged (see [`DEFAULT_THRESHOLD`]).
    pub threshold: f64,
    /// Exit with code 1 when any function exceeds the threshold.
    pub fail_above: bool,
    /// Output format for the report.
    pub format: report::OutputFormat,
    /// Print only the aggregate summary (human table / JSON entries are omitted).
    pub summary: bool,
}

/// Result of a [`run`]: the exit code to return and the rendered report.
#[derive(Debug)]
pub struct RunOutcome {
    /// 0 = ok, 1 = gate failure (with `fail_above`).
    pub exit_code: i32,
    /// The rendered report (human table or JSON).
    pub report: String,
}

/// Directories never analyzed, mirroring cargo-crap's default exclusions:
/// integration-test and bench/example code exists to cover production code,
/// so it would only add 0%-coverage noise.
const SKIP_DIRS: &[&str] = &[
    "target",
    ".git",
    "node_modules",
    "tests",
    "benches",
    "examples",
    "mutants.out",
];

/// Walk `roots` for `.rs` files and return `(relative_path, source)` pairs.
fn walk_rs_files(roots: &[PathBuf]) -> anyhow::Result<Vec<(PathBuf, String)>> {
    let mut out = Vec::new();
    for root in roots {
        if !root.exists() {
            anyhow::bail!("path does not exist: {}", root.display());
        }
        walk(root, root, &mut out)?;
    }
    Ok(out)
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<(PathBuf, String)>) -> anyhow::Result<()> {
    let read =
        std::fs::read_dir(dir).with_context(|| format!("failed to read dir {}", dir.display()))?;
    for entry in read {
        let entry =
            entry.with_context(|| format!("failed to read dir entry in {}", dir.display()))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        // Use the dirent file type (does not follow symlinks) so a link cycle
        // cannot recurse forever; symlinked dirs/files are simply skipped.
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to stat dir entry {} in {}", name, dir.display()))?;
        if file_type.is_dir() {
            if SKIP_DIRS.contains(&name.as_str()) {
                continue;
            }
            walk(root, &path, out)?;
        } else if file_type.is_file() && name.ends_with(".rs") {
            let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
            let source = std::fs::read_to_string(&path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            out.push((rel, source));
        }
    }
    Ok(())
}

/// Run a full analysis: walk source, compute complexity, merge coverage, and
///
/// render the report. Returns the process exit code and rendered report so
/// both the binary and the in-process tests can share this entry point.
pub fn run(opts: &Opts) -> anyhow::Result<RunOutcome> {
    let coverage = match &opts.lcov_path {
        Some(path) => {
            let contents = std::fs::read_to_string(path)
                .with_context(|| format!("failed to read LCOV file {}", path.display()))?;
            Some(lcov::parse_lcov(&contents)?)
        }
        None => None,
    };

    let mut entries = Vec::new();
    for (rel, source) in walk_rs_files(&opts.path)? {
        let rel_str = rel.to_string_lossy().to_string();
        for f in complexity::analyze_file(&source) {
            let coverage_pct = coverage_pct_of(&coverage, &rel_str, f.start_line, f.end_line);
            let score = crap_score(f.complexity as f64, coverage_pct);
            entries.push(report::Entry {
                file: rel_str.clone(),
                function: f.name,
                line: f.start_line,
                cyclomatic: f.complexity,
                coverage: coverage_pct,
                crap: score,
            });
        }
    }

    entries.sort_by(|a, b| {
        b.crap
            .partial_cmp(&a.crap)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let report = match opts.format {
        report::OutputFormat::Json => report::render_json(&entries, opts.threshold, opts.summary)?,
        report::OutputFormat::Human => report::render_human(&entries, opts.threshold, opts.summary),
    };
    let crappy = entries
        .iter()
        .filter(|e| is_crappy(e.crap, opts.threshold))
        .count();
    let exit_code = if opts.fail_above && crappy > 0 { 1 } else { 0 };
    Ok(RunOutcome { exit_code, report })
}

/// Per-function coverage percent from the parsed LCOV report: functions with
/// no coverage data at all score 0% (pessimistic — the same default as
/// cargo-crap's `--missing pessimistic`).
fn coverage_pct_of(
    coverage: &Option<lcov::LcovCoverage>,
    file: &str,
    start: usize,
    end: usize,
) -> f64 {
    coverage
        .as_ref()
        .and_then(|c| c.coverage_for(file, start, end))
        .map(|(covered, total)| {
            if total == 0 {
                0.0
            } else {
                covered as f64 / total as f64 * 100.0
            }
        })
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "expected {expected}, got {actual}"
        );
    }

    // ── Golden values from the NDepend blog post ──────────────────────────

    #[test]
    fn minimum_score_is_one() {
        assert_close(crap_score(1.0, 100.0), 1.0);
    }

    #[test]
    fn uncovered_trivial_function_scores_two() {
        assert_close(crap_score(1.0, 0.0), 2.0);
    }

    /// The blog's worked example says CC=6 at 0% coverage → 37, but that is a
    /// typo: 6² × 1³ + 6 = 42. We pin the formula, not the typo.
    #[test]
    fn blog_example_complexity_six_uncovered() {
        assert_close(crap_score(6.0, 0.0), 42.0);
    }

    #[test]
    fn complexity_ten_uncovered_is_110() {
        assert_close(crap_score(10.0, 0.0), 110.0);
    }

    /// CC=10 needs ≥ 42% coverage to drop below the 30 threshold.
    #[test]
    fn complexity_ten_at_42_percent_is_below_threshold() {
        let score = crap_score(10.0, 42.0);
        assert!(score < 30.0, "score {score} should be below 30");
        assert!(score > 29.0, "score {score} should be just below 30");
    }

    #[test]
    fn complexity_ten_at_41_percent_is_crappy() {
        let score = crap_score(10.0, 41.0);
        assert!(score > 30.0, "score {score} should be above 30");
    }

    /// CC=25 needs ~80% coverage; exactly 80% lands on the boundary (not > 30).
    #[test]
    fn complexity_twentyfive_at_80_percent_is_boundary() {
        assert_close(crap_score(25.0, 80.0), 30.0);
    }

    #[test]
    fn complexity_twentyfive_at_79_percent_is_crappy() {
        let score = crap_score(25.0, 79.0);
        assert!(score > 30.0, "score {score} should be above 30");
    }

    /// CC=30 with 100% coverage scores exactly 30 (not crappy, boundary).
    #[test]
    fn complexity_thirty_fully_covered_is_boundary() {
        assert_close(crap_score(30.0, 100.0), 30.0);
        assert!(!is_crappy(crap_score(30.0, 100.0), DEFAULT_THRESHOLD));
    }

    /// Above CC=30 no amount of coverage keeps the score under the threshold.
    #[test]
    fn complexity_above_thirty_cannot_be_saved_by_coverage() {
        assert_close(crap_score(31.0, 100.0), 31.0);
        assert!(is_crappy(crap_score(31.0, 100.0), DEFAULT_THRESHOLD));
        assert!(crap_score(40.0, 100.0) > 30.0);
    }

    // ── Clamping / edge inputs (negative tests) ──────────────────────────

    #[test]
    fn coverage_above_100_is_clamped() {
        assert_close(crap_score(1.0, 150.0), 1.0);
        assert_close(crap_score(5.0, 200.0), 5.0);
    }

    #[test]
    fn negative_coverage_is_clamped_to_zero() {
        assert_close(crap_score(2.0, -10.0), 6.0); // 4 × 1³ + 2
        assert_close(crap_score(5.0, -1.0), 30.0); // 25 + 5
    }

    #[test]
    fn negative_complexity_is_clamped() {
        assert_close(crap_score(-3.0, 0.0), 0.0);
    }

    #[test]
    fn never_returns_nan_for_bad_inputs() {
        assert!(!crap_score(f64::NAN, 50.0).is_nan());
        assert!(!crap_score(5.0, f64::NAN).is_nan());
        assert!(!crap_score(f64::INFINITY, 0.0).is_nan());
    }

    #[test]
    fn threshold_boundary_is_strict() {
        assert!(!is_crappy(30.0, 30.0), "exactly 30 is not crappy");
        assert!(is_crappy(30.0001, 30.0), "just above 30 is crappy");
        assert!(!is_crappy(1.0, 30.0));
        assert!(is_crappy(30.0, 29.9));
    }

    #[test]
    fn coverage_math_matches_formula_shape() {
        // Higher coverage always lowers the score for a fixed complexity.
        let low = crap_score(12.0, 10.0);
        let high = crap_score(12.0, 90.0);
        assert!(low > high);
        assert!(high > 12.0, "at 100% coverage CRAP equals CC");
        assert_close(crap_score(12.0, 100.0), 12.0);
    }
}
