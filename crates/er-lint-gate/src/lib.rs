//! Clippy as a ratcheted gate (docs/adr/0038-lint-is-a-ratcheted-gate.md).
//!
//! Every diagnostic fails, exactly as `-D warnings` did, except the lints the
//! workspace lists as budgeted. Those are counted per file against
//! `clippy-warning-budget.json`: a count may not rise, and a count that falls
//! fails until the budget is pruned, so a fix cannot leave slack behind.
//!
//! Clippy has no bulk suppression for errors, so the error tier has no
//! baseline: it lands at zero, and a real problem with many existing sites is
//! budgeted here instead.

pub mod cohesion;
pub mod diagnostics;
pub mod ratchet;

use std::collections::HashSet;

use diagnostics::Diagnostic;
use ratchet::{Baseline, RatchetResult};

/// What a finished clippy run means for the gate. Pure: the binary does the IO.
#[derive(Debug)]
pub struct Evaluation {
    /// Diagnostics outside the budget. Any one fails the gate.
    pub failures: Vec<Diagnostic>,
    /// Budgeted diagnostics, in stream order.
    pub budgeted: Vec<Diagnostic>,
    pub ratchet: RatchetResult,
    /// Whether every crate was linted. An error in one crate stops its
    /// dependents from being checked, so their budgeted counts read zero;
    /// stale reports and baseline writes are refused until the run is whole.
    pub complete: bool,
}

pub fn evaluate(
    diags: Vec<Diagnostic>,
    budgeted_lints: &HashSet<String>,
    budget: &Baseline,
    cargo_succeeded: bool,
) -> Evaluation {
    let (budgeted, failures): (Vec<_>, Vec<_>) = diags.into_iter().partition(|d| {
        d.level == "warning"
            && d.file.is_some()
            && d.code.as_ref().is_some_and(|c| budgeted_lints.contains(c))
    });
    let current = ratchet::count_by_file_lint(
        budgeted
            .iter()
            .filter_map(|d| Some((d.file.as_deref()?, d.code.as_deref()?))),
    );
    let complete = cargo_succeeded && !failures.iter().any(|d| d.level == "error");
    Evaluation {
        failures,
        budgeted,
        ratchet: ratchet::ratchet(&current, budget),
        complete,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratchet::Count;

    fn diag(level: &str, code: Option<&str>, file: Option<&str>) -> Diagnostic {
        Diagnostic {
            level: level.into(),
            code: code.map(Into::into),
            file: file.map(Into::into),
            line: 1,
            column: 1,
            rendered: String::new(),
        }
    }

    fn lints(names: &[&str]) -> HashSet<String> {
        names.iter().map(ToString::to_string).collect()
    }

    fn budget(file: &str, lint: &str, count: usize) -> Baseline {
        let mut b = Baseline::new();
        b.entry(file.into())
            .or_default()
            .insert(lint.into(), Count { count });
        b
    }

    #[test]
    fn a_budgeted_warning_within_budget_passes() {
        let e = evaluate(
            vec![diag("warning", Some("clippy::unwrap_used"), Some("a.rs"))],
            &lints(&["clippy::unwrap_used"]),
            &budget("a.rs", "clippy::unwrap_used", 1),
            true,
        );
        assert!(e.failures.is_empty());
        assert!(e.ratchet.over.is_empty());
        assert!(e.ratchet.stale.is_empty());
        assert!(e.complete);
    }

    #[test]
    fn any_other_warning_fails_like_deny_warnings() {
        let e = evaluate(
            vec![diag("warning", Some("unused_variables"), Some("a.rs"))],
            &lints(&["clippy::unwrap_used"]),
            &Baseline::new(),
            true,
        );
        assert_eq!(e.failures.len(), 1);
        assert!(e.complete, "a warning does not stop other crates");
    }

    #[test]
    fn a_budgeted_lint_raised_to_error_fails() {
        let e = evaluate(
            vec![diag("error", Some("clippy::unwrap_used"), Some("a.rs"))],
            &lints(&["clippy::unwrap_used"]),
            &budget("a.rs", "clippy::unwrap_used", 1),
            false,
        );
        assert_eq!(e.failures.len(), 1);
    }

    #[test]
    fn a_budgeted_lint_with_no_workspace_file_fails() {
        let e = evaluate(
            vec![diag("warning", Some("clippy::unwrap_used"), None)],
            &lints(&["clippy::unwrap_used"]),
            &Baseline::new(),
            true,
        );
        assert_eq!(e.failures.len(), 1);
    }

    #[test]
    fn an_error_or_failed_cargo_marks_the_run_incomplete() {
        let errored = evaluate(
            vec![diag("error", Some("clippy::float_cmp"), Some("a.rs"))],
            &HashSet::new(),
            &Baseline::new(),
            false,
        );
        assert!(!errored.complete);
        let cargo_failed = evaluate(vec![], &HashSet::new(), &Baseline::new(), false);
        assert!(!cargo_failed.complete);
    }

    #[test]
    fn a_new_budgeted_warning_is_over_budget() {
        let e = evaluate(
            vec![
                diag("warning", Some("clippy::unwrap_used"), Some("a.rs")),
                diag("warning", Some("clippy::unwrap_used"), Some("b.rs")),
            ],
            &lints(&["clippy::unwrap_used"]),
            &budget("a.rs", "clippy::unwrap_used", 1),
            true,
        );
        assert_eq!(e.ratchet.over.len(), 1);
        assert_eq!(e.ratchet.over[0].file, "b.rs");
    }
}
