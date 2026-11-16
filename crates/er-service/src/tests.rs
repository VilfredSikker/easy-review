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
    // The loop, its reload gate, and the content patches it applies.
    for (file, source) in [
        ("actor.rs", include_str!("actor.rs")),
        ("content.rs", include_str!("content.rs")),
        ("workspace.rs", include_str!("workspace.rs")),
        ("sidecar_reload.rs", include_str!("sidecar_reload.rs")),
    ] {
        for forbidden in [".github", ".git", "CliGit", "CliGitHub"] {
            assert!(
                !source.contains(forbidden),
                "{file} reached a port through `{forbidden}`"
            );
        }
    }
    let source = include_str!("actor.rs");
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
    use crate::protocol::{
        AnnotationKind, AnnotationTarget, Command, OpenTarget, Reply, TabContext,
    };
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
            is_remote: false,
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

    /// A temp root and one bucket under it. The caller keeps the root alive.
    fn temp_bucket() -> anyhow::Result<(tempfile::TempDir, String)> {
        let dir = tempfile::tempdir()?;
        let bucket = dir.path().join("bucket");
        std::fs::create_dir_all(&bucket)?;
        Ok((dir, bucket.to_string_lossy().into_owned()))
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

            // Each GitHub comment command is refused before its effect reaches
            // the poster.
            let stale = || context(&bucket_text, "/repo-b");
            let late = || "too late".to_string();
            let comments = [
                Command::AddComment {
                    context: stale(),
                    target: target("src/b.rs", Some(2)),
                    body: late(),
                    side: None,
                    client_id: None,
                },
                Command::ReplyToGithubThread {
                    context: stale(),
                    parent_id: "gh-1".to_string(),
                    body: late(),
                    client_id: None,
                },
                Command::PromoteToComment {
                    context: stale(),
                    id: "q-1".to_string(),
                    body: None,
                },
                Command::PromoteFindingToComment {
                    context: stale(),
                    finding_id: "f-1".to_string(),
                    body: None,
                },
            ];
            for comment in comments {
                let refused = handle.send_and_wait_with(comment, Duration::from_secs(5));
                assert!(
                    matches!(refused, Err(WorkspaceError::Failed(ref message)) if message.contains("moved")),
                    "got {refused:?}"
                );
            }
            Ok(())
        })
    }

    #[test]
    fn mark_reviewed_sets_and_clears_the_named_path() -> anyhow::Result<()> {
        let (dir, bucket) = temp_bucket()?;
        er_engine::storage::with_process_test_storage_root(dir.path(), || -> anyhow::Result<()> {
            let handle = actor(RAW);
            let store = er_engine::store_registry::store();
            let scope = er_store::Scope::dir(&bucket)?;
            for (reviewed, present) in [(true, true), (false, false)] {
                let reply = handle.send_and_wait_with(
                    Command::MarkReviewed {
                        context: context(&bucket, "/repo"),
                        path: "src/a.rs".to_string(),
                        reviewed,
                    },
                    Duration::from_secs(5),
                )?;
                assert_eq!(reply, Reply::ReviewedChanged { changed: 1 });
                let map = er_engine::session_store::load_reviewed(&store, &scope)?;
                assert_eq!(map.contains_key("src/a.rs"), present);
            }
            Ok(())
        })
    }

    #[test]
    fn bulk_review_marks_every_path_of_the_named_pillar() -> anyhow::Result<()> {
        let (dir, bucket) = temp_bucket()?;
        er_engine::test_support::write_sidecar_json::<er_engine::ai::ErTour>(
            &dir.path().join("bucket"),
            serde_json::json!({
                "version": 1,
                "diff_hash": "",
                "pillars": [{"id": "p-1", "title": "One", "files": [{"path": "src/a.rs"}]}]
            }),
        );
        er_engine::storage::with_process_test_storage_root(dir.path(), || -> anyhow::Result<()> {
            let handle = actor(RAW);
            let reply = handle.send_and_wait_with(
                Command::BulkReviewPillar {
                    context: context(&bucket, "/repo"),
                    pillar_id: "p-1".to_string(),
                    reviewed: true,
                },
                Duration::from_secs(5),
            )?;
            assert_eq!(reply, Reply::ReviewedChanged { changed: 1 });

            let store = er_engine::store_registry::store();
            let scope = er_store::Scope::dir(&bucket)?;
            let map = er_engine::session_store::load_reviewed(&store, &scope)?;
            assert!(map.contains_key("src/a.rs"), "the pillar's path flips");
            assert!(!map.contains_key("src/b.rs"), "another pillar's path stays");
            Ok(())
        })
    }

    #[test]
    fn add_note_writes_a_note_with_the_painted_id() -> anyhow::Result<()> {
        let (dir, bucket) = temp_bucket()?;
        er_engine::storage::with_process_test_storage_root(dir.path(), || -> anyhow::Result<()> {
            let handle = actor(RAW);
            let reply = handle.send_and_wait_with(
                Command::AddNote {
                    context: context(&bucket, "/repo"),
                    target: target("src/b.rs", Some(2)),
                    body: "remember this".to_string(),
                    side: None,
                    client_id: Some("n-opt-1".to_string()),
                    reply_to: None,
                    finding_ref: None,
                },
                Duration::from_secs(5),
            )?;
            assert_eq!(
                reply,
                Reply::AnnotationWritten {
                    id: "n-opt-1".to_string()
                }
            );

            let store = er_engine::store_registry::store();
            let scope = er_store::Scope::dir(&bucket)?;
            let er_store::Loaded::Found(stored) =
                er_engine::ai::annotation_store::load_notes(&store, &scope)?
            else {
                anyhow::bail!("notes were not written");
            };
            let note = &stored.value.notes[0];
            assert_eq!(note.id, "n-opt-1");
            assert_eq!(note.file, "src/b.rs");
            assert_eq!(note.text, "remember this");
            Ok(())
        })
    }

    #[test]
    fn add_document_thread_anchors_on_the_document_line() -> anyhow::Result<()> {
        let (dir, bucket) = temp_bucket()?;
        er_engine::storage::with_process_test_storage_root(dir.path(), || -> anyhow::Result<()> {
            let handle = actor(RAW);
            let reply = handle.send_and_wait_with(
                Command::AddDocumentThread {
                    context: context(&bucket, "/repo"),
                    file: "src/b.rs".to_string(),
                    line: 2,
                    line_end: None,
                    kind: AnnotationKind::Question,
                    body: "why this line?".to_string(),
                    source: "alpha\nbravo\ncharlie".to_string(),
                },
                Duration::from_secs(5),
            )?;
            let Reply::AnnotationWritten { id } = reply else {
                anyhow::bail!("got {reply:?}");
            };

            let store = er_engine::store_registry::store();
            let scope = er_store::Scope::dir(&bucket)?;
            let er_store::Loaded::Found(stored) =
                er_engine::ai::annotation_store::load_questions(&store, &scope)?
            else {
                anyhow::bail!("questions were not written");
            };
            let question = stored
                .value
                .questions
                .iter()
                .find(|q| q.id == id)
                .expect("the document thread");
            assert!(question.is_document_anchor(), "no hunk owns the line");
            assert_eq!(question.line_start, Some(2));
            assert_eq!(question.line_content, "bravo");
            assert_eq!(question.hunk_index, None);
            Ok(())
        })
    }

    #[test]
    fn update_thread_message_replaces_the_body() -> anyhow::Result<()> {
        let (dir, bucket) = temp_bucket()?;
        er_engine::storage::with_process_test_storage_root(dir.path(), || -> anyhow::Result<()> {
            let handle = actor(RAW);
            handle.send_and_wait_with(
                Command::AddQuestion {
                    context: context(&bucket, "/repo"),
                    target: target("src/a.rs", Some(2)),
                    body: "first".to_string(),
                    side: None,
                    client_id: Some("q-opt-1".to_string()),
                    reply_to: None,
                    finding_ref: None,
                },
                Duration::from_secs(5),
            )?;
            let reply = handle.send_and_wait_with(
                Command::UpdateThreadMessage {
                    context: context(&bucket, "/repo"),
                    id: "q-opt-1".to_string(),
                    body: "edited".to_string(),
                },
                Duration::from_secs(5),
            )?;
            assert!(
                matches!(reply, Reply::ThreadChanged { ref path } if path.ends_with("questions.json"))
            );

            let store = er_engine::store_registry::store();
            let scope = er_store::Scope::dir(&bucket)?;
            let er_store::Loaded::Found(stored) =
                er_engine::ai::annotation_store::load_questions(&store, &scope)?
            else {
                anyhow::bail!("questions were not written");
            };
            assert_eq!(stored.value.questions[0].text, "edited");
            Ok(())
        })
    }

    #[test]
    fn delete_thread_removes_the_root_and_its_replies() -> anyhow::Result<()> {
        let (dir, bucket) = temp_bucket()?;
        er_engine::storage::with_process_test_storage_root(dir.path(), || -> anyhow::Result<()> {
            let handle = actor(RAW);
            handle.send_and_wait_with(
                Command::AddQuestion {
                    context: context(&bucket, "/repo"),
                    target: target("src/a.rs", Some(2)),
                    body: "root".to_string(),
                    side: None,
                    client_id: Some("q-opt-1".to_string()),
                    reply_to: None,
                    finding_ref: None,
                },
                Duration::from_secs(5),
            )?;
            handle.send_and_wait_with(
                Command::ReplyToThread {
                    context: context(&bucket, "/repo"),
                    parent_id: "q-opt-1".to_string(),
                    body: "reply".to_string(),
                    client_id: Some("q-opt-2".to_string()),
                },
                Duration::from_secs(5),
            )?;
            let reply = handle.send_and_wait_with(
                Command::DeleteThread {
                    context: context(&bucket, "/repo"),
                    id: "q-opt-1".to_string(),
                },
                Duration::from_secs(5),
            )?;
            assert!(
                matches!(reply, Reply::ThreadChanged { ref path } if path.ends_with("questions.json"))
            );

            let store = er_engine::store_registry::store();
            let scope = er_store::Scope::dir(&bucket)?;
            let er_store::Loaded::Found(stored) =
                er_engine::ai::annotation_store::load_questions(&store, &scope)?
            else {
                anyhow::bail!("questions were not written");
            };
            assert!(
                stored.value.questions.is_empty(),
                "the root and its reply are both gone"
            );
            Ok(())
        })
    }

    #[test]
    fn resolve_thread_marks_the_row_resolved() -> anyhow::Result<()> {
        let (dir, bucket) = temp_bucket()?;
        er_engine::storage::with_process_test_storage_root(dir.path(), || -> anyhow::Result<()> {
            let handle = actor(RAW);
            handle.send_and_wait_with(
                Command::AddQuestion {
                    context: context(&bucket, "/repo"),
                    target: target("src/a.rs", Some(2)),
                    body: "open".to_string(),
                    side: None,
                    client_id: Some("q-opt-1".to_string()),
                    reply_to: None,
                    finding_ref: None,
                },
                Duration::from_secs(5),
            )?;
            let reply = handle.send_and_wait_with(
                Command::ResolveThread {
                    context: context(&bucket, "/repo"),
                    id: "q-opt-1".to_string(),
                },
                Duration::from_secs(5),
            )?;
            assert!(
                matches!(reply, Reply::ThreadChanged { ref path } if path.ends_with("questions.json"))
            );

            let store = er_engine::store_registry::store();
            let scope = er_store::Scope::dir(&bucket)?;
            let er_store::Loaded::Found(stored) =
                er_engine::ai::annotation_store::load_questions(&store, &scope)?
            else {
                anyhow::bail!("questions were not written");
            };
            assert!(stored.value.questions[0].resolved);
            Ok(())
        })
    }

    #[test]
    fn promote_to_note_moves_the_question_into_the_notes() -> anyhow::Result<()> {
        let (dir, bucket) = temp_bucket()?;
        er_engine::storage::with_process_test_storage_root(dir.path(), || -> anyhow::Result<()> {
            let handle = actor(RAW);
            handle.send_and_wait_with(
                Command::AddQuestion {
                    context: context(&bucket, "/repo"),
                    target: target("src/a.rs", Some(2)),
                    body: "keep this".to_string(),
                    side: None,
                    client_id: Some("q-opt-1".to_string()),
                    reply_to: None,
                    finding_ref: None,
                },
                Duration::from_secs(5),
            )?;
            let reply = handle.send_and_wait_with(
                Command::PromoteToNote {
                    context: context(&bucket, "/repo"),
                    id: "q-opt-1".to_string(),
                    body: None,
                },
                Duration::from_secs(5),
            )?;
            let Reply::NotePromoted { id } = reply else {
                anyhow::bail!("got {reply:?}");
            };
            assert!(id.starts_with("n-"), "promotion mints a note id: {id}");

            let store = er_engine::store_registry::store();
            let scope = er_store::Scope::dir(&bucket)?;
            let er_store::Loaded::Found(notes) =
                er_engine::ai::annotation_store::load_notes(&store, &scope)?
            else {
                anyhow::bail!("notes were not written");
            };
            let note = notes
                .value
                .notes
                .iter()
                .find(|n| n.id == id)
                .expect("the promoted note");
            assert_eq!(note.file, "src/a.rs");
            assert_eq!(note.text, "keep this");
            let er_store::Loaded::Found(questions) =
                er_engine::ai::annotation_store::load_questions(&store, &scope)?
            else {
                anyhow::bail!("the questions file vanished");
            };
            assert!(
                questions.value.questions.is_empty(),
                "the source question is gone"
            );
            Ok(())
        })
    }

    #[test]
    fn reply_to_finding_starts_a_thread_rooted_at_the_finding() -> anyhow::Result<()> {
        let (dir, bucket) = temp_bucket()?;
        er_engine::test_support::write_sidecar_json::<er_engine::ai::ErReview>(
            &dir.path().join("bucket"),
            serde_json::json!({
                "version": 1,
                "diff_hash": "diff-hash",
                "files": {
                    "src/b.rs": {
                        "risk": "low",
                        "findings": [{
                            "id": "f-1",
                            "severity": "low",
                            "title": "A defect",
                            "hunk_index": 0,
                            "line_start": 2
                        }]
                    }
                }
            }),
        );
        er_engine::storage::with_process_test_storage_root(dir.path(), || -> anyhow::Result<()> {
            let handle = actor(RAW);
            let reply = handle.send_and_wait_with(
                Command::ReplyToFinding {
                    context: context(&bucket, "/repo"),
                    finding_id: "f-1".to_string(),
                    body: "agreed".to_string(),
                    ai_assist: false,
                    client_id: None,
                },
                Duration::from_secs(5),
            )?;
            let Reply::FindingReplied { root_id, reply_id } = reply else {
                anyhow::bail!("got {reply:?}");
            };
            assert_ne!(root_id, reply_id);

            let store = er_engine::store_registry::store();
            let scope = er_store::Scope::dir(&bucket)?;
            let er_store::Loaded::Found(stored) =
                er_engine::ai::annotation_store::load_github_comments(&store, &scope)?
            else {
                anyhow::bail!("comments were not written");
            };
            let root = stored
                .value
                .comments
                .iter()
                .find(|c| c.id == root_id)
                .expect("the thread root");
            assert_eq!(root.finding_ref.as_deref(), Some("f-1"));
            assert_eq!(root.file, "src/b.rs");
            let reply_row = stored
                .value
                .comments
                .iter()
                .find(|c| c.id == reply_id)
                .expect("the reply");
            assert_eq!(reply_row.in_reply_to.as_deref(), Some(root_id.as_str()));
            assert_eq!(reply_row.comment, "agreed");
            Ok(())
        })
    }

    // ── GitHub comment creates and promotes (P41.2) ──

    fn pr_context(bucket: &str) -> TabContext {
        TabContext {
            is_remote: true,
            remote_repo: Some("acme/widgets".to_string()),
            pr_number: Some(7),
            // A remote tab has no checkout to diff against.
            local_branch_view: None,
            ..context(bucket, "/repo")
        }
    }

    fn posting_actor(raw: &str, github: FakeGitHub) -> WorkspaceHandle {
        posting_actor_shared(raw, &Arc::new(github))
    }

    /// Keeps the fake, so a test can read the calls the effects made.
    fn posting_actor_shared(raw: &str, github: &Arc<FakeGitHub>) -> WorkspaceHandle {
        let git: Arc<dyn er_git::Git> =
            Arc::new(FakeGit::default().with_diff_against_branch(Ok(raw.to_string())));
        let github: Arc<dyn er_github::GitHub> = github.clone();
        spawn(WorkspaceDeps::new(Ports { git, github }))
    }

    fn calls_to(github: &FakeGitHub, method: &str) -> usize {
        let calls = github.calls();
        calls.iter().filter(|call| call.starts_with(method)).count()
    }

    fn line_post(id: u64) -> er_github::CreatedComment {
        er_github::CreatedComment {
            id,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            author: "you".to_string(),
        }
    }

    fn scripted_line_post(id: u64) -> FakeGitHub {
        FakeGitHub::default()
            .with_pr_diff_remote(Ok(RAW.to_string()))
            .with_pr_head_oid(Ok("abc123".to_string()))
            .with_post_line_comment(Ok(line_post(id)))
    }

    fn github_rows(bucket: &str) -> anyhow::Result<er_engine::ai::ErGithubComments> {
        let store = er_engine::store_registry::store();
        let scope = er_store::Scope::dir(bucket)?;
        let er_store::Loaded::Found(stored) =
            er_engine::ai::annotation_store::load_github_comments(&store, &scope)?
        else {
            anyhow::bail!("no GitHub comment was recorded");
        };
        Ok(stored.value)
    }

    fn ask(handle: &WorkspaceHandle, bucket: &str, id: &str, body: &str) -> anyhow::Result<()> {
        let question = Command::AddQuestion {
            context: context(bucket, "/repo"),
            target: target("src/b.rs", Some(2)),
            body: body.to_string(),
            side: None,
            client_id: Some(id.to_string()),
            reply_to: None,
            finding_ref: None,
        };
        handle.send_and_wait_with(question, Duration::from_secs(5))?;
        Ok(())
    }

    #[test]
    fn add_comment_posts_and_records_the_github_row() -> anyhow::Result<()> {
        let (dir, bucket) = temp_bucket()?;
        er_engine::storage::with_process_test_storage_root(dir.path(), || -> anyhow::Result<()> {
            let github = Arc::new(scripted_line_post(4242));
            let handle = posting_actor_shared(RAW, &github);
            let reply = handle.send_and_wait_with(
                Command::AddComment {
                    context: pr_context(&bucket),
                    target: target("src/b.rs", Some(2)),
                    body: "look here".to_string(),
                    side: Some("RIGHT".to_string()),
                    client_id: Some("c-opt-1".to_string()),
                },
                Duration::from_secs(5),
            )?;
            assert_eq!(
                reply,
                Reply::AnnotationWritten {
                    id: "gh-4242".to_string()
                }
            );
            let dropped = calls_to(&github, "invalidate_comments(");
            assert!(dropped >= 1, "the pull's comment cache kept the old list");

            let rows = github_rows(&bucket)?;
            let row = &rows.comments[0];
            assert_eq!(row.id, "gh-4242");
            assert_eq!(row.comment, "look here");
            assert_eq!(row.file, "src/b.rs");
            assert_eq!(row.line_start, Some(2));
            assert_eq!(row.line_content, "b-new");
            assert_eq!(row.github_id, Some(4242));
            assert_eq!(row.in_reply_to, None);
            Ok(())
        })
    }

    #[test]
    fn reply_to_github_thread_posts_a_reply_under_its_parent() -> anyhow::Result<()> {
        let (dir, bucket) = temp_bucket()?;
        er_engine::test_support::write_sidecar_json::<er_engine::ai::ErGithubComments>(
            &dir.path().join("bucket"),
            serde_json::json!({
                "version": 1,
                "diff_hash": "",
                "comments": [{
                    "id": "gh-99",
                    "timestamp": "2026-01-01T00:00:00Z",
                    "file": "src/b.rs",
                    "hunk_index": 0,
                    "line_start": 2,
                    "line_content": "b-new",
                    "comment": "parent",
                    "github_id": 99,
                    "author": "you",
                    "source": "github",
                    "side": "RIGHT"
                }]
            }),
        );
        er_engine::storage::with_process_test_storage_root(dir.path(), || -> anyhow::Result<()> {
            let github = FakeGitHub::default()
                .with_pr_diff_remote(Ok(RAW.to_string()))
                .with_post_reply(Ok(line_post(500)));
            let handle = posting_actor(RAW, github);
            let reply = handle.send_and_wait_with(
                Command::ReplyToGithubThread {
                    context: pr_context(&bucket),
                    parent_id: "gh-99".to_string(),
                    body: "agreed".to_string(),
                    client_id: Some("c-opt-2".to_string()),
                },
                Duration::from_secs(5),
            )?;
            assert_eq!(
                reply,
                Reply::AnnotationWritten {
                    id: "gh-500".to_string()
                }
            );

            let rows = github_rows(&bucket)?;
            let row = rows
                .comments
                .iter()
                .find(|c| c.id == "gh-500")
                .expect("the reply row");
            assert_eq!(row.in_reply_to.as_deref(), Some("gh-99"));
            assert_eq!(row.comment, "agreed");
            Ok(())
        })
    }

    #[test]
    fn promote_to_comment_writes_only_the_github_comment() -> anyhow::Result<()> {
        let (dir, bucket) = temp_bucket()?;
        er_engine::storage::with_process_test_storage_root(dir.path(), || -> anyhow::Result<()> {
            let handle = posting_actor(RAW, scripted_line_post(77));
            ask(&handle, &bucket, "q-opt-1", "promote me")?;
            handle.send_and_wait_with(
                Command::ReplyToThread {
                    context: context(&bucket, "/repo"),
                    parent_id: "q-opt-1".to_string(),
                    body: "seconded".to_string(),
                    client_id: Some("q-opt-2".to_string()),
                },
                Duration::from_secs(5),
            )?;
            let reply = handle.send_and_wait_with(
                Command::PromoteToComment {
                    context: pr_context(&bucket),
                    id: "q-opt-1".to_string(),
                    body: None,
                },
                Duration::from_secs(5),
            )?;
            assert_eq!(
                reply,
                Reply::CommentPromoted {
                    id: "gh-77".to_string()
                }
            );

            let comments = github_rows(&bucket)?.comments;
            assert_eq!(comments.len(), 1);
            let posted = &comments[0].comment;
            let quoted = posted.starts_with("promote me") && posted.contains("> seconded");
            assert!(quoted, "the reply is quoted: {posted:?}");
            let store = er_engine::store_registry::store();
            let scope = er_store::Scope::dir(&bucket)?;
            let notes_are_private =
                match er_engine::ai::annotation_store::load_notes(&store, &scope) {
                    Ok(er_store::Loaded::Found(notes)) => notes.value.notes.is_empty(),
                    _ => true,
                };
            assert!(notes_are_private, "promotion must not write a private note");
            let er_store::Loaded::Found(questions) =
                er_engine::ai::annotation_store::load_questions(&store, &scope)?
            else {
                anyhow::bail!("the questions file vanished");
            };
            assert!(
                questions.value.questions.is_empty(),
                "the source question and its reply are removed"
            );
            Ok(())
        })
    }

    #[test]
    fn promote_finding_to_comment_posts_and_removes_the_finding() -> anyhow::Result<()> {
        let (dir, bucket) = temp_bucket()?;
        er_engine::test_support::write_sidecar_json::<er_engine::ai::ErReview>(
            &dir.path().join("bucket"),
            serde_json::json!({
                "version": 1,
                "diff_hash": "diff-hash",
                "files": {
                    "src/b.rs": {
                        "risk": "low",
                        "findings": [{
                            "id": "f-1",
                            "severity": "low",
                            "title": "A defect",
                            "description": "desc",
                            "hunk_index": 0,
                            "line_start": 2
                        }]
                    }
                }
            }),
        );
        er_engine::storage::with_process_test_storage_root(dir.path(), || -> anyhow::Result<()> {
            let handle = posting_actor(RAW, scripted_line_post(88));
            let reply = handle.send_and_wait_with(
                Command::PromoteFindingToComment {
                    context: pr_context(&bucket),
                    finding_id: "f-1".to_string(),
                    body: None,
                },
                Duration::from_secs(5),
            )?;
            assert_eq!(
                reply,
                Reply::CommentPromoted {
                    id: "gh-88".to_string()
                }
            );

            let comments = github_rows(&bucket)?.comments;
            assert_eq!(comments.len(), 1);
            assert_eq!(comments[0].github_id, Some(88));
            let review = er_engine::ai::load_ai_state(&bucket, "diff-hash", None).review;
            let findings = review
                .and_then(|review| review.files.get("src/b.rs").map(|f| f.findings.len()))
                .unwrap_or(0);
            assert_eq!(findings, 0, "the finding is removed after the post");
            Ok(())
        })
    }

    #[cfg(unix)]
    #[test]
    fn a_promote_whose_source_removal_failed_adopts_the_post_on_retry() -> anyhow::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let (dir, private) = temp_bucket()?;
        let comments_dir = dir.path().join("comments");
        std::fs::create_dir_all(&comments_dir)?;
        let ctx = TabContext {
            comments_dir: comments_dir.to_string_lossy().into_owned(),
            ..pr_context(&private)
        };
        let now = er_engine::sync::chrono_now();
        // The comment the first attempt landed, as the retry lists it.
        let on_github: er_github::GitHubComment = serde_json::from_value(serde_json::json!({
            "id": 77, "body": "retry me", "path": "src/b.rs", "line": 2, "original_line": 2,
            "side": "RIGHT", "in_reply_to_id": null, "user": { "login": "you" },
            "created_at": now, "updated_at": now, "diff_hunk": null,
        }))?;
        let github = Arc::new(
            scripted_line_post(77)
                .with_viewer_login(Ok("you".to_string()))
                .with_pr_comments_remote(Ok(vec![on_github])),
        );
        er_engine::storage::with_process_test_storage_root(dir.path(), || -> anyhow::Result<()> {
            let handle = posting_actor_shared(RAW, &github);
            ask(&handle, &private, "q-retry-1", "retry me")?;
            let promote = |body: Option<&str>| Command::PromoteToComment {
                context: ctx.clone(),
                id: "q-retry-1".to_string(),
                body: body.map(str::to_string),
            };
            // A read-only bucket makes the source removal fail after the post.
            std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o555))?;
            let first = handle.send_and_wait_with(promote(None), Duration::from_secs(5));
            std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o755))?;
            assert!(first.is_err(), "the removal failure is reported: {first:?}");
            assert_eq!(calls_to(&github, "post_line_comment("), 1);

            // The dialog keeps an edit open after the error; sending it adopts
            // the earlier post and still removes the source.
            let edited = promote(Some("retry me, edited"));
            let retried = handle.send_and_wait_with(edited, Duration::from_secs(5))?;
            let id = "gh-77".to_string();
            assert_eq!(retried, Reply::CommentPromoted { id });
            assert_eq!(
                calls_to(&github, "post_line_comment("),
                1,
                "the retry posted again"
            );
            assert_eq!(
                calls_to(&github, "pr_comments_remote("),
                1,
                "the retry did not list"
            );
            let questions = er_engine::ai::load_ai_state(&private, &ctx.diff_hash, None).questions;
            assert_eq!(
                questions.map_or(0, |q| q.questions.len()),
                0,
                "the source stayed"
            );
            let rows = github_rows(&ctx.comments_dir)?.comments;
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].comment, "retry me", "the row holds what GitHub has");
            Ok(())
        })
    }
}

mod app_registry;
mod comment_patches;
mod github_sync;
mod pr_posts;
