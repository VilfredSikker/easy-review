//! The app registry: tab commands by key, one writer per app record, and the
//! open caches warmed through the ports.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use er_engine::ports::Ports;
use er_engine::pr_open_cache::load_persisted_pr_open_cache;
use er_engine::pr_open_entry::{PrOpenCacheKey, PrOpenFreshness};
use er_engine::tabs_record::{load_tabs, save_tabs, TabDescriptor, TabKind};
use er_git::FakeGit;
use er_github::FakeGitHub;

use super::github_sync::{calls_named, overview};
use crate::app_registry::{AppCommand, AppRegistry};
use crate::protocol::{Event, Reply};

const WAIT: Duration = Duration::from_secs(5);

fn working(root: &str) -> TabDescriptor {
    TabDescriptor {
        kind: TabKind::Working,
        repo_root: root.to_string(),
        branch: None,
        pr_owner: None,
        pr_repo: None,
        pr_number: None,
        pr_head_ref: None,
        base_ref: None,
        extra: serde_json::Map::new(),
    }
}

fn registry(github: FakeGitHub) -> (Arc<AppRegistry>, Arc<FakeGitHub>) {
    let github = Arc::new(github);
    let git = FakeGit::default().with_rev_parse_verify(Ok(Some("base-oid".to_string())));
    let ports = Ports {
        git: Arc::new(git),
        github: github.clone(),
    };
    (Arc::new(AppRegistry::new(ports)), github)
}

/// Polls `ready` until it holds or the deadline passes.
fn wait_for(mut ready: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + WAIT;
    while Instant::now() < deadline {
        if ready() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    ready()
}

#[test]
fn tab_commands_edit_the_list_by_key_and_save_it() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    er_engine::storage::with_test_storage_root(dir.path(), || -> anyhow::Result<()> {
        let mut browsed = working("/b");
        browsed
            .extra
            .insert("front_end_key".to_string(), serde_json::json!("kept"));
        save_tabs(&[working("/a"), browsed, working("/c")], 1)?;
        let (registry, _) = registry(FakeGitHub::default());
        let events = registry.subscribe();

        let loaded = registry.tabs_view();
        let keys: Vec<_> = loaded.tabs.iter().map(|tab| tab.key).collect();
        assert_eq!(keys.len(), 3);
        assert_eq!(loaded.active, Some(keys[1]));

        let reply = registry.send(AppCommand::SelectTab { key: keys[2] })?;
        let Reply::Tabs(view) = reply else {
            anyhow::bail!("select answered {reply:?}");
        };
        assert_eq!(view.active, Some(keys[2]));
        assert_eq!(
            *registry.tabs_view(),
            view,
            "the reply is the published view"
        );

        registry.send(AppCommand::ReorderTabs {
            key: keys[2],
            to_idx: 0,
        })?;
        registry.send(AppCommand::CloseTab { key: keys[2] })?;
        registry.send(AppCommand::NewTab { tab: working("/d") })?;
        let view = registry.tabs_view();
        let roots: Vec<&str> = view
            .tabs
            .iter()
            .map(|tab| tab.descriptor.repo_root.as_str())
            .collect();
        assert_eq!(roots, ["/a", "/b", "/d"]);
        assert_eq!(
            view.active_entry().map(|t| t.descriptor.repo_root.as_str()),
            Some("/d")
        );

        let saved = load_tabs().ok_or_else(|| anyhow::anyhow!("no saved record"))?;
        let descriptors: Vec<TabDescriptor> =
            view.tabs.iter().map(|tab| tab.descriptor.clone()).collect();
        assert_eq!(saved.tabs, descriptors);
        assert_eq!(saved.active_idx, 2);
        assert_eq!(
            saved.tabs[1].extra.get("front_end_key"),
            Some(&serde_json::json!("kept")),
            "a front end's keys survive the service's save"
        );

        let revisions: Vec<u64> = events
            .try_iter()
            .filter_map(|event| match event {
                Event::TabsChanged { revision } => Some(revision),
                _ => None,
            })
            .collect();
        assert_eq!(revisions, [1, 2, 3, 4]);
        Ok(())
    })
}

#[test]
fn a_tab_command_with_no_such_key_or_the_last_tab_is_refused() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    er_engine::storage::with_test_storage_root(dir.path(), || -> anyhow::Result<()> {
        save_tabs(&[working("/a")], 0)?;
        let (registry, _) = registry(FakeGitHub::default());
        let only = registry
            .tabs_view()
            .key_at(0)
            .ok_or_else(|| anyhow::anyhow!("no tab"))?;
        assert!(registry.send(AppCommand::CloseTab { key: only }).is_err());
        let gone = er_engine::TabKey::next();
        assert!(registry.send(AppCommand::SelectTab { key: gone }).is_err());
        assert_eq!(
            registry.tabs_view().revision,
            0,
            "a refused command publishes nothing"
        );
        Ok(())
    })
}

/// Puts a directory's permissions back when dropped, so a failing assertion
/// never leaves a read-only directory behind.
#[cfg(unix)]
struct RestorePermissions(std::path::PathBuf, std::fs::Permissions);

#[cfg(unix)]
impl Drop for RestorePermissions {
    fn drop(&mut self) {
        let _ = std::fs::set_permissions(&self.0, self.1.clone());
    }
}

/// A change whose save fails still takes effect at once, and the failure is
/// reported: holding it back until a later save let a retry close nothing or
/// open a duplicate tab.
#[cfg(unix)]
#[test]
fn a_tab_command_whose_save_fails_takes_effect_and_reports_it() -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir()?;
    er_engine::storage::with_test_storage_root(dir.path(), || -> anyhow::Result<()> {
        save_tabs(&[working("/a"), working("/b")], 0)?;
        let (registry, _) = registry(FakeGitHub::default());
        let keys: Vec<_> = registry.tabs_view().tabs.iter().map(|t| t.key).collect();
        let events = registry.subscribe();

        let record = er_engine::store_registry::app_path::<er_engine::tabs_record::TabsFile>()
            .ok_or_else(|| anyhow::anyhow!("no tabs path"))?;
        let app_dir = record
            .parent()
            .ok_or_else(|| anyhow::anyhow!("no app dir"))?
            .to_path_buf();
        let restore =
            RestorePermissions(app_dir.clone(), std::fs::metadata(&app_dir)?.permissions());
        std::fs::set_permissions(&app_dir, std::fs::Permissions::from_mode(0o555))?;

        let reply = registry.send(AppCommand::CloseTab { key: keys[1] });
        let Ok(Reply::Tabs(view)) = reply else {
            anyhow::bail!("the close answered {reply:?}");
        };
        assert_eq!(view.tabs.len(), 1);
        assert_eq!(*registry.tabs_view(), view, "the change is published now");
        let events: Vec<Event> = events.try_iter().collect();
        assert!(
            events.contains(&Event::TabsChanged { revision: 1 }),
            "{events:?}"
        );
        assert!(
            events
                .iter()
                .any(|event| matches!(event, Event::CommandFailed { .. })),
            "the failed save is reported: {events:?}"
        );

        drop(restore);
        registry.send(AppCommand::SelectTab { key: keys[0] })?;
        let saved = load_tabs().ok_or_else(|| anyhow::anyhow!("no saved record"))?;
        let roots: Vec<&str> = saved
            .tabs
            .iter()
            .map(|tab| tab.repo_root.as_str())
            .collect();
        assert_eq!(roots, ["/a"], "the next save writes the list as published");
        Ok(())
    })
}

/// `tabs.json` and `pr-open-cache.json` have one writer each in the app's
/// code: the engine's record modules, called from the desktop's flag-off
/// path and from this registry. The desktop's calls are compiled out with
/// `v1-service`, which its routing test checks.
#[test]
fn the_tabs_record_has_one_writer() -> anyhow::Result<()> {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut sources = Vec::new();
    for krate in ["er-engine", "er-desktop", "er-service", "er-tui", "er-mcp"] {
        collect_rust(&crates.join(krate).join("src"), &mut sources)?;
    }
    let allowed = |path: &Path, of: &[&str]| of.iter().any(|end| path.ends_with(end));
    for (path, text) in &sources {
        // Test code seeds records; only the app's own code is held to one writer.
        if path.components().any(|part| part.as_os_str() == "tests") || path.ends_with("tests.rs") {
            continue;
        }
        let text = text
            .split("#[cfg(test)]\nmod tests")
            .next()
            .unwrap_or_default();
        // Naming the record's key or path is what a direct store write needs.
        if text.contains("::<TabsFile>") {
            assert!(
                allowed(path, &["er-engine/src/tabs_record.rs"]),
                "{} keys the tabs record",
                path.display()
            );
        }
        if text.contains("save_tabs(") {
            assert!(
                allowed(
                    path,
                    &[
                        "er-engine/src/tabs_record.rs",
                        "er-desktop/src/tabs.rs",
                        "er-service/src/app_registry.rs",
                    ]
                ),
                "{} calls save_tabs",
                path.display()
            );
        }
        if text.contains("save_persisted_pr_open_cache(") {
            assert!(
                allowed(
                    path,
                    &[
                        "er-engine/src/pr_open_cache.rs",
                        "er-desktop/src/commands.rs",
                        "er-desktop/src/main.rs",
                        "er-service/src/app_registry.rs",
                    ]
                ),
                "{} saves the PR open cache",
                path.display()
            );
        }
    }
    Ok(())
}

fn collect_rust(dir: &Path, out: &mut Vec<(std::path::PathBuf, String)>) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_rust(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            let text = std::fs::read_to_string(&path)?;
            out.push((path, text));
        }
    }
    Ok(())
}

/// The service holds no front-end tab state: its tab list is the record. Doc
/// comments may still cite the engine's tab methods, so comment lines are
/// skipped.
#[test]
fn the_service_has_no_tabstate() -> anyhow::Result<()> {
    let mut sources = Vec::new();
    collect_rust(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut sources,
    )?;
    let probes = [["Tab", "State"].concat(), ["er_engine::", "app::"].concat()];
    for (path, text) in &sources {
        let code = text
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"));
        for (line, probe) in code.flat_map(|line| probes.iter().map(move |p| (line, p))) {
            assert!(
                !line.contains(probe.as_str()),
                "{} names the front end's tab state: {line}",
                path.display()
            );
        }
    }
    Ok(())
}

fn remote_github() -> FakeGitHub {
    FakeGitHub::default()
        .with_pr_head_oid_remote(Ok("head-1".to_string()))
        .with_overview_remote(Ok(Some(overview("octo"))))
        .with_pr_diff_remote(Ok("diff --git a/x b/x\n".to_string()))
        .with_commits_remote(Ok(Vec::new()))
}

#[test]
fn the_remote_cache_warms_through_the_github_port() -> anyhow::Result<()> {
    let (registry, github) = registry(remote_github());
    let warm = AppCommand::PrefetchRemoteOpenTarget {
        owner: "acme".to_string(),
        repo: "widgets".to_string(),
        number: 7,
    };
    assert_eq!(registry.send(warm)?, Reply::Acknowledged);
    assert!(wait_for(|| registry
        .remote_pr_open_entry("acme", "widgets", 7)
        .is_some()));
    let entry = registry
        .remote_pr_open_entry("acme", "widgets", 7)
        .ok_or_else(|| anyhow::anyhow!("not warmed"))?;
    assert_eq!(entry.raw_diff, "diff --git a/x b/x\n");
    assert_eq!(
        (entry.base_branch.as_str(), entry.head_branch.as_str()),
        ("main", "feature")
    );
    assert_eq!(entry.head_oid.as_deref(), Some("head-1"));
    let read = ["overview_remote", "pr_diff_remote", "commits_remote"];
    let mut seen = calls_named(&github, &read);
    seen.sort();
    assert_eq!(
        seen,
        ["commits_remote", "overview_remote", "pr_diff_remote"]
    );
    Ok(())
}

/// The remote cache is in-memory by design: warming it writes nothing under
/// the storage root, and a new registry starts without it.
#[test]
fn the_remote_cache_stays_in_memory() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    er_engine::storage::with_process_test_storage_root(dir.path(), || -> anyhow::Result<()> {
        let (registry, _) = registry(remote_github());
        let warm = AppCommand::PrefetchRemoteOpenTarget {
            owner: "acme".to_string(),
            repo: "widgets".to_string(),
            number: 7,
        };
        registry.send(warm)?;
        assert!(wait_for(|| registry
            .remote_pr_open_entry("acme", "widgets", 7)
            .is_some()));
        let mut written = Vec::new();
        collect_files(dir.path(), &mut written)?;
        assert!(written.is_empty(), "the remote cache wrote {written:?}");
        let (fresh, _) = registry_with_no_calls();
        assert!(fresh.remote_pr_open_entry("acme", "widgets", 7).is_none());
        Ok(())
    })
}

fn registry_with_no_calls() -> (Arc<AppRegistry>, Arc<FakeGitHub>) {
    registry(FakeGitHub::default())
}

fn collect_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_files(&path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}

#[test]
fn the_local_cache_warms_through_the_ports_and_prunes() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    er_engine::storage::with_process_test_storage_root(dir.path(), || -> anyhow::Result<()> {
        let (registry, github) = registry(
            FakeGitHub::default()
                .with_pr_diff(Ok("diff --git a/y b/y\n".to_string()))
                .with_pr_commits(Ok(Vec::new())),
        );
        let freshness = PrOpenFreshness {
            base_branch: "main".to_string(),
            head_branch: "feature".to_string(),
            head_oid: "head-1".to_string(),
            updated_at: "2026-11-15T00:00:00Z".to_string(),
        };
        let key = PrOpenCacheKey {
            project_id: "proj".to_string(),
            repo_root: "/repo".to_string(),
            pr_number: 7,
        };
        let warm = AppCommand::PrefetchOpenTarget {
            project_id: key.project_id.clone(),
            repo_root: key.repo_root.clone(),
            pr_number: 7,
            freshness: freshness.clone(),
        };
        assert_eq!(registry.send(warm)?, Reply::Acknowledged);
        let saved = || {
            load_persisted_pr_open_cache()
                .ok()
                .flatten()
                .is_some_and(|map| map.contains_key(&key))
        };
        assert!(wait_for(saved), "the warmed entry was not saved");
        let entry = registry
            .pr_open_entry(&key)
            .ok_or_else(|| anyhow::anyhow!("not warmed"))?;
        assert_eq!(entry.raw_diff, "diff --git a/y b/y\n");
        assert_eq!(entry.freshness, freshness);
        let mut seen = calls_named(&github, &["pr_diff", "pr_commits"]);
        seen.sort();
        assert_eq!(seen, ["pr_commits", "pr_diff"]);

        registry.prune_pr_open_cache(&HashSet::from([("proj".to_string(), 8)]));
        assert!(registry.pr_open_entry(&key).is_none());
        assert!(!saved(), "the pruned cache was not saved");
        Ok(())
    })
}
