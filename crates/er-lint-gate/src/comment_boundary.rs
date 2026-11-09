//! `er::comment_boundary` — a hand-built GitHub comment intent.
//!
//! A private annotation reaches GitHub only through `ai::promote`, which is the
//! one conversion the write path accepts. The comment write types are built in
//! a few boundary files, listed in `allow`; a construction anywhere else is an
//! error. The list may only shrink, because an entry with no findings fails as
//! stale.
//!
//! Only *expressions* count. A struct definition is an item, a destructuring
//! `let NewLineComment { .. } = comment` is a pattern, and a `use` import names
//! the type without building it. Test code (`#[cfg(test)]` and `#[test]`) is
//! exempt.

use std::collections::BTreeMap;

use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{ExprStruct, Item, Path};

use crate::attrs::{is_test_item, item_attrs};
use crate::diagnostics::Diagnostic;

/// The code the gate reports these findings under.
pub const CODE: &str = "er::comment_boundary";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub line: usize,
    /// The construction as the diagnostic words it, e.g.
    /// `` `NewLineComment { .. }` ``.
    pub message: String,
}

struct CommentFinder {
    findings: Vec<Finding>,
}

impl<'ast> Visit<'ast> for CommentFinder {
    fn visit_item(&mut self, item: &'ast Item) {
        if item_attrs(item).is_some_and(is_test_item) {
            return;
        }
        visit::visit_item(self, item);
    }

    fn visit_expr_struct(&mut self, expr: &'ast ExprStruct) {
        if let Some(built) = comment_type(&expr.path) {
            self.findings.push(Finding {
                line: expr.span().start().line,
                message: format!("`{built} {{ .. }}`"),
            });
        }
        visit::visit_expr_struct(self, expr);
    }
}

/// The comment type `path` builds, when it is one of the guarded ones. The
/// last segment names the type or enum variant, so a qualified or aliased
/// module path cannot hide it.
fn comment_type(path: &Path) -> Option<String> {
    let segments: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
    let last = segments.last()?;
    let owner = segments.len().checked_sub(2).and_then(|i| segments.get(i));
    let is_new_github_comment = owner.is_some_and(|owner| owner == "NewGitHubComment");
    match last.as_str() {
        "Line" | "Reply" | "General" if is_new_github_comment => {
            Some(format!("NewGitHubComment::{last}"))
        }
        "ReviewBatchEntry" | "NewLineComment" => Some(last.clone()),
        _ => None,
    }
}

/// Findings for one source file. A file that does not parse yields none.
pub fn findings(source: &str) -> Vec<Finding> {
    let Ok(file) = syn::parse_file(source) else {
        return Vec::new();
    };
    let mut finder = CommentFinder {
        findings: Vec::new(),
    };
    finder.visit_file(&file);
    finder.findings
}

/// Turn findings into gate diagnostics. A file under `allow` is a known
/// builder; anything else fails. An allow entry with no findings fails as
/// stale, which is what makes the list shrink.
pub fn evaluate(
    findings_by_file: &BTreeMap<String, Vec<Finding>>,
    allow: &[String],
    code: &str,
) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (file, findings) in findings_by_file {
        if allow.iter().any(|entry| entry == file) {
            continue;
        }
        for finding in findings {
            let message = format!(
                "{} built outside the promotion boundary: build it through `ai::promote`",
                finding.message
            );
            out.push(error(file, finding.line, code, &message));
        }
    }
    for entry in allow {
        let found = findings_by_file
            .get(entry)
            .is_some_and(|findings| !findings.is_empty());
        if !found {
            out.push(error(
                entry,
                1,
                code,
                &format!("stale allow-list entry: remove {entry}"),
            ));
        }
    }
    out
}

fn error(file: &str, line: usize, code: &str, message: &str) -> Diagnostic {
    Diagnostic {
        level: "error".into(),
        code: Some(code.into()),
        file: Some(file.into()),
        line,
        column: 1,
        rendered: format!("error[{code}]: {message}\n  --> {file}:{line}\n\n"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LISTED: &str = "crates/er-engine/src/app/state/github_post.rs";

    fn by_file(file: &str, source: &str) -> BTreeMap<String, Vec<Finding>> {
        let mut map = BTreeMap::new();
        let found = findings(source);
        if !found.is_empty() {
            map.insert(file.to_string(), found);
        }
        map
    }

    #[test]
    fn new_github_comment_literal_in_a_listed_file_is_flagged() {
        for source in [
            "fn f() { let _ = NewGitHubComment::Line { body: String::new() }; }",
            "fn f() { let _ = NewGitHubComment::Reply { body: String::new() }; }",
            "fn f() { let _ = NewGitHubComment::General { body: String::new() }; }",
            "fn f() { let _ = er_engine::app::NewGitHubComment::Line { body: String::new() }; }",
        ] {
            assert_eq!(findings(source).len(), 1, "{source}");
        }
    }

    #[test]
    fn new_line_comment_literal_is_flagged() {
        for source in [
            "fn f() { let _ = NewLineComment { path: \"a\", body: \"b\" }; }",
            "fn f() { let _ = er_engine::github::NewLineComment { path: \"a\" }; }",
        ] {
            assert_eq!(findings(source).len(), 1, "{source}");
        }
    }

    #[test]
    fn review_batch_entry_literal_is_flagged() {
        for source in [
            "fn f() { let _ = ReviewBatchEntry { file: String::new() }; }",
            "fn f() { let _ = er_engine::github::ReviewBatchEntry { file: String::new() }; }",
        ] {
            assert_eq!(findings(source).len(), 1, "{source}");
        }
    }

    #[test]
    fn a_cfg_test_literal_is_ignored() {
        for source in [
            "#[cfg(test)] mod tests { fn f() { let _ = NewLineComment { path: \"a\" }; } }",
            "#[cfg(test)] fn f() { let _ = ReviewBatchEntry { file: String::new() }; }",
            "#[test] fn f() { let _ = NewLineComment { path: \"a\" }; }",
        ] {
            assert!(findings(source).is_empty(), "{source}");
        }
    }

    #[test]
    fn a_destructuring_pattern_is_ignored() {
        for source in [
            "fn f(comment: NewLineComment) { let NewLineComment { path, .. } = comment; }",
            "fn f(c: NewGitHubComment) { if let NewGitHubComment::Line { .. } = c {} }",
            "fn f(e: ReviewBatchEntry) { match e { ReviewBatchEntry { file, .. } => file } }",
        ] {
            assert!(findings(source).is_empty(), "{source}");
        }
    }

    #[test]
    fn a_definition_or_import_is_ignored() {
        for source in [
            "pub struct NewLineComment<'a> { pub path: &'a str }",
            "struct ReviewBatchEntry { pub file: String }",
            "use er_engine::github::NewLineComment;",
        ] {
            assert!(findings(source).is_empty(), "{source}");
        }
    }

    #[test]
    fn a_file_not_on_the_list_is_flagged() {
        let map = by_file(
            "crates/er-desktop/src/commands.rs",
            "fn f() { let _ = NewLineComment { path: \"a\" }; }",
        );
        let diags = evaluate(&map, &[], CODE);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code.as_deref(), Some(CODE));
    }

    #[test]
    fn a_finding_outside_the_allow_list_fails() {
        let mut map = by_file(
            "crates/er-desktop/src/commands.rs",
            "fn f() { let _ = NewLineComment { path: \"a\" }; }",
        );
        // The allowed entry is live, so the only diagnostic is the outsider.
        map.extend(by_file(
            LISTED,
            "fn f() { let _ = NewLineComment { path: \"a\" }; }",
        ));
        let diags = evaluate(&map, &[LISTED.to_string()], CODE);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].level, "error");
        assert_eq!(diags[0].code.as_deref(), Some(CODE));
        assert!(diags[0].rendered.contains("promotion boundary"));
    }

    #[test]
    fn a_finding_in_an_allowed_file_passes() {
        let map = by_file(LISTED, "fn f() { let _ = NewLineComment { path: \"a\" }; }");
        assert!(evaluate(&map, &[LISTED.to_string()], CODE).is_empty());
    }

    #[test]
    fn an_entry_with_no_findings_is_stale() {
        let diags = evaluate(&BTreeMap::new(), &[LISTED.to_string()], CODE);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].rendered.contains("stale allow-list entry"));
        assert!(diags[0].rendered.contains(LISTED));
    }
}
