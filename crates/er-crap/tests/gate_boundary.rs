//! Boundary gate test: a function scoring EXACTLY the threshold (30.0) must
//! NOT be flagged — the gate uses strict `>` semantics. This closes the
//! `>` vs `>=` mutation gap through the full pipeline.

use er_crap::run;

mod common;
use common::{opts, write_fixture};

#[test]
fn exactly_threshold_is_not_crappy() {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path(), "boundary");

    let outcome = run(&opts(dir.path(), "boundary.lcov", true)).expect("run succeeds");
    assert_eq!(
        outcome.exit_code, 0,
        "score of exactly 30.0 must pass the gate:\n{}",
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
    std::fs::write(
        dir.path().join("above.rs"),
        "pub fn above(a: bool, b: bool, c: bool, d: bool, e: bool, f: bool) -> bool {\n    a && b || c && d || e || f\n}\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("above.lcov"),
        "SF:above.rs\nend_of_record\n",
    )
    .unwrap();

    let outcome = run(&opts(dir.path(), "above.lcov", true)).expect("run succeeds");
    assert_eq!(outcome.exit_code, 1, "CC 6 at 0% coverage is crappy");
    let json: serde_json::Value = serde_json::from_str(&outcome.report).unwrap();
    assert_eq!(json["crappy"], 1);
    assert!(json["entries"][0]["crap"].as_f64().unwrap() > 30.0);
}
