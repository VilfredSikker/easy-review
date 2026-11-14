//! `er::core_io` — IO in a crate that must stay pure.
//!
//! `crates/er-core` holds the domain types and the artifact contract, so it
//! reads no file, starts no process and starts no thread or channel. The rule
//! also fails a dependency outside the crate's allow-list. Test code is not
//! exempt: a core test that needs a file belongs in the crate that consumes it.

use std::collections::BTreeMap;

use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{ExprPath, ItemUse, TypePath, UseTree};

use crate::diagnostics::Diagnostic;

/// The code the gate reports these findings under.
pub const CODE: &str = "er::core_io";
/// The crate source the rule scans, workspace-relative.
pub const SOURCE: &str = "crates/er-core/src";
/// The manifest whose `[dependencies]` the allow-list checks.
pub const MANIFEST: &str = "crates/er-core/Cargo.toml";
/// The core's arena module, the one place a `crate::arena` path is expected.
pub const ARENA_SOURCE: &str = "crates/er-core/src/arena/";
/// The dependencies the core crate may declare.
pub const ALLOWED: &[&str] = &[
    "anyhow",
    "glob",
    "schemars",
    "serde",
    "serde_json",
    "sha1",
    "sha2",
];

/// Module prefixes that reach the outside world.
const BANNED: &[&[&str]] = &[
    &["std", "fs"],
    &["std", "process"],
    &["std", "thread"],
    &["std", "sync", "mpsc"],
];

/// Bare roots a `use std::fs;` leaves behind at a call site.
const BANNED_ROOTS: &[&str] = &["fs", "process", "thread", "mpsc"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub line: usize,
    /// The path as the diagnostic words it, e.g. `` `std::fs::copy` ``.
    pub message: String,
}

struct IoFinder {
    findings: Vec<Finding>,
}

impl IoFinder {
    fn check(&mut self, segments: &[String], line: usize) {
        if let Some(found) = banned(segments) {
            self.findings.push(Finding {
                line,
                message: format!("`{found}` is IO in the pure core crate"),
            });
            return;
        }
        if let Some(found) = glob_walk(segments) {
            self.findings.push(Finding {
                line,
                message: format!("`{found}` walks the filesystem in the pure core crate"),
            });
        }
    }
}

impl<'ast> Visit<'ast> for IoFinder {
    fn visit_item_use(&mut self, item: &'ast ItemUse) {
        let mut paths = Vec::new();
        collect_use_paths(&item.tree, &mut Vec::new(), &mut paths);
        for path in paths {
            let segments: Vec<String> = path.split("::").map(str::to_string).collect();
            self.check(&segments, item.span().start().line);
        }
    }

    fn visit_expr_path(&mut self, path: &'ast ExprPath) {
        self.check(&path_segments(&path.path), path.span().start().line);
        visit::visit_expr_path(self, path);
    }

    fn visit_type_path(&mut self, path: &'ast TypePath) {
        self.check(&path_segments(&path.path), path.span().start().line);
        visit::visit_type_path(self, path);
    }
}

fn path_segments(path: &syn::Path) -> Vec<String> {
    path.segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect()
}

/// Every full path a `use` tree names, groups included.
fn collect_use_paths(tree: &UseTree, prefix: &mut Vec<String>, out: &mut Vec<String>) {
    match tree {
        UseTree::Path(path) => {
            prefix.push(path.ident.to_string());
            collect_use_paths(&path.tree, prefix, out);
            prefix.pop();
        }
        UseTree::Name(name) => {
            let mut segments = prefix.clone();
            segments.push(name.ident.to_string());
            out.push(segments.join("::"));
        }
        UseTree::Rename(rename) => {
            let mut segments = prefix.clone();
            segments.push(rename.ident.to_string());
            out.push(segments.join("::"));
        }
        UseTree::Glob(_) => out.push(prefix.join("::")),
        UseTree::Group(group) => {
            for item in &group.items {
                collect_use_paths(item, prefix, out);
            }
        }
    }
}

/// The rendered path when `segments` reaches the filesystem, a process, a
/// thread or a channel.
fn banned(segments: &[String]) -> Option<String> {
    for prefix in BANNED {
        if segments.len() >= prefix.len()
            && segments
                .iter()
                .map(String::as_str)
                .take(prefix.len())
                .eq(prefix.iter().copied())
        {
            return Some(segments.join("::"));
        }
    }
    let root = segments.first()?;
    (segments.len() >= 2 && BANNED_ROOTS.contains(&root.as_str())).then(|| segments.join("::"))
}

/// The glob entry points that walk the filesystem; `Pattern` and `MatchOptions`
/// stay allowed, because matching a string is pure.
fn glob_walk(segments: &[String]) -> Option<String> {
    let [root, call] = segments else {
        return None;
    };
    (root == "glob" && (call == "glob" || call == "glob_with")).then(|| segments.join("::"))
}

/// Findings for one source file. A file that does not parse yields none.
pub fn findings(source: &str) -> Vec<Finding> {
    let Ok(file) = syn::parse_file(source) else {
        return Vec::new();
    };
    let mut finder = IoFinder {
        findings: Vec::new(),
    };
    finder.visit_file(&file);
    finder.findings
}

/// Whether `file` is inside the core's arena module.
pub fn is_arena_file(file: &str) -> bool {
    file.starts_with(ARENA_SOURCE)
}

/// A path into the core's arena module, from a file outside it.
fn arena_path(segments: &[String]) -> Option<String> {
    let [root, arena, ..] = segments else {
        return None;
    };
    ((root == "crate" || root == "super") && arena == "arena").then(|| segments.join("::"))
}

struct ArenaFinder {
    findings: Vec<Finding>,
}

impl ArenaFinder {
    fn check(&mut self, segments: &[String], line: usize) {
        if let Some(found) = arena_path(segments) {
            self.findings.push(Finding {
                line,
                message: format!("`{found}` reaches the arena rules from outside the arena module"),
            });
        }
    }
}

impl<'ast> Visit<'ast> for ArenaFinder {
    fn visit_item_use(&mut self, item: &'ast ItemUse) {
        let mut paths = Vec::new();
        collect_use_paths(&item.tree, &mut Vec::new(), &mut paths);
        for path in paths {
            let segments: Vec<String> = path.split("::").map(str::to_string).collect();
            self.check(&segments, item.span().start().line);
        }
    }

    fn visit_expr_path(&mut self, path: &'ast ExprPath) {
        self.check(&path_segments(&path.path), path.span().start().line);
        visit::visit_expr_path(self, path);
    }

    fn visit_type_path(&mut self, path: &'ast TypePath) {
        self.check(&path_segments(&path.path), path.span().start().line);
        visit::visit_type_path(self, path);
    }
}

/// Findings for an arena reference in a source file outside the arena module.
pub fn arena_findings(source: &str) -> Vec<Finding> {
    let Ok(file) = syn::parse_file(source) else {
        return Vec::new();
    };
    let mut finder = ArenaFinder {
        findings: Vec::new(),
    };
    finder.visit_file(&file);
    finder.findings
}

/// Turn findings into gate diagnostics for every file the rule scanned.
pub fn evaluate(findings_by_file: &BTreeMap<String, Vec<Finding>>, code: &str) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (file, findings) in findings_by_file {
        for finding in findings {
            out.push(error(file, finding.line, code, &finding.message));
        }
    }
    out
}

/// The dependencies in `[dependencies]` that the allow-list does not name.
pub fn manifest_findings(manifest: &str) -> Vec<String> {
    let Ok(value) = manifest.parse::<toml::Table>() else {
        return Vec::new();
    };
    let Some(dependencies) = value.get("dependencies").and_then(toml::Value::as_table) else {
        return Vec::new();
    };
    let mut extra: Vec<String> = dependencies
        .keys()
        .filter(|key| !ALLOWED.contains(&key.as_str()))
        .cloned()
        .collect();
    extra.sort();
    extra
}

/// Diagnostics for dependencies outside the allow-list.
pub fn manifest_diagnostics(manifest: &str, code: &str) -> Vec<Diagnostic> {
    manifest_findings(manifest)
        .into_iter()
        .map(|dependency| {
            let message = format!(
                "dependency `{dependency}` is not in er-core's allow-list ({})",
                ALLOWED.join(", ")
            );
            error(MANIFEST, 1, code, &message)
        })
        .collect()
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

    #[test]
    fn a_std_fs_call_is_flagged() {
        let found = findings("fn f() { std::fs::write(\"x\", \"y\").unwrap(); }");
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].message.contains("std::fs::write"));
    }

    #[test]
    fn a_std_process_use_is_flagged() {
        assert_eq!(findings("use std::process::Command;").len(), 1);
    }

    #[test]
    fn thread_and_mpsc_paths_are_flagged() {
        assert_eq!(findings("fn f() { std::thread::spawn(|| {}); }").len(), 1);
        assert_eq!(
            findings("fn f() { let (_tx, _rx) = std::sync::mpsc::channel(); }").len(),
            1
        );
    }

    #[test]
    fn an_aliased_use_is_flagged() {
        assert_eq!(findings("use std::fs as disk;").len(), 1);
    }

    #[test]
    fn a_grouped_use_is_flagged() {
        assert_eq!(findings("use std::{fs, io};").len(), 1);
    }

    #[test]
    fn a_glob_walk_is_flagged() {
        assert_eq!(findings("fn f() { let _ = glob::glob(\"*\"); }").len(), 1);
        assert_eq!(
            findings("fn f() { let _ = glob::glob_with(\"*\", glob::MatchOptions::new()); }").len(),
            1
        );
    }

    #[test]
    fn a_crate_arena_use_is_flagged() {
        assert_eq!(
            arena_findings("use crate::arena::model::ArenaFinding;").len(),
            1
        );
        let source = "fn f() -> Option<super::arena::model::Verdict> { None }";
        assert_eq!(arena_findings(source).len(), 1);
    }

    #[test]
    fn a_review_path_passes() {
        assert!(arena_findings("use crate::review::Finding;").is_empty());
    }

    #[test]
    fn only_the_arena_module_is_local() {
        assert!(is_arena_file("crates/er-core/src/arena/voting.rs"));
        assert!(!is_arena_file("crates/er-core/src/projection.rs"));
    }

    #[test]
    fn glob_pattern_matching_passes() {
        let source = "fn f() -> bool { glob::Pattern::new(\"*\").is_ok_and(|p| p.matches(\"x\")) }";
        assert!(findings(source).is_empty(), "{:?}", findings(source));
    }

    #[test]
    fn an_unrelated_std_path_passes() {
        let source = "use std::sync::Arc; \
                      fn f() -> std::collections::HashMap<u8, u8> { std::collections::HashMap::new() }";
        assert!(findings(source).is_empty(), "{:?}", findings(source));
    }

    #[test]
    fn the_allow_list_accepts_the_declared_set() {
        let manifest = "[dependencies]\nserde.workspace = true\nserde_json.workspace = true\nschemars.workspace = true\nsha1.workspace = true\nglob.workspace = true\nsha2.workspace = true\nanyhow.workspace = true\n";
        assert!(manifest_findings(manifest).is_empty());
    }

    #[test]
    fn an_extra_dependency_is_flagged() {
        let manifest = "[dependencies]\nserde.workspace = true\ndirs.workspace = true\n";
        assert_eq!(manifest_findings(manifest), vec!["dirs".to_string()]);
    }
}
