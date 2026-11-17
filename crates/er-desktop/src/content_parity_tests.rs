//! Parity between the two readers of a tab: the Workspace's content load and
//! a `TabState` refresh read one fixture into the same sections, in each mode
//! the service loads. A section is everything the snapshot reads from
//! `TabState` for it: the files and their lazy stubs, the filter-visible
//! lists, the reviewed marks, the AI state, the tour, the staleness, the PR
//! and the preview keys with their stamps.
//!
//! Each side runs under its own storage root with the same fixture, because
//! a refresh writes relocated anchors and pruned marks back; one shared root
//! would let the second reader see the first one's writes.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use er_engine::ai::{ErGithubComments, ErNotes, ErQuestions, ErReview, ErTour};
use er_engine::app::{DiffMode, TabState};
use er_engine::config::{FileKindRepoConfig, RiskRules};
use er_engine::filter::parse_filter_expr;
use er_engine::git::DiffFile;
use er_engine::github::{FakeGitHub, PrOverviewData};
use er_engine::ports::Ports;
use er_engine::preview_key;
use er_engine::tab_identity::{resolve_descriptor, ResolvedTab};
use er_engine::tabs_record::{TabDescriptor, TabKind};
use er_engine::test_support::write_sidecar_json;
use er_engine::visible_files::{visible_files, FileListView};
use er_engine::TabKey;
use er_git::FakeGit;
use er_service::{Command, Reply, TabContent, WorkspaceDeps};
use serde_json::{json, Value};

const HEAD: &str = "\
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
diff --git a/docs/old.md b/docs/new.md
similarity index 90%
rename from docs/old.md
rename to docs/new.md
index 1111111..2222222 100644
--- a/docs/old.md
+++ b/docs/new.md
@@ -1 +1 @@
-old
+moved
";

/// Over the lazy-parse size, with more files than a lazy diff parses up
/// front, so every file starts as a stub.
fn raw() -> String {
    let filler = "x".repeat(120);
    let mut raw = HEAD.to_string();
    for n in 0..600 {
        let _ = write!(
            raw,
            "diff --git a/gen/f{n}.rs b/gen/f{n}.rs\n--- a/gen/f{n}.rs\n+++ b/gen/f{n}.rs\n\
             @@ -1,2 +1,2 @@\n-old {filler}\n+new {filler}\n {filler}\n"
        );
    }
    raw
}

const WAIT: Duration = Duration::from_secs(10);
const PR: u64 = 12;

fn overview() -> PrOverviewData {
    PrOverviewData {
        number: PR,
        title: "Widgets".to_string(),
        body: String::new(),
        state: "OPEN".to_string(),
        author: "you".to_string(),
        url: String::new(),
        base_branch: "main".to_string(),
        head_branch: "feature".to_string(),
        checks: Vec::new(),
        reviewers: Vec::new(),
    }
}

fn commit(hash: &str) -> er_engine::git::CommitInfo {
    er_engine::git::CommitInfo {
        hash: hash.to_string(),
        short_hash: hash.to_string(),
        subject: String::new(),
        author: String::new(),
        date: String::new(),
        relative_date: String::new(),
        file_count: 0,
        adds: 0,
        dels: 0,
        is_merge: false,
        diffstat_known: false,
    }
}

fn ports() -> Ports {
    let raw = raw();
    let git: Arc<dyn er_git::Git> = Arc::new(
        FakeGit::default()
            .with_current_branch(Ok("feature".to_string()))
            .with_detect_base_branch(Ok("main".to_string()))
            .with_diff_raw(Ok(raw.clone()))
            .with_is_merge_in_progress(false)
            .with_remote_url(Ok("https://github.com/acme/widgets.git".to_string()))
            .with_rev_parse_verify(Ok(Some("h-local".to_string())))
            .with_blob_oid(Ok("b".repeat(40))),
    );
    let github: Arc<dyn er_engine::github::GitHub> = Arc::new(
        FakeGitHub::default()
            .with_pr_diff(Ok(raw))
            .with_overview(Ok(Some(overview())))
            .with_pr_commits(Ok(vec![commit("c1")])),
    );
    Ports { git, github }
}

fn descriptor(kind: &TabKind, repo: &str) -> TabDescriptor {
    TabDescriptor {
        kind: kind.clone(),
        repo_root: repo.to_string(),
        branch: Some("feature".to_string()),
        pr_owner: None,
        pr_repo: None,
        pr_number: (*kind == TabKind::LocalPr).then_some(PR),
        pr_head_ref: None,
        base_ref: Some("main".to_string()),
        extra: serde_json::Map::new(),
    }
}

/// The views the filter-visible check lists: a `risk:` rule, which reads the
/// review, and the unreviewed-only toggle, which reads the marks. The rule
/// tables come from config on both paths, so both sides get the same ones.
const VIEWS: [(&str, bool); 2] = [("risk:high", false), ("", true)];

fn visible(files: &[&DiffFile], ai: &er_engine::ai::AiState, reviewed: &Marks) -> Vec<Vec<String>> {
    let (risk_rules, file_kinds) = (RiskRules::default(), FileKindRepoConfig::default());
    let reviewed: std::collections::HashMap<String, String> =
        reviewed.clone().into_iter().collect();
    VIEWS
        .iter()
        .map(|(expr, unreviewed_only)| {
            let rules = parse_filter_expr(expr);
            let view = FileListView {
                rules: &rules,
                review: ai.review.as_ref(),
                risk_rules: &risk_rules,
                file_kinds: &file_kinds,
                search_lower: "",
                unreviewed_only: *unreviewed_only,
                reviewed: &reviewed,
            };
            visible_files(files.iter().copied(), &view)
                .into_iter()
                .map(|(_, file)| file.path.clone())
                .collect()
        })
        .collect()
}

type Marks = BTreeMap<String, String>;

/// One file as the snapshot lists it: path, counts, hunks and compaction.
type FileRow = (String, usize, usize, usize, bool);

/// What both readers must agree on, with every storage path left out.
#[derive(Debug)]
struct Observed {
    files: Vec<FileRow>,
    lazy: bool,
    hashes: (String, String),
    visible: Vec<Vec<String>>,
    reviewed: Marks,
    saved_reviewed: Marks,
    ai: [Value; 5],
    tour: Value,
    staleness: (bool, bool, Vec<String>),
    pr: (Option<String>, Vec<String>, Option<String>),
    preview_keys: Vec<(String, String)>,
    stamps: (BTreeMap<String, SystemTime>, BTreeMap<String, String>),
}

/// What a reader holds of one tab, borrowed from its own types.
struct Read<'a> {
    files: Vec<&'a DiffFile>,
    lazy: bool,
    hashes: (&'a str, &'a str),
    reviewed: &'a std::collections::HashMap<String, String>,
    ai: &'a er_engine::ai::AiState,
    pr: (
        Option<&'a PrOverviewData>,
        &'a [er_engine::git::CommitInfo],
        Option<&'a str>,
    ),
    /// The context key, then each previewable file's key.
    preview_keys: Vec<(String, String)>,
    /// The mtimes and blob ids the keys read.
    stamps: (&'a HashMap<String, SystemTime>, &'a HashMap<String, String>),
}

/// The keys the snapshot reads, given the key functions of one reader.
fn preview_keys(
    files: &[&DiffFile],
    context: String,
    file_key: impl Fn(&str) -> String,
) -> Vec<(String, String)> {
    let mut keys = vec![(String::new(), context)];
    keys.extend(
        files
            .iter()
            .filter(|file| preview_key::has_preview(&file.path))
            .map(|file| (file.path.clone(), file_key(&file.path))),
    );
    keys
}

fn observe(read: Read<'_>, er_dir: &str) -> anyhow::Result<Observed> {
    let store = er_engine::store_registry::store();
    let scope = er_store::Scope::dir(er_dir)?;
    let ai = read.ai;
    let mut stale_files: Vec<String> = ai.stale_files.iter().cloned().collect();
    stale_files.sort();
    let saved_questions = match er_engine::ai::annotation_store::load_questions(&store, &scope) {
        Ok(er_store::Loaded::Found(stored)) => json!(stored.value),
        _ => Value::Null,
    };
    let reviewed: Marks = read.reviewed.clone().into_iter().collect();
    let (overview, commits, head) = read.pr;
    Ok(Observed {
        files: read
            .files
            .iter()
            .map(|f| (f.path.clone(), f.adds, f.dels, f.hunks.len(), f.compacted))
            .collect(),
        lazy: read.lazy,
        hashes: (read.hashes.0.to_string(), read.hashes.1.to_string()),
        visible: visible(&read.files, ai, &reviewed),
        reviewed,
        saved_reviewed: er_engine::session_store::load_reviewed(&store, &scope)?
            .into_iter()
            .collect(),
        ai: [
            json!(ai.review),
            json!(ai.questions),
            json!(ai.notes),
            json!(ai.github_comments),
            saved_questions,
        ],
        tour: json!(ai.tour),
        staleness: (ai.is_stale, ai.tour_stale, stale_files),
        pr: (
            overview.map(|o| o.title.clone()),
            commits.iter().map(|c| c.hash.clone()).collect(),
            head.map(str::to_string),
        ),
        preview_keys: read.preview_keys,
        stamps: (
            read.stamps.0.clone().into_iter().collect(),
            read.stamps.1.clone().into_iter().collect(),
        ),
    })
}

/// A review against an older diff with a high-risk file, a tour, a painted
/// question on a line the fixture moved, a note, and marks: one current, one
/// changed, one orphaned.
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
            "files": {
                "src/a.rs": { "risk": "high", "findings": [{
                    "id": "f-1", "severity": "high", "title": "claim",
                    "hunk_index": 0, "line_start": 2
                }] },
                "src/b.rs": { "risk": "low", "findings": [] }
            },
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
    let b_hash = er_engine::ai::compute_per_file_hash(&raw(), "src/b.rs").unwrap_or_default();
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

/// A temp dir holding the checkout both readers share, with the renamed
/// document in it; the caller keeps the dir alive. The preview keys read the
/// root and the document's mtime, so the two sides must see one checkout.
fn temp_repo() -> anyhow::Result<(tempfile::TempDir, String)> {
    let root = tempfile::tempdir()?;
    let repo = root.path().join("repo");
    std::fs::create_dir_all(repo.join("docs"))?;
    std::fs::write(repo.join("docs/new.md"), "moved\n")?;
    let repo = repo.to_string_lossy().into_owned();
    Ok((root, repo))
}

/// The App's side: a restored tab shown in `mode`, refreshed `refreshes`
/// times. The first refresh sees no AI state yet, as on a restore. A PR tab
/// then reads what the App's other paths fill: the overview the comment sync
/// reads, the commits a PR open reads, and the head a restore seeds from the
/// fetched ref.
fn through_tab_state(
    kind: &TabKind,
    mode: DiffMode,
    refreshes: usize,
    (root, repo): (&Path, &str),
) -> anyhow::Result<Observed> {
    er_engine::storage::with_process_test_storage_root(root.join("app-storage"), || {
        let ports = ports();
        let github = Arc::clone(&ports.github);
        let resolved = resolved(kind, repo, mode, &ports)?;
        let er_dir = write_fixture(&resolved, &*ports.git)?;
        let mut tab = TabState::from_resolved(resolved, ports)?;
        tab.sync_managed_storage_light();
        for _ in 0..refreshes {
            tab.refresh_diff()?;
        }
        anyhow::ensure!(tab.er_dir() == er_dir, "the tab left its bucket");
        if let Some(number) = tab.pr_number {
            tab.pr_data = github.overview(&tab.repo_root, Some(number)).ok().flatten();
            let limit = er_engine::pr_open_entry::PR_COMMIT_CACHE_LIMIT;
            tab.pr_commits = github.pr_commits(&tab.repo_root, number, limit)?;
            tab.seed_last_diff_head_oid_from_local_ref();
        }
        let files: Vec<&DiffFile> = tab.files.iter().collect();
        let preview_keys = preview_keys(&files, tab.preview_context_key(), |path| {
            tab.file_preview_key(path)
        });
        observe(
            Read {
                files,
                lazy: tab.lazy_mode,
                hashes: (&tab.diff_hash, &tab.branch_diff_hash),
                reviewed: &tab.reviewed,
                ai: &tab.ai,
                pr: (
                    tab.pr_data.as_ref(),
                    &tab.pr_commits,
                    tab.last_diff_head_oid.as_deref(),
                ),
                preview_keys,
                stamps: (&tab.mtime_cache, &tab.preview_blob_ids),
            },
            &er_dir,
        )
    })
}

/// The Workspace's side: the same tab loaded under a key, then refreshed.
fn through_the_workspace(
    kind: &TabKind,
    mode: DiffMode,
    refreshes: usize,
    (root, repo): (&Path, &str),
) -> anyhow::Result<Observed> {
    er_engine::storage::with_process_test_storage_root(root.join("service-storage"), || {
        let ports = ports();
        let er_dir = write_fixture(&resolved(kind, repo, mode, &ports)?, &*ports.git)?;
        let handle = er_service::spawn(WorkspaceDeps::new(ports));
        let key = TabKey::next();
        let reply = handle.send_and_wait_with(
            Command::LoadTab {
                key,
                tab: descriptor(kind, repo),
                mode: Some(mode),
            },
            WAIT,
        )?;
        anyhow::ensure!(
            matches!(reply, Reply::ContentLoaded { .. }),
            "unexpected reply: {reply:?}"
        );
        for _ in 1..refreshes {
            let refresh = Command::RefreshDiff {
                key,
                force: false,
                adopt_head_oid: None,
            };
            handle.send_and_wait_with(refresh, WAIT)?;
        }
        // A `TabState` refresh parses its selected file, the first; the
        // desktop's file routes ask for the shown file's parse instead.
        let first = handle
            .view()
            .tab(key)
            .and_then(|content| content.diff.files.first().map(|file| file.path.clone()));
        let paths = first.into_iter().collect();
        let parse = Command::ParseFiles {
            key,
            paths,
            expand: false,
        };
        handle.send_and_wait_with(parse, WAIT)?;
        let view = handle.view();
        let content: &TabContent = view
            .tab(key)
            .ok_or_else(|| anyhow::anyhow!("no content under the loaded key"))?;
        anyhow::ensure!(content.context.er_dir == er_dir, "the load left its bucket");
        let pr = &content.pr;
        let files: Vec<&DiffFile> = content.diff.files.iter().map(Arc::as_ref).collect();
        let inputs = content.preview_key_inputs();
        let context = preview_key::preview_context_key(&inputs);
        let preview_keys = preview_keys(&files, context, |path| {
            preview_key::file_preview_key(&inputs, path)
        });
        let stamps = &content.diff.meta.stamps;
        observe(
            Read {
                files,
                lazy: content.diff.meta.lazy,
                hashes: (
                    &content.diff.meta.diff_hash,
                    &content.diff.meta.branch_diff_hash,
                ),
                reviewed: &content.reviewed.marks,
                ai: &content.ai.state,
                pr: (pr.overview.as_ref(), &pr.commits, pr.head_oid.as_deref()),
                preview_keys,
                stamps: (&stamps.mtimes, &stamps.blob_ids),
            },
            &er_dir,
        )
    })
}

/// The first row where two file lists part, so a failure names one file
/// rather than printing six hundred.
fn first_difference(service: &[FileRow], app: &[FileRow]) -> Option<String> {
    if service.len() != app.len() {
        return Some(format!("{} files against {}", service.len(), app.len()));
    }
    let (s, a) = service.iter().zip(app).find(|(s, a)| s != a)?;
    Some(format!("{s:?} against {a:?}"))
}

/// Compares section by section, so a failure names the section that moved.
fn assert_sections(service: &Observed, app: &Observed, case: &str) {
    if let Some(difference) = first_difference(&service.files, &app.files) {
        panic!("{case}: the files differ: {difference}");
    }
    assert_eq!(service.lazy, app.lazy, "{case}: the lazy flag differs");
    assert_eq!(service.hashes, app.hashes, "{case}: the diff hashes differ");
    assert_eq!(
        service.visible, app.visible,
        "{case}: the visible files differ"
    );
    assert_eq!(
        service.reviewed, app.reviewed,
        "{case}: the reviewed marks differ"
    );
    assert_eq!(
        service.saved_reviewed, app.saved_reviewed,
        "{case}: the saved marks differ"
    );
    assert_eq!(service.ai, app.ai, "{case}: the AI state differs");
    assert_eq!(service.tour, app.tour, "{case}: the tour differs");
    assert_eq!(
        service.staleness, app.staleness,
        "{case}: the staleness differs"
    );
    assert_eq!(service.pr, app.pr, "{case}: the PR section differs");
    // The keys read the stamps, so the stamps go first.
    let same_stamps = service.stamps == app.stamps;
    assert!(same_stamps, "{case}: the preview stamps differ");
    assert_eq!(
        service.preview_keys, app.preview_keys,
        "{case}: the preview keys differ"
    );
}

/// Every section the fixture fills must hold something on the App's side,
/// or a section both readers dropped would pass.
fn assert_filled(app: &Observed, kind: &TabKind, case: &str) {
    let stubs = app.files.iter().filter(|f| f.3 == 0 && !f.4).count();
    assert!(
        app.lazy && stubs > 0,
        "{case}: the fixture left no lazy stub"
    );
    let [risky, unreviewed] = [&app.visible[0], &app.visible[1]];
    assert_eq!(risky, &["src/a.rs".to_string()], "{case}: the risk filter");
    let filtered = !unreviewed.is_empty() && unreviewed.len() < app.files.len();
    assert!(filtered, "{case}: the unreviewed toggle filtered nothing");
    assert!(!app.reviewed.is_empty(), "{case}: no mark survived");
    assert!(app.ai[0] != Value::Null, "{case}: the review did not load");
    assert!(app.tour != Value::Null, "{case}: the tour did not load");
    assert!(app.staleness.0, "{case}: the older review was not stale");
    let document = app
        .preview_keys
        .iter()
        .find(|(path, _)| path == "docs/new.md");
    let keyed = document.is_some_and(|(_, key)| !key.is_empty());
    assert!(keyed, "{case}: the document has no preview key");
    assert!(!app.stamps.0.is_empty(), "{case}: no mtime was stamped");
    if *kind == TabKind::LocalPr {
        let (overview, commits, head) = &app.pr;
        assert!(overview.is_some(), "{case}: no PR overview");
        assert!(!commits.is_empty(), "{case}: no PR commits");
        assert!(head.is_some(), "{case}: no PR head");
    }
}

fn assert_parity(kind: TabKind, mode: DiffMode) -> anyhow::Result<()> {
    // One refresh is the restore's first load; a second one sees the AI
    // state the first loaded, which is when a working-tree view fetches the
    // branch diff.
    for refreshes in [1, 2] {
        let case = format!("{kind:?} in {mode:?} after {refreshes} refresh(es)");
        let (root, repo) = temp_repo()?;
        let checkout = (root.path(), repo.as_str());
        let app = through_tab_state(&kind, mode, refreshes, checkout)?;
        let service = through_the_workspace(&kind, mode, refreshes, checkout)?;
        assert_sections(&service, &app, &case);
        assert_filled(&app, &kind, &case);
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
fn a_plain_tab_loads_the_same_tour() -> anyhow::Result<()> {
    assert_parity(TabKind::Working, DiffMode::Tour)
}

#[test]
fn a_local_pr_tab_loads_the_same_pr_diff() -> anyhow::Result<()> {
    assert_parity(TabKind::LocalPr, DiffMode::PrDiff)
}
