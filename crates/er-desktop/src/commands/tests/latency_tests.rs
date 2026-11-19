//! The flag-off baseline for the hot actions: each command's body as the
//! default build runs it, the snapshot it answers with, and that snapshot's
//! serialisation, over a 5,000-line diff. Each entry that moves one of these
//! actions to the resource path compares its new median with these.
//!
//! Run with `cargo test -p er-desktop --lib latency_tests -- --ignored
//! --nocapture` and paste the medians in the PR. It is never a CI assertion.
//! The webview step of a tab switch is left out: it needs a window.

use std::sync::Arc;
use std::time::{Duration, Instant};

use er_engine::ai::CommentType;
use er_engine::app::CommentTarget;

use super::{review_flow_test_state, ReviewFlowTestDir};
use crate::commands::*;

const RUNS: usize = 50;
/// The first runs warm caches and the allocator; they are not timed.
const WARMUP: usize = 5;
const FILES: usize = 250;
/// Changed lines per file: 250 files of 20 lines is the 5,000-line diff.
const LINES: usize = 20;

fn raw_diff() -> String {
    use std::fmt::Write as _;

    let mut raw = String::new();
    for file in 0..FILES {
        let _ = write!(
            raw,
            "diff --git a/src/f{file}.rs b/src/f{file}.rs\n--- a/src/f{file}.rs\n+++ b/src/f{file}.rs\n@@ -1,{half} +1,{half} @@\n",
            half = LINES / 2
        );
        for line in 0..LINES / 2 {
            let _ = writeln!(raw, "-old {file} {line}");
            let _ = writeln!(raw, "+new {file} {line}");
        }
    }
    raw
}

/// Scripted git, so the timings carry no subprocess.
fn ports(raw: &str) -> er_engine::ports::Ports {
    let git: Arc<dyn er_git::Git> = Arc::new(
        er_git::FakeGit::default()
            .with_current_branch(Ok("feature".to_string()))
            .with_detect_base_branch(Ok("main".to_string()))
            .with_diff_raw(Ok(raw.to_string()))
            .with_is_merge_in_progress(false)
            .with_remote_url(Ok("https://github.com/acme/widgets.git".to_string()))
            .with_diff_shortstat((0, 0, 0))
            .with_list_worktrees(Ok(Vec::new()))
            .with_log_range(Ok(Vec::new()))
            .with_log_head(Ok(Vec::new())),
    );
    let github: Arc<dyn er_engine::github::GitHub> =
        Arc::new(er_engine::github::FakeGitHub::default());
    er_engine::ports::Ports { git, github }
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

/// What the command hands Tauri, serialised as Tauri does.
fn answered(snapshot: Result<AppSnapshot, String>) -> usize {
    let snapshot = snapshot.unwrap_or_else(|e| panic!("the command failed: {e}"));
    serde_json::to_vec(&snapshot)
        .unwrap_or_else(|e| panic!("serialise: {e}"))
        .len()
}

#[test]
#[ignore = "latency harness: run with --ignored --nocapture and paste the numbers in the PR; never a CI assertion"]
fn the_hot_actions_report_their_flag_off_medians() {
    let dir = ReviewFlowTestDir::new();
    let storage = dir.path().join("storage");
    er_engine::storage::with_process_test_storage_root(storage, || {
        let raw = raw_diff();
        let ports = ports(&raw);
        let mut state = review_flow_test_state(&dir);
        state.ports = ports.clone();
        let root = dir.path().join("repo").to_string_lossy().into_owned();
        {
            let mut app = state.app.lock().unwrap_or_else(|e| e.into_inner());
            app.ports = ports.clone();
            let tab = app.tab_mut();
            tab.ports = ports.clone();
            tab.set_raw_diff_for_test(&raw);
            let second = er_engine::app::TabState::new(root.clone(), ports.clone())
                .unwrap_or_else(|e| panic!("second tab: {e}"));
            app.open_tab(second);
            app.select_tab(0);
        }
        let project = crate::projects::ensure_remote_project("acme/widgets")
            .unwrap_or_else(|e| panic!("project: {e}"));
        let inbox_id = "inbox-latency".to_string();
        if let Ok(mut inbox) = state.inbox.lock() {
            inbox.add_item(crate::inbox::InboxItem {
                id: inbox_id.clone(),
                kind: "ai_review_done".into(),
                severity: "success".into(),
                title: "AI review completed".into(),
                body: String::new(),
                source: "ai".into(),
                target: crate::inbox::InboxTarget {
                    project_id: None,
                    repo_root: None,
                    remote: None,
                    pr_number: None,
                    branch: None,
                    url: None,
                },
                created_at_ms: 1,
                read_at_ms: None,
                dedupe_key: "latency".into(),
            });
        }

        let mut rows: Vec<(&str, Duration, usize)> = Vec::new();
        let mut bytes = 0;

        // open target: a new Working tab on the active repo, as `new_tab`.
        let open = median(|| {
            let mut app = state.app.lock().unwrap_or_else(|e| e.into_inner());
            let tab = er_engine::app::TabState::new(root.clone(), state.ports.clone())
                .unwrap_or_else(|e| panic!("open: {e}"));
            app.open_tab(tab);
            crate::tabs::persist_app_tabs(&app, &state.view);
            kick_meta_refresh(&state, root.clone());
            bytes = answered(Ok(snap_from(&app, &state)));
            let opened = app.active_tab;
            app.close_tab_at(opened);
            app.select_tab(0);
        });
        rows.push(("open target", open, bytes));

        // save PR, as `save_pr`.
        let save = median(|| {
            let saved = (|| {
                let title = resolve_pr_title_for_project(&project, 7, None, &state)?;
                crate::projects::save_pr(&project, 7, &title).map_err(|e| e.to_string())?;
                confirm_projects_chrome(&state, "projects_pin")
            })();
            bytes = answered(saved);
        });
        rows.push(("save PR", save, bytes));

        // mark read, as `mark_inbox_item_read`.
        let read = median(|| {
            let now = now_ms();
            bytes = answered(mutate_inbox(&state, "mark_inbox_item_read", |inbox| {
                inbox.mark_item_read(&inbox_id, now);
            }));
        });
        rows.push(("mark read", read, bytes));

        // tab switch, as `select_tab` without its webview step.
        let mut next = 1;
        let switch = median(|| {
            let mut app = state.app.lock().unwrap_or_else(|e| e.into_inner());
            app.select_tab(next);
            kick_deferred_tab_refresh(&mut app, &state);
            kick_active_gh_status(&app, &state);
            crate::tabs::persist_app_tabs(&app, &state.view);
            bytes = answered(Ok(snap_from_command(&app, &state)));
            next = 1 - next;
        });
        rows.push(("tab switch", switch, bytes));
        {
            let mut app = state.app.lock().unwrap_or_else(|e| e.into_inner());
            app.select_tab(0);
        }

        // mark reviewed, as `mark_reviewed`.
        let reviewed = median(|| {
            let mut app = state.app.lock().unwrap_or_else(|e| e.into_inner());
            {
                let tab = app.tab_mut();
                let path = "src/f0.rs".to_string();
                let hash = tab.per_file_hash(&path);
                tab.reviewed.insert(path, hash);
                tab.reviewed_revision += 1;
                let _ = tab.save_reviewed_files();
            }
            bytes = answered(Ok(chrome_snap_from(&app, &state)));
        });
        rows.push(("mark reviewed", reviewed, bytes));

        // next file, as `next_file`.
        let next_file = median(|| {
            let mut app = state.app.lock().unwrap_or_else(|e| e.into_inner());
            app.tab_mut().next_file();
            bytes = answered(Ok(snap_from(&app, &state)));
        });
        rows.push(("next file", next_file, bytes));

        // add question, as `add_question`.
        let question = median(|| {
            let mut app = state.app.lock().unwrap_or_else(|e| e.into_inner());
            let target = CommentTarget {
                file: "src/f1.rs".to_string(),
                hunk_idx: 0,
                line_num: Some(1),
                line_num_end: None,
            };
            let added = app.submit_comment_text_with_id(
                target,
                "Why this line?".to_string(),
                CommentType::Question,
                None,
                None,
                None,
            );
            assert!(added.is_ok(), "{added:?}");
            bytes = answered(Ok(snap_from_confirmed(&app, &state)));
        });
        rows.push(("add question", question, bytes));

        println!(
            "flag-off baseline over a {}-line diff, {} timed runs:",
            FILES * LINES,
            RUNS - WARMUP
        );
        for (action, median, bytes) in rows {
            println!("{action}: median {median:?}, answer {bytes} bytes");
        }
    });
}
