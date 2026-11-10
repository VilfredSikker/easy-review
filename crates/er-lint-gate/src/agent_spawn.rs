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
use syn::{Expr, ExprCall, ExprLit, ExprPath, Item, Lit, Macro, Path, UseTree};

use crate::attrs::{impl_item_attrs, is_test_item, is_test_support_gate, item_attrs};
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

    fn visit_impl_item(&mut self, item: &'ast syn::ImplItem) {
        if is_test_item(impl_item_attrs(item)) {
            return;
        }
        visit::visit_impl_item(self, item);
    }

    fn visit_expr_call(&mut self, call: &'ast ExprCall) {
        if is_command_new(&call.func, self.aliases) && !is_literal_owned_spawn(call.args.first()) {
            self.findings.push(Finding {
                line: call.span().start().line,
                message: message_for(call.args.first()),
            });
        }
        // A matching path callee was just reported; walking it again would
        // report the same call twice. Every other callee still gets visited,
        // because an immediately invoked closure keeps its body there.
        let func: &Expr = &call.func;
        match func {
            Expr::Path(path) if is_command_new_path(&path.path, self.aliases) => {}
            func => self.visit_expr(func),
        }
        for arg in &call.args {
            self.visit_expr(arg);
        }
    }

    fn visit_expr_path(&mut self, path: &'ast ExprPath) {
        if is_command_new_path(&path.path, self.aliases) {
            let owner = &path.path.segments[path.path.segments.len() - 2].ident;
            self.findings.push(Finding {
                line: path.span().start().line,
                message: format!("`{owner}::new` used as a value"),
            });
        }
        visit::visit_expr_path(self, path);
    }

    fn visit_macro(&mut self, mac: &'ast Macro) {
        if let Some((line, owner)) = command_new_in_tokens(mac.tokens.clone(), self.aliases) {
            self.findings.push(Finding {
                line,
                message: format!("`{owner}::new(..)` inside a macro"),
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
    // Type aliases can point at each other in any order, so collect until the
    // set stops growing.
    loop {
        let before = collector.aliases.len();
        collector.visit_file(&file);
        if collector.aliases.len() == before {
            break;
        }
    }
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
        if let Item::Type(alias) = item {
            if let syn::Type::Path(target) = &*alias.ty {
                if path_names_command(&target.path, &self.aliases) {
                    self.aliases.insert(alias.ident.to_string());
                }
            }
        }
        visit::visit_item(self, item);
    }

    fn visit_impl_item(&mut self, item: &'ast syn::ImplItem) {
        if is_test_item(impl_item_attrs(item)) {
            return;
        }
        visit::visit_impl_item(self, item);
    }
}

/// Whether the last segment names `Command` or one of its aliases.
fn path_names_command(path: &Path, aliases: &BTreeSet<String>) -> bool {
    path.segments.last().is_some_and(|segment| {
        segment.ident == "Command" || aliases.contains(&segment.ident.to_string())
    })
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
    matches!(func, Expr::Path(path) if is_command_new_path(&path.path, aliases))
}

/// Whether the path names `Command::new`, an alias of it, or the PTY crate's
/// `CommandBuilder::new`, whether called or passed as a value.
fn is_command_new_path(path: &Path, aliases: &BTreeSet<String>) -> bool {
    let segments = &path.segments;
    if segments.len() < 2 {
        return false;
    }
    let owner = &segments[segments.len() - 2].ident;
    (owner == "Command" || owner == "CommandBuilder" || aliases.contains(&owner.to_string()))
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
fn command_new_in_tokens(
    tokens: TokenStream,
    aliases: &BTreeSet<String>,
) -> Option<(usize, String)> {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    for (index, tree) in trees.iter().enumerate() {
        if let TokenTree::Group(group) = tree {
            if let Some(found) = command_new_in_tokens(group.stream(), aliases) {
                return Some(found);
            }
        }
        let TokenTree::Ident(ident) = tree else {
            continue;
        };
        if ident != "Command" && ident != "CommandBuilder" && !aliases.contains(&ident.to_string())
        {
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
        return Some((ident.span().start().line, ident.to_string()));
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

/// The rule exempts the reviewed test-support module by path. That is safe
/// only while the module itself is gated to test builds; this check fails when
/// `crates/er-engine/src/lib.rs` declares `mod test_support` without that gate.
pub fn validate_test_support_gate(source: &str, code: &str) -> Vec<Diagnostic> {
    let Ok(file) = syn::parse_file(source) else {
        return Vec::new();
    };
    let module = file.items.iter().find_map(|item| match item {
        Item::Mod(module) if module.ident == "test_support" => Some(module),
        _ => None,
    });
    let Some(module) = module else {
        return vec![error(
            "crates/er-engine/src/lib.rs",
            1,
            code,
            "er-engine no longer declares `mod test_support`; remove its agent-spawn exemption",
        )];
    };
    if is_test_support_gate(&module.attrs) {
        return Vec::new();
    }
    vec![error(
        "crates/er-engine/src/lib.rs",
        module.span().start().line,
        code,
        "`mod test_support` must stay behind #[cfg(any(test, feature = \"test-support\"))]; the agent-spawn rule exempts its file by path",
    )]
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
    fn a_spawn_inside_an_immediately_invoked_closure_is_flagged() {
        // The closure is the callee of an IIFE, not an argument: the finder
        // must still walk it.
        let source = r#"
            struct App;
            impl App {
                pub fn spawn(&self) {
                    let _ = (|| -> Result<(), ()> {
                        std::process::Command::new("sh");
                        Ok(())
                    })();
                }
            }
        "#;
        assert_eq!(findings(source).len(), 1);
    }

    #[test]
    fn a_spawn_passed_as_a_value_is_flagged() {
        for source in [
            "fn f() { let _ = [\"a\"].map(Command::new); }",
            "use std::process::Command as Cmd; fn f() { let _ = [\"a\"].map(Cmd::new); }",
            "fn f() { let _ = [\"a\"].map(std::process::Command::new); }",
        ] {
            assert_eq!(findings(source).len(), 1, "{source}");
        }
    }

    #[test]
    fn a_type_alias_of_command_is_followed() {
        for source in [
            "type Cmd = Command; fn f() { Cmd::new(\"sh\"); }",
            "type Base = std::process::Command; type Cmd = Base; fn f() { Cmd::new(\"sh\"); }",
        ] {
            assert_eq!(findings(source).len(), 1, "{source}");
        }
    }

    #[test]
    fn a_cfg_test_method_inside_an_impl_is_skipped() {
        let source = r#"
            struct S;
            impl S {
                #[cfg(test)]
                fn f(&self) { Command::new("sh"); }
                #[cfg(test)]
                type X = Command;
            }
        "#;
        assert!(findings(source).is_empty());
    }

    #[test]
    fn command_builder_new_is_flagged() {
        for source in [
            "fn f() { portable_pty::CommandBuilder::new(\"sh\"); }",
            "use portable_pty::CommandBuilder; fn f() { CommandBuilder::new(\"sh\"); }",
            "fn f() { let _ = [\"sh\"].map(CommandBuilder::new); }",
            "fn f() { let _ = vec![CommandBuilder::new(\"sh\")]; }",
        ] {
            assert_eq!(findings(source).len(), 1, "{source}");
        }
    }

    #[test]
    fn the_test_support_module_must_keep_its_gate() {
        let gated = "#[cfg(any(test, feature = \"test-support\"))]\nmod test_support;";
        assert!(validate_test_support_gate(gated, CODE).is_empty());
        let ungated = "mod test_support;";
        let diags = validate_test_support_gate(ungated, CODE);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].rendered.contains("mod test_support"), "{diags:?}");
        let not_test = "#[cfg(not(test))]\nmod test_support;";
        assert_eq!(validate_test_support_gate(not_test, CODE).len(), 1);
    }

    #[test]
    fn a_missing_test_support_module_fails_the_exemption() {
        let diags = validate_test_support_gate("pub mod other;", CODE);
        assert_eq!(diags.len(), 1);
        assert!(
            diags[0].rendered.contains("no longer declares"),
            "{diags:?}"
        );
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
