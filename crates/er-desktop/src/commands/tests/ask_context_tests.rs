//! The raw diff an ask reads from a tab's published content: the content's
//! own when the review rule says it is the one the ask's scope reads, else a
//! fresh fetch.

use std::collections::HashMap;
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
use crate::commands::{content_routes, AppState};
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

/// The flag-on side: a state whose workspace reads `ports`.
fn service_state(dir: &ReviewFlowTestDir, ports: &Ports) -> AppState {
    let mut state = review_flow_test_state(dir);
    state.desktop.ports = ports.clone();
    let deps = WorkspaceDeps::new(ports.clone());
    let config = dir.path().join("settings").join("config.toml");
    state.service = Arc::new(ServiceState::with_deps(deps, |_| {}).with_config_path(config));
    state
}

/// The tab opened in the registry and loaded, then switched to `mode`.
fn content_tab(
    state: &AppState,
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
        let moved = content_routes::set_tab_mode(&state.desktop, key, mode.git_mode(), mode, None);
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

        assert_eq!(
            er_service::ask_raw_diff(&state.desktop.ports, &content),
            None
        );
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

fn git(repo: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap_or_else(|e| panic!("git: {e}"));
    assert!(output.status.success(), "git {args:?}: {output:?}");
}
