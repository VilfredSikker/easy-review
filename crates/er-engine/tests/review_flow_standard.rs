//! End-to-end Quick flow coverage with a fake producer CLI and fake arbiter.
#![expect(
    clippy::unwrap_used,
    reason = "integration fixture setup should fail immediately"
)]

use er_engine::ai::finding_key;
use er_engine::ai::review_flow::FlowState;
use er_engine::app::App;
use er_engine::arena::{load_run, ArenaPaths, RunStatus, Verdict};
use er_engine::config::{
    AiHubConfig, AiModelConfig, AiProviderConfig, ReviewModelGroups, ReviewModelRef,
};
use er_engine::ErRoot;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

static FLOW_SLOT_LOCK: Mutex<()> = Mutex::new(());

struct SlotGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
}
impl Drop for SlotGuard {
    fn drop(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while er_engine::agent_slots::active_count() > 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}

struct TestDir(PathBuf);
impl TestDir {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "er-review-flow-standard-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct RestoreFakeArenaEnv(Option<std::ffi::OsString>);
impl Drop for RestoreFakeArenaEnv {
    fn drop(&mut self) {
        if let Some(previous) = self.0.take() {
            std::env::set_var("ER_FAKE_ARENA_DIR", previous);
        } else {
            std::env::remove_var("ER_FAKE_ARENA_DIR");
        }
    }
}

#[test]
fn quick_flow_pins_filtered_hash_and_leaves_review_json_unchanged() {
    let _slot_guard = SlotGuard {
        _lock: FLOW_SLOT_LOCK.lock().unwrap(),
    };
    let dir = TestDir::new();
    let repo = dir.0.join("repo");
    let bucket = dir.0.join("bucket");
    let fake_arena = dir.0.join("fake-arena");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::create_dir_all(&bucket).unwrap();
    std::fs::create_dir_all(&fake_arena).unwrap();

    let script = format!(
        "case \"$1\" in\n\
         *triage.json*) hash=$(cat '{bucket}/.diff-tmp.sha256'); printf '{{\"version\":1,\"diff_hash\":\"%s\",\"verdict\":{{\"primary\":\"general\"}}}}' \"$hash\" > '{bucket}/triage.json' ;;\n\
         *review.json*) hash=$(cat '{bucket}/.diff-tmp.sha256'); printf '{{\"version\":1,\"diff_hash\":\"%s\",\"files\":{{\"src/lib.rs\":{{\"risk\":\"high\",\"findings\":[{{\"id\":\"fake-1\",\"severity\":\"high\",\"title\":\"unchecked input\",\"description\":\"The value reaches a command.\",\"line_start\":1,\"confidence\":\"confirmed\"}}]}}}}}}' \"$hash\" > '{bucket}/review.json'; cp '{bucket}/review.json' '{bucket}/review.before-arbiter' ;;\n\
         *) echo \"unexpected review-flow prompt\" >&2; exit 2 ;;\n\
         esac",
        bucket = bucket.display(),
    );
    let provider = AiProviderConfig {
        command: "/bin/sh".into(),
        args: vec!["-c".into(), script, "fake-flow".into(), "{prompt}".into()],
        models: vec![AiModelConfig {
            id: "fake-model".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    let mut app = App::new_for_test(Vec::new());
    app.config.ai_hub = AiHubConfig {
        default_provider: Some("fake".into()),
        providers: [("fake".into(), provider)].into(),
        review_models: ReviewModelGroups {
            fast: vec![ReviewModelRef {
                provider_id: "fake".into(),
                model_id: "fake-model".into(),
            }],
            standard: vec![ReviewModelRef {
                provider_id: "fake".into(),
                model_id: "fake-model".into(),
            }],
            advanced: vec![ReviewModelRef {
                provider_id: "fake".into(),
                model_id: "fake-model".into(),
            }],
        },
        max_concurrent_reviews: 16,
        max_concurrent_arena_reviews: 16,
        max_concurrent_agents: 16,
        ..Default::default()
    };
    let tab = app.tab_mut();
    tab.repo_root = repo.to_string_lossy().into_owned();
    tab.er_root = ErRoot::Managed {
        agent_dir: bucket.to_string_lossy().into_owned(),
        session_dir: bucket.to_string_lossy().into_owned(),
    };
    tab.set_raw_diff_for_test(concat!(
        "diff --git a/src/lib.rs b/src/lib.rs\n",
        "index 1111111..2222222 100644\n",
        "--- a/src/lib.rs\n",
        "+++ b/src/lib.rs\n",
        "@@ -1 +1 @@\n",
        "-old\n",
        "+new\n",
    ));

    let hash = er_engine::ai::compute_diff_hash(
        "diff --git a/src/lib.rs b/src/lib.rs\nindex 1111111..2222222 100644\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-old\n+new\n",
    );
    let finding_id = finding_key("src/lib.rs", Some(1), "unchecked input");
    let arbiter_reply = json!({
        "verdicts": [{
            "finding_id": finding_id,
            "verdict": "kept",
            "confidence": 0.95,
            "rationale": "The finding is present in the prepared diff."
        }]
    });
    for round in 1..=3 {
        std::fs::write(
            fake_arena.join(format!("round{round}.json")),
            serde_json::to_vec(&arbiter_reply).unwrap(),
        )
        .unwrap();
    }
    let before_calls = er_engine::arena::fake_arena_call_count();
    let previous_env = std::env::var_os("ER_FAKE_ARENA_DIR");
    std::env::set_var("ER_FAKE_ARENA_DIR", &fake_arena);
    let _restore_env = RestoreFakeArenaEnv(previous_env);

    app.start_review_flow("branch").unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    let flow = loop {
        app.poll_background_tasks();
        let flow = app.review_flow_for(&bucket.to_string_lossy()).unwrap();
        if matches!(
            flow.state,
            FlowState::Done { .. } | FlowState::Failed { .. }
        ) {
            break flow.clone();
        }
        assert!(
            Instant::now() < deadline,
            "Quick review flow did not finish"
        );
        std::thread::sleep(Duration::from_millis(20));
    };

    let pinned_hash = std::fs::read_to_string(bucket.join(".diff-tmp.sha256")).unwrap();
    assert_eq!(pinned_hash.trim(), hash);
    assert_eq!(flow.diff_hash, hash);
    assert!(matches!(
        flow.state,
        FlowState::Done {
            kept: 1,
            dropped: 0,
            awaiting_accept: 0
        }
    ));
    let run_id = flow.arena_run_id.as_deref().unwrap();
    let run = load_run(&ArenaPaths::for_run(&bucket, run_id)).unwrap();
    assert_eq!(
        run.diff_hash, hash,
        "seeded run uses the pinned filtered hash"
    );
    assert_eq!(run.status, RunStatus::Complete);
    assert_eq!(run.config.reviewers.len(), 1);
    assert_eq!(run.config.reviewers[0].provider_id, "fake");
    assert_eq!(
        run.config.reviewers[0].agent_kind.as_deref(),
        Some("general")
    );
    assert_eq!(run.findings[0].verdict, Verdict::Kept);
    assert_eq!(flow.successful_finder_providers, ["fake"]);
    assert_eq!(flow.successful_arbiter_provider.as_deref(), Some("fake"));
    assert_eq!(flow.providers_used, ["fake"]);
    let arbiter: er_engine::ai::ArbiterReview =
        serde_json::from_slice(&std::fs::read(bucket.join("arbiter.json")).unwrap()).unwrap();
    assert_eq!(
        arbiter.diff_hash, hash,
        "arbiter sidecar is pinned to the same diff"
    );
    assert_eq!(
        er_engine::arena::fake_arena_call_count() - before_calls,
        1,
        "the seeded flow makes one arbiter call"
    );
    let review_bytes = std::fs::read(bucket.join("review.before-arbiter")).unwrap();
    assert_eq!(
        std::fs::read(bucket.join("review.json")).unwrap(),
        review_bytes,
        "the seeded verifier leaves producer review.json byte-identical"
    );
}
