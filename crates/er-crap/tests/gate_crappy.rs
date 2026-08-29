//! Negative gate tests: complex, barely-tested fixtures must FAIL the gate.
//!
//! "Bad input must fail the gate" is the canonical negative test for the CRAP
//! tooling: the assertion is that exit code 1 is returned and the JSON report
//! proves *why* (score above threshold, coverage as declared).

use er_crap::report::OutputFormat;
use er_crap::{run, Opts};

mod common;
use common::{opts, write_fixture};

#[test]
fn crappy_fixtures_fail_the_gate() {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path(), "crappy");
    let outcome =
        run(&opts(dir.path(), "crappy.lcov", true)).expect("run succeeds on crappy fixtures");
    assert_eq!(
        outcome.exit_code, 1,
        "crappy code must fail the gate (exit 1):\n{}",
        outcome.report
    );
}

#[test]
fn crappy_fixtures_report_scores_above_threshold() {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path(), "crappy");
    let outcome = run(&opts(dir.path(), "crappy.lcov", false)).expect("run succeeds");
    let json: serde_json::Value = serde_json::from_str(&outcome.report).expect("valid JSON");
    assert_eq!(
        json["total"], 2,
        "two functions expected:\n{}",
        outcome.report
    );
    assert!(json["crappy"].as_u64().unwrap() >= 2);

    let by_name = |name: &str| {
        json["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["function"] == name)
            .unwrap_or_else(|| panic!("missing function {name}"))
            .clone()
    };

    // classify: CC 9, 20% coverage (one hit out of five DA lines) → > 30.
    let classify = by_name("classify");
    assert_eq!(classify["cyclomatic"], 9);
    assert_eq!(classify["coverage"], 20.0);
    assert!(classify["crap"].as_f64().unwrap() > 30.0);

    // evaluate: CC 11, no coverage data → pessimistic 0% → > 30.
    let evaluate = by_name("evaluate");
    assert_eq!(evaluate["cyclomatic"], 11);
    assert_eq!(evaluate["coverage"], 0.0);
    assert!(evaluate["crap"].as_f64().unwrap() > 30.0);
}

#[test]
fn without_fail_above_the_gate_reports_but_passes() {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path(), "crappy");
    let outcome = run(&opts(dir.path(), "crappy.lcov", false)).expect("run succeeds");
    assert_eq!(
        outcome.exit_code, 0,
        "gate off → exit 0 even for crappy code"
    );
}

#[test]
fn human_report_lists_crappy_functions_and_summary() {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path(), "crappy");
    let outcome = run(&Opts {
        format: OutputFormat::Human,
        summary: false,
        ..opts(dir.path(), "crappy.lcov", false)
    })
    .expect("run succeeds");
    assert!(
        outcome.report.contains("classify"),
        "human table lists crappy fn:\n{}",
        outcome.report
    );
    assert!(outcome.report.contains("evaluate"));
    assert!(
        outcome.report.contains("exceed the CRAP threshold of 30"),
        "summary line present:\n{}",
        outcome.report
    );
}

#[test]
fn summary_only_omits_the_table() {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path(), "crappy");
    let outcome = run(&Opts {
        format: OutputFormat::Human,
        summary: true,
        ..opts(dir.path(), "crappy.lcov", false)
    })
    .expect("run succeeds");
    assert!(
        !outcome.report.contains("classify"),
        "summary mode has no table:\n{}",
        outcome.report
    );
    assert!(outcome.report.contains("exceed the CRAP threshold of 30"));
}
