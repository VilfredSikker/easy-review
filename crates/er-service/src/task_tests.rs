//! The task host: a stop reaches the runner, a bucket's log keeps its last
//! lines after its tasks leave the list, and the job rows list the tasks,
//! the arena runs and the review flows.

use std::sync::Arc;
use std::time::{Duration, Instant};

use er_engine::agent_log::{AgentLogEntry, AgentLogSource};
use er_engine::agent_prompt::AgentPromptRun;
use er_engine::ai::review_flow::ReviewFlow;
use er_engine::arena::RunStatus;
use er_engine::config::ErConfig;
use er_engine::jobs::AgentRunHandle;
use er_engine::ports::Ports;
use er_engine::task_target::BackgroundTaskTarget;

use crate::tasks::{self, Launch, TaskHooks, TaskHost, BUCKET_LOG_LINES};
use crate::test_support::{open_gate, pid_alive, Agent};
use crate::workspace::{LiveArenaRun, LiveReviewFlow, WorkspaceView};
use crate::ServiceHost;

const WAIT: Duration = Duration::from_secs(10);

fn wait_until(what: &str, mut done: impl FnMut() -> bool) -> anyhow::Result<()> {
    let deadline = Instant::now() + WAIT;
    while !done() {
        anyhow::ensure!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

fn target(bucket: &str) -> BackgroundTaskTarget {
    BackgroundTaskTarget {
        repo_root: "/repo".into(),
        er_dir: bucket.into(),
        branch_label: "feature".into(),
        base_branch: "main".into(),
        scope: "branch".into(),
        pr_number: None,
        remote_repo: None,
        managed_local: true,
    }
}

/// Hooks with the default config that tell nothing.
fn quiet_hooks() -> TaskHooks {
    let config = Arc::new(ErConfig::default());
    TaskHooks {
        ports: Ports::for_test(),
        config: Arc::new(move || Arc::clone(&config)),
        notify: Arc::new(|_, _| {}),
        finished: Arc::new(|_| {}),
    }
}

/// A launcher task on `bucket` whose agent printed `lines` lines and
/// succeeded.
fn printed(host: &TaskHost, bucket: &str, lines: usize) -> anyhow::Result<String> {
    printed_with(host, &quiet_hooks(), bucket, lines)
}

fn printed_with(
    host: &TaskHost,
    hooks: &TaskHooks,
    bucket: &str,
    lines: usize,
) -> anyhow::Result<String> {
    tasks::hold_agent(host, hooks, target(bucket), "notes", |log_tx| {
        for n in 0..lines {
            let _ = log_tx.send(AgentLogEntry {
                timestamp: Instant::now(),
                command_name: Arc::from("notes"),
                source: AgentLogSource::Stdout,
                text: format!("line-{n}"),
            });
        }
        let (done, result) = std::sync::mpsc::channel();
        let _ = done.send(Ok(()));
        Ok(AgentPromptRun {
            run: AgentRunHandle::new(),
            result,
        })
    })
    .map_err(anyhow::Error::msg)
}

/// The stop a front end sends reaches the runner: the agent's process is
/// gone and the task ends failed.
#[test]
fn a_stop_reaches_the_runner() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    er_engine::storage::with_process_test_storage_root(dir.path().join("storage"), || {
        let agent = Agent::in_dir(dir.path());
        let host = ServiceHost::new(Ports::for_test(), agent.write_config(50));
        let bucket = dir.path().join("bucket");
        std::fs::create_dir_all(&bucket)?;
        let config = (host.task_hooks().config)();
        let launch = Launch {
            kind: "review".into(),
            command_name: "review".into(),
            target: BackgroundTaskTarget {
                repo_root: dir.path().to_string_lossy().into_owned(),
                ..target(&bucket.to_string_lossy())
            },
            prompt: "Review the change".into(),
            prepared_diff: false,
            host_write_diagram: None,
            scoped_stamp: None,
            selection: er_engine::config::AiSelection {
                provider_id: Some("gated".into()),
                model_id: Some("one".into()),
                effort: None,
            },
            config,
            quiet: false,
        };
        let id =
            tasks::enqueue(host.tasks(), &host.task_hooks(), launch).map_err(anyhow::Error::msg)?;
        let mut pid = String::new();
        wait_until("the agent to start", || {
            pid = std::fs::read_to_string(&agent.pid).unwrap_or_default();
            pid.ends_with('\n')
        })
        .map_err(|e| anyhow::anyhow!("{e}: {:?}", host.tasks().rows()))?;
        let pid = pid.trim().to_string();
        assert!(pid_alive(&pid));

        assert!(host.tasks().stop(&id), "no running task to stop");
        wait_until("the agent's process to end", || !pid_alive(&pid))?;
        let row = |status: &str| {
            host.tasks()
                .rows()
                .iter()
                .any(|row| row.id == id && row.status == status)
        };
        wait_until("the task to end failed", || row("failed"))?;
        open_gate(&agent.first);
        open_gate(&agent.second);
        Ok(())
    })
}

/// A bucket keeps its last lines across its tasks, and keeps them after the
/// task that wrote them has left the list.
#[test]
fn a_bucket_log_keeps_its_last_lines_after_the_task_leaves() -> anyhow::Result<()> {
    let host = TaskHost::default();
    let id = printed(&host, "/bucket", BUCKET_LOG_LINES + 10)?;
    let done = || {
        host.rows()
            .iter()
            .any(|row| row.id == id && row.status == "done")
    };
    wait_until("the task to finish", done)?;
    let texts = |log: Vec<AgentLogEntry>| log.into_iter().map(|line| line.text).collect::<Vec<_>>();

    let kept = texts(host.bucket_log("/bucket"));
    assert_eq!(kept.len(), BUCKET_LOG_LINES, "the log outgrew its cap");
    // The finish adds a status line, so eleven of the agent's lines went.
    assert_eq!(kept.first().map(String::as_str), Some("line-11"));
    assert!(kept.last().is_some_and(|line| line.contains("completed")));

    tasks::retire(&host, u64::MAX);
    assert!(host.rows().is_empty(), "the finished task is still listed");
    assert_eq!(
        texts(host.bucket_log("/bucket")),
        kept,
        "the lines left with the task"
    );
    assert!(host.bucket_log("/other").is_empty());
    Ok(())
}

/// The rows hold the host's tasks, and every view's arena runs and review
/// flows, each part sorted by its key.
#[test]
fn the_job_rows_list_tasks_runs_and_flows() -> anyhow::Result<()> {
    let host = TaskHost::default();
    let id = printed(&host, "/bucket", 1)?;
    let run = |run_id: &str| LiveArenaRun {
        run_id: run_id.into(),
        er_dir: "/bucket".into(),
        status: RunStatus::Running { round: 1 },
        progress: Default::default(),
    };
    let flow = |er_dir: &str| LiveReviewFlow {
        er_dir: er_dir.into(),
        flow: ReviewFlow::new("hash"),
        draining: false,
        run_targets: Vec::new(),
        rerun_stages: Vec::new(),
    };
    let views = [
        Arc::new(WorkspaceView {
            arena_runs: vec![run("b")],
            review_flows: vec![flow("/z")],
            ..WorkspaceView::default()
        }),
        Arc::new(WorkspaceView {
            arena_runs: vec![run("a")],
            review_flows: vec![flow("/y")],
            ..WorkspaceView::default()
        }),
    ];

    let rows = crate::job_rows(&views, &host);

    let ids: Vec<_> = rows.tasks.iter().map(|row| row.id.as_str()).collect();
    assert_eq!(ids, [id.as_str()]);
    let runs: Vec<_> = rows
        .arena_runs
        .iter()
        .map(|run| run.run_id.as_str())
        .collect();
    assert_eq!(runs, ["a", "b"]);
    assert!(rows
        .arena_runs
        .iter()
        .all(|run| run.status == er_protocol::ArenaRunStatus::Running { round: 1 }));
    let flows: Vec<_> = rows
        .review_flows
        .iter()
        .map(|flow| flow.er_dir.as_str())
        .collect();
    assert_eq!(flows, ["/y", "/z"]);
    Ok(())
}

/// The host's row changes reach the events sender as `TasksChanged` with a
/// rising revision, and a task's finish message as `TaskNotice`, even when
/// the task host started before the sender was set.
#[test]
fn the_task_events_reach_the_sender() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    er_engine::storage::with_process_test_storage_root(dir.path().join("storage"), || {
        let host = ServiceHost::new(Ports::for_test(), dir.path().join("config.toml"));
        let _ = host.tasks();
        let (tx, events) = std::sync::mpsc::channel();
        let host = host.with_events(tx);
        printed_with(host.tasks(), &host.task_hooks(), "/bucket", 1)?;

        let mut revisions = Vec::new();
        let deadline = Instant::now() + WAIT;
        let notice = loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match events.recv_timeout(left)? {
                crate::Event::TasksChanged { revision } => revisions.push(revision),
                crate::Event::TaskNotice { message, .. } => break message,
                _ => {}
            }
        };
        assert!(notice.contains("notes completed"), "{notice}");
        assert!(
            revisions.len() >= 2,
            "{revisions:?}: no TasksChanged for the start and the finish"
        );
        assert!(
            revisions.windows(2).all(|pair| pair[0] < pair[1]),
            "{revisions:?}"
        );
        Ok(())
    })
}

/// A committed importance run has the host's settings view read the repo's
/// rules again, as the desktop's does, so a TUI importance run moves
/// `risk(key)` too. The table is committed by hand: a run that commits one
/// needs a real provider CLI.
#[test]
fn a_committed_importance_run_reloads_the_risk_rules() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    er_engine::storage::with_process_test_storage_root(dir.path().join("storage"), || {
        let host = ServiceHost::new(Ports::for_test(), dir.path().join("config.toml"));
        let repo_root = "/projects/widgets";
        assert!(host.settings().view().risk_for(repo_root, None).is_none());
        let committed = er_engine::config::RiskRuleProposal {
            repo: "widgets".into(),
            rules: [("src/f2.rs".to_string(), "low".into())].into(),
            ..Default::default()
        };
        let store = er_engine::store_registry::store();
        committed.commit_to_store("widgets", &store)?;
        let row = er_protocol::TaskJob {
            id: "importance-1".into(),
            label: "Importance".into(),
            kind: er_engine::ai::prompts::IMPORTANCE_TASK_KIND.into(),
            target_label: "widgets".into(),
            scope: "branch".into(),
            repo_root: repo_root.into(),
            branch_label: "feature".into(),
            pr_number: None,
            remote_repo: None,
            er_dir: "/bucket".into(),
            status: "done".into(),
            error: None,
            started_at_ms: 0,
            finished_at_ms: Some(1),
        };
        let target = BackgroundTaskTarget {
            repo_root: repo_root.into(),
            ..target("/bucket")
        };
        (host.task_hooks().finished)(crate::Finished {
            row: &row,
            target: &target,
            agent: false,
        });
        assert!(
            host.settings().view().risk_for(repo_root, None).is_some(),
            "the rules were not reloaded"
        );
        Ok(())
    })
}
