//! The prompt context of an ask, read from a tab's published content
//! (`ask_context`): the raw diff, the branches, the provider's directory, and
//! for a finding its fields and its line content.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;
use std::sync::Arc;

use er_engine::ports::Ports;
use er_engine::tab_identity::DiffMode;
use er_engine::tabs_record::{TabDescriptor, TabKind};
use er_engine::TabKey;
use er_service::{TabContent, WorkspaceDeps};

use super::arena_resource_tests::wait_until;
use super::diff_resource_tests::with_storage;
use super::{review_flow_test_state, ReviewFlowTestDir};
use crate::ask_context::{finding_ask, AskTab};
use crate::resources::content_routes;
use crate::route_helpers::DesktopState;
use crate::service::ServiceState;

const HEAD: &str = "\
diff --git a/src/a.rs b/src/a.rs
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,3 +1,3 @@
 a-one
-a-old
+a-new
 a-three
";

/// The newest commit's diff, which History shows first.
const COMMIT: &str = "\
diff --git a/src/c.rs b/src/c.rs
--- a/src/c.rs
+++ b/src/c.rs
@@ -1 +1 @@
-c-old
+c-new
";

/// A diff over the lazy-parse size, so every file past the first starts as
/// a stub.
fn lazy_raw() -> String {
    let mut raw = HEAD.to_string();
    let filler = "x".repeat(120);
    for n in 0..2000 {
        let _ = write!(
            raw,
            "diff --git a/gen/f{n}.rs b/gen/f{n}.rs\n--- a/gen/f{n}.rs\n+++ b/gen/f{n}.rs\n\
             @@ -1,2 +1,2 @@\n-old {filler}\n+new {filler}\n {filler}\n"
        );
    }
    raw
}

fn logged(hash: &str) -> er_engine::git::CommitInfo {
    er_engine::git::CommitInfo {
        hash: hash.to_string(),
        short_hash: hash.to_string(),
        subject: format!("subject {hash}"),
        author: "you".to_string(),
        date: "2026-11-01T00:00:00Z".to_string(),
        relative_date: String::new(),
        file_count: 0,
        adds: 1,
        dels: 1,
        is_merge: false,
        diffstat_known: true,
    }
}

fn ports(raw: &str) -> Ports {
    let git: Arc<dyn er_git::Git> = Arc::new(
        er_git::FakeGit::default()
            .with_current_branch(Ok("feature".to_string()))
            .with_detect_base_branch(Ok("main".to_string()))
            .with_diff_shortstat((1, 1, 1))
            .with_commit_shortstats(HashMap::new())
            .with_diff_raw(Ok(raw.to_string()))
            .with_diff_against_branch(Ok(raw.to_string()))
            .with_log_range(Ok(vec![logged("c2"), logged("c1")]))
            .with_diff_commit(Ok(COMMIT.to_string()))
            .with_unpushed_shas(["c2".to_string()].into_iter().collect())
            .with_rev_parse_verify(Ok(Some("b".repeat(40))))
            .with_blob_oid(Ok("b".repeat(40)))
            .with_is_merge_in_progress(false),
    );
    Ports::with_git(git)
}

fn repo(dir: &ReviewFlowTestDir) -> String {
    let root = dir.path().join("repo");
    std::fs::create_dir_all(&root).unwrap_or_else(|e| panic!("repo: {e}"));
    root.to_string_lossy().into_owned()
}

/// A local branch view of `topic`, which no checkout has.
fn branch_view(repo: &str) -> TabDescriptor {
    let mut tab = content_routes::working_tab(repo.to_string());
    tab.kind = TabKind::LocalBranch;
    tab.branch = Some("topic".to_string());
    tab
}

/// A state whose workspace reads `ports`.
fn service_state(dir: &ReviewFlowTestDir, ports: &Ports) -> DesktopState {
    let mut state = review_flow_test_state(dir);
    state.ports = ports.clone();
    let deps = WorkspaceDeps::new(ports.clone());
    let config = dir.path().join("settings").join("config.toml");
    state.service = Arc::new(ServiceState::with_deps(deps, |_| {}).with_config_path(config));
    state
}

/// The tab opened in the registry and loaded, then switched to `mode`.
fn content_tab(
    state: &DesktopState,
    descriptor: &TabDescriptor,
    mode: Option<DiffMode>,
) -> (TabKey, Arc<TabContent>) {
    let opened = content_routes::open(state, descriptor.clone(), false);
    assert!(opened.is_ok(), "{opened:?}");
    let key = state
        .service
        .app_registry()
        .tabs_view()
        .active
        .unwrap_or_else(|| panic!("no active tab"));
    wait_until("the content", || {
        state.service.content(key, descriptor).is_some()
    });
    if let Some(mode) = mode {
        let moved = content_routes::set_tab_mode(state, key, mode.git_mode(), mode, None);
        assert!(moved.is_ok(), "{moved:?}");
        wait_until("the switch", || {
            state
                .service
                .content(key, descriptor)
                .is_some_and(|c| c.tab.mode == mode)
        });
    }
    let content = state
        .service
        .content(key, descriptor)
        .unwrap_or_else(|| panic!("no content"));
    (key, content)
}

/// The rows of an ask's context that every view sets.
fn shown(tab: &AskTab<'_>) -> (String, String, String, String, Option<String>) {
    (
        tab.repo_root.to_string(),
        tab.base_branch.to_string(),
        tab.current_branch.to_string(),
        tab.work_dir.clone(),
        tab.raw_diff.clone(),
    )
}

/// On a plain local tab in History, with the newest commit shown, the ask
/// reads no raw diff: the fetch refuses History. The content holds the
/// commit's diff, which a step with no guard would answer.
#[test]
fn a_history_ask_reads_no_raw_diff() {
    with_storage(|dir| {
        let ports = ports(HEAD);
        let repo = repo(dir);
        let descriptor = content_routes::working_tab(repo.clone());
        let state = service_state(dir, &ports);
        let (_, content) = content_tab(&state, &descriptor, Some(DiffMode::History));
        assert!(
            !content.diff.meta.raw.is_empty(),
            "the content shows no commit"
        );

        let on = shown(&crate::ai_ask::content_ask_tab(&state, &content));
        assert_eq!(on.0, repo);
        assert_eq!(on.4, None);
    });
}

/// A branch view whose branch is checked out nowhere matches the review rule
/// in every view, so in History only the view skip keeps the ask off the
/// content's diff, the commit's; the fetch refuses History.
#[test]
fn a_history_ask_on_an_unchecked_out_branch_reads_no_diff() {
    with_storage(|dir| {
        let ports = ports(HEAD);
        let repo = repo(dir);
        let descriptor = branch_view(&repo);
        let state = service_state(dir, &ports);
        let (_, content) = content_tab(&state, &descriptor, Some(DiffMode::History));
        let id = content.tab.identity();
        assert!(id.checkout_root.is_none(), "the branch has a checkout");
        assert!(er_engine::review_flow_driver::cached_diff_matches_scope(
            &id, "branch"
        ));
        assert!(!content.diff.meta.branch_diff_hash.is_empty(), "no hash");
        assert!(
            !content.diff.meta.raw.is_empty(),
            "the content shows no commit"
        );

        assert_eq!(er_service::ask_raw_diff(&state.ports, &content), None);
    });
}

/// A finding in a file the content holds as a stub: the ask parses the stub
/// first, so it reads the finding's line content.
#[test]
fn a_finding_ask_on_a_stub_reads_its_line_content() {
    with_storage(|dir| {
        let raw = lazy_raw();
        assert!(raw.len() > 200_000, "the fixture is not lazy");
        let ports = ports(&raw);
        let repo = repo(dir);
        let descriptor = content_routes::working_tab(repo.clone());
        let bucket = er_service::descriptor_bucket(&*ports.git, &descriptor)
            .unwrap_or_else(|e| panic!("{e}"));
        write_review(Path::new(&bucket), &raw);
        let path = "gen/f300.rs";

        let state = service_state(dir, &ports);
        let (key, content) = content_tab(&state, &descriptor, None);
        assert!(content.diff.meta.lazy, "the content is not lazy");
        assert!(
            content
                .diff
                .files
                .iter()
                .any(|f| f.path == path && f.hunks.is_empty()),
            "the finding's file is not a stub"
        );
        // The page asks the content for the file it shows.
        let first = content.diff.files[0].path.clone();
        let parse = er_service::Command::ParseFiles {
            key,
            paths: vec![first],
            expand: false,
        };
        let parsed = state
            .service
            .send(&crate::service::actor_key(&descriptor), parse);
        assert!(parsed.is_ok(), "{parsed:?}");
        let tab = crate::arena_service::tab(&state, Some(er_service::wire_tab_key(key)))
            .unwrap_or_else(|e| panic!("{e:?}"));
        let content = crate::ai_ask::parsed_for_finding(&state, &tab, "f-1");
        let on = finding_ask(crate::ai_ask::content_ask_tab(&state, &content), "f-1")
            .unwrap_or_else(|e| panic!("{e}"));

        assert!(on.line_content.is_some(), "the ask read no line content");
    });
}

/// A working tab loaded straight into its unstaged view, with no AI data, has
/// no branch hash, so the ask fetches the diff again rather than trust the
/// content's, which the working tree has moved past since.
#[test]
fn an_unhashed_diff_is_fetched_again() {
    with_storage(|dir| {
        let repo = dir.path().join("live");
        std::fs::create_dir_all(&repo).unwrap_or_else(|e| panic!("{e}"));
        for args in [
            &["init", "-q", "-b", "main"][..],
            &["config", "user.name", "Ask Test"],
            &["config", "user.email", "ask-test@example.invalid"],
            &["config", "commit.gpgsign", "false"],
        ] {
            git(&repo, args);
        }
        std::fs::write(repo.join("tracked.rs"), "one\n").unwrap_or_else(|e| panic!("{e}"));
        git(&repo, &["add", "tracked.rs"]);
        git(&repo, &["commit", "-qm", "baseline"]);
        std::fs::write(repo.join("tracked.rs"), "two\n").unwrap_or_else(|e| panic!("{e}"));

        let ports = Ports::cli();
        let descriptor = content_routes::working_tab(repo.to_string_lossy().into_owned());
        let handle = er_service::spawn(WorkspaceDeps::new(ports.clone()));
        let key = TabKey::next();
        let load = er_service::Command::LoadTab {
            key,
            tab: descriptor,
            mode: Some(DiffMode::Unstaged),
        };
        let loaded = handle.send_and_wait_with(load, std::time::Duration::from_secs(10));
        assert!(loaded.is_ok(), "{loaded:?}");
        let content = handle
            .view()
            .tab(key)
            .cloned()
            .unwrap_or_else(|| panic!("no content"));
        let meta = &content.diff.meta;
        assert!(
            meta.branch_diff_hash.is_empty(),
            "the view has a branch hash"
        );
        assert!(meta.raw.contains("+two"), "{}", meta.raw);
        let id = content.tab.identity();
        assert!(er_engine::review_flow_driver::cached_diff_matches_scope(
            &id, "unstaged"
        ));

        std::fs::write(repo.join("tracked.rs"), "three\n").unwrap_or_else(|e| panic!("{e}"));
        let fresh = er_engine::diff_load::fetch_raw(&ports, &id, "unstaged", false).ok();
        assert!(
            fresh.as_deref().is_some_and(|raw| raw.contains("+three")),
            "{fresh:?}"
        );

        assert_eq!(er_service::ask_raw_diff(&ports, &content), fresh);
    });
}

// A fixture repo the test spawns git in, as a test may.
#[cfg(test)]
fn git(repo: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap_or_else(|e| panic!("git: {e}"));
    assert!(output.status.success(), "git {args:?}: {output:?}");
}

fn write_review(bucket: &Path, raw: &str) {
    er_engine::test_support::write_sidecar_json::<er_engine::ai::ErReview>(
        bucket,
        serde_json::json!({
            "version": 1,
            "diff_hash": er_engine::ai::compute_diff_hash(raw),
            "files": {
                "gen/f300.rs": {
                    "risk": "low",
                    "findings": [{
                        "id": "f-1",
                        "severity": "low",
                        "title": "A defect",
                        "description": "It breaks.",
                        "hunk_index": 0,
                        "line_start": 1
                    }]
                }
            }
        }),
    );
}
