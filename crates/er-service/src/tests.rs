//! Actor behaviour: the round trip, events, failures, the guard, and the
//! bounded wait. Every test owns its deadline: a stuck actor fails the test
//! instead of hanging the suite.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use er_engine::ports::Ports;
use er_github::{FakeGitHub, GhError};

use crate::actor::WorkspaceDeps;
use crate::effect::resolve_target;
use crate::guard::TRIPS;
use crate::handle::WorkspaceError;
use crate::protocol::{Command, Event, OpenTarget, Reply};
use crate::{spawn, WorkspaceHandle};

/// A fake whose `pr_state` answers with `state`, or fails with its message.
fn github(state: Result<&str, &str>) -> Arc<FakeGitHub> {
    let answer = match state {
        Ok(value) => Ok(value.to_string()),
        Err(message) => Err(GhError::Local {
            message: message.to_string(),
        }),
    };
    Arc::new(FakeGitHub::default().with_pr_state(answer))
}

fn remote_pr() -> OpenTarget {
    OpenTarget::RemotePr {
        owner: "acme".into(),
        repo: "widgets".into(),
        number: 7,
        repo_root: "/repo".into(),
    }
}

fn open_remote() -> Command {
    Command::OpenTarget {
        target: remote_pr(),
        replace: false,
    }
}

fn actor(github: Arc<FakeGitHub>) -> WorkspaceHandle {
    spawn(WorkspaceDeps::new(Ports::with_github(github)))
}

#[test]
fn a_command_round_trips_and_publishes_a_view() -> anyhow::Result<()> {
    let github = github(Ok("OPEN"));
    let handle = actor(Arc::clone(&github));

    let reply = handle.send_and_wait_with(open_remote(), Duration::from_secs(5))?;
    let Reply::TargetOpened(target) = reply else {
        anyhow::bail!("unexpected reply: {reply:?}");
    };
    assert_eq!(target.repo_root, "/repo");
    assert_eq!(target.remote, "acme/widgets");
    assert_eq!(target.pr_number, Some(7));
    assert_eq!(target.label, "acme/widgets#7");
    assert_eq!(target.pr_state.as_deref(), Some("OPEN"));
    assert_eq!(
        github.calls(),
        vec!["pr_state(owner=\"acme\", repo=\"widgets\", number=7)"]
    );

    let view = handle.view();
    assert_eq!(view.revision, 1);
    assert_eq!(view.targets, vec![target]);
    assert_eq!(view.active, Some(0));
    assert_eq!(view.last_error, None);
    Ok(())
}

#[test]
fn opening_publishes_the_view_then_the_target() -> anyhow::Result<()> {
    let handle = actor(github(Ok("OPEN")));
    let events = handle.subscribe();

    let reply = handle.send(open_remote());
    let first = events.recv_timeout(Duration::from_secs(5))?;
    assert_eq!(first, Event::ViewPublished { revision: 1 });
    let second = events.recv_timeout(Duration::from_secs(5))?;
    assert!(matches!(second, Event::TargetOpened { .. }));
    let Ok(Ok(Reply::TargetOpened(_))) = reply.recv_timeout(Duration::from_secs(5)) else {
        anyhow::bail!("the reply did not arrive");
    };
    Ok(())
}

#[test]
fn a_failed_effect_publishes_the_error_and_answers_with_it() -> anyhow::Result<()> {
    let handle = actor(github(Err("gh exploded")));
    let events = handle.subscribe();

    let result = handle.send_and_wait_with(open_remote(), Duration::from_secs(5));
    assert_eq!(
        result,
        Err(WorkspaceError::Failed("gh exploded".to_string()))
    );

    let view = handle.view();
    assert_eq!(view.revision, 1);
    assert!(view.targets.is_empty());
    assert_eq!(view.active, None);
    assert_eq!(view.last_error.as_deref(), Some("gh exploded"));

    let first = events.recv_timeout(Duration::from_secs(5))?;
    assert_eq!(first, Event::ViewPublished { revision: 1 });
    let second = events.recv_timeout(Duration::from_secs(5))?;
    assert_eq!(
        second,
        Event::CommandFailed {
            message: "gh exploded".to_string()
        }
    );
    Ok(())
}

#[test]
fn reopening_the_same_target_replaces_it_and_keeps_one_active_entry() -> anyhow::Result<()> {
    let handle = actor(github(Ok("OPEN")));
    handle.send_and_wait_with(open_remote(), Duration::from_secs(5))?;
    handle.send_and_wait_with(open_remote(), Duration::from_secs(5))?;

    let view = handle.view();
    assert_eq!(view.revision, 2);
    assert_eq!(view.targets.len(), 1);
    assert_eq!(view.active, Some(0));
    Ok(())
}

#[test]
fn a_command_without_a_reply_times_out_within_its_own_deadline() -> anyhow::Result<()> {
    let handle = actor(github(Ok("OPEN")));
    let started = Instant::now();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let caller = handle.clone();
    std::thread::spawn(move || {
        let result = caller.send_and_wait_with(Command::Stall, Duration::from_millis(100));
        let _ = done_tx.send(result);
    });

    let result = done_rx.recv_timeout(Duration::from_secs(2))?;
    assert_eq!(
        result,
        Err(WorkspaceError::Timeout(Duration::from_millis(100)))
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "the caller hung"
    );

    // The actor was not blocked by the stalled command.
    let reply = handle.send_and_wait_with(Command::Ping, Duration::from_secs(2))?;
    assert_eq!(reply, Reply::Pong);
    Ok(())
}

#[test]
fn the_actor_refuses_a_port_call_on_its_own_thread() -> anyhow::Result<()> {
    let handle = actor(github(Ok("OPEN")));
    let trips = TRIPS.load(Ordering::SeqCst);
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let caller = handle.clone();
    std::thread::spawn(move || {
        let result = caller.send_and_wait_with(Command::PortProbe, Duration::from_secs(5));
        let _ = done_tx.send(result);
    });

    // The probe panics the actor thread on purpose; its panic is expected and
    // printed by the default hook.
    let result = done_rx.recv_timeout(Duration::from_secs(5))?;
    assert_eq!(result, Err(WorkspaceError::Disconnected));
    assert_eq!(TRIPS.load(Ordering::SeqCst), trips + 1);
    Ok(())
}

#[test]
fn the_actor_loop_never_dereferences_a_port() {
    let source = include_str!("actor.rs");
    for forbidden in [".github", ".git", "CliGit", "CliGitHub"] {
        assert!(
            !source.contains(forbidden),
            "the actor loop reached a port through `{forbidden}`"
        );
    }
    assert!(
        source.contains("resolve_target(&deps"),
        "the effect call moved out of the loop"
    );
}

/// Times the real routed open command: parse the target, fetch the PR state
/// through the scripted port off-actor, apply the result, answer the caller.
/// The baseline is the same effect called directly with the same port.
#[test]
#[ignore = "latency harness: run with --ignored --nocapture and paste the numbers in the PR; never a CI assertion"]
fn the_actor_hop_stays_within_the_latency_budget() -> anyhow::Result<()> {
    const RUNS: usize = 100;
    let github = github(Ok("OPEN"));
    let github_port: Arc<dyn er_github::GitHub> = github.clone();
    let deps = WorkspaceDeps::new(Ports::with_github(github_port));
    let target = remote_pr();

    // Baseline: the same effect called directly, with the same scripted port.
    let direct = median_duration(RUNS, || Ok(resolve_target(&deps, &target).map(|_| ())?))?;

    let handle = spawn(deps);
    handle.send_and_wait_with(open_remote(), Duration::from_secs(5))?;
    // Every timed command must do the real work — parse, the off-actor port
    // call, the apply — or the comparison times an empty round trip.
    let calls_before = github.calls().len();
    let actor = median_duration(RUNS, || {
        Ok(handle
            .send_and_wait_with(open_remote(), Duration::from_secs(5))
            .map(|_| ())?)
    })?;
    assert_eq!(
        github.calls().len(),
        calls_before + RUNS,
        "a timed command skipped the port call"
    );

    println!("direct median {direct:?}, actor median {actor:?} over {RUNS} runs");
    let budget = direct * 11 / 10 + Duration::from_millis(2);
    assert!(
        actor <= budget,
        "the actor hop cost {actor:?}, over the {budget:?} budget (direct {direct:?})"
    );
    Ok(())
}

fn median_duration(
    runs: usize,
    mut operation: impl FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<Duration> {
    let mut samples = Vec::with_capacity(runs);
    for _ in 0..runs {
        let started = Instant::now();
        operation()?;
        samples.push(started.elapsed());
    }
    samples.sort_unstable();
    Ok(samples[runs / 2])
}

/// The annotation commands: the diff is fetched through the port, the target
/// is the command's, and the painted id survives the write.
mod annotations {
    use std::sync::Arc;
    use std::time::Duration;

    use er_engine::ports::Ports;
    use er_git::FakeGit;
    use er_github::FakeGitHub;

    use crate::actor::WorkspaceDeps;
    use crate::handle::WorkspaceError;
    use crate::protocol::{AnnotationTarget, Command, OpenTarget, Reply, TabContext};
    use crate::{spawn, WorkspaceHandle};

    const RAW: &str = "\
diff --git a/src/a.rs b/src/a.rs
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,2 +1,2 @@
 a-one
-a-old
+a-new
diff --git a/src/b.rs b/src/b.rs
--- a/src/b.rs
+++ b/src/b.rs
@@ -1,3 +1,3 @@
 b-one
-b-old
+b-new
 b-three
";

    fn context(bucket: &str, repo_root: &str) -> TabContext {
        TabContext {
            er_dir: bucket.to_string(),
            comments_dir: bucket.to_string(),
            repo_root: repo_root.to_string(),
            base_branch: "main".to_string(),
            scope: "branch".to_string(),
            remote_repo: None,
            pr_number: None,
            pr_head_ref: None,
            local_branch_view: Some("feature".to_string()),
            checkout_root: None,
            branch_diff_hash: String::new(),
            diff_hash: "diff-hash".to_string(),
        }
    }

    fn ports(raw: &str) -> Ports {
        let git: Arc<dyn er_git::Git> =
            Arc::new(FakeGit::default().with_diff_against_branch(Ok(raw.to_string())));
        let github: Arc<dyn er_github::GitHub> = Arc::new(FakeGitHub::default());
        Ports { git, github }
    }

    fn actor(raw: &str) -> WorkspaceHandle {
        spawn(WorkspaceDeps::new(ports(raw)))
    }

    fn target(file: &str, line: Option<usize>) -> AnnotationTarget {
        AnnotationTarget {
            file: file.to_string(),
            hunk_idx: 0,
            line_num: line,
            line_num_end: None,
        }
    }

    #[test]
    fn toggle_reviewed_uses_the_named_path_and_the_diff_hash() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let bucket = dir.path().join("bucket");
        std::fs::create_dir_all(&bucket)?;
        let bucket_text = bucket.to_string_lossy().into_owned();
        er_engine::storage::with_process_test_storage_root(dir.path(), || -> anyhow::Result<()> {
            let handle = actor(RAW);
            let reply = handle.send_and_wait_with(
                Command::ToggleReviewed {
                    context: context(&bucket_text, "/repo"),
                    path: "src/b.rs".to_string(),
                },
                Duration::from_secs(5),
            )?;
            assert_eq!(reply, Reply::ReviewedChanged { changed: 1 });

            let store = er_engine::store_registry::store();
            let scope = er_store::Scope::dir(&bucket_text)?;
            let map = er_engine::session_store::load_reviewed(&store, &scope)?;
            let expected =
                er_engine::ai::compute_per_file_hash(RAW, "src/b.rs").expect("b.rs is in the diff");
            assert_eq!(map.get("src/b.rs"), Some(&expected));
            assert!(!map.contains_key("src/a.rs"), "only the named path flips");

            let reply = handle.send_and_wait_with(
                Command::ToggleReviewed {
                    context: context(&bucket_text, "/repo"),
                    path: "src/b.rs".to_string(),
                },
                Duration::from_secs(5),
            )?;
            assert_eq!(reply, Reply::ReviewedChanged { changed: 1 });
            let map = er_engine::session_store::load_reviewed(&store, &scope)?;
            assert!(!map.contains_key("src/b.rs"), "the second toggle clears");
            Ok(())
        })
    }

    #[test]
    fn an_annotation_lands_on_the_named_target_and_keeps_the_painted_id() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let bucket = dir.path().join("bucket");
        std::fs::create_dir_all(&bucket)?;
        let bucket_text = bucket.to_string_lossy().into_owned();
        er_engine::storage::with_process_test_storage_root(dir.path(), || -> anyhow::Result<()> {
            let handle = actor(RAW);
            let reply = handle.send_and_wait_with(
                Command::AddQuestion {
                    context: context(&bucket_text, "/repo"),
                    target: target("src/b.rs", Some(2)),
                    body: "why?".to_string(),
                    side: Some("RIGHT".to_string()),
                    client_id: Some("q-opt-7".to_string()),
                    reply_to: None,
                    finding_ref: None,
                },
                Duration::from_secs(5),
            )?;
            assert_eq!(
                reply,
                Reply::AnnotationWritten {
                    id: "q-opt-7".to_string()
                }
            );

            let store = er_engine::store_registry::store();
            let scope = er_store::Scope::dir(&bucket_text)?;
            let er_store::Loaded::Found(stored) =
                er_engine::ai::annotation_store::load_questions(&store, &scope)?
            else {
                anyhow::bail!("questions were not written");
            };
            let question = &stored.value.questions[0];
            assert_eq!(question.id, "q-opt-7");
            assert_eq!(question.file, "src/b.rs");
            assert_eq!(question.line_content, "b-new");
            assert_eq!(question.relocated_at_hash, "diff-hash");
            Ok(())
        })
    }

    #[test]
    fn a_reply_reuses_the_painted_id_and_points_at_its_parent() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let bucket = dir.path().join("bucket");
        std::fs::create_dir_all(&bucket)?;
        let bucket_text = bucket.to_string_lossy().into_owned();
        er_engine::storage::with_process_test_storage_root(dir.path(), || -> anyhow::Result<()> {
            let handle = actor(RAW);
            handle.send_and_wait_with(
                Command::AddQuestion {
                    context: context(&bucket_text, "/repo"),
                    target: target("src/a.rs", Some(2)),
                    body: "root".to_string(),
                    side: None,
                    client_id: Some("q-opt-1".to_string()),
                    reply_to: None,
                    finding_ref: None,
                },
                Duration::from_secs(5),
            )?;
            let reply = handle.send_and_wait_with(
                Command::ReplyToThread {
                    context: context(&bucket_text, "/repo"),
                    parent_id: "q-opt-1".to_string(),
                    body: "reply".to_string(),
                    client_id: Some("q-opt-2".to_string()),
                },
                Duration::from_secs(5),
            )?;
            assert_eq!(
                reply,
                Reply::AnnotationWritten {
                    id: "q-opt-2".to_string()
                }
            );
            let store = er_engine::store_registry::store();
            let scope = er_store::Scope::dir(&bucket_text)?;
            let er_store::Loaded::Found(stored) =
                er_engine::ai::annotation_store::load_questions(&store, &scope)?
            else {
                anyhow::bail!("questions were not written");
            };
            let reply_row = stored
                .value
                .questions
                .iter()
                .find(|q| q.id == "q-opt-2")
                .expect("the reply row");
            assert_eq!(reply_row.in_reply_to.as_deref(), Some("q-opt-1"));
            assert_eq!(reply_row.file, "src/a.rs");
            Ok(())
        })
    }

    #[test]
    fn a_command_for_another_repo_is_refused_as_stale() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let bucket = dir.path().join("bucket");
        std::fs::create_dir_all(&bucket)?;
        let bucket_text = bucket.to_string_lossy().into_owned();
        er_engine::storage::with_process_test_storage_root(dir.path(), || -> anyhow::Result<()> {
            let git: Arc<dyn er_git::Git> =
                Arc::new(FakeGit::default().with_diff_against_branch(Ok(RAW.to_string())));
            let github: Arc<dyn er_github::GitHub> =
                Arc::new(FakeGitHub::default().with_pr_state(Ok("OPEN".to_string())));
            let handle = spawn(WorkspaceDeps::new(Ports { git, github }));
            let opened = handle.send_and_wait_with(
                Command::OpenTarget {
                    target: OpenTarget::RemotePr {
                        owner: "acme".to_string(),
                        repo: "widgets".to_string(),
                        number: 7,
                        repo_root: "/repo-a".to_string(),
                    },
                    replace: false,
                },
                Duration::from_secs(5),
            )?;
            assert!(matches!(opened, Reply::TargetOpened(_)));

            let refused = handle.send_and_wait_with(
                Command::ToggleReviewed {
                    context: context(&bucket_text, "/repo-b"),
                    path: "src/b.rs".to_string(),
                },
                Duration::from_secs(5),
            );
            assert!(
                matches!(refused, Err(WorkspaceError::Failed(ref message)) if message.contains("moved")),
                "got {refused:?}"
            );
            let store = er_engine::store_registry::store();
            let scope = er_store::Scope::dir(&bucket_text)?;
            let map = er_engine::session_store::load_reviewed(&store, &scope)?;
            assert!(map.is_empty(), "a refused write changes nothing");
            Ok(())
        })
    }
}
