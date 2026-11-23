//! The desktop's task host on the flag path: `jobs` lists each task's row
//! and moves with its status and never with an output line, which
//! `task_log` reads; a stop reaches the runner; a queued task can be
//! removed; and none of it takes the App lock.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use er_engine::task_target::BackgroundTaskTarget;
use er_engine::TabKey;
use er_protocol::{Command, Resource, ResourceKey, TaskJob};
use er_service::WorkspaceDeps;

use super::ai_ask_tests::jobs;
use super::arena_resource_tests::{bucket, local_ports, open, wait_until};
use super::diff_command_tests::send;
use super::diff_resource_tests::{raw_diff, with_storage};
use super::{review_flow_test_state, ReviewFlowTestDir};
use crate::commands::AppState;
use crate::service::ServiceState;

/// The files a gated agent reads and writes.
struct Agent {
    pid: PathBuf,
    first: PathBuf,
    second: PathBuf,
}

impl Agent {
    fn in_dir(dir: &Path) -> Self {
        Self {
            pid: dir.join("agent.pid"),
            first: dir.join("gate-1"),
            second: dir.join("gate-2"),
        }
    }

    /// Prints its pid to a file and `line-a`, waits for the first gate,
    /// prints `line-b`, waits for the second. Each wait gives up after
    /// about thirty seconds, so a failed test leaves no shell behind.
    fn config(&self, cap: usize) -> String {
        let (pid, first, second) = (
            self.pid.display(),
            self.first.display(),
            self.second.display(),
        );
        let wait = |gate: &dyn std::fmt::Display| {
            format!(
                r#"n=0; while [ ! -e "{gate}" ] && [ $n -lt 1500 ]; do sleep 0.02; n=$((n+1)); done"#
            )
        };
        let script = format!(
            r#"echo $$ > "{pid}"; printf 'line-a\n'; {}; printf 'line-b\n'; {}"#,
            wait(&first),
            wait(&second)
        );
        format!(
            r#"
[ai_hub]
default_provider = "gated"
default_model = "one"
max_concurrent_reviews = {cap}

[ai_hub.providers.gated]
label = "Gated"
command = "/bin/sh"
args = ["-c", '''{script}''', "gated-agent", "{{prompt}}"]

[[ai_hub.providers.gated.models]]
id = "one"
"#
        )
    }
}

fn open_gate(gate: &Path) {
    std::fs::write(gate, "").unwrap_or_else(|e| panic!("gate: {e}"));
}

/// A state whose tasks run the gated agent, under `cap` running reviews.
fn tasking(dir: &ReviewFlowTestDir, cap: usize) -> (AppState, Agent) {
    let agent = Agent::in_dir(dir.path());
    let path = dir.path().join("settings").join("config.toml");
    std::fs::create_dir_all(dir.path().join("settings")).unwrap_or_else(|e| panic!("{e}"));
    std::fs::write(&path, agent.config(cap)).unwrap_or_else(|e| panic!("config: {e}"));
    let mut state = review_flow_test_state(dir);
    let ports = local_ports();
    state.desktop.ports = ports.clone();
    let deps = WorkspaceDeps::new(ports);
    state.service = Arc::new(ServiceState::with_deps(deps, |_| {}).with_config_path(path));
    (state, agent)
}

fn tasks(state: &AppState) -> Vec<TaskJob> {
    jobs(state).tasks
}

fn version(state: &AppState) -> u64 {
    crate::resources::jobs::version(&state.desktop)
}

fn log(state: &AppState, id: &str) -> Vec<String> {
    let key = ResourceKey::TaskLog { id: id.to_string() };
    match crate::resources::fetch(&state.desktop, key).map(|f| f.value) {
        Ok(Resource::TaskLog(log)) => log.lines.into_iter().map(|l| l.text).collect(),
        other => panic!("not a task log: {other:?}"),
    }
}

/// A task of `kind` on a bucket under `dir`, run with the settings view's
/// config and saved pick, queued with the App locked. Answers its id.
fn queued(state: &AppState, dir: &Path, kind: &str) -> String {
    let launch = launch(state, dir, kind, "feature");
    super::while_the_app_is_locked(state, "the enqueue", move |state| {
        crate::tasks::enqueue(&state.desktop, launch)
    })
    .unwrap_or_else(|e| panic!("{e}"))
}

/// A task of `kind` on `branch`, on a bucket under `dir`.
fn launch(state: &AppState, dir: &Path, kind: &str, branch: &str) -> crate::tasks::Launch {
    let config = crate::arena_service::config(&state.desktop);
    let (provider_id, model_id, effort) = crate::ai_ask::selection(&config);
    let bucket = dir.join("bucket");
    std::fs::create_dir_all(&bucket).unwrap_or_else(|e| panic!("bucket: {e}"));
    crate::tasks::Launch {
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

fn row(state: &AppState, id: &str) -> Option<TaskJob> {
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
        open_gate(&agent.first);
        open_gate(&agent.second);
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
        open_gate(&agent.first);
        open_gate(&agent.second);
    });
}

/// The host and its commands never name the App, so no task read or write
/// waits on it.
#[test]
fn the_task_host_names_no_app() {
    for (name, text) in [
        ("tasks.rs", include_str!("../../tasks.rs")),
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

/// With the App locked, a review run through `er_send` reads the tab's
/// content: the diff it prepares in the tab's bucket is the diff the content
/// holds, and `jobs` lists its task under that bucket and scope.
#[test]
fn a_review_run_prepares_the_content_diff_with_the_app_locked() {
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
        open_gate(&agent.first);
        open_gate(&agent.second);
    });
}

/// The run commands' flag path never takes the App lock: the tour and a
/// scoped triage each start with the App held.
#[test]
fn the_run_commands_start_with_the_app_locked() {
    with_storage(|dir| {
        let (state, agent) = tasking(dir, 50);
        let tab: (TabKey, _) = open(&state, dir.path(), "a");
        let _ = bucket(&state, &tab);
        let started = super::while_the_app_is_locked(&state, "the runs", |state| {
            let tour =
                crate::commands::ask_on_desktop(state, |desktop| crate::runs::tour(desktop, None));
            let triage = crate::commands::ask_on_desktop(state, |desktop| {
                let reviewers = vec!["triage".to_string()];
                crate::runs::scoped(desktop, None, "branch".into(), Vec::new(), reviewers, None)
            });
            (tour.map(drop), triage.map(drop))
        });
        assert_eq!(started, (Ok(()), Ok(())));
        let kinds: Vec<String> = tasks(&state).into_iter().map(|row| row.kind).collect();
        assert!(kinds.contains(&"tour".to_string()), "{kinds:?}");
        assert!(kinds.contains(&"triage".to_string()), "{kinds:?}");
        open_gate(&agent.first);
        open_gate(&agent.second);
    });
}

/// Each run command on `er_send` starts its own run with the App locked:
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
        open_gate(&agent.first);
        open_gate(&agent.second);
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
        let told = super::while_the_app_is_locked(&state, "the scoped run", move |state| {
            let effects = crate::resources::send::Effects {
                notify: &|_, _| {},
                follow_selection: &|_| {},
                check_update: || panic!("no update check in this test"),
                webviews: &crate::browser_webview::NoWebviews,
                pick_folder: &|| None,
            };
            let _ = crate::resources::notice::told(&state.desktop);
            crate::resources::send::apply(&state.desktop, command, &effects).map(drop)?;
            Ok::<_, er_protocol::ErError>(crate::resources::notice::told(&state.desktop))
        });
        let told = told.unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(told.len(), 1, "{told:?}");
        assert!(told[0].starts_with("Started 2 reviewer(s)"), "{told:?}");
        open_gate(&agent.first);
        open_gate(&agent.second);
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
        let _ = crate::resources::notice::told(&state.desktop);
        let ran = crate::commands::ask_on_desktop(&state, |desktop| {
            crate::runs::scoped(desktop, None, "branch".into(), Vec::new(), reviewers, None)
        });
        assert!(ran.is_ok(), "{:?}", ran.err());
        let mut told = crate::resources::notice::told(&state.desktop);
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
        // The slot pool is the process's, so another test's agent can hold
        // the one slot; the tick's poll starts the queue once it frees.
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            crate::tasks::poll(&state.desktop);
            told.extend(crate::resources::notice::told(&state.desktop));
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
        told.extend(crate::resources::notice::told(&state.desktop));
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
        let triage = |state: &AppState| {
            crate::commands::ask_on_desktop(state, |desktop| {
                let reviewers = vec!["triage".to_string()];
                crate::runs::scoped(desktop, None, "branch".into(), Vec::new(), reviewers, None)
            })
            .map(drop)
        };
        assert_eq!(triage(&state), Ok(()));
        let again = triage(&state).expect_err("a second triage started");
        assert!(again.contains("already running"), "{again}");
        let triages = tasks(&state)
            .into_iter()
            .filter(|row| row.kind == "triage")
            .count();
        assert_eq!(triages, 1);
        open_gate(&agent.first);
        open_gate(&agent.second);
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
        let ctx = crate::commands::auto_triage_ctx(&state);
        // No clone, so the request ends at once.
        crate::auto_triage::dispatch_branch_triage(&ctx, "p", "o/r", "", "feature", &[]);
        wait_until("the request to move jobs twice", || {
            version(&state) >= before + 2
        });

        let held = "branch:o/r:other".to_string();
        if let Ok(mut queue) = state.desktop.auto_triage_in_flight.lock() {
            queue.insert(held.clone());
        }
        assert_eq!(jobs(&state).triage, vec![held]);
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
                            crate::tasks::enqueue(&state.desktop, one).is_ok()
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
        open_gate(&agent.first);
        open_gate(&agent.second);
    });
}
