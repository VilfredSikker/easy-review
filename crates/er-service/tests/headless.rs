//! The `headless` module holds what MCP calls and nothing more (ADR 0075),
//! and the engine no longer carries the modules that moved into it.

use std::path::Path;

const MOVED: [&str; 5] = [
    "pr_resolve",
    "pr_review_feedback",
    "review_queue",
    "sidecar_summary",
    "diagram_upload",
];

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The code of a module, up to its inline test module.
fn code(path: &Path) -> String {
    let text = read(path);
    text.split("#[cfg(test)]")
        .next()
        .unwrap_or_default()
        .to_string()
}

fn crates() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

#[test]
fn the_engine_no_longer_declares_the_modules_mcp_moved() {
    let lib = read(&crates().join("er-engine/src/lib.rs"));
    for name in MOVED {
        assert!(
            !lib.contains(&format!("mod {name}")),
            "er-engine declares {name}"
        );
    }
}

#[test]
fn every_headless_function_is_one_mcp_calls_or_one_it_needs() {
    let headless: Vec<String> = MOVED
        .iter()
        .map(|name| code(&crates().join(format!("er-service/src/headless/{name}.rs"))))
        .collect();
    let all_headless = headless.join("\n");
    let mcp = std::fs::read_dir(crates().join("er-mcp/src"))
        .expect("er-mcp sources")
        .map(|entry| read(&entry.expect("dir entry").path()))
        .collect::<Vec<_>>()
        .join("\n");
    for module in &headless {
        for (at, _) in module.match_indices("pub fn ") {
            let name: String = module
                .get(at + 7..)
                .unwrap_or_default()
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            let called = format!("{name}(");
            let used_inside = all_headless.matches(&called).count() > 1;
            assert!(
                mcp.contains(&name) || used_inside,
                "headless::{name} is neither called by er-mcp nor needed by what it calls"
            );
        }
    }
}

/// Every non-test Rust file under `dir`.
fn sources(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
        let path = entry
            .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
            .path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if path.is_dir() {
            if !name.ends_with("tests") {
                sources(&path, out);
            }
        } else if name.ends_with(".rs") && !name.ends_with("tests.rs") && name != "test_support.rs"
        {
            out.push(path);
        }
    }
}

/// The struct literal `text` starts with, up to its matching brace.
fn literal(text: &str) -> &str {
    let mut depth = 0usize;
    for (at, c) in text.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return text.get(..=at).unwrap_or(text);
                }
            }
            _ => {}
        }
    }
    text
}

/// Question and note replies have one writer (ADR 0075): of all the
/// `PrivateAnnotation` rows built outside tests, one sets `in_reply_to`.
#[test]
fn one_writer_builds_question_and_note_replies() {
    let mut files = Vec::new();
    for dir in ["er-engine/src", "er-service/src", "er-mcp/src"] {
        sources(&crates().join(dir), &mut files);
    }
    let mut writers = Vec::new();
    for file in files {
        let text = read(&file);
        let code = text
            .split("#[cfg(test)]\nmod tests")
            .next()
            .unwrap_or_default();
        for (at, _) in code.match_indices("PrivateAnnotation {") {
            let row = literal(code.get(at..).unwrap_or_default());
            let sets_parent = row.lines().any(|line| {
                line.trim_start().starts_with("in_reply_to:") && !line.contains("in_reply_to: None")
            });
            if sets_parent && !code.get(..at).unwrap_or_default().ends_with("struct ") {
                writers.push(file.display().to_string());
            }
        }
    }
    assert_eq!(writers.len(), 1, "reply rows are built in {writers:?}");
    assert!(
        writers[0].ends_with("headless/pr_review_feedback.rs"),
        "{writers:?}"
    );
}
