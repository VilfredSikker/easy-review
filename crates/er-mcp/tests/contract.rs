//! The MCP contract tests; see `contract/harness.rs`.
#![expect(
    clippy::expect_used,
    reason = "contract-test scaffolding: a failed setup step should abort the test"
)]

#[path = "contract/harness.rs"]
mod harness;

use std::path::Path;

use er_engine::ai::{ErNotes, ErQuestions, ErReview};
use er_engine::test_support::write_sidecar_json;
use harness::{assert_golden, mask_root, run_twice, run_twice_with, Mcp, Scenario, SEEDED_AT};
use serde_json::{json, Value};

/// The PR the `gh` fixtures were captured from.
const PR: &str = "VilfredSikker/easy-review#575";
const REPO: &str = "VilfredSikker/easy-review";
const FILE: &str = ".github/workflows/ci.yml";

/// `tools/list` on one fresh child, tools in name order, the temp root masked.
fn tools_list() -> Value {
    let mut mcp = Mcp::start();
    let mut list = mcp.request("tools/list", json!({}));
    if let Some(tools) = list.get_mut("tools").and_then(Value::as_array_mut) {
        tools.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    }
    let text = mask_root(&list.to_string(), mcp.root());
    mcp.finish();
    serde_json::from_str(&text).expect("masked JSON")
}

#[test]
fn tools_list_matches_its_golden_on_two_roots() {
    let first = tools_list();
    let second = tools_list();
    assert_eq!(first, second, "tools/list differs between two temp roots");
    assert_golden("tools_list", &first);
}

#[test]
fn a_gh_call_with_no_fixture_fails_the_scenario() {
    // With no title, a pin asks gh for one and ignores the failure, so only
    // the fake's log can catch it.
    let mut mcp = Mcp::start();
    let reply = mcp.call(
        "pr_saved",
        json!({ "action": "pin", "repo": "acme/widgets", "number": 7 }),
    );
    assert!(
        reply.get("error").is_some() || reply.get("content").is_some(),
        "{reply}"
    );
    let finished = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| mcp.finish()));
    let message = finished
        .expect_err("a stray gh call must fail the scenario")
        .downcast::<String>()
        .map(|message| *message)
        .unwrap_or_default();
    assert!(message.contains("called gh with no fixture"), "{message}");
}

/// The tour example the kit offers, as `pr_guide` prepare returns it.
fn tour_example() -> Value {
    json!({
        "version": 1,
        "diff_hash": "<DIFF_HASH>",
        "created_at": "2026-01-01T00:00:00Z",
        "title": "Tour: Example change",
        "overview": "Start with the foundation types, then the UI.",
        "pillars": [{
            "id": "p-1",
            "title": "Foundation",
            "summary": "Core types other files depend on",
            "description": "Core types other files depend on.",
            "order": 0,
            "importance": 90,
            "foundation": true,
            "files": [{ "path": "src/types.rs", "reason": "Shared model", "finding_ids": [] }],
        }],
    })
}

/// A review-kind kit with a REVIEW.md, every sidecar uploaded, then the
/// pinned row and the summary that read them. Its fake `gh` serves PR #575
/// with a REVIEW.md on its base (`tests/contract/gh-review-md/`).
fn review_kinds(s: &mut Scenario) {
    let pr = PR;
    let kit = s.call(
        "pr_prepare",
        json!({ "ref": pr, "kinds": ["triage", "review"] }),
    );
    let hash = kit["kit"]["diff_hash"]
        .as_str()
        .expect("diff_hash")
        .to_string();
    let examples = |at: usize| kit["artifact_specs"]["specs"][at]["examples"].clone();
    let triage = examples(0);
    s.call(
        "pr_upload",
        json!({ "ref": pr, "kind": "triage", "files": { "triage.json": with_hash(&triage["triage.json"], &hash) } }),
    );
    let review = examples(1);
    let files: serde_json::Map<String, Value> = ["review.json", "order.json", "checklist.json"]
        .iter()
        .map(|name| {
            (
                name.to_string(),
                Value::String(with_hash(&review[*name], &hash)),
            )
        })
        .chain(std::iter::once((
            "summary.md".to_string(),
            review["summary.md"].clone(),
        )))
        .collect();
    s.call(
        "pr_upload",
        json!({ "ref": pr, "kind": "review", "files": files }),
    );
    s.call(
        "pr_guide",
        json!({ "ref": pr, "action": "upload", "files": { "tour.json": with_hash(&tour_example(), &hash) } }),
    );
    s.call(
        "pr_saved",
        json!({ "ref": pr, "action": "pin", "title": "Remove a line" }),
    );
    s.call(
        "pr_saved",
        json!({ "repo": REPO, "action": "list", "source": "pinned" }),
    );
    s.call("pr_summarize", json!({ "ref": pr }));
}

#[test]
fn the_review_kinds_match_their_golden() {
    assert_golden(
        "review_kinds",
        &run_twice_with("gh-review-md", review_kinds),
    );
}

/// `text` with the kit's `<DIFF_HASH>` placeholder filled in.
fn with_hash(example: &Value, hash: &str) -> String {
    example.to_string().replace("<DIFF_HASH>", hash)
}

/// Prepare, upload triage (accepted, then rejected twice), guide, summarize.
fn review_flow(s: &mut Scenario) {
    let kit = s.call("pr_prepare", json!({ "ref": PR }));
    let hash = kit["kit"]["diff_hash"]
        .as_str()
        .expect("diff_hash")
        .to_string();
    let triage = &kit["artifact_specs"]["specs"][0]["examples"]["triage.json"];
    s.call(
        "pr_upload",
        json!({ "ref": PR, "kind": "triage", "files": { "triage.json": with_hash(triage, &hash) } }),
    );
    s.call(
        "pr_upload",
        json!({ "ref": PR, "kind": "triage", "files": { "triage.json": with_hash(triage, "stale") } }),
    );
    s.call(
        "pr_upload",
        json!({ "ref": PR, "kind": "tour", "files": { "tour.json": "{}" } }),
    );
    let guide = s.call("pr_guide", json!({ "ref": PR }));
    let tour = &guide["artifact_specs"]["specs"][0]["examples"]["tour.json"];
    s.call(
        "pr_guide",
        json!({ "ref": PR, "action": "upload", "files": { "tour.json": with_hash(tour, &hash) } }),
    );
    s.call(
        "pr_saved",
        json!({ "repo": REPO, "action": "list", "source": "artifacts" }),
    );
    s.call("pr_summarize", json!({ "ref": PR }));
}

/// List, prepare, upload (accepted and rejected), list again.
fn diagrams(s: &mut Scenario) {
    s.call("pr_diagram", json!({ "ref": PR }));
    let kit = s.call(
        "pr_diagram",
        json!({ "ref": PR, "action": "prepare", "kind": "flows" }),
    );
    let diagram = json!({
        "version": 1,
        "diff_hash": kit["kit"]["diff_hash"],
        "kind": "flows",
        "title": "CI triggers",
        "prompt": "",
        "mermaid": "flowchart TD\n  PR-->CI",
    })
    .to_string();
    for file in ["flows.json", "mental-model.json"] {
        s.call(
            "pr_diagram",
            json!({ "ref": PR, "action": "upload", "kind": "flows", "files": { file: diagram } }),
        );
    }
    s.call("pr_diagram", json!({ "ref": PR, "action": "list" }));
}

/// Pin (which adds the project), list, the projects record, unpin, list.
fn saved(s: &mut Scenario) {
    s.call(
        "pr_saved",
        json!({ "ref": PR, "action": "pin", "title": "CI: run on pull requests into staging" }),
    );
    s.call("pr_saved", json!({ "repo": REPO, "action": "list" }));
    s.call("projects_list", json!({}));
    s.call("pr_saved", json!({ "ref": PR, "action": "unpin" }));
    s.call("pr_saved", json!({ "repo": REPO, "action": "list" }));
}

fn thread(id: &str, text: &str) -> Value {
    json!({
        "id": id,
        "timestamp": SEEDED_AT,
        "file": FILE,
        "hunk_index": 0,
        "line_start": 7,
        "line_content": "    branches: [main, master, staging, 'release/**', 'release-*']",
        "text": text,
        "hunk_header": "@@ -4,7 +4,7 @@ on:",
    })
}

/// A question, a note and a finding in `bucket`.
fn seed_feedback(bucket: &Path) {
    write_sidecar_json::<ErQuestions>(
        bucket,
        json!({ "version": 1, "diff_hash": "", "questions": [thread("q-seed", "Why staging?")] }),
    );
    write_sidecar_json::<ErNotes>(
        bucket,
        json!({ "version": 1, "diff_hash": "", "notes": [thread("n-seed", "Mirror this in release.yml")] }),
    );
    write_sidecar_json::<ErReview>(
        bucket,
        json!({
            "version": 1,
            "diff_hash": "",
            "created_at": SEEDED_AT,
            "files": { FILE: {
                "risk": "low",
                "risk_reason": "CI triggers only",
                "summary": "Runs CI on PRs into staging",
                "findings": [{
                    "id": "f-seed",
                    "severity": "low",
                    "category": "correctness",
                    "title": "crap job skips staging pushes",
                    "description": "Only PRs from staging run it.",
                    "hunk_index": 2,
                    "line_start": 173,
                }],
            } },
        }),
    );
}

/// Reply to each kind (the first with no author), resolve each, read back.
fn feedback_steps(s: &mut Scenario, target: &Value) {
    let with = |extra: Value| {
        let mut args = target.clone();
        args.as_object_mut()
            .expect("target object")
            .extend(extra.as_object().expect("args object").clone());
        args
    };
    s.call("pr_feedback_get", with(json!({})));
    let replies = [
        json!({ "type": "question", "id": "q-seed", "text": "Staging gets release candidates." }),
        json!({ "type": "note", "id": "n-seed", "text": "Done.", "author": "reviewer" }),
        json!({ "type": "finding", "id": "f-seed", "text": "Intended: staging is PR-only." }),
    ];
    for (n, reply) in replies.into_iter().enumerate() {
        let made = s.call("pr_feedback_reply", with(reply));
        let id = made["reply"]["id"].as_str().expect("reply id").to_string();
        s.generated(&id, &format!("<reply-{}>", n + 1));
    }
    for (kind, id) in [
        ("question", "q-seed"),
        ("question", "q-seed"),
        ("note", "n-seed"),
        ("finding", "f-seed"),
    ] {
        s.call(
            "pr_feedback_resolve",
            with(json!({ "type": kind, "id": id })),
        );
    }
    s.call("pr_feedback_get", with(json!({})));
    s.call("pr_feedback_get", with(json!({ "include_resolved": true })));
}

fn feedback_pr_bucket(s: &mut Scenario) {
    let pr = s.call("pr_resolve", json!({ "ref": PR }));
    seed_feedback(Path::new(pr["bucket_path"].as_str().expect("bucket_path")));
    feedback_steps(s, &json!({ "ref": PR }));
}

fn git(dir: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=contract",
            "-c",
            "user.email=contract@example.invalid",
        ])
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .status()
        .expect("run git");
    assert!(status.success(), "git {args:?}");
}

/// The child's working dir is a checkout of the fixture PR's head branch.
fn feedback_local_bucket(s: &mut Scenario) {
    let work = s.root().join("work");
    git(&work, &["init", "-q", "-b", "ci/staging-prs"]);
    git(
        &work,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/VilfredSikker/easy-review.git",
        ],
    );
    git(&work, &["commit", "-q", "--allow-empty", "-m", "head"]);
    s.call("pr_feedback_get", json!({ "bucket": "local", "ref": PR }));
    let empty = s.call("pr_feedback_get", json!({ "bucket": "local" }));
    seed_feedback(Path::new(
        empty["bucket_path"].as_str().expect("bucket_path"),
    ));
    feedback_steps(s, &json!({ "bucket": "local" }));
}

#[test]
fn review_flow_matches_its_golden() {
    assert_golden("review_flow", &run_twice(review_flow));
}

#[test]
fn diagrams_match_their_golden() {
    assert_golden("diagrams", &run_twice(diagrams));
}

#[test]
fn saved_prs_match_their_golden() {
    assert_golden("saved", &run_twice(saved));
}

#[test]
fn pr_bucket_feedback_matches_its_golden() {
    assert_golden("feedback_pr_bucket", &run_twice(feedback_pr_bucket));
}

#[test]
fn local_bucket_feedback_matches_its_golden() {
    assert_golden("feedback_local_bucket", &run_twice(feedback_local_bucket));
}

/// A pin with no title asks gh for one: `gh pr view 7 --repo acme/widgets
/// --json title`. The fixtures dir lists that call against `fixture`.
fn pin_with_fixture(fixture: &str, body: Option<&str>) -> (Mcp, Value, tempfile::TempDir) {
    let fixtures = tempfile::tempdir().expect("fixtures dir");
    let call = [
        "pr",
        "view",
        "7",
        "--repo",
        "acme/widgets",
        "--json",
        "title",
    ]
    .join("\t");
    std::fs::write(
        fixtures.path().join("index.tsv"),
        format!("{fixture}\t0\t{call}\n"),
    )
    .expect("index");
    if let Some(body) = body {
        std::fs::write(fixtures.path().join(fixture), body).expect("fixture");
    }
    let mut mcp = Mcp::start_with_fixtures(fixtures.path());
    let reply = mcp.call(
        "pr_saved",
        json!({ "action": "pin", "repo": "acme/widgets", "number": 7 }),
    );
    (mcp, reply, fixtures)
}

#[test]
fn a_listed_call_is_served_from_its_fixture() {
    let (mcp, reply, _fixtures) = pin_with_fixture("title.json", Some(r#"{"title":"Fix"}"#));
    mcp.finish();
    assert!(
        reply.to_string().contains(r#"\"title\": \"Fix\""#),
        "{reply}"
    );
}

#[test]
fn a_listed_call_whose_fixture_is_missing_fails_the_scenario() {
    let (mcp, _reply, _fixtures) = pin_with_fixture("missing.json", None);
    let finished = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| mcp.finish()));
    let message = finished
        .expect_err("a missing fixture must fail the scenario")
        .downcast::<String>()
        .map(|message| *message)
        .unwrap_or_default();
    assert!(
        message.contains("pr view 7 --repo acme/widgets"),
        "{message}"
    );
}
