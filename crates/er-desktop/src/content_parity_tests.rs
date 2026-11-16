//! Parity between the two readers of a tab: the Workspace's content load and
//! a `TabState` refresh read one fixture into the same files, hashes,
//! reviewed marks, AI state and sidecars, in each mode the service loads.
//!
//! Each side runs under its own storage root with the same fixture, because
//! a refresh writes relocated anchors and pruned marks back; one shared root
//! would let the second reader see the first one's writes.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use er_engine::ai::{ErGithubComments, ErNotes, ErQuestions, ErReview, ErTour};
use er_engine::app::{DiffMode, TabState};
use er_engine::github::FakeGitHub;
use er_engine::ports::Ports;
use er_engine::tab_identity::{resolve_descriptor, ResolvedTab};
use er_engine::tabs_record::{TabDescriptor, TabKind};
use er_engine::test_support::write_sidecar_json;
use er_engine::TabKey;
use er_git::FakeGit;
use er_service::{Command, Reply, TabContent, WorkspaceDeps};
use serde_json::{json, Value};

const RAW: &str = "\
diff --git a/src/a.rs b/src/a.rs
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,3 +1,3 @@
 a-one
-a-old
+a-new
 a-three
diff --git a/src/b.rs b/src/b.rs
--- a/src/b.rs
+++ b/src/b.rs
@@ -1,3 +1,3 @@
 b-one
-b-old
+b-new
 b-three
";

const WAIT: Duration = Duration::from_secs(10);

fn ports() -> Ports {
    let git: Arc<dyn er_git::Git> = Arc::new(
        FakeGit::default()
            .with_current_branch(Ok("feature".to_string()))
            .with_detect_base_branch(Ok("main".to_string()))
            .with_diff_raw(Ok(RAW.to_string()))
            .with_is_merge_in_progress(false)
            .with_remote_url(Ok("https://github.com/acme/widgets.git".to_string())),
    );
    let github: Arc<dyn er_engine::github::GitHub> =
        Arc::new(FakeGitHub::default().with_pr_diff(Ok(RAW.to_string())));
    Ports { git, github }
}

fn descriptor(kind: &TabKind, repo: &str) -> TabDescriptor {
    TabDescriptor {
        kind: kind.clone(),
        repo_root: repo.to_string(),
        branch: Some("feature".to_string()),
        pr_owner: None,
        pr_repo: None,
        pr_number: (*kind == TabKind::LocalPr).then_some(12),
        pr_head_ref: None,
        base_ref: Some("main".to_string()),
        extra: serde_json::Map::new(),
    }
}

/// What both readers must agree on, with every storage path left out.
#[derive(Debug, PartialEq)]
struct Observed {
    files: Vec<(String, usize, usize, usize, bool)>,
    diff_hash: String,
    branch_diff_hash: String,
    reviewed: BTreeMap<String, String>,
    review: Value,
    questions: Value,
    notes: Value,
    comments: Value,
    tour: Value,
    is_stale: bool,
    tour_stale: bool,
    stale_files: Vec<String>,
    saved_reviewed: BTreeMap<String, String>,
    saved_questions: Value,
}

fn observe(
    files: Vec<(String, usize, usize, usize, bool)>,
    hashes: (&str, &str),
    reviewed: &std::collections::HashMap<String, String>,
    ai: &er_engine::ai::AiState,
    er_dir: &str,
) -> anyhow::Result<Observed> {
    let store = er_engine::store_registry::store();
    let scope = er_store::Scope::dir(er_dir)?;
    let mut stale_files: Vec<String> = ai.stale_files.iter().cloned().collect();
    stale_files.sort();
    let saved_questions = match er_engine::ai::annotation_store::load_questions(&store, &scope) {
        Ok(er_store::Loaded::Found(stored)) => json!(stored.value),
        _ => Value::Null,
    };
    Ok(Observed {
        files,
        diff_hash: hashes.0.to_string(),
        branch_diff_hash: hashes.1.to_string(),
        reviewed: reviewed.clone().into_iter().collect(),
        review: json!(ai.review),
        questions: json!(ai.questions),
        notes: json!(ai.notes),
        comments: json!(ai.github_comments),
        tour: json!(ai.tour),
        is_stale: ai.is_stale,
        tour_stale: ai.tour_stale,
        stale_files,
        saved_reviewed: er_engine::session_store::load_reviewed(&store, &scope)?
            .into_iter()
            .collect(),
        saved_questions,
    })
}

/// A review against an older diff, a tour, a painted question on a line the
/// fixture moved, a note, and marks: one current, one changed, one orphaned.
fn write_fixture(resolved: &ResolvedTab, git: &dyn er_git::Git) -> anyhow::Result<String> {
    let id = resolved.identity();
    let er_dir = id
        .managed_root(git)
        .ok_or_else(|| anyhow::anyhow!("the fixture's bucket did not resolve"))?
        .er_dir();
    let dir = Path::new(&er_dir);
    std::fs::create_dir_all(dir)?;
    write_sidecar_json::<ErReview>(
        dir,
        json!({
            "version": 1, "diff_hash": "older",
            "files": { "src/a.rs": { "risk": "high", "findings": [{
                "id": "f-1", "severity": "high", "title": "claim",
                "hunk_index": 0, "line_start": 2
            }] } },
            "file_hashes": { "src/a.rs": "older", "src/b.rs": "older" }
        }),
    );
    write_sidecar_json::<ErQuestions>(
        dir,
        json!({
            "version": 1, "diff_hash": "older", "questions": [{
                "id": "q-painted", "file": "src/b.rs", "hunk_index": 0, "line_start": 9,
                "line_content": "b-new", "text": "why?", "resolved": false,
                "relocated_at_hash": "older"
            }]
        }),
    );
    write_sidecar_json::<ErNotes>(
        dir,
        json!({
            "version": 1, "diff_hash": "older", "notes": [{
                "id": "n-1", "file": "src/a.rs", "hunk_index": 0, "line_start": 2,
                "line_content": "a-new", "text": "check", "resolved": false
            }]
        }),
    );
    write_sidecar_json::<ErGithubComments>(
        dir,
        json!({ "version": 1, "diff_hash": "older", "comments": [] }),
    );
    if let Some(tour_dir) = id.tour_bucket_er_dir(git) {
        std::fs::create_dir_all(&tour_dir)?;
        write_sidecar_json::<ErTour>(
            Path::new(&tour_dir),
            json!({
                "version": 1, "diff_hash": "older", "title": "Walk",
                "pillars": [{ "id": "p1", "title": "P1" }]
            }),
        );
    }
    let b_hash = er_engine::ai::compute_per_file_hash(RAW, "src/b.rs").unwrap_or_default();
    let marks = [
        ("src/a.rs".to_string(), "older".to_string()),
        ("src/b.rs".to_string(), b_hash),
        ("src/gone.rs".to_string(), "orphan".to_string()),
    ]
    .into_iter()
    .collect();
    let store = er_engine::store_registry::store();
    er_engine::session_store::save_reviewed(&store, &er_store::Scope::dir(&er_dir)?, &marks)?;
    Ok(er_dir)
}

fn resolved(
    kind: &TabKind,
    repo: &str,
    mode: DiffMode,
    ports: &Ports,
) -> anyhow::Result<ResolvedTab> {
    let mut resolved = resolve_descriptor(&*ports.git, &descriptor(kind, repo))?;
    resolved.mode = mode;
    Ok(resolved)
}

/// A temp dir holding an empty repo dir; the caller keeps the dir alive.
fn temp_repo() -> anyhow::Result<(tempfile::TempDir, String)> {
    let root = tempfile::tempdir()?;
    let repo = root.path().join("repo");
    std::fs::create_dir_all(&repo)?;
    let repo = repo.to_string_lossy().into_owned();
    Ok((root, repo))
}

/// The App's side: a restored tab shown in `mode`, refreshed `refreshes`
/// times. The first refresh sees no AI state yet, as on a restore.
fn through_tab_state(kind: &TabKind, mode: DiffMode, refreshes: usize) -> anyhow::Result<Observed> {
    let (root, repo) = temp_repo()?;
    er_engine::storage::with_process_test_storage_root(root.path().join("storage"), || {
        let ports = ports();
        let resolved = resolved(kind, &repo, mode, &ports)?;
        let er_dir = write_fixture(&resolved, &*ports.git)?;
        let mut tab = TabState::from_resolved(resolved, ports)?;
        tab.sync_managed_storage_light();
        for _ in 0..refreshes {
            tab.refresh_diff()?;
        }
        anyhow::ensure!(tab.er_dir() == er_dir, "the tab left its bucket");
        let files = tab
            .files
            .iter()
            .map(|f| (f.path.clone(), f.adds, f.dels, f.hunks.len(), f.compacted))
            .collect();
        observe(
            files,
            (&tab.diff_hash, &tab.branch_diff_hash),
            &tab.reviewed,
            &tab.ai,
            &er_dir,
        )
    })
}

/// The Workspace's side: the same tab loaded under a key, then refreshed.
fn through_the_workspace(
    kind: &TabKind,
    mode: DiffMode,
    refreshes: usize,
) -> anyhow::Result<Observed> {
    let (root, repo) = temp_repo()?;
    er_engine::storage::with_process_test_storage_root(root.path().join("storage"), || {
        let ports = ports();
        let er_dir = write_fixture(&resolved(kind, &repo, mode, &ports)?, &*ports.git)?;
        let handle = er_service::spawn(WorkspaceDeps::new(ports));
        let key = TabKey::next();
        let reply = handle.send_and_wait_with(
            Command::LoadTab {
                key,
                tab: descriptor(kind, &repo),
                mode: Some(mode),
            },
            WAIT,
        )?;
        anyhow::ensure!(
            matches!(reply, Reply::ContentLoaded { .. }),
            "unexpected reply: {reply:?}"
        );
        for _ in 1..refreshes {
            handle.send_and_wait_with(Command::RefreshDiff { key, force: false }, WAIT)?;
        }
        let view = handle.view();
        let content: &TabContent = view
            .tab(key)
            .ok_or_else(|| anyhow::anyhow!("no content under the loaded key"))?;
        anyhow::ensure!(content.context.er_dir == er_dir, "the load left its bucket");
        let files = content
            .diff
            .files
            .iter()
            .map(|f| (f.path.clone(), f.adds, f.dels, f.hunks.len(), f.compacted))
            .collect();
        observe(
            files,
            (
                &content.diff.meta.diff_hash,
                &content.diff.meta.branch_diff_hash,
            ),
            &content.reviewed.marks,
            &content.ai.state,
            &er_dir,
        )
    })
}

fn assert_parity(kind: TabKind, mode: DiffMode) -> anyhow::Result<()> {
    // One refresh is the restore's first load; a second one sees the AI
    // state the first loaded, which is when a working-tree view fetches the
    // branch diff.
    for refreshes in [1, 2] {
        let app = through_tab_state(&kind, mode, refreshes)?;
        let service = through_the_workspace(&kind, mode, refreshes)?;
        assert_eq!(
            service, app,
            "{kind:?} in {mode:?} after {refreshes} refresh(es)"
        );
        assert!(!app.files.is_empty(), "the fixture loaded no diff");
        assert!(app.tour != Value::Null, "the fixture's tour did not load");
    }
    Ok(())
}

#[test]
fn a_plain_tab_loads_the_same_branch_view() -> anyhow::Result<()> {
    assert_parity(TabKind::Working, DiffMode::Branch)
}

#[test]
fn a_plain_tab_loads_the_same_unstaged_view() -> anyhow::Result<()> {
    assert_parity(TabKind::Working, DiffMode::Unstaged)
}

#[test]
fn a_plain_tab_loads_the_same_staged_view() -> anyhow::Result<()> {
    assert_parity(TabKind::Working, DiffMode::Staged)
}

#[test]
fn a_local_pr_tab_loads_the_same_pr_diff() -> anyhow::Result<()> {
    assert_parity(TabKind::LocalPr, DiffMode::PrDiff)
}
