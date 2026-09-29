//! Per-file, per-lint counts compared against a baseline that may only shrink.
//!
//! The file format matches the desktop UI's `eslint-warning-budget.json`, so
//! both halves of the repo read the same way: `{ file: { lint: { count } } }`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Count {
    pub count: usize,
}

pub type Baseline = BTreeMap<String, BTreeMap<String, Count>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub file: String,
    pub lint: String,
    pub count: usize,
    pub allowed: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RatchetResult {
    /// (file, lint) pairs above their baseline.
    pub over: Vec<Entry>,
    /// Baseline entries whose current count is lower: the baseline must be pruned.
    pub stale: Vec<Entry>,
    /// The baseline shrunk to the current counts, never grown.
    pub pruned: Baseline,
    /// The current counts, growth included.
    pub current: Baseline,
}

/// Count occurrences per (file, lint).
pub fn count_by_file_lint<'a>(pairs: impl IntoIterator<Item = (&'a str, &'a str)>) -> Baseline {
    let mut out = Baseline::new();
    for (file, lint) in pairs {
        out.entry(file.to_string())
            .or_default()
            .entry(lint.to_string())
            .or_insert(Count { count: 0 })
            .count += 1;
    }
    out
}

/// Compare current counts with a baseline. A file cannot spend slack from
/// another file, and a lint cannot spend slack from another lint.
pub fn ratchet(current: &Baseline, baseline: &Baseline) -> RatchetResult {
    let mut over = Vec::new();
    let mut stale = Vec::new();
    let mut pruned = Baseline::new();

    let mut files: Vec<&String> = current.keys().chain(baseline.keys()).collect();
    files.sort();
    files.dedup();
    for file in files {
        let now = current.get(file);
        let was = baseline.get(file);
        let mut lints: Vec<&String> = now
            .into_iter()
            .flat_map(BTreeMap::keys)
            .chain(was.into_iter().flat_map(BTreeMap::keys))
            .collect();
        lints.sort();
        lints.dedup();
        for lint in lints {
            let count = now.and_then(|m| m.get(lint)).map_or(0, |c| c.count);
            let allowed = was.and_then(|m| m.get(lint)).map_or(0, |c| c.count);
            let entry = || Entry {
                file: file.clone(),
                lint: lint.clone(),
                count,
                allowed,
            };
            if count > allowed {
                over.push(entry());
            }
            if count < allowed {
                stale.push(entry());
            }
            let keep = count.min(allowed);
            if keep > 0 {
                pruned
                    .entry(file.clone())
                    .or_default()
                    .insert(lint.clone(), Count { count: keep });
            }
        }
    }
    RatchetResult {
        over,
        stale,
        pruned,
        current: current.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(entries: &[(&str, &str, usize)]) -> Baseline {
        let mut out = Baseline::new();
        for &(file, lint, count) in entries {
            out.entry(file.to_string())
                .or_default()
                .insert(lint.to_string(), Count { count });
        }
        out
    }

    fn entry(file: &str, lint: &str, count: usize, allowed: usize) -> Entry {
        Entry {
            file: file.into(),
            lint: lint.into(),
            count,
            allowed,
        }
    }

    #[test]
    fn counts_per_file_and_lint() {
        let counted =
            count_by_file_lint([("b.rs", "x"), ("a.rs", "y"), ("a.rs", "x"), ("a.rs", "x")]);
        assert_eq!(
            counted,
            b(&[("a.rs", "x", 2), ("a.rs", "y", 1), ("b.rs", "x", 1)])
        );
    }

    #[test]
    fn passes_when_counts_match() {
        let base = b(&[("a.rs", "x", 2)]);
        let r = ratchet(&base, &base);
        assert!(r.over.is_empty());
        assert!(r.stale.is_empty());
        assert_eq!(r.pruned, base);
    }

    #[test]
    fn fails_a_pair_that_grows() {
        let r = ratchet(&b(&[("a.rs", "x", 3)]), &b(&[("a.rs", "x", 2)]));
        assert_eq!(r.over, vec![entry("a.rs", "x", 3, 2)]);
        assert_eq!(r.pruned, b(&[("a.rs", "x", 2)]));
    }

    #[test]
    fn fails_a_file_or_lint_with_no_baseline() {
        let r = ratchet(
            &b(&[("a.rs", "x", 2), ("a.rs", "y", 1), ("b.rs", "x", 1)]),
            &b(&[("a.rs", "x", 2)]),
        );
        assert_eq!(
            r.over,
            vec![entry("a.rs", "y", 1, 0), entry("b.rs", "x", 1, 0)]
        );
    }

    #[test]
    fn one_file_cannot_spend_another_files_slack() {
        let r = ratchet(&b(&[("b.rs", "x", 1)]), &b(&[("a.rs", "x", 2)]));
        assert_eq!(r.over, vec![entry("b.rs", "x", 1, 0)]);
        assert_eq!(r.stale, vec![entry("a.rs", "x", 0, 2)]);
    }

    #[test]
    fn shrinkage_is_stale_and_prunes_to_the_lower_count() {
        let r = ratchet(&b(&[("a.rs", "x", 1)]), &b(&[("a.rs", "x", 2)]));
        assert!(r.over.is_empty());
        assert_eq!(r.stale, vec![entry("a.rs", "x", 1, 2)]);
        assert_eq!(r.pruned, b(&[("a.rs", "x", 1)]));
    }

    #[test]
    fn prune_drops_fully_fixed_entries_and_never_grows() {
        assert!(ratchet(&Baseline::new(), &b(&[("a.rs", "x", 2)]))
            .pruned
            .is_empty());
        let r = ratchet(
            &b(&[("a.rs", "x", 5), ("c.rs", "z", 1)]),
            &b(&[("a.rs", "x", 2)]),
        );
        assert_eq!(r.pruned, b(&[("a.rs", "x", 2)]));
        assert_eq!(r.current, b(&[("a.rs", "x", 5), ("c.rs", "z", 1)]));
    }
}
