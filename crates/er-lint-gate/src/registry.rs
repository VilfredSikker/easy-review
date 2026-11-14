//! `er::registry` — a stored kind that no front end registers, or a
//! registered kind nothing reads.
//!
//! The rule scans every crate source (test items are exempt). It collects
//! three facts: `impl Artifact for T` with its `KIND`/`FILE` literals, the
//! typed registrations (`register::<T>()`, `register_text(kind, file)`), and
//! the typed store calls (`ArtifactKey::<T>`, `key::<T>`, `read_found::<T>`,
//! …). A type counts as read when it appears in a function that also makes a
//! store read call, because most keys are bound to a local before use.
//!
//! There is no allow list: a kind the rule cannot see is fixed in the
//! composition, not exempted here.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Expr, Item, Lit, PathArguments, Type};

use crate::attrs::{is_test_item, item_attrs};
use crate::diagnostics::Diagnostic;

pub const CODE: &str = "er::registry";

/// The call names whose turbofish names a stored kind.
const TYPED_CALLS: &[&str] = &[
    "ArtifactKey",
    "app_key",
    "app_path",
    "delete",
    "key",
    "list",
    "read",
    "read_found",
    "snapshot",
    "update",
    "write",
    "write_durable",
];

/// Method names that make their enclosing function a reader.
const READ_CALLS: &[&str] = &[
    "read",
    "read_found",
    "read_optional",
    "read_text",
    "snapshot",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KindDef {
    pub type_name: String,
    pub kind: String,
    pub file: String,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registration {
    pub type_name: Option<String>,
    pub text_file: Option<String>,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedUse {
    pub type_name: String,
    pub read: bool,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextUse {
    pub file: String,
    pub line: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileFacts {
    pub definitions: Vec<KindDef>,
    pub registrations: Vec<Registration>,
    pub typed_uses: Vec<TypedUse>,
    pub text_uses: Vec<TextUse>,
}

/// Scan one source file.
pub fn findings(source: &str) -> FileFacts {
    let Ok(file) = syn::parse_file(source) else {
        return FileFacts::default();
    };
    let mut scanner = Scanner {
        facts: FileFacts::default(),
        reader_scope: false,
        impl_type: None,
    };
    scanner.visit_file(&file);
    scanner.facts
}

struct Scanner {
    facts: FileFacts,
    reader_scope: bool,
    impl_type: Option<String>,
}

impl<'ast> Visit<'ast> for Scanner {
    fn visit_item(&mut self, item: &'ast Item) {
        if item_attrs(item).is_some_and(is_test_item) {
            return;
        }
        visit::visit_item(self, item);
    }

    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        let previous = self.impl_type.clone();
        if let Some(def) = artifact_definition(item) {
            self.impl_type = Some(def.type_name.clone());
            self.facts.definitions.push(def);
        } else if let Type::Path(self_type) = &*item.self_ty {
            self.impl_type = self_type
                .path
                .segments
                .last()
                .map(|segment| segment.ident.to_string());
        }
        visit::visit_item_impl(self, item);
        self.impl_type = previous;
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        let previous = self.reader_scope;
        if body_has_read_call(&item.block) {
            self.reader_scope = true;
        }
        self.facts.text_uses.extend(text_uses_in_block(&item.block));
        visit::visit_item_fn(self, item);
        self.reader_scope = previous;
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        let previous = self.reader_scope;
        if body_has_read_call(&item.block) {
            self.reader_scope = true;
        }
        self.facts.text_uses.extend(text_uses_in_block(&item.block));
        visit::visit_impl_item_fn(self, item);
        self.reader_scope = previous;
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        let method = call.method.to_string();
        if method == "register" {
            if let Some(type_name) = turbofish_type(call.turbofish.as_ref()) {
                self.facts.registrations.push(Registration {
                    type_name: Some(type_name),
                    text_file: None,
                    line: call.span().start().line,
                });
            }
        } else if method == "register_text" {
            if let Some(file) = str_literal(call.args.get(1)) {
                self.facts.registrations.push(Registration {
                    type_name: None,
                    text_file: Some(file),
                    line: call.span().start().line,
                });
            }
        }
        visit::visit_expr_method_call(self, call);
    }

    fn visit_type_path(&mut self, path: &'ast syn::TypePath) {
        if self.reader_scope {
            if let Some(segment) = path.path.segments.last() {
                let name = if segment.ident == "Self" {
                    self.impl_type.clone()
                } else {
                    Some(segment.ident.to_string())
                };
                if let Some(type_name) = name {
                    self.facts.typed_uses.push(TypedUse {
                        type_name,
                        read: true,
                        line: segment.ident.span().start().line,
                    });
                }
            }
        }
        visit::visit_type_path(self, path);
    }

    fn visit_expr_path(&mut self, path: &'ast syn::ExprPath) {
        for segment in &path.path.segments {
            if !TYPED_CALLS.iter().any(|name| segment.ident == name) {
                continue;
            }
            if let Some(type_name) = segment_type(&segment.arguments) {
                self.facts.typed_uses.push(TypedUse {
                    type_name,
                    read: self.reader_scope,
                    line: segment.ident.span().start().line,
                });
            }
        }
        visit::visit_expr_path(self, path);
    }
}

/// `impl Artifact for T { const KIND = "…"; const FILE = "…"; }`.
fn artifact_definition(item: &syn::ItemImpl) -> Option<KindDef> {
    let (trait_path, _) = item.trait_.as_ref()?;
    if trait_path.segments.last()?.ident != "Artifact" {
        return None;
    }
    let Type::Path(self_type) = &*item.self_ty else {
        return None;
    };
    let type_name = self_type.path.segments.last()?.ident.to_string();
    let mut kind = None;
    let mut file = None;
    for impl_item in &item.items {
        let syn::ImplItem::Const(constant) = impl_item else {
            continue;
        };
        let value = match &constant.expr {
            Expr::Lit(literal) => match &literal.lit {
                Lit::Str(text) => Some(text.value()),
                _ => None,
            },
            _ => None,
        };
        match constant.ident.to_string().as_str() {
            "KIND" => kind = value,
            "FILE" => file = value,
            _ => {}
        }
    }
    Some(KindDef {
        type_name,
        kind: kind?,
        file: file?,
        line: item.span().start().line,
    })
}

/// The single type in `::<T>` / `<T>`, if there is exactly one and it is a
/// plain path type.
fn turbofish_type(arguments: Option<&syn::AngleBracketedGenericArguments>) -> Option<String> {
    let arguments = arguments?;
    let mut types = arguments.args.iter().filter_map(|argument| match argument {
        syn::GenericArgument::Type(Type::Path(path)) => Some(path),
        _ => None,
    });
    let first = types.next()?;
    if types.next().is_some() {
        return None;
    }
    Some(first.path.segments.last()?.ident.to_string())
}

fn segment_type(arguments: &PathArguments) -> Option<String> {
    match arguments {
        PathArguments::AngleBracketed(arguments) => turbofish_type(Some(arguments)),
        PathArguments::None | PathArguments::Parenthesized(_) => None,
    }
}

/// Text artifacts count as used only when a `RelPath` literal reaches a
/// `read_text`/`write_text` call, directly or through a local binding. A
/// `delete_text` over a host file is cleanup, not a use.
fn text_uses_in_block(block: &syn::Block) -> Vec<TextUse> {
    struct Collector {
        bindings: HashMap<String, (String, usize)>,
        uses: Vec<TextUse>,
    }
    impl Collector {
        fn record_argument(&mut self, argument: &Expr) {
            if let Some(literal) = rel_path_literal(argument) {
                self.uses.push(TextUse {
                    file: literal,
                    line: argument.span().start().line,
                });
            } else if let Some(ident) = argument_ident(argument) {
                if let Some((literal, line)) = self.bindings.get(&ident) {
                    self.uses.push(TextUse {
                        file: literal.clone(),
                        line: *line,
                    });
                }
            }
        }
    }
    impl<'ast> Visit<'ast> for Collector {
        fn visit_local(&mut self, local: &'ast syn::Local) {
            if let Some(ident) = bound_ident(&local.pat) {
                if let Some(init) = &local.init {
                    if let Some(literal) = rel_path_literal(&init.expr) {
                        self.bindings
                            .insert(ident, (literal, local.span().start().line));
                    }
                }
            }
            visit::visit_local(self, local);
        }

        fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
            let method = call.method.to_string();
            if method.contains("read_text") || method.contains("write_text") {
                for argument in &call.args {
                    self.record_argument(argument);
                }
            }
            visit::visit_expr_method_call(self, call);
        }
    }
    let mut collector = Collector {
        bindings: HashMap::new(),
        uses: Vec::new(),
    };
    collector.visit_block(block);
    collector.uses
}

fn rel_path_literal(expression: &Expr) -> Option<String> {
    match expression {
        Expr::Call(call) => {
            let Expr::Path(path) = &*call.func else {
                return None;
            };
            let segments: Vec<_> = path.path.segments.iter().collect();
            if segments.len() >= 2
                && segments[segments.len() - 2].ident == "RelPath"
                && segments[segments.len() - 1].ident == "new"
            {
                return str_literal(call.args.first());
            }
            None
        }
        Expr::Try(inner) => rel_path_literal(&inner.expr),
        Expr::Reference(inner) => rel_path_literal(&inner.expr),
        Expr::MethodCall(call)
            if matches!(call.method.to_string().as_str(), "unwrap" | "expect") =>
        {
            rel_path_literal(&call.receiver)
        }
        _ => None,
    }
}

fn bound_ident(pattern: &syn::Pat) -> Option<String> {
    match pattern {
        syn::Pat::Ident(ident) => Some(ident.ident.to_string()),
        syn::Pat::TupleStruct(tuple) => tuple.elems.iter().find_map(bound_ident),
        _ => None,
    }
}

fn argument_ident(expression: &Expr) -> Option<String> {
    match expression {
        Expr::Reference(inner) => argument_ident(&inner.expr),
        Expr::Path(path) if path.path.segments.len() == 1 => {
            Some(path.path.segments[0].ident.to_string())
        }
        _ => None,
    }
}

fn str_literal(expression: Option<&Expr>) -> Option<String> {
    let Expr::Lit(literal) = expression? else {
        return None;
    };
    match &literal.lit {
        Lit::Str(text) => Some(text.value()),
        _ => None,
    }
}

/// Whether the body contains a store read call. Keys are usually bound to a
/// local first, so the read marks the whole function.
fn body_has_read_call(block: &syn::Block) -> bool {
    struct ReadFinder {
        found: bool,
    }
    impl<'ast> Visit<'ast> for ReadFinder {
        fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
            if READ_CALLS.iter().any(|name| call.method == name) {
                self.found = true;
            }
            visit::visit_expr_method_call(self, call);
        }
        fn visit_expr_path(&mut self, path: &'ast syn::ExprPath) {
            if let Some(last) = path.path.segments.last() {
                if READ_CALLS.iter().any(|name| last.ident == name) {
                    self.found = true;
                }
            }
            visit::visit_expr_path(self, path);
        }
    }
    let mut finder = ReadFinder { found: false };
    finder.visit_block(block);
    finder.found
}

/// The workspace-level conditions. `by_file` maps workspace-relative paths to
/// their facts.
pub fn evaluate(by_file: &BTreeMap<String, FileFacts>) -> Vec<Diagnostic> {
    let mut definitions: BTreeMap<&str, (&str, &str)> = BTreeMap::new();
    for facts in by_file.values() {
        for def in &facts.definitions {
            definitions.insert(
                def.type_name.as_str(),
                (def.kind.as_str(), def.file.as_str()),
            );
        }
    }
    let registered: BTreeSet<&str> = by_file
        .values()
        .flat_map(|facts| facts.registrations.iter())
        .filter_map(|registration| registration.type_name.as_deref())
        .collect();
    let text_registered: BTreeSet<&str> = by_file
        .values()
        .flat_map(|facts| facts.registrations.iter())
        .filter_map(|registration| registration.text_file.as_deref())
        .collect();
    let read_types: BTreeSet<&str> = by_file
        .values()
        .flat_map(|facts| facts.typed_uses.iter())
        .filter(|typed_use| typed_use.read)
        .map(|typed_use| typed_use.type_name.as_str())
        .collect();

    let mut out = Vec::new();
    let mut seen: BTreeSet<(String, usize, String)> = BTreeSet::new();
    let mut push = |diagnostic: Diagnostic, file: &str, line: usize, message: String| {
        if seen.insert((file.to_string(), line, message)) {
            out.push(diagnostic);
        }
    };
    for (file, facts) in by_file {
        for typed_use in &facts.typed_uses {
            let Some((kind, def_file)) = definitions.get(typed_use.type_name.as_str()) else {
                continue;
            };
            if !registered.contains(typed_use.type_name.as_str()) {
                let message = format!(
                    "kind `{}` (`{kind}`, {def_file}) is used here but never registered",
                    typed_use.type_name
                );
                push(
                    diagnostic(file, typed_use.line, message.clone()),
                    file,
                    typed_use.line,
                    message,
                );
            }
        }
        for registration in &facts.registrations {
            let Some(type_name) = registration.type_name.as_deref() else {
                continue;
            };
            if definitions.contains_key(type_name) && !read_types.contains(type_name) {
                let message = format!("registered kind `{type_name}` has no reader");
                push(
                    diagnostic(file, registration.line, message.clone()),
                    file,
                    registration.line,
                    message,
                );
            }
        }
        for text_use in &facts.text_uses {
            if !text_registered.contains(text_use.file.as_str()) {
                let message = format!(
                    "text artifact `{}` is read or written but never registered",
                    text_use.file
                );
                push(
                    diagnostic(file, text_use.line, message.clone()),
                    file,
                    text_use.line,
                    message,
                );
            }
        }
    }
    out
}

fn diagnostic(file: &str, line: usize, message: String) -> Diagnostic {
    Diagnostic {
        level: "warning".into(),
        code: Some(CODE.into()),
        rendered: format!("warning: {message}\n  --> {file}:{line}\n\n"),
        file: Some(file.to_string()),
        line,
        column: 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(sources: &[(&str, &str)]) -> BTreeMap<String, FileFacts> {
        sources
            .iter()
            .map(|(file, source)| ((*file).to_string(), findings(source)))
            .collect()
    }

    const DEFINITION: &str = r#"
impl er_store::Artifact for Widget {
    const KIND: &'static str = "widget";
    const VERSION: u32 = 1;
    const FILE: &'static str = "widget.json";
    type Slot = ();
}
"#;

    #[test]
    fn a_used_kind_must_be_registered() {
        let source = format!(
            "{DEFINITION}\nfn read_it(store: &er_store::ArtifactStore) {{\n    let key = er_store::ArtifactKey::<Widget>::new(scope, ()).unwrap();\n    let _ = store.read(&key);\n}}\n"
        );
        let diagnostics = evaluate(&scope(&[("crates/er-engine/src/a.rs", &source)]));
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(diagnostics[0]
            .rendered
            .contains("used here but never registered"));
    }

    #[test]
    fn a_registered_kind_must_have_a_reader() {
        let source = format!(
            "{DEFINITION}\nfn register_it(registry: &mut er_store::Registry) {{\n    registry.register::<Widget>().unwrap();\n}}\n"
        );
        let diagnostics = evaluate(&scope(&[("crates/er-engine/src/a.rs", &source)]));
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(diagnostics[0].rendered.contains("has no reader"));
    }

    #[test]
    fn a_registered_and_read_kind_passes() {
        let source = format!(
            "{DEFINITION}\nfn register_it(registry: &mut er_store::Registry) {{\n    registry.register::<Widget>().unwrap();\n}}\nfn read_it(store: &er_store::ArtifactStore) {{\n    let key = er_store::ArtifactKey::<Widget>::new(scope, ()).unwrap();\n    let _ = store.read(&key);\n}}\n"
        );
        assert!(evaluate(&scope(&[("crates/er-engine/src/a.rs", &source)])).is_empty());
    }

    #[test]
    fn a_text_artifact_must_be_registered() {
        let source = r#"
fn read_summary(store: &er_store::ArtifactStore) {
    let file = er_store::RelPath::new("summary.md").unwrap();
    let _ = store.read_text(&scope, &file);
}
"#;
        let diagnostics = evaluate(&scope(&[("crates/er-engine/src/a.rs", source)]));
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(diagnostics[0].rendered.contains("never registered"));
    }

    #[test]
    fn a_registered_text_artifact_passes() {
        let source = r#"
fn register(registry: &mut er_store::Registry) {
    registry.register_text("summary", "summary.md").unwrap();
}
fn read_summary(store: &er_store::ArtifactStore) {
    let file = er_store::RelPath::new("summary.md").unwrap();
    let _ = store.read_text(&scope, &file);
}
"#;
        assert!(evaluate(&scope(&[("crates/er-engine/src/a.rs", source)])).is_empty());
    }

    #[test]
    fn test_items_are_exempt() {
        let source = format!(
            "{DEFINITION}\n#[cfg(test)]\nmod tests {{\n    fn read_it(store: &er_store::ArtifactStore) {{\n        let key = er_store::ArtifactKey::<Widget>::new(scope, ()).unwrap();\n        let _ = store.read(&key);\n    }}\n}}\n"
        );
        assert!(evaluate(&scope(&[("crates/er-engine/src/a.rs", &source)])).is_empty());
    }
}
