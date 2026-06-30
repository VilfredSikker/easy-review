//! Reproduction: local GitHub comments vanish when `pr_number` is assigned to a
//! local branch tab AFTER comments were already added.
//!
//! A local-branch tab starts with `pr_number = None`; the desktop sets it later
//! (e.g. when the user toggles the PR Diff view, see `set_mode` in commands.rs).
//! `github_comments_dir()` resolves to the branch view bucket while `pr_number`
//! is None and to the PR bucket once it is set, so comments written early end up
//! orphaned in the branch bucket and disappear from the panel.

use std::process::Command;

use er_engine::ai::CommentType;
use er_engine::app::App;

fn git(dir: &std::path::Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .status()
        .expect("git command failed to run");
    assert!(status.success(), "git {:?} failed", args);
}

#[test]
fn local_comments_survive_late_pr_number_assignment() {
    let base = std::env::temp_dir().join(format!(
        "er-bucket-repro-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let repo = base.join("repo");
    let storage = base.join("storage");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::create_dir_all(&storage).unwrap();

    std::env::set_var("ER_STORAGE_ROOT", &storage);
    std::env::remove_var("ER_REPO_LOCAL");

    // Build a repo with a `main` base and a `feature` branch carrying one change.
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@example.com"]);
    git(&repo, &["config", "user.name", "Test"]);
    git(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/acme/widgets.git",
        ],
    );
    std::fs::write(repo.join("f.txt"), "line1\nline2\nline3\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "base"]);
    git(&repo, &["checkout", "-q", "-b", "feature"]);
    std::fs::write(repo.join("f.txt"), "line1\nCHANGED\nline3\nline4\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "change"]);

    let repo_str = repo.to_string_lossy().to_string();
    let mut app = App::new_with_args(&[repo_str]).expect("app");

    // Sanity: Branch mode, no PR yet.
    assert_eq!(app.tab().pr_number, None);
    let file = app.tab().files[0].path.clone();

    // Add two comments BEFORE the PR number is known (as happens when a user
    // comments on a freshly opened local branch before toggling PR Diff).
    app.submit_comment_text(
        file.clone(),
        0,
        Some(2),
        None,
        "comment one".to_string(),
        CommentType::GitHubComment,
        None,
        None,
    )
    .unwrap();
    app.submit_comment_text(
        file.clone(),
        0,
        Some(2),
        None,
        "comment two".to_string(),
        CommentType::GitHubComment,
        None,
        None,
    )
    .unwrap();

    let count_before = app
        .tab()
        .ai
        .github_comments
        .as_ref()
        .map(|gc| gc.comments.len())
        .unwrap_or(0);
    assert_eq!(count_before, 2, "two local comments should be visible");

    // Now the PR number becomes known (desktop set_mode / PR detection), and the
    // AI state reloads.
    app.tab_mut().pr_number = Some(7);
    app.tab_mut().reload_ai_state();

    let after: Vec<String> = app
        .tab()
        .ai
        .github_comments
        .as_ref()
        .map(|gc| gc.comments.iter().map(|c| c.comment.clone()).collect())
        .unwrap_or_default();
    assert_eq!(
        after.len(),
        2,
        "local comments must survive late pr_number assignment (found {after:?})"
    );
    assert!(after.iter().any(|c| c == "comment one"));
    assert!(after.iter().any(|c| c == "comment two"));

    // A comment added after the flip accumulates with the migrated ones.
    app.submit_comment_text(
        file,
        0,
        Some(2),
        None,
        "comment three".to_string(),
        CommentType::GitHubComment,
        None,
        None,
    )
    .unwrap();
    app.tab_mut().reload_ai_state();
    let total = app
        .tab()
        .ai
        .github_comments
        .as_ref()
        .map(|gc| gc.comments.len())
        .unwrap_or(0);

    std::fs::remove_dir_all(&base).ok();

    assert_eq!(total, 3, "all three local comments should be visible");
}
