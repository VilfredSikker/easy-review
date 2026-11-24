//! Reviewed writes on one bucket keep each other's marks: each, the load's
//! auto-unmark included, reads and saves under one hold of the scope lock.

use std::collections::HashMap;
use std::sync::{Arc, Barrier};
use std::time::Duration;

use er_engine::ports::Ports;
use er_engine::session_store::{self, ReviewedFiles};
use er_git::FakeGit;
use er_github::FakeGitHub;
use er_store::{ArtifactKey, Scope};

use crate::actor::WorkspaceDeps;
use crate::annotations::toggle_reviewed;
use crate::protocol::TabContext;

const RAW: &str = "\
diff --git a/src/a.rs b/src/a.rs
--- a/src/a.rs
+++ b/src/a.rs
@@ -1 +1 @@
-a-old
+a-new
diff --git a/src/b.rs b/src/b.rs
--- a/src/b.rs
+++ b/src/b.rs
@@ -1 +1 @@
-b-old
+b-new
";

fn deps() -> Arc<WorkspaceDeps> {
    deps_on(RAW)
}

fn deps_on(raw: &str) -> Arc<WorkspaceDeps> {
    let git = FakeGit::default().with_diff_against_branch(Ok(raw.to_string()));
    Arc::new(WorkspaceDeps::new(Ports {
        git: Arc::new(git),
        github: Arc::new(FakeGitHub::default()),
    }))
}

fn context(bucket: &str) -> TabContext {
    TabContext {
        er_dir: bucket.to_string(),
        comments_dir: bucket.to_string(),
        is_remote: false,
        repo_root: "/repo".to_string(),
        base_branch: "main".to_string(),
        scope: "branch".to_string(),
        remote_repo: None,
        pr_number: None,
        pr_head_ref: None,
        local_branch_view: Some("feature".to_string()),
        checkout_root: None,
        branch_diff_hash: String::new(),
        diff_hash: "diff-hash".to_string(),
    }
}

fn toggle_on_a_thread(
    deps: &Arc<WorkspaceDeps>,
    bucket: &str,
    path: &'static str,
    barrier: Option<Arc<Barrier>>,
) -> std::thread::JoinHandle<()> {
    let (deps, context) = (Arc::clone(deps), context(bucket));
    std::thread::spawn(move || {
        if let Some(barrier) = barrier {
            barrier.wait();
        }
        if let Err(error) = toggle_reviewed(&deps, &context, path) {
            panic!("toggle {path}: {error}");
        }
    })
}

fn marks(bucket: &str) -> anyhow::Result<HashMap<String, String>> {
    let store = er_engine::store_registry::store();
    Ok(session_store::load_reviewed(&store, &Scope::dir(bucket)?)?)
}

/// Two toggles on different files, started together from two threads, keep
/// both marks, round after round.
#[test]
fn two_toggles_at_once_keep_both_marks() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    er_engine::storage::with_process_test_storage_root(dir.path(), || -> anyhow::Result<()> {
        let deps = deps();
        for round in 0..100 {
            let bucket = dir.path().join(format!("bucket-{round}"));
            std::fs::create_dir_all(&bucket)?;
            let bucket = bucket.to_string_lossy().into_owned();
            let barrier = Arc::new(Barrier::new(2));
            let threads = ["src/a.rs", "src/b.rs"]
                .map(|path| toggle_on_a_thread(&deps, &bucket, path, Some(Arc::clone(&barrier))));
            for thread in threads {
                thread
                    .join()
                    .map_err(|_| anyhow::anyhow!("a toggle panicked"))?;
            }
            let marks = marks(&bucket)?;
            assert_eq!(marks.len(), 2, "round {round} lost a mark: {marks:?}");
        }
        Ok(())
    })
}

/// A toggle whose read lands while another writer holds the scope lock to
/// save waits for that save, and keeps the mark it saved.
#[test]
fn a_toggle_keeps_a_mark_saved_while_it_read() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let bucket = dir.path().join("bucket");
    std::fs::create_dir_all(&bucket)?;
    let bucket = bucket.to_string_lossy().into_owned();
    er_engine::storage::with_process_test_storage_root(dir.path(), || -> anyhow::Result<()> {
        let store = er_engine::store_registry::store();
        let scope = Scope::dir(&bucket)?;
        let guard = store.lock_scope(&scope, Duration::from_secs(5))?;
        let toggle = toggle_on_a_thread(&deps(), &bucket, "src/a.rs", None);
        std::thread::sleep(Duration::from_millis(300));
        let other = ReviewedFiles {
            files: HashMap::from([("src/b.rs".to_string(), "hash".to_string())]),
        };
        store.write_locked(&guard, &ArtifactKey::new(scope, ())?, &other)?;
        drop(guard);
        toggle
            .join()
            .map_err(|_| anyhow::anyhow!("the toggle panicked"))?;
        let marks = marks(&bucket)?;
        assert!(
            marks.contains_key("src/b.rs"),
            "the toggle lost b: {marks:?}"
        );
        assert!(
            marks.contains_key("src/a.rs"),
            "the toggle did not land: {marks:?}"
        );
        Ok(())
    })
}

/// A load's auto-unmark keeps a toggle saved after the load read the marks,
/// on disk and in the marks the load publishes.
#[test]
fn the_auto_unmark_keeps_a_toggle_saved_since_the_load_read() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let bucket = dir.path().join("bucket");
    std::fs::create_dir_all(&bucket)?;
    let bucket = bucket.to_string_lossy().into_owned();
    er_engine::storage::with_process_test_storage_root(dir.path(), || -> anyhow::Result<()> {
        let store = er_engine::store_registry::store();
        let scope = Scope::dir(&bucket)?;
        let stale = HashMap::from([("src/a.rs".to_string(), "stale".to_string())]);
        session_store::save_reviewed(&store, &scope, &stale)?;
        let mut read_by_the_load = session_store::load_reviewed(&store, &scope)?;
        toggle_reviewed(&deps(), &context(&bucket), "src/b.rs")
            .map_err(|error| anyhow::anyhow!("toggle: {error}"))?;
        let files = er_engine::git::parse_diff(RAW);
        let (cleared, _) = crate::effect::clear_moved_marks(
            &mut read_by_the_load,
            RAW,
            &files,
            true,
            &store,
            &scope,
        );
        assert_eq!(cleared, 1, "the stale mark was not cleared");
        let saved = marks(&bucket)?;
        assert!(
            saved.contains_key("src/b.rs"),
            "the toggle was lost: {saved:?}"
        );
        assert!(
            !saved.contains_key("src/a.rs"),
            "the stale mark stayed: {saved:?}"
        );
        assert_eq!(read_by_the_load, saved, "the load publishes other marks");
        Ok(())
    })
}

/// A file edited after the load fetched its diff and marked since keeps its
/// mark: the load judges only the marks it read.
#[test]
fn the_auto_unmark_keeps_a_mark_made_on_a_newer_diff() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let bucket = dir.path().join("bucket");
    std::fs::create_dir_all(&bucket)?;
    let bucket = bucket.to_string_lossy().into_owned();
    er_engine::storage::with_process_test_storage_root(dir.path(), || -> anyhow::Result<()> {
        let store = er_engine::store_registry::store();
        let scope = Scope::dir(&bucket)?;
        let stale = HashMap::from([("src/a.rs".to_string(), "stale".to_string())]);
        session_store::save_reviewed(&store, &scope, &stale)?;
        let mut read_by_the_load = session_store::load_reviewed(&store, &scope)?;
        let newer = RAW.replace("+b-new", "+b-newer");
        toggle_reviewed(&deps_on(&newer), &context(&bucket), "src/b.rs")
            .map_err(|error| anyhow::anyhow!("toggle: {error}"))?;
        let files = er_engine::git::parse_diff(RAW);
        crate::effect::clear_moved_marks(&mut read_by_the_load, RAW, &files, true, &store, &scope);
        let saved = marks(&bucket)?;
        assert!(
            saved.contains_key("src/b.rs"),
            "the newer mark was cleared: {saved:?}"
        );
        assert_eq!(read_by_the_load, saved, "the load publishes other marks");
        Ok(())
    })
}

/// A load whose marks all still hold takes no lock and saves nothing.
#[test]
fn an_auto_unmark_that_clears_nothing_takes_no_lock() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let bucket = dir.path().join("bucket");
    std::fs::create_dir_all(&bucket)?;
    let bucket = bucket.to_string_lossy().into_owned();
    er_engine::storage::with_process_test_storage_root(dir.path(), || -> anyhow::Result<()> {
        let store = er_engine::store_registry::store();
        let scope = Scope::dir(&bucket)?;
        let hash = er_engine::ai::compute_per_file_hash(RAW, "src/a.rs").unwrap_or_default();
        let held = HashMap::from([("src/a.rs".to_string(), hash)]);
        session_store::save_reviewed(&store, &scope, &held)?;
        let lock = std::path::Path::new(&bucket).join(".lock");
        std::fs::remove_file(&lock)?;
        let mut read_by_the_load = session_store::load_reviewed(&store, &scope)?;
        let files = er_engine::git::parse_diff(RAW);
        let (cleared, _) = crate::effect::clear_moved_marks(
            &mut read_by_the_load,
            RAW,
            &files,
            true,
            &store,
            &scope,
        );
        assert_eq!(cleared, 0, "a mark that holds was cleared");
        assert!(!lock.exists(), "a load that cleared nothing took the lock");
        Ok(())
    })
}
