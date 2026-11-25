//! No TUI module writes a store artifact, the working tree or GitHub itself:
//! each write is a command the service runs (ADR 0073). The one session save
//! is `service/session.rs`'s, and the one file write is `update.rs`'s
//! downloaded archive and installed binary. Inline test modules are skipped,
//! and the scan goes on after one. Code is matched joined into one line, so no
//! line break, import group or receiver name hides a write. One form is out of
//! reach: a git push whose result a match arm or a closure returns, since a
//! list's push looks the same there.

use std::path::{Path, PathBuf};

const STORE: [&str; 28] = [
    "save_reviewed",
    "update_reviewed",
    "save_config",
    "annotation_store::update_",
    "annotation_store::delete",
    "update_questions(",
    "update_notes(",
    "update_github_comments(",
    "save_session(",
    "save_snapshot",
    "write_text",
    "write_durable",
    "write_locked",
    "write_with_timeout",
    "update_durable",
    "delete_text",
    "delete_locked",
    "Store::write(",
    "Store::update(",
    "Store::delete(",
    "write_atomic",
    "write_typed_if_unchanged",
    "restore_if_unchanged",
    "restore_backup",
    "write_backup_to_dir",
    "archive_to_dir",
    "create_backup_dir",
    "remove_backup_dir",
];

const GIT: [&str; 14] = [
    "stage_file",
    "unstage_file",
    "stage_all",
    "delete_watched_file",
    "fetch_pr_head",
    "fetch_base_branch_ref",
    "fetch_pr_base_ref",
    "ensure_base_ref_available",
    "fetch(",
    "update_ref",
    "shallow_clone",
    "git.push(",
    "::commit(",
    "::push(",
];

/// er-github's writes (`er_engine::github` re-exports all), its port's, the engine's post.
const GITHUB: [&str; 20] = [
    "GitHub::approve(",
    ">::approve(",
    "post_line_comment",
    "post_reply",
    "post_general",
    "submit_review",
    "pending_review_add_comment",
    "pending_review_remove_comment",
    "pending_review_submit",
    "delete_comment",
    "gh_pr_approve",
    "push_comment",
    "reply_comment",
    "gh_pr_general_comment",
    "update_review_comment",
    "edit_body",
    "run_pr_action",
    "delete_remote_branch",
    "restore_remote_branch",
    "execute_comment_post",
];

/// er-engine's own writers of a store artifact, a sidecar, a job's inputs or
/// the app's records, and the runs that write as they go.
const ENGINE: [&str; 79] = [
    "save_tabs",
    "save_persisted_pr_open_cache",
    "prune_pr_open_cache",
    "projects_pins::update",
    "ensure_project_for_remote",
    "save_config_to",
    "save_config_pair_to",
    "save_agents_to",
    "model_discovery::save_cache",
    "save_cached_release",
    "replace_executable",
    "uninstall::run",
    "write_annotation",
    "write_review_md",
    "write_change_facts",
    "write_arbiter_review",
    "write_pulled_comments",
    "apply_comment_sync",
    "migrate_orphaned_github_comments",
    "append_finding_response",
    "update_finding_response",
    "delete_finding_response",
    "append_promote_replies",
    "save_finding_promotions",
    "save_ui_annotations",
    "set_checked",
    "toggle_checked",
    "record_accepted",
    "mark_accepted",
    "accept_findings",
    "accept_review_flow_findings",
    "promote_to_github",
    "promote_to_note",
    "promote_finding",
    "reply_to_question_in_dir",
    "reply_to_pr_question",
    "reply_to_note_in_dir",
    "reply_to_pr_note",
    "reply_to_finding_in_dir",
    "reply_to_pr_finding",
    "resolve_question_in_dir",
    "resolve_note_in_dir",
    "remove_finding_from_sidecars",
    "set_finding_resolved",
    "delete_threads_linked_to_finding",
    "cleanup_questions_and_notes",
    "cleanup_note_replies",
    "cleanup_question_answers",
    "cleanup_reviews",
    "cleanup_triage",
    "cleanup_review_artifacts",
    "clear_full_run_artifacts",
    "stamp_scoped_overlay",
    "edit_finding_overlays",
    "persist_diagram",
    "upload_artifacts_to_dir",
    "upload_pr_artifacts",
    "upload_diagram",
    "commit_staged",
    "commit_diagram",
    "commit_to_store",
    "workflow_record::save",
    "workflow_record::update",
    "panel_run::save",
    "save_run",
    "save_diff_patch",
    "save_round_output",
    "save_arbiter_output",
    "delete_run_dir",
    "stage_inputs",
    "stage_update_inputs",
    "ensure_diff_artifacts",
    "ensure_review_inputs",
    "start_review_flow",
    "start_arena_run",
    "start_seeded_run",
    "start_arena_batch",
    "launch_agent_prompt",
    "launch_shell_command",
];

/// The installer's writes, which only `update.rs` makes.
const INSTALL: [&str; 2] = ["save_cached_release", "replace_executable"];

const FILES: [&str; 13] = [
    "fs::write",
    "remove_file",
    "remove_dir",
    "create_dir",
    "rename(",
    "File::create",
    "File::options",
    "OpenOptions",
    "fs::copy",
    "set_permissions",
    "hard_link",
    "::symlink(",
    "::soft_link(",
];

/// Method calls on any receiver; one with no argument (`RwLock::write()`) is skipped.
const CALLS: [&str; 5] = [".write(", ".update(", ".delete(", ".commit(", ".approve("];

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{e}")) {
        let path = entry.unwrap_or_else(|e| panic!("{e}")).path();
        if path.is_dir() {
            walk(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// Each source file's code outside its test module and comments, named by
/// its path from `src/`.
fn sources() -> Vec<(String, String)> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut paths = Vec::new();
    walk(&src, &mut paths);
    let mut out = Vec::new();
    for path in paths {
        let name = path.strip_prefix(&src).unwrap_or(&path);
        let name = name.to_string_lossy().replace('\\', "/");
        if name.ends_with("tests.rs") || name == "golden.rs" || name == "service/parity.rs" {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{e}"));
        let mut code = String::new();
        for line in without_test_modules(&text).lines() {
            code.push_str(line.split("//").next().unwrap_or(""));
            code.push('\n');
        }
        out.push((name, code));
    }
    out
}

/// The text with each inline test module (`#[cfg(test)] mod name { … }`) cut
/// out; the code after one is kept.
fn without_test_modules(text: &str) -> String {
    const ATTR: &str = "#[cfg(test)]";
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find(ATTR) {
        let item = rest.get(at + ATTR.len()..).unwrap_or_default().trim_start();
        let open = item.find('{').map(|open| rest.len() - item.len() + open);
        let header = item.split('{').next().unwrap_or_default();
        let cut = match open {
            Some(open) if header.starts_with("mod ") && !header.contains(';') => {
                out.push_str(rest.get(..at).unwrap_or_default());
                block_end(rest, open)
            }
            _ => {
                out.push_str(rest.get(..at + ATTR.len()).unwrap_or_default());
                at + ATTR.len()
            }
        };
        rest = rest.get(cut..).unwrap_or_default();
    }
    out.push_str(rest);
    out
}

/// The index just past the `}` that closes the `{` at `open`, skipping
/// comments and string, raw string and char literals.
fn block_end(text: &str, open: usize) -> usize {
    let bytes = text.as_bytes();
    let find = |from: usize, what: &str| text.get(from..).and_then(|rest| rest.find(what));
    let (mut at, mut depth) = (open, 0usize);
    while let Some(&byte) = bytes.get(at) {
        let next = bytes.get(at + 1).copied();
        match byte {
            b'/' if next == Some(b'/') => at = find(at, "\n").map_or(text.len(), |n| at + n),
            b'/' if next == Some(b'*') => at = find(at, "*/").map_or(text.len(), |n| at + n + 1),
            b'r' if matches!(next, Some(b'"' | b'#')) => {
                let hashes = bytes.get(at + 1..).unwrap_or_default();
                let hashes = hashes.iter().take_while(|&&c| c == b'#').count();
                if bytes.get(at + 1 + hashes) == Some(&b'"') {
                    let close = format!("\"{}", "#".repeat(hashes));
                    let from = at + 2 + hashes;
                    at = find(from, &close).map_or(text.len(), |n| from + n + close.len() - 1);
                }
            }
            b'"' => {
                at += 1;
                while let Some(&c) = bytes.get(at) {
                    match c {
                        b'\\' => at += 2,
                        b'"' => break,
                        _ => at += 1,
                    }
                }
            }
            b'\'' if bytes.get(at + 2) == Some(&b'\'') => at += 2,
            b'\'' if next == Some(b'\\') => {
                at = find(at + 3, "'").map_or(text.len(), |n| at + 3 + n);
            }
            b'{' => depth += 1,
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return at + 1;
                }
            }
            _ => {}
        }
        at += 1;
    }
    text.len()
}

/// Whether a `use` names `fs`, so no alias or grouped import hides a file write.
fn uses_fs(code: &str) -> bool {
    code.split(';').any(|piece| {
        let words: Vec<&str> = piece
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .filter(|word| !word.is_empty())
            .collect();
        words
            .iter()
            .rposition(|word| *word == "use")
            .is_some_and(|at| words[at..].contains(&"fs"))
    })
}

/// Whether a `.push(` result is used (`?`, `.`, `{` after; `=`, `(`, `,`, `return` before)
/// or returned as a block's last expression (`}` after). A `Vec`'s gives `()`, and the lint
/// gate's `semicolon_if_nothing_returned` refuses a unit push left as a block's tail; the Git
/// port's `Result` is `#[must_use]`, so a bare one fails lint.
fn uses_a_push(flat: &str) -> bool {
    flat.match_indices(".push(").any(|(at, needle)| {
        let after = close_of(flat, at + needle.len())
            .and_then(|end| flat.get(end + 1..))
            .and_then(|rest| rest.chars().next());
        let start = receiver_start(flat, at);
        let before = flat.get(..start).and_then(|head| head.chars().last());
        matches!(after, Some('?' | '.' | '{' | '}'))
            || matches!(before, Some('=' | '(' | ','))
            || flat
                .get(start..at)
                .is_some_and(|recv| recv.starts_with("return"))
    })
}

/// The `)` closing a call whose arguments start at `open`, past string and char literals.
fn close_of(flat: &str, open: usize) -> Option<usize> {
    let bytes = flat.as_bytes();
    let (mut at, mut depth, mut quoted) = (open, 1, false);
    while let Some(&byte) = bytes.get(at) {
        match byte {
            b'\\' if quoted => at += 1,
            b'"' => quoted = !quoted,
            _ if quoted => {}
            b'\'' if bytes.get(at + 2) == Some(&b'\'') => at += 2,
            b'(' => depth += 1,
            b')' if depth == 1 => return Some(at),
            b')' => depth -= 1,
            _ => {}
        }
        at += 1;
    }
    None
}

/// Where the path, field or call chain before the `.` at `dot` starts.
fn receiver_start(flat: &str, dot: usize) -> usize {
    let bytes = flat.as_bytes();
    let (mut at, mut depth) = (dot, 0);
    while let Some(&byte) = at.checked_sub(1).and_then(|before| bytes.get(before)) {
        match byte {
            b')' | b']' => depth += 1,
            b'(' | b'[' if depth > 0 => depth -= 1,
            _ if depth > 0 || byte.is_ascii_alphanumeric() => {}
            b'.' | b':' | b'_' => {}
            _ => break,
        }
        at -= 1;
    }
    at
}

#[test]
fn no_tui_module_writes_for_itself() {
    let sources = sources();
    assert!(sources.iter().any(|(name, _)| name == "service/shell.rs"));
    for (name, code) in &sources {
        let flat: String = code.split_whitespace().collect();
        let needles = (STORE.iter().chain(&GIT).chain(&GITHUB))
            .chain(&ENGINE)
            .chain(&FILES);
        for needle in needles {
            let allowed = match *needle {
                "save_session(" => name == "service/session.rs",
                _ if FILES.contains(needle) || INSTALL.contains(needle) => name == "update.rs",
                _ => false,
            };
            assert!(allowed || !flat.contains(needle), "{name} names {needle}");
        }
        for call in CALLS {
            let called = (flat.match_indices(call)).any(|(at, _)| {
                !flat
                    .get(at + call.len()..)
                    .is_some_and(|rest| rest.starts_with(')'))
            });
            assert!(!called, "{name} calls {call}");
        }
        assert!(name == "update.rs" || !uses_fs(code), "{name} imports fs");
        assert!(!uses_a_push(&flat), "{name} uses a push's result");
    }
}

/// No path the TUI runs says an area is not on this build: every key, hub
/// row and panel goes to the service.
#[test]
fn no_unported_path_is_left() {
    for (name, code) in &sources() {
        let flat: String = code.split_whitespace().collect();
        for needle in ["UNPORTED", "notonthisbuild", "fnunported"] {
            assert!(!flat.contains(needle), "{name} names {needle}");
        }
    }
}
