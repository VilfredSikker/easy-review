//! The desktop's task host: `jobs` lists each task's row
//! and moves with its status and never with an output line, which
//! `task_log` reads; a stop reaches the runner; a queued task can be
//! removed.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use er_engine::task_target::BackgroundTaskTarget;
use er_engine::TabKey;
use er_protocol::{Command, Resource, ResourceKey, TaskJob};
use er_service::test_support::{open_gate, Agent};
use er_service::WorkspaceDeps;

use super::ai_ask_tests::jobs;
use super::arena_resource_tests::{bucket, local_ports, open, wait_until};
use super::diff_command_tests::send;
use super::diff_resource_tests::{fake_ports, opened, raw_diff, with_storage};
use super::{review_flow_test_state, ReviewFlowTestDir};
use crate::service::ServiceState;
use crate::state::DesktopState;

/// Drops the queued tasks, opens both gates and waits for every task to
/// end, so no agent outlives the test. One that did would hold a slot of
/// the process's pool for a minute, and a later test at a cap of one would
/// wait on it.
fn release(state: &DesktopState, agent: &Agent) {
    for row in tasks(state)
        .into_iter()
        .filter(|row| row.status == "queued")
    {
        state.tasks.cancel_queued(&row.id);
    }
    open_gate(&agent.first);
    open_gate(&agent.second);
    wait_until("every task to end", || {
        tasks(state)
            .iter()
            .all(|row| !matches!(row.status.as_str(), "queued" | "running"))
    });
}

/// A state whose tasks run the gated agent, under `cap` running reviews.
fn tasking(dir: &ReviewFlowTestDir, cap: usize) -> (DesktopState, Agent) {
    tasking_on(dir, cap, local_ports())
}

/// `tasking` on the git `ports` give.
fn tasking_on(
    dir: &ReviewFlowTestDir,
    cap: usize,
    ports: er_engine::ports::Ports,
) -> (DesktopState, Agent) {
    let agent = Agent::in_dir(dir.path());
    let path = agent.settings.join("config.toml");
    std::fs::create_dir_all(&agent.settings).unwrap_or_else(|e| panic!("{e}"));
    std::fs::write(&path, agent.config(cap)).unwrap_or_else(|e| panic!("config: {e}"));
    let mut state = review_flow_test_state(dir);
    state.ports = ports.clone();
    let deps = WorkspaceDeps::new(ports);
    state.service = Arc::new(ServiceState::with_deps(deps, |_| {}).with_config_path(path));
    (state, agent)
}

fn tasks(state: &DesktopState) -> Vec<TaskJob> {
    jobs(state).tasks
}

fn version(state: &DesktopState) -> u64 {
    crate::resources::jobs::version(state)
}

fn log(state: &DesktopState, id: &str) -> Vec<String> {
    let key = ResourceKey::TaskLog { id: id.to_string() };
    match crate::resources::fetch(state, key).map(|f| f.value) {
        Ok(Resource::TaskLog(log)) => log.lines.into_iter().map(|l| l.text).collect(),
        other => panic!("not a task log: {other:?}"),
    }
}

/// A task of `kind` on a bucket under `dir`, run with the settings view's
/// config and saved pick, queued on a thread. Answers its id.
fn queued(state: &DesktopState, dir: &Path, kind: &str) -> String {
    let launch = launch(state, dir, kind, "feature");
    super::on_a_thread(state, "the enqueue", move |state| {
        crate::task_hooks::enqueue(state, launch)
    })
    .unwrap_or_else(|e| panic!("{e}"))
}

/// A task of `kind` on `branch`, on a bucket under `dir`.
fn launch(state: &DesktopState, dir: &Path, kind: &str, branch: &str) -> er_service::Launch {
    let config = crate::arena_service::config(state);
    let (provider_id, model_id, effort) = er_service::card_asks::run::selection(&config);
    let bucket = dir.join("bucket");
    std::fs::create_dir_all(&bucket).unwrap_or_else(|e| panic!("bucket: {e}"));
    er_service::Launch {
        kind: kind.to_string(),
        command_name: kind.to_string(),
        target: BackgroundTaskTarget {
            repo_root: dir.to_string_lossy().into_owned(),
            er_dir: bucket.to_string_lossy().into_owned(),
            branch_label: branch.into(),
            base_branch: "main".into(),
            scope: "branch".into(),
            pr_number: None,
            remote_repo: None,
            managed_local: true,
        },
        prompt: "Review the change".into(),
        prepared_diff: false,
        host_write_diagram: None,
        scoped_stamp: None,
        config,
        selection: er_engine::config::AiSelection {
            provider_id,
            model_id,
            effort,
        },
        quiet: false,
    }
}

fn row(state: &DesktopState, id: &str) -> Option<TaskJob> {
    tasks(state).into_iter().find(|row| row.id == id)
}

#[cfg(test)]
fn pid_alive(pid: &str) -> bool {
    std::process::Command::new("kill")
        .args(["-0", pid])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// An output line lands in `task_log` and leaves `jobs`' version alone;
/// the task finishing moves it.
#[test]
fn an_output_line_moves_no_jobs_version() {
    with_storage(|dir| {
        let (state, agent) = tasking(dir, 50);
        let id = queued(&state, dir.path(), "review");
        assert!(row(&state, &id).is_some(), "{:?}", tasks(&state));
        wait_until("the first line", || {
            log(&state, &id).iter().any(|line| line.contains("line-a"))
        });
        let before = version(&state);
        open_gate(&agent.first);
        wait_until("the second line", || {
            log(&state, &id).iter().any(|line| line.contains("line-b"))
        });
        assert_eq!(version(&state), before, "an output line moved jobs");
        open_gate(&agent.second);
        wait_until("the task to finish", || {
            row(&state, &id).is_some_and(|row| row.status != "running")
        });
        assert!(version(&state) > before, "the finish did not move jobs");
    });
}

/// `CancelRunningTask` reaches the runner, as P43's `CancelJob` does: the
/// agent's process is gone and the task ends failed.
#[test]
fn a_stop_reaches_the_runner() {
    with_storage(|dir| {
        let (state, agent) = tasking(dir, 50);
        let id = queued(&state, dir.path(), "review");
        let deadline = Instant::now() + Duration::from_secs(10);
        let pid = loop {
            if let Some(pid) = std::fs::read_to_string(&agent.pid)
                .ok()
                .filter(|text| text.ends_with('\n'))
            {
                break pid.trim().to_string();
            }
            assert!(Instant::now() < deadline, "the agent never started");
            std::thread::sleep(Duration::from_millis(20));
        };
        assert!(pid_alive(&pid));

        let stop = Command::CancelRunningTask { id: id.clone() };
        let stopped = send(&state, "cancel_running_task", stop);
        assert!(stopped.is_ok(), "{stopped:?}");
        let deadline = Instant::now() + Duration::from_secs(5);
        while pid_alive(&pid) {
            assert!(
                Instant::now() < deadline,
                "the agent's process outlived the stop"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        wait_until("the task to end", || {
            row(&state, &id).is_some_and(|row| row.status == "failed")
        });
        release(&state, &agent);
    });
}

/// Under a cap of one, a second task waits queued, and removing it drops its
/// row from `jobs`.
#[test]
fn a_queued_task_is_removed_from_jobs() {
    with_storage(|dir| {
        let (state, agent) = tasking(dir, 1);
        let _first = queued(&state, dir.path(), "review");
        let second = queued(&state, dir.path(), "tour");
        assert_eq!(
            row(&state, &second).map(|row| row.status),
            Some("queued".to_string())
        );

        let cancel = Command::CancelQueuedTask { id: second.clone() };
        let removed = send(&state, "cancel_queued_task", cancel);
        assert!(removed.is_ok(), "{removed:?}");
        assert!(row(&state, &second).is_none(), "{:?}", tasks(&state));
        release(&state, &agent);
    });
}

/// An importance run that finishes well has the settings view load the
/// repo's rules again, so `risk(key)` moves, and refreshes the loaded tabs on
/// the repo, so their rows take the committed tiers at once. The test commits
/// the table and finishes a queued importance task by hand: a run that
/// commits a table needs a real provider CLI.
#[test]
fn a_finished_importance_run_refilters_the_loaded_tabs_on_its_repo() {
    with_storage(|dir| {
        let (state, agent) = tasking_on(dir, 1, fake_ports());
        let (key, tab) = opened(&state, dir, "one");
        let _first = queued(&state, dir.path(), "review");
        let mut importance = launch(
            &state,
            dir.path(),
            er_engine::ai::prompts::IMPORTANCE_TASK_KIND,
            "feature",
        );
        importance.target.repo_root.clone_from(&tab.repo_root);
        let importance = super::on_a_thread(&state, "the enqueue", move |state| {
            crate::task_hooks::enqueue(state, importance)
        })
        .unwrap_or_else(|e| panic!("{e}"));
        let tier = || {
            let diff = ResourceKey::Diff {
                tab: er_service::wire_tab_key(key),
            };
            match crate::resources::fetch(&state, diff).map(|f| f.value) {
                Ok(Resource::Diff(diff)) => (diff.files.iter())
                    .find(|f| f.path == "src/f2.rs")
                    .map(|f| f.risk_tier.clone()),
                other => panic!("not a diff: {other:?}"),
            }
        };
        assert_eq!(tier().as_deref(), Some("ai"));
        let committed = er_engine::config::RiskRuleProposal {
            repo: "widgets".into(),
            rules: [("src/f2.rs".to_string(), "low".into())].into(),
            ..Default::default()
        };
        let store = er_engine::store_registry::store();
        assert!(committed.commit_to_store("widgets", &store).is_ok());

        crate::task_hooks::finish(&state, &importance, Ok(()));

        let view = state.service.settings().view();
        assert!(
            view.risk_for(&tab.repo_root, None).is_some(),
            "rules unloaded"
        );
        wait_until("the tab's rows to take the committed tier", || {
            tier().as_deref() == Some("low")
        });
        // The review still runs on the gated agent: end it before the
        // storage goes, so it frees the process's one slot.
        release(&state, &agent);
    });
}

/// The host and its commands never name the App, so no task read or write
/// waits on it.
#[test]
fn the_task_host_names_no_app() {
    for (name, text) in [
        (
            "tasks.rs",
            include_str!("../../../../er-service/src/tasks.rs"),
        ),
        ("resources/runs.rs", include_str!("../../resources/runs.rs")),
    ] {
        for word in [
            "AppState",
            "background_task_log_tail",
            "app.lock",
            "current_ai_",
        ] {
            assert!(!text.contains(word), "{name} names {word}");
        }
    }
}

/// A review run through `er_send` reads the tab's
/// content: the diff it prepares in the tab's bucket is the diff the content
/// holds, and `jobs` lists its task under that bucket and scope.
#[test]
fn a_review_run_prepares_the_content_diff() {
    with_storage(|dir| {
        let (state, agent) = tasking(dir, 50);
        let tab = open(&state, dir.path(), "a");
        let bucket = bucket(&state, &tab);
        let command = Command::RunReview {
            tab: er_service::wire_tab_key(tab.0),
            scope: "branch".into(),
        };
        let ack = send(&state, "run_review", command).unwrap_or_else(|e| panic!("{e:?}"));
        assert!(
            ack.changed.iter().any(|c| c.key == ResourceKey::Jobs),
            "{ack:?}"
        );
        let task = tasks(&state)
            .into_iter()
            .find(|row| row.kind == "review")
            .unwrap_or_else(|| panic!("no review row"));
        assert_eq!(
            (task.er_dir.as_str(), task.scope.as_str()),
            (bucket.as_str(), "branch")
        );
        let prepared = std::fs::read_to_string(Path::new(&bucket).join("diff-tmp"));
        assert_eq!(prepared.ok().as_deref(), Some(raw_diff().as_str()));
        release(&state, &agent);
    });
}

/// The tour and a scoped triage each start.
#[test]
fn the_run_commands_start() {
    with_storage(|dir| {
        let (state, agent) = tasking(dir, 50);
        let tab: (TabKey, _) = open(&state, dir.path(), "a");
        let _ = bucket(&state, &tab);
        let started = super::on_a_thread(&state, "the runs", |state| {
            let tour = crate::runs::tour(state, None).map_err(|e| e.message);
            let reviewers = vec!["triage".to_string()];
            let triage =
                crate::runs::scoped(state, None, "branch".into(), Vec::new(), reviewers, None)
                    .map_err(|e| e.message);
            (tour.map(drop), triage.map(drop))
        });
        assert_eq!(started, (Ok(()), Ok(())));
        let kinds: Vec<String> = tasks(&state).into_iter().map(|row| row.kind).collect();
        assert!(kinds.contains(&"tour".to_string()), "{kinds:?}");
        assert!(kinds.contains(&"triage".to_string()), "{kinds:?}");
        release(&state, &agent);
    });
}

/// Each run command on `er_send` starts its own run:
/// `jobs` lists a row of the command's kind, and a scoped run keeps its
/// reviewers and paths.
#[test]
fn each_run_command_through_er_send_lists_its_kind_in_jobs() {
    with_storage(|dir| {
        let (state, agent) = tasking(dir, 50);
        let tab = open(&state, dir.path(), "a");
        let bucket = bucket(&state, &tab);
        let wire = er_service::wire_tab_key(tab.0);
        let commands = [
            ("tour", Command::GenerateTour { tab: wire }),
            ("importance", Command::RunImportanceAgent { tab: wire }),
            (
                "diagram:flows",
                Command::GenerateDiagram {
                    tab: wire,
                    diagram_kind: "flows".into(),
                    custom_prompt: None,
                },
            ),
            (
                "expert:security",
                Command::RunExpertReview {
                    tab: wire,
                    scope: "branch".into(),
                    expert_id: "security".into(),
                },
            ),
            (
                "triage",
                Command::RunScopedReview {
                    tab: wire,
                    scope: "branch".into(),
                    paths: vec!["src/f1.rs".into()],
                    reviewers: vec!["triage".into()],
                    focus_prompt: None,
                },
            ),
        ];
        for (kind, command) in commands {
            let ack = send(&state, "a run", command).unwrap_or_else(|e| panic!("{kind}: {e:?}"));
            assert!(ack.changed.iter().any(|c| c.key == ResourceKey::Jobs));
            let kinds: Vec<String> = tasks(&state).into_iter().map(|row| row.kind).collect();
            assert!(kinds.iter().any(|k| k == kind), "{kind}: {kinds:?}");
        }
        let listed = std::fs::read_to_string(Path::new(&bucket).join("review-files.txt"));
        assert_eq!(listed.ok().as_deref(), Some("src/f1.rs\n"));
        release(&state, &agent);
    });
}

/// A scoped run through `er_send` tells the person once, with its own
/// message, as the App's scoped arm does; its tasks send no started notice
/// of their own.
#[test]
fn a_scoped_run_sends_one_notice() {
    with_storage(|dir| {
        let (state, agent) = tasking(dir, 50);
        let tab = open(&state, dir.path(), "a");
        let _ = bucket(&state, &tab);
        let command = Command::RunScopedReview {
            tab: er_service::wire_tab_key(tab.0),
            scope: "branch".into(),
            paths: Vec::new(),
            reviewers: vec!["triage".into(), "general".into()],
            focus_prompt: None,
        };
        let told = super::on_a_thread(&state, "the scoped run", move |state| {
            let effects = crate::resources::send::Effects {
                notify: &|_, _| {},
                check_update: || panic!("no update check in this test"),
                webviews: &crate::browser_webview::NoWebviews,
                pick_folder: &|| None,
            };
            let _ = crate::resources::notice::told(state);
            crate::resources::send::apply(state, command, &effects).map(drop)?;
            Ok::<_, er_protocol::ErError>(crate::resources::notice::told(state))
        });
        let told = told.unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(told.len(), 1, "{told:?}");
        assert!(told[0].starts_with("Started 2 reviewer(s)"), "{told:?}");
        release(&state, &agent);
    });
}

/// A scoped reviewer that waits for a slot tells the person once, when it
/// starts from the queue, as the App tells a queued task's start; at accept
/// time the scoped message is the run's one notice.
#[test]
fn a_queued_scoped_reviewer_tells_its_start_once() {
    with_storage(|dir| {
        let (state, agent) = tasking(dir, 1);
        let tab = open(&state, dir.path(), "a");
        let _ = bucket(&state, &tab);
        let reviewers = vec!["triage".to_string(), "general".to_string()];
        let _ = crate::resources::notice::told(&state);
        let ran = crate::runs::scoped(&state, None, "branch".into(), Vec::new(), reviewers, None);
        assert!(ran.is_ok(), "{:?}", ran.err());
        let mut told = crate::resources::notice::told(&state);
        assert_eq!(told.len(), 1, "{told:?}");
        assert!(told[0].starts_with("Started 2 reviewer(s)"), "{told:?}");
        let queued = tasks(&state)
            .into_iter()
            .any(|row| row.kind == "review" && row.status == "queued");
        assert!(
            queued,
            "the second reviewer did not wait: {:?}",
            tasks(&state)
        );
        open_gate(&agent.first);
        open_gate(&agent.second);
        // The slot pool is the process's, so a test in another module can
        // hold the one slot; the tick's poll starts the queue once it frees.
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            crate::task_hooks::poll(&state);
            told.extend(crate::resources::notice::told(&state));
            let done = tasks(&state).into_iter().any(|row| {
                row.kind == "review" && !matches!(row.status.as_str(), "queued" | "running")
            });
            if done {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "the queued reviewer never finished: {told:?}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        told.extend(crate::resources::notice::told(&state));
        let started: Vec<&String> = told
            .iter()
            .filter(|m| m.starts_with("review started ("))
            .collect();
        assert_eq!(started.len(), 1, "{told:?}");
    });
}

/// The run module never names the App.
#[test]
fn the_runs_name_no_app() {
    let text = include_str!("../../runs.rs");
    for word in ["AppState", "app.lock", "current_ai_", "app.tab()"] {
        assert!(!text.contains(word), "runs.rs names {word}");
    }
}

/// A lens already queued or running on the tab's bucket is skipped, so a
/// second click starts no second agent on the same sidecar.
#[test]
fn a_second_run_of_a_running_lens_is_refused() {
    with_storage(|dir| {
        let (state, agent) = tasking(dir, 50);
        let tab: (TabKey, _) = open(&state, dir.path(), "a");
        let _ = bucket(&state, &tab);
        let triage = |state: &DesktopState| {
            let reviewers = vec!["triage".to_string()];
            crate::runs::scoped(state, None, "branch".into(), Vec::new(), reviewers, None)
                .map(drop)
                .map_err(|e| e.message)
        };
        assert_eq!(triage(&state), Ok(()));
        let again = triage(&state).expect_err("a second triage started");
        assert!(again.contains("already running"), "{again}");
        let triages = tasks(&state)
            .into_iter()
            .filter(|row| row.kind == "triage")
            .count();
        assert_eq!(triages, 1);
        release(&state, &agent);
    });
}

/// A sidebar triage request moves `jobs` as it is taken and as it ends, so
/// the spinner that reads `jobs.triage` shows it, and `jobs` lists each
/// request held.
#[test]
fn a_triage_request_shows_in_jobs() {
    with_storage(|dir| {
        let (state, _agent) = tasking(dir, 50);
        let before = version(&state);
        let ctx = crate::route_helpers::triage_ctx(&state);
        // No clone, so the request ends at once.
        crate::auto_triage::dispatch_branch_triage(&ctx, "p", "o/r", "", "feature", &[]);
        wait_until("the request to move jobs twice", || {
            version(&state) >= before + 2
        });

        let held = "branch:o/r:other".to_string();
        if let Ok(mut queue) = state.auto_triage_in_flight.lock() {
            queue.insert(held.clone());
        }
        assert_eq!(jobs(&state).triage, vec![held]);
    });
}

/// A sidebar branch triage queues its run on the task host: `jobs` lists a
/// triage task for the branch.
#[test]
fn a_sidebar_triage_queues_on_the_host() {
    with_storage(|dir| {
        let (mut state, agent) = tasking(dir, 50);
        let git: Arc<dyn er_git::Git> = Arc::new(
            er_git::FakeGit::default()
                .with_detect_base_branch(Ok("main".to_string()))
                .with_diff_raw_range(Ok(raw_diff())),
        );
        state.ports = er_engine::ports::Ports::with_git(git);
        let root = dir.path().join("a");
        std::fs::create_dir_all(&root).unwrap_or_else(|e| panic!("{e}"));
        let root = root.to_string_lossy().into_owned();
        let queued_root = root.clone();
        super::on_a_thread(&state, "the sidebar triage", move |state| {
            let ctx = crate::route_helpers::triage_ctx(state);
            crate::auto_triage::dispatch_branch_triage(&ctx, "p", "o/r", &root, "feature", &[]);
            wait_until("a triage task in jobs", || {
                tasks(state).iter().any(|row| row.kind == "triage")
            });
        });
        let row = tasks(&state)
            .into_iter()
            .find(|row| row.kind == "triage")
            .unwrap_or_else(|| panic!("no triage row"));
        assert_eq!(
            (row.repo_root.as_str(), row.branch_label.as_str()),
            (queued_root.as_str(), "feature")
        );
        release(&state, &agent);
    });
}

/// A sidebar triage tells the person once, with the sidebar's own message;
/// its task sends no started notice of its own.
#[test]
fn a_sidebar_triage_sends_one_notice() {
    with_storage(|dir| {
        let (mut state, agent) = tasking(dir, 50);
        let git: Arc<dyn er_git::Git> = Arc::new(
            er_git::FakeGit::default()
                .with_detect_base_branch(Ok("main".to_string()))
                .with_diff_raw_range(Ok(raw_diff())),
        );
        state.ports = er_engine::ports::Ports::with_git(git);
        let root = dir.path().join("a");
        std::fs::create_dir_all(&root).unwrap_or_else(|e| panic!("{e}"));
        let root = root.to_string_lossy().into_owned();
        let _ = crate::resources::notice::told(&state);
        let ctx = crate::route_helpers::triage_ctx(&state);
        crate::auto_triage::dispatch_branch_triage(&ctx, "p", "o/r", &root, "feature", &[]);
        let mut told = Vec::new();
        wait_until("the sidebar's notice", || {
            told.extend(crate::resources::notice::told(&state));
            told.iter()
                .any(|m| m.starts_with("Triage started for branch"))
        });
        assert_eq!(told, vec!["Triage started for branch feature".to_string()]);
        release(&state, &agent);
    });
}

/// Sidebar triage and the tick's task duties name no App.
#[test]
fn triage_and_the_tick_are_off_the_app() {
    let triage = include_str!("../../auto_triage.rs");
    for word in ["er_engine::app", "App>", "app.lock"] {
        assert!(!triage.contains(word), "auto_triage.rs names {word}");
    }
    let tick = include_str!("../../resources/tick.rs");
    for word in ["poll_background_tasks", "process_ai_task_inbox"] {
        assert!(!tick.contains(word), "the tick runs {word}");
    }
}

/// A note on the tab's first file, written through its actor; answers its id
/// once the content holds it.
fn note(
    state: &DesktopState,
    (key, tab): &(TabKey, er_engine::tabs_record::TabDescriptor),
) -> String {
    let content = state
        .service
        .content(*key, tab)
        .unwrap_or_else(|| panic!("no content"));
    let written = state.service.send(
        &crate::service::actor_key(tab),
        er_service::Command::AddNote {
            context: content.context.clone(),
            target: er_service::AnnotationTarget {
                file: "src/f0.rs".into(),
                hunk_idx: 0,
                line_num: Some(1),
                line_num_end: None,
            },
            body: "Rename this".into(),
            side: None,
            client_id: None,
            reply_to: None,
            finding_ref: None,
        },
    );
    match written {
        Ok(er_service::Reply::AnnotationWritten { id }) => {
            wait_until("the note in the content", || {
                state.service.content(*key, tab).is_some_and(|c| {
                    c.ai.state
                        .notes
                        .as_ref()
                        .is_some_and(|n| n.notes.iter().any(|n| n.id == id))
                })
            });
            id
        }
        other => panic!("no note: {other:?}"),
    }
}

/// A note sent to the agent through
/// `er_send` is read from the tab's AI section and runs on the task host,
/// which lists it and reads its output lines.
#[test]
fn a_note_goes_to_the_agent_from_the_ai_section() {
    with_storage(|dir| {
        let (state, agent) = tasking(dir, 50);
        let tab = open(&state, dir.path(), "a");
        let bucket = bucket(&state, &tab);
        let id = note(&state, &tab);
        let command = Command::SendNoteToAgent {
            tab: er_service::wire_tab_key(tab.0),
            note_id: id,
        };
        let sent = send(&state, "send_note_to_agent", command);
        assert!(sent.is_ok(), "{sent:?}");
        let row = tasks(&state)
            .into_iter()
            .find(|row| row.kind == crate::route_helpers::SEND_NOTE_COMMAND)
            .unwrap_or_else(|| panic!("no note task: {:?}", tasks(&state)));
        assert_eq!(
            (row.er_dir.as_str(), row.status.as_str()),
            (bucket.as_str(), "running")
        );
        wait_until("the agent's first line in the task log", || {
            log(&state, &row.id).iter().any(|line| line == "line-a")
        });
        open_gate(&agent.first);
        open_gate(&agent.second);
        wait_until("the note task to finish", || {
            tasks(&state)
                .iter()
                .any(|r| r.id == row.id && r.status == "done")
        });
    });
}

/// A plain local tab on a detached HEAD reads no branch, so its branch
/// resolves no checkout; it is still the main checkout, and a note goes to
/// the agent there as it does with the flag off.
#[test]
fn a_note_goes_to_the_agent_on_a_detached_head() {
    with_storage(|dir| {
        let git: Arc<dyn er_git::Git> = Arc::new(
            er_git::FakeGit::default()
                .with_current_branch(Ok(String::new()))
                .with_detect_base_branch(Ok("main".to_string()))
                .with_diff_raw(Ok(raw_diff()))
                .with_diff_shortstat((3, 3, 3))
                .with_rev_parse_verify(Ok(Some("b".repeat(40))))
                .with_is_merge_in_progress(false),
        );
        let (state, agent) = tasking_on(dir, 50, er_engine::ports::Ports::with_git(git));
        let tab = open(&state, dir.path(), "a");
        let id = note(&state, &tab);
        let content = state
            .service
            .content(tab.0, &tab.1)
            .unwrap_or_else(|| panic!("no content"));
        assert_eq!(
            content.tab.checkout_root, None,
            "the fixture's detached HEAD"
        );
        let command = Command::SendNoteToAgent {
            tab: er_service::wire_tab_key(tab.0),
            note_id: id,
        };
        let sent = send(&state, "send_note_to_agent", command);
        assert!(sent.is_ok(), "{sent:?}");
        assert!(
            tasks(&state)
                .iter()
                .any(|row| row.kind == crate::route_helpers::SEND_NOTE_COMMAND),
            "{:?}",
            tasks(&state)
        );
        release(&state, &agent);
    });
}

/// A validation through `er_send` starts its agent on
/// the task host when the bucket holds a review.
#[test]
fn a_validation_starts_on_the_host() {
    with_storage(|dir| {
        let (state, agent) = tasking(dir, 50);
        let tab = open(&state, dir.path(), "a");
        let bucket = bucket(&state, &tab);
        let review = serde_json::json!({
            "version": 1,
            "diff_hash": "h",
            "created_at": "2026-11-23T00:00:00Z",
            "base_branch": "main",
            "head_branch": "feature",
            "files": {}
        });
        std::fs::create_dir_all(&bucket).unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(Path::new(&bucket).join("review.json"), review.to_string())
            .unwrap_or_else(|e| panic!("{e}"));
        let command = Command::RunValidate {
            tab: er_service::wire_tab_key(tab.0),
            scope: "branch".into(),
        };
        let sent = send(&state, "run_validate", command);
        assert!(sent.is_ok(), "{sent:?}");
        let kinds: Vec<String> = tasks(&state).into_iter().map(|row| row.kind).collect();
        assert!(kinds.contains(&"validate".to_string()), "{kinds:?}");
        release(&state, &agent);
    });
}

/// Two enqueues of the same task at once queue one and refuse the other:
/// the busy check and the push are one step.
#[test]
fn two_clicks_at_once_queue_one_task() {
    with_storage(|dir| {
        let (state, agent) = tasking(dir, 1);
        for round in 0..20 {
            let branch = format!("feature-{round}");
            let gate = std::sync::Barrier::new(2);
            let accepted = std::thread::scope(|scope| {
                let clicks: Vec<_> = (0..2)
                    .map(|_| {
                        let one = launch(&state, dir.path(), "review", &branch);
                        let (state, gate) = (&state, &gate);
                        scope.spawn(move || {
                            gate.wait();
                            crate::task_hooks::enqueue(state, one).is_ok()
                        })
                    })
                    .collect();
                clicks
                    .into_iter()
                    .map(|click| click.join().unwrap_or(false))
                    .filter(|accepted| *accepted)
                    .count()
            });
            assert_eq!(accepted, 1, "round {round} queued {accepted} reviews");
        }
        release(&state, &agent);
    });
}

/// Agent launches a test counts, each held running by its kept result
/// sender.
#[derive(Default)]
struct Launches {
    count: std::sync::atomic::AtomicUsize,
    results: std::sync::Mutex<Vec<std::sync::mpsc::Sender<anyhow::Result<()>>>>,
}

impl Launches {
    fn count(&self) -> usize {
        self.count.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// A launch that takes a moment and leaves its task running.
    fn launch(&self) -> anyhow::Result<er_engine::agent_prompt::AgentPromptRun> {
        self.count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(2));
        let (tx, result) = std::sync::mpsc::channel();
        self.results
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(tx);
        Ok(er_engine::agent_prompt::AgentPromptRun {
            run: er_engine::jobs::AgentRunHandle::new(),
            result,
        })
    }
}

/// Sends `validate` on `target` once the gate opens; answers whether the
/// host took it.
fn send_at_gate(
    state: &DesktopState,
    gate: &std::sync::Barrier,
    target: BackgroundTaskTarget,
    launches: &Launches,
) -> bool {
    gate.wait();
    crate::task_hooks::hold_agent(state, target, "validate", |_| launches.launch()).is_ok()
}

/// Two sends of the same tab command on one bucket at once start one
/// agent: the busy check, the launch and the record are one step.
#[test]
fn two_sends_at_once_start_one_agent() {
    with_storage(|dir| {
        let (state, _agent) = tasking(dir, 50);
        let launches = Launches::default();
        for round in 0..60 {
            let mut target = launch(&state, dir.path(), "validate", "feature").target;
            let bucket = dir.path().join(format!("bucket-{round}"));
            target.er_dir = bucket.to_string_lossy().into_owned();
            let before = launches.count();
            let gate = std::sync::Barrier::new(2);
            let accepted = std::thread::scope(|scope| {
                let sends: Vec<_> = (0..2)
                    .map(|_| {
                        let (state, gate, launches) = (&state, &gate, &launches);
                        let target = target.clone();
                        scope.spawn(move || send_at_gate(state, gate, target, launches))
                    })
                    .collect();
                sends
                    .into_iter()
                    .map(|send| send.join().unwrap_or(false))
                    .filter(|accepted| *accepted)
                    .count()
            });
            let started = launches.count() - before;
            assert_eq!(
                (accepted, started),
                (1, 1),
                "round {round} accepted {accepted} sends and started {started} agents"
            );
        }
    });
}

/// The tick runs none of the App's task duties.
#[test]
fn the_tick_leaves_the_app() {
    let tick = include_str!("../../resources/tick.rs");
    for word in ["drain_agent_log", "check_commands", "poll_background_tasks"] {
        assert!(!tick.contains(word), "the tick runs {word}");
    }
}
