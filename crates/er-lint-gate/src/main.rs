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

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Context};
use er_lint_gate::diagnostics::Diagnostic;
use er_lint_gate::ratchet::{Baseline, Entry};
use er_lint_gate::{cohesion, diagnostics, evaluate};

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

struct GateConfig {
    budgeted: HashSet<String>,
    impl_cohesion: cohesion::Options,
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
    })
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
