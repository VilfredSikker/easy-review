//! Positive gate tests: clean, fully covered fixtures must pass the gate.
//!
//! This is the "way to test for it": the fixtures pin both the analyzer's
//! complexity numbers and the coverage merge, and the assertion is on the
//! gate exit code plus the JSON report.

use er_crap::run;

mod common;
use common::{opts, write_fixture};

#[test]
fn clean_fixtures_pass_the_gate() {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path(), "clean");
    let outcome =
        run(&opts(dir.path(), "clean.lcov", true)).expect("run succeeds on clean fixtures");
    assert_eq!(
        outcome.exit_code, 0,
        "clean code must pass the gate (exit 0):\n{}",
        outcome.report
    );
}

#[test]
fn clean_fixtures_are_fully_covered_and_score_their_complexity() {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path(), "clean");
    let outcome = run(&opts(dir.path(), "clean.lcov", false)).expect("run succeeds");
    let json: serde_json::Value = serde_json::from_str(&outcome.report).expect("valid JSON");
    assert_eq!(
        json["total"], 5,
        "five functions expected:\n{}",
        outcome.report
    );
    assert_eq!(json["crappy"], 0);
    for entry in json["entries"].as_array().expect("entries array") {
        assert_eq!(
            entry["coverage"], 100.0,
            "every clean fixture function must be fully covered (pins clean.lcov):\n{}",
            outcome.report
        );
        // At 100% coverage CRAP collapses to CC, so score == complexity.
        assert_eq!(
            entry["crap"],
            entry["cyclomatic"].as_f64().unwrap(),
            "score must equal complexity at 100% coverage"
        );
    }
}

#[test]
fn clean_fixtures_have_expected_complexities() {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path(), "clean");
    let outcome = run(&opts(dir.path(), "clean.lcov", false)).expect("run succeeds");
    let json: serde_json::Value = serde_json::from_str(&outcome.report).unwrap();
    let by_name = |name: &str| {
        json["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["function"] == name)
            .unwrap_or_else(|| panic!("missing function {name}"))
            .clone()
    };
    assert_eq!(by_name("add")["cyclomatic"], 1);
    assert_eq!(by_name("sign")["cyclomatic"], 2);
    assert_eq!(by_name("describe")["cyclomatic"], 3);
    assert_eq!(by_name("pick")["cyclomatic"], 4);
    assert_eq!(by_name("within")["cyclomatic"], 2);
}
