//! Cyclomatic complexity analysis for Rust source files.
//!
//! The analyzer is a [`syn`] AST walker that scores every top-level
//! function, inherent/trait method, and trait default method in a file.
//! The definition is deliberately McCabe-classic and documented so the
//! numbers are reproducible:
//!
//! - base score 1 for every function;
//! - `+1` per `if` / `else if` / `if let` (each condition, including
//!   `else if` chains);
//! - `+1` per `for`, `while` / `while let`, and `loop`;
//! - `+1` per `match` arm (including the `_` arm);
//! - `+1` per short-circuit boolean `&&` / `||`;
//! - `?`, `async`, and `unsafe` blocks add nothing;
//! - decisions inside closures count toward the enclosing function;
//! - a nested `fn` item inside a function body gets its own entry and its
//!   decisions do not count toward the outer function.
//!
//! Files that fail to parse are skipped (they cannot be scored); callers
//! can detect this by comparing the returned count with expectation.

use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{
    BinOp, ExprBinary, ExprForLoop, ExprIf, ExprLoop, ExprMatch, ExprWhile, ImplItemFn, ItemFn,
    ItemImpl, TraitItemFn,
};

/// Complexity of one function, with its source location for coverage lookup.
#[derive(Debug, Clone)]
pub struct FnComplexity {
    /// Readable name: method names are prefixed with their impl type
    /// (e.g. `TabState::refresh_diff`); trait defaults use the bare name.
    pub name: String,
    /// 1-based line of the `fn` item start (used to match LCOV coverage).
    pub start_line: usize,
    /// 1-based line of the item end (the closing brace, usually).
    pub end_line: usize,
    /// Cyclomatic complexity per the definition above.
    pub complexity: usize,
}

/// Analyze a Rust source file and return one entry per scored function.
/// Unparseable files yield an empty vector.
pub fn analyze_file(source: &str) -> Vec<FnComplexity> {
    let Ok(file) = syn::parse_file(source) else {
        return Vec::new();
    };
    let mut visitor = Visitor::default();
    visitor.visit_file(&file);
    visitor.frames
}

#[derive(Default)]
struct Visitor {
    frames: Vec<FnComplexity>,
    /// Indices into `frames` for function bodies currently being walked;
    /// decision nodes bump the innermost (last) open frame.
    open: Vec<usize>,
    /// Current impl self type name (for method display names).
    current_impl: Option<String>,
}

impl Visitor {
    fn bump(&mut self, amount: usize) {
        if let Some(&idx) = self.open.last() {
            self.frames[idx].complexity += amount;
        }
    }

    fn push_fn(&mut self, name: String, node_span: proc_macro2::Span) {
        self.frames.push(FnComplexity {
            name,
            start_line: node_span.start().line,
            end_line: node_span.end().line,
            complexity: 1,
        });
        self.open.push(self.frames.len() - 1);
    }

    fn pop_fn(&mut self) {
        self.open.pop();
    }
}

impl<'ast> Visit<'ast> for Visitor {
    fn visit_item_fn(&mut self, i: &'ast ItemFn) {
        self.push_fn(i.sig.ident.to_string(), i.span());
        visit::visit_item_fn(self, i);
        self.pop_fn();
    }

    fn visit_item_impl(&mut self, i: &'ast ItemImpl) {
        let prev = self.current_impl.take();
        self.current_impl = Some(self_type_name(&i.self_ty));
        visit::visit_item_impl(self, i);
        self.current_impl = prev;
    }

    fn visit_impl_item_fn(&mut self, i: &'ast ImplItemFn) {
        let name = match &self.current_impl {
            Some(ty) => format!("{ty}::{}", i.sig.ident),
            None => i.sig.ident.to_string(),
        };
        self.push_fn(name, i.span());
        visit::visit_impl_item_fn(self, i);
        self.pop_fn();
    }

    fn visit_trait_item_fn(&mut self, i: &'ast TraitItemFn) {
        self.push_fn(i.sig.ident.to_string(), i.span());
        visit::visit_trait_item_fn(self, i);
        self.pop_fn();
    }

    fn visit_expr_if(&mut self, i: &'ast ExprIf) {
        self.bump(1);
        visit::visit_expr_if(self, i);
    }

    fn visit_expr_for_loop(&mut self, i: &'ast ExprForLoop) {
        self.bump(1);
        visit::visit_expr_for_loop(self, i);
    }

    fn visit_expr_while(&mut self, i: &'ast ExprWhile) {
        self.bump(1);
        visit::visit_expr_while(self, i);
    }

    fn visit_expr_loop(&mut self, i: &'ast ExprLoop) {
        self.bump(1);
        visit::visit_expr_loop(self, i);
    }

    fn visit_expr_match(&mut self, i: &'ast ExprMatch) {
        self.bump(i.arms.len());
        visit::visit_expr_match(self, i);
    }

    fn visit_expr_binary(&mut self, i: &'ast ExprBinary) {
        if matches!(i.op, BinOp::And(_) | BinOp::Or(_)) {
            self.bump(1);
        }
        visit::visit_expr_binary(self, i);
    }
}

/// Best-effort human name for an impl self type (e.g. `TabState` for
/// `impl TabState`). Falls back to `impl` for exotic self types.
fn self_type_name(ty: &syn::Type) -> String {
    if let syn::Type::Path(p) = ty {
        if let Some(seg) = p.path.segments.last() {
            return seg.ident.to_string();
        }
    }
    "impl".to_string()
}
#[cfg(test)]
mod tests {
    use super::*;

    /// Score the single function in `src`; panics if there isn't exactly one.
    fn cc(src: &str) -> usize {
        let fns = analyze_file(src);
        assert_eq!(fns.len(), 1, "expected exactly one function in: {src}");
        fns[0].complexity
    }

    #[test]
    fn trivial_function_is_one() {
        assert_eq!(cc("fn f() {}"), 1);
    }

    #[test]
    fn single_if_adds_one() {
        assert_eq!(cc("fn f(x: bool) { if x {} }"), 2);
    }

    #[test]
    fn if_let_counts_as_if() {
        assert_eq!(
            cc("fn f(o: Option<i32>) { if let Some(x) = o { let _ = x; } }"),
            2
        );
    }

    #[test]
    fn else_if_chain_counts_each_condition() {
        assert_eq!(
            cc("fn f(x: i32) { if x == 0 {} else if x > 0 {} else {} }"),
            3
        );
    }

    #[test]
    fn match_arms_are_decision_points() {
        assert_eq!(
            cc("fn f(x: u8) { match x { 0 => {}, 1 => {}, _ => {} } }"),
            4
        );
    }

    #[test]
    fn loops_count() {
        assert_eq!(
            cc("fn f(n: usize) { for _ in 0..n {} while n > 0 {} loop { break; } }"),
            4
        );
    }

    #[test]
    fn while_let_counts_as_while() {
        assert_eq!(cc("fn f(it: &mut dyn Iterator<Item = i32>) { while let Some(x) = it.next() { let _ = x; } }"), 2);
    }

    #[test]
    fn short_circuit_operators_count() {
        assert_eq!(
            cc("fn f(a: bool, b: bool) { if a && b {} if a || b {} }"),
            5
        );
    }

    #[test]
    fn question_mark_is_not_a_decision_point() {
        assert_eq!(cc("fn f() -> Option<i32> { Some(1)?; None }"), 1);
    }

    #[test]
    fn closure_decisions_count_into_the_enclosing_function() {
        assert_eq!(
            cc("fn f(xs: &[i32]) { xs.iter().for_each(|&x| if x > 0 {}); }"),
            2
        );
    }

    #[test]
    fn nested_fn_items_get_their_own_entry() {
        let fns = analyze_file("fn outer() { fn inner(x: bool) { if x {} } }");
        assert_eq!(fns.len(), 2, "outer and inner should both be scored");
        assert_eq!(fns[0].name, "outer");
        assert_eq!(fns[0].complexity, 1, "inner's if must not leak into outer");
        assert_eq!(fns[1].name, "inner");
        assert_eq!(fns[1].complexity, 2);
    }

    #[test]
    fn impl_methods_are_prefixed_with_their_type() {
        let fns = analyze_file("struct S; impl S { fn m(&self) { if true {} } }");
        assert_eq!(fns.len(), 1);
        assert_eq!(fns[0].name, "S::m");
        assert_eq!(fns[0].complexity, 2);
    }

    #[test]
    fn trait_default_methods_are_scored() {
        let fns =
            analyze_file("trait T { fn m(&self) -> bool { if true { true } else { false } } }");
        assert_eq!(fns.len(), 1);
        assert_eq!(fns[0].name, "m");
        assert_eq!(fns[0].complexity, 2);
    }

    #[test]
    fn line_numbers_are_reported() {
        let fns = analyze_file("// c1\n// c2\nfn f() {\n    if true {}\n}\n");
        assert_eq!(fns.len(), 1);
        assert_eq!(fns[0].start_line, 3);
        assert_eq!(fns[0].end_line, 5);
    }

    #[test]
    fn unparseable_file_yields_no_functions() {
        assert!(analyze_file("fn broken( {").is_empty());
    }

    #[test]
    fn module_level_conditionals_are_not_scored_as_functions() {
        // A const-evaluated block at module level has no function to own it.
        let fns = analyze_file("const _: () = { if true {} };");
        assert!(fns.is_empty());
    }

    #[test]
    fn outer_decisions_after_a_nested_fn_belong_to_outer() {
        // The nested fn pushes its own frame; once it closes, outer's own
        // decisions must keep counting toward outer (not the closed inner).
        let fns = analyze_file("fn outer() { fn inner(x: bool) { if x {} } if true {} }");
        assert_eq!(fns.len(), 2);
        assert_eq!(fns[0].name, "outer");
        assert_eq!(
            fns[0].complexity, 2,
            "outer's if after inner must count to outer"
        );
        assert_eq!(fns[1].name, "inner");
        assert_eq!(fns[1].complexity, 2);
    }
}
