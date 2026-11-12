//! A supervisor that never starts must fail the saved run.
//!
//! `start_arena_run` saves `run.json` as `Queued` before the supervisor thread
//! exists. If that thread cannot be created, nothing else moves the record;
//! this pins the launcher's failure path.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use er_engine::arena::{
    list_run_ids, load_run, start_arena_run, ArenaPaths, ArenaRegistry, ArenaRunInputs, ArenaScope,
    ArenaStartParams, ReviewerRef, RunStatus,
};
use er_engine::config::{AiModelConfig, AiProviderConfig, ErConfig};

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

fn fake_ref() -> ReviewerRef {
    ReviewerRef {
        provider_id: "fake".to_string(),
        model_id: "fake-1".to_string(),
        agent_kind: None,
    }
}

#[test]
fn a_failed_supervisor_start_marks_the_run_failed() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let repo_root = tmp.path().join("repo");
    let er_dir = tmp.path().join("er");
    fs::create_dir_all(&repo_root).expect("repo dir");
    fs::create_dir_all(&er_dir).expect("er dir");
    let script = tmp.path().join("fake-provider.sh");
    er_engine::test_support::write_executable_script(&script, "#!/bin/sh\nexit 1\n")
        .expect("write fake provider");

    let mut config = ErConfig::default();
    config.ai_hub.providers.insert(
        "fake".to_string(),
        AiProviderConfig {
            command: script.to_string_lossy().to_string(),
            args: vec!["{prompt}".to_string()],
            models: vec![AiModelConfig {
                id: "fake-1".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        },
    );
    let registry = Arc::new(ArenaRegistry::new(Arc::new(|| {})));
    registry.fail_next_start_for_test();
    let er_dir_str = er_dir.to_string_lossy().to_string();

    let error = start_arena_run(
        Arc::clone(&registry),
        config,
        ArenaRunInputs {
            repo_root: repo_root.to_string_lossy().to_string(),
            er_dir: er_dir_str.clone(),
            branch_ref: "feature/x".to_string(),
            base_branch: "main".to_string(),
            raw_diff: SAMPLE_DIFF.to_string(),
            reviewed_ref_note: None,
        },
        ArenaStartParams {
            title: Some("start failure".to_string()),
            reviewers: vec![fake_ref()],
            scope: ArenaScope::Branch,
            files: None,
            rounds: None,
            arbiter: Some(fake_ref()),
            confirm: true,
            agent_kind: None,
            effort: None,
        },
        None,
    )
    .expect_err("the forced start failure must surface");
    assert!(
        format!("{error:#}").contains("start the arena supervisor"),
        "{error:#}"
    );

    let ids = list_run_ids(Path::new(&er_dir_str)).expect("run ids");
    assert_eq!(ids.len(), 1, "{ids:?}");
    let run = load_run(&ArenaPaths::for_run(Path::new(&er_dir_str), &ids[0])).expect("run.json");
    assert_eq!(
        run.status,
        RunStatus::Failed,
        "the failed start left run.json as {:?}",
        run.status
    );
}
