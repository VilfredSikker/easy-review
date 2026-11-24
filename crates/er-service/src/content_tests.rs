//! Tab content: a load publishes the sections, a write patches every tab on
//! its bucket, a tick shares the sections, a key the actor never loaded is
//! refused before any effect, and a patch from a stale load is refused with
//! nothing moved.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use er_engine::ai::AiState;
use er_engine::ports::Ports;
use er_engine::tab_identity::{DiffMode, ResolvedTab};
use er_engine::tabs_record::{TabDescriptor, TabKind};
use er_engine::TabKey;
use er_git::FakeGit;
use er_github::FakeGitHub;

use crate::actor::WorkspaceDeps;
use crate::content::{ContentPatch, DiffMeta, LoadedDiff, LoadedTab, PatchRefused};
use crate::handle::{ActorMsg, WorkspaceError};
use crate::protocol::{AnnotationTarget, Command, Reply, TabContext};
use crate::workspace::{WorkspaceState, WorkspaceView};
use crate::{spawn, WorkspaceHandle};

mod gaps;
mod hunks;
mod reviewed;
mod stamps;
mod worktree;

const RAW: &str = "\
diff --git a/src/a.rs b/src/a.rs
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,2 +1,2 @@
 a-one
-a-old
+a-new
diff --git a/src/b.rs b/src/b.rs
--- a/src/b.rs
+++ b/src/b.rs
@@ -1,3 +1,3 @@
 b-one
-b-old
+b-new
 b-three
";

const WAIT: Duration = Duration::from_secs(5);

fn git(raw: &str) -> Arc<FakeGit> {
    Arc::new(
        FakeGit::default()
            .with_current_branch(Ok("feature".to_string()))
            .with_diff_shortstat((0, 0, 0))
            .with_diff_raw(Ok(raw.to_string())),
    )
}

fn actor_over(git: Arc<FakeGit>) -> WorkspaceHandle {
    let github: Arc<dyn er_github::GitHub> = Arc::new(FakeGitHub::default());
    let git: Arc<dyn er_git::Git> = git;
    spawn(WorkspaceDeps::new(Ports { git, github }))
}

/// A plain tab on `repo` with a saved base, so a load reads the checkout's
/// branch and the diff and nothing else.
fn working_tab(repo: &str) -> TabDescriptor {
    TabDescriptor {
        kind: TabKind::Working,
        repo_root: repo.to_string(),
        branch: None,
        pr_owner: None,
        pr_repo: None,
        pr_number: None,
        pr_head_ref: None,
        base_ref: Some("main".to_string()),
        extra: serde_json::Map::new(),
    }
}

fn load(handle: &WorkspaceHandle, key: TabKey, repo: &str) -> anyhow::Result<Reply> {
    Ok(handle.send_and_wait_with(
        Command::LoadTab {
            key,
            tab: working_tab(repo),
            mode: None,
        },
        WAIT,
    )?)
}

/// A plain refresh of `key`.
pub(crate) fn refresh(key: TabKey) -> Command {
    Command::RefreshDiff {
        key,
        force: false,
        adopt_head_oid: None,
    }
}

/// Runs `test` with a storage root every effect thread sees, and a repo dir
/// under it.
fn with_root(test: impl FnOnce(&str) -> anyhow::Result<()>) -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(&repo)?;
    let repo = repo.to_string_lossy().into_owned();
    er_engine::storage::with_process_test_storage_root(dir.path().join("storage"), || test(&repo))
}

fn content_of(view: &WorkspaceView, key: TabKey) -> anyhow::Result<Arc<crate::TabContent>> {
    view.tab(key)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("no content under {key:?}"))
}

/// A load that ran on the actor thread would trip the guard there, and the
/// reply would never arrive.
#[test]
fn a_loaded_tab_publishes_its_diff_and_context() -> anyhow::Result<()> {
    with_root(|repo| {
        let handle = actor_over(git(RAW));
        let key = TabKey::next();

        let reply = load(&handle, key, repo)?;

        assert_eq!(
            reply,
            Reply::ContentLoaded {
                key,
                reviewed_cleared: 0
            }
        );
        let content = content_of(&handle.view(), key)?;
        let paths: Vec<&str> = content.diff.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["src/a.rs", "src/b.rs"]);
        let hash = er_engine::ai::compute_diff_hash(RAW);
        assert_eq!(content.diff.meta.diff_hash, hash);
        assert_eq!(content.context.diff_hash, hash);
        assert_eq!(content.context.branch_diff_hash, hash);
        assert_eq!(content.context.scope, "branch");
        assert_eq!(content.context.repo_root, repo);
        assert!(!content.context.er_dir.is_empty());
        Ok(())
    })
}

#[test]
fn a_mark_moves_the_reviewed_section_of_every_tab_on_its_bucket() -> anyhow::Result<()> {
    with_root(|repo| {
        let handle = actor_over(git(RAW));
        let (first, second) = (TabKey::next(), TabKey::next());
        load(&handle, first, repo)?;
        load(&handle, second, repo)?;
        let before = handle.view();
        let context = content_of(&before, first)?.context.clone();

        let reply = handle.send_and_wait_with(
            Command::MarkReviewed {
                context,
                path: "src/b.rs".to_string(),
                reviewed: true,
            },
            WAIT,
        )?;

        assert_eq!(reply, Reply::ReviewedChanged { changed: 1 });
        let after = handle.view();
        for key in [first, second] {
            let (old, new) = (content_of(&before, key)?, content_of(&after, key)?);
            assert!(
                new.reviewed.marks.contains_key("src/b.rs"),
                "{key:?} missed the mark"
            );
            assert!(new.reviewed.revision > old.reviewed.revision);
            assert!(
                Arc::ptr_eq(&old.diff, &new.diff),
                "a mark rebuilt the diff section"
            );
            assert!(Arc::ptr_eq(&old.ai, &new.ai));
        }
        Ok(())
    })
}

#[test]
fn a_question_lands_in_the_ai_section_of_every_tab_on_its_bucket() -> anyhow::Result<()> {
    with_root(|repo| {
        let handle = actor_over(git(RAW));
        let (first, second) = (TabKey::next(), TabKey::next());
        load(&handle, first, repo)?;
        load(&handle, second, repo)?;
        let before = handle.view();
        let context = content_of(&before, first)?.context.clone();

        handle.send_and_wait_with(
            Command::AddQuestion {
                context,
                target: AnnotationTarget {
                    file: "src/b.rs".to_string(),
                    hunk_idx: 0,
                    line_num: Some(2),
                    line_num_end: None,
                },
                body: "why?".to_string(),
                side: Some("RIGHT".to_string()),
                client_id: Some("q-painted".to_string()),
                reply_to: None,
                finding_ref: None,
            },
            WAIT,
        )?;

        let after = handle.view();
        for key in [first, second] {
            let (old, new) = (content_of(&before, key)?, content_of(&after, key)?);
            let ids: Vec<&str> = new
                .ai
                .state
                .questions
                .iter()
                .flat_map(|questions| questions.questions.iter().map(|q| q.id.as_str()))
                .collect();
            assert_eq!(ids, vec!["q-painted"], "{key:?} missed the question");
            assert!(new.ai.revision > old.ai.revision);
            assert!(
                Arc::ptr_eq(&old.diff, &new.diff),
                "a question rebuilt the diff section"
            );
            assert!(Arc::ptr_eq(&old.reviewed, &new.reviewed));
        }
        Ok(())
    })
}

/// An arena tick republishes the view; the sections behind it must be the
/// same allocations, or every tick would copy every open diff.
#[test]
fn an_arena_tick_shares_every_section() -> anyhow::Result<()> {
    with_root(|repo| {
        let handle = actor_over(git(RAW));
        let key = TabKey::next();
        load(&handle, key, repo)?;
        let before = handle.view();

        let _ = handle.tx.send(ActorMsg::ArenaChanged);
        let deadline = Instant::now() + WAIT;
        while handle.view().revision == before.revision {
            anyhow::ensure!(Instant::now() < deadline, "the tick never published");
            std::thread::yield_now();
        }

        let (old, new) = (content_of(&before, key)?, content_of(&handle.view(), key)?);
        assert!(
            Arc::ptr_eq(&old.diff, &new.diff),
            "the tick copied the diff"
        );
        assert!(Arc::ptr_eq(&old.reviewed, &new.reviewed));
        assert!(Arc::ptr_eq(&old.ai, &new.ai));
        Ok(())
    })
}

#[test]
fn a_tab_command_for_a_key_never_loaded_is_refused_before_any_effect() -> anyhow::Result<()> {
    with_root(|_| {
        let git = git(RAW);
        let handle = actor_over(Arc::clone(&git));
        let before = handle.view().revision;

        for command in [
            Command::RefreshDiff {
                key: TabKey::next(),
                force: false,
                adopt_head_oid: None,
            },
            Command::SetDiffMode {
                key: TabKey::next(),
                mode: er_engine::tab_identity::DiffMode::Unstaged,
                pr_number: None,
            },
            Command::ParseFiles {
                key: TabKey::next(),
                paths: vec!["src/a.rs".to_string()],
                expand: false,
            },
            Command::ReloadSidecars {
                key: TabKey::next(),
            },
        ] {
            let result = handle.send_and_wait_with(command, WAIT);
            assert_eq!(
                result,
                Err(WorkspaceError::Failed("no loaded tab has that key".into()))
            );
        }

        let view = handle.view();
        assert_eq!(view.revision, before, "a refused command published");
        assert_eq!(view.last_error, None);
        assert!(git.calls().is_empty(), "a refused command ran git");
        Ok(())
    })
}

/// A full load drops the marks whose file left the diff or changed since it
/// was marked, and saves the marks that are left.
#[test]
fn a_load_clears_moved_and_orphaned_marks_and_saves_them() -> anyhow::Result<()> {
    with_root(|repo| {
        let handle = actor_over(git(RAW));
        let key = TabKey::next();
        load(&handle, key, repo)?;
        let er_dir = content_of(&handle.view(), key)?.context.er_dir.clone();
        let store = er_engine::store_registry::store();
        let scope = er_store::Scope::dir(&er_dir)?;
        let b_hash = er_engine::ai::compute_per_file_hash(RAW, "src/b.rs").unwrap_or_default();
        let marks = [
            ("src/a.rs", "an older hash".to_string()),
            ("src/b.rs", b_hash.clone()),
            ("src/gone.rs", "left the diff".to_string()),
        ]
        .into_iter()
        .map(|(path, hash)| (path.to_string(), hash))
        .collect();
        er_engine::session_store::save_reviewed(&store, &scope, &marks)?;

        let reply = handle.send_and_wait_with(refresh(key), WAIT)?;

        assert_eq!(
            reply,
            Reply::ContentLoaded {
                key,
                reviewed_cleared: 2
            }
        );
        let saved = er_engine::session_store::load_reviewed(&store, &scope)?;
        assert_eq!(saved.len(), 1);
        assert_eq!(saved.get("src/b.rs"), Some(&b_hash));
        let content = content_of(&handle.view(), key)?;
        assert_eq!(content.reviewed.marks, saved);
        Ok(())
    })
}

/// Loading, refreshing and closing a tab read the diff and the sidecars; none
/// of them may submit an agent job.
#[test]
fn a_tab_load_submits_no_job() -> anyhow::Result<()> {
    with_root(|repo| {
        let sink = Arc::new(crate::flow_tests::CountingSink::default());
        let git: Arc<dyn er_git::Git> = git(RAW);
        let github: Arc<dyn er_github::GitHub> = Arc::new(FakeGitHub::default());
        let jobs: Arc<dyn er_engine::jobs::JobSink> = sink.clone();
        let handle = spawn(WorkspaceDeps::new(Ports { git, github }).with_jobs(jobs));
        let key = TabKey::next();

        load(&handle, key, repo)?;
        handle.send_and_wait_with(refresh(key), WAIT)?;
        handle.send_and_wait_with(Command::DropTab { key }, WAIT)?;

        let submitted = sink.submitted.lock().map(|n| *n).unwrap_or(usize::MAX);
        assert_eq!(submitted, 0, "a tab command submitted a job");
        assert!(
            handle.view().tab(key).is_none(),
            "the drop kept the content"
        );
        Ok(())
    })
}

/// The PR the header detected seeds a tab that has none, so entering the PR
/// diff fetches that PR's refs, as the App path does before `enter_pr_diff`.
/// A tab that already has a PR keeps its own.
#[test]
fn a_mode_switch_seeds_the_detected_pr_only_onto_a_tab_without_one() -> anyhow::Result<()> {
    with_root(|repo| {
        let git = Arc::new(
            FakeGit::default()
                .with_current_branch(Ok("feature".to_string()))
                .with_diff_shortstat((0, 0, 0))
                .with_diff_raw(Ok(RAW.to_string()))
                .with_fetch(Ok(())),
        );
        let github = Arc::new(
            FakeGitHub::default()
                .with_pr_base_branch_and_commits(Ok(("main".to_string(), Vec::new())))
                .with_fetch_pr_base_ref(Ok("origin/main".to_string()))
                .with_pr_diff(Ok(RAW.to_string())),
        );
        let git_port: Arc<dyn er_git::Git> = git.clone();
        let github_port: Arc<dyn er_github::GitHub> = github.clone();
        let handle = spawn(WorkspaceDeps::new(Ports {
            git: git_port,
            github: github_port,
        }));
        let key = TabKey::next();
        load(&handle, key, repo)?;
        assert_eq!(content_of(&handle.view(), key)?.tab.pr_number, None);

        let to_pr = |pr_number| Command::SetDiffMode {
            key,
            mode: DiffMode::PrDiff,
            pr_number: Some(pr_number),
        };
        handle.send_and_wait_with(to_pr(9), WAIT)?;

        let content = content_of(&handle.view(), key)?;
        assert_eq!(content.tab.pr_number, Some(9));
        assert!(content.pr_refs_fetched, "the PR refs were not fetched");
        let head =
            format!(r#"fetch(root={repo:?}, args=["origin", "+pull/9/head:refs/er/pr/9/head"])"#);
        assert!(git.calls().contains(&head), "{:?}", git.calls());
        let base = github.calls().into_iter().any(|call| {
            call.starts_with("pr_base_branch_and_commits(") && call.contains("number=9,")
        });
        assert!(base, "{:?}", github.calls());

        let to_branch = Command::SetDiffMode {
            key,
            mode: DiffMode::Branch,
            pr_number: None,
        };
        handle.send_and_wait_with(to_branch, WAIT)?;
        handle.send_and_wait_with(to_pr(11), WAIT)?;
        let content = content_of(&handle.view(), key)?;
        assert_eq!(
            content.tab.pr_number,
            Some(9),
            "the detected PR replaced the tab's"
        );
        Ok(())
    })
}

/// A diff of `lines` changed lines, a hundred to a file.
fn large_diff(lines: usize) -> String {
    let mut raw = String::new();
    for file in 0..lines.div_ceil(100) {
        let path = format!("src/f{file}.rs");
        raw.push_str(&format!(
            "diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -1,100 +1,100 @@\n"
        ));
        for line in 0..100 {
            raw.push_str(&format!("-old {file} {line}\n+new {file} {line}\n"));
        }
    }
    raw
}

fn median(
    runs: usize,
    mut operation: impl FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<Duration> {
    let mut samples = Vec::with_capacity(runs);
    for _ in 0..runs {
        let started = Instant::now();
        operation()?;
        samples.push(started.elapsed());
    }
    samples.sort_unstable();
    Ok(samples[runs / 2])
}

/// Times a tab load and a reviewed mark through the actor over a 5,000-line
/// diff, against the same effects called directly with the same ports.
#[test]
#[ignore = "latency harness: run with --ignored --nocapture and paste the numbers in the PR; never a CI assertion"]
fn the_content_routes_stay_within_the_latency_budget() -> anyhow::Result<()> {
    const RUNS: usize = 30;
    let raw = large_diff(5_000);
    with_root(|repo| {
        let github: Arc<dyn er_github::GitHub> = Arc::new(FakeGitHub::default());
        let git: Arc<dyn er_git::Git> = git(&raw);
        let deps = WorkspaceDeps::new(Ports { git, github });
        let tab = working_tab(repo);

        let direct_open = median(RUNS, || {
            let input = crate::effect::first_load(&deps, &tab, None)?;
            crate::effect::load_tab(&deps, input)?;
            Ok(())
        })?;
        let handle = spawn(deps.clone());
        let key = TabKey::next();
        let routed_open = median(RUNS, || load(&handle, key, repo).map(|_| ()))?;

        let context = content_of(&handle.view(), key)?.context.clone();
        let mut reviewed = false;
        let direct_mark = median(RUNS, || {
            reviewed = !reviewed;
            crate::annotations::mark_reviewed(&deps, &context, "src/f0.rs", reviewed)?;
            Ok(())
        })?;
        let routed_mark = median(RUNS, || {
            reviewed = !reviewed;
            handle.send_and_wait_with(
                Command::MarkReviewed {
                    context: context.clone(),
                    path: "src/f0.rs".to_string(),
                    reviewed,
                },
                WAIT,
            )?;
            Ok(())
        })?;

        println!(
            "open: direct median {direct_open:?}, actor median {routed_open:?}; \
             mark reviewed: direct median {direct_mark:?}, actor median {routed_mark:?}; \
             {RUNS} runs over {} bytes",
            raw.len()
        );
        for (name, direct, routed) in [
            ("open", direct_open, routed_open),
            ("mark reviewed", direct_mark, routed_mark),
        ] {
            // 10% or 2 ms over the direct median, whichever is larger.
            let budget = (direct * 11 / 10).max(direct + Duration::from_millis(2));
            assert!(
                routed <= budget,
                "{name} through the actor cost {routed:?}, over the {budget:?} budget"
            );
        }
        Ok(())
    })
}

/// What a load effect would hand back for a tab with no files, built without
/// an actor so the patch steps run on their own.
fn loaded_tab(repo: &str) -> Box<LoadedTab> {
    Box::new(LoadedTab {
        tab: ResolvedTab {
            repo_root: repo.to_string(),
            current_branch: "feature".to_string(),
            base_branch: "main".to_string(),
            mode: DiffMode::Branch,
            tour_is_pr: false,
            remote_repo: None,
            pr_number: None,
            local_branch_view: None,
            pr_head_ref: None,
            checkout_root: None,
        },
        context: TabContext {
            er_dir: String::new(),
            comments_dir: String::new(),
            is_remote: false,
            repo_root: repo.to_string(),
            base_branch: "main".to_string(),
            scope: "branch".to_string(),
            remote_repo: None,
            pr_number: None,
            pr_head_ref: None,
            local_branch_view: None,
            checkout_root: None,
            branch_diff_hash: String::new(),
            diff_hash: String::new(),
        },
        pr_refs_fetched: false,
        diff: LoadedDiff {
            files: Vec::new(),
            meta: DiffMeta {
                headers: Vec::new(),
                raw: String::new(),
                lazy: false,
                diff_hash: String::new(),
                branch_diff_hash: String::new(),
                preview_head_oid: None,
                stamps: Default::default(),
                scope_stats: Default::default(),
                rules_key: String::new(),
                risk_artifact: None,
                commits: Vec::new(),
                committed_unpushed: false,
            },
            expanded: Default::default(),
        },
        marks: HashMap::new(),
        file_hashes: HashMap::new(),
        ai: AiState::default(),
        ai_stamp: Default::default(),
        ai_extras: Default::default(),
        pr: Default::default(),
        freshness: Default::default(),
        history: None,
        cleared: 0,
    })
}

/// A load or parse that started before the tab moved must not overwrite what
/// landed meanwhile: the patch is refused, and no section or revision moves.
#[test]
fn a_patch_from_before_the_tab_moved_is_refused_with_nothing_moved() -> anyhow::Result<()> {
    let mut state = WorkspaceState::new();
    let key = TabKey::next();
    let first = ContentPatch::Loaded {
        key,
        since: None,
        loaded: loaded_tab("/repo"),
    };
    let (started_at, _) = state
        .apply_patch(first)
        .map_err(|refused| anyhow::anyhow!("{refused}"))?;
    let refresh = ContentPatch::Loaded {
        key,
        since: Some(started_at),
        loaded: loaded_tab("/repo"),
    };
    let (revision, _) = state
        .apply_patch(refresh)
        .map_err(|refused| anyhow::anyhow!("{refused}"))?;
    let before = content_of(state.view(), key)?;

    for (name, stale) in [
        (
            "a load",
            ContentPatch::Loaded {
                key,
                since: Some(started_at),
                loaded: loaded_tab("/repo"),
            },
        ),
        (
            "a parse",
            ContentPatch::Parsed {
                key,
                since: started_at,
                files: Vec::new(),
                expanded: Vec::new(),
            },
        ),
    ] {
        assert_eq!(
            state.apply_patch(stale),
            Err(PatchRefused::Moved),
            "{name} from a moved tab applied"
        );
        let after = content_of(state.view(), key)?;
        assert_eq!(state.view().revision, revision, "{name} moved the revision");
        assert!(
            Arc::ptr_eq(&before.diff, &after.diff),
            "{name} replaced the diff"
        );
        assert!(Arc::ptr_eq(&before.reviewed, &after.reviewed));
        assert!(Arc::ptr_eq(&before.ai, &after.ai));
    }
    Ok(())
}

/// A tab closed while its first load is still running: the drop reaches the
/// actor first, and the load that lands after it must not bring the closed
/// tab's content back.
#[test]
fn a_first_load_landing_after_its_tab_closed_is_refused() -> anyhow::Result<()> {
    with_root(|repo| {
        let handle = actor_over(git(RAW));
        let key = TabKey::next();

        handle.send_and_wait_with(Command::DropTab { key }, WAIT)?;
        let late = load(&handle, key, repo);

        assert!(late.is_err(), "the late load answered {late:?}");
        assert!(
            handle.view().tab(key).is_none(),
            "the closed tab's content came back"
        );
        let other = TabKey::next();
        load(&handle, other, repo)?;
        assert!(
            handle.view().tab(other).is_some(),
            "an open tab still loads"
        );
        Ok(())
    })
}

/// A drop of a key that never loaded changes no view: it moves no revision,
/// and a later first load for the key is still refused. A drop of a loaded
/// key does move it.
#[test]
fn a_drop_of_a_key_that_never_loaded_moves_no_version() -> anyhow::Result<()> {
    with_root(|repo| {
        let handle = actor_over(git(RAW));
        let before = handle.view().revision;

        let never = TabKey::next();
        handle.send_and_wait_with(Command::DropTab { key: never }, WAIT)?;

        assert_eq!(handle.view().revision, before, "the drop moved the view");
        assert!(load(&handle, never, repo).is_err(), "the closed key loaded");
        let loaded = TabKey::next();
        load(&handle, loaded, repo)?;
        let at = handle.view().revision;
        handle.send_and_wait_with(Command::DropTab { key: loaded }, WAIT)?;
        assert!(
            handle.view().revision > at,
            "dropping content moved nothing"
        );
        Ok(())
    })
}

/// A tab closed before its workspace actor started: the drop reached no
/// actor, so only the app registry's list can tell the late load belongs to
/// a closed tab.
#[test]
fn a_load_for_a_key_the_registry_no_longer_lists_is_refused() -> anyhow::Result<()> {
    use crate::app_registry::{AppCommand, AppRegistry};

    with_root(|repo| {
        let github: Arc<dyn er_github::GitHub> = Arc::new(FakeGitHub::default());
        let git: Arc<dyn er_git::Git> = git(RAW);
        let ports = Ports { git, github };
        let registry = Arc::new(AppRegistry::new(ports.clone()));
        for _ in 0..2 {
            registry.send(AppCommand::NewTab {
                tab: working_tab(repo),
            })?;
        }
        let opened = registry.tabs_view();
        let (closed, kept) = (opened.tabs[0].key, opened.tabs[1].key);
        registry.send(AppCommand::CloseTab { key: closed })?;

        let deps = WorkspaceDeps::new(ports).with_app_registry(Arc::clone(&registry));
        assert!(
            deps.app_registry().is_some(),
            "the actor has no registry to ask"
        );
        let handle = spawn(deps);
        let late = load(&handle, closed, repo);

        assert!(late.is_err(), "the late load answered {late:?}");
        assert!(
            handle.view().tab(closed).is_none(),
            "the closed tab's content came back"
        );
        load(&handle, kept, repo)?;
        assert!(
            handle.view().tab(kept).is_some(),
            "a listed tab did not load"
        );
        Ok(())
    })
}

/// The workspace remembers a bounded number of closed keys, the newest ones,
/// however many tabs open and close; a load landing after the newest close
/// is still refused.
#[test]
fn the_closed_keys_stay_bounded_after_ten_thousand_closes() {
    let mut state = WorkspaceState::new();
    let mut last = None;
    for _ in 0..10_000 {
        let key = TabKey::next();
        let loaded = ContentPatch::Loaded {
            key,
            since: None,
            loaded: loaded_tab("/repo"),
        };
        assert!(
            state.apply_patch(loaded).is_ok(),
            "a first load was refused"
        );
        assert!(state.apply_patch(ContentPatch::Dropped { key }).is_ok());
        last = Some(key);
    }
    assert!(
        state.closed_len() <= 256,
        "{} closed keys kept",
        state.closed_len()
    );
    let Some(key) = last else {
        panic!("no tab closed");
    };
    let late = ContentPatch::Loaded {
        key,
        since: None,
        loaded: loaded_tab("/repo"),
    };
    assert!(
        state.apply_patch(late).is_err(),
        "the newest closed key loaded again"
    );
}

/// `diff(tab)` lists the newest file first when the view sorts by mtime, by
/// the mtimes the load stamped, and keeps each file's index in the diff, which
/// the file commands name.
#[test]
fn the_diff_resource_keeps_the_mtime_order() {
    let file = |path: &str| er_engine::git::DiffFile {
        path: path.to_string(),
        status: er_engine::git::FileStatus::Modified,
        hunks: Vec::new(),
        adds: 1,
        dels: 1,
        compacted: false,
        raw_hunk_count: 0,
        similarity: None,
    };
    let at = |secs: u64| std::time::UNIX_EPOCH + Duration::from_secs(secs);
    let mut loaded = loaded_tab("/repo");
    loaded.diff.files = vec![file("a.rs"), file("b.rs"), file("c.rs")];
    loaded.diff.meta.stamps.mtimes = HashMap::from([
        ("a.rs".to_string(), at(100)),
        ("b.rs".to_string(), at(300)),
        ("c.rs".to_string(), at(200)),
    ]);
    let content = crate::content::TabContent::from_loaded(TabKey::next(), *loaded, 1);
    let listed = |sort_by_mtime| {
        let inputs = crate::DiffInputs {
            sort_by_mtime,
            ..crate::DiffInputs::default()
        };
        crate::diff_resource(&content, &inputs)
            .files
            .into_iter()
            .map(|row| (row.path, row.source_index))
            .collect::<Vec<_>>()
    };
    let by_mtime = [("b.rs", 1), ("c.rs", 2), ("a.rs", 0)];
    let in_diff = [("a.rs", 0), ("b.rs", 1), ("c.rs", 2)];
    let named = |rows: [(&str, u64); 3]| rows.map(|(p, i)| (p.to_string(), i)).to_vec();
    assert_eq!(listed(true), named(by_mtime));
    assert_eq!(listed(false), named(in_diff));
}

/// `diff(tab)` names the PR head its diff was built from on any PR tab, which
/// the stale mark compares with the newest head, and the head on screen only
/// while the view shows the PR's own head, which the merge pins to.
#[test]
fn the_diff_resource_names_both_pr_heads() {
    let pr_tab = |mode: DiffMode, remote: bool| {
        let mut loaded = loaded_tab("/repo");
        loaded.tab.mode = mode;
        loaded.tab.pr_number = Some(12);
        if remote {
            loaded.tab.remote_repo = Some("acme/widgets".to_string());
        } else {
            loaded.tab.local_branch_view = Some("feature".to_string());
        }
        loaded.diff.meta.preview_head_oid = Some("shown-head".to_string());
        loaded.pr.head_oid = Some("built-head".to_string());
        crate::content::TabContent::from_loaded(TabKey::next(), *loaded, 1)
    };
    let heads = |content: &crate::content::TabContent| {
        let diff = crate::diff_resource(content, &crate::DiffInputs::default());
        (diff.built_from_head_oid, diff.pr_diff_head_oid)
    };
    let both = (
        Some("built-head".to_string()),
        Some("shown-head".to_string()),
    );
    assert_eq!(heads(&pr_tab(DiffMode::PrDiff, false)), both, "PR Diff");
    assert_eq!(heads(&pr_tab(DiffMode::Branch, true)), both, "a remote PR");
    // The local branch view shows the working tree, which no PR head names.
    let local = (Some("built-head".to_string()), None);
    assert_eq!(heads(&pr_tab(DiffMode::Branch, false)), local, "local view");

    // A tab with no PR has no PR head, whatever its section holds.
    let mut plain = loaded_tab("/repo");
    plain.pr.head_oid = Some("built-head".to_string());
    let plain = crate::content::TabContent::from_loaded(TabKey::next(), *plain, 1);
    assert_eq!(heads(&plain), (None, None), "a plain tab");
}

/// A write to any one section `diff(tab)` reads raises its version.
#[test]
fn each_section_the_diff_reads_raises_its_version() {
    use crate::content::TabContent;
    fn section<T>(arc: &mut Arc<T>) -> &mut T {
        Arc::get_mut(arc).unwrap_or_else(|| panic!("a fresh section is shared"))
    }
    let fresh = || TabContent::from_loaded(TabKey::next(), *loaded_tab("/repo"), 1);
    let base = crate::diff_version(&fresh());
    for name in ["reviewed", "ai", "pr", "mode"] {
        let mut content = fresh();
        match name {
            "reviewed" => section(&mut content.reviewed).revision += 5,
            "ai" => section(&mut content.ai).revision += 5,
            "pr" => section(&mut content.pr).revision += 5,
            _ => content.mode_revision += 5,
        }
        assert!(crate::diff_version(&content) > base, "{name} left {base}");
    }
}
