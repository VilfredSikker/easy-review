//! Tab content: a load publishes the sections, a tick shares them, and a key
//! the actor never loaded is refused before any effect.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use er_engine::ports::Ports;
use er_engine::tabs_record::{TabDescriptor, TabKind};
use er_engine::TabKey;
use er_git::FakeGit;
use er_github::FakeGitHub;

use crate::actor::WorkspaceDeps;
use crate::guard::TRIPS;
use crate::handle::{ActorMsg, WorkspaceError};
use crate::protocol::{Command, Reply};
use crate::workspace::WorkspaceView;
use crate::{spawn, WorkspaceHandle};

const RAW: &str = "\
diff --git a/src/a.rs b/src/a.rs
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,2 +1,2 @@
 a-one
-a-old
+a-new
diff --git a/src/b.rs b/src/b.rs
--- a/src/b.rs
+++ b/src/b.rs
@@ -1,3 +1,3 @@
 b-one
-b-old
+b-new
 b-three
";

const WAIT: Duration = Duration::from_secs(5);

fn git(raw: &str) -> Arc<FakeGit> {
    Arc::new(
        FakeGit::default()
            .with_current_branch(Ok("feature".to_string()))
            .with_diff_raw(Ok(raw.to_string())),
    )
}

fn actor_over(git: Arc<FakeGit>) -> WorkspaceHandle {
    let github: Arc<dyn er_github::GitHub> = Arc::new(FakeGitHub::default());
    let git: Arc<dyn er_git::Git> = git;
    spawn(WorkspaceDeps::new(Ports { git, github }))
}

/// A plain tab on `repo` with a saved base, so a load reads the checkout's
/// branch and the diff and nothing else.
fn working_tab(repo: &str) -> TabDescriptor {
    TabDescriptor {
        kind: TabKind::Working,
        repo_root: repo.to_string(),
        branch: None,
        pr_owner: None,
        pr_repo: None,
        pr_number: None,
        pr_head_ref: None,
        base_ref: Some("main".to_string()),
        extra: serde_json::Map::new(),
    }
}

fn load(handle: &WorkspaceHandle, key: TabKey, repo: &str) -> anyhow::Result<Reply> {
    Ok(handle.send_and_wait_with(
        Command::LoadTab {
            key,
            tab: working_tab(repo),
            mode: None,
        },
        WAIT,
    )?)
}

/// Runs `test` with a storage root every effect thread sees, and a repo dir
/// under it.
fn with_root(test: impl FnOnce(&str) -> anyhow::Result<()>) -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(&repo)?;
    let repo = repo.to_string_lossy().into_owned();
    er_engine::storage::with_process_test_storage_root(dir.path().join("storage"), || test(&repo))
}

fn content_of(view: &WorkspaceView, key: TabKey) -> anyhow::Result<Arc<crate::TabContent>> {
    view.tab(key)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("no content under {key:?}"))
}

#[test]
fn a_loaded_tab_publishes_its_diff_and_context() -> anyhow::Result<()> {
    with_root(|repo| {
        let handle = actor_over(git(RAW));
        let key = TabKey::next();
        let trips = TRIPS.load(Ordering::SeqCst);

        let reply = load(&handle, key, repo)?;

        assert_eq!(
            reply,
            Reply::ContentLoaded {
                key,
                reviewed_cleared: 0
            }
        );
        assert_eq!(
            TRIPS.load(Ordering::SeqCst),
            trips,
            "the load reached a port on the actor thread"
        );
        let content = content_of(&handle.view(), key)?;
        let paths: Vec<&str> = content.diff.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["src/a.rs", "src/b.rs"]);
        let hash = er_engine::ai::compute_diff_hash(RAW);
        assert_eq!(content.diff.meta.diff_hash, hash);
        assert_eq!(content.context.diff_hash, hash);
        assert_eq!(content.context.branch_diff_hash, hash);
        assert_eq!(content.context.scope, "branch");
        assert_eq!(content.context.repo_root, repo);
        assert!(!content.context.er_dir.is_empty());
        Ok(())
    })
}

/// An arena tick republishes the view; the sections behind it must be the
/// same allocations, or every tick would copy every open diff.
#[test]
fn an_arena_tick_shares_every_section() -> anyhow::Result<()> {
    with_root(|repo| {
        let handle = actor_over(git(RAW));
        let key = TabKey::next();
        load(&handle, key, repo)?;
        let before = handle.view();

        let _ = handle.tx.send(ActorMsg::ArenaChanged);
        let deadline = Instant::now() + WAIT;
        while handle.view().revision == before.revision {
            anyhow::ensure!(Instant::now() < deadline, "the tick never published");
            std::thread::yield_now();
        }

        let (old, new) = (content_of(&before, key)?, content_of(&handle.view(), key)?);
        assert!(
            Arc::ptr_eq(&old.diff, &new.diff),
            "the tick copied the diff"
        );
        assert!(Arc::ptr_eq(&old.reviewed, &new.reviewed));
        assert!(Arc::ptr_eq(&old.ai, &new.ai));
        Ok(())
    })
}

#[test]
fn a_tab_command_for_a_key_never_loaded_is_refused_before_any_effect() -> anyhow::Result<()> {
    with_root(|_| {
        let git = git(RAW);
        let handle = actor_over(Arc::clone(&git));
        let before = handle.view().revision;

        for command in [
            Command::RefreshDiff {
                key: TabKey::next(),
                force: false,
            },
            Command::SetDiffMode {
                key: TabKey::next(),
                mode: er_engine::tab_identity::DiffMode::Unstaged,
            },
            Command::ParseFiles {
                key: TabKey::next(),
                paths: vec!["src/a.rs".to_string()],
            },
        ] {
            let result = handle.send_and_wait_with(command, WAIT);
            assert_eq!(
                result,
                Err(WorkspaceError::Failed("no loaded tab has that key".into()))
            );
        }

        let view = handle.view();
        assert_eq!(view.revision, before, "a refused command published");
        assert_eq!(view.last_error, None);
        assert!(git.calls().is_empty(), "a refused command ran git");
        Ok(())
    })
}

/// A full load drops the marks whose file left the diff or changed since it
/// was marked, and saves the marks that are left.
#[test]
fn a_load_clears_moved_and_orphaned_marks_and_saves_them() -> anyhow::Result<()> {
    with_root(|repo| {
        let handle = actor_over(git(RAW));
        let key = TabKey::next();
        load(&handle, key, repo)?;
        let er_dir = content_of(&handle.view(), key)?.context.er_dir.clone();
        let store = er_engine::store_registry::store();
        let scope = er_store::Scope::dir(&er_dir)?;
        let b_hash = er_engine::ai::compute_per_file_hash(RAW, "src/b.rs").unwrap_or_default();
        let marks = [
            ("src/a.rs", "an older hash".to_string()),
            ("src/b.rs", b_hash.clone()),
            ("src/gone.rs", "left the diff".to_string()),
        ]
        .into_iter()
        .map(|(path, hash)| (path.to_string(), hash))
        .collect();
        er_engine::session_store::save_reviewed(&store, &scope, &marks)?;

        let reply = handle.send_and_wait_with(Command::RefreshDiff { key, force: false }, WAIT)?;

        assert_eq!(
            reply,
            Reply::ContentLoaded {
                key,
                reviewed_cleared: 2
            }
        );
        let saved = er_engine::session_store::load_reviewed(&store, &scope)?;
        assert_eq!(saved.len(), 1);
        assert_eq!(saved.get("src/b.rs"), Some(&b_hash));
        let content = content_of(&handle.view(), key)?;
        assert_eq!(content.reviewed.marks, saved);
        Ok(())
    })
}
