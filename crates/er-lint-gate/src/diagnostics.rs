//! Read `cargo clippy --message-format=json` into one diagnostic per site.

use std::collections::HashSet;

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// `warning` or `error`.
    pub level: String,
    /// Lint or error code, e.g. `clippy::unwrap_used` or `E0308`.
    pub code: Option<String>,
    /// Workspace-relative file. `None` when no span in the expansion chain
    /// lands in the workspace, which the gate treats as a failure.
    pub file: Option<String>,
    pub line: usize,
    pub column: usize,
    pub rendered: String,
}

#[derive(Deserialize)]
struct Envelope {
    reason: String,
    message: Option<Message>,
}

#[derive(Deserialize)]
struct Message {
    level: String,
    code: Option<Code>,
    spans: Vec<Span>,
    rendered: Option<String>,
}

#[derive(Deserialize)]
struct Code {
    code: String,
}

#[derive(Deserialize)]
struct Span {
    file_name: String,
    line_start: usize,
    column_start: usize,
    is_primary: bool,
    expansion: Option<Box<Expansion>>,
}

#[derive(Deserialize)]
struct Expansion {
    span: Span,
}

/// Cargo reports workspace files relative to the workspace root, and
/// dependency or standard-library files as absolute paths or `<…>` pseudo-files.
fn in_workspace(file_name: &str) -> bool {
    !(file_name.starts_with('/') || file_name.starts_with('<') || file_name.contains(":\\"))
}

/// Follow a span out of macro expansions until it reaches workspace code.
/// `assert_eq!(a, 0.0)` reports its primary span inside the standard library;
/// the call site is what the gate must count, or the diagnostic vanishes.
fn resolve(span: &Span) -> Option<&Span> {
    let mut at = span;
    loop {
        if in_workspace(&at.file_name) {
            return Some(at);
        }
        at = &at.expansion.as_ref()?.span;
    }
}

/// Parse a cargo JSON stream. Build-progress lines, and the codeless summary
/// messages cargo prints after each crate ("N warnings emitted"), are skipped.
/// Lib and lib-test targets lint the same file, so sites are deduplicated.
pub fn parse(stream: &str) -> Vec<Diagnostic> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for line in stream.lines() {
        if !line.starts_with('{') {
            continue;
        }
        let Ok(envelope) = serde_json::from_str::<Envelope>(line) else {
            continue;
        };
        let Some(message) = envelope
            .message
            .filter(|_| envelope.reason == "compiler-message")
        else {
            continue;
        };
        if message.level != "warning" && message.level != "error" {
            continue;
        }
        if message.code.is_none() && message.spans.is_empty() {
            continue;
        }
        let primary = message
            .spans
            .iter()
            .find(|s| s.is_primary)
            .or_else(|| message.spans.first());
        let site = primary.and_then(resolve);
        let diagnostic = Diagnostic {
            level: message.level,
            code: message.code.map(|c| c.code),
            file: site.map(|s| s.file_name.clone()),
            line: site.map_or(0, |s| s.line_start),
            column: site.map_or(0, |s| s.column_start),
            rendered: message.rendered.unwrap_or_default(),
        };
        let key = (
            diagnostic.level.clone(),
            diagnostic.code.clone(),
            diagnostic.file.clone(),
            diagnostic.line,
            diagnostic.column,
        );
        if seen.insert(key) {
            out.push(diagnostic);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../tests/fixtures/clippy.jsonl");

    fn by_code<'a>(diags: &'a [Diagnostic], code: &str) -> Vec<&'a Diagnostic> {
        diags
            .iter()
            .filter(|d| d.code.as_deref() == Some(code))
            .collect()
    }

    #[test]
    fn reads_a_plain_lint_at_its_primary_span() {
        let diags = parse(FIXTURE);
        let unwraps = by_code(&diags, "clippy::unwrap_used");
        assert_eq!(
            unwraps.len(),
            1,
            "lib and lib-test copies collapse into one"
        );
        assert_eq!(
            unwraps[0].file.as_deref(),
            Some("crates/er-engine/src/lib.rs")
        );
        assert_eq!((unwraps[0].line, unwraps[0].column), (10, 5));
        assert_eq!(unwraps[0].level, "warning");
    }

    #[test]
    fn follows_a_macro_expansion_back_to_the_workspace_call_site() {
        let diags = parse(FIXTURE);
        let floats = by_code(&diags, "clippy::float_cmp");
        assert_eq!(floats.len(), 1);
        assert_eq!(
            floats[0].file.as_deref(),
            Some("crates/er-desktop/src/window_placement.rs")
        );
        assert_eq!(floats[0].line, 176);
    }

    #[test]
    fn keeps_a_diagnostic_with_no_workspace_span_as_fileless() {
        let diags = parse(FIXTURE);
        let foreign = by_code(&diags, "clippy::foreign_only");
        assert_eq!(foreign.len(), 1, "never dropped");
        assert_eq!(foreign[0].file, None);
    }

    #[test]
    fn keeps_a_codeless_error_and_skips_summaries_and_build_lines() {
        let diags = parse(FIXTURE);
        let codeless: Vec<_> = diags.iter().filter(|d| d.code.is_none()).collect();
        assert_eq!(codeless.len(), 1, "only the real error, not the summaries");
        assert_eq!(codeless[0].level, "error");
        assert_eq!(
            codeless[0].file.as_deref(),
            Some("crates/er-tui/src/main.rs")
        );
        assert_eq!(diags.len(), 4);
    }
}
