//! `er::impl_cohesion` — an inherent `impl` block that is several types in one.
//!
//! The Rust counterpart of desktop-ui's `class-cohesion` rule (LCOM4). Methods
//! that take `self` are nodes; two are joined when one reaches the other, or
//! a field the other touches, through `self` (`self.x`, `self.m()`,
//! `Self::m(..)`). Each connected group never touches another group's state
//! or methods, so it could be its own type.
//!
//! Associated functions (no receiver) are left out: constructors initialise
//! every field, which would join every group. Trait impls are skipped, since
//! their methods answer to the trait, and so is `#[cfg(test)]` code.
//!
//! `syn` does not parse macro bodies, so `self.x` inside `format!` or
//! `matches!` is read from the macro's tokens; missing it would split groups
//! that do share state.

use std::collections::{HashMap, HashSet};

use proc_macro2::{TokenStream, TokenTree};
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{ExprField, ExprMethodCall, ExprPath, ImplItem, Item, ItemImpl, Macro, Member};

/// The code the gate reports and budgets these findings under.
pub const CODE: &str = "er::impl_cohesion";

#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub max_groups: usize,
    pub min_group_lines: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub line: usize,
    pub message: String,
}

/// Findings for one source file. A file that does not parse yields none.
pub fn impl_cohesion(source: &str, opts: Options) -> Vec<Finding> {
    let Ok(file) = syn::parse_file(source) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    check_items(&file.items, opts, &mut out);
    out
}

fn is_cfg_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| {
        a.path().is_ident("cfg")
            && a.meta
                .require_list()
                .is_ok_and(|l| l.tokens.to_string().contains("test"))
    })
}

/// An impl under an attribute macro (`#[tool_router]`) is shaped by that
/// macro's contract, the way a trait impl is: each method is an independent
/// handler by design.
fn has_attribute_macro(attrs: &[syn::Attribute]) -> bool {
    const BUILTIN: &[&str] = &["cfg", "cfg_attr", "allow", "expect", "warn", "deny", "doc"];
    attrs
        .iter()
        .any(|a| !BUILTIN.iter().any(|b| a.path().is_ident(b)))
}

fn check_items(items: &[Item], opts: Options, out: &mut Vec<Finding>) {
    for item in items {
        match item {
            Item::Mod(m) if !is_cfg_test(&m.attrs) => {
                if let Some((_, inner)) = &m.content {
                    check_items(inner, opts, out);
                }
            }
            Item::Impl(imp)
                if imp.trait_.is_none()
                    && !is_cfg_test(&imp.attrs)
                    && !has_attribute_macro(&imp.attrs) =>
            {
                if let Some(finding) = check_impl(imp, opts) {
                    out.push(finding);
                }
            }
            _ => {}
        }
    }
}

/// Names reached through `self` / `Self` in one method body.
#[derive(Default)]
struct SelfUses(HashSet<String>);

fn is_self_expr(expr: &syn::Expr) -> bool {
    matches!(expr, syn::Expr::Path(p) if p.path.is_ident("self"))
}

impl<'ast> Visit<'ast> for SelfUses {
    fn visit_expr_field(&mut self, node: &'ast ExprField) {
        if is_self_expr(&node.base) {
            if let Member::Named(name) = &node.member {
                self.0.insert(name.to_string());
            }
        }
        visit::visit_expr_field(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        if is_self_expr(&node.receiver) {
            self.0.insert(node.method.to_string());
        }
        visit::visit_expr_method_call(self, node);
    }

    fn visit_expr_path(&mut self, node: &'ast ExprPath) {
        let segments = &node.path.segments;
        if segments.len() == 2 && segments[0].ident == "Self" {
            self.0.insert(segments[1].ident.to_string());
        }
        visit::visit_expr_path(self, node);
    }

    fn visit_macro(&mut self, node: &'ast Macro) {
        scan_tokens(node.tokens.clone(), &mut self.0);
    }
}

/// `self . name` and `Self :: name` sequences in a macro's tokens.
fn scan_tokens(tokens: TokenStream, out: &mut HashSet<String>) {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    for (i, tree) in trees.iter().enumerate() {
        match tree {
            TokenTree::Group(g) => scan_tokens(g.stream(), out),
            TokenTree::Ident(id) if id == "self" => {
                if let (Some(TokenTree::Punct(dot)), Some(TokenTree::Ident(name))) =
                    (trees.get(i + 1), trees.get(i + 2))
                {
                    if dot.as_char() == '.' {
                        out.insert(name.to_string());
                    }
                }
            }
            TokenTree::Ident(id) if id == "Self" => {
                if let (
                    Some(TokenTree::Punct(a)),
                    Some(TokenTree::Punct(b)),
                    Some(TokenTree::Ident(name)),
                ) = (trees.get(i + 1), trees.get(i + 2), trees.get(i + 3))
                {
                    if a.as_char() == ':' && b.as_char() == ':' {
                        out.insert(name.to_string());
                    }
                }
            }
            _ => {}
        }
    }
}

struct Method {
    name: String,
    lines: usize,
    uses: HashSet<String>,
}

#[derive(Default)]
struct Group<'a> {
    lines: usize,
    members: Vec<&'a Method>,
}

fn find(parent: &mut [usize], i: usize) -> usize {
    let mut root = i;
    while parent[root] != root {
        root = parent[root];
    }
    let mut at = i;
    while parent[at] != root {
        let next = parent[at];
        parent[at] = root;
        at = next;
    }
    root
}

fn check_impl(imp: &ItemImpl, opts: Options) -> Option<Finding> {
    let methods: Vec<Method> = imp
        .items
        .iter()
        .filter_map(|item| match item {
            ImplItem::Fn(f) if f.sig.receiver().is_some() && !is_cfg_test(&f.attrs) => {
                let mut uses = SelfUses::default();
                uses.visit_block(&f.block);
                let span = f.span();
                Some(Method {
                    name: f.sig.ident.to_string(),
                    lines: span.end().line.saturating_sub(span.start().line) + 1,
                    uses: uses.0,
                })
            }
            _ => None,
        })
        .collect();
    if methods.len() <= opts.max_groups {
        return None;
    }

    // Nodes are method names plus every name reached through `self`, so two
    // methods that touch the same field end up joined through it.
    let mut index: HashMap<&str, usize> = HashMap::new();
    for m in &methods {
        let next = index.len();
        index.entry(m.name.as_str()).or_insert(next);
        for used in &m.uses {
            let next = index.len();
            index.entry(used.as_str()).or_insert(next);
        }
    }
    let mut parent: Vec<usize> = (0..index.len()).collect();
    for m in &methods {
        let from = index[m.name.as_str()];
        for used in &m.uses {
            let (a, b) = (
                find(&mut parent, from),
                find(&mut parent, index[used.as_str()]),
            );
            parent[a] = b;
        }
    }

    let mut groups: HashMap<usize, Group> = HashMap::new();
    for m in &methods {
        let root = find(&mut parent, index[m.name.as_str()]);
        let group = groups.entry(root).or_default();
        group.lines += m.lines;
        group.members.push(m);
    }
    let mut substantial: Vec<Group> = groups
        .into_values()
        .filter(|g| g.lines >= opts.min_group_lines)
        .collect();
    if substantial.len() <= opts.max_groups {
        return None;
    }
    for group in &mut substantial {
        group
            .members
            .sort_by(|a, b| b.lines.cmp(&a.lines).then(a.name.cmp(&b.name)));
    }
    // Largest group first; ties by name, so the message is stable run to run.
    substantial.sort_by(|a, b| {
        b.lines
            .cmp(&a.lines)
            .then_with(|| a.members[0].name.cmp(&b.members[0].name))
    });

    let described: Vec<String> = substantial
        .iter()
        .map(|g| {
            let names: Vec<&str> = g.members.iter().take(3).map(|m| m.name.as_str()).collect();
            let more = if g.members.len() > 3 { ", …" } else { "" };
            format!("[{}{more}]", names.join(", "))
        })
        .collect();
    let ty = imp.self_ty.span();
    let name = quote_type(&imp.self_ty);
    Some(Finding {
        line: ty.start().line,
        message: format!(
            "`impl {name}` holds {} groups of methods that never use each other: {}. \
             Each group could be its own type.",
            substantial.len(),
            described.join(" · ")
        ),
    })
}

fn quote_type(ty: &syn::Type) -> String {
    match ty {
        syn::Type::Path(p) => p
            .path
            .segments
            .last()
            .map_or_else(|| "?".into(), |s| s.ident.to_string()),
        _ => "?".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPTS: Options = Options {
        max_groups: 1,
        min_group_lines: 3,
    };

    fn body(lines: usize) -> String {
        "        let _v = 0;\n".repeat(lines)
    }

    fn run(src: &str) -> Vec<String> {
        impl_cohesion(src, OPTS)
            .into_iter()
            .map(|f| f.message)
            .collect()
    }

    #[test]
    fn passes_methods_joined_through_a_shared_field() {
        let src = format!(
            "impl Cart {{\n    fn add(&mut self) {{\n{}        self.items.push(1);\n    }}\n    \
             fn count(&self) -> usize {{\n{}        self.items.len()\n    }}\n}}\n",
            body(3),
            body(3)
        );
        assert!(run(&src).is_empty());
    }

    #[test]
    fn flags_methods_in_separate_groups_and_names_them() {
        let src = format!(
            "impl Mixed {{\n    fn add(&mut self) {{\n{}        self.items.push(1);\n    }}\n    \
             fn zoom_in(&mut self) {{\n{}        self.zoom += 1;\n    }}\n}}\n",
            body(3),
            body(3)
        );
        let messages = run(&src);
        assert_eq!(messages.len(), 1);
        assert!(
            messages[0].contains("`impl Mixed` holds 2 groups"),
            "{}",
            messages[0]
        );
        assert!(messages[0].contains("[add]") && messages[0].contains("[zoom_in]"));
    }

    #[test]
    fn reads_self_inside_macro_bodies() {
        let src = format!(
            "impl Label {{\n    fn set(&mut self) {{\n{}        self.text = String::new();\n    }}\n    \
             fn show(&self) -> String {{\n{}        format!(\"{{}}\", self.text)\n    }}\n}}\n",
            body(3),
            body(3)
        );
        assert!(run(&src).is_empty());
    }

    #[test]
    fn ignores_constructors_trait_impls_and_test_code() {
        let two_groups = format!(
            "    fn add(&mut self) {{\n{}        self.items.push(1);\n    }}\n    \
             fn zoom_in(&mut self) {{\n{}        self.zoom += 1;\n    }}\n",
            body(3),
            body(3)
        );
        let with_new = format!(
            "impl Mixed {{\n    fn new() -> Self {{ Self {{ items: vec![], zoom: 1 }} }}\n{two_groups}}}\n"
        );
        assert_eq!(run(&with_new).len(), 1, "the constructor joins nothing");
        assert!(run(&format!("impl Trait for Mixed {{\n{two_groups}}}\n")).is_empty());
        assert!(run(&format!("#[tool_router]\nimpl Mixed {{\n{two_groups}}}\n")).is_empty());
        assert_eq!(
            run(&format!(
                "#[allow(dead_code)]\nimpl Mixed {{\n{two_groups}}}\n"
            ))
            .len(),
            1,
            "a lint attribute is not a macro contract"
        );
        assert!(run(&format!(
            "#[cfg(test)]\nmod tests {{\n    impl Mixed {{\n{two_groups}    }}\n}}\n"
        ))
        .is_empty());
    }

    #[test]
    fn ignores_groups_below_the_size_floor() {
        let src =
            "impl Small {\n    fn a(&self) -> u8 { self.x }\n    fn b(&self) -> u8 { self.y }\n}\n";
        assert!(run(src).is_empty());
    }
}
