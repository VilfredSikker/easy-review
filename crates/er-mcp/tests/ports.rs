//! MCP reaches `gh` and git through the `Ports` its server holds, so a
//! test can script them. This scan fails on a call that builds its own
//! handle, in er-mcp, in er-service's `headless` module, or in the
//! `sidecar_upload` functions MCP calls.

use std::path::Path;

/// The source of `path` up to its inline test module.
fn code(path: &Path) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    match text.find("#[cfg(test)]") {
        Some(at) => text[..at].to_string(),
        None => text,
    }
}

fn rust_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|entry| entry.expect("dir entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .collect();
    files.sort();
    files
}

/// Each place that names one of these builds a handle the server did not
/// pass in.
const OUTSIDE_THE_PORTS: [&str; 7] = [
    "default_handle()",
    "Ports::cli()",
    "gh_pr_title(",
    "gh_pr_list_queue(",
    "gh_pr_checks_state_remote(",
    "gh_pr_thread_addressing_remote(",
    "gh_pr_diff_remote(",
];

#[test]
fn mcp_reaches_gh_and_git_only_through_its_ports() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = rust_files(&crates.join("er-mcp/src"));
    files.extend(rust_files(&crates.join("er-service/src/headless")));
    files.push(crates.join("er-engine/src/sidecar_upload.rs"));
    for file in files {
        let code = code(&file);
        for needle in OUTSIDE_THE_PORTS {
            let allowed =
                usize::from(needle == "Ports::cli()" && file.ends_with("er-mcp/src/server.rs"));
            assert!(
                code.matches(needle).count() <= allowed,
                "{} names {needle}",
                file.display()
            );
        }
        assert!(
            !code.contains("gh_pr_metadata_remote("),
            "{} names gh_pr_metadata_remote",
            file.display()
        );
    }
}
