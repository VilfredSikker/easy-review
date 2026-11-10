//! `er-lint-gate` — run clippy over the workspace and apply the gate.
//!
//! ```text
//! er-lint-gate                    check (the CI gate)
//! er-lint-gate --prune            shrink the budget to the current counts
//! er-lint-gate --accept-warnings  rewrite the budget to the current counts,
//!                                 growth included (the diff is reviewed)
//! er-lint-gate --no-stale         skip the stale check (CI: counts differ by platform)
//! er-lint-gate --cohesion         print only the impl-cohesion findings (for calibrating)
//! ```
//!
//! Exit codes: 0 clean, 1 gate failure, 2 the gate itself could not run.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Context};
use er_lint_gate::diagnostics::Diagnostic;
use er_lint_gate::ratchet::{Baseline, Entry};
use er_lint_gate::{
    agent_spawn, cohesion, comment_boundary, diagnostics, evaluate, sidecar_fs, spawn,
};

const BUDGET_FILE: &str = "clippy-warning-budget.json";

struct Flags {
    prune: bool,
    accept_warnings: bool,
    no_stale: bool,
    cohesion_only: bool,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flags = Flags {
        prune: args.iter().any(|a| a == "--prune"),
        accept_warnings: args.iter().any(|a| a == "--accept-warnings"),
        no_stale: args.iter().any(|a| a == "--no-stale"),
        cohesion_only: args.iter().any(|a| a == "--cohesion"),
    };
    match run(&flags) {
        Ok(true) => {}
        Ok(false) => std::process::exit(1),
        Err(err) => {
            eprintln!("er-lint-gate: {err:#}");
            std::process::exit(2);
        }
    }
}

fn workspace_root() -> anyhow::Result<PathBuf> {
    let mut dir = std::env::current_dir()?;
    loop {
        let manifest = dir.join("Cargo.toml");
        if manifest.is_file() && std::fs::read_to_string(&manifest)?.contains("[workspace]") {
            return Ok(dir);
        }
        if !dir.pop() {
            bail!("no workspace Cargo.toml above the current directory");
        }
    }
}

struct SpawnConfig {
    home: String,
    allow: Vec<String>,
}

struct GateConfig {
    budgeted: HashSet<String>,
    impl_cohesion: cohesion::Options,
    spawn_git: SpawnConfig,
    spawn_gh: SpawnConfig,
    sidecar_fs_deny: Vec<String>,
    comment_boundary_allow: Vec<String>,
    agent_spawn_os_launch: Vec<String>,
    agent_spawn_allow: Vec<String>,
}

/// `[workspace.metadata.lint-gate]` in the root manifest.
fn gate_config(root: &Path) -> anyhow::Result<GateConfig> {
    let manifest: toml::Table = std::fs::read_to_string(root.join("Cargo.toml"))?.parse()?;
    let table = manifest
        .get("workspace")
        .and_then(|w| w.get("metadata"))
        .and_then(|m| m.get("lint-gate"))
        .context("Cargo.toml has no [workspace.metadata.lint-gate] table")?;
    let budgeted = table
        .get("budgeted")
        .and_then(toml::Value::as_array)
        .context("[workspace.metadata.lint-gate] has no budgeted list")?
        .iter()
        .filter_map(toml::Value::as_str)
        .map(str::to_string)
        .collect();
    let threshold = |key: &str| {
        table
            .get("impl-cohesion")
            .and_then(|c| c.get(key))
            .and_then(toml::Value::as_integer)
            .and_then(|n| usize::try_from(n).ok())
            .with_context(|| format!("[workspace.metadata.lint-gate] impl-cohesion needs {key}"))
    };
    Ok(GateConfig {
        budgeted,
        impl_cohesion: cohesion::Options {
            max_groups: threshold("max-groups")?,
            min_group_lines: threshold("min-group-lines")?,
        },
        spawn_git: spawn_config(table, "git")?,
        spawn_gh: spawn_config(table, "gh")?,
        sidecar_fs_deny: string_list(table.get("sidecar-fs-deny"), "sidecar-fs-deny")?,
        comment_boundary_allow: string_list(
            table.get("comment-boundary").and_then(|c| c.get("allow")),
            "comment-boundary.allow",
        )?,
        agent_spawn_os_launch: string_list(
            table.get("agent-spawn-os-launch"),
            "agent-spawn-os-launch",
        )?,
        agent_spawn_allow: string_list(table.get("agent-spawn-allow"), "agent-spawn-allow")?,
    })
}

fn string_list(value: Option<&toml::Value>, key: &str) -> anyhow::Result<Vec<String>> {
    Ok(value
        .and_then(toml::Value::as_array)
        .with_context(|| format!("[workspace.metadata.lint-gate] {key} must be a list"))?
        .iter()
        .filter_map(toml::Value::as_str)
        .map(str::to_string)
        .collect())
}

fn spawn_config(table: &toml::Value, program: &str) -> anyhow::Result<SpawnConfig> {
    let entry = table
        .get("spawn")
        .and_then(|spawn| spawn.get(program))
        .with_context(|| format!("[workspace.metadata.lint-gate] has no spawn.{program} table"))?;
    let home = entry
        .get("home")
        .and_then(toml::Value::as_str)
        .with_context(|| format!("[workspace.metadata.lint-gate.spawn.{program}] has no home"))?
        .to_string();
    let allow = match entry.get("allow") {
        Some(value) => value
            .as_array()
            .with_context(|| {
                format!("[workspace.metadata.lint-gate.spawn.{program}] allow must be a list")
            })?
            .iter()
            .filter_map(toml::Value::as_str)
            .map(str::to_string)
            .collect(),
        None => Vec::new(),
    };
    Ok(SpawnConfig { home, allow })
}

/// Every `.rs` file under each crate's `src/`, workspace-relative.
fn crate_sources(root: &Path) -> anyhow::Result<Vec<PathBuf>> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> anyhow::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            let path = entry.path();
            if kind.is_dir() {
                walk(&path, out)?;
            } else if kind.is_file() && path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    for krate in std::fs::read_dir(root.join("crates"))? {
        let src = krate?.path().join("src");
        if src.is_dir() {
            walk(&src, &mut out)?;
        }
    }
    out.sort();
    Ok(out
        .into_iter()
        .filter_map(|p| p.strip_prefix(root).ok().map(Path::to_path_buf))
        .collect())
}

fn cohesion_diagnostics(root: &Path, opts: cohesion::Options) -> anyhow::Result<Vec<Diagnostic>> {
    let mut out = Vec::new();
    for rel in crate_sources(root)? {
        let source = std::fs::read_to_string(root.join(&rel))?;
        let file = rel.to_string_lossy().replace('\\', "/");
        for f in cohesion::impl_cohesion(&source, opts) {
            out.push(Diagnostic {
                level: "warning".into(),
                code: Some(cohesion::CODE.into()),
                rendered: format!("warning: {}\n  --> {file}:{}\n\n", f.message, f.line),
                file: Some(file.clone()),
                line: f.line,
                column: 1,
            });
        }
    }
    Ok(out)
}

fn spawn_diagnostics(
    root: &Path,
    config: &SpawnConfig,
    program: &str,
    trait_name: &str,
) -> anyhow::Result<Vec<Diagnostic>> {
    let mut by_file = BTreeMap::new();
    for rel in crate_sources(root)? {
        let source = std::fs::read_to_string(root.join(&rel))?;
        let file = rel.to_string_lossy().replace('\\', "/");
        let findings = spawn::findings(&source, program);
        if !findings.is_empty() {
            by_file.insert(file, findings);
        }
    }
    let code = format!("er::{program}_spawn");
    Ok(spawn::evaluate(
        &by_file,
        &config.home,
        &config.allow,
        &code,
        trait_name,
    ))
}

fn sidecar_fs_diagnostics(root: &Path, deny: &[String]) -> anyhow::Result<Vec<Diagnostic>> {
    let mut by_file = BTreeMap::new();
    for rel in crate_sources(root)? {
        let source = std::fs::read_to_string(root.join(&rel))?;
        let file = rel.to_string_lossy().replace('\\', "/");
        let findings = sidecar_fs::findings(&source);
        if !findings.is_empty() {
            by_file.insert(file, findings);
        }
    }
    Ok(sidecar_fs::evaluate(&by_file, deny, sidecar_fs::CODE))
}

fn agent_spawn_diagnostics(
    root: &Path,
    os_launch: &[String],
    allow: &[String],
) -> anyhow::Result<Vec<Diagnostic>> {
    let mut by_file = BTreeMap::new();
    for rel in crate_sources(root)? {
        let source = std::fs::read_to_string(root.join(&rel))?;
        let file = rel.to_string_lossy().replace('\\', "/");
        let findings = agent_spawn::findings(&source);
        if !findings.is_empty() {
            by_file.insert(file, findings);
        }
    }
    let mut out = agent_spawn::evaluate(&by_file, os_launch, allow, agent_spawn::CODE);
    out.extend(agent_spawn::validate_os_launch(
        os_launch,
        agent_spawn::CODE,
    ));
    Ok(out)
}

fn comment_boundary_diagnostics(root: &Path, allow: &[String]) -> anyhow::Result<Vec<Diagnostic>> {
    let mut by_file = BTreeMap::new();
    for rel in crate_sources(root)? {
        let source = std::fs::read_to_string(root.join(&rel))?;
        let file = rel.to_string_lossy().replace('\\', "/");
        let findings = comment_boundary::findings(&source);
        if !findings.is_empty() {
            by_file.insert(file, findings);
        }
    }
    Ok(comment_boundary::evaluate(
        &by_file,
        allow,
        comment_boundary::CODE,
    ))
}

fn read_budget(path: &Path) -> anyhow::Result<Baseline> {
    if !path.exists() {
        return Ok(Baseline::new());
    }
    let text = std::fs::read_to_string(path)?;
    serde_json::from_str(&text).with_context(|| format!("{} is not a budget file", path.display()))
}

fn write_budget(path: &Path, budget: &Baseline) -> anyhow::Result<()> {
    std::fs::write(path, format!("{}\n", serde_json::to_string_pretty(budget)?))?;
    Ok(())
}

fn describe(entries: &[Entry]) -> Vec<String> {
    entries
        .iter()
        .map(|e| {
            format!(
                "  {}  {}  {} (budget {})",
                e.file, e.lint, e.count, e.allowed
            )
        })
        .collect()
}

fn run(flags: &Flags) -> anyhow::Result<bool> {
    let root = workspace_root()?;
    let config = gate_config(&root)?;
    let budget_path = root.join(BUDGET_FILE);
    let budget = read_budget(&budget_path)?;
    let cohesion = cohesion_diagnostics(&root, config.impl_cohesion)?;
    let spawn = spawn_diagnostics(&root, &config.spawn_git, "git", "Git")?;
    let spawn_gh = spawn_diagnostics(&root, &config.spawn_gh, "gh", "GitHub")?;
    let sidecar = sidecar_fs_diagnostics(&root, &config.sidecar_fs_deny)?;
    let boundary = comment_boundary_diagnostics(&root, &config.comment_boundary_allow)?;
    let agent = agent_spawn_diagnostics(
        &root,
        &config.agent_spawn_os_launch,
        &config.agent_spawn_allow,
    )?;
    if flags.cohesion_only {
        for d in &cohesion {
            print!("{}", d.rendered);
        }
        println!("er-lint-gate: {} impl-cohesion findings.", cohesion.len());
        return Ok(true);
    }

    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let output = Command::new(cargo)
        .current_dir(&root)
        .args([
            "clippy",
            "--workspace",
            "--all-targets",
            "--message-format=json",
        ])
        .stderr(Stdio::inherit())
        .output()
        .context("could not run cargo clippy")?;
    let stream = String::from_utf8_lossy(&output.stdout);
    let mut diags = diagnostics::parse(&stream);
    diags.extend(cohesion);
    diags.extend(spawn);
    diags.extend(spawn_gh);
    diags.extend(sidecar);
    diags.extend(boundary);
    diags.extend(agent);
    let e = evaluate(diags, &config.budgeted, &budget, output.status.success());

    let mut problems = Vec::new();
    let writing = flags.prune || flags.accept_warnings;
    if writing && !e.complete {
        problems.push(format!(
            "Refusing to write {BUDGET_FILE}: clippy did not lint every crate, so its counts are \
             incomplete. Fix the errors above first."
        ));
    } else if flags.accept_warnings {
        write_budget(&budget_path, &e.ratchet.current)?;
    } else if flags.prune {
        write_budget(&budget_path, &e.ratchet.pruned)?;
    }

    for d in &e.failures {
        eprint!("{}", d.rendered);
    }
    if !e.failures.is_empty() {
        problems.push(
            "Clippy reported the diagnostics above. Fix them, or scope an \
             #[expect(lint, reason = \"…\")] to the one site."
                .into(),
        );
    }

    // Every message of an over-budget (file, lint) pair, since a count can't
    // say which one is new.
    let over = if flags.accept_warnings && e.complete {
        Vec::new()
    } else {
        e.ratchet.over.clone()
    };
    let failing: HashSet<(&str, &str)> = over
        .iter()
        .map(|o| (o.file.as_str(), o.lint.as_str()))
        .collect();
    for d in &e.budgeted {
        if let (Some(file), Some(code)) = (d.file.as_deref(), d.code.as_deref()) {
            if failing.contains(&(file, code)) {
                eprint!("{}", d.rendered);
            }
        }
    }
    if !over.is_empty() {
        problems.push(format!(
            "Files gained budgeted warnings (above):\n{}\nSimplify, or if the growth is justified \
             run `just clippy-accept` and commit {BUDGET_FILE}.",
            describe(&over).join("\n")
        ));
    }

    if e.complete && !writing && !flags.no_stale && !e.ratchet.stale.is_empty() {
        problems.push(format!(
            "The budget is stale: these counts went down. Run `just clippy-prune` and commit \
             {BUDGET_FILE}.\n{}",
            describe(&e.ratchet.stale).join("\n")
        ));
    }
    if !output.status.success() && e.failures.is_empty() {
        problems.push("cargo clippy failed without a diagnostic; see its output above.".into());
    }

    println!(
        "er-lint-gate: {} diagnostics outside the budget, {} budgeted warnings.",
        e.failures.len(),
        e.budgeted.len()
    );
    if problems.is_empty() {
        return Ok(true);
    }
    eprintln!("\n{}", problems.join("\n\n"));
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_allow_key_means_an_empty_list() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(
            dir.join("Cargo.toml"),
            r#"
[workspace.metadata.lint-gate]
budgeted = []
sidecar-fs-deny = ["crates/er-engine/src/ai/annotation_store.rs"]
comment-boundary = { allow = ["crates/er-engine/src/github_post.rs"] }
agent-spawn-os-launch = ["crates/er-engine/src/os_launch.rs"]
agent-spawn-allow = ["crates/x/src/agent.rs"]

[workspace.metadata.lint-gate.impl-cohesion]
max-groups = 1
min-group-lines = 1

[workspace.metadata.lint-gate.spawn.git]
home = "crates/er-git"

[workspace.metadata.lint-gate.spawn.gh]
home = "crates/er-github"
allow = ["crates/x/src/a.rs"]
"#,
        )
        .unwrap();

        let config = gate_config(dir).expect("config without an allow key");

        assert!(config.spawn_git.allow.is_empty());
        assert_eq!(config.spawn_gh.allow, ["crates/x/src/a.rs"]);
        assert_eq!(
            config.sidecar_fs_deny,
            ["crates/er-engine/src/ai/annotation_store.rs"]
        );
        assert_eq!(
            config.comment_boundary_allow,
            ["crates/er-engine/src/github_post.rs"]
        );
        assert_eq!(
            config.agent_spawn_os_launch,
            ["crates/er-engine/src/os_launch.rs"]
        );
        assert_eq!(config.agent_spawn_allow, ["crates/x/src/agent.rs"]);
    }
}
