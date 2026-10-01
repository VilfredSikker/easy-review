use crate::ai::{ErReview, RiskLevel};
use crate::config::{FileKindRepoConfig, ImportanceRepoConfig, ImportanceTier};
use crate::git::{classify_path, DiffFile, FileKind, FileStatus};
use glob::{MatchOptions, Pattern};

// ── Types ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusKind {
    Added,
    Modified,
    Deleted,
    Renamed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    /// Case-insensitive substring match against the full path. Plain segments
    /// without glob metacharacters parse to this, so typing `Regi` matches
    /// `src/registry.ts` without needing `*Regi*`.
    Substring {
        include: bool,
        /// Stored lowercased; matched against the lowercased path.
        needle: String,
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
    /// Declared tiers from the global config, so unlike `Risk` this ranks a
    /// diff no review has run against.
    Importance {
        include: bool,
        tiers: Vec<ImportanceTier>,
    },
    /// File kind (`kind:production` is the header's code count), after the
    /// repo's `[file_kinds]` overrides.
    Kind {
        include: bool,
        kinds: Vec<FileKind>,
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
    FilterPreset {
        name: "schema",
        expr: "*.sql,*.prisma,*.graphql,migration",
    },
    FilterPreset {
        name: "api",
        expr: "*.proto,openapi,api/,routes/,handlers/,controllers/",
    },
];

impl FilterRule {
    const fn is_include(&self) -> bool {
        match self {
            Self::Glob { include, .. } => *include,
            Self::Substring { include, .. } => *include,
            Self::Status { include, .. } => *include,
            Self::Size { include, .. } => *include,
            Self::Risk { include, .. } => *include,
            Self::Importance { include, .. } => *include,
            Self::Kind { include, .. } => *include,
        }
    }
}

// ── Parser ──

/// Parse a comma-separated filter expression into a list of rules.
/// Invalid globs are silently skipped.
///
/// The comma separates rules and also the values of a `risk:`, `importance:`
/// or `kind:` list, so `-kind:test,docs` excludes both kinds. A bare segment
/// right after such a rule joins it when it reads as one of its values;
/// anything else starts a new rule with its own sign. Prefixes match in any
/// case.
pub fn parse_filter_expr(expr: &str) -> Vec<FilterRule> {
    const LIST_PREFIXES: [&str; 3] = ["risk:", "importance:", "kind:"];

    /// The value list a bare segment may still join: its prefix and sign, and
    /// the rule it built — `None` while every value so far failed to read, so
    /// `kind:bogus,test` still selects tests.
    struct OpenList {
        prefix: &'static str,
        include: bool,
        rule: Option<usize>,
    }

    let mut rules: Vec<FilterRule> = Vec::new();
    let mut open: Option<OpenList> = None;
    for segment in expr.split(',') {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }

        // Extract +/- prefix
        let (include, body, signed) = if let Some(rest) = segment.strip_prefix('-') {
            (false, rest.trim(), true)
        } else if let Some(rest) = segment.strip_prefix('+') {
            (true, rest.trim(), true)
        } else {
            (true, segment, false)
        };

        if body.is_empty() {
            continue;
        }

        if let Some(list) = open.as_mut().filter(|_| !signed) {
            let joined = match list.rule {
                Some(idx) => extend_value_list(&mut rules[idx], body),
                None => match parse_list_rule(list.include, &format!("{}{body}", list.prefix)) {
                    Some(rule) => {
                        rules.push(rule);
                        list.rule = Some(rules.len() - 1);
                        true
                    }
                    None => false,
                },
            };
            if joined {
                continue;
            }
        }
        open = None;

        // A list prefix with no value that reads stays out of the rule set. As
        // a substring it would match no path and empty the list without saying
        // why.
        let lower = body.to_ascii_lowercase();
        if let Some(prefix) = LIST_PREFIXES.iter().find(|p| lower.starts_with(**p)) {
            let rule = parse_list_rule(include, &lower).map(|rule| {
                rules.push(rule);
                rules.len() - 1
            });
            open = Some(OpenList {
                prefix,
                include,
                rule,
            });
            continue;
        }

        // Try size: >N or <N
        if let Some(rule) = try_parse_size(include, body) {
            rules.push(rule);
            continue;
        }

        // Try status keywords
        if let Some(rule) = try_parse_status(include, body) {
            rules.push(rule);
            continue;
        }

        // Plain text without glob metacharacters → case-insensitive substring
        // match on the full path. (Status/size/risk keywords already parsed
        // above keep their existing semantics.)
        if !body.contains(['*', '?', '[']) {
            rules.push(FilterRule::Substring {
                include,
                needle: body.to_lowercase(),
            });
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

fn risk_value(value: &str) -> Option<RiskLevel> {
    match value.trim().to_lowercase().as_str() {
        "high" => Some(RiskLevel::High),
        "medium" | "med" => Some(RiskLevel::Medium),
        "low" => Some(RiskLevel::Low),
        "info" => Some(RiskLevel::Info),
        _ => None,
    }
}

fn kind_value(value: &str) -> Option<FileKind> {
    match value.trim().to_ascii_lowercase().as_str() {
        // The header labels production lines "code".
        "code" => Some(FileKind::Production),
        "tests" => Some(FileKind::Test),
        "doc" => Some(FileKind::Docs),
        "story" | "stories" => Some(FileKind::Storybook),
        other => FileKind::parse(other),
    }
}

fn try_parse_risk(include: bool, body: &str) -> Option<FilterRule> {
    let level = risk_value(body.strip_prefix("risk:")?)?;
    Some(FilterRule::Risk {
        include,
        levels: vec![level],
    })
}

fn try_parse_importance(include: bool, body: &str) -> Option<FilterRule> {
    let tier = ImportanceTier::parse(body.strip_prefix("importance:")?)?;
    Some(FilterRule::Importance {
        include,
        tiers: vec![tier],
    })
}

fn try_parse_kind(include: bool, body: &str) -> Option<FilterRule> {
    let kind = kind_value(body.strip_prefix("kind:")?)?;
    Some(FilterRule::Kind {
        include,
        kinds: vec![kind],
    })
}

/// A `risk:` / `importance:` / `kind:` segment with one value, or `None` when
/// the value does not read.
fn parse_list_rule(include: bool, body: &str) -> Option<FilterRule> {
    try_parse_risk(include, body)
        .or_else(|| try_parse_importance(include, body))
        .or_else(|| try_parse_kind(include, body))
}

/// Add `value` to a value-list rule when it reads as one of that rule's
/// values. False leaves the rule untouched and the segment to parse on its own.
fn extend_value_list(rule: &mut FilterRule, value: &str) -> bool {
    match rule {
        FilterRule::Risk { levels, .. } => risk_value(value).map(|l| levels.push(l)).is_some(),
        FilterRule::Importance { tiers, .. } => ImportanceTier::parse(value)
            .map(|t| tiers.push(t))
            .is_some(),
        FilterRule::Kind { kinds, .. } => kind_value(value).map(|k| kinds.push(k)).is_some(),
        _ => false,
    }
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
///
/// Note: risk and importance rules are evaluated without review data (always
/// include). Use `apply_filter_with_context` when any rule data is available.
pub fn apply_filter(rules: &[FilterRule], file: &DiffFile) -> bool {
    apply_filter_with_context(rules, file, None, None)
}

/// Apply filter rules to a file with the rule data that does not come from the
/// diff itself.
///
/// `importance` is the active repo's declared rule table. Absent, or absent for
/// this repo, leaves every path at [`ImportanceTier::Normal`] — the answer the
/// config's own default gives — so `importance:normal` selects the whole diff
/// and any other tier selects none of it.
pub fn apply_filter_with_context(
    rules: &[FilterRule],
    file: &DiffFile,
    review: Option<&ErReview>,
    importance: Option<&ImportanceRepoConfig>,
) -> bool {
    apply_filter_with_kinds(rules, file, review, importance, None)
}

/// [`apply_filter_with_context`] plus the repo's `[file_kinds]` overrides, so
/// `kind:` agrees with the header's code count. Without them `kind:` falls
/// back to the built-in conventions.
pub fn apply_filter_with_kinds(
    rules: &[FilterRule],
    file: &DiffFile,
    review: Option<&ErReview>,
    importance: Option<&ImportanceRepoConfig>,
    file_kinds: Option<&FileKindRepoConfig>,
) -> bool {
    if rules.is_empty() {
        return true;
    }

    let has_includes = rules.iter().any(|r| r.is_include());

    // Phase 1: Check include rules (OR logic)
    let included = if has_includes {
        rules.iter().any(|r| {
            r.is_include() && matches_rule_with_context(r, file, review, importance, file_kinds)
        })
    } else {
        // No include rules → start with all files
        true
    };

    if !included {
        return false;
    }

    // Phase 2: Check exclude rules (any match removes the file)
    let excluded = rules.iter().any(|r| {
        !r.is_include() && matches_rule_with_context(r, file, review, importance, file_kinds)
    });

    !excluded
}

fn matches_rule_with_context(
    rule: &FilterRule,
    file: &DiffFile,
    review: Option<&ErReview>,
    importance: Option<&ImportanceRepoConfig>,
    file_kinds: Option<&FileKindRepoConfig>,
) -> bool {
    match rule {
        FilterRule::Risk { levels, .. } => {
            if let Some(review) = review {
                if let Some(fr) = review.files.get(&file.path) {
                    return levels.contains(&fr.risk);
                }
            }
            false
        }
        FilterRule::Importance { tiers, .. } => {
            let tier = importance.map_or(ImportanceTier::Normal, |rules| rules.resolve(&file.path));
            tiers.contains(&tier)
        }
        FilterRule::Kind { kinds, .. } => {
            let kind =
                file_kinds.map_or_else(|| classify_path(&file.path), |k| k.classify(&file.path));
            kinds.contains(&kind)
        }
        _ => matches_rule(rule, file),
    }
}

fn matches_rule(rule: &FilterRule, file: &DiffFile) -> bool {
    match rule {
        FilterRule::Glob { pattern, .. } => pattern.matches_with(&file.path, MATCH_OPTIONS),
        // Full-path substring subsumes a basename check — the basename is
        // itself a substring of the path.
        FilterRule::Substring { needle, .. } => file.path.to_lowercase().contains(needle),
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
        // Nothing to resolve a tier against here, which is the case a repo with
        // no declared rules is in anyway: every path reads as `Normal`.
        FilterRule::Importance { tiers, .. } => tiers.contains(&ImportanceTier::Normal),
        FilterRule::Kind { kinds, .. } => kinds.contains(&classify_path(&file.path)),
    }
}

const fn matches_status(kind: StatusKind, file_status: &FileStatus) -> bool {
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

    // ── Substring filter tests ──

    #[test]
    fn parse_plain_text_becomes_substring() {
        let rules = parse_filter_expr("Regi");
        assert_eq!(rules.len(), 1);
        match &rules[0] {
            FilterRule::Substring { include, needle } => {
                assert!(*include);
                assert_eq!(needle, "regi"); // stored lowercased
            }
            _ => panic!("expected Substring rule"),
        }
    }

    #[test]
    fn substring_matches_case_insensitive_prefix_of_basename() {
        let rules = parse_filter_expr("Regi");
        let registry = make_file("src/registry.ts", FileStatus::Modified, 5, 3);
        let registry_test = make_file("src/registry.test.ts", FileStatus::Modified, 5, 3);
        let other = make_file("src/main.rs", FileStatus::Modified, 1, 0);
        assert!(apply_filter(&rules, &registry));
        assert!(apply_filter(&rules, &registry_test));
        assert!(!apply_filter(&rules, &other));
    }

    #[test]
    fn substring_matches_uppercase_component_files() {
        let rules = parse_filter_expr("Exp");
        let qc_wells = make_file("src/ExperimentQcWells.ts", FileStatus::Modified, 5, 3);
        let list = make_file("src/ExperimentList.svelte", FileStatus::Modified, 5, 3);
        let other = make_file("src/registry.ts", FileStatus::Modified, 1, 0);
        assert!(apply_filter(&rules, &qc_wells));
        assert!(apply_filter(&rules, &list));
        assert!(!apply_filter(&rules, &other));
    }

    #[test]
    fn substring_matches_mid_basename() {
        let rules = parse_filter_expr("QcWells");
        let qc_wells = make_file("src/ExperimentQcWells.ts", FileStatus::Modified, 5, 3);
        let qc_wells_view = make_file(
            "src/ExperimentQcWellsView.svelte",
            FileStatus::Modified,
            5,
            3,
        );
        let other = make_file("src/ExperimentList.svelte", FileStatus::Modified, 1, 0);
        assert!(apply_filter(&rules, &qc_wells));
        assert!(apply_filter(&rules, &qc_wells_view));
        assert!(!apply_filter(&rules, &other));
    }

    #[test]
    fn substring_matches_exact_basename() {
        let rules = parse_filter_expr("registry.ts");
        let registry = make_file("src/registry.ts", FileStatus::Modified, 5, 3);
        assert!(apply_filter(&rules, &registry));
    }

    #[test]
    fn glob_with_metachars_keeps_glob_semantics() {
        // `*.ts` stays a glob: it matches `a.ts` but NOT `a.tsx` — a substring
        // rule would match both, so this proves the glob path is preserved.
        let rules = parse_filter_expr("*.ts");
        assert!(matches!(&rules[0], FilterRule::Glob { .. }));
        let ts = make_file("src/a.ts", FileStatus::Modified, 1, 0);
        let tsx = make_file("src/a.tsx", FileStatus::Modified, 1, 0);
        assert!(apply_filter(&rules, &ts));
        assert!(!apply_filter(&rules, &tsx));
    }

    #[test]
    fn substring_exclude_filters() {
        let rules = parse_filter_expr("-lock");
        let lock = make_file("Cargo.lock", FileStatus::Modified, 100, 50);
        let rs = make_file("src/main.rs", FileStatus::Modified, 5, 3);
        assert!(!apply_filter(&rules, &lock));
        assert!(apply_filter(&rules, &rs));
    }

    #[test]
    fn substring_composes_with_other_rules() {
        let rules = parse_filter_expr("+Exp, -*.svelte");
        let ts = make_file("src/ExperimentQcWells.ts", FileStatus::Modified, 5, 3);
        let svelte = make_file("src/ExperimentList.svelte", FileStatus::Modified, 5, 3);
        let other = make_file("src/registry.ts", FileStatus::Modified, 1, 0);
        assert!(apply_filter(&rules, &ts));
        assert!(!apply_filter(&rules, &svelte)); // included by Exp, excluded by glob
        assert!(!apply_filter(&rules, &other)); // no include rule matches
    }

    #[test]
    fn substring_matches_directory_component() {
        // Full-path matching: a needle can hit a directory name too.
        let rules = parse_filter_expr("experiments/");
        let nested = make_file(
            "src/experiments/quality-control/wells.ts",
            FileStatus::Modified,
            1,
            0,
        );
        let other = make_file("src/registry.ts", FileStatus::Modified, 1, 0);
        assert!(apply_filter(&rules, &nested));
        assert!(!apply_filter(&rules, &other));
    }

    #[test]
    fn test_parse_risk_unknown_level_skipped() {
        // "critical" is not a level. The segment is dropped: as a substring
        // "risk:critical" would match no path and empty the list.
        let rules = parse_filter_expr("+risk:critical");
        assert!(rules.is_empty(), "{rules:?}");
    }

    // ── Importance filter tests ──

    use crate::config::test_support::importance_rules;

    fn review_with_risk(path: &str, risk: RiskLevel) -> ErReview {
        use std::collections::HashMap;

        let mut review = ErReview {
            version: 1,
            diff_hash: "hash".to_string(),
            created_at: String::new(),
            base_branch: "main".to_string(),
            head_branch: "feature".to_string(),
            files: HashMap::new(),
            file_hashes: HashMap::new(),
        };
        review.files.insert(
            path.to_string(),
            crate::ai::ErFileReview {
                risk,
                risk_reason: String::new(),
                summary: String::new(),
                findings: Vec::new(),
            },
        );
        review
    }

    #[test]
    fn parse_importance_filter() {
        let rules = parse_filter_expr("+importance:foundational");
        assert_eq!(rules.len(), 1);
        match &rules[0] {
            FilterRule::Importance { include, tiers } => {
                assert!(*include);
                assert_eq!(tiers, &[ImportanceTier::Foundational]);
            }
            _ => panic!("expected Importance rule"),
        }
    }

    #[test]
    fn parse_importance_filter_exclude() {
        let rules = parse_filter_expr("-importance:isolated");
        assert_eq!(rules.len(), 1);
        match &rules[0] {
            FilterRule::Importance { include, tiers } => {
                assert!(!*include);
                assert_eq!(tiers, &[ImportanceTier::Isolated]);
            }
            _ => panic!("expected Importance rule"),
        }
    }

    #[test]
    fn parse_importance_unknown_tier_produces_no_rule() {
        // Parallel to the risk case: "importance:critical" is not a tier, so the
        // segment is dropped rather than becoming a substring nothing matches.
        let rules = parse_filter_expr("+importance:critical");
        assert!(rules.is_empty(), "{rules:?}");
    }

    #[test]
    fn importance_filters_without_review_data() {
        // The case `risk:*` cannot serve: nothing has been reviewed, so every
        // call passes no review, and the declared tier still decides.
        let rules = parse_filter_expr("+importance:foundational");
        let importance = importance_rules(&[("crates/er-engine/src/**", "foundational")], "normal");
        let core = make_file(
            "crates/er-engine/src/app/filter.rs",
            FileStatus::Modified,
            5,
            3,
        );
        let other = make_file("crates/er-tui/src/main.rs", FileStatus::Modified, 5, 3);
        assert!(apply_filter_with_context(
            &rules,
            &core,
            None,
            Some(&importance)
        ));
        assert!(!apply_filter_with_context(
            &rules,
            &other,
            None,
            Some(&importance)
        ));
    }

    #[test]
    fn importance_exclude_hides_only_the_tier_it_names() {
        let rules = parse_filter_expr("-importance:isolated");
        let importance = importance_rules(&[("*.md", "isolated")], "normal");
        let doc = make_file("docs/readme.md", FileStatus::Modified, 2, 0);
        let code = make_file("src/main.rs", FileStatus::Modified, 5, 3);
        assert!(!apply_filter_with_context(
            &rules,
            &doc,
            None,
            Some(&importance)
        ));
        assert!(apply_filter_with_context(
            &rules,
            &code,
            None,
            Some(&importance)
        ));
    }

    #[test]
    fn importance_is_not_gated_on_review_data() {
        // A review exists here, which is what a risk rule needs and an
        // importance rule must not: the tier answers the same either way.
        let rules = parse_filter_expr("+importance:foundational");
        let importance = importance_rules(&[("crates/er-engine/src/**", "foundational")], "normal");
        let core = make_file(
            "crates/er-engine/src/app/filter.rs",
            FileStatus::Modified,
            5,
            3,
        );
        let review = review_with_risk(&core.path, RiskLevel::Low);
        assert!(apply_filter_with_context(
            &rules,
            &core,
            Some(&review),
            Some(&importance)
        ));
    }

    #[test]
    fn importance_without_declared_rules_resolves_every_path_to_normal() {
        // With no generated rules, every path resolves through the config's own
        // default, so only a rule naming `normal` matches anything.
        let foundational = parse_filter_expr("+importance:foundational");
        let normal = parse_filter_expr("+importance:normal");
        let file = make_file("src/main.rs", FileStatus::Modified, 5, 3);
        assert!(!apply_filter_with_context(&foundational, &file, None, None));
        assert!(apply_filter_with_context(&normal, &file, None, None));
    }

    #[test]
    fn schema_and_api_presets_select_their_files() {
        let schema = FILTER_PRESETS.iter().find(|p| p.name == "schema").unwrap();
        let schema_rules = parse_filter_expr(schema.expr);
        let migration = make_file("db/migrations/0001_init.sql", FileStatus::Modified, 5, 3);
        let graphic = make_file("schema/schema.graphql", FileStatus::Modified, 5, 3);
        let component = make_file(
            "desktop-ui/src/lib/components/FileTree.svelte",
            FileStatus::Modified,
            5,
            3,
        );
        assert!(apply_filter(&schema_rules, &migration));
        assert!(apply_filter(&schema_rules, &graphic));
        assert!(!apply_filter(&schema_rules, &component));

        let api = FILTER_PRESETS.iter().find(|p| p.name == "api").unwrap();
        let api_rules = parse_filter_expr(api.expr);
        let route = make_file(
            "desktop-ui/src/lib/api/reviews.ts",
            FileStatus::Modified,
            5,
            3,
        );
        let proto = make_file("proto/review.proto", FileStatus::Modified, 5, 3);
        let docs = make_file(
            "docs/adr/0036-importance-as-declared-config.md",
            FileStatus::Modified,
            1,
            0,
        );
        assert!(apply_filter(&api_rules, &route));
        assert!(apply_filter(&api_rules, &proto));
        assert!(!apply_filter(&api_rules, &docs));
    }

    // ── Kind filter tests ──

    #[test]
    fn kind_code_is_an_alias_for_production() {
        let rules = parse_filter_expr("kind:code");
        assert!(matches!(
            rules.as_slice(),
            [FilterRule::Kind { include: true, kinds }] if kinds == &[FileKind::Production]
        ));
        assert!(matches!(
            parse_filter_expr("-kind:test").as_slice(),
            [FilterRule::Kind { include: false, kinds }] if kinds == &[FileKind::Test]
        ));
    }

    /// An unknown kind must not fall through to a substring rule: "kind:sources"
    /// matches no path, so the list would go empty without saying why.
    #[test]
    fn unknown_kind_parses_to_no_rule() {
        let rules = parse_filter_expr("kind:sources");
        assert!(rules.is_empty(), "{rules:?}");
        let file = make_file("src/lib.rs", FileStatus::Modified, 1, 0);
        assert!(apply_filter(&rules, &file));
    }

    #[test]
    fn kind_accepts_plural_and_short_spellings() {
        assert!(matches!(
            parse_filter_expr("kind:tests").as_slice(),
            [FilterRule::Kind { kinds, .. }] if kinds == &[FileKind::Test]
        ));
        assert!(matches!(
            parse_filter_expr("kind:stories").as_slice(),
            [FilterRule::Kind { kinds, .. }] if kinds == &[FileKind::Storybook]
        ));
    }

    /// `-kind:test,docs` excludes both kinds. Read as "exclude tests" plus an
    /// include substring `docs`, it would hide every code file.
    #[test]
    fn comma_list_values_join_the_rule_before_them() {
        let rules = parse_filter_expr("-kind:test,docs");
        assert!(matches!(
            rules.as_slice(),
            [FilterRule::Kind { include: false, kinds }]
                if kinds == &[FileKind::Test, FileKind::Docs]
        ));
        let src = make_file("src/lib.rs", FileStatus::Modified, 1, 0);
        let doc = make_file("README.md", FileStatus::Modified, 1, 0);
        assert!(apply_filter(&rules, &src));
        assert!(!apply_filter(&rules, &doc));

        assert!(matches!(
            parse_filter_expr("risk:high,medium").as_slice(),
            [FilterRule::Risk { levels, .. }] if levels == &[RiskLevel::High, RiskLevel::Medium]
        ));
        assert!(matches!(
            parse_filter_expr("importance:foundational,isolated").as_slice(),
            [FilterRule::Importance { tiers, .. }]
                if tiers == &[ImportanceTier::Foundational, ImportanceTier::Isolated]
        ));
    }

    /// A first value that does not read must not strand the ones after it as
    /// substrings: `kind:bogus,test` still selects tests.
    #[test]
    fn values_after_an_unknown_first_value_still_join_the_list() {
        assert!(matches!(
            parse_filter_expr("kind:bogus,test").as_slice(),
            [FilterRule::Kind { include: true, kinds }] if kinds == &[FileKind::Test]
        ));
        assert!(matches!(
            parse_filter_expr("-kind:bogus,test,docs").as_slice(),
            [FilterRule::Kind { include: false, kinds }]
                if kinds == &[FileKind::Test, FileKind::Docs]
        ));
    }

    /// `Kind:code` as a substring would match no path and empty the list.
    #[test]
    fn list_prefixes_match_in_any_case() {
        assert!(matches!(
            parse_filter_expr("Kind:Code").as_slice(),
            [FilterRule::Kind { kinds, .. }] if kinds == &[FileKind::Production]
        ));
        assert!(matches!(
            parse_filter_expr("RISK:high").as_slice(),
            [FilterRule::Risk { levels, .. }] if levels == &[RiskLevel::High]
        ));
    }

    /// After an exclude list, a word that is not one of its values is an
    /// include: `-risk:info,auth` hides info-risk files and keeps the paths
    /// containing `auth`. A new rule keeps its own sign.
    #[test]
    fn non_values_after_an_exclude_list_keep_their_meaning() {
        assert!(matches!(
            parse_filter_expr("-risk:info,auth").as_slice(),
            [
                FilterRule::Risk { include: false, .. },
                FilterRule::Substring { include: true, needle },
            ] if needle == "auth"
        ));
        assert!(matches!(
            parse_filter_expr("-kind:test,risk:high").as_slice(),
            [
                FilterRule::Kind { include: false, .. },
                FilterRule::Risk { include: true, .. },
            ]
        ));
        assert!(matches!(
            parse_filter_expr("-risk:low,src/api").as_slice(),
            [
                FilterRule::Risk { include: false, .. },
                FilterRule::Substring { include: true, needle },
            ] if needle == "src/api"
        ));
    }

    /// A bare segment that is not a value of the list before it, or a signed
    /// one, starts its own rule.
    #[test]
    fn non_values_after_a_list_start_new_rules() {
        let rules = parse_filter_expr("kind:code,src/api");
        assert!(matches!(
            rules.as_slice(),
            [FilterRule::Kind { .. }, FilterRule::Substring { needle, .. }] if needle == "src/api"
        ));
        let rules = parse_filter_expr("kind:code,-docs");
        assert!(matches!(
            rules.as_slice(),
            [FilterRule::Kind { kinds, .. }, FilterRule::Substring { include: false, .. }]
                if kinds == &[FileKind::Production]
        ));
    }

    #[test]
    fn kind_code_keeps_only_production_files() {
        let rules = parse_filter_expr("kind:code");
        let src = make_file("src/lib.rs", FileStatus::Modified, 5, 0);
        let test = make_file("src/lib.test.ts", FileStatus::Added, 5, 0);
        let lock = make_file("Cargo.lock", FileStatus::Modified, 5, 0);
        let doc = make_file("README.md", FileStatus::Modified, 5, 0);
        assert!(apply_filter(&rules, &src));
        assert!(!apply_filter(&rules, &test));
        assert!(!apply_filter(&rules, &lock));
        assert!(!apply_filter(&rules, &doc));
    }

    /// Same files the header's code count sums, so the repo's overrides apply.
    #[test]
    fn kind_filter_applies_file_kind_overrides() {
        let rules = parse_filter_expr("kind:code");
        let schema = make_file("src/api/schema.ts", FileStatus::Modified, 900, 0);
        let overrides = FileKindRepoConfig::new(std::collections::BTreeMap::from([(
            "src/api/schema.ts".to_string(),
            "generated".to_string(),
        )]));
        assert!(apply_filter(&rules, &schema));
        assert!(!apply_filter_with_kinds(
            &rules,
            &schema,
            None,
            None,
            Some(&overrides)
        ));
    }
}
