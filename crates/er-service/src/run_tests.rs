//! Runs and flow commands through the host.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::TryRecvError;
use std::sync::Arc;
use std::time::{Duration, Instant};

use er_engine::ports::Ports;
use er_engine::tabs_record::{TabDescriptor, TabKind};
use er_engine::TabKey;
use er_git::FakeGit;
use er_github::FakeGitHub;

use crate::app_registry::AppCommand;
use crate::content::TabContent;
use crate::protocol::{AnnotationTarget, Command, Reply};
use crate::test_support::Agent;
use crate::{RunRequest, ServiceHost};

const WAIT: Duration = Duration::from_secs(10);

const RAW: &str = "\
diff --git a/src/a.rs b/src/a.rs
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,2 +1,2 @@
 a-one
-a-old
+a-new
";

/// A git port whose `read_blob`, which a review's planning reads the base's
/// `REVIEW.md` with, waits while `closed` is set.
fn gated_ports(closed: Arc<AtomicBool>) -> Ports {
    let git = FakeGit::default()
        .with_is_merge_in_progress(false)
        .with_current_branch(Ok("feature".to_string()))
        .with_diff_shortstat((0, 0, 0))
        .with_diff_raw(Ok(RAW.to_string()))
        .with_paths_present_at(Ok(vec!["REVIEW.md".to_string()]))
        .with_read_blob(Ok(b"Review rules".to_vec()))
        .with_read_blob_hook(move || {
            let deadline = Instant::now() + Duration::from_secs(30);
            while closed.load(Ordering::Acquire) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
        });
    Ports {
        git: Arc::new(git),
        github: Arc::new(FakeGitHub::default()),
    }
}

/// A working tab on `repo`, listed and loaded on `host`.
fn loaded(host: &ServiceHost, repo: &str) -> anyhow::Result<(TabKey, Arc<TabContent>)> {
    let tab = TabDescriptor {
        kind: TabKind::Working,
        repo_root: repo.to_string(),
        branch: None,
        pr_owner: None,
        pr_repo: None,
        pr_number: None,
        pr_head_ref: None,
        base_ref: Some("main".to_string()),
        extra: serde_json::Map::new(),
    };
    let Reply::Tabs(view) = host
        .registry()
        .send(AppCommand::NewTab { tab: tab.clone() })?
    else {
        anyhow::bail!("a new tab answered no tabs");
    };
    let key = view
        .active
        .ok_or_else(|| anyhow::anyhow!("no active tab"))?;
    let load = Command::LoadTab {
        key,
        tab: tab.clone(),
        mode: None,
    };
    host.post(&tab.workspace_key(), load).recv_timeout(WAIT)??;
    let content = host
        .content(&tab.workspace_key(), key)
        .ok_or_else(|| anyhow::anyhow!("no content"))?;
    Ok((key, content))
}

fn with_repo(
    test: impl FnOnce(&std::path::Path, &str) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(&repo)?;
    let repo_text = repo.to_string_lossy().into_owned();
    er_engine::storage::with_process_test_storage_root(dir.path().join("storage"), || {
        test(dir.path(), &repo_text)
    })
}

/// A review whose planning waits on git answers the caller within one
/// render tick, still planning; the plan finishes once git answers.
#[test]
fn planning_that_reads_git_does_not_hold_the_caller() -> anyhow::Result<()> {
    with_repo(|dir, repo| {
        let closed = Arc::new(AtomicBool::new(false));
        let host = ServiceHost::new(gated_ports(Arc::clone(&closed)), dir.join("config.toml"));
        let (key, _) = loaded(&host, repo)?;
        closed.store(true, Ordering::Release);
        // Git answers after a few ticks whether or not the caller waits.
        let opener = Arc::clone(&closed);
        let open = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(2));
            opener.store(false, Ordering::Release);
        });

        let started = Instant::now();
        let planned = host.run(
            key,
            RunRequest::Review {
                scope: "branch".into(),
            },
        );
        let took = started.elapsed();
        let still_planning = matches!(planned.try_recv(), Err(TryRecvError::Empty));
        let _ = open.join();
        assert!(
            took < Duration::from_millis(250),
            "the run held the caller for {took:?}"
        );
        assert!(still_planning, "the plan finished before git answered");
        planned.recv_timeout(WAIT)?.ok();
        Ok(())
    })
}

/// A question added on the tab lands in the bucket file the agent of a
/// questions run, planned through the host, is told to read.
#[test]
fn a_questions_run_points_the_agent_at_the_new_question() -> anyhow::Result<()> {
    with_repo(|dir, repo| {
        let closed = Arc::new(AtomicBool::new(false));
        let agent = Agent::in_dir(dir);
        // Cats the questions file its prompt names, as the agent would read it.
        let script = r#"f=$(printf '%s' "$1" | grep -o "'[^']*'/questions.json" | head -n 1 | tr -d "'"); cat "$f""#;
        let host = ServiceHost::new(gated_ports(closed), agent.write_script_config(script));
        let (key, content) = loaded(&host, repo)?;
        let add = Command::AddQuestion {
            context: content.context.clone(),
            target: AnnotationTarget {
                file: "src/a.rs".into(),
                hunk_idx: 0,
                line_num: Some(2),
                line_num_end: None,
            },
            body: "why-a-new".into(),
            side: None,
            client_id: None,
            reply_to: None,
            finding_ref: None,
        };
        let workspace = content.context.workspace_key();
        let written = host.post(&workspace, add).recv_timeout(WAIT)??;
        anyhow::ensure!(
            matches!(written, Reply::AnnotationWritten { .. }),
            "the question was not written: {written:?}"
        );
        // The actor writes the tab's bucket; the question must be there.
        let bucket = content.context.er_dir.clone();
        let path = std::path::Path::new(&bucket).join("questions.json");
        assert!(std::fs::read_to_string(&path)?.contains("why-a-new"));

        host.run(key, RunRequest::Questions)
            .recv_timeout(WAIT)?
            .map_err(|error| anyhow::anyhow!(error.message))?;
        let read = || {
            host.tasks()
                .bucket_log(&bucket)
                .iter()
                .any(|line| line.text.contains("why-a-new"))
        };
        let deadline = Instant::now() + WAIT;
        while !read() {
            let log: Vec<_> = (host.tasks().bucket_log(&bucket).into_iter())
                .map(|line| line.text)
                .collect();
            anyhow::ensure!(
                Instant::now() < deadline,
                "the agent never read the new question: {log:?}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        Ok(())
    })
}

/// A control for a bucket the tab no longer shows is refused before it
/// reaches the actor, as the desktop's flow controls are.
#[test]
fn a_control_for_a_moved_bucket_is_refused() -> anyhow::Result<()> {
    with_repo(|dir, repo| {
        let host = ServiceHost::new(gated_ports(Arc::default()), dir.join("config.toml"));
        let (key, _) = loaded(&host, repo)?;
        let cancel = crate::FlowRequest::Cancel {
            er_dir: "/another/bucket".into(),
        };
        let answer = host.flow(key, cancel).recv_timeout(WAIT)?;
        assert!(
            matches!(&answer, Err(crate::WorkspaceError::Failed(message))
                if message.contains("active review bucket changed")),
            "{answer:?}"
        );
        Ok(())
    })
}

/// An acceptance goes to the actor whose view holds the flow, even from a
/// tab of another project; a control goes to the tab's own actor.
#[test]
fn an_acceptance_goes_to_the_actor_holding_the_flow() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let host = ServiceHost::new(Ports::for_test(), dir.path().join("config.toml"));
    let holder = host.handle_for("/a");
    host.handle_for("/b");
    let flow = crate::LiveReviewFlow {
        er_dir: "/flows/a".into(),
        flow: er_engine::ai::review_flow::ReviewFlow::new("hash"),
        draining: false,
        run_targets: Vec::new(),
        rerun_stages: Vec::new(),
    };
    *holder
        .view
        .write()
        .map_err(|_| anyhow::anyhow!("poisoned view"))? = Arc::new(crate::WorkspaceView {
        review_flows: vec![flow],
        ..Default::default()
    });

    let accept = crate::FlowRequest::Accept {
        er_dir: "/flows/a".into(),
        run_id: "run".into(),
        finding_ids: Vec::new(),
    };
    assert_eq!(host.flow_actor("/b", &accept), "/a");
    let cancel = crate::FlowRequest::Cancel {
        er_dir: "/flows/a".into(),
    };
    assert_eq!(host.flow_actor("/b", &cancel), "/b");
    Ok(())
}

/// Runs `request` on a loaded tab with the gated agent, and answers the task
/// kinds listed for the tab's bucket and whether the bucket holds the
/// prepared diff.
fn lens_run(request: RunRequest) -> anyhow::Result<(Vec<String>, bool)> {
    let mut seen = (Vec::new(), false);
    with_repo(|dir, repo| {
        let agent = Agent::in_dir(dir);
        let host = ServiceHost::new(gated_ports(Arc::default()), agent.write_config(50));
        let (key, content) = loaded(&host, repo)?;
        host.run(key, request)
            .recv_timeout(WAIT)?
            .map_err(|error| anyhow::anyhow!(error.message))?;
        let bucket = &content.context.er_dir;
        seen.0 = (host.tasks().rows().into_iter())
            .filter(|row| &row.er_dir == bucket)
            .map(|row| row.kind)
            .collect();
        seen.1 = std::path::Path::new(bucket).join("diff-tmp").exists();
        for row in host.tasks().rows() {
            host.tasks().stop(&row.id);
        }
        crate::test_support::open_gate(&agent.first);
        crate::test_support::open_gate(&agent.second);
        Ok(())
    })?;
    Ok(seen)
}

/// Professor queues the professor lens on a prepared diff.
#[test]
fn a_professor_run_queues_the_professor_lens() -> anyhow::Result<()> {
    let (kinds, prepared) = lens_run(RunRequest::Professor)?;
    assert_eq!(kinds, [er_engine::ai::professor_task_kind()]);
    assert!(prepared, "no prepared diff in the bucket");
    Ok(())
}

/// Triage queues the triage lens on a prepared diff.
#[test]
fn a_triage_run_queues_the_triage_lens() -> anyhow::Result<()> {
    let (kinds, prepared) = lens_run(RunRequest::Triage)?;
    assert_eq!(kinds, [er_engine::ai::triage_task_kind()]);
    assert!(prepared, "no prepared diff in the bucket");
    Ok(())
}

/// Professor refuses History, and both lenses refuse a remote tab, each
/// with the App's message and with no task queued.
#[test]
fn the_lenses_refuse_what_the_app_refuses() -> anyhow::Result<()> {
    with_repo(|dir, repo| {
        let host = ServiceHost::new(gated_ports(Arc::default()), dir.join("config.toml"));
        let (_, content) = loaded(&host, repo)?;
        let mut history = (*content).clone();
        history.tab.mode = er_engine::tab_identity::DiffMode::History;
        let mut remote = (*content).clone();
        remote.context.is_remote = true;
        let hooks = host.task_hooks();
        let refusal =
            |content: &TabContent,
             plan: fn(&crate::runs::RunTab<'_>) -> Result<(), er_protocol::ErError>| {
                let run = crate::runs::RunTab {
                    host: host.tasks(),
                    hooks: &hooks,
                    content,
                };
                plan(&run).err().map(|error| error.message)
            };
        let cases = [
            (
                refusal(&history, crate::runs::professor),
                "Professor not available in this mode",
            ),
            (
                refusal(&remote, crate::runs::professor),
                "Professor is local-only in v1 — checkout the PR first",
            ),
            (
                refusal(&remote, crate::runs::triage),
                "Triage is local-only in v1 — checkout the PR first",
            ),
        ];
        for (refused, message) in cases {
            assert_eq!(refused.as_deref(), Some(message));
        }
        assert!(
            host.tasks().rows().is_empty(),
            "a refused lens queued a task"
        );
        Ok(())
    })
}

/// An acceptance sent through `flow` from another project's tab reaches the
/// actor whose view holds the flow: that actor is held, so no answer comes,
/// where the tab's own actor would answer at once.
#[test]
fn a_flow_acceptance_reaches_the_actor_holding_the_flow() -> anyhow::Result<()> {
    with_repo(|dir, repo| {
        let host = ServiceHost::new(gated_ports(Arc::default()), dir.join("config.toml"));
        let (key, _) = loaded(&host, repo)?;
        let holder = host.handle_for("/a");
        drop(holder.send(Command::Hold));
        let flow = crate::LiveReviewFlow {
            er_dir: "/flows/a".into(),
            flow: er_engine::ai::review_flow::ReviewFlow::new("hash"),
            draining: false,
            run_targets: Vec::new(),
            rerun_stages: Vec::new(),
        };
        *holder
            .view
            .write()
            .map_err(|_| anyhow::anyhow!("poisoned view"))? = Arc::new(crate::WorkspaceView {
            review_flows: vec![flow],
            ..Default::default()
        });
        let accept = crate::FlowRequest::Accept {
            er_dir: "/flows/a".into(),
            run_id: "run".into(),
            finding_ids: Vec::new(),
        };
        let answer = host.flow(key, accept).recv_timeout(Duration::from_secs(1));
        assert!(
            matches!(answer, Err(std::sync::mpsc::RecvTimeoutError::Timeout)),
            "the tab's own actor answered: {answer:?}"
        );
        Ok(())
    })
}
