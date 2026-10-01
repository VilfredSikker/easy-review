//! Matching a repo-relative path against a per-repo rule table.
//!
//! `[importance.<repo>]` and `[file_kinds.<repo>]` are both tables whose keys are
//! paths, globs or file types. They share one precedence order here so the two
//! cannot drift into resolving the same key differently.

use glob::{MatchOptions, Pattern};
use std::collections::BTreeMap;

/// Path-shaped rules match separator by separator, so `src/*` names the files
/// directly under `src`. A bare extension pattern never reaches here — the
/// file-type level matches those against the basename.
const MATCH_OPTIONS: MatchOptions = MatchOptions {
    case_sensitive: true,
    require_literal_separator: true,
    require_literal_leading_dot: false,
};

/// Whether a rule key is a pattern rather than the name of one path.
pub(crate) fn is_pattern(key: &str) -> bool {
    key.contains(['*', '?', '['])
}

fn matches_pattern(pattern: &str, target: &str) -> bool {
    Pattern::new(pattern).is_ok_and(|pattern| pattern.matches_with(target, MATCH_OPTIONS))
}

/// The rule that claims `path`: the key as it was written, and its parsed value.
///
/// Precedence runs most-specific-first: exact path, then glob, then file type.
/// Within the pattern levels the longest matching key wins, so a narrow rule can
/// carve an exception out of a broader one that would otherwise claim the same
/// path.
///
/// A winning key whose value does not parse resolves to nothing at its level:
/// the fall-through goes to the next level, never to the next-shortest matching
/// key, so a typo cannot silently promote a broader rule into the answer.
pub(crate) fn matching_rule<'a, T>(
    rules: &'a BTreeMap<String, String>,
    path: &str,
    parse: impl Fn(&str) -> Option<T>,
) -> Option<(&'a str, T)> {
    let exact = || {
        let (key, value) = rules.get_key_value(path)?;
        Some((key.as_str(), parse(value)?))
    };
    let glob = || pattern_rule(rules, path, true, &parse);
    let file_type = || {
        let basename = path.rsplit(['/', '\\']).next().unwrap_or(path);
        pattern_rule(rules, basename, false, &parse)
    };
    exact().or_else(glob).or_else(file_type)
}

/// A key carrying a separator names a place in the tree and is matched against
/// the whole path; one without names a kind of file and is matched against the
/// basename.
fn pattern_rule<'a, T>(
    rules: &'a BTreeMap<String, String>,
    target: &str,
    path_shaped: bool,
    parse: &impl Fn(&str) -> Option<T>,
) -> Option<(&'a str, T)> {
    let (key, value) = rules
        .iter()
        .filter(|(key, _)| is_pattern(key) && key.contains('/') == path_shaped)
        .filter(|(key, _)| matches_pattern(key, target))
        .max_by_key(|(key, _)| key.len())?;
    Some((key.as_str(), parse(value)?))
}
