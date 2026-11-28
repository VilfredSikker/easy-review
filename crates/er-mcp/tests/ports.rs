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

/// Each table cargo builds er-mcp's code with: `[dependencies]`,
/// `[build-dependencies]` and their `[target.<cfg>.…]` forms. The
/// dev-dependency tables are left out.
fn non_dev_dependency_tables(manifest: &toml::Table) -> Vec<&toml::Table> {
    let mut tables = dependency_tables_in(manifest);
    if let Some(targets) = manifest.get("target").and_then(toml::Value::as_table) {
        for target in targets.values().filter_map(toml::Value::as_table) {
            tables.extend(dependency_tables_in(target));
        }
    }
    tables
}

fn dependency_tables_in(table: &toml::Table) -> Vec<&toml::Table> {
    ["dependencies", "build-dependencies", "build_dependencies"]
        .iter()
        .filter_map(|kind| table.get(*kind)?.as_table())
        .collect()
}

/// The package name of every workspace member.
#[expect(
    clippy::expect_used,
    reason = "a test helper: a manifest it cannot read fails the test"
)]
fn workspace_packages() -> Vec<String> {
    let root: toml::Table = read(&crates().join("../Cargo.toml"))
        .parse()
        .expect("the workspace manifest parses");
    let members = root
        .get("workspace")
        .and_then(|workspace| workspace.get("members"))
        .and_then(toml::Value::as_array)
        .expect("the workspace lists its members");
    members
        .iter()
        .filter_map(toml::Value::as_str)
        .map(|member| {
            let manifest: toml::Table = read(&crates().join("..").join(member).join("Cargo.toml"))
                .parse()
                .expect("a member manifest parses");
            manifest
                .get("package")
                .and_then(|package| package.get("name"))
                .and_then(toml::Value::as_str)
                .expect("a member names its package")
                .to_string()
        })
        .collect()
}

/// er-mcp's code reaches no workspace crate but er-service and er-protocol.
/// Every non-dev table counts, under the package's real name, so a
/// `package = "…"` rename does not hide one.
#[test]
fn er_mcp_depends_on_er_service_alone() {
    let manifest: toml::Table = read(&crates().join("er-mcp/Cargo.toml"))
        .parse()
        .expect("er-mcp's manifest parses");
    let workspace = workspace_packages();
    assert!(
        workspace.iter().any(|name| name == "er-engine"),
        "{workspace:?}"
    );
    let named: Vec<String> = non_dev_dependency_tables(&manifest)
        .into_iter()
        .flatten()
        .map(|(key, spec)| {
            spec.get("package")
                .and_then(toml::Value::as_str)
                .unwrap_or(key.as_str())
                .to_string()
        })
        .filter(|name| workspace.contains(name))
        .collect();
    assert!(
        named
            .iter()
            .all(|name| ["er-service", "er-protocol"].contains(&name.as_str())),
        "er-mcp's code depends on {named:?}"
    );
    assert!(named.iter().any(|name| name == "er-service"), "{named:?}");
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
            "fs::rename",
            "fs::copy",
            "File::create",
            "File::options",
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
