//! The flag-on PR lists: `prs(remote)` holds the snapshot's lists, the PR
//! commands go through `er_send` to the sidebar's owner, which applies the
//! dismissed filter and the saved cap, and the head probe's target follows
//! the registry with the App locked.

use std::time::{Duration, Instant};

use er_engine::tabs_record::{TabDescriptor, TabKind};
use er_protocol::{Command, ErErrorKind, Prs, Resource, ResourceKey, ResourceVersion};
use er_service::AppCommand;

use super::{review_flow_test_state, snap_from, ReviewFlowTestDir};
use crate::app_owner::startup::pr_head_probe_target;
use crate::commands::AppState;
use crate::projects::{self, ProjectRecord, ProjectsFile};
use crate::resources;
use crate::snapshot::PrInfo;

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

fn fetch_prs(state: &AppState, remote: &str) -> (u64, Prs) {
    let key = ResourceKey::Prs {
        remote: remote.into(),
    };
    let fetched = resources::fetch(&state.desktop, key).unwrap();
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
fn command_fixture(dir: &ReviewFlowTestDir) -> AppState {
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

fn send(state: &AppState, command: Command) -> er_protocol::Ack {
    resources::send::apply(
        &state.desktop,
        command,
        &resources::send::Effects {
            notify: &|_, _| {},
            follow_selection: &|_| {},
            check_update: || panic!("no update check in this test"),
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

/// Every project's lists in `prs(remote)` name what the snapshot's project
/// rows list, in their order, and the rows carry the fields the snapshot's
/// rows show: two projects share a remote and dismiss differently, one is
/// known only by its remote, and more PRs merged than the list keeps.
#[test]
fn the_remote_prs_match_the_snapshot_lists() {
    with_storage(|dir| {
        let state = review_flow_test_state(dir);
        let active_root = state.app.lock().unwrap().tab().repo_root.clone();
        let entries = |numbers: &[(u64, u64)], at: &str| -> serde_json::Value {
            numbers
                .iter()
                .map(|(number, ms)| {
                    serde_json::json!({"number": number, at: ms, "title": format!("T {number}")})
                })
                .collect()
        };
        let mut active = record("active", &active_root, "acme/widgets");
        active.dismissed_prs = vec![3, 100];
        active.saved_prs =
            serde_json::from_value(entries(&[(5, 1), (99, 2)], "saved_at_ms")).unwrap();
        active.recent_prs =
            serde_json::from_value(entries(&[(1, 2), (6, 1)], "viewed_at_ms")).unwrap();
        let twin = record("twin", "/src/twin", "acme/widgets");
        let mut remote = record("remote", "", "acme/gadgets");
        remote.saved_prs = serde_json::from_value(entries(&[(11, 1)], "saved_at_ms")).unwrap();
        save(vec![active, twin, remote]);
        *state.gh_user.lock().unwrap() = Some("me".into());
        let mut approved = pr(6, "OPEN", "someone");
        approved.latest_reviewer_states = vec![("me".into(), "APPROVED".into())];
        let mut draft = pr(4, "OPEN", "someone");
        draft.is_draft = true;
        let mut widgets = vec![
            pr(1, "OPEN", "me"),
            pr(3, "OPEN", "someone"),
            draft,
            pr(5, "OPEN", "someone"),
            approved,
        ];
        widgets.extend((100..127).map(|number| pr(number, "MERGED", "someone")));
        state.pr_cache.lock().unwrap().extend([
            ("acme/widgets".to_string(), widgets),
            ("acme/gadgets".to_string(), vec![pr(12, "OPEN", "me")]),
        ]);
        state.app_owner.publish();

        let snapshot = {
            let app = state.app.lock().unwrap();
            snap_from(&app, &state)
        };
        let numbers = |prs: &[PrInfo]| prs.iter().map(|pr| pr.number).collect::<Vec<_>>();
        for row in &snapshot.projects {
            let remote = row.remote.as_deref().unwrap();
            let (_, prs) = fetch_prs(&state, remote);
            let wire = lists(&prs, &row.id);
            let id = &row.id;
            assert_eq!(wire.mine, numbers(&row.my_prs), "{id} mine");
            assert_eq!(
                wire.to_review,
                numbers(&row.prs_to_review),
                "{id} to review"
            );
            assert_eq!(
                wire.recently_merged,
                numbers(&row.recently_merged),
                "{id} merged"
            );
            assert_eq!(wire.saved, numbers(&row.saved_prs), "{id} saved");
            assert_eq!(wire.recent, numbers(&row.recent_prs), "{id} recent");
            assert_eq!(wire.dismissed, row.dismissed_prs, "{id} dismissed");
            let row_of = |number: u64| prs.rows.iter().find(|r| r.number == number).unwrap();
            let listed = row.my_prs.iter().chain(&row.prs_to_review);
            for pr in listed.chain(&row.recently_merged) {
                assert_eq!(
                    row_of(pr.number).approved_by_me,
                    pr.approved_by_me,
                    "{id} #{}",
                    pr.number
                );
            }
            let shown = row
                .my_prs
                .iter()
                .chain(&row.prs_to_review)
                .chain(&row.saved_prs);
            for pr in shown.chain(&row.recent_prs).chain(&row.recently_merged) {
                let wire = row_of(pr.number);
                assert_eq!(
                    (&wire.title, &wire.head_ref, &wire.state, &wire.head_oid),
                    (&pr.title, &pr.head_ref, &pr.state, &pr.head_oid),
                    "{id} #{}",
                    pr.number
                );
            }
        }
        let active = &snapshot.projects[0];
        assert_eq!(
            numbers(&active.saved_prs),
            vec![99, 5],
            "the fixture orders saves"
        );
        assert_eq!(
            active.recently_merged.len(),
            25,
            "the fixture fills the cap"
        );
        assert_eq!(numbers(&snapshot.projects[1].prs_to_review), vec![3, 5]);
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
        let fetched = resources::fetch(&state.desktop, key.clone()).unwrap();
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
            &state.desktop,
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
                &state.desktop,
                command.clone(),
                &resources::send::Effects {
                    notify: &|_, _| {},
                    follow_selection: &|_| {},
                    check_update: || panic!("no update check in this test"),
                },
            );
            assert!(applied.is_ok(), "{command:?}: {applied:?}");
        }
        let missing = resources::send::apply(
            &state.desktop,
            Command::RefreshProjectPrList {
                project: "gone".into(),
            },
            &resources::send::Effects {
                notify: &|_, _| {},
                follow_selection: &|_| {},
                check_update: || panic!("no update check in this test"),
            },
        );
        assert_eq!(
            missing.map(|_| ()).map_err(|e| e.kind),
            Err(ErErrorKind::NotFound)
        );
        // The refresh fetches on its own thread, which writes the cache's
        // file: it finishes inside the test's storage root.
        let deadline = Instant::now() + Duration::from_secs(10);
        while state.loading.lock().unwrap().pr_list {
            assert!(Instant::now() < deadline, "the refresh never finished");
            std::thread::sleep(Duration::from_millis(5));
        }
    });
}

/// The head probe's target is the registry's active PR tab, read with the
/// App locked. Read from the App, a held lock skips the probe.
#[test]
fn the_probe_target_reads_the_registry_with_the_app_locked() {
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
        let _held = state.app.lock().unwrap();
        assert_eq!(
            pr_head_probe_target(&state),
            Some(("acme/widgets".to_string(), 7))
        );
    });
}
