//! `er::sidecar_fs` — a direct filesystem call in a file that must go through
//! the artifact store.
//!
//! `deny` is a grow-only list of workspace-relative paths. Inside one of them,
//! a path expression rooted at `std::fs::` or `fs::`, or a `File::` /
//! `OpenOptions::` call, is an error: those files read and write sidecars
//! through `er_store::ArtifactStore`, which owns the envelope, the lock and the
//! tmp+rename write. Test code (`#[cfg(test)]` and `#[test]`) is exempt, since
//! a test may stage a broken file on disk.

use std::collections::BTreeMap;

use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{ExprPath, Item};

use crate::attrs::{is_test_item, item_attrs};
use crate::diagnostics::Diagnostic;

/// The code the gate reports these findings under.
pub const CODE: &str = "er::sidecar_fs";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub line: usize,
    /// The call as the diagnostic words it, e.g. `` `std::fs::write` ``.
    pub message: String,
}

struct FsFinder {
    findings: Vec<Finding>,
}

impl<'ast> Visit<'ast> for FsFinder {
    fn visit_item(&mut self, item: &'ast Item) {
        if item_attrs(item).is_some_and(is_test_item) {
            return;
        }
        visit::visit_item(self, item);
    }

    fn visit_expr_path(&mut self, path: &'ast ExprPath) {
        if let Some(found) = fs_path(path) {
            self.findings.push(Finding {
                line: path.span().start().line,
                message: format!("`{found}` reaches the filesystem directly"),
            });
        }
        visit::visit_expr_path(self, path);
    }
}

/// The rendered path when `path` reaches the filesystem. A `use std::fs;`
/// import does not hide `fs::write(..)`: the call itself is still a path
/// expression starting `fs::`.
fn fs_path(path: &ExprPath) -> Option<String> {
    let segments: Vec<String> = path
        .path
        .segments
        .iter()
        .map(|s| s.ident.to_string())
        .collect();
    let root = segments.first()?;
    let rooted = match root.as_str() {
        "std" => segments.get(1).is_some_and(|s| s == "fs") && segments.len() >= 3,
        "fs" | "File" | "OpenOptions" => segments.len() >= 2,
        _ => false,
    };
    rooted.then(|| segments.join("::"))
}

/// Findings for one source file. A file that does not parse yields none.
pub fn findings(source: &str) -> Vec<Finding> {
    let Ok(file) = syn::parse_file(source) else {
        return Vec::new();
    };
    let mut finder = FsFinder {
        findings: Vec::new(),
    };
    finder.visit_file(&file);
    finder.findings
}

/// Turn findings into gate diagnostics, only for the files on the deny list.
/// The list grows as modules move to the store; an entry with no findings is
/// not stale, because a later migration may add the file before its calls do.
pub fn evaluate(
    findings_by_file: &BTreeMap<String, Vec<Finding>>,
    deny: &[String],
    code: &str,
) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for entry in deny {
        let Some(findings) = findings_by_file.get(entry) else {
            continue;
        };
        for finding in findings {
            let message = format!(
                "{} in a file that must read and write through the artifact store",
                finding.message
            );
            out.push(error(entry, finding.line, code, &message));
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

    const LISTED: &str = "crates/er-engine/src/ai/annotation_store.rs";

    fn by_file(file: &str, source: &str) -> BTreeMap<String, Vec<Finding>> {
        let mut map = BTreeMap::new();
        let found = findings(source);
        if !found.is_empty() {
            map.insert(file.to_string(), found);
        }
        map
    }

    #[test]
    fn std_fs_call_in_a_listed_file_is_flagged() {
        let map = by_file(LISTED, "fn f() { std::fs::write(\"x\", \"y\").unwrap(); }");
        let diags = evaluate(&map, &[LISTED.to_string()], CODE);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].level, "error");
        assert_eq!(diags[0].code.as_deref(), Some(CODE));
        assert!(diags[0].rendered.contains("`std::fs::write`"), "{diags:?}");
    }

    #[test]
    fn fs_call_in_a_cfg_test_module_is_ignored() {
        for source in [
            "#[cfg(test)] mod tests { fn f() { std::fs::write(\"x\", \"y\"); } }",
            "#[cfg(test)] fn f() { std::fs::read_to_string(\"x\"); }",
            "#[test] fn f() { std::fs::write(\"x\", \"y\"); }",
        ] {
            assert!(findings(source).is_empty(), "{source}");
        }
    }

    #[test]
    fn a_file_not_on_the_list_is_ignored() {
        let map = by_file(
            "crates/er-engine/src/ai/comments.rs",
            "fn f() { std::fs::write(\"x\", \"y\").unwrap(); }",
        );
        assert!(!map.is_empty(), "the finder still sees the call");
        let diags = evaluate(&map, &[LISTED.to_string()], CODE);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn a_use_fs_import_does_not_hide_the_call() {
        let source = "use std::fs;\nfn f() { fs::write(\"x\", \"y\").unwrap(); }";
        let found = findings(source);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].line, 2);
    }

    #[test]
    fn file_and_open_options_calls_are_flagged() {
        for source in [
            "fn f() { let _ = File::open(\"x\"); }",
            "fn f() { let _ = OpenOptions::new().write(true).open(\"x\"); }",
        ] {
            assert_eq!(findings(source).len(), 1, "{source}");
        }
    }

    #[test]
    fn a_plain_path_or_use_statement_is_not_flagged() {
        for source in [
            "use std::fs;",
            "use std::fs::write;",
            "fn f(path: &str) { let _ = path; }",
        ] {
            assert!(findings(source).is_empty(), "{source}");
        }
    }
}
