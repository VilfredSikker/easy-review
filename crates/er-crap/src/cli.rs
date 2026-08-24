//! Command-line interface (clap derive).

use std::path::PathBuf;

use clap::Parser;

use crate::report::OutputFormat;
use crate::{Opts, DEFAULT_THRESHOLD};

/// Compute the CRAP (Change Risk Anti-Patterns) metric for Rust functions.
///
/// CRAP combines cyclomatic complexity with unit-test coverage; functions
/// scoring above the threshold (30 by default) are risky to change. Feed it
/// an LCOV report from `cargo llvm-cov --lcov` for real coverage data.
#[derive(Parser, Debug)]
#[command(name = "er-crap", version, about, long_about = None)]
pub struct Cli {
    /// LCOV coverage report (from `cargo llvm-cov --lcov`).
    #[arg(long, value_name = "FILE")]
    pub lcov: Option<PathBuf>,

    /// Root directory to walk for `.rs` files.
    #[arg(long, default_value = ".")]
    pub path: PathBuf,

    /// Score above which a function is flagged as CRAPpy.
    #[arg(long, default_value_t = DEFAULT_THRESHOLD)]
    pub threshold: f64,

    /// Exit 1 when any function exceeds the threshold (CI gate).
    #[arg(long)]
    pub fail_above: bool,

    /// Output format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
    pub format: OutputFormat,

    /// Print only the aggregate summary (human format only).
    #[arg(long)]
    pub summary: bool,
}

impl From<Cli> for Opts {
    fn from(cli: Cli) -> Self {
        Self {
            lcov_path: cli.lcov,
            path: cli.path,
            threshold: cli.threshold,
            fail_above: cli.fail_above,
            format: cli.format,
            summary: cli.summary,
        }
    }
}
