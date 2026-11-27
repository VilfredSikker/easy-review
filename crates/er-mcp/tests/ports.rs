//! MCP runs on er-service. It reaches `gh` and git through the `Ports` its
//! server holds, so a test can script them; its manifest names no workspace
//! crate but er-service and er-protocol; it starts no workspace actor; and it
//! leaves the projects record to the owner in er-service. These scans read
//! code with comments and test items left out.

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

/// er-mcp's code outside tests, one file per entry.
fn mcp_sources() -> Vec<(PathBuf, String)> {
    rust_files(&crates().join("er-mcp/src"))
        .into_iter()
        .map(|file| {
            let code = code_of(&read(&file));
            (file, code)
        })
        .collect()
}

/// The workspace crates er-mcp's `[dependencies]` names; dev-dependencies
/// are exempt.
#[test]
fn er_mcp_depends_on_er_service_alone() {
    let manifest = read(&crates().join("er-mcp/Cargo.toml"));
    let table = manifest
        .split("\n[")
        .find(|section| section.starts_with("dependencies]"))
        .expect("a [dependencies] table");
    let workspace: Vec<&str> = table
        .lines()
        .filter(|line| line.contains("path = \"../"))
        .filter_map(|line| line.split_whitespace().next())
        .collect();
    assert!(
        workspace
            .iter()
            .all(|name| ["er-service", "er-protocol"].contains(name)),
        "er-mcp's [dependencies] names {workspace:?}"
    );
    assert!(workspace.contains(&"er-service"), "{workspace:?}");
}

/// MCP calls er-service's functions and starts no actor (ADR 0075).
#[test]
fn mcp_starts_no_actor() {
    for (file, code) in mcp_sources() {
        assert!(
            !code.contains("er_service::spawn"),
            "{} starts an actor (er_service::spawn)",
            file.display()
        );
        for name in [
            "WorkspaceDeps",
            "WorkspaceHandle",
            "ServiceHost",
            "AppOwner",
            "SidebarOwner",
        ] {
            assert_eq!(
                mentions(&code, name),
                0,
                "{} starts an actor ({name})",
                file.display()
            );
        }
    }
}

/// The projects record has one writer, the owner in er-service: MCP reads
/// and pins through it and writes no record itself.
#[test]
fn mcp_writes_no_projects_record_itself() {
    for (file, code) in mcp_sources() {
        for writer in [
            "projects_pins",
            "projects.json",
            "ProjectsFile>()",
            "fs::write",
            "File::create",
            "OpenOptions",
        ] {
            assert!(
                !code.contains(writer),
                "{} writes the projects record itself ({writer})",
                file.display()
            );
        }
    }
}
