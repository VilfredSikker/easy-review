//! MCP reaches `gh` and git through the `Ports` its server holds, so a
//! test can script them. This scan fails on a call that builds its own
//! handle, in er-mcp, in er-service's `headless` module, or in the
//! `sidecar_upload` functions MCP calls.

#[path = "../../er-service/tests/support/source_scan.rs"]
mod source_scan;

use std::path::{Path, PathBuf};

use source_scan::{code_of, functions, mentions, read, rust_files};

fn crates() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// Every free function in er-github, or in er-engine's `github` module,
/// that builds the default handle inside: calling one bypasses the ports.
fn wrappers() -> Vec<String> {
    let mut files = rust_files(&crates().join("er-github/src"));
    files.push(crates().join("er-engine/src/github.rs"));
    let mut names: Vec<String> = files
        .iter()
        .flat_map(|file| functions(&code_of(&read(file))))
        .filter(|(_, body)| mentions(body, "default_handle") > 0)
        .map(|(name, _)| name)
        .collect();
    names.push("default_handle".to_string());
    names.sort();
    names.dedup();
    names
}

#[test]
fn mcp_reaches_gh_and_git_only_through_its_ports() {
    let wrappers = wrappers();
    for known in [
        "gh_pr_title",
        "get_pr_info",
        "gh_current_login",
        "gh_open_pr_number_for_head",
    ] {
        assert!(
            wrappers.iter().any(|name| name == known),
            "{known} is not found as a wrapper"
        );
    }
    let mut files = rust_files(&crates().join("er-mcp/src"));
    files.extend(rust_files(&crates().join("er-service/src/headless")));
    files.push(crates().join("er-service/src/headless.rs"));
    files.push(crates().join("er-engine/src/sidecar_upload.rs"));
    for file in files {
        let code = code_of(&read(&file));
        let named = |what: &str| format!("{} names {what}", file.display());
        for name in &wrappers {
            assert_eq!(mentions(&code, name), 0, "{}", named(name));
        }
        for handle in ["CliGitHub", "CliGit"] {
            assert_eq!(mentions(&code, handle), 0, "{}", named(handle));
        }
        let allowed = usize::from(file.ends_with("er-mcp/src/server.rs"));
        assert!(
            code.matches("Ports::cli()").count() <= allowed,
            "{}",
            named("Ports::cli()")
        );
    }
}
