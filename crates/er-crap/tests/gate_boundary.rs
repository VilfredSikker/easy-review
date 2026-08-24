//! Boundary gate test: a function scoring EXACTLY the threshold (30.0) must
//! NOT be flagged — the gate uses strict \`>\` semantics. This closes the
//! \`>\` vs \`>=\` mutation gap through the full pipeline.

use std::fs;

use er_crap::report::OutputFormat;
use er_crap::{run, Opts};

#[test]
fn exactly_threshold_is_not_crappy() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("boundary.rs"),
        include_str!("fixtures/boundary.rs"),
    )
    .unwrap();
    fs::write(
        dir.path().join("boundary.lcov"),
        include_str!("fixtures/boundary.lcov"),
    )
    .unwrap();

    let opts = Opts {
        lcov_path: Some(dir.path().join("boundary.lcov")),
        path: dir.path().to_path_buf(),
        threshold: 30.0,
        fail_above: true,
        format: OutputFormat::Json,
        summary: false,
    };
    let outcome = run(&opts).expect("run succeeds");
    assert_eq!(
        outcome.exit_code, 0,
        "score of exactly 30.0 must pass the gate:
{}",
        outcome.report
    );
    let json: serde_json::Value = serde_json::from_str(&outcome.report).unwrap();
    assert_eq!(json["crappy"], 0);
    let entry = &json["entries"][0];
    assert_eq!(entry["function"], "boundary");
    assert_eq!(entry["cyclomatic"], 5);
    assert_eq!(entry["coverage"], 0.0);
    assert_eq!(
        entry["crap"], 30.0,
        "CC 5 at 0% coverage scores exactly 30.0"
    );
}

#[test]
fn just_above_threshold_is_crappy() {
    // Same shape but with one extra decision: CC 6 at 0% → 42 > 30.
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("above.rs"),
        "pub fn above(a: bool, b: bool, c: bool, d: bool, e: bool, f: bool) -> bool {
    a && b || c && d || e || f
}
",
    )
    .unwrap();
    fs::write(
        dir.path().join("above.lcov"),
        "SF:above.rs
end_of_record
",
    )
    .unwrap();

    let opts = Opts {
        lcov_path: Some(dir.path().join("above.lcov")),
        path: dir.path().to_path_buf(),
        threshold: 30.0,
        fail_above: true,
        format: OutputFormat::Json,
        summary: false,
    };
    let outcome = run(&opts).expect("run succeeds");
    assert_eq!(outcome.exit_code, 1, "CC 6 at 0% coverage is crappy");
    let json: serde_json::Value = serde_json::from_str(&outcome.report).unwrap();
    assert_eq!(json["crappy"], 1);
    assert!(json["entries"][0]["crap"].as_f64().unwrap() > 30.0);
}
