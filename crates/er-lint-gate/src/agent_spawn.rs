//! `er::agent_spawn` — a program spawn outside the agent runtime or a reviewed
//! OS-launch module.
//!
//! `crates/er-agent` owns the process runtime. Every other spawn is an OS
//! handoff (clipboard, editor, file manager, tar) or an agent call site that
//! has not moved yet. The OS handoffs are whole files, listed under
//! `agent-spawn-os-launch`; the remaining agent call sites are listed under
//! `agent-spawn-allow`, which only shrinks. A literal `git` or `gh` spawn is
//! left to `er::git_spawn` / `er::gh_spawn`. Test code (`#[cfg(test)]` and
//! `#[test]`) is exempt, and so is the reviewed test-support module, which
//! compiles only under `test` or a `test-support` feature.

use std::collections::{BTreeMap, BTreeSet};

use proc_macro2::{Delimiter, TokenStream, TokenTree};
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Expr, ExprCall, ExprLit, Item, Lit, Macro, UseTree};

use crate::attrs::{is_test_item, item_attrs};
use crate::diagnostics::Diagnostic;

/// The code the gate reports these findings under.
pub const CODE: &str = "er::agent_spawn";

/// The crate that owns the process runtime.
pub const HOME: &str = "crates/er-agent";

/// Crates that shell out to cargo themselves; they are tooling, not agents.
const TOOL_CRATES: [&str; 2] = ["crates/er-lint-gate", "crates/er-crap"];

/// Test scaffolding that compiles only under `test` or a `test-support`
/// feature. The path is exact, so a normal module cannot borrow the exemption
/// by name.
const TEST_SUPPORT_FILES: [&str; 1] = ["crates/er-engine/src/test_support.rs"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub line: usize,
    /// The call as the diagnostic words it, e.g. `` `Command::new("pbcopy")` ``.
    pub message: String,
}

struct SpawnFinder<'a> {
    aliases: &'a BTreeSet<String>,
    findings: Vec<Finding>,
}

impl<'ast> Visit<'ast> for SpawnFinder<'_> {
    fn visit_item(&mut self, item: &'ast Item) {
        if item_attrs(item).is_some_and(is_test_item) {
            return;
        }
        visit::visit_item(self, item);
    }

    fn visit_expr_call(&mut self, call: &'ast ExprCall) {
        if is_command_new(&call.func, self.aliases) && !is_literal_owned_spawn(call.args.first()) {
            self.findings.push(Finding {
                line: call.span().start().line,
                message: message_for(call.args.first()),
            });
        }
        visit::visit_expr_call(self, call);
    }

    fn visit_macro(&mut self, mac: &'ast Macro) {
        if let Some(line) = command_new_in_tokens(mac.tokens.clone(), self.aliases) {
            self.findings.push(Finding {
                line,
                message: "`Command::new(..)` inside a macro".into(),
            });
        }
        visit::visit_macro(self, mac);
    }
}

/// Findings for one source file. A file that does not parse yields none.
pub fn findings(source: &str) -> Vec<Finding> {
    let Ok(file) = syn::parse_file(source) else {
        return Vec::new();
    };
    let mut collector = AliasCollector::default();
    collector.visit_file(&file);
    let mut finder = SpawnFinder {
        aliases: &collector.aliases,
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
        if item_attrs(item).is_some_and(is_test_item) {
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

/// A literal `git` or `gh` program belongs to the spawn rule for that tool.
fn is_literal_owned_spawn(first: Option<&Expr>) -> bool {
    matches!(
        first,
        Some(Expr::Lit(ExprLit {
            lit: Lit::Str(literal),
            ..
        })) if literal.value() == "git" || literal.value() == "gh"
    )
}

fn message_for(first: Option<&Expr>) -> String {
    match first {
        Some(Expr::Lit(ExprLit {
            lit: Lit::Str(literal),
            ..
        })) => format!("`Command::new({:?})`", literal.value()),
        _ => "`Command::new(..)`".to_string(),
    }
}

/// A `Command::new` inside a macro's token stream, e.g.
/// `vec![Command::new("sh")]`. `syn` does not parse macro tokens as
/// expressions, so the tokens are walked directly.
fn command_new_in_tokens(tokens: TokenStream, aliases: &BTreeSet<String>) -> Option<usize> {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    for (index, tree) in trees.iter().enumerate() {
        if let TokenTree::Group(group) = tree {
            if let Some(line) = command_new_in_tokens(group.stream(), aliases) {
                return Some(line);
            }
        }
        let TokenTree::Ident(ident) = tree else {
            continue;
        };
        if ident != "Command" && !aliases.contains(&ident.to_string()) {
            continue;
        }
        let colon_one =
            matches!(trees.get(index + 1), Some(TokenTree::Punct(p)) if p.as_char() == ':');
        let colon_two =
            matches!(trees.get(index + 2), Some(TokenTree::Punct(p)) if p.as_char() == ':');
        let is_new = matches!(trees.get(index + 3), Some(TokenTree::Ident(next)) if next == "new");
        if !(colon_one && colon_two && is_new) {
            continue;
        }
        let Some(TokenTree::Group(args)) = trees.get(index + 4) else {
            continue;
        };
        if args.delimiter() != Delimiter::Parenthesis {
            continue;
        }
        if first_macro_token_is_owned_spawn(&args.stream()) {
            continue;
        }
        return Some(ident.span().start().line);
    }
    None
}

fn first_macro_token_is_owned_spawn(tokens: &TokenStream) -> bool {
    let mut trees = tokens.clone().into_iter();
    let Some(TokenTree::Literal(literal)) = trees.next() else {
        return false;
    };
    let text = literal.to_string();
    matches!(text.trim_matches('"'), "git" | "gh")
}

/// Turn findings into gate diagnostics. A file under the home crate, on the
/// OS-launch list, on the allow list, or in a tool crate passes; anything else
/// fails. An allow entry with no findings fails as stale, which is what makes
/// the list shrink.
pub fn evaluate(
    findings_by_file: &BTreeMap<String, Vec<Finding>>,
    os_launch: &[String],
    allow: &[String],
    code: &str,
) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (file, findings) in findings_by_file {
        let home = file == HOME || file.starts_with(&format!("{HOME}/"));
        let os_launch_file = os_launch.iter().any(|entry| entry == file);
        let allowed = allow.iter().any(|entry| entry == file);
        let tool = TOOL_CRATES
            .iter()
            .any(|krate| file == *krate || file.starts_with(&format!("{krate}/")));
        let test_support = TEST_SUPPORT_FILES.contains(&file.as_str());
        if home || os_launch_file || allowed || tool || test_support {
            continue;
        }
        for finding in findings {
            let message = format!(
                "{} outside {HOME} and the OS-launch modules: route it through er-agent, or move it to os_launch.rs",
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

/// Every `agent-spawn-os-launch` entry must be `crates/<crate>/src/os_launch.rs`,
/// with at most one per crate. The list is permanent, so an entry with no
/// findings is not stale; the path check is what keeps the carve-out small.
pub fn validate_os_launch(entries: &[String], code: &str) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for entry in entries {
        let Some(krate) = os_launch_crate(entry) else {
            out.push(error(
                entry,
                1,
                code,
                "agent-spawn-os-launch entries must be crates/<crate>/src/os_launch.rs",
            ));
            continue;
        };
        if !seen.insert(krate) {
            out.push(error(
                entry,
                1,
                code,
                &format!("a second agent-spawn-os-launch entry for crates/{krate}"),
            ));
        }
    }
    out
}

fn os_launch_crate(entry: &str) -> Option<&str> {
    let mut parts = entry.split('/');
    match (
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
    ) {
        (Some("crates"), Some(krate), Some("src"), Some("os_launch.rs"), None)
            if !krate.is_empty() =>
        {
            Some(krate)
        }
        _ => None,
    }
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

    fn finding() -> Finding {
        Finding {
            line: 1,
            message: "`Command::new(\"sh\")`".into(),
        }
    }

    fn by_file(file: &str) -> BTreeMap<String, Vec<Finding>> {
        let mut map = BTreeMap::new();
        map.insert(file.to_string(), vec![finding()]);
        map
    }

    #[test]
    fn a_plain_spawn_is_flagged() {
        assert_eq!(findings("fn f() { Command::new(\"sh\"); }").len(), 1);
    }

    #[test]
    fn a_non_literal_spawn_is_flagged() {
        for source in [
            "fn f(cmd: &str) { Command::new(cmd); }",
            "fn f() { Command::new(program()); }",
        ] {
            assert_eq!(findings(source).len(), 1, "{source}");
        }
    }

    #[test]
    fn literal_git_and_gh_are_left_to_their_rules() {
        for source in [
            "fn f() { Command::new(\"git\"); }",
            "fn f() { Command::new(\"gh\"); }",
        ] {
            assert!(findings(source).is_empty(), "{source}");
        }
    }

    #[test]
    fn test_code_is_skipped() {
        for source in [
            "#[cfg(test)] mod tests { fn f() { Command::new(\"sh\"); } }",
            "#[cfg(test)] fn f() { Command::new(\"sh\"); }",
            "#[test] fn f() { Command::new(\"sh\"); }",
            "#[tokio::test] async fn f() { Command::new(\"sh\"); }",
        ] {
            assert!(findings(source).is_empty(), "{source}");
        }
    }

    #[test]
    fn renamed_and_tokio_command_imports_are_flagged() {
        for source in [
            "use std::process::Command as Cmd; fn f() { Cmd::new(\"sh\"); }",
            "use tokio::process::Command as TokioCmd; fn f() { TokioCmd::new(\"sh\"); }",
            "use std::process::{Command as C}; fn f() { C::new(\"sh\"); }",
            "fn f() { tokio::process::Command::new(\"sh\"); }",
        ] {
            assert_eq!(findings(source).len(), 1, "{source}");
        }
    }

    #[test]
    fn a_spawn_inside_a_macro_is_flagged() {
        for source in [
            "fn f() { let _ = vec![Command::new(\"sh\")]; }",
            "fn f() { let _ = vec![format!(\"{}\", Command::new(\"sh\"))]; }",
        ] {
            assert_eq!(findings(source).len(), 1, "{source}");
        }
    }

    #[test]
    fn a_renamed_spawn_inside_a_macro_is_flagged() {
        let source = "use std::process::Command as Cmd; fn f() { let _ = vec![Cmd::new(\"sh\")]; }";
        assert_eq!(findings(source).len(), 1);
    }

    #[test]
    fn a_owned_spawn_inside_a_macro_is_exempt() {
        assert!(findings("fn f() { let _ = vec![Command::new(\"git\")]; }").is_empty());
    }

    #[test]
    fn a_spawn_outside_the_lists_fails() {
        let diags = evaluate(&by_file("crates/x/src/a.rs"), &[], &[], CODE);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].level, "error");
        assert_eq!(diags[0].code.as_deref(), Some(CODE));
        assert!(
            diags[0].rendered.contains(HOME),
            "the message names the home crate: {diags:?}"
        );
    }

    #[test]
    fn a_spawn_in_the_home_crate_passes() {
        let map = by_file("crates/er-agent/src/agent_run.rs");
        assert!(evaluate(&map, &[], &[], CODE).is_empty());
    }

    #[test]
    fn a_spawn_in_an_os_launch_file_passes() {
        let os_launch = ["crates/er-engine/src/os_launch.rs".to_string()];
        let map = by_file("crates/er-engine/src/os_launch.rs");
        assert!(evaluate(&map, &os_launch, &[], CODE).is_empty());
    }

    #[test]
    fn a_spawn_in_an_allow_listed_file_passes() {
        let allow = ["crates/er-engine/src/app/card_ai_spawn.rs".to_string()];
        let map = by_file("crates/er-engine/src/app/card_ai_spawn.rs");
        assert!(evaluate(&map, &[], &allow, CODE).is_empty());
    }

    #[test]
    fn a_tool_crate_spawn_is_exempt() {
        for file in [
            "crates/er-lint-gate/src/main.rs",
            "crates/er-crap/src/main.rs",
        ] {
            assert!(
                evaluate(&by_file(file), &[], &[], CODE).is_empty(),
                "{file}"
            );
        }
    }

    #[test]
    fn only_the_reviewed_test_support_file_is_exempt() {
        let reviewed = by_file("crates/er-engine/src/test_support.rs");
        assert!(evaluate(&reviewed, &[], &[], CODE).is_empty());
        let same_name_elsewhere = by_file("crates/x/src/test_support.rs");
        assert_eq!(evaluate(&same_name_elsewhere, &[], &[], CODE).len(), 1);
    }

    #[test]
    fn an_allow_entry_without_findings_is_stale() {
        let allow = ["crates/x/src/a.rs".to_string()];
        let diags = evaluate(&BTreeMap::new(), &[], &allow, CODE);
        assert_eq!(diags.len(), 1);
        assert!(
            diags[0]
                .rendered
                .contains("stale allow-list entry: remove crates/x/src/a.rs"),
            "{diags:?}"
        );
    }

    #[test]
    fn one_os_launch_module_per_crate_passes() {
        let entries = [
            "crates/er-engine/src/os_launch.rs".to_string(),
            "crates/er-desktop/src/os_launch.rs".to_string(),
        ];
        assert!(validate_os_launch(&entries, CODE).is_empty());
    }

    #[test]
    fn a_second_os_launch_entry_for_one_crate_fails() {
        let entries = [
            "crates/er-engine/src/os_launch.rs".to_string(),
            "crates/er-engine/src/os_launch.rs".to_string(),
        ];
        let diags = validate_os_launch(&entries, CODE);
        assert_eq!(diags.len(), 1);
        assert!(
            diags[0]
                .rendered
                .contains("a second agent-spawn-os-launch entry"),
            "{diags:?}"
        );
    }

    #[test]
    fn an_os_launch_entry_at_another_path_fails() {
        for entry in [
            "crates/er-engine/src/navigation.rs",
            "crates/er-engine/src/nested/os_launch.rs",
            "src/os_launch.rs",
            "crates/er-engine/src/os_launch.rs/extra",
        ] {
            let entries = [entry.to_string()];
            let diags = validate_os_launch(&entries, CODE);
            assert_eq!(diags.len(), 1, "{entry}");
        }
    }
}
