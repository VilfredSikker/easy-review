//! The PR lists: `prs(remote)` holds each project's lists, the PR commands go
//! through `er_send` to the sidebar's owner, which applies the dismissed
//! filter and the saved cap, and the head probe's target follows the
//! registry.

use std::time::{Duration, Instant};

use er_engine::tabs_record::{TabDescriptor, TabKind};
use er_protocol::{Command, ErErrorKind, Prs, Resource, ResourceKey, ResourceVersion};
use er_service::AppCommand;

use super::{review_flow_test_state, ReviewFlowTestDir};
use crate::app_owner::startup::pr_head_probe_target;
use crate::page_view::PrInfo;
use crate::resources;
use crate::state::DesktopState;
use er_service::app_records::projects::{self, ProjectRecord, ProjectsFile};

fn with_storage(test: impl FnOnce(&ReviewFlowTestDir)) {
    let dir = ReviewFlowTestDir::new();
    let storage = dir.path().join("storage");
    er_engine::storage::with_process_test_storage_root(storage, || test(&dir));
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

fn record(id: &str, root: &str, remote: &str) -> ProjectRecord {
    ProjectRecord {
        id: id.into(),
        name: id.into(),
        root_path: root.into(),
        remote: Some(remote.into()),
        ..Default::default()
    }
}

fn pr(number: u64, state: &str, author: &str) -> PrInfo {
    serde_json::from_value(serde_json::json!({
        "number": number,
        "title": format!("PR {number}"),
        "head_ref": format!("branch-{number}"),
        "head_oid": format!("{number:040}"),
        "state": state,
        "is_draft": false,
        "author": author,
        "merged_at": (state == "MERGED").then(|| format!("2026-01-01T00:00:{number:02}Z")),
    }))
    .unwrap()
}

fn fetch_prs(state: &DesktopState, remote: &str) -> (u64, Prs) {
    let key = ResourceKey::Prs {
        remote: remote.into(),
    };
    let fetched = resources::fetch(state, key).unwrap();
    let Resource::Prs(prs) = fetched.value else {
        panic!("{:?}", fetched.value);
    };
    (fetched.version.0, prs)
}

fn lists(prs: &Prs, project: &str) -> er_protocol::ProjectPrs {
    prs.projects
        .iter()
        .find(|lists| lists.project == project)
        .cloned()
        .unwrap_or_else(|| panic!("no lists for {project}"))
}

/// Project `p` on `acme/p`, signed in as `me`: one PR of mine, two to
/// review and one merged.
fn command_fixture(dir: &ReviewFlowTestDir) -> DesktopState {
    let mut state = review_flow_test_state(dir);
    state.ports = er_engine::ports::Ports {
        git: std::sync::Arc::new(er_git::FakeGit::default()),
        github: std::sync::Arc::new(er_engine::github::FakeGitHub::default()),
    };
    save(vec![record("p", "/src/p", "acme/p")]);
    *state.gh_user.lock().unwrap() = Some("me".into());
    state.pr_cache.lock().unwrap().insert(
        "acme/p".into(),
        vec![
            pr(1, "OPEN", "me"),
            pr(2, "OPEN", "someone"),
            pr(3, "OPEN", "someone"),
            pr(4, "MERGED", "someone"),
        ],
    );
    state.app_owner.publish();
    state
}

fn send(state: &DesktopState, command: Command) -> er_protocol::Ack {
    resources::send::apply(
        state,
        command,
        &resources::send::Effects {
            notify: &|_, _| {},
            check_update: || panic!("no update check in this test"),
            webviews: &crate::browser_webview::NoWebviews,
            pick_folder: &|| None,
        },
    )
    .unwrap()
    .ack
}

/// A dismiss through `er_send` leaves the PR out of mine, to review and
/// recently merged on the server, keeps its row for the frontend's own
/// lookups, lists it as dismissed, and names the moved `prs` in the `Ack`.
#[test]
fn a_dismissed_pr_leaves_the_lists_on_the_server() {
    with_storage(|dir| {
        let state = command_fixture(dir);
        let (before, prs) = fetch_prs(&state, "acme/p");
        let shown = lists(&prs, "p");
        assert_eq!(
            (shown.mine, shown.to_review, shown.recently_merged),
            (vec![1], vec![2, 3], vec![4])
        );

        for number in [1, 3, 4] {
            send(
                &state,
                Command::DismissPr {
                    project: "p".into(),
                    number,
                },
            );
        }
        let ack = send(
            &state,
            Command::DismissPr {
                project: "p".into(),
                number: 2,
            },
        );
        let (after, prs) = fetch_prs(&state, "acme/p");
        assert!(after > before);
        assert!(ack.changed.iter().any(|changed| changed.key
            == ResourceKey::Prs {
                remote: "acme/p".into()
            }
            && changed.version == ResourceVersion(after)));
        let shown = lists(&prs, "p");
        assert!(shown.mine.is_empty(), "{shown:?}");
        assert!(shown.to_review.is_empty(), "{shown:?}");
        assert!(shown.recently_merged.is_empty(), "{shown:?}");
        assert_eq!(shown.dismissed, vec![1, 3, 4, 2]);
        assert_eq!(prs.rows.len(), 4, "dismissed rows stay");

        send(
            &state,
            Command::UndismissPr {
                project: "p".into(),
                number: 2,
            },
        );
        let (_, prs) = fetch_prs(&state, "acme/p");
        assert_eq!(lists(&prs, "p").to_review, vec![2]);
    });
}

/// The saved list keeps today's rule on the server: each save goes to the
/// front, and the 51st drops the oldest.
#[test]
fn the_51st_save_drops_the_oldest() {
    with_storage(|dir| {
        let state = command_fixture(dir);
        for number in 1..=51 {
            send(
                &state,
                Command::SavePr {
                    project: "p".into(),
                    number,
                    title: Some(format!("Saved {number}")),
                },
            );
        }
        let saved = &projects::load().projects[0].saved_prs;
        assert_eq!(saved.len(), 50);
        assert_eq!(saved[0].number, 51);
        assert!(!saved.iter().any(|entry| entry.number == 1));
        let (_, prs) = fetch_prs(&state, "acme/p");
        let listed = lists(&prs, "p").saved;
        assert_eq!(listed.len(), 50);
        assert!(!listed.contains(&1), "{listed:?}");

        send(
            &state,
            Command::UnsavePr {
                project: "p".into(),
                number: 51,
            },
        );
        let (_, prs) = fetch_prs(&state, "acme/p");
        assert_eq!(lists(&prs, "p").saved.len(), 49);
    });
}

/// The save-PR median on the resource path: `er_send` `save_pr` to its
/// `Ack`, then `er_fetch` of what it names and the serialisation. Each run
/// saves a PR the list does not hold, so the version moves; an untimed unsave
/// follows.
#[test]
#[ignore = "latency harness: run with --ignored --nocapture and paste the numbers in the PR; never a CI assertion"]
fn the_save_pr_reports_its_median() {
    const RUNS: usize = 50;
    const WARMUP: usize = 5;
    with_storage(|dir| {
        let state = command_fixture(dir);
        let mut samples = Vec::with_capacity(RUNS);
        let mut bytes = 0;
        for run in 0..RUNS {
            let started = Instant::now();
            let ack = send(
                &state,
                Command::SavePr {
                    project: "p".into(),
                    number: 7,
                    title: Some("Saved 7".into()),
                },
            );
            let fetched: Vec<_> = ack
                .changed
                .into_iter()
                .map(|changed| resources::fetch(&state, changed.key))
                .collect::<Result<_, _>>()
                .unwrap_or_else(|e| panic!("fetch: {e:?}"));
            bytes = serde_json::to_vec(&fetched).map(|v| v.len()).unwrap_or(0);
            if run >= WARMUP {
                samples.push(started.elapsed());
            }
            send(
                &state,
                Command::UnsavePr {
                    project: "p".into(),
                    number: 7,
                },
            );
        }
        samples.sort_unstable();
        let median = samples[samples.len() / 2];
        println!("save PR: median {median:?}, answer {bytes} bytes");
    });
}

/// `available_prs(project)` is computed on read at version 0: the cached PRs
/// the project neither tracks nor dismissed that are not the user's.
#[test]
fn the_available_prs_are_computed_on_read() {
    with_storage(|dir| {
        let state = command_fixture(dir);
        send(
            &state,
            Command::TrackPr {
                project: "p".into(),
                number: 2,
            },
        );
        let key = ResourceKey::AvailablePrs {
            project: "p".into(),
        };
        let fetched = resources::fetch(&state, key.clone()).unwrap();
        assert_eq!(fetched.version, ResourceVersion(0));
        let Resource::AvailablePrs(available) = fetched.value else {
            panic!("{:?}", fetched.value);
        };
        let numbers: Vec<u64> = available.prs.iter().map(|pr| pr.number).collect();
        assert_eq!(numbers, vec![3, 4]);
        send(
            &state,
            Command::UntrackPr {
                project: "p".into(),
                number: 2,
            },
        );
        assert_eq!(projects::load().projects[0].tracked_prs, Vec::<u64>::new());
        let missing = resources::fetch(
            &state,
            ResourceKey::AvailablePrs {
                project: "gone".into(),
            },
        );
        assert_eq!(missing.map_err(|e| e.kind), Err(ErErrorKind::NotFound));
    });
}

/// Each PR command reaches the sidebar's owner through `er_send`'s body,
/// none is answered as another owner's, and one naming a project the
/// record does not hold is not found.
#[test]
fn every_pr_command_reaches_the_sidebar_owner() {
    with_storage(|dir| {
        let state = command_fixture(dir);
        let project = || "p".to_string();
        let commands = [
            Command::RefreshPrList,
            Command::RefreshProjectPrList { project: project() },
            Command::DismissPr {
                project: project(),
                number: 2,
            },
            Command::UndismissPr {
                project: project(),
                number: 2,
            },
            Command::TrackPr {
                project: project(),
                number: 3,
            },
            Command::UntrackPr {
                project: project(),
                number: 3,
            },
            Command::SavePr {
                project: project(),
                number: 2,
                title: None,
            },
            Command::UnsavePr {
                project: project(),
                number: 2,
            },
        ];
        for command in commands {
            let applied = resources::send::apply(
                &state,
                command.clone(),
                &resources::send::Effects {
                    notify: &|_, _| {},
                    check_update: || panic!("no update check in this test"),
                    webviews: &crate::browser_webview::NoWebviews,
                    pick_folder: &|| None,
                },
            );
            assert!(applied.is_ok(), "{command:?}: {applied:?}");
        }
        let missing = resources::send::apply(
            &state,
            Command::RefreshProjectPrList {
                project: "gone".into(),
            },
            &resources::send::Effects {
                notify: &|_, _| {},
                check_update: || panic!("no update check in this test"),
                webviews: &crate::browser_webview::NoWebviews,
                pick_folder: &|| None,
            },
        );
        assert_eq!(
            missing.map(|_| ()).map_err(|e| e.kind),
            Err(ErErrorKind::NotFound)
        );
        // The refresh fetches on its own thread, which writes the cache's
        // file: it finishes inside the test's storage root.
        let deadline = Instant::now() + Duration::from_secs(10);
        while state
            .pr_list_running
            .load(std::sync::atomic::Ordering::Acquire)
        {
            assert!(Instant::now() < deadline, "the refresh never finished");
            std::thread::sleep(Duration::from_millis(5));
        }
    });
}

/// The head probe's target is the registry's active PR tab.
#[test]
fn the_probe_target_reads_the_registry() {
    with_storage(|dir| {
        let state = review_flow_test_state(dir);
        let tab = TabDescriptor {
            kind: TabKind::RemotePr,
            repo_root: String::new(),
            branch: None,
            pr_owner: Some("acme".into()),
            pr_repo: Some("widgets".into()),
            pr_number: Some(7),
            pr_head_ref: None,
            base_ref: None,
            extra: Default::default(),
        };
        let sent = state
            .service
            .app_registry()
            .send(AppCommand::NewTab { tab });
        assert!(sent.is_ok(), "{sent:?}");
        assert_eq!(
            pr_head_probe_target(&state),
            Some(("acme/widgets".to_string(), 7))
        );
    });
}
