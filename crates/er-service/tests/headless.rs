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
    match text.find("#[cfg(test)]") {
        Some(at) => text[..at].to_string(),
        None => text,
    }
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
            let name: String = module[at + 7..]
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
