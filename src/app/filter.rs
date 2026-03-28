use crate::ai::{ErReview, RiskLevel};
use crate::git::{DiffFile, FileStatus};
use glob::{MatchOptions, Pattern};

// ── Types ──

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StatusKind {
    Added,
    Modified,
    Deleted,
    Renamed,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SizeOp {
    GreaterThan,
    LessThan,
}

#[derive(Debug, Clone)]
pub enum FilterRule {
    Glob {
        include: bool,
        pattern: Pattern,
    },
    Status {
        include: bool,
        status: StatusKind,
    },
    Size {
        include: bool,
        op: SizeOp,
        threshold: usize,
    },
    Risk {
        include: bool,
        levels: Vec<RiskLevel>,
    },
}

pub struct FilterPreset {
    pub name: &'static str,
    pub expr: &'static str,
}

pub const FILTER_PRESETS: &[FilterPreset] = &[
    FilterPreset {
        name: "frontend",
        expr: "*.ts,*.tsx,*.js,*.jsx,*.html,*.css,*.scss,*.svelte,*.vue",
    },
    FilterPreset {
        name: "backend",
        expr: "*.rs,*.py,*.go,*.java,*.sql,*.ts",
    }, // *.ts intentionally in both — TS is used on both sides
    FilterPreset {
        name: "config",
        expr: "*.toml,*.yaml,*.yml,*.json,*.env",
    },
    FilterPreset {
        name: "docs",
        expr: "*.md,*.txt,*.rst",
    },
    FilterPreset {
        name: "review",
        expr: "-risk:info",
    },
];

impl FilterRule {
    fn is_include(&self) -> bool {
        match self {
            FilterRule::Glob { include, .. } => *include,
            FilterRule::Status { include, .. } => *include,
            FilterRule::Size { include, .. } => *include,
            FilterRule::Risk { include, .. } => *include,
        }
    }
}

// ── Parser ──

/// Parse a comma-separated filter expression into a list of rules.
/// Invalid globs are silently skipped.
pub fn parse_filter_expr(expr: &str) -> Vec<FilterRule> {
    let mut rules = Vec::new();
    for segment in expr.split(',') {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }

        // Extract +/- prefix
        let (include, body) = if let Some(rest) = segment.strip_prefix('-') {
            (false, rest.trim())
        } else if let Some(rest) = segment.strip_prefix('+') {
            (true, rest.trim())
        } else {
            (true, segment)
        };

        if body.is_empty() {
            continue;
        }

        // Try size: >N or <N
        if let Some(rule) = try_parse_size(include, body) {
            rules.push(rule);
            continue;
        }

        // Try risk: risk:high,medium,low,info
        if let Some(rule) = try_parse_risk(include, body) {
            rules.push(rule);
            continue;
        }

        // Try status keywords
        if let Some(rule) = try_parse_status(include, body) {
            rules.push(rule);
            continue;
        }

        // Otherwise treat as glob pattern
        if let Ok(pattern) = Pattern::new(body) {
            rules.push(FilterRule::Glob { include, pattern });
        }
        // Invalid globs silently skipped
    }
    rules
}

fn try_parse_risk(include: bool, body: &str) -> Option<FilterRule> {
    let rest = body.strip_prefix("risk:")?;
    let levels: Vec<RiskLevel> = rest
        .split(',')
        .filter_map(|s| match s.trim().to_lowercase().as_str() {
            "high" => Some(RiskLevel::High),
            "medium" | "med" => Some(RiskLevel::Medium),
            "low" => Some(RiskLevel::Low),
            "info" => Some(RiskLevel::Info),
            _ => None,
        })
        .collect();
    if levels.is_empty() {
        return None;
    }
    Some(FilterRule::Risk { include, levels })
}

fn try_parse_size(include: bool, body: &str) -> Option<FilterRule> {
    if let Some(num_str) = body.strip_prefix('>') {
        if let Ok(n) = num_str.trim().parse::<usize>() {
            return Some(FilterRule::Size {
                include,
                op: SizeOp::GreaterThan,
                threshold: n,
            });
        }
    }
    if let Some(num_str) = body.strip_prefix('<') {
        if let Ok(n) = num_str.trim().parse::<usize>() {
            return Some(FilterRule::Size {
                include,
                op: SizeOp::LessThan,
                threshold: n,
            });
        }
    }
    None
}

fn try_parse_status(include: bool, body: &str) -> Option<FilterRule> {
    let status = match body.to_lowercase().as_str() {
        "added" => StatusKind::Added,
        "modified" => StatusKind::Modified,
        "deleted" => StatusKind::Deleted,
        "renamed" => StatusKind::Renamed,
        _ => return None,
    };
    Some(FilterRule::Status { include, status })
}

// ── Evaluator ──

const MATCH_OPTIONS: MatchOptions = MatchOptions {
    case_sensitive: true,
    require_literal_separator: false,
    require_literal_leading_dot: false,
};

/// Apply filter rules to a file. Returns true if the file should be visible.
/// Note: Risk rules are evaluated without review data (always include). Use
/// `apply_filter_with_review` when review data is available.
#[cfg_attr(not(test), allow(dead_code))]
pub fn apply_filter(rules: &[FilterRule], file: &DiffFile) -> bool {
    if rules.is_empty() {
        return true;
    }

    let has_includes = rules.iter().any(|r| r.is_include());

    // Phase 1: Check include rules (OR logic)
    let included = if has_includes {
        rules
            .iter()
            .any(|r| r.is_include() && matches_rule(r, file))
    } else {
        // No include rules → start with all files
        true
    };

    if !included {
        return false;
    }

    // Phase 2: Check exclude rules (any match removes the file)
    let excluded = rules
        .iter()
        .any(|r| !r.is_include() && matches_rule(r, file));

    !excluded
}

/// Apply filter rules to a file with optional review data for risk filtering.
pub fn apply_filter_with_review(
    rules: &[FilterRule],
    file: &DiffFile,
    review: Option<&ErReview>,
) -> bool {
    if rules.is_empty() {
        return true;
    }

    let has_includes = rules.iter().any(|r| r.is_include());

    let included = if has_includes {
        rules
            .iter()
            .any(|r| r.is_include() && matches_rule_with_review(r, file, review))
    } else {
        true
    };

    if !included {
        return false;
    }

    let excluded = rules
        .iter()
        .any(|r| !r.is_include() && matches_rule_with_review(r, file, review));

    !excluded
}

fn matches_rule_with_review(rule: &FilterRule, file: &DiffFile, review: Option<&ErReview>) -> bool {
    match rule {
        FilterRule::Risk { levels, .. } => {
            if let Some(review) = review {
                if let Some(fr) = review.files.get(&file.path) {
                    return levels.contains(&fr.risk);
                }
            }
            false
        }
        _ => matches_rule(rule, file),
    }
}

fn matches_rule(rule: &FilterRule, file: &DiffFile) -> bool {
    match rule {
        FilterRule::Glob { pattern, .. } => pattern.matches_with(&file.path, MATCH_OPTIONS),
        FilterRule::Status { status, .. } => matches_status(*status, &file.status),
        FilterRule::Size { op, threshold, .. } => {
            let changed = file.adds + file.dels;
            match op {
                SizeOp::GreaterThan => changed > *threshold,
                SizeOp::LessThan => changed < *threshold,
            }
        }
        FilterRule::Risk { levels, .. } => {
            // Without review data, risk rules can't be evaluated — include the file
            let _ = levels;
            true
        }
    }
}

fn matches_status(kind: StatusKind, file_status: &FileStatus) -> bool {
    matches!(
        (kind, file_status),
        (StatusKind::Added, FileStatus::Added)
            | (StatusKind::Modified, FileStatus::Modified)
            | (StatusKind::Deleted, FileStatus::Deleted)
            | (StatusKind::Renamed, FileStatus::Renamed(_))
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::DiffFile;

    fn make_file(path: &str, status: FileStatus, adds: usize, dels: usize) -> DiffFile {
        DiffFile {
            path: path.to_string(),
            status,
            hunks: Vec::new(),
            adds,
            dels,
            compacted: false,
            raw_hunk_count: 0,
        }
    }

    // ── Parser tests ──

    #[test]
    fn parse_empty_string_returns_empty() {
        assert!(parse_filter_expr("").is_empty());
    }

    #[test]
    fn parse_whitespace_only_returns_empty() {
        assert!(parse_filter_expr("  ,  , ").is_empty());
    }

    #[test]
    fn parse_simple_glob_include() {
        let rules = parse_filter_expr("*.rs");
        assert_eq!(rules.len(), 1);
        assert!(matches!(&rules[0], FilterRule::Glob { include: true, .. }));
    }

    #[test]
    fn parse_explicit_include_glob() {
        let rules = parse_filter_expr("+*.ts");
        assert_eq!(rules.len(), 1);
        assert!(matches!(&rules[0], FilterRule::Glob { include: true, .. }));
    }

    #[test]
    fn parse_exclude_glob() {
        let rules = parse_filter_expr("-*.lock");
        assert_eq!(rules.len(), 1);
        assert!(matches!(&rules[0], FilterRule::Glob { include: false, .. }));
    }

    #[test]
    fn parse_status_added() {
        let rules = parse_filter_expr("+added");
        assert_eq!(rules.len(), 1);
        assert!(matches!(
            &rules[0],
            FilterRule::Status {
                include: true,
                status: StatusKind::Added
            }
        ));
    }

    #[test]
    fn parse_status_case_insensitive() {
        let rules = parse_filter_expr("+MODIFIED");
        assert_eq!(rules.len(), 1);
        assert!(matches!(
            &rules[0],
            FilterRule::Status {
                include: true,
                status: StatusKind::Modified
            }
        ));
    }

    #[test]
    fn parse_exclude_status() {
        let rules = parse_filter_expr("-deleted");
        assert_eq!(rules.len(), 1);
        assert!(matches!(
            &rules[0],
            FilterRule::Status {
                include: false,
                status: StatusKind::Deleted
            }
        ));
    }

    #[test]
    fn parse_status_renamed() {
        let rules = parse_filter_expr("+renamed");
        assert_eq!(rules.len(), 1);
        assert!(matches!(
            &rules[0],
            FilterRule::Status {
                include: true,
                status: StatusKind::Renamed
            }
        ));
    }

    #[test]
    fn parse_size_greater_than() {
        let rules = parse_filter_expr("+>10");
        assert_eq!(rules.len(), 1);
        assert!(matches!(
            &rules[0],
            FilterRule::Size {
                include: true,
                op: SizeOp::GreaterThan,
                threshold: 10
            }
        ));
    }

    #[test]
    fn parse_size_less_than_exclude() {
        let rules = parse_filter_expr("-<3");
        assert_eq!(rules.len(), 1);
        assert!(matches!(
            &rules[0],
            FilterRule::Size {
                include: false,
                op: SizeOp::LessThan,
                threshold: 3
            }
        ));
    }

    #[test]
    fn parse_mixed_rules() {
        let rules = parse_filter_expr("+*.ts, -*.lock, +>10, +added");
        assert_eq!(rules.len(), 4);
        assert!(matches!(&rules[0], FilterRule::Glob { include: true, .. }));
        assert!(matches!(&rules[1], FilterRule::Glob { include: false, .. }));
        assert!(matches!(&rules[2], FilterRule::Size { include: true, .. }));
        assert!(matches!(
            &rules[3],
            FilterRule::Status { include: true, .. }
        ));
    }

    #[test]
    fn parse_invalid_glob_silently_skipped() {
        // '[' without closing ']' is invalid
        let rules = parse_filter_expr("[invalid, *.rs");
        // The invalid glob is skipped, *.rs is parsed
        assert_eq!(rules.len(), 1);
        assert!(matches!(&rules[0], FilterRule::Glob { include: true, .. }));
    }

    #[test]
    fn parse_whitespace_around_segments() {
        let rules = parse_filter_expr("  +*.rs  ,  -*.lock  ");
        assert_eq!(rules.len(), 2);
    }

    #[test]
    fn parse_size_with_spaces() {
        let rules = parse_filter_expr("+> 10");
        assert_eq!(rules.len(), 1);
        assert!(matches!(
            &rules[0],
            FilterRule::Size {
                include: true,
                op: SizeOp::GreaterThan,
                threshold: 10
            }
        ));
    }

    #[test]
    fn parse_bare_plus_minus_skipped() {
        let rules = parse_filter_expr("+, -");
        assert!(rules.is_empty());
    }

    // ── Evaluator tests ──

    #[test]
    fn no_rules_includes_everything() {
        let file = make_file("src/main.rs", FileStatus::Modified, 5, 3);
        assert!(apply_filter(&[], &file));
    }

    #[test]
    fn include_glob_matches() {
        let rules = parse_filter_expr("*.rs");
        let file = make_file("src/main.rs", FileStatus::Modified, 5, 3);
        assert!(apply_filter(&rules, &file));
    }

    #[test]
    fn include_glob_no_match() {
        let rules = parse_filter_expr("*.ts");
        let file = make_file("src/main.rs", FileStatus::Modified, 5, 3);
        assert!(!apply_filter(&rules, &file));
    }

    #[test]
    fn exclude_glob_removes_match() {
        let rules = parse_filter_expr("-*.lock");
        let file = make_file("package-lock.json", FileStatus::Modified, 100, 50);
        // No include rules → starts with all, but *.lock doesn't match .json
        assert!(apply_filter(&rules, &file));

        let lock_file = make_file("Cargo.lock", FileStatus::Modified, 100, 50);
        assert!(!apply_filter(&rules, &lock_file));
    }

    #[test]
    fn include_then_exclude_compose() {
        let rules = parse_filter_expr("+*.rs, -src/test*");
        let src = make_file("src/main.rs", FileStatus::Modified, 5, 3);
        assert!(apply_filter(&rules, &src));

        let test = make_file("src/test_utils.rs", FileStatus::Modified, 5, 3);
        assert!(!apply_filter(&rules, &test));
    }

    #[test]
    fn multiple_includes_are_or() {
        let rules = parse_filter_expr("+*.rs, +*.toml");
        let rs = make_file("src/main.rs", FileStatus::Modified, 5, 3);
        let toml = make_file("Cargo.toml", FileStatus::Modified, 1, 0);
        let ts = make_file("src/app.ts", FileStatus::Modified, 5, 3);
        assert!(apply_filter(&rules, &rs));
        assert!(apply_filter(&rules, &toml));
        assert!(!apply_filter(&rules, &ts));
    }

    #[test]
    fn status_include_filters() {
        let rules = parse_filter_expr("+added");
        let added = make_file("new.rs", FileStatus::Added, 10, 0);
        let modified = make_file("old.rs", FileStatus::Modified, 5, 3);
        assert!(apply_filter(&rules, &added));
        assert!(!apply_filter(&rules, &modified));
    }

    #[test]
    fn status_exclude_filters() {
        let rules = parse_filter_expr("-deleted");
        let deleted = make_file("gone.rs", FileStatus::Deleted, 0, 10);
        let modified = make_file("old.rs", FileStatus::Modified, 5, 3);
        assert!(!apply_filter(&rules, &deleted));
        assert!(apply_filter(&rules, &modified));
    }

    #[test]
    fn status_renamed_matches_renamed_variant() {
        let rules = parse_filter_expr("+renamed");
        let renamed = make_file(
            "new_name.rs",
            FileStatus::Renamed("old_name.rs".to_string()),
            2,
            1,
        );
        let modified = make_file("other.rs", FileStatus::Modified, 1, 0);
        assert!(apply_filter(&rules, &renamed));
        assert!(!apply_filter(&rules, &modified));
    }

    #[test]
    fn size_greater_than_filters() {
        let rules = parse_filter_expr("+>10");
        let big = make_file("big.rs", FileStatus::Modified, 8, 5); // 13 changes
        let small = make_file("small.rs", FileStatus::Modified, 3, 2); // 5 changes
        assert!(apply_filter(&rules, &big));
        assert!(!apply_filter(&rules, &small));
    }

    #[test]
    fn size_less_than_exclude() {
        let rules = parse_filter_expr("-<3");
        let tiny = make_file("tiny.rs", FileStatus::Modified, 1, 0); // 1 change
        let normal = make_file("normal.rs", FileStatus::Modified, 5, 3); // 8 changes
        assert!(!apply_filter(&rules, &tiny));
        assert!(apply_filter(&rules, &normal));
    }

    #[test]
    fn glob_matches_at_any_depth() {
        // With require_literal_separator: false, *.rs matches nested paths
        let rules = parse_filter_expr("*.rs");
        let nested = make_file("src/deeply/nested/file.rs", FileStatus::Modified, 1, 0);
        assert!(apply_filter(&rules, &nested));
    }

    #[test]
    fn exclude_only_starts_with_all() {
        // No include rules → all files included, excludes remove
        let rules = parse_filter_expr("-*.lock, -*.json");
        let rs = make_file("src/main.rs", FileStatus::Modified, 5, 3);
        let lock = make_file("Cargo.lock", FileStatus::Modified, 100, 50);
        let json = make_file("package.json", FileStatus::Modified, 2, 1);
        assert!(apply_filter(&rules, &rs));
        assert!(!apply_filter(&rules, &lock));
        assert!(!apply_filter(&rules, &json));
    }

    #[test]
    fn mixed_glob_and_status() {
        let rules = parse_filter_expr("+*.rs, +added");
        let added_ts = make_file("new.ts", FileStatus::Added, 10, 0);
        let modified_rs = make_file("src/main.rs", FileStatus::Modified, 5, 3);
        let modified_ts = make_file("src/app.ts", FileStatus::Modified, 5, 3);
        // added_ts: matches +added → included
        assert!(apply_filter(&rules, &added_ts));
        // modified_rs: matches +*.rs → included
        assert!(apply_filter(&rules, &modified_rs));
        // modified_ts: matches neither → excluded
        assert!(!apply_filter(&rules, &modified_ts));
    }

    #[test]
    fn size_boundary_exactly_at_threshold() {
        let rules = parse_filter_expr("+>10");
        let exactly_10 = make_file("exact.rs", FileStatus::Modified, 5, 5); // 10 changes
                                                                            // > 10 means strictly greater, 10 does NOT pass
        assert!(!apply_filter(&rules, &exactly_10));
    }

    // ── Risk filter tests ──

    #[test]
    fn test_parse_risk_filter() {
        // risk: accepts multiple levels within the same token via "risk:high medium"
        // but the top-level parser splits on ',', so each risk level is a separate token.
        // "+risk:high" produces one Risk rule with [High].
        let rules = parse_filter_expr("+risk:high");
        assert_eq!(rules.len(), 1);
        match &rules[0] {
            FilterRule::Risk { include, levels } => {
                assert!(*include);
                assert_eq!(levels.len(), 1);
                assert!(levels.contains(&RiskLevel::High));
            }
            _ => panic!("expected Risk rule"),
        }
    }

    #[test]
    fn test_parse_risk_filter_exclude() {
        let rules = parse_filter_expr("-risk:info");
        assert_eq!(rules.len(), 1);
        match &rules[0] {
            FilterRule::Risk { include, levels } => {
                assert!(!*include);
                assert_eq!(levels.len(), 1);
                assert!(levels.contains(&RiskLevel::Info));
            }
            _ => panic!("expected Risk rule"),
        }
    }

    #[test]
    fn test_parse_risk_filter_single() {
        let rules = parse_filter_expr("+risk:high");
        assert_eq!(rules.len(), 1);
        match &rules[0] {
            FilterRule::Risk { include, levels } => {
                assert!(*include);
                assert_eq!(levels.len(), 1);
                assert!(levels.contains(&RiskLevel::High));
            }
            _ => panic!("expected Risk rule"),
        }
    }

    #[test]
    fn test_risk_filter_preset() {
        let review_preset = FILTER_PRESETS.iter().find(|p| p.name == "review");
        assert!(review_preset.is_some(), "review preset should exist");
        let preset = review_preset.unwrap();
        let rules = parse_filter_expr(preset.expr);
        assert_eq!(rules.len(), 1);
        assert!(matches!(&rules[0], FilterRule::Risk { include: false, .. }));
    }

    #[test]
    fn test_parse_risk_med_alias() {
        // "med" should be an alias for Medium
        let rules = parse_filter_expr("+risk:med");
        assert_eq!(rules.len(), 1);
        match &rules[0] {
            FilterRule::Risk { levels, .. } => {
                assert!(levels.contains(&RiskLevel::Medium));
            }
            _ => panic!("expected Risk rule"),
        }
    }

    #[test]
    fn test_parse_risk_all_levels() {
        // Each level is a separate token in a comma-separated expression.
        // Four separate risk:* tokens produce four Risk rules.
        let _rules = parse_filter_expr("+risk:high +risk:medium +risk:low +risk:info");
        // top-level splits on space too? No — only on comma. So this is one segment.
        // Use four separate segments with commas:
        let rules = parse_filter_expr("+risk:high, +risk:medium, +risk:low, +risk:info");
        assert_eq!(rules.len(), 4);
        for rule in &rules {
            assert!(matches!(rule, FilterRule::Risk { include: true, .. }));
        }
    }

    #[test]
    fn test_parse_risk_unknown_level_skipped() {
        // "risk:critical" strips the prefix to "critical" but that's not a known level →
        // try_parse_risk returns None (empty levels), falls through to glob pattern "risk:critical"
        // which is a valid (if unusual) glob. The rule count will be 1 as a Glob, not Risk.
        let rules = parse_filter_expr("+risk:critical");
        // The important check: no Risk rule is produced
        for rule in &rules {
            assert!(
                !matches!(rule, FilterRule::Risk { .. }),
                "should not produce a Risk rule for unknown level"
            );
        }
    }
}
