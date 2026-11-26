use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use er_protocol::{ResourceChanged, ResourceKey, ResourceVersion};

use super::{AppOwner, SidebarOwner};
use crate::app_records::projects::{self, ProjectRecord, ProjectsFile};
use crate::app_records::sidebar::PrInfo;

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
        Arc::new(Mutex::new(HashMap::new())),
        Arc::new(Mutex::new(Some("me".into()))),
        Arc::new(Mutex::new(crate::app_records::inbox::InboxState::default())),
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
        let stamped = |key: ResourceKey| ResourceChanged {
            version: ResourceVersion(version(&owner, &key)),
            key,
        };
        assert_eq!(
            changed,
            vec![stamped(branches_key("a")), stamped(prs_key("acme/a"))]
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

fn prs_key(remote: &str) -> ResourceKey {
    ResourceKey::Prs {
        remote: remote.into(),
    }
}

/// A head probe's patch through the owner moves the remote's `prs` with no
/// tick between, and the row carries the new commit. The patch changes no
/// row count and no fetch time, so a version that followed those would stay.
#[test]
fn a_head_oid_patch_moves_the_remote_prs() {
    in_storage(|| {
        save(&ProjectsFile {
            projects: vec![project("a", "/src/a", Some("acme/a"))],
            active_id: None,
        });
        let owner = owner();
        let mut row = pr(7, "feature", "someone");
        row.head_oid = "1".repeat(40);
        lock(&owner.pr_cache).insert("acme/a".into(), vec![row, pr(8, "other", "me")]);
        lock(&owner.pr_fetched_at).insert("acme/a".into(), 1_000);
        owner.publish();
        let (before, held) = owner.prs("acme/a");
        assert!(before > 0);

        assert!(owner.patch_pr_head_oid("acme/a", 7, &"2".repeat(40)));

        let (after, prs) = owner.prs("acme/a");
        assert!(after > before, "the patch did not move prs(acme/a)");
        assert_eq!(version(&owner, &prs_key("acme/a")), after);
        assert_eq!(
            (prs.rows.len(), prs.fetched_at_ms),
            (held.rows.len(), held.fetched_at_ms),
            "the fixture must change only what a count-and-time key ignores"
        );
        let row = prs.rows.iter().find(|row| row.number == 7).unwrap();
        assert_eq!(row.head_oid, "2".repeat(40));
        assert!(!owner.patch_pr_head_oid("acme/a", 7, &"2".repeat(40)));
        assert_eq!(
            owner.prs("acme/a").0,
            after,
            "an unchanged oid moves nothing"
        );
    });
}

/// A saved or recent PR the cache does not hold becomes a title-only row,
/// so every number a list names has a row; one with no title is left out.
#[test]
fn an_uncached_saved_pr_has_a_title_only_row() {
    in_storage(|| {
        let mut record = project("a", "", Some("acme/a"));
        record.saved_prs = serde_json::from_value(serde_json::json!([
            {"number": 5, "saved_at_ms": 2, "title": "Saved"},
            {"number": 6, "saved_at_ms": 1, "title": ""},
        ]))
        .unwrap();
        save(&ProjectsFile {
            projects: vec![record],
            active_id: None,
        });
        let owner = owner();
        owner.publish();
        let (_, prs) = owner.prs("acme/a");
        assert_eq!(prs.projects.len(), 1);
        assert_eq!(prs.projects[0].saved, vec![5]);
        let titles: Vec<(u64, &str)> = prs
            .rows
            .iter()
            .map(|row| (row.number, row.title.as_str()))
            .collect();
        assert_eq!(titles, vec![(5, "Saved")]);
        assert_eq!(prs.fetched_at_ms, None);
    });
}

/// The owner keeps the worktree lists of roots an open tab shows and drops
/// the rest, so the keys do not grow with every tab ever opened.
#[test]
fn the_owner_keeps_the_worktrees_of_open_roots_only() {
    in_storage(|| {
        let owner = owner();
        let list = |path: &str| {
            super::worktree_rows(vec![crate::app_records::sidebar::WorktreeSnapshot {
                path: path.into(),
                branch: "main".into(),
                is_current: true,
                is_pr: false,
                pr_number: None,
                is_merged: false,
                remote: None,
            }])
        };
        let open = |roots: &[&str]| roots.iter().map(|r| r.to_string()).collect();
        owner.hold_worktrees("/a", list("/a"), &open(&["/a", "/b"]));
        owner.hold_worktrees("/b", list("/b"), &open(&["/a", "/b"]));
        owner.publish();
        assert!(owner.worktrees("/a").0 > 0);
        assert_eq!(owner.worktrees("/b").1.worktrees[0].path, "/b");

        owner.hold_worktrees("/c", list("/c"), &open(&["/b", "/c"]));
        owner.publish();
        assert_eq!(owner.worktrees("/a"), (0, Default::default()));
        assert!(owner.worktrees("/b").0 > 0);
        assert!(owner.worktrees("/c").0 > 0);
    });
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
        (
            "src/app_records/app_owner.rs",
            production_source("src/app_records/app_owner.rs"),
        ),
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
    let text = production_source("src/app_records/app_owner.rs");
    for name in [
        "AppState",
        "er_engine::app",
        "Mutex<App>",
        "crate::route_helpers",
    ] {
        assert!(!text.contains(name), "app_owner.rs names {name}");
    }
}

/// An inbox write publishes before it answers: the owner's version of
/// `inbox` moves with no publish between, and the read shows the write.
/// Before the owner reads the prefs it publishes no inbox at all.
#[test]
fn an_inbox_write_moves_the_inbox() {
    in_storage(|| {
        let owner = owner();
        owner.inbox.lock().unwrap().items = vec![inbox_item("a", "ci_failed")];
        owner.publish();
        assert_eq!(owner.inbox(), (0, er_protocol::Inbox::default()));
        assert_eq!(version(&owner, &ResourceKey::Inbox), 0);

        owner.read_inbox_prefs_from(|| Some(er_engine::config::InboxConfig::default()));
        owner.publish();
        let (before, inbox) = owner.inbox();
        assert!(before > 0);
        assert_eq!(inbox.unread_count, 1);

        owner
            .write_inbox(|inbox| {
                inbox.mark_item_read("a", 9);
            })
            .unwrap();
        let (after, inbox) = owner.inbox();
        assert!(after > before, "the write did not move inbox");
        assert_eq!(version(&owner, &ResourceKey::Inbox), after);
        assert_eq!(
            (inbox.unread_count, inbox.items[0].read_at_ms),
            (0, Some(9))
        );
    });
}

fn inbox_item(id: &str, kind: &str) -> crate::app_records::inbox::InboxItem {
    crate::app_records::inbox::InboxItem {
        id: id.into(),
        kind: kind.into(),
        severity: "info".into(),
        title: format!("title {id}"),
        body: format!("body {id}"),
        source: "github".into(),
        target: crate::app_records::inbox::InboxTarget {
            project_id: Some("a".into()),
            repo_root: None,
            remote: Some("acme/a".into()),
            pr_number: Some(7),
            branch: None,
            url: None,
        },
        created_at_ms: 1,
        read_at_ms: None,
        dedupe_key: id.into(),
    }
}

/// A PR-list refresh tells the change hook about each remote it fetched and
/// about the sweep's end, with their fields; the desktop logs what it hears.
#[test]
fn a_pr_list_refresh_reports_to_the_change_hook() {
    in_storage(|| {
        save(&ProjectsFile {
            projects: vec![project("a", "/src/a", Some("acme/a"))],
            active_id: None,
        });
        let owner = owner();
        let heard = Arc::new(Mutex::new(Vec::new()));
        let hears = Arc::clone(&heard);
        owner.set_change_hook(move |kind, fields| {
            let names: Vec<&str> = fields.iter().map(|(name, _)| *name).collect();
            lock(&hears).push(format!("{kind}: {}", names.join(" ")));
        });
        let github = |json: &[u8]| -> Arc<dyn er_engine::github::GitHub> {
            Arc::new(er_engine::github::FakeGitHub::default().with_pr_list_json(Ok(json.to_vec())))
        };

        assert!(owner.refresh_prs(github(b"[]")).is_empty());
        assert_eq!(
            *lock(&heard),
            [
                "pr_list_fetch: count remote ms",
                "pr_list_refresh_done: remotes ms"
            ]
        );

        lock(&heard).clear();
        assert!(owner.refresh_remote_prs(github(b"[]"), "acme/a"));
        assert_eq!(*lock(&heard), ["pr_list_fetch: count remote ms"]);
    });
}

/// Each kick starts through its guard and fetches on a thread of its own, so
/// a caller (a resource read, a command) never waits on `gh`. Reads the
/// source, so no `gh` runs.
#[test]
fn each_kick_fetches_on_its_own_thread_under_its_guard() {
    let text = production_source("src/app_records/app_owner/fetches.rs");
    for (name, begin, fetch) in [
        (
            "kick_pr_list_refresh",
            "begin_pr_list_refresh()",
            "refresh_prs(",
        ),
        (
            "kick_github_status_refresh",
            "begin_status_fetch(",
            "fetch_and_store_github_status(",
        ),
    ] {
        let pattern = format!("fn {name}(");
        let body = text.split_once(&pattern).map_or("", |(_, rest)| rest);
        let body = body.split("\n    }\n").next().unwrap_or_default();
        let begun = body
            .find(begin)
            .unwrap_or_else(|| panic!("{name} skips {begin}"));
        let spawned = body
            .find("std::thread::spawn(")
            .unwrap_or_else(|| panic!("{name} fetches on its caller's thread"));
        let fetched = body
            .find(fetch)
            .unwrap_or_else(|| panic!("{name} never fetches"));
        assert!(begun < spawned, "{name} spawns before {begin}");
        assert!(spawned < fetched, "{name} fetches on its caller's thread");
    }
}
