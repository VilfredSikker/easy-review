//! No TUI module writes a store artifact, the working tree or GitHub itself:
//! each write is a command the service runs (ADR 0073). The one session save
//! is `service/session.rs`'s, and the one file write is `update.rs`'s
//! downloaded archive. Test modules are skipped. Code is matched joined into
//! one line, so no line break, import group or receiver name hides a write.

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

const GIT: [&str; 11] = [
    "stage_file",
    "unstage_file",
    "stage_all",
    "delete_watched_file",
    "fetch_pr_head",
    "fetch_base_branch_ref",
    "fetch_pr_base_ref",
    "ensure_base_ref_available",
    "git.push(",
    "::commit(",
    "::push(",
];

/// er-github's writes (`er_engine::github` re-exports all), its port's, the engine's post.
const GITHUB: [&str; 19] = [
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
    "gh_pr_actions",
    "delete_remote_branch",
    "restore_remote_branch",
    "execute_comment_post",
];

const FILES: [&str; 9] = [
    "fs::write",
    "remove_file",
    "remove_dir",
    "create_dir",
    "rename(",
    "File::create",
    "File::options",
    "OpenOptions",
    "fs::copy",
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
        let mut lines = text.lines().peekable();
        while let Some(line) = lines.next() {
            let next = lines.peek().map_or("", |next| next.trim());
            if line.trim() == "#[cfg(test)]" && next.starts_with("mod ") && next.ends_with('{') {
                break;
            }
            code.push_str(line.split("//").next().unwrap_or(""));
            code.push('\n');
        }
        out.push((name, code));
    }
    out
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

/// Whether a `.push(` result is used (`?`, `.`, `{` after; `=`, `(`, `,`, `return` before).
/// A `Vec`'s gives `()`; the Git port's `Result` is `#[must_use]`, so a bare one fails lint.
fn uses_a_push(flat: &str) -> bool {
    flat.match_indices(".push(").any(|(at, needle)| {
        let after = close_of(flat, at + needle.len())
            .and_then(|end| flat.get(end + 1..))
            .and_then(|rest| rest.chars().next());
        let start = receiver_start(flat, at);
        let before = flat.get(..start).and_then(|head| head.chars().last());
        matches!(after, Some('?' | '.' | '{'))
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
        let needles = STORE.iter().chain(&GIT).chain(&GITHUB).chain(&FILES);
        for needle in needles {
            let allowed = match *needle {
                "save_session(" => name == "service/session.rs",
                _ if FILES.contains(needle) => name == "update.rs",
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
