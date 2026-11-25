//! The start and loop duties the App's `main` and `run_app` carry: the
//! update notice, the gh check, the first-launch notice, the resume hint and
//! the "AI data refreshed" notice (P75.4).

use std::path::Path;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use crossterm::event::KeyCode;
use er_engine::github::PrRef;
use er_engine::ports::Ports;
use er_github::{FakeGitHub, GhError};

use super::parity::assert_json;
use super::shell::Shell;
use super::startup::{self, Args};
use super::tests::{
    app_on, assert_parity, fixture, key, loaded, path_of, press_app, rows, settle, settle_within,
    shell, shell_frame,
};
use crate::ui::highlight::Highlighter;

fn storage() -> tempfile::TempDir {
    tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"))
}

fn args(paths: Vec<String>) -> Args {
    Args {
        paths,
        ..Args::default()
    }
}

fn shown(shell: &mut Shell) -> String {
    rows(&shell_frame(shell, &mut Highlighter::new())).join("\n")
}

/// The update check answers on its own time: the frames and keys before it
/// do not wait, and once it lands the status bar shows it as the App's does.
#[test]
fn the_update_notice_lands_without_holding_a_frame() {
    let _golden = super::parity::guard();
    let repo = fixture(2);
    let storage = storage();
    let path = path_of(&repo);
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut app = app_on(&path);
        app.available_update = Some("9.9.9".into());
        let mut shell = shell(&args(vec![path.clone()]), storage.path());
        let (tx, updates) = mpsc::channel();
        shell.listen_for_update(updates);
        settle(&mut shell, loaded);
        assert!(!shown(&mut shell).contains("er update"));
        for code in [KeyCode::Char('j'), KeyCode::Char('k')] {
            press_app(&mut app, code);
            shell.handle_key(key(code));
        }
        shell.tick(Instant::now());
        assert!(!shown(&mut shell).contains("er update"));

        tx.send("9.9.9".to_string())
            .unwrap_or_else(|e| panic!("{e}"));
        shell.tick(Instant::now());
        assert!(shell.take_redraw(), "the landed version asked for no draw");
        let mut hl = Highlighter::new();
        assert_parity(&app, &shell_frame(&mut shell, &mut hl), &mut hl, "update");
    });
}

fn gh_missing() -> GhError {
    GhError::Local {
        message: "GitHub CLI (gh) is not installed. Install it: https://cli.github.com".into(),
    }
}

fn ports_with(github: &Arc<FakeGitHub>) -> Ports {
    let github: Arc<dyn er_github::GitHub> = github.clone();
    Ports {
        git: Ports::cli().git,
        github,
    }
}

/// A `--remote` or `--pr` start checks `gh` first and stops with its error,
/// as the App's; a start on paths alone never asks.
#[test]
fn a_remote_or_pr_start_checks_gh_first() {
    let repo = fixture(1);
    let path = path_of(&repo);
    let starts = [
        Args {
            paths: vec!["https://github.com/acme/widgets/pull/9".into()],
            remote: true,
            ..Args::default()
        },
        Args {
            paths: vec![path.clone()],
            pr: Some(7),
            ..Args::default()
        },
    ];
    for start in &starts {
        let github = Arc::new(FakeGitHub::default().with_ensure_installed(Err(gh_missing())));
        let error = startup::plan(start, &ports_with(&github))
            .err()
            .unwrap_or_else(|| panic!("{start:?} started without gh"));
        assert_eq!(error.to_string(), gh_missing().to_string());
    }
    let github = Arc::new(FakeGitHub::default().with_ensure_installed(Err(gh_missing())));
    let plain = startup::plan(&args(vec![path]), &ports_with(&github));
    assert!(plain.is_ok(), "a path start asked for gh");
    let asked = github
        .calls()
        .iter()
        .any(|call| call == "ensure_installed()");
    assert!(!asked, "a path start asked for gh");
}

fn first_launch_dirs() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let base = dir.path().join("base");
    let root = dir.path().join("root");
    std::fs::create_dir_all(base.join("repos")).unwrap_or_else(|e| panic!("{e}"));
    (dir, base, root)
}

/// The engine's first-launch notice shows on the status line for the long
/// dwell, as the App's `notify_long`, and a second launch has none.
#[test]
fn the_first_launch_notice_shows_once_for_longer() {
    let repo = fixture(1);
    let storage = storage();
    let (_dir, base, root) = first_launch_dirs();
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = shell(&args(vec![path_of(&repo)]), storage.path());
        settle(&mut shell, loaded);
        let notice = er_engine::first_launch::notice_in(&base, &root);
        let at = Instant::now();
        shell.first_launch(notice);
        let frame = shown(&mut shell);
        assert!(
            frame.contains("Easy Review 1.0 starts with empty storage"),
            "{frame}"
        );
        shell.tick(at + Duration::from_secs(4));
        assert!(
            shell.view().notification.is_some(),
            "the notice left before the long dwell"
        );
        shell.tick(Instant::now() + Duration::from_secs(6));
        assert_eq!(shell.view().notification, None);

        shell.first_launch(er_engine::first_launch::notice_in(&base, &root));
        assert_eq!(shell.view().notification, None, "a second launch spoke");
    });
}

/// On exit the flag path gives the App's `er <args>` line: none for one
/// working tab, both roots for two, and `--remote` with the URLs when every
/// tab is a remote PR.
#[test]
fn the_resume_hint_matches_the_app() {
    let _golden = super::parity::guard();
    let (one, two) = (fixture(1), fixture(1));
    let storage = storage();
    let paths = vec![path_of(&one), path_of(&two)];
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        for paths in [paths[..1].to_vec(), paths.clone()] {
            let app = er_engine::app::App::new_with_args(&paths, Ports::cli())
                .unwrap_or_else(|e| panic!("{e:#}"));
            let mut shell = shell(&args(paths.clone()), storage.path());
            // Loaded, so no actor reads storage after the scope ends.
            settle(&mut shell, loaded);
            assert_eq!(shell.resume_hint(), crate::resume_hint(&app), "{paths:?}");
            let step = format!("{} paths", paths.len());
            assert_json(&step, &shell.resume_hint(), &crate::resume_hint(&app));
        }
    });
    let pr = |number| PrRef {
        owner: "acme".into(),
        repo: "widgets".into(),
        number,
    };
    let remote: Vec<_> = [pr(9), pr(10)]
        .into_iter()
        .map(|pr| startup::remote_pr(pr).tab)
        .collect();
    assert_eq!(
        startup::resume_hint(&remote).as_deref(),
        Some(
            "er --remote https://github.com/acme/widgets/pull/9 https://github.com/acme/widgets/pull/10"
        )
    );
    assert_eq!(
        startup::resume_hint(&remote[..1]).as_deref(),
        Some("er --remote https://github.com/acme/widgets/pull/9")
    );
}

/// An outside write to the active tab's sidecars says "✓ AI data refreshed"
/// once its reload lands, as the App's poll does for the same write; the
/// reloads that find nothing say nothing.
#[test]
fn an_outside_ai_write_says_the_data_refreshed() {
    let repo = fixture(1);
    let storage = storage();
    let path = path_of(&repo);
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut app = app_on(&path);
        assert!(!app.tab_mut().check_ai_files_changed());
        let mut shell = shell(&args(vec![path.clone()]), storage.path());
        settle(&mut shell, loaded);
        let quiet = settle_within(&mut shell, Duration::from_millis(2500), |shell| {
            shell.view().notification.is_some()
        });
        assert!(!quiet, "{:?}", shell.view().notification);

        let content = shell
            .active_content()
            .unwrap_or_else(|| panic!("no content"));
        let er_dir = &content.context.er_dir;
        std::fs::create_dir_all(er_dir).unwrap_or_else(|e| panic!("{e}"));
        er_engine::test_support::write_sidecar_json::<er_engine::ai::ErQuestions>(
            Path::new(er_dir),
            serde_json::json!({
                "version": 1,
                "diff_hash": content.context.branch_diff_hash,
                "questions": [{
                    "id": "q1", "file": "src/f000.rs", "hunk_index": 0, "line_start": 1,
                    "line_content": "c", "text": "outside", "resolved": false
                }]
            }),
        );
        assert!(
            app.tab_mut().check_ai_files_changed(),
            "the App saw no write"
        );
        let refreshed =
            |shell: &Shell| shell.view().notification.as_deref() == Some("✓ AI data refreshed");
        assert!(
            settle_within(&mut shell, Duration::from_secs(5), refreshed),
            "{:?}",
            shell.view().notification
        );
    });
}
