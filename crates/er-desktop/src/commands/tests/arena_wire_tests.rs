//! The arena on `er_send` and `er_fetch`: a command names the tab the page
//! painted and runs there with the App locked, a read answers from the tab's
//! buckets, and the live runs reach the page through `jobs`.

use std::sync::Arc;

use er_engine::TabKey;
use er_protocol::{
    ArenaPlan, ArenaReviewer, ArenaRunJob, ArenaRunStatus, ArenaRuns, Command, Resource,
    ResourceKey,
};

use super::arena_resource_tests::{
    arena_state, bucket, local_ports, open, record, wait_until, HoldingSink,
};
use super::diff_command_tests::send;
use super::diff_resource_tests::with_storage;
use crate::commands::AppState;
use crate::resources::Watched;

fn fake() -> ArenaReviewer {
    ArenaReviewer {
        provider_id: "fake".into(),
        model_id: "fake-1".into(),
        agent_kind: None,
    }
}

fn plan(files: Option<Vec<String>>) -> ArenaPlan {
    ArenaPlan {
        scope: "branch".into(),
        files,
        rounds: Some(1),
        arbiter: Some(fake()),
        reviewers: vec![fake(), fake()],
        groups: Vec::new(),
    }
}

fn start(tab: TabKey) -> Command {
    Command::ArenaStart {
        tab: er_service::wire_tab_key(tab),
        plan: Box::new(plan(None)),
        title: None,
        confirm: true,
        agent_kind: None,
        effort: None,
    }
}

fn started(state: &AppState, tab: TabKey) -> String {
    let ack = send(state, "arena start", start(tab)).unwrap_or_else(|e| panic!("{e:?}"));
    ack.created
        .unwrap_or_else(|| panic!("the start named no run"))
}

fn jobs(state: &AppState) -> (u64, Vec<ArenaRunJob>) {
    match crate::resources::fetch(&state.desktop, ResourceKey::Jobs) {
        Ok(fetched) => match fetched.value {
            Resource::Jobs(jobs) => (fetched.version.0, jobs.arena_runs),
            other => panic!("not jobs: {other:?}"),
        },
        Err(e) => panic!("{e:?}"),
    }
}

fn ended(job: &ArenaRunJob) -> bool {
    matches!(
        job.status,
        ArenaRunStatus::Complete | ArenaRunStatus::Failed
    )
}

/// The registry moved to B while the page shows A: an `er_send` start keyed
/// to A, sent with the App locked, files its run in A's bucket and none in
/// B's, and `jobs` lists it there.
#[test]
fn an_er_send_start_runs_on_the_tab_it_names_with_the_app_locked() {
    with_storage(|dir| {
        let sink = Arc::new(HoldingSink::default());
        let state = arena_state(dir, local_ports(), Arc::clone(&sink));
        let painted = open(&state, dir.path(), "a");
        let active = open(&state, dir.path(), "b");
        let (painted_bucket, active_bucket) = (bucket(&state, &painted), bucket(&state, &active));
        assert_ne!(
            painted_bucket, active_bucket,
            "the fixture's two buckets are one"
        );

        let run_id = started(&state, painted.0);

        assert!(
            record(&painted_bucket, &run_id).is_some(),
            "no run in the named tab's bucket"
        );
        assert!(
            record(&active_bucket, &run_id).is_none(),
            "the run went to the active tab"
        );
        let (_, live) = jobs(&state);
        assert!(
            live.iter()
                .any(|run| run.run_id == run_id && run.er_dir == painted_bucket),
            "jobs does not list the run in its bucket: {live:?}"
        );
        sink.release();
    });
}

/// A live run's start and its end each move `jobs` and the relay names it;
/// a diff refresh, which publishes the tab's actor again, moves neither.
#[test]
fn a_live_run_moves_jobs_and_a_diff_publish_does_not() {
    with_storage(|dir| {
        let sink = Arc::new(HoldingSink::default());
        let state = arena_state(dir, local_ports(), Arc::clone(&sink));
        let tab = open(&state, dir.path(), "a");
        let (before, _) = jobs(&state);
        let mut last = crate::resources::watched(&state.desktop);

        let run_id = started(&state, tab.0);
        wait_until("the run to show in jobs", || {
            jobs(&state).1.iter().any(|run| run.run_id == run_id)
        });
        let (live, _) = jobs(&state);
        assert!(live > before, "jobs stayed at {before}");
        let now = crate::resources::watched(&state.desktop);
        let moved = crate::resources::relay::moved(&mut last, now);
        assert!(
            moved.iter().any(|(key, _)| *key == Watched::Jobs),
            "{moved:?}"
        );

        sink.release();
        wait_until("the run to end in jobs", || {
            jobs(&state)
                .1
                .iter()
                .any(|run| run.run_id == run_id && ended(run))
        });
        let (over, _) = jobs(&state);
        assert!(over > live, "the run's end left jobs at {live}");

        let refresh = Command::RefreshDiff {
            tab: er_service::wire_tab_key(tab.0),
            sync: false,
        };
        let refreshed = send(&state, "refresh", refresh);
        assert!(refreshed.is_ok(), "{refreshed:?}");
        assert_eq!(jobs(&state).0, over, "a diff publish moved jobs");
    });
}

/// One reviewer's end moves the run's progress while its status holds: the
/// progress alone moves `jobs` and the relay names it.
#[test]
fn a_progress_tick_moves_jobs_while_the_status_holds() {
    with_storage(|dir| {
        let sink = Arc::new(HoldingSink::default());
        let state = arena_state(dir, local_ports(), Arc::clone(&sink));
        let tab = open(&state, dir.path(), "a");
        let run_id = started(&state, tab.0);
        let live = |state: &AppState| jobs(state).1.into_iter().find(|run| run.run_id == run_id);
        wait_until("both reviewers to be held", || sink.held() == 2);
        wait_until("the reviewers to show as thinking", || {
            live(&state).is_some_and(|run| !run.progress.thinking.is_empty())
        });
        let before = live(&state).unwrap_or_else(|| panic!("the run left jobs"));
        let (version, _) = jobs(&state);
        let mut last = crate::resources::watched(&state.desktop);

        sink.end_one();
        wait_until("the reviewer's end to show in jobs", || {
            live(&state).is_some_and(|run| run.progress != before.progress)
        });

        let after = live(&state).unwrap_or_else(|| panic!("the run left jobs"));
        assert_eq!(after.status, before.status, "the status moved too");
        let (moved_to, _) = jobs(&state);
        assert!(moved_to > version, "a progress tick left jobs at {version}");
        let now = crate::resources::watched(&state.desktop);
        let moved = crate::resources::relay::moved(&mut last, now);
        assert!(
            moved.iter().any(|(key, _)| *key == Watched::Jobs),
            "{moved:?}"
        );
        sink.release();
    });
}

/// Two lists that differ only in a run's progress move the count; the same
/// list read again does not.
#[test]
fn live_jobs_counts_a_change_of_progress_alone() {
    let run = |done: &[&str]| ArenaRunJob {
        run_id: "run-1".into(),
        er_dir: "/bucket".into(),
        status: ArenaRunStatus::Running { round: 1 },
        progress: er_protocol::ArenaRunProgress {
            round: 1,
            total_rounds: 2,
            phase: String::new(),
            thinking: vec!["b".into()],
            done: done.iter().map(|id| (*id).to_string()).collect(),
        },
    };
    let live = crate::state::LiveJobs::default();
    let first = live.observe(&[run(&[])]);
    let ticked = live.observe(&[run(&["a"])]);
    assert!(ticked > first, "a progress tick left the count at {first}");
    assert_eq!(
        live.observe(&[run(&["a"])]),
        ticked,
        "the same runs moved the count"
    );
}

/// `arena_estimate` prices the plan on the files it names, read with the App
/// locked and never through the actor's queue.
#[test]
fn an_estimate_prices_the_files_its_plan_names() {
    with_storage(|dir| {
        let sink = Arc::new(HoldingSink::default());
        let state = arena_state(dir, local_ports(), sink);
        let tab = open(&state, dir.path(), "a");
        let files = vec!["src/f0.rs".to_string()];
        let key = ResourceKey::ArenaEstimate {
            tab: er_service::wire_tab_key(tab.0),
            plan: plan(Some(files.clone())),
        };

        let fetched = super::while_the_app_is_locked(&state, "estimate", move |state| {
            crate::resources::fetch(&state.desktop, key).map(|f| f.value)
        });

        let Ok(Resource::ArenaEstimate(estimate)) = fetched else {
            panic!("not an estimate: {fetched:?}");
        };
        let raw = super::diff_resource_tests::raw_diff();
        let named = er_engine::git::filter_raw_diff_by_paths(&raw, &files);
        assert!(named.len() < raw.len(), "the fixture names every file");
        assert_eq!(estimate.diff_bytes, named.len() as u64);
    });
}

/// `arena_run`'s text is the run read with no App, serialised whole.
#[test]
fn an_arena_run_reads_as_its_snapshot_text() {
    with_storage(|dir| {
        let sink = Arc::new(HoldingSink::default());
        let state = arena_state(dir, local_ports(), Arc::clone(&sink));
        let tab = open(&state, dir.path(), "a");
        let run_id = started(&state, tab.0);
        sink.release();
        wait_until("the run's end to reach jobs", || {
            jobs(&state)
                .1
                .iter()
                .any(|run| run.run_id == run_id && ended(run))
        });
        let wire = er_service::wire_tab_key(tab.0);
        let key = ResourceKey::ArenaRun {
            tab: wire,
            run_id: run_id.clone(),
        };

        let fetched = crate::resources::fetch(&state.desktop, key).map(|f| f.value);

        let Ok(Resource::ArenaRun(run)) = fetched else {
            panic!("not a run: {fetched:?}");
        };
        let arena_tab = crate::arena_service::tab(&state.desktop, Some(wire));
        let arena_tab = arena_tab.unwrap_or_else(|e| panic!("{e:?}"));
        let read = crate::arena_service::get(&state.desktop, &arena_tab, &run_id);
        let read = read.unwrap_or_else(|e| panic!("{e:?}"));
        let text = serde_json::to_string(&read).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(run.json, text);
    });
}

/// A listed tab nothing has loaded answers an empty run list at version 0
/// and is sent a load, so the page reads again once the content publishes.
#[test]
fn an_unloaded_tab_answers_no_runs_and_loads() {
    with_storage(|dir| {
        let sink = Arc::new(HoldingSink::default());
        let state = arena_state(dir, local_ports(), sink);
        let (key, tab) = super::diff_resource_tests::listed(&state, dir, "a");
        assert!(
            state.service.content(key, &tab).is_none(),
            "the fixture loaded the tab"
        );
        let wire = er_service::wire_tab_key(key);

        let fetched = crate::resources::fetch(&state.desktop, ResourceKey::ArenaRuns { tab: wire });

        let fetched = fetched.unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(fetched.version.0, 0);
        assert_eq!(fetched.value, Resource::ArenaRuns(ArenaRuns::default()));
        wait_until("the load the read posted", || {
            state.service.content(key, &tab).is_some()
        });
    });
}

/// `arena_runs` lists what the flag-off App lists for the same bucket and
/// branch, newest first.
#[test]
fn the_run_list_matches_the_flag_off_list() {
    with_storage(|dir| {
        let sink = Arc::new(HoldingSink::default());
        let state = arena_state(dir, local_ports(), Arc::clone(&sink));
        sink.release();
        let tab = open(&state, dir.path(), "a");
        let mut ids = Vec::new();
        for _ in 0..2 {
            let run_id = started(&state, tab.0);
            wait_until("the run to end and leave its bucket", || {
                jobs(&state)
                    .1
                    .iter()
                    .any(|run| run.run_id == run_id && ended(run))
                    && er_engine::arena::check_bucket_admission(&bucket(&state, &tab)).is_ok()
            });
            ids.push(run_id);
        }
        let wire = er_service::wire_tab_key(tab.0);

        let fetched = crate::resources::fetch(&state.desktop, ResourceKey::ArenaRuns { tab: wire });

        let Ok(Resource::ArenaRuns(listed)) = fetched.map(|f| f.value) else {
            panic!("not a run list");
        };
        let mut app = er_engine::app::App::new_for_test(Vec::new());
        app.tab_mut().er_root = er_engine::paths::ErRoot::managed(bucket(&state, &tab));
        let flag_off = app.arena_list_summaries(Some("feature"));
        let flag_off = flag_off.unwrap_or_else(|e| panic!("{e}"));
        let rows = |runs: Vec<(String, String, u32)>| runs;
        assert_eq!(flag_off.len(), 2, "the fixture lists {flag_off:?}");
        assert_eq!(
            rows(
                listed
                    .runs
                    .iter()
                    .map(|r| (r.id.clone(), r.created_at.clone(), r.reviewer_count))
                    .collect()
            ),
            rows(
                flag_off
                    .iter()
                    .map(|s| (s.id.clone(), s.created_at.clone(), s.reviewer_count as u32))
                    .collect()
            ),
        );
    });
}

/// The wire mapping names the App nowhere outside its tests.
#[test]
fn the_arena_wire_names_no_app() {
    let source = include_str!("../../resources/arena.rs");
    let body = source.split("#[cfg(test)]").next().unwrap_or(source);
    for app in ["er_engine::app", "AppState", ".app.lock(", "state.app"] {
        assert!(!body.contains(app), "resources/arena.rs names {app}");
    }
}
