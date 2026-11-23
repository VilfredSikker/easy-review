//! The sidebar wiring: the startup registration and the metadata interval
//! read the registry, the tick warms and publishes from it, and the owner's
//! publishes reach the wire.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use er_engine::tabs_record::{TabDescriptor, TabKind};
use er_protocol::{Command, ErErrorKind, Resource, ResourceChanged, ResourceKey, ResourceVersion};
use er_service::AppCommand;

use super::{git_fixture_command, review_flow_test_state, ReviewFlowTestDir};
use crate::app_owner::startup::{meta_refresh_interval, register_open_projects};
use crate::app_owner::SidebarOwner;
use crate::commands::{content_routes, DesktopState};
use crate::projects::{self, ProjectRecord, ProjectsFile};
use crate::resources::{self, relay, tick, Watched};
use crate::service::ServiceState;
use crate::snapshot::PrInfo;

const WAIT: Duration = Duration::from_secs(10);

fn with_storage(test: impl FnOnce(&ReviewFlowTestDir)) {
    let dir = ReviewFlowTestDir::new();
    let storage = dir.path().join("storage");
    er_engine::storage::with_process_test_storage_root(storage, || test(&dir));
}

fn wait_until(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !ready() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn repo(dir: &ReviewFlowTestDir, name: &str) -> PathBuf {
    let root = dir.path().join(name);
    std::fs::create_dir_all(&root).unwrap();
    git_fixture_command(&root, &["init", "-q"]);
    root.canonicalize().unwrap()
}

fn working(root: &Path) -> TabDescriptor {
    TabDescriptor {
        kind: TabKind::Working,
        repo_root: root.to_string_lossy().into_owned(),
        branch: None,
        pr_owner: None,
        pr_repo: None,
        pr_number: None,
        pr_head_ref: None,
        base_ref: None,
        extra: Default::default(),
    }
}

fn new_tab(state: &DesktopState, tab: TabDescriptor) {
    let sent = state
        .service
        .app_registry()
        .send(AppCommand::NewTab { tab });
    assert!(sent.is_ok(), "{sent:?}");
}

/// The registry lists two saved tabs on two repositories. Git is real, so
/// registration resolves each root as the app does.
fn registry_fixture(dir: &ReviewFlowTestDir) -> (DesktopState, BTreeSet<String>) {
    let mut state = review_flow_test_state(dir);
    state.ports = er_engine::ports::Ports {
        git: er_engine::ports::Ports::cli().git,
        github: Arc::new(er_engine::github::FakeGitHub::default()),
    };
    let saved: BTreeSet<String> = ["saved-a", "saved-b"]
        .iter()
        .map(|name| {
            let root = repo(dir, name);
            new_tab(&state, working(&root));
            root.to_string_lossy().into_owned()
        })
        .collect();
    (state, saved)
}

fn registered_roots() -> BTreeSet<String> {
    projects::invalidate_load_cache();
    projects::load()
        .projects
        .into_iter()
        .map(|project| project.root_path)
        .collect()
}

/// The registration registers the project of every tab the registry lists.
#[test]
fn the_startup_registration_registers_the_registry_tabs() {
    with_storage(|dir| {
        let (state, saved) = registry_fixture(dir);
        register_open_projects(&state);
        assert_eq!(registered_roots(), saved);
        let (_, published) = state.app_owner.projects();
        assert_eq!(published.projects.len(), 2, "the registration publishes");
    });
}

/// The interval follows the registry's active tab: a remote PR waits twice
/// as long.
#[test]
fn the_meta_interval_reads_the_active_tab() {
    with_storage(|dir| {
        let state = review_flow_test_state(dir);
        new_tab(
            &state,
            TabDescriptor {
                kind: TabKind::RemotePr,
                pr_owner: Some("acme".into()),
                pr_repo: Some("widgets".into()),
                pr_number: Some(7),
                ..working(Path::new(""))
            },
        );
        assert_eq!(meta_refresh_interval(&state), Duration::from_secs(120));
        new_tab(&state, working(&dir.path().join("local")));
        assert_eq!(meta_refresh_interval(&state), Duration::from_secs(60));
    });
}

fn service_git() -> er_git::FakeGit {
    er_git::FakeGit::default()
        .with_current_branch(Ok("feature".to_string()))
        .with_detect_base_branch(Ok("main".to_string()))
        .with_diff_shortstat((0, 0, 0))
        .with_diff_raw(Ok(
            "diff --git a/f.rs b/f.rs\n--- a/f.rs\n+++ b/f.rs\n@@ -1 +1 @@\n-old\n+new\n".into(),
        ))
        .with_is_merge_in_progress(false)
        .with_remote_url(Ok("https://github.com/acme/widgets.git".to_string()))
}

/// A loaded tab on `loaded`, active in the registry; the desktop's git
/// records what the tick asks of it.
fn loaded_fixture(dir: &ReviewFlowTestDir) -> (DesktopState, String, Arc<er_git::FakeGit>) {
    let desktop_git = er_git::FakeGit::default()
        .with_list_worktrees(Ok(Vec::new()))
        .with_branch_merged(Ok(String::new()));
    loaded_with(dir, desktop_git)
}

/// `loaded_fixture` with `desktop_git` as the desktop's git.
fn loaded_with(
    dir: &ReviewFlowTestDir,
    desktop_git: er_git::FakeGit,
) -> (DesktopState, String, Arc<er_git::FakeGit>) {
    let mut state = review_flow_test_state(dir);
    let service_ports = er_engine::ports::Ports {
        git: Arc::new(service_git()),
        github: Arc::new(er_engine::github::FakeGitHub::default()),
    };
    let settings = dir.path().join("settings").join("config.toml");
    state.service = Arc::new(ServiceState::with_settings_path(service_ports, settings));
    let desktop_git = Arc::new(desktop_git);
    state.ports = er_engine::ports::Ports {
        git: desktop_git.clone(),
        github: Arc::new(er_engine::github::FakeGitHub::default()),
    };
    let root = dir.path().join("loaded");
    std::fs::create_dir_all(&root).unwrap();
    let root = root.to_string_lossy().into_owned();
    let opened = content_routes::open(&state, content_routes::working_tab(root.clone()), false);
    assert!(opened.is_ok(), "{opened:?}");
    let view = state.service.app_registry().tabs_view();
    let active = view.active_entry().unwrap().clone();
    wait_until("the tab to load", || {
        state
            .service
            .content(active.key, &active.descriptor)
            .is_some()
    });
    (state, root, desktop_git)
}

/// The tick warms the worktree list of the registry's active tab, with the
/// base its loaded content holds.
#[test]
fn the_tick_warms_the_registry_active_tab() {
    with_storage(|dir| {
        let (state, root, git) = loaded_fixture(dir);
        tick::run(&state);
        let warmed = format!("list_worktrees(repo_root={root:?})");
        wait_until("the warm of the active tab", || {
            git.calls().contains(&warmed)
        });
    });
}

fn worktree(path: &str, branch: &str) -> er_git::Worktree {
    er_git::Worktree {
        path: path.into(),
        branch: branch.into(),
        head: "1".repeat(40),
    }
}

/// The tick serves the active tab's worktrees as `build_worktrees` lists
/// them for the same root and base, less which one is current, and asks
/// which branches are merged into the base the loaded tab holds. A PR's merge
/// ref, a merged branch, a detached checkout and a worktree on a branch the
/// project dismissed all stay, which `branches(project)` cannot show.
#[test]
fn the_tick_serves_the_active_tab_worktrees_as_build_worktrees_lists_them() {
    with_storage(|dir| {
        let root = dir.path().join("loaded").to_string_lossy().into_owned();
        let git = er_git::FakeGit::default()
            .with_list_worktrees(Ok(vec![
                worktree(&root, "main"),
                worktree("/src/pr-7", "pr-7"),
                worktree("/src/old", "old"),
                worktree("/src/gone", "gone"),
                worktree("/src/loose", "(detached)"),
            ]))
            .with_branch_merged(Ok("old\n".into()))
            .with_config_get_regexp(Ok(
                "remote.origin.url https://github.com/acme/widgets.git\n\
                 branch.pr-7.merge refs/pull/7/head\n"
                    .into(),
            ));
        let (state, loaded, git) = loaded_with(dir, git);
        assert_eq!(loaded, root);
        save(vec![ProjectRecord {
            dismissed_branches: vec!["gone".into()],
            ..record("p", &root, Some("acme/widgets"))
        }]);
        tick::run(&state);
        // The fake answers the merged list for any base, so the rows alone
        // cannot show the tick asked with the loaded tab's base.
        let merged_asks: Vec<String> = git
            .calls()
            .into_iter()
            .filter(|call| call.starts_with("branch_merged("))
            .collect();
        let asked = format!("branch_merged(root={root:?}, base=\"main\")");
        assert!(
            !merged_asks.is_empty() && merged_asks.iter().all(|call| *call == asked),
            "the tick asked {merged_asks:?}, expected only {asked}"
        );

        let key = ResourceKey::Worktrees { root: root.clone() };
        let fetched = resources::fetch(&state, key.clone()).unwrap();
        assert!(fetched.version.0 > 0, "the tick published no worktrees");
        assert_eq!(resources::version(&state, key), Ok(fetched.version));
        let Resource::Worktrees(served) = fetched.value else {
            panic!("{:?}", fetched.value);
        };
        let ports = &state.ports;
        let listed =
            crate::snapshot::build_worktrees(&*ports.github, &*ports.git, &root, "main", &root);
        let listed: Vec<_> = listed
            .into_iter()
            .map(|w| {
                (
                    w.path,
                    w.branch,
                    w.is_pr,
                    w.pr_number,
                    w.is_merged,
                    w.remote,
                )
            })
            .collect();
        let served: Vec<_> = served
            .worktrees
            .into_iter()
            .map(|w| {
                (
                    w.path,
                    w.branch,
                    w.is_pr,
                    w.pr_number,
                    w.is_merged,
                    w.remote,
                )
            })
            .collect();
        assert_eq!(served, listed);
        let covers = |what: &str, found: bool| assert!(found, "the fixture covers {what}");
        covers(
            "a PR merge ref",
            listed.iter().any(|w| w.3 == Some(7) && w.2),
        );
        covers(
            "a merged branch",
            listed.iter().any(|w| w.1 == "old" && w.4),
        );
        covers(
            "a detached checkout",
            listed.iter().any(|w| w.1 == "(detached)"),
        );
        covers("a dismissed branch", listed.iter().any(|w| w.1 == "gone"));
        covers(
            "the remote",
            listed
                .iter()
                .all(|w| w.5.as_deref() == Some("acme/widgets")),
        );
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

fn save(projects: Vec<ProjectRecord>) {
    let key = er_engine::store_registry::app_key::<ProjectsFile>().unwrap();
    er_engine::store_registry::store()
        .write_durable(
            &key,
            &ProjectsFile {
                projects,
                active_id: None,
            },
        )
        .unwrap();
    projects::invalidate_load_cache();
}

/// `main` checked out, `feature` in its own worktree, `old` merged.
fn meta_git() -> er_git::FakeGit {
    er_git::FakeGit::default()
        .with_current_branch(Ok("main".into()))
        .with_rev_parse_verify(Ok(Some("0".repeat(40))))
        .with_list_worktrees(Ok(vec![er_git::Worktree {
            path: "/src/feature-tree".into(),
            branch: "feature".into(),
            head: "1".repeat(40),
        }]))
        .with_for_each_ref(Ok("main|origin/main\nfeature|origin/feature\nold|\n".into()))
        .with_branch_merged(Ok("old\n".into()))
}

fn record(id: &str, root: &str, remote: Option<&str>) -> ProjectRecord {
    ProjectRecord {
        id: id.into(),
        name: id.into(),
        root_path: root.into(),
        remote: remote.map(Into::into),
        tracked_branches: vec!["feature".into()],
        auto_triage_max_diff_kb: 64,
        review_ignore_globs: vec!["*.lock".into()],
        ..Default::default()
    }
}

fn branches_version(state: &DesktopState, project: &str) -> u64 {
    state.app_owner.branches(project).0
}

/// A PR-cache write the owner does not make reaches `branches(project)` at
/// the next tick, which publishes.
#[test]
fn a_pr_cache_write_reaches_the_branches_within_one_tick() {
    with_storage(|dir| {
        let state = review_flow_test_state(dir);
        save(vec![record("p", "/src/p", Some("acme/p"))]);
        state.app_owner.refresh_meta(&meta_git(), "");
        let before = branches_version(&state, "p");
        state
            .pr_cache
            .lock()
            .unwrap()
            .insert("acme/p".into(), vec![pr(9, "feature", "someone")]);
        tick::run(&state);
        assert!(
            branches_version(&state, "p") > before,
            "the tick did not publish"
        );
        let (_, branches) = state.app_owner.branches("p");
        let feature = branches.local_branches.iter().find(|b| b.name == "feature");
        assert_eq!(feature.and_then(|b| b.pr_number), Some(9));
    });
}

/// An owner publish wakes the relay, which hands the moved sidebar keys on,
/// and the host serves what was published.
#[test]
fn an_owner_publish_reaches_the_wire() {
    with_storage(|dir| {
        let mut state = review_flow_test_state(dir);
        let (wake, wakes) = mpsc::channel();
        let settings = dir.path().join("settings").join("config.toml");
        let service =
            ServiceState::with_settings_path(er_engine::ports::Ports::for_test(), settings);
        state.service = Arc::new(service.with_resource_wake(wake.clone()));
        state.app_owner.set_wake(wake);
        std::thread::sleep(Duration::from_millis(100));
        while wakes.try_recv().is_ok() {}
        let (tx, bursts) = mpsc::channel();
        relay::spawn(state.clone(), wakes, move |moved| {
            let _ = tx.send(moved);
        });

        save(vec![record("p", "", Some("acme/p"))]);
        state.app_owner.publish();
        let moved = bursts
            .recv_timeout(WAIT)
            .expect("no burst after the publish");
        let keys: Vec<Watched> = moved.into_iter().map(|(key, _)| key).collect();
        assert!(
            keys.contains(&Watched::Sidebar(ResourceKey::Projects)),
            "{keys:?}"
        );
        let fetched = resources::fetch(&state, ResourceKey::Projects).unwrap();
        let Resource::Projects(projects) = fetched.value else {
            panic!("{:?}", fetched.value);
        };
        assert_eq!(projects.projects[0].id, "p");
        assert!(projects.projects[0].remote_only);
    });
}

/// The desktop's git for the project commands: `main` checked out, with
/// `feature` and `spare` beside it, every branch present.
fn command_git() -> er_git::FakeGit {
    er_git::FakeGit::default()
        .with_show_ref_verify(Ok(true))
        .with_current_branch_show(Ok("main".into()))
        .with_current_branch(Ok("main".into()))
        .with_rev_parse_verify(Ok(Some("0".repeat(40))))
        .with_list_worktrees(Ok(Vec::new()))
        .with_for_each_ref(Ok("main\nfeature\nspare\n".into()))
        .with_branch_merged(Ok(String::new()))
}

/// The registered projects `a` and `b`, published, with the command git.
fn command_fixture(dir: &ReviewFlowTestDir) -> DesktopState {
    let mut state = review_flow_test_state(dir);
    state.ports = er_engine::ports::Ports {
        git: Arc::new(command_git()),
        github: Arc::new(er_engine::github::FakeGitHub::default()),
    };
    let untracked = |id: &str| ProjectRecord {
        tracked_branches: Vec::new(),
        ..record(id, &format!("/src/{id}"), None)
    };
    save(vec![untracked("a"), untracked("b")]);
    state.app_owner.refresh_meta(&command_git(), "");
    state
}

fn project_ids(state: &DesktopState) -> Vec<String> {
    let fetched = resources::fetch(&state, ResourceKey::Projects).unwrap();
    let Resource::Projects(projects) = fetched.value else {
        panic!("{:?}", fetched.value);
    };
    projects.projects.into_iter().map(|p| p.id).collect()
}

/// A project command through `er_send` answers the versions it moved, so
/// the frontend refetches them before the push.
#[test]
fn a_project_command_answers_the_versions_it_moved() {
    with_storage(|dir| {
        let state = command_fixture(dir);
        let ack = resources::send(
            &state,
            Command::ReorderProjects {
                ordered: vec!["b".into(), "a".into()],
            },
        )
        .unwrap();
        let (version, _) = state.app_owner.projects();
        assert_eq!(
            ack.changed,
            vec![ResourceChanged {
                key: ResourceKey::Projects,
                version: ResourceVersion(version),
            }]
        );
        assert_eq!(project_ids(&state), ["b", "a"]);

        resources::send(
            &state,
            Command::DeleteProject {
                project: "a".into(),
            },
        )
        .unwrap();
        assert_eq!(project_ids(&state), ["b"]);
    });
}

/// `er_send`'s body hands a project command to the sidebar's owner and
/// leaves the settings actor unstarted.
#[test]
fn er_send_routes_a_project_command_to_the_sidebar_owner() {
    with_storage(|dir| {
        let state = command_fixture(dir);
        let applied = resources::send::apply(
            &state,
            Command::ReorderProjects {
                ordered: vec!["b".into(), "a".into()],
            },
            &resources::send::Effects {
                notify: &|_, _| {},
                check_update: || panic!("no update check in this test"),
                webviews: &crate::browser_webview::NoWebviews,
                pick_folder: &|| None,
            },
        )
        .unwrap();
        let (version, _) = state.app_owner.projects();
        assert_eq!(
            applied.ack.changed,
            vec![ResourceChanged {
                key: ResourceKey::Projects,
                version: ResourceVersion(version),
            }]
        );
        assert!(applied.uninstalled.is_none());
        assert_eq!(project_ids(&state), ["b", "a"]);
        assert!(state.service.settings_if_started().is_none());
    });
}

/// Tracking a branch refreshes its project's metadata before the answer,
/// so the `Ack` names that project's branches and they list the branch.
#[test]
fn tracking_a_branch_answers_its_branches() {
    with_storage(|dir| {
        let state = command_fixture(dir);
        let ack = resources::send(
            &state,
            Command::AddTrackedBranch {
                project: "a".into(),
                name: "spare".into(),
            },
        )
        .unwrap();
        let keys: Vec<ResourceKey> = ack.changed.into_iter().map(|c| c.key).collect();
        assert!(
            keys.contains(&ResourceKey::Branches {
                project: "a".into()
            }),
            "{keys:?}"
        );
        let (_, branches) = state.app_owner.branches("a");
        let local: Vec<&str> = branches
            .local_branches
            .iter()
            .map(|b| b.name.as_str())
            .collect();
        assert_eq!(local, ["main", "spare"]);

        resources::send(
            &state,
            Command::RemoveTrackedBranch {
                project: "a".into(),
                name: "spare".into(),
            },
        )
        .unwrap();
        let (_, branches) = state.app_owner.branches("a");
        assert_eq!(branches.local_branches.len(), 1);
    });
}

/// `available_branches(project)` is computed on read: the local branches
/// neither checked out nor tracked, at version 0.
#[test]
fn the_available_branches_are_computed_on_read() {
    with_storage(|dir| {
        let state = command_fixture(dir);
        resources::send(
            &state,
            Command::AddTrackedBranch {
                project: "a".into(),
                name: "feature".into(),
            },
        )
        .unwrap();
        let key = ResourceKey::AvailableBranches {
            project: "a".into(),
        };
        let fetched = resources::fetch(&state, key.clone()).unwrap();
        assert_eq!(fetched.version, ResourceVersion(0));
        let Resource::AvailableBranches(available) = fetched.value else {
            panic!("{:?}", fetched.value);
        };
        assert_eq!(available.branches, ["spare"]);
        assert_eq!(resources::version(&state, key), Ok(ResourceVersion(0)));
    });
}

/// A command or read naming a project the record does not hold is not
/// found; one the backend declines is refused.
#[test]
fn a_project_command_on_an_unknown_project_is_not_found() {
    with_storage(|dir| {
        let state = command_fixture(dir);
        let missing = resources::send(
            &state,
            Command::DeleteProject {
                project: "gone".into(),
            },
        );
        assert_eq!(missing.map_err(|e| e.kind), Err(ErErrorKind::NotFound));
        let read = resources::fetch(
            &state,
            ResourceKey::AvailableBranches {
                project: "gone".into(),
            },
        );
        assert_eq!(read.map_err(|e| e.kind), Err(ErErrorKind::NotFound));
        let checked_out = resources::send(
            &state,
            Command::RemoveTrackedBranch {
                project: "a".into(),
                name: "main".into(),
            },
        );
        assert_eq!(checked_out.map_err(|e| e.kind), Err(ErErrorKind::Refused));
    });
}
