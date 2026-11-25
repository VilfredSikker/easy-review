#[cfg(test)]
mod golden;
mod input;
mod os_launch;
mod screen;
mod service;
mod ui;
mod update;

use anyhow::Result;
use clap::{Parser, Subcommand};
use crossterm::{
    cursor::Show,
    execute,
    terminal::{disable_raw_mode, LeaveAlternateScreen},
};
use er_engine::{github, uninstall};
use std::io::{self, Write};

/// Terminal UI for reviewing git diffs
#[derive(Parser)]
#[command(name = "er", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Repository paths to open (defaults to current directory)
    paths: Vec<String>,

    /// Open a specific PR by number (from the current repo)
    #[arg(long)]
    pr: Option<u64>,

    /// Pre-apply a file filter expression (e.g. '+*.rs,-*.lock')
    #[arg(long)]
    filter: Option<String>,

    /// Review a PR from any directory without a local clone
    #[arg(long)]
    remote: bool,

    /// Override the base branch to diff against (useful for stacked branches)
    #[arg(long)]
    target: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Remove Easy Review config, review data, and installed apps from this machine
    Uninstall {
        /// Skip the confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
        /// Print what would be removed without deleting anything
        #[arg(long)]
        dry_run: bool,
        /// Keep managed review storage (`…/easy-review`)
        #[arg(long)]
        keep_data: bool,
        /// Keep `~/.config/er`
        #[arg(long)]
        keep_config: bool,
        /// Keep the `er` binary and desktop app bundle
        #[arg(long)]
        keep_apps: bool,
    },
    /// Download and install the latest release in place of this binary
    Update,
}

/// Restore the terminal before the default panic handler runs, so a panic
/// inside the event loop doesn't leave the shell in raw mode with no cursor.
/// A panic the service's planning worker catches stands the hook down: the
/// worker answers it as the run's error, and the TUI keeps drawing.
fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if !leaves_the_screen() {
            return;
        }
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, Show);
        default_hook(info);
    }));
}

/// Whether a panic here now ends the TUI, so the hook restores the terminal.
fn leaves_the_screen() -> bool {
    !er_service::catching_panics()
}

fn run_uninstall(yes: bool, dry_run: bool, opts: uninstall::UninstallOptions) -> Result<()> {
    let plan = uninstall::plan(&opts);
    let existing = uninstall::existing_targets(&opts);

    println!("Easy Review uninstall");
    println!();
    if plan.is_empty() {
        println!("Nothing selected to remove.");
        return Ok(());
    }

    for t in &plan {
        let mark = if t.exists { "•" } else { "·" };
        let status = if t.exists { "" } else { " (not present)" };
        println!("  {mark} {}{status}", t.description());
    }
    println!();

    if existing.is_empty() {
        println!("Nothing installed to remove.");
        return Ok(());
    }

    if dry_run {
        println!("Dry run — no changes made.");
        return Ok(());
    }

    if !yes {
        print!("Type uninstall to confirm: ");
        io::stdout().flush()?;
        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
        if line.trim() != "uninstall" {
            println!("Cancelled.");
            return Ok(());
        }
    }

    let report = uninstall::execute(&existing);
    if !report.deferred.is_empty() {
        uninstall::schedule_deferred_removal(&report.deferred)
            .map_err(|e| anyhow::anyhow!("Could not schedule removal of in-use paths: {e}"))?;
    }
    for line in report.summary_lines() {
        println!("{line}");
    }

    if !report.deferred.is_empty() {
        println!();
        println!("Some paths are in use by this process and will be removed after it exits.");
    }

    if !report.is_success() {
        anyhow::bail!("Uninstall completed with errors");
    }

    println!();
    println!("Done. Easy Review data and apps have been removed.");
    Ok(())
}

fn main() -> Result<()> {
    er_engine::env_path::init_cli_path();
    install_panic_hook();
    if cfg!(debug_assertions) {
        er_engine::store_registry::startup_audit(
            er_engine::store_registry::FrontEnd::Tui,
            &er_engine::store_registry::registry(),
        );
    }
    let cli = Cli::parse();

    if let Some(Commands::Uninstall {
        yes,
        dry_run,
        keep_data,
        keep_config,
        keep_apps,
    }) = cli.command
    {
        let opts = uninstall::UninstallOptions {
            remove_config: !keep_config,
            remove_data: !keep_data,
            remove_cache: true,
            remove_binaries: !keep_apps,
            remove_desktop_app: !keep_apps,
        };
        return run_uninstall(yes, dry_run, opts);
    }
    if matches!(cli.command, Some(Commands::Update)) {
        return update::run();
    }

    // Reject conflicting --pr and PR URL arguments
    if cli.pr.is_some() && cli.paths.iter().any(|p| github::is_github_pr_url(p)) {
        anyhow::bail!("Cannot use --pr together with a PR URL argument");
    }

    // Validate --remote flag
    if cli.remote {
        if cli.pr.is_some() {
            anyhow::bail!("Cannot use --remote together with --pr");
        }
        if !cli.paths.iter().any(|p| github::is_github_pr_url(p)) {
            anyhow::bail!("--remote requires a GitHub PR URL argument");
        }
    }

    // --target conflicts with PR-driven base resolution
    if cli.target.is_some() {
        if cli.pr.is_some() {
            anyhow::bail!("Cannot use --target together with --pr");
        }
        if cli.remote {
            anyhow::bail!("Cannot use --target together with --remote");
        }
        if cli.paths.iter().any(|p| github::is_github_pr_url(p)) {
            anyhow::bail!("Cannot use --target together with a PR URL argument");
        }
    }

    let args = service::Args {
        paths: cli.paths,
        pr: cli.pr,
        filter: cli.filter,
        remote: cli.remote,
        target: cli.target,
    };
    service::run(&args, update::spawn_background_check())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A panic the planning worker catches leaves the terminal as it is; any
    /// other panic restores it, on this thread too while a plan runs.
    #[test]
    fn a_caught_planning_panic_leaves_the_screen_alone() {
        assert!(leaves_the_screen());
        let on_worker = er_service::test_support::on_planning_worker(leaves_the_screen);
        assert_eq!(on_worker, Ok(false), "the hook would leave the screen");
        let here = er_service::test_support::while_planning(leaves_the_screen);
        assert!(here, "a panic here while a plan runs would keep the screen");
    }
}
