//! End-to-end cover for the seeded arbiter path (`.scratch/experts-arbiter`,
//! slice 2): expert sidecars in, one arbiter call, verdicts applied, and
//! `review.json` left exactly as it was.
//!
//! One test in the file on purpose — `ER_FAKE_ARENA_DIR` is process-wide, and a
//! second test here would race this one for it.

use er_engine::ai::finding_key;
use er_engine::arena::{
    load_run, start_seeded_run, ArenaPaths, ArenaRegistry, ArenaRunKind, RunStatus,
    SeededStartParams, Verdict,
};
use serde_json::json;
use std::sync::Arc;

const HASH: &str = "diff-hash";

fn write_expert(er_dir: &std::path::Path, expert_id: &str, path: &str, title: &str, line: usize) {
    let dir = er_dir.join("experts");
    std::fs::create_dir_all(&dir).unwrap();
    let sidecar = json!({
        "version": 1,
        "expert_id": expert_id,
        "diff_hash": HASH,
        "files": {
            path: {
                "findings": [{
                    "id": format!("{expert_id}-1"),
                    "severity": "high",
                    "title": title,
                    "description": format!("{title} — body"),
                    "line_start": line,
                    "confidence": "confirmed",
                }]
            }
        }
    });
    std::fs::write(
        dir.join(format!("{expert_id}.json")),
        serde_json::to_string(&sidecar).unwrap(),
    )
    .unwrap();
}

#[test]
fn a_seeded_run_rules_on_expert_findings_without_touching_review_json() {
    let tmp = tempfile::tempdir().unwrap();
    let er_dir = tmp.path().join("er");
    std::fs::create_dir_all(&er_dir).unwrap();

    // Two experts raising the same issue: one row after the dedupe.
    write_expert(&er_dir, "security", "src/a.rs", "unchecked user input", 10);
    write_expert(
        &er_dir,
        "reliability",
        "src/a.rs",
        "unchecked user input",
        10,
    );

    // A review this pass must not rewrite. ADR 0001: verdicts go to their own
    // sidecar and merge at load, so `review.json` keeps its three writers.
    let review_path = er_dir.join("review.json");
    let review = json!({
        "version": 1,
        "diff_hash": HASH,
        "files": {
            "src/a.rs": {
                "risk": "high",
                "findings": [{
                    "id": "sec-1",
                    "severity": "high",
                    "title": "unchecked user input",
                    "line_start": 10,
                    "lens": "security",
                    "raised_by": ["security"],
                    "confidence": "confirmed"
                }]
            }
        }
    });
    let before = serde_json::to_string_pretty(&review).unwrap();
    std::fs::write(&review_path, &before).unwrap();

    // The arbiter's answer, keyed by the id the dedupe will compute. The fake
    // harness reads `round<N>.json` off a process-wide counter, so every round
    // gets the same content rather than guessing where the counter lands.
    // The key carries the anchor line, so it has to match where the experts
    // anchored the finding.
    let id = finding_key("src/a.rs", Some(10), "unchecked user input");
    let fake = tmp.path().join("fake");
    std::fs::create_dir_all(&fake).unwrap();
    let arbiter_output = json!({
        "verdicts": [{
            "finding_id": id,
            "verdict": "kept",
            "confidence": 0.92,
            "rationale": "Reproduced against the annotated diff."
        }]
    });
    for round in 1..=3 {
        std::fs::write(
            fake.join(format!("round{round}.json")),
            serde_json::to_string(&arbiter_output).unwrap(),
        )
        .unwrap();
    }

    std::env::set_var("ER_FAKE_ARENA_DIR", &fake);
    let calls_before = er_engine::arena::fake_arena_call_count();

    let mut config = er_engine::config::ErConfig::default();
    er_engine::config::supplement_ai_hub(&mut config.ai_hub);
    let registry = Arc::new(ArenaRegistry::new(Arc::new(|| {})));

    let started = start_seeded_run(
        Arc::clone(&registry),
        config,
        tmp.path().to_string_lossy().to_string(),
        SeededStartParams {
            er_dir: er_dir.to_string_lossy().to_string(),
            branch_ref: "feature".to_string(),
            base_branch: "main".to_string(),
            scope: er_engine::arena::ArenaScope::Branch,
            diff_hash: HASH.to_string(),
            review_hash: String::new(),
            raw_diff: concat!(
                "diff --git a/src/a.rs b/src/a.rs\n",
                "--- a/src/a.rs\n",
                "+++ b/src/a.rs\n",
                "@@ -1,3 +1,4 @@\n",
                " fn handle(input: &str) {\n",
                "+    let key = format!(\"cache:{input}\");\n",
                "     store.get(&key)\n",
                " }\n",
            )
            .to_string(),
            arbiter: None,
        },
    );

    let run_id = match started {
        Ok(Some(run_id)) => run_id,
        Ok(None) => {
            std::env::remove_var("ER_FAKE_ARENA_DIR");
            panic!("two expert findings were written, so there is something to validate");
        }
        Err(e) => {
            std::env::remove_var("ER_FAKE_ARENA_DIR");
            panic!("start_seeded_run failed: {e:#}");
        }
    };

    let paths = ArenaPaths::for_run(&er_dir, &run_id);

    // Wait for the worker rather than racing it. The handle's join is internal,
    // and the run record is what the assertions care about anyway.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let run = loop {
        let run = load_run(&paths).expect("the seeded run was saved");
        if run.status == RunStatus::Complete || matches!(run.status, RunStatus::Failed) {
            break run;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the seeded run never finished"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    std::env::remove_var("ER_FAKE_ARENA_DIR");

    assert_eq!(run.config.run_kind, ArenaRunKind::Seeded);
    assert_eq!(run.status, RunStatus::Complete, "the arbiter was reached");
    assert_eq!(
        run.findings.len(),
        1,
        "the two experts raised one issue, so one finding"
    );
    assert_eq!(run.findings[0].raised_by, vec!["reliability", "security"]);
    // The path's whole economic argument: one arbiter call however many experts
    // contributed. Two experts ran here, and the count must not follow them.
    assert_eq!(
        er_engine::arena::fake_arena_call_count() - calls_before,
        1,
        "exactly one arbiter call, not one per expert"
    );
    assert_eq!(
        run.findings[0].verdict,
        Verdict::Kept,
        "the arbiter's ruling landed on the finding"
    );
    assert!(
        run.findings[0].confidence > 0.9,
        "the grade is the arbiter's, not the expert's self-report"
    );

    assert_eq!(
        std::fs::read_to_string(&review_path).unwrap(),
        before,
        "review.json is byte-identical: the seeded pass writes its own sidecar only"
    );

    // The verdicts landed in their own sidecar, and the review picks them up on
    // the next load — the overlay is what makes the grade visible.
    let arbiter_path = er_dir.join("arbiter.json");
    assert!(arbiter_path.is_file(), "arbiter.json was written");

    let state = er_engine::ai::load_ai_state(&er_dir.to_string_lossy(), HASH, None);
    let graded = &state.review.expect("review loads").files["src/a.rs"].findings[0];
    assert_eq!(
        graded.confidence,
        er_engine::ai::Confidence::Confirmed,
        "0.92 grades back to confirmed"
    );
    assert_eq!(
        state.arbiter_effect.regraded, 0,
        "the expert already said confirmed, so there is nothing to regrade"
    );
    assert!(
        graded.responses.is_empty(),
        "an agreeing verdict leaves no trail"
    );
    // CONTEXT.md: a claim several producers raised carries all of them. Two
    // experts raised this one, and that survives into the review rather than
    // being flattened to whichever the merge happened to file it under.
    assert_eq!(graded.raised_by, vec!["reliability", "security"]);
    assert_eq!(
        graded.raisers(),
        vec!["reliability", "security"],
        "the reader sees both"
    );
}
