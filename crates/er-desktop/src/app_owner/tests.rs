use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use er_protocol::{ResourceChanged, ResourceKey, ResourceVersion};

use super::{AppOwner, SidebarOwner};
use crate::projects::{self, ProjectRecord, ProjectsFile};
use crate::snapshot::PrInfo;

fn project(id: &str, root: &str, remote: Option<&str>) -> ProjectRecord {
    ProjectRecord {
        id: id.into(),
        name: id.into(),
        root_path: root.into(),
        remote: remote.map(Into::into),
        tracked_branches: vec!["feature".into()],
        ..Default::default()
    }
}

fn save(file: &ProjectsFile) {
    let key = er_engine::store_registry::app_key::<ProjectsFile>().unwrap();
    er_engine::store_registry::store()
        .write_durable(&key, file)
        .unwrap();
    projects::invalidate_load_cache();
}

fn owner() -> AppOwner {
    AppOwner::new(
        Arc::new(Mutex::new(HashMap::new())),
        Arc::new(Mutex::new(HashMap::new())),
        Arc::new(Mutex::new(Some("me".into()))),
    )
}

/// A repository on `main` with `feature` beside it, and `worktrees` checked out.
fn git(worktrees: Vec<er_git::Worktree>) -> er_git::FakeGit {
    er_git::FakeGit::default()
        .with_current_branch(Ok("main".into()))
        .with_rev_parse_verify(Ok(Some("0".repeat(40))))
        .with_list_worktrees(Ok(worktrees))
        .with_for_each_ref(Ok("main|origin/main\nfeature|origin/feature\n".into()))
        .with_branch_merged(Ok(String::new()))
}

fn feature_worktree() -> er_git::Worktree {
    er_git::Worktree {
        path: "/src/a-feature".into(),
        branch: "feature".into(),
        head: "1".repeat(40),
    }
}

fn branches_key(project: &str) -> ResourceKey {
    ResourceKey::Branches {
        project: project.into(),
    }
}

fn version(owner: &AppOwner, key: &ResourceKey) -> u64 {
    owner
        .versions()
        .into_iter()
        .find(|(k, _)| k == key)
        .map_or(0, |(_, version)| version)
}

fn in_storage(test: impl FnOnce()) {
    let tmp = tempfile::tempdir().unwrap();
    er_engine::storage::with_test_storage_root(tmp.path(), test);
}

/// A refresh that gives one project's branch a worktree moves that
/// project's branches and nothing else. The cache's fingerprint counts
/// branches and does not see the change, so a version that followed it
/// would stay put.
#[test]
fn a_refresh_moves_only_the_branches_it_changed() {
    in_storage(|| {
        save(&ProjectsFile {
            projects: vec![project("a", "/src/a", None), project("b", "/src/b", None)],
            active_id: Some("a".into()),
        });
        let owner = owner();
        owner.refresh_meta(&git(Vec::new()), "");
        let before = owner.versions();
        assert_eq!(before.len(), 3, "{before:?}");

        let fingerprint_moved = owner.refresh_project_meta(&git(vec![feature_worktree()]), "a");
        assert!(
            !fingerprint_moved,
            "the fixture must change what the fingerprint ignores"
        );

        let moved: Vec<ResourceKey> = owner
            .versions()
            .into_iter()
            .filter(|entry| !before.contains(entry))
            .map(|(key, _)| key)
            .collect();
        assert_eq!(moved, vec![branches_key("a")]);
        let (_, branches) = owner.branches("a");
        let feature = branches
            .local_branches
            .iter()
            .find(|branch| branch.name == "feature")
            .expect("feature is tracked");
        assert_eq!(feature.worktree_path.as_deref(), Some("/src/a-feature"));
    });
}

/// A publish over unchanged sources stamps nothing and answers nothing.
#[test]
fn a_publish_that_changes_nothing_moves_nothing() {
    in_storage(|| {
        save(&ProjectsFile {
            projects: vec![project("a", "/src/a", None)],
            active_id: None,
        });
        let owner = owner();
        assert_eq!(owner.publish().len(), 2);
        let before = owner.versions();
        assert!(owner.publish().is_empty());
        assert_eq!(owner.versions(), before);
    });
}

/// A project removed and registered again under its id comes back above
/// any version it had, and while it is gone its key reads empty at 0.
#[test]
fn a_project_that_returns_never_repeats_a_version() {
    in_storage(|| {
        let file = ProjectsFile {
            projects: vec![project("a", "/src/a", None)],
            active_id: None,
        };
        save(&file);
        let owner = owner();
        owner.refresh_meta(&git(Vec::new()), "");
        let first = version(&owner, &branches_key("a"));

        save(&ProjectsFile::default());
        owner.publish();
        assert_eq!(owner.branches("a"), (0, Default::default()));

        save(&file);
        owner.publish();
        assert!(version(&owner, &branches_key("a")) > first);
    });
}

fn pr(number: u64, head: &str, author: &str) -> PrInfo {
    serde_json::from_value(serde_json::json!({
        "number": number,
        "title": format!("PR {number}"),
        "head_ref": head,
        "state": "OPEN",
        "is_draft": false,
        "author": author,
    }))
    .unwrap()
}

/// A branch carries the number of the open PR whose head it is, among the
/// PRs the sidebar lists as mine or to review; a dismissed PR names none.
#[test]
fn a_branch_carries_its_open_pr_number() {
    in_storage(|| {
        let mut record = project("a", "/src/a", Some("acme/a"));
        save(&ProjectsFile {
            projects: vec![record.clone()],
            active_id: None,
        });
        let owner = owner();
        owner.refresh_meta(&git(Vec::new()), "");
        lock(&owner.pr_cache).insert("acme/a".into(), vec![pr(7, "feature", "someone")]);
        let changed = owner.publish();
        assert_eq!(
            changed,
            vec![ResourceChanged {
                key: branches_key("a"),
                version: ResourceVersion(version(&owner, &branches_key("a"))),
            }]
        );
        let number = |owner: &AppOwner| {
            owner
                .branches("a")
                .1
                .local_branches
                .iter()
                .find(|branch| branch.name == "feature")
                .and_then(|branch| branch.pr_number)
        };
        assert_eq!(number(&owner), Some(7));

        record.dismissed_prs = vec![7];
        save(&ProjectsFile {
            projects: vec![record],
            active_id: None,
        });
        owner.publish();
        assert_eq!(number(&owner), None);
    });
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap()
}

fn production_source(path: &str) -> String {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(root.join(path)).unwrap();
    // Unit tests sit at the bottom of each file, behind `#[cfg(test)]`.
    text.split("#[cfg(test)]\nmod tests {")
        .next()
        .unwrap_or_default()
        .to_string()
}

fn sources() -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut dirs = vec![std::path::PathBuf::from("src")];
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(root.join(&dir)).unwrap() {
            let path = dir.join(entry.unwrap().file_name());
            if root.join(&path).is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs")
                && !path.components().any(|part| part.as_os_str() == "tests")
                && !path.ends_with("tests.rs")
            {
                let name = path.to_string_lossy().into_owned();
                out.push((name.clone(), production_source(&name)));
            }
        }
    }
    out
}

/// The projects record has one writer, `projects.rs`, and the metadata
/// cache has one, the owner: no other file writes either.
#[test]
fn the_projects_record_and_the_metadata_cache_each_have_one_writer() {
    for (path, text) in sources() {
        if path != "src/projects.rs" {
            for writer in ["projects_pins::update", "ProjectsFile>()"] {
                assert!(
                    !text.contains(writer),
                    "{path} writes the projects record ({writer})"
                );
            }
        }
        if path != "src/app_owner.rs" && path != "src/snapshot.rs" {
            assert!(
                !text.contains("refresh_meta_cache"),
                "{path} writes the metadata cache"
            );
        }
    }
}

/// Which project and branch are active is the frontend's to derive from
/// `tabs`; no sidebar wire type or builder carries it.
#[test]
fn no_sidebar_resource_says_what_is_active() {
    let protocol =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../er-protocol/src/sidebar.rs");
    let protocol = std::fs::read_to_string(protocol).unwrap();
    for (path, text) in [
        ("er-protocol/src/sidebar.rs", protocol),
        ("src/app_owner.rs", production_source("src/app_owner.rs")),
    ] {
        for field in ["is_active", "is_current", "isActive", "isCurrent"] {
            assert!(!text.contains(field), "{path} carries {field}");
        }
    }
}

/// The owner reads its own sources and never the App, so it can move into
/// the service.
#[test]
fn the_owner_never_names_the_app() {
    let text = production_source("src/app_owner.rs");
    for name in [
        "AppState",
        "er_engine::app",
        "Mutex<App>",
        "crate::commands",
    ] {
        assert!(!text.contains(name), "app_owner.rs names {name}");
    }
}

/// On the flag the App holds only its startup tab, so the registry's tabs register projects.
#[test]
fn main_registers_projects_from_the_registry_on_the_flag_path() {
    let main = production_source("src/main.rs");
    let gate = "(not(feature = \"v1-service\"))]\n    {\n        projects::sync_projects";
    let block = &main[main.find(gate).expect("the tab sync is gated off the flag")..];
    let block = &block[..block.find("\n    }\n").unwrap()];
    for call in ["sync_projects_from_tabs", "auto_register("] {
        let once = main.matches(call).count() == 1;
        assert!(once && block.contains(call), "{call}");
    }
    assert!(main.contains("startup::register_open_projects(&state)"));
    assert!(main.contains("startup::meta_refresh_interval(&meta_state)"));
}
