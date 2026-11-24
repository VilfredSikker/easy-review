//! Runs through the host: planning a run, which reads git, never holds the
//! caller, and a questions run points the agent at the bucket file that
//! holds the question just added.

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
use crate::tab_commands::{tab_command_prompt, TabCommand};
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

/// A question added on the tab lands in the bucket file the questions
/// run's prompt tells the agent to read.
#[test]
fn a_questions_run_points_the_agent_at_the_new_question() -> anyhow::Result<()> {
    with_repo(|dir, repo| {
        let closed = Arc::new(AtomicBool::new(false));
        let host = ServiceHost::new(gated_ports(closed), dir.join("config.toml"));
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

        let content = host
            .content(&workspace, key)
            .ok_or_else(|| anyhow::anyhow!("no content"))?;
        let prompt = tab_command_prompt(&content, TabCommand::Questions)
            .map_err(|error| anyhow::anyhow!(error.message))?;
        let file = format!(
            "{}/questions.json",
            er_engine::ai::prompts::sanitize_for_shell(&bucket)
        );
        assert!(
            prompt.contains(&format!("Read `{file}`")),
            "the prompt names no {file}:\n{prompt}"
        );
        Ok(())
    })
}
