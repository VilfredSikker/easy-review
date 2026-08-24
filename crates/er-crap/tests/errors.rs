//! Error-path (negative) tests for the CRAP tooling itself: invalid inputs
//! must fail loudly, empty inputs must degrade gracefully.

use std::fs;

use er_crap::report::OutputFormat;
use er_crap::{run, Opts};

fn base_opts(dir: &tempfile::TempDir) -> Opts {
    Opts {
        lcov_path: None,
        path: dir.path().to_path_buf(),
        threshold: 30.0,
        fail_above: true,
        format: OutputFormat::Json,
        summary: false,
    }
}

#[test]
fn missing_lcov_file_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("a.rs"),
        "fn f() {}
",
    )
    .unwrap();
    let opts = Opts {
        lcov_path: Some(dir.path().join("missing.lcov")),
        ..base_opts(&dir)
    };
    let err = run(&opts).expect_err("missing LCOV file must be an error");
    assert!(
        err.to_string().contains("missing.lcov"),
        "error mentions the file: {err}"
    );
}

#[test]
fn malformed_lcov_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("a.rs"),
        "fn f() {}
",
    )
    .unwrap();
    fs::write(
        dir.path().join("bad.lcov"),
        "DA:not-a-number,1
",
    )
    .unwrap();
    let opts = Opts {
        lcov_path: Some(dir.path().join("bad.lcov")),
        ..base_opts(&dir)
    };
    assert!(run(&opts).is_err(), "malformed LCOV must be an error");
}

#[test]
fn missing_source_path_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let opts = Opts {
        path: dir.path().join("does-not-exist"),
        ..base_opts(&dir)
    };
    let err = run(&opts).expect_err("missing source path must be an error");
    assert!(err.to_string().contains("does-not-exist"));
}

#[test]
fn empty_directory_reports_zero_functions_and_passes() {
    let dir = tempfile::tempdir().unwrap();
    let outcome = run(&base_opts(&dir)).expect("empty dir is a clean run");
    assert_eq!(outcome.exit_code, 0);
    let json: serde_json::Value = serde_json::from_str(&outcome.report).unwrap();
    assert_eq!(json["total"], 0);
    assert_eq!(json["crappy"], 0);
}

#[test]
fn threshold_zero_flags_every_function() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("a.rs"),
        "fn f() {}
",
    )
    .unwrap();
    let opts = Opts {
        threshold: 0.0,
        ..base_opts(&dir)
    };
    let outcome = run(&opts).expect("run succeeds");
    assert_eq!(
        outcome.exit_code, 1,
        "threshold 0 flags even a trivial function"
    );
    let json: serde_json::Value = serde_json::from_str(&outcome.report).unwrap();
    assert_eq!(json["crappy"], 1);
}

#[test]
fn files_without_coverage_data_score_zero_coverage() {
    // No --lcov at all: every function is pessimistically 0% covered.
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("a.rs"),
        "fn f() {}
",
    )
    .unwrap();
    let outcome = run(&base_opts(&dir)).expect("run succeeds without lcov");
    let json: serde_json::Value = serde_json::from_str(&outcome.report).unwrap();
    let entry = &json["entries"][0];
    assert_eq!(entry["coverage"], 0.0);
    assert_eq!(entry["crap"], 2.0); // CC 1 at 0% → 1 + 1
}

#[test]
fn unparseable_rust_files_are_skipped_not_fatal() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("broken.rs"),
        "fn broken( {
",
    )
    .unwrap();
    let outcome = run(&base_opts(&dir)).expect("run succeeds on unparseable files");
    assert_eq!(outcome.exit_code, 0);
    let json: serde_json::Value = serde_json::from_str(&outcome.report).unwrap();
    assert_eq!(json["total"], 0, "broken file contributes no functions");
}
