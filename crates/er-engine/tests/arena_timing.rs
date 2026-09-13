//! Phase 0 harness: arena wall-clock, measured end to end through the real
//! supervisor rather than inferred from code structure.
//!
//! Drives [`start_arena_run`] with a fake provider — a shell script that
//! sleeps for a fixed time and then echoes the matching round fixture — so the
//! numbers are deterministic and no model is ever called. The provider's sleep
//! stands in for model latency, which is where an agent actually spends its
//! time (measured separately: ~6% CPU utilisation).
//!
//! Run with output visible:
//!
//! ```text
//! ER_AGENT_TIMING=1 cargo test -p er-engine --test arena_timing -- --nocapture
//! ```
//!
//! The test sets the env var itself, so `--nocapture` alone is enough. It is
//! the only test in this binary, which keeps the process-global gate and
//! `ER_FAKE_ARENA_DIR` free of cross-test races.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use er_engine::arena::{
    load_run, start_arena_run, ArenaPaths, ArenaRegistry, ArenaScope, ArenaStartParams, ReviewerRef,
    RunStatus,
};
use er_engine::config::{AiModelConfig, AiProviderConfig, ErConfig};

/// Simulated reviewer latency, in seconds, per round.
const REVIEWER_SECS: u64 = 2;
/// Simulated arbiter latency, in seconds.
const ARBITER_SECS: u64 = 1;
/// Three reviewers; with a single round the run must still finish in about one
/// reviewer's latency, which is what proves the round is concurrent.
const REVIEWERS: usize = 3;

const SAMPLE_DIFF: &str = "\
diff --git a/src/lib.rs b/src/lib.rs
index 0000000..1111111 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,2 +1,3 @@
 fn a() {}
+fn b() {}
 fn c() {}
";

/// Write a fake provider that sleeps, then echoes the fixture for whichever
/// round its prompt belongs to. The prompt arrives as an argv, so the script
/// can tell the rounds apart by their text.
fn write_fake_provider(dir: &Path) -> PathBuf {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/arena/fake")
        .canonicalize()
        .expect("arena fixtures are present");
    let script = dir.join("fake-provider.sh");
    fs::write(
        &script,
        format!(
            r#"#!/bin/sh
# Phase 0 fake provider: sleep, then echo the fixture for this round.
case "$*" in
  *"arena arbiter"*) sleep {arb}; exec cat "{f}/round3.json" ;;
  *"cross-check"*)   sleep {rev}; exec cat "{f}/round2.json" ;;
  *)                 sleep {rev}; exec cat "{f}/round1.json" ;;
esac
"#,
            arb = ARBITER_SECS,
            rev = REVIEWER_SECS,
            f = fixtures.display()
        ),
    )
    .expect("write fake provider");
    let mut perms = fs::metadata(&script).expect("stat fake provider").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script, perms).expect("chmod fake provider");
    script
}

fn config_with_provider(script: &Path) -> ErConfig {
    let mut config = ErConfig::default();
    config.ai_hub.providers.insert(
        "fake".to_string(),
        AiProviderConfig {
            command: script.to_string_lossy().to_string(),
            args: vec!["{prompt}".to_string()],
            models: vec![AiModelConfig {
                id: "fake-1".to_string(),
                label: Some("Fake 1".to_string()),
                cost_per_1k_in: Some(0.0),
                cost_per_1k_out: Some(0.0),
                ..Default::default()
            }],
            ..Default::default()
        },
    );
    config
}

fn fake_ref() -> ReviewerRef {
    ReviewerRef {
        provider_id: "fake".to_string(),
        model_id: "fake-1".to_string(),
        agent_kind: None,
    }
}

/// Run one arena to completion and return how long it took.
fn run_arena(label: &str, reviewers: usize, rounds: Option<u8>, tmp: &Path) -> Duration {
    let repo_root = tmp.join(format!("repo-{label}"));
    let er_dir = tmp.join(format!("er-{label}"));
    fs::create_dir_all(&repo_root).expect("repo dir");
    fs::create_dir_all(&er_dir).expect("er dir");

    let script = write_fake_provider(tmp);
    let config = config_with_provider(&script);
    let registry = Arc::new(ArenaRegistry::new(Arc::new(|| {})));
    let er_dir_str = er_dir.to_string_lossy().to_string();

    let params = ArenaStartParams {
        title: Some(label.to_string()),
        reviewers: (0..reviewers).map(|_| fake_ref()).collect(),
        scope: ArenaScope::Branch,
        files: None,
        rounds,
        arbiter: Some(fake_ref()),
        confirm: true,
        agent_kind: None,
        effort: None,
    };

    eprintln!("--- arena {label}: {reviewers} reviewers, rounds={rounds:?} ---");
    let started = Instant::now();
    let run_id = start_arena_run(
        Arc::clone(&registry),
        config,
        repo_root.to_string_lossy().to_string(),
        er_dir_str.clone(),
        "feature/x".to_string(),
        "main".to_string(),
        SAMPLE_DIFF.to_string(),
        params,
    )
    .expect("arena starts");

    // The supervisor owns its own thread; wait for it to record a terminal
    // status rather than guessing a sleep duration.
    let paths = ArenaPaths::for_run(Path::new(&er_dir_str), &run_id);
    let deadline = started + Duration::from_secs(180);
    loop {
        let run = load_run(&paths).expect("run is readable");
        if matches!(
            run.status,
            RunStatus::Complete | RunStatus::Failed | RunStatus::Cancelled
        ) {
            eprintln!("--- arena {label}: {:?} ---", run.status);
            break;
        }
        assert!(
            Instant::now() < deadline,
            "{label}: arena did not reach a terminal status within 180s"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    started.elapsed()
}

/// The claim under test: arena wall-clock is set by the round count, not by
/// the concurrency cap. Every reviewer sleeps the same fixed time, so any
/// difference between the two runs below is the cost of one extra round —
/// and if the cap were the binding constraint, the single-round run would
/// still serialise its three reviewers.
#[test]
fn arena_wall_clock_tracks_round_count_not_the_cap() {
    // Must be set before anything calls into agent_timing.
    std::env::set_var("ER_AGENT_TIMING", "1");
    let tmp = tempfile::tempdir().expect("tempdir");

    let one_round = run_arena("one-round", REVIEWERS, Some(1), tmp.path());
    let two_rounds = run_arena("two-rounds", REVIEWERS, Some(2), tmp.path());
    // More reviewers than the default cap: the surplus has to queue for a
    // slot, so this round costs two reviewer latencies instead of one.
    let above_cap = run_arena("above-cap", REVIEWERS + 2, Some(1), tmp.path());

    er_engine::agent_timing::emit_slot_summary("arena_harness");

    let per_round = Duration::from_secs(REVIEWER_SECS);
    println!(
        "one_round={one_round:?} two_rounds={two_rounds:?} above_cap={above_cap:?} \
         (reviewer={}s, arbiter={}s, {REVIEWERS} reviewers)",
        REVIEWER_SECS, ARBITER_SECS
    );

    // Round 1 fans out across reviewers, so a single round costs about one
    // reviewer's latency. Serialised, three reviewers plus spawn overhead
    // would land near 3x that.
    assert!(
        one_round < per_round * 3,
        "one round with {REVIEWERS} reviewers took {one_round:?}; \
         reviewers in a round should run concurrently, not serialised"
    );
    // A second round adds roughly one more reviewer latency (plus the
    // arbiter, which a single-round run skips).
    assert!(
        two_rounds > one_round,
        "adding a round should cost wall-clock: one={one_round:?} two={two_rounds:?}"
    );
    // Above the cap the surplus queues for a slot, so the same single round
    // now costs about two reviewer latencies. This is the only case where the
    // cap, rather than the round count, sets the round's duration.
    assert!(
        above_cap > one_round + per_round / 2,
        "with {} reviewers over a cap of 3 the round should need a second wave: \
         one_round={one_round:?} above_cap={above_cap:?}",
        REVIEWERS + 2
    );
}
