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
