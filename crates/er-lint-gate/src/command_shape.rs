//! `er::command_shape` — the return type of a desktop command.
//!
//! A `#[tauri::command]` function in er-desktop returns `Result<T, ErError>`
//! or a type that is not a `Result`, and no path segment of its return type
//! ends in `Snapshot`. The commands that predate the rule are listed by name in
//! `allow`. The list only shrinks: an entry outside the frozen fixture fails,
//! and so does a stale entry, one with no command or whose every body passes.
//!
//! The source is read with syn, so a body under either arm of a `cfg` is
//! checked. A `type` alias declared in the same file is expanded first, so
//! `type R = Result<u8, String>` cannot hide the shape.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use syn::visit::{self, Visit};
use syn::{GenericArgument, PathArguments, ReturnType, Type};

use crate::diagnostics::Diagnostic;

/// The code the gate reports these findings under.
pub const CODE: &str = "er::command_shape";

/// The crate whose commands the rule reads, workspace-relative.
pub const SOURCE: &str = "crates/er-desktop/src";

/// Where the allow-list lives, for the diagnostics about its entries.
const MANIFEST: &str = "Cargo.toml";

/// The names that failed the rule when it landed. The allow-list may only
/// hold names from this set.
const FROZEN: &str = include_str!("../tests/fixtures/command_shape_frozen.txt");

/// Alias expansions followed before giving up, so a cycle cannot hang the gate.
const MAX_ALIAS_DEPTH: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub name: String,
    pub line: usize,
    /// Why the return type fails the rule, or `None` when it passes.
    pub problem: Option<String>,
}

/// The frozen names, one per line in the fixture.
pub fn frozen() -> BTreeSet<&'static str> {
    FROZEN
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect()
}

struct Alias {
    params: Vec<String>,
    ty: Type,
}

#[derive(Default)]
struct AliasFinder {
    aliases: HashMap<String, Alias>,
}

impl<'ast> Visit<'ast> for AliasFinder {
    fn visit_item_type(&mut self, item: &'ast syn::ItemType) {
        let params = item
            .generics
            .params
            .iter()
            .filter_map(|param| match param {
                syn::GenericParam::Type(param) => Some(param.ident.to_string()),
                _ => None,
            })
            .collect();
        self.aliases.insert(
            item.ident.to_string(),
            Alias {
                params,
                ty: (*item.ty).clone(),
            },
        );
        visit::visit_item_type(self, item);
    }
}

struct CommandFinder<'a> {
    aliases: &'a HashMap<String, Alias>,
    commands: Vec<Command>,
}

impl<'ast> Visit<'ast> for CommandFinder<'_> {
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        if item.attrs.iter().any(is_tauri_command) {
            self.commands.push(Command {
                name: item.sig.ident.to_string(),
                line: item.sig.ident.span().start().line,
                problem: shape_problem(&item.sig.output, self.aliases),
            });
        }
        visit::visit_item_fn(self, item);
    }
}

/// `#[tauri::command]`, with or without arguments.
fn is_tauri_command(attr: &syn::Attribute) -> bool {
    let segments: Vec<String> = attr
        .path()
        .segments
        .iter()
        .map(|s| s.ident.to_string())
        .collect();
    segments == ["tauri", "command"]
}

/// Rebuild `ty`, replacing each path type `f` answers for. Children are
/// rebuilt only where `f` declines, since its answer is already final.
fn map_type(ty: &Type, f: &dyn Fn(&syn::Path) -> Option<Type>) -> Type {
    match ty {
        Type::Path(path) if path.qself.is_none() => {
            if let Some(replaced) = f(&path.path) {
                return replaced;
            }
            let mut path = path.clone();
            for segment in &mut path.path.segments {
                if let PathArguments::AngleBracketed(args) = &mut segment.arguments {
                    for arg in &mut args.args {
                        if let GenericArgument::Type(inner) = arg {
                            *inner = map_type(inner, f);
                        }
                    }
                }
            }
            Type::Path(path)
        }
        Type::Paren(inner) => map_type(&inner.elem, f),
        Type::Group(inner) => map_type(&inner.elem, f),
        Type::Reference(reference) => {
            let mut reference = reference.clone();
            reference.elem = Box::new(map_type(&reference.elem, f));
            Type::Reference(reference)
        }
        Type::Slice(slice) => {
            let mut slice = slice.clone();
            slice.elem = Box::new(map_type(&slice.elem, f));
            Type::Slice(slice)
        }
        Type::Array(array) => {
            let mut array = array.clone();
            array.elem = Box::new(map_type(&array.elem, f));
            Type::Array(array)
        }
        Type::Tuple(tuple) => {
            let mut tuple = tuple.clone();
            for elem in &mut tuple.elems {
                *elem = map_type(elem, f);
            }
            Type::Tuple(tuple)
        }
        other => other.clone(),
    }
}

/// The type arguments of a path segment, lifetimes and bindings left out.
fn type_args(segment: &syn::PathSegment) -> Vec<&Type> {
    match &segment.arguments {
        PathArguments::AngleBracketed(args) => args
            .args
            .iter()
            .filter_map(|arg| match arg {
                GenericArgument::Type(ty) => Some(ty),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// `ty` with every same-file alias expanded, at any depth, its generic
/// parameters replaced by the arguments at the use site.
fn resolve(ty: &Type, aliases: &HashMap<String, Alias>, depth: usize) -> Type {
    map_type(ty, &|path| {
        if depth >= MAX_ALIAS_DEPTH || path.segments.len() != 1 {
            return None;
        }
        let segment = path.segments.first()?;
        let alias = aliases.get(&segment.ident.to_string())?;
        let bound: HashMap<&str, Type> = alias
            .params
            .iter()
            .map(String::as_str)
            .zip(type_args(segment).into_iter().cloned())
            .collect();
        let expanded = map_type(&alias.ty, &|inner| {
            let ident = inner.get_ident()?.to_string();
            bound.get(ident.as_str()).cloned()
        });
        Some(resolve(&expanded, aliases, depth + 1))
    })
}

#[derive(Default)]
struct SnapshotSegment {
    found: Option<String>,
}

impl<'ast> Visit<'ast> for SnapshotSegment {
    fn visit_path_segment(&mut self, segment: &'ast syn::PathSegment) {
        let name = segment.ident.to_string();
        if self.found.is_none() && name.ends_with("Snapshot") {
            self.found = Some(name);
        }
        visit::visit_path_segment(self, segment);
    }
}

fn is_er_error(ty: &Type) -> bool {
    matches!(ty, Type::Path(path)
        if path.path.segments.last().is_some_and(|segment| segment.ident == "ErError"))
}

/// Why a command's return type fails the rule, or `None` when it passes.
fn shape_problem(output: &ReturnType, aliases: &HashMap<String, Alias>) -> Option<String> {
    let ReturnType::Type(_, ty) = output else {
        return None;
    };
    let ty = resolve(ty, aliases, 0);
    let mut snapshot = SnapshotSegment::default();
    snapshot.visit_type(&ty);
    if let Some(name) = snapshot.found {
        return Some(format!("names `{name}` in its return type"));
    }
    let Type::Path(path) = &ty else {
        return None;
    };
    let last = path.path.segments.last()?;
    if last.ident != "Result" {
        return None;
    }
    match type_args(last).as_slice() {
        [_, error] if is_er_error(error) => None,
        _ => Some("returns a `Result` whose error is not `ErError`".into()),
    }
}

/// Every `#[tauri::command]` function in one source file, with its shape.
/// A file that does not parse yields none.
pub fn commands(source: &str) -> Vec<Command> {
    let Ok(file) = syn::parse_file(source) else {
        return Vec::new();
    };
    let mut aliases = AliasFinder::default();
    aliases.visit_file(&file);
    let mut finder = CommandFinder {
        aliases: &aliases.aliases,
        commands: Vec::new(),
    };
    finder.visit_file(&file);
    finder.commands
}

/// Turn the commands into gate diagnostics. A failing command whose name is
/// on `allow` passes. An entry outside `frozen` fails, and so does a stale
/// one: a name with no command, or whose every body passes.
pub fn evaluate(
    commands_by_file: &BTreeMap<String, Vec<Command>>,
    allow: &[String],
    frozen: &BTreeSet<&str>,
    code: &str,
) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (file, commands) in commands_by_file {
        for command in commands {
            let Some(problem) = &command.problem else {
                continue;
            };
            if allow.contains(&command.name) {
                continue;
            }
            let message = format!(
                "`{}` {problem}: a command returns `Result<T, ErError>` or a type that is \
                 not a `Result`, and no path segment of its return type ends in `Snapshot`",
                command.name
            );
            out.push(error(file, command.line, code, &message));
        }
    }
    for entry in allow {
        if !frozen.contains(entry.as_str()) {
            out.push(error(
                MANIFEST,
                1,
                code,
                &format!(
                    "command-shape entry `{entry}` is not in the frozen list: the list only \
                     shrinks, so a new command takes the new shape"
                ),
            ));
        }
        let bodies: Vec<&Command> = commands_by_file
            .values()
            .flatten()
            .filter(|command| &command.name == entry)
            .collect();
        if bodies.is_empty() {
            out.push(error(
                MANIFEST,
                1,
                code,
                &format!("stale allow-list entry: no command is named `{entry}`; remove it"),
            ));
        } else if bodies.iter().all(|command| command.problem.is_none()) {
            out.push(error(
                MANIFEST,
                1,
                code,
                &format!("stale allow-list entry: every `{entry}` passes the rule; remove it"),
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

    const FILE: &str = "crates/er-desktop/src/commands.rs";

    fn by_file(source: &str) -> BTreeMap<String, Vec<Command>> {
        BTreeMap::from([(FILE.to_string(), commands(source))])
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(ToString::to_string).collect()
    }

    fn diagnose(source: &str, allow: &[&str], frozen: &[&str]) -> Vec<Diagnostic> {
        let frozen: BTreeSet<&str> = frozen.iter().copied().collect();
        evaluate(&by_file(source), &names(allow), &frozen, CODE)
    }

    fn assert_one_failure(source: &str) {
        let diags = diagnose(source, &[], &[]);
        assert_eq!(diags.len(), 1, "{source}: {diags:?}");
        assert_eq!(diags[0].code.as_deref(), Some(CODE));
        assert_eq!(diags[0].file.as_deref(), Some(FILE));
        assert!(diags[0].rendered.contains("`c`"), "{}", diags[0].rendered);
    }

    #[test]
    fn a_snapshot_result_outside_the_list_fails() {
        assert_one_failure("#[tauri::command] fn c() -> Result<AppSnapshot, String> { x() }");
    }

    #[test]
    fn a_string_error_fails() {
        assert_one_failure("#[tauri::command] fn c() -> Result<u8, String> { x() }");
        assert_one_failure("#[tauri::command] async fn c() -> anyhow::Result<u8> { x() }");
    }

    #[test]
    fn an_alias_to_a_string_result_fails() {
        assert_one_failure("type R = Result<u8, String>;\n#[tauri::command] fn c() -> R { x() }");
        assert_one_failure(
            "type R<E> = Result<u8, E>;\n#[tauri::command] fn c() -> R<String> { x() }",
        );
        assert_one_failure(
            "type S = AppSnapshot;\n#[tauri::command] fn c() -> Result<S, ErError> { x() }",
        );
    }

    #[test]
    fn an_alias_to_the_new_shape_passes() {
        let source = "type R<T> = Result<T, ErError>;\n#[tauri::command] fn c() -> R<u8> { x() }";
        assert!(diagnose(source, &[], &[]).is_empty());
    }

    #[test]
    fn a_snapshot_segment_fails_whatever_the_error() {
        for source in [
            "#[tauri::command] fn c() -> Result<FileSnapshot, ErError> { x() }",
            "#[tauri::command] fn c() -> Result<Vec<FileSnapshot>, ErError> { x() }",
            "#[tauri::command] fn c() -> AppSnapshot { x() }",
            "#[tauri::command] fn c() -> crate::snapshot::AppSnapshot { x() }",
        ] {
            assert_one_failure(source);
        }
    }

    #[test]
    fn a_flag_only_command_fails() {
        assert_one_failure(
            "#[cfg(feature = \"v1-service\")]\n#[tauri::command] fn c() -> Result<u8, String> { x() }",
        );
    }

    #[test]
    fn both_cfg_bodies_are_read() {
        let source = "#[cfg(feature = \"v1-service\")]\n#[tauri::command] fn c() -> Result<u8, ErError> { x() }\n\
                      #[cfg(not(feature = \"v1-service\"))]\n#[tauri::command] fn c() -> Result<u8, String> { x() }";
        let found = commands(source);
        assert_eq!(found.len(), 2);
        assert_eq!(diagnose(source, &[], &[]).len(), 1);
    }

    #[test]
    fn the_new_shapes_pass() {
        for source in [
            "#[tauri::command] fn c() -> Result<u8, ErError> { x() }",
            "#[tauri::command] fn c() -> Result<Vec<String>, er_protocol::ErError> { x() }",
            "#[tauri::command] fn c() -> bool { x() }",
            "#[tauri::command] fn c() -> Option<Vec<String>> { x() }",
            "#[tauri::command] fn c() {}",
            "#[tauri::command] fn c() -> Result<ArenaRunSnapshotWire, ErError> { x() }",
        ] {
            assert!(diagnose(source, &[], &[]).is_empty(), "{source}");
        }
    }

    #[test]
    fn only_tauri_commands_are_read() {
        assert!(commands("fn c() -> Result<u8, String> { x() }").is_empty());
        assert!(commands("#[other::command] fn c() -> Result<u8, String> { x() }").is_empty());
        for source in [
            "#[tauri::command(rename_all = \"snake_case\")] fn c() -> Result<u8, String> { x() }",
            "mod inner { #[tauri::command] pub fn c() -> Result<u8, String> { x() } }",
        ] {
            assert_one_failure(source);
        }
    }

    #[test]
    fn a_listed_command_passes() {
        let source = "#[tauri::command] fn c() -> Result<AppSnapshot, String> { x() }";
        assert!(diagnose(source, &["c"], &["c"]).is_empty());
    }

    #[test]
    fn an_entry_outside_the_frozen_list_fails() {
        let source = "#[tauri::command] fn c() -> Result<AppSnapshot, String> { x() }";
        let diags = diagnose(source, &["c"], &["other"]);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].rendered.contains("not in the frozen list"));
        assert_eq!(diags[0].file.as_deref(), Some(MANIFEST));
    }

    #[test]
    fn an_entry_whose_every_body_passes_is_stale() {
        let source = "#[tauri::command] fn c() -> Result<u8, ErError> { x() }";
        let diags = diagnose(source, &["c"], &["c"]);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].rendered.contains("stale allow-list entry"));
        assert!(diags[0].rendered.contains("`c`"));
    }

    #[test]
    fn an_entry_with_no_command_is_stale() {
        let diags = diagnose("fn c() {}", &["gone"], &["gone"]);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].rendered.contains("stale allow-list entry"));
        assert!(diags[0].rendered.contains("`gone`"));
    }

    #[test]
    fn an_entry_stays_live_while_one_body_fails() {
        let source = "#[cfg(feature = \"v1-service\")]\n#[tauri::command] fn c() -> Result<u8, ErError> { x() }\n\
                      #[cfg(not(feature = \"v1-service\"))]\n#[tauri::command] fn c() -> Result<u8, String> { x() }";
        assert!(diagnose(source, &["c"], &["c"]).is_empty());
    }

    #[test]
    fn the_frozen_fixture_parses_to_names() {
        let frozen = frozen();
        assert!(!frozen.is_empty());
        assert!(frozen
            .iter()
            .all(|name| !name.contains(char::is_whitespace)));
    }
}
