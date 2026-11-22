//! Parity between the two readers of a tab: the Workspace's content load and
//! a `TabState` refresh read one fixture into the same sections, in each mode.
//! History, Conflicts and Hidden load on a switch from the branch view, on
//! both sides, as the page reaches them. A section is everything the snapshot reads from
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

use er_engine::ai::{ErChecklist, ErGithubComments, ErNotes, ErQuestions, ErReview, ErTour};
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

/// The fixture's diff. A lazy one is over the lazy-parse size, with more
/// files than a lazy diff parses up front, so every file starts as a stub;
/// an eager one parses every file.
fn raw(lazy: bool) -> String {
    let mut raw = HEAD.to_string();
    if !lazy {
        return raw;
    }
    let filler = "x".repeat(120);
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
/// The commit the fetched `refs/er/pr/12/head` names, which a restored PR
/// tab takes as the head its diff was built from.
const PR_HEAD: &str = "1234567890abcdef1234567890abcdef12345678";

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

/// A commit on the branch, with the diffstat its log line carries.
fn logged(hash: &str) -> er_engine::git::CommitInfo {
    er_engine::git::CommitInfo {
        subject: format!("subject {hash}"),
        author: "you".to_string(),
        date: "2026-11-01T00:00:00Z".to_string(),
        adds: 4,
        dels: 2,
        diffstat_known: true,
        ..commit(hash)
    }
}

/// The newest commit's diff, which History shows first.
const COMMIT: &str = "\
diff --git a/src/a.rs b/src/a.rs
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,2 +1,2 @@
 a-one
-a-old
+a-new
diff --git a/docs/new.md b/docs/new.md
--- a/docs/new.md
+++ b/docs/new.md
@@ -1 +1 @@
-old
+moved
";

/// A merge changeset: `src/a.rs` is staged and still conflicted in the
/// working tree.
const CONFLICTS: &str = "\
diff --git a/src/a.rs b/src/a.rs
--- a/src/a.rs
+++ b/src/a.rs
@@ -1 +1 @@
-a-old
+a-staged
diff --git a/src/b.rs b/src/b.rs
--- a/src/b.rs
+++ b/src/b.rs
@@ -1 +1 @@
-b-old
+b-new
diff --git a/src/a.rs b/src/a.rs
--- a/src/a.rs
+++ b/src/a.rs
@@ -1 +1 @@
-a-old
+<<<<<<< ours
";

fn ports(lazy: bool) -> Ports {
    let raw = raw(lazy);
    let git: Arc<dyn er_git::Git> = Arc::new(
        FakeGit::default()
            .with_current_branch(Ok("feature".to_string()))
            .with_detect_base_branch(Ok("main".to_string()))
            .with_diff_shortstat((2, 5, 1))
            .with_commit_shortstats(HashMap::new())
            .with_diff_raw(Ok(raw.clone()))
            // A local PR tab's branch view diffs its checkout against the base.
            .with_diff_checkout_against_base(Ok(raw.clone()))
            .with_log_range(Ok(vec![logged("c2"), logged("c1")]))
            .with_unpushed_shas(["c2".to_string()].into_iter().collect())
            .with_diff_commit(Ok(COMMIT.to_string()))
            .with_diff_conflicts(Ok(CONFLICTS.to_string()))
            .with_unmerged_files(Ok(vec!["src/a.rs".to_string()]))
            .with_is_merge_in_progress(true)
            .with_remote_url(Ok("https://github.com/acme/widgets.git".to_string()))
            .with_rev_parse_verify(Ok(Some(PR_HEAD.to_string())))
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
#[derive(Debug, Clone)]
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
    /// The fields `diff(tab)` and `reviewed(tab)` carry, as the snapshot
    /// names them: from the snapshot on the App's side, from the resource
    /// builders on the other.
    resource: Value,
    /// The snapshot's `ai`, `tour` and `other_bucket` on the App's side, `annotations(tab)`
    /// on the other.
    annotations: Value,
}

/// The fields of a snapshot, or of a `diff(tab)`, that both must agree on.
/// The review bucket is a storage path, which each side roots apart.
fn resource_fields(value: &Value) -> Value {
    const FIELDS: [&str; 17] = [
        "mode",
        "branch",
        "base",
        "total_count",
        "diff_stats",
        "filter",
        "filter_suggestions",
        "preview_context_key",
        "local_branch",
        "local_branch_checked_out",
        "unstaged_stat",
        "staged_stat",
        "pr_diff_head_oid",
        "built_from_head_oid",
        "commits",
        "selected_commit_sha",
        // From `reviewed(tab)` on the service's side.
        "reviewed_count",
    ];
    const ROW: [&str; 19] = [
        "path",
        "status",
        "old_path",
        "similarity",
        "kind",
        "additions",
        "deletions",
        "reviewed",
        "compacted",
        "risk",
        "risk_tier",
        "risk_rule",
        "finding_count",
        "comment_count",
        "question_count",
        "is_lazy_stub",
        "source_index",
        "cache_key",
        "preview_key",
    ];
    // A field one side leaves out reads as null on the other.
    fn without_nulls(value: Value) -> Value {
        match value {
            Value::Object(map) => map
                .into_iter()
                .filter(|(_, v)| !v.is_null())
                .map(|(k, v)| (k, without_nulls(v)))
                .collect::<serde_json::Map<_, _>>()
                .into(),
            Value::Array(items) => items.into_iter().map(without_nulls).collect(),
            other => other,
        }
    }
    let pick = |value: &Value, names: &[&str]| -> Value {
        names
            .iter()
            .map(|name| {
                (
                    (*name).to_string(),
                    value.get(*name).cloned().unwrap_or(Value::Null),
                )
            })
            .collect::<serde_json::Map<_, _>>()
            .into()
    };
    let mut picked = pick(value, &FIELDS);
    let rows: Vec<Value> = value
        .get("files")
        .and_then(Value::as_array)
        .map(|files| files.iter().map(|row| pick(row, &ROW)).collect())
        .unwrap_or_default();
    if let Some(map) = picked.as_object_mut() {
        map.insert("files".into(), rows.into());
    }
    without_nulls(picked)
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
    resource: Value,
    annotations: Value,
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
        resource: resource_fields(&read.resource),
        annotations: read.annotations,
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
/// question on a moved line, a note, a note's reply ordered before the comment's own
/// reply, and marks: one current, one changed, one orphaned. A local PR tab's other view
/// gets its own bucket, by `write_other_bucket`. `current` is the hash of the diff on screen.
fn write_fixture(
    resolved: &ResolvedTab,
    git: &dyn er_git::Git,
    current: &str,
) -> anyhow::Result<String> {
    let id = resolved.identity();
    let er_dir = id
        .managed_root(git)
        .ok_or_else(|| anyhow::anyhow!("the fixture's bucket did not resolve"))?
        .er_dir();
    let dir = Path::new(&er_dir);
    std::fs::create_dir_all(dir)?;
    let reviewed = FRESH_REVIEW.with(|fresh| fresh.borrow().clone());
    let reviewed = reviewed.as_deref().unwrap_or("older");
    write_sidecar_json::<ErReview>(
        dir,
        json!({
            "version": 1, "diff_hash": reviewed, "created_at": "t0",
            "files": {
                "src/a.rs": { "risk": "high", "findings": [{
                    "id": "f-1", "severity": "high", "title": "claim",
                    "hunk_index": 0, "line_start": 2,
                    "responses": [{ "id": "r-1", "timestamp": "t1", "text": "Checked" }]
                }] },
                "src/b.rs": { "risk": "low", "findings": [{
                    "id": "f-2", "severity": "medium", "title": "fixed",
                    "hunk_index": 0, "line_start": 9, "resolved": true
                }] }
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
            }, {
                "id": "q-reply", "file": "src/b.rs", "hunk_index": 0, "line_start": 9,
                "line_content": "b-new", "text": "because", "in_reply_to": "q-painted",
                "author": "ai", "timestamp": "t2", "ai_action": "ask"
            }]
        }),
    );
    write_sidecar_json::<ErNotes>(
        dir,
        json!({
            "version": 1, "diff_hash": "older", "notes": [{
                "id": "n-1", "file": "src/a.rs", "hunk_index": 0, "line_start": 2,
                "line_content": "a-new", "text": "check", "resolved": false
            }, {
                "id": "n-reply", "file": "src/a.rs", "text": "Still off",
                "in_reply_to": "c-1", "timestamp": "t4"
            }]
        }),
    );
    // A local PR tab reads its GitHub comments from the PR bucket in every
    // view, so they go there.
    let comments_dir = id.pr_bucket_er_dir(git).unwrap_or_else(|| er_dir.clone());
    std::fs::create_dir_all(&comments_dir)?;
    write_sidecar_json::<ErGithubComments>(
        Path::new(&comments_dir),
        json!({ "version": 1, "diff_hash": "older", "comments": [{
            "id": "c-1", "file": "src/a.rs", "hunk_index": 0, "line_start": 2,
            "line_content": "a-new", "comment": "Looks off", "source": "github",
            "author": "octo", "github_id": 7, "finding_ref": "f-1"
        }, {
            "id": "c-2", "file": "src/a.rs", "comment": "Agreed", "source": "local",
            "in_reply_to": "c-1", "timestamp": "t3"
        }] }),
    );
    write_sidecar_json::<ErChecklist>(
        dir,
        json!({ "version": 1, "diff_hash": "older", "items": [{
            "id": "k-1", "text": "Check the auth path", "category": "auth",
            "related_files": ["src/a.rs"], "related_findings": ["f-1"]
        }] }),
    );
    write_sidecar_json::<er_engine::ai::TriageReview>(
        dir,
        json!({
            "version": 1, "diff_hash": reviewed, "created_at": "t0",
            "first_impression": "Small", "diff_stats": {
                "files_changed": 2, "approx_risk": "medium", "domains": ["api"]
            },
            "verdict": { "primary": "general", "experts": [], "rationale": "r", "confidence": "high" },
            "priority_files": [{ "path": "src/b.rs", "reason": "entry", "risk": "low" }]
        }),
    );
    er_engine::test_support::write_sidecar_json_slot::<er_engine::ai::ErDiagram>(
        dir,
        er_store::DiagramId::new("flows")?,
        json!({ "version": 1, "diff_hash": "older", "kind": "flows", "title": "Flow",
                "mermaid": "graph TD; a-->b" }),
    );
    let promoted = [("f-1".to_string(), "77".to_string())]
        .into_iter()
        .collect();
    er_engine::ai::save_finding_promotions(&er_dir, &promoted)?;
    if let Some(tour_dir) = id.tour_bucket_er_dir(git) {
        std::fs::create_dir_all(&tour_dir)?;
        write_sidecar_json::<ErTour>(Path::new(&tour_dir), tour_fixture());
    }
    if let Some(other_dir) = id.cross_tour_bucket_er_dir(git) {
        write_other_bucket(Path::new(&other_dir), id.tour_context_is_pr(), current)?;
    }
    let b_hash = er_engine::ai::compute_per_file_hash(HEAD, "src/b.rs").unwrap_or_default();
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

/// The tour both a tab's own bucket and its other view's bucket hold, so the
/// one on screen is the other bucket's file.
fn tour_fixture() -> Value {
    json!({
        "version": 1, "diff_hash": "older", "title": "Walk", "created_at": "t5",
        "pillars": [{ "id": "p1", "title": "P1", "files": [{
            "path": "src/a.rs", "reason": "core",
            "related": [{ "path": "src/b.rs", "kind": "test", "reason": "covers it" }]
        }, { "path": "src/gone.rs", "reason": "left the diff" }] }]
    })
}

/// The open findings the other view's bucket holds, two to the shown
/// bucket's one, so reading the wrong bucket changes the count.
const OTHER_FINDINGS: u64 = 2;

/// A local PR tab's other view: a triage of the diff on screen, two open
/// findings and the shared tour. The branch bucket, the PR diff's other view,
/// has its findings from an expert and no `review.json`, so its generation
/// hash comes from the newest sidecar; the PR bucket has a `review.json`.
fn write_other_bucket(dir: &Path, other_is_branch: bool, current: &str) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    write_sidecar_json::<ErTour>(dir, tour_fixture());
    write_sidecar_json::<er_engine::ai::TriageReview>(
        dir,
        json!({
            "version": 1, "diff_hash": current, "created_at": "t6",
            "first_impression": "Other", "diff_stats": {
                "files_changed": 2, "approx_risk": "low", "domains": []
            },
            "verdict": { "primary": "general", "experts": [], "rationale": "r", "confidence": "high" },
            "priority_files": []
        }),
    );
    let finding = |id: &str, line: u32| {
        json!({
            "id": id, "severity": "medium", "category": "logic",
            "title": format!("finding {id}"), "description": "d",
            "hunk_index": 0, "line_start": line, "line_end": line
        })
    };
    // A review's file names its risk; an expert's does not.
    let files = json!({ "src/a.rs": {
        "risk": "low", "findings": [finding("o-1", 2), finding("o-2", 3)]
    } });
    if other_is_branch {
        er_engine::test_support::write_sidecar_json_slot::<er_engine::ai::ExpertReview>(
            dir,
            er_store::ExpertId::new("security")?,
            json!({
                "version": 1, "expert_id": "security", "diff_hash": current,
                "created_at": "t7", "files": files
            }),
        );
    } else {
        write_sidecar_json::<ErReview>(
            dir,
            json!({
                "version": 1, "diff_hash": current, "created_at": "t7", "files": files
            }),
        );
    }
    Ok(())
}

thread_local! {
    /// The diff hash the fixture's review and triage name, unless a test sets the on-screen one.
    static FRESH_REVIEW: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
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

/// The fixture in the bucket of the view `steps` end on, when they end on
/// another bucket than the one the tab opened on.
fn shown_fixture(
    (kind, mode): (&TabKind, DiffMode),
    repo: &str,
    steps: &[DiffMode],
    ports: &Ports,
    current: &str,
) -> anyhow::Result<Option<String>> {
    let Some(&last) = steps.last() else {
        return Ok(None);
    };
    let shown = resolved(kind, repo, last, ports)?;
    let opened = resolved(kind, repo, mode, ports)?;
    if shown.identity().review_bucket() == opened.identity().review_bucket() {
        return Ok(None);
    }
    write_fixture(&shown, &*ports.git, current).map(Some)
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
/// times, then switched through `steps`. The first refresh sees no AI state
/// yet, as on a restore. A PR tab then reads what the App's other paths fill:
/// the overview the comment sync reads, the commits a PR open reads, and the
/// head a restore seeds from the fetched ref.
fn through_tab_state(
    (kind, mode): (&TabKind, DiffMode),
    refreshes: usize,
    lazy: bool,
    (root, repo): (&Path, &str),
    steps: &[DiffMode],
) -> anyhow::Result<Observed> {
    er_engine::storage::with_process_test_storage_root(root.join("app-storage"), || {
        let ports = ports(lazy);
        let (git, github) = (Arc::clone(&ports.git), Arc::clone(&ports.github));
        let resolved = resolved(kind, repo, mode, &ports)?;
        let current = er_engine::ai::compute_diff_hash(&raw(lazy));
        let er_dir = write_fixture(&resolved, &*ports.git, &current)?;
        let shown_dir = shown_fixture((kind, mode), repo, steps, &ports, &current)?;
        let mut tab = TabState::from_resolved(resolved, ports)?;
        // The running App's watcher sets the checkout root before a refresh,
        // by the rule the workspace's load runs on every load.
        let branch = er_engine::checkout::watched_branch(
            tab.is_remote(),
            tab.local_branch_view.as_deref(),
            &tab.current_branch,
        );
        tab.local_branch_checkout_root = branch.and_then(|branch| {
            er_engine::checkout::resolve_head_checkout(&*git, &tab.repo_root, &branch)
        });
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
        for step in steps {
            tab.set_mode(*step);
        }
        let er_dir = shown_dir.unwrap_or(er_dir);
        let mut app = er_engine::app::App::new_for_test(Vec::new());
        app.tabs = vec![tab];
        app.active_tab = 0;
        let snapshot = crate::snapshot::build_snapshot_with_delta(
            &app,
            &crate::snapshot::SnapshotSources::default(),
        );
        let mut resource = serde_json::to_value(&snapshot)?;
        let annotations = json!({
            "ai": resource["ai"],
            "tour": resource["tour"],
            "other_bucket": resource["other_bucket"],
        });
        let tab = app.tab();
        // The snapshot names no built-from head: the stale pill compares the
        // tab's `last_diff_head_oid` with the PR head, so that stands in.
        if let Some(map) = resource.as_object_mut() {
            let built_from = tab
                .last_diff_head_oid
                .clone()
                .filter(|_| tab.pr_number.is_some());
            map.insert("built_from_head_oid".into(), json!(built_from));
        }
        let files: Vec<&DiffFile> = tab.active_diff_files().iter().collect();
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
                resource,
                annotations,
            },
            &er_dir,
        )
    })
}

/// The Workspace's side: the same tab loaded under a key, refreshed, then
/// switched through `steps`.
fn through_the_workspace(
    (kind, mode): (&TabKind, DiffMode),
    refreshes: usize,
    lazy: bool,
    (root, repo): (&Path, &str),
    steps: &[DiffMode],
) -> anyhow::Result<Observed> {
    er_engine::storage::with_process_test_storage_root(root.join("service-storage"), || {
        let ports = ports(lazy);
        let current = er_engine::ai::compute_diff_hash(&raw(lazy));
        let er_dir = write_fixture(&resolved(kind, repo, mode, &ports)?, &*ports.git, &current)?;
        let shown_dir = shown_fixture((kind, mode), repo, steps, &ports, &current)?;
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
        // desktop's file routes ask for the shown file's parse instead, and
        // only while it is a stub.
        let stub = handle.view().tab(key).and_then(|content| {
            let first = content.diff.files.first()?;
            let is_stub = content.diff.meta.lazy && first.hunks.is_empty() && !first.compacted;
            is_stub.then(|| first.path.clone())
        });
        if let Some(path) = stub {
            let parse = Command::ParseFiles {
                key,
                paths: vec![path],
                expand: false,
            };
            handle.send_and_wait_with(parse, WAIT)?;
        }
        for step in steps {
            let switch = Command::SetDiffMode {
                key,
                mode: *step,
                pr_number: None,
            };
            handle.send_and_wait_with(switch, WAIT)?;
        }
        let er_dir = shown_dir.unwrap_or(er_dir);
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
        let mut resource = serde_json::to_value(er_service::diff_resource(
            content,
            &er_service::DiffInputs::default(),
        ))?;
        resource["reviewed_count"] = json!(er_service::reviewed_resource(content).reviewed_count);
        anyhow::ensure!(
            resource.get("built_from_head_oid") == Some(&json!(pr.head_oid)),
            "diff(tab) was not built from the content's PR head: {:?} against {:?}",
            resource.get("built_from_head_oid"),
            pr.head_oid
        );
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
                resource,
                annotations: serde_json::to_value(er_service::annotations_resource(content))?,
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
    // Which files are stubs follows from the lazy flag, so it goes first.
    assert_eq!(service.lazy, app.lazy, "{case}: the lazy flag differs");
    if let Some(difference) = first_difference(&service.files, &app.files) {
        panic!("{case}: the files differ: {difference}");
    }
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
    assert_annotations(&service.annotations, &app.annotations, case);
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
    if service.resource != app.resource {
        let field = |v: &Value, k: &str| v.get(k).cloned().unwrap_or(Value::Null);
        let keys = app
            .resource
            .as_object()
            .map(|m| m.keys().cloned().collect::<Vec<_>>());
        let differing: Vec<String> = keys
            .unwrap_or_default()
            .into_iter()
            .filter(|k| field(&service.resource, k) != field(&app.resource, k))
            .collect();
        panic!(
            "{case}: diff(tab) differs from the snapshot in {differing:?}:\n{}\nagainst\n{}",
            serde_json::json!(differing
                .iter()
                .map(|k| field(&service.resource, k))
                .collect::<Vec<_>>()),
            serde_json::json!(differing
                .iter()
                .map(|k| field(&app.resource, k))
                .collect::<Vec<_>>()),
        );
    }
}

/// `annotations(tab)` against the snapshot's `ai`, `tour` and `other_bucket`, naming the
/// fields that differ.
fn assert_annotations(service: &Value, app: &Value, case: &str) {
    for part in ["tour", "other_bucket"] {
        assert_eq!(
            service[part], app[part],
            "{case}: annotations' {part} differs"
        );
    }
    let (s, a) = (&service["ai"], &app["ai"]);
    let fields = a
        .as_object()
        .map(|map| map.keys().cloned().collect::<Vec<_>>());
    let differing: Vec<String> = fields
        .unwrap_or_default()
        .into_iter()
        .filter(|k| s[k.as_str()] != a[k.as_str()])
        .collect();
    assert!(
        differing.is_empty() && s == a,
        "{case}: annotations' ai differs in {differing:?}:\n{}\nagainst\n{}",
        json!(differing.iter().map(|k| &s[k.as_str()]).collect::<Vec<_>>()),
        json!(differing.iter().map(|k| &a[k.as_str()]).collect::<Vec<_>>()),
    );
}

/// Every section the fixture fills must hold something on the App's side,
/// or a section both readers dropped would pass. AI, staleness and stamps are
/// checked in part: the review, the stale flag and the mtimes.
fn assert_filled(app: &Observed, (kind, mode): (&TabKind, DiffMode), lazy: bool, case: &str) {
    let stubs = app.files.iter().filter(|f| f.3 == 0 && !f.4).count();
    if lazy {
        assert!(
            app.lazy && stubs > 0,
            "{case}: the fixture left no lazy stub"
        );
    } else {
        assert!(
            !app.lazy && stubs == 0,
            "{case}: the eager diff left a stub"
        );
    }
    let [risky, unreviewed] = [&app.visible[0], &app.visible[1]];
    assert_eq!(risky, &["src/a.rs".to_string()], "{case}: the risk filter");
    let filtered = !unreviewed.is_empty() && unreviewed.len() < app.files.len();
    assert!(filtered, "{case}: the unreviewed toggle filtered nothing");
    assert!(!app.hashes.0.is_empty(), "{case}: the diff hash is empty");
    assert!(!app.reviewed.is_empty(), "{case}: no mark survived");
    let orphan = app.reviewed.contains_key("src/gone.rs");
    assert!(!orphan, "{case}: the orphaned mark was kept");
    // The fixture saved the orphan, so saved marks equal to the live ones
    // were pruned and saved again.
    assert_eq!(
        app.saved_reviewed, app.reviewed,
        "{case}: the pruned marks were not saved"
    );
    assert!(app.ai[0] != Value::Null, "{case}: the review did not load");
    let ai = &app.annotations["ai"];
    for part in ["checklist", "triage"] {
        assert!(ai[part].is_object(), "{case}: no {part} in the annotations");
    }
    for part in ["threads", "findings", "resolved_findings", "diagrams"] {
        let listed = ai[part].as_array().is_some_and(|rows| !rows.is_empty());
        assert!(listed, "{case}: no {part} in the annotations");
    }
    assert!(
        app.annotations["tour"]["available"] == true,
        "{case}: no tour"
    );
    // The note's reply to c-1 is read before the comment's, at a later time.
    let thread = ai["threads"]
        .as_array()
        .and_then(|threads| threads.iter().find(|t| t["id"] == "c-1"));
    let replies: Vec<&Value> = thread
        .and_then(|t| t["replies"].as_array())
        .map(|rows| rows.iter().map(|r| &r["id"]).collect())
        .unwrap_or_default();
    assert_eq!(
        replies,
        ["c-2", "n-reply"],
        "{case}: c-1's replies, oldest first"
    );
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
        assert_eq!(head.as_deref(), Some(PR_HEAD), "{case}: no PR head");
        // The parity drops nulls, so a head both sides lost would pass it.
        // `pr_diff_head_oid` stays null here: a restored local PR tab has no
        // remote slug to read a preview head with, on either side.
        // Every fact differs from its default, so a fact both sides dropped
        // fails here, and the finding count tells the two buckets apart.
        let view = if mode == DiffMode::PrDiff {
            "branch"
        } else {
            "pr"
        };
        assert_eq!(
            app.annotations["other_bucket"],
            json!({
                "view": view, "has_triage": true, "has_review": true,
                "finding_count": OTHER_FINDINGS, "pillar_count": 1,
                "tour_shared": true, "same_diff": true,
            }),
            "{case}: the other bucket's summary"
        );
        let built_from = app.resource.get("built_from_head_oid");
        let built_from = built_from.and_then(Value::as_str);
        assert_eq!(built_from, Some(PR_HEAD), "{case}: no built-from head");
    }
}

fn assert_parity(kind: TabKind, mode: DiffMode) -> anyhow::Result<()> {
    // One refresh is the restore's first load; a second one sees the AI
    // state the first loaded, which is when a working-tree view fetches the
    // branch diff. An eager diff relocates anchors in parsed files; a lazy
    // one leaves stubs.
    for lazy in [false, true] {
        for refreshes in [1, 2] {
            let size = if lazy { "lazy" } else { "eager" };
            let case = format!("{kind:?} in {mode:?}, {size}, after {refreshes} refresh(es)");
            let (root, repo) = temp_repo()?;
            let checkout = (root.path(), repo.as_str());
            let app = through_tab_state((&kind, mode), refreshes, lazy, checkout, &[])?;
            let service = through_the_workspace((&kind, mode), refreshes, lazy, checkout, &[])?;
            assert_sections(&service, &app, &case);
            assert_filled(&app, (&kind, mode), lazy, &case);
        }
    }
    Ok(())
}

/// A review of the diff on screen regrades triage's call; both readers must agree on it.
#[test]
fn a_review_of_the_diff_on_screen_regrades_triage_alike() -> anyhow::Result<()> {
    let hash = er_engine::ai::compute_diff_hash(&raw(false));
    FRESH_REVIEW.with(|fresh| *fresh.borrow_mut() = Some(hash));
    let (root, repo) = temp_repo()?;
    let view = (&TabKind::Working, DiffMode::Branch);
    let app = through_tab_state(view, 1, false, (root.path(), &repo), &[]);
    let service = through_the_workspace(view, 1, false, (root.path(), &repo), &[]);
    FRESH_REVIEW.with(|fresh| fresh.borrow_mut().take());
    let (app, service) = (app?, service?);
    assert_annotations(&service.annotations, &app.annotations, "a fresh review");
    let update = &app.annotations["ai"]["triage"]["review_update"];
    assert!(update.is_object(), "the review did not regrade triage");
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

/// The branch view of a local PR tab, whose other view is the PR bucket.
#[test]
fn a_local_pr_tab_loads_the_same_branch_view() -> anyhow::Result<()> {
    assert_parity(TabKind::LocalPr, DiffMode::Branch)
}

/// `observed` without what a view with its own diff does not share across
/// the two readers. Neither reads the preview stamps there: the App keeps the
/// ones the branch view took, the service takes the view's own. Conflicts
/// and Hidden leave out the preview keys too: the App keys them on the diff
/// the tab showed before, which it keeps in those views, and the service on
/// the view's own diff. They leave out `relocated_at_hash` on a note with no
/// line anchor as well: these views skip relocation, so the App shows the
/// stamp its branch load set in memory and never wrote back, and the service
/// the bucket as stored. desktop-ui does not read the field.
fn in_own_view(mut observed: Observed, keyed: bool) -> Observed {
    observed.stamps = Default::default();
    if !keyed {
        observed.preview_keys.clear();
        if let Some(map) = observed.resource.as_object_mut() {
            map.remove("preview_context_key");
            if let Some(Value::Array(rows)) = map.get_mut("files") {
                for row in rows.iter_mut().filter_map(Value::as_object_mut) {
                    row.remove("preview_key");
                }
            }
        }
        if let Some(Value::Array(notes)) = observed.ai[2].get_mut("notes") {
            let unanchored = notes.iter_mut().filter(|n| n["line_start"].is_null());
            for note in unanchored.filter_map(Value::as_object_mut) {
                note.remove("relocated_at_hash");
            }
        }
    }
    observed
}

/// A plain tab loaded on the branch view, then switched through `steps`, on
/// both sides.
fn switched(steps: &[DiffMode]) -> anyhow::Result<(Observed, Observed)> {
    let (root, repo) = temp_repo()?;
    let checkout = (root.path(), repo.as_str());
    let opened = (&TabKind::Working, DiffMode::Branch);
    let app = through_tab_state(opened, 1, false, checkout, steps)?;
    let service = through_the_workspace(opened, 1, false, checkout, steps)?;
    Ok((app, service))
}

fn listed(observed: &Observed) -> Vec<&str> {
    observed.files.iter().map(|f| f.0.as_str()).collect()
}

fn scroller(observed: &Observed) -> (usize, Option<&str>) {
    let commits = observed.resource.get("commits").and_then(Value::as_array);
    let selected = observed.resource.get("selected_commit_sha");
    (
        commits.map_or(0, Vec::len),
        selected.and_then(Value::as_str),
    )
}

#[test]
fn a_plain_tab_loads_the_same_history() -> anyhow::Result<()> {
    let (app, service) = switched(&[DiffMode::History])?;
    assert_sections(
        &in_own_view(service, true),
        &in_own_view(app.clone(), true),
        "History",
    );
    assert_eq!(
        listed(&app),
        ["src/a.rs", "docs/new.md"],
        "History: the commit's files"
    );
    assert_eq!(scroller(&app), (2, Some("c2")), "History: the commit list");
    assert!(
        !app.reviewed.is_empty(),
        "History: the History bucket's marks"
    );
    assert!(
        app.ai[0] != Value::Null,
        "History: the History bucket's review"
    );
    let document = app
        .preview_keys
        .iter()
        .find(|(path, _)| path == "docs/new.md");
    assert!(
        document.is_some_and(|(_, key)| !key.is_empty()),
        "History: no preview key"
    );
    Ok(())
}

#[test]
fn a_plain_tab_loads_the_same_conflicts() -> anyhow::Result<()> {
    let (app, service) = switched(&[DiffMode::Conflicts])?;
    assert_sections(
        &in_own_view(service, false),
        &in_own_view(app.clone(), false),
        "Conflicts",
    );
    assert_eq!(
        listed(&app),
        ["src/a.rs", "src/b.rs"],
        "Conflicts: the merge's files"
    );
    assert_eq!(scroller(&app), (2, None), "Conflicts: the commit list");
    Ok(())
}

#[test]
fn a_plain_tab_loads_the_same_hidden_view() -> anyhow::Result<()> {
    let (app, service) = switched(&[DiffMode::Hidden])?;
    assert_sections(
        &in_own_view(service, false),
        &in_own_view(app.clone(), false),
        "Hidden",
    );
    assert!(app.files.is_empty(), "Hidden lists no diff file");
    assert_eq!(scroller(&app), (2, None), "Hidden: the commit list");
    Ok(())
}

/// Back on the branch view after History, the two readers key previews the
/// same: History's selection names no commit outside History.
#[test]
fn the_preview_keys_after_a_history_visit_match() -> anyhow::Result<()> {
    let (app, service) = switched(&[DiffMode::History, DiffMode::Branch])?;
    let case = "Branch after History";
    assert_sections(&service, &app, case);
    assert_filled(&app, (&TabKind::Working, DiffMode::Branch), false, case);
    assert_eq!(scroller(&app), (2, None), "{case}: the commit list");
    Ok(())
}
