//! `hunks(tab, path, hash)` on the flag path: a comment moves the tab's
//! annotations and never a file's hash, an unparsed file is parsed by its
//! actor, a hash the diff no longer holds answers `Stale`, compaction runs
//! both ways on the service and holds across a load, and nothing records
//! which hunks the page holds.

use std::path::Path;
use std::sync::Arc;

use er_engine::tab_identity::DiffMode;
use er_engine::tabs_record::TabDescriptor;
use er_engine::TabKey;
use er_protocol::{ErErrorKind, FileHunks, Resource, ResourceKey, TabDiff};
use er_service::{AnnotationTarget, Command, TabContent};

use super::diff_resource_tests::{fake_git, listed, state_over, with_storage};
use super::watcher_tests::{canonical, git};
use crate::commands::{content_routes, AppState};
use crate::resources;
use crate::resources::send::{apply, Effects};
use crate::service::actor_key;

fn ports(git: er_git::FakeGit) -> er_engine::ports::Ports {
    let git: Arc<dyn er_git::Git> = Arc::new(git);
    let github: Arc<dyn er_engine::github::GitHub> =
        Arc::new(er_engine::github::FakeGitHub::default());
    er_engine::ports::Ports { git, github }
}

fn wire(key: TabKey) -> er_protocol::TabKey {
    er_service::wire_tab_key(key)
}

fn hunks_key(key: TabKey, path: &str, hash: &str) -> ResourceKey {
    ResourceKey::Hunks {
        tab: wire(key),
        path: path.to_string(),
        hash: hash.to_string(),
    }
}

fn diff(state: &AppState, key: TabKey) -> TabDiff {
    let fetched = resources::fetch(&state.desktop, ResourceKey::Diff { tab: wire(key) });
    match fetched.map(|f| f.value) {
        Ok(Resource::Diff(diff)) => *diff,
        other => panic!("not a diff: {other:?}"),
    }
}

fn row_hash(state: &AppState, key: TabKey, path: &str) -> String {
    let diff = diff(state, key);
    let row = diff.files.iter().find(|row| row.path == path);
    row.unwrap_or_else(|| panic!("{path} is not listed"))
        .cache_key
        .clone()
}

fn hunks(state: &AppState, key: TabKey, path: &str, hash: &str) -> Result<FileHunks, ErErrorKind> {
    match resources::fetch(&state.desktop, hunks_key(key, path, hash)) {
        Ok(fetched) => match fetched.value {
            Resource::Hunks(hunks) => Ok(hunks),
            other => panic!("not hunks: {other:?}"),
        },
        Err(error) => Err(error.kind),
    }
}

fn content(state: &AppState, key: TabKey, tab: &TabDescriptor) -> Arc<TabContent> {
    let content = state.service.content(key, tab);
    content.unwrap_or_else(|| panic!("no content"))
}

fn load(state: &AppState, key: TabKey, tab: &TabDescriptor, mode: DiffMode) {
    let load = Command::LoadTab {
        key,
        tab: tab.clone(),
        mode: Some(mode),
    };
    let loaded = state.service.send(&actor_key(tab), load);
    assert!(loaded.is_ok(), "{loaded:?}");
}

/// A question on a file moves the tab's AI section, while the file's hash
/// and its hunks stay as they were: the payload carries no thread, so the
/// page has no file to fetch again.
#[test]
fn a_comment_moves_the_annotations_and_never_the_file_hash() {
    with_storage(|dir| {
        let state = state_over(dir, ports(fake_git()));
        let (key, tab) = listed(&state, dir, "repo");
        load(&state, key, &tab, DiffMode::Branch);
        let hash = row_hash(&state, key, "src/f0.rs");
        let before = hunks(&state, key, "src/f0.rs", &hash);
        let ai = content(&state, key, &tab).ai.revision;

        let asked = content_routes::annotate(&state, None, |context| Command::AddQuestion {
            context,
            target: AnnotationTarget {
                file: "src/f0.rs".into(),
                hunk_idx: 0,
                line_num: Some(1),
                line_num_end: None,
            },
            body: "why?".into(),
            side: None,
            client_id: None,
            reply_to: None,
            finding_ref: None,
        });
        assert!(asked.is_ok(), "{asked:?}");

        let after = content(&state, key, &tab);
        assert!(after.ai.revision > ai, "the question moved no annotations");
        assert_eq!(after.ai.state.file_question_count("src/f0.rs"), 1);
        assert_eq!(
            row_hash(&state, key, "src/f0.rs"),
            hash,
            "the comment moved the file's hash"
        );
        assert_eq!(hunks(&state, key, "src/f0.rs", &hash), before);
    });
}

/// More files than the eager parse takes, past the lazy size, so each one
/// lists as a stub until something parses it.
fn lazy_raw() -> String {
    use std::fmt::Write as _;

    let filler = "x".repeat(400);
    let mut raw = String::new();
    for n in 0..520 {
        let _ = write!(
            raw,
            "diff --git a/src/f{n}.rs b/src/f{n}.rs\n--- a/src/f{n}.rs\n+++ b/src/f{n}.rs\n\
             @@ -1,2 +1,2 @@\n-old {filler}\n+new {filler}\n ctx\n"
        );
    }
    raw
}

/// A batch of unparsed files is parsed by the tab's actor, once: the
/// published content holds them parsed, its diff section moved by one
/// publish, and each answer names the parsed hash the next `diff(tab)` lists.
#[test]
fn unparsed_files_are_parsed_through_their_actor_once_per_batch() {
    with_storage(|dir| {
        let state = state_over(dir, ports(fake_git().with_diff_raw(Ok(lazy_raw()))));
        let (key, tab) = listed(&state, dir, "repo");
        load(&state, key, &tab, DiffMode::Branch);
        let before = content(&state, key, &tab);
        assert!(before.diff.meta.lazy, "the fixture did not load lazily");
        let paths = ["src/f1.rs", "src/f2.rs", "src/f3.rs"];
        let listed = diff(&state, key);
        let stubs: Vec<(String, String)> = paths
            .iter()
            .map(|path| {
                let row = listed.files.iter().find(|row| row.path == *path);
                let row = row.unwrap_or_else(|| panic!("{path} is not listed"));
                assert!(row.is_lazy_stub, "{path} is not a stub");
                (path.to_string(), row.cache_key.clone())
            })
            .collect();

        let keys = stubs.iter().map(|(p, h)| hunks_key(key, p, h)).collect();
        let fetched = resources::fetch_all(&state.desktop, keys);
        let fetched = fetched.unwrap_or_else(|e| panic!("fetch: {e:?}"));

        let after = content(&state, key, &tab);
        assert_eq!(
            after.diff.revision,
            before.diff.revision + 1,
            "the batch was not one parse through the actor"
        );
        for ((path, stub), answer) in stubs.iter().zip(&fetched) {
            let file = after.diff.files.iter().find(|f| f.path == *path);
            let file = file.unwrap_or_else(|| panic!("{path} left the diff"));
            assert!(
                !file.hunks.is_empty(),
                "{path} is not parsed in the content"
            );
            let Resource::Hunks(answer) = &answer.value else {
                panic!("not hunks: {answer:?}");
            };
            assert_eq!(answer.hunks.len(), 1);
            assert_ne!(&answer.hash, stub);
            assert_eq!(answer.hash, row_hash(&state, key, path));
        }
    });
}

/// A repo whose `a.txt` has an unstaged edit.
fn edited_repo(dir: &super::ReviewFlowTestDir) -> std::path::PathBuf {
    let root = dir.path().join("repo");
    std::fs::create_dir_all(&root).unwrap_or_else(|e| panic!("repo dir: {e}"));
    git(&root, &["init", "-q", "-b", "main"]);
    std::fs::write(root.join("a.txt"), "one\n").unwrap_or_else(|e| panic!("{e}"));
    git(&root, &["add", "a.txt"]);
    git(&root, &["commit", "-q", "-m", "a"]);
    std::fs::write(root.join("a.txt"), "first\n").unwrap_or_else(|e| panic!("{e}"));
    canonical(&root)
}

/// A file rewritten after the page read its hash answers `Stale` for that
/// hash, never the new hunks; the next `diff(tab)` names a new hash, which
/// answers the new text.
#[test]
fn a_rewritten_file_answers_stale_and_never_new_hunks_under_its_old_hash() {
    with_storage(|dir| {
        let root = edited_repo(dir);
        let state = state_over(dir, er_engine::ports::Ports::cli());
        let tab = content_routes::working_tab(root.to_string_lossy().into_owned());
        let added = state
            .service
            .send_app(er_service::AppCommand::NewTab { tab: tab.clone() });
        assert!(added.is_ok(), "{added:?}");
        let key = state
            .service
            .app_registry()
            .tabs_view()
            .active
            .unwrap_or_else(|| panic!("no tab"));
        load(&state, key, &tab, DiffMode::Unstaged);
        let old = row_hash(&state, key, "a.txt");
        assert!(hunks(&state, key, "a.txt", &old).is_ok());

        std::fs::write(root.join("a.txt"), "second\n").unwrap_or_else(|e| panic!("{e}"));
        let refreshed = content_routes::refresh_tab(&state, key, false);
        assert!(refreshed.is_ok(), "{refreshed:?}");

        assert_eq!(hunks(&state, key, "a.txt", &old), Err(ErErrorKind::Stale));
        let new = row_hash(&state, key, "a.txt");
        assert_ne!(new, old);
        let answer = hunks(&state, key, "a.txt", &new).unwrap_or_else(|e| panic!("{e:?}"));
        let text: Vec<&str> = answer.hunks[0]
            .lines
            .iter()
            .map(|l| l.text.as_str())
            .collect();
        assert!(text.contains(&"second"), "{text:?}");
    });
}

fn raw_with_lockfile() -> String {
    format!("{}{LOCK_SECTION}", super::diff_resource_tests::raw_diff())
}

const LOCK_SECTION: &str =
    "diff --git a/Cargo.lock b/Cargo.lock\n--- a/Cargo.lock\n+++ b/Cargo.lock\n\
     @@ -1,2 +1,2 @@\n-old = 1\n+new = 2\n ctx\n";

fn set_compacted(state: &AppState, key: TabKey, compacted: bool) {
    let command = er_protocol::Command::SetFileCompacted {
        tab: wire(key),
        path: "Cargo.lock".into(),
        compacted,
    };
    let ack = super::while_the_app_is_locked(state, "set_file_compacted", move |state| {
        let effects = Effects {
            notify: &|_, _| {},
            follow_selection: &|_| {},
            check_update: || panic!("no update check in this test"),
            webviews: &crate::browser_webview::NoWebviews,
            pick_folder: &|| None,
        };
        apply(&state.desktop, command, &effects).map(|applied| applied.ack)
    });
    let ack = ack.unwrap_or_else(|e| panic!("{e:?}"));
    let named = ack
        .changed
        .iter()
        .any(|c| c.key == ResourceKey::Diff { tab: wire(key) });
    assert!(named, "the Ack named no diff: {ack:?}");
}

fn lock_row(state: &AppState, key: TabKey) -> er_protocol::FileRow {
    let diff = diff(state, key);
    let row = diff.files.into_iter().find(|row| row.path == "Cargo.lock");
    row.unwrap_or_else(|| panic!("Cargo.lock is not listed"))
}

/// A compacted file expanded through the service shows in full, and stays
/// expanded across a reload; compacted again, it shows compacted, leaves the
/// tab's expanded set, and a reload keeps it compacted.
#[test]
fn compaction_runs_both_ways_on_the_service_and_holds_across_a_load() {
    with_storage(|dir| {
        let git = fake_git().with_diff_raw(Ok(raw_with_lockfile()));
        let state = state_over(dir, ports(git.with_diff_raw_file(Ok(LOCK_SECTION.into()))));
        let (key, tab) = listed(&state, dir, "repo");
        load(&state, key, &tab, DiffMode::Branch);
        assert!(
            lock_row(&state, key).compacted,
            "the lockfile did not load compacted"
        );

        set_compacted(&state, key, false);
        let row = lock_row(&state, key);
        assert!(!row.compacted, "the expand did not show the file in full");
        let full = hunks(&state, key, "Cargo.lock", &row.cache_key);
        assert_eq!(full.map(|h| h.hunks.len()), Ok(1));
        assert!(content_routes::refresh_tab(&state, key, false).is_ok());
        assert!(
            !lock_row(&state, key).compacted,
            "a reload compacted the expanded file"
        );

        set_compacted(&state, key, true);
        assert!(
            lock_row(&state, key).compacted,
            "the file did not compact again"
        );
        let held = content(&state, key, &tab);
        assert!(
            !held.diff.expanded.contains("Cargo.lock"),
            "the file stayed expanded"
        );
        assert!(content_routes::refresh_tab(&state, key, false).is_ok());
        assert!(
            lock_row(&state, key).compacted,
            "a reload expanded the compacted file"
        );
    });
}

/// With the flag on nothing records which hunks the page holds: the
/// snapshot reads no sent-files map, so its differential path never runs.
#[test]
fn the_flag_on_snapshot_records_no_sent_hunks() {
    with_storage(|dir| {
        let state = state_over(dir, super::diff_resource_tests::fake_ports());
        assert!(crate::commands::snapshot_sources(&state)
            .sent_files
            .is_none());
    });
}

/// No flag-on code calls `record_sent_file` or reads `hunks_omitted`: each
/// use outside the snapshot module that defines them is compiled only
/// without the flag.
#[test]
fn no_flag_on_code_records_sent_hunks() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut uses = Vec::new();
    scan(&src, &mut uses);
    for (file, cfg) in &uses {
        assert_eq!(
            cfg, "#[cfg(not(feature = \"v1-service\"))]",
            "{file} records or reads sent hunks with the flag on"
        );
    }
    assert!(!uses.is_empty(), "the scan found no use to check");
}

fn scan(dir: &Path, uses: &mut Vec<(String, String)>) {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.to_string_lossy().into_owned();
        if path.is_dir() {
            if !name.ends_with("/tests") {
                scan(&path, uses);
            }
            continue;
        }
        if !name.ends_with(".rs") || name.ends_with("/snapshot.rs") || name.contains("_tests") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let code = text.split("#[cfg(test)]\nmod tests").next().unwrap_or("");
        for needle in ["record_sent_file(", "hunks_omitted"] {
            for (at, _) in code.match_indices(needle) {
                let mut lines = code.get(..at).unwrap_or("").rsplit('\n');
                if lines
                    .next()
                    .is_some_and(|l| l.trim_start().starts_with("//"))
                {
                    continue;
                }
                // The nearest `cfg` above the use is the one that compiles it.
                let cfg = lines.find(|l| l.trim_start().starts_with("#[cfg("));
                uses.push((name.clone(), cfg.unwrap_or("").trim().to_string()));
            }
        }
    }
}

/// The tab-switch median on the resource path, over the 5,000-line diff the
/// flag-off baseline in `latency_tests.rs` reads: `er_send` `select_tab` to
/// its `Ack`, then `er_fetch` of what it names and of the new tab's
/// `diff(tab)`, serialised. Warm: the page holds the tab's hunks by hash, so
/// that is all a switch back fetches. Cold: the first twelve files' hunks too.
#[test]
#[ignore = "latency harness: run with --ignored --nocapture and paste the numbers in the PR; never a CI assertion"]
fn the_tab_switch_reports_its_flag_on_median() {
    use std::time::{Duration, Instant};

    use std::fmt::Write as _;
    let mut raw = String::new();
    for file in 0..250 {
        let _ = write!(raw, "diff --git a/src/f{file}.rs b/src/f{file}.rs\n--- a/src/f{file}.rs\n+++ b/src/f{file}.rs\n@@ -1,10 +1,10 @@\n");
        for line in 0..10 {
            let _ = write!(raw, "-old {file} {line}\n+new {file} {line}\n");
        }
    }
    with_storage(|dir| {
        let state = state_over(dir, ports(fake_git().with_diff_raw(Ok(raw.clone()))));
        let (a, a_tab) = listed(&state, dir, "a");
        load(&state, a, &a_tab, DiffMode::Branch);
        let (b, b_tab) = listed(&state, dir, "b");
        load(&state, b, &b_tab, DiffMode::Branch);
        let effects = Effects {
            notify: &|_, _| {},
            follow_selection: &|_| {},
            check_update: || panic!("no update check in this test"),
            webviews: &crate::browser_webview::NoWebviews,
            pick_folder: &|| None,
        };
        let switch = |key: TabKey, window: usize| -> (Duration, usize) {
            let started = Instant::now();
            let select = er_protocol::Command::SelectTab { key: wire(key) };
            let ack = apply(&state.desktop, select, &effects).unwrap_or_else(|e| panic!("{e:?}"));
            let mut keys: Vec<ResourceKey> = ack.ack.changed.into_iter().map(|c| c.key).collect();
            keys.push(ResourceKey::Diff { tab: wire(key) });
            let mut fetched =
                resources::fetch_all(&state.desktop, keys).unwrap_or_else(|e| panic!("{e:?}"));
            if window > 0 {
                let Some(Resource::Diff(diff)) = fetched.last().map(|f| f.value.clone()) else {
                    panic!("no diff");
                };
                let hunks = diff
                    .files
                    .iter()
                    .take(window)
                    .map(|r| hunks_key(key, &r.path, &r.cache_key));
                fetched.extend(
                    resources::fetch_all(&state.desktop, hunks.collect())
                        .unwrap_or_else(|e| panic!("{e:?}")),
                );
            }
            let bytes = serde_json::to_vec(&fetched).map(|v| v.len()).unwrap_or(0);
            (started.elapsed(), bytes)
        };
        for (name, window) in [("warm", 0), ("cold", 12)] {
            let mut samples = Vec::new();
            let mut bytes = 0;
            for run in 0..50 {
                let (took, size) = switch(if run % 2 == 0 { a } else { b }, window);
                bytes = size;
                if run >= 5 {
                    samples.push(took);
                }
            }
            samples.sort_unstable();
            let median = samples[samples.len() / 2];
            println!("flag-on tab switch ({name}): median {median:?}, answer {bytes} bytes");
        }
    });
}
