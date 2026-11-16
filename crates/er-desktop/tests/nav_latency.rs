//! Times the desktop's navigation commands as they run on the default build:
//! the move under the engine lock, then the snapshot the command answers
//! with. The service path's own commands join this harness once they exist.
//!
//! Run with `cargo test -p er-desktop --test nav_latency -- --ignored
//! --nocapture` and paste the medians in the PR. It is never a CI assertion.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use er_desktop::snapshot::{build_snapshot_with_delta, SnapshotSources};
use er_engine::app::App;

const RUNS: usize = 50;
/// The first runs warm caches and the allocator; they are not timed.
const WARMUP: usize = 5;

/// A diff of `files` files with three small hunks each.
fn raw_diff(files: usize) -> String {
    let mut raw = String::new();
    for file in 0..files {
        raw.push_str(&format!(
            "diff --git a/src/f{file}.rs b/src/f{file}.rs\n--- a/src/f{file}.rs\n+++ b/src/f{file}.rs\n"
        ));
        for hunk in 0..3 {
            let at = hunk * 20 + 1;
            raw.push_str(&format!(
                "@@ -{at},3 +{at},3 @@\n ctx\n-old{hunk}\n+new{hunk}\n ctx\n"
            ));
        }
    }
    raw
}

/// The median of the timed runs, after the warm-up runs.
fn median(mut operation: impl FnMut()) -> Duration {
    let mut samples = Vec::with_capacity(RUNS - WARMUP);
    for run in 0..RUNS {
        let started = Instant::now();
        operation();
        if run >= WARMUP {
            samples.push(started.elapsed());
        }
    }
    samples.sort_unstable();
    samples[samples.len() / 2]
}

/// One command: lock, move, answer with a snapshot.
fn command(app: &Mutex<App>, step: impl Fn(&mut App)) -> Duration {
    median(|| {
        let mut app = app.lock().unwrap_or_else(|e| e.into_inner());
        step(&mut app);
        let snap = build_snapshot_with_delta(&app, &SnapshotSources::default());
        assert!(!snap.files.is_empty());
    })
}

#[test]
#[ignore = "latency harness: run with --ignored --nocapture and paste the numbers in the PR; never a CI assertion"]
fn navigation_commands_report_their_medians() {
    let storage = tempfile::tempdir().expect("temp dir");
    er_engine::storage::with_test_storage_root(storage.path(), || {
        // Scripted git, so the timings carry no subprocess.
        let git = er_git::FakeGit::default()
            .with_diff_shortstat((0, 0, 0))
            .with_list_worktrees(Ok(Vec::new()))
            .with_log_range(Ok(Vec::new()))
            .with_log_head(Ok(Vec::new()));
        let ports = er_engine::ports::Ports::with_git(std::sync::Arc::new(git));
        let files = er_engine::git::parse_diff(&raw_diff(60));
        let app = Mutex::new(App::new_for_test_with_ports(files, ports));
        let rows = [
            ("next file", command(&app, |app| app.tab_mut().next_file())),
            ("next line", command(&app, |app| app.tab_mut().next_line())),
            ("next unreviewed", command(&app, App::next_unreviewed_file)),
            (
                "mark reviewed",
                command(&app, |app| {
                    let _ = app.toggle_reviewed();
                }),
            ),
        ];
        for (action, median) in rows {
            println!("{action}: median {median:?} over {} runs", RUNS - WARMUP);
        }
    });
}
