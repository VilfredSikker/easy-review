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

#[test]
#[ignore = "latency harness: run with --ignored --nocapture and paste the numbers in the PR; never a CI assertion"]
fn the_actor_hop_stays_within_the_latency_budget() -> anyhow::Result<()> {
    const RUNS: usize = 100;
    let deps = WorkspaceDeps::new(Ports::with_github(github(Ok("OPEN"))));
    let target = remote_pr();

    let direct = median_duration(RUNS, || Ok(resolve_target(&deps, &target).map(|_| ())?))?;

    let handle = spawn(deps);
    handle.send_and_wait_with(open_remote(), Duration::from_secs(5))?;
    let actor = median_duration(RUNS, || {
        Ok(handle
            .send_and_wait_with(open_remote(), Duration::from_secs(5))
            .map(|_| ())?)
    })?;

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
