//! What a tab's content carries beyond the diff and its marks: the preview
//! stamps, the PR section, every mode, expanded files, and sidecars an outside
//! writer moved.

use std::path::Path;
use std::sync::mpsc::channel;
use std::sync::{Mutex, RwLock};

use er_engine::annotations::WrittenPayload;
use er_engine::preview_key::file_preview_key;

use super::*;
use crate::app_registry::AppRegistry;
use crate::content::{ReloadedAi, SidecarStamp};
use crate::sidecar_reload::SidecarReloads;
use crate::AppCommand;

fn calls_to(github: &FakeGitHub, method: &str) -> usize {
    let calls = github.calls();
    calls.iter().filter(|call| call.starts_with(method)).count()
}

fn send(handle: &WorkspaceHandle, command: Command) -> anyhow::Result<Reply> {
    Ok(handle.send_and_wait_with(command, WAIT)?)
}

fn set_mode(key: TabKey, mode: DiffMode) -> Command {
    Command::SetDiffMode {
        key,
        mode,
        pr_number: None,
    }
}

const RENAMED: &str = "\
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

#[test]
fn a_load_takes_the_preview_stamps_its_keys_read() -> anyhow::Result<()> {
    with_root(|repo| {
        std::fs::create_dir_all(Path::new(repo).join("docs"))?;
        std::fs::write(Path::new(repo).join("docs/new.md"), "moved\n")?;
        let blob = "b".repeat(40);
        let git = Arc::new(
            FakeGit::default()
                .with_current_branch(Ok("feature".to_string()))
                .with_diff_shortstat((0, 0, 0))
                .with_diff_raw(Ok(RENAMED.to_string()))
                .with_blob_oid(Ok(blob.clone())),
        );
        let handle = actor_over(git);
        let key = TabKey::next();
        load(&handle, key, repo)?;

        let content = content_of(&handle.view(), key)?;
        let stamps = &content.diff.meta.stamps;
        let mtime = std::fs::metadata(Path::new(repo).join("docs/new.md"))?.modified()?;
        assert_eq!(stamps.mtimes.get("docs/new.md"), Some(&mtime));
        assert_eq!(stamps.blob_ids.get("docs/new.md"), Some(&blob));
        let inputs = content.preview_key_inputs();
        assert!(!file_preview_key(&inputs, "docs/new.md").is_empty());
        Ok(())
    })
}

fn overview(title: &str) -> er_engine::github::PrOverviewData {
    er_engine::github::PrOverviewData {
        number: 7,
        title: title.to_string(),
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

fn remote_github() -> Arc<FakeGitHub> {
    Arc::new(
        FakeGitHub::default()
            .with_pr_diff_remote(Ok(RAW.to_string()))
            .with_pr_head_oid_remote(Ok("h1".to_string()))
            .with_overview_remote(Ok(Some(overview("From GitHub"))))
            .with_commits_remote(Ok(vec![commit("c1")])),
    )
}

fn remote_tab() -> TabDescriptor {
    TabDescriptor {
        kind: TabKind::RemotePr,
        repo_root: String::new(),
        pr_owner: Some("acme".to_string()),
        pr_repo: Some("widgets".to_string()),
        pr_number: Some(7),
        ..working_tab("")
    }
}

fn load_remote(handle: &WorkspaceHandle, key: TabKey) -> anyhow::Result<Reply> {
    let tab = remote_tab();
    let mode = None;
    send(handle, Command::LoadTab { key, tab, mode })
}

/// The overview and commits are read once; a refresh and a mode switch carry
/// them, and only a sync reads them again.
#[test]
fn a_pr_section_is_filled_once_and_read_again_only_on_a_sync() -> anyhow::Result<()> {
    with_root(|_| {
        let github = remote_github();
        let ports_github: Arc<dyn er_github::GitHub> = github.clone();
        let git: Arc<dyn er_git::Git> = git(RAW);
        let handle = spawn(WorkspaceDeps::new(Ports {
            git,
            github: ports_github,
        }));
        let key = TabKey::next();
        load_remote(&handle, key)?;

        let pr = content_of(&handle.view(), key)?.pr.clone();
        assert_eq!(pr.pr_number, Some(7));
        let title = pr.overview.as_ref().map(|o| o.title.as_str());
        assert_eq!(title, Some("From GitHub"));
        let hashes: Vec<&str> = pr.commits.iter().map(|c| c.hash.as_str()).collect();
        assert_eq!(hashes, ["c1"]);
        assert_eq!(pr.head_oid.as_deref(), Some("h1"));

        send(&handle, refresh(key))?;
        send(&handle, set_mode(key, DiffMode::Tour))?;
        let toured = content_of(&handle.view(), key)?;
        assert_eq!(toured.tab.mode, DiffMode::Tour);
        assert!(toured.tab.tour_is_pr, "a remote tab's tour left the PR");
        assert_eq!(calls_to(&github, "overview_remote("), 1);
        assert_eq!(calls_to(&github, "commits_remote("), 1);

        let sync = Command::RefreshDiff {
            key,
            force: true,
            adopt_head_oid: Some("h2".to_string()),
        };
        send(&handle, sync)?;
        assert_eq!(calls_to(&github, "overview_remote("), 2);
        assert_eq!(calls_to(&github, "commits_remote("), 2);
        let synced = content_of(&handle.view(), key)?;
        assert_eq!(synced.pr.head_oid.as_deref(), Some("h2"));
        Ok(())
    })
}

/// A PR the open cache already holds fills its section with no GitHub read.
#[test]
fn a_pr_section_comes_from_the_open_cache_when_a_prefetch_warmed_it() -> anyhow::Result<()> {
    with_root(|_| {
        let github = remote_github();
        let ports = Ports {
            git: git(RAW),
            github: github.clone(),
        };
        let registry = Arc::new(AppRegistry::new(ports.clone()));
        registry.send(AppCommand::PrefetchRemoteOpenTarget {
            owner: "acme".to_string(),
            repo: "widgets".to_string(),
            number: 7,
        })?;
        let deadline = Instant::now() + WAIT;
        while registry
            .remote_pr_open_entry("acme", "widgets", 7)
            .is_none()
        {
            anyhow::ensure!(Instant::now() < deadline, "the prefetch never landed");
            std::thread::yield_now();
        }
        let (overviews, commits) = (
            calls_to(&github, "overview_remote("),
            calls_to(&github, "commits_remote("),
        );
        // The registry lists the tab, as an open does before its load.
        registry.send(AppCommand::NewTab { tab: remote_tab() })?;
        let key = registry
            .tabs_view()
            .active
            .ok_or_else(|| anyhow::anyhow!("the tab did not open"))?;
        let handle = spawn(WorkspaceDeps::new(ports).with_app_registry(registry));
        load_remote(&handle, key)?;

        let pr = content_of(&handle.view(), key)?.pr.clone();
        assert!(pr.overview.is_some(), "the cached overview was not taken");
        assert_eq!(calls_to(&github, "overview_remote("), overviews);
        assert_eq!(calls_to(&github, "commits_remote("), commits);
        Ok(())
    })
}

const COMMIT_DIFF: &str = "\
diff --git a/src/c.rs b/src/c.rs
--- a/src/c.rs
+++ b/src/c.rs
@@ -1 +1 @@
-c
+c2
";

/// A merge changeset: `z.rs` is staged and still conflicted in the working
/// tree, so it appears twice.
const CONFLICTS: &str = "\
diff --git a/z.rs b/z.rs
--- a/z.rs
+++ b/z.rs
@@ -1 +1 @@
-z
+staged
diff --git a/z.rs b/z.rs
--- a/z.rs
+++ b/z.rs
@@ -1 +1 @@
-z
+<<<<<<< ours
diff --git a/a.rs b/a.rs
--- a/a.rs
+++ b/a.rs
@@ -1 +1 @@
-a
+a2
";

/// A checkout with two commits on the branch, a merge in progress and a
/// commit whose diff git answers.
fn views_git() -> Arc<FakeGit> {
    Arc::new(
        FakeGit::default()
            .with_current_branch(Ok("feature".to_string()))
            .with_diff_shortstat((0, 0, 0))
            .with_diff_raw(Ok(RAW.to_string()))
            .with_log_range(Ok(vec![commit("c2"), commit("c1")]))
            .with_commit_shortstats(HashMap::new())
            .with_unpushed_shas(["c2".to_string()].into_iter().collect())
            .with_diff_commit(Ok(COMMIT_DIFF.to_string()))
            .with_is_merge_in_progress(true)
            .with_diff_conflicts(Ok(CONFLICTS.to_string()))
            .with_unmerged_files(Ok(vec!["z.rs".to_string()])),
    )
}

fn paths(content: &crate::TabContent) -> Vec<&str> {
    content.diff.files.iter().map(|f| f.path.as_str()).collect()
}

/// A tab opened straight into History, Conflicts or Hidden loads that view:
/// History the newest commit's files, Conflicts each file of the merge once,
/// Hidden no files. Every view carries the commit scroller's rows.
#[test]
fn each_view_with_its_own_diff_loads_on_a_load_tab() -> anyhow::Result<()> {
    with_root(|repo| {
        let handle = actor_over(views_git());
        let cases: [(DiffMode, &[&str]); 3] = [
            (DiffMode::History, &["src/c.rs"]),
            (DiffMode::Conflicts, &["z.rs", "a.rs"]),
            (DiffMode::Hidden, &[]),
        ];
        for (mode, files) in cases {
            let key = TabKey::next();
            let command = Command::LoadTab {
                key,
                tab: working_tab(repo),
                mode: Some(mode),
            };
            let reviewed_cleared = 0;
            let loaded = Reply::ContentLoaded {
                key,
                reviewed_cleared,
            };
            assert_eq!(send(&handle, command)?, loaded, "{mode:?}");
            let content = content_of(&handle.view(), key)?;
            assert_eq!(content.tab.mode, mode);
            assert_eq!(paths(&content), files, "{mode:?}");
            let rows: Vec<(&str, bool)> = content
                .diff
                .meta
                .commits
                .iter()
                .map(|c| (c.sha.as_str(), c.unpushed))
                .collect();
            assert_eq!(rows, [("c2", true), ("c1", false)], "{mode:?}");
            let resource = crate::diff_resource(&content, &crate::DiffInputs::default());
            let selected = (mode == DiffMode::History).then(|| "c2".to_string());
            assert_eq!(resource.selected_commit_sha, selected, "{mode:?}");
            assert_eq!(resource.commits.len(), 2);
        }
        Ok(())
    })
}

/// Entering History reads the History bucket's context and AI, a commit
/// picked there keeps them, and leaving returns to the view it was entered
/// from with that view's bucket.
#[test]
fn a_switch_into_history_reads_the_history_bucket() -> anyhow::Result<()> {
    with_root(|repo| {
        let git = views_git();
        let handle = actor_over(Arc::clone(&git));
        let key = TabKey::next();
        load(&handle, key, repo)?;
        let branch = content_of(&handle.view(), key)?;
        let mut shown = branch.tab.clone();
        shown.mode = DiffMode::History;
        let history_dir = shown
            .identity()
            .managed_root(&*git)
            .unwrap_or_else(|| er_engine::paths::ErRoot::unresolved(repo, &*git))
            .er_dir();
        assert_ne!(history_dir, branch.context.er_dir);
        write_review(&history_dir, &branch.context.branch_diff_hash);

        send(&handle, set_mode(key, DiffMode::History))?;
        let history = content_of(&handle.view(), key)?;
        assert_eq!(
            history.context.er_dir, history_dir,
            "the old bucket's context"
        );
        assert_eq!(history.ai.state.total_findings(), 1, "the old bucket's AI");
        assert_eq!(paths(&history), ["src/c.rs"]);

        let pick = Command::SelectCommit {
            key,
            sha: "c1".to_string(),
        };
        send(&handle, pick)?;
        let picked = content_of(&handle.view(), key)?;
        let selected = picked.history.as_ref().and_then(|h| h.selected.as_deref());
        assert_eq!(selected, Some("c1"));
        assert_eq!(picked.context.er_dir, history_dir);

        send(&handle, Command::ExitHistory { key })?;
        let back = content_of(&handle.view(), key)?;
        assert_eq!(back.tab.mode, DiffMode::Branch);
        assert_eq!(back.context.er_dir, branch.context.er_dir);
        assert_eq!(back.ai.state.total_findings(), 0);
        assert_eq!(paths(&back), ["src/a.rs", "src/b.rs"]);
        Ok(())
    })
}

/// A tick's sidecar reload that lands while a load runs lets the load apply;
/// a sidecar the workspace wrote meanwhile still refuses it.
#[test]
fn a_load_applies_over_a_reload_but_not_over_a_write() -> Result<(), PatchRefused> {
    let mut state = WorkspaceState::new();
    let key = TabKey::next();
    let loaded = |since| ContentPatch::Loaded {
        key,
        since,
        loaded: loaded_tab("/repo"),
    };
    let (since, _) = state.apply_patch(loaded(None))?;
    let ai = Box::new(ReloadedAi {
        state: AiState::default(),
        stamp: SidecarStamp::default(),
    });
    state.apply_patch(ContentPatch::Ai { key, since, ai })?;
    let (since, _) = state.apply_patch(loaded(Some(since)))?;

    let payload = WrittenPayload::Questions(er_engine::ai::ErQuestions {
        version: 1,
        diff_hash: String::new(),
        questions: Vec::new(),
    });
    let dir = String::new();
    state.apply_patch(ContentPatch::Sidecar {
        dir,
        payload,
        written: None,
    })?;
    let stale = state.apply_patch(loaded(Some(since)));
    assert_eq!(stale.err(), Some(PatchRefused::Moved));
    Ok(())
}

const WITH_LOCK: &str = "\
diff --git a/Cargo.lock b/Cargo.lock
--- a/Cargo.lock
+++ b/Cargo.lock
@@ -1,2 +1,2 @@
 lock
-old
+new
diff --git a/src/a.rs b/src/a.rs
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,2 +1,2 @@
 a-one
-a-old
+a-new
";

/// An expanded compacted file is parsed in full and stays so across a
/// refresh; a plain parse leaves it compacted.
#[test]
fn an_expanded_file_stays_expanded_across_a_refresh() -> anyhow::Result<()> {
    with_root(|repo| {
        let lock = WITH_LOCK
            .split("diff --git a/src")
            .next()
            .unwrap_or_default();
        let git = Arc::new(
            FakeGit::default()
                .with_current_branch(Ok("feature".to_string()))
                .with_diff_shortstat((0, 0, 0))
                .with_diff_raw(Ok(WITH_LOCK.to_string()))
                .with_diff_raw_file(Ok(lock.to_string())),
        );
        let handle = actor_over(git);
        let key = TabKey::next();
        load(&handle, key, repo)?;
        let lock_file =
            |handle: &WorkspaceHandle| -> anyhow::Result<Arc<er_engine::git::DiffFile>> {
                let content = content_of(&handle.view(), key)?;
                let file = content.diff.files.iter().find(|f| f.path == "Cargo.lock");
                file.cloned()
                    .ok_or_else(|| anyhow::anyhow!("no Cargo.lock"))
            };
        assert!(
            lock_file(&handle)?.compacted,
            "the lock file was not compacted"
        );
        let parse = |expand| Command::ParseFiles {
            key,
            paths: vec!["Cargo.lock".to_string()],
            expand,
        };

        send(&handle, parse(false))?;
        assert!(lock_file(&handle)?.compacted, "a plain parse expanded it");
        let reply = send(&handle, parse(true))?;
        assert_eq!(reply, Reply::FilesParsed { key, parsed: 1 });
        send(&handle, refresh(key))?;

        let file = lock_file(&handle)?;
        assert!(!file.compacted, "the refresh compacted it again");
        assert_eq!(file.hunks.len(), 1);
        Ok(())
    })
}

/// A review an outside writer leaves in the tab's bucket.
fn write_review(er_dir: &str, diff_hash: &str) {
    er_engine::test_support::write_sidecar_json::<er_engine::ai::ErReview>(
        Path::new(er_dir),
        serde_json::json!({
            "version": 1,
            "diff_hash": diff_hash,
            "files": { "src/b.rs": { "risk": "low", "findings": [{
                "id": "f-1", "severity": "low", "title": "A defect",
                "description": "desc", "hunk_index": 0, "line_start": 2
            }] } }
        }),
    );
}

fn reloaded(reply: Reply) -> anyhow::Result<bool> {
    match reply {
        Reply::SidecarsReloaded { reloaded, .. } => Ok(reloaded),
        other => anyhow::bail!("unexpected reply: {other:?}"),
    }
}

/// An outside write reaches the AI section on the next reload; a reload with
/// nothing new publishes nothing.
#[test]
fn a_reload_takes_an_outside_write_and_publishes_nothing_when_idle() -> anyhow::Result<()> {
    with_root(|repo| {
        let handle = actor_over(git(RAW));
        let key = TabKey::next();
        load(&handle, key, repo)?;
        let loaded = content_of(&handle.view(), key)?;
        write_review(&loaded.context.er_dir, &loaded.context.branch_diff_hash);

        let reload = || send(&handle, Command::ReloadSidecars { key }).and_then(reloaded);
        assert!(reload()?, "the outside write was not read");
        let content = content_of(&handle.view(), key)?;
        assert_eq!(content.ai.state.total_findings(), 1);
        assert!(content.ai.revision > loaded.ai.revision);

        let revision = handle.view().revision;
        assert!(!reload()?, "an idle reload read again");
        assert!(!reload()?, "an idle reload read again");
        assert_eq!(handle.view().revision, revision, "an idle reload published");
        Ok(())
    })
}

/// A reload sent while one runs for the same tab is answered at once, and a
/// reload after the last one finished reads again. The gate is driven by
/// hand, so no actor decides when the read lands.
#[test]
fn a_reload_sent_while_one_runs_is_answered_at_once() -> anyhow::Result<()> {
    let mut state = WorkspaceState::new();
    let key = TabKey::next();
    let first_load = ContentPatch::Loaded {
        key,
        since: None,
        loaded: loaded_tab("/repo"),
    };
    state
        .apply_patch(first_load)
        .map_err(|refused| anyhow::anyhow!("{refused}"))?;
    let github: Arc<dyn er_github::GitHub> = Arc::new(FakeGitHub::default());
    let deps = WorkspaceDeps::new(Ports {
        git: git(RAW),
        github,
    });
    let (tx, done) = channel();
    let mut reloads = SidecarReloads::default();
    let start = |reloads: &mut SidecarReloads, state: &WorkspaceState| {
        let (reply, replies) = channel();
        reloads.start(&deps, state, &tx, key, reply);
        replies
    };

    let first = start(&mut reloads, &state);
    let second = start(&mut reloads, &state);
    let coalesced = Reply::SidecarsReloaded {
        key,
        reloaded: false,
    };
    assert_eq!(second.try_recv().ok(), Some(Ok(coalesced)));
    assert!(first.try_recv().is_err(), "the first read answered early");

    let Ok(ActorMsg::Reloaded(read)) = done.recv_timeout(WAIT) else {
        anyhow::bail!("the first read never finished");
    };
    let view = RwLock::new(Arc::new(WorkspaceView::default()));
    reloads.finish(&mut state, read, &Mutex::new(Vec::new()), &view);
    assert!(first.try_recv().is_ok(), "the first read was not answered");
    let third = start(&mut reloads, &state);
    assert!(third.try_recv().is_err(), "a later reload was coalesced");
    Ok(())
}

#[test]
fn sidecars_move_when_newer_created_deleted_or_rearbitrated() {
    let at = |secs| Some(std::time::UNIX_EPOCH + Duration::from_secs(secs));
    let stamp = |latest: Option<std::time::SystemTime>, arbiter: Option<&[u8]>| SidecarStamp {
        dirs: vec!["/bucket".to_string()],
        files: latest
            .map(|mtime| (std::path::PathBuf::from("/bucket/review.json"), mtime))
            .into_iter()
            .collect(),
        arbiter: arbiter.map(<[u8]>::to_vec),
    };
    let loaded = stamp(at(10), None);
    assert!(!loaded.moved_to(&stamp(at(10), None)));
    assert!(loaded.moved_to(&stamp(at(11), None)), "newer");
    assert!(stamp(None, None).moved_to(&loaded), "created");
    assert!(loaded.moved_to(&stamp(None, None)), "deleted");
    assert!(loaded.moved_to(&stamp(at(10), Some(b"x"))), "rearbitrated");
}
