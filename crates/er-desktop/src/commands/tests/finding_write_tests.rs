//! The finding, checklist and review-artifact writes on `er_send`: each
//! runs as a workspace command with the App locked, writes the tab's view
//! bucket and moves `annotations(tab)`. A review delete waits for no review
//! flow, and both deletes refuse a view the tab left.

use std::path::Path;
use std::time::Duration;

use er_engine::ai::{ErChecklist, ErGithubComments, ErReview, TriageReview};
use er_engine::tabs_record::TabDescriptor;
use er_engine::test_support::write_sidecar_json;
use er_engine::TabKey;
use er_protocol::{Annotations, Command, ErErrorKind, PaintedView, Resource, ResourceKey};
use serde_json::json;

use super::annotation_write_tests::{annotations, view};
use super::diff_command_tests::{send, with_tab};
use crate::commands::AppState;
use crate::resources::send::{apply, Effects};
use crate::service::actor_key;
use crate::snapshot::PendingAiReply;

/// A review with a finding that has a response and a linked GitHub thread,
/// a checklist, triage and a diagram, in `key`'s buckets, read into its AI
/// section.
fn seed(state: &AppState, key: TabKey, tab: &TabDescriptor) -> String {
    let content = state.service.content(key, tab).expect("the loaded tab");
    let (er_dir, comments_dir) = (&content.context.er_dir, &content.context.comments_dir);
    let dir = Path::new(er_dir);
    write_sidecar_json::<ErReview>(
        dir,
        json!({
            "version": 1, "diff_hash": "older", "created_at": "t0",
            "files": { "src/f0.rs": { "risk": "high", "findings": [{
                "id": "f-1", "severity": "high", "title": "claim", "hunk_index": 0, "line_start": 1,
                "responses": [{ "id": "r-1", "timestamp": "t1", "text": "Checked" }]
            }] } }
        }),
    );
    write_sidecar_json::<ErGithubComments>(
        Path::new(comments_dir),
        json!({ "version": 1, "diff_hash": "older", "comments": [{
            "id": "c-1", "file": "src/f0.rs", "hunk_index": 0, "line_start": 1,
            "comment": "Looks off", "source": "local", "finding_ref": "f-1"
        }] }),
    );
    write_sidecar_json::<ErChecklist>(
        dir,
        json!({ "version": 1, "diff_hash": "older", "items": [{ "id": "k-1", "text": "Check the auth path" }] }),
    );
    seed_artifacts(dir);
    reload(state, key, tab);
    er_dir.clone()
}

/// Triage and a diagram in `dir`, the two the deletes remove.
fn seed_artifacts(dir: &Path) {
    write_sidecar_json::<TriageReview>(
        dir,
        json!({
            "version": 1, "diff_hash": "older", "created_at": "t0", "first_impression": "Small",
            "diff_stats": { "files_changed": 1, "approx_risk": "low", "domains": [] },
            "verdict": { "primary": "general", "experts": [], "rationale": "r", "confidence": "high" },
            "priority_files": []
        }),
    );
    let flows = er_store::DiagramId::new("flows").unwrap_or_else(|e| panic!("{e}"));
    er_engine::test_support::write_sidecar_json_slot::<er_engine::ai::ErDiagram>(
        dir,
        flows,
        json!({ "version": 1, "diff_hash": "older", "kind": "flows", "title": "Flow", "mermaid": "graph TD; a-->b" }),
    );
}

fn reload(state: &AppState, key: TabKey, tab: &TabDescriptor) {
    let reloaded = state
        .service
        .send(&actor_key(tab), er_service::Command::ReloadSidecars { key });
    assert!(reloaded.is_ok(), "{reloaded:?}");
}

fn finding_ids(annotations: &Annotations) -> (Vec<String>, Vec<String>) {
    let ids = |findings: &[er_protocol::Finding]| findings.iter().map(|f| f.id.clone()).collect();
    (
        ids(&annotations.ai.findings),
        ids(&annotations.ai.resolved_findings),
    )
}

fn responses(annotations: &Annotations) -> Vec<String> {
    let all = annotations
        .ai
        .findings
        .iter()
        .chain(&annotations.ai.resolved_findings);
    all.flat_map(|f| f.responses.iter().map(|r| r.body_markdown.clone()))
        .collect()
}

type Check = fn(&Annotations) -> bool;

/// The nine writes, in an order each leaves something for the next, with
/// what each leaves in `annotations(tab)`.
fn writes(key: TabKey, view: &PaintedView) -> Vec<(&'static str, Command, Check)> {
    let tab = er_service::wire_tab_key(key);
    let view = || view.clone();
    let finding_id = || "f-1".to_string();
    vec![
        (
            "resolve_finding",
            Command::SetFindingResolved {
                tab,
                view: view(),
                finding_id: finding_id(),
                resolved: true,
            },
            |a| finding_ids(a).1 == ["f-1"],
        ),
        (
            "unresolve_finding",
            Command::SetFindingResolved {
                tab,
                view: view(),
                finding_id: finding_id(),
                resolved: false,
            },
            |a| finding_ids(a).0 == ["f-1"],
        ),
        (
            "update_finding_response",
            Command::UpdateFindingResponse {
                tab,
                view: view(),
                finding_id: finding_id(),
                response_id: "r-1".into(),
                body: "Rechecked".into(),
            },
            |a| responses(a) == ["Rechecked"],
        ),
        (
            "delete_finding_response",
            Command::DeleteFindingResponse {
                tab,
                view: view(),
                finding_id: finding_id(),
                response_id: "r-1".into(),
            },
            |a| responses(a).is_empty(),
        ),
        (
            "toggle_checklist_item",
            Command::ToggleChecklistItem {
                tab,
                view: view(),
                index: 0,
            },
            |a| {
                a.ai.checklist
                    .as_ref()
                    .is_some_and(|c| c.items.iter().all(|i| i.checked))
            },
        ),
        (
            "remove_finding_thread",
            Command::RemoveFindingThread {
                tab,
                view: view(),
                finding_id: finding_id(),
            },
            |a| a.ai.threads.is_empty(),
        ),
        (
            "delete_diagram",
            Command::DeleteDiagram {
                tab,
                view: view(),
                id: "flows".into(),
            },
            |a| a.ai.diagrams.is_empty(),
        ),
        (
            "delete_review_artifact",
            Command::DeleteReviewArtifact {
                tab,
                view: view(),
                artifact: "triage".into(),
            },
            |a| a.ai.triage.is_none(),
        ),
        (
            "dismiss_finding",
            Command::DismissFinding {
                tab,
                view: view(),
                finding_id: finding_id(),
            },
            |a| finding_ids(a) == (Vec::new(), Vec::new()),
        ),
    ]
}

/// With the App locked, each write answers through its workspace command,
/// moves `annotations(tab)`, and leaves its change there.
#[test]
fn each_finding_checklist_and_artifact_write_answers_with_the_app_locked() {
    with_tab(|state, (key, tab)| {
        seed(state, key, &tab);
        let (_, seeded) = annotations(state, key);
        assert_eq!(finding_ids(&seeded).0, ["f-1"], "the seed did not load");
        assert_eq!(seeded.ai.threads.len(), 1, "the linked thread did not load");
        for (name, command, check) in writes(key, &view(state, key)) {
            let (before, _) = annotations(state, key);

            let ack = send(state, name, command);

            assert!(ack.is_ok(), "{name}: {ack:?}");
            let (after, now) = annotations(state, key);
            assert!(after > before, "{name} moved no version");
            assert!(check(&now), "{name} left no change: {:?}", now.ai);
        }
    });
}

/// A resolve moves `annotations(tab)` once: the next tick reads the
/// bucket's own write back as nothing new.
#[test]
fn a_finding_resolve_moves_the_annotations_once_across_the_next_tick() {
    with_tab(|state, (key, tab)| {
        seed(state, key, &tab);
        let (before, _) = annotations(state, key);
        let resolve = writes(key, &view(state, key)).swap_remove(0).1;
        let resolved = send(state, "resolve", resolve);
        assert!(resolved.is_ok(), "{resolved:?}");
        let (written, _) = annotations(state, key);
        assert!(written > before, "the resolve moved no version");

        crate::resources::tick::run(&state.desktop);
        std::thread::sleep(Duration::from_millis(500));

        assert_eq!(
            annotations(state, key).0,
            written,
            "the tick read the write back"
        );
    });
}

/// A review delete is refused while a review flow holds the bucket, and
/// the review stays; once the flow lets go, it is deleted.
#[test]
fn a_review_delete_waits_for_no_review_flow() {
    with_tab(|state, (key, tab)| {
        let er_dir = seed(state, key, &tab);
        let scope = er_store::Scope::dir(&er_dir).unwrap_or_else(|e| panic!("{e}"));
        let store = er_engine::store_registry::store();
        let lease = store
            .lease(&scope, Duration::ZERO, "review flow")
            .unwrap_or_else(|e| panic!("{e}"));
        let delete = || Command::DeleteReviewArtifact {
            tab: er_service::wire_tab_key(key),
            view: view(state, key),
            artifact: "review".into(),
        };

        let refused = send(state, "delete review", delete());

        assert!(
            refused.is_err(),
            "the held bucket lost its review: {refused:?}"
        );
        reload(state, key, &tab);
        assert_eq!(
            finding_ids(&annotations(state, key).1).0,
            ["f-1"],
            "the review went"
        );
        drop(lease);
        let deleted = send(state, "delete review", delete());
        assert!(deleted.is_ok(), "{deleted:?}");
        assert!(
            finding_ids(&annotations(state, key).1).0.is_empty(),
            "the review stayed"
        );
    });
}

/// A diagram or triage delete painted while the tab showed Branch is refused
/// once it shows Staged, and the bucket it shows keeps both.
#[test]
fn a_delete_painted_in_another_mode_of_the_tab_is_refused() {
    with_tab(|state, (key, tab)| {
        let painted = view(state, key);
        let switched = send(
            state,
            "set_diff_mode",
            Command::SetDiffMode {
                tab: er_service::wire_tab_key(key),
                mode: "staged".into(),
                pr_number: None,
            },
        );
        assert!(switched.is_ok(), "{switched:?}");
        let content = state.service.content(key, &tab).expect("the staged tab");
        let shown = Path::new(&content.context.er_dir).to_path_buf();
        seed_artifacts(&shown);
        let diagram = shown.join("diagrams").join("flows.json");
        assert!(
            diagram.exists(),
            "the seed wrote no diagram at {}",
            diagram.display()
        );
        let wire = er_service::wire_tab_key(key);

        let deletes = [
            Command::DeleteDiagram {
                tab: wire,
                view: painted.clone(),
                id: "flows".into(),
            },
            Command::DeleteReviewArtifact {
                tab: wire,
                view: painted.clone(),
                artifact: "triage".into(),
            },
        ];
        for delete in deletes {
            let refused = send(state, "stale delete", delete);
            let kind = refused.as_ref().err().map(|e| e.kind);
            assert_eq!(kind, Some(ErErrorKind::Refused), "{refused:?}");
        }

        assert!(diagram.exists(), "the stale delete removed the diagram");
        reload(state, key, &tab);
        assert!(
            annotations(state, key).1.ai.triage.is_some(),
            "the stale delete removed triage"
        );
    });
}

/// Removing a finding's thread while an ask-AI reply is written for it drops
/// that reply from `jobs`, and the `Ack` names `jobs`.
#[test]
fn removing_a_finding_thread_drops_its_pending_ai_reply_from_jobs() {
    with_tab(|state, (key, tab)| {
        seed(state, key, &tab);
        let reply = PendingAiReply {
            started_at_ms: 1,
            ai_action: Some(er_engine::ai::AI_ACTION_ASK),
        };
        state.desktop.pending_ai_replies.insert("c-1".into(), reply);
        let remove = Command::RemoveFindingThread {
            tab: er_service::wire_tab_key(key),
            view: view(state, key),
            finding_id: "f-1".into(),
        };

        let ack = send(state, "remove finding thread", remove).unwrap_or_else(|e| panic!("{e:?}"));

        let jobs = crate::resources::fetch(&state.desktop, ResourceKey::Jobs)
            .unwrap_or_else(|e| panic!("{e:?}"));
        let Resource::Jobs(listed) = jobs.value else {
            panic!("not jobs: {:?}", jobs.value);
        };
        assert!(
            listed.ai_replies.is_empty(),
            "the thinking reply stayed: {listed:?}"
        );
        let named = ack
            .changed
            .iter()
            .any(|c| c.key == ResourceKey::Jobs && c.version == jobs.version);
        assert!(named, "{ack:?}");
    });
}

/// The stored checklist item's flag at index 0 of `er_dir`.
fn stored_first_item_checked(er_dir: &str) -> bool {
    let store = er_engine::store_registry::store();
    let scope = er_store::Scope::dir(er_dir).unwrap_or_else(|e| panic!("{e}"));
    match er_engine::ai::checklist_store::load_checklist(&store, &scope) {
        Ok(er_store::Loaded::Found(stored)) => stored.value.items[0].checked,
        other => panic!("no stored checklist: {other:?}"),
    }
}

/// Two toggles of one checklist item, sent together before either lands,
/// flip it twice: each round leaves the item as it found it.
#[test]
fn two_checklist_toggles_sent_together_flip_the_item_twice() {
    with_tab(|state, (key, tab)| {
        let er_dir = seed(state, key, &tab);
        let toggle = || Command::ToggleChecklistItem {
            tab: er_service::wire_tab_key(key),
            view: view(state, key),
            index: 0,
        };
        let send_now = |command: Command| {
            let effects = Effects {
                notify: &|_, _| {},
                check_update: || panic!("no update check in this test"),
                webviews: &crate::browser_webview::NoWebviews,
                pick_folder: &|| None,
            };
            apply(&state.desktop, command, &effects).map(|applied| applied.ack)
        };
        let rounds = 20;
        let mut flipped_once = 0;
        for _ in 0..rounds {
            let before = stored_first_item_checked(&er_dir);
            let (first, second) = (toggle(), toggle());
            let barrier = std::sync::Barrier::new(2);
            let acks = std::thread::scope(|scope| {
                let sent = [first, second].map(|command| {
                    let (barrier, send_now) = (&barrier, &send_now);
                    scope.spawn(move || {
                        barrier.wait();
                        send_now(command)
                    })
                });
                sent.map(|handle| {
                    handle
                        .join()
                        .unwrap_or_else(|_| panic!("a toggle panicked"))
                })
            });
            for ack in acks {
                assert!(ack.is_ok(), "{ack:?}");
            }
            if stored_first_item_checked(&er_dir) != before {
                flipped_once += 1;
            }
        }
        assert_eq!(
            flipped_once, 0,
            "{flipped_once} of {rounds} rounds of two toggles sent together flipped the item once"
        );
    });
}
