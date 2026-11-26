//! The MCP contract tests; see `contract/harness.rs`.
#![expect(
    clippy::expect_used,
    reason = "contract-test scaffolding: a failed setup step should abort the test"
)]

#[path = "contract/harness.rs"]
mod harness;

use harness::{assert_golden, mask_root, run_twice, Mcp, Scenario};
use serde_json::{json, Value};

/// The PR the `gh` fixtures were captured from.
const PR: &str = "VilfredSikker/easy-review#575";
const REPO: &str = "VilfredSikker/easy-review";

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
