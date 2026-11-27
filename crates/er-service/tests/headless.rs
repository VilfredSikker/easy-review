//! The `headless` module holds what MCP calls and nothing more (ADR 0075),
//! and the engine no longer carries the modules that moved into it.

#[path = "support/source_scan.rs"]
mod source_scan;

use std::path::{Path, PathBuf};

use source_scan::{code_of, functions, mentions, read, rust_files};

const MOVED: [&str; 5] = [
    "pr_resolve",
    "pr_review_feedback",
    "review_queue",
    "sidecar_summary",
    "diagram_upload",
];

fn crates() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// The code of every file in `files`, joined.
fn code(files: &[PathBuf]) -> String {
    files
        .iter()
        .map(|file| code_of(&read(file)))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_engine_no_longer_declares_the_modules_mcp_moved() {
    let lib = code_of(&read(&crates().join("er-engine/src/lib.rs")));
    for name in MOVED {
        assert!(
            !lib.contains(&format!("mod {name}")),
            "er-engine declares {name}"
        );
    }
}

/// A `headless` function is named in er-mcp's code (comments and tests
/// left out), or called by another `headless` function.
#[test]
fn every_headless_function_is_one_mcp_calls_or_one_it_needs() {
    let mut files = rust_files(&crates().join("er-service/src/headless"));
    files.push(crates().join("er-service/src/headless.rs"));
    let headless = code(&files);
    let mcp = code(&rust_files(&crates().join("er-mcp/src")));
    let names = functions(&headless);
    assert!(names.len() > 20, "the scan found {} functions", names.len());
    for (name, _) in names {
        assert!(
            mentions(&mcp, &name) > 0 || mentions(&headless, &name) > 1,
            "headless::{name} is neither called by er-mcp nor needed by what it calls"
        );
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
