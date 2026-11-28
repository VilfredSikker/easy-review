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

/// A project for the fixture repo, rooted at the child's working dir (a
/// checkout of the fixture PR's head branch), made the active project.
fn seed_project(s: &Scenario) {
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
    let root_path = work.to_string_lossy().into_owned();
    er_engine::storage::with_test_storage_root(s.root().join("storage"), || {
        er_engine::projects_pins::update(|file| {
            file.projects.push(er_engine::projects_pins::ProjectRecord {
                id: "easy-review".into(),
                name: "easy-review".into(),
                root_path,
                remote: Some(REPO.into()),
                ..Default::default()
            });
            file.active_id = Some("easy-review".into());
            Ok(true)
        })
    })
    .expect("seed the project");
}

/// The review queue, ranked three ways and across projects.
fn queue_and_stats(s: &mut Scenario) {
    seed_project(s);
    s.call("prs_query", json!({ "repo": REPO, "limit": 2 }));
    s.call(
        "prs_query",
        json!({ "repo": REPO, "sort": "updated", "limit": 2 }),
    );
    s.call(
        "prs_query",
        json!({ "repo": REPO, "filter": "review_debt", "limit": 2 }),
    );
    s.call("prs_query", json!({ "cross_repo": true, "limit": 2 }));
    s.call("pr_stats", json!({ "ref": PR, "include_hotspots": true }));
    s.call("pr_stats", json!({ "repo": REPO, "numbers": [575, 600] }));
}

/// The `pr_resolve` forms that ask `gh`, and the bare number.
fn resolve_forms(s: &mut Scenario) {
    seed_project(s);
    let work = s.root().join("work").to_string_lossy().into_owned();
    s.call("pr_resolve", json!({ "ref": work }));
    s.call("pr_resolve", json!({ "ref": REPO }));
    s.call("pr_resolve", json!({ "ref": "epic/p78-mcp-contract" }));
    s.call("pr_resolve", json!({ "ref": "main" }));
    s.call("pr_resolve", json!({ "ref": "no-such-branch" }));
    s.call("pr_resolve", json!({ "ref": "575" }));
}

#[test]
fn the_queue_and_stats_match_their_golden() {
    assert_golden("queue_and_stats", &run_twice(queue_and_stats));
}

#[test]
fn the_gh_resolve_forms_match_their_golden() {
    assert_golden("resolve_forms", &run_twice(resolve_forms));
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

/// The REVIEW.md body the fixture's response carries, after its headers.
fn review_md_body() -> String {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/contract/gh-review-md/review-md-main.txt");
    let response = std::fs::read_to_string(fixture).expect("REVIEW.md fixture");
    let (_, body) = response.split_once("\r\n\r\n").expect("headers end");
    body.to_string()
}

/// The file the kit names at `review_md_path`, as an agent reads it.
fn read_review_md(kit: &Value) -> String {
    let path = kit["kit"]["review_md_path"]
        .as_str()
        .expect("review_md_path");
    std::fs::read_to_string(path).expect("review-md.md")
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
    assert_eq!(
        read_review_md(&kit),
        format!(
            "REVIEW.md files from the base branch (main). Each applies to files under its folder.\n\n## REVIEW.md\n\n{}\n\n",
            review_md_body().trim()
        ),
        "the file at kit.review_md_path holds the fixture's REVIEW.md"
    );
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
        json!({ "ref": pr, "action": "pin", "title": "CI: run on pull requests into staging" }),
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

/// A reply id with its clock digits as `<millis>`: `q-1795…-0` reads
/// `q-<millis>-0`, so the prefix and the sequence stay in the golden.
fn clock_digits_masked(id: &str) -> String {
    let start = id.find(|c: char| c.is_ascii_digit()).unwrap_or(id.len());
    let end = id
        .get(start..)
        .and_then(|rest| rest.find(|c: char| !c.is_ascii_digit()))
        .map_or(id.len(), |len| start + len);
    format!(
        "{}<millis>{}",
        id.get(..start).unwrap_or_default(),
        id.get(end..).unwrap_or_default()
    )
}

/// A question, a note and a finding in `bucket`, and PR #575's diff as its
/// `diff-tmp`, so the replies and reads carry the real diff hash.
fn seed_feedback(bucket: &Path) {
    let diff = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/contract/gh/pr-575.diff");
    std::fs::create_dir_all(bucket).expect("bucket dir");
    std::fs::copy(diff, bucket.join("diff-tmp")).expect("seed diff-tmp");
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
    for reply in replies {
        let made = s.call("pr_feedback_reply", with(reply));
        let id = made["reply"]["id"].as_str().expect("reply id").to_string();
        s.generated(&id, &clock_digits_masked(&id));
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

/// MCP's reply copies its parent's row and fetches no diff: with no fixture
/// listed, any `gh` call it made would fail the scenario.
#[test]
fn a_feedback_reply_calls_no_gh() {
    let fixtures = tempfile::tempdir().expect("fixtures dir");
    let mut mcp = Mcp::start_with_fixtures(fixtures.path());
    let pr = mcp.call("pr_resolve", json!({ "ref": PR }));
    let pr: Value = serde_json::from_str(pr["content"][0]["text"].as_str().expect("text"))
        .expect("pr_resolve JSON");
    seed_feedback(Path::new(pr["bucket_path"].as_str().expect("bucket_path")));
    let reply = mcp.call(
        "pr_feedback_reply",
        json!({ "ref": PR, "type": "question", "id": "q-seed", "text": "Because." }),
    );
    assert_eq!(reply["isError"], json!(false), "{reply}");
    mcp.finish();
}
