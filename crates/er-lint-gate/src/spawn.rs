//! `er::git_spawn` — a program spawn outside the crate that owns it.
//!
//! The rule is generic over the program name so one implementation covers the
//! git and gh gates. `home` is the crate that may spawn it, and `allow` lists
//! files that still do; the list may only shrink, because an entry with no
//! findings fails as stale.

use std::collections::{BTreeMap, BTreeSet};

use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Expr, ExprCall, ExprLit, Item, Lit, UseTree};

use crate::attrs::{is_cfg_test, item_attrs};
use crate::diagnostics::Diagnostic;

/// The code the gate reports these findings under.
pub const CODE: &str = "er::git_spawn";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub line: usize,
    /// The call as the diagnostic words it, e.g. `` `Command::new("git")` ``.
    pub message: String,
}

struct SpawnFinder<'a> {
    program: &'a str,
    aliases: BTreeSet<String>,
    findings: Vec<Finding>,
}

impl<'ast> Visit<'ast> for SpawnFinder<'_> {
    fn visit_item(&mut self, item: &'ast Item) {
        if item_attrs(item).is_some_and(is_cfg_test) {
            return;
        }
        visit::visit_item(self, item);
    }

    fn visit_expr_call(&mut self, call: &'ast ExprCall) {
        if is_command_new(&call.func, &self.aliases) {
            if let Some(Expr::Lit(ExprLit {
                lit: Lit::Str(literal),
                ..
            })) = call.args.first()
            {
                if literal.value() == self.program {
                    self.findings.push(Finding {
                        line: call.span().start().line,
                        message: format!("`Command::new({:?})`", self.program),
                    });
                }
            }
        }
        visit::visit_expr_call(self, call);
    }
}

/// Findings for one source file. A file that does not parse yields none.
pub fn findings(source: &str, program: &str) -> Vec<Finding> {
    let Ok(file) = syn::parse_file(source) else {
        return Vec::new();
    };
    let mut collector = AliasCollector::default();
    collector.visit_file(&file);
    let mut finder = SpawnFinder {
        program,
        aliases: collector.aliases,
        findings: Vec::new(),
    };
    finder.visit_file(&file);
    finder.findings
}

/// Collects the local names `Command` is imported under, so a renamed import
/// cannot hide a spawn from the rule.
#[derive(Default)]
struct AliasCollector {
    aliases: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for AliasCollector {
    fn visit_item(&mut self, item: &'ast Item) {
        if item_attrs(item).is_some_and(is_cfg_test) {
            return;
        }
        if let Item::Use(item_use) = item {
            collect_use_aliases(&item_use.tree, &mut self.aliases);
        }
        visit::visit_item(self, item);
    }
}

fn collect_use_aliases(tree: &UseTree, aliases: &mut BTreeSet<String>) {
    match tree {
        UseTree::Path(path) => collect_use_aliases(&path.tree, aliases),
        UseTree::Group(group) => {
            for tree in &group.items {
                collect_use_aliases(tree, aliases);
            }
        }
        UseTree::Rename(rename) if rename.ident == "Command" => {
            aliases.insert(rename.rename.to_string());
        }
        _ => {}
    }
}

/// Turn findings into gate diagnostics. A file under `home` is the owner; a
/// file in `allow` is a known remaining caller; anything else fails. An allow
/// entry with no findings fails as stale, which is what makes the list shrink.
pub fn evaluate(
    findings_by_file: &BTreeMap<String, Vec<Finding>>,
    home: &str,
    allow: &[String],
    code: &str,
    trait_name: &str,
) -> Vec<Diagnostic> {
    let under_home = |file: &str| file == home || file.starts_with(&format!("{home}/"));
    let mut out = Vec::new();
    for (file, findings) in findings_by_file {
        if under_home(file) || allow.iter().any(|entry| entry == file) {
            continue;
        }
        for finding in findings {
            let message = format!(
                "{} outside {home}: call the {trait_name} trait",
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

fn is_command_new(func: &Expr, aliases: &BTreeSet<String>) -> bool {
    let Expr::Path(path) = func else {
        return false;
    };
    let segments = &path.path.segments;
    if segments.len() < 2 {
        return false;
    }
    let owner = &segments[segments.len() - 2].ident;
    (owner == "Command" || aliases.contains(&owner.to_string()))
        && segments[segments.len() - 1].ident == "new"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finding() -> Finding {
        Finding {
            line: 1,
            message: "`Command::new(\"git\")`".into(),
        }
    }

    #[test]
    fn flags_a_plain_git_spawn() {
        let found = findings("fn f() { std::process::Command::new(\"git\"); }", "git");
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn flags_tokio_and_unqualified_paths() {
        for source in [
            "fn f() { tokio::process::Command::new(\"git\"); }",
            "fn f() { Command::new(\"git\"); }",
        ] {
            assert_eq!(findings(source, "git").len(), 1, "{source}");
        }
    }

    #[test]
    fn ignores_other_programs() {
        assert!(findings("fn f() { Command::new(\"gh\"); }", "git").is_empty());
    }

    #[test]
    fn skips_cfg_test_modules_and_fns() {
        for source in [
            "#[cfg(test)] mod tests { fn f() { Command::new(\"git\"); } }",
            "#[cfg(test)] fn f() { Command::new(\"git\"); }",
        ] {
            assert!(findings(source, "git").is_empty(), "{source}");
        }
    }

    #[test]
    fn renamed_command_import_is_flagged() {
        for source in [
            "use std::process::Command as Cmd; fn f() { Cmd::new(\"git\"); }",
            "use tokio::process::Command as TokioCmd; fn f() { TokioCmd::new(\"git\"); }",
        ] {
            assert_eq!(findings(source, "git").len(), 1, "{source}");
        }
    }

    #[test]
    fn grouped_renamed_command_import_is_flagged() {
        let source = "use std::process::{Command as C}; fn f() { C::new(\"git\"); }";
        assert_eq!(findings(source, "git").len(), 1, "{source}");
    }

    #[test]
    fn a_spawn_through_a_variable_is_not_flagged() {
        assert!(findings("fn f(bin: &str) { Command::new(bin); }", "git").is_empty());
    }

    #[test]
    fn a_spawn_anywhere_outside_home_fails_with_no_allow_list() {
        let mut by_file = BTreeMap::new();
        by_file.insert("crates/er-desktop/src/a.rs".to_string(), vec![finding()]);
        let diags = evaluate(&by_file, "crates/er-git", &[], CODE, "Git");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].level, "error");
    }

    #[test]
    fn the_gh_rule_does_not_flag_a_git_spawn() {
        assert!(findings("fn f() { Command::new(\"git\"); }", "gh").is_empty());
    }

    #[test]
    fn the_gh_rule_names_the_github_trait() {
        let mut by_file = BTreeMap::new();
        by_file.insert("crates/x/src/a.rs".to_string(), vec![finding()]);
        let diags = evaluate(&by_file, "crates/er-github", &[], "er::gh_spawn", "GitHub");
        assert!(
            diags[0].rendered.contains("call the GitHub trait"),
            "{diags:?}"
        );
    }

    #[test]
    fn a_finding_outside_home_and_allow_list_fails() {
        let mut by_file = BTreeMap::new();
        by_file.insert("crates/x/src/a.rs".to_string(), vec![finding()]);
        let diags = evaluate(&by_file, "crates/er-git", &[], CODE, "Git");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].level, "error");
        assert_eq!(diags[0].code.as_deref(), Some(CODE));
        assert!(
            diags[0].rendered.contains("outside crates/er-git"),
            "{diags:?}"
        );
    }

    #[test]
    fn a_finding_in_the_allow_list_passes() {
        let mut by_file = BTreeMap::new();
        by_file.insert("crates/x/src/a.rs".to_string(), vec![finding()]);
        let allow = ["crates/x/src/a.rs".to_string()];
        assert!(evaluate(&by_file, "crates/er-git", &allow, CODE, "Git").is_empty());
    }

    #[test]
    fn a_finding_under_home_passes() {
        let mut by_file = BTreeMap::new();
        by_file.insert("crates/er-git/src/a.rs".to_string(), vec![finding()]);
        assert!(evaluate(&by_file, "crates/er-git", &[], CODE, "Git").is_empty());
    }

    #[test]
    fn an_allow_list_entry_without_findings_is_stale() {
        let allow = ["crates/x/src/a.rs".to_string()];
        let diags = evaluate(&BTreeMap::new(), "crates/er-git", &allow, CODE, "Git");
        assert_eq!(diags.len(), 1);
        assert!(
            diags[0]
                .rendered
                .contains("stale allow-list entry: remove crates/x/src/a.rs"),
            "{diags:?}"
        );
    }
}
