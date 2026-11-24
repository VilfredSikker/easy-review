//! The working-tree writes (P69): each runs the port off the actor, reloads
//! the tab so the diff section's revision moves, and keeps the App's rule for
//! showing the last commit in Staged.

use std::path::Path;
use std::process::Command as Git;
use std::sync::Arc;

use er_engine::ports::Ports;
use er_engine::tab_identity::DiffMode;
use er_engine::TabKey;
use er_github::FakeGitHub;

use super::{actor_over, content_of, load, with_root, working_tab, RAW, WAIT};
use crate::actor::WorkspaceDeps;
use crate::protocol::{Command, Reply};
use crate::{spawn, WorkspaceHandle};

fn run_git(dir: &str, args: &[&str]) -> String {
    let out = Git::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap_or_else(|error| panic!("git {args:?}: {error}"));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// A repo on `main` with `src/a.rs` committed, its identity set locally so a
/// commit through the port needs no global config.
fn repo_at(dir: &str) {
    run_git(dir, &["init", "-q", "-b", "main"]);
    for (key, value) in [
        ("user.name", "t"),
        ("user.email", "t@t"),
        ("commit.gpgsign", "false"),
    ] {
        run_git(dir, &["config", key, value]);
    }
    write(dir, "src/a.rs", "one\n");
    run_git(dir, &["add", "."]);
    run_git(dir, &["commit", "-q", "-m", "base"]);
}

fn write(dir: &str, path: &str, text: &str) {
    let path = Path::new(dir).join(path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap_or_else(|e| panic!("{e}"));
    }
    std::fs::write(path, text).unwrap_or_else(|e| panic!("{e}"));
}

fn cli_actor() -> WorkspaceHandle {
    spawn(WorkspaceDeps::new(Ports {
        git: Arc::new(er_git::CliGit),
        github: Arc::new(FakeGitHub::default()),
    }))
}

fn load_in(handle: &WorkspaceHandle, key: TabKey, repo: &str, mode: DiffMode) {
    let tab = working_tab(repo);
    let command = Command::LoadTab {
        key,
        tab,
        mode: Some(mode),
    };
    let reply = handle.send_and_wait_with(command, WAIT);
    assert!(reply.is_ok(), "load: {reply:?}");
}

fn send(handle: &WorkspaceHandle, command: Command) -> Reply {
    handle
        .send_and_wait_with(command, WAIT)
        .unwrap_or_else(|e| panic!("{e}"))
}

fn written(key: TabKey, notice: &str) -> Reply {
    let notice = notice.to_string();
    Reply::WorktreeWritten { key, notice }
}

fn paths(handle: &WorkspaceHandle, key: TabKey) -> Vec<String> {
    let content = content_of(&handle.view(), key).unwrap_or_else(|e| panic!("{e}"));
    content.diff.files.iter().map(|f| f.path.clone()).collect()
}

fn unmerged(content: &crate::TabContent) -> bool {
    let unmerged = er_engine::git::FileStatus::Unmerged;
    content
        .diff
        .files
        .iter()
        .any(|file| file.status == unmerged)
}

fn revision(handle: &WorkspaceHandle, key: TabKey) -> u64 {
    let content = content_of(&handle.view(), key).unwrap_or_else(|e| panic!("{e}"));
    content.diff.revision
}

/// A commit run on the actor thread would trip the port guard there, and the
/// reply would never come.
#[test]
fn a_commit_runs_off_the_actor_thread() -> anyhow::Result<()> {
    with_root(|repo| {
        let fake = Arc::new(
            er_git::FakeGit::default()
                .with_current_branch(Ok("feature".to_string()))
                .with_diff_shortstat((0, 0, 0))
                .with_diff_raw(Ok(RAW.to_string()))
                .with_commit(Ok(())),
        );
        let handle = actor_over(Arc::clone(&fake));
        let key = TabKey::next();
        load(&handle, key, repo)?;
        let before = revision(&handle, key);

        let message = "fix the thing".to_string();
        let reply = handle.send_and_wait_with(Command::Commit { key, message }, WAIT)?;

        assert_eq!(reply, written(key, "Committed! Ctrl+P to push"));
        let commit = format!("commit(repo_root={repo:?}, message=\"fix the thing\")");
        assert!(fake.calls().contains(&commit), "{:?}", fake.calls());
        assert!(revision(&handle, key) > before, "the diff did not reload");
        Ok(())
    })
}

/// Stage and unstage reload the tab in its own view, so the file leaves it
/// and the diff section's revision moves. In Conflicts a stage resolves the
/// path and the tab stays in Conflicts.
#[test]
fn a_stage_moves_the_diff_revision() -> anyhow::Result<()> {
    with_root(|repo| {
        repo_at(repo);
        write(repo, "src/a.rs", "two\n");
        let handle = cli_actor();
        let key = TabKey::next();
        load_in(&handle, key, repo, DiffMode::Unstaged);
        assert_eq!(paths(&handle, key), ["src/a.rs"]);
        let before = revision(&handle, key);

        let path = "src/a.rs".to_string();
        let reply = send(&handle, Command::StageFile { key, path });
        assert_eq!(reply, written(key, "Staged: src/a.rs"));
        assert!(paths(&handle, key).is_empty(), "the staged file stayed");
        assert!(revision(&handle, key) > before, "the stage did not reload");

        let to_staged = Command::SetDiffMode {
            key,
            mode: DiffMode::Staged,
            pr_number: None,
        };
        send(&handle, to_staged);
        assert_eq!(paths(&handle, key), ["src/a.rs"]);
        let before = revision(&handle, key);
        let path = "src/a.rs".to_string();
        let reply = send(&handle, Command::UnstageFile { key, path });
        assert_eq!(reply, written(key, "Unstaged: src/a.rs"));
        assert!(paths(&handle, key).is_empty(), "the unstaged file stayed");
        assert!(
            revision(&handle, key) > before,
            "the unstage did not reload"
        );

        // A conflicted merge of `src/a.rs`.
        run_git(repo, &["checkout", "-q", "--", "."]);
        run_git(repo, &["checkout", "-q", "-b", "other"]);
        write(repo, "src/a.rs", "theirs\n");
        run_git(repo, &["commit", "-q", "-am", "theirs"]);
        run_git(repo, &["checkout", "-q", "main"]);
        write(repo, "src/a.rs", "ours\n");
        run_git(repo, &["commit", "-q", "-am", "ours"]);
        run_git(repo, &["merge", "-q", "other"]);
        let conflicts = TabKey::next();
        load_in(&handle, conflicts, repo, DiffMode::Conflicts);
        let content = content_of(&handle.view(), conflicts)?;
        assert!(unmerged(&content), "no conflict");
        let before = revision(&handle, conflicts);
        let path = "src/a.rs".to_string();
        let reply = send(
            &handle,
            Command::StageFile {
                key: conflicts,
                path,
            },
        );
        assert_eq!(reply, written(conflicts, "Resolved: src/a.rs"));
        let content = content_of(&handle.view(), conflicts)?;
        assert_eq!(content.tab.mode, DiffMode::Conflicts);
        assert!(!unmerged(&content), "the resolved path is still unmerged");
        assert!(content.diff.revision > before, "the resolve did not reload");
        Ok(())
    })
}

/// After a commit, Staged shows the last commit until a push, as the App
/// does; a mode switch clears it too.
#[test]
fn a_commit_shows_the_last_commit_until_push() -> anyhow::Result<()> {
    with_root(|repo| {
        repo_at(repo);
        let origin = format!("{repo}-origin");
        run_git(repo, &["init", "-q", "--bare", &origin]);
        run_git(repo, &["remote", "add", "origin", &origin]);
        run_git(repo, &["push", "-q", "-u", "origin", "main"]);
        let handle = cli_actor();
        let key = TabKey::next();
        let committed = |handle: &WorkspaceHandle| {
            let content = content_of(&handle.view(), key).unwrap_or_else(|e| panic!("{e}"));
            content.diff.meta.committed_unpushed
        };
        let commit = |handle: &WorkspaceHandle, text: &str| {
            write(repo, "src/a.rs", text);
            run_git(repo, &["add", "."]);
            let message = "next".to_string();
            send(handle, Command::Commit { key, message })
        };
        load_in(&handle, key, repo, DiffMode::Staged);

        assert_eq!(
            commit(&handle, "two\n"),
            written(key, "Committed! Ctrl+P to push")
        );
        assert!(committed(&handle), "the commit did not mark the view");
        assert_eq!(
            paths(&handle, key),
            ["src/a.rs"],
            "Staged is not the commit"
        );

        assert_eq!(
            send(&handle, Command::Push { key }),
            written(key, "Pushed!")
        );
        assert!(!committed(&handle), "the push kept the mark");
        assert!(
            paths(&handle, key).is_empty(),
            "Staged still shows the commit"
        );

        commit(&handle, "three\n");
        assert!(
            committed(&handle),
            "the second commit did not mark the view"
        );
        let mode = |mode| Command::SetDiffMode {
            key,
            mode,
            pr_number: None,
        };
        send(&handle, mode(DiffMode::Unstaged));
        send(&handle, mode(DiffMode::Staged));
        assert!(!committed(&handle), "the mode switch kept the mark");
        assert!(
            paths(&handle, key).is_empty(),
            "Staged still shows the commit"
        );
        Ok(())
    })
}
